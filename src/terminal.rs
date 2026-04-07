use crate::timecode::Timecode;
use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{MoveToColumn, MoveUp, RestorePosition, SavePosition, Show},
    execute, queue,
    terminal::{self, Clear, ClearType},
};
use std::io::{self, IsTerminal, Stdout, Write};
use tokio::time::{Duration, MissedTickBehavior, interval};

const ACTIVE_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderMode {
    Ansi,
    Plain,
}

fn render_mode() -> RenderMode {
    match std::env::var("QUICKBRIDGE_RENDER_MODE") {
        Ok(value) if value.eq_ignore_ascii_case("plain") => RenderMode::Plain,
        _ => RenderMode::Ansi,
    }
}

pub fn format_playback_time(
    estimated_position: Timecode,
    total_runtime: Option<Timecode>,
) -> String {
    match total_runtime {
        Some(total_runtime) => format!("{estimated_position} / {total_runtime}"),
        None => estimated_position.to_string(),
    }
}

pub fn emphasize(text: &str) -> String {
    match render_mode() {
        RenderMode::Ansi => format!("\x1b[1m{text}\x1b[0m"),
        RenderMode::Plain => text.to_string(),
    }
}

pub fn muted(text: &str) -> String {
    match render_mode() {
        RenderMode::Ansi => format!("\x1b[90m{text}\x1b[0m"),
        RenderMode::Plain => text.to_string(),
    }
}

pub fn format_warning(text: &str) -> String {
    match render_mode() {
        RenderMode::Ansi => format!("\x1b[1;38;5;136m[WARN]\x1b[0m {text}"),
        RenderMode::Plain => format!("[WARN] {text}"),
    }
}

pub fn require_interactive_terminal() -> Result<()> {
    validate_terminal(io::stdin().is_terminal(), io::stdout().is_terminal())
}

pub struct InteractiveScreen;

pub fn enter_interactive_screen() -> Result<InteractiveScreen> {
    Ok(InteractiveScreen)
}

impl Drop for InteractiveScreen {
    fn drop(&mut self) {
        // No cleanup needed since we no longer switch to alternate screen
    }
}

fn validate_terminal(stdin_is_terminal: bool, stdout_is_terminal: bool) -> Result<()> {
    if stdin_is_terminal && stdout_is_terminal {
        Ok(())
    } else {
        bail!(
            "quickbridge requires an interactive terminal. Run it from Terminal, iTerm, or another local shell session"
        )
    }
}

struct BlockRenderer {
    stdout: Stdout,
    rendered_lines: usize,
    anchored: bool,
    mode: RenderMode,
    last_lines: Vec<String>,
}

impl BlockRenderer {
    fn new() -> Self {
        Self {
            stdout: io::stdout(),
            rendered_lines: 0,
            anchored: false,
            mode: render_mode(),
            last_lines: Vec::new(),
        }
    }

    fn replace(&mut self, lines: &[String]) -> Result<()> {
        if self.last_lines == lines && (self.mode == RenderMode::Plain || self.anchored) {
            return Ok(());
        }

        if self.mode == RenderMode::Plain {
            for line in lines {
                write!(self.stdout, "{line}\r\n")?;
            }
            self.stdout.flush()?;
            self.rendered_lines = lines.len();
            self.last_lines = lines.to_vec();
            return Ok(());
        }

        if self.anchored {
            queue!(
                self.stdout,
                RestorePosition,
                MoveToColumn(0),
                Clear(ClearType::FromCursorDown)
            )?;
        } else {
            queue!(self.stdout, MoveToColumn(0), SavePosition)?;
            self.anchored = true;
        }

        for line in lines {
            write!(self.stdout, "{line}\r\n")?;
        }
        self.stdout.flush()?;
        self.rendered_lines = lines.len();
        self.last_lines = lines.to_vec();
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        if self.mode == RenderMode::Plain {
            self.rendered_lines = 0;
            self.anchored = false;
            self.last_lines.clear();
            return Ok(());
        }

        if self.anchored {
            queue!(
                self.stdout,
                RestorePosition,
                MoveToColumn(0),
                Clear(ClearType::FromCursorDown)
            )?;
            self.stdout.flush()?;
        }
        self.rendered_lines = 0;
        self.anchored = false;
        self.last_lines.clear();
        Ok(())
    }

