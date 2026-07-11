use super::app::{
    AppState, HistoryEntry, HistoryTone, LauncherState, ProgressModel, ProgressStatus,
    RunningState, Screen, SourceErrorState, TrackSelectionState,
};
use crate::text::{format_bytes, format_bytes_per_second, format_playback_time};
use quickbridge_core::{
    AudioStream, JumpStep, LaunchStep, PlayerState, PrepareStep, VideoStream, help_text,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph, Wrap},
};

const ACTIVE_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(crate) fn render(frame: &mut Frame<'_>, state: &AppState, full_screen: bool) {
    let width = frame.area().width as usize;
    if full_screen && frame.area().width >= 50 && frame.area().height >= 16 {
        render_workspace(frame, state);
        return;
    }
    if let Screen::Launcher(launcher) = &state.screen {
        render_launcher(frame, state, launcher);
        return;
    }
    if let Screen::TrackSelection(selection) = &state.screen {
        render_track_selection(frame, state, selection);
        return;
    }

    if let Screen::Running(running) = &state.screen {
        if full_screen {
            render_running_dashboard(frame, state, running);
            return;
        }
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
        Screen::Launcher(_) => unreachable!("handled above"),
        Screen::Inspecting { progress } => {
            progress_screen_lines("Inspect source", None, progress, prepare_label)
        }
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
        Screen::SourceError(error) => source_error_lines(state, error, width),
    };

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        frame.area(),
    );
}

fn render_workspace(frame: &mut Frame<'_>, state: &AppState) {
    let area = frame.area();
    let regions = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(area);
    let content_width = area.width;

    let version = state
        .version
        .strip_prefix("quickbridge ")
        .unwrap_or(&state.version);
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!("quickbridge v{version}"),
            version_style(),
        )),
        regions[0],
    );

    let source = match &state.screen {
        Screen::Launcher(launcher) if !launcher.input.is_empty() => launcher.input.as_str(),
        _ if !state.source_url.is_empty() => state.source_url.as_str(),
        _ => "Enter a direct media URL below",
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled("Source", section_style()),
            Line::styled(source.to_string(), text_style()),
        ]),
        Rect::new(regions[1].x, regions[1].y, content_width, regions[1].height),
    );

    render_workspace_body(frame, state, regions[2]);
    render_workspace_input(frame, state, regions[3]);
    frame.render_widget(
        Paragraph::new(Line::styled(workspace_footer(state), detail_style())),
        regions[4],
    );

    match &state.screen {
        Screen::Launcher(launcher) if launcher.help_open => render_overlay(
            frame,
            " Help ",
            &[
                "Enter a direct http:// or https:// media URL.",
                "Enter  Inspect source",
                "Ctrl+C  Quit",
                "Esc or F1  Close",
            ],
        ),
        Screen::Running(running) if running.help_open => {
            let help = help_text();
            let mut lines = help.lines().collect::<Vec<_>>();
            lines.push("");
            lines.push("Esc  Close");
            render_overlay(frame, " Help ", &lines);
        }
        Screen::Running(running) if running.details.is_some() => {
            let lines = running
                .details
                .as_deref()
                .unwrap_or_default()
                .lines()
                .collect::<Vec<_>>();
            render_overlay(frame, " Session details ", &lines);
        }
        _ => {}
    }
}

fn render_workspace_body(frame: &mut Frame<'_>, state: &AppState, area: Rect) {
    match &state.screen {
        Screen::Launcher(_) => {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Open media in QuickTime Player", label_style()),
                    Line::styled(
                        "Enter a direct http:// or https:// media URL below.",
                        detail_style(),
                    ),
                ])
                .block(rounded_block(" Session ")),
                top_rect(area, 5),
            );
        }
        Screen::Inspecting { progress } => {
            let lines = progress_flow_lines(progress, prepare_label);
            render_activity_card(frame, area, lines, 0);
        }
        Screen::TrackSelection(selection) => {
            let cards_height = track_cards_height(selection, area.height);
            let gap = u16::from(area.height >= cards_height.saturating_add(4));
            let regions = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(gap),
                Constraint::Length(cards_height),
            ])
            .split(area);
            render_activity_card(
                frame,
                regions[0],
                state.prepare_history.iter().map(history_line).collect(),
                0,
            );
            render_track_cards(frame, regions[2], selection);
        }
        Screen::Starting {
            selection_title,
            progress,
            prepare_history,
        } => render_starting_workspace(frame, area, selection_title, prepare_history, progress),
        Screen::Running(running) => render_running_workspace(frame, area, running),
        Screen::SourceError(error) => {
            let lines = vec![
                Line::styled(format!("! {}", error.summary), warning_style()),
                Line::styled(error.diagnostic.clone(), detail_style()),
            ];
            frame.render_widget(
                Paragraph::new(lines).block(rounded_block(" Couldn't open this source ")),
                top_rect(area, 4),
            );
        }
    }
}

