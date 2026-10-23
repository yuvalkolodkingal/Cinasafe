# Contributing to Cinasafe

Thanks for helping. Cinasafe keeps people's media encrypted on a stick they
carry around, so correctness and the "nothing leaks to the host" rule matter
more than features. Read [`docs/BRIEF.md`](docs/BRIEF.md) for the design and
[`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) for what is and isn't protected.

## Building and testing

You need Rust (stable), Node 22, and on Linux the WebKitGTK 4.1 and GTK 3
development packages (`libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev`) plus
`libmpv` at runtime (`libmpv2` on Debian/Ubuntu, `mpv-libs` on Fedora).

The app embeds the built UI, so build the UI first:

```sh
cd app/ui
npm ci
npm run typecheck
npm test            # translations, no physical left/right CSS, no autofill, the IPC contract
npm run build
npm run e2e         # screenshot tests of every screen in he/ar (and some en)
cd ../..
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run the app against the UI's dev server with
`cd app/src-tauri && npx @tauri-apps/cli@2.12.4 dev`. The UI alone runs in a
browser with an in-memory mock: `cd app/ui && npm run dev`
(see [`app/ui/README.md`](app/ui/README.md) for its URL switches).

If you change a screen on purpose, regenerate its baselines with
`npm run e2e:update`, look at the new images under `app/ui/e2e/__screenshots__`,
and commit them with the change.

## How the pieces fit

- `crates/cinasafe-core`: the vault and blob format ([`docs/FORMAT.md`](docs/FORMAT.md)),
  the library, import queue, recognition, subtitles and metadata.
- `crates/cinasafe-player`: libmpv (loaded at runtime) and the `cinasafe://`
  stream bridge that feeds it decrypted bytes from memory.
- `app/src-tauri`: the Tauri shell. Its commands and events are the **IPC
  contract**, written down in [`app/ui/src/api/types.ts`](app/ui/src/api/types.ts)
  and [`app/ui/src/api/index.ts`](app/ui/src/api/index.ts). Change both sides
  together, and keep the mock (`app/ui/src/api/mock.ts`) in step; a test checks
  that the mock answers every command.
- `app/ui`: React + TypeScript.

## Rules

These are release-blocking, not style preferences:

- **No hand-rolled cryptography.** Use only the crates listed in brief section 5.1.
  Any change to the on-disk format updates `docs/FORMAT.md` and the test
  vectors, and bumps the format version.
- **Never write decrypted data to the host.** No temp files, no caches on
  disk, no logs containing titles, file names or paths from the vault. Media,
  subtitles and artwork stay in memory. If a library you add might write a
  cache, prove it doesn't (or turn it off) in the PR.
- **Logical CSS only** (`margin-inline-start`, `inset-inline-end`,
  `text-align: start`). Never `left`/`right`; a test enforces it. Titles and
  file names render with `dir="auto"` or `<bdi>`.
- **Text fields** set `spellCheck={false}` and `autoComplete="off"` (also enforced).
- **Every user-visible string** goes through i18next, with the key added to
  `en.json`, `he.json` and `ar.json` (see [`docs/TRANSLATING.md`](docs/TRANSLATING.md)).
- Keep commits small and focused, with messages that say why. Put
  decisions that the brief leaves open in [`docs/OPEN_QUESTIONS.md`](docs/OPEN_QUESTIONS.md),
  taking the most conservative option until someone decides.

## Sign your work (DCO)

Every commit must carry a `Signed-off-by` line, certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/):
that you wrote the change, or otherwise have the right to submit it under the
project's licence (Apache-2.0). `git commit -s` adds the line from your Git
name and email:

```
Signed-off-by: Your Name <you@example.org>
```

To sign commits you already made on your branch: `git rebase --signoff main`.

## Translations

The UI ships in English, Hebrew and Arabic. To fix or add a translation, see
[`docs/TRANSLATING.md`](docs/TRANSLATING.md).

## Security issues

Don't open public issues for vulnerabilities. See [`SECURITY.md`](SECURITY.md).

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
