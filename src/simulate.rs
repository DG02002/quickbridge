use crate::{
    probe::{AudioStream, MediaInfo, StreamSelection, VideoStream},
    session::SessionPaths,
    source::{SeekSupport, SourceInspection, SourceMetadata},
    timecode::Timecode,
};
use anyhow::Result;
use clap::ValueEnum;
use reqwest::Url;
use tokio::{
    fs,
    time::{Duration, sleep},
};

#[derive(Clone, Debug, ValueEnum, Eq, PartialEq)]
pub enum SimulationScenario {
    HappyPath,
    NoRanges,
}

impl SimulationScenario {
    pub fn label(&self) -> &'static str {
        match self {
            Self::HappyPath => "happy-path",
            Self::NoRanges => "no-ranges",
        }
    }

    pub async fn inspect_source(&self, source_url: &str) -> Result<SourceInspection> {
        sleep(Duration::from_millis(120)).await;

        let metadata = SourceMetadata::new(filename_from_url(source_url), Some(1_377_078_272));
        let seek_support = match self {
            Self::HappyPath => SeekSupport::Enabled,
            Self::NoRanges => SeekSupport::Disabled {
                warning: String::from(
                    "This source doesn't appear to support jumping to a different time.",
                ),
            },
        };

        Ok(SourceInspection::new(metadata, seek_support))
    }

    pub async fn probe_source(&self, _source_url: &str) -> Result<MediaInfo> {
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

    pub fn render_probe_commands(&self, source_url: &str) -> Vec<String> {
        vec![
            format!("simulate ffprobe --json {source_url}"),
            format!("simulate ffprobe --summary {source_url}"),
        ]
    }

    pub fn render_spawn_command(
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

    pub async fn stage_playback(
        &self,
        session: &SessionPaths,
        source_url: &str,
        start_at: Timecode,
        selection: &StreamSelection,
    ) -> Result<()> {
        let _ = (source_url, start_at, selection);
        sleep(Duration::from_millis(220)).await;

        let playlist = format!(
            "#EXTM3U\n#EXT-X-VERSION:7\n#EXTINF:2.0,\n{}\n",
            session.segment_filename(1)
        );
        fs::write(&session.playlist_path, playlist).await?;
        fs::write(session.segment_path(1), b"segment").await?;
        Ok(())
    }

    pub fn render_open_command(&self, stream_url: &str) -> String {
        format!("simulate quicktime open {stream_url}")
    }

    pub async fn open_player(&self, _stream_url: &str) -> Result<()> {
        sleep(Duration::from_millis(120)).await;
        Ok(())
    }

    pub async fn reload_player(&self, _stream_url: &str) -> Result<()> {
        sleep(Duration::from_millis(180)).await;
        Ok(())
    }

    pub async fn quit_player(&self) -> Result<()> {
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
