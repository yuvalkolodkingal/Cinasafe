//! `cinasafe scan` and `cinasafe identify`: what an import of a folder would
//! recognise, and what online metadata it would attach. Read-only.

use std::path::Path;
use std::sync::Arc;

use cinasafe_core::import::{self, ImportPlan, PlannedEpisode, SubtitleFile};
use cinasafe_core::metadata::identify::{episodes_for, IdentifiedMovie, IdentifiedShow};
use cinasafe_core::metadata::{
    self, fetch_artwork, identify_movie, identify_show, tmdb::Tmdb, tvmaze::TvMaze, ArtworkRef, HttpClient,
    MetadataProvider, ReqwestClient,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum ProviderChoice {
    /// TMDB when a key is set, otherwise TVmaze for shows (movies need TMDB).
    Auto,
    Tmdb,
    Tvmaze,
}

fn human_size(n: u64) -> String {
    let gib = n as f64 / (1u64 << 30) as f64;
    if gib >= 1.0 {
        format!("{gib:.1} GB")
    } else {
        format!("{:.0} MB", n as f64 / (1u64 << 20) as f64)
    }
}

fn sub_line(s: &SubtitleFile) -> String {
    let mut parts = vec![s.language.unwrap_or("??").to_owned(), format!("{:?}", s.format).to_lowercase()];
    if s.forced {
        parts.push("forced".into());
    }
    if s.sdh {
        parts.push("SDH".into());
    }
    match s.language_from {
        import::LanguageSource::Content => parts.push("language from text".into()),
        import::LanguageSource::Unknown => parts.push("language unknown".into()),
        import::LanguageSource::FileName => {}
    }
    if s.source_encoding != "UTF-8" {
        parts.push(format!("{} -> UTF-8", s.source_encoding));
    }
    if s.suggest_rtl_fix {
        parts.push("offer RTL punctuation fix".into());
    }
    format!("sub [{}] {}", parts.join(", "), s.path.display())
}

fn episode_label(e: &PlannedEpisode) -> String {
    let nums: Vec<String> = e.episodes.iter().map(|n| format!("E{n:02}")).collect();
    match (e.season, &e.air_date) {
        (_, Some(d)) => d.clone(),
        (Some(s), _) => format!("S{s:02}{}", nums.join("")),
        (None, _) => format!("#{}", e.episodes.iter().map(u32::to_string).collect::<Vec<_>>().join("+")),
    }
}

fn year(y: Option<u16>) -> String {
    y.map(|y| format!(" ({y})")).unwrap_or_default()
}

pub fn print_plan(plan: &ImportPlan) {
    println!("Movies ({})", plan.movies.len());
    for m in &plan.movies {
        println!("  {}{}  [{}]  {}", m.title, year(m.year), human_size(m.video.size), m.video.path.display());
        for s in &m.subtitles {
            println!("      {}", sub_line(s));
        }
    }
    println!("Shows ({})", plan.shows.len());
    for show in &plan.shows {
        println!("  {}{}  ({} files)", show.title, year(show.year), show.episodes.len());
        for e in &show.episodes {
            println!("    {:<10} {}", episode_label(e), e.video.path.display());
            for s in &e.subtitles {
                println!("      {}", sub_line(s));
            }
        }
    }
    if !plan.unmatched_subtitles.is_empty() {
        println!("Subtitles not matched to a video ({})", plan.unmatched_subtitles.len());
        for s in &plan.unmatched_subtitles {
            println!("  {}", sub_line(s));
        }
    }
    if !plan.skipped.is_empty() {
        println!("Skipped ({})", plan.skipped.len());
        for s in &plan.skipped {
            println!("  {:?}: {}", s.reason, s.path.display());
        }
    }
}

pub fn scan(dir: &Path, as_json: bool) -> Result<(), String> {
    let plan = import::scan(dir).map_err(|e| format!("cannot scan {}: {e}", dir.display()))?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?);
    } else {
        print_plan(&plan);
    }
    Ok(())
}

pub struct IdentifyArgs<'a> {
    pub dir: &'a Path,
    pub provider: ProviderChoice,
    pub tmdb_key: Option<String>,
    pub language: String,
    pub artwork: bool,
    pub json: bool,
}

#[derive(Serialize)]
struct Report {
    movies: Vec<MovieReport>,
    shows: Vec<ShowReport>,
}

#[derive(Serialize)]
struct MovieReport {
    file: String,
    parsed_title: String,
    parsed_year: Option<u16>,
    result: Result<IdentifiedMovie, String>,
    artwork: Vec<ArtReport>,
}

#[derive(Serialize)]
struct ShowReport {
    parsed_title: String,
    parsed_year: Option<u16>,
    result: Result<IdentifiedShow, String>,
    episodes: Vec<EpisodeReport>,
    artwork: Vec<ArtReport>,
}

#[derive(Serialize)]
struct EpisodeReport {
    file: String,
    matched: Vec<String>,
}

#[derive(Serialize)]
struct ArtReport {
    url: String,
    result: Result<String, String>,
}

fn grab(http: &dyn HttpClient, art: &[Option<&ArtworkRef>]) -> Vec<ArtReport> {
    art.iter()
        .flatten()
        .map(|a| ArtReport {
            url: a.url.clone(),
            result: fetch_artwork(http, a)
                .map(|img| format!("{:?} {} ({} KB, in memory)", img.kind, img.mime, img.bytes.len() / 1024))
                .map_err(|e| e.to_string()),
        })
        .collect()
}

