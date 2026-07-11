# Plan 008: Keep recoverable source and player errors inside the TUI

> **Executor instructions**: Implement only explicitly recoverable paths. Run
> all gates and stop if cleanup/ownership is ambiguous. Update
> `plans/README.md` when complete.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates/quickbridge-ui/src/app.rs crates/quickbridge-ui/src/error.rs crates/quickbridge-ui/src/render/mod.rs crates/quickbridge-runtime/src/prepare.rs crates/quickbridge-runtime/src/probe.rs`.
> Plans 005 and 007 must be reconciled before proceeding.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: Plans 005 and 007
- **Category**: bug/ux
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

Invalid launcher input receives useful inline guidance, but network/probe
failures during source preparation exit the TUI entirely. The `reopen` command
also propagates an error and terminates the session instead of preserving
working playback state. Recoverable failures should stay close to the action,
retain the source/input, and offer a clear retry or edit path.

## Current state

- `app.rs:359-389` handles malformed launcher commands locally and clearly.
- `app.rs:46-65` returns preparation errors from `run_interactive`, dropping
  the TUI and requiring a restart.
- `app.rs:315-319` uses `?` for `reopen_player`; a failed reopen ends the app.
- Jump failures are already handled well at `app.rs:340-350`: progress clears,
  the old session survives, and an actionable history warning appears. Match
  this pattern.
- `UiError` is typed, but no screen state represents a recoverable source
  error.
- Retrying preparation may require making `ProbeRunner` cloneable or borrowing
  it; it contains only an executable path, but preserve injected-test behavior.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| UI tests | `cargo test -p quickbridge-ui` | all pass |
| Runtime tests | `cargo test -p quickbridge-runtime` | all runtime tests pass |
| Snapshot gate | `cargo test -p quickbridge-ui render::tests` | committed snapshots match; `.snap.new` files are reviewed before acceptance |
| Check/lint | `cargo check-all && cargo lint` | exit 0 |
| Full suite | `cargo xtest` | exit 0 |

## Scope

**In scope**:
- `crates/quickbridge-ui/src/app.rs`
- `crates/quickbridge-ui/src/error.rs`
- `crates/quickbridge-ui/src/render/mod.rs`
- Error-state snapshots/tests
- `crates/quickbridge-runtime/src/prepare.rs` and `probe.rs` only to make a
  safe retry seam
- `plans/README.md` status only

**Out of scope**:
- Retrying partially started FFmpeg/server sessions without proven cleanup
- Automatic infinite retries
- Hiding verbose diagnostics or changing exit codes
- A general notification framework
- Media pipeline changes

## Git workflow

- Suggested branch: `codex/008-recoverable-tui-errors`
- Suggested commit if requested: `fix: keep recoverable failures in the TUI`.
- Do not push/open a PR unless instructed.

## Before and after

| Before | After |
|---|---|
| Prepare/network failure exits and loses launcher context | Error state retains URL and offers Retry, Edit URL, or Quit |
| `reopen` failure terminates an otherwise live session | Warning remains in dashboard with an explicit retry action |
| Error output is mostly a chain after TUI teardown | Primary message and next step appear near the failed action; details remain available |

## Steps

### Step 1: Model recoverable source preparation failure

Add a typed screen/state containing the attempted URL, concise user-facing
summary, optional diagnostic detail, and input/actions for Retry, Edit URL, and
Quit. Map known preparation errors to actionable language without matching on
formatted error strings. Preserve the underlying typed error for verbose
details.

Do not make partially started playback failures retryable until resource
cleanup is proven; those may remain fatal with clear messaging.

**Verify**: state tests cover invalid URL, unavailable probe, HTTP inspection
warning/failure where applicable, malformed probe output, and no video track.

### Step 2: Add a safe preparation retry seam

Retain or recreate the exact configured `ProbeRunner` safely across attempts;
do not silently discard an injected binary path. Retry the same URL or return
to editable launcher input without recursively calling `run_interactive`.
Clear stale progress/history for the new attempt while retaining the prior
error as one concise activity item.

**Verify**: a fake probe fails once then succeeds; one TUI state machine run
reaches track selection without restarting the process.

### Step 3: Keep player action failures nonfatal

Handle `reopen_player` like jump failure: keep the current playback/session,
update a dashboard warning with the actual action that failed, and allow retry.
Do not swallow errors silently. If QuickTime status is unavailable, show that
state with `reopen` guidance rather than terminating.

**Verify**: a test driver makes reopen fail once then succeed; the first attempt
leaves `Screen::Running`, the second clears/replaces the warning.

### Step 4: Render accessible recovery actions

Use the fixed dashboard/error region from Plan 007. Provide text labels and
keyboard keys, never color alone. Keep the primary message plain and concise;
show raw diagnostics only in verbose/details view. Follow
`docs/cli-writing.md` and the existing “Couldn't …” language pattern.

**Verify**: snapshots at 80x24 and 60x18 show message, failed action, and Retry,
Edit URL, Quit choices without truncating them.

### Step 5: Run all gates

**Verify**: UI/runtime focused tests, reviewed snapshots, `cargo check-all`,
`cargo lint`, and `cargo xtest` pass.

## Test plan

- Prepare fail-once/succeed-on-retry using injected fake probe.
- Edit URL returns to launcher with URL available for correction.
- Reopen fail-once/succeed while playback state remains running.
- Fatal partial-start failure remains fatal unless cleanup is verified.
- Normal and verbose error rendering at narrow widths.

## Done criteria

- [ ] Recoverable preparation failures do not require process restart.
- [ ] Retry preserves configured probe behavior and succeeds in tests.
- [ ] Reopen failure leaves the active session intact.
- [ ] Every recoverable error provides an explicit next action.
- [ ] Raw diagnostics are available but do not dominate normal mode.
- [ ] Focused and full repository gates pass.

## STOP conditions

- Retry would reuse or leak a partially consumed process/server resource.
- Preserving an injected `ProbeRunner` requires global state or loses the
  configured binary path.
- A supposedly recoverable start failure cannot prove cleanup of temp files,
  FFmpeg child and local server.
- Error classification requires matching strings rather than typed variants;
  add/narrow typed runtime errors first or report the blocker.

## Maintenance notes

Every new recoverable operation should define rollback state and a retry test.
Do not turn programmer/configuration errors into endless retry loops. Reviewers
should verify the working playback session survives command-level failures.
