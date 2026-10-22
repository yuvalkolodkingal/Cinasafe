//! The Movie Database (TMDB) API v3: movies and TV.
//!
//! Needs a key: either a v3 API key (sent as `api_key`) or a v4 read access
//! token (sent as a Bearer token). TMDB's terms require attribution in the
//! app: the TMDB logo and "This product uses the TMDB API but is not endorsed
//! or certified by TMDB." (docs/METADATA.md).

use std::sync::Arc;

use serde::Deserialize;

use super::http::{encode, json, HttpClient};
use super::*;

const API: &str = "https://api.themoviedb.org/3";
pub const IMAGE_BASE: &str = "https://image.tmdb.org/t/p/";
const POSTER_SIZE: &str = "w500";
const BACKDROP_SIZE: &str = "w1280";
const STILL_SIZE: &str = "w300";
const MAX_JSON: usize = 4 << 20;

enum Auth {
    /// v4 read access token (a JWT).
    Bearer(String),
    /// v3 API key.
    ApiKey(String),
}

pub struct Tmdb {
    http: Arc<dyn HttpClient>,
    auth: Auth,
    /// e.g. `he-IL`, `ar-SA`, `en-US`.
    language: String,
    /// Used to fill overviews that have no translation in `language`.
    fallback_language: String,
}

impl Tmdb {
    pub fn new(http: Arc<dyn HttpClient>, key: &str, language: &str) -> Tmdb {
        let key = key.trim();
        let auth = if key.starts_with("eyJ") { Auth::Bearer(key.to_owned()) } else { Auth::ApiKey(key.to_owned()) };
        Tmdb { http, auth, language: language.to_owned(), fallback_language: "en-US".into() }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str, params: &[(&str, &str)]) -> Result<T> {
        let mut url = format!("{API}{path}?");
        for (i, (k, v)) in params.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str(&format!("{k}={}", encode(v)));
        }
        let bearer;
        let mut headers = vec![("Accept", "application/json")];
        match &self.auth {
            Auth::Bearer(t) => {
                bearer = format!("Bearer {t}");
                headers.push(("Authorization", bearer.as_str()));
            }
            Auth::ApiKey(k) => url.push_str(&format!("&api_key={}", encode(k))),
        }
        json(self.http.get(&url, &headers, MAX_JSON)?)
    }

    fn image(kind: ArtKind, size: &str, path: Option<String>) -> Option<ArtworkRef> {
        let path = non_empty(path)?;
        Some(ArtworkRef { kind, url: format!("{IMAGE_BASE}{size}{path}"), width: None, height: None })
    }

    fn wants_fallback(&self) -> bool {
        self.language != self.fallback_language
    }
}

#[derive(Deserialize)]
struct Page<T> {
    #[serde(default = "Vec::new")]
    results: Vec<T>,
}

#[derive(Deserialize)]
struct MovieHit {
    id: i64,
    #[serde(default)]
    title: String,
    original_title: Option<String>,
    release_date: Option<String>,
    overview: Option<String>,
    #[serde(default)]
    popularity: f64,
    poster_path: Option<String>,
}

#[derive(Deserialize)]
struct TvHit {
    id: i64,
    #[serde(default)]
    name: String,
    original_name: Option<String>,
    first_air_date: Option<String>,
    overview: Option<String>,
    #[serde(default)]
    popularity: f64,
    poster_path: Option<String>,
}

#[derive(Deserialize)]
struct Genre {
    name: String,
}

#[derive(Deserialize)]
struct Movie {
    id: i64,
    #[serde(default)]
    title: String,
    original_title: Option<String>,
    release_date: Option<String>,
    runtime: Option<u32>,
    overview: Option<String>,
    #[serde(default)]
    genres: Vec<Genre>,
    imdb_id: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
}

#[derive(Deserialize)]
struct TvSeasonStub {
    season_number: u32,
    name: Option<String>,
    poster_path: Option<String>,
}

#[derive(Deserialize)]
struct Tv {
    id: i64,
    #[serde(default)]
    name: String,
    original_name: Option<String>,
    first_air_date: Option<String>,
    last_air_date: Option<String>,
    overview: Option<String>,
    #[serde(default)]
    genres: Vec<Genre>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    #[serde(default)]
    seasons: Vec<TvSeasonStub>,
}

#[derive(Deserialize)]
struct TvEpisode {
    episode_number: u32,
    season_number: u32,
    name: Option<String>,
    overview: Option<String>,
    air_date: Option<String>,
    runtime: Option<u32>,
    still_path: Option<String>,
}

#[derive(Deserialize)]
struct TvSeason {
    name: Option<String>,
    poster_path: Option<String>,
    #[serde(default)]
    episodes: Vec<TvEpisode>,
}

