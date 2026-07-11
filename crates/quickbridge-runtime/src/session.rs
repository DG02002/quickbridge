use crate::Result;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
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

    pub fn root_path(&self) -> &Path {
        &self.root
    }

    pub async fn total_storage_bytes(&self) -> Result<u64> {
        dir_size(&self.root).await
    }
}

async fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0_u64;
    let mut pending = vec![path.to_path_buf()];

    while let Some(next) = pending.pop() {
        let mut entries = match fs::read_dir(&next).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::SessionManager;

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
}
