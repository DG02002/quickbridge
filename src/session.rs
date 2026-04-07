use crate::timecode::Timecode;
use anyhow::{Result, bail};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::fs;

static ROOT_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionPaths {
    pub id: u64,
    pub dir: PathBuf,
    pub playlist_path: PathBuf,
    pub segment_pattern: PathBuf,
    pub init_filename: String,
}

impl SessionPaths {
    pub fn segment_filename(&self, index: u64) -> String {
        format!("segment_{:04}_{index:05}.m4s", self.id)
    }

    pub fn segment_path(&self, index: u64) -> PathBuf {
        self.dir.join(self.segment_filename(index))
    }
}

#[derive(Debug)]
pub struct SessionManager {
    root: PathBuf,
    counter: AtomicU64,
    keep_temp: bool,
}

impl SessionManager {
    pub async fn new(keep_temp: bool) -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let unique = ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "quickbridge-{nanos}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).await?;
        Ok(Self {
            root,
            counter: AtomicU64::new(1),
            keep_temp,
        })
    }

    #[cfg(test)]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub async fn create_session(&self) -> Result<SessionPaths> {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        let dir = self.root.join(format!("session-{id:04}"));
        fs::create_dir_all(&dir).await?;
        Ok(SessionPaths {
            id,
            playlist_path: dir.join("stream.m3u8"),
            segment_pattern: dir.join(format!("segment_{id:04}_%05d.m4s")),
            init_filename: format!("init_{id:04}.mp4"),
            dir,
        })
    }

    pub async fn remove_session(&self, session: &SessionPaths) -> Result<()> {
        if self.keep_temp {
            return Ok(());
        }

        if fs::try_exists(&session.dir).await? {
            fs::remove_dir_all(&session.dir).await?;
        }
        Ok(())
    }

    pub async fn cleanup_root(&self) -> Result<()> {
        if self.keep_temp {
            return Ok(());
        }

        if fs::try_exists(&self.root).await? {
            fs::remove_dir_all(&self.root).await?;
        }
        Ok(())
    }
}

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

    pub fn commit_switch(&mut self, now: Instant) -> Result<()> {
        let Some(session_id) = self.staged_session_id.take() else {
            bail!("cannot commit a session switch without a staged session");
        };
        let Some(offset) = self.staged_offset.take() else {
            bail!("cannot commit a session switch without a staged offset");
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

#[cfg(test)]
mod tests {
    use super::{SessionManager, SessionState};
    use crate::timecode::Timecode;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn creates_and_cleans_up_session_directories() {
        let manager = SessionManager::new(false).await.unwrap();
        let session = manager.create_session().await.unwrap();
        assert!(tokio::fs::try_exists(&session.dir).await.unwrap());
        manager.remove_session(&session).await.unwrap();
        assert!(!tokio::fs::try_exists(&session.dir).await.unwrap());
        manager.cleanup_root().await.unwrap();
        assert!(!tokio::fs::try_exists(manager.root()).await.unwrap());
    }

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

    #[test]
    fn estimated_position_uses_new_baseline_after_commit() {
        let started = Instant::now();
        let mut state = SessionState::new(1, Timecode::from_seconds(30), started);
        state.stage_switch(2, Timecode::from_seconds(90));
        let switched_at = started + Duration::from_secs(2);
        state.commit_switch(switched_at).unwrap();

        let estimated = state.estimated_position(switched_at + Duration::from_secs(7));
        assert_eq!(estimated, Timecode::from_seconds(97));
    }
}
