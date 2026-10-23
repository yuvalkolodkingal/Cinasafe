//! The library: an SQLite database held only in memory while the vault is
//! unlocked, and stored on the stick as an encrypted blob (brief 4.5, 8.1).

pub mod model;

use std::io::Read;

use rusqlite::{params, Connection, OptionalExtension, Row};
use zeroize::Zeroizing;

use crate::BlobId;
pub use model::*;

pub const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = include_str!("schema.sql");
/// Fraction of a file that counts as watched (brief 6.4).
pub const WATCHED_AT: f64 = 0.9;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("library database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("library schema {0} is newer than this app supports")]
    TooNew(i64),
    #[error("not found")]
    NotFound,
}

pub type Result<T> = std::result::Result<T, LibraryError>;

pub struct Library {
    conn: Connection,
    /// Bumped on every change; a save records the generation it wrote.
    generation: u64,
    saved_generation: u64,
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("0 ns is valid")
        .format(&time::format_description::well_known::Rfc3339)
        .expect("RFC 3339 formatting of a UTC time cannot fail")
}

fn blob(row: &Row<'_>, idx: usize) -> rusqlite::Result<BlobId> {
    let v: Vec<u8> = row.get(idx)?;
    let arr: [u8; 16] = v.as_slice().try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Blob, "blob id is not 16 bytes".into())
    })?;
    Ok(BlobId(arr))
}

fn genres_json(g: &[String]) -> String {
    serde_json::to_string(g).unwrap_or_else(|_| "[]".into())
}

fn genres_parse(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Turns free text into a safe FTS5 prefix query: `"foo"* "bar"*`.
fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

const MOVIE_COLS: &str = "m.id, m.title, m.original_title, m.year, m.runtime_min, m.overview, m.genres, m.tmdb_id, \
     m.poster_id, m.backdrop_id, m.media_file_id, m.audio_lang_override, m.subtitle_lang_override, m.added_at, \
     p.position_ms, f.duration_ms, p.watched, p.last_watched_at";
const MOVIE_FROM: &str =
    "FROM movie m JOIN media_file f ON f.id = m.media_file_id LEFT JOIN progress p ON p.media_file_id = m.media_file_id";

fn movie_row(r: &Row<'_>) -> rusqlite::Result<Movie> {
    Ok(Movie {
        id: r.get(0)?,
        title: r.get(1)?,
        original_title: r.get(2)?,
        year: r.get(3)?,
        runtime_min: r.get(4)?,
        overview: r.get(5)?,
        genres: genres_parse(r.get(6)?),
        tmdb_id: r.get(7)?,
        poster_id: r.get(8)?,
        backdrop_id: r.get(9)?,
        media_file_id: r.get(10)?,
        audio_lang_override: r.get(11)?,
        subtitle_lang_override: r.get(12)?,
        added_at: r.get(13)?,
        progress: Progress {
            position_ms: r.get::<_, Option<i64>>(14)?.unwrap_or(0),
            duration_ms: r.get(15)?,
            watched: r.get::<_, Option<bool>>(16)?.unwrap_or(false),
            last_watched_at: r.get(17)?,
        },
    })
}

const EPISODE_COLS: &str = "e.id, e.season_id, s.show_id, s.number, e.number, e.title, e.overview, e.air_date, \
     e.runtime_min, e.thumb_id, e.media_file_id, p.position_ms, f.duration_ms, p.watched, p.last_watched_at";
const EPISODE_FROM: &str =
    "FROM episode e JOIN season s ON s.id = e.season_id JOIN media_file f ON f.id = e.media_file_id \
     LEFT JOIN progress p ON p.media_file_id = e.media_file_id";

fn episode_row(r: &Row<'_>) -> rusqlite::Result<Episode> {
    Ok(Episode {
        id: r.get(0)?,
        season_id: r.get(1)?,
        show_id: r.get(2)?,
        season_number: r.get(3)?,
        number: r.get(4)?,
        title: r.get(5)?,
        overview: r.get(6)?,
        air_date: r.get(7)?,
        runtime_min: r.get(8)?,
        thumb_id: r.get(9)?,
        media_file_id: r.get(10)?,
        progress: Progress {
            position_ms: r.get::<_, Option<i64>>(11)?.unwrap_or(0),
            duration_ms: r.get(12)?,
            watched: r.get::<_, Option<bool>>(13)?.unwrap_or(false),
            last_watched_at: r.get(14)?,
        },
    })
}

const SHOW_COLS: &str = "sh.id, sh.title, sh.original_title, sh.first_year, sh.last_year, sh.overview, sh.genres, \
     sh.tmdb_id, sh.tvmaze_id, sh.poster_id, sh.backdrop_id, sh.audio_lang_override, sh.subtitle_lang_override, sh.added_at, \
     (SELECT COUNT(*) FROM season s WHERE s.show_id = sh.id), \
     (SELECT COUNT(*) FROM episode e JOIN season s ON s.id = e.season_id WHERE s.show_id = sh.id), \
     (SELECT COUNT(*) FROM episode e JOIN season s ON s.id = e.season_id JOIN progress p ON p.media_file_id = e.media_file_id \
        WHERE s.show_id = sh.id AND p.watched = 1)";

fn show_row(r: &Row<'_>) -> rusqlite::Result<Show> {
    Ok(Show {
        id: r.get(0)?,
        title: r.get(1)?,
        original_title: r.get(2)?,
        first_year: r.get(3)?,
        last_year: r.get(4)?,
        overview: r.get(5)?,
        genres: genres_parse(r.get(6)?),
        tmdb_id: r.get(7)?,
        tvmaze_id: r.get(8)?,
        poster_id: r.get(9)?,
        backdrop_id: r.get(10)?,
        audio_lang_override: r.get(11)?,
        subtitle_lang_override: r.get(12)?,
        added_at: r.get(13)?,
        season_count: r.get(14)?,
        episode_count: r.get(15)?,
        watched_count: r.get(16)?,
    })
}

fn watch_clause(f: WatchFilter) -> &'static str {
    match f {
        WatchFilter::All => "1",
        WatchFilter::Unwatched => "(p.watched IS NULL OR p.watched = 0) AND COALESCE(p.position_ms, 0) = 0",
        WatchFilter::InProgress => "COALESCE(p.watched, 0) = 0 AND COALESCE(p.position_ms, 0) > 0",
        WatchFilter::Watched => "p.watched = 1",
    }
}

