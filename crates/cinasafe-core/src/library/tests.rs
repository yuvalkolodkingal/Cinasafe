use super::*;

fn b(n: u8) -> BlobId {
    BlobId([n; 16])
}

fn media(lib: &mut Library, n: u8, name: &str, dur_ms: i64) -> Id {
    let f = NewMediaFile {
        original_name: name.into(),
        size_bytes: 1_000_000 * n as u64,
        duration_ms: Some(dur_ms),
        ..Default::default()
    };
    lib.add_media_file(b(n), &f).unwrap()
}

fn movie(lib: &mut Library, n: u8, title: &str, year: u16, genres: &[&str]) -> Id {
    let mf = media(lib, n, &format!("{title}.mkv"), 6_000_000);
    let meta = MovieMeta {
        title: title.into(),
        year: Some(year),
        genres: genres.iter().map(|g| g.to_string()).collect(),
        ..Default::default()
    };
    lib.add_movie(mf, &meta, None, None).unwrap()
}

/// A show with `seasons` seasons of `eps` episodes; returns (show, [episode ids in order]).
fn show(lib: &mut Library, first_blob: u8, title: &str, seasons: u32, eps: u32) -> (Id, Vec<Id>) {
    let sid = lib.upsert_show(&ShowMeta { title: title.into(), first_year: Some(2015), ..Default::default() }).unwrap();
    let mut ids = Vec::new();
    let mut n = first_blob;
    for s in 1..=seasons {
        let season = lib.upsert_season(sid, s, None, None).unwrap();
        for e in 1..=eps {
            let mf = media(lib, n, &format!("{title}.S{s:02}E{e:02}.mkv"), 2_700_000);
            n += 1;
            let meta = EpisodeMeta { number: e, title: Some(format!("{title} {s}x{e}")), ..Default::default() };
            ids.push(lib.add_episode(season, mf, &meta, None).unwrap());
        }
    }
    (sid, ids)
}

#[test]
fn serialize_roundtrip() {
    let mut lib = Library::new().unwrap();
    movie(&mut lib, 1, "Inception", 2010, &["Sci-fi"]);
    lib.set_settings(&serde_json::json!({"language": "he"})).unwrap();
    let bytes = lib.serialize().unwrap();
    let back = Library::load(&bytes[..], bytes.len()).unwrap();
    assert!(!back.is_dirty());
    assert_eq!(back.movies(WatchFilter::All, None, SortBy::Title).unwrap()[0].title, "Inception");
    assert_eq!(back.settings().unwrap()["language"], "he");
    // Search index survives too.
    assert_eq!(back.search("incep", 10).unwrap().movies.len(), 1);
}

#[test]
fn rejects_newer_schema() {
    let lib = Library::new().unwrap();
    lib.conn.execute("UPDATE meta SET value = '99' WHERE key = 'schema_version'", []).unwrap();
    let bytes = lib.serialize().unwrap();
    assert!(matches!(Library::load(&bytes[..], bytes.len()), Err(LibraryError::TooNew(99))));
}

#[test]
fn movies_filter_and_sort() {
    let mut lib = Library::new().unwrap();
    let a = movie(&mut lib, 1, "Arrival", 2016, &["Sci-fi", "Drama"]);
    let z = movie(&mut lib, 2, "Zodiac", 2007, &["Thriller"]);
    movie(&mut lib, 3, "Memento", 2000, &["Thriller"]);
    let titles = |v: Vec<Movie>| v.into_iter().map(|m| m.title).collect::<Vec<_>>();

    assert_eq!(titles(lib.movies(WatchFilter::All, None, SortBy::Title).unwrap()), ["Arrival", "Memento", "Zodiac"]);
    assert_eq!(titles(lib.movies(WatchFilter::All, None, SortBy::Year).unwrap()), ["Arrival", "Zodiac", "Memento"]);
    assert_eq!(titles(lib.movies(WatchFilter::All, Some("Thriller"), SortBy::Title).unwrap()), ["Memento", "Zodiac"]);

    let za = lib.movie(z).unwrap().media_file_id;
    lib.record_position(za, 60_000, None).unwrap();
    let aa = lib.movie(a).unwrap().media_file_id;
    lib.mark_watched(aa, true).unwrap();
    assert_eq!(titles(lib.movies(WatchFilter::InProgress, None, SortBy::Title).unwrap()), ["Zodiac"]);
    assert_eq!(titles(lib.movies(WatchFilter::Watched, None, SortBy::Title).unwrap()), ["Arrival"]);
    assert_eq!(titles(lib.movies(WatchFilter::Unwatched, None, SortBy::Title).unwrap()), ["Memento"]);
    assert_eq!(lib.genres().unwrap(), ["Drama", "Sci-fi", "Thriller"]);
    let (movies, movie_bytes, shows, _) = lib.totals().unwrap();
    assert_eq!((movies, movie_bytes, shows), (3, 6_000_000, 0));
}

