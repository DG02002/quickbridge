use crate::Timecode;
use std::time::Instant;
use thiserror::Error;

/// Tracks playback time across session switches.
#[derive(Clone, Debug)]
pub struct SessionState {
    active_session_id: u64,
    committed_offset: Timecode,
    activated_at: Instant,
    staged_session_id: Option<u64>,
    staged_offset: Option<Timecode>,
}

impl SessionState {
    pub fn new(active_session_id: u64, committed_offset: Timecode, activated_at: Instant) -> Self {
        Self {
            active_session_id,
            committed_offset,
            activated_at,
            staged_session_id: None,
            staged_offset: None,
        }
    }

    pub fn active_session_id(&self) -> u64 {
        self.active_session_id
    }

    pub fn committed_offset(&self) -> Timecode {
        self.committed_offset
    }

    pub fn estimated_position(&self, now: Instant) -> Timecode {
        let elapsed = now.saturating_duration_since(self.activated_at).as_secs();
        self.committed_offset.apply_delta(elapsed as i64)
    }

    pub fn stage_switch(&mut self, session_id: u64, target_offset: Timecode) {
        self.staged_session_id = Some(session_id);
        self.staged_offset = Some(target_offset);
    }

    pub fn commit_switch(&mut self, now: Instant) -> Result<(), SessionStateError> {
        let Some(session_id) = self.staged_session_id.take() else {
            return Err(SessionStateError::MissingStagedSession);
        };
        let Some(offset) = self.staged_offset.take() else {
            return Err(SessionStateError::MissingStagedOffset);
        };
        self.active_session_id = session_id;
        self.committed_offset = offset;
        self.activated_at = now;
        Ok(())
    }

    pub fn abort_stage(&mut self) {
        self.staged_session_id = None;
        self.staged_offset = None;
    }
}

/// Errors returned by [`SessionState::commit_switch`].
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SessionStateError {
    #[error("cannot commit a session switch without a staged session")]
    MissingStagedSession,
    #[error("cannot commit a session switch without a staged offset")]
    MissingStagedOffset,
}

#[cfg(test)]
mod tests {
    use super::SessionState;
    use crate::Timecode;
    use std::time::{Duration, Instant};

    #[test]
    fn tracks_estimated_position() {
        let started = Instant::now();
        let state = SessionState::new(1, Timecode::from_seconds(120), started);
        let estimated = state.estimated_position(started + Duration::from_secs(8));
        assert_eq!(estimated, Timecode::from_seconds(128));
    }

    #[test]
    fn stages_and_commits_switches() {
        let started = Instant::now();
        let mut state = SessionState::new(1, Timecode::from_seconds(30), started);
        state.stage_switch(2, Timecode::from_seconds(90));
        state
            .commit_switch(started + Duration::from_secs(2))
            .unwrap();
        assert_eq!(state.active_session_id(), 2);
        assert_eq!(state.committed_offset(), Timecode::from_seconds(90));
    }

    #[test]
    fn aborted_stage_keeps_previous_state() {
        let started = Instant::now();
        let mut state = SessionState::new(1, Timecode::from_seconds(30), started);
        state.stage_switch(2, Timecode::from_seconds(90));
        state.abort_stage();
        assert_eq!(state.active_session_id(), 1);
        assert_eq!(state.committed_offset(), Timecode::from_seconds(30));
    }
}
