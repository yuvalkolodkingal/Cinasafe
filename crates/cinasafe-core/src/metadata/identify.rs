//! From a parsed file name to metadata: search, rank, fetch details, map
//! episodes, and download artwork into memory.

use std::fmt;

use serde::Serialize;

use super::matching::{self, Ranked};
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct Scored {
    pub candidate: Candidate,
    pub confidence: f32,
}

impl From<Ranked> for Scored {
    fn from(r: Ranked) -> Scored {
        Scored { candidate: r.candidate, confidence: r.confidence }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentifiedMovie {
    /// Safe to apply without asking the user.
    pub confident: bool,
    pub confidence: f32,
    /// Details of the best hit, if there was any hit.
    pub details: Option<MovieDetails>,
    /// Every hit, best first, for the "not this one?" picker.
    pub candidates: Vec<Scored>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentifiedShow {
    pub confident: bool,
    pub confidence: f32,
    pub details: Option<ShowDetails>,
    pub candidates: Vec<Scored>,
}

/// Searches with the year first, then without it if that found nothing (the
/// year in a file name is sometimes a release or air year that doesn't match).
fn search(
    title: &str,
    year: Option<u16>,
    f: impl Fn(&str, Option<u16>) -> Result<Vec<Candidate>>,
) -> Result<Vec<Ranked>> {
    let mut hits = f(title, year)?;
    if hits.is_empty() && year.is_some() {
        hits = f(title, None)?;
    }
    hits.truncate(20);
    Ok(matching::rank(title, year, hits))
}

pub fn identify_movie(p: &dyn MetadataProvider, title: &str, year: Option<u16>) -> Result<IdentifiedMovie> {
    let ranked = search(title, year, |t, y| p.search_movie(t, y))?;
    let confident = matching::is_confident(&ranked);
    let confidence = ranked.first().map_or(0.0, |r| r.confidence);
    let details = match ranked.first() {
        Some(best) => Some(p.movie(&best.candidate.id)?),
        None => None,
    };
    Ok(IdentifiedMovie { confident, confidence, details, candidates: ranked.into_iter().map(Scored::from).collect() })
}

/// `seasons`: the season numbers present on disk; empty fetches every season
/// (needed for absolute-numbered or dated episodes).
pub fn identify_show(
    p: &dyn MetadataProvider,
    title: &str,
    year: Option<u16>,
    seasons: &[u32],
) -> Result<IdentifiedShow> {
    let ranked = search(title, year, |t, y| p.search_show(t, y))?;
    let confident = matching::is_confident(&ranked);
    let confidence = ranked.first().map_or(0.0, |r| r.confidence);
    let details = match ranked.first() {
        Some(best) => Some(p.show(&best.candidate.id, seasons)?),
        None => None,
    };
    Ok(IdentifiedShow { confident, confidence, details, candidates: ranked.into_iter().map(Scored::from).collect() })
}

/// Episode metadata for a file, by season/episode, absolute number or air date.
pub fn episodes_for<'a>(
    show: &'a ShowDetails,
    season: Option<u32>,
    episodes: &[u32],
    air_date: Option<&str>,
) -> Vec<&'a EpisodeDetails> {
    if let Some(date) = air_date {
        return show.seasons.iter().flat_map(|s| &s.episodes).filter(|e| e.air_date.as_deref() == Some(date)).collect();
    }
    match season {
        Some(s) => episodes.iter().filter_map(|&n| show.episode(s, n)).collect(),
        None => {
            // Absolute numbering: count regular episodes across seasons, specials excluded.
            let mut all: Vec<&EpisodeDetails> =
                show.seasons.iter().filter(|s| s.number > 0).flat_map(|s| &s.episodes).collect();
            all.sort_by_key(|e| (e.season, e.number));
            episodes.iter().filter_map(|&n| all.get((n as usize).checked_sub(1)?).copied()).collect()
        }
    }
}

/// Artwork bytes, in memory only. They are encrypted into a blob on import.
#[derive(Clone)]
pub struct Artwork {
    pub kind: ArtKind,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for Artwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Artwork({:?}, {}, {} bytes)", self.kind, self.mime, self.bytes.len())
    }
}

const MAX_IMAGE: usize = 20 << 20;
/// Artwork URLs come from provider responses; only fetch from their image hosts.
const IMAGE_HOSTS: &[&str] = &["https://image.tmdb.org/", "https://static.tvmaze.com/"];

fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    }
}

pub fn fetch_artwork(http: &dyn HttpClient, art: &ArtworkRef) -> Result<Artwork> {
    if !IMAGE_HOSTS.iter().any(|h| art.url.starts_with(h)) {
        return Err(Error::BadImage);
    }
    let resp = http.get(&art.url, &[], MAX_IMAGE)?;
    match resp.status {
        200 => {}
        404 => return Err(Error::NotFound),
        s => return Err(Error::Status(s)),
    }
    // Trust the bytes, not the Content-Type header.
    let mime = sniff(&resp.body).ok_or(Error::BadImage)?;
    Ok(Artwork { kind: art.kind, mime, bytes: resp.body })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::http::fake::FakeHttp;
    use super::super::{tmdb::Tmdb, tvmaze::TvMaze};
    use super::*;

    #[test]
    fn identifies_movie_end_to_end() {
        let http = Arc::new(
            FakeHttp::new()
                .route("/search/movie", "tmdb/search-movie-inception.json")
                .route("/movie/27205?language=he-IL", "tmdb/movie-27205-he.json")
                .route("/movie/27205?language=en-US", "tmdb/movie-27205-en.json"),
        );
        let t = Tmdb::new(http, "k", "he-IL");
        let r = identify_movie(&t, "Inception", Some(2010)).unwrap();
        assert!(r.confident);
        assert_eq!(r.details.unwrap().id, "27205");
        assert_eq!(r.candidates.len(), 3);
    }

    #[test]
    fn retries_without_year() {
        let http = Arc::new(
            FakeHttp::new()
                .status(
                    "/search/movie?query=Inception&include_adult=false&language=en-US&year=1999",
                    200,
                    br#"{"results":[]}"#,
                )
                .route("/search/movie", "tmdb/search-movie-inception.json")
                .route("/movie/27205?", "tmdb/movie-27205-en.json"),
        );
        let t = Tmdb::new(http.clone(), "k", "en-US");
        let r = identify_movie(&t, "Inception", Some(1999)).unwrap();
        assert_eq!(http.urls().iter().filter(|u| u.contains("/search/movie")).count(), 2);
        // Found, but the year disagrees, so it is only a suggestion.
        assert!(r.details.is_some());
    }

    #[test]
    fn identifies_show_and_maps_episodes() {
        let m = TvMaze::new(Arc::new(
            FakeHttp::new()
                .route("/search/shows", "tvmaze/search-fauda.json")
                .route("/shows/15315?", "tvmaze/show-15315.json")
                .route("/shows/15315/images", "tvmaze/images-15315.json"),
        ));
        let r = identify_show(&m, "Fauda", None, &[]).unwrap();
        assert!(r.confident);
        let show = r.details.unwrap();
        assert_eq!(episodes_for(&show, Some(1), &[2], None)[0].number, 2);
        // Absolute episode 14 = season 2 episode 2 (12 episodes in season 1).
        let abs = episodes_for(&show, None, &[14], None);
        assert_eq!((abs[0].season, abs[0].number), (2, 2));
        assert_eq!(episodes_for(&show, None, &[], Some("2015-02-15"))[0].number, 1);
        assert!(episodes_for(&show, Some(1), &[99], None).is_empty());
    }

    struct Img(Vec<u8>);
    impl HttpClient for Img {
        fn get(&self, _: &str, _: &[(&str, &str)], _: usize) -> Result<HttpResponse> {
            Ok(HttpResponse { status: 200, content_type: Some("image/jpeg".into()), body: self.0.clone() })
        }
    }

    #[test]
    fn artwork_is_checked() {
        let art = |url: &str| ArtworkRef { kind: ArtKind::Poster, url: url.into(), width: None, height: None };
        let jpeg = Img(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]);
        let a = fetch_artwork(&jpeg, &art("https://image.tmdb.org/t/p/w500/x.jpg")).unwrap();
        assert_eq!(a.mime, "image/jpeg");
        assert!(matches!(fetch_artwork(&jpeg, &art("https://evil.example/x.jpg")), Err(Error::BadImage)));
        assert!(matches!(fetch_artwork(&jpeg, &art("http://image.tmdb.org/x.jpg")), Err(Error::BadImage)));
        let html = Img(b"<html>".to_vec());
        assert!(matches!(fetch_artwork(&html, &art("https://static.tvmaze.com/x.jpg")), Err(Error::BadImage)));
    }
}