fn render_workspace_input(frame: &mut Frame<'_>, state: &AppState, area: Rect) {
    let line = match &state.screen {
        Screen::Launcher(launcher) => launcher_editor_line(launcher, area.width),
        Screen::Inspecting { .. } => Line::styled("Inspecting source…", detail_style()),
        Screen::TrackSelection(_) => Line::styled(
            "Use ↑ or ↓ to select a track, then press Enter",
            detail_style(),
        ),
        Screen::Starting { .. } => Line::styled("Starting QuickTime Player…", detail_style()),
        Screen::Running(running) => {
            command_input_line(running, area.width.saturating_sub(2) as usize)
        }
        Screen::SourceError(error) if error.editing => Line::from(vec![
            Span::styled("> ", prompt_prefix_style()),
            Span::styled(error.input.clone(), prompt_text_style()),
        ]),
        Screen::SourceError(_) => {
            Line::styled("R try again  •  E edit URL  •  Q quit", detail_style())
        }
    };
    frame.render_widget(
        Paragraph::new(line).block(
            rounded_block(" Input ").border_style(Style::default().add_modifier(Modifier::BOLD)),
        ),
        area,
    );
}

fn workspace_footer(state: &AppState) -> &'static str {
    match &state.screen {
        Screen::Launcher(_) => "Enter inspect source  •  F1 help  •  Ctrl+C quit",
        Screen::Inspecting { .. } => "Ctrl+C cancel",
        Screen::TrackSelection(selection) => track_selection_hint(selection),
        Screen::Starting { .. } => "Ctrl+C cancel",
        Screen::Running(_) => {
            "Enter run  •  PgUp/PgDn activity  •  Esc clear/close  •  Ctrl+C quit"
        }
        Screen::SourceError(error) if error.editing => "Enter try again  •  Esc cancel",
        Screen::SourceError(_) => "R try again  •  E edit URL  •  Q quit",
    }
}

fn top_rect(area: Rect, height: u16) -> Rect {
    Rect::new(area.x, area.y, area.width, area.height.min(height))
}

fn render_launcher(frame: &mut Frame<'_>, state: &AppState, launcher: &LauncherState) {
    let area = frame.area();
    let content = Layout::vertical([Constraint::Length(7), Constraint::Min(0)])
        .margin(1)
        .split(area)[0];
    let message = launcher
        .history
        .last()
        .map(history_line)
        .unwrap_or_else(|| {
            Line::styled(
                "Enter a direct http:// or https:// media URL",
                detail_style(),
            )
        });
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(state.version.clone(), version_style()),
            Line::styled("Open media in QuickTime Player", detail_style()),
            Line::raw(""),
            message,
            launcher_editor_line(launcher, content.width),
            Line::raw(""),
            Line::styled(
                "Enter inspect source  •  F1 help  •  Ctrl+C quit",
                detail_style(),
            ),
        ]),
        content,
    );
    if launcher.help_open {
        render_overlay(
            frame,
            " Launcher help ",
            &[
                "Enter a direct http:// or https:// media URL.",
                "Enter  Inspect source",
                "Ctrl+C  Quit",
                "Esc or F1  Close",
            ],
        );
    }
}

fn rounded_block(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
}

fn launcher_editor_line(launcher: &LauncherState, width: u16) -> Line<'static> {
    if launcher.input.is_empty() {
        return Line::from(vec![
            Span::styled("> ", prompt_prefix_style()),
            Span::styled("▏https://example.com/movie.mkv", prompt_placeholder_style()),
        ]);
    }
    let caret_position = launcher.input[..launcher.cursor].chars().count();
    let mut displayed = launcher.input.clone();
    displayed.insert(launcher.cursor, '▏');
    let available = usize::from(width.saturating_sub(5)).max(1);
    let start = caret_position.saturating_sub(available.saturating_sub(1));
    let visible = displayed
        .chars()
        .skip(start)
        .take(available)
        .collect::<String>();
    Line::from(vec![
        Span::styled(if start > 0 { "< " } else { "> " }, prompt_prefix_style()),
        Span::styled(visible, prompt_text_style()),
    ])
}

fn source_error_lines(
    state: &AppState,
    error: &SourceErrorState,
    _width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(state.version.clone(), version_style()),
        Line::raw(""),
        section_title("Unable to open source"),
        Line::styled(format!("! {}", error.summary), warning_style()),
        Line::styled(error.attempted_url.clone(), detail_style()),
    ];
    if error.editing {
        lines.push(Line::raw(""));
        lines.push(Line::styled("Edit URL", label_style()));
        lines.push(Line::from(vec![
            Span::styled("> ", prompt_prefix_style()),
            Span::styled(error.input.clone(), prompt_text_style()),
        ]));
        lines.push(Line::styled(
            "Enter try again  •  Esc cancel",
            detail_style(),
        ));
    } else {
        lines.push(Line::raw(""));
        lines.push(Line::styled(error.diagnostic.clone(), detail_style()));
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "R try again  •  E edit URL  •  Q quit",
            detail_style(),
        ));
    }
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

fn render_track_selection(
    frame: &mut Frame<'_>,
    state: &AppState,
    selection: &TrackSelectionState,
) {
    let area = frame.area();
    if area.width < 30 || area.height < 10 {
        frame.render_widget(
            Paragraph::new(vec![
                section_title("Terminal too small"),
                Line::styled("Resize to at least 30×10.", detail_style()),
            ]),
            area,
        );
        return;
    }

    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("quickbridge", section_style()),
                Span::styled("  •  Select tracks", detail_style()),
            ]),
            Line::styled("Choose video, then audio.", detail_style()),
        ]),
        regions[0],
    );

    render_track_cards(frame, regions[1], selection);
    frame.render_widget(
        Paragraph::new(Line::styled(
            track_selection_hint(selection),
            detail_style(),
        )),
        regions[2],
    );

    let _ = state;
}

