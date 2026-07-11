use clap::{Parser, ValueEnum};
use quickbridge_core::{SimulationScenario, Timecode};

const AFTER_HELP: &str = "\
Environment:
  QUICKBRIDGE_FFMPEG_BIN   Override the ffmpeg executable path
  QUICKBRIDGE_FFPROBE_BIN  Override the ffprobe executable path
  QUICKBRIDGE_RENDER_MODE  Set `plain` or `ansi` to override plain stdout formatting
  RUST_LOG                 Set the log filter. `--verbose` enables `quickbridge=debug`.
";

#[derive(Clone, Debug, ValueEnum)]
enum SimulationArg {
    HappyPath,
    NoRanges,
    UiTour,
}

impl From<SimulationArg> for SimulationScenario {
    fn from(value: SimulationArg) -> Self {
        match value {
            SimulationArg::HappyPath => SimulationScenario::HappyPath,
            SimulationArg::NoRanges => SimulationScenario::NoRanges,
            SimulationArg::UiTour => SimulationScenario::UiTour,
        }
    }
}

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
    pub url: Option<String>,
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
    simulate: Option<SimulationArg>,
    /// Disable the alternate screen and keep the TUI inline in the current terminal buffer.
    #[arg(long, default_value_t = false)]
    pub no_alt_screen: bool,
    /// Run prompt commands non-interactively. Repeat the flag to script multiple commands.
    #[arg(long, value_name = "COMMAND")]
    pub script: Vec<String>,
}

impl Cli {
    pub fn simulation(&self) -> Option<SimulationScenario> {
        self.simulate.clone().map(Into::into)
    }
}
