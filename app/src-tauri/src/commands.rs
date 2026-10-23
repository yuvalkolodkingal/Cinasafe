//! Tauri commands (brief section 11; contract in app/ui/src/api/types.ts).
//!
//! Every command is `async` so it runs off the GTK/Win32 main thread; slow
//! ones (key derivation, encryption, dialogs) go through `state::blocking`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cinasafe_core::drives::{self, Drive};
use cinasafe_core::importer::{ImportStatusItems, Lookup, QueueItem, Resolution};
use cinasafe_core::library::{EpisodeMeta, Id, Movie, MovieMeta, SearchResults, Show, ShowMeta, SortBy, WatchFilter};
use cinasafe_core::metadata::identify::Scored;
use cinasafe_core::metadata::matching;
use cinasafe_core::{Password, RecoveryWords, Vault};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

use crate::dto::{self, EpisodeDetail, Grid, Home, MovieDetail, RecentItem, SeasonDetail, ShowDetail};
use crate::error::{AppError, Result};
use crate::player::{PlayerCommand, PlayerState, Session};
use crate::settings::Settings;
use crate::state::{blocking, AppState, LockReason};

type St<'a> = State<'a, Arc<AppState>>;

const MIN_PASSWORD_CHARS: usize = 8;

// ---------- vault ----------