fn render_track_cards(frame: &mut Frame<'_>, area: Rect, selection: &TrackSelectionState) -> u16 {
    let cards = area;
    if selection.request.videos().len() > 1 {
        let video_height = (selection.request.videos().len() as u16)
            .saturating_add(1)
            .min(cards.height.saturating_sub(4))
            .max(3);
        let audio_height = (selection.request.audios().len().max(1) as u16)
            .saturating_add(1)
            .min(cards.height.saturating_sub(video_height).saturating_sub(1))
            .max(3);
        let card_regions = Layout::vertical([
            Constraint::Length(video_height),
            Constraint::Length(1),
            Constraint::Length(audio_height),
            Constraint::Min(0),
        ])
        .split(cards);
        let (video_lines, video_focus) = video_choice_lines(selection);
        render_track_card(
            frame,
            card_regions[0],
            "Video",
            selection.focus == super::app::SelectionFocus::Video,
            video_lines,
            video_focus,
        );
        let (audio_lines, audio_focus) = audio_choice_lines(selection, cards.width as usize);
        render_track_card(
            frame,
            card_regions[2],
            "Audio",
            selection.focus == super::app::SelectionFocus::Audio,
            audio_lines,
            audio_focus,
        );
        video_height.saturating_add(1).saturating_add(audio_height)
    } else {
        let (audio_lines, audio_focus) = audio_choice_lines(selection, cards.width as usize);
        let audio_area = Rect::new(
            cards.x,
            cards.y,
            cards.width,
            (selection.request.audios().len().max(1) as u16)
                .saturating_add(1)
                .min(cards.height),
        );
        render_track_card(frame, audio_area, "Audio", true, audio_lines, audio_focus);
        audio_area.height
    }
}

fn track_cards_height(selection: &TrackSelectionState, available: u16) -> u16 {
    if selection.request.videos().len() <= 1 {
        return (selection.request.audios().len().max(1) as u16)
            .saturating_add(1)
            .min(available);
    }

    let video = (selection.request.videos().len() as u16)
        .saturating_add(1)
        .min(available.saturating_sub(4))
        .max(3);
    let audio = (selection.request.audios().len().max(1) as u16)
        .saturating_add(1)
        .min(available.saturating_sub(video).saturating_sub(1))
        .max(3);
    video.saturating_add(1).saturating_add(audio).min(available)
}

fn track_selection_hint(selection: &TrackSelectionState) -> &'static str {
    match selection.focus {
        super::app::SelectionFocus::Video if !selection.request.audios().is_empty() => {
            "↑↓ choose  •  Enter audio"
        }
        _ => "↑↓ choose  •  Enter play  •  Tab switch",
    }
}

fn video_choice_lines(selection: &TrackSelectionState) -> (Vec<Line<'static>>, usize) {
    let mut lines = Vec::new();
    let mut focused_line = 0;
    for (index, stream) in selection.request.videos().iter().enumerate() {
        let active =
            selection.focus == super::app::SelectionFocus::Video && index == selection.video_index;
        if active {
            focused_line = lines.len();
        }
        lines.push(selectable_line(
            active,
            index == selection.video_index,
            compact_video_label(stream),
        ));
    }
    (lines, focused_line)
}

fn audio_choice_lines(
    selection: &TrackSelectionState,
    width: usize,
) -> (Vec<Line<'static>>, usize) {
    let mut lines = Vec::new();
    let mut focused_line = selection.audio_index.unwrap_or_default();
    if selection.request.audios().is_empty() {
        lines.push(Line::styled("No audio track available", detail_style()));
    } else {
        for (index, stream) in selection.request.audios().iter().enumerate() {
            let active = selection.focus == super::app::SelectionFocus::Audio
                && selection.audio_index == Some(index);
            if active {
                focused_line = lines.len();
            }
            lines.push(selectable_line(
                active,
                selection.audio_index == Some(index),
                compact_audio_label(stream, width.saturating_sub(4)),
            ));
        }
    }
    (lines, focused_line)
}

fn render_track_card(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    focused: bool,
    lines: Vec<Line<'static>>,
    focused_line: usize,
) {
    let viewport_height = usize::from(area.height.saturating_sub(1)).max(1);
    let scroll = focused_line.saturating_sub(viewport_height.saturating_sub(1));
    let regions = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    frame.render_widget(
        Paragraph::new(Line::styled(
            title.to_string(),
            if focused {
                label_style()
            } else {
                section_style()
            },
        )),
        regions[0],
    );
    frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), regions[1]);
}

