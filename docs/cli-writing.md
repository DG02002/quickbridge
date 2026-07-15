# CLI Writing Guide

This guide defines the writing standard for `quickbridge`. It is derived from
the Apple HIG writing guidance in this repository and applies to help text,
status output, notices, logs, and errors.

## Principles

- Use sentence case.
- Use plain, direct language.
- Put the important information first.
- Avoid jargon when a simpler term works.
- Avoid blame.
- Avoid "we".
- Offer a clear next step when possible.
- Use inclusive language throughout the CLI.

## Product Terms

Use these terms consistently:

- `source URL`
- `stream URL`
- `timestamp`
- `playhead`
- `session`
- `audio track`
- `video track`

## Error Messages

- Write errors in a calm, factual tone.
- State what failed before adding details.
- Prefer `Unable to ...` for actionable runtime failures.
- Keep the first line short enough to scan quickly in a terminal.
- Write user-facing errors to `stderr`.
- Reserve stack-like detail for debug output and chained causes.

Examples:

- `Unable to use 'ffmpeg'. Install ffmpeg and make sure the executable is available on PATH.`
- `quickbridge requires an interactive terminal. Run it from Terminal, iTerm, or another local shell session.`

## Help and Status Text

- Use short labels that are easy to scan.
- Keep command descriptions parallel.
- Use the same label for the same concept everywhere.
- Prefer `Show`, `Stop`, `Start`, `Switch`, and `Open` over more vague verbs.

## Inclusive Language

- Avoid violent, oppressive, or ableist metaphors.
- Avoid filler like `Oops`, `Uh-oh`, or `We hit a problem`.
- Avoid unnecessary possessive pronouns such as `your` when context is clear.

## Output Classes

- `stdout`: user workflow output, status, prompts, and help
- `stderr`: errors and debug logs
- `--verbose` or `RUST_LOG`: diagnostic detail for debugging

## Typography in a Terminal

`quickbridge` runs in a terminal, so it does not control the font family, font size,
or rendering engine. For terminal UI, apply the Apple typography principles through
layout and hierarchy instead of custom type styling.

- Keep headings short and consistent.
- Use sentence case for section labels and notices.
- Use fixed-width alignment for status labels so values are easy to scan.
- Avoid decorative ASCII art that reduces legibility.
- Avoid all caps except where external standards require them.
- Keep transient messages short so they remain readable in narrow terminal windows.
- Prefer one level of emphasis at a time: heading, label, or value.
