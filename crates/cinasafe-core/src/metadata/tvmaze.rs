//! TVmaze (https://www.tvmaze.com/api): TV only, no API key, English text.
//!
//! Data is CC BY-SA 4.0; the app must credit and link TVmaze. Rate limit is at
//! least 20 calls per 10 s per IP, so a whole show costs two calls here.

use std::sync::Arc;

use serde::Deserialize;

use super::http::{encode, json, HttpClient};
use super::*;

const API: &str = "https://api.tvmaze.com";
const MAX_JSON: usize = 16 << 20;

pub struct TvMaze {
    http: Arc<dyn HttpClient>,
}

impl TvMaze {
    pub fn new(http: Arc<dyn HttpClient>) -> TvMaze {
        TvMaze { http }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path_and_query: &str) -> Result<T> {
        json(self.http.get(&format!("{API}{path_and_query}"), &[("Accept", "application/json")], MAX_JSON)?)
    }
}

#[derive(Deserialize)]
struct Image {
    medium: Option<String>,
    original: Option<String>,
}

fn pick(img: Option<Image>, kind: ArtKind) -> Option<ArtworkRef> {
    let img = img?;
    let url = img.original.or(img.medium)?;
    Some(ArtworkRef { kind, url, width: None, height: None })
}

#[derive(Deserialize)]
struct Genre(String);

#[derive(Deserialize)]
struct Show {
    id: i64,
    #[serde(default)]
    name: String,
    premiered: Option<String>,
    ended: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    genres: Vec<Genre>,
    image: Option<Image>,
    #[serde(rename = "_embedded")]
    embedded: Option<Embedded>,
}

#[derive(Deserialize)]
struct Embedded {
    #[serde(default)]
    seasons: Vec<Season>,
    #[serde(default)]
    episodes: Vec<Episode>,
}

#[derive(Deserialize)]
struct Season {
    number: u32,
    name: Option<String>,
    image: Option<Image>,
}

#[derive(Deserialize)]
struct Episode {
    season: u32,
    number: Option<u32>,
    name: Option<String>,
    summary: Option<String>,
    airdate: Option<String>,
    runtime: Option<u32>,
    image: Option<Image>,
}

#[derive(Deserialize)]
struct Hit {
    score: f64,
    show: Show,
}

#[derive(Deserialize)]
struct Resolution {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize)]
struct Resolutions {
    original: Option<Resolution>,
}

#[derive(Deserialize)]
struct ShowImage {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    main: bool,
    resolutions: Resolutions,
}

/// TVmaze summaries are HTML fragments.
pub(crate) fn strip_html(s: Option<String>) -> Option<String> {
    let s = s?;
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    non_empty(Some(decoded.split_whitespace().collect::<Vec<_>>().join(" ")))
}

