//! Ratatui front-end for `quickbridge`.

mod app;
mod error;
mod event;
mod render;
mod runtime;
mod terminal_detection;
mod text;

#[cfg(test)]
mod test_backend;

use quickbridge_core::{RunOutcome, SimulationScenario, Timecode};
use quickbridge_runtime::{FfmpegRunner, ProbeRunner};

pub use app::run_interactive;
pub use error::{Result, UiError};

/// Options needed to launch the interactive TUI.
#[derive(Clone, Debug)]
pub struct InteractiveOptions {
    pub url: Option<String>,
    pub port: u16,
    pub at: Option<Timecode>,
    pub verbose: bool,
    pub keep_temp: bool,
    pub simulation: Option<SimulationScenario>,
    pub no_alt_screen: bool,
}

pub type UiResult = Result<RunOutcome>;

pub async fn run(
    options: InteractiveOptions,
    runner: FfmpegRunner,
    probe: ProbeRunner,
) -> UiResult {
    run_interactive(options, runner, probe).await
}
