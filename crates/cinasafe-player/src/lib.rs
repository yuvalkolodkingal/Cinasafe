//! libmpv wrapper and the `cinasafe://` stream bridge (brief section 6).
//!
//! mpv never sees a file path. It opens `cinasafe://<blob-id-hex>`, and the
//! bridge serves decrypted bytes from a [`StreamOpener`] (normally the vault's
//! `BlobReader`) straight from memory.

pub mod ffi;
mod render;
mod stream;
mod vault;

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;
use std::time::Duration;

pub use render::{GlRenderContext, NativeDisplay, RenderUpdate};
pub use stream::{MediaStream, StreamOpener, StreamStats};
pub use vault::{MpvProber, VaultOpener};

use ffi::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Library(String),
    #[error("mpv {what} failed: {msg} ({code})")]
    Mpv { what: String, code: c_int, msg: String },
    #[error("string contains a NUL byte")]
    Nul,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Options from brief section 6.2. Every one of them is mandatory: if libmpv
/// rejects one, playback must not start.
pub const SAFE_OPTIONS: &[(&str, &str)] = &[
    ("config", "no"),
    ("load-scripts", "no"),
    ("ytdl", "no"),
    ("cache", "yes"),
    ("cache-on-disk", "no"),
    ("demuxer-max-bytes", "256MiB"),
    ("demuxer-max-back-bytes", "64MiB"),
    ("save-position-on-quit", "no"),
    ("write-filename-in-watch-later-config", "no"),
    ("hwdec", "auto-safe"),
    ("osc", "no"),
    ("input-default-bindings", "no"),
    ("input-vo-keyboard", "no"),
    ("sub-ass-shaper", "complex"),
    ("sub-codepage", "utf-8"),
    // Not in the brief's list, but these would otherwise write to the host disk.
    ("gpu-shader-cache", "no"),
    ("icc-cache", "no"),
    ("log-file", ""),
];

/// Options from [`SAFE_OPTIONS`] that only turn a feature off. A libmpv built
/// without that feature (e.g. without Lua, so no youtube-dl hook, scripts or
/// on-screen controller) doesn't know the option, which is just as safe.
const ABSENT_IS_SAFE: &[&str] = &["ytdl", "load-scripts", "osc"];

/// mpv properties of interest, in a form that owns its data.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    None,
    Flag(bool),
    Int(i64),
    Double(f64),
    Str(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    None,
    Shutdown,
    StartFile,
    FileLoaded,
    EndFile { reason: i32, error: i32 },
    Seek,
    PlaybackRestart,
    VideoReconfig,
    AudioReconfig,
    PropertyChange { id: u64, name: String, value: PropertyValue },
    Other(i32),
}

pub struct Mpv {
    api: &'static Api,
    handle: *mut mpv_handle,
    streams: Option<Box<stream::Bridge>>,
}

// SAFETY: the mpv client API is fully thread-safe for a given handle.
unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::Nul)
}

impl Mpv {
    /// Creates an uninitialised handle with [`SAFE_OPTIONS`] applied.
    pub fn new() -> Result<Mpv> {
        let api = ffi::api().map_err(Error::Library)?;
        let handle = unsafe { (api.mpv_create)() };
        if handle.is_null() {
            return Err(Error::Library("mpv_create returned NULL".into()));
        }
        let mpv = Mpv { api, handle, streams: None };
        for (k, v) in SAFE_OPTIONS {
            match mpv.set_option(k, v) {
                // The feature this option would switch off isn't in this build.
                Err(Error::Mpv { code: ffi::MPV_ERROR_OPTION_NOT_FOUND, .. }) if ABSENT_IS_SAFE.contains(k) => {}
                other => other?,
            }
        }
        Ok(mpv)
    }

    fn check(&self, what: impl FnOnce() -> String, code: c_int) -> Result<()> {
        if code >= 0 {
            return Ok(());
        }
        let msg = unsafe { CStr::from_ptr((self.api.mpv_error_string)(code)) }.to_string_lossy().into_owned();
        Err(Error::Mpv { what: what(), code, msg })
    }

