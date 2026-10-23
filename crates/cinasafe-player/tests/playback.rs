//! Brief 13 playback list and the M4 acceptance check, through the real path:
//! file → encrypted vault blob → `cinasafe://` → libmpv (headless).
//!
//! Media is generated with ffmpeg at test time. Each test skips (with a
//! note on stderr) when libmpv, ffmpeg or the needed encoder is missing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cinasafe_core::crypto::KdfParams;
use cinasafe_core::{BlobId, BlobWriteOptions, Vault};
use cinasafe_player::{Event, Mpv, VaultOpener};
use serde_json::Value;
use zeroize::Zeroizing;

const TIMEOUT: Duration = Duration::from_secs(15);

fn libmpv_available() -> bool {
    if cinasafe_player::ffi::api().is_err() {
        eprintln!("skipped: libmpv not available");
        return false;
    }
    true
}

fn has_encoder(name: &str) -> bool {
    let out = Command::new("ffmpeg").args(["-hide_banner", "-encoders"]).output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).lines().any(|l| l.split_whitespace().nth(1) == Some(name)),
        Err(_) => false,
    }
}

/// Runs ffmpeg quietly; false if it is missing or fails.
fn ffmpeg(args: &[&str]) -> bool {
    Command::new("ffmpeg").args(["-v", "error", "-y", "-nostdin"]).args(args).status().is_ok_and(|s| s.success())
}

/// A 3 s test pattern with a tone.
fn make_video(out: &Path, vcodec: &[&str]) -> bool {
    let mut args =
        vec!["-f", "lavfi", "-i", "testsrc2=duration=3:size=320x180:rate=24", "-f", "lavfi", "-i", "sine=duration=3"];
    args.extend_from_slice(vcodec);
    args.extend_from_slice(&["-c:a", "aac", "-shortest"]);
    let o = out.to_string_lossy().into_owned();
    args.push(&o);
    ffmpeg(&args)
}

struct Stick {
    _dir: tempfile::TempDir,
    vault: Arc<Vault>,
}

impl Stick {
    fn new() -> Stick {
        let dir = tempfile::tempdir().unwrap();
        let params = KdfParams { m_kib: 64, t: 1, p: 1, salt: [3; 16] };
        let (v, _) = Vault::create_with_params(dir.path(), &Zeroizing::new("pw".into()), false, params).unwrap();
        Stick { _dir: dir, vault: Arc::new(v) }
    }

    fn put_file(&self, p: &Path, media: bool) -> BlobId {
        self.put_bytes(&std::fs::read(p).unwrap(), media)
    }

    fn put_bytes(&self, b: &[u8], media: bool) -> BlobId {
        let opts = if media { BlobWriteOptions::media() } else { BlobWriteOptions::default() };
        self.vault.write_blob(b, b.len() as u64, &opts).unwrap()
    }
}

fn url(id: BlobId) -> String {
    format!("cinasafe://{}", id.to_hex())
}

/// Headless mpv with the app's safety options and the vault bridge.
struct Player {
    mpv: Mpv,
}

impl Player {
    fn new(stick: &Stick) -> Player {
        let mut mpv = Mpv::new().unwrap();
        for (k, v) in [("vo", "null"), ("ao", "null"), ("keep-open", "yes"), ("idle", "yes")] {
            mpv.set_option(k, v).unwrap();
        }
        mpv.register_stream_protocol(Arc::new(VaultOpener(stick.vault.clone()))).unwrap();
        mpv.initialize().unwrap();
        Player { mpv }
    }

    /// Waits for `want`; fails on a playback error or timeout.
    fn wait_for(&self, what: &str, mut want: impl FnMut(&Event) -> bool) {
        let end = Instant::now() + TIMEOUT;
        while Instant::now() < end {
            let ev = self.mpv.wait_event(Duration::from_millis(100));
            if let Event::EndFile { reason, error } = ev {
                assert!(error >= 0, "playback failed while waiting for {what}: reason {reason}, error {error}");
            }
            if want(&ev) {
                return;
            }
        }
        panic!("timed out waiting for {what}");
    }

    fn load(&self, id: BlobId) {
        self.mpv.command(&["loadfile", &url(id)]).unwrap();
        self.wait_for("file-loaded", |e| *e == Event::FileLoaded);
    }

