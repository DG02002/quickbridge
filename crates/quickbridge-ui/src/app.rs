use super::{
    event::{AppEvent, AppEventStream},
    runtime::{RuntimeOptions, TuiRuntime},
    terminal_detection::TerminalInfo,
};
use crate::{
    InteractiveOptions, Result, UiError,
    text::{format_bytes, format_bytes_per_second, format_warning},
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use quickbridge_core::{
    Command as PromptCommand, JumpEvent, JumpStep, LaunchEvent, LaunchStep, MediaInfo,
    PlaybackMode, PlaybackSnapshot, PlayerState, PrepareEvent, PrepareStep, ProgressEvent,
    ProgressSink, RunOutcome, SeekSupport, SimulationScenario, SourceInspection, StreamSelection,
    Timecode, TrackSelectionRequest, parse_command, resolve_target,
};
use quickbridge_runtime::{
    FfmpegRunner, PlaybackCoordinator, PrepareRequest, PreparedSource, ProbeRunner, RuntimeError,
    StartRequest, prepare_source,
};
use std::time::{Duration, Instant};

const ACTIVE_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_ACTIVITY_ENTRIES: usize = 200;

pub async fn run_interactive(
    cli: InteractiveOptions,
    runner: FfmpegRunner,
    probe: ProbeRunner,
) -> Result<RunOutcome> {
    let runtime_options = runtime_options(&cli, &TerminalInfo::detect());
    let mut runtime = TuiRuntime::enter(runtime_options)?;
    let mut state = AppState::new(cli.url.clone());
    runtime.draw(&state)?;
    let mut source_url = match cli.url.clone() {
        Some(url) => url,
        None => match prompt_for_source_url(&mut runtime, &mut state).await? {
            SourcePromptResult::Ready(url) => url,
            SourcePromptResult::Completed => return Ok(RunOutcome::Completed),
            SourcePromptResult::Interrupted => return Ok(RunOutcome::Interrupted),
            SourcePromptResult::Continue => unreachable!("source prompt loops until completion"),
        },
    };
    let prepared = loop {
        state.begin_prepare(source_url.clone());
        runtime.draw(&state)?;
        let attempt = {
            let mut sink = PrepareProgressRenderer {
                runtime: &mut runtime,
                state: &mut state,
                verbose: cli.verbose,
            };
            prepare_source(
                PrepareRequest {
                    source_url: source_url.clone(),
                    simulation: cli.simulation.clone(),
                    probe: probe.clone(),
                },
                &mut sink,
            )
            .await
        };
        match attempt {
            Ok(prepared) => match prepared.media_info().selection_request() {
                Ok(_) => break prepared,
                Err(quickbridge_core::TrackSelectionError::NoVideoTrack) => {
                    state.show_source_error(SourceErrorState::no_video(source_url.clone()));
                }
                Err(error) => return Err(UiError::from(error)),
            },
            Err(RuntimeError::Interrupted) => return Ok(RunOutcome::Interrupted),
            Err(error) if is_recoverable_prepare_error(&error) => {
                state.show_source_error(SourceErrorState::from_runtime(source_url.clone(), &error));
            }
            Err(error) => return Err(UiError::from(error)),
        }

        match recover_source(&mut runtime, &mut state).await? {
            SourceRecoveryAction::Retry => {}
            SourceRecoveryAction::Edit(edited) => source_url = edited,
            SourceRecoveryAction::Quit => return Ok(RunOutcome::Completed),
            SourceRecoveryAction::Interrupted => return Ok(RunOutcome::Interrupted),
        }
    };

    let selection = match choose_tracks(&mut runtime, &mut state, &prepared).await {
        Ok(selection) => selection,
        Err(UiError::Interrupted) => return Ok(RunOutcome::Interrupted),
        Err(error) => return Err(error),
    };
    let requested_start_at = cli.at.unwrap_or(Timecode::ZERO);
    let actual_start_at = if prepared.inspection().seeking_enabled() {
        requested_start_at
    } else {
        Timecode::ZERO
    };
    let playback_mode = playback_mode_from_simulation(cli.simulation.clone());

    state.show_startup(prepared.inspection(), &selection);
    runtime.draw(&state)?;
    let (mut playback, _start_outcome) = {
        let mut sink = LaunchProgressRenderer {
            runtime: &mut runtime,
            state: &mut state,
            verbose: cli.verbose,
        };
        match PlaybackCoordinator::start(
            StartRequest {
                source_url: source_url.clone(),
                port: cli.port,
                start_at: actual_start_at,
                keep_temp: cli.keep_temp,
                selection: selection.clone(),
                mode: playback_mode.clone(),
                runner,
            },
            &mut sink,
        )
        .await
        {
            Ok(started) => started,
            Err(RuntimeError::Interrupted) => return Ok(RunOutcome::Interrupted),
            Err(error) => return Err(UiError::from(error)),
        }
    };

    let live_snapshot = playback.snapshot(Instant::now()).await;
    state.show_running(RunningState::new(
        source_url,
        playback_mode,
        selection,
        prepared.inspection().clone(),
        prepared.media_info().clone(),
        live_snapshot,
        StartupContext {
            requested_start_at,
            actual_start_at,
        },
    ));
    runtime.draw(&state)?;

    let outcome = run_live_loop(&mut runtime, &mut state, &mut playback).await;
    let cleanup = playback.shutdown().await;
    match (outcome, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(UiError::from(error)),
        (Ok(outcome), Ok(())) => Ok(outcome),
    }
}

fn runtime_options(cli: &InteractiveOptions, terminal: &TerminalInfo) -> RuntimeOptions {
    RuntimeOptions {
        use_alt_screen: terminal.should_use_alt_screen(cli.no_alt_screen),
    }
}

async fn prompt_for_source_url(
    runtime: &mut TuiRuntime,
    state: &mut AppState,
) -> Result<SourcePromptResult> {
    let mut events = AppEventStream::new(Duration::from_millis(100));
    loop {
        match events.next().await? {
            AppEvent::CtrlC => return Ok(SourcePromptResult::Interrupted),
            AppEvent::Resize => runtime.draw(state)?,
            AppEvent::Tick if state.tick() => runtime.draw(state)?,
            AppEvent::Tick => {}
            AppEvent::Paste(text) => {
                state.append_input(&text);
                runtime.draw(state)?;
            }
            AppEvent::Key(key_event) if should_handle_key(key_event) => {
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(SourcePromptResult::Interrupted);
                    }
                    KeyCode::Backspace => {
                        state.pop_input();
                    }
                    KeyCode::Left => state.move_input_cursor(false),
                    KeyCode::Right => state.move_input_cursor(true),
                    KeyCode::Home => state.move_input_cursor_to_edge(false),
                    KeyCode::End => state.move_input_cursor_to_edge(true),
                    KeyCode::F(1) => state.toggle_launcher_help(),
                    KeyCode::Esc => state.close_launcher_help(),
                    KeyCode::Enter => match submit_source_input(state) {
                        SourcePromptResult::Ready(url) => {
                            return Ok(SourcePromptResult::Ready(url));
                        }
                        SourcePromptResult::Completed => {
                            return Ok(SourcePromptResult::Completed);
                        }
                        SourcePromptResult::Interrupted => {
                            return Ok(SourcePromptResult::Interrupted);
                        }
                        SourcePromptResult::Continue => {}
                    },
                    KeyCode::Char(ch)
                        if !key_event.modifiers.intersects(
                            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                        ) =>
                    {
                        state.push_input(ch);
                    }
                    _ => {}
                }
                runtime.draw(state)?;
            }
            AppEvent::Key(_) => {}
            AppEvent::ScrollUp | AppEvent::ScrollDown => {}
        }
    }
}

