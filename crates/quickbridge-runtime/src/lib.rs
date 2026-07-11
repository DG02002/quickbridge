//! Runtime adapters and async workflows for `quickbridge`.

mod diagnostics;
mod error;
mod ffmpeg;
mod playback;
mod player;
mod prepare;
mod probe;
mod progress;
mod server;
mod session;
mod simulate;
mod source;

pub use error::{Result, RuntimeError};
pub use ffmpeg::{FfmpegProcess, FfmpegRunner, has_playable_output};
pub use playback::{PlaybackCoordinator, StartRequest};
pub use player::{PlaybackStatus, QuickTimePlayer};
pub use prepare::{PrepareRequest, PreparedSource, prepare_source};
pub use probe::ProbeRunner;
pub use server::{ActiveSession, ServerHandle, resolve_request_path};
pub use session::{SessionManager, SessionPaths};
pub use source::inspect_source;
