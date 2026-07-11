use crate::{Result, session::SessionPaths};
use quickbridge_core::{
    AudioStream, MediaInfo, SeekSupport, SimulationScenario, SourceInspection, SourceMetadata,
    StreamSelection, Timecode, VideoStream,
};
use reqwest::Url;
use tokio::{
    fs,
    time::{Duration, sleep},
};

pub trait SimulationRuntimeExt {
    fn render_probe_commands(&self, source_url: &str) -> Vec<String>;
    fn render_spawn_command(
        &self,
        source_url: &str,
        start_at: Timecode,
        selection: &StreamSelection,
    ) -> String;
    fn render_open_command(&self, stream_url: &str) -> String;
    async fn inspect_source(&self, source_url: &str) -> Result<SourceInspection>;
    async fn probe_source(&self, source_url: &str) -> Result<MediaInfo>;
    async fn stage_playback(
        &self,
        session: &SessionPaths,
        source_url: &str,
        start_at: Timecode,
        selection: &StreamSelection,
    ) -> Result<()>;
    async fn open_player(&self, stream_url: &str) -> Result<()>;
    async fn reload_player(&self, stream_url: &str) -> Result<()>;
    async fn quit_player(&self) -> Result<()>;
}

impl SimulationRuntimeExt for SimulationScenario {
    async fn inspect_source(&self, source_url: &str) -> Result<SourceInspection> {
        let scenario = self.clone();
        let source_url = source_url.to_string();
        sleep(Duration::from_millis(120)).await;

        let metadata = SourceMetadata::new(filename_from_url(&source_url), Some(1_377_078_272));
        let seek_support = match scenario {
            SimulationScenario::HappyPath => SeekSupport::Enabled,
            SimulationScenario::NoRanges => SeekSupport::Disabled {
                warning: String::from(
                    "This source doesn't appear to support jumping to a different time.",
                ),
            },
        };

        Ok(SourceInspection::new(metadata, seek_support))
    }

    async fn probe_source(&self, _source_url: &str) -> Result<MediaInfo> {
        sleep(Duration::from_millis(160)).await;

        Ok(MediaInfo::new(
            vec![VideoStream::new(
                0,
                "Stream #0:0: Video: h264 (High), yuv420p, 1920x1080 (default)",
                true,
            )],
            vec![AudioStream::new(
                1,
                Some(String::from("aac")),
                "Stream #0:1(eng): Audio: aac, 48000 Hz, stereo, fltp, 160 kb/s (default)",
                true,
            )],
            Some(Timecode::from_seconds(1_452)),
        ))
    }

    fn render_probe_commands(&self, source_url: &str) -> Vec<String> {
        vec![format!("simulate ffprobe --json {source_url}")]
    }

    fn render_spawn_command(
        &self,
        source_url: &str,
        start_at: Timecode,
        selection: &StreamSelection,
    ) -> String {
        format!(
            "simulate ffmpeg --source {source_url} --at {start_at} --video {}{}",
            selection.video_stream_index(),
            selection
                .audio_stream_index()
                .map(|index| format!(" --audio {index}"))
                .unwrap_or_default()
        )
    }

    async fn stage_playback(
        &self,
        session: &SessionPaths,
        _source_url: &str,
        _start_at: Timecode,
        _selection: &StreamSelection,
    ) -> Result<()> {
        let playlist_path = session.playlist_path.clone();
        let segment_path = session.segment_path(1);
        let segment_name = session.segment_filename(1);
        sleep(Duration::from_millis(220)).await;

        let playlist = format!("#EXTM3U\n#EXT-X-VERSION:7\n#EXTINF:2.0,\n{segment_name}\n");
        fs::write(playlist_path, playlist).await?;
        fs::write(segment_path, b"segment").await?;
        Ok(())
    }

    fn render_open_command(&self, stream_url: &str) -> String {
        format!("simulate quicktime open {stream_url}")
    }

    async fn open_player(&self, _stream_url: &str) -> Result<()> {
        sleep(Duration::from_millis(120)).await;
        Ok(())
    }

    async fn reload_player(&self, _stream_url: &str) -> Result<()> {
        sleep(Duration::from_millis(180)).await;
        Ok(())
    }

    async fn quit_player(&self) -> Result<()> {
        sleep(Duration::from_millis(60)).await;
        Ok(())
    }
}

fn filename_from_url(source_url: &str) -> String {
    Url::parse(source_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
                .map(str::to_string)
        })
        .unwrap_or_else(|| String::from("simulation-source.mkv"))
}
