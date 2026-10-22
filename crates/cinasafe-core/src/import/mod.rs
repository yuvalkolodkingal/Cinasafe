//! Recognising media on import: what each file is and which subtitles belong to it.
//!
//! [`scan`] walks a folder the user picked, guesses movie vs episode for every
//! video, groups episodes into shows, and attaches external subtitles to the
//! video they belong to. It reads subtitle files into memory to detect their
//! charset and language; it writes nothing.

pub mod lang;
pub mod parse;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::subtitles::{self, SubFormat};
pub use parse::{guess, normalize_title, MediaGuess};

pub const VIDEO_EXTENSIONS: &[&str] =
    &["mkv", "mp4", "m4v", "avi", "mov", "wmv", "webm", "ts", "m2ts", "mts", "mpg", "mpeg", "flv", "ogv", "3gp"];
/// Larger "subtitle" files are not subtitles; they are left out unread.
const MAX_SUBTITLE_BYTES: u64 = 10 << 20;
/// Files called "sample" below this size are release samples, not the feature.
const SAMPLE_MAX_BYTES: u64 = 300 << 20;
const EXTRAS_DIRS: &[&str] = &[
    "extras",
    "featurettes",
    "behind the scenes",
    "deleted scenes",
    "trailers",
    "interviews",
    "shorts",
    "sample",
    "samples",
    "bonus",
];
const SUBTITLE_DIRS: &[&str] = &["subs", "sub", "subtitles", "כתוביות", "ترجمة"];

