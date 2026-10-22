//! `mpv_stream_cb_add_ro` callbacks for `cinasafe://<blob-id-hex>`.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cinasafe_core::{BlobId, BlobReader};

use crate::ffi::*;

pub const PROTOCOL: &str = "cinasafe";

/// A plaintext view of one blob.
pub trait MediaStream: Read + Seek + Send {
    fn size(&self) -> Option<u64>;
}

impl MediaStream for BlobReader {
    fn size(&self) -> Option<u64> {
        Some(self.len())
    }
}

/// Resolves a blob id to a stream; implemented by the vault.
pub trait StreamOpener: Send + Sync {
    fn open(&self, id: BlobId) -> io::Result<Box<dyn MediaStream>>;
}

/// Read throughput as seen by mpv. Feeds the "stick too slow" notice (6.4).
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamStats {
    pub opens: u64,
    pub open_failures: u64,
    pub reads: u64,
    pub bytes: u64,
    pub read_time: Duration,
    pub seeks: u64,
    pub max_read: Duration,
}

impl StreamStats {
    pub fn read_mib_per_s(&self) -> f64 {
        let secs = self.read_time.as_secs_f64();
        if secs == 0.0 {
            0.0
        } else {
            self.bytes as f64 / (1024.0 * 1024.0) / secs
        }
    }
}

pub(crate) struct Bridge {
    opener: Arc<dyn StreamOpener>,
    stats: Arc<Mutex<StreamStats>>,
}

impl Bridge {
    pub(crate) fn new(opener: Arc<dyn StreamOpener>) -> Bridge {
        Bridge { opener, stats: Arc::default() }
    }

    pub(crate) fn stats(&self) -> StreamStats {
        *self.stats.lock().unwrap()
    }
}

struct OpenStream {
    stream: Box<dyn MediaStream>,
    stats: Arc<Mutex<StreamStats>>,
}

fn parse_uri(uri: &str) -> Option<BlobId> {
    BlobId::from_hex(uri.strip_prefix(PROTOCOL)?.strip_prefix("://")?)
}

pub(crate) unsafe extern "C" fn open_cb(user: *mut c_void, uri: *mut c_char, info: *mut mpv_stream_cb_info) -> c_int {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let bridge = unsafe { &*(user as *const Bridge) };
        let uri = unsafe { CStr::from_ptr(uri) }.to_str().ok();
        let Some(id) = uri.and_then(parse_uri) else {
            return MPV_ERROR_LOADING_FAILED;
        };
        match bridge.opener.open(id) {
            Ok(stream) => {
                bridge.stats.lock().unwrap().opens += 1;
                let cookie = Box::into_raw(Box::new(OpenStream { stream, stats: bridge.stats.clone() }));
                unsafe {
                    *info = mpv_stream_cb_info {
                        cookie: cookie as *mut c_void,
                        read_fn: Some(read_cb),
                        seek_fn: Some(seek_cb),
                        size_fn: Some(size_cb),
                        close_fn: Some(close_cb),
                        cancel_fn: None,
                    };
                }
                0
            }
            Err(_) => {
                bridge.stats.lock().unwrap().open_failures += 1;
                MPV_ERROR_LOADING_FAILED
            }
        }
    }));
    r.unwrap_or(MPV_ERROR_LOADING_FAILED)
}

unsafe extern "C" fn read_cb(cookie: *mut c_void, buf: *mut c_char, n: u64) -> i64 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let s = unsafe { &mut *(cookie as *mut OpenStream) };
        let len = usize::try_from(n).unwrap_or(usize::MAX);
        let out = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, len) };
        let t = Instant::now();
        let res = loop {
            match s.stream.read(out) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                other => break other,
            }
        };
        let dt = t.elapsed();
        match res {
            Ok(k) => {
                let mut st = s.stats.lock().unwrap();
                st.reads += 1;
                st.bytes += k as u64;
                st.read_time += dt;
                st.max_read = st.max_read.max(dt);
                k as i64
            }
            Err(_) => -1,
        }
    }));
    r.unwrap_or(-1)
}

unsafe extern "C" fn seek_cb(cookie: *mut c_void, offset: i64) -> i64 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let s = unsafe { &mut *(cookie as *mut OpenStream) };
        let Ok(off) = u64::try_from(offset) else { return MPV_ERROR_GENERIC };
        s.stats.lock().unwrap().seeks += 1;
        match s.stream.seek(SeekFrom::Start(off)) {
            Ok(p) => p as i64,
            Err(_) => MPV_ERROR_GENERIC,
        }
    }));
    r.unwrap_or(MPV_ERROR_GENERIC)
}

unsafe extern "C" fn size_cb(cookie: *mut c_void) -> i64 {
    let s = unsafe { &*(cookie as *const OpenStream) };
    match s.stream.size() {
        Some(n) => i64::try_from(n).unwrap_or(MPV_ERROR_UNSUPPORTED),
        None => MPV_ERROR_UNSUPPORTED,
    }
}

unsafe extern "C" fn close_cb(cookie: *mut c_void) {
    // Dropping the BlobReader zeroizes its chunk cache.
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(cookie as *mut OpenStream) })));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_parsing() {
        let hex = "00112233445566778899aabbccddeeff";
        assert_eq!(parse_uri(&format!("cinasafe://{hex}")).unwrap().to_hex(), hex);
        assert!(parse_uri(&format!("file://{hex}")).is_none());
        assert!(parse_uri("cinasafe://../secret").is_none());
        assert!(parse_uri(&format!("cinasafe://{hex}/x")).is_none());
    }
}