    pub fn api(&self) -> &'static Api {
        self.api
    }

    pub fn raw(&self) -> *mut mpv_handle {
        self.handle
    }

    pub fn set_option(&self, name: &str, value: &str) -> Result<()> {
        let (n, v) = (cstr(name)?, cstr(value)?);
        let rc = unsafe { (self.api.mpv_set_option_string)(self.handle, n.as_ptr(), v.as_ptr()) };
        self.check(|| format!("option {name}={value}"), rc)
    }

    pub fn initialize(&self) -> Result<()> {
        let rc = unsafe { (self.api.mpv_initialize)(self.handle) };
        self.check(|| "initialize".into(), rc)
    }

    pub fn command(&self, args: &[&str]) -> Result<()> {
        let owned = args.iter().map(|a| cstr(a)).collect::<Result<Vec<_>>>()?;
        let mut argv: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        argv.push(ptr::null());
        let rc = unsafe { (self.api.mpv_command)(self.handle, argv.as_mut_ptr()) };
        self.check(|| format!("command {}", args.first().unwrap_or(&"")), rc)
    }

    pub fn set_property(&self, name: &str, value: &str) -> Result<()> {
        let (n, v) = (cstr(name)?, cstr(value)?);
        let rc = unsafe { (self.api.mpv_set_property_string)(self.handle, n.as_ptr(), v.as_ptr()) };
        self.check(|| format!("set {name}"), rc)
    }

    pub fn get_f64(&self, name: &str) -> Result<f64> {
        let n = cstr(name)?;
        let mut out = 0f64;
        let rc = unsafe {
            (self.api.mpv_get_property)(self.handle, n.as_ptr(), MPV_FORMAT_DOUBLE, &mut out as *mut f64 as *mut c_void)
        };
        self.check(|| format!("get {name}"), rc).map(|_| out)
    }

    pub fn get_i64(&self, name: &str) -> Result<i64> {
        let n = cstr(name)?;
        let mut out = 0i64;
        let rc = unsafe {
            (self.api.mpv_get_property)(self.handle, n.as_ptr(), MPV_FORMAT_INT64, &mut out as *mut i64 as *mut c_void)
        };
        self.check(|| format!("get {name}"), rc).map(|_| out)
    }

    pub fn get_string(&self, name: &str) -> Result<String> {
        let n = cstr(name)?;
        let mut out: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            (self.api.mpv_get_property)(self.handle, n.as_ptr(), MPV_FORMAT_STRING, &mut out as *mut _ as *mut c_void)
        };
        self.check(|| format!("get {name}"), rc)?;
        let s = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { (self.api.mpv_free)(out as *mut c_void) };
        Ok(s)
    }

    /// `format` is one of the `MPV_FORMAT_*` constants.
    pub fn observe(&self, id: u64, name: &str, format: c_int) -> Result<()> {
        let n = cstr(name)?;
        let rc = unsafe { (self.api.mpv_observe_property)(self.handle, id, n.as_ptr(), format) };
        self.check(|| format!("observe {name}"), rc)
    }

    pub fn wakeup(&self) {
        unsafe { (self.api.mpv_wakeup)(self.handle) }
    }

    /// Waits for the next event. Only one thread may call this at a time.
    pub fn wait_event(&self, timeout: Duration) -> Event {
        let ev = unsafe { &*(self.api.mpv_wait_event)(self.handle, timeout.as_secs_f64()) };
        match ev.event_id {
            0 => Event::None,
            1 => Event::Shutdown,
            6 => Event::StartFile,
            7 => {
                let ef = unsafe { &*(ev.data as *const mpv_event_end_file) };
                Event::EndFile { reason: ef.reason, error: ef.error }
            }
            8 => Event::FileLoaded,
            17 => Event::VideoReconfig,
            18 => Event::AudioReconfig,
            20 => Event::Seek,
            21 => Event::PlaybackRestart,
            22 => {
                let p = unsafe { &*(ev.data as *const mpv_event_property) };
                let name = unsafe { CStr::from_ptr(p.name) }.to_string_lossy().into_owned();
                let value = unsafe {
                    match p.format {
                        MPV_FORMAT_FLAG => PropertyValue::Flag(*(p.data as *const c_int) != 0),
                        MPV_FORMAT_INT64 => PropertyValue::Int(*(p.data as *const i64)),
                        MPV_FORMAT_DOUBLE => PropertyValue::Double(*(p.data as *const f64)),
                        MPV_FORMAT_STRING => PropertyValue::Str(
                            CStr::from_ptr(*(p.data as *const *const c_char)).to_string_lossy().into_owned(),
                        ),
                        _ => PropertyValue::None,
                    }
                };
                Event::PropertyChange { id: ev.reply_userdata, name, value }
            }
            other => Event::Other(other),
        }
    }

    /// Registers the `cinasafe://` protocol. Call once, before `loadfile`.
    pub fn register_stream_protocol(&mut self, opener: std::sync::Arc<dyn StreamOpener>) -> Result<()> {
        let bridge = Box::new(stream::Bridge::new(opener));
        let proto = cstr(stream::PROTOCOL)?;
        let rc = unsafe {
            (self.api.mpv_stream_cb_add_ro)(
                self.handle,
                proto.as_ptr(),
                &*bridge as *const stream::Bridge as *mut c_void,
                stream::open_cb,
            )
        };
        self.check(|| "stream_cb_add_ro".into(), rc)?;
        self.streams = Some(bridge);
        Ok(())
    }

    /// Totals across every stream mpv has opened through the bridge.
    pub fn stream_stats(&self) -> StreamStats {
        self.streams.as_ref().map(|b| b.stats()).unwrap_or_default()
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // Closes every open stream (their close_fn runs) before the bridge
        // they point into is dropped with `self.streams`.
        unsafe { (self.api.mpv_terminate_destroy)(self.handle) };
    }
}
