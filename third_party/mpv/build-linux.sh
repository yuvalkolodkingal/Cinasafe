#!/usr/bin/env bash
# Builds a static, LGPL libmpv for Linux, with the FFmpeg, libplacebo, dav1d
# and libass it needs, into $OUT (default: third_party/mpv/linux-$(uname -m)).
# The app links all of it into its own binary (cargo feature `static-mpv`), so
# no libmpv file ships or is looked for at run time.
#
# $OUT holds the static libraries (position-independent, for a PIE executable)
# and link.txt, which crates/cinasafe-player/build.rs reads: one line per
# library, in link order, either `static=<file in $OUT>` or `dylib=<name>`.
# The dylibs are the desktop's own libraries (glibc, X11/Wayland, EGL, ALSA,
# PulseAudio, FreeType, HarfBuzz, FriBidi, fontconfig, libva, lcms2), which
# every system that runs the GTK/WebKitGTK app already has; the AppImage
# bundles the ones that aren't on its exclude list.
#
# Licensing (docs/OPEN_QUESTIONS.md Q2): mpv is configured with -Dgpl=false
# and FFmpeg without --enable-gpl/--enable-nonfree, so everything built here
# is LGPL-2.1-or-later (dav1d: BSD-2-Clause, libass: ISC). The script fails
# if either ends up GPL. Static linking meets LGPL-2.1 section 6 because the
# app's complete source is public, so anyone can relink it against a modified
# library. Sources are pinned by commit; $OUT/SOURCES.txt records them and
# $OUT/../libmpv-sources-linux.tar.gz holds them (the LGPL source offer).
#
# Build dependencies on Debian/Ubuntu:
#   build-essential nasm meson ninja-build pkg-config python3-jinja2
#   libfreetype-dev libfribidi-dev libharfbuzz-dev libfontconfig-dev
#   libva-dev libdrm-dev libegl-dev libwayland-dev libxkbcommon-dev
#   libx11-dev libxext-dev libxpresent-dev libxrandr-dev libxss-dev libxv-dev
#   libasound2-dev libpulse-dev liblcms2-dev

set -euo pipefail

FFMPEG_TAG=n8.1.3
FFMPEG_COMMIT=1041abdc962f4cc4f394aa8de9dc5236c0c3b9e7
DAV1D_TAG=1.5.3
DAV1D_COMMIT=b546257f770768b2c88258c533da38b91a06f737
LIBPLACEBO_TAG=v7.360.1
LIBPLACEBO_COMMIT=cee9b076f2c63104ccfd497fa79c39a867293ec4
LIBASS_TAG=0.17.5
LIBASS_COMMIT=4a05d8127f525943ebf45fdc6497c9e665947f0d
MPV_TAG=v0.41.0
MPV_COMMIT=41f6a645068483470267271e1d09966ca3b9f413

here=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:-$here/linux-$(uname -m)}
WORK=${WORK:-$here/build-linux}
JOBS=${JOBS:-$(nproc)}
PREFIX=$WORK/prefix-static

mkdir -p "$WORK" "$OUT"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

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
fetch libass https://github.com/libass/libass.git "$LIBASS_TAG" "$LIBASS_COMMIT"
fetch mpv https://github.com/mpv-player/mpv.git "$MPV_TAG" "$MPV_COMMIT"

meson_build() { # srcdir builddir args...
    local src=$1 build=$2
    shift 2
    local opts=(--prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static -Db_staticpic=true)
    meson setup --reconfigure "$build" "$src" "${opts[@]}" "$@" || meson setup "$build" "$src" "${opts[@]}" "$@"
    ninja -C "$build" -j "$JOBS"
    ninja -C "$build" install
}

# dav1d: AV1 decoding (FFmpeg has no software AV1 decoder of its own).
meson_build "$WORK/src/dav1d" "$WORK/b/dav1d" -Denable_tools=false -Denable_tests=false

