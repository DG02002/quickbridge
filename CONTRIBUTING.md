# Contributing

## Requirements

- macOS
- Rust toolchain `1.94`
- `ffmpeg` available on `PATH`
- `ffprobe` available on `PATH`

Install the standard Rust components once:

```console
$ rustup component add rustfmt clippy rust-analyzer
```

Cargo aliases are configured in [`.cargo/config.toml`](./.cargo/config.toml):

```console
$ cargo check-all
$ cargo fmt-check
$ cargo lint
$ cargo xtest
```

## Development workflow

Run the full local verification flow before committing:

```console
$ cargo fmt --all
$ cargo check-all
$ cargo lint
$ cargo xtest
$ cargo package --allow-dirty
$ cargo publish --dry-run
```

`cargo` aliases can only wrap a single Cargo command, so the project keeps the
full verification flow as a short sequence instead of adding a custom task runner.

## Running locally

Run the CLI with a source URL:

```console
$ cargo run -- "https://example.com/video.mkv"
```

Start from a timestamp and a fixed port:

```console
$ cargo run -- --at 01:23:45 --port 50505 "https://example.com/video.mkv"
```

Build a development binary:

```console
$ cargo build
```

Build an optimized release binary:

```console
$ cargo build --release
```

## Test notes

- `QUICKBRIDGE_FFMPEG_BIN` overrides the `ffmpeg` executable path for local testing.
- `QUICKBRIDGE_FFPROBE_BIN` overrides the `ffprobe` executable path for local testing.
- The automated test suite covers timestamp parsing, session transitions, server path safety, and mocked `ffmpeg` lifecycle behavior.
- Real QuickTime behavior still needs a manual macOS smoke test with `cargo run`.
- See [docs/release-checklist.md](./docs/release-checklist.md) before cutting a public release.
- Keep CLI wording aligned with [docs/cli-writing.md](./docs/cli-writing.md).
