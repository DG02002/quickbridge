# Contributing to quickbridge

This guide covers local development, verification, and release checks. See the [README](./README.md) for installation and product usage.

## Set up the project

You need macOS, Rust `1.94`, `ffmpeg`, and `ffprobe`. Install the Rust components used by the project:

```console
rustup component add rustfmt clippy rust-analyzer
```

Build and run quickbridge from the workspace:

```console
cargo build
cargo run -- "https://example.com/video.mkv"
```

Use an optimized build when testing release behavior:

```console
cargo build --release
```

## Explore without external tools

Use the UI tour to test the full flow without `ffmpeg`, `ffprobe`, QuickTime, networking, or a media URL:

```console
cargo run -p quickbridge -- --simulate ui-tour
```

Use `--simulate happy-path` to test the launcher. Use `--simulate no-ranges` to verify behavior when a media source doesn't support seeking.

## Verify changes

Run the complete verification sequence before committing:

```console
cargo fmt --all
cargo check-all
cargo lint
cargo xtest
cargo package --workspace --allow-dirty
```

The aliases in [`.cargo/config.toml`](./.cargo/config.toml) map to individual Cargo commands. The test suite covers timestamp parsing, session transitions, server path safety, and mocked `ffmpeg` lifecycle behavior.

Complete a manual macOS smoke test with `cargo run` to verify QuickTime integration. You can override local binary paths with `QUICKBRIDGE_FFMPEG_BIN` and `QUICKBRIDGE_FFPROBE_BIN`.

## Prepare a release

Follow the [release checklist](./docs/release-checklist.md) before publishing. Keep command output aligned with the [CLI writing guide](./docs/cli-writing.md).

quickbridge follows Semantic Versioning before `1.0.0`:

- Patch releases contain backward-compatible fixes and polish
- Minor releases may change the command-line interface
- `1.0.0` marks the first stable command-line interface contract
