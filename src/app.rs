use crate::{
    cli::Cli,
    diagnostics::render_request,
    ffmpeg::{FfmpegProcess, FfmpegRunner},
    player::{PlaybackStatus, QuickTimePlayer},
    probe::{ProbeRunner, StreamSelection},
    prompt::{Command as PromptCommand, help_text, parse_command, resolve_target},
    server::ServerHandle,
    session::{SessionManager, SessionPaths, SessionState},
    simulate::SimulationScenario,
    source::{SeekSupport, inspect_source},
    terminal::{
        LivePrompt, LiveStepProgress, StageProgress, StepLabels, emphasize,
        enter_interactive_screen, format_playback_time, format_warning, muted,
        require_interactive_terminal, spin_step_while, spin_while,
    },
    timecode::Timecode,
};
use anyhow::{Context, Result, bail};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use reqwest::Url;
use std::{
    pin::Pin,
    time::{Duration, Instant},
};
use tokio::signal::ctrl_c;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, info};

const INSPECT_STEPS: [StepLabels; 4] = [
    StepLabels::new("Source URL", "Checking source URL", "Checked source URL"),
    StepLabels::with_warn(
        "Time jumps",
        "Checking whether time jumps are available",
        "Time jumps are available",
        "Time jumps aren't available",
    ),
    StepLabels::new(
        "Source details",
        "Reading source details",
        "Read source details",
    ),
    StepLabels::new(
        "Video and audio tracks",
        "Finding video and audio tracks",
        "Found video and audio tracks",
    ),
];

const START_SESSION_STEPS: [StepLabels; 3] = [
    StepLabels::new(
        "Local stream server",
        "Starting local stream server",
        "Started local stream server",
    ),
    StepLabels::new(
        "ffmpeg relay",
        "Starting ffmpeg relay",
        "Started ffmpeg relay",
    ),
    StepLabels::new(
        "QuickTime Player",
        "Opening QuickTime Player",
        "Opened QuickTime Player",
    ),
];

enum PlaybackProcess {
    Live(Box<FfmpegProcess>),
    Simulated,
}

impl PlaybackProcess {
    async fn shutdown(&mut self) -> Result<()> {
        match self {
            Self::Live(process) => process.shutdown().await,
            Self::Simulated => Ok(()),
        }
    }
}

struct ActivePlayback {
    process: PlaybackProcess,
    session: SessionPaths,
}

struct PlaybackLaunch {
    active: ActivePlayback,
    relay_command: String,
}

struct PlaybackStart<'a> {
    source_url: &'a str,
    target: Timecode,
    selection: &'a StreamSelection,
    simulation: Option<&'a SimulationScenario>,
}

