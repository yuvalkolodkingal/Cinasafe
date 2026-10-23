# Translating Cinasafe

The interface ships in **English**, **Hebrew** and **Arabic** (brief 7.4).
Every string lives in one JSON file per language:

```
app/ui/src/i18n/en.json   source language (edit this first)
app/ui/src/i18n/he.json
app/ui/src/i18n/ar.json
```

The files use the [i18next JSON v4](https://www.i18next.com/misc/json-format)
format: nested keys, `{{variables}}`, and plural forms written as suffixed keys.
The Hebrew and Arabic strings still need review by native speakers.

## Rules

- **Same keys everywhere.** Every key in `en.json` exists in each translation
  and nothing else does. `npm test` (in `app/ui`) checks this.
- **Keep `{{variables}}` exactly.** Translate around them, never inside them,
  and keep every variable the English string uses. Also checked by `npm test`.
- **Plural forms follow CLDR, per language.** English needs `_one` and `_other`.
  Hebrew uses `_one`, `_two` and `_other`. Arabic needs all six: `_zero`,
  `_one`, `_two`, `_few`, `_many`, `_other`. The test folds the suffixes, so
  each language carries exactly the forms its rules need.
- **Right-to-left.** Write the text naturally, as it should read. Don't add
  direction marks (RLM, LRM) to fix ordering; the UI isolates titles, sizes and
  paths itself. If something still comes out in the wrong order, open an
  issue with a screenshot instead of working around it in the string.
- **Keep it short.** Buttons and menu items have limited room, so use the
  shortest natural wording. Check your change in the app or in the mock
  (`npm run dev`, then add `?lang=he` or `?lang=ar` to the URL).
- **Terms.** Use one word for "vault", "stick", "recovery key", "subtitles"
  and "lock" throughout. Keep "Cinasafe", "TMDB", "TVmaze" and "mpv"
  untranslated.

After changing strings, run `npm test` and `npm run e2e` in `app/ui`. If the
screenshot tests fail only because the text changed, update the baselines with
`npm run e2e:update` and look at the new images before committing.

## Adding a language

1. Copy `en.json` to `<code>.json` (ISO 639-1, e.g. `fr.json`) and translate it.
2. Register it in `app/ui/src/i18n/index.ts`: import the file, add it to
   `LANGUAGES` with its native name, add it to the resources, and add the code
   to `RTL` if it is written right to left.
3. Add the code to the `Language` type in `app/ui/src/api/types.ts`, and to the
   translation tests and screenshot tests.
4. Make sure the bundled fonts cover its script. Rubik covers Latin and Hebrew,
   and Noto Sans Arabic covers Arabic. Other scripts need a font added under
   `app/ui` from `@fontsource`, and a subtitle fallback font in
   `app/src-tauri/fonts`.

## Weblate

The files are laid out for [Weblate](https://weblate.org). Weblate takes its
component settings from its own interface or API, not from a file in this
repository. Set the component up like this:

| Setting | Value |
| --- | --- |
| File format | **i18next JSON file v4** (API id `i18nextv4`) |
| File mask | `app/ui/src/i18n/*.json` |
| Monolingual base language file | `app/ui/src/i18n/en.json` |
| Edit base file | Off (English is edited in the repository) |
| Template for new translations | `app/ui/src/i18n/en.json` |
| Language filter | `^(?!en$).*` |
| Repository branch | `main` |
| Push branch | `weblate` (Weblate pushes there and a maintainer opens the pull request) |
| Add-ons | "Squash Git commits" (per language), "Customize JSON output" (indent 2, keep key order) |

Weblate's own checks catch broken `{{variables}}` and missing plural forms
too. The repository's tests remain the final gate: a Weblate pull request has
to pass CI like any other.

Translations come in under the project's licence (Apache-2.0); translators
sign off their commits like other contributors (see
[`CONTRIBUTING.md`](../CONTRIBUTING.md)). Weblate can add the `Signed-off-by` line.