#[test]
fn progress_marks_watched_at_ninety_percent() {
    let mut lib = Library::new().unwrap();
    let m = movie(&mut lib, 1, "Heat", 1995, &[]);
    let mf = lib.movie(m).unwrap().media_file_id;
    let p = lib.record_position(mf, 3_000_000, None).unwrap();
    assert!(!p.watched);
    assert!((p.fraction() - 0.5).abs() < 1e-9);
    let p = lib.record_position(mf, 5_500_000, None).unwrap();
    assert!(p.watched, "{p:?}");
    assert_eq!(p.position_ms, 0, "watched resets the resume point");
    // Seeking back later doesn't un-watch it.
    let p = lib.record_position(mf, 1_000, None).unwrap();
    assert!(p.watched);
    // The player's duration fills a missing one.
    let other = media(&mut lib, 9, "x.mkv", 0);
    lib.conn.execute("UPDATE media_file SET duration_ms = NULL WHERE id = ?1", [other]).unwrap();
    assert!(lib.record_position(other, 950, Some(1000)).unwrap().watched);
}

#[test]
fn shows_seasons_and_up_next() {
    let mut lib = Library::new().unwrap();
    let (sid, eps) = show(&mut lib, 10, "Fauda", 2, 3);
    let s = lib.show(sid).unwrap();
    assert_eq!((s.season_count, s.episode_count, s.watched_count), (2, 6, 0));
    assert_eq!(lib.seasons(sid).unwrap().len(), 2);

    // Watch S1E1..E3: up next is S2E1 (crosses the season).
    for id in &eps[..3] {
        let mf = lib.episode(*id).unwrap().media_file_id;
        lib.mark_watched(mf, true).unwrap();
    }
    let next = lib.up_next(5).unwrap();
    match &next[..] {
        [WatchItem::Episode { episode, show_title, .. }] => {
            assert_eq!((episode.season_number, episode.number), (2, 1));
            assert_eq!(show_title, "Fauda");
        }
        other => panic!("{other:?}"),
    }
    let titles = |v: Vec<Show>| v.into_iter().map(|s| s.title).collect::<Vec<_>>();
    assert_eq!(titles(lib.shows(WatchFilter::InProgress, None, SortBy::Title).unwrap()), ["Fauda"]);
    assert!(lib.shows(WatchFilter::Watched, None, SortBy::Title).unwrap().is_empty());

    // Starting S2E1 moves it to Continue watching and out of Up next.
    let mf = lib.episode(eps[3]).unwrap().media_file_id;
    lib.record_position(mf, 60_000, None).unwrap();
    assert!(lib.up_next(5).unwrap().is_empty());
    assert_eq!(lib.continue_watching(5).unwrap().len(), 1);

    lib.mark_season_watched(lib.seasons(sid).unwrap()[1].id, true).unwrap();
    assert_eq!(titles(lib.shows(WatchFilter::Watched, None, SortBy::Title).unwrap()), ["Fauda"]);
}

#[test]
fn upsert_show_matches_provider_ids_then_title() {
    let mut lib = Library::new().unwrap();
    let a = lib.upsert_show(&ShowMeta { title: "Fauda".into(), tvmaze_id: Some(15315), ..Default::default() }).unwrap();
    let again =
        lib.upsert_show(&ShowMeta { title: "פאודה".into(), tvmaze_id: Some(15315), ..Default::default() }).unwrap();
    assert_eq!(a, again);
    let plain = lib.upsert_show(&ShowMeta { title: "Shtisel".into(), ..Default::default() }).unwrap();
    assert_eq!(plain, lib.upsert_show(&ShowMeta { title: "Shtisel".into(), ..Default::default() }).unwrap());
    assert_ne!(a, plain);
}

