use crate::{SimulationScenario, Timecode};

/// Selects whether playback uses the real system integrations or a simulation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackMode {
    Live,
    Simulated(SimulationScenario),
}

impl PlaybackMode {
    pub fn label(&self) -> String {
        match self {
            Self::Live => String::from("Live"),
            Self::Simulated(scenario) => format!("Simulation ({})", scenario.label()),
        }
    }
}

/// Startup details needed by callers for user-facing output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartOutcome {
    relay_command: String,
    stream_url: String,
}

impl StartOutcome {
    pub fn new(relay_command: String, stream_url: String) -> Self {
        Self {
            relay_command,
            stream_url,
        }
    }

    pub fn relay_command(&self) -> &str {
        &self.relay_command
    }

    pub fn stream_url(&self) -> &str {
        &self.stream_url
    }
}

/// Live telemetry derived from the current relay session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamTelemetry {
    relay_write_bytes_per_second: u64,
    storage_bytes: u64,
    buffer_ahead: Timecode,
}

impl StreamTelemetry {
    pub fn new(
        relay_write_bytes_per_second: u64,
        storage_bytes: u64,
        buffer_ahead: Timecode,
    ) -> Self {
        Self {
            relay_write_bytes_per_second,
            storage_bytes,
            buffer_ahead,
        }
    }

    pub fn relay_write_bytes_per_second(self) -> u64 {
        self.relay_write_bytes_per_second
    }

    pub fn storage_bytes(self) -> u64 {
        self.storage_bytes
    }

    pub fn buffer_ahead(self) -> Timecode {
        self.buffer_ahead
    }
}

/// A stable view of the active playback session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackSnapshot {
    session_id: u64,
    stream_url: String,
    start_time: Timecode,
    current_time: Timecode,
    player_state: PlayerState,
    telemetry: StreamTelemetry,
}

impl PlaybackSnapshot {
    pub fn new(
        session_id: u64,
        stream_url: String,
        start_time: Timecode,
        current_time: Timecode,
        player_state: PlayerState,
        telemetry: StreamTelemetry,
    ) -> Self {
        Self {
            session_id,
            stream_url,
            start_time,
            current_time,
            player_state,
            telemetry,
        }
    }

    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    pub fn stream_url(&self) -> &str {
        &self.stream_url
    }

    pub fn start_time(&self) -> Timecode {
        self.start_time
    }

    pub fn current_time(&self) -> Timecode {
        self.current_time
    }

    pub fn player_state(&self) -> PlayerState {
        self.player_state
    }

    pub fn telemetry(&self) -> StreamTelemetry {
        self.telemetry
    }
}

/// The current QuickTime-facing playback state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerState {
    Playing,
    Paused,
    WindowClosed,
    AppClosed,
    Unavailable,
}

/// Process exit result for quickbridge runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Completed,
    Interrupted,
}