struct App {
    cli: Cli,
    runner: FfmpegRunner,
    sessions: SessionManager,
    server: ServerHandle,
    player: QuickTimePlayer,
    active: ActivePlayback,
    selection: StreamSelection,
    session_state: SessionState,
    stream_url: String,
    total_runtime: Option<Timecode>,
    live_playhead: Timecode,
    live_player_state: LivePlayerState,
    seek_support: SeekSupport,
    simulate: Option<SimulationScenario>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LivePlayerState {
    Playing,
    Paused,
    WindowClosed,
    AppClosed,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptAction {
    Continue,
    Completed,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Completed,
    Interrupted,
}

pub async fn run(cli: Cli) -> Result<RunOutcome> {
    if cli.simulate.is_none() {
        ensure_supported_platform()?;
    }

    let requested_start_at = cli.at.unwrap_or(Timecode::ZERO);
    let runner = FfmpegRunner::new(cli.verbose);
    let probe = ProbeRunner::new();
    if cli.simulate.is_none() {
        runner.ensure_available().await?;
        probe.ensure_available().await?;
    }
    if cli.script.is_empty() {
        require_interactive_terminal()?;
    }
    let _interactive_screen = if cli.script.is_empty() {
        Some(enter_interactive_screen()?)
    } else {
        None
    };

    info!("starting quickbridge session");

    println!(
        "{}",
        emphasize(&format!("quickbridge {}", env!("CARGO_PKG_VERSION")))
    );
    println!();

    let mut inspect_progress = StageProgress::new("Inspect source", &INSPECT_STEPS, cli.verbose)?;

    inspect_progress.activate(
        0,
        vec![
            format!("Input: {}", cli.url),
            String::from("Rule: a valid source URL is required"),
        ],
    )?;
    let parsed_url = Url::parse(&cli.url)
        .with_context(|| format!("Unable to use the source URL `{}`", cli.url))?;
    inspect_progress.complete(0)?;

    inspect_progress.activate(
        1,
        vec![
            render_request("HEAD", parsed_url.as_str(), None),
            render_request("GET", parsed_url.as_str(), Some("Range: bytes=0-0")),
        ],
    )?;
    let inspection = match &cli.simulate {
        Some(simulation) => {
            spin_while(
                &mut inspect_progress,
                simulation.inspect_source(parsed_url.as_str()),
            )
            .await?
        }
        None => {
            spin_while(&mut inspect_progress, async {
                Ok(inspect_source(&parsed_url).await)
            })
            .await?
        }
    };
    match inspection.seek_support() {
        SeekSupport::Enabled => inspect_progress.complete(1)?,
        SeekSupport::Disabled { warning } => inspect_progress.warn(1, vec![warning.clone()])?,
    }
    inspect_progress.complete(2)?;

    inspect_progress.activate(
        3,
        render_probe_detail_lines(cli.simulate.as_ref(), &probe, &cli.url),
    )?;
    let media_info = match &cli.simulate {
        Some(simulation) => {
            spin_while(&mut inspect_progress, simulation.probe_source(&cli.url)).await?
        }
        None => spin_while(&mut inspect_progress, probe.probe(&cli.url)).await?,
    };
    inspect_progress.complete(3)?;
    inspect_progress.finish()?;
    println!();

    let selection = media_info.select_streams().await?;
    println!("{}", emphasize("Selected media"));
    println!("{}", inspection.metadata().filename());
    if let Some(summary) = selection.selected_audio_summary() {
        println!("{summary}");
    }
    println!("{}", inspection.metadata().display_size());
    println!();

    let sessions = SessionManager::new(cli.keep_temp).await?;
    let mut start_progress =
        StageProgress::new("Start session", &START_SESSION_STEPS, cli.verbose)?;

    start_progress.activate(0, vec![format!("Bind: http://127.0.0.1:{}", cli.port)])?;
    let server = spin_while(&mut start_progress, ServerHandle::start(cli.port)).await?;
    start_progress.complete(0)?;

    let actual_start_at = if inspection.seeking_enabled() {
        requested_start_at
    } else {
        Timecode::ZERO
    };

    let player = QuickTimePlayer::new();

    let playback = start_playback_with_progress(
        &mut start_progress,
        1,
        &runner,
        &sessions,
        PlaybackStart {
            source_url: &cli.url,
            target: actual_start_at,
            selection: &selection,
            simulation: cli.simulate.as_ref(),
        },
    )
    .await?;
    server
        .state()
        .set_active_dir(playback.active.session.dir.clone())
        .await;
    let active_session_id = playback.active.session.id;
    let stream_url = cli.stream_url(server.port(), active_session_id);
    start_progress.complete(1)?;

    start_progress.activate(
        2,
        vec![format!(
            "Command: {}",
            render_open_command(cli.simulate.as_ref(), &player, &stream_url)
        )],
    )?;
    match &cli.simulate {
        Some(simulation) => {
            spin_while(&mut start_progress, simulation.open_player(&stream_url)).await?
        }
        None => {
            spin_while(&mut start_progress, async {
                player
                    .open(&stream_url)
                    .await
                    .context("Unable to open QuickTime Player with the local stream URL")
            })
            .await?
        }
    }
    start_progress.complete(2)?;
    start_progress.finish()?;
    println!("{} {}", emphasize("[FFMPEG]"), muted(&playback.relay_command));
    println!("{} {}", emphasize("[SERVER]"), muted(&stream_url));

    if requested_start_at != Timecode::ZERO && actual_start_at == Timecode::ZERO {
        println!();
        println!(
            "{}",
            format_warning(
                "Started from the beginning because this source doesn't support jumping to a different time.",
            )
        );
    }

    if let Some(warning) = inspection.seek_warning() {
        println!();
        println!("{}", format_warning(warning));
    }

    if let Some(notice) = selection.audio_notice() {
        println!();
        println!("{notice}");
    }

    println!();
    println!(
        "{} {} {}",
        muted("Type"),
        emphasize("h + enter"),
        muted("to see available commands.")
    );

    let session_state = SessionState::new(active_session_id, actual_start_at, Instant::now());
    let simulate = cli.simulate.clone();

    let mut app = App {
        cli,
        runner,
        sessions,
        server,
        player,
        active: playback.active,
        selection,
        session_state,
        stream_url,
        total_runtime: media_info.duration(),
        live_playhead: actual_start_at,
        live_player_state: if simulate.is_some() {
            LivePlayerState::Playing
        } else {
            LivePlayerState::Unavailable
        },
        seek_support: inspection.seek_support().clone(),
        simulate,
    };

    let prompt_result = if app.cli.script.is_empty() {
        app.prompt_loop().await
    } else {
        app.run_scripted_commands().await
    };

    let cleanup_result = app.cleanup().await;
    match (prompt_result, cleanup_result) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(outcome), Ok(())) => Ok(outcome),
    }
}

