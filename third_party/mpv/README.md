# libmpv for Cinasafe builds

Cinasafe loads libmpv at runtime (`crates/cinasafe-player/src/ffi.rs`); nothing links against it.

- **Linux (.deb / .rpm):** the packages depend on the distribution's libmpv
  (`libmpv2` on Debian 12+/Ubuntu 23.04+, `libmpv.so.2()(64bit)` from `mpv-libs` on Fedora or
  `libmpv2` on openSUSE). Ubuntu 22.04 only ships libmpv1 (client API 1.x), which Cinasafe does not
  support.
- **Windows (.exe):** `libmpv-2.dll` must sit next to the executable. CI downloads an x86_64
  "mpv-dev" build from https://github.com/zhongfly/mpv-winbuild and places it in
  `windows-x86_64/` (git-ignored) before bundling. Pin a release and its SHA-256 with the
  repository variables `LIBMPV_RELEASE` and `LIBMPV_SHA256` (see `.github/workflows/build.yml`).
  Those builds enable GPL parts of mpv and FFmpeg. Cinasafe is Apache-2.0, so before releases
  this must become our own LGPL build (`-Dgpl=false`, FFmpeg without `--enable-gpl`) shipped
  with its licence texts and a source offer. See `docs/OPEN_QUESTIONS.md` Q2.

Building libmpv from source per target (brief section 3.1) is still to do.
