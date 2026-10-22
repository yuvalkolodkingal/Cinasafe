//! Linux: GTK3 + WebKitGTK. Both embedding modes replace the window's box
//! content with a `GtkOverlay` whose base child shows video and whose overlay
//! child is the (transparent) webview, so the web UI receives all input.

use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use cinasafe_player::{ffi::GetProcAddressFn, GlRenderContext, NativeDisplay};
use gtk::glib;
use gtk::prelude::*;

use crate::{Mode, Spike};

type ProcLoader = unsafe extern "C" fn(*const c_char) -> *mut c_void;

/// `glXGetProcAddressARB` or `eglGetProcAddress`, matching the GDK backend.
static GL_LOADER: OnceLock<(libloading::Library, ProcLoader)> = OnceLock::new();

fn init_gl_loader(wayland: bool) -> Result<(), String> {
    if GL_LOADER.get().is_some() {
        return Ok(());
    }
    let (lib, sym): (&str, &[u8]) =
        if wayland { ("libEGL.so.1", b"eglGetProcAddress\0") } else { ("libGL.so.1", b"glXGetProcAddressARB\0") };
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

fn gl_get_integer(pname: u32) -> i32 {
    type GetIntegerv = unsafe extern "C" fn(u32, *mut i32);
    let p = unsafe { get_proc_address(std::ptr::null_mut(), c"glGetIntegerv".as_ptr()) };
    if p.is_null() {
        return 0;
    }
    let f: GetIntegerv = unsafe { std::mem::transmute(p) };
    let mut v = 0;
    unsafe { f(pname, &mut v) };
    v
}

const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;

thread_local! {
    /// The GL area, reachable from the `Send` idle callback mpv's update hook posts.
    static GL_AREA: RefCell<Option<gtk::GLArea>> = const { RefCell::new(None) };
}

pub fn embed(window: &tauri::WebviewWindow, spike: Arc<Spike>) -> Result<(), Box<dyn std::error::Error>> {
    if spike.mode == Mode::Separate {
        crate::attach_separate(window, spike);
        return Ok(());
    }
    window.with_webview(move |pv| {
        let webview = pv.inner();
        if let Err(e) = reparent(webview, spike) {
            eprintln!("[spike] embedding failed: {e}");
        }
    })?;
    Ok(())
}

fn reparent(webview: webkit2gtk::WebView, spike: Arc<Spike>) -> Result<(), String> {
    let parent = webview
        .parent()
        .and_then(|p| p.downcast::<gtk::Box>().ok())
        .ok_or("webview is not inside the default GtkBox")?;
    let display = gtk::gdk::Display::default().ok_or("no GDK display")?;
    let backend = display.type_().name().to_string();
    eprintln!("[spike] GDK backend {backend}");

    let overlay = gtk::Overlay::new();
    parent.remove(&webview);
    match spike.mode {
        Mode::GlOverlay => overlay.add(&gl_area(spike.clone(), backend.contains("Wayland"))?),
        Mode::Wid => overlay.add(&wid_area(spike.clone(), &backend)?),
        Mode::Separate => unreachable!(),
    }
    overlay.add_overlay(&webview);
    parent.pack_start(&overlay, true, true, 0);
    overlay.show_all();
    Ok(())
}

fn gl_area(spike: Arc<Spike>, wayland: bool) -> Result<gtk::GLArea, String> {
    init_gl_loader(wayland)?;
    let area = gtk::GLArea::new();
    area.set_auto_render(false);
    area.set_has_alpha(false);
    area.set_hexpand(true);
    area.set_vexpand(true);

    let ctx: Rc<RefCell<Option<GlRenderContext>>> = Rc::default();

    {
        let ctx = ctx.clone();
        let spike = spike.clone();
        area.connect_realize(move |a| {
            a.make_current();
            if let Some(e) = a.error() {
                eprintln!("[spike] GLArea realize error: {e}");
                return;
            }
            let native = native_display(wayland);
            // SAFETY: GL context is current; the context is freed in unrealize
            // (with the context current) before mpv itself is destroyed.
            let r = unsafe {
                GlRenderContext::new(&spike.mpv, get_proc_address as GetProcAddressFn, std::ptr::null_mut(), native)
            };
            match r {
                Ok(mut rc) => {
                    rc.set_update_callback(|| {
                        glib::idle_add_once(|| {
                            GL_AREA.with(|a| {
                                if let Some(a) = a.borrow().as_ref() {
                                    a.queue_render();
                                }
                            })
                        });
                    });
                    *ctx.borrow_mut() = Some(rc);
                    GL_AREA.with(|g| *g.borrow_mut() = Some(a.clone()));
                    let gl = a.context().map(|c| {
                        let (maj, min) = c.version();
                        format!("{}GL {maj}.{min}", if c.uses_es() { "ES " } else { "" })
                    });
                    eprintln!("[spike] mpv render context created ({})", gl.unwrap_or_default());
                    spike.start();
                }
                Err(e) => eprintln!("[spike] mpv render context failed: {e}"),
            }
        });
    }
    {
        let ctx = ctx.clone();
        let stats = spike.stats.clone();
        area.connect_render(move |a, _| {
            if let Some(rc) = ctx.borrow().as_ref() {
                let t = Instant::now();
                let scale = a.scale_factor();
                let fbo = gl_get_integer(GL_FRAMEBUFFER_BINDING);
                let (w, h) = (a.allocated_width() * scale, a.allocated_height() * scale);
                if let Err(e) = rc.render(fbo, w, h, true) {
                    eprintln!("[spike] render: {e}");
                }
                stats.renders.fetch_add(1, Ordering::Relaxed);
                stats.render_us_total.fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
            }
            glib::Propagation::Stop
        });
    }
    area.connect_unrealize(move |a| {
        a.make_current();
        GL_AREA.with(|g| g.borrow_mut().take());
        ctx.borrow_mut().take();
    });
    Ok(area)
}

fn native_display(wayland: bool) -> NativeDisplay {
    // Handing mpv the native display enables VAAPI/EGL hwdec interop.
    if std::env::var_os("CINASAFE_SPIKE_NO_NATIVE_DISPLAY").is_some() {
        return NativeDisplay::None;
    }
    let Some(display) = gtk::gdk::Display::default() else { return NativeDisplay::None };
    use gtk::glib::translate::ToGlibPtr;
    let raw: *mut gtk::gdk::ffi::GdkDisplay = display.to_glib_none().0;
    unsafe {
        if wayland {
            extern "C" {
                fn gdk_wayland_display_get_wl_display(d: *mut gtk::gdk::ffi::GdkDisplay) -> *mut c_void;
            }
            NativeDisplay::Wayland(gdk_wayland_display_get_wl_display(raw))
        } else {
            extern "C" {
                fn gdk_x11_display_get_xdisplay(d: *mut gtk::gdk::ffi::GdkDisplay) -> *mut c_void;
            }
            NativeDisplay::X11(gdk_x11_display_get_xdisplay(raw))
        }
    }
}

/// brief 6.3 option 1 on X11: a native child X window handed to mpv's `wid`.
fn wid_area(spike: Arc<Spike>, backend: &str) -> Result<gtk::DrawingArea, String> {
    if !backend.contains("X11") {
        return Err(format!("wid mode needs X11, GDK backend is {backend}"));
    }
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.connect_realize(move |a| {
        let Some(win) = a.window() else { return };
        if !win.ensure_native() {
            eprintln!("[spike] could not make a native X window");
            return;
        }
        let xid = match win.downcast::<gdkx11::X11Window>() {
            Ok(x) => x.xid(),
            Err(_) => {
                eprintln!("[spike] not an X11 window");
                return;
            }
        };
        eprintln!("[spike] native child window xid 0x{xid:x}");
        if let Err(e) = spike.mpv.set_option("wid", &xid.to_string()) {
            eprintln!("[spike] {e}");
        }
        spike.start();
    });
    Ok(area)
}
