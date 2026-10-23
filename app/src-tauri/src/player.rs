//! One playback session: an mpv instance reading `cinasafe://` from the
//! vault, drawing under the webview, reporting state to the UI (brief 6.4,
//! 7.3) and saving the resume position in the library.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cinasafe_core::library::{Id, Library, SubtitleFile};
use cinasafe_core::subtitles::{fix_rtl_punctuation, SubFormat};
use cinasafe_core::{BlobWriteOptions, Vault};
use cinasafe_player::{ffi, Event, Mpv, PropertyValue, VaultOpener};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::dto::{track_from_node, TrackInfo};
use crate::error::{AppError, Result};
use crate::settings::Settings;
use crate::video;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub title: Option<String>,
    pub time: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextUp {
    pub media_file_id: Id,
    pub title: String,
    pub line: String,
    pub thumb_id: Option<Id>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub media_file_id: Id,
    pub title: String,
    pub line: Option<String>,
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub buffering: bool,
    pub volume: f64,
    pub muted: bool,
    pub speed: f64,
    pub fullscreen: bool,
    pub audio_tracks: Vec<TrackInfo>,
    pub sub_tracks: Vec<TrackInfo>,
    pub audio_id: Option<i64>,
    pub sub_id: Option<i64>,
    pub sub_delay_ms: i64,
    pub sub_scale: f64,
    pub sub_pos: f64,
    pub rtl_fix: Option<bool>,
    pub chapters: Vec<Chapter>,
    pub next: Option<NextUp>,
    pub slow_stick: bool,
    pub error: Option<&'static str>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PlayerCommand {
    Play,
    Pause,
    TogglePause,
    Seek {
        seconds: f64,
        #[serde(default)]
        relative: bool,
    },
    Volume {
        value: f64,
    },
    Mute {
        on: bool,
    },
    SetAudio {
        id: Option<i64>,
    },
    SetSub {
        id: Option<i64>,
    },
    SubDelay {
        ms: i64,
    },
    SubScale {
        value: f64,
    },
    SubPos {
        value: f64,
    },
    Speed {
        value: f64,
    },
    RtlFix {
        on: bool,
    },
    Fullscreen {
        on: bool,
    },
    Next,
}

/// ISO 639-1/639-2 codes and regions reduced to one comparable key.
pub fn lang_key(code: &str) -> String {
    let c = code.trim().to_ascii_lowercase();
    let c = c.split(['-', '_']).next().unwrap_or_default();
    let two = match c {
        "eng" => "en",
        "heb" | "iw" => "he",
        "ara" | "arb" => "ar",
        "fre" | "fra" => "fr",
        "ger" | "deu" => "de",
        "spa" => "es",
        "rus" => "ru",
        "ita" => "it",
        "por" => "pt",
        "jpn" => "ja",
        "chi" | "zho" => "zh",
        "kor" => "ko",
        "dut" | "nld" => "nl",
        "pol" => "pl",
        "tur" => "tr",
        "swe" => "sv",
        "per" | "fas" => "fa",
        "yid" => "yi",
        other => other,
    };
    two.to_owned()
}

struct SubEntry {
    file: SubtitleFile,
    url: String,
}

struct Inner {
    state: PlayerState,
    subs: Vec<SubEntry>,
    /// Raw track-list, to map external tracks back to library subtitles.
    last_track_list: Value,
    file_loaded: bool,
    ended: bool,
    last_record: Instant,
    last_emit: Instant,
    last_speed_check: Instant,
    bytes_per_s: Option<f64>,
    audio_prefs: Vec<String>,
    sub_prefs: Vec<String>,
}

pub struct Session {
    app: AppHandle,
    vault: Arc<Vault>,
    mpv: Arc<Mpv>,
    media_file_id: Id,
    inner: Arc<Mutex<Inner>>,
    stop: Arc<AtomicBool>,
    events: Option<JoinHandle<()>>,
}

fn sub_url(blob: cinasafe_core::BlobId) -> String {
    format!("cinasafe://{}", blob.to_hex())
}

