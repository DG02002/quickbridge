use crate::{Result, RuntimeError, diagnostics::render_command, session::SessionPaths};
use quickbridge_core::{AudioHandling, StreamSelection, Timecode, VideoPackaging};
use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};
use tokio::{
    fs,
    process::{Child, Command},
    time::{Instant, sleep},
};
use tracing::debug;

const READY_TIMEOUT: Duration = Duration::from_secs(45);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
// Keep a bounded ten-second lead, then allow modest catch-up after stalls.
const INPUT_READ_RATE: f32 = 1.0;
const INPUT_INITIAL_BURST_SECONDS: u64 = 10;
const INPUT_CATCHUP_RATE: f32 = 1.5;
const HLS_SEGMENT_SECONDS: u64 = 2;
// About 24 seconds, or roughly 255 MB at 85.2 Mb/s plus deletion grace.
const HLS_WINDOW_SEGMENTS: u64 = 12;
const READY_MEDIA_SEGMENTS: usize = 3;

#[derive(Clone, Debug)]
pub struct FfmpegRunner {
    binary: OsString,
    verbose: bool,
}

impl FfmpegRunner {
    pub fn new(verbose: bool) -> Self {
        let binary =
            std::env::var_os("QUICKBRIDGE_FFMPEG_BIN").unwrap_or_else(|| OsString::from("ffmpeg"));
        Self { binary, verbose }
    }

    #[cfg(test)]
    pub fn with_binary(binary: impl Into<OsString>, verbose: bool) -> Self {
        Self {
            binary: binary.into(),
            verbose,
        }
    }

    pub async fn ensure_available(&self) -> Result<()> {
        let status = match Command::new(&self.binary)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
        {
            Ok(status) => status,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(RuntimeError::FfmpegUnavailable {
                    binary: self.binary.to_string_lossy().into_owned(),
                });
            }
            Err(error) => {
                return Err(RuntimeError::ExecuteBinary {
                    binary: self.binary.to_string_lossy().into_owned(),
                    source: error,
                });
            }
        };

        if !status.success() {
            return Err(RuntimeError::FfmpegUnavailable {
                binary: self.binary.to_string_lossy().into_owned(),
            });
        }

        Ok(())
    }

    pub async fn spawn(
        &self,
        source_url: &str,
        start_at: Timecode,
        session: SessionPaths,
        selection: &StreamSelection,
    ) -> Result<FfmpegProcess> {
        let args = build_args(source_url, start_at, &session, selection)?;
        let command_line = render_command(&self.binary, &args);
        debug!(
            session_id = session.id,
            source_url,
            start_at = %start_at,
            command = %command_line,
            "spawning ffmpeg"
        );

        let mut command = Command::new(&self.binary);
        command.args(&args);
        command.stdout(Stdio::null());
        command.stdin(Stdio::null());
        if self.verbose {
            command.stderr(Stdio::inherit());
        } else {
            command.stderr(Stdio::null());
        }

        let child = command
            .spawn()
            .map_err(|source| RuntimeError::StartBinary {
                binary: self.binary.to_string_lossy().into_owned(),
                session_id: session.id,
                source,
            })?;

        Ok(FfmpegProcess { child, session })
    }

    pub fn render_spawn_command(
        &self,
        source_url: &str,
        start_at: Timecode,
        session: &SessionPaths,
        selection: &StreamSelection,
    ) -> String {
        match build_args(source_url, start_at, session, selection) {
            Ok(args) => render_command(&self.binary, &args),
            Err(error) => format!("unable to build ffmpeg command: {error}"),
        }
    }
}

