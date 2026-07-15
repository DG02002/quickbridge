# Release checklist

This checklist defines the release baseline for `quickbridge`.

## Versioning policy

- Keep the project on `0.x` until the CLI contract is stable.
- Use patch releases for backward-compatible fixes and polish.
- Use minor releases for breaking CLI changes while the project is on `0.x`.
- Tag releases as `vX.Y.Z`.

## Quality gates

Run these commands before creating a release:

```console
cargo fmt --all --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo package --workspace --allow-dirty
```

## Manual macOS smoke test

Verify the following on macOS with QuickTime Player:

- Open a source URL
- Select video and audio tracks
- Start from `--at`
- Switch playback with an absolute timestamp
- Switch playback with a relative timestamp
- Confirm `status`, `help`, and `quit`
- Confirm `Ctrl+C` stops the session cleanly
- Confirm `--verbose` prints useful diagnostics to `stderr`

## GitHub release

1. Update `Cargo.toml` and `CHANGELOG.md`.
2. Commit the release changes.
3. Create a tag such as `v0.2.0`.
4. Push the branch and the tag to GitHub.
5. Confirm that the release workflow uploads macOS archives and checksum files to the GitHub Release page.

## Homebrew tap follow-up

After a tagged release exists:

- Point the formula at the tagged source archive.
- Declare the `ffmpeg` dependency.
- Keep the formula macOS-only.
- Use `quickbridge --version` or `quickbridge --help` as the formula test.

Do not target `homebrew/core` until the project has a clearer support policy and
more release history.
