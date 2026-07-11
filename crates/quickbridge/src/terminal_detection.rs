use std::io::{self, IsTerminal};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalInfo {
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    term: Option<String>,
    zellij_session: Option<String>,
}

impl TerminalInfo {
    pub fn detect() -> Self {
        Self {
            stdin_is_terminal: io::stdin().is_terminal(),
            stdout_is_terminal: io::stdout().is_terminal(),
            term: std::env::var("TERM").ok(),
            zellij_session: std::env::var("ZELLIJ_SESSION_NAME").ok(),
        }
    }

    pub fn supports_interactive_ui(&self) -> bool {
        self.stdin_is_terminal && self.stdout_is_terminal && !self.is_dumb_terminal()
    }

    fn is_dumb_terminal(&self) -> bool {
        self.term
            .as_deref()
            .is_some_and(|term| term.eq_ignore_ascii_case("dumb"))
    }
}