pub fn identify(args: IdentifyArgs) -> Result<(), String> {
    let plan = import::scan(args.dir).map_err(|e| format!("cannot scan {}: {e}", args.dir.display()))?;
    let http: Arc<ReqwestClient> = Arc::new(ReqwestClient::new().map_err(|e| e.to_string())?);
    let key = args.tmdb_key.filter(|k| !k.trim().is_empty());
    let tmdb: Option<Tmdb> = key.as_deref().map(|k| Tmdb::new(http.clone(), k, &args.language));
    let tvmaze = TvMaze::new(http.clone());
    let (movie_provider, show_provider): (Option<&dyn MetadataProvider>, &dyn MetadataProvider) =
        match (args.provider, &tmdb) {
            (ProviderChoice::Tvmaze, _) => (None, &tvmaze),
            (ProviderChoice::Tmdb, None) => return Err("--provider tmdb needs CINASAFE_TMDB_KEY or --tmdb-key".into()),
            (_, Some(t)) => (Some(t), t),
            (ProviderChoice::Auto, None) => (None, &tvmaze),
        };
    if !args.json {
        eprintln!(
            "Looking up {} movies and {} shows online ({}). Titles are sent to the provider.",
            plan.movies.len(),
            plan.shows.len(),
            match movie_provider {
                Some(_) => "TMDB",
                None => "TVmaze; movies need a TMDB key",
            }
        );
    }

    let mut report = Report { movies: Vec::new(), shows: Vec::new() };
    for m in &plan.movies {
        let result = match movie_provider {
            Some(p) => identify_movie(p, &m.title, m.year).map_err(|e| e.to_string()),
            None => Err(metadata::Error::MoviesUnsupported("TVmaze").to_string()),
        };
        let artwork = match (&result, args.artwork) {
            (Ok(r), true) => r
                .details
                .as_ref()
                .map(|d| grab(http.as_ref(), &[d.poster.as_ref(), d.backdrop.as_ref()]))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        report.movies.push(MovieReport {
            file: m.video.path.display().to_string(),
            parsed_title: m.title.clone(),
            parsed_year: m.year,
            result,
            artwork,
        });
    }
    for s in &plan.shows {
        // Absolute-numbered or dated files need every season to be mapped.
        let needs_all = s.episodes.iter().any(|e| e.season.is_none() || e.air_date.is_some());
        let mut seasons: Vec<u32> =
            if needs_all { Vec::new() } else { s.episodes.iter().filter_map(|e| e.season).collect() };
        seasons.sort_unstable();
        seasons.dedup();
        let titles: Vec<&str> =
            std::iter::once(s.title.as_str()).chain(s.alt_titles.iter().map(String::as_str)).collect();
        let result = identify_show(show_provider, &titles, s.year, &seasons).map_err(|e| e.to_string());
        let mut episodes = Vec::new();
        let mut artwork = Vec::new();
        if let Ok(IdentifiedShow { details: Some(d), .. }) = &result {
            for e in &s.episodes {
                let found = episodes_for(d, e.season, &e.episodes, e.air_date.as_deref());
                if args.artwork {
                    artwork.extend(grab(http.as_ref(), &found.iter().map(|f| f.thumb.as_ref()).collect::<Vec<_>>()));
                }
                episodes.push(EpisodeReport {
                    file: e.video.path.display().to_string(),
                    matched: found
                        .iter()
                        .map(|f| {
                            format!(
                                "S{:02}E{:02} {}{}",
                                f.season,
                                f.number,
                                f.title.as_deref().unwrap_or("(untitled)"),
                                f.air_date.as_deref().map(|a| format!(" [{a}]")).unwrap_or_default()
                            )
                        })
                        .collect(),
                });
            }
            if args.artwork {
                let season_posters: Vec<Option<&ArtworkRef>> = d.seasons.iter().map(|s| s.poster.as_ref()).collect();
                artwork.extend(grab(http.as_ref(), &[d.poster.as_ref(), d.backdrop.as_ref()]));
                artwork.extend(grab(http.as_ref(), &season_posters));
            }
        }
        report.shows.push(ShowReport { parsed_title: s.title.clone(), parsed_year: s.year, result, episodes, artwork });
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
        return Ok(());
    }
    let verdict = |confident: bool, c: f32| {
        if confident {
            format!("match {:.0}%", c * 100.0)
        } else {
            format!("SUGGESTION {:.0}%, confirm", c * 100.0)
        }
    };
    for m in &report.movies {
        println!("{}{}  <- {}", m.parsed_title, year(m.parsed_year), m.file);
        match &m.result {
            Ok(IdentifiedMovie { details: Some(d), confident, confidence, .. }) => println!(
                "  {} {}:{}  {}{}  {}  {}",
                verdict(*confident, *confidence),
                d.provider,
                d.id,
                d.title,
                year(d.year),
                d.runtime_min.map(|r| format!("{r} min")).unwrap_or_default(),
                d.genres.join(", ")
            ),
            Ok(_) => println!("  no match"),
            Err(e) => println!("  not looked up: {e}"),
        }
        for a in &m.artwork {
            println!("  art {}", a.result.as_ref().unwrap_or_else(|e| e));
        }
    }
    for s in &report.shows {
        println!("{}{}", s.parsed_title, year(s.parsed_year));
        match &s.result {
            Ok(IdentifiedShow { details: Some(d), confident, confidence, .. }) => println!(
                "  {} {}:{}  {}{}  {} seasons fetched",
                verdict(*confident, *confidence),
                d.provider,
                d.id,
                d.title,
                year(d.first_year),
                d.seasons.len()
            ),
            Ok(_) => println!("  no match"),
            Err(e) => println!("  lookup failed: {e}"),
        }
        for e in &s.episodes {
            let m = if e.matched.is_empty() { "no episode data".to_owned() } else { e.matched.join(" + ") };
            println!("    {m}  <- {}", e.file);
        }
        for a in &s.artwork {
            println!("  art {}", a.result.as_ref().unwrap_or_else(|e| e));
        }
    }
    Ok(())
}
