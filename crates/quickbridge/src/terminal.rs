use crate::terminal_detection::TerminalInfo;
use anyhow::{Result, bail};
use quickbridge_core::Timecode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderMode {
    Ansi,
    Plain,
}

fn render_mode() -> RenderMode {
    match std::env::var("QUICKBRIDGE_RENDER_MODE") {
        Ok(value) if value.eq_ignore_ascii_case("ansi") => RenderMode::Ansi,
        _ => RenderMode::Plain,
    }
}

pub fn require_interactive_terminal() -> Result<()> {
    let terminal = TerminalInfo::detect();
    if terminal.supports_interactive_ui() {
        Ok(())
    } else {
        bail!(
            "quickbridge requires an interactive terminal. Run it from Terminal, iTerm, or another local shell session"
        )
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
