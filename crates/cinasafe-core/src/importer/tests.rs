use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::crypto::KdfParams;
use crate::library::{SortBy, WatchFilter};
use crate::metadata::{Candidate, EpisodeDetails, HttpResponse, SeasonDetails};

fn fast() -> KdfParams {
    KdfParams { m_kib: 64, t: 1, p: 1, salt: [7; 16] }
}

fn new_vault() -> (tempfile::TempDir, Arc<Vault>) {
    let dir = tempfile::tempdir().unwrap();
    let (v, _) = Vault::create_with_params(dir.path(), &Zeroizing::new("pw".into()), false, fast()).unwrap();
    (dir, Arc::new(v))
}

fn importer(v: &Arc<Vault>, lookup: Lookup) -> Importer {
    let v = v.clone();
    Importer::new(Arc::new(move || Some(v.clone())), lookup, None, Arc::new(|_| {}))
}

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// 20 videos: 6 movies, three shows (one ambiguous by name), and subtitles.
fn mixed_folder() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let video = |n: u8| vec![n; 20_000 + n as usize];
    for (i, name) in [
        "Inception.2010.1080p.BluRay.x264.mkv",
        "Arrival (2016)/Arrival.2016.mkv",
        "Heat.1995.mkv",
        "Memento (2000)/Memento.2000.mp4",
        "Blade.Runner.2049.2017.mkv",
        "Zodiac.2007.avi",
    ]
    .iter()
    .enumerate()
    {
        put(r, name, &video(i as u8));
    }
    put(r, "Inception.2010.1080p.BluRay.x264.srt", b"1\n00:00:01,000 --> 00:00:02,000\nHello\n");
    for e in 1..=5 {
        put(r, &format!("Fauda/Season 1/Fauda.S01E{e:02}.mkv"), &video(10 + e));
    }
    for e in 1..=2 {
        put(r, &format!("Fauda/Season 2/Fauda.S02E{e:02}.mkv"), &video(20 + e));
    }
    // windows-1255 Hebrew: "שלום"
    put(r, "Fauda/Season 1/Fauda.S01E01.he.srt", b"1\n00:00:01,000 --> 00:00:02,000\n\xf9\xec\xe5\xed\n");
    for e in 1..=4 {
        put(r, &format!("The.Office.S01E{e:02}.720p.mkv"), &video(30 + e));
    }
    for e in 1..=3 {
        put(r, &format!("Shtisel/Shtisel.S02E{e:02}.mkv"), &video(40 + e));
    }
    d
}

fn cand(id: &str, title: &str, year: Option<u16>, poster: Option<&str>) -> Candidate {
    Candidate {
        provider: "fake",
        id: id.into(),
        title: title.into(),
        original_title: None,
        year,
        overview: None,
        popularity: 1.0,
        poster: poster.map(|u| art(ArtKind::Poster, u)),
    }
}

fn art(kind: ArtKind, url: &str) -> ArtworkRef {
    ArtworkRef { kind, url: url.into(), width: None, height: None }
}

/// Knows Inception, Fauda and two shows called "The Office".
struct Fake {
    show_calls: AtomicUsize,
}

