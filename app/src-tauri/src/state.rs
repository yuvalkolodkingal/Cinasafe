//! App state: the unlocked vault, the import queue, the player session, and
//! the things that lock the vault (Lock button, idle timeout, sleep, removal).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use cinasafe_core::drives::RemovalWatch;
use cinasafe_core::importer::{Importer, Lookup, QueueItem};
use cinasafe_core::metadata::{tmdb::Tmdb, tvmaze::TvMaze, MetadataProvider, ReqwestClient};
use cinasafe_core::Vault;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, Result};
use crate::player::Session;
use crate::settings::Settings;

pub type SharedVault = Arc<RwLock<Option<Arc<Vault>>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LockReason {
    User,
    Idle,
    Removed,
    Sleep,
}

pub struct AppState {
    pub app: AppHandle,
    pub vault: SharedVault,
    /// Where the vault lives (the stick's root), once known.
    pub root: Mutex<Option<PathBuf>>,
    pub importer: Importer,
    pub player: Mutex<Option<Session>>,
    pub settings: Mutex<Settings>,
    watch: Mutex<Option<RemovalWatch>>,
    activity: Mutex<Instant>,
    http: OnceLock<Arc<ReqwestClient>>,
    /// Serialises unlock and lock.
    transition: Mutex<()>,
}

impl AppState {
    pub fn new(app: AppHandle) -> Arc<AppState> {
        let vault: SharedVault = Arc::default();
        let getter_vault = vault.clone();
        let emitter = app.clone();
        let importer = Importer::new(
            Arc::new(move || getter_vault.read().unwrap().clone()),
            Lookup::Offline,
            Some(Arc::new(cinasafe_player::MpvProber::default())),
            Arc::new(move |item: &QueueItem| {
                let _ = emitter.emit("import-progress", item);
            }),
        );
        Arc::new(AppState {
            app,
            vault,
            root: Mutex::default(),
            importer,
            player: Mutex::default(),
            settings: Mutex::default(),
            watch: Mutex::default(),
            activity: Mutex::new(Instant::now()),
            http: OnceLock::new(),
            transition: Mutex::default(),
        })
    }

    pub fn vault(&self) -> Result<Arc<Vault>> {
        self.vault.read().unwrap().clone().ok_or_else(AppError::locked)
    }

    pub fn is_unlocked(&self) -> bool {
        self.vault.read().unwrap().is_some()
    }