fn build_args(
    source_url: &str,
    start_at: Timecode,
    session: &SessionPaths,
    selection: &StreamSelection,
) -> Result<Vec<OsString>> {
    let packaging = selection.video_packaging()?;
    let mut args = vec![OsString::from("-y")];
    if start_at.as_seconds() > 0 {
        args.push(OsString::from("-ss"));
        args.push(OsString::from(start_at.as_seconds().to_string()));
    }

    args.extend([
        OsString::from("-readrate"),
        OsString::from(INPUT_READ_RATE.to_string()),
        OsString::from("-readrate_initial_burst"),
        OsString::from(INPUT_INITIAL_BURST_SECONDS.to_string()),
        OsString::from("-readrate_catchup"),
        OsString::from(INPUT_CATCHUP_RATE.to_string()),
        OsString::from("-i"),
        OsString::from(source_url),
    ]);
    args.extend([
        OsString::from("-map"),
        OsString::from(format!("0:{}", selection.video_stream_index())),
    ]);
    if let Some(audio_stream_index) = selection.audio_stream_index() {
        args.extend([
            OsString::from("-map"),
            OsString::from(format!("0:{audio_stream_index}")),
        ]);
    }

    args.extend([OsString::from("-sn"), OsString::from("-dn")]);
    args.extend([OsString::from("-c:v"), OsString::from("copy")]);
    if let VideoPackaging::Hevc { tag, unofficial } = packaging {
        args.extend([OsString::from("-tag:v"), OsString::from(tag)]);
        if unofficial {
            args.extend([OsString::from("-strict"), OsString::from("unofficial")]);
        }
    }
    match selection.audio_handling() {
        Some(AudioHandling::Copy) => {
            args.extend([OsString::from("-c:a"), OsString::from("copy")]);
        }
        Some(AudioHandling::TranscodeAlac) => {
            args.extend([OsString::from("-c:a"), OsString::from("alac")]);
        }
        None => {}
    }

    args.extend([
        OsString::from("-f"),
        OsString::from("hls"),
        OsString::from("-hls_segment_type"),
        OsString::from("fmp4"),
        OsString::from("-hls_fmp4_init_filename"),
        OsString::from(session.init_filename.as_str()),
        OsString::from("-hls_time"),
        OsString::from(HLS_SEGMENT_SECONDS.to_string()),
        OsString::from("-hls_list_size"),
        OsString::from(HLS_WINDOW_SEGMENTS.to_string()),
        OsString::from("-hls_flags"),
        OsString::from("delete_segments+omit_endlist+temp_file"),
        OsString::from("-hls_segment_filename"),
        session.segment_pattern.as_os_str().to_os_string(),
        session.playlist_path.as_os_str().to_os_string(),
    ]);

    Ok(args)
}

#[derive(Debug)]
pub struct FfmpegProcess {
    child: Child,
    session: SessionPaths,
}

impl FfmpegProcess {
    pub fn session(&self) -> &SessionPaths {
        &self.session
    }

    pub async fn wait_until_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if has_playable_output(self.session()).await? {
                debug!(session_id = self.session.id, "ffmpeg session is ready");
                return Ok(());
            }

            if let Some(status) = self.child.try_wait()? {
                return Err(RuntimeError::FfmpegExitedEarly { status });
            }

            if Instant::now() >= deadline {
                return Err(RuntimeError::FfmpegReadyTimeout {
                    playlist_path: self.session.playlist_path.clone(),
                });
            }

            sleep(READY_POLL_INTERVAL).await;
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child
                .kill()
                .await
                .map_err(|source| RuntimeError::StopFfmpeg { source })?;
        }
        let _ = self.child.wait().await;
        Ok(())
    }
}

