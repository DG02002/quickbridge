use anyhow::Error;
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

pub fn init_logging(verbose: bool) {
    let filter = match EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(_) if verbose => EnvFilter::new("quickbridge=debug"),
        Err(_) => EnvFilter::new("warn"),
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(std::io::stderr)
        .without_time()
        .compact()
        .try_init();
}

pub fn print_error(error: &Error) {
    eprintln!("ERROR: {error}");

    let mut causes = error.chain().skip(1).peekable();
    if causes.peek().is_none() {
        return;
    }

    eprintln!();
    eprintln!("DETAILS:");
    for cause in causes {
        eprintln!("  - {cause}");
    }
}
