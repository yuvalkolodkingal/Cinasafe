//! Guessing what a media file is from its path.
//!
//! The heavy lifting is `hunch` (a Rust port of guessit). Around it sit rules
//! it lacks: Hebrew and Arabic season/episode words, `Season N` folders whose
//! files carry only an episode number, and a guard against reading a number in
//! a movie title as an episode (`Blade.Runner.2049.2017` is not S20E49).

use std::path::{Component, Path};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MediaGuess {
    Movie {
        title: String,
        year: Option<u16>,
    },
    Episode {
        show: String,
        year: Option<u16>,
        /// `None` for absolute numbering (common with anime) or dated episodes.
        season: Option<u32>,
        /// Usually one; multi-episode files list each (`S01E01-E02`).
        episodes: Vec<u32>,
        /// `YYYY-MM-DD` for daily shows.
        air_date: Option<String>,
        episode_title: Option<String>,
    },
}

impl MediaGuess {
    pub fn title(&self) -> &str {
        match self {
            MediaGuess::Movie { title, .. } => title,
            MediaGuess::Episode { show, .. } => show,
        }
    }
}

static HEBREW_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<show>.*?)[\s._\-]*(?:עונה|ע['׳]?)\s*(?P<s>\d{1,2})[\s._\-,]*(?:פרק|פ['׳]?)\s*(?P<e>\d{1,4})")
        .unwrap()
});
static ARABIC_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<show>.*?)[\s._\-]*(?:ال)?موسم\s*(?P<s>\d{1,2})[\s._\-,]*(?:ال)?حلقة\s*(?P<e>\d{1,4})").unwrap()
});
/// A file name that only says which episode it is (`05`, `E05`, `Episode 5`, `פרק 5`).
static EPISODE_ONLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:(?:ep(?:isode)?|e|פרק|الحلقة|حلقة)[\s._\-]*)?(?P<e>\d{1,3})(?:$|[\s._\-])").unwrap()
});
static SEASON_DIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:season|series|saison|staffel|temporada|stagione|עונה|الموسم|موسم|s)[\s._\-]*(?P<s>\d{1,2})$")
        .unwrap()
});
/// Evidence that a name really denotes an episode.
static EPISODE_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(?:\bs\d{1,3}[\s._\-]?e\d{1,4}",
        r"|\b\d{1,2}x\d{2,3}\b",
        r"|\b(?:season|episode|ep)\b",
        r"|^\[[^\]]+\].*\s-\s\d{1,4}\b",
        r"|\b(?:19|20)\d\d[.\-](?:0[1-9]|1[0-2])[.\-](?:[0-2]\d|3[01])\b",
        r"|\bs\d{1,2}\b)"
    ))
    .unwrap()
});
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:19|20)\d\d$").unwrap());
static PAREN_YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[\(\[]((?:19|20)\d\d)[\)\]]\s*$").unwrap());

/// Lowercase, alphanumerics only, single spaces. Used to compare titles.
pub fn normalize_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim_end().to_owned()
}

fn clean(s: &str) -> String {
    let replaced: String = s.chars().map(|c| if matches!(c, '.' | '_') { ' ' } else { c }).collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_matches(|c: char| c == '-' || c == ' ' || c == ',').to_owned()
}

/// Splits `Show Name (2015)` into (`Show Name`, 2015).
fn split_paren_year(s: &str) -> (String, Option<u16>) {
    match PAREN_YEAR.captures(s) {
        Some(c) => (clean(&s[..c.get(0).unwrap().start()]), c[1].parse().ok()),
        None => (clean(s), None),
    }
}

fn dirs_of(rel: &Path) -> Vec<String> {
    let parent = rel.parent().unwrap_or(Path::new(""));
    parent
        .components()
        .filter_map(|c| matches!(c, Component::Normal(_)).then(|| c.as_os_str().to_string_lossy().into_owned()))
        .collect()
}

fn season_dir(name: &str) -> Option<u32> {
    SEASON_DIR.captures(name.trim()).and_then(|c| c["s"].parse().ok())
}

/// Nearest folder above the file that names the show (skipping season folders).
fn show_from_dirs(dirs: &[String]) -> Option<(String, Option<u16>)> {
    dirs.iter().rev().find(|d| season_dir(d).is_none()).map(|d| split_paren_year(d))
}

fn hunch_episodes(r: &hunch::HunchResult) -> Vec<u32> {
    match r.to_flat_map().get("episode") {
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|e| vec![e as u32]).unwrap_or_default(),
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_u64().map(|e| e as u32)).collect(),
        _ => Vec::new(),
    }
}