impl MetadataProvider for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn search_movie(&self, title: &str, _: Option<u16>) -> crate::metadata::Result<Vec<Candidate>> {
        Ok(match normalize_title(title).as_str() {
            "inception" => vec![cand("m1", "Inception", Some(2010), None)],
            _ => vec![],
        })
    }
    fn search_show(&self, title: &str, _: Option<u16>) -> crate::metadata::Result<Vec<Candidate>> {
        Ok(match normalize_title(title).as_str() {
            "fauda" => vec![cand("s1", "Fauda", Some(2015), None)],
            "the office" => {
                vec![cand("s2", "The Office", Some(2005), None), cand("s3", "The Office", Some(2001), None)]
            }
            _ => vec![],
        })
    }
    fn movie(&self, id: &str) -> crate::metadata::Result<MovieDetails> {
        assert_eq!(id, "m1");
        Ok(MovieDetails {
            provider: "tmdb",
            id: "27205".into(),
            title: "Inception".into(),
            original_title: None,
            year: Some(2010),
            runtime_min: Some(148),
            overview: Some("Dreams.".into()),
            genres: vec!["Science Fiction".into()],
            imdb_id: None,
            poster: Some(art(ArtKind::Poster, "https://image.tmdb.org/t/p/w780/inception.png")),
            backdrop: None,
        })
    }
    fn show(&self, id: &str, seasons: &[u32]) -> crate::metadata::Result<ShowDetails> {
        self.show_calls.fetch_add(1, Ordering::SeqCst);
        let ep = |s: u32, n: u32| EpisodeDetails {
            season: s,
            number: n,
            title: Some(format!("{id} {s}x{n}")),
            overview: None,
            air_date: None,
            runtime_min: Some(45),
            thumb: None,
        };
        let season = |s: u32| SeasonDetails {
            number: s,
            title: None,
            poster: None,
            episodes: (1..=12).map(|n| ep(s, n)).collect(),
        };
        let (title, year, provider) = match id {
            "s1" => ("Fauda", 2015, "tvmaze"),
            "s2" => ("The Office", 2005, "tvmaze"),
            _ => ("The Office", 2001, "tvmaze"),
        };
        if id == "s1" {
            assert_eq!(seasons, [1, 2], "all seasons of the batch are asked for at once");
        }
        Ok(ShowDetails {
            provider,
            id: id.trim_start_matches('s').into(),
            title: title.into(),
            original_title: None,
            first_year: Some(year),
            last_year: None,
            overview: None,
            genres: vec!["Drama".into()],
            poster: None,
            backdrop: None,
            seasons: vec![season(1), season(2)],
        })
    }
}