async fn recover_source(
    runtime: &mut TuiRuntime,
    state: &mut AppState,
) -> Result<SourceRecoveryAction> {
    runtime.draw(state)?;
    let mut events = AppEventStream::new(Duration::from_millis(100));
    loop {
        match events.next().await? {
            AppEvent::CtrlC => return Ok(SourceRecoveryAction::Interrupted),
            AppEvent::Resize => runtime.draw(state)?,
            AppEvent::Tick => {}
            AppEvent::Paste(text) => state.append_input(&text),
            AppEvent::Key(key) if should_handle_key(key) => {
                let editing = matches!(&state.screen, Screen::SourceError(error) if error.editing);
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(SourceRecoveryAction::Interrupted);
                    }
                    KeyCode::Char('r') if !editing => return Ok(SourceRecoveryAction::Retry),
                    KeyCode::Char('q') if !editing => return Ok(SourceRecoveryAction::Quit),
                    KeyCode::Char('e') if !editing => state.edit_source_error_url(),
                    KeyCode::Backspace if editing => state.pop_input(),
                    KeyCode::Enter if editing => {
                        return Ok(SourceRecoveryAction::Edit(state.take_input()));
                    }
                    KeyCode::Esc if editing => state.cancel_source_error_edit(),
                    KeyCode::Char(ch)
                        if editing
                            && !key.modifiers.intersects(
                                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                            ) =>
                    {
                        state.push_input(ch);
                    }
                    _ => {}
                }
                runtime.draw(state)?;
            }
            AppEvent::Key(_) => {}
            AppEvent::ScrollUp | AppEvent::ScrollDown => {}
        }
    }
}

fn is_recoverable_prepare_error(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::InvalidSourceUrl { .. }
            | RuntimeError::FfprobeUnavailable { .. }
            | RuntimeError::ExecuteBinary { .. }
            | RuntimeError::FfprobeFailed { .. }
            | RuntimeError::MediaInfoParse(_)
    )
}

async fn choose_tracks(
    runtime: &mut TuiRuntime,
    state: &mut AppState,
    prepared: &PreparedSource,
) -> Result<StreamSelection> {
    let request = prepared.media_info().selection_request()?;
    if request.videos().len() == 1 && request.audios().len() <= 1 {
        return Ok(request
            .build_selection(request.default_video_index(), request.default_audio_index())?);
    }

    state.show_track_selection(prepared.inspection(), request);
    runtime.draw(state)?;

    let mut events = AppEventStream::new(Duration::from_millis(100));
    loop {
        match events.next().await? {
            AppEvent::CtrlC => return Err(UiError::Interrupted),
            AppEvent::Resize => runtime.draw(state)?,
            AppEvent::Tick => {}
            AppEvent::Paste(text) => {
                if text.contains('\n')
                    && let Some(selection) = state.advance_or_confirm_track_selection()?
                {
                    return Ok(selection);
                }
            }
            AppEvent::Key(key_event) if should_handle_key(key_event) => match key_event.code {
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err(UiError::Interrupted);
                }
                KeyCode::Up => state.move_track_selection(-1),
                KeyCode::Down => state.move_track_selection(1),
                KeyCode::Left | KeyCode::BackTab => state.switch_track_focus(false),
                KeyCode::Right | KeyCode::Tab => state.switch_track_focus(true),
                KeyCode::Enter => {
                    if let Some(selection) = state.advance_or_confirm_track_selection()? {
                        return Ok(selection);
                    }
                }
                _ => {}
            },
            AppEvent::Key(_) => {}
            AppEvent::ScrollUp | AppEvent::ScrollDown => {}
        }
        runtime.draw(state)?;
    }
}

async fn run_live_loop(
    runtime: &mut TuiRuntime,
    state: &mut AppState,
    playback: &mut PlaybackCoordinator,
) -> Result<RunOutcome> {
    let mut events = AppEventStream::new(Duration::from_millis(100));
    let mut last_snapshot_refresh = Instant::now();

    loop {
        match events.next().await? {
            AppEvent::CtrlC => return Ok(RunOutcome::Interrupted),
            AppEvent::Resize => runtime.draw(state)?,
            AppEvent::Tick => {
                let mut needs_draw = state.tick();
                if last_snapshot_refresh.elapsed() >= Duration::from_secs(1) {
                    let snapshot = playback.snapshot(Instant::now()).await;
                    state.update_snapshot(snapshot);
                    last_snapshot_refresh = Instant::now();
                    needs_draw = true;
                }
                if needs_draw {
                    runtime.draw(state)?;
                }
            }
            AppEvent::Paste(text) => {
                state.append_input(&text);
                runtime.draw(state)?;
            }
            AppEvent::Key(key_event) if should_handle_key(key_event) => {
                if matches!(key_event.code, KeyCode::PageUp | KeyCode::PageDown) {
                    state.scroll_activity(matches!(key_event.code, KeyCode::PageUp));
                    runtime.draw(state)?;
                    continue;
                }
                match handle_running_key_event(runtime, state, playback, key_event).await? {
                    RunningAction::Continue => {}
                    RunningAction::Completed => return Ok(RunOutcome::Completed),
                    RunningAction::Interrupted => return Ok(RunOutcome::Interrupted),
                }
            }
            AppEvent::Key(_) => {}
            AppEvent::ScrollUp => {
                state.scroll_activity(true);
                runtime.draw(state)?;
            }
            AppEvent::ScrollDown => {
                state.scroll_activity(false);
                runtime.draw(state)?;
            }
        }
    }
}

async fn handle_running_key_event(
    runtime: &mut TuiRuntime,
    state: &mut AppState,
    playback: &mut PlaybackCoordinator,
    key_event: KeyEvent,
) -> Result<RunningAction> {
    match key_event.code {
        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            Ok(RunningAction::Interrupted)
        }
        KeyCode::Esc => {
            state.close_running_overlay();
            runtime.draw(state)?;
            Ok(RunningAction::Continue)
        }
        KeyCode::Backspace => {
            state.pop_input();
            runtime.draw(state)?;
            Ok(RunningAction::Continue)
        }
        KeyCode::Left => {
            state.move_input_cursor(false);
            runtime.draw(state)?;
            Ok(RunningAction::Continue)
        }
        KeyCode::Right => {
            state.move_input_cursor(true);
            runtime.draw(state)?;
            Ok(RunningAction::Continue)
        }
        KeyCode::Home => {
            state.move_input_cursor_to_edge(false);
            runtime.draw(state)?;
            Ok(RunningAction::Continue)
        }
        KeyCode::End => {
            state.move_input_cursor_to_edge(true);
            runtime.draw(state)?;
            Ok(RunningAction::Continue)
        }
        KeyCode::Enter => submit_input(runtime, state, playback).await,
        KeyCode::Char(ch)
            if !key_event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            state.push_input(ch);
            runtime.draw(state)?;
            Ok(RunningAction::Continue)
        }
        _ => Ok(RunningAction::Continue),
    }
}

