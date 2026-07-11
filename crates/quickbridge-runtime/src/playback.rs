use crate::{
    Result, RuntimeError,
    ffmpeg::{FfmpegProcess, FfmpegRunner},
    player::{PlaybackStatus, QuickTimePlayer},
    progress::spin_with_ticks,
    server::ServerHandle,
    session::{SessionManager, SessionPaths},
    simulate::SimulationRuntimeExt,
};
use quickbridge_core::{JumpEvent, JumpStep, LaunchEvent, LaunchStep, ProgressEvent, ProgressSink};
use quickbridge_core::{
    PlaybackMode, PlaybackSnapshot, PlayerState, SessionState, SimulationScenario, StartOutcome,
    StreamSelection, StreamTelemetry, Timecode,
};
#[cfg(test)]
use std::sync::Arc;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::fs;

/// Configuration used to start a playback session.
#[derive(Clone, Debug)]
pub struct StartRequest {
    pub source_url: String,
    pub port: u16,
    pub start_at: Timecode,
    pub keep_temp: bool,
    pub selection: StreamSelection,
    pub mode: PlaybackMode,
    pub runner: FfmpegRunner,
}

#[derive(Debug)]
pub struct PlaybackCoordinator {
    driver: PlaybackDriver,
    source_url: String,
    selection: StreamSelection,
    sessions: SessionManager,
    server: ServerHandle,
    active: ActivePlayback,
    session_state: SessionState,
    stream_url: String,
    last_known_time: Timecode,
    last_player_state: PlayerState,
    telemetry: TelemetryTracker,
}

#[derive(Debug)]
struct ActivePlayback {
    process: PlaybackProcess,
    session: SessionPaths,
}

#[derive(Debug)]
enum PlaybackProcess {
    Live(Box<FfmpegProcess>),
    Inert,
}

