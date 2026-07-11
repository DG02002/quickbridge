use crate::ProgressEvent;

/// Steps emitted while preparing the source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PrepareStep {
    SourceUrl,
    TimeJumps,
    SourceDetails,
    Tracks,
}

impl PrepareStep {
    pub const ALL: [Self; 4] = [
        Self::SourceUrl,
        Self::TimeJumps,
        Self::SourceDetails,
        Self::Tracks,
    ];
}

/// Steps emitted while starting playback.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LaunchStep {
    LocalStreamServer,
    Relay,
    Player,
}

impl LaunchStep {
    pub const ALL: [Self; 3] = [Self::LocalStreamServer, Self::Relay, Self::Player];
}

/// Steps emitted while switching to a new playback session.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum JumpStep {
    PrepareNextStream,
    WaitForStream,
    RefreshPlayer,
    CleanupPreviousSession,
}

impl JumpStep {
    pub const ALL: [Self; 4] = [
        Self::PrepareNextStream,
        Self::WaitForStream,
        Self::RefreshPlayer,
        Self::CleanupPreviousSession,
    ];
}

/// Structured preparation progress event.
pub type PrepareEvent = ProgressEvent<PrepareStep>;
/// Structured launch progress event.
pub type LaunchEvent = ProgressEvent<LaunchStep>;
/// Structured jump progress event.
pub type JumpEvent = ProgressEvent<JumpStep>;