fn render_starting_workspace(
    frame: &mut Frame<'_>,
    area: Rect,
    selection_title: &str,
    prepare_history: &[HistoryEntry],
    progress: &ProgressModel<LaunchStep>,
) {
    let selection_lines = selection_title
        .lines()
        .skip(1)
        .map(|line| Line::styled(line.to_string(), text_style()))
        .collect::<Vec<_>>();
    let selection_height = (selection_lines.len() as u16)
        .saturating_add(2)
        .clamp(3, area.height);
    let gap = u16::from(area.height >= selection_height.saturating_add(4));
    let regions = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(gap),
        Constraint::Length(selection_height),
    ])
    .split(area);
    let mut activity = prepare_history.iter().map(history_line).collect::<Vec<_>>();
    activity.extend(progress_flow_lines(progress, launch_label));
    render_activity_card(frame, regions[0], activity, 0);
    frame.render_widget(
        Paragraph::new(section_lines("Tracks", selection_lines)),
        regions[2],
    );
}

fn render_running_workspace(frame: &mut Frame<'_>, area: Rect, running: &RunningState) {
    let player_lines = player_card_lines(running, area.width.saturating_sub(2) as usize);
    let player_height = (player_lines.len() as u16).saturating_add(1);
    let gap = u16::from(area.height >= player_height.saturating_add(4).saturating_add(5));
    let regions = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(gap),
        Constraint::Length(4),
        Constraint::Length(gap),
        Constraint::Length(player_height),
    ])
    .split(area);
    let mut activity = running
        .prepare_history
        .iter()
        .chain(&running.startup_history)
        .chain(&running.history)
        .map(history_line)
        .collect::<Vec<_>>();
    if let Some(progress) = &running.jump_progress {
        activity.extend(progress_flow_lines(progress, jump_label));
    }
    if let Some(warning) = live_warning_line(running) {
        activity.push(Line::styled(warning, warning_style()));
    }
    render_activity_card(frame, regions[0], activity, running.activity_scroll);
    frame.render_widget(
        Paragraph::new(section_lines(
            "Tracks",
            selected_track_lines(running, area.width as usize),
        )),
        regions[2],
    );
    frame.render_widget(
        Paragraph::new(section_lines("Player", player_lines)),
        regions[4],
    );
}

fn render_activity_card(
    frame: &mut Frame<'_>,
    area: Rect,
    mut lines: Vec<Line<'static>>,
    scroll_from_bottom: usize,
) {
    if area.height < 3 || area.width < 4 {
        return;
    }
    if lines.is_empty() {
        lines.push(Line::styled("No activity yet.", detail_style()));
    }
    let viewport_height = usize::from(area.height.saturating_sub(1)).max(1);
    let max_scroll = lines.len().saturating_sub(viewport_height);
    let scroll = max_scroll.saturating_sub(scroll_from_bottom.min(max_scroll));
    frame.render_widget(
        Paragraph::new(section_lines("Activity", lines))
            .scroll((scroll as u16, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn section_lines(title: &'static str, lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let mut section = Vec::with_capacity(lines.len() + 1);
    section.push(Line::styled(title, section_style()));
    section.extend(lines);
    section
}

fn running_lines(state: &AppState, running: &RunningState, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("quickbridge", section_style()),
        Span::styled(
            format!("  {}", running.playback_mode.label()),
            detail_style(),
        ),
    ])];
    lines.extend(selected_track_lines(running, width));
    lines.extend(health_lines(running, width));

    if let Some(warning) = live_warning_line(running) {
        lines.push(Line::styled(warning, warning_style()));
    }

    if let Some(progress) = &running.jump_progress {
        lines.push(Line::raw(""));
        lines.extend(progress_flow_lines(progress, jump_label));
    }

    lines.push(Line::raw(""));
    lines.push(command_input_line(running, width));
    lines.extend(running_controls(running));
    let _ = state;
    lines
}

fn render_running_dashboard(frame: &mut Frame<'_>, state: &AppState, running: &RunningState) {
    let area = frame.area();
    frame.render_widget(
        Paragraph::new(running_lines(state, running, area.width as usize))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn running_controls(running: &RunningState) -> Vec<Line<'static>> {
    let _ = running;
    vec![Line::styled(
        "Enter run  •  Esc clear/close  •  Ctrl+C quit",
        detail_style(),
    )]
}

fn selected_track_lines(running: &RunningState, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled("Video  ", label_style()),
        Span::styled(
            truncate_label(
                &compact_video_label(running.selection.selected_video()),
                width.saturating_sub(7),
            ),
            text_style(),
        ),
    ])];
    lines.push(Line::from(vec![
        Span::styled("Audio  ", label_style()),
        Span::styled(
            running
                .selection
                .selected_audio()
                .map(|audio| compact_audio_label(audio, width.saturating_sub(7)))
                .unwrap_or_else(|| "No audio".to_string()),
            text_style(),
        ),
    ]));
    lines
}

fn command_input_line(running: &RunningState, width: usize) -> Line<'static> {
    if running.input.is_empty() {
        return Line::from(vec![
            Span::styled("> ", prompt_prefix_style()),
            Span::styled(
                "Enter a time or command. Type help for options",
                prompt_placeholder_style(),
            ),
        ]);
    }

    let caret_position = running.input[..running.input_cursor].chars().count();
    let mut displayed = running.input.clone();
    displayed.insert(running.input_cursor, '▏');
    let available = width.saturating_sub(2).max(1);
    let start = caret_position.saturating_sub(available.saturating_sub(1));
    let visible = displayed
        .chars()
        .skip(start)
        .take(available)
        .collect::<String>();
    Line::from(vec![
        Span::styled(if start > 0 { "< " } else { "> " }, prompt_prefix_style()),
        Span::styled(visible, prompt_text_style()),
    ])
}

