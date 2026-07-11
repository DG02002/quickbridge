use crate::{
    cli::Cli,
    diagnostics::print_error,
    terminal::{
        emphasize, format_playback_time, format_warning, muted, require_interactive_terminal,
    },
};
use anyhow::{Result, bail};
use quickbridge_core::{
    Command as PromptCommand, JumpEvent, JumpStep, LaunchEvent, LaunchStep, PlaybackMode,
    PlaybackSnapshot, PlayerState, PrepareEvent, PrepareStep, ProgressEvent, ProgressSink,
    RunOutcome, SeekSupport, StreamSelection, Timecode, help_text, parse_command, resolve_target,
};
use quickbridge_runtime::{
    FfmpegRunner, PlaybackCoordinator, PrepareRequest, ProbeRunner, RuntimeError, StartRequest,
    prepare_source,
};
use quickbridge_ui::InteractiveOptions;
use std::time::Instant;

pub async fn run(cli: Cli) -> Result<RunOutcome> {
    if !cli.script.is_empty() && cli.url.is_none() {
        bail!("scripted mode requires a source URL");
    }

    if cli.simulation().is_none() {
        ensure_supported_platform()?;
    }

    let runner = FfmpegRunner::new(cli.verbose);
    let probe = ProbeRunner::new();
    if cli.simulation().is_none() {
        runner.ensure_available().await?;
        probe.ensure_available().await?;
    }

    if cli.script.is_empty() {
        require_interactive_terminal()?;
        return quickbridge_ui::run(
            InteractiveOptions {
                url: cli.url.clone(),
                port: cli.port,
                at: cli.at,
                verbose: cli.verbose,
                keep_temp: cli.keep_temp,
                simulation: cli.simulation(),
                no_alt_screen: cli.no_alt_screen,
            },
            runner,
            probe,
        )
        .await
        .map_err(Into::into);
    }

    run_scripted(cli, runner, probe).await
}

async fn run_scripted(cli: Cli, runner: FfmpegRunner, probe: ProbeRunner) -> Result<RunOutcome> {
    println!(
        "{}",
        emphasize(&format!("quickbridge {}", env!("CARGO_PKG_VERSION")))
    );
    println!();

    let mut prepare_reporter = PlainStageReporter::new("Inspect source", cli.verbose);
    let prepared = prepare_source(
        PrepareRequest {
            source_url: cli.url.clone().expect("scripted mode validates URL"),
            simulation: cli.simulation(),
            probe,
        },
        &mut prepare_reporter,
    )
    .await?;
    println!();

    let selection = prepared.media_info().default_selection()?;
    print_selected_media(
        prepared.inspection().metadata().filename(),
        &selection,
        prepared.inspection().metadata().display_size().as_str(),
    );

    let requested_start_at = cli.at.unwrap_or(Timecode::ZERO);
    let actual_start_at = if prepared.inspection().seeking_enabled() {
        requested_start_at
    } else {
        Timecode::ZERO
    };
    let playback_mode = playback_mode_from_simulation(cli.simulation());

    let mut launch_reporter = PlainStageReporter::new("Start session", cli.verbose);
    let (mut playback, start_outcome) = PlaybackCoordinator::start(
        StartRequest {
            source_url: cli.url.clone().expect("scripted mode validates URL"),
            port: cli.port,
            start_at: actual_start_at,
            keep_temp: cli.keep_temp,
            selection: selection.clone(),
            mode: playback_mode.clone(),
            runner,
        },
        &mut launch_reporter,
    )
    .await?;

    println!(
        "{} {}",
        emphasize("[FFMPEG]"),
        muted(start_outcome.relay_command())
    );
    println!(
        "{} {}",
        emphasize("[SERVER]"),
        muted(start_outcome.stream_url())
    );

    if requested_start_at != Timecode::ZERO && actual_start_at == Timecode::ZERO {
        println!();
        println!(
            "{}",
            format_warning(
                "Started from the beginning because this source doesn't support jumping to a different time.",
            )
        );
    }
    if let Some(warning) = prepared.inspection().seek_warning() {
        println!();
        println!("{}", format_warning(warning));
    }
    if let Some(notice) = selection.audio_notice() {
        println!();
        println!("{notice}");
    }

    let result = run_script_commands(
        &cli,
        &playback_mode,
        prepared.inspection().seek_support(),
        prepared.media_info().duration(),
        cli.url.as_deref().expect("scripted mode validates URL"),
        &selection,
        &mut playback,
    )
    .await;
    let shutdown = playback.shutdown().await;

    match (result, shutdown) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Ok(outcome), Ok(())) => Ok(outcome),
    }
}