    pub fn touch(&self) {
        *self.activity.lock().unwrap() = Instant::now();
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    fn http(&self) -> Result<Arc<ReqwestClient>> {
        if let Some(h) = self.http.get() {
            return Ok(h.clone());
        }
        let h = Arc::new(ReqwestClient::new().map_err(AppError::from)?);
        Ok(self.http.get_or_init(|| h).clone())
    }

    /// The metadata sources the settings allow (offline unless turned on).
    pub fn lookup(&self, s: &Settings) -> Lookup {
        if !s.lookup_online {
            return Lookup::Offline;
        }
        let Ok(http) = self.http() else { return Lookup::Offline };
        match s.tmdb_key.as_deref() {
            Some(key) => {
                let t: Arc<dyn MetadataProvider> = Arc::new(Tmdb::new(http.clone(), key, &s.metadata_language));
                Lookup::Online { movies: Some(t.clone()), shows: t, http }
            }
            None => Lookup::Online { movies: None, shows: Arc::new(TvMaze::new(http.clone())), http },
        }
    }

    pub fn apply_settings(&self, s: Settings) {
        self.importer.set_lookup(self.lookup(&s));
        *self.settings.lock().unwrap() = s;
    }

    /// Takes ownership of a freshly unlocked vault.
    pub fn unlocked(self: &Arc<Self>, vault: Vault) {
        let _t = self.transition.lock().unwrap();
        let root = vault.root().to_path_buf();
        let settings = vault.library().settings().map(Settings::from_value).unwrap_or_default();
        *self.vault.write().unwrap() = Some(Arc::new(vault));
        *self.root.lock().unwrap() = Some(root.clone());
        self.apply_settings(settings);
        self.touch();
        let me = Arc::downgrade(self);
        *self.watch.lock().unwrap() = Some(RemovalWatch::start(&root, move || {
            if let Some(me) = me.upgrade() {
                let _ = me.app.emit("drive-removed", serde_json::json!({}));
                // A new thread: lock() drops this watch, whose thread we're on.
                std::thread::spawn(move || me.lock(LockReason::Removed));
            }
        }));
    }

    /// Brief 9.4: stop playback, destroy mpv, drop every reader, zeroize keys
    /// and caches, and tell the UI to return to Unlock.
    pub fn lock(&self, reason: LockReason) {
        let _t = self.transition.lock().unwrap();
        if let Some(session) = self.player.lock().unwrap().take() {
            session.close(reason != LockReason::Removed);
        }
        // Stop the importer: the running file is cancelled and its partial blob removed.
        self.importer.cancel(None);
        self.importer.wait_idle();
        self.importer.clear_all();
        let vault = self.vault.write().unwrap().take();
        if let Some(v) = vault {
            if reason != LockReason::Removed {
                let _ = v.save_library_if_dirty();
            }
            // Dropping the last Arc zeroizes the master key. Other holders
            // (image requests in flight) finish within moments.
            drop(v);
        }
        self.watch.lock().unwrap().take();
        *self.settings.lock().unwrap() = Settings::default();
        let _ = self.app.emit("vault-locked", serde_json::json!({ "reason": reason }));
    }

    fn playing(&self) -> bool {
        self.player.lock().unwrap().as_ref().is_some_and(|s| !s.is_paused())
    }

    /// Idle lock and sleep detection, once a second.
    pub fn start_watchers(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("lock-watch".into())
            .spawn(move || {
                let mut clocks = Clocks::now();
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let Some(me) = me.upgrade() else { return };
                    let slept = clocks.slept();
                    if !me.is_unlocked() {
                        continue;
                    }
                    let s = me.settings();
                    if slept && s.lock_on_sleep {
                        me.lock(LockReason::Sleep);
                        continue;
                    }
                    if me.playing() {
                        me.touch();
                    }
                    let idle = me.activity.lock().unwrap().elapsed();
                    if s.idle_lock_minutes > 0 && idle > Duration::from_secs(s.idle_lock_minutes as u64 * 60) {
                        me.lock(LockReason::Idle);
                    }
                }
            })
            .expect("spawning a thread");
    }
}

/// Detects a suspend: a clock that runs during sleep pulls ahead of one that doesn't.
struct Clocks {
    with_sleep: u64,
    without_sleep: u64,
}

impl Clocks {
    fn now() -> Clocks {
        let (with_sleep, without_sleep) = clock_pair_ms();
        Clocks { with_sleep, without_sleep }
    }

    fn slept(&mut self) -> bool {
        let n = Clocks::now();
        let gap = (n.with_sleep - self.with_sleep).saturating_sub(n.without_sleep - self.without_sleep);
        *self = n;
        gap > 3_000
    }
}

#[cfg(target_os = "linux")]
fn clock_pair_ms() -> (u64, u64) {
    fn ms(clock: libc::clockid_t) -> u64 {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: valid out-pointer.
        unsafe { libc::clock_gettime(clock, &mut ts) };
        ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000
    }
    (ms(libc::CLOCK_BOOTTIME), ms(libc::CLOCK_MONOTONIC))
}

#[cfg(windows)]
fn clock_pair_ms() -> (u64, u64) {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    use windows_sys::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;
    let mut unbiased = 0u64;
    // SAFETY: valid out-pointer; both calls are infallible on supported Windows.
    unsafe { QueryUnbiasedInterruptTime(&mut unbiased) };
    (unsafe { GetTickCount64() }, unbiased / 10_000)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn clock_pair_ms() -> (u64, u64) {
    (0, 0)
}

/// Runs `f` on a blocking thread with the app state (for slow commands).
pub async fn blocking<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(Arc<AppState>) -> Result<T> + Send + 'static,
) -> Result<T> {
    let state = app.state::<Arc<AppState>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(state))
        .await
        .map_err(|e| AppError::new("internal", e.to_string()))?
}
