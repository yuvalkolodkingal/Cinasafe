//! Milestone 0 spike: put libmpv video inside the Tauri window with the web UI
//! drawn on top (brief section 6.3). Results are recorded in docs/SPIKE.md.
//!
//! Environment:
//! - `CINASAFE_SPIKE_ROOT`  spike vault root (created by `cinasafe spike-init`)
//! - `CINASAFE_SPIKE_BLOB`  blob id to autoplay
//! - `CINASAFE_SPIKE_SUB`   optional subtitle blob id
//! - `CINASAFE_SPIKE_MODE`  `gl-overlay` (Linux default), `wid` (Windows default), `separate`
//! - `CINASAFE_SPIKE_EXIT_AFTER`  seconds; quit and print stats (for automated runs)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod win;

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cinasafe_core::spike::SpikeVault;
use cinasafe_core::BlobId;
use cinasafe_player::{ffi, Event, MediaStream, Mpv, PropertyValue, StreamOpener};
use tauri::{Emitter, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// mpv render API into a GL surface under a transparent webview (one window).
    GlOverlay,
    /// mpv draws into a native child window via `--wid`.
    Wid,
    /// mpv's own borderless window behind a transparent Tauri window.
    Separate,
}

impl Mode {
    fn from_env() -> Mode {
        match std::env::var("CINASAFE_SPIKE_MODE").as_deref() {
            Ok("gl-overlay") => Mode::GlOverlay,
            Ok("wid") => Mode::Wid,
            Ok("separate") => Mode::Separate,
            _ if cfg!(windows) => Mode::Wid,
            _ => Mode::GlOverlay,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mode::GlOverlay => "gl-overlay",
            Mode::Wid => "wid",
            Mode::Separate => "separate",
        }
    }
}

/// Counters shown in the overlay and printed at exit.
#[derive(Default)]
pub struct RenderStats {
    pub renders: AtomicU64,
    pub render_us_total: AtomicU64,
}

pub struct Spike {
    pub mpv: Arc<Mpv>,
    pub mode: Mode,
    pub stats: Arc<RenderStats>,
    pub started: Instant,
    blob: Option<BlobId>,
    sub: Option<BlobId>,
}

impl Spike {
    /// Initialise mpv and start playback; called by the platform code once the
    /// video surface exists.
    pub fn start(&self) {
        if let Err(e) = self.mpv.initialize() {
            eprintln!("[spike] mpv initialize failed: {e}");
            return;
        }
        if let Some(id) = self.blob {
            if let Err(e) = self.mpv.command(&["loadfile", &format!("cinasafe://{}", id.to_hex())]) {
                eprintln!("[spike] loadfile failed: {e}");
            }
        }
    }
}

/// Without a test vault the app still starts; nothing can be opened.
struct Opener(Option<SpikeVault>);

impl StreamOpener for Opener {
    fn open(&self, id: BlobId) -> io::Result<Box<dyn MediaStream>> {
        let vault = self.0.as_ref().ok_or(io::ErrorKind::NotFound)?;
        Ok(Box::new(vault.open_blob(id).map_err(io::Error::other)?))
    }
}

#[tauri::command]
fn mpv_command(spike: tauri::State<'_, Arc<Spike>>, args: Vec<String>) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    spike.mpv.command(&args).map_err(|e| e.to_string())
}

