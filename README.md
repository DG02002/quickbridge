# quickbridge

> [!WARNING]
> `quickbridge` is currently in an alpha stage. Expect bugs and breaking changes while the CLI contract is being stabilized.

`quickbridge` is a macOS-first CLI that relays a media URL through `ffmpeg`,
serves a stable local HLS stream, opens that stream in QuickTime Player, and
lets you jump to new timestamps from the terminal without quitting QuickTime.

## Support

- Supported platform: macOS
- Required apps: QuickTime Player, `ffmpeg`, and `ffprobe`
- Required terminal mode: interactive TTY
- Public contract: CLI behavior only

## Install

Install from the local checkout during development:

```console
cargo install --path .
```

Tagged release builds are also published on GitHub:

- [GitHub Releases](https://github.com/DG02002/quickbridge/releases)

Homebrew support is coming soon.

## Usage

```console
quickbridge
quickbridge "https://example.com/video.mkv"
quickbridge --at 01:23:45 --port 50505 "https://example.com/video.mkv"
```

Running `quickbridge` without a positional URL opens the TUI launcher. From
there you can paste a media URL and press Enter to start a session.

When a source has multiple video or audio tracks, `quickbridge` inspects the
stream layout with `ffprobe` and shows selection menus before playback starts.
Unsupported audio such as DTS is transcoded to ALAC for QuickTime compatibility.

The live TUI keeps the selected tracks, player timeline, telemetry, and command
composer in one persistent workspace. The composer accepts absolute timestamps
(`90`, `01:30`, `01:02:03`), relative jumps (`+30`, `-10`, `+01:30`), and the
commands `help`, `status`, `details`, `reopen`, and `quit`. Press Enter to run
the current input, Escape to clear it or close an overlay, and Ctrl+C to quit.
Completed inspection and startup steps remain in the flexible Activity region;
use PgUp/PgDn or the mouse wheel to review older or newer entries.

The launcher uses a focused URL field: paste or type a direct URL and press
Enter. F1 opens launcher help.

Scripted automation remains available separately:

```console
quickbridge --script status --script quit "https://example.com/video.mkv"
```

The TUI keeps live relay telemetry above the contextual controls, including the
current playback time, relay write rate, buffer ahead, and temp-session storage
usage. In live mode, quickbridge polls QuickTime Player's front document
playhead so the displayed source timestamp tracks pauses and other playback
changes from the player window.

### Manual UI tour

Run the complete interactive flow safely in a terminal or Zellij pane without
ffmpeg, ffprobe, networking, or QuickTime:

```console
cargo run -p quickbridge -- --simulate ui-tour
```

The tour supplies its own demo URL and opens directly into multi-track
selection, followed by startup and the live dashboard. Try `01:30`, `details`,
`help`, `reopen`, and `quit`, then resize the Zellij pane to review compact layouts.
To test the launcher separately, run `--simulate happy-path` without a URL.

## Environment

- `QUICKBRIDGE_FFMPEG_BIN`: override the `ffmpeg` executable path
- `QUICKBRIDGE_FFPROBE_BIN`: override the `ffprobe` executable path
- `RUST_LOG`: override the log filter
- `--verbose`: enable `quickbridge=debug` logs to `stderr`

## Exit Codes

- `0`: success
- `1`: runtime error or unsupported environment
- `2`: command-line usage error
- `130`: interrupted with `Ctrl+C`

## Versioning

`quickbridge` follows Semantic Versioning, with a conservative pre-`1.0.0`
policy for CLI stability:

- `0.x.y` patch releases are for backward-compatible fixes and polish
- `0.x.0` minor releases may include breaking CLI changes
- `1.0.0` will mark the first stable CLI contract