async fn start_playback_with_progress(
    progress: &mut StageProgress,
    step_index: usize,
    runner: &FfmpegRunner,
    sessions: &SessionManager,
    start: PlaybackStart<'_>,
) -> Result<PlaybackLaunch> {
    let session = sessions.create_session().await?;
    let command = match start.simulation {
        Some(simulation) => {
            simulation.render_spawn_command(start.source_url, start.target, start.selection)
        }
        None => {
            runner.render_spawn_command(start.source_url, start.target, &session, start.selection)
        }
    };
    progress.activate(step_index, vec![format!("Command: {command}")])?;

    debug!(session_id = session.id, target = %start.target, "starting playback session");
    let process = match start.simulation {
        Some(simulation) => {
            spin_while(
                progress,
                simulation.stage_playback(
                    &session,
                    start.source_url,
                    start.target,
                    start.selection,
                ),
            )
            .await
            .with_context(|| format!("Playback session {} did not become ready", session.id))?;
            PlaybackProcess::Simulated
        }
        None => {
            let mut process = runner
                .spawn(
                    start.source_url,
                    start.target,
                    session.clone(),
                    start.selection,
                )
                .await?;
            spin_while(progress, async {
                process.wait_until_ready().await.with_context(|| {
                    format!("Playback session {} did not become ready", session.id)
                })
            })
            .await?;
            PlaybackProcess::Live(Box::new(process))
        }
    };
    Ok(PlaybackLaunch {
        active: ActivePlayback { process, session },
        relay_command: command,
    })
}

impl App {
    async fn run_scripted_commands(&mut self) -> Result<RunOutcome> {
        for line in self.cli.script.clone() {
            println!();
            println!("Scripted command");
            println!("  {line}");

            let command = parse_command(&line)
                .with_context(|| format!("Unable to parse the scripted command `{line}`"))?
                .with_context(|| format!("Scripted command `{line}` is empty"))?;

            match self.execute_command(command, None).await? {
                PromptAction::Continue => {}
                PromptAction::Completed => return Ok(RunOutcome::Completed),
                PromptAction::Interrupted => return Ok(RunOutcome::Interrupted),
            }
        }

        Ok(RunOutcome::Completed)
    }