/// Removable drives. Debug builds can add a folder posing as an exFAT stick
/// (`CINASAFE_TEST_DRIVE=<dir>`), to test the wizard without hardware.
fn removable_drives() -> std::result::Result<Vec<Drive>, drives::DriveError> {
    #[allow(unused_mut)]
    let mut list =
        drives::list_removable().or_else(|e| if cfg!(debug_assertions) { Ok(Vec::new()) } else { Err(e) })?;
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("CINASAFE_TEST_DRIVE").map(PathBuf::from) {
        list.push(Drive {
            id: "test-drive".into(),
            name: "Test stick".into(),
            label: Some("TESTSTICK".into()),
            size_bytes: 32_000_000_000,
            free_bytes: drives::free_space(&dir).ok(),
            filesystem: Some("exfat".into()),
            has_vault: Vault::detect(&dir).is_some(),
            mount_path: Some(dir),
            status: drives::DriveStatus::Ready,
        });
    }
    Ok(list)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSummary {
    root: PathBuf,
    label: Option<String>,
    size_bytes: u64,
    used_bytes: u64,
    free_bytes: Option<u64>,
    version: u32,
    supported: bool,
    has_recovery: bool,
    created_at: String,
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Places the app may have been started from, nearest first.
fn candidate_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(r) = std::env::var_os("CINASAFE_VAULT_ROOT") {
        out.push(PathBuf::from(r));
    }
    // An AppImage runs from a temporary mount; $APPIMAGE is the file on the stick.
    if let Some(a) = std::env::var_os("APPIMAGE") {
        out.extend(Path::new(&a).parent().map(Path::to_path_buf));
    }
    if let Ok(exe) = std::env::current_exe() {
        out.extend(exe.ancestors().skip(1).take(3).map(Path::to_path_buf));
    }
    out
}

fn summarize(root: &Path, drives: &[Drive]) -> Option<VaultSummary> {
    let info = Vault::detect(root)?;
    let drive = drives.iter().find(|d| d.mount_path.as_deref() == Some(root));
    let free = drives::free_space(root).ok();
    let used = dir_size(&Vault::vault_dir(root));
    Some(VaultSummary {
        root: root.to_path_buf(),
        label: drive.and_then(|d| d.label.clone()),
        size_bytes: drive.map_or(used + free.unwrap_or(0), |d| d.size_bytes),
        used_bytes: used,
        free_bytes: free,
        version: info.version,
        supported: info.supported,
        has_recovery: info.has_recovery,
        created_at: info.created_at,
    })
}

#[tauri::command]
pub async fn vault_detect(app: AppHandle) -> Result<Option<VaultSummary>> {
    blocking(&app, |st| {
        let drives = removable_drives().unwrap_or_default();
        let known = st.root.lock().unwrap().clone();
        let roots = known
            .into_iter()
            .chain(candidate_roots())
            .chain(drives.iter().filter(|d| d.has_vault).filter_map(|d| d.mount_path.clone()));
        for r in roots {
            if let Some(s) = summarize(&r, &drives) {
                *st.root.lock().unwrap() = Some(r);
                return Ok(Some(s));
            }
        }
        Ok(None)
    })
    .await
}

#[derive(Serialize)]
pub struct Status {
    unlocked: bool,
}

#[tauri::command]
pub async fn vault_status(state: St<'_>) -> Result<Status> {
    Ok(Status { unlocked: state.is_unlocked() })
}

#[tauri::command]
pub async fn drives_list(app: AppHandle) -> Result<Vec<Drive>> {
    blocking(&app, |_| Ok(removable_drives()?)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockResult {
    restored_from_backup: bool,
}

fn vault_root(st: &AppState) -> Result<PathBuf> {
    st.root.lock().unwrap().clone().ok_or_else(|| AppError::new("notAVault", "no vault found"))
}

#[tauri::command]
pub async fn vault_unlock(app: AppHandle, password: String) -> Result<UnlockResult> {
    let password: Password = Zeroizing::new(password);
    blocking(&app, move |st| {
        let v = Vault::unlock_with_password(&vault_root(&st)?, &password)?;
        let restored = v.library_restored_from_backup();
        st.unlocked(v);
        Ok(UnlockResult { restored_from_backup: restored })
    })
    .await
}

#[tauri::command]
pub async fn vault_unlock_recovery(app: AppHandle, words: String) -> Result<UnlockResult> {
    let words = Zeroizing::new(words);
    blocking(&app, move |st| {
        let w = RecoveryWords::parse(&words)?;
        let v = Vault::unlock_with_recovery(&vault_root(&st)?, &w)?;
        let restored = v.library_restored_from_backup();
        st.unlocked(v);
        Ok(UnlockResult { restored_from_backup: restored })
    })
    .await
}

#[tauri::command]
pub async fn vault_lock(app: AppHandle) -> Result<()> {
    blocking(&app, |st| {
        st.lock(LockReason::User);
        Ok(())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    created_at: String,
    has_recovery: bool,
    size_bytes: u64,
    used_bytes: u64,
    free_bytes: Option<u64>,
    movie_count: u32,
    movie_bytes: u64,
    show_count: u32,
    show_bytes: u64,
}

#[tauri::command]
pub async fn vault_info(app: AppHandle) -> Result<VaultInfo> {
    blocking(&app, |st| {
        let v = st.vault()?;
        let (movie_count, movie_bytes, show_count, show_bytes) = v.library().totals()?;
        let used = v.used_bytes()?;
        let free = drives::free_space(v.root()).ok();
        let size = removable_drives()
            .unwrap_or_default()
            .into_iter()
            .find(|d| d.mount_path.as_deref() == Some(v.root()))
            .map_or(used + free.unwrap_or(0), |d| d.size_bytes);
        Ok(VaultInfo {
            created_at: v.created_at(),
            has_recovery: v.has_recovery(),
            size_bytes: size,
            used_bytes: used,
            free_bytes: free,
            movie_count,
            movie_bytes,
            show_count,
            show_bytes,
        })
    })
    .await
}

fn check_new_password(p: &str) -> Result<()> {
    if p.chars().count() < MIN_PASSWORD_CHARS {
        return Err(AppError::new("weakPassword", "use at least 8 characters"));
    }
    Ok(())
}

#[tauri::command]
pub async fn vault_change_password(app: AppHandle, old_password: String, new_password: String) -> Result<()> {
    let (old, new) = (Zeroizing::new(old_password), Zeroizing::new(new_password));
    blocking(&app, move |st| {
        check_new_password(&new)?;
        let v = st.vault()?;
        if !v.check_password(&old)? {
            return Err(AppError::new("wrongPassword", "wrong password"));
        }
        v.change_password(&new)?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn vault_replace_recovery(app: AppHandle, password: String) -> Result<Vec<String>> {
    let password = Zeroizing::new(password);
    blocking(&app, move |st| {
        let v = st.vault()?;
        if !v.check_password(&password)? {
            return Err(AppError::new("wrongPassword", "wrong password"));
        }
        let words = v.replace_recovery()?;
        Ok(words.words().iter().map(|w| w.to_string()).collect())
    })
    .await
}

/// Used by the wizard's "Skip" on the recovery-key step (the vault was
/// created with a key so its words could be shown) and by Settings.
#[tauri::command]
pub async fn vault_remove_recovery(app: AppHandle) -> Result<()> {
    blocking(&app, |st| Ok(st.vault()?.remove_recovery()?)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    checked: u64,
    bytes: u64,
    corrupt_count: usize,
    missing_count: usize,
    orphaned_count: usize,
    library_restored_from_backup: bool,
}

#[tauri::command]
pub async fn vault_integrity_check(app: AppHandle) -> Result<IntegrityReport> {
    let emitter = app.clone();
    blocking(&app, move |st| {
        let v = st.vault()?;
        let r = v.integrity_check(|p| {
            let _ = emitter.emit("integrity-progress", p);
        })?;
        Ok(IntegrityReport {
            checked: r.checked,
            bytes: r.bytes,
            corrupt_count: r.corrupt.len(),
            missing_count: r.missing.len(),
            orphaned_count: r.orphaned.len(),
            library_restored_from_backup: r.library_restored_from_backup,
        })
    })
    .await
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CreateMode {
    UseAsIs,
    Format,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResult {
    root: PathBuf,
    recovery_words: Option<Vec<String>>,
}

fn create_step(app: &AppHandle, step: &str) {
    let _ = app.emit("create-progress", serde_json::json!({ "step": step }));
}

#[tauri::command]
pub async fn vault_create(
    app: AppHandle,
    drive_id: String,
    mode: CreateMode,
    label: Option<String>,
    password: String,
    with_recovery: bool,
) -> Result<CreateResult> {
    let password = Zeroizing::new(password);
    let emitter = app.clone();
    blocking(&app, move |st| {
        check_new_password(&password)?;
        let drive = removable_drives()?
            .into_iter()
            .find(|d| d.id == drive_id)
            .ok_or_else(|| AppError::new("notFound", "that drive is gone"))?;
        let root = match mode {
            CreateMode::Format => {
                create_step(&emitter, "formatting");
                drives::format_exfat(&drive, label.as_deref().unwrap_or("CINASAFE"))?
            }
            CreateMode::UseAsIs => {
                if drive.status != drives::DriveStatus::Ready {
                    return Err(AppError::new("formatFailed", "this drive must be formatted to exFAT first"));
                }
                drive.mount_path.clone().ok_or_else(|| AppError::new("io", "drive not mounted"))?
            }
        };
        create_step(&emitter, "deriving");
        let (vault, words) = Vault::create(&root, &password, with_recovery)?;
        create_step(&emitter, "writing");
        vault.save_library()?;
        create_step(&emitter, "copyingApps");
        if let Err(e) = crate::apps::copy_player_apps(&root) {
            eprintln!("cinasafe: copying the player apps failed: {e}");
        }
        st.unlocked(vault);
        create_step(&emitter, "done");
        Ok(CreateResult { root, recovery_words: words.map(|w| w.words().iter().map(|s| s.to_string()).collect()) })
    })
    .await
}

#[tauri::command]
pub async fn eject_drive(app: AppHandle) -> Result<()> {
    blocking(&app, |st| {
        let root = st.root.lock().unwrap().clone();
        st.lock(LockReason::User);
        if let Some(root) = root {
            crate::apps::eject(&root)?;
        }
        Ok(())
    })
    .await
}

// ---------- library ----------

#[tauri::command]
pub async fn library_home(state: St<'_>) -> Result<Home> {
    let v = state.vault()?;
    let lib = v.library();
    let continue_watching = lib.continue_watching(20)?;
    let up_next = lib.up_next(20)?;
    let (rm, rs) = lib.recently_added(20)?;
    let mut recently_added: Vec<(String, RecentItem)> = rm
        .into_iter()
        .map(|m| (m.added_at.clone(), RecentItem::Movie { movie: m }))
        .chain(rs.into_iter().map(|s| (s.added_at.clone(), RecentItem::Show { show: s })))
        .collect();
    recently_added.sort_by(|a, b| b.0.cmp(&a.0));
    recently_added.truncate(20);
    let mut movies = lib.movies(WatchFilter::All, None, SortBy::Title)?;
    movies.truncate(30);
    let mut shows = lib.shows(WatchFilter::All, None, SortBy::Title)?;
    shows.truncate(30);
    let (mc, _, sc, _) = lib.totals()?;
    Ok(Home {
        hero: continue_watching.first().or(up_next.first()).cloned(),
        continue_watching,
        up_next,
        recently_added: recently_added.into_iter().map(|(_, r)| r).collect(),
        movies,
        shows,
        empty: mc == 0 && sc == 0,
    })
}

#[tauri::command]
pub async fn library_movies(
    state: St<'_>,
    filter: WatchFilter,
    genre: Option<String>,
    sort: SortBy,
) -> Result<Grid<Movie>> {
    let v = state.vault()?;
    let lib = v.library();
    let (count, bytes, _, _) = lib.totals()?;
    Ok(Grid {
        items: lib.movies(filter, genre.as_deref(), sort)?,
        total_count: count,
        total_bytes: bytes,
        genres: lib.genres()?,
    })
}

#[tauri::command]
pub async fn library_shows(
    state: St<'_>,
    filter: WatchFilter,
    genre: Option<String>,
    sort: SortBy,
) -> Result<Grid<Show>> {
    let v = state.vault()?;
    let lib = v.library();
    let (_, _, count, bytes) = lib.totals()?;
    Ok(Grid {
        items: lib.shows(filter, genre.as_deref(), sort)?,
        total_count: count,
        total_bytes: bytes,
        genres: lib.genres()?,
    })
}

#[tauri::command]
pub async fn library_movie(state: St<'_>, id: Id) -> Result<MovieDetail> {
    let v = state.vault()?;
    let lib = v.library();
    let movie = lib.movie(id)?;
    let file = dto::media_info(&v, &lib, lib.media_file(movie.media_file_id)?)?;
    Ok(MovieDetail { movie, file })
}

#[tauri::command]
pub async fn library_show(state: St<'_>, id: Id) -> Result<ShowDetail> {
    let v = state.vault()?;
    let lib = v.library();
    Ok(ShowDetail { show: lib.show(id)?, seasons: lib.seasons(id)?, resume: dto::resume_episode(&lib, id)? })
}

#[tauri::command]
pub async fn library_season(state: St<'_>, id: Id) -> Result<SeasonDetail> {
    let v = state.vault()?;
    let lib = v.library();
    let episodes = lib.episodes(id)?;
    let season = lib
        .seasons(episodes.first().map(|e| e.show_id).unwrap_or_default())?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(AppError::not_found)?;
    let up_next_episode_id = dto::resume_episode(&lib, season.show_id)?.map(|e| e.id);
    Ok(SeasonDetail { season, episodes, up_next_episode_id })
}

#[tauri::command]
pub async fn library_episode(state: St<'_>, id: Id) -> Result<EpisodeDetail> {
    let v = state.vault()?;
    let lib = v.library();
    let episode = lib.episode(id)?;
    let show_title = lib.show(episode.show_id)?.title;
    let file = dto::media_info(&v, &lib, lib.media_file(episode.media_file_id)?)?;
    Ok(EpisodeDetail { episode, show_title, file })
}

fn patch_str(p: &Value, k: &str, cur: Option<String>) -> Option<String> {
    match p.get(k) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Some(Value::String(_) | Value::Null) => None,
        _ => cur,
    }
}

fn patch_num<T: TryFrom<u64>>(p: &Value, k: &str, cur: Option<T>) -> Option<T> {
    match p.get(k) {
        Some(Value::Number(n)) => n.as_u64().and_then(|n| T::try_from(n).ok()),
        Some(Value::Null) => None,
        _ => cur,
    }
}

fn patch_genres(p: &Value, cur: Vec<String>) -> Vec<String> {
    match p.get("genres") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        _ => cur,
    }
}

#[tauri::command]
pub async fn library_update_metadata(state: St<'_>, kind: String, id: Id, patch: Value) -> Result<()> {
    let v = state.vault()?;
    {
        let mut lib = v.library();
        match kind.as_str() {
            "movie" => {
                let m = lib.movie(id)?;
                let meta = MovieMeta {
                    title: patch_str(&patch, "title", Some(m.title.clone())).unwrap_or(m.title),
                    original_title: patch_str(&patch, "originalTitle", m.original_title),
                    year: patch_num(&patch, "year", m.year),
                    runtime_min: patch_num(&patch, "runtimeMin", m.runtime_min),
                    overview: patch_str(&patch, "overview", m.overview),
                    genres: patch_genres(&patch, m.genres),
                    tmdb_id: m.tmdb_id,
                };
                lib.update_movie(id, &meta)?;
            }
            "show" => {
                let s = lib.show(id)?;
                let meta = ShowMeta {
                    title: patch_str(&patch, "title", Some(s.title.clone())).unwrap_or(s.title),
                    original_title: patch_str(&patch, "originalTitle", s.original_title),
                    first_year: patch_num(&patch, "year", s.first_year),
                    last_year: patch_num(&patch, "lastYear", s.last_year),
                    overview: patch_str(&patch, "overview", s.overview),
                    genres: patch_genres(&patch, s.genres),
                    tmdb_id: s.tmdb_id,
                    tvmaze_id: s.tvmaze_id,
                };
                lib.update_show(id, &meta)?;
            }
            "episode" => {
                let e = lib.episode(id)?;
                let meta = EpisodeMeta {
                    number: patch_num(&patch, "number", Some(e.number)).unwrap_or(e.number),
                    title: patch_str(&patch, "title", e.title),
                    overview: patch_str(&patch, "overview", e.overview),
                    air_date: patch_str(&patch, "airDate", e.air_date),
                    runtime_min: patch_num(&patch, "runtimeMin", e.runtime_min),
                };
                lib.update_episode(id, &meta)?;
            }
            _ => return Err(AppError::new("internal", "unknown kind")),
        }
    }
    v.save_library()?;
    Ok(())
}

#[tauri::command]
pub async fn library_set_overrides(
    state: St<'_>,
    kind: String,
    id: Id,
    audio: Option<String>,
    subtitles: Option<String>,
) -> Result<()> {
    let v = state.vault()?;
    match kind.as_str() {
        "movie" => v.library().set_movie_overrides(id, audio.as_deref(), subtitles.as_deref())?,
        "show" => v.library().set_show_overrides(id, audio.as_deref(), subtitles.as_deref())?,
        _ => return Err(AppError::new("internal", "unknown kind")),
    }
    v.save_library()?;
    Ok(())
}

#[tauri::command]
pub async fn library_mark_watched(state: St<'_>, media_file_id: Id, watched: bool) -> Result<()> {
    let v = state.vault()?;
    v.library().mark_watched(media_file_id, watched)?;
    v.save_library()?;
    Ok(())
}

#[tauri::command]
pub async fn library_mark_season_watched(state: St<'_>, season_id: Id, watched: bool) -> Result<()> {
    let v = state.vault()?;
    v.library().mark_season_watched(season_id, watched)?;
    v.save_library()?;
    Ok(())
}

#[tauri::command]
pub async fn library_remove(app: AppHandle, kind: String, id: Id) -> Result<()> {
    blocking(&app, move |st| {
        let v = st.vault()?;
        let media: Vec<Id> = {
            let lib = v.library();
            match kind.as_str() {
                "movie" => vec![lib.movie(id)?.media_file_id],
                "episode" => vec![lib.episode(id)?.media_file_id],
                "show" => {
                    let mut ids = Vec::new();
                    for s in lib.seasons(id)? {
                        ids.extend(lib.episodes(s.id)?.into_iter().map(|e| e.media_file_id));
                    }
                    ids.sort_unstable();
                    ids.dedup();
                    ids
                }
                _ => return Err(AppError::new("internal", "unknown kind")),
            }
        };
        if let Some(s) = st.player.lock().unwrap().as_ref() {
            if media.contains(&s.media_file_id()) {
                return Err(AppError::new("internal", "stop playback first"));
            }
        }
        let mut blobs = Vec::new();
        for m in media {
            blobs.extend(v.library().remove_media_file(m)?);
        }
        // The library forgets them first, so an interruption leaves orphans
        // (reported by the integrity check), never dangling references.
        v.save_library()?;
        for b in blobs {
            let _ = v.delete_blob(b);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn library_export(app: AppHandle, media_file_id: Id) -> Result<Option<PathBuf>> {
    let dialog_app = app.clone();
    blocking(&app, move |st| {
        let v = st.vault()?;
        let f = v.library().media_file(media_file_id)?;
        let Some(dest) = dialog_app.dialog().file().set_file_name(&f.original_name).blocking_save_file() else {
            return Ok(None);
        };
        let dest = dest.into_path().map_err(|e| AppError::new("io", e.to_string()))?;
        let mut r = v.open_blob(f.blob_id)?;
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut r, &mut out)?;
        out.sync_all()?;
        Ok(Some(dest))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    #[serde(flatten)]
    results: SearchResults,
    show_titles: std::collections::HashMap<String, String>,
}

#[tauri::command]
pub async fn search(state: St<'_>, query: String) -> Result<Search> {
    let v = state.vault()?;
    let lib = v.library();
    let results = lib.search(&query, 20)?;
    let mut show_titles = std::collections::HashMap::new();
    for e in &results.episodes {
        if let std::collections::hash_map::Entry::Vacant(slot) = show_titles.entry(e.show_id.to_string()) {
            slot.insert(lib.show(e.show_id)?.title);
        }
    }
    Ok(Search { results, show_titles })
}

// ---------- import ----------

#[tauri::command]
pub async fn import_add(app: AppHandle, paths: Vec<PathBuf>) -> Result<Vec<QueueItem>> {
    blocking(&app, move |st| {
        st.vault()?;
        Ok(st.importer.add(&paths)?)
    })
    .await
}

#[tauri::command]
pub async fn import_browse(app: AppHandle, folders: bool) -> Result<Vec<QueueItem>> {
    let dialog_app = app.clone();
    blocking(&app, move |st| {
        st.vault()?;
        let picker = dialog_app.dialog().file();
        let picked = if folders { picker.blocking_pick_folders() } else { picker.blocking_pick_files() };
        let paths: Vec<PathBuf> = picked.unwrap_or_default().into_iter().filter_map(|p| p.into_path().ok()).collect();
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        Ok(st.importer.add(&paths)?)
    })
    .await
}

#[tauri::command]
pub async fn import_status(state: St<'_>) -> Result<ImportStatusItems> {
    Ok(state.importer.status())
}

#[tauri::command]
pub async fn import_pause(state: St<'_>, app: AppHandle) -> Result<()> {
    state.importer.pause();
    let _ = app.emit("import-queue", serde_json::json!({ "paused": true }));
    Ok(())
}

#[tauri::command]
pub async fn import_resume(state: St<'_>, app: AppHandle) -> Result<()> {
    state.importer.resume();
    let _ = app.emit("import-queue", serde_json::json!({ "paused": false }));
    Ok(())
}

#[tauri::command]
pub async fn import_cancel(state: St<'_>, id: Option<u64>) -> Result<()> {
    state.importer.cancel(id);
    Ok(())
}

#[tauri::command]
pub async fn import_clear_finished(state: St<'_>) -> Result<()> {
    state.importer.clear_finished();
    Ok(())
}

#[tauri::command]
pub async fn import_resolve(app: AppHandle, item_id: u64, resolution: Resolution) -> Result<()> {
    blocking(&app, move |st| Ok(st.importer.resolve(item_id, &resolution)?)).await
}

#[tauri::command]
pub async fn tmdb_search(app: AppHandle, kind: String, query: String, year: Option<u16>) -> Result<Vec<Scored>> {
    blocking(&app, move |st| {
        let Lookup::Online { movies, shows, .. } = st.lookup(&st.settings()) else {
            return Err(AppError::new("lookupsOff", "online lookups are off"));
        };
        let hits = match kind.as_str() {
            "movie" => movies
                .ok_or_else(|| AppError::new("lookupsOff", "movies need a TMDB key"))?
                .search_movie(&query, year)?,
            _ => shows.search_show(&query, year)?,
        };
        Ok(matching::rank(&query, year, hits).into_iter().map(Scored::from).collect())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    size_bytes: u64,
    used_bytes: u64,
    free_bytes: Option<u64>,
    queued_bytes: u64,
}

#[tauri::command]
pub async fn storage(app: AppHandle) -> Result<Storage> {
    blocking(&app, |st| {
        let v = st.vault()?;
        let used = v.used_bytes()?;
        let free = drives::free_space(v.root()).ok();
        let size = removable_drives()
            .unwrap_or_default()
            .into_iter()
            .find(|d| d.mount_path.as_deref() == Some(v.root()))
            .map_or(used + free.unwrap_or(0), |d| d.size_bytes);
        Ok(Storage { size_bytes: size, used_bytes: used, free_bytes: free, queued_bytes: st.importer.queued_bytes() })
    })
    .await
}

// ---------- player ----------

fn open_session(st: &AppState, media_file_id: Id, from_start: bool) -> Result<PlayerState> {
    let v = st.vault()?;
    if let Some(old) = st.player.lock().unwrap().take() {
        old.close(true);
    }
    let session = Session::open(&st.app, v, &st.settings(), media_file_id, from_start)?;
    let state = session.state();
    *st.player.lock().unwrap() = Some(session);
    st.touch();
    Ok(state)
}

#[tauri::command]
pub async fn player_open(app: AppHandle, media_file_id: Id, from_start: bool) -> Result<PlayerState> {
    blocking(&app, move |st| open_session(&st, media_file_id, from_start)).await
}

#[tauri::command]
pub async fn player_command(app: AppHandle, cmd: PlayerCommand) -> Result<()> {
    blocking(&app, move |st| {
        if matches!(cmd, PlayerCommand::Next) {
            let next = st.player.lock().unwrap().as_ref().and_then(Session::next);
            let next = next.ok_or_else(AppError::not_found)?;
            open_session(&st, next, false)?;
            return Ok(());
        }
        let guard = st.player.lock().unwrap();
        let s = guard.as_ref().ok_or_else(|| AppError::new("player", "nothing is playing"))?;
        s.command(cmd)
    })
    .await
}

#[tauri::command]
pub async fn player_close(app: AppHandle) -> Result<()> {
    blocking(&app, |st| {
        if let Some(s) = st.player.lock().unwrap().take() {
            s.close(true);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn player_state(state: St<'_>) -> Result<Option<PlayerState>> {
    Ok(state.player.lock().unwrap().as_ref().map(Session::state))
}

// ---------- settings and app ----------

#[tauri::command]
pub async fn settings_get(state: St<'_>) -> Result<Settings> {
    state.vault()?;
    Ok(state.settings())
}

#[tauri::command]
pub async fn settings_set(state: St<'_>, patch: Value) -> Result<Settings> {
    let v = state.vault()?;
    let s = state.settings().merged(patch);
    v.library().set_settings(&serde_json::to_value(&s).map_err(|e| AppError::new("internal", e.to_string()))?)?;
    v.save_library()?;
    state.apply_settings(s.clone());
    Ok(s)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    platform: String,
    mpv_version: Option<String>,
    player_apps: Vec<crate::apps::PlayerApp>,
}

#[tauri::command]
pub async fn app_info(app: AppHandle) -> Result<AppInfo> {
    let version = app.package_info().version.to_string();
    blocking(&app, move |st| {
        let mpv_version = cinasafe_player::Mpv::new().ok().and_then(|m| m.get_string("mpv-version").ok());
        let root = st.root.lock().unwrap().clone();
        Ok(AppInfo {
            version,
            platform: crate::apps::platform().to_owned(),
            mpv_version,
            player_apps: root.map(|r| crate::apps::list(&r)).unwrap_or_default(),
        })
    })
    .await
}

#[tauri::command]
pub async fn update_player_apps(app: AppHandle) -> Result<()> {
    blocking(&app, |st| {
        let root = st.vault()?.root().to_path_buf();
        crate::apps::copy_player_apps(&root).map_err(|e| match e.kind() {
            std::io::ErrorKind::Unsupported => AppError::new("notPortable", e.to_string()),
            _ => e.into(),
        })
    })
    .await
}

#[tauri::command]
pub async fn activity(state: St<'_>) -> Result<()> {
    state.touch();
    Ok(())
}