impl Tmdb {
    fn season(&self, id: &str, stub: &TvSeasonStub) -> Result<SeasonDetails> {
        let path = format!("/tv/{id}/season/{}", stub.season_number);
        let s: TvSeason = self.get(&path, &[("language", &self.language)])?;
        let mut episodes: Vec<EpisodeDetails> = s
            .episodes
            .into_iter()
            .map(|e| EpisodeDetails {
                season: e.season_number,
                number: e.episode_number,
                title: non_empty(e.name),
                overview: non_empty(e.overview),
                air_date: non_empty(e.air_date),
                runtime_min: e.runtime,
                thumb: Self::image(ArtKind::Thumb, STILL_SIZE, e.still_path),
            })
            .collect();
        // One extra request per season when the translation lacks overviews.
        if self.wants_fallback() && episodes.iter().any(|e| e.overview.is_none()) {
            if let Ok(fb) = self.get::<TvSeason>(&path, &[("language", &self.fallback_language)]) {
                for e in &mut episodes {
                    if e.overview.is_none() {
                        e.overview = fb
                            .episodes
                            .iter()
                            .find(|f| f.episode_number == e.number)
                            .and_then(|f| non_empty(f.overview.clone()));
                    }
                }
            }
        }
        Ok(SeasonDetails {
            number: stub.season_number,
            title: non_empty(s.name).or_else(|| non_empty(stub.name.clone())),
            poster: Self::image(ArtKind::Poster, POSTER_SIZE, s.poster_path.or_else(|| stub.poster_path.clone())),
            episodes,
        })
    }
}