fn player_card_lines(running: &RunningState, width: usize) -> Vec<Line<'static>> {
    let time = format_playback_time(
        running.snapshot.current_time(),
        running.media_info.duration(),
    );
    let mut lines = vec![Line::styled(time, text_style())];
    let telemetry = running.snapshot.telemetry();
    if width >= 50 {
        lines.push(Line::styled(
            format!(
                "Buffer {}    Relay {}    Storage {}",
                telemetry.buffer_ahead(),
                format_bytes_per_second(telemetry.relay_write_bytes_per_second()),
                format_bytes(telemetry.storage_bytes())
            ),
            detail_style(),
        ));
    } else {
        lines.push(Line::styled(
            format!("Buffer {}", telemetry.buffer_ahead()),
            detail_style(),
        ));
        lines.push(Line::styled(
            format!(
                "Relay {}    Storage {}",
                format_bytes_per_second(telemetry.relay_write_bytes_per_second()),
                format_bytes(telemetry.storage_bytes())
            ),
            detail_style(),
        ));
    }
    lines
}

fn render_overlay(frame: &mut Frame<'_>, title: &str, lines: &[&str]) {
    let area = frame.area();
    let content_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or_default() as u16;
    let width = content_width
        .saturating_add(4)
        .max(36)
        .min(area.width.saturating_sub(4));
    let height = (lines.len() as u16)
        .saturating_add(2)
        .min(area.height.saturating_sub(4));
    let popup = centered_rect(area, width, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(
            lines
                .iter()
                .map(|line| Line::styled((*line).to_string(), text_style()))
                .collect::<Vec<_>>(),
        )
        .block(rounded_block(title))
        .wrap(Wrap { trim: true }),
        popup,
    );
}

fn centered_rect(area: ratatui::layout::Rect, width: u16, height: u16) -> ratatui::layout::Rect {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .split(vertical[1])[1]
}

fn health_lines(running: &RunningState, width: usize) -> Vec<Line<'static>> {
    let (symbol, player) = player_state_label(running.snapshot.player_state());
    let fields = [
        format!("Player  {symbol} {player}"),
        format!(
            "Time    {}",
            format_playback_time(
                running.snapshot.current_time(),
                running.media_info.duration()
            )
        ),
        format!("Buffer  {}", running.snapshot.telemetry().buffer_ahead()),
        format!(
            "Relay   {}",
            format_bytes_per_second(running.snapshot.telemetry().relay_write_bytes_per_second())
        ),
        format!(
            "Storage {}",
            format_bytes(running.snapshot.telemetry().storage_bytes())
        ),
    ];
    if width >= 80 {
        vec![
            Line::styled(fields[..2].join("    "), text_style()),
            Line::styled(fields[2..].join("    "), text_style()),
        ]
    } else {
        fields
            .into_iter()
            .map(|field| Line::styled(field, text_style()))
            .collect()
    }
}