async fn run_script_commands(
    cli: &Cli,
    playback_mode: &PlaybackMode,
    seek_support: &SeekSupport,
    total_runtime: Option<Timecode>,
    source_url: &str,
    selection: &StreamSelection,
    playback: &mut PlaybackCoordinator,
) -> Result<RunOutcome> {
    let mut live_snapshot = playback.snapshot(Instant::now()).await;

    for line in &cli.script {
        println!();
        println!("Scripted command");
        println!("  {line}");

        let command = parse_command(line)?
            .ok_or_else(|| anyhow::anyhow!("Scripted command `{line}` is empty"))?;

        match command {
            PromptCommand::Help => {
                println!("{}", help_text());
            }
            PromptCommand::Reopen => {
                playback.reopen_player().await?;
            }
            PromptCommand::Status => {
                live_snapshot = playback.snapshot(Instant::now()).await;
                println!(
                    "{}",
                    status_text(
                        playback_mode,
                        source_url,
                        selection,
                        seek_support,
                        total_runtime,
                        &live_snapshot,
                    )
                );
            }
            PromptCommand::Quit => return Ok(RunOutcome::Completed),
            PromptCommand::JumpAbsolute(_) | PromptCommand::JumpRelative(_) => {
                if let SeekSupport::Disabled { warning } = seek_support {
                    println!("Jumping to a different time isn't available: {warning}");
                    continue;
                }

                let target = resolve_target(live_snapshot.current_time(), &command)?;
                let mut jump_reporter = PlainJumpReporter::default();
                if let Err(error) = playback.jump_to(target, &mut jump_reporter).await {
                    let error = anyhow::Error::new(error);
                    print_error(&error);
                }
                live_snapshot = playback.snapshot(Instant::now()).await;
            }
        }
    }

    Ok(RunOutcome::Completed)
}

fn print_selected_media(filename: &str, selection: &StreamSelection, size: &str) {
    println!("{}", emphasize("Selected media"));
    println!("{filename}");
    if let Some(summary) = selection.selected_audio_summary() {
        println!("{summary}");
    }
    println!("{size}");
    println!();
}

fn status_text(
    playback_mode: &PlaybackMode,
    source_url: &str,
    selection: &StreamSelection,
    seek_support: &SeekSupport,
    total_runtime: Option<Timecode>,
    snapshot: &PlaybackSnapshot,
) -> String {
    let mut lines = vec![
        format!("Mode               | {}", playback_mode.label()),
        format!("Source             | {source_url}"),
        format!("Stream             | {}", snapshot.stream_url()),
        format!("Session ID         | {}", snapshot.session_id()),
        format!("Start time         | {}", snapshot.start_time()),
        format!(
            "Current time       | {}",
            format_playback_time(snapshot.current_time(), total_runtime)
        ),
        format!(
            "Relay write rate   | {}",
            format_bytes_per_second(snapshot.telemetry().relay_write_bytes_per_second())
        ),
        format!(
            "Buffer ahead       | {}",
            snapshot.telemetry().buffer_ahead()
        ),
        format!(
            "Storage used       | {}",
            format_bytes(snapshot.telemetry().storage_bytes())
        ),
        format!(
            "QuickTime Player   | {}",
            match snapshot.player_state() {
                PlayerState::Playing => "Playing",
                PlayerState::Paused => "Paused",
                PlayerState::WindowClosed => "Window closed",
                PlayerState::AppClosed => "Closed",
                PlayerState::Unavailable => "Status unavailable",
            }
        ),
        format!(
            "Time jumps         | {}",
            match seek_support {
                SeekSupport::Enabled => "Available",
                SeekSupport::Disabled { .. } => "Unavailable",
            }
        ),
        String::from("Tracks"),
    ];
    lines.extend(
        selection
            .render_output_file()
            .lines()
            .map(|line| format!("  {line}")),
    );
    if let Some(audio_notice) = selection.audio_notice() {
        lines.push(audio_notice);
    }
    lines.join("\n")
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    format!("{value:.1} {}", UNITS[unit_index])
}

