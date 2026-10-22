# Recognising media and fetching metadata

Research and design notes for automatic recognition of movies and TV episodes, subtitle
matching and artwork (brief sections 5 and 7). Researched 2026-10-22. Terms change, so
re-check each provider's terms before a release.

## What happens on import

1. **Scan** (`cinasafe_core::import::scan`, offline): walk the chosen folder and classify
   video and subtitle files. Samples, trailers and `Extras/`-style folders are skipped.
2. **Recognise** (`import::parse::guess`, offline): file and folder names become a movie
   (title, year) or an episode (show, season, episode(s), or absolute number, or air date).
3. **Attach subtitles** (offline): each `.srt/.ass/.ssa/.vtt` is matched to one video. The
   charset becomes UTF-8, the language comes from the file name or the text itself, and the
   RTL punctuation fix is offered where it applies.
4. **Look up** (`metadata`, online, opt-in): search the provider, rank hits, fetch details,
   map episodes, download artwork into memory.
5. **Store** (later milestone): everything is encrypted into blobs and the library database.

Try steps 1–4 with the CLI:

```sh
cinasafe scan /path/to/folder                      # offline
cinasafe identify /path/to/folder --artwork        # TVmaze for shows, no key needed
CINASAFE_TMDB_KEY=... cinasafe identify /path --language he-IL --artwork
```

## Providers compared

| Service | Covers | Key / cost | Terms that matter | Use in Cinasafe |
| --- | --- | --- | --- | --- |
| **TMDB** | Movies and TV, seasons, episodes, posters, backdrops, episode stills; titles and overviews in Hebrew, Arabic and many other languages | Free API key for non-commercial use; commercial use needs a licence | Must show the TMDB logo and "This product uses the TMDB API but is not endorsed or certified by TMDB." in About/Credits; logo less prominent than ours [1] | **Primary**, the provider the brief names. Implemented (`metadata::tmdb`) |
| **TVmaze** | TV only: shows, seasons, episodes, posters, backgrounds, episode stills; English | No key for the public API | CC BY-SA 4.0: credit and link TVmaze. At least 20 calls / 10 s per IP; 429 when exceeded; image URLs are immutable [2] | **Fallback for TV with no TMDB key**, so shows work out of the box. Implemented (`metadata::tvmaze`) |
| TheTVDB | TV (strong on episode orderings, anime) | Needs a commercial licence or end-user subscriptions; per-user fee or revenue-tiered licence [3] | Fees and terms can change without notice | Not used: paid, and TMDB + TVmaze cover the need |
| OMDb | Movies and TV from IMDb data, little artwork | Free key, 1,000 requests/day; posters for patrons only [4] | — | Not used: low limit, no free artwork |
| fanart.tv | Extra artwork: logos, clearart, backgrounds | Personal API key per user [5] | — | Candidate for later "more artwork" option; needs the user's own key |
| OpenSubtitles.com | Subtitle search, including by file hash; can identify a file | API key; downloads limited per day and need login for more [6] | Descriptive User-Agent required | Not used in v1. Downloading subtitles is a separate feature; noted for later |
| Wikidata | IDs and titles in many languages, CC0 | Free, no key | Rate limits on the query service | Not needed now; could fill missing Hebrew/Arabic titles later |

Recognition libraries: **hunch** [7] (MIT, Rust port of guessit, ~49 properties) does the
release-name parsing. It doesn't know Hebrew/Arabic season words and occasionally reads
title numbers as episodes, so `import::parse` wraps it with extra rules (below).

## Decisions

- **TMDB primary, TVmaze fallback.** With a TMDB key everything (movies and shows) goes to
  TMDB in the UI language (`he-IL`, `ar-*`, `en-US`). Overviews TMDB lacks in that language
  are filled from English. Without a key, shows use TVmaze (English) and movies are not
  looked up. That keeps the app working out of the box and honest about what needs a key.
- **The key.** TMDB keys are free for non-commercial use, but TMDB's terms do not say whether
  a key may be shipped inside an open-source binary. **Conservative choice:** the user pastes
  their own key in Settings (or sets `CINASAFE_TMDB_KEY`). Shipping a project key in release
  builds (from a CI secret) is listed in `OPEN_QUESTIONS.md` (Q9).
