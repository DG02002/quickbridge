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
$ cargo install --path .
```

Tagged release builds are also published on GitHub:

- [GitHub Releases](https://github.com/DG02002/quickbridge/releases)

Homebrew support is coming soon.

## Usage

```console
$ quickbridge "https://example.com/video.mkv"
$ quickbridge --at 01:23:45 --port 50505 "https://example.com/video.mkv"
```

When a source has multiple video or audio tracks, `quickbridge` inspects the
stream layout with `ffprobe` and shows selection menus before playback starts.
Unsupported audio such as DTS is transcoded to ALAC for QuickTime compatibility.

Interactive commands:

- Absolute timestamps: `90`, `01:30`, `01:02:03`
- Relative timestamps: `+30`, `-10`, `+01:30`
- Operational commands: `status`, `help`, `quit`

The CLI keeps a live status line above the prompt, for example
`00:05:12 / 00:24:20`. In live mode, quickbridge polls QuickTime Player's
front document playhead so the displayed source timestamp tracks pauses and
other playback changes from the player window.

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