    async fn prompt_loop(&mut self) -> Result<RunOutcome> {
        let mut terminal = LivePrompt::enter()?;
        let mut events = EventStream::new();
        let mut ticker = interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        ticker.tick().await;
        let mut ctrl_c_signal: Pin<Box<_>> = Box::pin(ctrl_c());
        let mut input_buffer = String::new();

        self.refresh_live_playhead(Instant::now()).await;
        self.redraw_prompt(&mut terminal, &input_buffer)?;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.refresh_live_playhead(Instant::now()).await;
                    self.redraw_prompt(&mut terminal, &input_buffer)?;
                }
                signal_result = &mut ctrl_c_signal => {
                    signal_result.context("unable to listen for Ctrl+C")?;
                    terminal.clear_live_area()?;
                    return Ok(RunOutcome::Interrupted);
                }
                maybe_event = events.next() => {
                    let Some(event) = maybe_event.transpose().context("unable to read terminal input")? else {
                        terminal.clear_live_area()?;
                        return Ok(RunOutcome::Completed);
                    };

                    match event {
                        Event::Key(key_event) if should_handle_key(key_event) => {
                            match self
                                .handle_key_event(&mut terminal, &mut input_buffer, key_event)
                                .await?
                            {
                                PromptAction::Continue => {}
                                PromptAction::Completed => return Ok(RunOutcome::Completed),
                                PromptAction::Interrupted => return Ok(RunOutcome::Interrupted),
                            }
                        }
                        Event::Paste(text) => {
                            input_buffer.push_str(&text);
                            self.redraw_prompt(&mut terminal, &input_buffer)?;
                        }
                        Event::Resize(_, _) => {
                            self.redraw_prompt(&mut terminal, &input_buffer)?;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    async fn switch_playback_to(&mut self, target: Timecode) -> Result<()> {
        let mut progress = LiveStepProgress::new();
        progress.show_active("Getting ready to jump")?;

        let staging_session = self.sessions.create_session().await?;
        debug!(
            active_session_id = self.session_state.active_session_id(),
            staging_session_id = staging_session.id,
            target = %target,
            "switching playback session"
        );
        self.session_state.stage_switch(staging_session.id, target);

        let mut staging_process = match self.simulate.as_ref() {
            Some(simulation) => {
                if let Err(error) = spin_step_while(
                    &mut progress,
                    "Preparing the next stream",
                    simulation.stage_playback(
                        &staging_session,
                        &self.cli.url,
                        target,
                        &self.selection,
                    ),
                )
                .await
                {
                    self.session_state.abort_stage();
                    self.sessions.remove_session(&staging_session).await?;
                    progress.clear()?;
                    return Err(error);
                }
                PlaybackProcess::Simulated
            }
            None => match self
                .runner
                .spawn(
                    &self.cli.url,
                    target,
                    staging_session.clone(),
                    &self.selection,
                )
                .await
            {
                Ok(process) => PlaybackProcess::Live(Box::new(process)),
                Err(error) => {
                    self.session_state.abort_stage();
                    self.sessions.remove_session(&staging_session).await?;
                    progress.clear()?;
                    return Err(error);
                }
            },
        };

        progress.show_active("Waiting for the stream")?;
        if let PlaybackProcess::Live(process) = &mut staging_process
            && let Err(error) = spin_step_while(
                &mut progress,
                "Waiting for the stream",
                process.wait_until_ready(),
            )
            .await
        {
            self.session_state.abort_stage();
            staging_process.shutdown().await?;
            self.sessions.remove_session(&staging_session).await?;
            progress.clear()?;
            return Err(error);
        }

        self.server
            .state()
            .set_active_dir(staging_session.dir.clone())
            .await;
        let staging_stream_url = self.cli.stream_url(self.server.port(), staging_session.id);

        let reload_result = match self.simulate.as_ref() {
            Some(simulation) => {
                spin_step_while(
                    &mut progress,
                    "Refreshing QuickTime Player",
                    simulation.reload_player(&staging_stream_url),
                )
                .await
            }
            None => {
                spin_step_while(
                    &mut progress,
                    "Refreshing QuickTime Player",
                    self.player.reload(&staging_stream_url),
                )
                .await
            }
        };
        if let Err(error) = reload_result {
            self.server
                .state()
                .set_active_dir(self.active.session.dir.clone())
                .await;
            self.session_state.abort_stage();
            staging_process.shutdown().await?;
            self.sessions.remove_session(&staging_session).await?;
            progress.clear()?;
            return Err(error);
        }

        let previous = std::mem::replace(
            &mut self.active,
            ActivePlayback {
                process: staging_process,
                session: staging_session,
            },
        );
        self.stream_url = staging_stream_url;

        progress.show_active("Cleaning up the last session")?;
        self.session_state.commit_switch(Instant::now())?;
        let mut previous = previous;
        previous.process.shutdown().await?;
        self.sessions.remove_session(&previous.session).await?;
        progress.clear()?;

        Ok(())
    }

    fn live_status_line(&self) -> String {
        let playback_time = format_playback_time(self.live_playhead, self.total_runtime);
        match self.live_player_state {
            LivePlayerState::Playing => playback_time,
            LivePlayerState::Paused => format!("{playback_time} (Paused)"),
            LivePlayerState::WindowClosed
            | LivePlayerState::AppClosed
            | LivePlayerState::Unavailable => playback_time,
        }
    }

    fn live_warning_line(&self) -> Option<String> {
        match self.live_player_state {
            LivePlayerState::WindowClosed => Some(format_warning(
                "QuickTime Player window is closed. Type `reopen` to open the stream again.",
            )),
            LivePlayerState::AppClosed => Some(format_warning(
                "QuickTime Player is closed. Type `reopen` to open the stream again.",
            )),
            LivePlayerState::Unavailable => Some(format_warning(
                "QuickTime Player status isn't available right now.",
            )),
            LivePlayerState::Playing | LivePlayerState::Paused => None,
        }
    }

    fn live_prompt_state_key(&self) -> &'static str {
        match self.live_player_state {
            LivePlayerState::Playing => "playing",
            LivePlayerState::Paused => "paused",
            LivePlayerState::WindowClosed => "window-closed",
            LivePlayerState::AppClosed => "app-closed",
            LivePlayerState::Unavailable => "unavailable",
        }
    }

    fn redraw_prompt(&self, terminal: &mut LivePrompt, input_buffer: &str) -> Result<()> {
        let warning = self.live_warning_line();
        terminal.redraw(
            self.live_prompt_state_key(),
            warning.as_deref(),
            &self.live_status_line(),
            input_buffer,
        )
    }

    async fn refresh_live_playhead(&mut self, now: Instant) {
        let (playhead, state) = self.current_source_position(now).await;
        self.live_playhead = playhead;
        self.live_player_state = state;
    }

    async fn current_source_position(&self, now: Instant) -> (Timecode, LivePlayerState) {
        if self.simulate.is_some() {
            return (
                self.session_state.estimated_position(now),
                LivePlayerState::Playing,
            );
        }

        match self.player.playback_status().await {
            Ok(PlaybackStatus::Snapshot(snapshot)) => (
                self.session_state
                    .committed_offset()
                    .apply_delta(snapshot.current_time().as_seconds() as i64),
                if snapshot.playing() {
                    LivePlayerState::Playing
                } else {
                    LivePlayerState::Paused
                },
            ),
            Ok(PlaybackStatus::NoDocument) => (self.live_playhead, LivePlayerState::WindowClosed),
            Ok(PlaybackStatus::AppClosed) => (self.live_playhead, LivePlayerState::AppClosed),
            Err(_) => (self.live_playhead, LivePlayerState::Unavailable),
        }
    }

    async fn status_text(&self) -> String {
        let (current_playhead, state) = self.current_source_position(Instant::now()).await;
        let mut lines = vec![
            format!(
                "Mode               | {}",
                self.simulate
                    .as_ref()
                    .map(|simulation| format!("Simulation ({})", simulation.label()))
                    .unwrap_or_else(|| String::from("Live"))
            ),
            format!("Source             | {}", self.cli.url),
            format!("Stream             | {}", self.stream_url),
            format!(
                "Session ID         | {}",
                self.session_state.active_session_id()
            ),
            format!(
                "Start time         | {}",
                self.session_state.committed_offset()
            ),
            format!("Current time       | {}", current_playhead),
            format!(
                "QuickTime Player   | {}",
                match state {
                    LivePlayerState::Playing => "Playing",
                    LivePlayerState::Paused => "Paused",
                    LivePlayerState::WindowClosed => "Window closed",
                    LivePlayerState::AppClosed => "Closed",
                    LivePlayerState::Unavailable => "Status unavailable",
                }
            ),
            format!(
                "Time jumps         | {}",
                match self.seek_support {
                    SeekSupport::Enabled => "Available",
                    SeekSupport::Disabled { .. } => "Unavailable",
                }
            ),
            String::from("Tracks"),
        ];
        lines.extend(
            self.selection
                .render_output_file()
                .lines()
                .map(|line| format!("  {line}")),
        );
        lines.push(
            self.selection
                .audio_notice()
                .unwrap_or_else(|| String::from("Audio handling: play the original track")),
        );
        if let SeekSupport::Disabled { ref warning } = self.seek_support {
            lines.push(String::new());
            lines.push(format!("Note: {warning}"));
        }
        lines.join("\n")
    }

    async fn handle_key_event(
        &mut self,
        terminal: &mut LivePrompt,
        input_buffer: &mut String,
        key_event: KeyEvent,
    ) -> Result<PromptAction> {
        match key_event.code {
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                terminal.clear_live_area()?;
                Ok(PromptAction::Interrupted)
            }
            KeyCode::Backspace => {
                input_buffer.pop();
                self.redraw_prompt(terminal, input_buffer)?;
                Ok(PromptAction::Continue)
            }
            KeyCode::Enter => self.submit_input(terminal, input_buffer).await,
            KeyCode::Char(ch)
                if !key_event.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                input_buffer.push(ch);
                self.redraw_prompt(terminal, input_buffer)?;
                Ok(PromptAction::Continue)
            }
            _ => Ok(PromptAction::Continue),
        }
    }

