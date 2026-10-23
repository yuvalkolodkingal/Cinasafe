# Threat model

What Cinasafe protects, from whom, and how. Brief section 10 is the source;
this file records what the code actually does and where the limits are.
The format itself is specified in [`FORMAT.md`](FORMAT.md).

## Assets

- **Media**: films, episodes and subtitles the user imported.
- **Library metadata**: titles, overviews, posters, what was watched and when.
- **Keys**: the password, the 24-word recovery key, and the master key derived from them.

## Threats

| Threat | Protected? | How |
| --- | --- | --- |
| Stick lost, stolen or browsed | Yes | Everything under `.cinasafe/` is AES-256-GCM encrypted: media, subtitles, artwork and the library database. The password goes through Argon2id (256 MiB, 3 passes, 4 lanes). Blob names are random. |
| Tampered or corrupted files | Detected | Every 1 MiB chunk is authenticated, with its index and a last-chunk flag in the AAD, so truncation, reordering, splicing between blobs and header edits all fail. Playback stops on a failed chunk; Settings › Vault › integrity check lists corrupt, missing and orphaned blobs. Each write is decrypted and compared before it counts. |
| Traces on a shared computer | Mostly | See [Host traces](#host-traces). |
| Keylogger, malware or a memory dump on the host | No | Out of scope. Anything running as the user while the vault is open can read what the user can see. Stated in the README. |
| Someone seeing the screen | No | The Lock button, the idle lock and lock-on-sleep limit the window. |
| Size and file-count analysis | Partly | Media blobs are padded to whole MiB and named randomly. The number of blobs and the total size stay visible. So does the vault's creation time in `vault.json`. |
| Forgotten password | By design, no recovery without the recovery key | The optional recovery key is a second wrap of the same master key. There's no back door. |

## What someone holding the stick learns

From `vault.json`: that it's a Cinasafe vault, its format version, the KDF
parameters and salt, whether a recovery key exists, and when the vault was
created. From the file system: how many blobs exist, their padded sizes, and
modification times. `.cinasafe/apps.json` lists which player app versions are
on the stick. Nothing reveals titles, subtitles, posters or watch history.

## Keys in memory

- The password crosses the IPC boundary as a string and becomes
  `Zeroizing<String>` at once (`app/src-tauri/src/commands.rs`). The webview's
  copy of what was typed is outside our control; the Unlock screen clears its field.
- The master key lives in an `mlock`ed, zeroize-on-drop buffer (`SecretKey`).
  Per-blob keys are derived on demand, held only by an open reader or
  writer, and zeroized when it closes.
- Decrypted chunks sit in a 16-chunk cache per open blob, zeroized on eviction.
- **Lock** (button, idle timeout, sleep, stick removed) stops playback and
  destroys mpv, cancels the import queue and deletes any partly written blob,
  drops every reader and the vault, which zeroizes the master key, and clears
  the queue's titles and the settings from memory. The UI drops its state and
  returns to Unlock.
- **Limits.** The decrypted SQLite library is a plain in-memory database.
  SQLite's page cache is freed on lock but not zeroized. mpv's demuxer cache
  (up to 256 MiB) and decoded frames are freed when mpv is destroyed, not
  zeroized. Text and images the webview rendered stay in its process memory
  until the app exits. Swap can hold any of this; `mlock` covers the master key only.

## Host traces

Rules (brief 10) and how each is met:

| Rule | Implementation |
| --- | --- |
| No plaintext files | Media streams to mpv through `cinasafe://` from memory. Subtitles are added the same way. Artwork reaches the webview through `cinasafe-img://` with `Cache-Control: no-store`. Imports read the originals and write only encrypted blobs. |
| RAM-only caches | mpv: `cache-on-disk=no`, `gpu-shader-cache=no`, `icc-cache=no`, no config, scripts, watch-later or log file (`SAFE_OPTIONS`). |
| Web engine data in a temp dir | Each run creates `$TMP/cinasafe-session-<random>/` and deletes it on exit. Leftovers from a crashed run are deleted at the next start. The WebView2 user-data folder (`data_directory`) lives there. On Linux, `XDG_CACHE_HOME` and `XDG_DATA_HOME` point there too, which covers WebKitGTK's salts, HSTS and cache, fontconfig's cache and GTK's recently-used list. The webview also runs in incognito mode. |
| GPU shader caches | `MESA_SHADER_CACHE_DISABLE=true` and `__GL_SHADER_DISK_CACHE=0` are set before any GL use. |
| No recent-files entries | Linux: GTK's list is redirected (above). Windows: the app never calls `SHAddToRecentDocs`, and file dialogs come from `tauri-plugin-dialog`. Whether the Windows common dialog records its last folder in the registry still has to be checked on Windows. |
| No logs with titles or paths | Nothing is logged by default. Errors sent to the UI carry a code and a message without vault content. Fatal-start messages on stderr name no vault content. |
| Exports are explicit | "Export decrypted copy" is the one path that writes plaintext, to a place the user picks in a save dialog. |

**Online metadata** is off by default. When on, the parsed title (from the
file name) and optional year go to TMDB or TVmaze over HTTPS. Artwork is
downloaded into memory, checked by magic bytes, resized and encrypted. The
webview never loads remote URLs (CSP: `img-src 'self' data: cinasafe-img:`).

## Still open

- External security review of the format and core (milestone 6). Needs people outside the project.
- Disk-trace test in CI (brief 13): run unlock, import, play and lock under
  `fanotify` and fail on any write outside the stick and the session dir.
- Windows: verify that WebView2, the file dialogs and the GPU driver leave
  nothing behind. This needs a real Windows 11 machine.
- Crash dumps: Windows Error Reporting may write a dump of the process, which
  could contain decrypted data. Opting out per process (`WerAddExcludedApplication`)
  needs testing.
