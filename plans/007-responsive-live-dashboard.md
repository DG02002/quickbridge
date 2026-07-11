# Plan 007: Replace the running transcript with a responsive live dashboard

> **Executor instructions**: Follow every step and gate. This is a layout and
> state-presentation change, not a media-runtime rewrite. Stop on scope drift
> and update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates/quickbridge-ui/src/app.rs crates/quickbridge-ui/src/render/mod.rs crates/quickbridge-ui/src/runtime.rs crates/quickbridge-ui/src/text.rs crates/quickbridge-core/src/playback.rs crates/quickbridge-ui/src/render/snapshots`.
> Plans 002, 003 and 005 must be reconciled first.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: Plans 002, 003, and 005
- **Category**: ux/perf
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

The running screen permanently replays inspection/startup history before live
status, jump progress, and input. At 120x40 it is already visually dominated by
completed setup; at smaller sizes the renderer bottom-scrolls the entire
document. In the primary full-screen TUI, live playback health and focused
actions should remain stable while history becomes a bounded secondary region.
Inline compatibility mode may use a simpler condensed renderer rather than
imitating the full dashboard as a transcript.

## Current state

- The running snapshot shows 12 lines of completed setup before session data.
- `render/mod.rs:193-234` concatenates setup history, command history, warning,
  one dense metric line, jump progress, and input into a single vector.
- `render/mod.rs:291-311` computes a bottom scroll over the whole document.
- `app.rs:560-572` stores three separate unbounded history vectors.
- `app.rs:637-694` renders `status` as a large multiline block that is appended
  to history each time.
- `run_live_loop` redraws every 100 ms and refreshes the snapshot each second;
  rendering cost grows with unbounded history.
- Plan 003 renames the misleading download metric to relay write rate; use the
  final name rather than reintroducing `Download speed`.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| UI tests | `cargo test -p quickbridge-ui` | all pass |
| Snapshot gate | `cargo test -p quickbridge-ui render::tests` | committed snapshots match; `.snap.new` files are reviewed before acceptance |
| Check/lint | `cargo check-all && cargo lint` | exit 0 |
| Full suite | `cargo xtest` | exit 0 |

## Scope

**In scope**:
- `crates/quickbridge-ui/src/app.rs`
- `crates/quickbridge-ui/src/render/mod.rs`
- `crates/quickbridge-ui/src/text.rs`
- Running-screen snapshots/tests
- `crates/quickbridge-core/src/playback.rs` only for presentation-safe health
  data that cannot be derived in UI
- `plans/README.md` status only

**Out of scope**:
- Changing FFmpeg buffer/segment behavior
- Changing server telemetry collection
- Mouse support or web-style animation
- A persistent configuration system
- QuickTime automation changes

## Git workflow

- Suggested branch: `codex/007-live-dashboard`
- Suggested commit if requested: `feat: redesign the live session dashboard`.
- Do not push/open a PR unless instructed.

## Before and after

| Before | After |
|---|---|
| Completed setup occupies the primary running view | One compact ready/media summary; details remain in history/status |
| Metrics are one dense unlabeled sentence | Stable labeled health fields with responsive wrapping |
| Whole document bottom-scrolls | Header/status, activity, warning, and composer have explicit regions |
| `status` appends a large block repeatedly | `status` toggles or refreshes a bounded details panel |
| History grows without limit | History is bounded and scrollable without moving the composer |

## Steps

### Step 1: Define running-screen information hierarchy

Create explicit Ratatui regions in the primary full-screen mode in this order:

1. compact app/session header;
2. live playback status and health metrics;
3. current warning or jump progress;
4. bounded recent activity/details;
5. fixed command composer and keyboard hint.

Collapse completed inspection/startup into a single summary such as ready
state plus selected video/audio. Preserve detailed setup in the status/details
view; do not silently discard warnings.

Provide a separate compact inline renderer that shows the latest health,
warning/action, and input without attempting to preserve full-screen panel
geometry. Do not make the primary design lowest-common-denominator for the
fallback.

**Verify**: a 120x40 snapshot shows current time/player/buffer and the composer
without setup steps consuming the primary region.

### Step 2: Make health readable and responsive

Use explicit labels: `Player`, `Time`, `Buffer`, `Relay`, and `Storage`.
Represent playing/paused/closed/unavailable in text plus a symbol so status is
not color-only. Derive warning language from the final Plan 002 buffer policy;
do not invent a low-buffer threshold without a testable rationale.

At wide widths use compact columns; at narrow widths stack fields. Do not
truncate the current time or recovery action. Treat 80 columns as normal and
60 columns as supported compact mode.

**Verify**: snapshots at 120x40, 80x24, 60x18 and paused/window-closed states
contain all critical labels and next actions.

### Step 3: Bound and scroll activity independently

Replace unbounded rendered history with a bounded policy (for example, retain
the latest 200 logical entries) and a viewport offset. Add keyboard navigation
for older activity that does not interfere with command entry; document keys in
the composer hint. The fixed composer must never scroll away.

Preserve command/error grouping for multiline entries. Use terminal display
width rather than raw character count where wrapping decisions depend on it.

**Verify**: a test with 1,000 history entries retains the configured bound,
renders in constant bounded input size, and keeps the composer visible while
scrolling history.

### Step 4: Turn `status` into a bounded details view

Keep the public `status` command, but make it toggle/open a details region
instead of appending a full status dump on every invocation. A subsequent
status refresh may update the same region. `help` should likewise be a bounded
view or concise activity item, not permanent layout growth.

**Verify**: invoking `status` repeatedly does not increase history length;
tests prove details contain source, stream URL, session ID, tracks and player
state.

### Step 5: Run all gates

**Verify**: UI tests, reviewed snapshots, `cargo check-all`, `cargo lint`, and
`cargo xtest` pass.

## Test plan

- Wide, normal, compact, and too-small terminals.
- Playing, paused, closed, unavailable, low/healthy buffer when defined.
- Jump progress replaces only the activity/progress region.
- Bounded history and independent scroll.
- Repeated `status`/`help` commands do not grow the document without bound.
- Existing launcher/inspection/selection snapshots remain unchanged except
  where shared primitives intentionally improve them.

## Done criteria

- [ ] Live health and input remain visible at 80x24 and 60x18.
- [ ] Completed setup is summarized, not permanently expanded.
- [ ] Status uses accurate Plan 003 telemetry names.
- [ ] Activity history is bounded and independently scrollable.
- [ ] `status` does not append duplicate multiline blocks.
- [ ] Critical states use text/symbols in addition to color.
- [ ] UI, snapshot, check, lint and full tests pass.

## STOP conditions

- Plan 002/003 telemetry semantics are not final enough to label accurately.
- Inline mode cannot support fixed regions without corrupting terminal history;
  preserve a deliberately simpler inline renderer and report the divergence.
- Supporting 60x18 would require hiding the player state, current time, warning
  action, or composer.
- Layout work changes runtime playback behavior.

## Maintenance notes

Treat live health and composer as invariant regions. New metrics belong in the
details view unless they directly help the user decide whether playback is
healthy. Review snapshots in monochrome and at 80 columns.