impl PlaybackProcess {
    async fn shutdown(&mut self) -> Result<()> {
        match self {
            Self::Live(process) => process.shutdown().await,
            Self::Inert => Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
enum PlaybackDriver {
    Live {
        runner: FfmpegRunner,
        player: QuickTimePlayer,
    },
    Simulated(SimulationScenario),
    #[cfg(test)]
    Test(TestDriver),
}

impl PlaybackDriver {
    fn from_request(request: &StartRequest) -> Self {
        match request.mode.clone() {
            PlaybackMode::Live => Self::Live {
                runner: request.runner.clone(),
                player: QuickTimePlayer::new(),
            },
            PlaybackMode::Simulated(scenario) => Self::Simulated(scenario),
        }
    }

    fn render_spawn_command(
        &self,
        source_url: &str,
        start_at: Timecode,
        session: &SessionPaths,
        selection: &StreamSelection,
    ) -> String {
        match self {
            Self::Live { runner, .. } => {
                runner.render_spawn_command(source_url, start_at, session, selection)
            }
            Self::Simulated(scenario) => {
                scenario.render_spawn_command(source_url, start_at, selection)
            }
            #[cfg(test)]
            Self::Test(driver) => driver.render_spawn_command(source_url, start_at, selection),
        }
    }

    fn render_open_command(&self, stream_url: &str) -> String {
        match self {
            Self::Live { player, .. } => player.render_open_command(stream_url),
            Self::Simulated(scenario) => scenario.render_open_command(stream_url),
            #[cfg(test)]
            Self::Test(driver) => driver.render_open_command(stream_url),
        }
    }

    async fn open_player(&self, stream_url: &str) -> Result<()> {
        match self {
            Self::Live { player, .. } => player.open(stream_url).await,
            Self::Simulated(scenario) => scenario.open_player(stream_url).await,
            #[cfg(test)]
            Self::Test(driver) => driver.open_player(stream_url).await,
        }
    }

    async fn reload_player(&self, stream_url: &str) -> Result<()> {
        match self {
            Self::Live { player, .. } => player.reload(stream_url).await,
            Self::Simulated(scenario) => scenario.reload_player(stream_url).await,
            #[cfg(test)]
            Self::Test(driver) => driver.reload_player(stream_url).await,
        }
    }

    async fn quit_player(&self) -> Result<()> {
        match self {
            Self::Live { player, .. } => player.quit().await,
            Self::Simulated(scenario) => scenario.quit_player().await,
            #[cfg(test)]
            Self::Test(driver) => driver.quit_player().await,
        }
    }

    async fn launch_initial_playback<S>(
        &self,
        sink: &mut S,
        source_url: &str,
        target: Timecode,
        selection: &StreamSelection,
        sessions: &SessionManager,
    ) -> Result<(ActivePlayback, String)>
    where
        S: ProgressSink<LaunchEvent, Error = RuntimeError>,
    {
        let session = sessions.create_session().await?;
        let relay_command = self.render_spawn_command(source_url, target, &session, selection);
        sink.on_event(ProgressEvent::Started {
            step: LaunchStep::Relay,
            details: vec![format!("Command: {relay_command}")],
        })?;

        let process = match self {
            Self::Live { runner, .. } => {
                spin_with_ticks(sink, async {
                    let mut process = runner
                        .spawn(source_url, target, session.clone(), selection)
                        .await?;
                    process.wait_until_ready().await?;
                    Ok::<_, RuntimeError>(PlaybackProcess::Live(Box::new(process)))
                })
                .await?
            }
            Self::Simulated(scenario) => {
                spin_with_ticks(
                    sink,
                    scenario.stage_playback(&session, source_url, target, selection),
                )
                .await?;
                PlaybackProcess::Inert
            }
            #[cfg(test)]
            Self::Test(driver) => {
                spin_with_ticks(
                    sink,
                    driver.stage_playback(&session, source_url, target, selection),
                )
                .await?;
                PlaybackProcess::Inert
            }
        };
        sink.on_event(ProgressEvent::Finished {
            step: LaunchStep::Relay,
        })?;

        Ok((ActivePlayback { process, session }, relay_command))
    }

    async fn launch_staged_playback<S>(
        &self,
        sink: &mut S,
        source_url: &str,
        target: Timecode,
        selection: &StreamSelection,
        session: &SessionPaths,
    ) -> Result<PlaybackProcess>
    where
        S: ProgressSink<JumpEvent, Error = RuntimeError>,
    {
        match self {
            Self::Live { runner, .. } => {
                sink.on_event(ProgressEvent::Started {
                    step: JumpStep::PrepareNextStream,
                    details: Vec::new(),
                })?;
                let mut process = runner
                    .spawn(source_url, target, session.clone(), selection)
                    .await?;
                sink.on_event(ProgressEvent::Finished {
                    step: JumpStep::PrepareNextStream,
                })?;

                sink.on_event(ProgressEvent::Started {
                    step: JumpStep::WaitForStream,
                    details: Vec::new(),
                })?;
                spin_with_ticks(sink, async { process.wait_until_ready().await }).await?;
                sink.on_event(ProgressEvent::Finished {
                    step: JumpStep::WaitForStream,
                })?;
                Ok(PlaybackProcess::Live(Box::new(process)))
            }
            Self::Simulated(scenario) => {
                sink.on_event(ProgressEvent::Started {
                    step: JumpStep::PrepareNextStream,
                    details: Vec::new(),
                })?;
                spin_with_ticks(
                    sink,
                    scenario.stage_playback(session, source_url, target, selection),
                )
                .await?;
                sink.on_event(ProgressEvent::Finished {
                    step: JumpStep::PrepareNextStream,
                })?;
                Ok(PlaybackProcess::Inert)
            }
            #[cfg(test)]
            Self::Test(driver) => {
                sink.on_event(ProgressEvent::Started {
                    step: JumpStep::PrepareNextStream,
                    details: Vec::new(),
                })?;
                spin_with_ticks(
                    sink,
                    driver.stage_playback(session, source_url, target, selection),
                )
                .await?;
                sink.on_event(ProgressEvent::Finished {
                    step: JumpStep::PrepareNextStream,
                })?;
                Ok(PlaybackProcess::Inert)
            }
        }
    }

    fn is_simulated(&self) -> bool {
        matches!(self, Self::Simulated(_))
    }
}

impl PlaybackCoordinator {
    /// Starts playback and owns the resulting relay, server, and player lifecycle.
    pub async fn start<S>(request: StartRequest, sink: &mut S) -> Result<(Self, StartOutcome)>
    where
        S: ProgressSink<LaunchEvent, Error = RuntimeError>,
    {
        let driver = PlaybackDriver::from_request(&request);
        Self::start_with_driver(request, sink, driver).await
    }

    async fn start_with_driver<S>(
        request: StartRequest,
        sink: &mut S,
        driver: PlaybackDriver,
    ) -> Result<(Self, StartOutcome)>
    where
        S: ProgressSink<LaunchEvent, Error = RuntimeError>,
    {
        let sessions = SessionManager::new(request.keep_temp).await?;

        sink.on_event(ProgressEvent::Started {
            step: LaunchStep::LocalStreamServer,
            details: vec![format!("Bind: http://127.0.0.1:{}", request.port)],
        })?;
        let server = spin_with_ticks(sink, ServerHandle::start(request.port)).await?;
        sink.on_event(ProgressEvent::Finished {
            step: LaunchStep::LocalStreamServer,
        })?;

        let (active, relay_command) = driver
            .launch_initial_playback(
                sink,
                &request.source_url,
                request.start_at,
                &request.selection,
                &sessions,
            )
            .await?;

        server
            .state()
            .set_active_dir(active.session.dir.clone())
            .await;
        let stream_url = render_stream_url(server.port(), active.session.id);

        sink.on_event(ProgressEvent::Started {
            step: LaunchStep::Player,
            details: vec![format!(
                "Command: {}",
                driver.render_open_command(&stream_url)
            )],
        })?;
        spin_with_ticks(sink, driver.open_player(&stream_url)).await?;
        sink.on_event(ProgressEvent::Finished {
            step: LaunchStep::Player,
        })?;

        let session_state = SessionState::new(active.session.id, request.start_at, Instant::now());
        let last_player_state = if driver.is_simulated() {
            PlayerState::Playing
        } else {
            PlayerState::Unavailable
        };

        let coordinator = Self {
            driver,
            source_url: request.source_url,
            selection: request.selection,
            sessions,
            server,
            active,
            session_state,
            stream_url: stream_url.clone(),
            last_known_time: request.start_at,
            last_player_state,
            telemetry: TelemetryTracker::default(),
        };
        let outcome = StartOutcome::new(relay_command, stream_url);

        Ok((coordinator, outcome))
    }

    /// Switches playback to a new source timestamp. The previous session remains active on error.
    pub async fn jump_to<S>(&mut self, target: Timecode, sink: &mut S) -> Result<()>
    where
        S: ProgressSink<JumpEvent, Error = RuntimeError>,
    {
        let staging_session = self.sessions.create_session().await?;
        self.session_state.stage_switch(staging_session.id, target);

        let mut staging_process = match self
            .driver
            .launch_staged_playback(
                sink,
                &self.source_url,
                target,
                &self.selection,
                &staging_session,
            )
            .await
        {
            Ok(process) => process,
            Err(error) => {
                self.session_state.abort_stage();
                self.sessions.remove_session(&staging_session).await?;
                return Err(error);
            }
        };

        self.server
            .state()
            .set_active_dir(staging_session.dir.clone())
            .await;
        let staging_stream_url = render_stream_url(self.server.port(), staging_session.id);

        sink.on_event(ProgressEvent::Started {
            step: JumpStep::RefreshPlayer,
            details: Vec::new(),
        })?;
        if let Err(error) =
            spin_with_ticks(sink, self.driver.reload_player(&staging_stream_url)).await
        {
            self.server
                .state()
                .set_active_dir(self.active.session.dir.clone())
                .await;
            self.session_state.abort_stage();
            staging_process.shutdown().await?;
            self.sessions.remove_session(&staging_session).await?;
            return Err(error);
        }
        sink.on_event(ProgressEvent::Finished {
            step: JumpStep::RefreshPlayer,
        })?;

        let previous = std::mem::replace(
            &mut self.active,
            ActivePlayback {
                process: staging_process,
                session: staging_session,
            },
        );
        self.stream_url = staging_stream_url;
        self.session_state.commit_switch(Instant::now())?;
        self.last_known_time = target;

        sink.on_event(ProgressEvent::Started {
            step: JumpStep::CleanupPreviousSession,
            details: Vec::new(),
        })?;
        let mut previous = previous;
        previous.process.shutdown().await?;
        self.sessions.remove_session(&previous.session).await?;
        sink.on_event(ProgressEvent::Finished {
            step: JumpStep::CleanupPreviousSession,
        })?;

        Ok(())
    }

    /// Opens the current stream URL in the active player again.
    pub async fn reopen_player(&mut self) -> Result<()> {
        self.driver.open_player(&self.stream_url).await
    }

    /// Produces an up-to-date view of the active playback state.
    pub async fn snapshot(&mut self, now: Instant) -> PlaybackSnapshot {
        let (current_time, player_state) = match &self.driver {
            PlaybackDriver::Live { player, .. } => match player.playback_status().await {
                Ok(PlaybackStatus::Snapshot(snapshot)) => (
                    self.session_state
                        .committed_offset()
                        .apply_delta(snapshot.current_time().as_seconds() as i64),
                    if snapshot.playing() {
                        PlayerState::Playing
                    } else {
                        PlayerState::Paused
                    },
                ),
                Ok(PlaybackStatus::NoDocument) => (self.last_known_time, PlayerState::WindowClosed),
                Ok(PlaybackStatus::AppClosed) => (self.last_known_time, PlayerState::AppClosed),
                Err(_) => (self.last_known_time, PlayerState::Unavailable),
            },
            PlaybackDriver::Simulated(_) => (
                self.session_state.estimated_position(now),
                PlayerState::Playing,
            ),
            #[cfg(test)]
            PlaybackDriver::Test(driver) => driver.player_snapshot(
                self.last_known_time,
                self.session_state.committed_offset(),
                now,
            ),
        };

        self.last_known_time = current_time;
        self.last_player_state = player_state;
        let telemetry = self
            .telemetry
            .sample(
                self.sessions.root_path(),
                &self.active.session.playlist_path,
                current_time,
                now,
            )
            .await
            .unwrap_or_default();

        PlaybackSnapshot::new(
            self.session_state.active_session_id(),
            self.stream_url.clone(),
            self.session_state.committed_offset(),
            current_time,
            player_state,
            telemetry,
        )
    }

    /// Shuts down the active playback, player, server, and temporary files.
    pub async fn shutdown(&mut self) -> Result<()> {
        self.active.process.shutdown().await?;
        self.driver.quit_player().await?;
        self.server.state().clear().await;
        self.sessions.remove_session(&self.active.session).await?;
        self.server.shutdown().await?;
        self.sessions.cleanup_root().await?;
        Ok(())
    }
}

fn render_stream_url(port: u16, session_id: u64) -> String {
    format!("http://127.0.0.1:{port}/stream.m3u8?session={session_id}")
}

#[derive(Debug, Default)]
struct TelemetryTracker {
    observed_sizes: HashMap<PathBuf, u64>,
    cumulative_bytes_written: u64,
    last_sample: Option<TelemetrySample>,
}

#[derive(Clone, Copy, Debug)]
struct TelemetrySample {
    cumulative_bytes_written: u64,
    observed_at: Instant,
}

impl TelemetryTracker {
    async fn sample(
        &mut self,
        root: &Path,
        playlist_path: &Path,
        current_time: Timecode,
        now: Instant,
    ) -> Result<StreamTelemetry> {
        let storage_bytes = scan_storage(root).await?;
        let buffer_ahead = read_buffer_ahead(playlist_path, current_time).await?;
        let download_bytes_per_second = self.record_download_rate(root, now).await?;
        Ok(StreamTelemetry::new(
            download_bytes_per_second,
            storage_bytes,
            buffer_ahead,
        ))
    }

    async fn record_download_rate(&mut self, root: &Path, now: Instant) -> Result<u64> {
        let mut current_sizes = HashMap::new();
        let mut pending = vec![root.to_path_buf()];

        while let Some(next) = pending.pop() {
            let mut entries = match fs::read_dir(&next).await {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let metadata = entry.metadata().await?;
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    let size = metadata.len();
                    let previous = self.observed_sizes.get(&path).copied().unwrap_or(0);
                    let written_now = if size >= previous {
                        size - previous
                    } else {
                        size
                    };
                    self.cumulative_bytes_written =
                        self.cumulative_bytes_written.saturating_add(written_now);
                    current_sizes.insert(path, size);
                }
            }
        }

        self.observed_sizes = current_sizes;

        let bytes_per_second = if let Some(previous) = self.last_sample {
            let elapsed = now.saturating_duration_since(previous.observed_at);
            if elapsed.is_zero() {
                0
            } else {
                let delta = self
                    .cumulative_bytes_written
                    .saturating_sub(previous.cumulative_bytes_written);
                ((delta as f64) / elapsed.as_secs_f64()).round() as u64
            }
        } else {
            0
        };

        self.last_sample = Some(TelemetrySample {
            cumulative_bytes_written: self.cumulative_bytes_written,
            observed_at: now,
        });

        Ok(bytes_per_second)
    }
}

async fn scan_storage(root: &Path) -> Result<u64> {
    let mut total = 0_u64;
    let mut pending = vec![root.to_path_buf()];

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

async fn read_buffer_ahead(playlist_path: &Path, current_time: Timecode) -> Result<Timecode> {
    let playlist = match fs::read_to_string(playlist_path).await {
        Ok(playlist) => playlist,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Timecode::ZERO),
        Err(error) => return Err(error.into()),
    };

    let buffered_until = buffered_until(&playlist);
    Ok(Timecode::from_seconds(
        buffered_until
            .as_seconds()
            .saturating_sub(current_time.as_seconds()),
    ))
}

fn buffered_until(playlist: &str) -> Timecode {
    let mut next_start = 0_u64;
    let mut next_duration = None::<u64>;
    let mut last_end = 0_u64;

    for line in playlist.lines().map(str::trim) {
        if let Some(duration) = line
            .strip_prefix("#EXTINF:")
            .and_then(|value| value.split(',').next())
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && !value.is_sign_negative())
            .map(|seconds| seconds.ceil() as u64)
        {
            next_duration = Some(duration);
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let duration = next_duration.take().unwrap_or(0);
        let start_seconds = next_start;
        let end_seconds = start_seconds.saturating_add(duration);
        last_end = last_end.max(end_seconds);
        next_start = end_seconds;
    }

    Timecode::from_seconds(last_end)
}

#[cfg(test)]
#[derive(Clone, Debug, Default)]
struct TestDriver {
    state: Arc<std::sync::Mutex<TestDriverState>>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct TestDriverState {
    stage_attempts: usize,
    fail_stage_attempts: Vec<usize>,
    reload_attempts: usize,
    fail_reload_attempts: Vec<usize>,
    opened_urls: Vec<String>,
    reloaded_urls: Vec<String>,
    quit_calls: usize,
}

#[cfg(test)]
impl TestDriver {
    fn new() -> Self {
        Self::default()
    }

    fn fail_stage_on_attempt(self, attempt: usize) -> Self {
        self.state.lock().unwrap().fail_stage_attempts.push(attempt);
        self
    }

    fn fail_reload_on_attempt(self, attempt: usize) -> Self {
        self.state
            .lock()
            .unwrap()
            .fail_reload_attempts
            .push(attempt);
        self
    }

    fn render_spawn_command(
        &self,
        source_url: &str,
        start_at: Timecode,
        selection: &StreamSelection,
    ) -> String {
        format!(
            "test-driver --source {source_url} --at {start_at} --video {}{}",
            selection.video_stream_index(),
            selection
                .audio_stream_index()
                .map(|index| format!(" --audio {index}"))
                .unwrap_or_default()
        )
    }

    fn render_open_command(&self, stream_url: &str) -> String {
        format!("test-driver open {stream_url}")
    }

    async fn stage_playback(
        &self,
        session: &SessionPaths,
        _source_url: &str,
        _target: Timecode,
        _selection: &StreamSelection,
    ) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.stage_attempts += 1;
            if state.fail_stage_attempts.contains(&state.stage_attempts) {
                return Err(RuntimeError::TestDriver(String::from(
                    "test driver refused to stage playback",
                )));
            }
        }

        let playlist = format!(
            "#EXTM3U\n#EXT-X-VERSION:7\n#EXTINF:2.0,\n{}\n",
            session.segment_filename(1)
        );
        tokio::fs::write(&session.playlist_path, playlist).await?;
        tokio::fs::write(session.segment_path(1), b"segment").await?;
        Ok(())
    }

    async fn open_player(&self, stream_url: &str) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .opened_urls
            .push(stream_url.to_string());
        Ok(())
    }

