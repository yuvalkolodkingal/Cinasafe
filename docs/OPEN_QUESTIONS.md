# Open questions

Ambiguities in the brief, each with the conservative choice taken until someone decides.

## Q1. The brief was truncated (blocking)

The copy of the brief this work started from ends in section 8.1, partway through
`CREATE TABLE progress (media_file_id INTEGER`. Sections 9–15 are missing: the rest of the
schema, 10 (threat model), 12 (CI and releases), 14 (milestones and acceptance criteria) and
15 (open questions).

**Taken:** Milestone 0 was scoped from sections 0 and 6.3 alone: prove the stream bridge and
the window embedding, record results in `docs/SPIKE.md`, build nothing on top.
**Needed:** the full brief, especially section 14, before Milestone 1.

## Q2. Licence file disagrees with the brief

The brief says GPL-3.0-or-later. The repository's `LICENSE` is Apache-2.0.
**Taken:** neither the licence file nor any `license =` field was changed. Relicensing is the
owner's call. (libmpv and FFmpeg builds with GPL parts would require GPL anyway.)

## Q3. Subtitle base direction (section 7.2)

libass lays out lines left-to-right unless the style says otherwise, so correctly written
Hebrew/Arabic lines show their final punctuation on the wrong side (`docs/SPIKE.md` §4).
**Proposal:** always set `sub-ass-style-overrides=Encoding=-1` (per-line auto direction). Then
the "Fix RTL punctuation" rewrite only has to move leading punctuation to the end; the RLE…PDF
wrap stays as specified and is harmless. Not yet decided; nothing in the app depends on it.

## Q4. Traces on the host from the web engine and GPU driver (section 0)

Even with Tauri `incognito`, WebKitGTK writes `~/.local/share/<identifier>/` (salts, HSTS
database, cache directory). Mesa writes `~/.cache/mesa_shader_cache/`. WebView2 will want a
user-data folder on Windows. None of these contain library data, but they show that Cinasafe
ran on the machine. The brief forbids decrypted data on the host. It does not say whether
usage traces count; that is presumably section 10, which is missing.
**Conservative proposal:** Linux: put the webview data directory in a fresh directory under
`$XDG_RUNTIME_DIR` (RAM-backed), delete it on exit, and set `MESA_SHADER_CACHE_DISABLE=true`
before any GL init. Windows: put the WebView2 user-data folder on the stick under `.cinasafe/`.
The spike shows the Linux variant leaves nothing behind.

## Q5. Other caches mpv/libass may write

- `gpu-shader-cache=no` and `icc-cache=no` are now set alongside the section 6.2 options.
  mpv 0.37 accepts both.
- `screenshot-directory=<none>` (6.2): mpv has no "disable screenshots" option. **Taken:** the
  app never sends a screenshot command and `input-default-bindings=no` stops mpv binding a key.
- fontconfig may write a cache for `sub-fonts-dir` into `~/.cache/fontconfig`. That cache
  lists font files only, but it is still a trace. Covered by the Q4 redirect if we set
  `XDG_CACHE_HOME` for the process.

## Q6. Software GL renderers

With no GPU (or a broken driver), mpv's default quality settings were unusable on llvmpipe
(black video, ~160 ms per frame). **Proposal:** when mpv reports a software renderer, switch to
`profile=fast` automatically.

## Q7. Native display handle for hwdec on Wayland

Passing `wl_display` to the render API enables VAAPI interop, but it crashed in `vaInitialize`
on a machine without a GPU. **Proposal:** pass native display handles only when a DRM render
node exists; otherwise fall back to software decoding.

## Q8. Format detail: maximum chunk size

Section 4.4 sets the default chunk size but no upper bound. A hostile header could otherwise
ask the reader to allocate up to 4 GiB per chunk. **Taken:** readers reject chunk sizes of 0
or over 64 MiB (`MAX_CHUNK_SIZE`). Writers only use 1 MiB (tests use smaller). Documented in
`FORMAT.md`; no format-version change, since every blob the spec allows writing is still
readable.

## Q9. Shipping a TMDB API key in release builds

TMDB is free for non-commercial use with attribution, but its terms don't say whether an
open-source app may embed a key. **Taken:** no key is shipped. Users without a key still
get TV metadata from TVmaze; movie metadata needs their own free TMDB key
(`CINASAFE_TMDB_KEY`, later a Settings field). If TMDB confirms a project key is fine, CI can
inject it from a repository secret at build time.

## Q10. Packaging formats vs. the brief's "run from the stick"

The brief ships a portable `.exe` and AppImages on the stick, with no install. `.deb`, `.rpm`
and the Windows installer were added on request; they install Cinasafe on a computer and
complement the portable builds. Notes:

- The `.deb` depends on `libmpv2` (Debian 12+, Ubuntu 23.04+). **Ubuntu 22.04 and Mint 21
  only have libmpv1** (client API 1.x, unsupported). For those, libmpv has to be bundled,
  which is what the AppImage is for (not built yet: libmpv is loaded at runtime, so
  `linuxdeploy` won't pick it up automatically; it needs an explicit bundling recipe).
- The `.rpm` requires the soname `libmpv.so.2()(64bit)`, which Fedora (`mpv-libs`) and
  openSUSE (`libmpv2`) both provide.
- Windows builds are **unsigned**, so SmartScreen will warn. Code signing needs a certificate
  (decision and cost for the owner).
- Only x86_64 is built so far. ARM64 Windows and aarch64 Linux need ARM runners, or cross
  builds plus an ARM64 libmpv.
- `libmpv-2.dll` comes from a third-party build (zhongfly/mpv-winbuild) and is GPL. Pin it
  (`LIBMPV_RELEASE`, `LIBMPV_SHA256` repository variables) before any public release, or
  build it ourselves (`third_party/mpv`).