#[tauri::command]
fn spike_info(spike: tauri::State<'_, Arc<Spike>>) -> serde_json::Value {
    let mpv = &spike.mpv;
    let renders = spike.stats.renders.load(Ordering::Relaxed);
    let secs = spike.started.elapsed().as_secs_f64();
    serde_json::json!({
        "mode": spike.mode.name(),
        "renders": renders,
        "app_renders_per_s": (renders as f64 / secs * 10.0).round() / 10.0,
        "avg_render_ms": if renders > 0 { spike.stats.render_us_total.load(Ordering::Relaxed) as f64 / renders as f64 / 1000.0 } else { 0.0 },
        "vo": mpv.get_string("current-vo").unwrap_or_default(),
        "est_vf_fps": mpv.get_f64("estimated-vf-fps").unwrap_or(0.0),
        "frame_drops": mpv.get_i64("frame-drop-count").unwrap_or(-1),
        "decoder_drops": mpv.get_i64("decoder-frame-drop-count").unwrap_or(-1),
        "hwdec": mpv.get_string("hwdec-current").unwrap_or_default(),
        "duration": mpv.get_f64("duration").unwrap_or(0.0),
    })
}

fn env_blob(name: &str) -> Option<BlobId> {
    let v = std::env::var(name).ok()?;
    let id = BlobId::from_hex(&v);
    if id.is_none() {
        eprintln!("[spike] {name} is not a blob id");
    }
    id
}

fn event_loop(app: tauri::AppHandle, mpv: Arc<Mpv>) {
    for (i, name) in ["time-pos", "duration", "pause"].iter().enumerate() {
        let fmt = if *name == "pause" { ffi::MPV_FORMAT_FLAG } else { ffi::MPV_FORMAT_DOUBLE };
        let _ = mpv.observe(i as u64 + 1, name, fmt);
    }
    loop {
        match mpv.wait_event(Duration::from_secs(1)) {
            Event::Shutdown => break,
            Event::PropertyChange { name, value, .. } => {
                let value = match value {
                    PropertyValue::Double(d) => serde_json::json!(d),
                    PropertyValue::Flag(b) => serde_json::json!(b),
                    _ => serde_json::Value::Null,
                };
                let _ = app.emit("mpv-prop", serde_json::json!({ "name": name, "value": value }));
            }
            Event::EndFile { reason, error } if error < 0 => {
                eprintln!("[spike] playback failed: reason {reason}, error {error}");
            }
            _ => {}
        }
    }
}

#[tauri::command]
fn unavailable_info(error: tauri::State<'_, String>) -> serde_json::Value {
    serde_json::json!({ "mode": "unavailable", "error": error.inner() })
}

#[tauri::command]
fn unavailable_command(error: tauri::State<'_, String>) -> Result<(), String> {
    Err(error.inner().clone())
}

fn run_without_player(error: String) {
    tauri::Builder::default()
        .setup(|app| {
            // No video underneath: a transparent window would show nothing (or the desktop).
            if let Some(w) = app.get_webview_window("main") {
                w.set_background_color(Some(tauri::window::Color(17, 17, 17, 255)))?;
            }
            Ok(())
        })
        .manage(error)
        .invoke_handler(tauri::generate_handler![unavailable_info, unavailable_command])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// brief 6.3 option 3: mpv keeps its own borderless window, glued under a
/// transparent, always-on-top Tauri window that carries the controls.
pub fn attach_separate(window: &tauri::WebviewWindow, spike: Arc<Spike>) {
    fn geometry(w: &tauri::WebviewWindow) -> Option<String> {
        let p = w.inner_position().ok()?;
        let s = w.inner_size().ok()?;
        Some(format!("{}x{}+{}+{}", s.width, s.height, p.x, p.y))
    }
    let _ = window.set_always_on_top(true);
    if let Some(g) = geometry(window) {
        let _ = spike.mpv.set_option("geometry", &g);
    }
    spike.start();
    let w = window.clone();
    window.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
            if let Some(g) = geometry(&w) {
                let _ = spike.mpv.set_property("geometry", &g);
            }
        }
    });
}

