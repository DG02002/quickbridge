# Plan 004: Establish a real-media HDR/DV performance regression gate

> **Executor instructions**: This plan validates the prior implementation
> plans. Do not declare the lag fixed using JSON/unit tests alone. Stop and
> report if no authorized real media source is available. Update
> `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates sample-jsons docs scripts plans`.
> Plans 001-003 must be DONE and their excerpts reconciled before proceeding.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: plans 001, 002 and 003
- **Category**: tests/perf
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

The reported failure is visible lag in QuickTime with large 2160p HDR/DV
media. Probe JSON proves codec and bitrate facts but contains no packets,
keyframe timing, network stalls, HLS requests, decoder behavior, or rendered
HDR state. A repeatable real-media gate is required to distinguish buffering,
segmentation, server delivery and codec-tag failures and prevent regression.

## Current state

- Automated tests cover argument construction and mocked FFmpeg lifecycle, but
  no real QuickTime/2160p playback gate exists.
- The supplied JSON corpus in `sample-jsons/` covers DV P5, PQ HDR, high-rate
  HEVC, TrueHD, E-AC-3, VC-1 and attached cover art.
- `quickbridge --verbose --keep-temp <URL>` exposes the spawned command and
  retains generated HLS assets.
- The current known demanding samples are about 70 Mb/s 4K60 PQ and 85.2 Mb/s
  4K24 HEVC. A P5 sample is about 32.2 Mb/s container/24.6 Mb/s video.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Full suite | `cargo xtest` | exit 0 |
| Release build | `cargo build --release` | exit 0 |
| Inspect generated init | `ffprobe -v error -show_streams -show_format -of json <init.mp4>` | valid JSON; expected codec tag/side data |
| Inspect segment frames | `ffprobe -v error -select_streams v:0 -show_frames -show_entries frame=key_frame,pict_type,pts_time -of json <segment.m4s>` | first decodable video frame is random-access |

## Suggested executor toolkit

- Use `diagnosing-bugs`; its one-variable-at-a-time loop is mandatory here.
- Use Apple HLS validation tools if installed, but do not make undocumented
  third-party tools a required repository dependency.

## Scope

**In scope**:
- `docs/performance-testing.md` (create)
- `scripts/` only for a non-destructive capture/analyze helper
- Test-only code needed to parse retained session artifacts
- Small corrective changes to Plans 001-003 files only when a measured gate
  proves them necessary
- `plans/README.md` status only

**Out of scope**:
- Committing copyrighted media files
- Downloading media without explicit authorization
- Dolby Vision Profile 7 transformation
- Quality-changing video transcoding
- Broad CLI/UI feature work

## Git workflow

- Suggested branch: `codex/004-hdr-dv-performance-gate`
- Suggested commit if requested: `test: add UHD relay performance gate`.
- Never commit media, source URLs, tokens, cookies, or personal paths.

## Steps

### Step 1: Secure authorized real-media fixtures

Obtain explicit paths/URLs from the operator for at least:

1. high-rate 2160p HEVC/HDR10 (target 70 Mb/s or above);
2. 2160p Dolby Vision Profile 5;
3. a lower-rate 1080p control.

Record only sanitized metadata in docs. Never copy URLs containing credentials
or local personal paths into the repository.

**Verify**: `ffprobe` confirms the expected resolution, rate, transfer
characteristic and DOVI profile before testing.

### Step 2: Build a repeatable measurement loop

Document and, where practical, script a loop that:

- runs the release binary with `--verbose --keep-temp`;
- observes at least ten minutes of uninterrupted playback or the full short
  fixture;
- records stall count/duration, FFmpeg exit/errors, playlist depth, segment
  production intervals, missing HTTP assets, process CPU, memory and retained
  storage;
- repeats each configuration three times;
- changes one variable per comparison.

The pass signal is zero rebuffering stalls after playback begins on a network
that sustains at least 1.5x the source peak rate. Frame pacing/UI animation is
not a substitute for detecting playback stalls.

**Verify**: run the loop once against the 1080p control and once against the
high-rate fixture; it produces a deterministic summary without modifying
source media.

### Step 3: Validate packaging and segment independence

For retained sessions:

- confirm P5 initialization data is tagged `dvh1` and still exposes Dolby
  Vision configuration;
- confirm ordinary HEVC/HDR uses `hvc1`;
- confirm playlist duration/window matches Plan 002;
- inspect the first decodable video frame of multiple segments around startup
  and a timestamp jump;
- confirm QuickTime reports/visually engages HDR/DV only on a capable display.

Use Apple HLS validation tools when available and record tool/version and
results. Do not weaken packaging merely to silence an unverifiable warning.

**Verify**: all inspected init/segments meet the expected tag and random-access
conditions; failures identify the exact artifact and command.

### Step 4: Run controlled comparisons and tune only from evidence

Compare baseline versus final configuration for the same source/network. If
lag remains, change exactly one of: initial burst, catch-up speed, playlist
window, segment target, or server streaming. Do not change codec tags and
buffering simultaneously. Keep defaults bounded; reject settings that cause a
full-file download.

**Verify**: three final runs per UHD fixture meet the pass signal and show no
unbounded temp growth.

### Step 5: Document and lock the regression

Write `docs/performance-testing.md` with sanitized commands, expected ranges,
pass/fail rules, supported DV profiles, and known limitations. Add the tightest
automated artifact checks that do not require QuickTime to CI; keep the actual
player test as a documented macOS release gate.

**Verify**: `cargo xtest` and `cargo build --release` pass; documentation can
be followed from a fresh checkout without secret or copyrighted fixtures.

## Test plan

- 1080p control distinguishes general server failures from UHD-only failures.
- 70+ Mb/s 2160p source exercises buffering and allocations.
- DV P5 exercises `dvh1` and configuration preservation.
- Initial startup and at least three timestamp jumps are measured separately.
- Each comparison runs three times with one variable changed.

## Done criteria

- [ ] A red-capable, repeatable real-media lag loop exists and has been run.
- [ ] Three final UHD runs have zero playback stalls under the stated network
  condition.
- [ ] P5 retained output is `dvh1` with DV configuration intact.
- [ ] Ordinary compatible HEVC retained output is `hvc1`.
- [ ] Segment-start checks pass around startup and jumps.
- [ ] Temp storage remains bounded.
- [ ] No media, credentials, private URLs, or personal paths are committed.
- [ ] Full tests and release build pass.

## STOP conditions

- No authorized real media path/URL is available. JSON alone cannot close the
  reported performance bug.
- Network capacity is below the source peak bitrate; classify that run as an
  invalid environment, not a QuickBridge regression.
- QuickTime's HDR/DV status cannot be observed on a capable Mac/display.
- Fixing a failure requires Dolby Vision RPU transformation or video
  re-encoding; report and scope a separate plan.
- A change improves startup but worsens sustained playback or causes
  unbounded downloading.

## Maintenance notes

Keep sanitized probe JSON as classification fixtures and real media outside
git. Re-run this gate when changing FFmpeg minimum version, HLS flags, segment
duration, buffer policy, codec tags, or server response implementation.

