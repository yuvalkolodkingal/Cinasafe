#!/usr/bin/env bash
# Disk-trace test (brief 13): runs a full session (create, import, play,
# verify, and with --app the desktop app: unlock, resume playback, lock,
# quit) and fails if plaintext from the test media shows up in any file
# written outside the stick.
#
# The test media carry a random marker in their file name, container title
# and subtitle text. Afterwards every file written during the session (seen
# by fatrace when run as root, plus everything newer than the start under
# $HOME, /tmp and /var/tmp) is searched for the marker. The marker is never
# printed, so CI logs can't produce false hits.
#
# Usage: scripts/disk-trace.sh [--app]
# Needs: ffmpeg; target/release/cinasafe; with --app also
# target/release/cinasafe-app (built with --features tauri/custom-protocol),
# Xvfb and xdotool; fatrace for the system-wide view (as root or via sudo).

set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
bin=${BIN:-$repo/target/release}
with_app=false
[ "${1:-}" = "--app" ] && with_app=true
play_seconds=${PLAY_SECONDS:-30}

work=$(mktemp -d /var/tmp/cinasafe-trace.XXXXXX)
src=$work/media
stick=$work/stick
mkdir -p "$src" "$stick"
marker=cnsf$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')
log() { echo "[disk-trace] $*"; }

# Test media: 60 s of video with the marker in its name and title, and a subtitle.
ffmpeg -v error -f lavfi -i "testsrc2=duration=60:size=640x360:rate=24" -f lavfi -i "sine=duration=60" \
    -c:v libx264 -preset ultrafast -c:a aac -shortest -metadata title="$marker title" \
    "$src/$marker.Movie.2020.mkv"
printf '1\n00:00:01,000 --> 00:00:50,000\n%s subtitle line\n' "$marker" > "$src/$marker.Movie.2020.en.srt"

export CINASAFE_PASSWORD="disk trace password"
touch "$work/t0"
sleep 1

# fatrace needs root: run it directly as root, or through passwordless sudo
# (CI), so the session itself runs as the normal user.
tracer=""
fatrace_cmd=""
if command -v fatrace >/dev/null; then
    if [ "$(id -u)" = 0 ]; then
        fatrace_cmd="fatrace"
    elif sudo -n true 2>/dev/null; then
        fatrace_cmd="sudo -n fatrace"
    fi
fi
if [ -n "$fatrace_cmd" ]; then
    $fatrace_cmd -f W -o "$work/fatrace.log" &
    tracer=$!
    sleep 1
    log "fatrace running"
else
    log "no fatrace (needs root or sudo): checking files by modification time only"
fi

# Everything the session prints goes to files in $work (it contains titles).
run() { "$@" >> "$work/session.log" 2>&1; }
run "$bin/cinasafe" create "$stick" --no-recovery
run "$bin/cinasafe" import "$stick" "$src"
id=$("$bin/cinasafe" ls "$stick" 2>/dev/null | awk 'NR == 2 { print $1 }')
run "$bin/cinasafe" play "$stick" "$id" --seconds "$play_seconds" --seeks 0
run "$bin/cinasafe" verify "$stick"
log "CLI session done"

if $with_app; then
    display=:${DISPLAY_NUMBER:-91}
    Xvfb "$display" -screen 0 1280x800x24 >/dev/null 2>&1 &
    xvfb=$!
    sleep 2
    export DISPLAY=$display
    CINASAFE_VAULT_ROOT=$stick "$bin/cinasafe-app" >> "$work/app.log" 2>&1 &
    app=$!
    sleep 12
    # Unlock (password field), then Resume on Home's hero, play, back, Lock.
    xdotool mousemove 620 409 click 1
    xdotool type --delay 20 "$CINASAFE_PASSWORD"
    xdotool key Return
    sleep 8
    import -window root "$work/home.png" 2>/dev/null || true
    xdotool mousemove 110 502 click 1
    sleep 20
    import -window root "$work/player.png" 2>/dev/null || true
    xdotool key Escape
    sleep 3
    xdotool mousemove 1210 35 click 1
    sleep 3
    kill -TERM "$app"
    for _ in $(seq 30); do kill -0 "$app" 2>/dev/null || break; sleep 0.5; done
    if kill -0 "$app" 2>/dev/null; then
        log "FAIL: the app did not exit on SIGTERM"
        kill -9 "$app"
        exit 1
    fi
    kill "$xvfb" 2>/dev/null || true
    log "app session done"
    if ls -d "${TMPDIR:-/tmp}"/cinasafe-session-* >/dev/null 2>&1; then
        log "FAIL: the app left its session directory behind"
        exit 1
    fi
fi

# DISK_TRACE_SELF_TEST=1: leak the marker on purpose; the check must then fail.
if [ "${DISK_TRACE_SELF_TEST:-}" = 1 ]; then
    leak=$(mktemp /tmp/cinasafe-selftest-leak.XXXXXX)
    echo "$marker" > "$leak"
fi

if [ -n "$tracer" ]; then
    sleep 1
    if [ "$(id -u)" = 0 ]; then kill "$tracer"; else sudo -n kill "$tracer" 2>/dev/null || kill "$tracer"; fi
    wait "$tracer" 2>/dev/null || true
fi

# Files written outside the allowed places.
allowed() {
    case "$1" in
        "$work"/* | /proc/* | /sys/* | /dev/* | "$repo"/* ) return 0 ;;
        "${TMPDIR:-/tmp}"/cinasafe-session-*) return 0 ;;  # the app's own temp dir (deleted on exit)
    esac
    return 1
}
: > "$work/written.txt"
if [ -f "$work/fatrace.log" ]; then
    sed -n 's/^[^:]*: [A-Z+<>]* *\(\/.*\)$/\1/p' "$work/fatrace.log" | sort -u >> "$work/written.txt"
fi
find "$HOME" /tmp /var/tmp -xdev -type f -newer "$work/t0" 2>/dev/null >> "$work/written.txt" || true
sort -u -o "$work/written.txt" "$work/written.txt"

hits=0
outside=0
while IFS= read -r f; do
    allowed "$f" && continue
    [ -f "$f" ] || continue
    outside=$((outside + 1))
    echo "$f" >> "$work/outside.txt"
    if grep -qaF "$marker" "$f" 2>/dev/null; then
        hits=$((hits + 1))
        log "PLAINTEXT FOUND: $f"
    fi
done < "$work/written.txt"

if grep -rqaF "$marker" "$stick"; then
    log "FAIL: the marker is readable on the stick"
    hits=$((hits + 1))
fi

log "files written outside the stick and the session dir: $outside (list: $work/outside.txt)"
if [ "${DISK_TRACE_SELF_TEST:-}" = 1 ]; then
    rm -f "$leak"
    if [ "$hits" -gt 0 ]; then
        log "PASS (self-test): the planted leak was found"
        exit 0
    fi
    log "FAIL (self-test): the planted leak was not found"
    exit 1
fi
if [ "$hits" -gt 0 ]; then
    log "FAIL: plaintext from the test media was written to the host ($hits files)"
    exit 1
fi
log "PASS: no plaintext from the test media outside the stick"
