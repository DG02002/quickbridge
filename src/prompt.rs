use crate::timecode::Timecode;
use anyhow::{Result, bail};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    JumpAbsolute(Timecode),
    JumpRelative(i64),
    Help,
    Reopen,
    Status,
    Quit,
}

pub fn parse_command(line: &str) -> Result<Option<Command>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    match line {
        "help" | "h" => return Ok(Some(Command::Help)),
        "reopen" | "open" => return Ok(Some(Command::Reopen)),
        "status" => return Ok(Some(Command::Status)),
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

    let timecode = Timecode::parse(line)?;
    Ok(Some(Command::JumpAbsolute(timecode)))
}

use crate::terminal::{emphasize, muted};

pub fn help_text() -> String {
    format!(
        "{}\n  {} {} {}\n  {} {} {}\n  {} {} {}\n  {} {} {}\n  {} {} {}\n  {} {} {}\n  {} {} {}\n  {} {} {}\n  {} {} {}",
        emphasize("Available commands"),
        muted("press"),
        emphasize("HH:MM:SS + enter"),
        muted("to jump to that time"),
        muted("press"),
        emphasize("MM:SS + enter"),
        muted("to jump to that time"),
        muted("press"),
        emphasize("SS + enter"),
        muted("to jump to that time"),
        muted("press"),
        emphasize("+MM:SS + enter"),
        muted("to jump forward from the current time"),
        muted("press"),
        emphasize("-MM:SS + enter"),
        muted("to jump backward from the current time"),
        muted("press"),
        emphasize("reopen + enter"),
        muted("to open the stream again in QuickTime Player"),
        muted("press"),
        emphasize("status + enter"),
        muted("to show the current stream and playback time"),
        muted("press"),
        emphasize("h + enter"),
        muted("to show this list"),
        muted("press"),
        emphasize("q + enter"),
        muted("to stop quickbridge and close QuickTime Player")
    )
}

pub fn resolve_target(estimated_position: Timecode, command: &Command) -> Result<Timecode> {
    match command {
        Command::JumpAbsolute(target) => Ok(*target),
        Command::JumpRelative(delta) => Ok(estimated_position.apply_delta(*delta)),
        _ => bail!("command does not resolve to a timestamp"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command, resolve_target};
    use crate::timecode::Timecode;

    #[test]
    fn parses_operational_commands() {
        assert_eq!(parse_command("help").unwrap(), Some(Command::Help));
        assert_eq!(parse_command("reopen").unwrap(), Some(Command::Reopen));
        assert_eq!(parse_command("status").unwrap(), Some(Command::Status));
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