    fn release_anchor(&mut self) {
        self.rendered_lines = 0;
        self.anchored = false;
        self.last_lines.clear();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StepState {
    Pending,
    Active,
    Done,
    Warn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepLabels {
    pending: &'static str,
    active: &'static str,
    done: &'static str,
    warn: &'static str,
}

impl StepLabels {
    pub const fn new(pending: &'static str, active: &'static str, done: &'static str) -> Self {
        Self {
            pending,
            active,
            done,
            warn: done,
        }
    }

    pub const fn with_warn(
        pending: &'static str,
        active: &'static str,
        done: &'static str,
        warn: &'static str,
    ) -> Self {
        Self {
            pending,
            active,
            done,
            warn,
        }
    }

    fn for_state(&self, state: StepState) -> &'static str {
        match state {
            StepState::Pending => self.pending,
            StepState::Active => self.active,
            StepState::Done => self.done,
            StepState::Warn => self.warn,
        }
    }
}

#[derive(Clone, Debug)]
struct StepLine {
    labels: StepLabels,
    state: StepState,
    detail_lines: Vec<String>,
}

impl StepLine {
    fn new(labels: StepLabels) -> Self {
        Self {
            labels,
            state: StepState::Pending,
            detail_lines: Vec::new(),
        }
    }
}

pub struct StageProgress {
    renderer: BlockRenderer,
    title: String,
    steps: Vec<StepLine>,
    active_step: Option<usize>,
    frame_index: usize,
    verbose: bool,
}

impl StageProgress {
    pub fn new(title: impl Into<String>, steps: &[StepLabels], verbose: bool) -> Result<Self> {
        let mut progress = Self {
            renderer: BlockRenderer::new(),
            title: title.into(),
            steps: steps.iter().copied().map(StepLine::new).collect(),
            active_step: None,
            frame_index: 0,
            verbose,
        };
        progress.render()?;
        Ok(progress)
    }

    pub fn activate(&mut self, index: usize, detail_lines: Vec<String>) -> Result<()> {
        if let Some(previous) = self.active_step.take() {
            self.steps[previous].detail_lines.clear();
            if self.steps[previous].state == StepState::Active {
                self.steps[previous].state = StepState::Pending;
            }
        }

        self.steps[index].state = StepState::Active;
        self.steps[index].detail_lines = if self.verbose {
            detail_lines
        } else {
            Vec::new()
        };
        self.active_step = Some(index);
        self.render()
    }

    pub fn complete(&mut self, index: usize) -> Result<()> {
        self.steps[index].state = StepState::Done;
        self.steps[index].detail_lines.clear();
        if self.active_step == Some(index) {
            self.active_step = None;
        }
        self.render()
    }

    pub fn warn(&mut self, index: usize, detail_lines: Vec<String>) -> Result<()> {
        self.steps[index].state = StepState::Warn;
        self.steps[index].detail_lines = if self.verbose {
            detail_lines
        } else {
            Vec::new()
        };
        if self.active_step == Some(index) {
            self.active_step = None;
        }
        self.render()
    }

    pub fn tick(&mut self) -> Result<()> {
        if self.renderer.mode == RenderMode::Plain {
            return Ok(());
        }
        self.frame_index = (self.frame_index + 1) % ACTIVE_FRAMES.len();
        self.render()
    }

    pub fn finish(mut self) -> Result<()> {
        self.active_step = None;
        self.render()?;
        self.renderer.release_anchor();
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        let mut lines = vec![emphasize(&self.title)];
        for step in &self.steps {
            lines.push(match step.state {
                StepState::Pending => format!("○ {}", step.labels.for_state(step.state)),
                StepState::Done => format!("✓ {}", step.labels.for_state(step.state)),
                StepState::Warn => format!("! {}", step.labels.for_state(step.state)),
                StepState::Active => {
                    format!(
                        "{} {}",
                        ACTIVE_FRAMES[self.frame_index],
                        step.labels.for_state(step.state)
                    )
                }
            });
            if self.verbose && !step.detail_lines.is_empty() {
                lines.extend(step.detail_lines.iter().map(|line| format!("      {line}")));
            }
        }
        self.renderer.replace(&lines)
    }
}

pub struct LiveStepProgress {
    renderer: BlockRenderer,
    frame_index: usize,
}

impl LiveStepProgress {
    pub fn new() -> Self {
        Self {
            renderer: BlockRenderer::new(),
            frame_index: 0,
        }
    }

    pub fn show_active(&mut self, label: &str) -> Result<()> {
        self.renderer
            .replace(&[format!("{} {label}", ACTIVE_FRAMES[self.frame_index])])
    }

    pub fn tick(&mut self, label: &str) -> Result<()> {
        if self.renderer.mode == RenderMode::Plain {
            return Ok(());
        }
        self.frame_index = (self.frame_index + 1) % ACTIVE_FRAMES.len();
        self.show_active(label)
    }

    pub fn clear(&mut self) -> Result<()> {
        self.renderer.clear()
    }
}

pub async fn spin_while<T, F>(progress: &mut StageProgress, future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    let mut ticker = interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tokio::pin!(future);

    loop {
        tokio::select! {
            result = &mut future => return result,
            _ = ticker.tick() => progress.tick()?,
        }
    }
}

pub async fn spin_step_while<T, F>(
    progress: &mut LiveStepProgress,
    label: &str,
    future: F,
) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    progress.show_active(label)?;
    let mut ticker = interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tokio::pin!(future);

    loop {
        tokio::select! {
            result = &mut future => return result,
            _ = ticker.tick() => progress.tick(label)?,
        }
    }
}

pub struct LivePrompt {
    stdout: Stdout,
    mode: RenderMode,
    raw_enabled: bool,
    last_plain_state_key: Option<String>,
    last_ansi_lines: Vec<String>,
    rendered_lines: usize,
    cursor_on_input_line: bool,
}

impl LivePrompt {
    pub fn enter() -> Result<Self> {
        let raw_enabled = render_mode() == RenderMode::Ansi;
        if raw_enabled {
            terminal::enable_raw_mode().context("Unable to enable raw terminal mode")?;
        }
        Ok(Self {
            stdout: io::stdout(),
            mode: render_mode(),
            raw_enabled,
            last_plain_state_key: None,
            last_ansi_lines: Vec::new(),
            rendered_lines: 0,
            cursor_on_input_line: false,
        })
    }

