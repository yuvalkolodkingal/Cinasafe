#!/bin/sh
# Reproduces the Milestone 0 measurements in docs/SPIKE.md on Linux.
#
# Needs: cargo, ffmpeg, libmpv (libmpv.so.2), WebKitGTK 4.1 dev packages.
# For the windowed runs: a display (or Xvfb + ImageMagick `import` + xdotool).
#
#   spike/run-spike.sh            # headless: vectors, bench, playback, slow-stick, strace
#   spike/run-spike.sh --window   # also run the Tauri embedding spike on $DISPLAY
set -eu

REPO=$(cd "$(dirname "$0")/.." && pwd)
OUT="$REPO/spike/out"
CLI="$REPO/target/release/cinasafe"
mkdir -p "$OUT"
cd "$OUT"

cargo build --release -p cinasafe-cli --manifest-path "$REPO/Cargo.toml"
cargo test -p cinasafe-core -p cinasafe-player --manifest-path "$REPO/Cargo.toml"
python3 -I "$REPO/crates/cinasafe-core/tests/vectors/verify_vectors.py"

# Test media: synthetic, so nothing copyrighted is involved.
[ -f he.srt ] || printf '1\n00:00:00,500 --> 00:00:04,000\nשלום! זהו מבחן כתוביות.\n\n2\n00:00:04,000 --> 00:00:08,000\n- Hello, אני Cinasafe 2026.\n\n3\n00:00:08,000 --> 00:00:12,000\nمرحبا بالعالم، هذا اختبار.\n\n4\n00:00:12,000 --> 00:01:59,000\n?מה שלומך\n' > he.srt
[ -f test-1080p.mkv ] || ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i testsrc2=size=1920x1080:rate=30 -f lavfi -i sine=frequency=440:sample_rate=48000 -i he.srt \
  -t 120 -map 0:v -map 1:a -map 2:s -c:v libx264 -preset veryfast -b:v 8M -maxrate 10M -bufsize 16M -g 60 \
  -c:a aac -b:a 192k -c:s srt -metadata:s:s:0 language=heb test-1080p.mkv
[ -f test-2160p.mp4 ] || ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i testsrc2=size=3840x2160:rate=30 -f lavfi -i sine=frequency=220:sample_rate=48000 \
  -t 30 -c:v libx264 -preset ultrafast -b:v 40M -maxrate 45M -bufsize 60M -g 60 -c:a aac test-2160p.mp4

rm -rf stick
"$CLI" spike-init stick
V1=$("$CLI" spike-import stick test-1080p.mkv)
V2=$("$CLI" spike-import stick test-2160p.mp4)
S=$("$CLI" spike-import stick he.srt --no-pad)
echo "$V1 $V2 $S" > ids.txt

echo "== reader benchmark"
"$CLI" spike-bench stick "$V1"
"$CLI" spike-bench stick "$V2"

echo "== headless playback through cinasafe://"
"$CLI" spike-play stick "$V1" --sub "$S" --seconds 6 --seeks 15 | grep -v track-list
"$CLI" spike-play stick "$V2" --seconds 3 --seeks 10 | grep -v track-list

SMALL_CACHE="-o demuxer-max-bytes=16MiB -o demuxer-max-back-bytes=4MiB"
for rate in 25 80; do
  echo "== simulated stick: $rate MiB/s, 2 ms per read"
  # shellcheck disable=SC2086
  "$CLI" spike-play stick "$V1" --seconds 4 --seeks 15 --throttle-mibps "$rate" --throttle-latency-ms 2 $SMALL_CACHE | grep -v track-list
  # shellcheck disable=SC2086
  "$CLI" spike-play stick "$V2" --seconds 4 --seeks 10 --throttle-mibps "$rate" --throttle-latency-ms 2 $SMALL_CACHE | grep -v track-list
done

if command -v strace >/dev/null; then
  echo "== host-disk write audit (headless player)"
  strace -f -qq -e trace=openat,creat,rename,renameat2,unlink,unlinkat,mkdir,mkdirat -o strace-cli.txt \
    "$CLI" spike-play stick "$V1" --sub "$S" --seconds 3 --seeks 5 >/dev/null
  if grep -E 'O_WRONLY|O_RDWR|O_CREAT|creat\(|rename|unlink|mkdir' strace-cli.txt | grep -v ' = -1 '; then
    echo "WRITES FOUND (see above)"
  else
    echo "no successful write-mode opens, renames, unlinks or mkdirs"
  fi
fi

[ "${1:-}" = "--window" ] || exit 0

echo "== Tauri embedding spike on ${DISPLAY:-$WAYLAND_DISPLAY}"
cargo build -p tauri-embed-spike --manifest-path "$REPO/Cargo.toml"
for mode in gl-overlay wid; do
  CINASAFE_SPIKE_ROOT="$OUT/stick" CINASAFE_SPIKE_BLOB="$V1" CINASAFE_SPIKE_SUB="$S" \
  CINASAFE_SPIKE_MODE="$mode" CINASAFE_SPIKE_EXIT_AFTER=15 \
    "$REPO/target/debug/tauri-embed-spike" &
  sleep 10
  if command -v import >/dev/null && [ -n "${DISPLAY:-}" ]; then
    command -v xdotool >/dev/null && xdotool mousemove 600 400
    sleep 1
    import -window root "shot-$mode.png" && echo "screenshot: $OUT/shot-$mode.png"
  fi
  wait
done