    async fn reload_player(&self, stream_url: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.reload_attempts += 1;
        if state.fail_reload_attempts.contains(&state.reload_attempts) {
            return Err(RuntimeError::TestDriver(String::from(
                "test driver refused to reload the player",
            )));
        }
        state.reloaded_urls.push(stream_url.to_string());
        Ok(())
    }

    async fn quit_player(&self) -> Result<()> {
        self.state.lock().unwrap().quit_calls += 1;
        Ok(())
    }

    fn player_snapshot(
        &self,
        last_known_time: Timecode,
        _committed_offset: Timecode,
        _now: Instant,
    ) -> (Timecode, PlayerState) {
        (last_known_time, PlayerState::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JumpEvent, JumpStep, LaunchEvent, LaunchStep, PlaybackCoordinator, PlaybackMode,
        PlayerState, Result, StartRequest, TestDriver,
    };
    use crate::RuntimeError;
    use crate::ffmpeg::FfmpegRunner;
    use quickbridge_core::{
        ProgressEvent, ProgressSink, SimulationScenario, StreamSelection, Timecode, VideoStream,
    };
    use std::time::{Duration, Instant};

    struct RecordingSink<E> {
        events: Vec<E>,
        ticks: usize,
    }

    impl<E> Default for RecordingSink<E> {
        fn default() -> Self {
            Self {
                events: Vec::new(),
                ticks: 0,
            }
        }
    }

    impl<E> ProgressSink<E> for RecordingSink<E> {
        type Error = RuntimeError;

        fn on_event(&mut self, event: E) -> Result<()> {
            self.events.push(event);
            Ok(())
        }

        fn on_tick(&mut self) -> Result<()> {
            self.ticks += 1;
            Ok(())
        }
    }

    fn selection() -> StreamSelection {
        StreamSelection::new(VideoStream::new(0, "Stream #0:0: Video: h264", true), None)
    }

    fn request() -> StartRequest {
        StartRequest {
            source_url: String::from("https://example.com/video.mkv"),
            port: 0,
            start_at: Timecode::from_seconds(30),
            keep_temp: false,
            selection: selection(),
            mode: PlaybackMode::Simulated(SimulationScenario::HappyPath),
            runner: FfmpegRunner::new(false),
        }
    }

    #[tokio::test]
    async fn startup_in_simulation_mode_returns_initial_snapshot() {
        let mut sink = RecordingSink::<LaunchEvent>::default();
        let (mut coordinator, outcome) = PlaybackCoordinator::start(request(), &mut sink)
            .await
            .unwrap();
        let snapshot = coordinator.snapshot(Instant::now()).await;

        assert!(outcome.relay_command().contains("simulate ffmpeg"));
        assert_eq!(snapshot.session_id(), 1);
        assert_eq!(snapshot.start_time(), Timecode::from_seconds(30));
        assert_eq!(snapshot.player_state(), PlayerState::Playing);
        assert!(sink.events.contains(&ProgressEvent::Finished {
            step: LaunchStep::Player
        }));
        coordinator.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn successful_jump_updates_active_session_and_stream_url() {
        let mut launch_sink = RecordingSink::<LaunchEvent>::default();
        let (mut coordinator, _) = PlaybackCoordinator::start(request(), &mut launch_sink)
            .await
            .unwrap();
        let initial = coordinator.snapshot(Instant::now()).await;
        let mut jump_sink = RecordingSink::<JumpEvent>::default();

        coordinator
            .jump_to(Timecode::from_seconds(90), &mut jump_sink)
            .await
            .unwrap();

        let snapshot = coordinator.snapshot(Instant::now()).await;
        assert_eq!(initial.session_id(), 1);
        assert_eq!(snapshot.session_id(), 2);
        assert_ne!(initial.stream_url(), snapshot.stream_url());
        assert_eq!(snapshot.start_time(), Timecode::from_seconds(90));
        assert!(jump_sink.events.contains(&ProgressEvent::Finished {
            step: JumpStep::CleanupPreviousSession
        }));
        coordinator.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn staged_playback_failure_rolls_back_cleanly() {
        let request = StartRequest {
            mode: PlaybackMode::Live,
            ..request()
        };
        let driver = TestDriver::new().fail_stage_on_attempt(2);
        let mut launch_sink = RecordingSink::<LaunchEvent>::default();
        let (mut coordinator, _) = PlaybackCoordinator::start_with_driver(
            request,
            &mut launch_sink,
            super::PlaybackDriver::Test(driver),
        )
        .await
        .unwrap();
        let before = coordinator.snapshot(Instant::now()).await;
        let mut jump_sink = RecordingSink::<JumpEvent>::default();

        let error = coordinator
            .jump_to(Timecode::from_seconds(120), &mut jump_sink)
            .await
            .unwrap_err();

        let after = coordinator.snapshot(Instant::now()).await;
        assert!(
            error
                .to_string()
                .contains("test driver refused to stage playback")
        );
        assert_eq!(before.session_id(), after.session_id());
        assert_eq!(before.stream_url(), after.stream_url());
        assert!(
            tokio::fs::try_exists(coordinator.sessions.root().join("session-0001"))
                .await
                .unwrap()
        );
        assert!(
            !tokio::fs::try_exists(coordinator.sessions.root().join("session-0002"))
                .await
                .unwrap()
        );
        coordinator.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn reload_failure_restores_previous_session() {
        let request = StartRequest {
            mode: PlaybackMode::Live,
            ..request()
        };
        let driver = TestDriver::new().fail_reload_on_attempt(1);
        let mut launch_sink = RecordingSink::<LaunchEvent>::default();
        let (mut coordinator, _) = PlaybackCoordinator::start_with_driver(
            request,
            &mut launch_sink,
            super::PlaybackDriver::Test(driver),
        )
        .await
        .unwrap();
        let before = coordinator.snapshot(Instant::now()).await;

        let error = coordinator
            .jump_to(
                Timecode::from_seconds(120),
                &mut RecordingSink::<JumpEvent>::default(),
            )
            .await
            .unwrap_err();

        let after = coordinator.snapshot(Instant::now()).await;
        assert!(
            error
                .to_string()
                .contains("test driver refused to reload the player")
        );
        assert_eq!(before.session_id(), after.session_id());
        assert_eq!(before.stream_url(), after.stream_url());
        coordinator.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_uses_elapsed_time_in_simulation_mode() {
        let mut sink = RecordingSink::<LaunchEvent>::default();
        let (mut coordinator, _) = PlaybackCoordinator::start(request(), &mut sink)
            .await
            .unwrap();
        let now = Instant::now() + Duration::from_secs(5);
        let snapshot = coordinator.snapshot(now).await;

        assert_eq!(snapshot.current_time(), Timecode::from_seconds(35));
        assert_eq!(snapshot.player_state(), PlayerState::Playing);
        coordinator.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_clears_server_state_and_session_root() {
        let mut sink = RecordingSink::<LaunchEvent>::default();
        let (mut coordinator, _) = PlaybackCoordinator::start(request(), &mut sink)
            .await
            .unwrap();
        let active_dir = coordinator.active.session.dir.clone();
        let root_dir = coordinator.sessions.root().to_path_buf();

        coordinator.shutdown().await.unwrap();

        assert!(!tokio::fs::try_exists(active_dir).await.unwrap());
        assert!(!tokio::fs::try_exists(root_dir).await.unwrap());
    }
}