/// Title, episode line, next episode and per-title language overrides.
struct Describe {
    title: String,
    line: Option<String>,
    next: Option<NextUp>,
    audio_override: Option<String>,
    sub_override: Option<String>,
}

fn describe(lib: &Library, media_file_id: Id) -> Result<Describe> {
    if let Some(m) = lib.movie_by_media(media_file_id)? {
        return Ok(Describe {
            title: m.title,
            line: None,
            next: None,
            audio_override: m.audio_lang_override,
            sub_override: m.subtitle_lang_override,
        });
    }
    let ep = lib.episodes_by_media(media_file_id)?.into_iter().next().ok_or_else(AppError::not_found)?;
    let show = lib.show(ep.show_id)?;
    let ep_line = |e: &cinasafe_core::library::Episode| {
        let mut s = format!("S{} · E{}", e.season_number, e.number);
        if let Some(t) = &e.title {
            s.push_str(" · ");
            s.push_str(t);
        }
        s
    };
    let next = lib.next_episode(ep.id)?.map(|n| NextUp {
        media_file_id: n.media_file_id,
        title: show.title.clone(),
        line: ep_line(&n),
        thumb_id: n.thumb_id,
    });
    Ok(Describe {
        title: show.title.clone(),
        line: Some(ep_line(&ep)),
        next,
        audio_override: show.audio_lang_override,
        sub_override: show.subtitle_lang_override,
    })
}

impl Session {
    pub fn open(
        app: &AppHandle,
        vault: Arc<Vault>,
        s: &Settings,
        media_file_id: Id,
        from_start: bool,
    ) -> Result<Session> {
        let (file, desc, progress, subs) = {
            let lib = vault.library();
            let file = lib.media_file(media_file_id)?;
            (file, describe(&lib, media_file_id)?, lib.progress(media_file_id)?, lib.subtitles(media_file_id)?)
        };
        let resume = if from_start || progress.watched { 0.0 } else { progress.position_ms as f64 / 1000.0 };

        let mut mpv = Mpv::new()?;
        // Debugging only: mpv's log on stderr. Off by default (logs may name tracks).
        if std::env::var_os("CINASAFE_MPV_LOG").is_some() {
            mpv.set_option("terminal", "yes")?;
            mpv.set_option("msg-level", "all=v")?;
        }
        for (k, v) in [("keep-open", "yes"), ("idle", "yes"), ("hwdec", if s.hwdec { "auto-safe" } else { "no" })] {
            mpv.set_option(k, v)?;
        }
        // Subtitle look (7.3). Cosmetic: a rejected value doesn't stop playback.
        let fonts = app.path().resource_dir().map(|d| d.join("fonts")).ok().filter(|d| d.is_dir());
        if let Some(d) = &fonts {
            let _ = mpv.set_option("sub-fonts-dir", &d.to_string_lossy());
        }
        let _ = mpv.set_option("sub-font", "Noto Sans");
        let _ = mpv.set_option("sub-scale", &s.subtitle_scale.to_string());
        let _ = mpv.set_option("sub-pos", &s.subtitle_pos.to_string());
        let _ = mpv.set_option("sub-color", &s.subtitle_color);
        // Each line's base direction from its own text (brief 7.2): Hebrew and
        // Arabic lines keep their final punctuation and embedded Latin words in
        // the right place without the "Fix RTL punctuation" rewrite
        // (docs/OPEN_QUESTIONS.md Q3).
        let mut overrides = vec!["Encoding=-1"];
        if s.subtitle_background {
            let _ = mpv.set_option("sub-back-color", "#99000000");
            // mpv 0.38+; older libmpv (e.g. Ubuntu's 0.37) gets libass's opaque box instead.
            if mpv.set_option("sub-border-style", "background-box").is_err() {
                overrides.extend(["BorderStyle=3", "OutlineColour=&H66000000"]);
            }
        }
        let _ = mpv.set_option("sub-ass-style-overrides", &overrides.join(","));
        video::prepare(&mpv)?;
        mpv.register_stream_protocol(Arc::new(VaultOpener(vault.clone())))?;
        mpv.initialize()?;
        let mpv = Arc::new(mpv);
        let software_gl = video::attach(app, mpv.clone()).map_err(|e| {
            // Graphics-driver detail, no vault data: shown in the player and on stderr.
            eprintln!("cinasafe: video output failed: {e}");
            AppError::new("videoUnavailable", e)
        })?;
        if software_gl {
            // Default scaling and HDR shaders are too slow on a software
            // rasteriser and end up drawing nothing (docs/OPEN_QUESTIONS.md Q6).
            let _ = mpv.command(&["apply-profile", "fast"]);
        }

        let prefs = |over: Option<String>, list: &[String]| -> Vec<String> {
            over.into_iter().chain(list.iter().cloned()).map(|l| lang_key(&l)).collect()
        };
        let duration = file.duration_ms.map(|d| d as f64 / 1000.0);
        let inner = Inner {
            state: PlayerState {
                media_file_id,
                title: desc.title,
                line: desc.line,
                position: resume,
                duration,
                paused: false,
                volume: 100.0,
                speed: 1.0,
                sub_scale: s.subtitle_scale,
                sub_pos: s.subtitle_pos,
                next: desc.next,
                ..Default::default()
            },
            subs: subs
                .into_iter()
                .map(|f| SubEntry { url: sub_url(Library::subtitle_blob_to_play(&f)), file: f })
                .collect(),
            last_track_list: Value::Null,
            file_loaded: false,
            ended: false,
            last_record: Instant::now(),
            last_emit: Instant::now() - Duration::from_secs(1),
            last_speed_check: Instant::now(),
            bytes_per_s: duration.filter(|d| *d > 0.0).map(|d| file.size_bytes as f64 / d),
            audio_prefs: prefs(desc.audio_override, &s.audio_languages),
            sub_prefs: prefs(desc.sub_override, &s.subtitle_languages),
        };

        if resume > 1.0 {
            mpv.set_property("start", &format!("{resume:.3}"))?;
        }
        mpv.command(&["loadfile", &sub_url(file.blob_id)])?;

        let mut session = Session {
            app: app.clone(),
            vault,
            mpv,
            media_file_id,
            inner: Arc::new(Mutex::new(inner)),
            stop: Arc::default(),
            events: None,
        };
        session.start_events()?;
        Ok(session)
    }

