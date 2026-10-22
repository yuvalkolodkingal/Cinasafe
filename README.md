# Cinasafe

Cinasafe turns a USB stick into an encrypted, portable movie and TV library that runs straight
from the stick on Windows and Linux.

**Status: early development.** The playback spike is done ([`docs/SPIKE.md`](docs/SPIKE.md)). The full build brief is [`docs/BRIEF.md`](docs/BRIEF.md).
Automatic recognition of movies, episodes and subtitles, plus online metadata and artwork,
works from the CLI ([`docs/METADATA.md`](docs/METADATA.md)). The app itself is a developer
preview. Open decisions: [`docs/OPEN_QUESTIONS.md`](docs/OPEN_QUESTIONS.md).

Builds: every push builds a Windows `.exe` (portable zip and installer), a `.deb` and an
`.rpm` in GitHub Actions ("Build packages" workflow, artifacts on each run). Pushing a `v*`
tag publishes them as a pre-release.

| Path | What |
| --- | --- |
| `crates/cinasafe-core` | Encrypted blob format (`docs/FORMAT.md`), media recognition (`import`), subtitles, metadata (TMDB, TVmaze) |
| `crates/cinasafe-player` | libmpv wrapper (loaded at runtime) and the `cinasafe://` stream bridge |
| `crates/cinasafe-cli` | `cinasafe` CLI: `scan` and `identify` a folder, plus spike commands |
| `spike/tauri-embed` | Tauri 2 experiment: mpv video under a transparent web UI |
| `spike/run-spike.sh` | Reproduces the spike measurements |

Build and test (Linux needs `libmpv` at runtime; the Tauri spike needs WebKitGTK 4.1 headers):

```sh
cargo test
spike/run-spike.sh
```

Recognising a folder (offline), then looking it up online:

```sh
cargo run --release -p cinasafe-cli -- scan ~/Videos
cargo run --release -p cinasafe-cli -- identify ~/Videos --artwork               # TV via TVmaze
CINASAFE_TMDB_KEY=... cargo run --release -p cinasafe-cli -- identify ~/Videos --language he-IL
```

Metadata attribution: this product uses the TMDB API but is not endorsed or certified by
TMDB. TV data also comes from [TVmaze](https://www.tvmaze.com/) (CC BY-SA 4.0).

## License

Cinasafe is licensed under the [Apache License 2.0](LICENSE). Third-party components keep
their own licences; libmpv in particular is LGPL or GPL depending on how it is built (see
[`third_party/mpv/README.md`](third_party/mpv/README.md)).
