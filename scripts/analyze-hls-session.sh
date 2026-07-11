#!/usr/bin/env bash
set -euo pipefail

session_dir=${1:?usage: scripts/analyze-hls-session.sh SESSION_DIR}
playlist="$session_dir/stream.m3u8"

if [[ ! -f "$playlist" ]]; then
  echo "missing playlist: $playlist" >&2
  exit 1
fi

init_name=$(sed -n 's/^#EXT-X-MAP:URI="\([^"]*\)"/\1/p' "$playlist" | head -1)
if [[ -z "$init_name" || ! -s "$session_dir/$init_name" ]]; then
  echo "missing or empty initialization segment" >&2
  exit 1
fi

echo "Initialization"
ffprobe -v error -select_streams v:0 \
  -show_entries stream=codec_name,codec_tag_string,extradata_size:stream_side_data \
  -of json "$session_dir/$init_name"

echo "Playlist"
awk '
  /^#EXT-X-TARGETDURATION:/ { target=$0 }
  /^#EXT-X-MEDIA-SEQUENCE:/ { sequence=$0 }
  /^#EXTINF:/ { count++; duration += substr($0, 9) + 0 }
  END {
    print target
    print sequence
    printf "segments=%d advertised_duration=%.3fs\n", count, duration
    if (count < 3) exit 1
  }
' "$playlist"

echo "First video packets"
packets=$(ffprobe -v error -select_streams v:0 -read_intervals '%+#12' \
  -show_packets -show_entries packet=pts_time,flags -of csv=p=0 "$playlist")
printf '%s\n' "$packets"
first_flags=$(printf '%s\n' "$packets" | head -1 | cut -d, -f2)
if [[ "$first_flags" != *K* ]]; then
  echo "first video packet is not random-access" >&2
  exit 1
fi

echo "storage_bytes=$(du -sk "$session_dir" | awk '{print $1 * 1024}')"
