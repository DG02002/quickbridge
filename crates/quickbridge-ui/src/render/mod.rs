use super::app::{
    AppState, HistoryEntry, HistoryTone, LauncherState, ProgressModel, ProgressStatus, RunningState,
    Screen, TrackSelectionState,
};
use crate::text::{format_bytes, format_bytes_per_second, format_playback_time};
use quickbridge_core::{JumpStep, LaunchStep, PlayerState, PrepareStep};
use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

const ACTIVE_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(crate) fn render(frame: &mut Frame<'_>, state: &AppState) {
    let width = frame.area().width as usize;
    if let Screen::TrackSelection(selection) = &state.screen {
        let lines = track_selection_lines(state, selection);
        let scroll = state.track_selection_scroll_offset(frame.area().height as usize);
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((scroll as u16, 0))
                .wrap(Wrap { trim: false }),
            frame.area(),
        );
        return;
    }

    if let Screen::Running(running) = &state.screen {
        let lines = running_lines(state, running, width);
        let scroll = bottom_scroll_offset(&lines, width, frame.area().height as usize);
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((scroll as u16, 0))
                .wrap(Wrap { trim: false }),
            frame.area(),
        );
        return;
    }

    let lines = match &state.screen {
        Screen::Launcher(launcher) => launcher_lines(state, launcher, width),
        Screen::Inspecting { progress } => progress_screen_lines(
            "Inspect source",
            None,
            progress,
            prepare_label,
        ),
        Screen::Starting {
            selection_title,
            progress,
            prepare_history,
        } => starting_screen_lines(
            state,
            &progress.title,
            selection_title.lines().collect::<Vec<_>>(),
            prepare_history,
            progress,
        ),
        Screen::Running(_) => unreachable!("handled above"),
        Screen::TrackSelection(_) => unreachable!("handled above"),
    };

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        frame.area(),
    );
}

fn launcher_lines(state: &AppState, launcher: &LauncherState, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(state.version.clone(), version_style()),
        Line::raw(""),
        section_title("quickbridge"),
        Line::styled(
            "Paste a media URL or run `/url https://example.com/video.mkv`.",
            text_style(),
        ),
        Line::styled(
            "The session history stays in the terminal buffer.",
            detail_style(),
        ),
        Line::raw(""),
        section_title("Session"),
    ];
    lines.extend(history_lines(&launcher.history));
    lines.push(Line::raw(""));
    lines.push(section_title("Input"));
    lines.extend(input_lines(
        &launcher.input,
        "/url https://example.com/video.mkv",
        "Enter submit  •  Ctrl+C exit",
        width,
    ));
    lines
}

fn progress_screen_lines<Step>(
    heading: &str,
    section: Option<(&str, Vec<&str>)>,
    progress: &ProgressModel<Step>,
    label_for: fn(Step, ProgressStatus) -> &'static str,
) -> Vec<Line<'static>>
where
    Step: Copy,
{
    let mut lines = vec![Line::styled(
        format!("quickbridge {}", env!("CARGO_PKG_VERSION")),
        version_style(),
    )];
    lines.push(Line::raw(""));
    lines.push(section_title(heading));
    if let Some((_label, section_lines)) = section {
        lines.extend(
            section_lines
                .into_iter()
                .map(|line| Line::styled(line.to_owned(), text_style())),
        );
        lines.push(Line::raw(""));
    }
    lines.extend(progress_flow_lines(progress, label_for));
    lines
}

fn starting_screen_lines(
    state: &AppState,
    heading: &str,
    selection_lines: Vec<&str>,
    prepare_history: &[HistoryEntry],
    progress: &ProgressModel<LaunchStep>,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(state.version.clone(), version_style())];
    lines.push(Line::raw(""));
    lines.push(section_title("Inspect source"));
    if !prepare_history.is_empty() {
        lines.extend(history_lines(prepare_history));
    }
    lines.push(Line::raw(""));
    lines.push(section_title("Selected media"));
    lines.extend(
        selection_lines
            .into_iter()
            .map(|line| Line::styled(line.to_owned(), text_style())),
    );
    lines.push(Line::raw(""));
    lines.push(section_title(heading));
    lines.extend(progress_flow_lines(progress, launch_label));
    lines
}