    async fn submit_input(
        &mut self,
        terminal: &mut LivePrompt,
        input_buffer: &mut String,
    ) -> Result<PromptAction> {
        let line = std::mem::take(input_buffer);
        let command = match parse_command(&line) {
            Ok(Some(command)) => command,
            Ok(None) => {
                self.redraw_prompt(terminal, input_buffer)?;
                return Ok(PromptAction::Continue);
            }
            Err(error) => {
                terminal.print_transient(&format!("Couldn't understand that command: {error}"))?;
                self.redraw_prompt(terminal, input_buffer)?;
                return Ok(PromptAction::Continue);
            }
        };

        if matches!(
            command,
            PromptCommand::JumpAbsolute(_) | PromptCommand::JumpRelative(_)
        ) {
            terminal.clear_live_area()?;
        }

        match self.execute_command(command, Some(terminal)).await? {
            PromptAction::Continue => {}
            PromptAction::Completed => {
                terminal.clear_live_area()?;
                return Ok(PromptAction::Completed);
            }
            PromptAction::Interrupted => {
                terminal.clear_live_area()?;
                return Ok(PromptAction::Interrupted);
            }
        }

        self.refresh_live_playhead(Instant::now()).await;
        self.redraw_prompt(terminal, input_buffer)?;
        Ok(PromptAction::Continue)
    }