    pub fn media_file_id(&self) -> Id {
        self.media_file_id
    }

    pub fn state(&self) -> PlayerState {
        self.inner.lock().unwrap().state.clone()
    }

    pub fn is_paused(&self) -> bool {
        let i = self.inner.lock().unwrap();
        i.state.paused || i.ended
    }

    pub fn next(&self) -> Option<Id> {
        self.inner.lock().unwrap().state.next.as_ref().map(|n| n.media_file_id)
    }

    fn start_events(&mut self) -> Result<()> {
        use ffi::{MPV_FORMAT_DOUBLE as D, MPV_FORMAT_FLAG as F, MPV_FORMAT_STRING as S};
        let props: [(&str, i32); 15] = [
            ("time-pos", D),
            ("duration", D),
            ("pause", F),
            ("paused-for-cache", F),
            ("volume", D),
            ("mute", F),
            ("speed", D),
            ("aid", S),
            ("sid", S),
            ("sub-delay", D),
            ("sub-scale", D),
            ("sub-pos", D),
            ("track-list", S),
            ("chapter-list", S),
            ("eof-reached", F),
        ];
        for (i, (name, fmt)) in props.iter().enumerate() {
            self.mpv.observe(i as u64 + 1, name, *fmt)?;
        }
        let (mpv, inner, stop, app, vault) =
            (self.mpv.clone(), self.inner.clone(), self.stop.clone(), self.app.clone(), self.vault.clone());
        let id = self.media_file_id;
        self.events = Some(
            std::thread::Builder::new()
                .name("player-events".into())
                .spawn(move || event_loop(mpv, inner, stop, app, vault, id))?,
        );
        Ok(())
    }

