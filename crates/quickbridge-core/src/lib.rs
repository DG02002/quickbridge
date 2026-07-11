//! Core domain types for `quickbridge`.
//!
//! This crate holds pure data structures, parsing helpers, and workflow event
//! models that can be shared across the CLI, runtime adapters, and TUI.

mod command;
mod media;
mod playback;
mod progress;
mod session;
mod simulation;
mod timecode;
mod workflow;

pub use command::{Command, CommandParseError, help_text, parse_command, resolve_target};
pub use media::{
    AudioHandling, AudioStream, MediaInfo, MediaInfoParseError, SeekSupport, SourceInspection,
    SourceMetadata, StreamSelection, TrackSelectionError, TrackSelectionRequest, VideoPackaging,
    VideoPackagingError, VideoStream,
};
pub use playback::{
    PlaybackMode, PlaybackSnapshot, PlayerState, RunOutcome, StartOutcome, StreamTelemetry,
};
pub use progress::{ProgressEvent, ProgressSink};
pub use session::{SessionState, SessionStateError};
pub use simulation::SimulationScenario;
pub use timecode::{Timecode, TimecodeParseError};
pub use workflow::{JumpEvent, JumpStep, LaunchEvent, LaunchStep, PrepareEvent, PrepareStep};
