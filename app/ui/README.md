# Cinasafe UI

React + TypeScript + Vite. Built into `dist/` and served by the Tauri shell
in `app/src-tauri`. Brief sections 7.4, 9 and 11 describe what it shows.

## Run it without the app

```sh
npm ci
npm run dev        # http://localhost:1420, backed by the in-memory mock
```

Outside Tauri, every command goes to `src/api/mock.ts`, which has sample
movies and shows (Hebrew titles included), an import queue and a player
that ticks. URL switches: `?locked=1` (Unlock), `?wizard=1` (no vault:
create-vault wizard), `?empty=1` (empty library), `?tooNew=1`, `?slow=1`
(slow-stick notice), `?pos=<seconds>` (player start), `?lang=en|he|ar`,
`?freeze=1` (player and import queue stand still, for screenshot tests).
For example `http://localhost:1420/?lang=he#/show/1`.

## The contract

`src/api/types.ts` and `src/api/index.ts` are the IPC contract with the Rust
core: command names and argument names (camelCase in JS, snake_case in Rust;
Tauri maps them), payload shapes (serde `rename_all = "camelCase"`) and
events. Commands reject with `AppError { code, message }`; `code` is also the
i18n key `errors.<code>`. Artwork comes from the `cinasafe-img` protocol via
`imageUrl(id)`; the webview never sees paths or remote URLs.

## Checks

```sh
npm run typecheck
npm test           # i18n key parity, no physical left/right in CSS, formatting, mock covers the contract
npm run build
npm run e2e        # screenshots of every screen in he/ar (and some en) vs e2e/__screenshots__
```

`npm run e2e` builds, serves `dist` with `vite preview` and compares each
screen against its baseline, with the mock frozen (`?freeze=1`) and the clock
fixed. After an intended UI change, run `npm run e2e:update` and look at the
new images before committing them. `@playwright/test` is pinned to 1.56.1
because its Chromium (revision 1194) is the one preinstalled here; in CI,
`npx playwright install --with-deps chromium` fetches the same build.

## Rules

- Layout uses CSS logical properties only (`margin-inline-start`,
  `inset-inline-end`, `text-align: start`); `src/test/logical-css.test.ts`
  fails on `left`/`right`. RTL comes from `dir` on `<html>`.
- Titles, overviews and file names render inside `<bdi>` or `dir="auto"`;
  sizes and paths go through `isolate()` so they keep their order in Hebrew
  and Arabic text.
- Strings live in `src/i18n/{en,he,ar}.json` with the same keys (plural
  forms differ per language). Hebrew and Arabic need review by native
  speakers.
- Fonts are bundled from `@fontsource` packages; nothing loads from a CDN.
