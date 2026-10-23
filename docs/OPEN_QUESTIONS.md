# Open questions

Ambiguities in the brief, each with the conservative choice taken until someone decides.

## Q1. The brief was truncated (resolved)

Milestone 0 started from a copy of the brief that stopped in section 8.1. The full brief is now
in `docs/BRIEF.md`. Milestone 0 is checked against its section 14 criteria in
`docs/SPIKE.md`. Several earlier guesses turn out to match it: user-supplied TMDB key (8.3),
temp webview data dir (10), checksummed tag releases (12).

## Q2. Licence (resolved: Apache-2.0)

The brief originally said GPL-3.0-or-later; the owner chose **Apache-2.0**, which `LICENSE`
already contained. `docs/BRIEF.md`, every crate's `license` field and the package metadata now
say Apache-2.0.

**Consequence for libmpv (open):** the app loads libmpv at runtime, but the Windows packages
ship `libmpv-2.dll` next to it. The third-party build CI downloads today (zhongfly/mpv-winbuild)
enables GPL parts of mpv and FFmpeg. Shipping that DLL makes the bundle as a whole subject to
the GPL, and its source must be offered. To keep releases cleanly Apache-2.0:
- build libmpv ourselves in its LGPL configuration (mpv `-Dgpl=false`, FFmpeg without
  `--enable-gpl`/`--enable-nonfree`), as `third_party/mpv` is meant to (brief 3.1, 12), and
- ship the LGPL licence texts and a source offer for the DLL.
LGPL builds lose a few GPL-only filters and encoders that playback doesn't need. Until that
exists, treat CI's Windows artifacts as test builds, not releases. Linux packages use the
distribution's libmpv, so the distribution handles it.

## Q3. Subtitle base direction (section 7.2)

libass lays out lines left-to-right unless the style says otherwise, so correctly written
Hebrew/Arabic lines show their final punctuation on the wrong side (`docs/SPIKE.md` §4).
**Proposal:** always set `sub-ass-style-overrides=Encoding=-1` (per-line auto direction). Then
the "Fix RTL punctuation" rewrite only has to move leading punctuation to the end; the RLE…PDF
wrap stays as specified and is harmless. Not yet decided; nothing in the app depends on it.

## Q4. Traces on the host from the web engine and GPU driver (answered by brief 10)

Even with Tauri `incognito`, WebKitGTK writes `~/.local/share/<identifier>/` (salts, HSTS
database, cache directory), Mesa writes `~/.cache/mesa_shader_cache/`, and WebView2 needs a
user-data folder. Brief section 10 settles it: point the WebView2 user-data folder and
WebKitGTK's data and cache dirs at a fresh temp dir, delete it on exit and on next launch, and
disable disk caching. To do in the app shell. Also set `MESA_SHADER_CACHE_DISABLE=true` before
any GL init. The spike shows the Linux variant leaves nothing behind.

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

## Q11. Library schema additions (section 8.1)

`crates/cinasafe-core/src/library/schema.sql` follows the brief's schema, with these additions:
- `show.tvmaze_id`: shows identified through TVmaze (no TMDB key) need their own id.
- `show.audio_lang_override` / `subtitle_lang_override`: brief 7.3 asks for per-title
  overrides on detail pages, and shows have detail pages too.
- `idx_movie_media`, and the FTS5 table `search_index(kind, ref_id, title)` that brief 8.1
  mentions without defining.
`schema_version` stays 1 because no library has been written with an earlier layout.

## Q12. Online lookups default and keys (brief 15)

**Taken:** off by default. Add media has a checkbox, bound to Settings › Privacy. When on,
shows use TVmaze without a key, and TMDB is used for movies and shows once the user enters
their own key in Settings (see Q9). The checkbox explains that parsed titles are sent to the
provider.

## Q13. Decisions made while building the import queue and player (M2–M4)

- **Subtitles at import:** converted to UTF-8 and encrypted. "Fix RTL punctuation" is
  *offered* (the queue notes it), not applied. The fixed copy is made the first time the
  user ticks the box in the player, and the original is kept (brief 7.2).
- **Default subtitle track:** the first preferred language that matches. With no preference
  set, the first imported subtitle file (the user brought it on purpose); otherwise Off.
- **Subtitle timing** is remembered per imported subtitle file. Embedded tracks' offsets
  last only for the session, because the schema has no place for them. Proposal: a
  `media_file.sub_delay_ms` column in schema v2.
- **Artwork** is re-encoded as JPEG (quality 85) at 500/1280/400 px (poster, backdrop, still).
- **Progress** is saved every 10 s of playback; each save rewrites the encrypted library on
  the stick. Pulling the stick loses at most 10 s.
- **Idle lock** doesn't fire while a video is playing (paused playback counts as idle).
- **Removing a title** forgets it in the library first, then deletes the blobs, so an
  interruption leaves orphans (the integrity check reports them), never dangling references.

## Q14. Layout of the player apps on the stick (brief 4.1)

- Windows needs `libmpv-2.dll` next to `Cinasafe-Windows.exe`, so the stick root also has
  that DLL. Brief 4.1 shows only the exe.
- `.cinasafe/apps.json` records which app version was copied for each platform, so
  Settings › Vault can show it. It is plaintext but says nothing about the library.
- **Eject:** Linux unmounts through UDisks2 and powers the drive off. Windows doesn't call
  an eject API yet; locking already flushed every write, so pulling the stick is safe once
  locked.
- An app killed by the OS (not closed) leaves its temp session dir until the next start,
  which deletes it.
