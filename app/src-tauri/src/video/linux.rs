//! Linux: the webview sits in a `GtkOverlay` over a `GtkGLArea`. While a
//! player session is open, mpv's render API draws into the GL area
//! (docs/SPIKE.md: works on X11 and Wayland, one window, UI on top).

use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

use cinasafe_player::{ffi::GetProcAddressFn, GlRenderContext, Mpv, NativeDisplay};
use gtk::glib;
use gtk::prelude::*;
use tauri::AppHandle;

type ProcLoader = unsafe extern "C" fn(*const c_char) -> *mut c_void;

static GL_LOADER: OnceLock<(libloading::Library, ProcLoader)> = OnceLock::new();

thread_local! {
    // GTK objects live on the main thread only.
    static AREA: RefCell<Option<gtk::GLArea>> = const { RefCell::new(None) };
    static RENDER: RefCell<Option<GlRenderContext>> = const { RefCell::new(None) };
}

fn init_gl_loader(wayland: bool) -> Result<(), String> {
    if GL_LOADER.get().is_some() {
        return Ok(());
    }
    let (lib, sym): (&str, &[u8]) =
        if wayland { ("libEGL.so.1", b"eglGetProcAddress\0") } else { ("libGL.so.1", b"glXGetProcAddressARB\0") };
    // SAFETY: loading a system GL library and one well-known symbol.
    unsafe {
        let l = libloading::Library::new(lib).map_err(|e| format!("{lib}: {e}"))?;
        let f = *l.get::<ProcLoader>(sym).map_err(|e| e.to_string())?;
        let _ = GL_LOADER.set((l, f));
    }
    Ok(())
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    match GL_LOADER.get() {
        Some((_, f)) => unsafe { f(name) },
        None => std::ptr::null_mut(),
    }
}

fn gl_fn(name: &std::ffi::CStr) -> *mut c_void {
    unsafe { get_proc_address(std::ptr::null_mut(), name.as_ptr()) }
}

fn current_fbo() -> i32 {
    type GetIntegerv = unsafe extern "C" fn(u32, *mut i32);
    const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
    let p = gl_fn(c"glGetIntegerv");
    if p.is_null() {
        return 0;
    }
    let f: GetIntegerv = unsafe { std::mem::transmute(p) };
    let mut v = 0;
    unsafe { f(GL_FRAMEBUFFER_BINDING, &mut v) };
    v
}

fn clear_black() {
    type ClearColor = unsafe extern "C" fn(f32, f32, f32, f32);
    type Clear = unsafe extern "C" fn(u32);
    const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
    let (cc, c) = (gl_fn(c"glClearColor"), gl_fn(c"glClear"));
    if cc.is_null() || c.is_null() {
        return;
    }
    unsafe {
        std::mem::transmute::<*mut c_void, ClearColor>(cc)(0.0, 0.0, 0.0, 1.0);
        std::mem::transmute::<*mut c_void, Clear>(c)(GL_COLOR_BUFFER_BIT);
    }
}

/// GL_RENDERER of the current context, e.g. "llvmpipe (LLVM 20.1.2, 256 bits)".
fn gl_renderer() -> String {
    type GetString = unsafe extern "C" fn(u32) -> *const std::ffi::c_char;
    const GL_RENDERER: u32 = 0x1F01;
    let p = gl_fn(c"glGetString");
    if p.is_null() {
        return String::new();
    }
    let f: GetString = unsafe { std::mem::transmute(p) };
    let s = unsafe { f(GL_RENDERER) };
    if s.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(s) }.to_string_lossy().into_owned()
}

/// Software rasterisers (no GPU, or a broken driver).
pub fn is_software_renderer(renderer: &str) -> bool {
    let r = renderer.to_ascii_lowercase();
    ["llvmpipe", "softpipe", "swiftshader", "software rasterizer", "lavapipe"].iter().any(|s| r.contains(s))
}

fn is_wayland() -> bool {
    gtk::gdk::Display::default().is_some_and(|d| d.type_().name().contains("Wayland"))
}

/// Puts the webview over a GL area. Called once, when the window exists.
pub fn setup(window: &tauri::WebviewWindow) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    window
        .with_webview(move |pv| {
            let _ = tx.send(reparent(pv.inner()));
        })
        .map_err(|e| e.to_string())?;
    // with_webview runs on the main thread; setup() is called from there too,
    // so don't block on it: report failures asynchronously.
    std::thread::spawn(move || {
        if let Ok(Err(e)) = rx.recv() {
            eprintln!("cinasafe: video surface unavailable: {e}");
        }
    });
    Ok(())
}

