# Plan 001: Classify video capabilities and emit Apple-compatible tags

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before continuing. If
> a STOP condition occurs, stop and report; do not improvise. When done, update
> this plan's row in `plans/README.md`.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates/quickbridge-core/src/media.rs crates/quickbridge-runtime/src/ffmpeg.rs crates/quickbridge-runtime/src/probe.rs crates/quickbridge-core/tests sample-jsons`
> and
> `git diff -- crates/quickbridge-core/src/media.rs crates/quickbridge-runtime/src/ffmpeg.rs crates/quickbridge-runtime/src/probe.rs`.
> This plan was written against a dirty refactor whose `crates/` tree is not in
> `112ebb1`; compare the Current state facts against the live files. Mismatch is
> a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: none
- **Category**: bug/perf
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

QuickBridge currently emits `-c:v copy` for every selected video without
knowing whether it is AVC, ordinary HEVC, HDR10, HLG, Dolby Vision, VC-1, or
attached cover art. Apple playback needs `hvc1` for compatible HEVC and
`dvh1` for Dolby Vision Profile 5; silently using the wrong packaging can lose
DV recognition or take an unreliable decoder path. Structured classification
also prevents a poster image from appearing as a selectable video track.

## Current state

- `crates/quickbridge-core/src/media.rs:110-126` creates `VideoStream` from all
  ffprobe streams whose `codec_type` is `video`.
- `crates/quickbridge-core/src/media.rs:281-301` retains only stream index,
  display text, and default disposition for video. Parsed codec/profile/color
  data is discarded.
- `crates/quickbridge-core/src/media.rs:620-639` does not deserialize
  `color_primaries`, `color_transfer`, Dolby Vision side data, codec tag,
  level/tier, frame rate, bitrate, or `attached_pic`.
- `crates/quickbridge-runtime/src/ffmpeg.rs:144-155` always copies video and
  only switches audio behavior. No `-tag:v` is emitted.
- `sample-jsons/2160p.DV.P5.EAC3.mkv.ffprobe.json` is the strongest fixture:
  video stream 0 is HEVC Main 10, Dolby Vision Profile 5 Level 6; stream 50 is
  MJPEG with `disposition.attached_pic=1`.
- Error handling uses typed `thiserror` errors in
  `crates/quickbridge-core/src/media.rs` and
  `crates/quickbridge-runtime/src/error.rs`. Match that convention; do not
  introduce stringly typed errors.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused tests | `cargo test -p quickbridge-core media::tests` | all media tests pass |
| Runtime tests | `cargo test -p quickbridge-runtime ffmpeg::tests` | all FFmpeg tests pass |
| Check | `cargo check-all` | exit 0 |
| Lint | `cargo lint` | exit 0, no warnings |
| Full suite | `cargo xtest` | exit 0; baseline currently 59 tests |
| Packaging | `cargo package --allow-dirty` | exit 0 |

## Suggested executor toolkit

- Use the `diagnosing-bugs` skill if available when validating profile-specific
  behavior.
- Primary references:
  - Apple HLS authoring specification:
    https://developer.apple.com/documentation/http-live-streaming/hls-authoring-specification-for-apple-devices/
  - Apple HLS codec appendixes:
    https://developer.apple.com/documentation/http-live-streaming/hls-authoring-specification-for-apple-devices-appendixes/
  - FFmpeg bitstream-filter docs:
    https://ffmpeg.org/ffmpeg-bitstream-filters.html

## Scope

**In scope**:
- `crates/quickbridge-core/src/media.rs`
- `crates/quickbridge-runtime/src/ffmpeg.rs`
- `crates/quickbridge-runtime/src/error.rs` only if a new typed unsupported
  video error belongs there
- `crates/quickbridge-core/tests/fixtures/` (create minimized JSON fixtures)
- Tests colocated in the files above
- `plans/README.md` status only

**Read-only evidence**:
- `sample-jsons/*.json`

**Out of scope**:
- Video transcoding or VideoToolbox fallback
- Dolby Vision Profile 7 conversion
- Subtitle/chapter preservation
- Audio policy changes
- UI redesign
- Restoring the deleted legacy root `src/` tree

## Git workflow

- Suggested branch: `codex/001-video-capabilities`
- Keep the user's existing dirty refactor intact.
- Use conventional commit wording such as
  `fix: classify HDR and Dolby Vision video packaging` if asked to commit.
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Preserve structured video metadata

Extend ffprobe deserialization and `VideoStream` so application logic retains:

