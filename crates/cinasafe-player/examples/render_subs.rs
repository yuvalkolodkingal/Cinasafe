//! Renders frames with subtitles burned in, through the vault and
//! `cinasafe://`, to check Hebrew/Arabic shaping and PGS by eye (brief M4).
//!
//! ```sh
//! cargo run -p cinasafe-player --example render_subs -- <out-dir> [fonts-dir]
//! ```
//!
//! Writes `<case>.png` files into `<out-dir>` (mpv's `screenshot-to-file`). Needs
//! ffmpeg with libx264. This writes images to disk on purpose; the app never does.
//! The `*-auto-dir` cases add `sub-ass-style-overrides=Encoding=-1` (per-line
//! base direction, docs/OPEN_QUESTIONS.md Q3) for comparison.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cinasafe_core::crypto::KdfParams;
use cinasafe_core::subtitles::{decode, fix_rtl_punctuation, SubFormat};
use cinasafe_core::{BlobWriteOptions, Vault};
use cinasafe_player::{Event, Mpv, VaultOpener};
use zeroize::Zeroizing;

const HEBREW_SRT_1255: &[u8] =
    b"1\r\n00:00:00,500 --> 00:00:02,500\r\n\xf9\xec\xe5\xed, \xf2\xe5\xec\xed!\r\n\xee\xe4 \xf9\xec\xe5\xee\xea? Fauda 2\r\n";

const ARABIC_ASS: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\n\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Noto Sans Arabic,28,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,20,1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.50,0:00:02.50,Default,,0,0,0,,مرحبا بالعالم!\\Nشاهدت فيلم Inception أمس.\n";

fn ffmpeg(args: &[&str]) {
    let ok =
        Command::new("ffmpeg").args(["-v", "error", "-y", "-nostdin"]).args(args).status().is_ok_and(|s| s.success());
    assert!(ok, "ffmpeg failed: {args:?}");
}

struct Case<'a> {
    name: &'a str,
    video: &'a Path,
    /// External subtitle bytes (UTF-8) and language; None = select the first embedded sub track.
    sub: Option<(Vec<u8>, &'a str)>,
    at: f64,
    /// Extra mpv options for this case.
    extra: &'a [(&'a str, &'a str)],
}

fn render(vault: &Arc<Vault>, out: &Path, fonts: Option<&Path>, case: &Case) {
    let mut mpv = Mpv::new().unwrap();
    let opts = [
        ("vo", "null"),
        ("ao", "null"),
        ("pause", "yes"),
        ("keep-open", "yes"),
        ("sub-font", "Noto Sans"),
        ("sub-border-style", "opaque-box"),
        ("sub-back-color", "#99000000"),
    ];
    for (k, v) in opts {
        if let Err(e) = mpv.set_option(k, v) {
            eprintln!("note: {e}");
        }
    }
    if let Some(f) = fonts {
        mpv.set_option("sub-fonts-dir", &f.to_string_lossy()).unwrap();
    }
    for (k, v) in case.extra {
        mpv.set_option(k, v).unwrap();
    }
    mpv.register_stream_protocol(Arc::new(VaultOpener(vault.clone()))).unwrap();
    mpv.initialize().unwrap();
    let bytes = std::fs::read(case.video).unwrap();
    let id = vault.write_blob(&bytes[..], bytes.len() as u64, &BlobWriteOptions::media()).unwrap();
    mpv.command(&["loadfile", &format!("cinasafe://{}", id.to_hex())]).unwrap();
    wait(&mpv, |e| *e == Event::FileLoaded);
    match &case.sub {
        Some((text, lang)) => {
            let sid = vault.write_blob(&text[..], text.len() as u64, &BlobWriteOptions::default()).unwrap();
            mpv.command(&["sub-add", &format!("cinasafe://{}", sid.to_hex()), "select", "", lang]).unwrap();
        }
        None => mpv.set_property("sid", "1").unwrap(),
    }
    mpv.command(&["seek", &case.at.to_string(), "absolute+exact"]).unwrap();
    wait(&mpv, |e| *e == Event::PlaybackRestart);
    // Give subtitle decoding a moment to catch up with the seek.
    std::thread::sleep(Duration::from_millis(500));
    let dest = out.join(format!("{}.png", case.name));
    mpv.command(&["screenshot-to-file", &dest.to_string_lossy(), "subtitles"]).unwrap();
    drop(mpv);
    println!("{}", dest.display());
}

fn wait(mpv: &Mpv, mut f: impl FnMut(&Event) -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        let e = mpv.wait_event(Duration::from_millis(100));
        if let Event::EndFile { error, .. } = e {
            assert!(error >= 0, "playback error {error}");
        }
        if f(&e) {
            return;
        }
    }
    panic!("timeout");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: render_subs <out-dir> [fonts-dir]"));
    let fonts = args.next().map(PathBuf::from);
    std::fs::create_dir_all(&out).unwrap();
    let work = tempfile::tempdir().unwrap();
    let clip = work.path().join("clip.mkv");
    let src = "testsrc2=duration=3:size=1280x720:rate=24";
    ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        src,
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
        &clip.to_string_lossy(),
    ]);
    let pgs = work.path().join("pgs.mkv");
    let sup = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pgs_sub.sup");
    ffmpeg(&["-i", &clip.to_string_lossy(), "-i", sup, "-map", "0", "-map", "1", "-c", "copy", &pgs.to_string_lossy()]);

    let params = KdfParams { m_kib: 64, t: 1, p: 1, salt: [5; 16] };
    let stick = tempfile::tempdir().unwrap();
    let (vault, _) = Vault::create_with_params(stick.path(), &Zeroizing::new("pw".into()), false, params).unwrap();
    let vault = Arc::new(vault);

    let hebrew = decode(HEBREW_SRT_1255).text;
    let hebrew_fixed = fix_rtl_punctuation(&hebrew, SubFormat::Srt);
    // docs/OPEN_QUESTIONS.md Q3: libass's base direction is LTR unless the style says otherwise.
    let auto_dir: &[(&str, &str)] = &[("sub-ass-style-overrides", "Encoding=-1")];
    let he = || Some((hebrew.clone().into_bytes(), "he"));
    let ar = || Some((ARABIC_ASS.as_bytes().to_vec(), "ar"));
    let cases = [
        Case { name: "hebrew-srt", video: &clip, sub: he(), at: 1.0, extra: &[] },
        Case {
            name: "hebrew-srt-rtl-fixed",
            video: &clip,
            sub: Some((hebrew_fixed.into_bytes(), "he")),
            at: 1.0,
            extra: &[],
        },
        Case { name: "hebrew-srt-auto-dir", video: &clip, sub: he(), at: 1.0, extra: auto_dir },
        Case { name: "arabic-ass", video: &clip, sub: ar(), at: 1.0, extra: &[] },
        Case { name: "arabic-ass-auto-dir", video: &clip, sub: ar(), at: 1.0, extra: auto_dir },
        Case { name: "pgs", video: &pgs, sub: None, at: 0.5, extra: &[] },
    ];
    for c in &cases {
        render(&vault, &out, fonts.as_deref(), c);
    }
}