fn track_selection_lines(state: &AppState, selection: &TrackSelectionState) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(state.version.clone(), version_style())];
    lines.push(Line::raw(""));
    lines.push(section_title("Inspect source"));
    lines.extend(history_lines(&selection.prepare_history));
    lines.push(Line::raw(""));
    lines.push(section_title("Select tracks"));
    if selection.request.videos().len() > 1 {
        lines.push(Line::styled("Video".to_string(), label_style()));
        lines.extend(
            selection
                .request
                .videos()
                .iter()
                .enumerate()
                .map(|(index, stream)| {
                    selectable_line(index == selection.video_index, stream.display_line())
                }),
        );
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled("Audio".to_string(), label_style()));
    if selection.request.audios().is_empty() {
        lines.push(Line::styled("No audio track available".to_string(), detail_style()));
    } else {
        lines.extend(
            selection
                .request
                .audios()
                .iter()
                .enumerate()
                .map(|(index, stream)| {
                    selectable_line(selection.audio_index == Some(index), stream.display_line())
                }),
        );
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("Arrow keys move • Enter confirm".to_string(), detail_style()));
    lines
}

fn running_lines(state: &AppState, running: &RunningState, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(state.version.clone(), version_style())];
    lines.push(Line::raw(""));
    if !running.prepare_history.is_empty() {
        lines.push(section_title("Inspect source"));
        lines.extend(history_lines(&running.prepare_history));
        lines.push(Line::raw(""));
    }

    if !running.startup_history.is_empty() {
        lines.push(section_title("Start session"));
        lines.extend(history_lines(&running.startup_history));
        lines.push(Line::raw(""));
    }

    if !running.history.is_empty() {
        lines.extend(history_lines(&running.history));
    }

    if let Some(warning) = live_warning_line(running) {
        lines.push(Line::raw(""));
        lines.push(Line::styled(warning, warning_style()));
    }

    lines.push(Line::raw(""));
    lines.push(section_title("Live session"));
    lines.push(playback_status_line(running));

    if let Some(progress) = &running.jump_progress {
        lines.push(Line::raw(""));
        lines.extend(progress_flow_lines(progress, jump_label));
    }

    lines.push(Line::raw(""));
    lines.push(section_title("Input"));
    lines.extend(input_lines(
        &running.input,
        "01:30, +10, status, help, quit",
        "",
        width,
    ));
    lines
}

fn progress_flow_lines<Step>(
    progress: &ProgressModel<Step>,
    label_for: fn(Step, ProgressStatus) -> &'static str,
) -> Vec<Line<'static>>
where
    Step: Copy,
{
    let mut lines = Vec::new();
    for line in &progress.lines {
        lines.push(bullet_line(
            progress_marker(line.status, progress.frame_index),
            label_for(line.step, line.status),
            progress_style(line.status),
        ));
        lines.extend(
            line.details
                .iter()
                .map(|detail| Line::styled(format!("  {detail}"), detail_style())),
        );
    }
    lines
}

fn history_lines(history: &[HistoryEntry]) -> Vec<Line<'static>> {
    if history.is_empty() {
        return vec![Line::styled("No activity yet.".to_string(), detail_style())];
    }
    history.iter().map(history_line).collect()
}

fn input_lines(input: &str, placeholder: &str, footer: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let content = if input.is_empty() { placeholder } else { input };
    let content_style = if input.is_empty() {
        prompt_placeholder_style()
    } else {
        prompt_text_style()
    };
    let padding = width.saturating_sub(2 + content.chars().count());
    let mut lines = vec![
        Line::styled(" ".repeat(width), prompt_background_style()),
        Line::from(vec![
            Span::styled("> ", prompt_prefix_style()),
            Span::styled(content.to_string(), content_style),
            Span::styled(" ".repeat(padding), prompt_background_style()),
        ]),
        Line::styled(" ".repeat(width), prompt_background_style()),
    ];
    if !footer.is_empty() {
        lines.push(Line::styled(footer.to_string(), detail_style()));
    }
    lines
}