    /// Plays until the position passes `secs`.
    fn play_past(&self, secs: f64) {
        let end = Instant::now() + TIMEOUT;
        loop {
            let ev = self.mpv.wait_event(Duration::from_millis(50));
            if let Event::EndFile { reason, error } = ev {
                assert!(error >= 0, "playback failed: reason {reason}, error {error}");
            }
            if self.mpv.get_f64("time-pos").unwrap_or(0.0) > secs {
                return;
            }
            assert!(Instant::now() < end, "never reached {secs} s");
        }
    }

    /// Pauses and seeks exactly to `secs`, then waits until playback settles there.
    fn seek_paused(&self, secs: f64) {
        self.mpv.set_property("pause", "yes").unwrap();
        self.mpv.command(&["seek", &secs.to_string(), "absolute+exact"]).unwrap();
        self.wait_for("playback-restart", |e| *e == Event::PlaybackRestart);
    }

    fn tracks(&self) -> Vec<Value> {
        let json = self.mpv.get_string("track-list").unwrap();
        serde_json::from_str::<Value>(&json).unwrap().as_array().cloned().unwrap_or_default()
    }

    fn track(&self, kind: &str) -> Option<Value> {
        self.tracks().into_iter().find(|t| t["type"] == kind)
    }

    /// Adds an external subtitle blob and selects it.
    fn add_sub(&self, id: BlobId, lang: &str) {
        self.mpv.command(&["sub-add", &url(id), "select", "", lang]).unwrap();
    }

    /// `sub-text`, polled briefly: subtitle decoding can trail the seek by a moment.
    fn sub_text(&self, expected: &str) -> String {
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let t = self.mpv.get_string("sub-text").unwrap_or_default();
            if t == expected || Instant::now() > end {
                return t;
            }
            self.mpv.wait_event(Duration::from_millis(50));
        }
    }
}

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn plays(vcodec: &[&str], expect_codec: &str, ext: &str) -> Option<(Stick, Player)> {
    let dir = scratch();
    let src: PathBuf = dir.path().join(format!("clip.{ext}"));
    assert!(make_video(&src, vcodec), "ffmpeg could not make the {expect_codec} sample");
    let stick = Stick::new();
    let id = stick.put_file(&src, true);
    let p = Player::new(&stick);
    p.load(id);
    let v = p.track("video").expect("a video track");
    assert_eq!(v["codec"], expect_codec, "{v}");
    p.play_past(1.0);
    Some((stick, p))
}

#[test]
fn h264_in_mkv() {
    if !libmpv_available() || !has_encoder("libx264") {
        return eprintln!("skipped: needs libmpv and ffmpeg with libx264");
    }
    plays(&["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"], "h264", "mkv");
}

#[test]
fn hevc_main10_in_mkv() {
    if !libmpv_available() || !has_encoder("libx265") {
        return eprintln!("skipped: needs libmpv and ffmpeg with libx265");
    }
    let (_s, p) = plays(
        &["-c:v", "libx265", "-preset", "ultrafast", "-pix_fmt", "yuv420p10le", "-x265-params", "log-level=error"],
        "hevc",
        "mkv",
    )
    .unwrap();
    let fmt = p.mpv.get_string("video-dec-params/pixelformat").unwrap_or_default();
    assert!(fmt.contains("10"), "decoded as {fmt}, expected a 10-bit format");
}

#[test]
fn av1_in_mkv() {
    if !libmpv_available() {
        return;
    }
    let enc: &[&str] = if has_encoder("libsvtav1") {
        &["-c:v", "libsvtav1", "-preset", "12"]
    } else if has_encoder("libaom-av1") {
        &["-c:v", "libaom-av1", "-cpu-used", "8", "-row-mt", "1"]
    } else if has_encoder("librav1e") {
        &["-c:v", "librav1e", "-speed", "10"]
    } else {
        return eprintln!("skipped: ffmpeg has no AV1 encoder");
    };
    plays(enc, "av1", "mkv");
}

#[test]
fn mp4_with_mov_text() {
    if !libmpv_available() || !has_encoder("libx264") || !has_encoder("mov_text") {
        return eprintln!("skipped: needs libmpv and ffmpeg with libx264 and mov_text");
    }
    let dir = scratch();
    let srt = dir.path().join("in.srt");
    std::fs::write(&srt, "1\n00:00:00,500 --> 00:00:02,500\nEmbedded mov_text line\n").unwrap();
    let mp4 = dir.path().join("clip.mp4");
    let (srt_s, mp4_s) = (srt.to_string_lossy().into_owned(), mp4.to_string_lossy().into_owned());
    assert!(ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc2=duration=3:size=320x180:rate=24",
        "-i",
        &srt_s,
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
        "-c:s",
        "mov_text",
        "-metadata:s:s:0",
        "language=eng",
        &mp4_s,
    ]));
    let stick = Stick::new();
    let id = stick.put_file(&mp4, true);
    let p = Player::new(&stick);
    p.load(id);
    let sub = p.track("sub").expect("a subtitle track");
    assert_eq!(sub["codec"], "mov_text", "{sub}");
    p.mpv.set_property("sid", &sub["id"].to_string()).unwrap();
    p.play_past(1.0);
    p.seek_paused(1.5);
    assert_eq!(p.sub_text("Embedded mov_text line"), "Embedded mov_text line");
}

