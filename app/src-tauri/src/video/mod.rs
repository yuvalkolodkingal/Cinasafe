//! Where mpv draws: under the transparent webview, inside the app window
//! (brief 6.3). One implementation per platform with the same four calls.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{attach, detach, prepare, setup};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{attach, detach, prepare, setup};

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
}
#[cfg(not(any(target_os = "linux", windows)))]
pub use other::{attach, detach, prepare, setup};
