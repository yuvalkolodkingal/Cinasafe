# libmpv for Cinasafe builds

Cinasafe loads libmpv at runtime (`crates/cinasafe-player/src/ffi.rs`); nothing links
against it. The builds here are **LGPL**, so the Apache-2.0 app can ship them
(`docs/OPEN_QUESTIONS.md` Q2):

- `build-linux.sh` builds from pinned commits: FFmpeg (without `--enable-gpl`/`--enable-nonfree`),
  dav1d, libplacebo, and mpv with `-Dgpl=false`. libass, HarfBuzz, FriBidi and FreeType come from
  the system; linuxdeploy bundles them into the AppImage. Output: `linux-<arch>/`, plus
  `libmpv-sources-linux.tar.gz`.
- `build-windows.sh` runs in MSYS2 (UCRT64 or CLANGARM64). It builds FFmpeg and mpv from
  pinned commits and takes the other libraries from MSYS2 packages. Output: `windows-<arch>/`,
  holding `libmpv-2.dll` and every DLL it needs, plus `licenses/`, `SOURCES.txt` and
  `libmpv-sources-windows-<arch>.tar.gz`.

Both scripts stop if FFmpeg or mpv comes out GPL. CI runs them (`.github/workflows/build.yml`)
and caches the result keyed on the script's contents.

LGPL mode leaves out mpv's X11 video output and the old Direct3D 9 output. Cinasafe uses
neither: on Linux it draws through the render API, on Windows through `vo=gpu` (d3d11).
With mpv's X11 code gone, VAAPI on X11 falls back to copy-back decoding.

The `.deb` and `.rpm` packages use the distribution's libmpv (`libmpv2` on Debian 12+ and
Ubuntu 23.04+, `mpv-libs` on Fedora, `libmpv2` on openSUSE). Ubuntu 22.04 only has libmpv1,
which Cinasafe doesn't support; use the AppImage there.

Build locally:

```sh
third_party/mpv/build-linux.sh                      # needs the packages listed at its top
CINASAFE_LIBMPV_DIR=$PWD/third_party/mpv/linux-x86_64 cargo test -p cinasafe-player
```