fn reparent(webview: webkit2gtk::WebView) -> Result<(), String> {
    let parent = webview
        .parent()
        .and_then(|p| p.downcast::<gtk::Box>().ok())
        .ok_or("webview is not inside the default GtkBox")?;
    let wayland = is_wayland();
    init_gl_loader(wayland)?;
    let area = gtk::GLArea::new();
    area.set_auto_render(false);
    area.set_has_alpha(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.connect_render(|a, _| {
        let drew = RENDER.with(|r| {
            r.borrow().as_ref().map(|rc| {
                let scale = a.scale_factor();
                let (w, h) = (a.allocated_width() * scale, a.allocated_height() * scale);
                let _ = rc.render(current_fbo(), w, h, true);
            })
        });
        if drew.is_none() {
            clear_black();
        }
        glib::Propagation::Stop
    });
    area.connect_unrealize(|a| {
        a.make_current();
        RENDER.with(|r| r.borrow_mut().take());
    });

    let overlay = gtk::Overlay::new();
    parent.remove(&webview);
    overlay.add(&area);
    overlay.add_overlay(&webview);
    parent.pack_start(&overlay, true, true, 0);
    overlay.show_all();
    AREA.with(|a| *a.borrow_mut() = Some(area));
    Ok(())
}

fn native_display(wayland: bool) -> NativeDisplay {
    // VAAPI interop needs the native display, but crashes in vaInitialize on
    // machines without a GPU render node (docs/OPEN_QUESTIONS.md Q7).
    if !std::path::Path::new("/dev/dri/renderD128").exists() {
        return NativeDisplay::None;
    }
    let Some(display) = gtk::gdk::Display::default() else { return NativeDisplay::None };
    use gtk::glib::translate::ToGlibPtr;
    let raw: *mut gtk::gdk::ffi::GdkDisplay = display.to_glib_none().0;
    extern "C" {
        fn gdk_wayland_display_get_wl_display(d: *mut gtk::gdk::ffi::GdkDisplay) -> *mut c_void;
        fn gdk_x11_display_get_xdisplay(d: *mut gtk::gdk::ffi::GdkDisplay) -> *mut c_void;
    }
    unsafe {
        if wayland {
            NativeDisplay::Wayland(gdk_wayland_display_get_wl_display(raw))
        } else {
            NativeDisplay::X11(gdk_x11_display_get_xdisplay(raw))
        }
    }
}

/// mpv options before `initialize`.
pub fn prepare(mpv: &Mpv) -> Result<(), cinasafe_player::Error> {
    mpv.set_option("vo", "libmpv")
}

fn on_main<T: Send + 'static>(app: &AppHandle, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    if gtk::is_initialized_main_thread() {
        return Some(f());
    }
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv().ok()
}

/// Connects mpv's renderer to the GL area (after `initialize`). Returns
/// whether GL is a software rasteriser.
pub fn attach(app: &AppHandle, mpv: Arc<Mpv>) -> Result<bool, String> {
    on_main(app, move || {
        let area = AREA.with(|a| a.borrow().clone()).ok_or("no video surface")?;
        area.make_current();
        if let Some(e) = area.error() {
            return Err(format!("GL: {e}"));
        }
        // SAFETY: the area's GL context is current; `detach` drops the render
        // context (with the context current) before the session drops mpv.
        let mut rc = unsafe {
            GlRenderContext::new(
                &mpv,
                get_proc_address as GetProcAddressFn,
                std::ptr::null_mut(),
                native_display(is_wayland()),
            )
        }
        .map_err(|e| e.to_string())?;
        rc.set_update_callback(|| {
            glib::idle_add_once(|| {
                AREA.with(|a| {
                    if let Some(a) = a.borrow().as_ref() {
                        a.queue_render();
                    }
                })
            });
        });
        RENDER.with(|r| *r.borrow_mut() = Some(rc));
        Ok(is_software_renderer(&gl_renderer()))
    })
    .unwrap_or_else(|| Err("main thread unavailable".into()))
}

/// Frees the render context; the area goes back to black.
pub fn detach(app: &AppHandle) {
    on_main(app, || {
        AREA.with(|a| {
            if let Some(a) = a.borrow().as_ref() {
                a.make_current();
                RENDER.with(|r| r.borrow_mut().take());
                a.queue_render();
            } else {
                RENDER.with(|r| r.borrow_mut().take());
            }
        })
    });
}