/// Serves a 1000×1500 PNG for any image URL.
struct Png;
impl HttpClient for Png {
    fn get(&self, _: &str, _: &[(&str, &str)], _: usize) -> crate::metadata::Result<HttpResponse> {
        let img = image::RgbImage::from_pixel(1000, 1500, image::Rgb([200, 30, 30]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        Ok(HttpResponse { status: 200, content_type: None, body: out.into_inner() })
    }
}

fn online() -> (Lookup, Arc<Fake>) {
    let fake = Arc::new(Fake { show_calls: AtomicUsize::new(0) });
    let p: Arc<dyn MetadataProvider> = fake.clone();
    (Lookup::Online { movies: Some(p.clone()), shows: p, http: Arc::new(Png) }, fake)
}

#[test]
fn offline_import_groups_twenty_files() {
    let (_d, v) = new_vault();
    let src = mixed_folder();
    let imp = importer(&v, Lookup::Offline);
    let queued = imp.add(&[src.path().to_path_buf()]).unwrap();
    assert_eq!(queued.len(), 20);
    imp.wait_idle();
    let items = imp.items();
    assert!(items.iter().all(|i| i.state == ItemState::Done), "{items:#?}");
    assert!(items.iter().all(|i| i.match_state == MatchState::Offline));

    let lib = v.library();
    let movies = lib.movies(WatchFilter::All, None, SortBy::Title).unwrap();
    let titles: Vec<_> = movies.iter().map(|m| display_movie(&m.title, m.year)).collect();
    assert_eq!(
        titles,
        [
            "Arrival (2016)",
            "Blade Runner 2049 (2017)",
            "Heat (1995)",
            "Inception (2010)",
            "Memento (2000)",
            "Zodiac (2007)"
        ]
    );
    let shows = lib.shows(WatchFilter::All, None, SortBy::Title).unwrap();
    let summary: Vec<_> = shows.iter().map(|s| (s.title.as_str(), s.season_count, s.episode_count)).collect();
    assert_eq!(summary, [("Fauda", 2, 7), ("Shtisel", 1, 3), ("The Office", 1, 4)]);

    // Subtitles were attached, converted to UTF-8 and encrypted.
    let inception = movies.iter().find(|m| m.title == "Inception").unwrap();
    assert_eq!(lib.subtitles(inception.media_file_id).unwrap().len(), 1);
    let fauda_s1 = lib.seasons(shows[0].id).unwrap().into_iter().find(|s| s.number == 1).unwrap();
    let e1 = lib.episodes(fauda_s1.id).unwrap().into_iter().find(|e| e.number == 1).unwrap();
    let subs = lib.subtitles(e1.media_file_id).unwrap();
    assert_eq!(subs[0].language.as_deref(), Some("he"));
    let blob = subs[0].blob_id;
    let fauda_item = items.iter().find(|i| i.file_name == "Fauda.S01E01.mkv").unwrap();
    assert!(fauda_item.subtitles[0].note.as_deref().unwrap().contains("windows-1255"), "{fauda_item:?}");
    drop(lib);
    let text = String::from_utf8(v.read_blob(blob).unwrap().to_vec()).unwrap();
    assert!(text.contains("שלום"));

    // Every blob decrypts and every original is intact.
    let report = v.integrity_check(|_| {}).unwrap();
    assert!(report.is_clean(), "{report:?}");
    let mf = v.library().media_file(inception.media_file_id).unwrap();
    assert_eq!(
        &v.read_blob(mf.blob_id).unwrap()[..],
        &fs::read(src.path().join("Inception.2010.1080p.BluRay.x264.mkv")).unwrap()[..]
    );

    // Importing again skips everything already in the vault.
    let again = imp.add(&[src.path().to_path_buf()]).unwrap();
    assert!(again.iter().all(|i| i.state == ItemState::AlreadyInVault));
}

#[test]
fn online_import_matches_and_sends_ambiguous_shows_to_review() {
    let (_d, v) = new_vault();
    let src = mixed_folder();
    let (lookup, fake) = online();
    let imp = importer(&v, lookup);
    imp.add(&[src.path().to_path_buf()]).unwrap();
    imp.wait_idle();
    let items = imp.items();
    let by_name = |n: &str| items.iter().find(|i| i.file_name == n).unwrap();
    assert!(items.iter().all(|i| i.state == ItemState::Done), "{items:#?}");

    assert_eq!(by_name("Inception.2010.1080p.BluRay.x264.mkv").match_state, MatchState::Matched);
    assert_eq!(by_name("Heat.1995.mkv").match_state, MatchState::NoMatch);
    assert_eq!(by_name("Fauda.S02E02.mkv").match_state, MatchState::Matched);
    assert_eq!(by_name("Shtisel.S02E01.mkv").match_state, MatchState::NoMatch);
    let office = by_name("The.Office.S01E03.720p.mkv");
    assert_eq!(office.match_state, MatchState::NeedsReview);
    assert_eq!(office.candidates.len(), 2);
    // One lookup per show, not per episode (Fauda, The Office).
    assert_eq!(fake.show_calls.load(Ordering::SeqCst), 2);

    {
        let lib = v.library();
        let m = lib.movies(WatchFilter::All, None, SortBy::Title).unwrap();
        let inception = m.iter().find(|m| m.title == "Inception").unwrap();
        assert_eq!((inception.runtime_min, inception.tmdb_id), (Some(148), Some(27205)));
        let poster = lib.artwork(inception.poster_id.unwrap()).unwrap();
        assert_eq!((poster.width, poster.height), (Some(POSTER_WIDTH), Some(750)));
        let shows = lib.shows(WatchFilter::All, None, SortBy::Title).unwrap();
        let fauda = shows.iter().find(|s| s.title == "Fauda").unwrap();
        assert_eq!(fauda.tvmaze_id, Some(1));
        let s2 = lib.seasons(fauda.id).unwrap().into_iter().find(|s| s.number == 2).unwrap();
        assert_eq!(lib.episodes(s2.id).unwrap()[1].title.as_deref(), Some("s1 2x2"));
        // The ambiguous show kept the file name's details, not a guess.
        let office = shows.iter().find(|s| s.title == "The Office").unwrap();
        assert_eq!(office.tvmaze_id, None);
    }

    // The user picks the 2001 one: every episode of it gets the details.
    imp.resolve(office.id, &Resolution::Candidate { id: "s3".into() }).unwrap();
    let lib = v.library();
    let shows = lib.shows(WatchFilter::All, None, SortBy::Title).unwrap();
    let office_show = shows.iter().find(|s| s.title == "The Office").unwrap();
    assert_eq!((office_show.tvmaze_id, office_show.first_year), (Some(3), Some(2001)));
    let s1 = lib.seasons(office_show.id).unwrap()[0].id;
    let eps = lib.episodes(s1).unwrap();
    assert_eq!(eps.len(), 4);
    assert!(eps.iter().all(|e| e.title.as_deref() == Some(&format!("s3 1x{}", e.number))), "{eps:?}");
    drop(lib);
    assert!(imp
        .items()
        .iter()
        .filter(|i| i.file_name.starts_with("The.Office"))
        .all(|i| i.match_state == MatchState::Matched));

    // A manual title for a movie with no match.
    let heat = imp.items().into_iter().find(|i| i.file_name == "Heat.1995.mkv").unwrap();
    imp.resolve(heat.id, &Resolution::Manual { title: "Heat".into(), year: Some(1995) }).unwrap();
    assert!(v.library().movies(WatchFilter::All, None, SortBy::Title).unwrap().iter().any(|m| m.title == "Heat"));
}

#[test]
fn pause_resume_and_cancel() {
    let (_d, v) = new_vault();
    let src = tempfile::tempdir().unwrap();
    // Big enough for several progress callbacks.
    for n in 0..3 {
        put(src.path(), &format!("Movie{n}.20{n:02}.mkv"), &vec![n as u8; 5 << 20]);
    }
    let imp = importer(&v, Lookup::Offline);
    imp.pause();
    let items = imp.add(&[src.path().to_path_buf()]).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(imp.items().iter().all(|i| i.state == ItemState::Waiting), "paused before starting");
    imp.cancel(Some(items[2].id));
    imp.resume();
    imp.wait_idle();
    let states: Vec<_> = imp.items().into_iter().map(|i| i.state).collect();
    assert_eq!(states, [ItemState::Done, ItemState::Done, ItemState::Cancelled]);
    assert_eq!(v.library().media_files().unwrap().len(), 2);
    assert!(v.integrity_check(|_| {}).unwrap().is_clean(), "no orphaned blobs");
}

#[test]
fn cancelling_a_running_import_removes_its_blob() {
    let (_d, v) = new_vault();
    let src = tempfile::tempdir().unwrap();
    put(src.path(), "Big.2020.mkv", &vec![1u8; 24 << 20]);
    let vv = v.clone();
    let started = Arc::new(std::sync::Barrier::new(2));
    let s2 = started.clone();
    let once = std::sync::Once::new();
    let imp = Importer::new(
        Arc::new(move || Some(vv.clone())),
        Lookup::Offline,
        None,
        Arc::new(move |i: &QueueItem| {
            if matches!(i.state, ItemState::Encrypting { .. }) {
                once.call_once(|| {
                    s2.wait();
                });
            }
        }),
    );
    let items = imp.add(&[src.path().to_path_buf()]).unwrap();
    started.wait();
    imp.cancel(Some(items[0].id));
    imp.wait_idle();
    assert_eq!(imp.items()[0].state, ItemState::Cancelled);
    assert!(v.library().media_files().unwrap().is_empty());
    assert!(v.blobs_on_disk().unwrap().is_empty(), "the partial blob is gone");
}

#[test]
fn resize_keeps_aspect_and_small_images() {
    let png = Png.get("", &[], 0).unwrap().body;
    let (_, w, h) = resize_artwork(&png, THUMB_WIDTH).unwrap();
    assert_eq!((w, h), (400, 600));
    let (_, w, h) = resize_artwork(&png, 2000).unwrap();
    assert_eq!((w, h), (1000, 1500));
    assert!(resize_artwork(b"not an image", 500).is_none());
}

#[test]
fn padded_size_estimate_is_never_low() {
    for size in [0u64, 1, (1 << 20) - 60, 1 << 20, 5 << 20, (7 << 20) + 3] {
        let (_d, v) = new_vault();
        let data = vec![0u8; size as usize];
        let id = v.write_blob(&data[..], size, &BlobWriteOptions::media()).unwrap();
        let on_disk = fs::metadata(v.blob_path(id)).unwrap().len();
        assert!(padded(size) >= on_disk, "{size}: estimate {} < {on_disk}", padded(size));
    }
}
