//! Shapes the UI asks for (app/ui/src/api/types.ts), built from library rows.

use cinasafe_core::library::{Episode, Id, Library, MediaFile, Movie, Season, Show, SubtitleFile, WatchItem};
use cinasafe_core::Vault;
use serde::Serialize;
use serde_json::Value;

use crate::error::Result;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub id: i64,
    pub kind: String,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub codec: Option<String>,
    pub external: bool,
    pub default: bool,
    pub forced: bool,
    pub subtitle_file_id: Option<Id>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub hdr: bool,
    pub channels: Option<i64>,
}

/// Parses mpv's `track-list` (as JSON).
pub fn parse_tracks(json: &str) -> Vec<TrackInfo> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    list.iter().filter_map(track_from_node).collect()
}

pub fn track_from_node(t: &Value) -> Option<TrackInfo> {
    let s = |k: &str| t.get(k).and_then(Value::as_str).map(str::to_owned);
    let b = |k: &str| t.get(k).and_then(Value::as_bool).unwrap_or(false);
    let i = |k: &str| t.get(k).and_then(Value::as_i64);
    let kind = s("type")?;
    let colormatrix = s("demux-colormatrix").unwrap_or_default();
    Some(TrackInfo {
        id: i("id")?,
        lang: s("lang"),
        title: s("title"),
        codec: s("codec"),
        external: b("external"),
        default: b("default"),
        forced: b("forced"),
        subtitle_file_id: None,
        width: i("demux-w"),
        height: i("demux-h"),
        hdr: colormatrix.contains("2020") || s("demux-gamma").is_some_and(|g| g == "pq" || g == "hlg"),
        channels: i("demux-channel-count"),
        kind,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFileInfo {
    pub id: Id,
    pub original_name: String,
    pub size_bytes: u64,
    pub encrypted_bytes: u64,
    pub duration_ms: Option<i64>,
    pub container: Option<String>,
    pub tracks: Vec<TrackInfo>,
    pub subtitles: Vec<SubtitleFile>,
    pub added_at: String,
}

pub fn media_info(vault: &Vault, lib: &Library, f: MediaFile) -> Result<MediaFileInfo> {
    let encrypted_bytes = std::fs::metadata(vault.blob_path(f.blob_id)).map(|m| m.len()).unwrap_or(0);
    Ok(MediaFileInfo {
        id: f.id,
        subtitles: lib.subtitles(f.id)?,
        tracks: f.streams_json.as_deref().map(parse_tracks).unwrap_or_default(),
        original_name: f.original_name,
        size_bytes: f.size_bytes,
        encrypted_bytes,
        duration_ms: f.duration_ms,
        container: f.container,
        added_at: f.added_at,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RecentItem {
    Movie { movie: Movie },
    Show { show: Show },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    pub hero: Option<WatchItem>,
    pub continue_watching: Vec<WatchItem>,
    pub up_next: Vec<WatchItem>,
    pub recently_added: Vec<RecentItem>,
    pub movies: Vec<Movie>,
    pub shows: Vec<Show>,
    pub empty: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Grid<T> {
    pub items: Vec<T>,
    pub total_count: u32,
    pub total_bytes: u64,
    pub genres: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MovieDetail {
    pub movie: Movie,
    pub file: MediaFileInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowDetail {
    pub show: Show,
    pub seasons: Vec<Season>,
    pub resume: Option<Episode>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonDetail {
    pub season: Season,
    pub episodes: Vec<Episode>,
    pub up_next_episode_id: Option<Id>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeDetail {
    pub episode: Episode,
    pub show_title: String,
    pub file: MediaFileInfo,
}

/// The episode to resume for a show: one in progress (most recent), else
/// the first unwatched after the last watched one, else the first.
pub fn resume_episode(lib: &Library, show_id: Id) -> Result<Option<Episode>> {
    let mut all = Vec::new();
    for s in lib.seasons(show_id)? {
        if s.number == 0 {
            continue;
        }
        all.extend(lib.episodes(s.id)?);
    }
    if let Some(e) = all
        .iter()
        .filter(|e| !e.progress.watched && e.progress.position_ms > 0)
        .max_by(|a, b| a.progress.last_watched_at.cmp(&b.progress.last_watched_at))
    {
        return Ok(Some(e.clone()));
    }
    let last_watched = all.iter().rposition(|e| e.progress.watched);
    let start = last_watched.map_or(0, |i| i + 1);
    Ok(all.get(start).or(all.iter().find(|e| !e.progress.watched)).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mpv_track_list() {
        let json = r#"[{"id":1,"type":"video","codec":"hevc","demux-w":3840,"demux-h":2160,"demux-colormatrix":"bt.2020-ncl","default":true},
            {"id":1,"type":"audio","lang":"heb","codec":"aac","demux-channel-count":6},
            {"id":2,"type":"sub","lang":"eng","title":"SDH","external":false,"forced":false},
            {"type":"sub"}]"#;
        let t = parse_tracks(json);
        assert_eq!(t.len(), 3);
        assert!(t[0].hdr && t[0].width == Some(3840));
        assert_eq!(t[1].channels, Some(6));
        assert_eq!(t[2].title.as_deref(), Some("SDH"));
        assert!(parse_tracks("not json").is_empty());
    }
}
