use crate::{diagnostics::render_command, timecode::Timecode};
use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use tokio::process::Command;
use tracing::debug;

#[derive(Clone, Debug, Default)]
pub struct QuickTimePlayer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackStatus {
    AppClosed,
    NoDocument,
    Snapshot(PlaybackSnapshot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackSnapshot {
    current_time: Timecode,
    playing: bool,
}

impl PlaybackSnapshot {
    pub fn current_time(&self) -> Timecode {
        self.current_time
    }

    pub fn playing(&self) -> bool {
        self.playing
    }
}

impl QuickTimePlayer {
    pub fn new() -> Self {
        Self
    }

    pub async fn open(&self, url: &str) -> Result<()> {
        let lines = [
            "on run argv",
            "set targetUrl to item 1 of argv",
            "tell application \"QuickTime Player\" to activate",
            "do shell script \"open -a \" & quoted form of \"QuickTime Player\" & \" \" & quoted form of targetUrl",
            "end run",
        ];
        debug!(url, "opening QuickTime Player");
        run_osascript(&lines, &[url]).await
    }

    pub async fn reload(&self, url: &str) -> Result<()> {
        let lines = [
            "on run argv",
            "set targetUrl to item 1 of argv",
            "tell application \"QuickTime Player\" to activate",
            "tell application \"QuickTime Player\" to if (count of documents) > 0 then close front document saving no",
            "do shell script \"open -a \" & quoted form of \"QuickTime Player\" & \" \" & quoted form of targetUrl",
            "end run",
        ];
        debug!(url, "reloading QuickTime Player");
        run_osascript(&lines, &[url]).await
    }

    pub async fn quit(&self) -> Result<()> {
        let lines = [
            "if application \"QuickTime Player\" is running then",
            "tell application \"QuickTime Player\" to quit saving no",
            "end if",
        ];
        debug!("quitting QuickTime Player");
        run_osascript(&lines, &[]).await
    }

    pub async fn playback_status(&self) -> Result<PlaybackStatus> {
        let lines = [
            "if application id \"com.apple.QuickTimePlayerX\" is not running then return \"app-closed\"",
            "tell application id \"com.apple.QuickTimePlayerX\"",
            "try",
            "if (count of documents) is 0 then return \"no-document\"",
            "set movieDocument to document 1",
            "return ((playing of movieDocument as string) & \"|\" & (current time of movieDocument as string))",
            "on error",
            "return \"no-document\"",
            "end try",
            "end tell",
        ];
        let output = run_osascript_capture(&lines, &[]).await?;
        parse_playback_status(&output)
    }

    pub fn render_open_command(&self, url: &str) -> String {
        render_osascript_command(
            &[
                "on run argv",
                "set targetUrl to item 1 of argv",
                "tell application \"QuickTime Player\" to activate",
                "do shell script \"open -a \" & quoted form of \"QuickTime Player\" & \" \" & quoted form of targetUrl",
                "end run",
            ],
            &[url],
        )
    }
}

async fn run_osascript(lines: &[&str], args: &[&str]) -> Result<()> {
    run_osascript_capture(lines, args).await.map(|_| ())
}

async fn run_osascript_capture(lines: &[&str], args: &[&str]) -> Result<String> {
    let mut command = Command::new("osascript");
    for line in lines {
        command.arg("-e").arg(line);
    }
    if !args.is_empty() {
        command.arg("--");
        for arg in args {
            command.arg(arg);
        }
    }

    let output = command
        .output()
        .await
        .context("unable to run osascript for QuickTime control")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Unable to control QuickTime Player: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn render_osascript_command(lines: &[&str], args: &[&str]) -> String {
    let mut rendered_args = Vec::with_capacity(lines.len() * 2 + args.len() + 1);
    for line in lines {
        rendered_args.push(OsString::from("-e"));
        rendered_args.push(OsString::from(*line));
    }
    if !args.is_empty() {
        rendered_args.push(OsString::from("--"));
        rendered_args.extend(args.iter().map(|arg| OsString::from(*arg)));
    }
    render_command(&OsString::from("osascript"), &rendered_args)
}

fn parse_playback_status(output: &str) -> Result<PlaybackStatus> {
    let output = output.trim();
    match output {
        "app-closed" => return Ok(PlaybackStatus::AppClosed),
        "no-document" | "" => return Ok(PlaybackStatus::NoDocument),
        _ => {}
    }

    let (playing, current_time) = output
        .split_once('|')
        .ok_or_else(|| anyhow!("QuickTime returned an unexpected playback status"))?;
    let current_time = current_time
        .parse::<f64>()
        .ok()
        .and_then(Timecode::from_seconds_f64)
        .ok_or_else(|| anyhow!("QuickTime returned an invalid playhead"))?;

    let playing = match playing {
        "true" => true,
        "false" => false,
        _ => return Err(anyhow!("QuickTime returned an invalid playback flag")),
    };

    Ok(PlaybackStatus::Snapshot(PlaybackSnapshot {
        current_time,
        playing,
    }))
}

#[cfg(test)]
mod tests {
    use super::{PlaybackSnapshot, PlaybackStatus, parse_playback_status};
    use crate::timecode::Timecode;

    #[test]
    fn parses_playback_status_snapshots() {
        assert_eq!(
            parse_playback_status("true|12.9").unwrap(),
            PlaybackStatus::Snapshot(PlaybackSnapshot {
                current_time: Timecode::from_seconds(12),
                playing: true,
            })
        );
        assert_eq!(
            parse_playback_status("false|3.1").unwrap(),
            PlaybackStatus::Snapshot(PlaybackSnapshot {
                current_time: Timecode::from_seconds(3),
                playing: false,
            })
        );
    }

    #[test]
    fn parses_closed_statuses() {
        assert_eq!(
            parse_playback_status("app-closed").unwrap(),
            PlaybackStatus::AppClosed
        );
        assert_eq!(
            parse_playback_status("no-document").unwrap(),
            PlaybackStatus::NoDocument
        );
    }

    #[test]
    fn rejects_invalid_playback_status() {
        assert!(parse_playback_status("bad").is_err());
        assert!(parse_playback_status("maybe|1.0").is_err());
    }
}
