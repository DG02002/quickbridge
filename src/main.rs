mod app;
mod cli;
mod diagnostics;
mod ffmpeg;
mod player;
mod probe;
mod prompt;
mod server;
mod session;
mod simulate;
mod source;
mod terminal;
mod timecode;

use clap::Parser;
use cli::Cli;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(error.exit_code() as u8);
        }
    };

    diagnostics::init_logging(cli.verbose);

    match app::run(cli).await {
        Ok(app::RunOutcome::Completed) => ExitCode::SUCCESS,
        Ok(app::RunOutcome::Interrupted) => ExitCode::from(130),
        Err(error) => {
            diagnostics::print_error(&error);
            ExitCode::from(1)
        }
    }
}
