# UHD, HDR, and Dolby Vision performance testing

This is a manual macOS release gate for QuickBridge's stream-copy HLS relay.
Keep real media, signed URLs, credentials, and retained sessions outside Git.

## Required fixtures

Use authorized sources covering:

- a lower-rate 1080p HEVC control;
- a 70 Mb/s or higher 2160p HEVC stress source;
- Dolby Vision Profile 5 at 2160p;
- PQ/BT.2020 HDR or compatible Dolby Vision Profile 8.

Confirm resolution, bitrate, transfer characteristic, and Dolby Vision profile
with `ffprobe` before testing. The supported Dolby Vision routes are Profile 5
(`dvh1`) and Profile 8 with compatibility ID 1 or 4 (`hvc1`). Other profiles
must fail closed.

## Release loop

Build and test from a clean checkout:

```console
cargo xtest
cargo build --release
target/release/quickbridge --keep-temp "MEDIA_URL"
```

Run each UHD fixture three times. Observe at least ten uninterrupted minutes,
or the full fixture when shorter. The test network must sustain at least 1.5x
the source peak bitrate. Keep the Mac awake and do not change network conditions
between comparisons.

For every run, record outside the repository:

- source class and sanitized probe facts;
- FFmpeg and macOS versions;
- startup time, stall count, and total stalled duration;
- three timestamp jumps and whether playback resumes;
- audio continuity and any FFmpeg/HTTP errors;
- process CPU and memory, playlist depth, and retained storage;
- whether a capable display visibly engages HDR or Dolby Vision.

Pass requires zero rebuffering stalls after playback begins, clean recovery from
all three jumps, correct audio, bounded rolling storage, and correct HDR/DV
presentation. A run with insufficient network capacity is invalid, not a
QuickBridge failure.

## Retained-artifact checks

After quitting with `--keep-temp`, analyze each retained session:

```console
scripts/analyze-hls-session.sh /path/to/session-0001
```

Expected results:

- ordinary HEVC/HDR initialization uses `hvc1`;
- Dolby Vision Profile 5 uses `dvh1` and retains its DOVI record;
- at least three complete segments are advertised before playback;
- the first video packet is random-access;
- the rolling playlist remains bounded near 12 segments and roughly 24 seconds,
  subject to source GOP spacing.

Repeat artifact checks after startup and timestamp jumps. QuickTime visual
validation remains mandatory because FFprobe cannot confirm display HDR mode.

## Known limitations

- Stream copy cannot create keyframes, so segment durations follow source GOPs.
- TrueHD audio is transcoded losslessly to ALAC for QuickTime compatibility.
- Relay write rate measures local HLS output growth, not source download speed.
- Dolby Vision Profile 7 transformation and video transcoding are unsupported.

## July 2026 validation notes

Real-media validation found that tag-only HEVC packaging could emit `hvc1`
without complete VPS/SPS/PPS configuration, which QuickTime rejected with
CoreMedia error `-19601`. Applying FFmpeg's `hevc_metadata` bitstream filter
during HEVC stream copy populated valid configuration records without
re-encoding.

Validated results include smooth 1080p HEVC, correct Dolby Vision Profile 5
color and `dvh1` signaling, compatible Profile 8/PQ playback through `hvc1`,
successful timestamp jumps, and synchronized ALAC output from TrueHD input.
The 85.2 Mb/s stress fixture played correctly when buffered, but repeat runs
were invalidated by a network below the required 1.5x capacity. Complete the
three-run UHD gate on a faster connection before treating that performance
result as closed.