- codec name, profile, level, codec tag, pixel format, dimensions and frame
  rate;
- color primaries, transfer characteristic and matrix;
- `disposition.attached_pic`;
- Dolby Vision configuration record fields from `side_data_list`: profile,
  level, base-layer presence, enhancement-layer presence and base-layer signal
  compatibility ID.

Use typed structs/enums and accessors. Keep `display_line` rendering behavior
compatible. Exclude `attached_pic=1` streams from `MediaInfo.videos`.

Create minimized fixtures under `crates/quickbridge-core/tests/fixtures/`
derived from the supplied JSON for: ordinary HEVC BT.709, HDR10 PQ, DV P5,
and DV P5 plus attached cover art. Do not copy irrelevant titles or metadata.

**Verify**: `cargo test -p quickbridge-core media::tests` -> new parsing tests
prove the correct DV profile and color classification and prove attached art is
excluded.

### Step 2: Introduce an explicit Apple packaging decision

Add a typed decision such as `VideoPackaging` with at least these outcomes:

- AVC stream copy, no forced HEVC tag;
- HEVC/HDR10/HLG stream copy tagged `hvc1`;
- Dolby Vision Profile 5 stream copy tagged `dvh1` and allowed to request
  FFmpeg's unofficial muxing mode when necessary;
- Dolby Vision Profile 8 with compatibility ID 1 or 4 tagged `hvc1` (DV master
  playlist signaling is deferred);
- unsupported Dolby Vision profile, unsupported codec (for example VC-1), or
  insufficient metadata: typed failure, never silent HEVC assumptions.

Classification must be based on structured fields, never filenames or the
human display line. Unknown DV profiles must fail closed with a clear message.

**Verify**: `cargo test -p quickbridge-core media::tests` -> table-driven tests
cover every outcome, including DV P7 rejection and VC-1 rejection.

### Step 3: Apply the decision to FFmpeg command construction

Pass the packaging decision into `build_args`. Preserve `-c:v copy`. Emit:

- `-tag:v hvc1` for ordinary compatible HEVC, HDR10/HLG, and the explicitly
  supported Profile 8 compatible-base route;
- `-tag:v dvh1` for Dolby Vision Profile 5;
- `-strict unofficial` only where the installed FFmpeg actually requires it.

Do not add `+faststart`, `-brand mp42`, chapter mapping, subtitle mapping, or
video transcoding. Rendered diagnostic commands must show the chosen tag.

**Verify**: `cargo test -p quickbridge-runtime ffmpeg::tests` -> command tests
assert `dvh1` for P5, `hvc1` for HDR10/ordinary HEVC, no forced HEVC tag for
H.264, and no spawn command for an unsupported route.

### Step 4: Run repository gates

**Verify**: run `cargo check-all`, `cargo lint`, `cargo xtest`, and
`cargo package --allow-dirty`; every command exits 0.

## Test plan

- Parse minimized real-derived fixtures for HDR10, DV P5 and attached art.
- Classify H.264, HEVC BT.709, HDR10, HLG, DV P5, compatible DV P8, DV P7,
  VC-1, and unknown video.
- Assert exact FFmpeg tags without asserting unrelated argument order.
- Model tests after existing `media::tests` and `ffmpeg::tests` modules.

## Done criteria

- [ ] No video decision depends on filenames or display strings.
- [ ] Attached pictures are absent from selectable video tracks.
- [ ] DV P5 produces `-tag:v dvh1`.
- [ ] Compatible ordinary HEVC/HDR produces `-tag:v hvc1`.
- [ ] Unsupported profiles/codecs return typed, user-readable errors.
- [ ] `cargo check-all`, `cargo lint`, `cargo xtest`, and package dry run pass.
- [ ] No out-of-scope files changed except `plans/README.md` status.

## STOP conditions

- The supplied P5 fixture no longer contains a DOVI configuration record.
- FFmpeg 8.x rejects `dvh1` fMP4 even with `-strict unofficial`; capture the
  exact stderr and stop rather than adding an unverified workaround.
- Correct Profile 8 handling requires RPU transformation rather than packaging
  metadata only; defer it instead of invoking external tools.
- Implementation requires video transcoding or changes to audio policy.

## Maintenance notes

Future codec support must add an explicit classification case and fixture.
Reviewers should scrutinize unknown-profile behavior, because silently copying
new Dolby Vision variants is more dangerous than rejecting them. Master
playlist DV signaling remains Plan 004 validation/follow-up territory.

