use crate::{Result, RuntimeError};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, Uri, header},
    response::Response,
    routing::get,
};
use quickbridge_core::Timecode;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Instant, SystemTime},
};
use tokio::{
    fs,
    net::TcpListener,
    sync::{RwLock, oneshot},
    task::JoinHandle,
};
use tokio_util::io::ReaderStream;
use tracing::debug;

#[derive(Debug, Default)]
pub struct ActiveSession {
    dir: RwLock<Option<PathBuf>>,
    playback: RwLock<PlaybackTracker>,
    playlist_cache: RwLock<Option<PlaylistCache>>,
}

impl ActiveSession {
    pub async fn set_active_dir(&self, dir: PathBuf) {
        *self.dir.write().await = Some(dir);
        self.playback.write().await.reset();
        *self.playlist_cache.write().await = None;
    }

    pub async fn clear(&self) {
        *self.dir.write().await = None;
        self.playback.write().await.reset();
        *self.playlist_cache.write().await = None;
    }

    pub async fn active_dir(&self) -> Option<PathBuf> {
        self.dir.read().await.clone()
    }

    pub async fn note_segment_request(&self, request_path: &str) {
        let Some(active_dir) = self.active_dir().await else {
            return;
        };
        let Some(segment_name) = request_path.strip_prefix('/') else {
            return;
        };

        let Ok(Some(segment)) = self
            .resolve_segment(active_dir.as_path(), segment_name)
            .await
        else {
            return;
        };

        self.playback.write().await.observe(segment);
    }

    async fn resolve_segment(
        &self,
        active_dir: &Path,
        segment_name: &str,
    ) -> Result<Option<SegmentObservation>> {
        let playlist_path = active_dir.join("stream.m3u8");
        let metadata = match fs::metadata(&playlist_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(RuntimeError::ReadTrackingPlaylist { source }),
        };
        let version = PlaylistVersion {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        };
        if let Some(cache) = self.playlist_cache.read().await.as_ref()
            && cache.version == version
        {
            return Ok(cache.segments.get(segment_name).copied());
        }
        let playlist = fs::read_to_string(&playlist_path)
            .await
            .map_err(|source| RuntimeError::ReadTrackingPlaylist { source })?;
        let segments = parse_segment_map(&playlist);
        let observation = segments.get(segment_name).copied();
        *self.playlist_cache.write().await = Some(PlaylistCache { version, segments });
        Ok(observation)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct PlaylistVersion {
    modified: Option<SystemTime>,
    len: u64,
}

#[derive(Debug)]
struct PlaylistCache {
    version: PlaylistVersion,
    segments: HashMap<String, SegmentObservation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SegmentObservation {
    start: Timecode,
    duration_seconds: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PlaybackTracker {
    last_segment: Option<SegmentObservation>,
    observed_at: Option<Instant>,
}

impl PlaybackTracker {
    fn reset(&mut self) {
        self.last_segment = None;
        self.observed_at = None;
    }

    fn observe(&mut self, segment: SegmentObservation) {
        self.last_segment = Some(segment);
        self.observed_at = Some(Instant::now());
    }
}

#[derive(Debug)]
pub struct ServerHandle {
    port: u16,
    state: Arc<ActiveSession>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<()>>>,
}

impl ServerHandle {
    pub async fn start(port: u16) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|source| RuntimeError::BindLocalServer { source })?;
        let port = listener.local_addr()?.port();
        debug!(port, "starting local HLS server");
        let state = Arc::new(ActiveSession::default());
        let router = Router::new()
            .route("/", get(root))
            .fallback(get(serve_asset))
            .with_state(Arc::clone(&state));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|source| RuntimeError::LocalServerStopped { source })
        });

        Ok(Self {
            port,
            state,
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn state(&self) -> Arc<ActiveSession> {
        Arc::clone(&self.state)
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        debug!(port = self.port, "stopping local HLS server");
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let result = task
                .await
                .map_err(|source| RuntimeError::LocalServerTask { source })?;
            result?;
        }
        Ok(())
    }
}

async fn root() -> &'static str {
    "quickbridge HLS relay"
}

async fn serve_asset(State(state): State<Arc<ActiveSession>>, uri: Uri) -> Response<Body> {
    match serve_asset_impl(&state, uri.path()).await {
        Ok(response) => response,
        Err(response) => response,
    }
}