fn bottom_scroll_offset(lines: &[Line<'static>], width: usize, viewport_height: usize) -> usize {
    if viewport_height == 0 {
        return 0;
    }

    visual_line_count(lines, width).saturating_sub(viewport_height)
}

fn visual_line_count(lines: &[Line<'static>], width: usize) -> usize {
    let width = width.max(1);
    lines.iter().map(|line| visual_line_height(line, width)).sum()
}

fn visual_line_height(line: &Line<'static>, width: usize) -> usize {
    let content_width = line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();

    content_width.max(1).div_ceil(width)
}

fn selectable_line(selected: bool, text: &str) -> Line<'static> {
    if selected {
        Line::from(vec![
            Span::styled("> ", selected_line_style()),
            Span::styled(text.to_owned(), selected_line_style()),
        ])
    } else {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(text.to_owned(), text_style()),
        ])
    }
}

fn history_line(entry: &HistoryEntry) -> Line<'static> {
    let style = match entry.tone {
        HistoryTone::Command => accent_style(),
        HistoryTone::Success => success_style(),
        HistoryTone::Info => text_style(),
        HistoryTone::Warning => warning_style(),
        HistoryTone::Muted => detail_style(),
    };

    Line::from(vec![
        Span::styled(format!("{} ", entry.prefix), style),
        Span::styled(entry.text.clone(), style),
    ])
}

fn bullet_line(marker: &str, text: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{marker} "), style),
        Span::styled(text.to_owned(), style),
    ])
}

fn progress_marker(status: ProgressStatus, frame_index: usize) -> &'static str {
    match status {
        ProgressStatus::Done => "✓",
        ProgressStatus::Active => ACTIVE_FRAMES[frame_index % ACTIVE_FRAMES.len()],
        ProgressStatus::Pending => "○",
        ProgressStatus::Warn => "!",
    }
}

fn progress_style(status: ProgressStatus) -> Style {
    match status {
        ProgressStatus::Done => success_style(),
        ProgressStatus::Active => accent_style(),
        ProgressStatus::Pending => detail_style(),
        ProgressStatus::Warn => warning_style(),
    }
}

fn section_title(title: &str) -> Line<'static> {
    Line::styled(title.to_owned(), section_style())
}

fn prepare_label(step: PrepareStep, status: ProgressStatus) -> &'static str {
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

fn launch_label(step: LaunchStep, status: ProgressStatus) -> &'static str {
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

fn jump_label(step: JumpStep, _status: ProgressStatus) -> &'static str {
    match step {
        JumpStep::PrepareNextStream => "Preparing the next stream",
        JumpStep::WaitForStream => "Waiting for the stream",
        JumpStep::RefreshPlayer => "Refreshing QuickTime Player",
        JumpStep::CleanupPreviousSession => "Cleaning up the last session",
    }
}

fn playback_status_line(running: &RunningState) -> Line<'static> {
    let mut current_time = format_playback_time(running.snapshot.current_time(), running.media_info.duration());
    if matches!(running.snapshot.player_state(), PlayerState::Paused) {
        current_time.push_str(" (Paused)");
    }

    Line::from(vec![
        Span::styled(current_time, accent_style()),
        Span::styled("  •  ", detail_style()),
        Span::styled(
            format_bytes_per_second(running.snapshot.telemetry().download_bytes_per_second()),
            success_style(),
        ),
        Span::styled("  •  buffer ", detail_style()),
        Span::styled(
            running.snapshot.telemetry().buffer_ahead().to_string(),
            text_style(),
        ),
        Span::styled("  •  storage ", detail_style()),
        Span::styled(
            format_bytes(running.snapshot.telemetry().storage_bytes()),
            text_style(),
        ),
    ])
}

