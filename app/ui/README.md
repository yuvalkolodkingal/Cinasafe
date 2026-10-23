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
(slow-stick notice), `?pos=<seconds>` (player start), `?lang=en|he|ar`.
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
```

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
