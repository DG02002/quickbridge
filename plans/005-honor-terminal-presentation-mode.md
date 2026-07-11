# Plan 005: Make the full-screen TUI the primary interactive surface

> **Executor instructions**: Follow the steps and run every verification gate.
> Stop on a STOP condition and update this plan's row in `plans/README.md` when
> complete.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates/quickbridge/src/app.rs crates/quickbridge/src/terminal_detection.rs crates/quickbridge-ui/src/app.rs crates/quickbridge-ui/src/lib.rs crates/quickbridge-ui/src/runtime.rs crates/quickbridge-ui/src/terminal_detection.rs`.
> This plan targets the current dirty crate refactor; compare the facts below
> with live code before editing.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug/ux
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

The interactive product has launcher, inspection, track selection, startup,
running, jump, help, and error states, so a full-screen TUI is a better fit than
an appended command transcript. The CLI already exposes `--no-alt-screen`, but
the TUI always starts inline, leaving the intended full-screen experience
unreachable. Alternate-screen mode should become the primary interactive
surface; inline mode remains an explicit compatibility fallback, and scripted
CLI mode remains separate for automation.

## Current state

- `crates/quickbridge/src/app.rs:39-48` passes `cli.no_alt_screen` to the UI.
- `crates/quickbridge-ui/src/app.rs:29-31` ignores it and hardcodes
  `use_alt_screen: false`.
- `crates/quickbridge-ui/src/runtime.rs:27-64` already supports alternate and
  inline viewports and cleans both up in `Drop`.
- `crates/quickbridge-ui/src/terminal_detection.rs:40-46` already models the
  intended policy: use alternate screen for a supported terminal unless
  `no_alt_screen` is true or Zellij is detected. The module is currently
  compiled only in tests from `lib.rs:8-10`.
- `crates/quickbridge/src/terminal_detection.rs` duplicates only the basic
  interactive-terminal subset. Avoid creating a third policy.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused UI tests | `cargo test -p quickbridge-ui terminal_detection` | all selected tests pass |
| UI suite | `cargo test -p quickbridge-ui` | exit 0; baseline 9 tests |
| Check | `cargo check-all` | exit 0 |
| Lint | `cargo lint` | exit 0, no warnings |
| Full suite | `cargo xtest` | exit 0 |

## Suggested executor toolkit

- Use `tdd` if available; the option-to-runtime seam is deterministic.
- Follow `docs/cli-writing.md` for any changed flag/help copy.

## Scope

**In scope**:
- `crates/quickbridge-ui/src/app.rs`
- `crates/quickbridge-ui/src/lib.rs`
- `crates/quickbridge-ui/src/runtime.rs`
- `crates/quickbridge-ui/src/terminal_detection.rs`
- `crates/quickbridge/src/app.rs` and
  `crates/quickbridge/src/terminal_detection.rs` only if needed to remove
  duplicated policy cleanly
- Tests in those modules
- `plans/README.md` status only

**Out of scope**:
- Dashboard visual redesign
- New CLI flags
- Changing Zellij behavior without evidence
- Shell/terminal-specific hacks
- Restoring the deleted legacy root `src/`

## Git workflow

- Suggested branch: `codex/005-terminal-presentation`
- Preserve the user's dirty refactor.
- Suggested commit if requested: `fix: honor terminal presentation mode`.
- Do not push or open a PR unless instructed.

## Before and after

| Before | After |
|---|---|
| `InteractiveOptions.no_alt_screen` is ignored | It deterministically controls runtime viewport selection |
| Every supported terminal uses inline mode | Normal terminals use a full-screen alternate-screen TUI; `--no-alt-screen` is a reduced fallback |
| Terminal capability policy is duplicated and test-only in the UI | One production policy is exercised by unit tests |
| Interactive and scripted presentation both feel command-oriented | Interactive mode gets a dedicated TUI shell; scripted mode stays automation-oriented |

## Steps

### Step 1: Turn terminal-mode selection into a tested production function

Compile the UI terminal-detection module in production and expose the smallest
crate-local/public seam required to choose `RuntimeOptions`. Keep environment
detection injectable for tests. Consolidate duplicate decision logic only when
the ownership boundary is clear: the binary may retain the basic "is this an
interactive terminal?" check, while the UI owns presentation mode.

Treat alternate-screen mode as the supported default. Validate modern Zellij
rather than automatically assuming it must be inline: if alternate-screen
entry/exit works correctly in the supported Zellij version, use the full TUI;
otherwise retain the existing fallback and document the reason/tested version.

**Verify**: focused tests cover normal terminal, `--no-alt-screen`, Zellij,
`TERM=dumb`, and non-TTY inputs with exact expected booleans.

### Step 2: Wire `InteractiveOptions.no_alt_screen` into runtime entry

Replace the hardcoded `false` in `run_interactive` with the tested decision.
Do not change the runtime's cleanup semantics. Make the computed mode explicit
enough that tests do not need to inspect ANSI output.

**Verify**: add a testable helper from `InteractiveOptions`/terminal facts to
`RuntimeOptions`; all truth-table cases pass.

### Step 3: Establish full-screen and fallback presentation contracts

Document in code/tests that full-screen mode is the primary interactive
experience. Inline mode must preserve basic launch, selection, playback status,
jump entry, errors, and quit, but it does not need identical multi-panel layout
or retained activity navigation. Scripted `--script` behavior remains outside
the TUI and must not enter raw or alternate terminal mode.

Remove or defer launcher wording that promises terminal-buffer history when
running full screen; Plan 009 replaces the command-shell launcher copy.

**Verify**: tests prove full-screen interactive selection, explicit inline
fallback, and scripted-mode separation without inspecting ANSI sequences.

### Step 4: Verify cleanup and repository gates

Ensure alternate-screen entry failures still restore cursor/raw mode through
the existing error paths. Do not add `unwrap`/`expect` to terminal operations.

**Verify**: `cargo test -p quickbridge-ui`, `cargo check-all`, `cargo lint`, and
`cargo xtest` all exit 0.

## Test plan

- Unit-test the full presentation-mode truth table.
- Retain existing terminal-detection tests as structural examples.
- Add a runtime-options integration seam without requiring a real terminal.
- Manual smoke gate: normal Terminal/iTerm opens a full-screen TUI;
  `--no-alt-screen` preserves terminal-buffer history; both restore the cursor.

## Done criteria

- [ ] No hardcoded `use_alt_screen: false` remains in `run_interactive`.
- [ ] `--no-alt-screen` selects inline mode.
- [ ] Zellij behavior is based on a recorded smoke test; full screen is used
  when safe and fallback behavior is documented otherwise.
- [ ] Normal supported terminals select alternate screen.
- [ ] Scripted CLI mode never enters the TUI runtime.
- [ ] Automated UI tests, check, lint, and full suite pass.
- [ ] Only in-scope files and plan status changed.

## STOP conditions

- Real terminal capability facts cannot be injected without changing the
  public CLI contract.
- Alternate mode corrupts/restores the terminal incorrectly in a manual smoke
  test; capture exact terminal and escape behavior before continuing.
- Zellij detection no longer matches the environment variable documented in
  current code.

## Maintenance notes

The UI crate should own presentation choice; the binary should only decide
whether interactive operation is permitted. Re-test cursor/raw-mode cleanup
whenever runtime entry or panic handling changes.