async fn serve_asset_impl(
    state: &ActiveSession,
    request_path: &str,
) -> std::result::Result<Response<Body>, Response<Body>> {
    let Some(active_dir) = state.active_dir().await else {
        return Err(response(
            StatusCode::SERVICE_UNAVAILABLE,
            "stream is not ready",
        ));
    };
    let Some(path) = resolve_request_path(&active_dir, request_path) else {
        return Err(response(StatusCode::NOT_FOUND, "not found"));
    };

    if request_path.ends_with(".m3u8") {
        return match fs::read(&path).await {
            Ok(bytes) => Ok(asset_response(
                Body::from(bytes.clone()),
                &path,
                bytes.len() as u64,
                "no-cache",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(response(StatusCode::NOT_FOUND, "not found"))
            }
            Err(_) => Err(response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "unable to read the stream asset",
            )),
        };
    }
    match fs::File::open(&path).await {
        Ok(file) => {
            let len = match file.metadata().await {
                Ok(metadata) => metadata.len(),
                Err(_) => {
                    return Err(response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "unable to read the stream asset",
                    ));
                }
            };
            if is_segment_request(request_path) {
                state.note_segment_request(request_path).await;
            }
            Ok(asset_response(
                Body::from_stream(ReaderStream::new(file)),
                &path,
                len,
                "public, max-age=31536000, immutable",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(response(StatusCode::NOT_FOUND, "not found"))
        }
        Err(_) => Err(response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "unable to read the stream asset",
        )),
    }
}

fn asset_response(
    body: Body,
    path: &Path,
    len: u64,
    cache_control: &'static str,
) -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, content_type(path))
        .header(header::CONTENT_LENGTH, len)
        .header(header::CACHE_CONTROL, cache_control)
        .body(body)
        .expect("valid asset response")
}

fn is_segment_request(request_path: &str) -> bool {
    Path::new(request_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "m4s" | "ts"))
}

fn parse_segment_map(playlist: &str) -> HashMap<String, SegmentObservation> {
    let mut segments = HashMap::new();
    let mut next_duration = None::<u64>;
    let mut next_start = 0_u64;
    let target_duration = parse_target_duration(playlist);

    for line in playlist.lines().map(str::trim) {
        if let Some(duration) = parse_extinf_seconds(line) {
            next_duration = Some(duration);
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let duration = next_duration.take().unwrap_or(0);
        let start_seconds = parse_segment_index(line)
            .zip(target_duration)
            .map(|(index, target_duration)| index.saturating_mul(target_duration))
            .unwrap_or(next_start);
        segments.insert(
            line.to_string(),
            SegmentObservation {
                start: Timecode::from_seconds(start_seconds),
                duration_seconds: duration,
            },
        );
        next_start = start_seconds.saturating_add(duration);
    }

    segments
}

fn parse_target_duration(playlist: &str) -> Option<u64> {
    playlist.lines().find_map(|line| {
        line.trim()
            .strip_prefix("#EXT-X-TARGETDURATION:")
            .and_then(|value| value.trim().parse::<u64>().ok())
    })
}

fn parse_extinf_seconds(line: &str) -> Option<u64> {
    let value = line.strip_prefix("#EXTINF:")?.split(',').next()?.trim();
    let seconds = value.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds.is_sign_negative() {
        return None;
    }
    Some(seconds.ceil() as u64)
}

fn parse_segment_index(name: &str) -> Option<u64> {
    let stem = Path::new(name).file_stem()?.to_str()?;
    stem.rsplit('_').next()?.parse::<u64>().ok()
}

fn response(status: StatusCode, body: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(body))
        .expect("valid response")
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("m3u8") => "application/vnd.apple.mpegurl",
        Some("m4s") => "video/iso.segment",
        Some("mp4") => "video/mp4",
        Some("ts") => "video/mp2t",
        _ => "application/octet-stream",
    }
}