fn player_state_label(state: PlayerState) -> (&'static str, &'static str) {
    match state {
        PlayerState::Playing => ("▶", "Playing"),
        PlayerState::Paused => ("Ⅱ", "Paused"),
        PlayerState::WindowClosed => ("!", "Window closed"),
        PlayerState::AppClosed => ("!", "App closed"),
        PlayerState::Unavailable => ("?", "Unavailable"),
    }
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

fn bottom_scroll_offset(lines: &[Line<'static>], width: usize, viewport_height: usize) -> usize {
    if viewport_height == 0 {
        return 0;
    }

    visual_line_count(lines, width).saturating_sub(viewport_height)
}

fn visual_line_count(lines: &[Line<'static>], width: usize) -> usize {
    let width = width.max(1);
    lines
        .iter()
        .map(|line| visual_line_height(line, width))
        .sum()
}

fn visual_line_height(line: &Line<'static>, width: usize) -> usize {
    let content_width = line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();

    content_width.max(1).div_ceil(width)
}

fn selectable_line(active: bool, selected: bool, text: String) -> Line<'static> {
    if active {
        Line::from(vec![
            Span::styled("› ", selected_line_style()),
            Span::styled(text, selected_line_style()),
        ])
    } else if selected {
        Line::from(vec![
            Span::styled("✓ ", success_style()),
            Span::styled(text, text_style()),
        ])
    } else {
        Line::from(vec![Span::raw("  "), Span::styled(text, text_style())])
    }
}

fn compact_video_label(stream: &VideoStream) -> String {
    let codec = match stream.codec_name() {
        Some("h264") => "H.264".to_string(),
        Some("hevc") => "HEVC".to_string(),
        Some(codec) => codec.to_ascii_uppercase(),
        None => "Unknown codec".to_string(),
    };
    let resolution = stream
        .dimensions()
        .map(|(width, height)| format!("{width}×{height}"))
        .unwrap_or_else(|| "Unknown resolution".to_string());
    let dynamic_range = if let Some(config) = stream.dolby_vision() {
        format!("Dolby Vision P{}", config.profile())
    } else {
        match stream.color_transfer() {
            Some("smpte2084") => "HDR10",
            Some("arib-std-b67") => "HLG",
            _ => "SDR",
        }
        .to_string()
    };
    format!(
        "{codec} • {resolution} • {dynamic_range}{}",
        if stream.is_default() {
            " • default"
        } else {
            ""
        }
    )
}

fn compact_audio_label(stream: &AudioStream, width: usize) -> String {
    let identity = stream
        .title()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| stream.language().filter(|language| !language.is_empty()))
        .unwrap_or("Unknown language");
    let codec = match stream.codec_name.as_deref() {
        Some("truehd") => "TrueHD",
        Some("eac3") => "E-AC-3",
        Some("ac3") => "AC-3",
        Some("aac") => "AAC",
        Some("dts") => "DTS",
        Some(codec) => codec,
        None => "Unknown codec",
    };
    let handling = if matches!(stream.codec_name.as_deref(), Some("truehd" | "dts")) {
        format!("{codec} → ALAC")
    } else {
        codec.to_string()
    };
    let channels = stream
        .channel_layout()
        .map(str::to_string)
        .or_else(|| stream.channels().map(|channels| format!("{channels} ch")));
    let mut parts = vec![identity.to_string(), handling];
    if let Some(channels) = channels {
        parts.push(channels);
    }
    if stream.is_atmos() {
        parts.push("Atmos".to_string());
    }
    if stream.is_default() {
        parts.push("default".to_string());
    }
    truncate_label(&parts.join(" • "), width)
}

fn truncate_label(label: &str, width: usize) -> String {
    if label.chars().count() <= width {
        return label.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let mut truncated = label.chars().take(width - 1).collect::<String>();
    truncated.push('…');
    truncated
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

fn live_warning_line(running: &RunningState) -> Option<String> {
    if let Some(error) = &running.player_action_error {
        return Some(format!("[WARN] {error}"));
    }
    match running.snapshot.player_state() {
        PlayerState::WindowClosed => Some(String::from(
            "[WARN] QuickTime Player window is closed — type `reopen`.",
        )),
        PlayerState::AppClosed => Some(String::from(
            "[WARN] QuickTime Player is closed — type `reopen`.",
        )),
        PlayerState::Unavailable => Some(String::from(
            "[WARN] QuickTime Player status isn't available right now.",
        )),
        PlayerState::Playing | PlayerState::Paused => None,
    }
}

fn version_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn section_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

fn label_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn text_style() -> Style {
    Style::default()
}

fn detail_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

fn accent_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

fn selected_line_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

fn success_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn warning_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

fn prompt_background_style() -> Style {
    Style::default()
}

fn prompt_prefix_style() -> Style {
    prompt_background_style().add_modifier(Modifier::BOLD)
}

fn prompt_text_style() -> Style {
    prompt_background_style()
}

fn prompt_placeholder_style() -> Style {
    prompt_background_style().add_modifier(Modifier::DIM)
}

#[cfg(test)]
mod tests {
    use super::{prompt_prefix_style, render};
    use crate::{
        app::{
            AppState, HistoryEntry, HistoryTone, LauncherState, ProgressModel, RunningState,
            Screen, SelectionFocus, SourceErrorState, StartupContext, TrackSelectionState,
        },
        test_backend::VT100Backend,
    };
    use insta::assert_snapshot;
    use quickbridge_core::{
        AudioStream, JumpStep, LaunchStep, MediaInfo, PlaybackMode, PlaybackSnapshot, PlayerState,
        PrepareStep, SeekSupport, SimulationScenario, SourceInspection, SourceMetadata,
        StreamSelection, StreamTelemetry, Timecode, VideoStream,
    };
    use ratatui::{Terminal, style::Modifier};

    fn render_contents(state: &AppState) -> String {
        render_contents_with_size(state, 120, 40)
    }

    fn render_contents_with_size(state: &AppState, width: u16, height: u16) -> String {
        let backend = VT100Backend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, state, true)).unwrap();
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
        assert!(contents.contains("Inspecting source"));
        assert!(contents.contains("✓ Checked source URL"));
        assert!(contents.contains("Checking whether time jumps are available"));
        assert_snapshot!("inspection_screen_120x40", contents);
    }

    #[test]
    fn launcher_screen_renders_clean_entrypoint() {
        let mut state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::new(),
            prepare_history: Vec::new(),
            screen: Screen::Launcher(LauncherState {
                input: String::new(),
                cursor: 0,
                help_open: false,
                history: Vec::new(),
            }),
        };

        let contents = render_contents(&state);
        assert!(contents.contains("Enter a direct http:// or https:// media URL"));
        assert!(contents.contains("> ▏https://example.com/movie.mkv"));
        assert!(contents.contains("Enter inspect source"));
        assert_snapshot!("launcher_screen_120x40", contents);

        let Screen::Launcher(launcher) = &mut state.screen else {
            unreachable!();
        };
        launcher.help_open = true;
        let help = render_contents_with_size(&state, 80, 24);
        assert!(help.contains("Help"));
        assert!(help.contains("Esc or F1  Close"));
        assert_snapshot!("launcher_help_80x24", help);
    }

    #[test]
    fn source_error_screen_renders_recovery_actions_at_supported_sizes() {
        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::from("https://example.com/broken.mkv"),
            prepare_history: Vec::new(),
            screen: Screen::SourceError(SourceErrorState {
                attempted_url: String::from("https://example.com/broken.mkv"),
                summary: String::from("ffprobe couldn't inspect this source."),
                diagnostic: String::from("unable to inspect the source with ffprobe: denied"),
                input: String::from("https://example.com/broken.mkv"),
                editing: false,
            }),
        };

        for (width, height) in [(80, 24), (60, 18)] {
            let contents = render_contents_with_size(&state, width, height);
            assert!(contents.contains("Couldn't open this source"));
            assert!(contents.contains("ffprobe couldn't inspect this source"));
            assert!(contents.contains("R try again  •  E edit URL  •  Q quit"));
            assert!(contents.contains("https://example.com/broken.mkv"));
            assert_snapshot!(format!("source_error_screen_{width}x{height}"), contents);
        }
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
        let mut state = AppState {
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
            }),
        };

        let contents = render_contents_with_size(&state, 80, 24);
        assert!(contents.contains("quickbridge v0.1.0"));
        assert!(contents.contains("› H.264 • Unknown resolution • SDR"));
        assert_eq!(contents.matches('›').count(), 1);
        assert!(contents.contains("✓ Unknown language • AAC • default"));
        assert!(contents.contains("↑↓ choose  •  Enter audio"));
        assert_snapshot!("track_selection_screen_80x24", contents);
        assert_snapshot!(
            "track_selection_screen_120x40",
            render_contents_with_size(&state, 120, 40)
        );

        let Screen::TrackSelection(selection) = &mut state.screen else {
            unreachable!();
        };
        selection.focus = SelectionFocus::Audio;
        let contents = render_contents_with_size(&state, 60, 18);
        assert_eq!(contents.matches('›').count(), 1);
        assert!(contents.contains("✓ H.264 • Unknown resolution • SDR"));
        assert!(contents.contains("↑↓ choose  •  Enter play  •  Tab switch"));
        assert_snapshot!("track_selection_screen_60x18", contents);
    }

    #[test]
    fn track_selection_renders_structured_hdr_and_audio_conversion() {
        let media_info = MediaInfo::from_ffprobe_json(
            r#"{"streams":[
              {"index":0,"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,"color_transfer":"smpte2084","disposition":{"default":1}},
              {"index":1,"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,"side_data_list":[{"side_data_type":"DOVI configuration record","dv_profile":5,"dv_level":6,"bl_present_flag":1,"el_present_flag":0,"dv_bl_signal_compatibility_id":0}]},
              {"index":2,"codec_type":"audio","codec_name":"truehd","profile":"Dolby TrueHD + Dolby Atmos","channel_layout":"7.1","tags":{"language":"eng","title":"English theatrical mix"},"disposition":{"default":1}}
            ]}"#,
        )
        .unwrap();
        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::new(),
            prepare_history: Vec::new(),
            screen: Screen::TrackSelection(TrackSelectionState {
                request: media_info.selection_request().unwrap(),
                focus: SelectionFocus::Video,
                video_index: 0,
                audio_index: Some(0),
            }),
        };

        let contents = render_contents_with_size(&state, 80, 24);
        assert!(contents.contains("HEVC • 3840×2160 • HDR10 • default"));
        assert!(contents.contains("HEVC • 3840×2160 • Dolby Vision P5"));
        assert!(contents.contains("TrueHD → ALAC • 7.1 • Atmos • default"));
    }

    #[test]
    fn track_selection_scrolls_long_lists_and_handles_tiny_terminals() {
        let media_info = MediaInfo::new(
            vec![VideoStream::new(0, "video", true)],
            (0..18)
                .map(|index| {
                    AudioStream::new(
                        index + 1,
                        Some(String::from("aac")),
                        format!("audio {index}"),
                        index == 0,
                    )
                })
                .collect(),
            None,
        );
        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::new(),
            prepare_history: Vec::new(),
            screen: Screen::TrackSelection(TrackSelectionState {
                request: media_info.selection_request().unwrap(),
                focus: SelectionFocus::Audio,
                video_index: 0,
                audio_index: Some(17),
            }),
        };

        let contents = render_contents_with_size(&state, 120, 40);
        assert!(contents.contains("› Unknown language • AAC"));
        assert!(contents.contains("↑↓ choose  •  Enter play  •  Tab switch"));
        assert_snapshot!("track_selection_long_list_120x40", contents);

        let tiny = render_contents_with_size(&state, 20, 6);
        assert!(tiny.contains("Terminal too small"));
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
                StreamTelemetry::new(3 * 1024 * 1024, 12 * 1024 * 1024, Timecode::from_seconds(6)),
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
        running.input_cursor = running.input.len();
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
        let mut state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::from("https://example.com/video.mkv"),
            prepare_history: Vec::new(),
            screen: Screen::Running(Box::new(running)),
        };

        let contents = render_contents(&state);
        assert!(contents.contains("quickbridge v0.1.0"));
        assert!(!contents.contains("Simulation"));
        assert!(contents.contains("00:00:12 / 00:02:00"));
        assert!(contents.contains("Relay 3.0 MB/s"));
        assert!(contents.contains("Buffer 00:00:06"));
        assert!(contents.contains("Storage 12.0 MB"));
        assert!(!contents.contains("▶ Playing"));
        assert!(contents.contains("Refreshing QuickTime Player"));
        assert!(contents.contains("Activity"));
        assert!(contents.contains("✓ Checked source URL"));
        assert!(contents.contains("✓ Started local stream server"));
        assert!(contents.contains("Input"));
        assert!(contents.contains("> status"));
        let row = |marker: &str| {
            contents
                .lines()
                .position(|line| line.contains(marker))
                .unwrap()
        };
        let section_row = |title: &str| {
            contents
                .lines()
                .position(|line| line.trim() == title)
                .unwrap()
        };
        assert!(section_row("Activity") < section_row("Tracks"));
        assert!(section_row("Tracks") < section_row("Player"));
        assert!(section_row("Player") < row("╭ Input"));
        assert_snapshot!("running_screen_120x40", contents);
        assert_snapshot!(
            "running_screen_131x36",
            render_contents_with_size(&state, 131, 36)
        );

        let Screen::Running(running) = &mut state.screen else {
            unreachable!();
        };
        running.snapshot = PlaybackSnapshot::new(
            1,
            String::from("http://127.0.0.1:1234/stream.m3u8?session=1"),
            Timecode::ZERO,
            Timecode::from_seconds(12),
            PlayerState::Paused,
            StreamTelemetry::new(3 * 1024 * 1024, 12 * 1024 * 1024, Timecode::from_seconds(6)),
        );
        let paused = render_contents_with_size(&state, 80, 24);
        assert!(!paused.contains("Ⅱ Paused"));
        assert!(paused.contains("00:00:12 / 00:02:00"));

        let Screen::Running(running) = &mut state.screen else {
            unreachable!();
        };
        running.help_open = true;
        let help = render_contents_with_size(&state, 80, 24);
        assert!(help.contains("HH:MM:SS"));
        assert!(help.contains("Esc  Close"));
        assert_snapshot!("running_help_80x24", help);

        let Screen::Running(running) = &mut state.screen else {
            unreachable!();
        };
        running.help_open = false;
        running.details = Some(String::from(
            "Source             | https://example.com/video.mkv\nSession ID         | 1\nTracks\n  HEVC HDR10\n  TrueHD → ALAC",
        ));
        let details = render_contents_with_size(&state, 120, 40);
        assert!(details.contains("Session details"));
        assert!(details.contains("TrueHD → ALAC"));
        assert_snapshot!("running_details_120x40", details);
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
                StreamTelemetry::new(3 * 1024 * 1024, 12 * 1024 * 1024, Timecode::from_seconds(6)),
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
        running.input_cursor = running.input.len();

        let state = AppState {
            version: String::from("quickbridge 0.1.0"),
            source_url: String::from("https://example.com/video.mkv"),
            prepare_history: Vec::new(),
            screen: Screen::Running(Box::new(running)),
        };

        let contents = render_contents_with_size(&state, 80, 24);
        assert!(contents.contains("Input"));
        assert!(contents.contains("> reopen"));
        assert!(contents.contains("quickbridge"));
        assert!(!contents.contains("Live"));
        assert!(contents.contains("type `reopen`"));
        assert_snapshot!("running_screen_closed_80x24", contents);

        let compact = render_contents_with_size(&state, 60, 18);
        assert!(compact.contains("00:00:12 / 00:02:00"));
        assert!(compact.contains("> reopen"));
        assert_snapshot!("running_screen_closed_60x18", compact);
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
                selection_title: String::from(
                    "video.mkv\nHEVC • 3840×2160 • HDR10\nEnglish • E-AC-3 • 5.1",
                ),
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
        assert!(contents.contains("quickbridge v0.1.0"));
        assert!(contents.contains("HEVC • 3840×2160 • HDR10"));
        assert!(contents.contains("✓ Started local stream server"));
        assert!(contents.contains("Starting ffmpeg relay"));
        assert_snapshot!("startup_screen_120x40", contents);
    }

    #[test]
    fn full_screen_workspace_keeps_shell_regions_fixed_across_phases() {
        let launcher = render_contents_with_size(&AppState::new(None), 80, 24);
        let inspecting = render_contents_with_size(
            &AppState::new(Some(String::from("https://example.com/video.mkv"))),
            80,
            24,
        );
        let row = |contents: &str, marker: &str| {
            contents
                .lines()
                .position(|line| line.contains(marker))
                .expect("workspace region should be visible")
        };

        assert_eq!(row(&launcher, "Source"), row(&inspecting, "Source"));
        assert_eq!(row(&launcher, "╭ Input"), row(&inspecting, "╭ Input"));
        assert_eq!(
            launcher
                .lines()
                .find(|line| line.contains("Source"))
                .unwrap()
                .chars()
                .count(),
            80
        );
        assert_eq!(
            launcher
                .lines()
                .find(|line| line.contains("╭ Input"))
                .unwrap()
                .chars()
                .count(),
            80
        );
    }

    #[test]
    fn prompt_prefix_does_not_reverse_the_background() {
        assert!(
            !prompt_prefix_style()
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }
}
