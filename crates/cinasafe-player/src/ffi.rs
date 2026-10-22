//! Minimal libmpv C API, loaded at runtime.
//!
//! Loading with `libloading` instead of linking means the app starts (and can
//! show a clear message) when libmpv is missing, and on Windows the DLL is
//! picked up from beside the executable on the stick.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::OnceLock;

use libloading::Library;

pub enum mpv_handle {}
pub enum mpv_render_context {}

pub const MPV_FORMAT_NONE: c_int = 0;
pub const MPV_FORMAT_STRING: c_int = 1;
pub const MPV_FORMAT_FLAG: c_int = 3;
pub const MPV_FORMAT_INT64: c_int = 4;
pub const MPV_FORMAT_DOUBLE: c_int = 5;

pub const MPV_ERROR_LOADING_FAILED: c_int = -13;
pub const MPV_ERROR_UNSUPPORTED: i64 = -18;
pub const MPV_ERROR_GENERIC: i64 = -20;

pub const MPV_RENDER_PARAM_INVALID: c_int = 0;
pub const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
pub const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
pub const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
pub const MPV_RENDER_PARAM_X11_DISPLAY: c_int = 8;
pub const MPV_RENDER_PARAM_WL_DISPLAY: c_int = 9;
pub const MPV_RENDER_UPDATE_FRAME: u64 = 1;

#[repr(C)]
pub struct mpv_event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_end_file {
    pub reason: c_int,
    pub error: c_int,
}

pub type StreamReadFn = unsafe extern "C" fn(*mut c_void, *mut c_char, u64) -> i64;
pub type StreamSeekFn = unsafe extern "C" fn(*mut c_void, i64) -> i64;
pub type StreamSizeFn = unsafe extern "C" fn(*mut c_void) -> i64;
pub type StreamCloseFn = unsafe extern "C" fn(*mut c_void);
pub type StreamCancelFn = unsafe extern "C" fn(*mut c_void);
pub type StreamOpenFn = unsafe extern "C" fn(*mut c_void, *mut c_char, *mut mpv_stream_cb_info) -> c_int;

#[repr(C)]
pub struct mpv_stream_cb_info {
    pub cookie: *mut c_void,
    pub read_fn: Option<StreamReadFn>,
    pub seek_fn: Option<StreamSeekFn>,
    pub size_fn: Option<StreamSizeFn>,
    pub close_fn: Option<StreamCloseFn>,
    pub cancel_fn: Option<StreamCancelFn>,
}

#[repr(C)]
pub struct mpv_render_param {
    pub type_: c_int,
    pub data: *mut c_void,
}

pub type GetProcAddressFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;

#[repr(C)]
pub struct mpv_opengl_init_params {
    pub get_proc_address: Option<GetProcAddressFn>,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct mpv_opengl_fbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

macro_rules! mpv_api {
    ($($name:ident : fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        pub struct Api {
            _lib: Library,
            $(pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Api {
            unsafe fn load(lib: Library) -> Result<Api, String> {
                $(
                    let $name = *lib
                        .get::<unsafe extern "C" fn($($arg),*) $(-> $ret)?>(
                            concat!(stringify!($name), "\0").as_bytes(),
                        )
                        .map_err(|e| format!("{}: {e}", stringify!($name)))?;
                )*
                Ok(Api { _lib: lib, $($name,)* })
            }
        }
    };
}

mpv_api! {
    mpv_client_api_version: fn() -> u64;
    mpv_error_string: fn(c_int) -> *const c_char;
    mpv_free: fn(*mut c_void);
    mpv_create: fn() -> *mut mpv_handle;
    mpv_initialize: fn(*mut mpv_handle) -> c_int;
    mpv_terminate_destroy: fn(*mut mpv_handle);
    mpv_set_option_string: fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int;
    mpv_set_property_string: fn(*mut mpv_handle, *const c_char, *const c_char) -> c_int;
    mpv_command: fn(*mut mpv_handle, *mut *const c_char) -> c_int;
    mpv_get_property: fn(*mut mpv_handle, *const c_char, c_int, *mut c_void) -> c_int;
    mpv_observe_property: fn(*mut mpv_handle, u64, *const c_char, c_int) -> c_int;
    mpv_wait_event: fn(*mut mpv_handle, f64) -> *mut mpv_event;
    mpv_wakeup: fn(*mut mpv_handle);
    mpv_stream_cb_add_ro: fn(*mut mpv_handle, *const c_char, *mut c_void, StreamOpenFn) -> c_int;
    mpv_render_context_create: fn(*mut *mut mpv_render_context, *mut mpv_handle, *mut mpv_render_param) -> c_int;
    mpv_render_context_set_update_callback: fn(*mut mpv_render_context, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void);
    mpv_render_context_update: fn(*mut mpv_render_context) -> u64;
    mpv_render_context_render: fn(*mut mpv_render_context, *mut mpv_render_param) -> c_int;
    mpv_render_context_report_swap: fn(*mut mpv_render_context);
    mpv_render_context_free: fn(*mut mpv_render_context);
}

#[cfg(target_os = "windows")]
const LIB_NAMES: &[&str] = &["libmpv-2.dll", "mpv-2.dll"];
#[cfg(target_os = "macos")]
const LIB_NAMES: &[&str] = &["libmpv.2.dylib", "libmpv.dylib"];
#[cfg(all(unix, not(target_os = "macos")))]
const LIB_NAMES: &[&str] = &["libmpv.so.2", "libmpv.so"];

/// Client API major version this wrapper is written against (mpv 0.35+).
const API_MAJOR: u64 = 2;

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(dir) = std::env::var("CINASAFE_LIBMPV_DIR") {
        out.extend(LIB_NAMES.iter().map(|n| PathBuf::from(&dir).join(n)));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)) {
        out.extend(LIB_NAMES.iter().map(|n| dir.join(n)));
    }
    // Bare names go through the platform search path (LD_LIBRARY_PATH in the AppImage).
    out.extend(LIB_NAMES.iter().map(PathBuf::from));
    out
}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

pub fn api() -> Result<&'static Api, String> {
    API.get_or_init(|| {
        let mut errors = Vec::new();
        for path in candidates() {
            // SAFETY: loading libmpv runs its initialisers, which have no
            // preconditions beyond being a genuine libmpv build.
            match unsafe { Library::new(&path) } {
                Ok(lib) => {
                    let api = unsafe { Api::load(lib) }?;
                    let v = unsafe { (api.mpv_client_api_version)() };
                    if v >> 16 != API_MAJOR {
                        return Err(format!(
                            "libmpv client API {}.{} is not supported (need {API_MAJOR}.x)",
                            v >> 16,
                            v & 0xffff
                        ));
                    }
                    return Ok(api);
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        Err(format!("libmpv not found ({})", errors.join("; ")))
    })
    .as_ref()
    .map_err(Clone::clone)
}
