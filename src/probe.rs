use crate::{diagnostics::render_command, timecode::Timecode};
use anyhow::{Context, Result, bail};
use dialoguer::{Select, console::style, theme::ColorfulTheme};
use serde::Deserialize;
use std::{collections::HashMap, ffi::OsString, process::Stdio};
use tokio::{process::Command, task};
use tracing::debug;

#[derive(Clone, Debug)]
pub struct MediaInfo {
    videos: Vec<VideoStream>,
    audios: Vec<AudioStream>,
    duration: Option<Timecode>,
}

impl MediaInfo {
    pub(crate) fn new(
        videos: Vec<VideoStream>,
        audios: Vec<AudioStream>,
        duration: Option<Timecode>,
    ) -> Self {
        Self {
            videos,
            audios,
            duration,
        }
    }

    pub fn from_ffprobe_outputs(json: &str, summary: &str) -> Result<Self> {
        let parsed: FfprobeOutput =
            serde_json::from_str(json).context("Unable to read ffprobe output")?;
        let stream_lines = parse_stream_lines(summary);

        let mut videos = Vec::new();
        let mut audios = Vec::new();
        for stream in parsed.streams {
            let display_line = stream_lines
                .get(&stream.index)
                .cloned()
                .unwrap_or_else(|| fallback_stream_line(&stream));

            match stream.codec_type.as_deref() {
                Some("video") => videos.push(VideoStream {
                    stream_index: stream.index,
                    display_line,
                    is_default: stream
                        .disposition
                        .as_ref()
                        .and_then(FfprobeDisposition::is_default)
                        .unwrap_or(false),
                }),
                Some("audio") => audios.push(AudioStream {
                    stream_index: stream.index,
                    codec_name: stream.codec_name,
                    display_line,
                    is_default: stream
                        .disposition
                        .as_ref()
                        .and_then(FfprobeDisposition::is_default)
                        .unwrap_or(false),
                }),
                Some("subtitle") => {}
                _ => {}
            }
        }

        videos.sort_by_key(|stream| stream.stream_index);
        audios.sort_by_key(|stream| stream.stream_index);

        let duration = parsed
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .and_then(Timecode::from_seconds_f64);

        Ok(Self {
            videos,
            audios,
            duration,
        })
    }

    pub fn duration(&self) -> Option<Timecode> {
        self.duration
    }

    #[cfg(test)]
    pub fn render_input_file(&self) -> String {
        let mut lines = Vec::new();

        if !self.videos.is_empty() {
            lines.push(String::from("  Video"));
            lines.extend(
                self.videos
                    .iter()
                    .map(|stream| format!("    {}", stream.display_line())),
            );
        }

        if !self.audios.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(String::from("  Audio"));
            lines.extend(
                self.audios
                    .iter()
                    .map(|stream| format!("    {}", stream.display_line())),
            );
        }

        if lines.is_empty() {
            String::from("  No supported tracks found")
        } else {
            lines.join("\n")
        }
    }

    pub async fn select_streams(&self) -> Result<StreamSelection> {
        if self.videos.is_empty() {
            bail!("The source does not contain a supported video track");
        }

        let video = if self.videos.len() == 1 {
            self.videos[0].clone()
        } else {
            let items = self
                .videos
                .iter()
                .map(|stream| stream.display_line.clone())
                .collect::<Vec<_>>();
            let selected =
                prompt_select("Select video track", items, default_index(&self.videos)).await?;
            self.videos[selected].clone()
        };

        let audio = if self.audios.is_empty() {
            None
        } else if self.audios.len() == 1 {
            Some(self.audios[0].clone())
        } else {
            let items = self
                .audios
                .iter()
                .map(|stream| stream.display_line.clone())
                .collect::<Vec<_>>();
            let selected =
                prompt_select("Select audio track", items, default_index(&self.audios)).await?;
            Some(self.audios[selected].clone())
        };

        Ok(StreamSelection::new(video, audio))
    }
}

#[derive(Clone, Debug)]
pub struct VideoStream {
    pub stream_index: usize,
    display_line: String,
    is_default: bool,
}