/// Movie title and year from a release name when hunch's split is wrong:
/// the year is the last plausible year token that isn't the first token.
fn movie_from_stem(stem: &str) -> (String, Option<u16>) {
    let cleaned = clean(stem);
    let tokens: Vec<&str> = cleaned.split(' ').collect();
    let year_at = (1..tokens.len()).rev().find(|&i| YEAR.is_match(tokens[i].trim_matches(|c| c == '(' || c == ')')));
    match year_at {
        Some(i) => (
            tokens[..i].join(" ").trim_end_matches([' ', '(', '[', '-']).to_owned(),
            tokens[i].trim_matches(|c| c == '(' || c == ')').parse().ok(),
        ),
        None => (cleaned, None),
    }
}

/// Guesses what `rel` (a path relative to the scanned folder) contains.
pub fn guess(rel: &Path) -> MediaGuess {
    let stem = rel.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dirs = dirs_of(rel);
    let dir_season = dirs.last().and_then(|d| season_dir(d));

    // 1. Hebrew / Arabic season + episode words.
    for re in [&*HEBREW_EPISODE, &*ARABIC_EPISODE] {
        if let Some(c) = re.captures(&stem) {
            let (mut show, mut year) = split_paren_year(&c["show"]);
            if show.is_empty() {
                (show, year) = show_from_dirs(&dirs).unwrap_or_default();
            }
            return MediaGuess::Episode {
                show,
                year,
                season: c["s"].parse().ok(),
                episodes: vec![c["e"].parse().unwrap_or(0)],
                air_date: None,
                episode_title: None,
            };
        }
    }

    // 2. hunch on the whole relative path, so folder names give context.
    let path_str = rel.to_string_lossy().replace('\\', "/");
    let r = hunch::hunch(&path_str);
    let marker_text = format!("{} {}", dirs.join(" "), stem).trim().to_owned();
    let marker_text = marker_text.as_str();
    let has_marker = EPISODE_MARKER.is_match(stem.trim())
        || EPISODE_MARKER.is_match(marker_text)
        || dir_season.is_some()
        || HEBREW_OR_ARABIC_WORD.is_match(&stem);

    if r.is_episode() && has_marker {
        let mut show = r.title().map(clean).unwrap_or_default();
        let mut year = r.year().and_then(|y| u16::try_from(y).ok());
        if show.is_empty() || season_dir(&show).is_some() {
            if let Some((s, y)) = show_from_dirs(&dirs) {
                show = s;
                year = year.or(y);
            }
        }
        let air_date = r.date().map(str::to_owned);
        let mut episodes = hunch_episodes(&r);
        if air_date.is_some() && r.season().is_none() {
            // Daily shows: hunch also reports the day as an "episode".
            episodes.clear();
        } else if episodes.is_empty() {
            // `Season 2/05.mkv`: hunch sees the season folder but not the bare number.
            if let Some(c) = EPISODE_ONLY.captures(&clean(&stem)) {
                episodes.push(c["e"].parse().unwrap_or(0));
            }
        }
        return MediaGuess::Episode {
            show,
            year,
            season: r.season().and_then(|s| u32::try_from(s).ok()).or(dir_season),
            episodes,
            air_date,
            episode_title: r.episode_title().map(clean),
        };
    }

    // 3. Season folder with files named only by episode number.
    if let Some(season) = dir_season {
        if let Some(c) = EPISODE_ONLY.captures(&clean(&stem)) {
            if let Some((show, year)) = show_from_dirs(&dirs) {
                return MediaGuess::Episode {
                    show,
                    year,
                    season: Some(season),
                    episodes: vec![c["e"].parse().unwrap_or(0)],
                    air_date: None,
                    episode_title: None,
                };
            }
        }
    }

    // 4. Movie. Prefer hunch's split unless it mistook part of the title for an episode.
    let (title, year) = if r.is_episode() || r.title().is_none() {
        movie_from_stem(&stem)
    } else {
        (clean(r.title().unwrap_or_default()), r.year().and_then(|y| u16::try_from(y).ok()))
    };
    // A movie in its own `Title (Year)` folder: the folder is often the cleaner name.
    if year.is_none() {
        if let Some((t, Some(y))) = dirs.last().map(|d| split_paren_year(d)) {
            if normalize_title(&t) == normalize_title(&title) || title.is_empty() {
                return MediaGuess::Movie { title: t, year: Some(y) };
            }
        }
    }
    MediaGuess::Movie { title, year }
}