impl MetadataProvider for Tmdb {
    fn name(&self) -> &'static str {
        "tmdb"
    }

    fn search_movie(&self, title: &str, year: Option<u16>) -> Result<Vec<Candidate>> {
        let y = year.map(|y| y.to_string());
        let mut params = vec![("query", title), ("include_adult", "false"), ("language", &self.language)];
        if let Some(y) = &y {
            params.push(("year", y));
        }
        let page: Page<MovieHit> = self.get("/search/movie", &params)?;
        Ok(page
            .results
            .into_iter()
            .map(|h| Candidate {
                provider: "tmdb",
                id: h.id.to_string(),
                title: h.title,
                original_title: non_empty(h.original_title),
                year: year_of(h.release_date.as_deref()),
                overview: non_empty(h.overview),
                popularity: h.popularity,
                poster: Self::image(ArtKind::Poster, POSTER_SIZE, h.poster_path),
            })
            .collect())
    }

    fn search_show(&self, title: &str, year: Option<u16>) -> Result<Vec<Candidate>> {
        let y = year.map(|y| y.to_string());
        let mut params = vec![("query", title), ("include_adult", "false"), ("language", &self.language)];
        if let Some(y) = &y {
            params.push(("first_air_date_year", y));
        }
        let page: Page<TvHit> = self.get("/search/tv", &params)?;
        Ok(page
            .results
            .into_iter()
            .map(|h| Candidate {
                provider: "tmdb",
                id: h.id.to_string(),
                title: h.name,
                original_title: non_empty(h.original_name),
                year: year_of(h.first_air_date.as_deref()),
                overview: non_empty(h.overview),
                popularity: h.popularity,
                poster: Self::image(ArtKind::Poster, POSTER_SIZE, h.poster_path),
            })
            .collect())
    }

    fn movie(&self, id: &str) -> Result<MovieDetails> {
        let path = format!("/movie/{}", encode(id));
        let m: Movie = self.get(&path, &[("language", &self.language)])?;
        let mut d = MovieDetails {
            provider: "tmdb",
            id: m.id.to_string(),
            title: m.title,
            original_title: non_empty(m.original_title),
            year: year_of(m.release_date.as_deref()),
            runtime_min: m.runtime.filter(|r| *r > 0),
            overview: non_empty(m.overview),
            genres: m.genres.into_iter().map(|g| g.name).collect(),
            imdb_id: non_empty(m.imdb_id),
            poster: Self::image(ArtKind::Poster, POSTER_SIZE, m.poster_path),
            backdrop: Self::image(ArtKind::Backdrop, BACKDROP_SIZE, m.backdrop_path),
        };
        if d.overview.is_none() && self.wants_fallback() {
            if let Ok(fb) = self.get::<Movie>(&path, &[("language", &self.fallback_language)]) {
                d.overview = non_empty(fb.overview);
            }
        }
        Ok(d)
    }

    fn show(&self, id: &str, seasons: &[u32]) -> Result<ShowDetails> {
        let id = encode(id);
        let path = format!("/tv/{id}");
        let t: Tv = self.get(&path, &[("language", &self.language)])?;
        let mut overview = non_empty(t.overview);
        if overview.is_none() && self.wants_fallback() {
            if let Ok(fb) = self.get::<Tv>(&path, &[("language", &self.fallback_language)]) {
                overview = non_empty(fb.overview);
            }
        }
        let wanted = t.seasons.iter().filter(|s| seasons.is_empty() || seasons.contains(&s.season_number));
        let season_details = wanted.map(|s| self.season(&id, s)).collect::<Result<Vec<_>>>()?;
        Ok(ShowDetails {
            provider: "tmdb",
            id: t.id.to_string(),
            title: t.name,
            original_title: non_empty(t.original_name),
            first_year: year_of(t.first_air_date.as_deref()),
            last_year: year_of(t.last_air_date.as_deref()),
            overview,
            genres: t.genres.into_iter().map(|g| g.name).collect(),
            poster: Self::image(ArtKind::Poster, POSTER_SIZE, t.poster_path),
            backdrop: Self::image(ArtKind::Backdrop, BACKDROP_SIZE, t.backdrop_path),
            seasons: season_details,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::http::fake::FakeHttp;
    use super::*;

    fn tmdb(http: FakeHttp, key: &str) -> (Arc<FakeHttp>, Tmdb) {
        let http = Arc::new(http);
        let t = Tmdb::new(http.clone(), key, "he-IL");
        (http, t)
    }

    #[test]
    fn movie_search_and_details_with_overview_fallback() {
        let (http, t) = tmdb(
            FakeHttp::new()
                .route("/search/movie", "tmdb/search-movie-inception.json")
                .route("/movie/27205?language=he-IL", "tmdb/movie-27205-he.json")
                .route("/movie/27205?language=en-US", "tmdb/movie-27205-en.json"),
            "v3key",
        );
        let hits = t.search_movie("Inception", Some(2010)).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[1].id, "27205");
        assert_eq!(hits[1].year, Some(2010));
        assert_eq!(hits[1].poster.as_ref().unwrap().url, "https://image.tmdb.org/t/p/w500/p-inception.jpg");
        assert!(hits[2].poster.is_none());

        let m = t.movie("27205").unwrap();
        assert_eq!(m.title, "התחלה");
        assert_eq!(m.genres, ["אקשן", "מדע בדיוני"]);
        assert!(m.overview.unwrap().starts_with("Cobb"), "English overview fills the missing Hebrew one");
        assert_eq!(m.runtime_min, Some(148));
        assert_eq!(m.backdrop.unwrap().url, "https://image.tmdb.org/t/p/w1280/bd-inception.jpg");

        let urls = http.urls();
        assert!(urls[0].starts_with("https://api.themoviedb.org/3/search/movie?query=Inception&"));
        assert!(urls[0].contains("&year=2010") && urls[0].ends_with("&api_key=v3key"));
    }

    #[test]
    fn bearer_token_goes_in_header() {
        let (http, t) = tmdb(FakeHttp::new().route("/search/tv", "tmdb/search-tv-fauda.json"), "eyJhbGciOi.token");
        let hits = t.search_show("פאודה", None).unwrap();
        assert_eq!((hits[0].title.as_str(), hits[0].original_title.as_deref()), ("Fauda", Some("פאודה")));
        let reqs = http.requests.lock().unwrap();
        assert!(!reqs[0].0.contains("api_key"));
        assert_eq!(reqs[0].1["Authorization"], "Bearer eyJhbGciOi.token");
        assert!(reqs[0].0.contains("query=%D7%A4%D7%90%D7%95%D7%93%D7%94"));
    }

    #[test]
    fn show_fetches_only_requested_seasons() {
        let (http, t) = tmdb(
            FakeHttp::new()
                .route("/tv/64230?", "tmdb/tv-64230.json")
                .route("/tv/64230/season/1?", "tmdb/tv-64230-season-1.json"),
            "k",
        );
        let s = t.show("64230", &[1]).unwrap();
        assert_eq!((s.title.as_str(), s.first_year, s.last_year), ("פאודה", Some(2015), Some(2023)));
        assert_eq!(s.seasons.len(), 1);
        let e1 = s.episode(1, 1).unwrap();
        assert_eq!(e1.thumb.as_ref().unwrap().url, "https://image.tmdb.org/t/p/w300/still-s1e1.jpg");
        assert!(s.episode(1, 2).unwrap().thumb.is_none());
        assert!(!http.urls().iter().any(|u| u.contains("/season/2")));
    }

    #[test]
    fn bad_key_is_reported() {
        let (_, t) = tmdb(FakeHttp::new().status("/search/movie", 401, b"{}"), "bad");
        assert!(matches!(t.search_movie("x", None), Err(Error::Unauthorized)));
    }
}