    pub fn redraw(
        &mut self,
        plain_state_key: &str,
        warning_line: Option<&str>,
        playback_time: &str,
        input_buffer: &str,
    ) -> Result<()> {
        if self.mode == RenderMode::Plain {
            if self.last_plain_state_key.as_deref() != Some(plain_state_key) {
                let mut lines = Vec::with_capacity(2);
                if let Some(warning_line) = warning_line {
                    lines.push(warning_line.to_string());
                }
                lines.push(playback_time.to_string());
                self.write_plain_lines(&lines)?;
                self.last_plain_state_key = Some(plain_state_key.to_string());
            }

            return Ok(());
        }

        let mut lines = Vec::with_capacity(3);
        if let Some(warning_line) = warning_line {
            lines.push(warning_line.to_string());
        }
        lines.push(playback_time.to_string());
        lines.push(format!("$ {input_buffer}"));
        let skipped = self.last_ansi_lines == lines;
        self.replace_ansi_lines(&lines, true)?;

        let cursor_col = prompt_column(input_buffer)?;
        if !skipped {
            queue!(self.stdout, MoveUp(1))?;
        }
        queue!(self.stdout, MoveToColumn(cursor_col))?;
        self.stdout.flush()?;
        self.cursor_on_input_line = true;

        Ok(())
    }