    pub fn command(&self, cmd: PlayerCommand) -> Result<()> {
        let m = &self.mpv;
        match cmd {
            PlayerCommand::Play => m.set_property("pause", "no")?,
            PlayerCommand::Pause => m.set_property("pause", "yes")?,
            PlayerCommand::TogglePause => m.command(&["cycle", "pause"])?,
            PlayerCommand::Seek { seconds, relative } => {
                m.command(&["seek", &format!("{seconds:.3}"), if relative { "relative" } else { "absolute" }])?
            }
            PlayerCommand::Volume { value } => m.set_property("volume", &value.clamp(0.0, 100.0).to_string())?,
            PlayerCommand::Mute { on } => m.set_property("mute", if on { "yes" } else { "no" })?,
            PlayerCommand::SetAudio { id } => m.set_property("aid", &id.map_or("no".into(), |i| i.to_string()))?,
            PlayerCommand::SetSub { id } => {
                m.set_property("sid", &id.map_or("no".into(), |i| i.to_string()))?;
                self.apply_stored_delay(id);
            }
            PlayerCommand::SubDelay { ms } => {
                m.set_property("sub-delay", &format!("{:.3}", ms as f64 / 1000.0))?;
                if let Some(f) = self.current_sub_file() {
                    self.vault.library().set_subtitle_delay(f.id, ms)?;
                }
            }
            PlayerCommand::SubScale { value } => m.set_property("sub-scale", &value.clamp(0.5, 3.0).to_string())?,
            PlayerCommand::SubPos { value } => m.set_property("sub-pos", &value.clamp(0.0, 150.0).to_string())?,
            PlayerCommand::Speed { value } => m.set_property("speed", &value.clamp(0.25, 4.0).to_string())?,
            PlayerCommand::RtlFix { on } => self.set_rtl_fix(on)?,
            PlayerCommand::Fullscreen { on } => {
                if let Some(w) = self.app.get_webview_window("main") {
                    w.set_fullscreen(on)?;
                }
                self.inner.lock().unwrap().state.fullscreen = on;
                self.emit();
            }
            PlayerCommand::Next => {} // handled by the caller (it opens a new session)
        }
        Ok(())
    }

    fn emit(&self) {
        let st = self.inner.lock().unwrap().state.clone();
        let _ = self.app.emit("player-state", st);
    }

    /// The imported subtitle file behind the selected track, if any.
    fn current_sub_file(&self) -> Option<SubtitleFile> {
        let i = self.inner.lock().unwrap();
        let sid = i.state.sub_id?;
        let fid = i.state.sub_tracks.iter().find(|t| t.id == sid)?.subtitle_file_id?;
        i.subs.iter().find(|s| s.file.id == fid).map(|s| s.file.clone())
    }

    fn apply_stored_delay(&self, sid: Option<i64>) {
        let delay = {
            let i = self.inner.lock().unwrap();
            sid.and_then(|sid| i.state.sub_tracks.iter().find(|t| t.id == sid))
                .and_then(|t| t.subtitle_file_id)
                .and_then(|fid| i.subs.iter().find(|s| s.file.id == fid))
                .map_or(0, |s| s.file.delay_ms)
        };
        let _ = self.mpv.set_property("sub-delay", &format!("{:.3}", delay as f64 / 1000.0));
    }

