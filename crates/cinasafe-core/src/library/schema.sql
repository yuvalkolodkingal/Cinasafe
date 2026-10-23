-- Cinasafe library schema v1 (brief section 8.1).
-- Deviations from the brief: show.tvmaze_id (TV metadata without a TMDB key),
-- media_file.watch columns live in `progress` as specified, plus the FTS table.

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE media_file (
  id INTEGER PRIMARY KEY,
  blob_id BLOB NOT NULL UNIQUE,
  original_name TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  duration_ms INTEGER,
  container TEXT,
  streams_json TEXT,
  added_at TEXT NOT NULL
);

CREATE TABLE artwork (
  id INTEGER PRIMARY KEY,
  blob_id BLOB NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('poster','backdrop','thumb')),
  width INTEGER, height INTEGER
);

CREATE TABLE movie (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL, original_title TEXT, year INTEGER,
  runtime_min INTEGER, overview TEXT, genres TEXT,
  tmdb_id INTEGER,
  poster_id INTEGER REFERENCES artwork(id),
  backdrop_id INTEGER REFERENCES artwork(id),
  media_file_id INTEGER NOT NULL REFERENCES media_file(id),
  audio_lang_override TEXT, subtitle_lang_override TEXT,
  added_at TEXT NOT NULL
);

CREATE TABLE show (
  id INTEGER PRIMARY KEY,
  title TEXT NOT NULL, original_title TEXT, first_year INTEGER, last_year INTEGER,
  overview TEXT, genres TEXT, tmdb_id INTEGER,
  tvmaze_id INTEGER,
  poster_id INTEGER REFERENCES artwork(id),
  backdrop_id INTEGER REFERENCES artwork(id),
  audio_lang_override TEXT, subtitle_lang_override TEXT,
  added_at TEXT NOT NULL
);

CREATE TABLE season (
  id INTEGER PRIMARY KEY,
  show_id INTEGER NOT NULL REFERENCES show(id) ON DELETE CASCADE,
  number INTEGER NOT NULL,
  title TEXT, poster_id INTEGER REFERENCES artwork(id),
  UNIQUE (show_id, number)
);

CREATE TABLE episode (
  id INTEGER PRIMARY KEY,
  season_id INTEGER NOT NULL REFERENCES season(id) ON DELETE CASCADE,
  number INTEGER NOT NULL,
  title TEXT, overview TEXT, air_date TEXT, runtime_min INTEGER,
  thumb_id INTEGER REFERENCES artwork(id),
  media_file_id INTEGER NOT NULL REFERENCES media_file(id),
  UNIQUE (season_id, number)
);

CREATE TABLE subtitle_file (
  id INTEGER PRIMARY KEY,
  media_file_id INTEGER NOT NULL REFERENCES media_file(id) ON DELETE CASCADE,
  blob_id BLOB NOT NULL UNIQUE,
  original_blob_id BLOB,
  language TEXT, format TEXT NOT NULL,
  rtl_fix INTEGER NOT NULL DEFAULT 0,
  delay_ms INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE progress (
  media_file_id INTEGER PRIMARY KEY REFERENCES media_file(id) ON DELETE CASCADE,
  position_ms INTEGER NOT NULL,
  watched INTEGER NOT NULL DEFAULT 0,
  last_watched_at TEXT NOT NULL
);

CREATE INDEX idx_movie_title ON movie(title);
CREATE INDEX idx_show_title ON show(title);
CREATE INDEX idx_episode_media ON episode(media_file_id);
CREATE INDEX idx_movie_media ON movie(media_file_id);

-- Search over movie, show and episode titles (brief 8.1).
CREATE VIRTUAL TABLE search_index USING fts5(
  kind UNINDEXED, ref_id UNINDEXED, title,
  tokenize = 'unicode61 remove_diacritics 2'
);
