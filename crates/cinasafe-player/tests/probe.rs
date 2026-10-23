//! Probing a real (tiny) video through the vault. Skips when libmpv or
//! ffmpeg isn't installed.

use std::process::Command;
use std::sync::Arc;

use cinasafe_core::crypto::KdfParams;
use cinasafe_core::importer::Prober;
use cinasafe_core::{BlobWriteOptions, Vault};
use cinasafe_player::MpvProber;
use zeroize::Zeroizing;

#[test]
fn probes_duration_container_and_tracks() {
    if cinasafe_player::ffi::api().is_err() {
        eprintln!("skipped: libmpv not available");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    let ok = Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i", "testsrc=duration=3:size=160x120:rate=10"])
        .args(["-f", "lavfi", "-i", "sine=duration=3", "-c:v", "mpeg4", "-c:a", "mp2", "-shortest"])
        .arg(&src)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("skipped: ffmpeg not available");
        return;
    }
    let stick = tempfile::tempdir().unwrap();
    let params = KdfParams { m_kib: 64, t: 1, p: 1, salt: [1; 16] };
    let (v, _) = Vault::create_with_params(stick.path(), &Zeroizing::new("pw".into()), false, params).unwrap();
    let v = Arc::new(v);
    let bytes = std::fs::read(&src).unwrap();
    let blob = v.write_blob(&bytes[..], bytes.len() as u64, &BlobWriteOptions::media()).unwrap();

    let info = MpvProber::default().probe(&v, blob).expect("probe");
    let ms = info.duration_ms.unwrap();
    assert!((2900..=3100).contains(&ms), "{ms}");
    let container = info.container.unwrap();
    assert!(container.contains("mkv") || container.contains("matroska"), "{container}");
    let tracks: serde_json::Value = serde_json::from_str(info.streams_json.as_deref().unwrap()).unwrap();
    let types: Vec<_> = tracks.as_array().unwrap().iter().map(|t| t["type"].as_str().unwrap().to_owned()).collect();
    assert!(types.contains(&"video".into()) && types.contains(&"audio".into()), "{types:?}");
}
