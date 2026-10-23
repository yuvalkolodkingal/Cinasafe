#!/usr/bin/env bash
# Builds an LGPL libmpv for Linux, with the FFmpeg, libplacebo and dav1d it
# needs, into $OUT (default: third_party/mpv/linux-$(uname -m)).
#
# Licensing (docs/OPEN_QUESTIONS.md Q2): mpv is configured with -Dgpl=false
# and FFmpeg without --enable-gpl/--enable-nonfree, so everything built here
# is LGPL-2.1-or-later (dav1d: BSD-2-Clause). The script fails if either
# ends up GPL. Sources are pinned by commit; $OUT/SOURCES.txt records them,
# and $OUT/../libmpv-sources-linux.tar.gz holds the exact sources (the LGPL
# source offer).
#
# Other libraries (libass, HarfBuzz, FriBidi, FreeType, fontconfig, libva,
# ALSA/PulseAudio/PipeWire) come from the system and are bundled into the
# AppImage by linuxdeploy. Build dependencies on Debian/Ubuntu:
#   build-essential nasm meson ninja-build pkg-config python3-jinja2 patchelf
#   libass-dev libva-dev libdrm-dev libegl-dev libwayland-dev libxkbcommon-dev
#   libasound2-dev libpulse-dev liblcms2-dev

set -euo pipefail

FFMPEG_TAG=n8.1.3
FFMPEG_COMMIT=1041abdc962f4cc4f394aa8de9dc5236c0c3b9e7
DAV1D_TAG=1.5.3
DAV1D_COMMIT=b546257f770768b2c88258c533da38b91a06f737
LIBPLACEBO_TAG=v7.360.1
LIBPLACEBO_COMMIT=cee9b076f2c63104ccfd497fa79c39a867293ec4
MPV_TAG=v0.41.0
MPV_COMMIT=41f6a645068483470267271e1d09966ca3b9f413

here=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$here/linux-$(uname -m)}
WORK=${WORK:-$here/build-linux}
JOBS=${JOBS:-$(nproc)}
PREFIX=$WORK/prefix

mkdir -p "$WORK" "$OUT"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$PREFIX/lib/$(uname -m)-linux-gnu/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="$PREFIX/lib:${LD_LIBRARY_PATH:-}"

fetch() { # name url tag commit
    local dir=$WORK/src/$1
    if [ ! -d "$dir/.git" ]; then
        git clone --quiet --depth 1 --branch "$3" --recurse-submodules --shallow-submodules "$2" "$dir"
    fi
    local got
    got=$(git -C "$dir" rev-parse HEAD)
    if [ "$got" != "$4" ]; then
        echo "error: $1 $3 is $got, expected $4" >&2
        exit 1
    fi
}

fetch ffmpeg https://github.com/FFmpeg/FFmpeg.git "$FFMPEG_TAG" "$FFMPEG_COMMIT"
fetch dav1d https://code.videolan.org/videolan/dav1d.git "$DAV1D_TAG" "$DAV1D_COMMIT"
fetch libplacebo https://code.videolan.org/videolan/libplacebo.git "$LIBPLACEBO_TAG" "$LIBPLACEBO_COMMIT"
fetch mpv https://github.com/mpv-player/mpv.git "$MPV_TAG" "$MPV_COMMIT"

meson_build() { # srcdir builddir args...
    local src=$1 build=$2
    shift 2
    meson setup --reconfigure "$build" "$src" --prefix="$PREFIX" --libdir=lib --buildtype=release \
        -Ddefault_library=shared "$@" || meson setup "$build" "$src" --prefix="$PREFIX" --libdir=lib \
        --buildtype=release -Ddefault_library=shared "$@"
    ninja -C "$build" -j "$JOBS"
    ninja -C "$build" install
}

# dav1d: AV1 decoding (FFmpeg has no software AV1 decoder of its own).
meson_build "$WORK/src/dav1d" "$WORK/b/dav1d" -Denable_tools=false -Denable_tests=false

# FFmpeg, LGPL: no --enable-gpl, --enable-nonfree or --enable-version3.
# Decoding only; no network, devices, programs or filters mpv doesn't need.
if [ ! -f "$PREFIX/lib/libavcodec.so" ]; then
    (
        cd "$WORK/src/ffmpeg"
        ./configure --prefix="$PREFIX" --enable-shared --disable-static \
            --disable-programs --disable-doc --disable-debug --disable-network \
            --disable-avdevice --disable-encoders \
            --enable-encoder=png,mjpeg --disable-muxers \
            --enable-libdav1d --enable-vaapi \
            --extra-ldflags="-Wl,-rpath,\$\$ORIGIN"
        make -j "$JOBS"
        make install
    )
