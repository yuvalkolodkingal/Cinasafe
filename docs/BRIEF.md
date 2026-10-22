# Cinasafe: build brief for Claude Code

Cinasafe turns any USB stick into an encrypted, portable movie and TV library with a streaming-service interface. It runs straight from the stick on Windows and Linux, with no install and no admin rights. It is free and open source (Apache-2.0).

This file is the single source of truth for building v1. Read all of it before writing code. Work milestone by milestone (section 14) and do not start a milestone until the previous one meets its acceptance criteria.

---

## 0. How to work on this project

- Start with **Milestone 0 (the spike)**. It decides whether the architecture holds. Report results before building anything else on top of it.
- Never hand-roll cryptography. Use only the crates named in section 5.
- Never write decrypted media, posters, subtitles or library data to the host disk. No temp files, no caches on disk, no logs that contain titles or file names. Treat any violation as a release-blocking bug.
- Every format decision in section 4 is a contract. If you need to change it, update `docs/FORMAT.md` and bump the format version, and say so.
- Write tests alongside code. Crypto and format code needs test vectors checked into `crates/cinasafe-core/tests/vectors/`.
- Keep commits small and scoped to one concern.
- When something in this brief is ambiguous, list it in `docs/OPEN_QUESTIONS.md` and pick the most conservative option.

---

## 1. Goals and non-goals

**Goals (v1)**

1. Plug in, type a password, browse posters, press play.
2. Nothing decrypted is ever written to the host computer's disk.
3. Plays anything mpv plays, with full subtitle support for LTR and RTL languages (Hebrew and Arabic must look correct).
4. Movies and TV shows are both first-class: seasons, episodes, resume, up next.
5. The same app binary creates vaults (the "formatter") and opens them.
6. Interface in English, Hebrew and Arabic, fully mirrored for RTL.

**Non-goals (v1)**

- macOS (planned for v2; needs signing and notarization).
- Casting or streaming to other devices.
- Cloud sync, accounts, multiple users or profiles per vault.
- Ripping or bypassing DRM. Cinasafe stores files the user already has.

---

## 2. Platforms

