use quickbridge_core::{MediaInfoParseError, SessionStateError, VideoPackagingError};
use std::{path::PathBuf, process::ExitStatus};
use thiserror::Error;

/// Typed runtime errors returned by async adapters and workflows.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("interrupted")]
    Interrupted,
    #[error("invalid source URL `{source_url}`")]
    InvalidSourceUrl { source_url: String },
    #[error(
        "unable to use `{binary}`. Install ffmpeg and make sure the executable is available on PATH"
    )]
    FfmpegUnavailable { binary: String },
    #[error(
        "unable to use `{binary}`. Install ffprobe and make sure the executable is available on PATH"
    )]
    FfprobeUnavailable { binary: String },
    #[error("unable to execute `{binary}`")]
    ExecuteBinary {
        binary: String,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to start `{binary}` for session {session_id}")]
    StartBinary {
        binary: String,
        session_id: u64,
        #[source]
        source: std::io::Error,
    },
    #[error("unable to inspect the source with ffprobe: {stderr}")]
    FfprobeFailed { stderr: String },
    #[error("ffmpeg exited before the stream was ready with status `{status}`")]
    FfmpegExitedEarly { status: ExitStatus },
    #[error("timed out while waiting for ffmpeg output at `{playlist_path}`")]
    FfmpegReadyTimeout { playlist_path: PathBuf },
    #[error("unable to stop ffmpeg")]
    StopFfmpeg {
        #[source]
        source: std::io::Error,
    },
    #[error("unable to control QuickTime Player: {stderr}")]
    QuickTimeControl { stderr: String },
    #[error("unable to run osascript for QuickTime control")]
    QuickTimeScript {
        #[source]
        source: std::io::Error,
    },
    #[error("QuickTime returned an unexpected playback status")]
    QuickTimeUnexpectedStatus,
    #[error("QuickTime returned an invalid playhead")]
    QuickTimeInvalidPlayhead,
    #[error("QuickTime returned an invalid playback flag")]
    QuickTimeInvalidPlaybackFlag,
    #[error("unable to bind the local HLS server")]
    BindLocalServer {
        #[source]
        source: std::io::Error,
    },
    #[error("the local HLS server stopped unexpectedly")]
    LocalServerStopped {
        #[source]
        source: std::io::Error,
    },
    #[error("the local HLS server task failed")]
    LocalServerTask {
        #[source]
        source: tokio::task::JoinError,
    },
    #[error("unable to read HLS playlist for playback tracking")]
    ReadTrackingPlaylist {
        #[source]
        source: std::io::Error,
    },
    #[error("progress sink failed: {message}")]
    ProgressSink { message: String },
    #[cfg(test)]
    #[error("{0}")]
    TestDriver(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    MediaInfoParse(#[from] MediaInfoParseError),
    #[error(transparent)]
    VideoPackaging(#[from] VideoPackagingError),
    #[error(transparent)]
    SessionState(#[from] SessionStateError),
}

/// Crate-local result alias using [`RuntimeError`].
pub type Result<T> = std::result::Result<T, RuntimeError>;
