use crate::{simulate::SimulationScenario, timecode::Timecode};
use clap::Parser;

const AFTER_HELP: &str = "\
Environment:
  QUICKBRIDGE_FFMPEG_BIN   Override the ffmpeg executable path
  QUICKBRIDGE_FFPROBE_BIN  Override the ffprobe executable path
  QUICKBRIDGE_RENDER_MODE  Set `plain` to disable ANSI redraws for scripted tests
  RUST_LOG                 Set the log filter. `--verbose` enables `quickbridge=debug`.
";

#[derive(Debug, Parser, Clone)]
#[command(
    name = "quickbridge",
    version,
    about = "Relay a media source through ffmpeg into QuickTime Player with interactive timestamp jumps",
    long_about = None,
    after_help = AFTER_HELP
)]
pub struct Cli {
    /// Media URL to relay through ffmpeg.
    #[arg(value_name = "URL")]
    pub url: String,
    /// Port to bind the local HLS server to. Use 0 to choose a free port automatically.
    #[arg(long, default_value_t = 0)]
    pub port: u16,
    /// Start playback at a source timestamp, for example `90`, `01:30`, or `01:02:03`.
    #[arg(long, value_name = "TIMESTAMP")]
    pub at: Option<Timecode>,
    /// Print debug logs to stderr.
    #[arg(long)]
    pub verbose: bool,
    /// Keep session files on disk after quickbridge exits.
    #[arg(long)]
    pub keep_temp: bool,
    /// Simulate the full quickbridge flow without ffmpeg, ffprobe, QuickTime, or remote servers.
    #[arg(long, value_enum, value_name = "SCENARIO")]
    pub simulate: Option<SimulationScenario>,
    /// Run prompt commands non-interactively. Repeat the flag to script multiple commands.
    #[arg(long, value_name = "COMMAND")]
    pub script: Vec<String>,
}

impl Cli {
    pub fn stream_url(&self, port: u16, session_id: u64) -> String {
        format!("http://127.0.0.1:{port}/stream.m3u8?session={session_id}")
    }
}
