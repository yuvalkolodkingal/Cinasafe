//! Cinasafe desktop app: the Tauri shell around the core (brief section 3).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod commands;
mod dto;
mod error;
mod hardening;
mod img;
mod player;
mod settings;
mod state;
mod video;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

use crate::state::{AppState, LockReason};

const SESSION_PREFIX: &str = "cinasafe-session-";

/// A fresh temp dir for everything the web engine, GTK and the GPU driver
/// would otherwise write under the user's profile (brief 10). Deleted on
/// exit, and leftovers from a crashed run are deleted on the next start.
fn session_dir() -> std::io::Result<PathBuf> {
    let tmp = std::env::temp_dir();
    if let Ok(rd) = std::fs::read_dir(&tmp) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(SESSION_PREFIX) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let dir = tempfile::Builder::new().prefix(SESSION_PREFIX).tempdir_in(&tmp)?.keep();
    for sub in ["webview", "cache", "data", "config"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    Ok(dir)
}

fn redirect_caches(dir: &Path) {
    // Set before any other thread exists.
    std::env::set_var("MESA_SHADER_CACHE_DISABLE", "true");
    std::env::set_var("__GL_SHADER_DISK_CACHE", "0");
    if cfg!(target_os = "linux") {
        // WebKitGTK's salts/HSTS/cache, fontconfig's cache and GTK's
        // recently-used list all follow these.
        std::env::set_var("XDG_CACHE_HOME", dir.join("cache"));
        std::env::set_var("XDG_DATA_HOME", dir.join("data"));
    }
}

/// Windows without the WebView2 runtime (some Windows 10 editions) can still
/// run the app from a stick that carries Microsoft's fixed-version runtime in
/// `.cinasafe/runtime/webview2-<arch>/` (or `webview2-<arch>/`) next to the
/// exe. An installed runtime is preferred: it gets security updates.
#[cfg(windows)]
fn use_carried_webview2() {
    if std::env::var_os("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER").is_some() {
        return;
    }
    let installed = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"].iter().any(|var| {
        std::env::var_os(var)
            .map(|base| PathBuf::from(base).join("Microsoft").join("EdgeWebView").join("Application"))
            .and_then(|dir| std::fs::read_dir(dir).ok())
            .is_some_and(|mut rd| rd.any(|e| e.is_ok_and(|e| e.path().join("msedgewebview2.exe").is_file())))
    });
    if installed {
        return;
    }
    let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        return;
    };
    let name = if cfg!(target_arch = "aarch64") { "webview2-arm64" } else { "webview2-x64" };
    for dir in [exe_dir.join(".cinasafe").join("runtime").join(name), exe_dir.join(name)] {
        if dir.join("msedgewebview2.exe").is_file() {
            std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &dir);
            return;
        }
    }
}

#[cfg(not(windows))]
fn use_carried_webview2() {}

/// Logout, shutdown and Ctrl-C end the app the normal way, so the vault is
/// locked and the session dir deleted (RunEvent::Exit).
#[cfg(unix)]
fn exit_on_signals(app: tauri::AppHandle) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    match signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        Ok(mut signals) => {
            std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    app.exit(0);
                }
            });
        }
        Err(e) => eprintln!("cinasafe: no signal handling: {e}"),
    }
}

#[cfg(not(unix))]
fn exit_on_signals(_: tauri::AppHandle) {}

fn main() {
    hardening::no_crash_dumps();
    let session = match session_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cinasafe: cannot create a session directory: {e}");
            std::process::exit(1);
        }
    };
    redirect_caches(&session);
    use_carried_webview2();
    let webview_dir = session.join("webview");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("cinasafe-img", img::handle)
        .invoke_handler(tauri::generate_handler![
            commands::vault_detect,
            commands::vault_status,
            commands::drives_list,
            commands::vault_create,
            commands::vault_unlock,
            commands::vault_unlock_recovery,
            commands::vault_lock,
            commands::vault_info,
            commands::vault_change_password,
            commands::vault_replace_recovery,
            commands::vault_remove_recovery,
            commands::vault_integrity_check,
            commands::eject_drive,
            commands::library_home,
            commands::library_movies,
            commands::library_shows,
            commands::library_movie,
            commands::library_show,
            commands::library_season,
            commands::library_episode,
            commands::library_update_metadata,
            commands::library_set_overrides,
            commands::library_mark_watched,
            commands::library_mark_season_watched,
            commands::library_remove,
            commands::library_export,
            commands::search,
            commands::import_add,
            commands::import_browse,
            commands::import_status,
            commands::import_pause,
            commands::import_resume,
            commands::import_cancel,
            commands::import_clear_finished,
            commands::import_resolve,
            commands::tmdb_search,
            commands::storage,
            commands::player_open,
            commands::player_command,
            commands::player_close,
            commands::player_state,
            commands::settings_get,
            commands::settings_set,
            commands::app_info,
            commands::update_player_apps,
            commands::activity,
        ])
        .setup(move |app| {
            let state = AppState::new(app.handle().clone());
            app.manage(state.clone());
            state.start_watchers();
            exit_on_signals(app.handle().clone());
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Cinasafe")
                .inner_size(1280.0, 800.0)
                .min_inner_size(720.0, 520.0)
                .transparent(true)
                .incognito(true)
                .data_directory(webview_dir.clone())
                // WebView2 only. Keeps Tauri's defaults and adds: no autofill
                // or translation services, no background network traffic.
                .additional_browser_args(
                    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,AutofillServerCommunication,msEdgeTranslate \
                     --disable-background-networking --disable-sync --no-first-run",
                )
                .build()?;
            if let Err(e) = video::setup(&window) {
                // The library still works; playback will report the problem.
                eprintln!("cinasafe: video surface unavailable: {e}");
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cinasafe: {e}");
            let _ = std::fs::remove_dir_all(&session);
            std::process::exit(1);
        }
    };
    app.run(move |handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<Arc<AppState>>() {
                if state.is_unlocked() {
                    state.lock(LockReason::User);
                }
            }
            let _ = std::fs::remove_dir_all(&session);
        }
    });
}
