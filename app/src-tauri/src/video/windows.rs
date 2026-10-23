//! Windows: a native child window at the bottom of the z-order receives
//! mpv's video (`wid`); the transparent WebView2 above it draws the UI.
//! Not yet verified on real hardware (docs/SPIKE.md).

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;

use cinasafe_player::Mpv;
use tauri::AppHandle;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, SetWindowPos, ShowWindow, HWND_BOTTOM, SWP_NOACTIVATE, SW_HIDE, SW_SHOW, WS_CHILD,
    WS_CLIPSIBLINGS, WS_DISABLED,
};

static CHILD: AtomicIsize = AtomicIsize::new(0);

fn child() -> HWND {
    CHILD.load(Ordering::Relaxed) as HWND
}

pub fn setup(window: &tauri::WebviewWindow) -> Result<(), String> {
    let parent = window.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    // Hidden until a session opens; disabled so input goes to the webview.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_CLIPSIBLINGS | WS_DISABLED,
            0,
            0,
            size.width as i32,
            size.height as i32,
            parent,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err("CreateWindowExW failed".into());
    }
    unsafe { SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, size.width as i32, size.height as i32, SWP_NOACTIVATE) };
    CHILD.store(hwnd as isize, Ordering::Relaxed);
    window.on_window_event(|e| {
        if let tauri::WindowEvent::Resized(s) = e {
            unsafe { SetWindowPos(child(), HWND_BOTTOM, 0, 0, s.width as i32, s.height as i32, SWP_NOACTIVATE) };
        }
    });
    Ok(())
}

pub fn prepare(mpv: &Mpv) -> Result<(), cinasafe_player::Error> {
    if child().is_null() {
        return Err(cinasafe_player::Error::Library("no video surface".into()));
    }
    mpv.set_option("vo", "gpu")?;
    mpv.set_option("wid", &(child() as isize).to_string())
}

pub fn attach(_app: &AppHandle, _mpv: Arc<Mpv>) -> Result<bool, String> {
    unsafe { ShowWindow(child(), SW_SHOW) };
    // mpv chooses d3d11 itself, including WARP when there's no GPU.
    Ok(false)
}

pub fn detach(_app: &AppHandle) {
    unsafe { ShowWindow(child(), SW_HIDE) };
}