fn format_bytes_per_second(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}

fn playback_mode_from_simulation(
    simulation: Option<quickbridge_core::SimulationScenario>,
) -> PlaybackMode {
    match simulation {
        Some(scenario) => PlaybackMode::Simulated(scenario),
        None => PlaybackMode::Live,
    }
}

fn ensure_supported_platform() -> Result<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        bail!("quickbridge supports macOS only. QuickTime Player is required")
    }
}

#[derive(Default)]
struct PlainJumpReporter {
    last_step: Option<JumpStep>,
}

impl ProgressSink<JumpEvent> for PlainJumpReporter {
    type Error = RuntimeError;

    fn on_event(&mut self, event: JumpEvent) -> quickbridge_runtime::Result<()> {
        if let ProgressEvent::Started { step, .. } = event
            && self.last_step != Some(step)
        {
            println!("• {}", jump_step_label(step));
            self.last_step = Some(step);
        }
        Ok(())
    }
}

struct PlainStageReporter {
    title: &'static str,
    verbose: bool,
    title_printed: bool,
}

impl PlainStageReporter {
    fn new(title: &'static str, verbose: bool) -> Self {
        Self {
            title,
            verbose,
            title_printed: false,
        }
    }

    fn print_title(&mut self) {
        if !self.title_printed {
            println!("{}", self.title);
            self.title_printed = true;
        }
    }
}

impl ProgressSink<PrepareEvent> for PlainStageReporter {
    type Error = RuntimeError;

    fn on_event(&mut self, event: PrepareEvent) -> quickbridge_runtime::Result<()> {
        self.print_title();
        match event {
            ProgressEvent::Started { .. } => {}
            ProgressEvent::Finished { step } => {
                println!("✓ {}", prepare_step_label(step, false))
            }
            ProgressEvent::Warned { step, details } => {
                println!("! {}", prepare_step_label(step, true));
                if self.verbose {
                    for detail in details {
                        println!("      {detail}");
                    }
                }
            }
        }
        Ok(())
    }
}

impl ProgressSink<LaunchEvent> for PlainStageReporter {
    type Error = RuntimeError;

    fn on_event(&mut self, event: LaunchEvent) -> quickbridge_runtime::Result<()> {
        self.print_title();
        match event {
            ProgressEvent::Started { .. } => {}
            ProgressEvent::Finished { step } => println!("✓ {}", launch_step_label(step)),
            ProgressEvent::Warned { step, .. } => println!("! {}", launch_step_label(step)),
        }
        Ok(())
    }
}

fn prepare_step_label(step: PrepareStep, warned: bool) -> &'static str {
    match (step, warned) {
        (PrepareStep::SourceUrl, _) => "Checked source URL",
        (PrepareStep::TimeJumps, false) => "Time jumps are available",
        (PrepareStep::TimeJumps, true) => "Time jumps aren't available",
        (PrepareStep::SourceDetails, _) => "Read source details",
        (PrepareStep::Tracks, _) => "Found video and audio tracks",
    }
}

fn launch_step_label(step: LaunchStep) -> &'static str {
    match step {
        LaunchStep::LocalStreamServer => "Started local stream server",
        LaunchStep::Relay => "Started ffmpeg relay",
        LaunchStep::Player => "Opened QuickTime Player",
    }
}

fn jump_step_label(step: JumpStep) -> &'static str {
    match step {
        JumpStep::PrepareNextStream => "Getting ready to jump",
        JumpStep::WaitForStream => "Waiting for the stream",
        JumpStep::RefreshPlayer => "Refreshing QuickTime Player",
        JumpStep::CleanupPreviousSession => "Cleaning up the last session",
    }
}