static HEBREW_OR_ARABIC_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:פרק|حلقة)\s*\d").unwrap());

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(t: &str, y: Option<u16>) -> MediaGuess {
        MediaGuess::Movie { title: t.into(), year: y }
    }

    fn ep(show: &str, year: Option<u16>, season: Option<u32>, episodes: &[u32]) -> MediaGuess {
        MediaGuess::Episode {
            show: show.into(),
            year,
            season,
            episodes: episodes.to_vec(),
            air_date: None,
            episode_title: None,
        }
    }

    fn g(p: &str) -> MediaGuess {
        let mut r = guess(Path::new(p));
        if let MediaGuess::Episode { episode_title, .. } = &mut r {
            *episode_title = None; // asserted separately where it matters
        }
        r
    }

    #[test]
    fn movies() {
        assert_eq!(g("Inception (2010) 1080p BluRay.mkv"), movie("Inception", Some(2010)));
        assert_eq!(g("2001.A.Space.Odyssey.1968.mkv"), movie("2001 A Space Odyssey", Some(1968)));
        assert_eq!(
            g("Blade.Runner.2049.2017.2160p.UHD.BluRay.x265-TERMiNAL.mkv"),
            movie("Blade Runner 2049", Some(2017))
        );
        assert_eq!(g("Movies/Waltz with Bashir (2008)/movie.mkv"), movie("Waltz with Bashir", Some(2008)));
        assert_eq!(g("ואלס עם באשיר 2008.mkv"), movie("ואלס עם באשיר", Some(2008)));
    }

    #[test]
    fn episodes() {
        assert_eq!(
            g("The.Walking.Dead.S05E03.720p.BluRay.x264-DEMAND.mkv"),
            ep("The Walking Dead", None, Some(5), &[3])
        );
        assert_eq!(g("Breaking Bad - 2x05 - Breakage.avi"), ep("Breaking Bad", None, Some(2), &[5]));
        assert_eq!(g("Fauda.S04E01.HEBREW.1080p.WEB.h264-RLSGRP.mkv"), ep("Fauda", None, Some(4), &[1]));
        assert_eq!(g("Doctor.Who.2005.S10E01.mkv"), ep("Doctor Who", Some(2005), Some(10), &[1]));
        assert_eq!(g("Friends.S01E01-E02.mkv"), ep("Friends", None, Some(1), &[1, 2]));
        assert_eq!(g("Shtisel/Season 1/Shtisel - S01E05.mkv"), ep("Shtisel", None, Some(1), &[5]));
        assert_eq!(g("[SubsPlease] Frieren - 12 (1080p) [ABCD1234].mkv"), ep("Frieren", None, None, &[12]));
    }

    #[test]
    fn episodes_from_folders() {
        assert_eq!(g("Shtisel (2013)/Season 2/05.mkv"), ep("Shtisel", Some(2013), Some(2), &[5]));
        assert_eq!(g("Shtisel/Season 02/Episode 7.mkv"), ep("Shtisel", None, Some(2), &[7]));
        assert_eq!(g("Fauda/עונה 3/פרק 4.mkv"), ep("Fauda", None, Some(3), &[4]));
    }

    #[test]
    fn hebrew_and_arabic_words() {
        assert_eq!(g("פאודה עונה 2 פרק 3.mkv"), ep("פאודה", None, Some(2), &[3]));
        assert_eq!(g("שטיסל ע2 פ10.mkv"), ep("שטיסל", None, Some(2), &[10]));
        assert_eq!(g("باب الحارة الموسم 1 الحلقة 12.mp4"), ep("باب الحارة", None, Some(1), &[12]));
        assert_eq!(g("פאודה/עונה 1/עונה 1 פרק 2.mkv"), ep("פאודה", None, Some(1), &[2]));
    }

    #[test]
    fn daily_show() {
        match guess(Path::new("The.Daily.Show.2024.03.14.mkv")) {
            MediaGuess::Episode { show, air_date, episodes, .. } => {
                assert_eq!(show, "The Daily Show");
                assert_eq!(air_date.as_deref(), Some("2024-03-14"));
                assert!(episodes.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize_title("The Office (US)"), "the office us");
        assert_eq!(normalize_title("  Shtisel: Season—One "), "shtisel season one");
        assert_eq!(normalize_title("פאודה!"), "פאודה");
    }
}