async fn submit_input(
    runtime: &mut TuiRuntime,
    state: &mut AppState,
    playback: &mut PlaybackCoordinator,
) -> Result<RunningAction> {
    let line = state.take_input();
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(RunningAction::Continue);
    }
    let command = match parse_command(&line) {
        Ok(Some(command)) => command,
        Ok(None) => return Ok(RunningAction::Continue),
        Err(error) => {
            state.push_history_warning(format!("Enter a valid time or command. {error}"));
            runtime.draw(state)?;
            return Ok(RunningAction::Continue);
        }
    };

    if !matches!(command, PromptCommand::Help | PromptCommand::Status) {
        state.record_command(trimmed);
    }

    match command {
        PromptCommand::Help => {
            state.show_help();
        }
        PromptCommand::Reopen => match playback.reopen_player().await {
            Ok(()) => {
                state.update_snapshot(playback.snapshot(Instant::now()).await);
                state.set_player_action_error(None);
                state.push_history_info("Reopened the stream in QuickTime Player.");
            }
            Err(error) => {
                state.set_player_action_error(Some(format!(
                    "Unable to reopen QuickTime Player: {error}. Enter `reopen` to try again."
                )));
            }
        },
        PromptCommand::Status => {
            let snapshot = playback.snapshot(Instant::now()).await;
            state.update_snapshot(snapshot);
            state.toggle_status_details();
        }
        PromptCommand::Quit => return Ok(RunningAction::Completed),
        PromptCommand::JumpAbsolute(_) | PromptCommand::JumpRelative(_) => {
            if let Some(warning) = state.jump_unavailable_warning() {
                state.push_history_warning(warning);
                runtime.draw(state)?;
                return Ok(RunningAction::Continue);
            }

            let target = resolve_target(state.current_time(), &command)?;
            state.start_jump();
            runtime.draw(state)?;
            let jump_result = {
                let mut sink = JumpProgressRenderer { runtime, state };
                playback.jump_to(target, &mut sink).await
            };
            match jump_result {
                Ok(()) => {
                    let snapshot = playback.snapshot(Instant::now()).await;
                    state.finish_jump();
                    state.update_snapshot(snapshot);
                    state.push_history_info(format!("Jumped to {target}."));
                }
                Err(error) => {
                    state.finish_jump();
                    state.push_history_warning(format!(
                        "Unable to jump to that time: {error:#}. Try another time."
                    ));
                }
            }
        }
    }

    runtime.draw(state)?;
    Ok(RunningAction::Continue)
}

fn submit_source_input(state: &mut AppState) -> SourcePromptResult {
    let line = state.take_input();
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return SourcePromptResult::Continue;
    }

    state.record_command(trimmed);
    match trimmed {
        "help" | "h" | "?" | "/help" => {
            state.push_history_info("Enter a direct media URL to start a session.");
            state.push_history_muted("Example: https://example.com/video.mkv");
            SourcePromptResult::Continue
        }
        "quit" | "q" | "exit" => SourcePromptResult::Completed,
        _ => {
            if let Some(rest) = trimmed.strip_prefix("/url") {
                let url = rest.trim();
                if url.is_empty() {
                    state.push_history_warning("Add a media URL after `/url`, such as `/url https://example.com/video.mkv`.");
                    SourcePromptResult::Continue
                } else {
                    SourcePromptResult::Ready(url.to_string())
                }
            } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                SourcePromptResult::Ready(trimmed.to_string())
            } else {
                state.push_history_warning(
                    "Enter a full media URL that starts with `http://` or `https://`.",
                );
                SourcePromptResult::Continue
            }
        }
    }
}

fn playback_mode_from_simulation(simulation: Option<SimulationScenario>) -> PlaybackMode {
    match simulation {
        Some(scenario) => PlaybackMode::Simulated(scenario),
        None => PlaybackMode::Live,
    }
}