- **Privacy.** Every lookup sends the parsed title to the provider from the user's IP. That
  makes looking titles up online the one deliberate leak in an otherwise offline app. It must
  be off until the user turns it on, and say so in the UI. Nothing fetched is cached on disk.
  Artwork is downloaded into memory and only ever written encrypted. Errors never include
  URLs (they contain titles and keys).
- **Confidence.** Hits are ranked by title similarity (against the localised and original
  title), year agreement and popularity. A match applies automatically only when it scores
  ≥ 0.75 *and* beats the runner-up by ≥ 0.08. Otherwise it becomes a suggestion the user
  confirms. Remakes ("Total Recall" 1990/2012) without a year are therefore never guessed.
- **Artwork safety.** Only `https://image.tmdb.org/` and `https://static.tvmaze.com/` URLs
  are fetched, at most 20 MB, and only bytes that start like JPEG/PNG/WebP are kept.

## Recognition rules added around hunch

| Case | Example | Result |
| --- | --- | --- |
| Hebrew season/episode words | `פאודה עונה 2 פרק 3.mkv`, `שטיסל ע2 פ10.mkv` | episode S02E03 / S02E10 |
| Arabic season/episode words | `باب الحارة الموسم 1 الحلقة 12.mp4` | episode S01E12 |
| Season folder, bare number | `Shtisel (2013)/Season 2/05.mkv`, `Fauda/עונה 3/פרק 4.mkv` | show from the folder, year from `(2013)` |
| Number in a movie title | `Blade.Runner.2049.2017.2160p…` | movie "Blade Runner 2049" (2017), not S20E49 |
| Episode needs evidence | hunch says "episode" but no SxxEyy / NxM / Season / date / anime marker | treated as a movie |
| Movie folder | `Waltz with Bashir (2008)/movie.mkv` | title and year from the folder |
| Show folder as alias | `Shtisel/עונה 2/שטיסל עונה 2 פרק 3.mkv` | grouped under "Shtisel"; lookups try "Shtisel" and "שטיסל" |
| Daily shows | `The.Daily.Show.2024.03.14.mkv` | air date 2024-03-14, mapped by date |
| Anime absolute numbers | `[SubsPlease] Frieren - 12 (1080p).mkv` | absolute episode 12, mapped across seasons |

## Subtitle matching

A subtitle attaches to exactly one video, by the strongest of:

1. its name starts with the video's name (`Movie.2010.1080p.mkv` ↔ `Movie.2010.1080p.he.srt`)
   or it sits in `Subs/<video name>/` (score 100);
2. both names give the same season and episode (80; *different* numbers never match);
3. same movie title (60);
4. the folder (or its `Subs/`) holds only one video (40).

Ties stay unmatched and are shown to the user rather than guessed. Language comes from name
tags (`he`, `heb`, `Hebrew`, `עברית`, `2_English`, …) plus `forced`/`SDH` flags. If the
text's script contradicts the name (Hebrew text in `.en.srt`), the text wins. Charset:
BOM → UTF-8 → `chardetng` (Windows-1255/1256, ISO-8859-8). Hebrew Windows-1255 files without
1255-only bytes are reported as ISO-8859-8; both decode Hebrew letters identically.

## Sources

1. TMDB developer FAQ (cost, commercial use, attribution): https://developer.themoviedb.org/docs/faq;
   rate limiting (~40 req/s soft limit, handle 429): https://developer.themoviedb.org/docs/rate-limiting
2. TVmaze API (licence, rate limit, endpoints): https://www.tvmaze.com/api
3. TheTVDB licensing FAQ: https://support.thetvdb.com/kb/faq.php?id=62; v4 API: https://github.com/thetvdb/v4-api
4. OMDb limits (secondary source): https://zuplo.com/blog/2024/10/03/best-movie-api-imdb-vs-omdb-vs-tmdb
5. fanart.tv keys: https://fanart.tv/get-an-api-key/ (page not readable from the build
   environment; key requirement confirmed via https://pypi.org/project/python-fanart/)
6. OpenSubtitles REST API (secondary sources; official docs at https://opensubtitles.stoplight.io/):
   https://apidog.com/de/blog/opensubtitles-api-4/
7. hunch: https://docs.rs/hunch, https://github.com/lijunzh/hunch
