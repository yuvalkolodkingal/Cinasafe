#!/bin/sh
# Starts the Cinasafe player that matches this computer, from the stick.
dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
case "$(uname -m)" in
  x86_64|amd64) app="$dir/Cinasafe-Linux-x86_64.AppImage" ;;
  aarch64|arm64) app="$dir/Cinasafe-Linux-aarch64.AppImage" ;;
  *) echo "Cinasafe has no player for $(uname -m) yet." >&2; exit 1 ;;
esac
if [ ! -f "$app" ]; then
  echo "The player for this computer isn't on the stick: $(basename "$app")" >&2
  exit 1
fi
# exFAT has no execute bit; run it through the shell's exec permission instead.
[ -x "$app" ] || chmod +x "$app" 2>/dev/null
if [ -x "$app" ]; then
  # Without FUSE, an AppImage can still run by unpacking itself (the app only, never the vault).
  if [ -e /dev/fuse ] || command -v fusermount >/dev/null 2>&1 || command -v fusermount3 >/dev/null 2>&1; then
    exec "$app" "$@"
  fi
  exec "$app" --appimage-extract-and-run "$@"
fi
echo "Can't run $app (is the stick mounted with noexec?)." >&2
echo "Copy it to your home folder, make it executable and start it from there." >&2
exit 1