    /// Brief 7.2: switch the selected imported subtitle to its RTL-fixed copy
    /// (made on first use and stored as a new blob) or back to the original.
    fn set_rtl_fix(&self, on: bool) -> Result<()> {
        let file = self.current_sub_file().ok_or_else(AppError::not_found)?;
        if on && !file.has_rtl_copy {
            let original = self.vault.read_blob(file.blob_id)?;
            let text = String::from_utf8_lossy(&original);
            let format = SubFormat::from_extension(&file.format).unwrap_or(SubFormat::Srt);
            let fixed = zeroize::Zeroizing::new(fix_rtl_punctuation(&text, format).into_bytes());
            let blob = self.vault.write_blob(&fixed[..], fixed.len() as u64, &BlobWriteOptions::default())?;
            self.vault.library().set_subtitle_rtl_copy(file.id, blob)?;
        } else {
            self.vault.library().set_subtitle_rtl_fix(file.id, on)?;
        }
        let updated = self.vault.library().subtitle(file.id)?;
        let url = sub_url(Library::subtitle_blob_to_play(&updated));
        let old_track = self.inner.lock().unwrap().state.sub_id;
        {
            let mut i = self.inner.lock().unwrap();
            if let Some(e) = i.subs.iter_mut().find(|s| s.file.id == file.id) {
                e.url = url.clone();
                e.file = updated.clone();
            }
        }
        if let Some(t) = old_track {
            let _ = self.mpv.command(&["sub-remove", &t.to_string()]);
        }
        let lang = updated.language.clone().unwrap_or_default();
        self.mpv.command(&["sub-add", &url, "select", "", &lang])?;
        self.vault.save_library_if_dirty()?;
        Ok(())
    }

    /// Stops playback and destroys mpv. `save` records the position.
    pub fn close(mut self, save: bool) {
        self.stop.store(true, Ordering::Relaxed);
        self.mpv.wakeup();
        if let Some(h) = self.events.take() {
            let _ = h.join();
        }
        if save {
            let (pos, dur, ended) = {
                let i = self.inner.lock().unwrap();
                (i.state.position, i.state.duration, i.ended)
            };
            let pos_ms = if ended { dur.unwrap_or(pos) } else { pos } * 1000.0;
            let _ = self.vault.library().record_position(
                self.media_file_id,
                pos_ms as i64,
                dur.map(|d| (d * 1000.0) as i64),
            );
            let _ = self.vault.save_library_if_dirty();
        }
        // The render context must go before mpv itself.
        video::detach(&self.app);
        if let Some(w) = self.app.get_webview_window("main") {
            if self.inner.lock().unwrap().state.fullscreen {
                let _ = w.set_fullscreen(false);
            }
        }
    }
}

fn opt_id(v: &str) -> Option<i64> {
    v.parse().ok()
}

/// Updates the track lists from mpv's track-list JSON.
fn apply_track_list(i: &mut Inner, json: &str) {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return };
    let mut audio = Vec::new();
    let mut subs = Vec::new();
    for t in v.as_array().into_iter().flatten() {
        let Some(mut info) = track_from_node(t) else { continue };
        if let Some(fname) = t.get("external-filename").and_then(Value::as_str) {
            if let Some(e) = i.subs.iter().find(|s| s.url == fname) {
                info.subtitle_file_id = Some(e.file.id);
                info.lang = info.lang.or(e.file.language.clone());
                info.codec = Some(e.file.format.clone());
            }
        }
        match info.kind.as_str() {
            "audio" => audio.push(info),
            "sub" => subs.push(info),
            _ => {}
        }
    }
    i.state.audio_tracks = audio;
    i.state.sub_tracks = subs;
    i.last_track_list = v;
    update_rtl_flag(i);
}

fn update_rtl_flag(i: &mut Inner) {
    let fid =
        i.state.sub_id.and_then(|sid| i.state.sub_tracks.iter().find(|t| t.id == sid)).and_then(|t| t.subtitle_file_id);
    i.state.rtl_fix = fid.and_then(|f| i.subs.iter().find(|s| s.file.id == f)).map(|s| s.file.rtl_fix);
}

