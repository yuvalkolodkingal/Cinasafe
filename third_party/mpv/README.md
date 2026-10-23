# libmpv for Cinasafe builds

Release packages have libmpv **compiled into the app binary**: the
`static-mpv` cargo feature of `crates/cinasafe-player` links static mpv,
FFmpeg, libplacebo, libass and dav1d into the exe. No libmpv file ships, and
nothing looks for one at run time. Without the feature (development, CI unit
tests), the player loads the system's libmpv at run time instead
(`crates/cinasafe-player/src/ffi.rs`).

Everything here is built as **LGPL** so the Apache-2.0 app can include it
(`docs/OPEN_QUESTIONS.md` Q2): mpv with `-Dgpl=false`, FFmpeg without
`gpl`/`version3`/`nonfree`. Both scripts stop if either comes out GPL, or if
anything links a GPL-only library. Static linking meets LGPL-2.1 section 6
because the app's whole source is public: anyone can rebuild it against a
modified libmpv or FFmpeg with these same scripts.

- `build-linux.sh`: static, position-independent mpv, FFmpeg, dav1d, libplacebo
  and libass from pinned commits. FreeType, FriBidi, HarfBuzz, fontconfig,
  libva, lcms2, X11/Wayland/EGL and ALSA/PulseAudio stay dynamic: they are part
  of every desktop that runs the GTK app, and the AppImage bundles the ones
  that aren't on its exclude list. Output: `linux-<arch>/` with the `.a` files
  and `link.txt`, plus `libmpv-sources-linux.tar.gz`.
- `build-windows.ps1`: clang targeting the MSVC ABI, with mpv's own native
  Windows build recipe (mpv `ci/build-win32.ps1`): meson builds mpv and every
  dependency from source as static subprojects, pinned by the wrap files in
  `subprojects-windows/`, with the static C runtime. The exe then depends on
  Windows' own DLLs only. Output: `windows-<arch>/` with the `.a` files,
  `link.txt`, `licenses/` and `SOURCES.txt`, plus
  `libmpv-sources-windows-<arch>.tar.gz`.

`link.txt` lists the libraries in link order, one per line: `static=<file>`
(in that folder) or `dylib=<system library>`. `crates/cinasafe-player/build.rs`
turns it into linker arguments; `CINASAFE_MPV_STATIC_DIR` points it at another
folder.

CI runs both scripts (`.github/workflows/build.yml`) and caches the result
keyed on the scripts and wrap files.

LGPL mode leaves out mpv's X11 video output and the old Direct3D 9 output.
Cinasafe uses neither: on Linux it draws through the render API, on Windows
through `vo=gpu` (Direct3D 11). With mpv's X11 code gone, VAAPI on X11 falls
back to copy-back decoding.

Build locally:

```sh
third_party/mpv/build-linux.sh             # needs the packages listed at its top
cargo test -p cinasafe-player --features static-mpv
cargo build --release -p cinasafe-app --features static-mpv
```

```powershell
third_party/mpv/build-windows.ps1          # needs the tools listed at its top
cargo build --release -p cinasafe-app --features static-mpv
```
