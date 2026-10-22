//! Live checks against the real services. Ignored by default (they need the
//! network and, for TMDB, a key):
//!
//!     cargo test -p cinasafe-core --test live_metadata -- --ignored
//!     CINASAFE_TMDB_KEY=... cargo test -p cinasafe-core --test live_metadata -- --ignored

use std::sync::Arc;

use cinasafe_core::metadata::identify::episodes_for;
use cinasafe_core::metadata::{
    fetch_artwork, identify_movie, identify_show, tmdb::Tmdb, tvmaze::TvMaze, ReqwestClient,
};

#[test]
#[ignore = "network"]
fn tvmaze_fauda() {
    let http = Arc::new(ReqwestClient::new().unwrap());
    let p = TvMaze::new(http.clone());
    let r = identify_show(&p, &["Fauda"], None, &[1]).unwrap();
    assert!(r.confident, "{:?}", r.candidates);
    let show = r.details.unwrap();
    assert_eq!(show.first_year, Some(2015));
    assert!(!episodes_for(&show, Some(1), &[1], None).is_empty());
    let art = fetch_artwork(http.as_ref(), show.poster.as_ref().unwrap()).unwrap();
    assert!(art.bytes.len() > 10_000, "{art:?}");
}

#[test]
#[ignore = "network and CINASAFE_TMDB_KEY"]
fn tmdb_inception_in_hebrew() {
    let Ok(key) = std::env::var("CINASAFE_TMDB_KEY") else {
        eprintln!("CINASAFE_TMDB_KEY not set; skipping");
        return;
    };
    let http = Arc::new(ReqwestClient::new().unwrap());
    let p = Tmdb::new(http.clone(), &key, "he-IL");
    let r = identify_movie(&p, "Inception", Some(2010)).unwrap();
    assert!(r.confident);
    let d = r.details.unwrap();
    assert_eq!(d.id, "27205");
    assert!(d.overview.is_some());
    fetch_artwork(http.as_ref(), d.poster.as_ref().unwrap()).unwrap();
    let s = identify_show(&p, &["פאודה"], None, &[1]).unwrap();
    assert!(s.details.unwrap().seasons[0].episodes.len() >= 10);
}