fn live_warning_line(running: &RunningState) -> Option<String> {
    match running.snapshot.player_state() {
        PlayerState::WindowClosed => Some(String::from(
            "[WARN] QuickTime Player window is closed. Type `reopen` to open the stream again.",
        )),
        PlayerState::AppClosed => Some(String::from(
            "[WARN] QuickTime Player is closed. Type `reopen` to open the stream again.",
        )),
        PlayerState::Unavailable => Some(String::from(
            "[WARN] QuickTime Player status isn't available right now.",
        )),
        PlayerState::Playing | PlayerState::Paused => None,
    }
}

fn version_style() -> Style {
    Style::default()
        .fg(Color::Rgb(170, 184, 204))
        .add_modifier(Modifier::BOLD)
}

fn section_style() -> Style {
    Style::default()
        .fg(Color::Rgb(103, 168, 222))
        .add_modifier(Modifier::BOLD)
}

fn label_style() -> Style {
    Style::default()
        .fg(Color::Rgb(214, 223, 237))
        .add_modifier(Modifier::BOLD)
}

fn text_style() -> Style {
    Style::default().fg(Color::Rgb(214, 223, 237))
}

fn detail_style() -> Style {
    Style::default().fg(Color::Rgb(122, 139, 164))
}

fn accent_style() -> Style {
    Style::default()
        .fg(Color::Rgb(112, 198, 255))
        .add_modifier(Modifier::BOLD)
}

fn selected_line_style() -> Style {
    Style::default()
        .fg(Color::Rgb(144, 214, 255))
        .add_modifier(Modifier::BOLD)
}

fn success_style() -> Style {
    Style::default()
        .fg(Color::Rgb(127, 211, 150))
        .add_modifier(Modifier::BOLD)
}

fn warning_style() -> Style {
    Style::default()
        .fg(Color::Rgb(255, 196, 107))
        .add_modifier(Modifier::BOLD)
}

fn prompt_background_style() -> Style {
    Style::default().bg(Color::Rgb(28, 38, 55))
}

fn prompt_prefix_style() -> Style {
    prompt_background_style()
        .fg(Color::Rgb(112, 198, 255))
        .add_modifier(Modifier::BOLD)
}

fn prompt_text_style() -> Style {
    prompt_background_style().fg(Color::Rgb(226, 233, 244))
}