fi
if grep -q '#define CONFIG_GPL 1' "$WORK/src/ffmpeg/config.h" || grep -q '#define CONFIG_NONFREE 1' "$WORK/src/ffmpeg/config.h"; then
    echo "error: FFmpeg was configured as GPL or nonfree" >&2
    exit 1
fi

# libplacebo: OpenGL only (mpv's render API on Linux); no Vulkan or shader compilers.
meson_build "$WORK/src/libplacebo" "$WORK/b/libplacebo" -Dvulkan=disabled -Dopengl=enabled \
    -Dd3d11=disabled -Dshaderc=disabled -Dglslang=disabled -Ddemos=false -Dtests=false

# mpv as a library only, LGPL.
meson_build "$WORK/src/mpv" "$WORK/b/mpv" -Dgpl=false -Dlibmpv=true -Dcplayer=false \
    -Dlua=disabled -Djavascript=disabled -Dcdda=disabled -Ddvdnav=disabled -Dlibbluray=disabled \
    -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled -Dtests=false \
    -Dlibarchive=disabled -Dzimg=disabled -Dvulkan=disabled -Dshaderc=disabled -Dspirv-cross=disabled
if ! grep -q "'gpl': false\|gpl *: *false\|GPL: *NO\|#define HAVE_GPL 0" "$WORK/b/mpv/config.h" "$WORK/b/mpv/meson-logs/meson-log.txt" 2>/dev/null; then
    if grep -q '#define HAVE_GPL 1' "$WORK/b/mpv/config.h"; then
        echo "error: mpv was configured as GPL" >&2
        exit 1
    fi
fi

# Collect the libraries, each with RUNPATH $ORIGIN so they find each other.
rm -rf "${OUT:?}"/*
for lib in libmpv.so.2 libavcodec.so libavformat.so libavutil.so libavfilter.so libswresample.so \
    libswscale.so libplacebo.so libdav1d.so; do
    real=$(readlink -f "$PREFIX/lib/$lib")
    soname=$(objdump -p "$real" | awk '/SONAME/ {print $2}')
    cp "$real" "$OUT/$soname"
    patchelf --set-rpath '$ORIGIN' "$OUT/$soname"
done
strip --strip-unneeded "$OUT"/*.so*

for f in mpv/LICENSE.LGPL ffmpeg/COPYING.LGPLv2.1 libplacebo/LICENSE dav1d/COPYING; do
    cp "$WORK/src/$f" "$OUT/$(dirname "$f")-$(basename "$f")"
done
{
    echo "LGPL libmpv for Cinasafe, built by third_party/mpv/build-linux.sh"
    echo "FFmpeg     $FFMPEG_TAG     $FFMPEG_COMMIT   https://github.com/FFmpeg/FFmpeg"
    echo "dav1d      $DAV1D_TAG      $DAV1D_COMMIT   https://code.videolan.org/videolan/dav1d"
    echo "libplacebo $LIBPLACEBO_TAG $LIBPLACEBO_COMMIT   https://code.videolan.org/videolan/libplacebo"
    echo "mpv        $MPV_TAG        $MPV_COMMIT   https://github.com/mpv-player/mpv"
    echo
    echo "Built on: $(. /etc/os-release && echo "$PRETTY_NAME"), $(uname -m)"
    echo "Source code: libmpv-sources-linux.tar.gz next to this build, or the commits above."
} > "$OUT/SOURCES.txt"

tar -czf "$OUT/../libmpv-sources-linux.tar.gz" -C "$WORK/src" --exclude=.git ffmpeg dav1d libplacebo mpv \
    -C "$here" build-linux.sh

# Smoke test: the library loads and reports an LGPL FFmpeg.
python3 - "$OUT/libmpv.so.2" <<'EOF'
import ctypes, sys
mpv = ctypes.CDLL(sys.argv[1])
mpv.mpv_client_api_version.restype = ctypes.c_ulong
v = mpv.mpv_client_api_version()
print(f"libmpv client API {v >> 16}.{v & 0xffff}")
EOF
for so in "$OUT"/libavutil.so.*; do
    python3 -c "import ctypes,sys; l=ctypes.CDLL(sys.argv[1]); l.avutil_license.restype=ctypes.c_char_p; s=l.avutil_license().decode(); print('FFmpeg licence:', s); sys.exit(0 if s.startswith('LGPL') else 1)" "$so"
done
echo "libmpv (LGPL) is in $OUT"
