# Plan 006: Make track selection focused, compact, and keyboard-clear

> **Executor instructions**: Execute step by step, run every gate, and stop on
> any STOP condition. Update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates/quickbridge-core/src/media.rs crates/quickbridge-ui/src/app.rs crates/quickbridge-ui/src/render/mod.rs crates/quickbridge-ui/src/render/snapshots`.
> Plans 001 and 005 must be reconciled before editing.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: `plans/001-classify-video-and-package-for-apple.md`,
  `plans/005-honor-terminal-presentation-mode.md`
- **Category**: ux/accessibility
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

The track selector marks both the chosen video and chosen audio with `>`, but
does not visibly identify which section currently owns arrow-key focus. Users
can press Up/Down and change a different group than they expect. Long ffprobe
descriptions and many language tracks also make 2160p files difficult to scan
in a small terminal.

## Current state

- `crates/quickbridge-ui/src/app.rs:516-529` stores `SelectionFocus`, video
  index, and audio index correctly.
- `crates/quickbridge-ui/src/app.rs:872-918` moves only the focused section and
  switches focus with Left/Right/Tab.
- `crates/quickbridge-ui/src/render/mod.rs:152-190` renders group headings but
  does not style or label active focus.
- `crates/quickbridge-ui/src/render/mod.rs:314-325` uses the same `>` marker for
  selected rows in both active and inactive groups.
- The 80x24 snapshot visibly contains two `>` markers and footer text only
  says `Arrow keys move • Enter confirm`.
- `track_selection_scroll_offset` scrolls the whole screen to the current row;
  instructions can disappear when many tracks exist.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| UI tests | `cargo test -p quickbridge-ui render::tests` | all render tests pass |
| Core tests | `cargo test -p quickbridge-core media::tests` | all media tests pass |
| Snapshot gate | `cargo test -p quickbridge-ui render::tests` | committed snapshots match; `.snap.new` files are reviewed before acceptance |
| Check/lint | `cargo check-all && cargo lint` | exit 0 |
| Full suite | `cargo xtest` | exit 0 |

## Suggested executor toolkit

- Use `make-interfaces-feel-better` as a hierarchy/focus lens; ignore its
  browser-only animation and pointer guidance.
- Use `tdd` for focus and viewport behavior.

## Scope

**In scope**:
- `crates/quickbridge-ui/src/app.rs`
- `crates/quickbridge-ui/src/render/mod.rs`
- Track-selection snapshots
- `crates/quickbridge-core/src/media.rs` only for concise structured labels
  already enabled by Plan 001
- Tests in those modules
- `plans/README.md` status only

**Out of scope**:
- Changing actual default-track selection
- Adding audio/video transcoding choices
- Mouse support
- A full settings screen
- Parsing display strings to recover metadata

## Git workflow

- Suggested branch: `codex/006-track-selection-ux`
- Suggested commit if requested: `feat: clarify track selection focus`.
- Do not push/open a PR unless instructed.

## Before and after

| Before | After |
|---|---|
| Both chosen rows use `>`; active group is ambiguous | Active cursor and chosen value use distinct, non-color-only markers |
| Full ffprobe sentences dominate the list | Compact primary labels with optional muted detail |
| Footer can scroll away | Keyboard guidance remains fixed and names section switching |
| Tested mainly at 80x24 | Snapshots cover 120x40, 80x24, 60x18, and long lists |

## Steps

### Step 1: Define non-color-only focus semantics

Render exactly one active cursor marker, such as `›`, in the focused group.
Render the currently chosen row in the inactive group with a separate check
marker such as `✓`. Style the active group heading distinctly, but never rely
on color alone. Keep focus state and selected indices separate in rendering.

Update footer copy to explicitly state controls, for example:
`↑↓ choose • Tab switch section • Enter play`.

**Verify**: state/render tests assert exactly one active cursor at a time and
prove Tab changes the active section without losing either selection.

### Step 2: Render compact structured track labels

Use structured metadata from Plan 001 to produce a concise first line:

- video: codec, resolution, SDR/HDR10/HLG/DV profile, default marker;
- audio: language/title when available, codec, channel layout, Atmos when
  known, default marker.

Keep less important technical detail muted on a continuation line only when
the viewport permits it. Do not parse `display_line` strings.

**Verify**: fixture-backed tests cover DV P5, HDR10, E-AC-3 Atmos, TrueHD,
missing language, and long titles at narrow widths.

### Step 3: Keep title and footer stable while the list scrolls

Use Ratatui layout regions for header, scrollable choices, and fixed footer.
Ensure the focused row stays visible for long lists. Clamp safely when terminal
height is too small and render a concise `Terminal too small` instruction
instead of slicing/panicking.

**Verify**: snapshot tests at 120x40, 80x24 and 60x18 plus a long-list test show
the focused row and footer. A tiny-height test does not panic.

### Step 4: Run repository gates

**Verify**: focused tests, intentional snapshot review followed by `cargo test -p quickbridge-ui`, `cargo check-all`,
`cargo lint`, and `cargo xtest` pass.

## Test plan

- Focus in Video and Audio states, both selected values preserved.
- Long lists scroll active row into view.
- Compact labels use structured data and degrade gracefully when fields are
  missing.
- No-color assertion uses text markers, not terminal color inspection.
- Tiny terminal fallback is deterministic.

## Done criteria

- [ ] Exactly one active focus cursor is visible.
- [ ] Chosen values remain visible in inactive groups without looking active.
- [ ] Footer names every supported selection control and stays visible.
- [ ] Labels clearly expose HDR/DV and audio compatibility facts.
- [ ] Narrow/long-list snapshots pass and no tiny-terminal panic occurs.
- [ ] Full check/lint/test gates pass.

## STOP conditions

- Plan 001 does not expose structured HDR/DV/audio fields needed for labels.
- A fixed footer cannot coexist with inline viewport behavior from Plan 005.
- Proposed markers render inconsistently in the supported macOS terminal test
  set; fall back to ASCII markers and report results.
- Implementation changes default selection or FFmpeg mapping behavior.

## Maintenance notes

Any new selectable media property should update concise-label tests. Reviewers
must check monochrome output and narrow terminals, not only the 120-column
colored snapshot.