fn should_handle_key(key_event: KeyEvent) -> bool {
    matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunningAction {
    Continue,
    Completed,
    Interrupted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourcePromptResult {
    Continue,
    Ready(String),
    Completed,
    Interrupted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceRecoveryAction {
    Retry,
    Edit(String),
    Quit,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgressStatus {
    Pending,
    Active,
    Done,
    Warn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryTone {
    Command,
    Success,
    Info,
    Warning,
    Muted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryEntry {
    pub(crate) prefix: String,
    pub(crate) text: String,
    pub(crate) tone: HistoryTone,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgressLine<Step> {
    pub step: Step,
    pub status: ProgressStatus,
    pub details: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgressModel<Step> {
    pub title: String,
    pub lines: Vec<ProgressLine<Step>>,
    pub frame_index: usize,
}

impl<Step> ProgressModel<Step>
where
    Step: Copy + Eq,
{
    pub(crate) fn new(title: impl Into<String>, steps: &[Step]) -> Self {
        Self {
            title: title.into(),
            lines: steps
                .iter()
                .copied()
                .map(|step| ProgressLine {
                    step,
                    status: ProgressStatus::Pending,
                    details: Vec::new(),
                })
                .collect(),
            frame_index: 0,
        }
    }

    pub(crate) fn start(&mut self, step: Step, details: Vec<String>) {
        for line in &mut self.lines {
            if line.status == ProgressStatus::Active {
                line.status = ProgressStatus::Pending;
                line.details.clear();
            }
        }

        if let Some(line) = self.lines.iter_mut().find(|line| line.step == step) {
            line.status = ProgressStatus::Active;
            line.details = details;
        }
    }

    pub(crate) fn finish(&mut self, step: Step) {
        if let Some(line) = self.lines.iter_mut().find(|line| line.step == step) {
            line.status = ProgressStatus::Done;
            line.details.clear();
        }
    }

    pub(crate) fn warn(&mut self, step: Step, details: Vec<String>) {
        if let Some(line) = self.lines.iter_mut().find(|line| line.step == step) {
            line.status = ProgressStatus::Warn;
            line.details = details;
        }
    }

    pub(crate) fn tick(&mut self) -> bool {
        if self
            .lines
            .iter()
            .any(|line| line.status == ProgressStatus::Active)
        {
            self.frame_index = (self.frame_index + 1) % ACTIVE_FRAMES.len();
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectionFocus {
    Video,
    Audio,
}

#[derive(Clone, Debug)]
pub(crate) struct TrackSelectionState {
    pub(crate) request: TrackSelectionRequest,
    pub(crate) focus: SelectionFocus,
    pub(crate) video_index: usize,
    pub(crate) audio_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct LauncherState {
    pub(crate) input: String,
    pub(crate) cursor: usize,
    pub(crate) history: Vec<HistoryEntry>,
    pub(crate) help_open: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SourceErrorState {
    pub(crate) attempted_url: String,
    pub(crate) summary: String,
    pub(crate) diagnostic: String,
    pub(crate) input: String,
    pub(crate) editing: bool,
}

impl SourceErrorState {
    fn from_runtime(attempted_url: String, error: &RuntimeError) -> Self {
        let summary = match error {
            RuntimeError::InvalidSourceUrl { .. } => {
                "Enter a full URL that starts with http:// or https://."
            }
            RuntimeError::FfprobeUnavailable { .. } => "Install ffprobe, then try again.",
            RuntimeError::ExecuteBinary { .. } => {
                "Unable to start ffprobe. Check its path, then try again."
            }
            RuntimeError::FfprobeFailed { .. } => {
                "Unable to inspect this source. Check the URL and access, then try again."
            }
            RuntimeError::MediaInfoParse(_) => {
                "Unable to read this source's media details. Try another source."
            }
            _ => "Unable to inspect this source. Check the URL, then try again.",
        };
        Self {
            input: attempted_url.clone(),
            attempted_url,
            summary: summary.to_string(),
            diagnostic: error.to_string(),
            editing: false,
        }
    }

    fn no_video(attempted_url: String) -> Self {
        Self {
            input: attempted_url.clone(),
            attempted_url,
            summary: String::from("No supported video track was found."),
            diagnostic: String::from(
                "Choose a source with a supported video track, then try again.",
            ),
            editing: false,
        }
    }
}

impl LauncherState {
    fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            help_open: false,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RunningState {
    pub(crate) source_url: String,
    pub(crate) playback_mode: PlaybackMode,
    pub(crate) selection: StreamSelection,
    pub(crate) inspection: SourceInspection,
    pub(crate) media_info: MediaInfo,
    pub(crate) snapshot: PlaybackSnapshot,
    pub(crate) input: String,
    pub(crate) input_cursor: usize,
    pub(crate) jump_progress: Option<ProgressModel<JumpStep>>,
    pub(crate) prepare_history: Vec<HistoryEntry>,
    pub(crate) startup_history: Vec<HistoryEntry>,
    pub(crate) history: Vec<HistoryEntry>,
    pub(crate) details: Option<String>,
    pub(crate) activity_scroll: usize,
    pub(crate) player_action_error: Option<String>,
    pub(crate) help_open: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct StartupContext {
    pub(crate) requested_start_at: Timecode,
    pub(crate) actual_start_at: Timecode,
}

impl RunningState {
    pub(crate) fn new(
        source_url: String,
        playback_mode: PlaybackMode,
        selection: StreamSelection,
        inspection: SourceInspection,
        media_info: MediaInfo,
        snapshot: PlaybackSnapshot,
        startup: StartupContext,
    ) -> Self {
        let mut history = Vec::new();
        if startup.requested_start_at != Timecode::ZERO && startup.actual_start_at == Timecode::ZERO
        {
            push_history_lines(
                &mut history,
                "!",
                "Started from the beginning because this source doesn't support jumping to a different time.",
                HistoryTone::Warning,
            );
        }
        if let Some(warning) = inspection.seek_warning() {
            push_history_lines(
                &mut history,
                "!",
                format_warning(warning),
                HistoryTone::Warning,
            );
        }
        if let Some(notice) = selection.audio_notice() {
            push_history_lines(&mut history, "·", notice, HistoryTone::Info);
        }

        Self {
            source_url,
            playback_mode,
            selection,
            inspection,
            media_info,
            snapshot,
            input: String::new(),
            input_cursor: 0,
            jump_progress: None,
            prepare_history: Vec::new(),
            startup_history: Vec::new(),
            history,
            details: None,
            activity_scroll: 0,
            player_action_error: None,
            help_open: false,
        }
    }

    fn jump_unavailable_warning(&self) -> Option<String> {
        match self.inspection.seek_support() {
            SeekSupport::Enabled => None,
            SeekSupport::Disabled { warning } => Some(format!(
                "Jumping to a different time isn't available: {warning}"
            )),
        }
    }

    fn status_text(&self) -> String {
        let mut lines = vec![
            format!("Mode               | {}", self.playback_mode.label()),
            format!("Source             | {}", self.source_url),
            format!("Stream             | {}", self.snapshot.stream_url()),
            format!("Session ID         | {}", self.snapshot.session_id()),
            format!("Start time         | {}", self.snapshot.start_time()),
            format!(
                "Current time       | {}",
                crate::text::format_playback_time(
                    self.snapshot.current_time(),
                    self.media_info.duration(),
                )
            ),
            format!(
                "Relay write rate   | {}",
                format_bytes_per_second(self.snapshot.telemetry().relay_write_bytes_per_second())
            ),
            format!(
                "Buffer ahead       | {}",
                self.snapshot.telemetry().buffer_ahead()
            ),
            format!(
                "Storage used       | {}",
                format_bytes(self.snapshot.telemetry().storage_bytes())
            ),
            format!(
                "QuickTime Player   | {}",
                match self.snapshot.player_state() {
                    PlayerState::Playing => "Playing",
                    PlayerState::Paused => "Paused",
                    PlayerState::WindowClosed => "Window closed",
                    PlayerState::AppClosed => "Closed",
                    PlayerState::Unavailable => "Status unavailable",
                }
            ),
            format!(
                "Time jumps         | {}",
                match self.inspection.seek_support() {
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
        if let Some(audio_notice) = self.selection.audio_notice() {
            lines.push(audio_notice);
        }
        lines.join("\n")
    }

    fn trim_history(&mut self) {
        if self.history.len() > MAX_ACTIVITY_ENTRIES {
            self.history
                .drain(..self.history.len() - MAX_ACTIVITY_ENTRIES);
        }
        self.activity_scroll = self
            .activity_scroll
            .min(self.history.len().saturating_sub(1));
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Screen {
    Launcher(LauncherState),
    Inspecting {
        progress: ProgressModel<PrepareStep>,
    },
    TrackSelection(TrackSelectionState),
    Starting {
        selection_title: String,
        progress: ProgressModel<LaunchStep>,
        prepare_history: Vec<HistoryEntry>,
    },
    Running(Box<RunningState>),
    SourceError(SourceErrorState),
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub version: String,
    pub source_url: String,
    pub prepare_history: Vec<HistoryEntry>,
    pub screen: Screen,
}

impl AppState {
    pub(crate) fn new(source_url: Option<String>) -> Self {
        let source_url = source_url.unwrap_or_default();
        Self {
            version: format!("quickbridge {}", env!("CARGO_PKG_VERSION")),
            source_url: source_url.clone(),
            prepare_history: Vec::new(),
            screen: if source_url.is_empty() {
                Screen::Launcher(LauncherState::new())
            } else {
                Screen::Inspecting {
                    progress: ProgressModel::new("Inspect source", &PrepareStep::ALL),
                }
            },
        }
    }

    fn begin_prepare(&mut self, source_url: String) {
        self.source_url = source_url;
        self.prepare_history.clear();
        self.screen = Screen::Inspecting {
            progress: ProgressModel::new("Inspect source", &PrepareStep::ALL),
        };
    }

    fn show_track_selection(
        &mut self,
        _inspection: &SourceInspection,
        request: TrackSelectionRequest,
    ) {
        let has_multiple_videos = request.videos().len() > 1;
        let has_audio_tracks = !request.audios().is_empty();
        self.prepare_history = match &self.screen {
            Screen::Inspecting { progress } => prepare_history(progress),
            _ => self.prepare_history.clone(),
        };
        self.screen = Screen::TrackSelection(TrackSelectionState {
            focus: if has_multiple_videos {
                SelectionFocus::Video
            } else if has_audio_tracks {
                SelectionFocus::Audio
            } else {
                SelectionFocus::Video
            },
            video_index: request.default_video_index(),
            audio_index: request.default_audio_index(),
            request,
        });
    }

    fn show_startup(&mut self, inspection: &SourceInspection, selection: &StreamSelection) {
        let prepare_history = self.prepare_history.clone();
        self.screen = Screen::Starting {
            selection_title: format!(
                "{}\n{}\n{}",
                inspection.metadata().filename(),
                selection.render_output_file(),
                inspection.metadata().display_size()
            ),
            progress: ProgressModel::new("Start session", &LaunchStep::ALL),
            prepare_history,
        };
    }

    fn show_running(&mut self, running: RunningState) {
        let mut running = running;
        running.prepare_history = self.prepare_history.clone();
        if let Screen::Starting { progress, .. } = &self.screen {
            running.startup_history = launch_history(progress);
        }
        self.screen = Screen::Running(Box::new(running));
    }

    fn show_source_error(&mut self, error: SourceErrorState) {
        self.screen = Screen::SourceError(error);
    }

    fn toggle_launcher_help(&mut self) {
        if let Screen::Launcher(launcher) = &mut self.screen {
            launcher.help_open = !launcher.help_open;
        }
    }

    fn close_launcher_help(&mut self) {
        if let Screen::Launcher(launcher) = &mut self.screen {
            launcher.help_open = false;
        }
    }

    fn move_input_cursor(&mut self, forward: bool) {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                launcher.cursor = if forward {
                    next_char_boundary(&launcher.input, launcher.cursor)
                } else {
                    previous_char_boundary(&launcher.input, launcher.cursor)
                };
            }
            Screen::Running(running) => {
                running.input_cursor = if forward {
                    next_char_boundary(&running.input, running.input_cursor)
                } else {
                    previous_char_boundary(&running.input, running.input_cursor)
                };
            }
            _ => {}
        }
    }

    fn move_input_cursor_to_edge(&mut self, end: bool) {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                launcher.cursor = if end { launcher.input.len() } else { 0 };
            }
            Screen::Running(running) => {
                running.input_cursor = if end { running.input.len() } else { 0 };
            }
            _ => {}
        }
    }

    fn edit_source_error_url(&mut self) {
        if let Screen::SourceError(error) = &mut self.screen {
            error.editing = true;
        }
    }

    fn cancel_source_error_edit(&mut self) {
        if let Screen::SourceError(error) = &mut self.screen {
            error.input.clone_from(&error.attempted_url);
            error.editing = false;
        }
    }

    fn update_snapshot(&mut self, snapshot: PlaybackSnapshot) {
        if let Screen::Running(running) = &mut self.screen {
            running.snapshot = snapshot;
        }
    }

    fn set_player_action_error(&mut self, message: Option<String>) {
        if let Screen::Running(running) = &mut self.screen {
            running.player_action_error = message;
        }
    }

    fn close_running_overlay(&mut self) {
        if let Screen::Running(running) = &mut self.screen {
            running.input.clear();
            running.input_cursor = 0;
            running.help_open = false;
            running.details = None;
        }
    }

    fn show_help(&mut self) {
        if let Screen::Running(running) = &mut self.screen {
            running.help_open = true;
            running.details = None;
        }
    }

    fn current_time(&self) -> Timecode {
        match &self.screen {
            Screen::Running(running) => running.snapshot.current_time(),
            _ => Timecode::ZERO,
        }
    }

    fn push_input(&mut self, ch: char) {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                launcher.input.insert(launcher.cursor, ch);
                launcher.cursor += ch.len_utf8();
            }
            Screen::Running(running) => {
                running.input.insert(running.input_cursor, ch);
                running.input_cursor += ch.len_utf8();
            }
            Screen::SourceError(error) if error.editing => error.input.push(ch),
            _ => {}
        }
    }

    fn append_input(&mut self, text: &str) {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                launcher.input.insert_str(launcher.cursor, text);
                launcher.cursor += text.len();
            }
            Screen::Running(running) => {
                running.input.insert_str(running.input_cursor, text);
                running.input_cursor += text.len();
            }
            Screen::SourceError(error) if error.editing => error.input.push_str(text),
            _ => {}
        }
    }

    fn pop_input(&mut self) {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                if launcher.cursor > 0 {
                    let previous = previous_char_boundary(&launcher.input, launcher.cursor);
                    launcher.input.drain(previous..launcher.cursor);
                    launcher.cursor = previous;
                }
            }
            Screen::Running(running) => {
                if running.input_cursor > 0 {
                    let previous = previous_char_boundary(&running.input, running.input_cursor);
                    running.input.drain(previous..running.input_cursor);
                    running.input_cursor = previous;
                }
            }
            Screen::SourceError(error) if error.editing => {
                error.input.pop();
            }
            _ => {}
        }
    }

    fn take_input(&mut self) -> String {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                launcher.cursor = 0;
                std::mem::take(&mut launcher.input)
            }
            Screen::Running(running) => {
                running.input_cursor = 0;
                std::mem::take(&mut running.input)
            }
            Screen::SourceError(error) if error.editing => std::mem::take(&mut error.input),
            _ => String::new(),
        }
    }

    fn start_jump(&mut self) {
        if let Screen::Running(running) = &mut self.screen {
            running.jump_progress = Some(ProgressModel::new(
                "Jump to a different time",
                &JumpStep::ALL,
            ));
        }
    }

    fn finish_jump(&mut self) {
        if let Screen::Running(running) = &mut self.screen {
            running.jump_progress = None;
        }
    }

    fn tick(&mut self) -> bool {
        match &mut self.screen {
            Screen::Launcher(_) => false,
            Screen::Inspecting { progress } => progress.tick(),
            Screen::Starting { progress, .. } => progress.tick(),
            Screen::Running(running) => {
                if let Some(progress) = &mut running.jump_progress {
                    progress.tick()
                } else {
                    false
                }
            }
            Screen::TrackSelection(_) => false,
            Screen::SourceError(_) => false,
        }
    }

    fn move_track_selection(&mut self, delta: isize) {
        let Screen::TrackSelection(selection) = &mut self.screen else {
            return;
        };

        match selection.focus {
            SelectionFocus::Video => {
                selection.video_index = shift_index(
                    selection.video_index,
                    selection.request.videos().len(),
                    delta,
                );
            }
            SelectionFocus::Audio => {
                if let Some(index) = selection.audio_index {
                    selection.audio_index =
                        Some(shift_index(index, selection.request.audios().len(), delta));
                }
            }
        }
    }

    fn switch_track_focus(&mut self, forward: bool) {
        let Screen::TrackSelection(selection) = &mut self.screen else {
            return;
        };
        let video_selectable = selection.request.videos().len() > 1;
        let audio_selectable = !selection.request.audios().is_empty();
        match (video_selectable, audio_selectable) {
            (true, true) => {
                selection.focus = match (selection.focus, forward) {
                    (SelectionFocus::Video, true) => SelectionFocus::Audio,
                    (SelectionFocus::Audio, true) => SelectionFocus::Video,
                    (SelectionFocus::Video, false) => SelectionFocus::Audio,
                    (SelectionFocus::Audio, false) => SelectionFocus::Video,
                };
            }
            (true, false) => {
                selection.focus = SelectionFocus::Video;
            }
            (false, true) => {
                selection.focus = SelectionFocus::Audio;
            }
            (false, false) => {
                selection.focus = SelectionFocus::Video;
            }
        }
    }

    fn confirm_track_selection(&self) -> Result<StreamSelection> {
        match &self.screen {
            Screen::TrackSelection(selection) => Ok(selection
                .request
                .build_selection(selection.video_index, selection.audio_index)?),
            _ => Err(UiError::TrackSelectionInactive),
        }
    }

    fn advance_or_confirm_track_selection(&mut self) -> Result<Option<StreamSelection>> {
        let Screen::TrackSelection(selection) = &mut self.screen else {
            return Err(UiError::TrackSelectionInactive);
        };
        if selection.focus == SelectionFocus::Video && !selection.request.audios().is_empty() {
            selection.focus = SelectionFocus::Audio;
            return Ok(None);
        }
        self.confirm_track_selection().map(Some)
    }

    fn jump_progress_mut(&mut self) -> Option<&mut ProgressModel<JumpStep>> {
        match &mut self.screen {
            Screen::Running(running) => running.jump_progress.as_mut(),
            _ => None,
        }
    }

    fn record_command(&mut self, command: &str) {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                push_history_lines(&mut launcher.history, "›", command, HistoryTone::Command);
            }
            Screen::Running(running) => {
                push_history_lines(&mut running.history, "›", command, HistoryTone::Command);
                running.trim_history();
            }
            _ => {}
        }
    }

    fn push_history_info(&mut self, text: impl AsRef<str>) {
        self.push_history("·", text.as_ref(), HistoryTone::Info);
    }

    fn push_history_warning(&mut self, text: impl AsRef<str>) {
        self.push_history("!", text.as_ref(), HistoryTone::Warning);
    }

    fn push_history_muted(&mut self, text: impl AsRef<str>) {
        self.push_history("·", text.as_ref(), HistoryTone::Muted);
    }

    fn push_history(&mut self, prefix: &str, text: &str, tone: HistoryTone) {
        match &mut self.screen {
            Screen::Launcher(launcher) => {
                push_history_lines(&mut launcher.history, prefix, text, tone)
            }
            Screen::Running(running) => {
                push_history_lines(&mut running.history, prefix, text, tone);
                running.trim_history();
            }
            _ => {}
        }
    }

    fn status_text(&self) -> String {
        match &self.screen {
            Screen::Running(running) => running.status_text(),
            _ => String::new(),
        }
    }

    fn toggle_status_details(&mut self) {
        let details = self.status_text();
        if let Screen::Running(running) = &mut self.screen {
            if running.details.as_deref() == Some(details.as_str()) {
                running.details = None;
            } else {
                running.details = Some(details);
                running.help_open = false;
            }
        }
    }

    fn scroll_activity(&mut self, older: bool) {
        if let Screen::Running(running) = &mut self.screen {
            let activity_len = running
                .prepare_history
                .len()
                .saturating_add(running.startup_history.len())
                .saturating_add(running.history.len())
                .saturating_add(
                    running
                        .jump_progress
                        .as_ref()
                        .map_or(0, |progress| progress.lines.len()),
                );
            running.activity_scroll = if older {
                (running.activity_scroll + 1).min(activity_len.saturating_sub(1))
            } else {
                running.activity_scroll.saturating_sub(1)
            };
        }
    }

    fn jump_unavailable_warning(&self) -> Option<String> {
        match &self.screen {
            Screen::Running(running) => running.jump_unavailable_warning(),
            _ => None,
        }
    }
}

fn push_history_lines(
    history: &mut Vec<HistoryEntry>,
    prefix: &str,
    text: impl AsRef<str>,
    tone: HistoryTone,
) {
    let text = text.as_ref();
    if text.is_empty() {
        history.push(HistoryEntry {
            prefix: prefix.to_string(),
            text: String::new(),
            tone,
        });
        return;
    }

    for (index, line) in text.lines().enumerate() {
        history.push(HistoryEntry {
            prefix: if index == 0 {
                prefix.to_string()
            } else {
                String::from("·")
            },
            text: line.to_string(),
            tone,
        });
    }
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(index, _)| cursor + index)
        .unwrap_or(text.len())
}

fn prepare_history(progress: &ProgressModel<PrepareStep>) -> Vec<HistoryEntry> {
    progress
        .lines
        .iter()
        .map(|line| HistoryEntry {
            prefix: progress_marker_for_status(line.status, progress.frame_index).to_string(),
            text: prepare_history_label(line.step, line.status).to_string(),
            tone: match line.status {
                ProgressStatus::Warn => HistoryTone::Warning,
                ProgressStatus::Pending => HistoryTone::Muted,
                ProgressStatus::Active => HistoryTone::Info,
                ProgressStatus::Done => HistoryTone::Success,
            },
        })
        .collect()
}

fn launch_history(progress: &ProgressModel<LaunchStep>) -> Vec<HistoryEntry> {
    progress
        .lines
        .iter()
        .map(|line| HistoryEntry {
            prefix: progress_marker_for_status(line.status, progress.frame_index).to_string(),
            text: launch_history_label(line.step, line.status).to_string(),
            tone: match line.status {
                ProgressStatus::Warn => HistoryTone::Warning,
                ProgressStatus::Pending => HistoryTone::Muted,
                ProgressStatus::Active => HistoryTone::Info,
                ProgressStatus::Done => HistoryTone::Success,
            },
        })
        .collect()
}

fn progress_marker_for_status(status: ProgressStatus, frame_index: usize) -> &'static str {
    match status {
        ProgressStatus::Done => "✓",
        ProgressStatus::Active => ACTIVE_FRAMES[frame_index % ACTIVE_FRAMES.len()],
        ProgressStatus::Pending => "·",
        ProgressStatus::Warn => "!",
    }
}

fn prepare_history_label(step: PrepareStep, status: ProgressStatus) -> &'static str {
    match (step, status) {
        (PrepareStep::SourceUrl, ProgressStatus::Pending) => "Source URL",
        (PrepareStep::SourceUrl, ProgressStatus::Active) => "Checking source URL",
        (PrepareStep::SourceUrl, ProgressStatus::Done | ProgressStatus::Warn) => {
            "Checked source URL"
        }
        (PrepareStep::TimeJumps, ProgressStatus::Pending) => "Time jumps",
        (PrepareStep::TimeJumps, ProgressStatus::Active) => {
            "Checking whether time jumps are available"
        }
        (PrepareStep::TimeJumps, ProgressStatus::Done) => "Time jumps are available",
        (PrepareStep::TimeJumps, ProgressStatus::Warn) => "Time jumps aren't available",
        (PrepareStep::SourceDetails, ProgressStatus::Pending) => "Read source details",
        (PrepareStep::SourceDetails, ProgressStatus::Active) => "Reading source details",
        (PrepareStep::SourceDetails, ProgressStatus::Done | ProgressStatus::Warn) => {
            "Read source details"
        }
        (PrepareStep::Tracks, ProgressStatus::Pending) => "Found video and audio tracks",
        (PrepareStep::Tracks, ProgressStatus::Active) => "Finding video and audio tracks",
        (PrepareStep::Tracks, ProgressStatus::Done | ProgressStatus::Warn) => {
            "Found video and audio tracks"
        }
    }
}

fn launch_history_label(step: LaunchStep, status: ProgressStatus) -> &'static str {
    match (step, status) {
        (LaunchStep::LocalStreamServer, ProgressStatus::Pending) => "Local stream server",
        (LaunchStep::LocalStreamServer, ProgressStatus::Active) => "Starting local stream server",
        (LaunchStep::LocalStreamServer, ProgressStatus::Done | ProgressStatus::Warn) => {
            "Started local stream server"
        }
        (LaunchStep::Relay, ProgressStatus::Pending) => "ffmpeg relay",
        (LaunchStep::Relay, ProgressStatus::Active) => "Starting ffmpeg relay",
        (LaunchStep::Relay, ProgressStatus::Done | ProgressStatus::Warn) => "Started ffmpeg relay",
        (LaunchStep::Player, ProgressStatus::Pending) => "QuickTime Player",
        (LaunchStep::Player, ProgressStatus::Active) => "Opening QuickTime Player",
        (LaunchStep::Player, ProgressStatus::Done | ProgressStatus::Warn) => {
            "Opened QuickTime Player"
        }
    }
}

fn shift_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }

    let next = current as isize + delta;
    next.clamp(0, len.saturating_sub(1) as isize) as usize
}

