//! `cinasafe` CLI for testing and scripting (brief 3.1, M1): vault commands,
//! `scan`/`identify` previews of an import, and the Milestone 0 `spike-*`
//! commands (plaintext-key test vault, kept for the playback benchmarks).

mod import_cmd;
mod library;
mod vault_cmds;

use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cinasafe_core::library::Library;
use cinasafe_core::spike::SpikeVault;
use cinasafe_core::{BlobId, BlobWriteOptions, ChunkSource};
use cinasafe_player::{Event, MediaStream, Mpv, StreamOpener, VaultOpener};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cinasafe", version, about = "Cinasafe command-line tool")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
#[allow(clippy::enum_variant_names)]
enum Cmd {
    /// Create a vault in <root>/.cinasafe (asks for a password).
    Create {
        root: PathBuf,
        /// Don't create a recovery key.
        #[arg(long)]
        no_recovery: bool,
    },
    /// Check the password and show a summary.
    Unlock { root: PathBuf },
    /// Encrypt files into the vault (verified after writing).
    Add {
        root: PathBuf,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// List the files in the vault.
    Ls { root: PathBuf },
    /// Write a decrypted file to standard output (`id` from `ls`, or a blob id).
    Cat { root: PathBuf, id: String },
    /// Write a decrypted copy to `dest` (a file or a folder).
    Export {
        root: PathBuf,
        id: String,
        dest: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Change the password (no media is re-encrypted).
    Passwd { root: PathBuf },
    /// Replace the recovery key and print the new words, or remove it.
    Recovery {
        root: PathBuf,
        #[arg(long)]
        remove: bool,
    },
    /// Decrypt everything and report corrupt, missing or orphaned files.
    Verify { root: PathBuf },
    /// Import files or folders: recognise, encrypt, verify and file them into the library.
    Import {
        root: PathBuf,
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Look titles up online (sends parsed titles to the provider).
        #[arg(long)]
        online: bool,
        #[arg(long, value_enum, default_value_t = library::ProviderChoice::Auto)]
        provider: library::ProviderChoice,
        /// TMDB v3 API key or v4 read access token. Prefer the environment variable.
        #[arg(long, env = "CINASAFE_TMDB_KEY", hide_env_values = true)]
        tmdb_key: Option<String>,
        #[arg(long, default_value = "en-US")]
        language: String,
        /// Don't read duration and tracks with libmpv.
        #[arg(long)]
        no_probe: bool,
    },
    /// Play a file from the vault headless through libmpv (testing): its imported subtitle
    /// is added, the resume position is saved afterwards.
    Play {
        root: PathBuf,
        /// Media file id from `ls`.
        id: i64,
        #[arg(long, default_value_t = 10.0)]
        seconds: f64,
        #[arg(long, default_value_t = 3)]
        seeks: u32,
        #[arg(long, default_value = "null")]
        vo: String,
    },
    /// Show what importing a folder would recognise: movies, shows, episodes and subtitles. Offline.
    Scan {
        dir: PathBuf,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Like scan, then look titles up online and show the metadata (and artwork) that would be attached.
    Identify {
        dir: PathBuf,
        #[arg(long, value_enum, default_value_t = library::ProviderChoice::Auto)]
        provider: library::ProviderChoice,
        /// TMDB v3 API key or v4 read access token. Prefer the environment variable.
        #[arg(long, env = "CINASAFE_TMDB_KEY", hide_env_values = true)]
        tmdb_key: Option<String>,
        /// Metadata language (TMDB), e.g. he-IL, ar-SA, en-US.
        #[arg(long, default_value = "en-US")]
        language: String,
        /// Also download posters, backdrops and episode stills (into memory) to check they exist.
        #[arg(long)]
        artwork: bool,
        #[arg(long)]
        json: bool,
    },
    /// Create a spike test vault with a PLAINTEXT key file under <root>/.cinasafe.
    SpikeInit { root: PathBuf },
    /// Encrypt a file into the spike vault and print its blob id.
    SpikeImport {
        root: PathBuf,
        file: PathBuf,
        /// Don't pad to 1 MiB (padding is for media blobs).
        #[arg(long)]
        no_pad: bool,
    },
    /// Measure BlobReader sequential throughput and random-read latency.
    SpikeBench {
        root: PathBuf,
        blob: String,
        #[arg(long, default_value_t = 200)]
        random_reads: u32,
    },
    /// Play a blob through libmpv and the cinasafe:// bridge, headless, and report timings.
    SpikePlay {
        root: PathBuf,
        blob: String,
        /// External subtitle blob to add with sub-add.
        #[arg(long)]
        sub: Option<String>,
        #[arg(long, default_value_t = 5.0)]
        seconds: f64,
        #[arg(long, default_value_t = 10)]
        seeks: u32,
        #[arg(long, default_value = "null")]
        vo: String,
        #[arg(long)]
        hwdec: Option<String>,
        /// Directory of bundled subtitle fallback fonts.
        #[arg(long)]
        fonts_dir: Option<PathBuf>,
        /// Simulate a slow stick: cap reads at this many MiB/s.
        #[arg(long)]
        throttle_mibps: Option<f64>,
        /// Extra per-read latency in ms when throttling (USB round trip).
        #[arg(long, default_value_t = 1.0)]
        throttle_latency_ms: f64,
        /// Extra mpv option, `name=value`; repeatable.
        #[arg(short = 'o', long = "mpv-opt")]
        mpv_opts: Vec<String>,
    },
}

#[derive(Clone, Copy)]
struct Throttle {
    bytes_per_s: f64,
    latency: Duration,
}

/// A file that reads no faster than a given USB stick would.
struct ThrottledFile {
    file: std::fs::File,
    throttle: Throttle,
}

impl ChunkSource for ThrottledFile {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let t = Instant::now();
        self.file.read_exact_at(buf, offset)?;
        let budget = self.throttle.latency + Duration::from_secs_f64(buf.len() as f64 / self.throttle.bytes_per_s);
        if let Some(rest) = budget.checked_sub(t.elapsed()) {
            std::thread::sleep(rest);
        }
        Ok(())
    }
}

struct Opener {
    vault: SpikeVault,
    throttle: Option<Throttle>,
}

impl StreamOpener for Opener {
    fn open(&self, id: BlobId) -> io::Result<Box<dyn MediaStream>> {
        let r = match self.throttle {
            None => self.vault.open_blob(id),
            Some(throttle) => {
                let file = std::fs::File::open(self.vault.blob_path(id))?;
                self.vault.open_blob_from(id, ThrottledFile { file, throttle })
            }
        };
        Ok(Box::new(r.map_err(io::Error::other)?))
    }
}

fn parse_id(s: &str) -> Result<BlobId, String> {
    BlobId::from_hex(s).ok_or_else(|| format!("not a blob id: {s}"))
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Small deterministic PRNG so runs are reproducible.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn unit(&mut self) -> f64 {
        (self.next() % 1_000_000) as f64 / 1_000_000.0
    }
}

fn summary(mut v: Vec<Duration>) -> String {
    if v.is_empty() {
        return "n/a".into();
    }
    v.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    format!(
        "min {:.1} ms, median {:.1} ms, p90 {:.1} ms, max {:.1} ms (n={})",
        ms(v[0]),
        ms(v[v.len() / 2]),
        ms(v[(v.len() * 9 / 10).min(v.len() - 1)]),
        ms(v[v.len() - 1]),
        v.len()
    )
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.cmd {
        Cmd::Create { root, no_recovery } => vault_cmds::create(&root, !no_recovery)?,
        Cmd::Unlock { root } => vault_cmds::unlock_check(&root)?,
        Cmd::Add { root, files } => vault_cmds::add(&root, &files)?,
        Cmd::Ls { root } => vault_cmds::ls(&root)?,
        Cmd::Cat { root, id } => vault_cmds::cat(&root, &id)?,
        Cmd::Export { root, id, dest, force } => vault_cmds::export(&root, &id, &dest, force)?,
        Cmd::Passwd { root } => vault_cmds::passwd(&root)?,
        Cmd::Recovery { root, remove } => vault_cmds::recovery(&root, remove)?,
        Cmd::Verify { root } => {
            if !vault_cmds::verify(&root)? {
                return Err("integrity check found problems".into());
            }
        }
        Cmd::Import { root, paths, online, provider, tmdb_key, language, no_probe } => {
            let args = import_cmd::ImportArgs { root, paths, online, provider, tmdb_key, language, probe: !no_probe };
            if !import_cmd::import(args)? {
                return Err("some files failed to import".into());
            }
        }
        Cmd::Scan { dir, json } => library::scan(&dir, json)?,
        Cmd::Identify { dir, provider, tmdb_key, language, artwork, json } => {
            library::identify(library::IdentifyArgs { dir: &dir, provider, tmdb_key, language, artwork, json })?
        }
        Cmd::SpikeInit { root } => {
            SpikeVault::init(&root).map_err(|e| e.to_string())?;
            println!("spike vault created at {}", root.join(".cinasafe").display());
        }
        Cmd::SpikeImport { root, file, no_pad } => {
            let v = SpikeVault::open(&root).map_err(|e| e.to_string())?;
            let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
            let opts = if no_pad { BlobWriteOptions::default() } else { BlobWriteOptions::media() };
            let t = Instant::now();
            let id = v.import(&file, &opts).map_err(|e| e.to_string())?;
            let dt = t.elapsed();
            eprintln!(
                "encrypted {:.1} MiB in {:.2} s ({:.0} MiB/s incl. fsync)",
                mib(size),
                dt.as_secs_f64(),
                mib(size) / dt.as_secs_f64()
            );
            println!("{}", id.to_hex());
        }
        Cmd::SpikeBench { root, blob, random_reads } => {
            let v = SpikeVault::open(&root).map_err(|e| e.to_string())?;
            let id = parse_id(&blob)?;
            let mut r = v.open_blob(id).map_err(|e| e.to_string())?;
            let size = r.len();
            let mut buf = vec![0u8; 256 * 1024];
            let t = Instant::now();
            let mut total = 0u64;
            loop {
                let n = r.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                total += n as u64;
            }
            let dt = t.elapsed();
            println!(
                "sequential: {:.1} MiB in {:.2} s = {:.0} MiB/s ({:?})",
                mib(total),
                dt.as_secs_f64(),
                mib(total) / dt.as_secs_f64(),
                r.stats()
            );
            assert_eq!(total, size);

            let mut rng = Lcg(42);
            let mut lat = Vec::new();
            let mut small = vec![0u8; 64 * 1024];
            for _ in 0..random_reads {
                let pos = rng.next() % size.max(1);
                let t = Instant::now();
                r.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
                r.read(&mut small).map_err(|e| e.to_string())?;
                lat.push(t.elapsed());
            }
            println!("random 64 KiB read after seek: {}", summary(lat));
        }
        Cmd::Play { root, id, seconds, seeks, vo } => {
            let vault = Arc::new(vault_cmds::unlock(&root)?);
            let (blob, sub) = {
                let lib = vault.library();
                let f = lib.media_file(id).map_err(|_| format!("no file with id {id}"))?;
                let sub = lib.subtitles(id).map_err(|e| e.to_string())?.first().map(Library::subtitle_blob_to_play);
                (f.blob_id, sub)
            };
            let mut mpv = Mpv::new().map_err(|e| e.to_string())?;
            mpv.set_option("vo", &vo).map_err(|e| e.to_string())?;
            mpv.register_stream_protocol(Arc::new(VaultOpener(vault.clone()))).map_err(|e| e.to_string())?;
            let position = play(mpv, blob, sub, seconds, seeks).map_err(|e| e.to_string())?;
            vault.library().record_position(id, (position * 1000.0) as i64, None).map_err(|e| e.to_string())?;
            vault.save_library().map_err(|e| e.to_string())?;
        }
        Cmd::SpikePlay {
            root,
            blob,
            sub,
            seconds,
            seeks,
            vo,
            hwdec,
            fonts_dir,
            throttle_mibps,
            throttle_latency_ms,
            mpv_opts,
        } => {
            let vault = SpikeVault::open(&root).map_err(|e| e.to_string())?;
            let id = parse_id(&blob)?;
            let sub = sub.as_deref().map(parse_id).transpose()?;
            let throttle = throttle_mibps.map(|r| Throttle {
                bytes_per_s: r * 1024.0 * 1024.0,
                latency: Duration::from_secs_f64(throttle_latency_ms / 1000.0),
            });
            let mut mpv = Mpv::new().map_err(|e| e.to_string())?;
            mpv.set_option("vo", &vo).map_err(|e| e.to_string())?;
            if let Some(h) = hwdec {
                mpv.set_option("hwdec", &h).map_err(|e| e.to_string())?;
            }
            if let Some(dir) = fonts_dir {
                mpv.set_option("sub-fonts-dir", &dir.to_string_lossy()).map_err(|e| e.to_string())?;
            }
            for o in &mpv_opts {
                let (k, v) = o.split_once('=').ok_or_else(|| format!("expected name=value: {o}"))?;
                mpv.set_option(k, v).map_err(|e| e.to_string())?;
            }
            mpv.register_stream_protocol(Arc::new(Opener { vault, throttle })).map_err(|e| e.to_string())?;
            play(mpv, id, sub, seconds, seeks).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Plays headless and reports timings; returns the position when it stopped.
fn play(
    mpv: Mpv,
    id: BlobId,
    sub: Option<BlobId>,
    seconds: f64,
    seeks: u32,
) -> Result<f64, Box<dyn std::error::Error>> {
    mpv.set_option("ao", "null")?;
    mpv.set_option("idle", "yes")?;
    mpv.set_option("keep-open", "always")?;
    mpv.initialize()?;

    let wait_for = |want: &dyn Fn(&Event) -> bool, timeout: Duration| -> Result<Duration, String> {
        let t = Instant::now();
        while t.elapsed() < timeout {
            let ev = mpv.wait_event(Duration::from_millis(100));
            if let Event::EndFile { reason, error } = ev {
                if error < 0 {
                    return Err(format!("playback ended: reason {reason}, error {error}"));
                }
            }
            if want(&ev) {
                return Ok(t.elapsed());
            }
        }
        Err("timed out".into())
    };

    let t0 = Instant::now();
    mpv.command(&["loadfile", &format!("cinasafe://{}", id.to_hex())])?;
    wait_for(&|e| matches!(e, Event::FileLoaded), Duration::from_secs(20))?;
    let loaded = t0.elapsed();
    wait_for(&|e| matches!(e, Event::PlaybackRestart), Duration::from_secs(20))?;
    let first_frame = t0.elapsed();
    println!("open -> file-loaded: {:.1} ms", loaded.as_secs_f64() * 1000.0);
    println!("open -> playing:     {:.1} ms", first_frame.as_secs_f64() * 1000.0);

    if let Some(s) = sub {
        mpv.command(&["sub-add", &format!("cinasafe://{}", s.to_hex()), "select"])?;
    }

    let duration = mpv.get_f64("duration").unwrap_or(0.0);
    println!("duration: {duration:.1} s, file-format: {}", mpv.get_string("file-format").unwrap_or_default());
    println!(
        "video: {} {}x{}, hwdec-current: {}",
        mpv.get_string("video-codec").unwrap_or_default(),
        mpv.get_i64("width").unwrap_or(0),
        mpv.get_i64("height").unwrap_or(0),
        mpv.get_string("hwdec-current").unwrap_or_else(|_| "none".into()),
    );

    let play_until = Instant::now() + Duration::from_secs_f64(seconds);
    let mut sub_texts = Vec::new();
    while Instant::now() < play_until {
        mpv.wait_event(Duration::from_millis(200));
        if let Ok(t) = mpv.get_string("sub-text") {
            if !t.is_empty() && sub_texts.last() != Some(&t) {
                sub_texts.push(t);
            }
        }
    }
    println!("position after {seconds} s: {:.2} s", mpv.get_f64("time-pos").unwrap_or(-1.0));

    let mut rng = Lcg(7);
    let mut lat = Vec::new();
    for _ in 0..seeks {
        let target = rng.unit() * (duration - 5.0).max(0.0);
        let t = Instant::now();
        mpv.command(&["seek", &format!("{target:.3}"), "absolute"])?;
        wait_for(&|e| matches!(e, Event::PlaybackRestart), Duration::from_secs(20))?;
        lat.push(t.elapsed());
        if let Ok(t) = mpv.get_string("sub-text") {
            if !t.is_empty() && sub_texts.last() != Some(&t) {
                sub_texts.push(t);
            }
        }
    }
    println!("seek -> playback-restart: {}", summary(lat));

    let s = mpv.stream_stats();
    println!(
        "bridge: {} opens, {} reads, {:.1} MiB, {} seeks, read speed {:.0} MiB/s, slowest read {:.1} ms",
        s.opens,
        s.reads,
        mib(s.bytes),
        s.seeks,
        s.read_mib_per_s(),
        s.max_read.as_secs_f64() * 1000.0
    );
    println!(
        "dropped frames: decoder {}, vo {}",
        mpv.get_i64("decoder-frame-drop-count").unwrap_or(-1),
        mpv.get_i64("frame-drop-count").unwrap_or(-1)
    );
    if let Ok(tracks) = mpv.get_string("track-list") {
        println!("track-list: {tracks}");
    }
    for t in &sub_texts {
        println!("subtitle seen: {t}");
    }
    Ok(mpv.get_f64("time-pos").unwrap_or(0.0))
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