impl VideoStream {
    pub(crate) fn new(
        stream_index: usize,
        display_line: impl Into<String>,
        is_default: bool,
    ) -> Self {
        Self {
            stream_index,
            display_line: display_line.into(),
            is_default,
        }
    }

    pub fn display_line(&self) -> &str {
        &self.display_line
    }
}

#[derive(Clone, Debug)]
pub struct AudioStream {
    pub stream_index: usize,
    pub codec_name: Option<String>,
    display_line: String,
    is_default: bool,
}

impl AudioStream {
    pub(crate) fn new(
        stream_index: usize,
        codec_name: Option<String>,
        display_line: impl Into<String>,
        is_default: bool,
    ) -> Self {
        Self {
            stream_index,
            codec_name,
            display_line: display_line.into(),
            is_default,
        }
    }

    pub fn display_line(&self) -> &str {
        &self.display_line
    }
}
trait DefaultTrack {
    fn is_default(&self) -> bool;
}

impl DefaultTrack for VideoStream {
    fn is_default(&self) -> bool {
        self.is_default
    }
}

impl DefaultTrack for AudioStream {
    fn is_default(&self) -> bool {
        self.is_default
    }
}

fn default_index<T: DefaultTrack>(tracks: &[T]) -> usize {
    tracks
        .iter()
        .position(DefaultTrack::is_default)
        .unwrap_or(0)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioHandling {
    Copy,
    TranscodeAlac,
}

#[derive(Clone, Debug)]
pub struct StreamSelection {
    video: VideoStream,
    audio: Option<AudioStream>,
    audio_handling: Option<AudioHandling>,
}

impl StreamSelection {
    pub fn new(video: VideoStream, audio: Option<AudioStream>) -> Self {
        let audio_handling = audio.as_ref().map(|stream| {
            if should_transcode_audio(stream.codec_name.as_deref()) {
                AudioHandling::TranscodeAlac
            } else {
                AudioHandling::Copy
            }
        });

        Self {
            video,
            audio,
            audio_handling,
        }
    }

    pub fn video_stream_index(&self) -> usize {
        self.video.stream_index
    }

    pub fn audio_stream_index(&self) -> Option<usize> {
        self.audio.as_ref().map(|stream| stream.stream_index)
    }

    pub fn audio_handling(&self) -> Option<&AudioHandling> {
        self.audio_handling.as_ref()
    }

    pub fn render_output_file(&self) -> String {
        let mut lines = vec![self.video.display_line().to_string()];
        if let Some(audio) = &self.audio {
            lines.push(audio.display_line().to_string());
        }
        lines.join("\n")
    }

    pub fn selected_audio_summary(&self) -> Option<String> {
        self.audio
            .as_ref()
            .map(|audio| audio.display_line().to_string())
    }

    pub fn audio_notice(&self) -> Option<String> {
        match (&self.audio, self.audio_handling()) {
            (Some(audio), Some(AudioHandling::TranscodeAlac)) => Some(format!(
                "Audio track #{} uses {}. quickbridge will convert it to ALAC so QuickTime Player can play it.",
                audio.stream_index,
                audio
                    .codec_name
                    .as_deref()
                    .unwrap_or("an unsupported codec")
            )),
            _ => None,
        }
    }
}

fn should_transcode_audio(codec_name: Option<&str>) -> bool {
    matches!(codec_name, Some("dts" | "truehd"))
}

async fn prompt_select(prompt: &'static str, items: Vec<String>, default: usize) -> Result<usize> {
    task::spawn_blocking(move || {
        let theme = ColorfulTheme {
            success_prefix: style(String::new()).for_stderr(),
            ..ColorfulTheme::default()
        };

        Select::with_theme(&theme)
            .with_prompt(prompt)
            .report(false)
            .items(&items)
            .default(default)
            .interact_opt()
    })
    .await
    .context("unable to start the selection prompt")?
    .context("unable to read the selection prompt")?
    .context("Selection was canceled")
}

#[derive(Debug)]
pub struct ProbeRunner {
    binary: OsString,
}

impl ProbeRunner {
    pub fn new() -> Self {
        let binary = std::env::var_os("QUICKBRIDGE_FFPROBE_BIN")
            .unwrap_or_else(|| OsString::from("ffprobe"));
        Self { binary }
    }

    #[cfg(test)]
    pub fn with_binary(binary: impl Into<OsString>) -> Self {
        Self {
            binary: binary.into(),
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
                bail!(
                    "Unable to use `{}`. Install ffprobe and make sure the executable is available on PATH",
                    self.binary.to_string_lossy(),
                );
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Unable to execute `{}`", self.binary.to_string_lossy())
                });
            }
        };

        if !status.success() {
            bail!(
                "Unable to use `{}`. Install ffprobe and make sure the executable is available on PATH",
                self.binary.to_string_lossy(),
            );
        }

        Ok(())
    }

    pub async fn probe(&self, source_url: &str) -> Result<MediaInfo> {
        debug!(source_url, "probing source with ffprobe");
        let json = self.probe_json(source_url).await?;
        let summary = self.probe_summary(source_url).await?;
        MediaInfo::from_ffprobe_outputs(&json, &summary)
    }

    pub fn render_probe_commands(&self, source_url: &str) -> Vec<String> {
        vec![
            render_command(
                &self.binary,
                &[
                    OsString::from("-v"),
                    OsString::from("error"),
                    OsString::from("-show_streams"),
                    OsString::from("-show_format"),
                    OsString::from("-of"),
                    OsString::from("json"),
                    OsString::from(source_url),
                ],
            ),
            render_command(
                &self.binary,
                &[
                    OsString::from("-hide_banner"),
                    OsString::from("-i"),
                    OsString::from(source_url),
                ],
            ),
        ]
    }

    async fn probe_json(&self, source_url: &str) -> Result<String> {
        let output = Command::new(&self.binary)
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(source_url)
            .output()
            .await
            .with_context(|| format!("Unable to execute `{}`", self.binary.to_string_lossy()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Unable to inspect the source with ffprobe: {}",
                stderr.trim()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    async fn probe_summary(&self, source_url: &str) -> Result<String> {
        let output = Command::new(&self.binary)
            .args(["-hide_banner", "-i"])
            .arg(source_url)
            .output()
            .await
            .with_context(|| format!("Unable to execute `{}`", self.binary.to_string_lossy()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("Stream #") {
                bail!(
                    "Unable to inspect the source with ffprobe: {}",
                    stderr.trim()
                );
            }
        }

        Ok(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

impl Default for ProbeRunner {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_stream_lines(summary: &str) -> HashMap<usize, String> {
    summary
        .lines()
        .filter_map(parse_stream_line)
        .collect::<HashMap<_, _>>()
}

fn parse_stream_line(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("Stream #")?;
    let (_, rest) = rest.split_once(':')?;
    let index_end = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(rest.len());
    let stream_index = rest.get(..index_end)?.parse::<usize>().ok()?;
    Some((stream_index, trimmed.to_string()))
}

fn fallback_stream_line(stream: &FfprobeStream) -> String {
    let kind = stream.codec_type.as_deref().unwrap_or("unknown");
    let codec = stream.codec_name.as_deref().unwrap_or("unknown");
    format!(
        "Stream #0:{}: {}: {}",
        stream.index,
        capitalize(kind),
        codec
    )
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    index: usize,
    codec_type: Option<String>,
    codec_name: Option<String>,
    disposition: Option<FfprobeDisposition>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeDisposition {
    default: Option<u8>,
}

impl FfprobeDisposition {
    fn is_default(&self) -> Option<bool> {
        self.default.map(|value| value != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioHandling, MediaInfo, ProbeRunner, StreamSelection, VideoStream, parse_stream_line,
    };
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn parses_stream_lines_from_ffprobe_summary() {
        let parsed = parse_stream_line(
            "    Stream #0:2(jpn): Audio: eac3, 48000 Hz, stereo, fltp, 224 kb/s",
        )
        .unwrap();
        assert_eq!(parsed.0, 2);
        assert_eq!(
            parsed.1,
            "Stream #0:2(jpn): Audio: eac3, 48000 Hz, stereo, fltp, 224 kb/s"
        );
    }

    #[test]
    fn renders_input_and_output_streams_with_ffprobe_style() {
        let media = MediaInfo::from_ffprobe_outputs(
            r#"{
              "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264", "disposition": {"default": 1}},
                {"index": 1, "codec_type": "audio", "codec_name": "dts", "disposition": {"default": 1}},
                {"index": 2, "codec_type": "subtitle", "codec_name": "subrip"}
              ],
              "format": {"duration": "1460.4"}
            }"#,
            r#"
Input #0, matroska,webm, from 'video.mkv':
  Stream #0:0: Video: h264 (High), yuv420p, 1920x1080 (default)
  Stream #0:1(eng): Audio: dts, 48000 Hz, 5.1, fltp, 1536 kb/s (default)
  Stream #0:2(eng): Subtitle: subrip
"#,
        )
        .unwrap();

        assert!(
            media
                .render_input_file()
                .contains("Stream #0:1(eng): Audio: dts, 48000 Hz, 5.1, fltp, 1536 kb/s (default)")
        );
        assert_eq!(media.duration().unwrap().to_string(), "00:24:20");

        let selection = media.select_streams_for_tests(0, Some(0));
        assert!(
            selection
                .render_output_file()
                .contains("Stream #0:0: Video: h264 (High), yuv420p, 1920x1080 (default)")
        );
    }

    #[test]
    fn marks_dts_audio_for_transcode() {
        let selection = StreamSelection::new(
            VideoStream {
                stream_index: 0,
                display_line: String::from("Stream #0:0: Video: h264"),
                is_default: true,
            },
            Some(super::AudioStream {
                stream_index: 1,
                codec_name: Some(String::from("dts")),
                display_line: String::from("Stream #0:1: Audio: dts"),
                is_default: true,
            }),
        );

        assert_eq!(
            selection.audio_handling(),
            Some(&AudioHandling::TranscodeAlac)
        );
        assert!(
            selection
                .audio_notice()
                .unwrap()
                .contains("convert it to ALAC")
        );
    }

    #[test]
    fn keeps_eac3_audio_as_copy() {
        let selection = StreamSelection::new(
            VideoStream {
                stream_index: 0,
                display_line: String::from("Stream #0:0: Video: h264"),
                is_default: true,
            },
            Some(super::AudioStream {
                stream_index: 1,
                codec_name: Some(String::from("eac3")),
                display_line: String::from("Stream #0:1: Audio: eac3"),
                is_default: true,
            }),
        );

        assert_eq!(selection.audio_handling(), Some(&AudioHandling::Copy));
        assert_eq!(selection.audio_notice(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probes_with_fake_ffprobe() {
        let script = write_script(
            r#"#!/bin/sh
set -eu
if [ "${1:-}" = "-version" ]; then
  exit 0
fi
if [ "${1:-}" = "-v" ]; then
cat <<'JSON'
{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","disposition":{"default":1}},{"index":1,"codec_type":"audio","codec_name":"aac","disposition":{"default":1}}],"format":{"duration":"65.0"}}
JSON
exit 0
fi
cat >&2 <<'TEXT'
Input #0, matroska,webm, from 'video.mkv':
  Stream #0:0: Video: h264 (High), yuv420p, 1280x720 (default)
  Stream #0:1(eng): Audio: aac, 48000 Hz, stereo, fltp, 160 kb/s (default)
TEXT
"#,
        );

        let runner = ProbeRunner::with_binary(&script);
        runner.ensure_available().await.unwrap();
        let media = runner.probe("https://example.com/video.mkv").await.unwrap();
        assert!(
            media.render_input_file().contains(
                "Stream #0:1(eng): Audio: aac, 48000 Hz, stereo, fltp, 160 kb/s (default)"
            )
        );
        assert_eq!(media.duration().unwrap().to_string(), "00:01:05");
    }

    impl MediaInfo {
        fn select_streams_for_tests(
            &self,
            video_index: usize,
            audio_index: Option<usize>,
        ) -> StreamSelection {
            StreamSelection::new(
                self.videos[video_index].clone(),
                audio_index.map(|index| self.audios[index].clone()),
            )
        }
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
            std::env::temp_dir().join(format!("quickbridge-fake-ffprobe-{millis}-{unique}.sh"));
        fs::write(&path, contents).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path.to_string_lossy().into_owned()
    }
}
