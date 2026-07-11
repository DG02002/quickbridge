use crate::Timecode;
use thiserror::Error;

/// Commands supported by the interactive command composer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    JumpAbsolute(Timecode),
    JumpRelative(i64),
    Help,
    Reopen,
    Status,
    Quit,
}

/// Errors returned when parsing user-entered commands.
#[derive(Debug, Error)]
pub enum CommandParseError {
    #[error("{0}")]
    InvalidTimecode(#[from] crate::TimecodeParseError),
    #[error("enter a timestamp, such as 01:30")]
    NotATimestampCommand,
}

/// Parse a command line entered by the user.
pub fn parse_command(line: &str) -> Result<Option<Command>, CommandParseError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    match line {
        "help" | "h" | "?" => return Ok(Some(Command::Help)),
        "reopen" | "open" | "r" => return Ok(Some(Command::Reopen)),
        "status" | "details" | "s" => return Ok(Some(Command::Status)),
        "quit" | "q" | "exit" => return Ok(Some(Command::Quit)),
        _ => {}
    }

    if let Some(rest) = line.strip_prefix('+') {
        let timecode = Timecode::parse(rest)?;
        return Ok(Some(Command::JumpRelative(timecode.as_seconds() as i64)));
    }

    if let Some(rest) = line.strip_prefix('-') {
        let timecode = Timecode::parse(rest)?;
        return Ok(Some(Command::JumpRelative(-(timecode.as_seconds() as i64))));
    }

    Ok(Some(Command::JumpAbsolute(Timecode::parse(line)?)))
}

/// Render help text for the command composer.
pub fn help_text() -> String {
    String::from(
        "Enter a time or command\n\
         HH:MM:SS  Jump to a time\n\
         MM:SS     Jump to a time\n\
         SS        Jump to a time\n\
         +MM:SS    Jump forward from the current time\n\
         -MM:SS    Jump back from the current time\n\
         reopen    Reopen the stream in QuickTime Player\n\
         status    Show stream and playback details\n\
         details   Show stream and playback details\n\
         help      Show commands\n\
         quit      Close QuickTime Player and Quickbridge",
    )
}

/// Resolve a parsed command into a concrete timestamp target.
pub fn resolve_target(
    estimated_position: Timecode,
    command: &Command,
) -> Result<Timecode, CommandParseError> {
    match command {
        Command::JumpAbsolute(target) => Ok(*target),
        Command::JumpRelative(delta) => Ok(estimated_position.apply_delta(*delta)),
        _ => Err(CommandParseError::NotATimestampCommand),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command, resolve_target};
    use crate::Timecode;

    #[test]
    fn parses_operational_commands() {
        assert_eq!(parse_command("help").unwrap(), Some(Command::Help));
        assert_eq!(parse_command("reopen").unwrap(), Some(Command::Reopen));
        assert_eq!(parse_command("status").unwrap(), Some(Command::Status));
        assert_eq!(parse_command("details").unwrap(), Some(Command::Status));
        assert_eq!(parse_command("quit").unwrap(), Some(Command::Quit));
    }

    #[test]
    fn parses_absolute_and_relative_jumps() {
        assert_eq!(
            parse_command("01:30").unwrap(),
            Some(Command::JumpAbsolute(Timecode::from_seconds(90)))
        );
        assert_eq!(
            parse_command("+30").unwrap(),
            Some(Command::JumpRelative(30))
        );
        assert_eq!(
            parse_command("-00:10").unwrap(),
            Some(Command::JumpRelative(-10))
        );
    }

    #[test]
    fn resolves_relative_targets_from_estimated_position() {
        let estimated = Timecode::from_seconds(75);
        let target = resolve_target(estimated, &Command::JumpRelative(-30)).unwrap();
        assert_eq!(target, Timecode::from_seconds(45));
    }
}