    pub fn print_transient(&mut self, text: &str) -> Result<()> {
        self.last_plain_state_key = None;
        if self.mode == RenderMode::Plain {
            let lines = text.lines().map(str::to_string).collect::<Vec<_>>();
            return self.write_plain_lines(&lines);
        }

        self.move_to_render_top()?;
        queue!(self.stdout, Clear(ClearType::FromCursorDown))?;
        for line in text.lines() {
            write!(self.stdout, "{line}\r\n")?;
        }
        self.stdout.flush()?;

        self.last_ansi_lines.clear();
        self.rendered_lines = 0;
        self.cursor_on_input_line = false;

        Ok(())
    }

    pub fn clear_live_area(&mut self) -> Result<()> {
        self.last_plain_state_key = None;
        if self.mode == RenderMode::Plain {
            self.rendered_lines = 0;
            self.cursor_on_input_line = false;
            return Ok(());
        }

        self.move_to_render_top()?;
        queue!(self.stdout, Clear(ClearType::FromCursorDown))?;
        self.stdout.flush()?;
        self.last_ansi_lines.clear();
        self.rendered_lines = 0;
        self.cursor_on_input_line = false;
        Ok(())
    }
}

impl Drop for LivePrompt {
    fn drop(&mut self) {
        if self.raw_enabled {
            let _ = execute!(self.stdout, Show);
            let _ = terminal::disable_raw_mode();
        }
    }
}

impl LivePrompt {
    fn write_plain_lines(&mut self, lines: &[String]) -> Result<()> {
        for line in lines {
            write!(self.stdout, "{line}\r\n")?;
        }
        self.stdout.flush()?;
        self.rendered_lines = lines.len();
        self.cursor_on_input_line = false;
        Ok(())
    }

    fn replace_ansi_lines(&mut self, lines: &[String], cursor_on_input_line: bool) -> Result<()> {
        if self.last_ansi_lines == lines {
            self.cursor_on_input_line = cursor_on_input_line;
            return Ok(());
        }

        self.move_to_render_top()?;
        queue!(self.stdout, Clear(ClearType::FromCursorDown))?;
        for line in lines {
            write!(self.stdout, "{line}\r\n")?;
        }
        self.stdout.flush()?;
        self.last_ansi_lines = lines.to_vec();
        self.rendered_lines = lines.len();
        self.cursor_on_input_line = cursor_on_input_line;
        Ok(())
    }

    fn move_to_render_top(&mut self) -> Result<()> {
        if self.rendered_lines == 0 {
            queue!(self.stdout, MoveToColumn(0))?;
            return Ok(());
        }

        let lines_up = if self.cursor_on_input_line {
            self.rendered_lines.saturating_sub(1)
        } else {
            self.rendered_lines
        };

        if lines_up > 0 {
            let lines_up = u16::try_from(lines_up)
                .context("live prompt block is too tall for terminal rendering")?;
            queue!(self.stdout, MoveUp(lines_up))?;
        }
        queue!(self.stdout, MoveToColumn(0))?;
        Ok(())
    }
}

fn prompt_column(input_buffer: &str) -> Result<u16> {
    let width = "$ "
        .chars()
        .count()
        .saturating_add(input_buffer.chars().count());
    u16::try_from(width).context("input line is too wide for terminal rendering")
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVE_FRAMES, StepLabels, StepLine, StepState, format_playback_time, validate_terminal,
    };
    use crate::timecode::Timecode;

    #[test]
    fn formats_playback_time_line() {
        let line = format_playback_time(
            Timecode::from_seconds(312),
            Some(Timecode::from_seconds(900)),
        );
        assert_eq!(line, "00:05:12 / 00:15:00");
    }

    #[test]
    fn validates_interactive_terminal_requirements() {
        assert!(validate_terminal(true, true).is_ok());
        assert!(validate_terminal(false, true).is_err());
        assert!(validate_terminal(true, false).is_err());
    }

    #[test]
    fn stage_symbols_are_stable() {
        assert_eq!(ACTIVE_FRAMES.len(), 10);
        let step = StepLine {
            labels: StepLabels::new(
                "Source details",
                "Reading source details",
                "Read source details",
            ),
            state: StepState::Done,
            detail_lines: Vec::new(),
        };
        assert_eq!(step.state, StepState::Done);
    }
}
