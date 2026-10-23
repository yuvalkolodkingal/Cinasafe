# Cinasafe

Cinasafe turns a USB stick into an encrypted, portable movie and TV library that runs straight
from the stick on Windows and Linux.

**Status: pre-release.** The vault format, the library and import queue, the desktop app
(every screen of brief section 9, in English, Hebrew and Arabic) and the embedded player work,
and every milestone of the brief is built. Still open: testing on real hardware, signed
Windows builds, ARM builds and an external security review. The full brief is [`docs/BRIEF.md`](docs/BRIEF.md); open decisions are
in [`docs/OPEN_QUESTIONS.md`](docs/OPEN_QUESTIONS.md); what is and isn't protected is in
[`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md).

Cinasafe does not protect against malware, keyloggers or memory dumps on the computer you
plug the stick into. It is for media you own.

Builds: every push builds a portable Windows `.exe` and Linux AppImage (each a single file,
libmpv compiled in, nothing to install), a Windows installer, a `.deb` and an `.rpm` in
GitHub Actions ("Build packages" workflow, artifacts on each run). Releases are published
from a `v*` tag or a manual run (`docs/RELEASING.md`). Windows builds are unsigned, so
SmartScreen warns ("More info", then "Run anyway").

| Path | What |
| --- | --- |
| `crates/cinasafe-core` | Vault and blob format (`docs/FORMAT.md`), library, import queue, recognition, subtitles, metadata (TMDB, TVmaze), drives |
| `crates/cinasafe-player` | libmpv wrapper (compiled in with `static-mpv`, else loaded at run time), the `cinasafe://` stream bridge, probing |
| `crates/cinasafe-cli` | `cinasafe` CLI: vault commands, `import`, `scan`, `identify`, spike benchmarks |
| `app/src-tauri` | The desktop app: Tauri 2 shell, IPC commands, player session, locking |
| `app/ui` | React + TypeScript UI ([`app/ui/README.md`](app/ui/README.md)) |
| `spike/` | Milestone 0 experiments ([`docs/SPIKE.md`](docs/SPIKE.md)) |

Build and test (development builds load the system's `libmpv` at run time; Linux needs
WebKitGTK 4.1 headers to build the app; release builds compile libmpv in, see
[`third_party/mpv/README.md`](third_party/mpv/README.md)):

```sh
(cd app/ui && npm ci && npm test && npm run build)
cargo test --workspace
cd app/src-tauri && npx @tauri-apps/cli@2.12.4 dev     # the app, against app/ui's dev server
```

A vault from the command line:

```sh
cinasafe create /media/me/STICK              # asks for a password, prints the recovery key
cinasafe import /media/me/STICK ~/Videos      # add --online to look titles up
cinasafe ls /media/me/STICK
cinasafe verify /media/me/STICK
```

Recognising a folder without importing it:

```sh
cargo run --release -p cinasafe-cli -- scan ~/Videos
cargo run --release -p cinasafe-cli -- identify ~/Videos --artwork               # TV via TVmaze
CINASAFE_TMDB_KEY=... cargo run --release -p cinasafe-cli -- identify ~/Videos --language he-IL
```

Metadata attribution: this product uses the TMDB API but is not endorsed or certified by
TMDB. TV data also comes from [TVmaze](https://www.tvmaze.com/) (CC BY-SA 4.0).

## License

Cinasafe is licensed under the [Apache License 2.0](LICENSE). Third-party components keep
their own licences ([`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md)). libmpv and FFmpeg
are built as LGPL and linked statically; the app's source being public lets anyone relink
it against modified versions (see [`third_party/mpv/README.md`](third_party/mpv/README.md)).
