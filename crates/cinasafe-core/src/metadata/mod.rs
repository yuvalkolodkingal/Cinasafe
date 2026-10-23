//! Online metadata: titles, overviews, seasons/episodes and artwork.
//!
//! Providers are behind [`MetadataProvider`]: TMDB (movies and TV; needs an
//! API key) and TVmaze (TV only; no key). Everything fetched, artwork
//! included, stays in memory; nothing is cached on disk. Every request sends
//! the parsed title to the provider, so callers must only use this module
//! after the user has turned online metadata on (docs/METADATA.md).

pub mod http;
pub mod identify;
pub mod matching;
pub mod tmdb;
pub mod tvmaze;

use serde::Serialize;

pub use http::{HttpClient, HttpResponse, ReqwestClient};
pub use identify::{fetch_artwork, identify_movie, identify_show, Artwork, IdentifiedMovie, IdentifiedShow};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("the provider rejected the API key")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    #[error("rate limited by the provider")]
    RateLimited,
    #[error("HTTP status {0}")]
    Status(u16),
    #[error("unexpected response: {0}")]
    Decode(String),
    #[error("{0} does not provide movie metadata")]
    MoviesUnsupported(&'static str),
    #[error("not an image, or too large")]
    BadImage,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtKind {
    Poster,
    Backdrop,
    Thumb,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkRef {
    pub kind: ArtKind,
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// A search hit, before details are fetched.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub provider: &'static str,
    pub id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub overview: Option<String>,
    /// Provider-specific popularity; only compared between hits of one search.
    pub popularity: f64,
    pub poster: Option<ArtworkRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MovieDetails {
    pub provider: &'static str,
    pub id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub runtime_min: Option<u32>,
    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub imdb_id: Option<String>,
    pub poster: Option<ArtworkRef>,
    pub backdrop: Option<ArtworkRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EpisodeDetails {
    pub season: u32,
    pub number: u32,
    pub title: Option<String>,
    pub overview: Option<String>,
    /// `YYYY-MM-DD`.
    pub air_date: Option<String>,
    pub runtime_min: Option<u32>,
    pub thumb: Option<ArtworkRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SeasonDetails {
    /// 0 = specials.
    pub number: u32,
    pub title: Option<String>,
    pub poster: Option<ArtworkRef>,
    pub episodes: Vec<EpisodeDetails>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShowDetails {
    pub provider: &'static str,
    pub id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub first_year: Option<u16>,
    pub last_year: Option<u16>,
    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub poster: Option<ArtworkRef>,
    pub backdrop: Option<ArtworkRef>,
    /// Seasons with their episodes (only those asked for, if the provider
    /// fetches seasons separately).
    pub seasons: Vec<SeasonDetails>,
}

impl ShowDetails {
    pub fn episode(&self, season: u32, number: u32) -> Option<&EpisodeDetails> {
        self.seasons.iter().find(|s| s.number == season)?.episodes.iter().find(|e| e.number == number)
    }
}

pub trait MetadataProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn search_movie(&self, title: &str, year: Option<u16>) -> Result<Vec<Candidate>>;
    fn search_show(&self, title: &str, year: Option<u16>) -> Result<Vec<Candidate>>;
    fn movie(&self, id: &str) -> Result<MovieDetails>;
    /// Show details with the episodes of `seasons` (all seasons if empty).
    fn show(&self, id: &str, seasons: &[u32]) -> Result<ShowDetails>;
}

pub(crate) fn year_of(date: Option<&str>) -> Option<u16> {
    date.filter(|d| d.len() >= 4).and_then(|d| d[..4].parse().ok())
}

pub(crate) fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}