    async fn execute_command(
        &mut self,
        command: PromptCommand,
        terminal: Option<&mut LivePrompt>,
    ) -> Result<PromptAction> {
        match command {
            PromptCommand::Help => {
                let help = help_text();
                if let Some(terminal) = terminal {
                    terminal.print_transient(&help)?;
                } else {
                    println!("{help}");
                }
                Ok(PromptAction::Continue)
            }
            PromptCommand::Reopen => {
                if let Some(simulation) = self.simulate.as_ref() {
                    simulation.open_player(&self.stream_url).await?;
                } else {
                    self.player.open(&self.stream_url).await?;
                }
                self.refresh_live_playhead(Instant::now()).await;
                Ok(PromptAction::Continue)
            }
            PromptCommand::Status => {
                if let Some(terminal) = terminal {
                    terminal.print_transient(&self.status_text().await)?;
                } else {
                    println!("{}", self.status_text().await);
                }
                Ok(PromptAction::Continue)
            }
            PromptCommand::Quit => Ok(PromptAction::Completed),
            PromptCommand::JumpAbsolute(_) | PromptCommand::JumpRelative(_) => {
                if let SeekSupport::Disabled { warning } = &self.seek_support {
                    if let Some(terminal) = terminal {
                        terminal.print_transient(&format!(
                            "Jumping to a different time isn't available: {warning}"
                        ))?;
                    } else {
                        println!("Jumping to a different time isn't available: {warning}");
                    }
                    return Ok(PromptAction::Continue);
                }

                let (current_playhead, _) = self.current_source_position(Instant::now()).await;
                let target = resolve_target(current_playhead, &command)?;
                if let Err(error) = self.switch_playback_to(target).await {
                    if let Some(terminal) = terminal {
                        terminal
                            .print_transient(&format!("Couldn't jump to that time: {error:#}"))?;
                    } else {
                        println!("Couldn't jump to that time: {error:#}");
                    }
                }
                Ok(PromptAction::Continue)
            }
        }
    }

    async fn cleanup(&mut self) -> Result<()> {
        debug!("cleaning up quickbridge session");
        self.active.process.shutdown().await?;
        match self.simulate.as_ref() {
            Some(simulation) => simulation.quit_player().await?,
            None => self.player.quit().await?,
        }
        self.server.state().clear().await;
        self.sessions.remove_session(&self.active.session).await?;
        self.server.shutdown().await?;
        self.sessions.cleanup_root().await?;
        Ok(())
    }
}

fn should_handle_key(key_event: KeyEvent) -> bool {
    matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn ensure_supported_platform() -> Result<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        bail!("quickbridge supports macOS only. QuickTime Player is required")
    }
}

fn render_probe_detail_lines(
    simulation: Option<&SimulationScenario>,
    probe: &ProbeRunner,
    source_url: &str,
) -> Vec<String> {
    match simulation {
        Some(simulation) => simulation
            .render_probe_commands(source_url)
            .into_iter()
            .map(|command| format!("Command: {command}"))
            .collect(),
        None => probe
            .render_probe_commands(source_url)
            .into_iter()
            .map(|command| format!("Command: {command}"))
            .collect(),
    }
}

fn render_open_command(
    simulation: Option<&SimulationScenario>,
    player: &QuickTimePlayer,
    stream_url: &str,
) -> String {
    match simulation {
        Some(simulation) => simulation.render_open_command(stream_url),
        None => player.render_open_command(stream_url),
    }
}