| Platform | Versions | Artifact on the stick | Web engine | Notes |
| --- | --- | --- | --- | --- |
| Windows | 10 (22H2), 11; x64 and ARM64 | `Cinasafe-Windows.exe` (portable, single exe plus `libmpv-2.dll` beside it if it can't be statically linked) | WebView2 | WebView2 ships with Windows 11 and current Windows 10. Detect a missing runtime and show a clear message. |
| Linux | Ubuntu 22.04+, Debian 12+, Fedora 39+, Mint 21+, Arch, openSUSE Tumbleweed; x86_64 and aarch64 | `Cinasafe-Linux-x86_64.AppImage`, `Cinasafe-Linux-aarch64.AppImage` | WebKitGTK 4.1, bundled in the AppImage | Must work on GNOME and KDE, Wayland and X11. |
| macOS | v2 | `Cinasafe.app` | WKWebView | Out of scope for v1. |

`run-linux.sh` sits next to the AppImages. It runs the AppImage with `sh` semantics for mounts where the exec bit is missing (common on exFAT), copying it to `$XDG_RUNTIME_DIR` first if direct execution fails.

---

## 3. Architecture

Three parts run on the host. Only the Rust core touches the stick or holds keys.

```
┌────────────────────── Host computer: plaintext only in RAM ──────────────────────┐
│                                                                                   │
│  ┌───────────── Tauri web UI ─────────────┐    ┌──────────── libmpv ────────────┐ │
│  │ React + TypeScript                      │    │ Decodes video, audio, subs     │ │
│  │ Library, detail pages, player controls  │    │ libass + HarfBuzz + FriBidi    │ │
│  │ i18n + RTL via dir="rtl", logical CSS   │    │ RAM-only cache, no history     │ │
│  └──────────────┬─────────────────────────┘    └───────────────▲────────────────┘ │
│                 │ Tauri commands + events                       │ cinasafe:// stream│
│                 │ cinasafe-img:// (artwork from memory)          │ (decrypted bytes) │
│  ┌──────────────▼───────────────────────────────────────────────┴────────────────┐ │
│  │ Rust core                                                                      │ │
│  │ Unlock (Argon2id), master key in locked RAM, zeroized on lock                  │ │
│  │ Blob reader/writer: 1 MiB AES-256-GCM chunks, read-ahead, chunk cache          │ │
│  │ Library: in-memory SQLite, saved encrypted with atomic rename                  │ │
│  │ Import queue, file-name parser, TMDB client, subtitle charset conversion       │ │
│  └──────────────────────────────────┬─────────────────────────────────────────────┘ │
└─────────────────────────────────────┼──────────────────────────────────────────────┘
                                      │ encrypted reads and writes
┌─────────────────────────────────────▼──────────────────────────────────────────────┐
│ USB stick (exFAT): vault.json · library.db.enc · blobs/                             │
└────────────────────────────────────────────────────────────────────────────────────┘
```

The UI never sees video bytes. It sends commands to the core; the core feeds mpv decrypted chunks through a custom stream protocol.

### 3.1 Repository layout

```
cinasafe/
├── Cargo.toml                      # workspace
├── crates/
│   ├── cinasafe-core/              # vault, crypto, blob format, library, import, metadata
│   │   ├── src/{vault,crypto,blob,library,import,metadata,subtitles,drives}.rs
│   │   └── tests/vectors/          # format test vectors (JSON + binary)
│   ├── cinasafe-player/            # libmpv wrapper + cinasafe:// stream bridge
│   └── cinasafe-cli/               # `cinasafe` CLI for testing and scripting
├── app/
│   ├── src-tauri/                  # Tauri 2 shell, commands, events, window/mpv embedding
│   └── ui/                         # React + TypeScript + Vite
│       ├── src/{screens,components,i18n,styles,api}/
│       └── public/fonts/           # Rubik, Noto Sans Hebrew/Arabic (bundled, no CDN)
├── third_party/mpv/                # scripts to build libmpv + FFmpeg + libass per target
├── docs/
│   ├── FORMAT.md                   # vault + blob format (section 4), with vectors
│   ├── THREAT_MODEL.md             # section 10
│   └── OPEN_QUESTIONS.md
├── packaging/                      # AppImage recipe, Windows bundling, run-linux.sh, README.txt
└── .github/workflows/              # CI builds and releases (section 12)
```

---

## 4. Drive layout and vault format

### 4.1 Drive layout

The stick is plain **exFAT**. exFAT mounts on every target without drivers and has no 4 GB file-size limit. FAT32 and NTFS sticks are offered a reformat; FAT32 is never used as is.

```
CINASAFE (exFAT)
├── Cinasafe-Windows.exe
├── Cinasafe-Linux-x86_64.AppImage
├── Cinasafe-Linux-aarch64.AppImage
├── run-linux.sh
├── README.txt
└── .cinasafe/
    ├── vault.json             header: format version, KDF params, salt, wrapped keys
    ├── library.db.enc         encrypted SQLite library (blob format)
    ├── library.db.enc.bak     previous library version (blob format)
    └── blobs/ab/cd/<blob-id>.cblob   encrypted media, artwork, subtitles
```

`<blob-id>` is 16 random bytes, lowercase hex (32 chars). `ab` and `cd` are its first two byte pairs. Blob file names reveal nothing about content.

### 4.2 Keys

1. **Password → KEK.** Argon2id, memory 256 MiB, 3 passes, 4 lanes, 16-byte random salt, 32-byte output. Parameters are stored in `vault.json` so they can change in later versions.
2. **Master key.** 32 random bytes from the OS CSPRNG, generated once at vault creation.
3. **Wrapping.** The master key is encrypted with AES-256-GCM under the KEK. AAD = `"cinasafe-wrap-v1" || vault_id || kind`. Changing the password re-wraps only; no media is re-encrypted.
4. **Recovery key (optional).** 24 BIP39 English words = 256 bits of entropy. Because it's high-entropy, its KEK is `HKDF-SHA256(ikm = entropy, salt = vault_id, info = "cinasafe-recovery-v1")`, no Argon2. It wraps the same master key a second time.
5. **Per-blob key.** `HKDF-SHA256(ikm = master_key, salt = vault_id, info = "cinasafe-blob-v1" || blob_id)` → 32 bytes.

### 4.3 `vault.json`

```json
{
  "format": "cinasafe-vault",
  "version": 1,
  "vault_id": "<16 bytes, base64>",
  "created_at": "2026-10-22T14:00:00Z",
  "app_version": "0.1.0",
  "kdf": { "alg": "argon2id", "m_kib": 262144, "t": 3, "p": 4, "salt": "<16 bytes, base64>" },
  "wraps": [
    { "kind": "password", "nonce": "<12 bytes, base64>", "ciphertext": "<48 bytes, base64>" },
    { "kind": "recovery", "nonce": "<12 bytes, base64>", "ciphertext": "<48 bytes, base64>" }
  ]
}
```

`vault.json` holds no titles or user data. A vault with `version` higher than the app supports shows "Update Cinasafe to open this vault" and opens nothing.

### 4.4 Blob format (`.cblob`)

Used for every encrypted file: media, posters, backdrops, thumbnails, subtitle files and the library database.

**Header, 44 bytes:**

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | Magic `CSFB` (ASCII) |
| 4 | 1 | Format version, `1` |
| 5 | 3 | Reserved, zero |
| 8 | 16 | Blob id |
| 24 | 8 | Plaintext size, u64 little-endian |
| 32 | 4 | Chunk size, u32 little-endian (default 1 048 576) |
| 36 | 8 | Nonce prefix, random per blob |

**Body:** chunks in order. Chunk `i` holds plaintext bytes `[i × chunk_size, min((i+1) × chunk_size, plaintext_size))`, sealed with AES-256-GCM:

- Key: the per-blob key (4.2).
- Nonce (12 bytes): `nonce_prefix (8) || i as u32 big-endian (4)`.
- AAD: `header (44 bytes) || i as u32 big-endian || last_flag (1 byte: 1 for the final chunk, else 0)`.
- On disk: ciphertext followed by the 16-byte tag.
- Chunk `i` starts at byte `44 + i × (chunk_size + 16)`.
- A zero-length file has exactly one empty final chunk.

Binding the header, index and last-chunk flag into every chunk's AAD prevents chunk swapping, reordering, truncation and header tampering. Any chunk decrypts on its own, which makes seeking instant.

**Padding:** pad media blobs' on-disk size up to the next multiple of 1 MiB with zero bytes after the last chunk (ignored when reading) to blur exact sizes.

### 4.5 Library database

- SQLite, held in memory after unlock (`rusqlite` with the `serialize` feature).
- On every committed change (debounced to at most once per 2 s): serialize, encrypt as a blob with the reserved blob id of 16 zero bytes, write `library.db.enc.tmp`, fsync, rename the current file to `library.db.enc.bak`, rename `.tmp` to `library.db.enc`.
- On unlock, if `library.db.enc` fails to decrypt, fall back to `.bak` and tell the user.

### 4.6 What leaks to someone holding the stick

That it's a Cinasafe vault, total space used, number of blobs and their approximate sizes. Not titles, artwork, file names, subtitles or watch history.

---

## 5. Core crate (`cinasafe-core`)

### 5.1 Dependencies (use these, nothing hand-rolled)

- `aes-gcm`, `argon2`, `hkdf`, `sha2` (RustCrypto)
- `rand_core` / `getrandom` for randomness
- `zeroize` for all key material and decrypted buffers
- `memsec` or `region` to lock key pages in RAM where the OS allows (best-effort)
- `rusqlite` with `bundled` and `serialize`
- `bip39` for recovery words
- `encoding_rs` + `chardetng` for subtitle charset detection
- `serde`, `serde_json`, `thiserror`
- `reqwest` (rustls) for TMDB

### 5.2 Public API (sketch)

```rust
pub struct Vault { /* path, vault_id, master_key: Zeroizing<[u8; 32]>, library: Library */ }

impl Vault {
    pub fn create(root: &Path, password: &SecretString, with_recovery: bool) -> Result<(Vault, Option<RecoveryWords>)>;
    pub fn unlock_with_password(root: &Path, password: &SecretString) -> Result<Vault>;
    pub fn unlock_with_recovery(root: &Path, words: &RecoveryWords) -> Result<Vault>;
    pub fn change_password(&mut self, new: &SecretString) -> Result<()>;
    pub fn replace_recovery(&mut self) -> Result<RecoveryWords>;
    pub fn lock(self);                                   // zeroizes everything
    pub fn open_blob(&self, id: BlobId) -> Result<BlobReader>;   // Read + Seek, plaintext view
    pub fn write_blob(&self, src: impl Read, size: u64) -> Result<BlobId>; // streaming encrypt + verify
    pub fn delete_blob(&self, id: BlobId) -> Result<()>;
    pub fn library(&self) -> &Library;
    pub fn integrity_check(&self, progress: impl Fn(Progress)) -> Result<Report>;
}
```

`BlobReader` implements `Read + Seek`, decrypts only the chunks a read touches, keeps an LRU cache of 16 decrypted chunks (zeroized on eviction) and prefetches 2 chunks ahead on a worker thread.

### 5.3 Drive detection (`drives.rs`)

List removable drives with name, size, free space, file system and mount path.
- Windows: `GetLogicalDrives` + `GetDriveTypeW == DRIVE_REMOVABLE` + `GetVolumeInformationW`.
- Linux: UDisks2 over D-Bus (`zbus`), filtering `Removable` / `HintSystem = false`.
- Watch for removal (Windows `WM_DEVICECHANGE`, Linux UDisks2 signals). Removal of the open vault's drive triggers an immediate lock.

### 5.4 Formatting (optional, needs admin)

- Windows: run `format <letter>: /FS:exFAT /Q /V:CINASAFE /Y` via an elevated helper (UAC prompt).
- Linux: `pkexec mkfs.exfat -n CINASAFE <device>` after unmounting via UDisks2, then remount.
- Always require the user to type the drive's label to confirm.

---

## 6. Playback (`cinasafe-player`)

### 6.1 Stream bridge

- Register a custom protocol with `mpv_stream_cb_add_ro(ctx, "cinasafe", ...)`.
- mpv opens `cinasafe://<blob-id-hex>`.
- `open` → `vault.open_blob(id)`; `read` → `BlobReader::read`; `seek` → `BlobReader::seek`; `size` → plaintext size; `close` → drop and zeroize.
- External subtitle blobs are added with `sub-add cinasafe://<id>`.

### 6.2 mpv options Cinasafe always sets

```
config=no
load-scripts=no
ytdl=no
cache=yes
cache-on-disk=no
demuxer-max-bytes=256MiB
demuxer-max-back-bytes=64MiB
save-position-on-quit=no
write-filename-in-watch-later-config=no
screenshot-directory=<none; screenshots disabled>
hwdec=auto-safe              # user can turn off in Settings
osc=no
input-default-bindings=no    # all keys handled by the app
sub-ass-shaper=complex
sub-fonts-dir=<bundled fonts dir inside the app, read-only>
sub-codepage=utf-8           # subtitles are converted to UTF-8 on import
```

Build libmpv with libass, HarfBuzz and FriBidi enabled.

### 6.3 Embedding mpv in the Tauri window (Milestone 0 spike)

The video must appear inside the app window, with the web UI drawn on top for controls. Try in this order and record what works per platform in `docs/SPIKE.md`:

1. **Windows and X11:** create a native child window under a transparent webview and pass its handle to mpv's `wid` option.
2. **Wayland:** use mpv's render API (`mpv_render_context` with OpenGL) into a native GL surface or subsurface under the transparent webview.
3. **Fallback for any platform:** mpv draws to its own borderless native window sized and positioned over the app window, with a second transparent, always-on-top Tauri window for controls. Acceptable if 1 and 2 fail; note the trade-offs.

### 6.4 Player behaviour

- Controls fade after 3 s without mouse movement; any input brings them back.
- Keyboard: Space play/pause, ←/→ seek 10 s, ↑/↓ volume, F fullscreen, M mute, S cycle subtitles, A cycle audio, Esc exit fullscreen or back.
- Resume: save position every 10 s and on stop (encrypted, in the library). Mark watched at 90%.
- Next episode: a card in the last 30 s (or from the "credits" chapter if one exists) with a 10 s countdown; Cancel stops autoplay.
- If the measured read speed from the stick falls below the file's bitrate, show a non-blocking "This stick may be too slow for this file" notice.

---

## 7. Subtitles and RTL

### 7.1 Formats

- External, imported next to their video: SRT, ASS/SSA, WebVTT.
- Embedded in MKV/MP4: text tracks, PGS and VobSub.
- Fonts attached inside MKV files are used by libass.

### 7.2 Correct RTL rendering

- libass with HarfBuzz and FriBidi shapes and orders Hebrew and Arabic correctly, including mixed Hebrew/English lines.
- Bundle Noto Sans, Noto Sans Hebrew and Noto Sans Arabic as fallback subtitle fonts.
- On import, detect the charset with `chardetng` (expect Windows-1255, Windows-1256, ISO-8859-8) and convert to UTF-8 with `encoding_rs` before encrypting.
- **Fix RTL punctuation** (per subtitle file, default off; offered automatically when a Hebrew/Arabic SRT has lines starting with `.`, `,`, `?`, `!` or `-`): rewrite each cue line so leading punctuation moves to the end and wrap the line in RLE (U+202B) … PDF (U+202C). Store the fixed version as a new blob; keep the original.

### 7.3 Subtitle controls

- Track picker including Off and imported files.
- Timing offset in 0.1 s steps (`sub-delay`), remembered per media file.
- Size, vertical position, text colour, background box.
- Preferred subtitle and audio languages in Settings (ordered list, e.g. Hebrew, English), applied when a file opens; per-title overrides on detail pages.

### 7.4 RTL interface

- Languages: English, Hebrew, Arabic. Strings in `app/ui/src/i18n/{en,he,ar}.json` via `i18next`.
- RTL locales set `dir="rtl"` and `lang` on `<html>`. All layout uses CSS logical properties (`margin-inline-start`, `padding-inline`, `inset-inline-end`), never left/right. No separate RTL stylesheet.
- Directional icons (back, chevrons) flip in RTL. Play, pause, volume and the seek bar do not; time runs left to right. (Open question, see section 15.)
- Every title, overview and file name renders with `dir="auto"` (or `<bdi>`), so an English title in a Hebrew UI reads correctly.

---

## 8. Library model

### 8.1 SQL schema (v1)

```sql
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);  -- schema_version, settings JSON

CREATE TABLE media_file (
  id INTEGER PRIMARY KEY,
  blob_id BLOB NOT NULL UNIQUE,          -- 16 bytes
  original_name TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  duration_ms INTEGER,
  container TEXT,
  streams_json TEXT,                      -- video/audio/subtitle stream summary from mpv probe
  added_at TEXT NOT NULL
);

CREATE TABLE artwork (
  id INTEGER PRIMARY KEY,
  blob_id BLOB NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('poster','backdrop','thumb')),
  width INTEGER, height INTEGER
);

CREATE TABLE movie (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL, original_title TEXT, year INTEGER,
  runtime_min INTEGER, overview TEXT, genres TEXT,       -- genres as JSON array
  tmdb_id INTEGER,
  poster_id INTEGER REFERENCES artwork(id),
  backdrop_id INTEGER REFERENCES artwork(id),
  media_file_id INTEGER NOT NULL REFERENCES media_file(id),
  audio_lang_override TEXT, subtitle_lang_override TEXT,
  added_at TEXT NOT NULL
);

CREATE TABLE show (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL, original_title TEXT, first_year INTEGER, last_year INTEGER,
  overview TEXT, genres TEXT, tmdb_id INTEGER,
  poster_id INTEGER REFERENCES artwork(id),
  backdrop_id INTEGER REFERENCES artwork(id),
  added_at TEXT NOT NULL
);

CREATE TABLE season (
  id INTEGER PRIMARY KEY,
  show_id INTEGER NOT NULL REFERENCES show(id) ON DELETE CASCADE,
  number INTEGER NOT NULL,                -- 0 = specials
  title TEXT, poster_id INTEGER REFERENCES artwork(id),
  UNIQUE (show_id, number)
);

CREATE TABLE episode (
  id INTEGER PRIMARY KEY,
  season_id INTEGER NOT NULL REFERENCES season(id) ON DELETE CASCADE,
  number INTEGER NOT NULL,
  title TEXT, overview TEXT, air_date TEXT, runtime_min INTEGER,
  thumb_id INTEGER REFERENCES artwork(id),
  media_file_id INTEGER NOT NULL REFERENCES media_file(id),
  UNIQUE (season_id, number)
);

CREATE TABLE subtitle_file (
  id INTEGER PRIMARY KEY,
  media_file_id INTEGER NOT NULL REFERENCES media_file(id) ON DELETE CASCADE,
  blob_id BLOB NOT NULL UNIQUE,
  original_blob_id BLOB,                  -- set when an RTL-fixed copy exists
  language TEXT, format TEXT NOT NULL,
  rtl_fix INTEGER NOT NULL DEFAULT 0,
  delay_ms INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE progress (
  media_file_id INTEGER PRIMARY KEY REFERENCES media_file(id) ON DELETE CASCADE,
  position_ms INTEGER NOT NULL,
  watched INTEGER NOT NULL DEFAULT 0,
  last_watched_at TEXT NOT NULL
);

CREATE INDEX idx_movie_title ON movie(title);
CREATE INDEX idx_show_title ON show(title);
CREATE INDEX idx_episode_media ON episode(media_file_id);
```

Search uses an FTS5 table over movie titles, show titles and episode titles.

### 8.2 Import pipeline

1. User drops files or folders, or uses Browse.
2. Walk folders; classify video files (by extension and probe) and subtitle files.
3. Parse names: `Show.Name.S02E05...`, `Show Name - 2x05`, `Movie Name (2014)`, `Movie.Name.2014.2160p...`. Sidecar subtitles (`Movie.he.srt`, `Movie.en.forced.srt`) attach to the matching video with the language code.
4. If online lookup is on, query TMDB, show candidates for confirmation; low-confidence matches go to "Needs review".
5. Check free space before starting; flag items that won't fit.
6. Encrypt each file in a streaming pass with `write_blob`, then re-read and verify every chunk before inserting library rows.
7. Download artwork once, resize (posters 500 px wide, backdrops 1280 px, thumbs 400 px), encrypt as blobs.
8. Queue can be paused, resumed and cancelled. Cancelling deletes partially written blobs.

All metadata is editable by hand. Everything works fully offline.

### 8.3 TMDB

- Off or on per the user's choice (Settings › Privacy). Lookups send titles to TMDB; say so in the UI.
- Users enter their own TMDB API key in Settings, or use a project key if the project gets one (open question).
- Show TMDB attribution in About, as their terms require.

---

## 9. UI

### 9.1 Design tokens (from the approved mockups)

| Token | Value | Use |
| --- | --- | --- |
| `--bg` | `#0E0F12` | App background |
| `--surface-1` | `#17191E` | Cards, panels, inputs on detail pages |
| `--surface-2` | `#20232A` | Secondary buttons, selected nav item |
| `--surface-3` | `#26292F` | Selected row inside menus |
| `--border` | `#343842` | Input and outline-button borders |
| `--text` | `#F2F1EE` | Primary text |
| `--text-muted` | `#A3A7B0` | Secondary text (passes 4.5:1 on `--bg`) |
| `--text-soft` | `#D6D8DD` | Labels |
| `--accent` | `#F2B33D` | Primary buttons, progress, focus ring, active state |
| `--on-accent` | `#141414` | Text on accent |
| `--good` | `#8FD18B` | Matched, ready, strong password |
| `--danger-border` | `#E0705F` | Destructive option border |
| `--danger-text` | `#FF9C94` | Destructive button text |
| `--warning-bg` | `#2A2418` | Warning note background |
| Hero backdrop placeholder | `#1C2B33` | Used until artwork exists |

- **Type:** Rubik 400/500/600/700 for everything (it covers Latin and Hebrew); Noto Sans Arabic as the Arabic fallback. Bundle font files; never load from a CDN at runtime.
- **Sizes:** page title 36 px/700; hero title 52–56 px/700; section heading 20 px/600; body 15–16 px; meta 13–14 px.
- **Shape:** buttons and inputs radius 10 px; cards and panels 14–16 px; posters 10 px; pills 999 px.
- **Spacing:** page gutter 48 px; grid gaps 20 px; card padding 24–28 px.
- **Targets:** every interactive element at least 44 × 44 px. Visible focus ring in `--accent`.
- **Posters:** 2:3. Thumbnails and continue-watching cards: 16:9 with a 4 px progress bar in `--accent` along the bottom.
- **Icons:** inline stroke SVG, 2 px stroke, round caps. No emoji.
- **Logo:** rounded square in `--accent` with a dark play triangle, next to the word "Cinasafe" in 20 px/700.
- Dark theme is the default; a light theme is optional for v1.

### 9.2 Global chrome

Top bar on every screen except Unlock, the player and the wizard: logo (links Home) · Home · Movies · Shows · Search · then at the end: **Add** button, Settings icon button, Lock icon button. Active item is `--text` + 600 weight; others `--text-muted`.

### 9.3 Screens

1. **Unlock.** Centered column, max 440 px. Large logo, "Unlock your vault", a pill with drive label, size and space used. Card with Password field + show/hide button, accent Unlock button, line "Unlocking takes about a second while your password is checked." Below: "Use recovery key" link and a Language select (English / עברית / العربية). Footer note: "Nothing is decrypted to this computer's disk. Pulling the stick locks the vault." States: unlocking spinner, wrong password, newer vault version, drive removed.
2. **Home.** Hero (backdrop, eyebrow "Continue watching", title, "Season 2 · Episode 5 · The Crossing", progress bar with minutes left, Resume and More info). Rows: Continue watching (16:9 cards with progress), Up next, Recently added (posters), Movies, Shows. Empty vault: large "Add your first movie or show" call to action.
3. **Movies / Shows grid.** Title with count and total size ("48 movies · 312 GB"). Segmented filter: All / Unwatched / In progress / Watched. Genre select. Sort select (Date added, Title, Year, Last watched). Poster grid; watched check badge top-end; progress bar on in-progress items; year and runtime under each.
4. **Movie detail.** Backdrop band with poster (220 px), title, meta line ("2021 · 1 h 58 min · Sci-fi, Drama · 4K HDR"), overview, progress, buttons Resume / Play from start / Mark watched. Two cards below: "Defaults for this movie" (Audio and Subtitles selects) and "File" (original name, video, audio, subtitles, encrypted size; buttons Edit details, Export decrypted copy, Remove from vault in danger style).
5. **Show detail.** Backdrop band with title, years, season and episode counts, genre, overview, "Resume S2 E5", Edit details. Season tabs (segmented), "4 of 8 watched", "Mark season watched". Episode list: 192 px 16:9 thumbnail, "5. The Crossing", runtime, status line ("Up next · 24 min left" / "Watched" / "Not started"), watched badge.
6. **Player.** Full window, black. Top: round back button, show title and episode line. Subtitles centered near the bottom with a translucent box. Bottom: time elapsed, seek bar with chapter ticks and thumb, remaining time; then back 10 s, play/pause (large, light), forward 10 s, volume; at the end: audio track button, subtitles button (shows current language, accent when menu open), speed, Next episode, fullscreen. Subtitle menu popover above the controls at the end side: radio list of tracks (Off, עברית · Hebrew · SRT, English · embedded, English SDH · embedded), Timing − value + , Size slider, "Fix RTL punctuation" checkbox.
7. **Search.** Large search input with accent border while focused. Results grouped under "Shows · n", "Movies · n", "Episodes · n" headings, each row with artwork, title (matched text highlighted in accent) and a meta line.
8. **Add media.** Title and a storage meter ("61.2 of 119 GB", stacked bar: used + queued, "Queue adds 21.4 GB · 36.4 GB left after"). Dashed drop zone: upload icon, "Drop movies, episodes, subtitles or whole folders here", "Files are encrypted as they're copied. Your originals stay where they are.", Browse files / Browse folder, checkbox "Look up posters and details online (TMDB)". Queue list with Pause and Cancel all: each row shows file name + size, detected title, status (dot + text: Matched / Needs review / Attached), and progress ("Encrypting · 64% · about 1 min left", "Waiting", a Review match button, "Converted from Windows-1255 to UTF-8").
9. **Settings.** Side menu (Playback, Subtitles, Interface, Security, Vault, About) that stacks above content on narrow windows. Security page cards: Password (Change password), Recovery key (created date, Replace recovery key), Locking (idle auto-lock select: 5 min / 15 min / 1 h / Never; "Lock when the stick is removed" always on and disabled; "Lock when the computer sleeps"), Privacy ("Look up details online" with an explanation). Other pages follow section 7.3 and these: Playback (hardware decoding, default audio/subtitle language order, autoplay next episode), Interface (language, theme), Vault (space used, integrity check with progress, Update player apps on this stick), About (version, licenses, TMDB attribution).
10. **Create vault wizard.** Shown when the app runs from anywhere that isn't a vault. Header "Create a Cinasafe vault" and a step pill row: Drive · Prepare · Password · Recovery key · Finish (done steps show a check, current is accent).
    - *Drive:* radio cards per removable drive (name and size; file system, free space, drive letter or mount point; status "Ready" or "Needs formatting"). Refresh list. Cancel / Next.
    - *Prepare:* "Use as is · recommended" (adds a hidden vault folder and the player apps; existing files stay; no admin needed) or "Erase and format to exFAT" (danger border; type the drive's name to confirm; button "Erase and continue" stays disabled until it matches). FAT32/NTFS drives only get the format option, with an explanation.
    - *Password:* field with four-segment strength meter, confirm field with match status, warning note: "There's no 'forgot password'. If you lose both this password and your recovery key, nobody can open this vault."
    - *Recovery key:* show 24 words in a numbered grid once, Copy and Print buttons, checkbox "I saved my recovery key" or an explicit "Skip" with a confirm dialog.
    - *Finish:* progress while writing `vault.json`, the empty library and copying the player apps; then "Add media now" or "Eject".

### 9.4 Lock behaviour

The Lock button, idle timeout, computer sleep and removing the stick all: stop playback, destroy the mpv instance, drop every `BlobReader`, zeroize keys and caches, clear UI state, and return to Unlock.

### 9.5 Artwork in the UI

Artwork reaches the webview through a custom scheme `cinasafe-img://<artwork-id>` registered with Tauri's asynchronous URI scheme protocol. Responses decrypt in memory and send `Cache-Control: no-store`. The webview never gets file paths.

---

## 10. Security rules and threat model

| Threat | Protected? | How |
| --- | --- | --- |
| Stick lost, stolen or browsed | Yes | All content and metadata encrypted; Argon2id password stretching |
| Tampered or corrupted files | Detected | Every chunk authenticated; playback stops on failure; integrity check reports it |
| Traces left on a shared computer | Mostly | No plaintext files, RAM-only caches, no OS recent-files entries, webview data in a temp dir wiped on exit and next launch |
| Keylogger, malware or memory dump on the host | No | Out of scope; stated in README |
| Someone seeing the screen | No | Lock button, idle lock |
| Size and file-count analysis | Partly | Random blob names and size padding; totals still visible |

Implementation rules:

- Point WebView2's user data folder and WebKitGTK's data and cache dirs at a fresh temp dir; delete it on exit and on next launch.
- Disable webview disk caching, spell-check dictionaries and form autofill.
- No crash dumps or logs containing titles, file names or paths inside the vault. Logs default to off; a debug flag writes redacted logs only.
- On Windows, never call APIs that add Recent Items or Jump List entries.
- `Zeroizing` for passwords from the moment they cross the IPC boundary.
- Write `docs/THREAT_MODEL.md` from this table before Milestone 6.

---

## 11. Tauri IPC surface

**Commands** (all return `Result<T, AppError>`; errors carry a code and a localizable message key):

| Command | Purpose |
| --- | --- |
| `drives_list()` | Removable drives for the wizard |
| `vault_detect()` | Is the app running from a vault? Returns drive info or `null` |
| `vault_create(drive, mode, password, with_recovery)` | Mode = `use_as_is` or `format`; returns recovery words once |
| `vault_unlock(password)` / `vault_unlock_recovery(words)` | Unlock |
| `vault_lock()` | Lock immediately |
| `vault_change_password(old, new)` / `vault_replace_recovery(password)` | Key management |
| `vault_integrity_check()` | Starts a check; progress via events |
| `library_home()` | Hero, continue watching, up next, recently added |
| `library_movies(filter, genre, sort)` / `library_shows(...)` | Grids |
| `library_movie(id)` / `library_show(id)` / `library_season(id)` | Detail pages |
| `library_update_metadata(kind, id, patch)` | Manual edits |
| `library_mark_watched(media_file_id, watched)` | Watched state |
| `library_remove(kind, id)` / `library_export(media_file_id, dest)` | Remove, export decrypted copy (asks where) |
| `search(query)` | Grouped results |
| `import_add(paths)` / `import_pause()` / `import_resume()` / `import_cancel(id?)` | Queue |
| `import_resolve(item_id, match)` | Pick a TMDB match or manual details |
| `tmdb_search(kind, query, year?)` | Candidates for review |
| `player_open(media_file_id)` / `player_command(cmd)` / `player_close()` | Playback (`cmd`: play, pause, seek, volume, set_audio, set_sub, sub_delay, sub_size, speed, next) |
| `settings_get()` / `settings_set(patch)` | Settings, stored encrypted in the library |

**Events:** `vault-locked {reason}`, `drive-removed`, `import-progress {...}`, `integrity-progress {...}`, `player-state {position, duration, paused, tracks, ...}`, `player-ended {next_media_file_id?}`, `read-speed-warning`.

---

## 12. Packaging, CI and licensing

- **License:** Apache-2.0. Add `LICENSE`, SPDX headers, and `THIRD_PARTY_LICENSES.md` generated from Cargo and npm dependencies plus mpv, FFmpeg, libass, HarfBuzz, FriBidi and the fonts.
- **CI (GitHub Actions):** matrix for windows-x64, windows-arm64, linux-x86_64, linux-aarch64. Build libmpv from source with FFmpeg, libass, HarfBuzz and FriBidi; build the Tauri app; produce the exe and AppImages; run tests; upload artifacts.
- **Releases:** tag-triggered; attach artifacts, SHA-256 checksums and signatures (minisign or Sigstore). Aim for reproducible builds and document the steps.
- **Windows signing:** apply to a free open-source signing programme (such as SignPath Foundation) to avoid SmartScreen warnings; until then, document the warning in README.
- **Size budget:** keep each platform build small enough that all of them fit comfortably on any stick; measure and record in Milestone 5.
- **Repo hygiene:** CONTRIBUTING.md, CODE_OF_CONDUCT.md, DCO sign-off, issue and PR templates, Weblate config for `app/ui/src/i18n`.
- **README.txt on the stick:** what Cinasafe is, which file to run on each OS, and that it's for media the user owns.

---

## 13. Testing

- **Unit:** KDF and wrap/unwrap; blob encrypt/decrypt; random-offset seeks; truncation, reorder, header-tamper and wrong-key detection; zero-length and exact-multiple-of-chunk files; library serialize/restore; `.bak` fallback.
- **Test vectors:** fixed keys, nonces and plaintexts for `vault.json` and three blobs (empty, 1 chunk, 3 chunks with short last chunk) in `tests/vectors/`, documented in `docs/FORMAT.md`.
- **Property tests (`proptest`):** any (size, chunk size, read offset, read length) returns exactly the original bytes.
- **Integration:** CLI round trip (`create → add → ls → cat | sha256` equals original); import of a sample folder tree with sidecar subtitles; simulated drive removal mid-playback locks within 1 s.
- **Disk-trace test:** run a full session (unlock, import, play 30 s, lock) under a watcher (Linux: `fanotify`/`inotifywait` on `/` excluding the stick; Windows: Process Monitor capture in CI if feasible) and fail if any file written outside the stick and the temp webview dir contains plaintext markers from the test media.
- **RTL visual checks:** screenshots of every screen in `he` and `ar`, compared to baselines.
- **Playback:** sample files covering H.264, HEVC 10-bit, AV1, MKV with PGS, MP4 with mov_text, external Hebrew SRT in Windows-1255.

---

## 14. Milestones and acceptance criteria

**M0 · Spike (go/no-go).** Encrypt one video with a minimal blob writer; play it through `cinasafe://` in libmpv embedded in a Tauri 2 window with an HTML overlay button that pauses it.
- Works on Windows 11, Ubuntu (Wayland) and Ubuntu (X11), or a documented fallback per section 6.3 works.
- Seeking to 10 random points in a 4K file each completes in under 1 s from a USB 3 stick.
- `docs/SPIKE.md` records approach, results and decisions.

**M1 · Core and CLI.** Section 4 and 5 in full, plus `cinasafe` CLI: `create`, `unlock` (checks only), `add <file>`, `ls`, `cat <id>`, `export <id> <dest>`, `passwd`, `recovery`, `verify`.
- All unit, property and vector tests pass on all CI targets.
- `docs/FORMAT.md` complete.

**M2 · Library and import.** Schema, FTS search, file-name parser, sidecar subtitles, charset conversion, RTL fix, TMDB client, artwork resize, import queue with pause/resume/cancel and free-space check.
- A folder of 20 mixed movies and episodes imports with correct grouping; mismatches land in Needs review.

**M3 · UI.** All screens in section 9 with real data, i18n in en/he/ar, RTL mirroring, keyboard navigation, focus states, lock behaviour.
- Every screen matches section 9.3 in English and Hebrew; no hard-coded left/right in CSS.

**M4 · Player.** Full controls, subtitle menu, audio tracks, chapters, resume, next-episode countdown, read-speed warning, all mpv options from 6.2.
- Hebrew SRT, Arabic ASS and an MKV with PGS all render correctly.

**M5 · Wizard and packaging.** Create-vault wizard including formatting, "Update player apps" in Settings, run-linux.sh, CI release pipeline, checksums and signatures.
- A fresh stick goes from blank to playing a movie on a second machine (Windows → Linux and back) with no install.

**M6 · Hardening and 1.0.** Disk-trace test in CI, threat model doc, external security review of the format and core, fixes, public beta, 1.0.

---

## 15. Open questions (decide or ask before the relevant milestone)

- [ ] Check "Cinasafe" is free on GitHub, Flathub and as a domain.
- [ ] In Hebrew/Arabic UI, should the player seek bar run right-to-left or stay left-to-right? (Default: left-to-right.)
- [ ] TMDB lookup on or off by default? Project API key or user-supplied key?
- [ ] Support FAT32 sticks by splitting files, or always require exFAT? (Default: require exFAT.)
- [ ] Ship native Wayland in v1, or XWayland fallback if M0 runs long?
- [ ] Light theme in v1 or later?

---

## 16. Reference: approved mockups

The approved mockups live on a private Cinasafe design canvas owned by the project lead (13 screens: Unlock, Home, Home in Hebrew, Movies grid, Movie detail, Show detail, Search, Player with subtitle menu open, Add media, Settings › Security, and three wizard steps). Section 9 transcribes them fully; when in doubt, follow section 9 and ask the project lead for a screenshot of the specific screen.