impl Library {
    /// A new, empty library.
    pub fn new() -> Result<Library> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute("INSERT INTO meta (key, value) VALUES ('schema_version', ?1)", [SCHEMA_VERSION.to_string()])?;
        Ok(Library { conn, generation: 1, saved_generation: 0 })
    }

    /// Loads a serialised database of `len` bytes from `r` (a decrypting blob reader).
    pub fn load(r: impl Read, len: usize) -> Result<Library> {
        let mut conn = Connection::open_in_memory()?;
        conn.deserialize_read_exact(rusqlite::MAIN_DB, r, len, false)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let v: i64 = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get::<_, String>(0))
            .map(|s| s.parse().unwrap_or(0))?;
        if v > SCHEMA_VERSION {
            return Err(LibraryError::TooNew(v));
        }
        Ok(Library { conn, generation: 0, saved_generation: 0 })
    }

    /// The whole database as bytes, for encrypting onto the stick.
    pub fn serialize(&self) -> Result<Zeroizing<Vec<u8>>> {
        let data = self.conn.serialize(rusqlite::MAIN_DB)?;
        Ok(Zeroizing::new(data.to_vec()))
    }

    pub fn is_dirty(&self) -> bool {
        self.generation != self.saved_generation
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Records that the state as of `generation` is on disk. Changes made
    /// since (while the save was writing) keep the library dirty.
    pub fn mark_saved(&mut self, generation: u64) {
        self.saved_generation = self.saved_generation.max(generation);
    }

    fn changed(&mut self) {
        self.generation += 1;
    }

    // ---- settings ---------------------------------------------------------

    pub fn settings(&self) -> Result<serde_json::Value> {
        let s: Option<String> =
            self.conn.query_row("SELECT value FROM meta WHERE key = 'settings'", [], |r| r.get(0)).optional()?;
        Ok(s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| serde_json::json!({})))
    }

    pub fn set_settings(&mut self, v: &serde_json::Value) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('settings', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [v.to_string()],
        )?;
        self.changed();
        Ok(())
    }

    // ---- media files and artwork -----------------------------------------

    pub fn add_media_file(&mut self, blob_id: BlobId, f: &NewMediaFile) -> Result<Id> {
        self.conn.execute(
            "INSERT INTO media_file (blob_id, original_name, size_bytes, duration_ms, container, streams_json, added_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![&blob_id.0[..], f.original_name, f.size_bytes as i64, f.duration_ms, f.container, f.streams_json, now()],
        )?;
        self.changed();
        Ok(self.conn.last_insert_rowid())
    }

    fn media_row(r: &Row<'_>) -> rusqlite::Result<MediaFile> {
        Ok(MediaFile {
            id: r.get(0)?,
            blob_id: blob(r, 1)?,
            original_name: r.get(2)?,
            size_bytes: r.get::<_, i64>(3)? as u64,
            duration_ms: r.get(4)?,
            container: r.get(5)?,
            streams_json: r.get(6)?,
            added_at: r.get(7)?,
        })
    }

    pub fn media_files(&self) -> Result<Vec<MediaFile>> {
        let mut st = self.conn.prepare(
            "SELECT id, blob_id, original_name, size_bytes, duration_ms, container, streams_json, added_at FROM media_file ORDER BY id",
        )?;
        let rows = st.query_map([], Self::media_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn media_file(&self, id: Id) -> Result<MediaFile> {
        self.conn
            .query_row(
                "SELECT id, blob_id, original_name, size_bytes, duration_ms, container, streams_json, added_at FROM media_file WHERE id = ?1",
                [id],
                Self::media_row,
            )
            .optional()?
            .ok_or(LibraryError::NotFound)
    }

    /// A media file with this original name and size (import de-duplication).
    pub fn find_media(&self, original_name: &str, size_bytes: u64) -> Result<Option<Id>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM media_file WHERE original_name = ?1 AND size_bytes = ?2",
                params![original_name, size_bytes as i64],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_media_info(
        &mut self,
        id: Id,
        duration_ms: Option<i64>,
        container: Option<&str>,
        streams_json: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE media_file SET duration_ms = COALESCE(?2, duration_ms), container = COALESCE(?3, container), \
             streams_json = COALESCE(?4, streams_json) WHERE id = ?1",
            params![id, duration_ms, container, streams_json],
        )?;
        self.changed();
        Ok(())
    }

    pub fn add_artwork(
        &mut self,
        blob_id: BlobId,
        kind: ArtKind,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<Id> {
        self.conn.execute(
            "INSERT INTO artwork (blob_id, kind, width, height) VALUES (?1, ?2, ?3, ?4)",
            params![&blob_id.0[..], kind.as_str(), width, height],
        )?;
        self.changed();
        Ok(self.conn.last_insert_rowid())
    }

    pub fn artwork(&self, id: Id) -> Result<Artwork> {
        self.conn
            .query_row("SELECT id, blob_id, kind, width, height FROM artwork WHERE id = ?1", [id], |r| {
                Ok(Artwork {
                    id: r.get(0)?,
                    blob_id: blob(r, 1)?,
                    kind: ArtKind::parse(&r.get::<_, String>(2)?).unwrap_or(ArtKind::Poster),
                    width: r.get(3)?,
                    height: r.get(4)?,
                })
            })
            .optional()?
            .ok_or(LibraryError::NotFound)
    }

    /// Every blob the library points at (media, artwork, subtitles and their originals).
    pub fn referenced_blobs(&self) -> Result<Vec<BlobId>> {
        let mut st = self.conn.prepare(
            "SELECT blob_id FROM media_file UNION ALL SELECT blob_id FROM artwork \
             UNION ALL SELECT blob_id FROM subtitle_file UNION ALL SELECT original_blob_id FROM subtitle_file WHERE original_blob_id IS NOT NULL",
        )?;
        let rows = st.query_map([], |r| blob(r, 0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- movies ------------------------------------------------------------

    pub fn add_movie(
        &mut self,
        media_file_id: Id,
        m: &MovieMeta,
        poster: Option<Id>,
        backdrop: Option<Id>,
    ) -> Result<Id> {
        self.conn.execute(
            "INSERT INTO movie (title, original_title, year, runtime_min, overview, genres, tmdb_id, poster_id, backdrop_id, media_file_id, added_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![m.title, m.original_title, m.year, m.runtime_min, m.overview, genres_json(&m.genres), m.tmdb_id, poster, backdrop, media_file_id, now()],
        )?;
        let id = self.conn.last_insert_rowid();
        self.index("movie", id, &m.title, m.original_title.as_deref())?;
        self.changed();
        Ok(id)
    }

    pub fn update_movie(&mut self, id: Id, m: &MovieMeta) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE movie SET title = ?2, original_title = ?3, year = ?4, runtime_min = ?5, overview = ?6, genres = ?7, tmdb_id = ?8 WHERE id = ?1",
            params![id, m.title, m.original_title, m.year, m.runtime_min, m.overview, genres_json(&m.genres), m.tmdb_id],
        )?;
        if n == 0 {
            return Err(LibraryError::NotFound);
        }
        self.index("movie", id, &m.title, m.original_title.as_deref())?;
        self.changed();
        Ok(())
    }

    pub fn set_movie_artwork(&mut self, id: Id, poster: Option<Id>, backdrop: Option<Id>) -> Result<()> {
        self.conn.execute(
            "UPDATE movie SET poster_id = COALESCE(?2, poster_id), backdrop_id = COALESCE(?3, backdrop_id) WHERE id = ?1",
            params![id, poster, backdrop],
        )?;
        self.changed();
        Ok(())
    }

    pub fn set_movie_overrides(&mut self, id: Id, audio: Option<&str>, subtitles: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE movie SET audio_lang_override = ?2, subtitle_lang_override = ?3 WHERE id = ?1",
            params![id, audio, subtitles],
        )?;
        self.changed();
        Ok(())
    }

    pub fn movie(&self, id: Id) -> Result<Movie> {
        self.conn
            .query_row(&format!("SELECT {MOVIE_COLS} {MOVIE_FROM} WHERE m.id = ?1"), [id], movie_row)
            .optional()?
            .ok_or(LibraryError::NotFound)
    }

    pub fn movie_by_media(&self, media_file_id: Id) -> Result<Option<Movie>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {MOVIE_COLS} {MOVIE_FROM} WHERE m.media_file_id = ?1"),
                [media_file_id],
                movie_row,
            )
            .optional()?)
    }

    pub fn movies(&self, filter: WatchFilter, genre: Option<&str>, sort: SortBy) -> Result<Vec<Movie>> {
        let order = match sort {
            SortBy::DateAdded => "m.added_at DESC, m.id DESC",
            SortBy::Title => "m.title COLLATE NOCASE",
            SortBy::Year => "m.year DESC, m.title COLLATE NOCASE",
            SortBy::LastWatched => "p.last_watched_at IS NULL, p.last_watched_at DESC",
        };
        let sql = format!(
            "SELECT {MOVIE_COLS} {MOVIE_FROM} WHERE {} AND (?1 IS NULL OR EXISTS (SELECT 1 FROM json_each(m.genres) WHERE value = ?1)) ORDER BY {order}",
            watch_clause(filter)
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([genre], movie_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- shows, seasons, episodes -------------------------------------------

    /// Finds a show by provider id, then by exact title; creates it otherwise.
    pub fn upsert_show(&mut self, m: &ShowMeta) -> Result<Id> {
        let existing: Option<Id> = self
            .conn
            .query_row(
                "SELECT id FROM show WHERE (?1 IS NOT NULL AND tmdb_id = ?1) OR (?2 IS NOT NULL AND tvmaze_id = ?2) \
                 OR (?1 IS NULL AND ?2 IS NULL AND title = ?3) LIMIT 1",
                params![m.tmdb_id, m.tvmaze_id, m.title],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO show (title, original_title, first_year, last_year, overview, genres, tmdb_id, tvmaze_id, added_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![m.title, m.original_title, m.first_year, m.last_year, m.overview, genres_json(&m.genres), m.tmdb_id, m.tvmaze_id, now()],
        )?;
        let id = self.conn.last_insert_rowid();
        self.index("show", id, &m.title, m.original_title.as_deref())?;
        self.changed();
        Ok(id)
    }

    pub fn update_show(&mut self, id: Id, m: &ShowMeta) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE show SET title = ?2, original_title = ?3, first_year = ?4, last_year = ?5, overview = ?6, genres = ?7, \
             tmdb_id = ?8, tvmaze_id = ?9 WHERE id = ?1",
            params![id, m.title, m.original_title, m.first_year, m.last_year, m.overview, genres_json(&m.genres), m.tmdb_id, m.tvmaze_id],
        )?;
        if n == 0 {
            return Err(LibraryError::NotFound);
        }
        self.index("show", id, &m.title, m.original_title.as_deref())?;
        self.changed();
        Ok(())
    }

    pub fn set_show_artwork(&mut self, id: Id, poster: Option<Id>, backdrop: Option<Id>) -> Result<()> {
        self.conn.execute(
            "UPDATE show SET poster_id = COALESCE(?2, poster_id), backdrop_id = COALESCE(?3, backdrop_id) WHERE id = ?1",
            params![id, poster, backdrop],
        )?;
        self.changed();
        Ok(())
    }

    pub fn set_show_overrides(&mut self, id: Id, audio: Option<&str>, subtitles: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE show SET audio_lang_override = ?2, subtitle_lang_override = ?3 WHERE id = ?1",
            params![id, audio, subtitles],
        )?;
        self.changed();
        Ok(())
    }

    pub fn upsert_season(&mut self, show_id: Id, number: u32, title: Option<&str>, poster: Option<Id>) -> Result<Id> {
        self.conn.execute(
            "INSERT INTO season (show_id, number, title, poster_id) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(show_id, number) DO UPDATE SET title = COALESCE(excluded.title, title), \
             poster_id = COALESCE(excluded.poster_id, poster_id)",
            params![show_id, number, title, poster],
        )?;
        self.changed();
        Ok(self.conn.query_row(
            "SELECT id FROM season WHERE show_id = ?1 AND number = ?2",
            params![show_id, number],
            |r| r.get(0),
        )?)
    }

    /// Adds an episode. If the slot is already taken (a second file for the
    /// same episode), the new file takes it over; metadata already there is kept
    /// unless the new values are set.
    pub fn add_episode(&mut self, season_id: Id, media_file_id: Id, e: &EpisodeMeta, thumb: Option<Id>) -> Result<Id> {
        self.conn.execute(
            "INSERT INTO episode (season_id, number, title, overview, air_date, runtime_min, thumb_id, media_file_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(season_id, number) DO UPDATE SET media_file_id = excluded.media_file_id, \
             title = COALESCE(excluded.title, title), overview = COALESCE(excluded.overview, overview), \
             air_date = COALESCE(excluded.air_date, air_date), runtime_min = COALESCE(excluded.runtime_min, runtime_min), \
             thumb_id = COALESCE(excluded.thumb_id, thumb_id)",
            params![season_id, e.number, e.title, e.overview, e.air_date, e.runtime_min, thumb, media_file_id],
        )?;
        let id: Id = self.conn.query_row(
            "SELECT id FROM episode WHERE season_id = ?1 AND number = ?2",
            params![season_id, e.number],
            |r| r.get(0),
        )?;
        if let Some(t) = &e.title {
            self.index("episode", id, t, None)?;
        }
        self.changed();
        Ok(id)
    }

    pub fn set_episode_thumb(&mut self, id: Id, thumb: Id) -> Result<()> {
        self.conn.execute("UPDATE episode SET thumb_id = ?2 WHERE id = ?1", params![id, thumb])?;
        self.changed();
        Ok(())
    }

    pub fn set_season(&mut self, id: Id, title: Option<&str>, poster: Option<Id>) -> Result<()> {
        self.conn.execute(
            "UPDATE season SET title = COALESCE(?2, title), poster_id = COALESCE(?3, poster_id) WHERE id = ?1",
            params![id, title, poster],
        )?;
        self.changed();
        Ok(())
    }

    /// Largest episode number used in a season (0 when empty).
    pub fn max_episode_number(&self, season_id: Id) -> Result<u32> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(number), 0) FROM episode WHERE season_id = ?1",
            [season_id],
            |r| r.get(0),
        )?)
    }

    pub fn update_episode(&mut self, id: Id, e: &EpisodeMeta) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE episode SET title = ?2, overview = ?3, air_date = ?4, runtime_min = ?5 WHERE id = ?1",
            params![id, e.title, e.overview, e.air_date, e.runtime_min],
        )?;
        if n == 0 {
            return Err(LibraryError::NotFound);
        }
        self.index("episode", id, e.title.as_deref().unwrap_or(""), None)?;
        self.changed();
        Ok(())
    }

    pub fn show(&self, id: Id) -> Result<Show> {
        self.conn
            .query_row(&format!("SELECT {SHOW_COLS} FROM show sh WHERE sh.id = ?1"), [id], show_row)
            .optional()?
            .ok_or(LibraryError::NotFound)
    }

    pub fn shows(&self, filter: WatchFilter, genre: Option<&str>, sort: SortBy) -> Result<Vec<Show>> {
        // The outer query sees the inner columns by name (no `sh.` prefix).
        let order = match sort {
            SortBy::DateAdded => "added_at DESC, id DESC",
            SortBy::Title => "title COLLATE NOCASE",
            SortBy::Year => "first_year DESC, title COLLATE NOCASE",
            SortBy::LastWatched => "last_watched IS NULL, last_watched DESC",
        };
        // "Watched" when every episode is; "in progress" when some are, or one is started.
        let keep = match filter {
            WatchFilter::All => "1",
            WatchFilter::Unwatched => "watched_n = 0 AND started_n = 0",
            WatchFilter::InProgress => "(watched_n > 0 OR started_n > 0) AND watched_n < episode_n",
            WatchFilter::Watched => "episode_n > 0 AND watched_n = episode_n",
        };
        let sql = format!(
            "SELECT * FROM (SELECT {SHOW_COLS}, \
               (SELECT COUNT(*) FROM episode e JOIN season s ON s.id = e.season_id WHERE s.show_id = sh.id) AS episode_n, \
               (SELECT COUNT(*) FROM episode e JOIN season s ON s.id = e.season_id JOIN progress p ON p.media_file_id = e.media_file_id \
                  WHERE s.show_id = sh.id AND p.watched = 1) AS watched_n, \
               (SELECT COUNT(*) FROM episode e JOIN season s ON s.id = e.season_id JOIN progress p ON p.media_file_id = e.media_file_id \
                  WHERE s.show_id = sh.id AND p.watched = 0 AND p.position_ms > 0) AS started_n, \
               (SELECT MAX(p.last_watched_at) FROM episode e JOIN season s ON s.id = e.season_id \
                  JOIN progress p ON p.media_file_id = e.media_file_id WHERE s.show_id = sh.id) AS last_watched \
             FROM show sh WHERE (?1 IS NULL OR EXISTS (SELECT 1 FROM json_each(sh.genres) WHERE value = ?1))) \
             WHERE {keep} ORDER BY {order}"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([genre], show_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn seasons(&self, show_id: Id) -> Result<Vec<Season>> {
        let mut st = self.conn.prepare(
            "SELECT s.id, s.show_id, s.number, s.title, s.poster_id, \
               (SELECT COUNT(*) FROM episode e WHERE e.season_id = s.id), \
               (SELECT COUNT(*) FROM episode e JOIN progress p ON p.media_file_id = e.media_file_id WHERE e.season_id = s.id AND p.watched = 1) \
             FROM season s WHERE s.show_id = ?1 ORDER BY s.number = 0, s.number",
        )?;
        let rows = st
            .query_map([show_id], |r| {
                Ok(Season {
                    id: r.get(0)?,
                    show_id: r.get(1)?,
                    number: r.get(2)?,
                    title: r.get(3)?,
                    poster_id: r.get(4)?,
                    episode_count: r.get(5)?,
                    watched_count: r.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn episodes(&self, season_id: Id) -> Result<Vec<Episode>> {
        let mut st = self
            .conn
            .prepare(&format!("SELECT {EPISODE_COLS} {EPISODE_FROM} WHERE e.season_id = ?1 ORDER BY e.number"))?;
        let rows = st.query_map([season_id], episode_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn episode(&self, id: Id) -> Result<Episode> {
        self.conn
            .query_row(&format!("SELECT {EPISODE_COLS} {EPISODE_FROM} WHERE e.id = ?1"), [id], episode_row)
            .optional()?
            .ok_or(LibraryError::NotFound)
    }

    pub fn episodes_by_media(&self, media_file_id: Id) -> Result<Vec<Episode>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {EPISODE_COLS} {EPISODE_FROM} WHERE e.media_file_id = ?1 ORDER BY s.number, e.number"
        ))?;
        let rows = st.query_map([media_file_id], episode_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The episode after `episode_id` in airing order (crossing into the next season).
    pub fn next_episode(&self, episode_id: Id) -> Result<Option<Episode>> {
        let cur = self.episode(episode_id)?;
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {EPISODE_COLS} {EPISODE_FROM} WHERE s.show_id = ?1 AND s.number > 0 AND \
                     (s.number > ?2 OR (s.number = ?2 AND e.number > ?3)) AND e.media_file_id != ?4 \
                     ORDER BY s.number, e.number LIMIT 1"
                ),
                params![cur.show_id, cur.season_number, cur.number, cur.media_file_id],
                episode_row,
            )
            .optional()?)
    }

    // ---- subtitles -----------------------------------------------------------

    fn subtitle_row(r: &Row<'_>) -> rusqlite::Result<SubtitleFile> {
        let original: Option<Vec<u8>> = r.get(3)?;
        let original_blob_id = original.and_then(|v| v.as_slice().try_into().ok().map(BlobId));
        Ok(SubtitleFile {
            id: r.get(0)?,
            media_file_id: r.get(1)?,
            blob_id: blob(r, 2)?,
            has_rtl_copy: original_blob_id.is_some(),
            original_blob_id,
            language: r.get(4)?,
            format: r.get(5)?,
            rtl_fix: r.get(6)?,
            delay_ms: r.get(7)?,
        })
    }

    pub fn add_subtitle(&mut self, s: &NewSubtitle) -> Result<Id> {
        self.conn.execute(
            "INSERT INTO subtitle_file (media_file_id, blob_id, original_blob_id, language, format, rtl_fix) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                s.media_file_id,
                &s.blob_id.0[..],
                s.original_blob_id.map(|b| b.0.to_vec()),
                s.language,
                s.format,
                s.rtl_fix
            ],
        )?;
        self.changed();
        Ok(self.conn.last_insert_rowid())
    }

    pub fn subtitles(&self, media_file_id: Id) -> Result<Vec<SubtitleFile>> {
        let mut st = self.conn.prepare(
            "SELECT id, media_file_id, blob_id, original_blob_id, language, format, rtl_fix, delay_ms FROM subtitle_file \
             WHERE media_file_id = ?1 ORDER BY id",
        )?;
        let rows = st.query_map([media_file_id], Self::subtitle_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn subtitle(&self, id: Id) -> Result<SubtitleFile> {
        self.conn
            .query_row(
                "SELECT id, media_file_id, blob_id, original_blob_id, language, format, rtl_fix, delay_ms FROM subtitle_file WHERE id = ?1",
                [id],
                Self::subtitle_row,
            )
            .optional()?
            .ok_or(LibraryError::NotFound)
    }

    /// Which blob to play for a subtitle: the RTL-fixed copy when the fix is on.
    pub fn subtitle_blob_to_play(s: &SubtitleFile) -> BlobId {
        match (s.rtl_fix, s.original_blob_id) {
            (true, _) | (false, None) => s.blob_id,
            (false, Some(orig)) => orig,
        }
    }

    pub fn set_subtitle_rtl_fix(&mut self, id: Id, on: bool) -> Result<()> {
        self.conn.execute("UPDATE subtitle_file SET rtl_fix = ?2 WHERE id = ?1", params![id, on])?;
        self.changed();
        Ok(())
    }

    /// Attaches an RTL-fixed copy: `fixed` becomes the main blob, the current one the original.
    pub fn set_subtitle_rtl_copy(&mut self, id: Id, fixed: BlobId) -> Result<()> {
        self.conn.execute(
            "UPDATE subtitle_file SET original_blob_id = COALESCE(original_blob_id, blob_id), blob_id = ?2, rtl_fix = 1 WHERE id = ?1",
            params![id, &fixed.0[..]],
        )?;
        self.changed();
        Ok(())
    }

    pub fn set_subtitle_delay(&mut self, id: Id, delay_ms: i64) -> Result<()> {
        self.conn.execute("UPDATE subtitle_file SET delay_ms = ?2 WHERE id = ?1", params![id, delay_ms])?;
        self.changed();
        Ok(())
    }

    // ---- progress -------------------------------------------------------------

    pub fn progress(&self, media_file_id: Id) -> Result<Progress> {
        self.conn
            .query_row(
                "SELECT p.position_ms, f.duration_ms, p.watched, p.last_watched_at FROM media_file f \
                 LEFT JOIN progress p ON p.media_file_id = f.id WHERE f.id = ?1",
                [media_file_id],
                |r| {
                    Ok(Progress {
                        position_ms: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
                        duration_ms: r.get(1)?,
                        watched: r.get::<_, Option<bool>>(2)?.unwrap_or(false),
                        last_watched_at: r.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or(LibraryError::NotFound)
    }

    /// Records a playback position. Marks the file watched at 90% (and keeps
    /// it watched); the position resets to 0 once watched so "Resume" restarts.
    pub fn record_position(
        &mut self,
        media_file_id: Id,
        position_ms: i64,
        duration_ms: Option<i64>,
    ) -> Result<Progress> {
        if let Some(d) = duration_ms.filter(|d| *d > 0) {
            self.conn.execute(
                "UPDATE media_file SET duration_ms = ?2 WHERE id = ?1 AND (duration_ms IS NULL OR duration_ms != ?2)",
                params![media_file_id, d],
            )?;
        }
        let duration = self.media_file(media_file_id)?.duration_ms;
        let reached_end = duration.is_some_and(|d| d > 0 && position_ms as f64 >= d as f64 * WATCHED_AT);
        let (pos, watched) = if reached_end { (0, true) } else { (position_ms.max(0), false) };
        self.conn.execute(
            "INSERT INTO progress (media_file_id, position_ms, watched, last_watched_at) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(media_file_id) DO UPDATE SET position_ms = excluded.position_ms, \
             watched = MAX(watched, excluded.watched), last_watched_at = excluded.last_watched_at",
            params![media_file_id, pos, watched, now()],
        )?;
        self.changed();
        self.progress(media_file_id)
    }

    pub fn mark_watched(&mut self, media_file_id: Id, watched: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO progress (media_file_id, position_ms, watched, last_watched_at) VALUES (?1, 0, ?2, ?3) \
             ON CONFLICT(media_file_id) DO UPDATE SET position_ms = 0, watched = excluded.watched, last_watched_at = excluded.last_watched_at",
            params![media_file_id, watched, now()],
        )?;
        self.changed();
        Ok(())
    }

    pub fn mark_season_watched(&mut self, season_id: Id, watched: bool) -> Result<()> {
        let ids: Vec<Id> = {
            let mut st = self.conn.prepare("SELECT media_file_id FROM episode WHERE season_id = ?1")?;
            let v = st.query_map([season_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        for id in ids {
            self.mark_watched(id, watched)?;
        }
        Ok(())
    }

    // ---- home rows ------------------------------------------------------------

    fn watch_item_for_media(&self, media_file_id: Id) -> Result<Option<WatchItem>> {
        if let Some(movie) = self.movie_by_media(media_file_id)? {
            return Ok(Some(WatchItem::Movie { movie }));
        }
        if let Some(episode) = self.episodes_by_media(media_file_id)?.into_iter().next() {
            let show = self.show(episode.show_id)?;
            return Ok(Some(WatchItem::Episode {
                show_id: show.id,
                show_title: show.title,
                show_backdrop_id: show.backdrop_id,
                show_poster_id: show.poster_id,
                episode,
            }));
        }
        Ok(None)
    }

    /// Started but not finished, most recent first.
    pub fn continue_watching(&self, limit: usize) -> Result<Vec<WatchItem>> {
        let ids: Vec<Id> = {
            let mut st = self.conn.prepare(
                "SELECT media_file_id FROM progress WHERE watched = 0 AND position_ms > 0 ORDER BY last_watched_at DESC LIMIT ?1",
            )?;
            let v = st.query_map([limit as i64], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.watch_item_for_media(id)?);
        }
        Ok(out)
    }

    /// For each show watched recently, the first unwatched episode after the
    /// last one watched.
    pub fn up_next(&self, limit: usize) -> Result<Vec<WatchItem>> {
        let last: Vec<Id> = {
            let mut st = self.conn.prepare(
                "SELECT e.id FROM episode e JOIN season s ON s.id = e.season_id JOIN progress p ON p.media_file_id = e.media_file_id \
                 WHERE p.watched = 1 AND p.last_watched_at = (SELECT MAX(p2.last_watched_at) FROM episode e2 \
                   JOIN season s2 ON s2.id = e2.season_id JOIN progress p2 ON p2.media_file_id = e2.media_file_id \
                   WHERE s2.show_id = s.show_id AND p2.watched = 1) \
                 GROUP BY s.show_id ORDER BY p.last_watched_at DESC LIMIT ?1",
            )?;
            let v = st.query_map([limit as i64], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        let mut out = Vec::new();
        for ep in last {
            let mut cur = ep;
            while let Some(next) = self.next_episode(cur)? {
                if !next.progress.watched {
                    if next.progress.position_ms == 0 {
                        out.extend(self.watch_item_for_media(next.media_file_id)?);
                    }
                    break;
                }
                cur = next.id;
            }
        }
        Ok(out)
    }

    /// Movies and shows by date added, newest first.
    pub fn recently_added(&self, limit: usize) -> Result<(Vec<Movie>, Vec<Show>)> {
        let mut movies = self.movies(WatchFilter::All, None, SortBy::DateAdded)?;
        movies.truncate(limit);
        let mut shows = self.shows(WatchFilter::All, None, SortBy::DateAdded)?;
        shows.truncate(limit);
        Ok((movies, shows))
    }

    // ---- search -----------------------------------------------------------------

    fn index(&mut self, kind: &str, id: Id, title: &str, alt: Option<&str>) -> Result<()> {
        self.conn.execute("DELETE FROM search_index WHERE kind = ?1 AND ref_id = ?2", params![kind, id])?;
        let text = match alt {
            Some(a) if a != title => format!("{title} {a}"),
            _ => title.to_owned(),
        };
        self.conn
            .execute("INSERT INTO search_index (kind, ref_id, title) VALUES (?1, ?2, ?3)", params![kind, id, text])?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<SearchResults> {
        let mut out = SearchResults::default();
        let Some(q) = fts_query(query) else { return Ok(out) };
        let hits: Vec<(String, Id)> = {
            let mut st = self
                .conn
                .prepare("SELECT kind, ref_id FROM search_index WHERE search_index MATCH ?1 ORDER BY rank LIMIT ?2")?;
            let v = st
                .query_map(params![q, (limit * 3) as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            v
        };
        for (kind, id) in hits {
            match kind.as_str() {
                "movie" if out.movies.len() < limit => out.movies.extend(self.movie(id).ok()),
                "show" if out.shows.len() < limit => out.shows.extend(self.show(id).ok()),
                "episode" if out.episodes.len() < limit => out.episodes.extend(self.episode(id).ok()),
                _ => {}
            }
        }
        Ok(out)
    }

    // ---- removal ------------------------------------------------------------------

    /// Removes a media file and everything that hangs off it (movie or episode
    /// rows, subtitles, progress, artwork no longer used, empty seasons and
    /// shows). Returns the blobs the caller must delete from the stick.
    pub fn remove_media_file(&mut self, media_file_id: Id) -> Result<Vec<BlobId>> {
        let mf = self.media_file(media_file_id)?;
        let mut blobs = vec![mf.blob_id];
        for s in self.subtitles(media_file_id)? {
            blobs.push(s.blob_id);
            blobs.extend(s.original_blob_id);
        }
        let tx = self.conn.transaction()?;
        let movie_ids: Vec<Id> = {
            let mut st = tx.prepare("SELECT id FROM movie WHERE media_file_id = ?1")?;
            let v = st.query_map([media_file_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        let episode_ids: Vec<Id> = {
            let mut st = tx.prepare("SELECT id FROM episode WHERE media_file_id = ?1")?;
            let v = st.query_map([media_file_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        for id in &movie_ids {
            tx.execute("DELETE FROM search_index WHERE kind = 'movie' AND ref_id = ?1", [id])?;
        }
        for id in &episode_ids {
            tx.execute("DELETE FROM search_index WHERE kind = 'episode' AND ref_id = ?1", [id])?;
        }
        tx.execute("DELETE FROM movie WHERE media_file_id = ?1", [media_file_id])?;
        tx.execute("DELETE FROM episode WHERE media_file_id = ?1", [media_file_id])?;
        tx.execute("DELETE FROM subtitle_file WHERE media_file_id = ?1", [media_file_id])?;
        tx.execute("DELETE FROM progress WHERE media_file_id = ?1", [media_file_id])?;
        tx.execute("DELETE FROM media_file WHERE id = ?1", [media_file_id])?;
        tx.execute("DELETE FROM season WHERE NOT EXISTS (SELECT 1 FROM episode e WHERE e.season_id = season.id)", [])?;
        let empty_shows: Vec<Id> = {
            let mut st =
                tx.prepare("SELECT id FROM show WHERE NOT EXISTS (SELECT 1 FROM season s WHERE s.show_id = show.id)")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        for id in &empty_shows {
            tx.execute("DELETE FROM search_index WHERE kind = 'show' AND ref_id = ?1", [id])?;
            tx.execute("DELETE FROM show WHERE id = ?1", [id])?;
        }
        // Artwork nobody points at any more.
        let orphan_art: Vec<(Id, BlobId)> = {
            let mut st = tx.prepare(
                "SELECT id, blob_id FROM artwork a WHERE \
                 NOT EXISTS (SELECT 1 FROM movie WHERE poster_id = a.id OR backdrop_id = a.id) AND \
                 NOT EXISTS (SELECT 1 FROM show WHERE poster_id = a.id OR backdrop_id = a.id) AND \
                 NOT EXISTS (SELECT 1 FROM season WHERE poster_id = a.id) AND \
                 NOT EXISTS (SELECT 1 FROM episode WHERE thumb_id = a.id)",
            )?;
            let v = st.query_map([], |r| Ok((r.get(0)?, blob(r, 1)?)))?.collect::<rusqlite::Result<_>>()?;
            v
        };
        for (id, b) in orphan_art {
            tx.execute("DELETE FROM artwork WHERE id = ?1", [id])?;
            blobs.push(b);
        }
        tx.commit()?;
        self.changed();
        Ok(blobs)
    }

    /// All genres in use, for the grid's genre filter.
    pub fn genres(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT DISTINCT value FROM (SELECT j.value FROM movie, json_each(movie.genres) j \
             UNION SELECT j.value FROM show, json_each(show.genres) j) ORDER BY value COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    /// Totals for grid headers ("48 movies · 312 GB").
    pub fn totals(&self) -> Result<(u32, u64, u32, u64)> {
        Ok(self.conn.query_row(
            "SELECT (SELECT COUNT(*) FROM movie), \
               (SELECT COALESCE(SUM(f.size_bytes), 0) FROM movie m JOIN media_file f ON f.id = m.media_file_id), \
               (SELECT COUNT(*) FROM show), \
               (SELECT COALESCE(SUM(f.size_bytes), 0) FROM media_file f WHERE EXISTS (SELECT 1 FROM episode e WHERE e.media_file_id = f.id))",
            [],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64, r.get(2)?, r.get::<_, i64>(3)? as u64)),
        )?)
    }
}

#[cfg(test)]
mod tests;
