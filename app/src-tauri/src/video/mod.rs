//! Where mpv draws: under the transparent webview, inside the app window
//! (brief 6.3). One implementation per platform with the same four calls.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{attach, describe, detach, prepare, setup};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{attach, describe, detach, prepare, setup};

#[cfg(not(any(target_os = "linux", windows)))]
mod other {
    use std::sync::Arc;

    use cinasafe_player::Mpv;
    use tauri::AppHandle;

    pub fn setup(_: &tauri::WebviewWindow) -> Result<(), String> {
        Ok(())
    }
    pub fn prepare(_: &Mpv) -> Result<(), cinasafe_player::Error> {
        Err(cinasafe_player::Error::Library("video output is not supported on this platform yet".into()))
    }
    pub fn attach(_: &AppHandle, _: Arc<Mpv>) -> Result<bool, String> {
        Ok(false)
    }
    pub fn detach(_: &AppHandle) {}
    pub fn describe(_: &AppHandle) -> String {
        "no video output on this platform".into()
    }
}
#[cfg(not(any(target_os = "linux", windows)))]
pub use other::{attach, describe, detach, prepare, setup};

/// `cinasafe-app --check-video`: starts the player engine and connects it to
/// the window exactly as playback does, without a vault, and reports what
/// happened. Returns whether it worked.
pub fn check(app: &tauri::AppHandle) -> bool {
    use std::sync::Arc;
    let result = (|| -> Result<bool, String> {
        let mpv = cinasafe_player::Mpv::new().map_err(|e| format!("player engine: {e}"))?;
        prepare(&mpv).map_err(|e| format!("player engine: {e}"))?;
        mpv.initialize().map_err(|e| format!("player engine: {e}"))?;
        let mpv = Arc::new(mpv);
        let software = attach(app, mpv.clone()).map_err(|e| format!("video output: {e}"))?;
        detach(app);
        Ok(software)
    })();
    println!("{}", describe(app));
    match result {
        Ok(software) => {
            println!("video check: OK{}", if software { " (software rendering)" } else { "" });
            true
        }
        Err(e) => {
            println!("video check: FAILED: {e}");
            false
        }
    }
}
