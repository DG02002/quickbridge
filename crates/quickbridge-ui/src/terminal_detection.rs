use std::io::{self, IsTerminal};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalInfo {
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    term: Option<String>,
    term_program: Option<String>,
    zellij_session: Option<String>,
}

impl TerminalInfo {
    #[allow(dead_code)]
    pub fn detect() -> Self {
        Self::from_env(
            io::stdin().is_terminal(),
            io::stdout().is_terminal(),
            std::env::var("TERM").ok(),
            std::env::var("TERM_PROGRAM").ok(),
            std::env::var("ZELLIJ_SESSION_NAME").ok(),
        )
    }

    fn from_env(
        stdin_is_terminal: bool,
        stdout_is_terminal: bool,
        term: Option<String>,
        term_program: Option<String>,
        zellij_session: Option<String>,
    ) -> Self {
        Self {
            stdin_is_terminal,
            stdout_is_terminal,
            term,
            term_program,
            zellij_session,
        }
    }

    pub fn supports_interactive_ui(&self) -> bool {
        self.stdin_is_terminal && self.stdout_is_terminal && !self.is_dumb_terminal()
    }

    pub fn should_use_alt_screen(&self, no_alt_screen: bool) -> bool {
        self.supports_interactive_ui() && !no_alt_screen && !self.is_zellij()
    }

    pub fn is_dumb_terminal(&self) -> bool {
        self.term
            .as_deref()
            .is_some_and(|term| term.eq_ignore_ascii_case("dumb"))
    }

    pub fn is_zellij(&self) -> bool {
        self.zellij_session
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    #[cfg(test)]
    fn term_program(&self) -> Option<&str> {
        self.term_program.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalInfo;

    #[test]
    fn disables_tui_for_dumb_terminals() {
        let info = TerminalInfo::from_env(true, true, Some(String::from("dumb")), None, None);

        assert!(info.is_dumb_terminal());
        assert!(!info.supports_interactive_ui());
    }

    #[test]
    fn disables_alt_screen_in_zellij() {
        let info = TerminalInfo::from_env(
            true,
            true,
            Some(String::from("xterm-256color")),
            Some(String::from("iTerm.app")),
            Some(String::from("workspace")),
        );

        assert_eq!(info.term_program(), Some("iTerm.app"));
        assert!(info.is_zellij());
        assert!(!info.should_use_alt_screen(false));
    }

    #[test]
    fn allows_alt_screen_for_normal_terminals() {
        let info = TerminalInfo::from_env(
            true,
            true,
            Some(String::from("xterm-256color")),
            Some(String::from("iTerm.app")),
            None,
        );

        assert!(info.supports_interactive_ui());
        assert!(info.should_use_alt_screen(false));
    }
}
