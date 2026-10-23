//! Windows: mpv draws (`wid`) into a borderless popup window that sits
//! directly behind the app window, over exactly its client area. The app
//! window is transparent where the player shows video, and DWM blends
//! top-level windows, so the video shows through the WebView2. (A child
//! window under the WebView2 doesn't work: the transparent web view then
//! shows the desktop instead of the child's video.)
//!
//! The popup is disabled and never activated, has no taskbar button, and
//! follows the app window's moves, resizes, focus and minimising: after each,
//! it is put back immediately below the app window in the z-order.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;

use cinasafe_player::Mpv;
use tauri::AppHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, GetStockObject, BLACK_BRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, IsIconic, RegisterClassW, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
    SW_HIDE, SW_SHOWNOACTIVATE, WNDCLASSW, WS_DISABLED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

static VIDEO: AtomicIsize = AtomicIsize::new(0);
static APP: AtomicIsize = AtomicIsize::new(0);
/// A player session is open: the popup is shown (unless the app is minimised).
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn video() -> HWND {
    VIDEO.load(Ordering::Relaxed) as HWND
}

fn app_window() -> HWND {
    APP.load(Ordering::Relaxed) as HWND
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// Creates the (hidden) video popup for the app window.
pub fn setup(window: &tauri::WebviewWindow) -> Result<(), String> {
    let app = window.hwnd().map_err(|e| e.to_string())?.0 as HWND;
    let class: Vec<u16> = "CinasafeVideo\0".encode_utf16().collect();
    let hwnd = unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            // Black until mpv draws its first frame.
            hbrBackground: GetStockObject(BLACK_BRUSH),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.as_ptr(),
            std::ptr::null(),
            WS_POPUP | WS_DISABLED,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err("CreateWindowExW failed".into());
    }
    VIDEO.store(hwnd as isize, Ordering::Relaxed);
    APP.store(app as isize, Ordering::Relaxed);
    window.on_window_event(|e| {
        use tauri::WindowEvent::{Focused, Moved, Resized, ScaleFactorChanged};
        if matches!(e, Moved(_) | Resized(_) | ScaleFactorChanged { .. } | Focused(_)) {
            follow();
        }
    });
    Ok(())
}

/// Puts the popup over the app window's client area (physical pixels),
/// immediately below the app window, and hides it while the app is minimised
/// or no session is open.
fn follow() {
    let (app, video) = (app_window(), video());
    if app.is_null() || video.is_null() {
        return;
    }
    unsafe {
        if !ACTIVE.load(Ordering::Relaxed) || IsIconic(app) != 0 {
            ShowWindow(video, SW_HIDE);
            return;
        }
        let mut r: RECT = std::mem::zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        GetClientRect(app, &mut r);
        ClientToScreen(app, &mut origin);
        // hWndInsertAfter = the app window: the popup goes right behind it.
        SetWindowPos(video, app, origin.x, origin.y, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE);
        ShowWindow(video, SW_SHOWNOACTIVATE);
    }
}

pub fn prepare(mpv: &Mpv) -> Result<(), cinasafe_player::Error> {
    if video().is_null() {
        return Err(cinasafe_player::Error::Library("no video surface".into()));
    }
    mpv.set_option("vo", "gpu")?;
    mpv.set_option("wid", &(video() as isize).to_string())
}

pub fn attach(_app: &AppHandle, _mpv: Arc<Mpv>) -> Result<bool, String> {
    ACTIVE.store(true, Ordering::Relaxed);
    follow();
    // mpv chooses d3d11 itself, including WARP when there's no GPU.
    Ok(false)
}

pub fn detach(_app: &AppHandle) {
    ACTIVE.store(false, Ordering::Relaxed);
    follow();
}

/// What the video surface runs on, for `--check-video`.
pub fn describe(_: &AppHandle) -> String {
    format!(
        "video: mpv draws into a popup window behind the app window (Direct3D 11); popup {}",
        if video().is_null() { "missing" } else { "created" }
    )
}