/// After the file loads: add imported subtitles, then pick tracks by language preference.
fn on_file_loaded(mpv: &Mpv, inner: &Mutex<Inner>) {
    let subs: Vec<(String, String)> = {
        let i = inner.lock().unwrap();
        i.subs.iter().map(|s| (s.url.clone(), s.file.language.clone().unwrap_or_default())).collect()
    };
    for (url, lang) in &subs {
        let _ = mpv.command(&["sub-add", url, "auto", "", lang]);
    }
    // Read the full list now (the observer may lag behind the sub-adds).
    if let Ok(json) = mpv.get_string("track-list") {
        apply_track_list(&mut inner.lock().unwrap(), &json);
    }
    let (aid, sid, delay) = {
        let i = inner.lock().unwrap();
        let pick = |tracks: &[TrackInfo], prefs: &[String], prefer_external: bool| -> Option<i64> {
            prefs.iter().find_map(|p| {
                let mut c: Vec<&TrackInfo> =
                    tracks.iter().filter(|t| t.lang.as_deref().map(lang_key).as_deref() == Some(p)).collect();
                if prefer_external {
                    c.sort_by_key(|t| (t.forced, !t.external));
                }
                c.first().map(|t| t.id)
            })
        };
        // No preference set: a subtitle file the user imported was meant to be used.
        let sid = if i.sub_prefs.is_empty() {
            i.state.sub_tracks.iter().find(|t| t.subtitle_file_id.is_some()).map(|t| t.id)
        } else {
            pick(&i.state.sub_tracks, &i.sub_prefs, true)
        };
        let delay = sid
            .and_then(|s| i.state.sub_tracks.iter().find(|t| t.id == s))
            .and_then(|t| t.subtitle_file_id)
            .and_then(|f| i.subs.iter().find(|s| s.file.id == f))
            .map(|s| s.file.delay_ms);
        (pick(&i.state.audio_tracks, &i.audio_prefs, false), sid, delay)
    };
    if let Some(a) = aid {
        let _ = mpv.set_property("aid", &a.to_string());
    }
    if let Some(s) = sid {
        let _ = mpv.set_property("sid", &s.to_string());
    }
    if let Some(d) = delay {
        let _ = mpv.set_property("sub-delay", &format!("{:.3}", d as f64 / 1000.0));
    }
}

