//! Glue between the vault and mpv: serving blobs to the stream bridge, and
//! reading stream information during import.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cinasafe_core::importer::{ProbeInfo, Prober};
use cinasafe_core::{BlobId, Vault};

use crate::{Event, MediaStream, Mpv, StreamOpener};

/// Serves `cinasafe://<blob>` from an unlocked vault.
pub struct VaultOpener(pub Arc<Vault>);

impl StreamOpener for VaultOpener {
    fn open(&self, id: BlobId) -> io::Result<Box<dyn MediaStream>> {
        self.0.open_blob(id).map(|r| Box::new(r) as Box<dyn MediaStream>).map_err(io::Error::other)
    }
}

/// Opens a blob in a headless mpv (no video or audio output) just long
/// enough to read its duration, container and track list.
pub struct MpvProber {
    pub timeout: Duration,
}

impl Default for MpvProber {
    fn default() -> Self {
        MpvProber { timeout: Duration::from_secs(20) }
    }
}

impl MpvProber {
    fn try_probe(&self, vault: &Arc<Vault>, blob: BlobId) -> crate::Result<Option<ProbeInfo>> {
        let mut mpv = Mpv::new()?;
        for (k, v) in [("vo", "null"), ("ao", "null"), ("pause", "yes"), ("sid", "no"), ("idle", "yes")] {
            mpv.set_option(k, v)?;
        }
        mpv.register_stream_protocol(Arc::new(VaultOpener(vault.clone())))?;
        mpv.initialize()?;
        mpv.command(&["loadfile", &format!("{}://{}", crate::stream::PROTOCOL, blob.to_hex())])?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            match mpv.wait_event(left.min(Duration::from_millis(500))) {
                Event::FileLoaded => break,
                Event::EndFile { .. } | Event::Shutdown => return Ok(None),
                _ => {}
            }
        }
        let duration_ms =
            mpv.get_f64("duration").ok().filter(|d| d.is_finite() && *d > 0.0).map(|d| (d * 1000.0) as i64);
        Ok(Some(ProbeInfo {
            duration_ms,
            container: mpv.get_string("file-format").ok(),
            // Node properties read as strings come back as JSON.
            streams_json: mpv.get_string("track-list").ok(),
        }))
    }
}

impl Prober for MpvProber {
    fn probe(&self, vault: &Arc<Vault>, blob: BlobId) -> Option<ProbeInfo> {
        self.try_probe(vault, blob).ok().flatten()
    }
}