impl MetadataProvider for TvMaze {
    fn name(&self) -> &'static str {
        "tvmaze"
    }

    fn search_movie(&self, _: &str, _: Option<u16>) -> Result<Vec<Candidate>> {
        Err(Error::MoviesUnsupported("TVmaze"))
    }

    fn search_show(&self, title: &str, _year: Option<u16>) -> Result<Vec<Candidate>> {
        // TVmaze has no year filter; ranking uses the premiere year instead.
        let hits: Vec<Hit> = self.get(&format!("/search/shows?q={}", encode(title)))?;
        Ok(hits
            .into_iter()
            .map(|h| Candidate {
                provider: "tvmaze",
                id: h.show.id.to_string(),
                title: h.show.name,
                original_title: None,
                year: year_of(h.show.premiered.as_deref()),
                overview: strip_html(h.show.summary),
                popularity: h.score,
                poster: pick(h.show.image, ArtKind::Poster),
            })
            .collect())
    }

    fn movie(&self, _: &str) -> Result<MovieDetails> {
        Err(Error::MoviesUnsupported("TVmaze"))
    }

    fn show(&self, id: &str, seasons: &[u32]) -> Result<ShowDetails> {
        let id = encode(id);
        let s: Show = self.get(&format!("/shows/{id}?embed%5B%5D=seasons&embed%5B%5D=episodes"))?;
        // Backdrops only come from the images endpoint; a failure there isn't fatal.
        let backdrop = self.get::<Vec<ShowImage>>(&format!("/shows/{id}/images")).ok().and_then(|imgs| {
            let mut bg: Vec<&ShowImage> = imgs.iter().filter(|i| i.kind.as_deref() == Some("background")).collect();
            bg.sort_by_key(|i| !i.main);
            bg.first().and_then(|i| i.resolutions.original.as_ref()).map(|r| ArtworkRef {
                kind: ArtKind::Backdrop,
                url: r.url.clone(),
                width: r.width,
                height: r.height,
            })
        });

        let embedded = s.embedded.unwrap_or(Embedded { seasons: Vec::new(), episodes: Vec::new() });
        let mut season_details: Vec<SeasonDetails> = embedded
            .seasons
            .into_iter()
            .filter(|se| seasons.is_empty() || seasons.contains(&se.number))
            .map(|se| SeasonDetails {
                number: se.number,
                title: non_empty(se.name),
                poster: pick(se.image, ArtKind::Poster),
                episodes: Vec::new(),
            })
            .collect();
        for e in embedded.episodes {
            // Specials have no number in TVmaze's main list; they can't be matched by number.
            let Some(number) = e.number else { continue };
            if let Some(season) = season_details.iter_mut().find(|se| se.number == e.season) {
                season.episodes.push(EpisodeDetails {
                    season: e.season,
                    number,
                    title: non_empty(e.name),
                    overview: strip_html(e.summary),
                    air_date: non_empty(e.airdate),
                    runtime_min: e.runtime,
                    thumb: pick(e.image, ArtKind::Thumb),
                });
            }
        }

        Ok(ShowDetails {
            provider: "tvmaze",
            id: s.id.to_string(),
            title: s.name,
            original_title: None,
            first_year: year_of(s.premiered.as_deref()),
            last_year: year_of(s.ended.as_deref()),
            overview: strip_html(s.summary),
            genres: s.genres.into_iter().map(|g| g.0).collect(),
            poster: pick(s.image, ArtKind::Poster),
            backdrop,
            seasons: season_details,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::http::fake::FakeHttp;
    use super::*;

    fn maze() -> TvMaze {
        TvMaze::new(Arc::new(
            FakeHttp::new()
                .route("/search/shows", "tvmaze/search-fauda.json")
                .route("/shows/15315?", "tvmaze/show-15315.json")
                .route("/shows/15315/images", "tvmaze/images-15315.json"),
        ))
    }

    #[test]
    fn search_and_show() {
        let m = maze();
        let hits = m.search_show("Fauda", None).unwrap();
        assert_eq!((hits[0].id.as_str(), hits[0].title.as_str(), hits[0].year), ("15315", "Fauda", Some(2015)));
        assert!(hits[0].overview.as_deref().is_some_and(|o| !o.contains('<')));

        let s = m.show("15315", &[1]).unwrap();
        assert_eq!(s.seasons.len(), 1);
        assert_eq!(s.seasons[0].episodes.len(), 12);
        let e = s.episode(1, 1).unwrap();
        assert_eq!(e.air_date.as_deref(), Some("2015-02-15"));
        assert!(e.overview.as_deref().unwrap().starts_with("Discovering that Abu Ahmad"));
        assert!(e.thumb.is_some());
        assert!(s.poster.is_some());
        assert!(s.genres.contains(&"Drama".to_string()));
        assert!(m.search_movie("x", None).is_err());
    }

    #[test]
    fn html_is_stripped() {
        assert_eq!(
            strip_html(Some("<p>Tom &amp; Jerry&#39;s <b>show</b></p><p><br />&nbsp;</p>".into())).as_deref(),
            Some("Tom & Jerry's show")
        );
        assert_eq!(strip_html(Some("<p></p>".into())), None);
    }
}
