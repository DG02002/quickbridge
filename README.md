# quickbridge

`quickbridge` plays remote media in QuickTime Player while you control playback from your terminal. It relays the source through `ffmpeg`, serves a local HTTP Live Streaming (HLS) stream, and supports timestamp jumps without restarting QuickTime.

> [!INFO]
> `quickbridge` is an experiment and may not work with every media source. Subtitle playback isn't supported. If you want to help revive the project or suggest an improvement, [open an issue](https://github.com/DG02002/quickbridge/issues).

## Demo

[Watch the quickbridge demo](./assets/demo/quickbridge-demo.mp4) to see the launcher, track selection, relay startup, and playback controls.

## Requirements

- macOS with QuickTime Player
- `ffmpeg` and `ffprobe` available on `PATH`
- An interactive terminal

## Install

Download `quickbridge-aarch64-apple-darwin.tar.gz` from [GitHub Releases](https://github.com/DG02002/quickbridge/releases), then extract and run it:

```console
tar -xzf quickbridge-aarch64-apple-darwin.tar.gz
chmod +x quickbridge
./quickbridge
```

You can move the binary to a directory on your `PATH` if you want to run `quickbridge` from anywhere. A Homebrew tap may be added in a future release, but it isn't available yet.

## Start playback

Open the launcher and paste a media URL:

```console
quickbridge
```

You can also pass the URL directly and choose a starting timestamp:

```console
quickbridge --at 01:23:45 "https://example.com/video.mkv"
```

For sources with multiple tracks, quickbridge displays video and audio selection menus before playback. It converts unsupported audio formats, including DTS, to Apple Lossless Audio Codec (ALAC) for QuickTime compatibility.

During playback, enter an absolute timestamp such as `01:30`, a relative jump such as `+30` or `-10`, or one of these commands: `help`, `status`, `details`, `reopen`, and `quit`. Press `Esc` to clear the input or close an overlay, and press `Ctrl+C` to exit.

## Command reference

```text
Usage: quickbridge [OPTIONS] [URL]

Arguments:
  [URL]  Media URL to relay through ffmpeg

Options:
      --port <PORT>          Port to bind the local HLS server to. Use 0 to
                             choose a free port automatically [default: 0]
      --at <TIMESTAMP>       Start playback at a source timestamp, for example
                             `90`, `01:30`, or `01:02:03`
      --verbose              Print debug logs to stderr
      --keep-temp            Keep session files on disk after quickbridge exits
      --simulate <SCENARIO>  Simulate the full quickbridge flow without ffmpeg,
                             ffprobe, QuickTime, or remote servers [possible
                             values: happy-path, no-ranges, ui-tour]
      --no-alt-screen        Disable the alternate screen and keep the TUI inline
                             in the current terminal buffer
      --script <COMMAND>     Run prompt commands non-interactively. Repeat the
                             flag to script multiple commands
  -h, --help                 Print help
  -V, --version              Print version

Environment:
  QUICKBRIDGE_FFMPEG_BIN   Override the ffmpeg executable path
  QUICKBRIDGE_FFPROBE_BIN  Override the ffprobe executable path
  QUICKBRIDGE_RENDER_MODE  Set `plain` or `ansi` to override plain stdout formatting
  RUST_LOG                 Set the log filter. `--verbose` enables `quickbridge=debug`.
```

Use `--script` more than once to run commands without interactive input:

```console
quickbridge --script status --script quit "https://example.com/video.mkv"
```

See [CONTRIBUTING.md](./CONTRIBUTING.md) for local development, testing, and release instructions.
