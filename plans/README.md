# QuickBridge 2160p HDR/Dolby Vision Plans

Generated with the `improve` skill on 2026-07-11. These plans target the
continuous lag and compatibility risks observed with high-bitrate 2160p HEVC,
HDR, and Dolby Vision sources. Execute them in order unless the dependency
notes say otherwise. Every executor must read its plan completely, honor the
STOP conditions, run every verification gate, and update its status here.

The plans were written against commit `112ebb1` plus the current dirty
workspace refactor. The `crates/` tree and `sample-jsons/` fixtures are
uncommitted at planning time; preserve them and do not restore the deleted
legacy `src/` tree.

## Execution order and status

| Plan | Title | Priority | Effort | Depends on | Status |
|---|---|---|---|---|---|
| 001 | Classify video capabilities and emit Apple-compatible tags | P1 | L | — | TODO |
| 002 | Make the UHD HLS relay resilient and keyframe-aligned | P1 | M | 001 | TODO |
| 003 | Stream HLS assets and remove duplicate hot-path filesystem work | P2 | M | 002 | TODO |
| 004 | Establish a real-media HDR/DV performance regression gate | P1 | M | 001, 002, 003 | TODO |
| 005 | Make the full-screen TUI the primary interactive surface | P1 | M | — | TODO |
| 006 | Make track selection focused, compact, and keyboard-clear | P1 | M | 001, 005 | TODO |
| 007 | Replace the running transcript with a responsive live dashboard | P1 | L | 002, 003, 005 | TODO |
| 008 | Keep recoverable source and player errors inside the TUI | P2 | M | 005, 007 | TODO |
| 009 | Replace command-shell interaction with native TUI actions | P1 | L | 005, 006, 007, 008 | TODO |

Status values: `TODO`, `IN PROGRESS`, `DONE`, `BLOCKED (<reason>)`, or
`REJECTED (<reason>)`.

## Dependency notes

- Plan 001 comes first because Plan 002 must choose `hvc1` versus `dvh1` from
  structured probe data rather than filenames or display strings.
- Plan 002 precedes Plan 003 because its larger, keyframe-aligned segments make
  the server's current whole-file allocation more expensive.
- Plan 004 is the release gate for the other plans. Unit tests can validate
  command construction, but only real HEVC/DV media and QuickTime can validate
  decoder behavior, sustained playback, and metadata preservation.
- Plan 005 is independent and can run alongside Plans 001-003. It fixes the
  currently ignored terminal-mode option and makes a full-screen alternate
  screen the primary interactive product surface. Inline mode remains a
  deliberately reduced compatibility fallback.
- Plan 006 follows Plan 001 so it can render concise, structured codec/HDR/DV
  labels rather than parsing ffprobe-style display strings.
- Plan 007 follows Plans 002 and 003 so its buffer/relay health language matches
  the final HLS policy and honest telemetry names.
- Plan 008 follows Plan 007 because recoverable errors need a stable dashboard
  region and composer rather than adding more lines to the current transcript.
- Plan 009 is the final interaction pass. It preserves scripted CLI mode for
  automation while replacing `/url`, `status`, `help`, `reopen`, and `quit` as
  the primary interactive vocabulary with fields, shortcuts, overlays, and
  explicit focused actions.

## Evidence summary

- `sample-jsons/2160p.SDR.BluRay.THD.7.1.mkv.ffprobe.json` is 3840x2160 HEVC
  Main 10 at an 85.2 Mb/s container rate. With the current six-second HLS
  window, only about 64 MB of media is advertised and `-re` gives no catch-up
  capacity after a network stall.
- `sample-jsons/2160p.DV.P5.EAC3.mkv.ffprobe.json` contains Dolby Vision
  Profile 5 Level 6 side data but reports an `hev1` sample entry. Its Apple HLS
  output must be explicitly classified and packaged as `dvh1`.
- That same DV sample has stream 50 marked `attached_pic=1`; the current parser
  incorrectly treats every `codec_type=video` stream as selectable content.
- `sample-jsons/1080p.HDR10+.mp4.ffprobe.json` is actually 3840x2160, 60 fps,
  approximately 70 Mb/s PQ/BT.2020 HEVC despite its filename. Classification
  must use ffprobe metadata, never filenames.

## Findings considered and rejected

- Replace live relay with full download/conversion: rejected. Stream-copy HLS
  avoids a second complete file and remains the right one-watch architecture.
- Add `-movflags +faststart` and `-brand mp42`: rejected for HLS. Those flags
  target a completed flat MP4; QuickBridge already creates fMP4 initialization
  and media segments.
- Always hardware-transcode 2160p HEVC: rejected. It wastes CPU/GPU, loses
  quality, and may strip Dolby Vision metadata. Stream copy remains the primary
  route; unsupported formats need explicit fallback policy.
- Treat the codec tag as the sole lag fix: rejected. `hvc1`/`dvh1` fixes Apple
  packaging and hardware-decoder compatibility, but exact-rate input and the
  six-second window are independent buffering problems.
- Add decorative motion or a web-style component system to the TUI: rejected.
  Terminal polish comes from hierarchy, stable regions, focus, contrast, and
  responsive text—not shadows, pointer hit areas, or animation-heavy effects.
- Show every completed startup step permanently in the running dashboard:
  rejected. Preserve detailed history, but summarize completed setup so live
  playback health and the command composer remain primary.
- Remove scripted/noninteractive CLI mode: rejected. A full TUI is best for
  people running long interactive sessions, while scripted CLI behavior is a
  valuable automation and test surface. The two surfaces should share core
  actions without sharing the same presentation.