/// A plain H.264 clip in the vault, loaded and paused; for the external-subtitle tests.
fn base_clip() -> Option<(Stick, Player)> {
    if !libmpv_available() || !has_encoder("libx264") {
        eprintln!("skipped: needs libmpv and ffmpeg with libx264");
        return None;
    }
    let dir = scratch();
    let src = dir.path().join("base.mkv");
    assert!(make_video(&src, &["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"]));
    let stick = Stick::new();
    let id = stick.put_file(&src, true);
    let p = Player::new(&stick);
    p.load(id);
    Some((stick, p))
}

#[test]
fn hebrew_srt_in_windows_1255() {
    let Some((stick, p)) = base_clip() else { return };
    // "שלום, עולם!" and "מה שלומך?" in Windows-1255, as an old Hebrew SRT would be.
    let srt: &[u8] = b"1\r\n00:00:00,500 --> 00:00:02,500\r\n\xf9\xec\xe5\xed, \xf2\xe5\xec\xed!\r\n\xee\xe4 \xf9\xec\xe5\xee\xea?\r\n";
    // Exactly what the importer does: detect, convert to UTF-8, encrypt.
    let decoded = cinasafe_core::subtitles::decode(srt);
    assert_eq!(decoded.source_encoding, "windows-1255");
    let id = stick.put_bytes(decoded.text.as_bytes(), false);
    p.add_sub(id, "he");
    p.seek_paused(1.0);
    assert_eq!(p.sub_text("שלום, עולם!\nמה שלומך?"), "שלום, עולם!\nמה שלומך?");
    let sub = p.track("sub").unwrap();
    assert_eq!((sub["external"].as_bool(), sub["lang"].as_str()), (Some(true), Some("he")), "{sub}");
}

pub const ARABIC_ASS: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\n\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Noto Sans Arabic,28,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,20,1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.50,0:00:02.50,Default,,0,0,0,,مرحبا بالعالم!\\Nشاهدت فيلم Inception أمس.\n";

#[test]
fn arabic_ass() {
    let Some((stick, p)) = base_clip() else { return };
    let id = stick.put_bytes(ARABIC_ASS.as_bytes(), false);
    p.add_sub(id, "ar");
    p.seek_paused(1.0);
    assert_eq!(p.sub_text("مرحبا بالعالم!\nشاهدت فيلم Inception أمس."), "مرحبا بالعالم!\nشاهدت فيلم Inception أمس.");
    let sub = p.track("sub").unwrap();
    assert_eq!(sub["codec"], "ass", "{sub}");
}

#[test]
fn mkv_with_pgs() {
    if !libmpv_available() || !has_encoder("libx264") {
        return eprintln!("skipped: needs libmpv and ffmpeg with libx264");
    }
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pgs_sub.sup");
    let dir = scratch();
    let video = dir.path().join("v.mkv");
    let muxed = dir.path().join("pgs.mkv");
    assert!(make_video(&video, &["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"]));
    let (v, m) = (video.to_string_lossy().into_owned(), muxed.to_string_lossy().into_owned());
    assert!(ffmpeg(&["-i", &v, "-i", fixture, "-map", "0", "-map", "1", "-c", "copy", &m]));
    let stick = Stick::new();
    let id = stick.put_file(&muxed, true);
    let p = Player::new(&stick);
    p.load(id);
    let sub = p.track("sub").expect("the PGS track");
    assert_eq!(sub["codec"], "hdmv_pgs_subtitle", "{sub}");
    p.mpv.set_property("sid", &sub["id"].to_string()).unwrap();
    p.seek_paused(0.5);
    p.mpv.set_property("pause", "no").unwrap();
    p.play_past(1.0);
    // Bitmap subtitles carry no text.
    assert_eq!(p.mpv.get_string("sub-text").unwrap_or_default(), "");
    assert_eq!(p.mpv.get_string("sid").unwrap(), sub["id"].to_string());
}
