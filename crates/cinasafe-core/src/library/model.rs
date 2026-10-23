//! Rows and summaries the library hands out. They serialise as-is over the
//! app's IPC, so field names are the UI's contract (camelCase there).

use serde::{Deserialize, Serialize};

use crate::BlobId;

pub type Id = i64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtKind {
    Poster,
    Backdrop,
    Thumb,
}

impl ArtKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtKind::Poster => "poster",
            ArtKind::Backdrop => "backdrop",
            ArtKind::Thumb => "thumb",
        }
    }

    pub fn parse(s: &str) -> Option<ArtKind> {
        match s {
            "poster" => Some(ArtKind::Poster),
            "backdrop" => Some(ArtKind::Backdrop),
            "thumb" => Some(ArtKind::Thumb),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFile {
    pub id: Id,
    #[serde(skip)]
    pub blob_id: BlobId,
    pub original_name: String,
    pub size_bytes: u64,
    pub duration_ms: Option<i64>,
    pub container: Option<String>,
    pub streams_json: Option<String>,
    pub added_at: String,
}

#[derive(Clone, Debug, Default)]
pub struct NewMediaFile {
    pub original_name: String,
    pub size_bytes: u64,
    pub duration_ms: Option<i64>,
    pub container: Option<String>,
    pub streams_json: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    pub id: Id,
    #[serde(skip)]
    pub blob_id: BlobId,
    pub kind: ArtKind,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Watch state of one media file.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    pub watched: bool,
    pub last_watched_at: Option<String>,
}

impl Progress {
    /// 0..=1 for progress bars; 0 when the duration is unknown.
    pub fn fraction(&self) -> f64 {
        match self.duration_ms {
            Some(d) if d > 0 => (self.position_ms as f64 / d as f64).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovieMeta {
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub runtime_min: Option<u32>,
    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub tmdb_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Movie {
    pub id: Id,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub runtime_min: Option<u32>,
    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub tmdb_id: Option<i64>,
    pub poster_id: Option<Id>,
    pub backdrop_id: Option<Id>,
    pub media_file_id: Id,
    pub audio_lang_override: Option<String>,
    pub subtitle_lang_override: Option<String>,
    pub added_at: String,
    pub progress: Progress,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowMeta {
    pub title: String,
    pub original_title: Option<String>,
    pub first_year: Option<u16>,
    pub last_year: Option<u16>,
    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub tmdb_id: Option<i64>,
    pub tvmaze_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Show {
    pub id: Id,
    pub title: String,
    pub original_title: Option<String>,
    pub first_year: Option<u16>,
    pub last_year: Option<u16>,
    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub tmdb_id: Option<i64>,
    pub tvmaze_id: Option<i64>,
    pub poster_id: Option<Id>,
    pub backdrop_id: Option<Id>,
    pub audio_lang_override: Option<String>,
    pub subtitle_lang_override: Option<String>,
    pub added_at: String,
    pub season_count: u32,
    pub episode_count: u32,
    pub watched_count: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    pub id: Id,
    pub show_id: Id,
    pub number: u32,
    pub title: Option<String>,
    pub poster_id: Option<Id>,
    pub episode_count: u32,
    pub watched_count: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeMeta {
    pub number: u32,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub runtime_min: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub id: Id,
    pub season_id: Id,
    pub show_id: Id,
    pub season_number: u32,
    pub number: u32,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub runtime_min: Option<u32>,
    pub thumb_id: Option<Id>,
    pub media_file_id: Id,
    pub progress: Progress,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleFile {
    pub id: Id,
    pub media_file_id: Id,
    #[serde(skip)]
    pub blob_id: BlobId,
    /// Set when an RTL-fixed copy exists: the untouched original.
    #[serde(skip)]
    pub original_blob_id: Option<BlobId>,
    pub has_rtl_copy: bool,
    pub language: Option<String>,
    pub format: String,
    pub rtl_fix: bool,
    pub delay_ms: i64,
}

#[derive(Clone, Debug)]
pub struct NewSubtitle {
    pub media_file_id: Id,
    pub blob_id: BlobId,
    pub original_blob_id: Option<BlobId>,
    pub language: Option<String>,
    pub format: String,
    pub rtl_fix: bool,
}

/// A row on the Home screen or in a grid: one movie, or one episode of a show.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum WatchItem {
    Movie {
        movie: Movie,
    },
    Episode {
        episode: Episode,
        show_id: Id,
        show_title: String,
        show_backdrop_id: Option<Id>,
        show_poster_id: Option<Id>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchFilter {
    #[default]
    All,
    Unwatched,
    InProgress,
    Watched,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    #[default]
    DateAdded,
    Title,
    Year,
    LastWatched,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub shows: Vec<Show>,
    pub movies: Vec<Movie>,
    pub episodes: Vec<Episode>,
}