# FFmpeg, LGPL: no --enable-gpl, --enable-nonfree or --enable-version3.
# Decoding only; no network, devices, programs or filters mpv doesn't need.
if [ ! -f "$PREFIX/lib/libavcodec.a" ]; then
    (
        cd "$WORK/src/ffmpeg"
        # --disable-autodetect: only the system libraries named here get linked.
        ./configure --prefix="$PREFIX" --enable-static --disable-shared --enable-pic \
            --disable-programs --disable-doc --disable-debug --disable-network \
            --disable-avdevice --disable-encoders \
            --enable-encoder=png,mjpeg --disable-muxers \
            --disable-autodetect --enable-zlib --enable-iconv --enable-libdrm --enable-xlib \
            --enable-vaapi --enable-libdav1d
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

# libass: subtitles. FreeType, FriBidi, HarfBuzz and fontconfig come from the system.
meson_build "$WORK/src/libass" "$WORK/b/libass" -Dfontconfig=enabled -Dlibunibreak=disabled \
    -Dtest=disabled -Dcompare=disabled -Dprofile=disabled -Dfuzz=disabled -Dcheckasm=disabled

# mpv as a library only, LGPL.
meson_build "$WORK/src/mpv" "$WORK/b/mpv" -Dgpl=false -Dlibmpv=true -Dcplayer=false \
    -Dlua=disabled -Djavascript=disabled -Dcdda=disabled -Ddvdnav=disabled -Dlibbluray=disabled \
    -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled -Dtests=false \
    -Dlibarchive=disabled -Dzimg=disabled -Dvulkan=disabled -Dshaderc=disabled -Dspirv-cross=disabled \
    -Drubberband=disabled -Dvapoursynth=disabled -Dsixel=disabled -Dsndio=disabled -Dcaca=disabled \
    -Djpeg=disabled -Duchardet=disabled -Dlibavdevice=disabled -Dvdpau=disabled -Dvdpau-gl-x11=disabled \
    -Dpipewire=disabled -Djack=disabled -Dopenal=disabled -Dsdl2-audio=disabled -Dsdl2-video=disabled \
    -Dsdl2-gamepad=disabled -Dgbm=disabled
if grep -q '#define HAVE_GPL 1' "$WORK/b/mpv/config.h"; then
    echo "error: mpv was configured as GPL" >&2
    exit 1
fi

# The link list: our static libraries in link order (from mpv's pkg-config
# file), then the system libraries they use directly, linked dynamically.
rm -rf "${OUT:?}"/*
python3 - "$PREFIX" "$OUT" <<'PY'
import os, re, shutil, subprocess, sys
prefix, out = sys.argv[1], sys.argv[2]
pcdir = os.path.join(prefix, "lib", "pkgconfig")
ours = {f[:-3] for f in os.listdir(pcdir) if f.endswith(".pc")}

def field(pkg, name):
    text = open(os.path.join(pcdir, pkg + ".pc")).read()
    m = re.search(rf"^{name}:(.*)$", text, re.M)
    return m.group(1).strip() if m else ""

def requires(pkg):
    deps = (field(pkg, "Requires") + "," + field(pkg, "Requires.private")).replace(",", " ")
    # Drop version constraints such as ">= 60.3.100".
    return [d for d in re.sub(r"[<>=!]+\s*\S+", " ", deps).split()]

static_order = []
for flag in subprocess.check_output(["pkg-config", "--static", "--libs", "mpv"], text=True).split():
    name = flag[2:] if flag.startswith("-l") else None
    if name and os.path.isfile(os.path.join(prefix, "lib", f"lib{name}.a")) and name not in static_order:
        static_order.append(name)

system_pkgs, raw_libs, seen = [], [], set()
todo = ["mpv"]
while todo:
    pkg = todo.pop()
    if pkg in seen:
        continue
    seen.add(pkg)
    for dep in requires(pkg):
        if dep in ours:
            todo.append(dep)
        elif dep not in system_pkgs:
            system_pkgs.append(dep)
    for flag in (field(pkg, "Libs") + " " + field(pkg, "Libs.private")).split():
        if flag.startswith("-l") and flag[2:] not in static_order and flag[2:] not in raw_libs:
            raw_libs.append(flag[2:])
        elif flag == "-pthread" and "pthread" not in raw_libs:
            raw_libs.append("pthread")

dylibs = []
if system_pkgs:
    for flag in subprocess.check_output(["pkg-config", "--libs", *system_pkgs], text=True).split():
        if flag.startswith("-l") and flag[2:] not in dylibs:
            dylibs.append(flag[2:])
        elif flag.startswith("-L") or flag == "-pthread":
            pass
        else:
            sys.exit(f"error: unexpected link flag {flag}")
dylibs += [l for l in raw_libs if l not in dylibs]
# libplacebo has C++ parts its pkg-config file doesn't mention.
if "stdc++" not in dylibs:
    dylibs.append("stdc++")

with open(os.path.join(out, "link.txt"), "w") as f:
    for name in static_order:
        shutil.copy(os.path.join(prefix, "lib", f"lib{name}.a"), out)
        f.write(f"static=lib{name}.a\n")
    for name in dylibs:
        f.write(f"dylib={name}\n")
print("system packages:", " ".join(system_pkgs))
PY
grep -q '^static=libmpv.a$' "$OUT/link.txt" || { echo "error: libmpv.a is not in the link list" >&2; exit 1; }
strip --strip-debug "$OUT"/*.a

# -Dgpl=false doesn't stop mpv from linking GPL-only libraries it auto-detects
# (librubberband did). Fail if the link list names one.
gpl_only='rubberband|x264|x265|xvidcore|postproc|cdio|dvdnav|dvdread|smbclient|fdk-aac'
if bad=$(sed -n 's/^[a-z]*=\(lib\)\{0,1\}//p' "$OUT/link.txt" | sed 's/\.a$//' | grep -E "^($gpl_only)"); then
    echo "error: GPL-only or non-free libraries are linked: $bad" >&2
    exit 1
fi

for f in mpv/LICENSE.LGPL ffmpeg/COPYING.LGPLv2.1 libplacebo/LICENSE dav1d/COPYING libass/COPYING; do
    cp "$WORK/src/$f" "$OUT/$(dirname "$f")-$(basename "$f")"
done
{
    echo "Static LGPL libmpv for Cinasafe, built by third_party/mpv/build-linux.sh"
    echo "FFmpeg     $FFMPEG_TAG     $FFMPEG_COMMIT   https://github.com/FFmpeg/FFmpeg"
    echo "dav1d      $DAV1D_TAG      $DAV1D_COMMIT   https://code.videolan.org/videolan/dav1d"
    echo "libplacebo $LIBPLACEBO_TAG $LIBPLACEBO_COMMIT   https://code.videolan.org/videolan/libplacebo"
    echo "libass     $LIBASS_TAG     $LIBASS_COMMIT   https://github.com/libass/libass"
    echo "mpv        $MPV_TAG        $MPV_COMMIT   https://github.com/mpv-player/mpv"
    echo
    echo "Built on: $(. /etc/os-release && echo "$PRETTY_NAME"), $(uname -m)"
    echo "Source code: libmpv-sources-linux.tar.gz next to this build, or the commits above."
    echo "The app links these statically; its own source (to relink against a modified"
    echo "library, LGPL-2.1 section 6) is at https://github.com/yuvalkolodkingal/cinasafe."
} > "$OUT/SOURCES.txt"

tar -czf "$OUT/../libmpv-sources-linux.tar.gz" -C "$WORK/src" --exclude=.git ffmpeg dav1d libplacebo libass mpv \
    -C "$here" build-linux.sh

# Check the FFmpeg we link is LGPL.
grep -q '^#define FFMPEG_LICENSE "LGPL version 2.1 or later"' "$WORK/src/ffmpeg/config.h" \
    || { echo "error: FFmpeg's licence is not LGPL 2.1 or later" >&2; exit 1; }
echo "Static libmpv (LGPL) is in $OUT:"
cat "$OUT/link.txt"