#[derive(Clone, Debug, Serialize)]
pub struct VideoFile {
    /// Relative to the scanned folder.
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageSource {
    FileName,
    Content,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct SubtitleFile {
    pub path: PathBuf,
    pub format: SubFormat,
    /// ISO 639-1.
    pub language: Option<&'static str>,
    pub language_from: LanguageSource,
    pub forced: bool,
    pub sdh: bool,
    /// Charset the file was in; it is converted to UTF-8 on import.
    pub source_encoding: &'static str,
    /// Offer "Fix RTL punctuation" for this file (brief 7.2).
    pub suggest_rtl_fix: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlannedMovie {
    pub title: String,
    pub year: Option<u16>,
    pub video: VideoFile,
    pub subtitles: Vec<SubtitleFile>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlannedEpisode {
    pub season: Option<u32>,
    pub episodes: Vec<u32>,
    pub air_date: Option<String>,
    pub episode_title: Option<String>,
    pub video: VideoFile,
    pub subtitles: Vec<SubtitleFile>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlannedShow {
    pub title: String,
    /// Other names the files or folders used (e.g. the Hebrew title in file
    /// names under an English folder). Lookups try these too.
    pub alt_titles: Vec<String>,
    pub year: Option<u16>,
    pub episodes: Vec<PlannedEpisode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    Sample,
    Extra,
    SubtitleTooLarge,
    Unreadable,
}

#[derive(Clone, Debug, Serialize)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: SkipReason,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ImportPlan {
    pub movies: Vec<PlannedMovie>,
    pub shows: Vec<PlannedShow>,
    pub unmatched_subtitles: Vec<SubtitleFile>,
    pub skipped: Vec<Skipped>,
}

struct Found {
    rel: PathBuf,
    size: u64,
    in_extras: bool,
}

fn lower_ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn walk(root: &Path, rel: &Path, in_extras: bool, out: &mut Vec<Found>) -> io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(root.join(rel))?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue; // hidden files, and the vault's own .cinasafe folder
        }
        // `file_type` does not follow symlinks, so link loops can't trap the walk.
        let ft = e.file_type()?;
        let child = rel.join(&name);
        if ft.is_dir() {
            let extras = in_extras || EXTRAS_DIRS.contains(&name_str.to_lowercase().as_str());
            walk(root, &child, extras, out)?;
        } else if ft.is_file() {
            out.push(Found { rel: child, size: e.metadata()?.len(), in_extras });
        }
    }
    Ok(())
}

fn is_sample_or_trailer(rel: &Path, size: u64) -> Option<SkipReason> {
    let stem = rel.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
    let tokens: Vec<&str> = stem.split(|c: char| !c.is_alphanumeric()).collect();
    if tokens.contains(&"sample") && size < SAMPLE_MAX_BYTES {
        return Some(SkipReason::Sample);
    }
    if tokens.last() == Some(&"trailer") || tokens.contains(&"featurette") {
        return Some(SkipReason::Extra);
    }
    None
}

fn analyse_subtitle(root: &Path, f: &Found, format: SubFormat) -> Result<SubtitleFile, SkipReason> {
    if f.size > MAX_SUBTITLE_BYTES {
        return Err(SkipReason::SubtitleTooLarge);
    }
    let bytes = fs::read(root.join(&f.rel)).map_err(|_| SkipReason::Unreadable)?;
    let decoded = subtitles::decode(&bytes);
    let stem = f.rel.file_stem().unwrap_or_default().to_string_lossy();
    let (name_lang, flags) = lang::tags_from_stem(&stem);
    let content_lang = subtitles::detect_language(&decoded.text, format);
    // Trust the text over the name when they disagree on an RTL script: a file
    // called `.en.srt` full of Hebrew is Hebrew.
    let (language, language_from) = match (name_lang, content_lang) {
        (Some(n), Some(c)) if n != c => (Some(c), LanguageSource::Content),
        (Some(n), _) => (Some(n), LanguageSource::FileName),
        (None, Some(c)) => (Some(c), LanguageSource::Content),
        (None, None) => (None, LanguageSource::Unknown),
    };
    Ok(SubtitleFile {
        path: f.rel.clone(),
        format,
        language,
        language_from,
        forced: flags.forced,
        sdh: flags.sdh,
        source_encoding: decoded.source_encoding,
        suggest_rtl_fix: subtitles::suggests_rtl_fix(&decoded.text, format, language),
    })
}

struct Video {
    file: VideoFile,
    guess: MediaGuess,
}

fn dir_name(p: Option<&Path>) -> String {
    p.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn stem_lower(p: &Path) -> String {
    p.file_stem().unwrap_or_default().to_string_lossy().to_lowercase()
}

/// How sure we are that `sub` belongs to `video` (0 = not at all).
fn match_score(sub: &Path, sub_guess: &MediaGuess, video: &Video, videos_in_dir: usize) -> u32 {
    let vrel = &video.file.path;
    let vdir = vrel.parent();
    let sdir = sub.parent();
    let vstem = stem_lower(vrel);
    // `Subs/` next to the video, or `Subs/<video stem>/` (common for episode packs).
    let in_subs_dir = SUBTITLE_DIRS.contains(&dir_name(sdir).as_str()) && sdir.and_then(Path::parent) == vdir;
    let in_per_video_dir = dir_name(sdir) == vstem
        && SUBTITLE_DIRS.contains(&dir_name(sdir.and_then(Path::parent)).as_str())
        && sdir.and_then(Path::parent).and_then(Path::parent) == vdir;
    if sdir != vdir && !in_subs_dir && !in_per_video_dir {
        return 0;
    }

    let sstem = stem_lower(sub);
    if in_per_video_dir {
        return 100;
    }
    if let Some(rest) = sstem.strip_prefix(&vstem) {
        if rest.is_empty() || rest.starts_with(['.', '_', ' ', '-', '[', '(']) {
            return 100;
        }
    }
    match (sub_guess, &video.guess) {
        (
            MediaGuess::Episode { season: ss, episodes: se, .. },
            MediaGuess::Episode { season: vs, episodes: ve, .. },
        ) if !se.is_empty() && !ve.is_empty() => {
            // Both name an episode: equal numbers match, different numbers never do.
            if ss == vs && se == ve {
                80
            } else {
                0
            }
        }
        (MediaGuess::Movie { title: st, .. }, MediaGuess::Movie { title: vt, .. })
            if !st.is_empty() && normalize_title(st) == normalize_title(vt) =>
        {
            60
        }
        _ if videos_in_dir == 1 => 40,
        _ => 0,
    }
}

/// Scans `root` and returns what would be imported. Does not write anything.
pub fn scan(root: &Path) -> io::Result<ImportPlan> {
    let mut found = Vec::new();
    walk(root, Path::new(""), false, &mut found)?;

    let mut plan = ImportPlan::default();
    let mut videos: Vec<Video> = Vec::new();
    let mut subs: Vec<SubtitleFile> = Vec::new();
    for f in &found {
        let ext = lower_ext(&f.rel);
        if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            let reason = if f.in_extras { Some(SkipReason::Extra) } else { is_sample_or_trailer(&f.rel, f.size) };
            match reason {
                Some(reason) => plan.skipped.push(Skipped { path: f.rel.clone(), reason }),
                None => {
                    videos.push(Video { file: VideoFile { path: f.rel.clone(), size: f.size }, guess: guess(&f.rel) })
                }
            }
        } else if let Some(format) = SubFormat::from_extension(&ext) {
            match analyse_subtitle(root, f, format) {
                Ok(s) => subs.push(s),
                Err(reason) => plan.skipped.push(Skipped { path: f.rel.clone(), reason }),
            }
        }
    }

    let mut per_dir: BTreeMap<Option<&Path>, usize> = BTreeMap::new();
    for v in &videos {
        *per_dir.entry(v.file.path.parent()).or_default() += 1;
    }
    let mut attached: Vec<Vec<SubtitleFile>> = vec![Vec::new(); videos.len()];
    for s in subs {
        let sub_guess = guess(&s.path);
        let scores: Vec<u32> = videos
            .iter()
            .map(|v| match_score(&s.path, &sub_guess, v, per_dir.get(&v.file.path.parent()).copied().unwrap_or(0)))
            .collect();
        let best = scores.iter().copied().max().unwrap_or(0);
        let winners: Vec<usize> = (0..scores.len()).filter(|&i| scores[i] == best).collect();
        if best > 0 && winners.len() == 1 {
            attached[winners[0]].push(s);
        } else {
            plan.unmatched_subtitles.push(s);
        }
    }

    // Group episodes into shows: by show folder when files sit in
    // `<Show>/<Season N>/`, otherwise by normalised title. The folder name
    // (or the first spelling seen) is the display title.
    #[derive(Default)]
    struct ShowGroup {
        titles: Vec<String>,
        years: Vec<Option<u16>>,
        episodes: Vec<PlannedEpisode>,
    }
    let mut shows: BTreeMap<String, ShowGroup> = BTreeMap::new();
    for (v, mut s) in videos.into_iter().zip(attached) {
        s.sort_by(|a, b| a.path.cmp(&b.path));
        match v.guess {
            MediaGuess::Movie { title, year } => {
                plan.movies.push(PlannedMovie { title, year, video: v.file, subtitles: s })
            }
            MediaGuess::Episode { show, year, season, episodes, air_date, episode_title } => {
                let folder = parse::show_folder(&v.file.path);
                let key = normalize_title(folder.as_ref().map_or(&show, |(f, _)| f));
                let group = shows.entry(key).or_default();
                if let Some((f, fy)) = folder {
                    group.titles.insert(0, f);
                    group.years.push(fy);
                }
                group.titles.push(show);
                group.years.push(year);
                group.episodes.push(PlannedEpisode {
                    season,
                    episodes,
                    air_date,
                    episode_title,
                    video: v.file,
                    subtitles: s,
                });
            }
        }
    }
    for (_, g) in shows {
        let ShowGroup { titles, years, mut episodes } = g;
        let mut seen = Vec::new();
        let mut distinct = Vec::new();
        for t in titles.into_iter().filter(|t| !t.is_empty()) {
            let n = normalize_title(&t);
            if !seen.contains(&n) {
                seen.push(n);
                distinct.push(t);
            }
        }
        let title = if distinct.is_empty() { String::new() } else { distinct.remove(0) };
        episodes.sort_by(|a, b| {
            (a.season, a.episodes.first(), &a.air_date, &a.video.path).cmp(&(
                b.season,
                b.episodes.first(),
                &b.air_date,
                &b.video.path,
            ))
        });
        plan.shows.push(PlannedShow { title, alt_titles: distinct, year: most_common(&years), episodes });
    }
    plan.movies.sort_by_key(|m| (normalize_title(&m.title), m.year));
    Ok(plan)
}

fn most_common(years: &[Option<u16>]) -> Option<u16> {
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for y in years.iter().flatten() {
        *counts.entry(*y).or_default() += 1;
    }
    counts.into_iter().max_by_key(|&(y, n)| (n, std::cmp::Reverse(y))).map(|(y, _)| y)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(name: &str) -> TempTree {
            let p = std::env::temp_dir().join(format!("cinasafe-import-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempTree(p)
        }

        fn file(&self, rel: &str, contents: &[u8]) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, contents).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const HE: &str = "1\n00:00:01,000 --> 00:00:02,000\nשלום, זו כתובית בעברית לבדיקה ארוכה מספיק\n";
    const EN: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello, this is an English subtitle\n";

    fn subs_of<'a>(plan: &'a ImportPlan, video: &str) -> Vec<(&'a str, Option<&'static str>)> {
        let all = plan
            .movies
            .iter()
            .map(|m| (&m.video, &m.subtitles))
            .chain(plan.shows.iter().flat_map(|s| s.episodes.iter().map(|e| (&e.video, &e.subtitles))));
        for (v, s) in all {
            if v.path == Path::new(video) {
                return s.iter().map(|s| (s.path.to_str().unwrap(), s.language)).collect();
            }
        }
        panic!("video {video} not in plan");
    }

    #[test]
    fn scans_movies_shows_and_subtitles() {
        let t = TempTree::new("mixed");
        t.file("Inception (2010)/Inception.2010.1080p.BluRay.mkv", b"v");
        t.file("Inception (2010)/Inception.2010.1080p.BluRay.he.srt", HE.as_bytes());
        t.file("Inception (2010)/Subs/2_English.srt", EN.as_bytes());
        t.file("Inception (2010)/Sample/inception-sample.mkv", b"v");
        t.file("Inception (2010)/Extras/Making Of.mkv", b"v");
        // Hebrew in Windows-1255 with a misleading English name: content wins.
        let (he1255, _, _) = encoding_rs::WINDOWS_1255.encode(HE);
        t.file("Waltz.With.Bashir.2008.mkv", b"v");
        t.file("Waltz.With.Bashir.2008.en.srt", &he1255);

        t.file("Fauda/Season 1/Fauda.S01E01.1080p.mkv", b"v");
        t.file("Fauda/Season 1/Fauda.S01E02.1080p.mkv", b"v");
        t.file("Fauda/Season 1/Fauda.S01E01.heb.srt", HE.as_bytes());
        t.file("Fauda/Season 1/Fauda.S01E02.Hebrew.forced.srt", HE.as_bytes());
        t.file("Fauda/Season 1/Fauda.S01E03.he.srt", HE.as_bytes()); // no such video
        t.file("Fauda/Season 1/Subs/Fauda.S01E02.1080p/3_English.srt", EN.as_bytes());
        t.file("פאודה/עונה 2/פרק 1.mkv", b"v");
        t.file("Fauda/Season 2/פאודה עונה 2 פרק 5.mkv", b"v");
        t.file(".cinasafe/blobs/aa/bb/x.cblob", b"x");

        let plan = scan(&t.0).unwrap();

        let titles: Vec<_> = plan.movies.iter().map(|m| (m.title.as_str(), m.year)).collect();
        assert_eq!(titles, [("Inception", Some(2010)), ("Waltz With Bashir", Some(2008))]);
        assert_eq!(
            subs_of(&plan, "Inception (2010)/Inception.2010.1080p.BluRay.mkv"),
            [
                ("Inception (2010)/Inception.2010.1080p.BluRay.he.srt", Some("he")),
                ("Inception (2010)/Subs/2_English.srt", Some("en")),
            ]
        );
        let bashir = &plan.movies[1].subtitles[0];
        assert_eq!((bashir.language, bashir.language_from), (Some("he"), LanguageSource::Content));
        assert_eq!(bashir.source_encoding, "windows-1255");

        let shows: Vec<_> = plan.shows.iter().map(|s| (s.title.as_str(), s.episodes.len())).collect();
        assert_eq!(shows, [("Fauda", 3), ("פאודה", 1)]);
        assert_eq!(plan.shows[0].alt_titles, ["פאודה"]);
        assert_eq!(
            subs_of(&plan, "Fauda/Season 1/Fauda.S01E01.1080p.mkv"),
            [("Fauda/Season 1/Fauda.S01E01.heb.srt", Some("he"))]
        );
        let e2 = subs_of(&plan, "Fauda/Season 1/Fauda.S01E02.1080p.mkv");
        assert_eq!(e2.len(), 2);
        assert!(plan.shows[0].episodes[1].subtitles.iter().any(|s| s.forced));
        assert_eq!(plan.shows[1].episodes[0].season, Some(2));

        let unmatched: Vec<_> = plan.unmatched_subtitles.iter().map(|s| s.path.to_str().unwrap()).collect();
        assert_eq!(unmatched, ["Fauda/Season 1/Fauda.S01E03.he.srt"]);
        let mut skipped: Vec<_> = plan.skipped.iter().map(|s| (s.path.to_str().unwrap(), s.reason)).collect();
        skipped.sort();
        assert_eq!(
            skipped,
            [
                ("Inception (2010)/Extras/Making Of.mkv", SkipReason::Extra),
                ("Inception (2010)/Sample/inception-sample.mkv", SkipReason::Extra),
            ]
        );
    }

    #[test]
    fn ambiguous_subtitle_stays_unmatched() {
        let t = TempTree::new("ambiguous");
        t.file("A.Movie.2001.mkv", b"v");
        t.file("Another.Film.2002.mkv", b"v");
        t.file("subtitle.srt", EN.as_bytes());
        let plan = scan(&t.0).unwrap();
        assert_eq!(plan.unmatched_subtitles.len(), 1);
    }
}