pub async fn has_playable_output(session: &SessionPaths) -> Result<bool> {
    if !fs::try_exists(&session.playlist_path).await? {
        return Ok(false);
    }

    let playlist = fs::read_to_string(&session.playlist_path).await?;
    let media_segments = playlist_media_entries(&playlist);
    if media_segments.len() < READY_MEDIA_SEGMENTS {
        return Ok(false);
    }

    if let Some(init_file) = playlist_init_file(&playlist)
        && !is_nonempty_file(&session.dir.join(init_file)).await?
    {
        return Ok(false);
    }

    for segment in media_segments.iter().take(READY_MEDIA_SEGMENTS) {
        if !is_nonempty_file(&session.dir.join(segment)).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn is_nonempty_file(path: &Path) -> Result<bool> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file() && metadata.len() > 0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn playlist_init_file(playlist: &str) -> Option<String> {
    playlist.lines().find_map(|line| {
        let line = line.trim();
        let prefix = "#EXT-X-MAP:URI=\"";
        let rest = line.strip_prefix(prefix)?;
        Some(rest.split('"').next()?.to_string())
    })
}

fn playlist_media_entries(playlist: &str) -> Vec<String> {
    playlist
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        FfmpegRunner, build_args, has_playable_output, playlist_init_file, playlist_media_entries,
    };
    use crate::session::{SessionManager, SessionPaths};
    use quickbridge_core::{AudioHandling, AudioStream, StreamSelection, Timecode, VideoStream};
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[tokio::test]
    async fn detects_missing_output() {
        let root = tempfile::tempdir().unwrap();
        let session = SessionPaths {
            id: 1,
            dir: root.path().join("session"),
            playlist_path: root.path().join("session/stream.m3u8"),
            segment_pattern: root.path().join("session/segment_0001_%05d.m4s"),
            init_filename: String::from("init_0001.mp4"),
        };
        tokio::fs::create_dir_all(&session.dir).await.unwrap();
        assert!(!has_playable_output(&session).await.unwrap());
    }

    #[tokio::test]
    async fn detects_manifest_and_segment() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("session");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let session = SessionPaths {
            id: 1,
            dir: dir.clone(),
            playlist_path: dir.join("stream.m3u8"),
            segment_pattern: dir.join("segment_0001_%05d.m4s"),
            init_filename: String::from("init_0001.mp4"),
        };
        tokio::fs::write(
            &session.playlist_path,
            "#EXTM3U\n#EXT-X-MAP:URI=\"init_0001.mp4\"\n#EXTINF:2.0,\nsegment_0001_00001.m4s\n#EXTINF:2.0,\nsegment_0001_00002.m4s\n#EXTINF:2.0,\nsegment_0001_00003.m4s\n",
        )
        .await
        .unwrap();
        tokio::fs::write(dir.join("init_0001.mp4"), "init")
            .await
            .unwrap();
        tokio::fs::write(session.segment_path(1), "segment")
            .await
            .unwrap();
        assert!(!has_playable_output(&session).await.unwrap());
        tokio::fs::write(session.segment_path(2), "segment")
            .await
            .unwrap();
        assert!(!has_playable_output(&session).await.unwrap());
        tokio::fs::write(session.segment_path(3), "segment")
            .await
            .unwrap();
        assert!(has_playable_output(&session).await.unwrap());
    }

    #[test]
    fn builds_args_with_selected_streams_and_transcode() {
        let session = SessionPaths {
            id: 1,
            dir: std::path::PathBuf::from("/tmp/quickbridge/session-0001"),
            playlist_path: std::path::PathBuf::from("/tmp/quickbridge/session-0001/stream.m3u8"),
            segment_pattern: std::path::PathBuf::from(
                "/tmp/quickbridge/session-0001/segment_0001_%05d.m4s",
            ),
            init_filename: String::from("init_0001.mp4"),
        };
        let selection = StreamSelection::new(
            VideoStream::new(2, "Stream #0:2: Video: h264", true),
            Some(AudioStream::new(
                5,
                Some(String::from("dts")),
                "Stream #0:5: Audio: dts",
                true,
            )),
        );

        let args = build_args(
            "https://example.com/video.mkv",
            Timecode::from_seconds(30),
            &session,
            &selection,
        )
        .unwrap()
        .into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

        assert!(args.windows(2).any(|window| window == ["-map", "0:2"]));
        assert!(args.windows(2).any(|window| window == ["-map", "0:5"]));
        assert!(args.windows(2).any(|window| window == ["-c:v", "copy"]));
        assert!(args.windows(2).any(|window| window == ["-c:a", "alac"]));
        assert!(!args.iter().any(|arg| arg == "-copyinkf"));
        assert!(!args.iter().any(|arg| arg == "-re"));
        let input_index = args.iter().position(|arg| arg == "-i").unwrap();
        for (option, value) in [
            ("-readrate", "1"),
            ("-readrate_initial_burst", "10"),
            ("-readrate_catchup", "1.5"),
        ] {
            let option_index = args.iter().position(|arg| arg == option).unwrap();
            assert!(option_index < input_index);
            assert_eq!(args[option_index + 1], value);
        }
        assert!(
            args.windows(2)
                .any(|window| window == ["-hls_fmp4_init_filename", "init_0001.mp4"])
        );
        assert!(args.windows(2).any(|window| window == ["-hls_time", "2"]));
        assert!(
            args.windows(2)
                .any(|window| window == ["-hls_list_size", "12"])
        );
        assert!(
            args.windows(2).any(|window| {
                window == ["-hls_flags", "delete_segments+omit_endlist+temp_file"]
            })
        );
        assert_eq!(
            selection.audio_handling(),
            Some(&AudioHandling::TranscodeAlac)
        );
    }

    #[test]
    fn applies_apple_video_tags_and_rejects_unsupported_routes() {
        let session = SessionPaths {
            id: 1,
            dir: std::path::PathBuf::from("/tmp/quickbridge/session-0001"),
            playlist_path: std::path::PathBuf::from("/tmp/quickbridge/session-0001/stream.m3u8"),
            segment_pattern: std::path::PathBuf::from(
                "/tmp/quickbridge/session-0001/segment_%05d.m4s",
            ),
            init_filename: String::from("init.mp4"),
        };
        let cases = [
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}]}"#,
                None,
            ),
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc","color_transfer":"smpte2084"}]}"#,
                Some("hvc1"),
            ),
            (
                r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc","side_data_list":[{"side_data_type":"DOVI configuration record","dv_profile":5,"dv_level":6,"bl_present_flag":1,"el_present_flag":0,"dv_bl_signal_compatibility_id":0}]}]}"#,
                Some("dvh1"),
            ),
        ];
        for (json, tag) in cases {
            let selection = quickbridge_core::MediaInfo::from_ffprobe_json(json)
                .unwrap()
                .default_selection()
                .unwrap();
            let args = build_args(
                "https://example.com/video",
                Timecode::ZERO,
                &session,
                &selection,
            )
            .unwrap()
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
            assert_eq!(
                args.windows(2)
                    .find(|pair| pair[0] == "-tag:v")
                    .map(|pair| pair[1].as_str()),
                tag
            );
            if tag == Some("dvh1") {
                assert!(
                    args.windows(2)
                        .any(|pair| pair == ["-strict", "unofficial"])
                );
            }
        }

        let selection = quickbridge_core::MediaInfo::from_ffprobe_json(
            r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"vc1"}]}"#,
        )
        .unwrap()
        .default_selection()
        .unwrap();
        assert!(
            build_args(
                "https://example.com/video",
                Timecode::ZERO,
                &session,
                &selection
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mocked_ffmpeg_process_becomes_ready() {
        let script = write_script(
            r#"#!/bin/sh
set -eu
if [ "${1:-}" = "-version" ]; then
  echo "fake ffmpeg"
  exit 0
fi
playlist=""
segment=""
prev=""
for arg in "$@"; do
  playlist="$arg"
  if [ "$prev" = "-hls_segment_filename" ]; then
    segment="$arg"
  fi
  prev="$arg"
done
mkdir -p "$(dirname "$playlist")"
printf 'init' > "$(dirname "$playlist")/init_0001.mp4"
printf '#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MAP:URI="init_0001.mp4"\n#EXTINF:2.0,\nsegment_0001_00001.m4s\n#EXTINF:2.0,\nsegment_0001_00002.m4s\n#EXTINF:2.0,\nsegment_0001_00003.m4s\n' > "$playlist"
printf 'segment' > "$(printf "$segment" 1)"
printf 'segment' > "$(printf "$segment" 2)"
printf 'segment' > "$(printf "$segment" 3)"
sleep 30
"#,
        );

        let runner = FfmpegRunner::with_binary(&script, false);
        runner.ensure_available().await.unwrap();
        let sessions = SessionManager::new(false).await.unwrap();
        let session = sessions.create_session().await.unwrap();
        let mut process = runner
            .spawn(
                "https://example.com/video.mkv",
                Timecode::ZERO,
                session.clone(),
                &selection(),
            )
            .await
            .unwrap();
        process.wait_until_ready().await.unwrap();
        process.shutdown().await.unwrap();
        sessions.remove_session(&session).await.unwrap();
        sessions.cleanup_root().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mocked_ffmpeg_failure_surfaces_readiness_error() {
        let script = write_script(
            r#"#!/bin/sh
set -eu
if [ "${1:-}" = "-version" ]; then
  echo "fake ffmpeg"
  exit 0
fi
exit 1
"#,
        );

        let runner = FfmpegRunner::with_binary(&script, false);
        let sessions = SessionManager::new(false).await.unwrap();
        let session = sessions.create_session().await.unwrap();
        let mut process = runner
            .spawn(
                "https://example.com/video.mkv",
                Timecode::ZERO,
                session.clone(),
                &selection(),
            )
            .await
            .unwrap();
        let error = process.wait_until_ready().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ffmpeg exited before the stream was ready")
        );
        sessions.remove_session(&session).await.unwrap();
        sessions.cleanup_root().await.unwrap();
    }

    fn selection() -> StreamSelection {
        StreamSelection::new(VideoStream::new(0, "Stream #0:0: Video: h264", true), None)
    }

    #[test]
    fn parses_playlist_asset_references() {
        let playlist =
            "#EXTM3U\n#EXT-X-MAP:URI=\"init_0001.mp4\"\n#EXTINF:1.0,\nsegment_0001_00001.m4s\n";
        assert_eq!(playlist_init_file(playlist).unwrap(), "init_0001.mp4");
        assert_eq!(
            playlist_media_entries(playlist),
            vec![String::from("segment_0001_00001.m4s")]
        );
    }

    #[cfg(unix)]
    fn write_script(contents: &str) -> String {
        use std::os::unix::fs::PermissionsExt;

        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let unique = SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("quickbridge-fake-ffmpeg-{millis}-{unique}.sh"));
        fs::write(&path, contents).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path.to_string_lossy().into_owned()
    }
}