struct PrepareProgressRenderer<'a> {
    runtime: &'a mut TuiRuntime,
    state: &'a mut AppState,
    verbose: bool,
}

impl ProgressSink<PrepareEvent> for PrepareProgressRenderer<'_> {
    type Error = RuntimeError;

    fn on_event(&mut self, event: PrepareEvent) -> quickbridge_runtime::Result<()> {
        let Screen::Inspecting { progress } = &mut self.state.screen else {
            return Ok(());
        };

        match event {
            ProgressEvent::Started { step, details } => {
                progress.start(step, filter_details(self.verbose, details));
            }
            ProgressEvent::Finished { step } => {
                progress.finish(step);
            }
            ProgressEvent::Warned { step, details } => {
                progress.warn(step, filter_details(self.verbose, details));
            }
        }
        self.runtime.draw(self.state).map_err(progress_sink_error)
    }

    fn on_tick(&mut self) -> quickbridge_runtime::Result<()> {
        poll_for_interrupt().map_err(progress_sink_error)?;
        self.state.tick();
        self.runtime.draw(self.state).map_err(progress_sink_error)
    }
}

struct LaunchProgressRenderer<'a> {
    runtime: &'a mut TuiRuntime,
    state: &'a mut AppState,
    verbose: bool,
}

impl ProgressSink<LaunchEvent> for LaunchProgressRenderer<'_> {
    type Error = RuntimeError;

    fn on_event(&mut self, event: LaunchEvent) -> quickbridge_runtime::Result<()> {
        let Screen::Starting { progress, .. } = &mut self.state.screen else {
            return Ok(());
        };

        match event {
            ProgressEvent::Started { step, details } => {
                progress.start(step, filter_details(self.verbose, details));
            }
            ProgressEvent::Finished { step } => {
                progress.finish(step);
            }
            ProgressEvent::Warned { step, details } => {
                progress.warn(step, filter_details(self.verbose, details));
            }
        }
        self.runtime.draw(self.state).map_err(progress_sink_error)
    }

    fn on_tick(&mut self) -> quickbridge_runtime::Result<()> {
        poll_for_interrupt().map_err(progress_sink_error)?;
        self.state.tick();
        self.runtime.draw(self.state).map_err(progress_sink_error)
    }
}