fn event_loop(
    mpv: Arc<Mpv>,
    inner: Arc<Mutex<Inner>>,
    stop: Arc<AtomicBool>,
    app: AppHandle,
    vault: Arc<Vault>,
    id: Id,
) {
    while !stop.load(Ordering::Relaxed) {
        let ev = mpv.wait_event(Duration::from_millis(250));
        let mut emit_now = false;
        let mut ended_now = None;
        match ev {
            Event::Shutdown => break,
            Event::FileLoaded => {
                let first = !std::mem::replace(&mut inner.lock().unwrap().file_loaded, true);
                if first {
                    on_file_loaded(&mpv, &inner);
                }
                emit_now = true;
            }
            Event::EndFile { error, .. } if error < 0 => {
                inner.lock().unwrap().state.error = Some("player");
                emit_now = true;
            }
            Event::PropertyChange { name, value, .. } => {
                let mut i = inner.lock().unwrap();
                emit_now = name != "time-pos";
                match (name.as_str(), value) {
                    ("time-pos", PropertyValue::Double(d)) => i.state.position = d,
                    ("duration", PropertyValue::Double(d)) if d > 0.0 => i.state.duration = Some(d),
                    ("pause", PropertyValue::Flag(b)) => i.state.paused = b,
                    ("paused-for-cache", PropertyValue::Flag(b)) => i.state.buffering = b,
                    ("volume", PropertyValue::Double(d)) => i.state.volume = d,
                    ("mute", PropertyValue::Flag(b)) => i.state.muted = b,
                    ("speed", PropertyValue::Double(d)) => i.state.speed = d,
                    ("aid", PropertyValue::Str(s)) => i.state.audio_id = opt_id(&s),
                    ("sid", PropertyValue::Str(s)) => {
                        i.state.sub_id = opt_id(&s);
                        update_rtl_flag(&mut i);
                    }
                    ("sub-delay", PropertyValue::Double(d)) => i.state.sub_delay_ms = (d * 1000.0).round() as i64,
                    ("sub-scale", PropertyValue::Double(d)) => i.state.sub_scale = d,
                    ("sub-pos", PropertyValue::Double(d)) => i.state.sub_pos = d,
                    ("track-list", PropertyValue::Str(s)) => apply_track_list(&mut i, &s),
                    ("chapter-list", PropertyValue::Str(s)) => {
                        i.state.chapters = serde_json::from_str::<Vec<Value>>(&s)
                            .unwrap_or_default()
                            .iter()
                            .map(|c| Chapter {
                                title: c.get("title").and_then(Value::as_str).map(str::to_owned),
                                time: c.get("time").and_then(Value::as_f64).unwrap_or(0.0),
                            })
                            .collect();
                    }
                    ("eof-reached", PropertyValue::Flag(true)) if !i.ended => {
                        i.ended = true;
                        ended_now = Some(i.state.next.as_ref().map(|n| n.media_file_id));
                    }
                    ("eof-reached", PropertyValue::Flag(false)) => i.ended = false,
                    _ => emit_now = false,
                }
            }
            _ => {}
        }

        // Resume point every 10 s of playback (brief 6.4).
        let record = {
            let mut i = inner.lock().unwrap();
            if i.last_record.elapsed() >= Duration::from_secs(10) && i.file_loaded {
                i.last_record = Instant::now();
                Some((i.state.position, i.state.duration))
            } else {
                None
            }
        };
        if let Some((pos, dur)) = record {
            let _ = vault.library().record_position(id, (pos * 1000.0) as i64, dur.map(|d| (d * 1000.0) as i64));
            let _ = vault.save_library_if_dirty();
        }
        if let Some(next) = ended_now {
            if let Some((_, dur)) = record.or(Some((0.0, inner.lock().unwrap().state.duration))) {
                let d = dur.unwrap_or(0.0) * 1000.0;
                let _ = vault.library().record_position(id, d as i64, Some(d as i64));
                let _ = vault.save_library_if_dirty();
            }
            let _ = app.emit("player-ended", serde_json::json!({ "nextMediaFileId": next }));
        }

        // "This stick may be too slow for this file" (6.4).
        let slow_now = {
            let mut i = inner.lock().unwrap();
            if i.last_speed_check.elapsed() >= Duration::from_secs(5) && !i.state.slow_stick {
                i.last_speed_check = Instant::now();
                let stats = mpv.stream_stats();
                let enough = stats.bytes > 16 << 20 && stats.read_time > Duration::from_millis(200);
                let rate = stats.read_mib_per_s() * 1024.0 * 1024.0;
                let slow = enough && i.bytes_per_s.is_some_and(|need| rate < need * 1.1);
                if slow {
                    i.state.slow_stick = true;
                }
                slow
            } else {
                false
            }
        };
        if slow_now {
            let _ = app.emit("read-speed-warning", serde_json::json!({}));
            emit_now = true;
        }

        let snapshot = {
            let mut i = inner.lock().unwrap();
            if emit_now || i.last_emit.elapsed() >= Duration::from_millis(250) {
                i.last_emit = Instant::now();
                Some(i.state.clone())
            } else {
                None
            }
        };
        if let Some(s) = snapshot {
            let _ = app.emit("player-state", s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_keys() {
        assert_eq!(lang_key("heb"), "he");
        assert_eq!(lang_key("iw"), "he");
        assert_eq!(lang_key("en-US"), "en");
        assert_eq!(lang_key("ARA"), "ar");
        assert_eq!(lang_key("he"), "he");
    }

    #[test]
    fn player_commands_parse() {
        let c: PlayerCommand = serde_json::from_str(r#"{"type":"seek","seconds":-10,"relative":true}"#).unwrap();
        assert!(matches!(c, PlayerCommand::Seek { relative: true, .. }));
        let c: PlayerCommand = serde_json::from_str(r#"{"type":"setSub","id":null}"#).unwrap();
        assert!(matches!(c, PlayerCommand::SetSub { id: None }));
        let c: PlayerCommand = serde_json::from_str(r#"{"type":"seek","seconds":5}"#).unwrap();
        assert!(matches!(c, PlayerCommand::Seek { relative: false, .. }));
    }
}
