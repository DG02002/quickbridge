# Plan 009: Replace command-shell interaction with native TUI actions

> **Executor instructions**: This is the final UI integration plan. Execute it
> only after Plans 005-008 are reconciled. Follow each gate, stop on scope
> drift, and update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 112ebb1..HEAD -- crates/quickbridge-core/src/command.rs crates/quickbridge-ui/src/app.rs crates/quickbridge-ui/src/event.rs crates/quickbridge-ui/src/render/mod.rs crates/quickbridge-ui/src/runtime.rs crates/quickbridge-ui/src/render/snapshots README.md docs/cli-writing.md`.
> Compare the Current state facts against the live post-Plan-008 code.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: Plans 005, 006, 007, and 008
- **Category**: ux/direction
- **Planned at**: commit `112ebb1` plus dirty worktree, 2026-07-11

## Why this matters

QuickBridge's interactive mode currently looks like a TUI but behaves mainly
like a command shell: the launcher teaches `/url`, the running screen expects
typed `status`, `help`, `reopen`, and `quit`, and output is appended to history.
The application already has a real screen/state model, so it should expose
focused fields, direct shortcuts, overlays, and explicit actions. Scripted CLI
mode remains valuable for automation and must keep its command parser.

## Current state

- `crates/quickbridge-ui/src/app.rs:537-555` seeds launcher history with
  `/url <media-url>`, `help`, and `quit` instructions.
- `app.rs:359-389` parses the launcher as a mini command shell even though a
  pasted URL is the primary task.
- `app.rs:290-356` sends every running input line through the core command
  parser and appends command/results to history.
- `crates/quickbridge-core/src/command.rs:24-65` is also used by scripted mode;
  preserve it for automation and backwards-compatible scripts.
- Plans 006-008 provide focused selection, stable dashboard regions, and
  recoverable error actions. This plan connects them into one coherent input
  and navigation model.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| UI tests | `cargo test -p quickbridge-ui` | all pass |
| CLI tests | `cargo test -p quickbridge --test cli` | all pass |
| Snapshot gate | `cargo test -p quickbridge-ui render::tests` | committed snapshots match; `.snap.new` files are reviewed before acceptance |
| Check/lint | `cargo check-all && cargo lint` | exit 0 |
| Full suite | `cargo xtest` | exit 0 |

## Suggested executor toolkit

- Use `make-interfaces-feel-better` for hierarchy, focus, concise feedback, and
  stable dynamic data; ignore browser-only effects.
- Use `tdd` for keyboard routing and state transitions.
- Follow `docs/cli-writing.md` for action labels and help copy.

## Scope

**In scope**:
- `crates/quickbridge-ui/src/app.rs`
- `crates/quickbridge-ui/src/event.rs`
- `crates/quickbridge-ui/src/render/mod.rs`
- `crates/quickbridge-ui/src/runtime.rs` only for cursor/focus rendering
- UI snapshots/tests
- `README.md` and `docs/cli-writing.md` for interactive controls
- `crates/quickbridge-core/src/command.rs` tests only if needed to prove the
  scripted parser remains unchanged
- `plans/README.md` status only

**Out of scope**:
- Removing or breaking `--script`
- Mouse support
- A general plugin/command system
- Media pipeline, server, or QuickTime automation changes
- New persistent settings

## Git workflow

- Suggested branch: `codex/009-native-tui-interactions`
- Suggested commit if requested: `feat: make interactive mode a native TUI`.
- Do not push or open a PR unless instructed.

## Before and after

| Before | After |
|---|---|
| Launcher teaches `/url …` | Focused URL field; paste/type and Enter starts |
| Running screen requires typed `status` | Dashboard is always visible; Details opens a panel |
| Typed `help` appends text | `?` opens a dismissible contextual help overlay |
| Typed `reopen` and `quit` are primary | Direct labeled shortcuts/actions |
| One generic text composer handles every intent | Dedicated jump entry plus global actions and visible focus |
| Scripted and interactive surfaces share command presentation | Scripted CLI keeps parser; TUI gets native controls |

## Steps

### Step 1: Define a context-aware action model

Introduce a typed UI action enum separate from `quickbridge_core::Command`.
At minimum model: submit URL, move/switch/confirm selection, open jump entry,
submit jump, cancel modal/input, toggle details, open/close help, reopen player,
retry/edit source error, and quit. Map keys by current screen and focus.

Keep `quickbridge_core::parse_command` unchanged for scripted CLI mode. The TUI
may optionally provide a secondary `:` command palette for expert parity, but
it must not be the primary or only route to actions.

**Verify**: table-driven tests map keys to actions for launcher, selection,
running, jump entry, help, details, and error screens. Context-dependent keys
must always be named in the visible footer/help for that context.

### Step 2: Turn the launcher into a focused URL form

Replace launcher command history with a clear purpose, one URL field, concise
format hint, and primary action. Paste or typing a full URL followed by Enter
starts inspection. Keep validation adjacent to the field and preserve the URL
after an error. Provide visible Quit and Help shortcuts without typing words.

Render a caret/focus indication using Ratatui cursor positioning or a non-color
marker. Add a reusable single-line editor supporting Backspace, Left/Right,
Home/End, clear line, paste, and a horizontal viewport for long URLs. Do not
keep raw `String::push/pop` as the only editing behavior.

**Verify**: tests cover editing in the middle, paste, long-URL viewport,
invalid URL guidance, retry preservation, and submit.

### Step 3: Use dedicated jump entry and global running actions

Make the live dashboard the default focus. A visible shortcut activates a
dedicated jump field accepting existing absolute/relative time syntax. Enter
submits; Escape cancels without changing playback. Provide direct actions for
Details, Help, Reopen when relevant, and Quit. Disable/hide Jump when ranges
are unavailable and explain why beside the action.

Reuse the existing timecode parser behind the jump field.

**Verify**: tests cover successful, cancelled, invalid, and range-disabled jump
states plus paused/closed-player actions and modal focus isolation.

### Step 4: Add contextual help and details overlays

Render bounded panels that do not append history. Help changes by screen and
lists only available actions. Details presents source, selected tracks,
session/stream identifiers, playback metrics, and HDR/DV classification from
prior plans. Escape restores previous focus.

Use text labels and symbols in addition to color. Do not add animated
transitions; immediate state changes suit terminal interaction.

**Verify**: snapshots cover launcher help, running help, details, and error
actions at 120x40, 80x24, and 60x18. Every panel shows its dismissal key.

### Step 5: Preserve scripted CLI compatibility

Run existing scripted tests unchanged. Add a regression asserting that
`--script status --script quit <URL>` still uses the core parser and never
enters raw/alternate-screen UI. README must present two explicit modes:
interactive full TUI and scripted automation.

**Verify**: `cargo test -p quickbridge --test cli` passes with existing syntax
and no ANSI/raw-mode noise.

### Step 6: Run all gates and the manual flow

Manually complete: launch, URL, inspection, selection, playback, details, jump,
close QuickTime, reopen, help, and quit. Repeat essentials at 80x24 and once in
the inline fallback.

**Verify**: reviewed snapshots, `cargo check-all`, `cargo lint`, and
`cargo xtest` pass; the manual checklist has no hidden action.

## Test plan

- Context-specific keyboard/action mapping.
- Single-line editor behavior and long URL viewport.
- Dedicated jump mode including disabled/cancelled/error cases.
- Focus restoration after help/details/error panels.
- Full state transitions without real integrations.
- Scripted CLI compatibility and no ANSI noise.
- Wide, normal, compact, and monochrome-readable snapshots.

## Done criteria

- [ ] Interactive startup no longer teaches `/url` as the primary path.
- [ ] Live status is visible without typing `status`.
- [ ] Help/details are bounded dismissible views, not transcript output.
- [ ] Jumping has a dedicated focused input with cancel behavior.
- [ ] Primary actions have visible shortcuts and non-color focus.
- [ ] Text input supports cursor editing beyond append/backspace.
- [ ] Scripted CLI syntax and tests remain compatible.
- [ ] Full UI/CLI/check/lint/test gates pass.

## STOP conditions

- Native actions would require removing/changing the public scripted contract.
- Keybindings conflict with text entry and visible focus cannot resolve them.
- Inline fallback cannot expose an essential action; provide a simple menu or
  prompt fallback instead of restoring the transcript shell.
- Ratatui cursor placement cannot be restored safely on errors/panic.
- A proposed interaction changes playback semantics rather than presentation.

## Maintenance notes

New interactive features should add typed actions and contextual help, not new
magic command strings. Keep scripted commands as an automation API and TUI
actions as the human interface; both should call the same domain operations.