struct JumpProgressRenderer<'a> {
    runtime: &'a mut TuiRuntime,
    state: &'a mut AppState,
}

impl ProgressSink<JumpEvent> for JumpProgressRenderer<'_> {
    type Error = RuntimeError;

    fn on_event(&mut self, event: JumpEvent) -> quickbridge_runtime::Result<()> {
        let Some(progress) = self.state.jump_progress_mut() else {
            return Ok(());
        };

        match event {
            ProgressEvent::Started { step, details } => progress.start(step, details),
            ProgressEvent::Finished { step } => progress.finish(step),
            ProgressEvent::Warned { step, details } => progress.warn(step, details),
        }
        self.runtime.draw(self.state).map_err(progress_sink_error)
    }

    fn on_tick(&mut self) -> quickbridge_runtime::Result<()> {
        poll_for_interrupt().map_err(progress_sink_error)?;
        self.state.tick();
        self.runtime.draw(self.state).map_err(progress_sink_error)
    }
}

fn filter_details(verbose: bool, details: Vec<String>) -> Vec<String> {
    if verbose { details } else { Vec::new() }
}

fn progress_sink_error(error: UiError) -> RuntimeError {
    match error {
        UiError::Interrupted => RuntimeError::Interrupted,
        other => RuntimeError::ProgressSink {
            message: other.to_string(),
        },
    }
}