fn main() {
    let mode = Mode::from_env();
    let root = PathBuf::from(std::env::var("CINASAFE_SPIKE_ROOT").unwrap_or_else(|_| ".".into()));
    let vault =
        SpikeVault::open(&root).inspect_err(|e| eprintln!("[spike] no spike vault at {}: {e}", root.display())).ok();

    let mut mpv = match Mpv::new() {
        Ok(m) => m,
        Err(e) => {
            // Start anyway so the window can say what is missing (e.g. libmpv-2.dll).
            eprintln!("[spike] {e}");
            return run_without_player(e.to_string());
        }
    };
    let set = |k: &str, v: &str| {
        if let Err(e) = mpv.set_option(k, v) {
            eprintln!("[spike] {e}");
        }
    };
    set("keep-open", "always");
    set("idle", "yes");
    if let Ok(ao) = std::env::var("CINASAFE_SPIKE_AO") {
        set("ao", &ao);
    }
    // Extra mpv options for experiments: `CINASAFE_SPIKE_MPV="name=value name=value"`.
    for kv in std::env::var("CINASAFE_SPIKE_MPV").unwrap_or_default().split_whitespace() {
        if let Some((k, v)) = kv.split_once('=') {
            set(k, v);
        }
    }
    match mode {
        Mode::GlOverlay => set("vo", "libmpv"),
        Mode::Wid => set("vo", "gpu"),
        Mode::Separate => {
            set("vo", "gpu");
            set("border", "no");
            set("force-window", "yes");
            set("geometry", "1280x720+0+0");
            set("title", "cinasafe-video");
        }
    }
    mpv.register_stream_protocol(Arc::new(Opener(vault))).expect("register cinasafe://");
    let sub = env_blob("CINASAFE_SPIKE_SUB");
    let spike = Arc::new(Spike {
        mpv: Arc::new(mpv),
        mode,
        stats: Arc::default(),
        started: Instant::now(),
        blob: env_blob("CINASAFE_SPIKE_BLOB"),
        sub,
    });
    eprintln!("[spike] mode {}", mode.name());

    let exit_after = std::env::var("CINASAFE_SPIKE_EXIT_AFTER").ok().and_then(|s| s.parse::<f64>().ok());

    tauri::Builder::default()
        .manage(spike.clone())
        .invoke_handler(tauri::generate_handler![mpv_command, spike_info])
        .setup(move |app| {
            let window = app.get_webview_window("main").expect("main window");
            let handle = app.handle().clone();
            let mpv = spike.mpv.clone();
            std::thread::Builder::new().name("mpv-events".into()).spawn(move || event_loop(handle, mpv))?;

            #[cfg(target_os = "linux")]
            linux::embed(&window, spike.clone())?;
            #[cfg(windows)]
            win::embed(&window, spike.clone())?;
            #[cfg(not(any(target_os = "linux", windows)))]
            {
                let _ = window;
                return Err("platform not covered by the spike".into());
            }

            if let Some(sub) = spike.sub {
                let mpv = spike.mpv.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(2));
                    let _ = mpv.command(&["sub-add", &format!("cinasafe://{}", sub.to_hex()), "select"]);
                });
            }
            if let Some(secs) = exit_after {
                let handle = app.handle().clone();
                let spike = spike.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs_f64(secs));
                    let s = spike.mpv.stream_stats();
                    eprintln!(
                        "[spike] exit stats: renders {} ({:.1}/s, avg {:.2} ms), mpv est fps {:.1}, drops {}, bridge {:.1} MiB @ {:.0} MiB/s, pos {:.2}",
                        spike.stats.renders.load(Ordering::Relaxed),
                        spike.stats.renders.load(Ordering::Relaxed) as f64 / spike.started.elapsed().as_secs_f64(),
                        spike.stats.render_us_total.load(Ordering::Relaxed) as f64
                            / spike.stats.renders.load(Ordering::Relaxed).max(1) as f64
                            / 1000.0,
                        spike.mpv.get_f64("estimated-vf-fps").unwrap_or(0.0),
                        spike.mpv.get_i64("frame-drop-count").unwrap_or(-1),
                        s.bytes as f64 / 1048576.0,
                        s.read_mib_per_s(),
                        spike.mpv.get_f64("time-pos").unwrap_or(-1.0),
                    );
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
