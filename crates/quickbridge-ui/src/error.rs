use thiserror::Error;

/// Typed UI errors returned by the interactive Ratatui front-end.
#[derive(Debug, Error)]
pub enum UiError {
    #[error("interrupted")]
    Interrupted,
    #[error(transparent)]
    Runtime(#[from] quickbridge_runtime::RuntimeError),
    #[error(transparent)]
    CommandParse(#[from] quickbridge_core::CommandParseError),
    #[error(transparent)]
    TrackSelection(#[from] quickbridge_core::TrackSelectionError),
    #[error("track selection is not active")]
    TrackSelectionInactive,
    #[error("unable to {action}")]
    Terminal {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
}

/// Crate-local result alias using [`UiError`].
pub type Result<T> = std::result::Result<T, UiError>;