fn poll_for_interrupt() -> Result<()> {
    while event::poll(Duration::from_millis(0)).map_err(|source| UiError::Terminal {
        action: "poll terminal input",
        source,
    })? {
        match event::read().map_err(|source| UiError::Terminal {
            action: "read terminal input",
            source,
        })? {
            Event::Key(key_event)
                if should_handle_key(key_event)
                    && matches!(key_event.code, KeyCode::Char('c'))
                    && key_event.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                return Err(UiError::Interrupted);
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, RunningState, Screen, SelectionFocus, SourceErrorState, SourcePromptResult,
        StartupContext, TrackSelectionState, is_recoverable_prepare_error,
    };
    use quickbridge_core::{
        AudioStream, JumpStep, MediaInfo, PlaybackMode, PlaybackSnapshot, PlayerState, SeekSupport,
        SourceInspection, SourceMetadata, StreamSelection, StreamTelemetry, Timecode, VideoStream,
    };
    use quickbridge_runtime::RuntimeError;

    fn running_state() -> AppState {
        let running = RunningState::new(
            String::from("https://example.com/video.mkv"),
            PlaybackMode::Live,
            StreamSelection::new(VideoStream::new(0, "video", true), None),
            SourceInspection::new(
                SourceMetadata::new("video.mkv", Some(42)),
                SeekSupport::Enabled,
            ),
            MediaInfo::new(Vec::new(), Vec::new(), Some(Timecode::from_seconds(120))),
            PlaybackSnapshot::new(
                7,
                String::from("http://127.0.0.1:1234/stream.m3u8?session=7"),
                Timecode::ZERO,
                Timecode::from_seconds(12),
                PlayerState::Playing,
                StreamTelemetry::new(1, 2, Timecode::from_seconds(6)),
            ),
            StartupContext {
                requested_start_at: Timecode::ZERO,
                actual_start_at: Timecode::ZERO,
            },
        );
        let mut state = AppState::new(None);
        state.screen = Screen::Running(Box::new(running));
        state
    }

    #[test]
    fn switching_track_focus_preserves_both_selections() {
        let request = MediaInfo::new(
            vec![
                VideoStream::new(0, "video 0", true),
                VideoStream::new(1, "video 1", false),
            ],
            vec![
                AudioStream::new(2, Some(String::from("aac")), "audio 0", true),
                AudioStream::new(3, Some(String::from("aac")), "audio 1", false),
            ],
            None,
        )
        .selection_request()
        .unwrap();
        let mut state = AppState::new(None);
        state.screen = Screen::TrackSelection(TrackSelectionState {
            request,
            focus: SelectionFocus::Video,
            video_index: 1,
            audio_index: Some(1),
        });

        state.switch_track_focus(true);

        let Screen::TrackSelection(selection) = &state.screen else {
            unreachable!();
        };
        assert_eq!(selection.focus, SelectionFocus::Audio);
        assert_eq!(selection.video_index, 1);
        assert_eq!(selection.audio_index, Some(1));
    }

    #[test]
    fn enter_on_video_advances_to_audio_before_confirming() {
        let request = MediaInfo::new(
            vec![
                VideoStream::new(0, "video 0", true),
                VideoStream::new(1, "video 1", false),
            ],
            vec![AudioStream::new(
                2,
                Some(String::from("aac")),
                "audio 0",
                true,
            )],
            None,
        )
        .selection_request()
        .unwrap();
        let mut state = AppState::new(None);
        state.screen = Screen::TrackSelection(TrackSelectionState {
            request,
            focus: SelectionFocus::Video,
            video_index: 1,
            audio_index: Some(0),
        });

        assert!(
            state
                .advance_or_confirm_track_selection()
                .unwrap()
                .is_none()
        );
        let Screen::TrackSelection(selection) = &state.screen else {
            unreachable!();
        };
        assert_eq!(selection.focus, SelectionFocus::Audio);

        let confirmed = state
            .advance_or_confirm_track_selection()
            .unwrap()
            .expect("audio confirmation should start playback");
        let output = confirmed.render_output_file();
        assert!(output.contains("video 1"));
        assert!(output.contains("audio 0"));
    }

    #[test]
    fn running_activity_retains_only_the_latest_two_hundred_entries() {
        let mut state = running_state();
        for index in 0..1_000 {
            state.push_history_info(format!("activity {index}"));
        }

        let Screen::Running(running) = &state.screen else {
            unreachable!();
        };
        assert_eq!(running.history.len(), 200);
        assert_eq!(running.history.first().unwrap().text, "activity 800");
        assert_eq!(running.history.last().unwrap().text, "activity 999");
    }

    #[test]
    fn status_details_toggle_without_growing_activity() {
        let mut state = running_state();
        let initial_len = match &state.screen {
            Screen::Running(running) => running.history.len(),
            _ => unreachable!(),
        };

        state.toggle_status_details();
        let Screen::Running(running) = &state.screen else {
            unreachable!();
        };
        let details = running.details.as_deref().unwrap();
        assert!(details.contains("Source             | https://example.com/video.mkv"));
        assert!(details.contains("Session ID         | 7"));
        assert!(details.contains("Tracks"));
        assert!(details.contains("QuickTime Player   | Playing"));
        assert_eq!(running.history.len(), initial_len);

        state.toggle_status_details();
        let Screen::Running(running) = &state.screen else {
            unreachable!();
        };
        assert!(running.details.is_none());
        assert_eq!(running.history.len(), initial_len);
    }

    #[test]
    fn prepare_error_states_use_typed_actionable_summaries() {
        let cases = [
            (
                RuntimeError::InvalidSourceUrl {
                    source_url: String::from("bad"),
                },
                "Enter a full URL that starts with http:// or https://.",
            ),
            (
                RuntimeError::FfprobeUnavailable {
                    binary: String::from("custom-ffprobe"),
                },
                "Install ffprobe, then try again.",
            ),
            (
                RuntimeError::FfprobeFailed {
                    stderr: String::from("denied"),
                },
                "Unable to inspect this source. Check the URL and access, then try again.",
            ),
        ];
        for (error, summary) in cases {
            assert!(is_recoverable_prepare_error(&error));
            let state = SourceErrorState::from_runtime(String::from("source"), &error);
            assert_eq!(state.summary, summary);
            assert!(!state.diagnostic.is_empty());
        }

        let no_video = SourceErrorState::no_video(String::from("source"));
        assert!(no_video.summary.contains("supported video track"));
    }

    #[test]
    fn enter_submits_a_direct_launcher_url() {
        let mut state = AppState::new(None);
        state.append_input("https://example.com/demo.mkv");

        assert_eq!(
            super::submit_source_input(&mut state),
            SourcePromptResult::Ready(String::from("https://example.com/demo.mkv"))
        );
    }

    #[test]
    fn launcher_editor_inserts_and_deletes_at_the_caret() {
        let mut state = AppState::new(None);
        state.append_input("https://example.com/mvie.mkv");
        for _ in 0..7 {
            state.move_input_cursor(false);
        }
        state.push_input('o');

        let Screen::Launcher(launcher) = &state.screen else {
            unreachable!();
        };
        assert_eq!(launcher.input, "https://example.com/movie.mkv");
        assert_eq!(launcher.cursor, "https://example.com/mo".len());
    }

    #[test]
    fn running_composer_and_help_restore_cleanly() {
        let mut state = running_state();
        state.append_input("status");
        let Screen::Running(running) = &state.screen else {
            unreachable!();
        };
        assert_eq!(running.input, "status");

        state.close_running_overlay();
        state.show_help();
        let Screen::Running(running) = &state.screen else {
            unreachable!();
        };
        assert!(running.help_open);

        state.close_running_overlay();
        let Screen::Running(running) = &state.screen else {
            unreachable!();
        };
        assert!(running.input.is_empty());
        assert!(!running.help_open);
    }

    #[test]
    fn idle_screens_do_not_request_periodic_redraws() {
        let mut launcher = AppState::new(None);
        assert!(!launcher.tick());

        let mut running = running_state();
        assert!(!running.tick());
        running.start_jump();
        running
            .jump_progress_mut()
            .unwrap()
            .start(JumpStep::PrepareNextStream, Vec::new());
        assert!(running.tick());
    }
}
