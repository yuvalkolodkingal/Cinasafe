# Cinasafe

Cinasafe turns a USB stick into an encrypted, portable movie and TV library that runs straight
from the stick on Windows and Linux.

**Status: Milestone 0 (spike).** Nothing here is usable as an app yet. See
[`docs/SPIKE.md`](docs/SPIKE.md) for results and [`docs/OPEN_QUESTIONS.md`](docs/OPEN_QUESTIONS.md)
for open decisions.

| Path | What |
| --- | --- |
| `crates/cinasafe-core` | Encrypted blob format (`docs/FORMAT.md`) and key derivation, with test vectors |
| `crates/cinasafe-player` | libmpv wrapper (loaded at runtime) and the `cinasafe://` stream bridge |
| `crates/cinasafe-cli` | `cinasafe` CLI; currently spike commands only |
| `spike/tauri-embed` | Tauri 2 experiment: mpv video under a transparent web UI |
| `spike/run-spike.sh` | Reproduces the spike measurements |

Build and test (Linux needs `libmpv` at runtime; the Tauri spike needs WebKitGTK 4.1 headers):

```sh
cargo test
spike/run-spike.sh
```
