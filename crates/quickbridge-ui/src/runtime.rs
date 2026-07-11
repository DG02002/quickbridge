use super::app::AppState;
use super::render;
use crate::{Result, UiError};
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};
use std::io::{self, Stdout};

const INLINE_VIEWPORT_HEIGHT: u16 = 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeOptions {
    pub use_alt_screen: bool,
}

pub struct TuiRuntime {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    use_alt_screen: bool,
    raw_mode_enabled: bool,
}

impl TuiRuntime {
    pub fn enter(options: RuntimeOptions) -> Result<Self> {
        let mut stdout = io::stdout();
        enable_raw_mode().map_err(|source| UiError::Terminal {
            action: "enable raw terminal mode",
            source,
        })?;
        execute!(stdout, Hide, EnableBracketedPaste, EnableMouseCapture).map_err(|source| {
            UiError::Terminal {
                action: "prepare the terminal for quickbridge",
                source,
            }
        })?;
        if options.use_alt_screen {
            execute!(stdout, EnterAlternateScreen).map_err(|source| UiError::Terminal {
                action: "enter the alternate screen",
                source,
            })?;
        }

        let backend = CrosstermBackend::new(stdout);
        let terminal = if options.use_alt_screen {
            Terminal::new(backend)
        } else {
            Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Inline(INLINE_VIEWPORT_HEIGHT),
                },
            )
        }
        .map_err(|source| UiError::Terminal {
            action: "initialize the TUI terminal",
            source,
        })?;

        Ok(Self {
            terminal,
            use_alt_screen: options.use_alt_screen,
            raw_mode_enabled: true,
        })
    }

    pub fn draw(&mut self, state: &AppState) -> Result<()> {
        let full_screen = self.use_alt_screen;
        self.terminal
            .draw(|frame| render::render(frame, state, full_screen))
            .map_err(|source| UiError::Terminal {
                action: "draw the TUI",
                source,
            })
            .map(|_| ())
    }
}

impl Drop for TuiRuntime {
    fn drop(&mut self) {
        if self.use_alt_screen {
            let _ = execute!(
                self.terminal.backend_mut(),
                Show,
                DisableBracketedPaste,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
        } else {
            let _ = execute!(
                self.terminal.backend_mut(),
                Show,
                DisableBracketedPaste,
                DisableMouseCapture
            );
        }
        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
        }
    }
}
