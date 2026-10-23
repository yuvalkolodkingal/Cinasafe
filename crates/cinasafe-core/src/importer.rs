//! The import queue (brief 8.2): recognise, look up, encrypt and verify,
//! probe, file into the library, attach subtitles, fetch artwork. Pausable,
//! resumable and cancellable; checks free space first; low-confidence
//! matches land in "Needs review" with their candidates.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::blob::PAD_MULTIPLE;
use crate::import::{self, normalize_title, PlannedEpisode, SubtitleFile};
use crate::library::{ArtKind as LibArtKind, EpisodeMeta, Id, MovieMeta, NewMediaFile, NewSubtitle, ShowMeta};
use crate::metadata::identify::{episodes_for, Scored};
use crate::metadata::{
    fetch_artwork, identify_movie, identify_show, ArtKind, ArtworkRef, HttpClient, IdentifiedShow, MetadataProvider,
    MovieDetails, ShowDetails,
};
use crate::subtitles;
use crate::{BlobId, BlobWriteOptions, Error, Vault};

/// Artwork widths from brief 8.2.
pub const POSTER_WIDTH: u32 = 500;
pub const BACKDROP_WIDTH: u32 = 1280;
pub const THUMB_WIDTH: u32 = 400;

/// What `probe` learns about a media file (duration, container, streams).
#[derive(Clone, Debug, Default)]
pub struct ProbeInfo {
    pub duration_ms: Option<i64>,
    pub container: Option<String>,
    pub streams_json: Option<String>,
}

/// Reads stream information from an encrypted media blob (libmpv in the app).
pub trait Prober: Send + Sync {
    fn probe(&self, vault: &Arc<Vault>, blob: BlobId) -> Option<ProbeInfo>;
}