fn prompt_placeholder_style() -> Style {
    prompt_background_style().fg(Color::Rgb(127, 144, 168))
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::{
        app::{
            AppState, HistoryEntry, HistoryTone, LauncherState, ProgressModel, RunningState,
            Screen, SelectionFocus, StartupContext, TrackSelectionState,
        },
        test_backend::VT100Backend,
    };
    use insta::assert_snapshot;
    use quickbridge_core::{
        AudioStream, JumpStep, LaunchStep, MediaInfo, PlaybackMode, PlaybackSnapshot, PlayerState,
        PrepareStep, SeekSupport, SimulationScenario, SourceInspection, SourceMetadata,
        StreamSelection, StreamTelemetry, Timecode, VideoStream,
    };
    use ratatui::Terminal;

    fn render_contents(state: &AppState) -> String {
        render_contents_with_size(state, 120, 40)
    }

    fn render_contents_with_size(state: &AppState, width: u16, height: u16) -> String {
        let backend = VT100Backend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, state)).unwrap();
        terminal.backend().to_string()
    }

    #[test]
    fn inspection_screen_renders_progress_states() {
        let mut state = AppState::new(Some(String::from("https://example.com/video.mkv")));
        let Screen::Inspecting { progress } = &mut state.screen else {
            panic!("expected inspecting screen");
        };
        progress.start(PrepareStep::TimeJumps, Vec::new());
        progress.finish(PrepareStep::SourceUrl);

        let contents = render_contents(&state);
        assert!(contents.contains("Inspect source"));
        assert!(contents.contains("✓ Checked source URL"));
        assert!(contents.contains("Checking whether time jumps are available"));
        assert_snapshot!("inspection_screen_120x40", contents);
    }

    #[test]
    fn launcher_screen_renders_clean_entrypoint() {
        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::new(),
            prepare_history: Vec::new(),
            screen: Screen::Launcher(LauncherState {
                input: String::new(),
                history: vec![HistoryEntry {
                    prefix: String::from("·"),
                    text: String::from("Paste a media URL or run `/url https://example.com/video.mkv` to start."),
                    tone: HistoryTone::Info,
                }],
            }),
        };

        let contents = render_contents(&state);
        assert!(contents.contains("/url https://example.com/video.mkv"));
        assert!(contents.contains("Input"));
    }

    #[test]
    fn track_selection_screen_renders_choices_and_focus() {
        let media_info = MediaInfo::new(
            vec![
                VideoStream::new(0, "Stream #0:0: Video: h264", true),
                VideoStream::new(2, "Stream #0:2: Video: hevc", false),
            ],
            vec![AudioStream::new(
                1,
                Some(String::from("aac")),
                "Stream #0:1: Audio: aac",
                true,
            )],
            Some(Timecode::from_seconds(60)),
        );
        let request = media_info.selection_request().unwrap();
        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::from("https://example.com/video.mkv"),
            prepare_history: vec![
                HistoryEntry {
                    prefix: String::from("✓"),
                    text: String::from("Checked source URL"),
                    tone: HistoryTone::Info,
                },
                HistoryEntry {
                    prefix: String::from("✓"),
                    text: String::from("Time jumps are available"),
                    tone: HistoryTone::Info,
                },
            ],
            screen: Screen::TrackSelection(TrackSelectionState {
                request,
                focus: SelectionFocus::Video,
                video_index: 1,
                audio_index: Some(0),
                prepare_history: vec![
                    HistoryEntry {
                        prefix: String::from("✓"),
                        text: String::from("Checked source URL"),
                        tone: HistoryTone::Info,
                    },
                    HistoryEntry {
                        prefix: String::from("✓"),
                        text: String::from("Time jumps are available"),
                        tone: HistoryTone::Info,
                    },
                ],
            }),
        };

        let contents = render_contents_with_size(&state, 80, 24);
        assert!(contents.contains("Select tracks"));
        assert!(contents.contains("> Stream #0:2: Video: hevc"));
        assert_snapshot!("track_selection_screen_80x24", contents);
    }

    #[test]
    fn running_screen_renders_history_metrics_and_jump_overlay() {
        let mut running = RunningState::new(
            String::from("https://example.com/video.mkv"),
            PlaybackMode::Simulated(SimulationScenario::NoRanges),
            StreamSelection::new(VideoStream::new(0, "Stream #0:0: Video: h264", true), None),
            SourceInspection::new(
                SourceMetadata::new("video.mkv", Some(42)),
                SeekSupport::Disabled {
                    warning: String::from("No ranges"),
                },
            ),
            MediaInfo::new(Vec::new(), Vec::new(), Some(Timecode::from_seconds(120))),
            PlaybackSnapshot::new(
                1,
                String::from("http://127.0.0.1:1234/stream.m3u8?session=1"),
                Timecode::ZERO,
                Timecode::from_seconds(12),
                PlayerState::Playing,
                StreamTelemetry::new(
                    3 * 1024 * 1024,
                    12 * 1024 * 1024,
                    Timecode::from_seconds(6),
                ),
            ),
            StartupContext {
                requested_start_at: Timecode::ZERO,
                actual_start_at: Timecode::ZERO,
            },
        );
        running.prepare_history = vec![
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Checked source URL"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Time jumps are available"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Read source details"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Found video and audio tracks"),
                tone: HistoryTone::Success,
            },
        ];
        running.startup_history = vec![
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Started local stream server"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Started ffmpeg relay"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Opened QuickTime Player"),
                tone: HistoryTone::Success,
            },
        ];
        running.input = String::from("status");
        running.history.push(HistoryEntry {
            prefix: String::from(">"),
            text: String::from("status"),
            tone: HistoryTone::Command,
        });
        let mut jump_progress = ProgressModel::new(
            "Jump",
            &[
                JumpStep::PrepareNextStream,
                JumpStep::WaitForStream,
                JumpStep::RefreshPlayer,
                JumpStep::CleanupPreviousSession,
            ],
        );
        jump_progress.start(JumpStep::RefreshPlayer, Vec::new());
        running.jump_progress = Some(jump_progress);
        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::from("https://example.com/video.mkv"),
            prepare_history: Vec::new(),
            screen: Screen::Running(Box::new(running)),
        };

        let contents = render_contents(&state);
        assert!(contents.contains("Start session"));
        assert!(contents.contains("✓ Opened QuickTime Player"));
        assert!(contents.contains("3.0 MB/s"));
        assert!(contents.contains("buffer 00:00:06"));
        assert!(contents.contains("12.0 MB"));
        assert!(contents.contains("Refreshing QuickTime Player"));
        assert_snapshot!("running_screen_120x40", contents);
    }

    #[test]
    fn running_screen_scrolls_to_keep_input_visible() {
        let mut running = RunningState::new(
            String::from("https://example.com/video.mkv"),
            PlaybackMode::Live,
            StreamSelection::new(VideoStream::new(0, "Stream #0:0: Video: h264", true), None),
            SourceInspection::new(
                SourceMetadata::new("video.mkv", Some(42)),
                SeekSupport::Enabled,
            ),
            MediaInfo::new(Vec::new(), Vec::new(), Some(Timecode::from_seconds(120))),
            PlaybackSnapshot::new(
                1,
                String::from("http://127.0.0.1:1234/stream.m3u8?session=1"),
                Timecode::ZERO,
                Timecode::from_seconds(12),
                PlayerState::WindowClosed,
                StreamTelemetry::new(
                    3 * 1024 * 1024,
                    12 * 1024 * 1024,
                    Timecode::from_seconds(6),
                ),
            ),
            StartupContext {
                requested_start_at: Timecode::ZERO,
                actual_start_at: Timecode::ZERO,
            },
        );
        running.prepare_history = vec![
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Checked source URL"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Time jumps are available"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Read source details"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Found video and audio tracks"),
                tone: HistoryTone::Success,
            },
        ];
        running.startup_history = vec![
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Started local stream server"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Started ffmpeg relay"),
                tone: HistoryTone::Success,
            },
            HistoryEntry {
                prefix: String::from("✓"),
                text: String::from("Opened QuickTime Player"),
                tone: HistoryTone::Success,
            },
        ];
        running.input = String::from("reopen");

        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::from("https://example.com/video.mkv"),
            prepare_history: Vec::new(),
            screen: Screen::Running(Box::new(running)),
        };

        let contents = render_contents_with_size(&state, 80, 12);
        assert!(contents.contains("Input"));
        assert!(contents.contains("> reopen"));
        assert!(contents.contains("Live session"));
    }

    #[test]
    fn startup_screen_renders_without_duplicate_sections() {
        let mut progress = ProgressModel::new(
            "Start session",
            &[
                LaunchStep::LocalStreamServer,
                LaunchStep::Relay,
                LaunchStep::Player,
            ],
        );
        progress.finish(LaunchStep::LocalStreamServer);
        progress.start(LaunchStep::Relay, Vec::new());
        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::from("https://example.com/video.mkv"),
            prepare_history: Vec::new(),
            screen: Screen::Starting {
                selection_title: String::from("video.mkv"),
                progress,
                prepare_history: vec![
                    HistoryEntry {
                        prefix: String::from("✓"),
                        text: String::from("Checked source URL"),
                        tone: HistoryTone::Info,
                    },
                    HistoryEntry {
                        prefix: String::from("✓"),
                        text: String::from("Time jumps are available"),
                        tone: HistoryTone::Info,
                    },
                ],
            },
        };

        let contents = render_contents(&state);
        assert_eq!(contents.matches("Start session").count(), 1);
        assert!(contents.contains("✓ Started local stream server"));
        assert!(contents.contains("Starting ffmpeg relay"));
    }
}
