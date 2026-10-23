#!/usr/bin/env bash
# Builds an LGPL libmpv-2.dll for Windows inside MSYS2 (UCRT64 for x86_64,
# CLANGARM64 for arm64) and collects it with every DLL it needs into
# $OUT (default: third_party/mpv/windows-<arch>). The app loads it from
# `mpv\` next to its exe, or `.cinasafe\runtime\windows-<arch>\` on a stick.
#
# FFmpeg and mpv are built from pinned sources as LGPL (no --enable-gpl, mpv
# -Dgpl=false); the script fails if either ends up GPL. Their other
# libraries (libass, libplacebo, dav1d, HarfBuzz, FriBidi, FreeType, ...) are
# MSYS2 packages; SOURCES.txt lists each with its exact package version, whose
# source is published at https://repo.msys2.org/mingw/sources/.
#
# Needs these MSYS2 packages (the workflow installs them with pacboy):
#   git make diffutils
#   toolchain meson ninja nasm pkgconf libass libplacebo dav1d lcms2 shaderc spirv-cross ntldd

set -euo pipefail

FFMPEG_TAG=n8.1.3
FFMPEG_COMMIT=1041abdc962f4cc4f394aa8de9dc5236c0c3b9e7
MPV_TAG=v0.41.0
MPV_COMMIT=41f6a645068483470267271e1d09966ca3b9f413

case "${MSYSTEM:-}" in
    UCRT64) arch=x86_64 ;;
    CLANGARM64) arch=arm64 ;;
    *) echo "run this in an MSYS2 UCRT64 or CLANGARM64 shell" >&2; exit 1 ;;
esac
sysroot=$(cygpath -u "$MINGW_PREFIX")
here=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$here/windows-$arch}
WORK=${WORK:-$here/build-windows-$arch}
JOBS=${JOBS:-$(nproc)}
PREFIX=$WORK/prefix
mkdir -p "$WORK" "$OUT"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export PATH="$PREFIX/bin:$PATH"

fetch() { # name url tag commit
    local dir=$WORK/src/$1
    if [ ! -d "$dir/.git" ]; then
        git clone --quiet --depth 1 --branch "$3" "$2" "$dir"
    fi
    local got
    got=$(git -C "$dir" rev-parse HEAD)
    [ "$got" = "$4" ] || { echo "error: $1 $3 is $got, expected $4" >&2; exit 1; }
}
fetch ffmpeg https://github.com/FFmpeg/FFmpeg.git "$FFMPEG_TAG" "$FFMPEG_COMMIT"
fetch mpv https://github.com/mpv-player/mpv.git "$MPV_TAG" "$MPV_COMMIT"

if [ ! -f "$PREFIX/lib/libavcodec.dll.a" ]; then
    (
        cd "$WORK/src/ffmpeg"
        ./configure --prefix="$PREFIX" --enable-shared --disable-static \
            --disable-programs --disable-doc --disable-debug --disable-network \
            --disable-avdevice --disable-encoders --enable-encoder=png,mjpeg --disable-muxers \
            --enable-libdav1d --enable-d3d11va --enable-dxva2
        make -j "$JOBS"
        make install
    )
fi
if grep -q '#define CONFIG_GPL 1' "$WORK/src/ffmpeg/config.h" || grep -q '#define CONFIG_NONFREE 1' "$WORK/src/ffmpeg/config.h"; then
    echo "error: FFmpeg was configured as GPL or nonfree" >&2
    exit 1
fi

meson setup "$WORK/b/mpv" "$WORK/src/mpv" --prefix="$PREFIX" --buildtype=release -Ddefault_library=shared \
    -Dgpl=false -Dlibmpv=true -Dcplayer=false -Dd3d11=enabled -Dlua=disabled -Djavascript=disabled \
    -Dcdda=disabled -Ddvdnav=disabled -Dlibbluray=disabled -Dlibarchive=disabled -Dzimg=disabled \
    -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled -Dtests=false \
    || meson configure "$WORK/b/mpv"
ninja -C "$WORK/b/mpv" -j "$JOBS"
ninja -C "$WORK/b/mpv" install
if grep -q '#define HAVE_GPL 1' "$WORK/b/mpv/config.h"; then
    echo "error: mpv was configured as GPL" >&2
    exit 1
fi

# libmpv-2.dll plus everything it loads that isn't part of Windows.
rm -rf "${OUT:?}"/*
cp "$PREFIX/bin/libmpv-2.dll" "$OUT/"
ntldd -R "$OUT/libmpv-2.dll" | tr '\\' '/' | awk '{print $3}' | while read -r dep; do
    dep=$(cygpath -u "$dep" 2>/dev/null || echo "$dep")
    case "$dep" in
        "$sysroot"/* | "$PREFIX"/*) cp -n "$dep" "$OUT/" ;;
    esac
done
strip --strip-unneeded "$OUT"/*.dll

# Licences and sources.
mkdir -p "$OUT/licenses"
cp "$WORK/src/mpv/LICENSE.LGPL" "$OUT/licenses/mpv-LICENSE.LGPL"
cp "$WORK/src/ffmpeg/COPYING.LGPLv2.1" "$OUT/licenses/ffmpeg-COPYING.LGPLv2.1"
{
    echo "LGPL libmpv for Cinasafe ($MSYSTEM), built by third_party/mpv/build-windows.sh"
    echo "FFmpeg $FFMPEG_TAG $FFMPEG_COMMIT https://github.com/FFmpeg/FFmpeg"
    echo "mpv    $MPV_TAG $MPV_COMMIT https://github.com/mpv-player/mpv"
    echo "Source code of the two: libmpv-sources-windows-$arch.tar.gz next to this build."
    echo
    echo "Other DLLs come from these MSYS2 packages (sources: https://repo.msys2.org/mingw/sources/):"
} > "$OUT/SOURCES.txt"
for dll in "$OUT"/*.dll; do
    name=$(basename "$dll")
    [ -f "$PREFIX/bin/$name" ] && continue
    pkg=$(pacman -Qqo "$sysroot/bin/$name" 2>/dev/null || true)
    [ -n "$pkg" ] || continue
    echo "  $name: $(pacman -Q "$pkg")" >> "$OUT/SOURCES.txt"
    if [ -d "$sysroot/share/licenses/${pkg#mingw-w64-*-}" ]; then
        cp -r "$sysroot/share/licenses/${pkg#mingw-w64-*-}" "$OUT/licenses/" 2>/dev/null || true
    fi
done
tar -czf "$OUT/../libmpv-sources-windows-$arch.tar.gz" -C "$WORK/src" --exclude=.git ffmpeg mpv -C "$here" build-windows.sh
echo "libmpv (LGPL) and $(ls "$OUT"/*.dll | wc -l) DLLs are in $OUT"