/// Where metadata comes from. Offline unless the user turned lookups on.
#[derive(Clone)]
pub enum Lookup {
    Offline,
    Online {
        /// `None` when no provider offers movies (TVmaze only).
        movies: Option<Arc<dyn MetadataProvider>>,
        shows: Arc<dyn MetadataProvider>,
        http: Arc<dyn HttpClient>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Movie,
    Episode,
    Subtitle,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ItemState {
    Waiting,
    /// Skipped: not enough free space on the stick.
    WontFit,
    /// Skipped: the same file (name and size) is already in the vault.
    AlreadyInVault,
    /// A subtitle with no video to attach to.
    NoVideo,
    LookingUp,
    Encrypting {
        fraction: f32,
    },
    Verifying {
        fraction: f32,
    },
    Finishing,
    Done,
    Failed {
        message: String,
    },
    Cancelled,
}

impl ItemState {
    pub fn is_finished(&self) -> bool {
        !matches!(
            self,
            ItemState::Waiting
                | ItemState::LookingUp
                | ItemState::Encrypting { .. }
                | ItemState::Verifying { .. }
                | ItemState::Finishing
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchState {
    /// Lookups are off: the file name decided everything.
    Offline,
    Matched,
    NeedsReview,
    NoMatch,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleNote {
    pub file_name: String,
    pub language: Option<String>,
    /// e.g. "Converted from windows-1255 to UTF-8".
    pub note: Option<String>,
    pub rtl_fix_offered: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub id: u64,
    pub kind: ItemKind,
    pub file_name: String,
    pub size_bytes: u64,
    /// "Inception (2010)" or "Fauda · S01E02".
    pub detected: String,
    #[serde(flatten)]
    pub state: ItemState,
    pub match_state: MatchState,
    pub candidates: Vec<Scored>,
    pub subtitles: Vec<SubtitleNote>,
    pub media_file_id: Option<Id>,
    pub eta_seconds: Option<u64>,
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Resolution {
    /// Use this search hit (its provider id).
    Candidate { id: String },
    /// Keep the file but use these details.
    Manual { title: String, year: Option<u16> },
}

#[derive(Clone, Debug)]
enum UnitKind {
    Movie { title: String, year: Option<u16> },
    Episode { show_key: String, titles: Vec<String>, year: Option<u16>, ep: PlannedEpisode },
}

struct Unit {
    item_id: u64,
    video: PathBuf,
    file_name: String,
    size: u64,
    kind: UnitKind,
    subtitles: Vec<(PathBuf, SubtitleFile)>,
}

#[derive(Clone)]
enum Done {
    Movie { movie_id: Id },
    Episode { show_id: Id, media_file_id: Id, ep: PlannedEpisode },
}

#[derive(Default)]
struct State {
    items: Vec<QueueItem>,
    pending: VecDeque<Unit>,
    paused: bool,
    cancelled: Vec<u64>,
    active: Option<u64>,
    worker: bool,
    next_id: u64,
    /// Show lookups per normalised show title, reused across a batch.
    shows: HashMap<String, Option<IdentifiedShow>>,
    done: HashMap<u64, Done>,
}

/// The whole queue, for a UI that just opened.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStatusItems {
    pub items: Vec<QueueItem>,
    pub paused: bool,
}

pub type VaultGetter = Arc<dyn Fn() -> Option<Arc<Vault>> + Send + Sync>;
pub type ItemListener = Arc<dyn Fn(&QueueItem) + Send + Sync>;

struct Inner {
    vault: VaultGetter,
    lookup: Mutex<Lookup>,
    prober: Option<Arc<dyn Prober>>,
    listener: ItemListener,
    state: Mutex<State>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
pub struct Importer {
    inner: Arc<Inner>,
}

fn display_movie(title: &str, year: Option<u16>) -> String {
    match year {
        Some(y) => format!("{title} ({y})"),
        None => title.to_owned(),
    }
}

fn display_episode(show: &str, ep: &PlannedEpisode) -> String {
    let nums: String = ep.episodes.iter().map(|n| format!("E{n:02}")).collect();
    match (ep.season, &ep.air_date) {
        (_, Some(d)) => format!("{show} · {d}"),
        (Some(s), _) => format!("{show} · S{s:02}{nums}"),
        (None, _) => format!("{show} · #{}", ep.episodes.iter().map(u32::to_string).collect::<Vec<_>>().join("+")),
    }
}

fn padded(size: u64) -> u64 {
    // Header + 16 bytes per MiB chunk, rounded up to the 1 MiB padding.
    (size + 44 + 16 * (size / (1 << 20) + 1)).div_ceil(PAD_MULTIPLE) * PAD_MULTIPLE
}

/// Decodes, shrinks to `max_width` and re-encodes as JPEG (quality 85).
pub fn resize_artwork(bytes: &[u8], max_width: u32) -> Option<(Zeroizing<Vec<u8>>, u32, u32)> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max_width {
        let h = (img.height() as u64 * max_width as u64 / img.width() as u64).max(1) as u32;
        img.resize_exact(max_width, h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let mut out = Zeroizing::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut *out, 85);
    enc.encode_image(&rgb).ok()?;
    Some((out, rgb.width(), rgb.height()))
}

impl Importer {
    pub fn new(
        vault: VaultGetter,
        lookup: Lookup,
        prober: Option<Arc<dyn Prober>>,
        listener: ItemListener,
    ) -> Importer {
        Importer {
            inner: Arc::new(Inner {
                vault,
                lookup: Mutex::new(lookup),
                prober,
                listener,
                state: Mutex::new(State { next_id: 1, ..Default::default() }),
                wake: Condvar::new(),
                worker: Mutex::new(None),
            }),
        }
    }

    pub fn set_lookup(&self, lookup: Lookup) {
        *self.inner.lookup.lock().unwrap() = lookup;
    }

    pub fn items(&self) -> Vec<QueueItem> {
        self.inner.state.lock().unwrap().items.clone()
    }

    pub fn status(&self) -> ImportStatusItems {
        let st = self.inner.state.lock().unwrap();
        ImportStatusItems { items: st.items.clone(), paused: st.paused }
    }

    pub fn is_paused(&self) -> bool {
        self.inner.state.lock().unwrap().paused
    }

    /// Plans an import of `paths` (files or folders) and queues it.
    pub fn add(&self, paths: &[PathBuf]) -> Result<Vec<QueueItem>, Error> {
        let vault = (self.inner.vault)().ok_or(Error::NotAVault)?;
        let mut new_units = Vec::new();
        let mut extra_items = Vec::new();
        for p in paths {
            let (dir, only) = if p.is_dir() {
                (p.clone(), None)
            } else {
                (p.parent().map(Path::to_path_buf).unwrap_or_default(), Some(p.clone()))
            };
            let plan = import::scan(&dir)?;
            let wanted = |rel: &Path| only.as_ref().is_none_or(|o| dir.join(rel) == *o);
            let subs = |list: &[SubtitleFile]| list.iter().map(|s| (dir.join(&s.path), s.clone())).collect::<Vec<_>>();
            for m in plan.movies.iter().filter(|m| wanted(&m.video.path)) {
                new_units.push(Unit {
                    item_id: 0,
                    video: dir.join(&m.video.path),
                    file_name: file_name(&m.video.path),
                    size: m.video.size,
                    kind: UnitKind::Movie { title: m.title.clone(), year: m.year },
                    subtitles: subs(&m.subtitles),
                });
            }
            for s in &plan.shows {
                let mut titles = vec![s.title.clone()];
                titles.extend(s.alt_titles.iter().cloned());
                for e in s.episodes.iter().filter(|e| wanted(&e.video.path)) {
                    new_units.push(Unit {
                        item_id: 0,
                        video: dir.join(&e.video.path),
                        file_name: file_name(&e.video.path),
                        size: e.video.size,
                        kind: UnitKind::Episode {
                            show_key: normalize_title(&s.title),
                            titles: titles.clone(),
                            year: s.year,
                            ep: e.clone(),
                        },
                        subtitles: subs(&e.subtitles),
                    });
                }
            }
            // A dropped subtitle with no video next to it.
            for s in plan.unmatched_subtitles.iter().filter(|s| wanted(&s.path)) {
                extra_items.push((file_name(&s.path), ItemKind::Subtitle, ItemState::NoVideo));
            }
        }

        let free = crate::drives::free_space(vault.root()).ok();
        let mut st = self.inner.state.lock().unwrap();
        let mut committed: u64 = st.pending.iter().map(|u| padded(u.size)).sum();
        let mut out = Vec::new();
        for mut u in new_units {
            if st.pending.iter().any(|p| p.video == u.video) {
                continue;
            }
            let id = st.next_id;
            st.next_id += 1;
            u.item_id = id;
            let (kind, detected) = match &u.kind {
                UnitKind::Movie { title, year } => (ItemKind::Movie, display_movie(title, *year)),
                UnitKind::Episode { titles, ep, .. } => (ItemKind::Episode, display_episode(&titles[0], ep)),
            };
            let already = vault.library().find_media(&u.file_name, u.size).ok().flatten();
            let need = padded(u.size) + u.subtitles.len() as u64 * PAD_MULTIPLE;
            let state = if already.is_some() {
                ItemState::AlreadyInVault
            } else if free.is_some_and(|f| committed + need > f) {
                ItemState::WontFit
            } else {
                committed += need;
                ItemState::Waiting
            };
            let item = QueueItem {
                id,
                kind,
                file_name: u.file_name.clone(),
                size_bytes: u.size,
                detected,
                state: state.clone(),
                match_state: MatchState::Offline,
                candidates: Vec::new(),
                subtitles: u
                    .subtitles
                    .iter()
                    .map(|(_, s)| SubtitleNote {
                        file_name: file_name(&s.path),
                        language: s.language.map(str::to_owned),
                        note: (s.source_encoding != "UTF-8")
                            .then(|| format!("Converted from {} to UTF-8", s.source_encoding)),
                        rtl_fix_offered: s.suggest_rtl_fix,
                    })
                    .collect(),
                media_file_id: already,
                eta_seconds: None,
                messages: Vec::new(),
            };
            st.items.push(item.clone());
            out.push(item);
            if state == ItemState::Waiting {
                st.pending.push_back(u);
            }
        }
        for (name, kind, state) in extra_items {
            let id = st.next_id;
            st.next_id += 1;
            let item = QueueItem {
                id,
                kind,
                file_name: name.clone(),
                size_bytes: 0,
                detected: name,
                state,
                match_state: MatchState::Offline,
                candidates: Vec::new(),
                subtitles: Vec::new(),
                media_file_id: None,
                eta_seconds: None,
                messages: Vec::new(),
            };
            st.items.push(item.clone());
            out.push(item);
        }
        let start = !st.worker && !st.pending.is_empty();
        if start {
            st.worker = true;
        }
        drop(st);
        for i in &out {
            (self.inner.listener)(i);
        }
        if start {
            let me = self.clone();
            let h = std::thread::Builder::new().name("importer".into()).spawn(move || me.run())?;
            *self.inner.worker.lock().unwrap() = Some(h);
        }
        Ok(out)
    }

    pub fn pause(&self) {
        self.inner.state.lock().unwrap().paused = true;
    }

    pub fn resume(&self) {
        self.inner.state.lock().unwrap().paused = false;
        self.inner.wake.notify_all();
    }

    /// Cancels one item, or everything waiting and running when `id` is `None`.
    /// Partly written blobs are removed.
    pub fn cancel(&self, id: Option<u64>) {
        let mut changed = Vec::new();
        {
            let mut st = self.inner.state.lock().unwrap();
            let ids: Vec<u64> = match id {
                Some(i) => vec![i],
                None => st.items.iter().filter(|i| !i.state.is_finished()).map(|i| i.id).collect(),
            };
            for i in ids {
                if st.active == Some(i) {
                    st.cancelled.push(i);
                } else if let Some(pos) = st.pending.iter().position(|u| u.item_id == i) {
                    st.pending.remove(pos);
                    if let Some(item) = st.items.iter_mut().find(|x| x.id == i) {
                        item.state = ItemState::Cancelled;
                        changed.push(item.clone());
                    }
                }
            }
        }
        self.inner.wake.notify_all();
        for i in &changed {
            (self.inner.listener)(i);
        }
    }

    /// Drops finished items from the list.
    pub fn clear_finished(&self) {
        let mut st = self.inner.state.lock().unwrap();
        st.items.retain(|i| !i.state.is_finished() || i.match_state == MatchState::NeedsReview);
    }

    /// Forgets everything (on lock): no titles or file names stay in memory.
    /// Call after `cancel(None)` and `wait_idle()`.
    pub fn clear_all(&self) {
        let mut st = self.inner.state.lock().unwrap();
        st.items.clear();
        st.pending.clear();
        st.done.clear();
        st.shows.clear();
        st.cancelled.clear();
        st.paused = false;
    }

    /// Bytes the waiting and running imports will still add to the stick.
    pub fn queued_bytes(&self) -> u64 {
        let st = self.inner.state.lock().unwrap();
        let running = st.items.iter().filter(|i| Some(i.id) == st.active).map(|i| padded(i.size_bytes)).sum::<u64>();
        running + st.pending.iter().map(|u| padded(u.size)).sum::<u64>()
    }

    /// Blocks until nothing is waiting or running (CLI and tests).
    pub fn wait_idle(&self) {
        let h = self.inner.worker.lock().unwrap().take();
        if let Some(h) = h {
            let _ = h.join();
        }
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut QueueItem)) {
        let snapshot = {
            let mut st = self.inner.state.lock().unwrap();
            st.items.iter_mut().find(|i| i.id == id).map(|i| {
                f(i);
                i.clone()
            })
        };
        if let Some(i) = snapshot {
            (self.inner.listener)(&i);
        }
    }

    fn is_cancelled(&self, id: u64) -> bool {
        self.inner.state.lock().unwrap().cancelled.contains(&id)
    }

    /// Blocks while paused; returns false if `id` was cancelled meanwhile.
    fn wait_while_paused(&self, id: u64) -> bool {
        let mut st = self.inner.state.lock().unwrap();
        while st.paused && !st.cancelled.contains(&id) {
            st = self.inner.wake.wait(st).unwrap();
        }
        !st.cancelled.contains(&id)
    }

    fn run(&self) {
        loop {
            let unit = {
                let mut st = self.inner.state.lock().unwrap();
                while st.paused && !st.pending.is_empty() {
                    st = self.inner.wake.wait(st).unwrap();
                }
                match st.pending.pop_front() {
                    Some(u) => {
                        st.active = Some(u.item_id);
                        u
                    }
                    None => {
                        st.worker = false;
                        st.shows.clear();
                        return;
                    }
                }
            };
            let id = unit.item_id;
            let result = self.process(&unit);
            {
                let mut st = self.inner.state.lock().unwrap();
                st.active = None;
                st.cancelled.retain(|c| *c != id);
                if let Ok(Some(done)) = &result {
                    st.done.insert(id, done.clone());
                }
            }
            match result {
                Ok(Some(_)) => self.update(id, |i| {
                    i.state = ItemState::Done;
                    i.eta_seconds = None;
                }),
                Ok(None) => self.update(id, |i| i.state = ItemState::Cancelled),
                Err(e) => self.update(id, |i| i.state = ItemState::Failed { message: e.to_string() }),
            }
        }
    }

    /// Returns `Ok(None)` when cancelled.
    fn process(&self, u: &Unit) -> Result<Option<Done>, Error> {
        let vault = (self.inner.vault)().ok_or(Error::NotAVault)?;
        let lookup = self.inner.lookup.lock().unwrap().clone();

        // 1. Look up first, so the queue shows "Matched"/"Needs review" early.
        self.update(u.item_id, |i| i.state = ItemState::LookingUp);
        let (movie_hit, show_hit) = self.look_up(u, &lookup);

        // 2. Encrypt and verify.
        let f = File::open(&u.video)?;
        let started = Instant::now();
        let mut last = Instant::now() - Duration::from_secs(1);
        let size = u.size.max(1);
        let id = u.item_id;
        let blob = vault.write_blob_with(
            BufReader::with_capacity(4 << 20, f),
            u.size,
            &BlobWriteOptions::media(),
            &mut |phase, n| {
                if !self.wait_while_paused(id) || self.is_cancelled(id) {
                    return false;
                }
                if last.elapsed() >= Duration::from_millis(250) {
                    last = Instant::now();
                    let fraction = (n as f64 / size as f64).min(1.0) as f32;
                    let total_fraction = match phase {
                        crate::vault::WritePhase::Encrypting => fraction * 0.5,
                        crate::vault::WritePhase::Verifying => 0.5 + fraction * 0.5,
                    };
                    let secs = started.elapsed().as_secs_f64();
                    let eta = (total_fraction > 0.02)
                        .then(|| (secs / total_fraction as f64 * (1.0 - total_fraction as f64)) as u64);
                    self.update(id, |i| {
                        i.state = match phase {
                            crate::vault::WritePhase::Encrypting => ItemState::Encrypting { fraction },
                            crate::vault::WritePhase::Verifying => ItemState::Verifying { fraction },
                        };
                        i.eta_seconds = eta;
                    });
                }
                true
            },
        );
        let blob = match blob {
            Ok(b) => b,
            Err(Error::Cancelled) => return Ok(None),
            Err(e) => return Err(e),
        };
        if self.is_cancelled(id) {
            vault.delete_blob(blob)?;
            return Ok(None);
        }
        self.update(id, |i| i.state = ItemState::Finishing);

        // 3. Probe (duration, container, streams) through the player.
        let probe = self.inner.prober.as_ref().and_then(|p| p.probe(&vault, blob)).unwrap_or_default();

        // 4. File it into the library.
        let media_file_id = vault.library().add_media_file(
            blob,
            &NewMediaFile {
                original_name: u.file_name.clone(),
                size_bytes: u.size,
                duration_ms: probe.duration_ms,
                container: probe.container.clone(),
                streams_json: probe.streams_json.clone(),
            },
        )?;
        self.update(id, |i| i.media_file_id = Some(media_file_id));
        let done = match &u.kind {
            UnitKind::Movie { title, year } => {
                let details = movie_hit.as_ref().filter(|(confident, _)| *confident).map(|(_, d)| d);
                let meta = match details {
                    Some(d) => movie_meta(d),
                    None => MovieMeta { title: title.clone(), year: *year, ..Default::default() },
                };
                let movie_id = vault.library().add_movie(media_file_id, &meta, None, None)?;
                if let (Some(d), Lookup::Online { http, .. }) = (details, &lookup) {
                    self.movie_artwork(&vault, http.as_ref(), movie_id, d);
                }
                Done::Movie { movie_id }
            }
            UnitKind::Episode { titles, year, ep, .. } => {
                let details = show_hit.as_ref().filter(|s| s.confident).and_then(|s| s.details.as_ref());
                let show_id = self.file_episode(&vault, &lookup, media_file_id, &titles[0], *year, ep, details)?;
                Done::Episode { show_id, media_file_id, ep: ep.clone() }
            }
        };

        // 5. Subtitles: UTF-8, encrypted, attached.
        for (path, s) in &u.subtitles {
            let raw = Zeroizing::new(std::fs::read(path)?);
            let decoded = subtitles::decode(&raw);
            let text = Zeroizing::new(decoded.text.into_bytes());
            let sblob = vault.write_blob(&text[..], text.len() as u64, &BlobWriteOptions::default())?;
            vault.library().add_subtitle(&NewSubtitle {
                media_file_id,
                blob_id: sblob,
                original_blob_id: None,
                language: s.language.map(str::to_owned),
                format: s.format.extension().to_owned(),
                rtl_fix: false,
            })?;
        }

        vault.save_library()?;
        Ok(Some(done))
    }

    fn look_up(&self, u: &Unit, lookup: &Lookup) -> (Option<(bool, MovieDetails)>, Option<IdentifiedShow>) {
        let Lookup::Online { movies, shows, .. } = lookup else { return (None, None) };
        match &u.kind {
            UnitKind::Movie { title, year } => {
                let Some(p) = movies else {
                    self.update(u.item_id, |i| {
                        i.match_state = MatchState::Offline;
                        i.messages.push("Movie details need a TMDB key (Settings › Privacy).".into());
                    });
                    return (None, None);
                };
                match identify_movie(p.as_ref(), title, *year) {
                    Ok(r) => {
                        let state = match (&r.details, r.confident) {
                            (None, _) => MatchState::NoMatch,
                            (Some(_), true) => MatchState::Matched,
                            (Some(_), false) => MatchState::NeedsReview,
                        };
                        let detected =
                            r.details.as_ref().filter(|_| r.confident).map(|d| display_movie(&d.title, d.year));
                        self.update(u.item_id, |i| {
                            i.match_state = state;
                            i.candidates = r.candidates.clone();
                            if let Some(d) = detected {
                                i.detected = d;
                            }
                        });
                        (r.details.map(|d| (r.confident, d)), None)
                    }
                    Err(e) => {
                        self.update(u.item_id, |i| {
                            i.match_state = MatchState::NoMatch;
                            i.messages.push(format!("Lookup failed: {e}"));
                        });
                        (None, None)
                    }
                }
            }
            UnitKind::Episode { show_key, titles, year, ep } => {
                let cached = self.inner.state.lock().unwrap().shows.get(show_key).cloned();
                let hit = match cached {
                    Some(h) => h,
                    None => {
                        let seasons = self.batch_seasons(show_key, ep);
                        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
                        let h = match identify_show(shows.as_ref(), &refs, *year, &seasons) {
                            Ok(r) => Some(r),
                            Err(e) => {
                                self.update(u.item_id, |i| i.messages.push(format!("Lookup failed: {e}")));
                                None
                            }
                        };
                        self.inner.state.lock().unwrap().shows.insert(show_key.clone(), h.clone());
                        h
                    }
                };
                let state = match &hit {
                    None => MatchState::NoMatch,
                    Some(h) if h.details.is_none() => MatchState::NoMatch,
                    Some(h) if h.confident => MatchState::Matched,
                    Some(_) => MatchState::NeedsReview,
                };
                self.update(u.item_id, |i| {
                    i.match_state = state;
                    if let Some(h) = &hit {
                        i.candidates = h.candidates.clone();
                    }
                });
                (None, hit)
            }
        }
    }

    /// Seasons of this show in the current batch (the unit being looked up
    /// included); empty when any of them is numbered by date or absolutely.
    fn batch_seasons(&self, show_key: &str, current: &PlannedEpisode) -> Vec<u32> {
        let st = self.inner.state.lock().unwrap();
        let mut seasons = Vec::new();
        let others = st.pending.iter().filter_map(|u| match &u.kind {
            UnitKind::Episode { show_key: k, ep, .. } if k == show_key => Some(ep),
            _ => None,
        });
        for ep in std::iter::once(current).chain(others) {
            match (ep.season, &ep.air_date) {
                (Some(s), None) => seasons.push(s),
                _ => return Vec::new(),
            }
        }
        seasons.sort_unstable();
        seasons.dedup();
        seasons
    }

    #[allow(clippy::too_many_arguments)]
    fn file_episode(
        &self,
        vault: &Arc<Vault>,
        lookup: &Lookup,
        media_file_id: Id,
        parsed_show: &str,
        year: Option<u16>,
        ep: &PlannedEpisode,
        details: Option<&ShowDetails>,
    ) -> Result<Id, Error> {
        let found =
            details.map(|d| episodes_for(d, ep.season, &ep.episodes, ep.air_date.as_deref())).unwrap_or_default();
        let show_meta = match details {
            Some(d) => show_meta(d),
            None => ShowMeta { title: parsed_show.to_owned(), first_year: year, ..Default::default() },
        };
        let (show_id, needs_show_art) = {
            let mut lib = vault.library();
            let id = lib.upsert_show(&show_meta)?;
            let s = lib.show(id)?;
            (id, s.poster_id.is_none())
        };
        // Season and episode numbers: from the match when there is one, else from the name.
        let season_no = found.first().map(|e| e.season).or(ep.season).unwrap_or(1);
        let season_meta = details.and_then(|d| d.seasons.iter().find(|s| s.number == season_no));
        let (season_id, needs_season_art) = {
            let mut lib = vault.library();
            let sid = lib.upsert_season(show_id, season_no, season_meta.and_then(|s| s.title.as_deref()), None)?;
            let has = lib.seasons(show_id)?.into_iter().any(|s| s.id == sid && s.poster_id.is_some());
            (sid, !has)
        };
        let mut episode_rows: Vec<(Id, Option<ArtworkRef>)> = Vec::new();
        {
            let mut lib = vault.library();
            if found.is_empty() {
                let numbers = if ep.episodes.is_empty() {
                    vec![lib.max_episode_number(season_id)? + 1]
                } else {
                    ep.episodes.clone()
                };
                for n in numbers {
                    let meta = EpisodeMeta {
                        number: n,
                        title: ep.episode_title.clone(),
                        air_date: ep.air_date.clone(),
                        ..Default::default()
                    };
                    episode_rows.push((lib.add_episode(season_id, media_file_id, &meta, None)?, None));
                }
            } else {
                for e in &found {
                    let meta = EpisodeMeta {
                        number: e.number,
                        title: e.title.clone(),
                        overview: e.overview.clone(),
                        air_date: e.air_date.clone(),
                        runtime_min: e.runtime_min,
                    };
                    episode_rows.push((lib.add_episode(season_id, media_file_id, &meta, None)?, e.thumb.clone()));
                }
            }
        }
        if let (Some(d), Lookup::Online { http, .. }) = (details, lookup) {
            let http = http.as_ref();
            if needs_show_art {
                let poster = d.poster.as_ref().and_then(|a| self.store_artwork(vault, http, a));
                let backdrop = d.backdrop.as_ref().and_then(|a| self.store_artwork(vault, http, a));
                vault.library().set_show_artwork(show_id, poster, backdrop)?;
            }
            if needs_season_art {
                if let Some(p) =
                    season_meta.and_then(|s| s.poster.as_ref()).and_then(|a| self.store_artwork(vault, http, a))
                {
                    vault.library().set_season(season_id, None, Some(p))?;
                }
            }
            for (eid, thumb) in episode_rows {
                if let Some(t) = thumb.as_ref().and_then(|a| self.store_artwork(vault, http, a)) {
                    vault.library().set_episode_thumb(eid, t)?;
                }
            }
        }
        Ok(show_id)
    }

    fn movie_artwork(&self, vault: &Arc<Vault>, http: &dyn HttpClient, movie_id: Id, d: &MovieDetails) {
        let poster = d.poster.as_ref().and_then(|a| self.store_artwork(vault, http, a));
        let backdrop = d.backdrop.as_ref().and_then(|a| self.store_artwork(vault, http, a));
        let _ = vault.library().set_movie_artwork(movie_id, poster, backdrop);
    }

    /// Downloads, resizes and encrypts one image. Failures just mean no artwork.
    fn store_artwork(&self, vault: &Arc<Vault>, http: &dyn HttpClient, art: &ArtworkRef) -> Option<Id> {
        let fetched = fetch_artwork(http, art).ok()?;
        let width = match art.kind {
            ArtKind::Poster => POSTER_WIDTH,
            ArtKind::Backdrop => BACKDROP_WIDTH,
            ArtKind::Thumb => THUMB_WIDTH,
        };
        let raw = Zeroizing::new(fetched.bytes);
        let (jpeg, w, h) = resize_artwork(&raw, width)?;
        let blob = vault.write_blob(&jpeg[..], jpeg.len() as u64, &BlobWriteOptions::default()).ok()?;
        let kind = match art.kind {
            ArtKind::Poster => LibArtKind::Poster,
            ArtKind::Backdrop => LibArtKind::Backdrop,
            ArtKind::Thumb => LibArtKind::Thumb,
        };
        vault.library().add_artwork(blob, kind, Some(w), Some(h)).ok()
    }

    /// Applies the user's choice for an item in "Needs review" (or any item).
    pub fn resolve(&self, item_id: u64, res: &Resolution) -> Result<(), Error> {
        let vault = (self.inner.vault)().ok_or(Error::NotAVault)?;
        let done = self
            .inner
            .state
            .lock()
            .unwrap()
            .done
            .get(&item_id)
            .cloned()
            .ok_or(Error::Import("that item hasn't finished importing"))?;
        let lookup = self.inner.lookup.lock().unwrap().clone();
        match (done, res) {
            (Done::Movie { movie_id }, Resolution::Manual { title, year }) => {
                let mut lib = vault.library();
                let cur = lib.movie(movie_id)?;
                let meta = MovieMeta {
                    title: title.clone(),
                    year: *year,
                    original_title: cur.original_title,
                    runtime_min: cur.runtime_min,
                    overview: cur.overview,
                    genres: cur.genres,
                    tmdb_id: cur.tmdb_id,
                };
                lib.update_movie(movie_id, &meta)?;
            }
            (Done::Movie { movie_id }, Resolution::Candidate { id }) => {
                let Lookup::Online { movies: Some(p), http, .. } = &lookup else {
                    return Err(Error::Import("movie lookups are off"));
                };
                let d = p.movie(id)?;
                vault.library().update_movie(movie_id, &movie_meta(&d))?;
                self.movie_artwork(&vault, http.as_ref(), movie_id, &d);
            }
            (Done::Episode { show_id, .. }, Resolution::Manual { title, year }) => {
                let mut lib = vault.library();
                let cur = lib.show(show_id)?;
                let meta = ShowMeta {
                    title: title.clone(),
                    first_year: *year,
                    original_title: cur.original_title,
                    last_year: cur.last_year,
                    overview: cur.overview,
                    genres: cur.genres,
                    tmdb_id: cur.tmdb_id,
                    tvmaze_id: cur.tvmaze_id,
                };
                lib.update_show(show_id, &meta)?;
            }
            (Done::Episode { show_id, .. }, Resolution::Candidate { id }) => {
                let Lookup::Online { shows, http, .. } = &lookup else {
                    return Err(Error::Import("lookups are off"));
                };
                // Every finished item of this show gets the new details.
                let siblings: Vec<(u64, Id, PlannedEpisode)> = self
                    .inner
                    .state
                    .lock()
                    .unwrap()
                    .done
                    .iter()
                    .filter_map(|(k, d)| match d {
                        Done::Episode { show_id: s, media_file_id, ep } if *s == show_id => {
                            Some((*k, *media_file_id, ep.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                let d = shows.show(id, &[])?;
                vault.library().update_show(show_id, &show_meta(&d))?;
                for (_, media_file_id, ep) in &siblings {
                    for e in episodes_for(&d, ep.season, &ep.episodes, ep.air_date.as_deref()) {
                        let rows = vault.library().episodes_by_media(*media_file_id)?;
                        if let Some(row) =
                            rows.iter().find(|r| r.season_number == e.season && r.number == e.number).or(rows.first())
                        {
                            vault.library().update_episode(
                                row.id,
                                &EpisodeMeta {
                                    number: row.number,
                                    title: e.title.clone(),
                                    overview: e.overview.clone(),
                                    air_date: e.air_date.clone(),
                                    runtime_min: e.runtime_min,
                                },
                            )?;
                            if row.thumb_id.is_none() {
                                if let Some(t) =
                                    e.thumb.as_ref().and_then(|a| self.store_artwork(&vault, http.as_ref(), a))
                                {
                                    vault.library().set_episode_thumb(row.id, t)?;
                                }
                            }
                        }
                    }
                }
                let poster = d.poster.as_ref().and_then(|a| self.store_artwork(&vault, http.as_ref(), a));
                let backdrop = d.backdrop.as_ref().and_then(|a| self.store_artwork(&vault, http.as_ref(), a));
                vault.library().set_show_artwork(show_id, poster, backdrop)?;
                let ids: Vec<u64> = siblings.iter().map(|s| s.0).collect();
                for i in ids {
                    self.update(i, |it| it.match_state = MatchState::Matched);
                }
            }
        }
        vault.save_library()?;
        self.update(item_id, |i| i.match_state = MatchState::Matched);
        Ok(())
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn movie_meta(d: &MovieDetails) -> MovieMeta {
    MovieMeta {
        title: d.title.clone(),
        original_title: d.original_title.clone().filter(|o| *o != d.title),
        year: d.year,
        runtime_min: d.runtime_min,
        overview: d.overview.clone(),
        genres: d.genres.clone(),
        tmdb_id: (d.provider == "tmdb").then(|| d.id.parse().ok()).flatten(),
    }
}

fn show_meta(d: &ShowDetails) -> ShowMeta {
    ShowMeta {
        title: d.title.clone(),
        original_title: d.original_title.clone().filter(|o| *o != d.title),
        first_year: d.first_year,
        last_year: d.last_year,
        overview: d.overview.clone(),
        genres: d.genres.clone(),
        tmdb_id: (d.provider == "tmdb").then(|| d.id.parse().ok()).flatten(),
        tvmaze_id: (d.provider == "tvmaze").then(|| d.id.parse().ok()).flatten(),
    }
}

#[cfg(test)]
mod tests;
