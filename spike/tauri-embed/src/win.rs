//! Windows: brief 6.3 option 1. A native child HWND at the bottom of the
//! z-order receives mpv's video (`--wid`); the WebView2 child above it has a
//! transparent background, so the web UI is drawn over the video.
//!
//! UNTESTED in Milestone 0 (no Windows machine in the build environment); see
//! docs/SPIKE.md for what to check.

use std::sync::Arc;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, WS_CHILD, WS_CLIPSIBLINGS, WS_DISABLED, WS_VISIBLE,
};

use crate::{Mode, Spike};

pub fn embed(window: &tauri::WebviewWindow, spike: Arc<Spike>) -> Result<(), Box<dyn std::error::Error>> {
    match spike.mode {
        Mode::Separate => {
            crate::attach_separate(window, spike);
            Ok(())
        }
        Mode::GlOverlay => Err("gl-overlay is Linux-only in the spike; use wid or separate".into()),
        Mode::Wid => {
            let parent = window.hwnd()?.0 as HWND;
            let size = window.inner_size()?;
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // WS_DISABLED: input goes to the webview/parent, never to the video surface.
            let child = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_DISABLED,
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
            if child.is_null() {
                return Err("CreateWindowExW failed".into());
            }
            unsafe { SetWindowPos(child, HWND_BOTTOM, 0, 0, size.width as i32, size.height as i32, SWP_NOACTIVATE) };
            spike.mpv.set_option("wid", &(child as isize).to_string())?;
            spike.start();

            let child_addr = child as isize;
            window.on_window_event(move |e| {
                if let tauri::WindowEvent::Resized(s) = e {
                    unsafe {
                        SetWindowPos(
                            child_addr as HWND,
                            HWND_BOTTOM,
                            0,
                            0,
                            s.width as i32,
                            s.height as i32,
                            SWP_NOACTIVATE,
                        )
                    };
                }
            });
            Ok(())
        }
    }
}