#[test]
fn search_finds_hebrew_and_prefixes() {
    let mut lib = Library::new().unwrap();
    movie(&mut lib, 1, "ואלס עם באשיר", 2008, &[]);
    let (_, _) = show(&mut lib, 10, "Breaking Bad", 1, 2);
    let r = lib.search("ואלס", 10).unwrap();
    assert_eq!(r.movies.len(), 1);
    let r = lib.search("break", 10).unwrap();
    assert_eq!((r.shows.len(), r.episodes.len()), (1, 2));
    assert!(lib.search("\" OR *", 10).unwrap().movies.is_empty(), "query syntax is neutralised");
    assert!(lib.search("", 10).unwrap().shows.is_empty());
}

#[test]
fn subtitle_rtl_copy_switches_blob() {
    let mut lib = Library::new().unwrap();
    let mf = media(&mut lib, 1, "x.mkv", 1000);
    let id = lib
        .add_subtitle(&NewSubtitle {
            media_file_id: mf,
            blob_id: b(50),
            original_blob_id: None,
            language: Some("he".into()),
            format: "srt".into(),
            rtl_fix: false,
        })
        .unwrap();
    assert_eq!(Library::subtitle_blob_to_play(&lib.subtitle(id).unwrap()), b(50));
    lib.set_subtitle_rtl_copy(id, b(51)).unwrap();
    let s = lib.subtitle(id).unwrap();
    assert!(s.rtl_fix && s.has_rtl_copy);
    assert_eq!(Library::subtitle_blob_to_play(&s), b(51));
    lib.set_subtitle_rtl_fix(id, false).unwrap();
    assert_eq!(Library::subtitle_blob_to_play(&lib.subtitle(id).unwrap()), b(50));
    lib.set_subtitle_delay(id, -300).unwrap();
    assert_eq!(lib.subtitle(id).unwrap().delay_ms, -300);
}

#[test]
fn removal_cascades_and_reports_blobs() {
    let mut lib = Library::new().unwrap();
    let (sid, eps) = show(&mut lib, 10, "Shtisel", 1, 2);
    let poster = lib.add_artwork(b(90), ArtKind::Poster, Some(500), Some(750)).unwrap();
    lib.set_show_artwork(sid, Some(poster), None).unwrap();
    let mf0 = lib.episode(eps[0]).unwrap().media_file_id;
    lib.add_subtitle(&NewSubtitle {
        media_file_id: mf0,
        blob_id: b(60),
        original_blob_id: Some(b(61)),
        language: None,
        format: "srt".into(),
        rtl_fix: true,
    })
    .unwrap();

    let gone = lib.remove_media_file(mf0).unwrap();
    assert_eq!(gone, [b(10), b(60), b(61)]);
    assert_eq!(lib.show(sid).unwrap().episode_count, 1);

    let mf1 = lib.episode(eps[1]).unwrap().media_file_id;
    let gone = lib.remove_media_file(mf1).unwrap();
    // Last episode: the show, its season and its poster go too.
    assert_eq!(gone, [b(11), b(90)]);
    assert!(matches!(lib.show(sid), Err(LibraryError::NotFound)));
    assert!(lib.search("shtisel", 10).unwrap().shows.is_empty());
    assert!(lib.referenced_blobs().unwrap().is_empty());
}

#[test]
fn fts_query_quotes_terms() {
    assert_eq!(fts_query("Breaking bad").as_deref(), Some("\"Breaking\"* \"bad\"*"));
    assert_eq!(fts_query("  -- ").as_deref(), None);
}

#[test]
fn changes_during_a_save_keep_it_dirty() {
    let mut lib = Library::new().unwrap();
    let snapshot = lib.generation();
    lib.mark_saved(snapshot);
    assert!(!lib.is_dirty());
    let during = lib.generation();
    movie(&mut lib, 1, "Late", 2020, &[]);
    lib.mark_saved(during);
    assert!(lib.is_dirty(), "a change made while saving must not be marked saved");
}