pub fn resolve_request_path(active_dir: &Path, request_path: &str) -> Option<PathBuf> {
    if request_path == "/stream.m3u8" {
        return Some(active_dir.join("stream.m3u8"));
    }

    let name = request_path.strip_prefix('/')?;
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return None;
    }

    let extension = Path::new(name).extension().and_then(|ext| ext.to_str())?;
    if !matches!(extension, "m4s" | "mp4" | "ts" | "m3u8") {
        return None;
    }

    Some(active_dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveSession, parse_extinf_seconds, parse_segment_index, parse_segment_map,
        parse_target_duration, resolve_request_path, serve_asset_impl,
    };
    use axum::{body::to_bytes, http::header};
    use quickbridge_core::Timecode;
    use std::path::Path;

    #[test]
    fn resolves_supported_asset_paths() {
        let root = Path::new("/tmp/quickbridge/session-0001");
        assert_eq!(
            resolve_request_path(root, "/stream.m3u8").unwrap(),
            root.join("stream.m3u8")
        );
        assert_eq!(
            resolve_request_path(root, "/segment_00001.m4s").unwrap(),
            root.join("segment_00001.m4s")
        );
    }

    #[test]
    fn rejects_unsafe_or_unknown_paths() {
        let root = Path::new("/tmp/quickbridge/session-0001");
        assert!(resolve_request_path(root, "/../../etc/passwd").is_none());
        assert!(resolve_request_path(root, "/nested/segment.m4s").is_none());
        assert!(resolve_request_path(root, "/segment.exe").is_none());
    }

    #[test]
    fn parses_extinf_segment_map() {
        let segments = parse_segment_map(
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2.0,\nsegment_00000.m4s\n#EXTINF:1.2,\nsegment_00001.m4s\n",
        );
        assert_eq!(
            segments.get("segment_00000.m4s").unwrap().start,
            Timecode::from_seconds(0)
        );
        assert_eq!(
            segments.get("segment_00001.m4s").unwrap().start,
            Timecode::from_seconds(2)
        );
        assert_eq!(
            parse_target_duration("#EXTM3U\n#EXT-X-TARGETDURATION:2\n").unwrap(),
            2
        );
        assert_eq!(parse_extinf_seconds("#EXTINF:1.2,").unwrap(), 2);
        assert_eq!(parse_segment_index("segment_0001_00042.m4s").unwrap(), 42);
    }

    #[tokio::test]
    async fn streams_media_with_metadata_and_keeps_playlists_uncached() {
        let root = tempfile::tempdir().unwrap();
        let state = ActiveSession::default();
        state.set_active_dir(root.path().to_path_buf()).await;
        let bytes = vec![0x5a; 2 * 1024 * 1024];
        tokio::fs::write(root.path().join("segment_00001.m4s"), &bytes)
            .await
            .unwrap();
        tokio::fs::write(root.path().join("stream.m3u8"), "#EXTM3U\n")
            .await
            .unwrap();

        let response = serve_asset_impl(&state, "/segment_00001.m4s")
            .await
            .unwrap();
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH],
            bytes.len().to_string()
        );
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "video/iso.segment"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            to_bytes(response.into_body(), bytes.len() + 1)
                .await
                .unwrap()
                .as_ref(),
            bytes
        );

        let playlist = serve_asset_impl(&state, "/stream.m3u8").await.unwrap();
        assert_eq!(playlist.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(serve_asset_impl(&state, "/missing.m4s").await.is_err());
        assert!(serve_asset_impl(&state, "/../secret.m4s").await.is_err());
    }

    #[tokio::test]
    async fn caches_segment_map_until_playlist_version_changes() {
        let root = tempfile::tempdir().unwrap();
        let state = ActiveSession::default();
        state.set_active_dir(root.path().to_path_buf()).await;
        let path = root.path().join("stream.m3u8");
        tokio::fs::write(&path, "#EXTM3U\n#EXTINF:2,\nsegment_00001.m4s\n")
            .await
            .unwrap();
        assert!(
            state
                .resolve_segment(root.path(), "segment_00001.m4s")
                .await
                .unwrap()
                .is_some()
        );
        let first_version = state
            .playlist_cache
            .read()
            .await
            .as_ref()
            .unwrap()
            .version
            .len;
        assert!(
            state
                .resolve_segment(root.path(), "segment_00001.m4s")
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            state
                .playlist_cache
                .read()
                .await
                .as_ref()
                .unwrap()
                .version
                .len,
            first_version
        );

        tokio::fs::write(
            &path,
            "#EXTM3U\n#EXTINF:2,\nsegment_00002.m4s\n#EXT-X-ENDLIST\n",
        )
        .await
        .unwrap();
        assert!(
            state
                .resolve_segment(root.path(), "segment_00002.m4s")
                .await
                .unwrap()
                .is_some()
        );
        assert_ne!(
            state
                .playlist_cache
                .read()
                .await
                .as_ref()
                .unwrap()
                .version
                .len,
            first_version
        );
    }
}
