# Plan 002: Make the UHD HLS relay resilient and keyframe-aligned

> **Executor instructions**: Follow every step and gate. Stop on any STOP
> condition; do not tune by intuition. Update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates/quickbridge-runtime/src/ffmpeg.rs crates/quickbridge-runtime/src/playback.rs crates/quickbridge-runtime/src/session.rs`.
> Compare Current state against the dirty worktree before editing.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: `plans/001-classify-video-and-package-for-apple.md`
- **Category**: perf/bug
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

The current relay advertises six one-second segments and throttles input to
exactly real time. An 85.2 Mb/s sample therefore has only about 64 MB/six
seconds in the visible window and cannot recover after a short network stall.
At the same time, `split_by_time` and `copyinkf` permit non-keyframe starts,
which are especially fragile after QuickTime reloads and timestamp jumps.

## Current state

- `crates/quickbridge-runtime/src/ffmpeg.rs:13-14` sets one-second segments and
  a six-entry playlist.
- `crates/quickbridge-runtime/src/ffmpeg.rs:126` emits `-re`.
- `crates/quickbridge-runtime/src/ffmpeg.rs:145-146` emits stream copy plus
  `-copyinkf`.
- `crates/quickbridge-runtime/src/ffmpeg.rs:165-170` emits one-second HLS and
  `split_by_time`.
- `crates/quickbridge-runtime/src/ffmpeg.rs:190-209` opens the player after the
  first playable segment exists.
- FFmpeg documents `-re` as `-readrate 1`, with
  `-readrate_initial_burst` and `-readrate_catchup` available for burst and
  recovery: https://ffmpeg.org/ffmpeg.html.
- FFmpeg warns `split_by_time` can worsen seeking:
  https://ffmpeg.org/ffmpeg-formats.html#hls-2.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused tests | `cargo test -p quickbridge-runtime ffmpeg::tests` | all pass |
| Check | `cargo check-all` | exit 0 |
| Lint | `cargo lint` | exit 0 |
| Full suite | `cargo xtest` | exit 0 |

## Suggested executor toolkit

- Use `diagnosing-bugs` for one-variable-at-a-time playback tests.
- Use official FFmpeg CLI documentation for option placement and semantics.

## Scope

**In scope**:
- `crates/quickbridge-runtime/src/ffmpeg.rs`
- Tests in that file
- `plans/README.md` status only

**Out of scope**:
- Codec classification/tag logic from Plan 001
- HTTP server implementation
- UI changes or new public CLI flags
- Video/audio transcoding
- Adaptive bitrate renditions

## Git workflow

- Suggested branch: `codex/002-uhd-hls-buffer`
- Preserve existing changes; do not restore legacy `src/`.
- Commit only if instructed; suggested message:
  `perf: make UHD relay buffering resilient`.

## Steps

### Step 1: Replace exact-rate-only input throttling

Replace `-re` with explicit input options before `-i`:

- `-readrate 1`
- `-readrate_initial_burst 10`
- `-readrate_catchup 1.5`

Name constants with units and explain the policy: a bounded initial lead and a
modest recovery rate, not unrestricted downloading. Do not apply these options
to an actual live capture protocol if such a source type is introduced later.

**Verify**: focused argument tests assert all three options occur before `-i`
and assert `-re` is absent.

### Step 2: Make segments keyframe-aligned

Remove `-copyinkf` and `split_by_time`. Set target segment duration to two
seconds. Let FFmpeg cut on the next source keyframe; stream copy cannot create
new IDRs. Add `independent_segments` only after an integration probe confirms
every generated segment begins with a random-access frame. Keep
`delete_segments`, `omit_endlist`, and `temp_file`.

Do not use `force_key_frames`, because it cannot create keyframes during stream
copy.

**Verify**: argument tests assert `copyinkf` and `split_by_time` are absent,
`hls_time` is `2`, and only verified flags remain.

### Step 3: Increase the bounded playlist window

Set the default playlist window to 12 two-second entries (approximately 24
seconds). Keep deletion enabled so the relay does not become a full download.
Document the approximate storage cost: around 255 MB at 85.2 Mb/s, plus a
small deletion grace window.

**Verify**: focused tests assert `-hls_list_size 12` and deletion remains on.

### Step 4: Require a small playable lead before opening QuickTime

Change readiness from one media entry to at least three complete entries plus
the nonempty init file. The ten-second initial burst should build this lead
quickly; the existing 45-second timeout remains the escape hatch. Update fake
FFmpeg tests so one/two segments are not ready and three are ready.

**Verify**: `cargo test -p quickbridge-runtime ffmpeg::tests` -> readiness and
mock-process tests pass deterministically.

### Step 5: Run all gates

**Verify**: `cargo check-all`, `cargo lint`, and `cargo xtest` all exit 0.

## Test plan

- Exact argument policy and option placement before/after `-i`.
- Readiness at zero, one, two and three complete segments.
- Missing/empty init or segment remains not ready.
- Mock FFmpeg lifecycle still shuts down cleanly.
- Plan 004 will provide the real-media QuickTime regression gate.

## Done criteria

- [ ] `-re`, `-copyinkf`, and `split_by_time` are absent.
- [ ] Initial burst and catch-up options precede `-i`.
- [ ] Playlist target is about 24 seconds and remains deletion-bounded.
- [ ] QuickTime is not opened before three complete segments exist.
- [ ] All focused and repository tests/lints pass.
- [ ] Only in-scope files and plan status changed.

## STOP conditions

- Installed FFmpeg does not support initial burst/catch-up options.
- Keyframe-aligned fMP4 fails to produce three segments within the existing
  readiness timeout on normal 24/30 fps HEVC.
- Real sources have GOPs so long that the playlist routinely contains fewer
  than three segments; report measured GOP spacing before changing policy.
- A proposed fix requires video re-encoding.

## Maintenance notes

The numerical defaults are designed around the supplied 70–85 Mb/s samples;
Plan 004 must validate them. Reviewers should ensure input-rate options remain
before `-i` and output HLS options remain after stream mapping.

