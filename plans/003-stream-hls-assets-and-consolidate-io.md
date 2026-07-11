# Plan 003: Stream HLS assets and remove duplicate hot-path filesystem work

> **Executor instructions**: Follow each step and verification gate. Stop and
> report on any STOP condition. Update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- Cargo.toml crates/quickbridge-runtime/Cargo.toml crates/quickbridge-runtime/src/server.rs crates/quickbridge-runtime/src/playback.rs`.
> Compare Current state with the dirty worktree.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW
- **Depends on**: `plans/002-resilient-keyframe-aligned-uhd-hls.md`
- **Category**: perf
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

The local server currently allocates and copies each complete segment before
responding. At the supplied 85.2 Mb/s rate that is roughly 10.6 MB for every
one-second segment and potentially over 20 MB after Plan 002 adopts two-second
segments. Snapshot telemetry also traverses the session tree twice per second,
and every segment request rereads/reparses the playlist. Streaming and one-pass
accounting remove avoidable memory and metadata churn.

## Current state

- `crates/quickbridge-runtime/src/server.rs:172-183` uses `fs::read` and
  `Body::from(bytes)` for every asset.
- `crates/quickbridge-runtime/src/server.rs:201-215` rereads and parses the
  playlist after each segment request for playback tracking.
- `crates/quickbridge-runtime/src/playback.rs:523-539` calls `scan_storage` and
  then `record_download_rate`; each recursively traverses the same root.
- `crates/quickbridge-runtime/src/playback.rs:541-615` implements the duplicate
  directory walks. The metric called download speed is actually local output
  file growth, especially inaccurate when audio is transcoded to ALAC.
- Axum responses and Tokio async filesystem APIs are the established stack.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Server tests | `cargo test -p quickbridge-runtime server::tests` | all pass |
| Playback tests | `cargo test -p quickbridge-runtime playback::tests` | all pass |
| Check | `cargo check-all` | exit 0 |
| Lint | `cargo lint` | exit 0 |
| Full suite | `cargo xtest` | exit 0 |

## Scope

**In scope**:
- `Cargo.toml` only if a direct streaming dependency is required
- `crates/quickbridge-runtime/Cargo.toml`
- `crates/quickbridge-runtime/src/server.rs`
- `crates/quickbridge-runtime/src/playback.rs`
- The smallest core/UI text files necessary to rename the misleading metric
- Tests colocated with those modules
- `plans/README.md` status only

**Out of scope**:
- FFmpeg/HLS arguments
- Range-serving arbitrary source media
- Public internet binding; server stays on loopback
- New telemetry backends or persistent metrics
- UI layout redesign

## Git workflow

- Suggested branch: `codex/003-stream-hls-assets`
- Suggested commit if requested: `perf: stream local HLS assets`.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Stream immutable media assets

Keep playlists small and memory-backed, but serve `.m4s`, `.mp4`, and `.ts`
from an async file stream. Set accurate `Content-Length` and existing content
types. Use `Cache-Control: no-cache` for `.m3u8`; use an immutable cache policy
for session-unique init and media assets. Preserve current path-safety rules and
404/500 behavior.

If using `tokio_util::io::ReaderStream`, declare it directly with only the
needed feature. Do not rely on a transitive dependency.

**Verify**: server tests assert body bytes, content length/type/cache headers,
404 behavior, and path traversal rejection.

### Step 2: Cache the tiny playlist tracking map by file version

Store parsed segment observations in active-session state and refresh only
when playlist modification metadata changes. Reset the cache on session switch
and clear. Do not hold an async lock while reading the file.

**Verify**: tests prove repeated segment observations reuse an unchanged map
and a playlist modification refreshes it. Prefer an injected/parser seam over
timing-sensitive sleeps.

### Step 3: Consolidate telemetry into one filesystem traversal

Replace `scan_storage` plus `record_download_rate` with one traversal returning
both total current bytes and newly written bytes. Preserve deletion-aware
accounting for rolling segments. Rename user-visible `Download speed` to
`Relay write rate` (or an equally accurate term) because the value measures
HLS output growth, not remote input bytes.

Do not claim true source download rate unless FFmpeg exposes a verified input
byte counter for this muxer.

**Verify**: playback tests cover file growth, replacement/deletion, storage
totals and rate calculation; UI/core snapshot tests are updated only for the
label change.

### Step 4: Run all gates

**Verify**: `cargo check-all`, `cargo lint`, and `cargo xtest` all exit 0.

## Test plan

- Stream a multi-megabyte temporary segment and compare exact response bytes.
- Assert response metadata and request-path safety.
- Exercise cached playlist invalidation deterministically.
- Exercise one-pass storage/rate accounting across create, grow and delete.
- Update affected UI snapshot only if the label is rendered there.

## Done criteria

- [ ] Media assets are streamed rather than loaded with `fs::read`.
- [ ] Playlist tracking does not reread an unchanged playlist per segment.
- [ ] Snapshot telemetry traverses the session tree once, not twice.
- [ ] The metric no longer claims to be true download speed.
- [ ] All tests, checks and lint pass.
- [ ] No out-of-scope files changed except the necessary label and plan status.

## STOP conditions

- QuickTime requires byte-range responses for individual fMP4 segments; report
  captured request headers before implementing range semantics.
- The chosen streaming body cannot surface mid-stream I/O errors through Axum
  without panics or silent truncation.
- Playlist caching requires holding an async lock across file I/O.
- Accurate source-rate reporting would require parsing undocumented FFmpeg
  output; keep the honest relay-rate name instead.

## Maintenance notes

Review memory behavior with two concurrent segment requests. Session filenames
are unique, so immutable cache headers are safe only for non-playlist assets.
If playlist generation changes, revisit cache invalidation tests.

