use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(1);

#[test]
fn help_prints_usage() {
    Command::cargo_bin("quickbridge")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: quickbridge [OPTIONS] <URL>",
        ))
        .stdout(predicate::str::contains("QUICKBRIDGE_FFMPEG_BIN"));
}

#[test]
fn version_prints_version() {
    Command::cargo_bin("quickbridge")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("quickbridge 0.1.0"));
}

#[test]
fn scripted_simulation_happy_path_runs_without_ansi_noise() {
    Command::cargo_bin("quickbridge")
        .unwrap()
        .env("QUICKBRIDGE_RENDER_MODE", "plain")
        .args([
            "--simulate",
            "happy-path",
            "--script",
            "00:00:10",
            "--script",
            "quit",
            "https://example.com/video.mkv",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("quickbridge 0.1.0"))
        .stdout(predicate::str::contains("Inspect source"))
        .stdout(predicate::str::contains("Start session"))
        .stdout(predicate::str::contains("Getting ready to jump"))
        .stdout(predicate::str::contains("Switch playback").not())
        .stdout(predicate::str::contains("Scripted command"))
        .stdout(predicate::str::contains("Preparing session").not())
        .stdout(predicate::str::contains("\u{1b}").not());
}

#[test]
fn scripted_simulation_no_ranges_blocks_seek_commands() {
    Command::cargo_bin("quickbridge")
        .unwrap()
        .env("QUICKBRIDGE_RENDER_MODE", "plain")
        .args([
            "--simulate",
            "no-ranges",
            "--script",
            "00:00:10",
            "--script",
            "quit",
            "https://example.com/video.mkv",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Jumping to a different time isn't available",
        ))
        .stdout(predicate::str::contains("Getting ready to jump").not())
        .stdout(predicate::str::contains("Switch playback").not());
}

#[test]
fn invalid_at_returns_usage_error() {
    Command::cargo_bin("quickbridge")
        .unwrap()
        .args(["--at", "invalid", "https://example.com/video.mkv"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("invalid value"))
        .stderr(predicate::str::contains("--at <TIMESTAMP>"));
}

#[cfg(target_os = "macos")]
#[test]
fn missing_ffmpeg_returns_runtime_error() {
    Command::cargo_bin("quickbridge")
        .unwrap()
        .env("QUICKBRIDGE_FFMPEG_BIN", "/tmp/quickbridge-missing-ffmpeg")
        .arg("https://example.com/video.mkv")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Install ffmpeg"));
}

#[cfg(target_os = "macos")]
#[test]
fn missing_ffprobe_returns_runtime_error() {
    let ffmpeg = write_script(
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = "-version" ]; then
  echo "fake ffmpeg"
  exit 0
fi
exit 0
"#,
    );

    Command::cargo_bin("quickbridge")
        .unwrap()
        .env("QUICKBRIDGE_FFMPEG_BIN", &ffmpeg)
        .env(
            "QUICKBRIDGE_FFPROBE_BIN",
            "/tmp/quickbridge-missing-ffprobe",
        )
        .arg("https://example.com/video.mkv")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Install ffprobe"));
}

#[cfg(target_os = "macos")]
#[test]
fn rejects_non_interactive_terminal_sessions() {
    let ffmpeg = write_script(
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = "-version" ]; then
  echo "fake ffmpeg"
  exit 0
fi
exit 0
"#,
    );
    let ffprobe = write_script(
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = "-version" ]; then
  echo "fake ffprobe"
  exit 0
fi
exit 0
"#,
    );

    Command::cargo_bin("quickbridge")
        .unwrap()
        .env("QUICKBRIDGE_FFMPEG_BIN", &ffmpeg)
        .env("QUICKBRIDGE_FFPROBE_BIN", &ffprobe)
        .arg("https://example.com/video.mkv")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("interactive terminal"));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn rejects_unsupported_platforms() {
    Command::cargo_bin("quickbridge")
        .unwrap()
        .arg("https://example.com/video.mkv")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("supports macOS only"));
}

#[cfg(unix)]
fn write_script(contents: &str) -> String {
    use std::os::unix::fs::PermissionsExt;

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let unique = SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("quickbridge-cli-test-{millis}-{unique}.sh"));
    fs::write(&path, contents).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path.to_string_lossy().into_owned()
}
