//! Vault commands (brief M1): create, unlock, add, ls, cat, export, passwd,
//! recovery, verify.
//!
//! Passwords are read from the terminal without echo. For scripts and tests,
//! `CINASAFE_PASSWORD` (and `CINASAFE_NEW_PASSWORD` for `passwd`) are used
//! instead when set; environment variables can leak, so don't use them for
//! real vaults.

use std::fs::File;
use std::io::{self, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};

use cinasafe_core::library::{MediaFile, NewMediaFile};
use cinasafe_core::vault::WritePhase;
use cinasafe_core::{BlobId, BlobWriteOptions, Error, Password, RecoveryWords, Vault};
use zeroize::Zeroizing;

fn read_password(prompt: &str, env: &str) -> Result<Password, String> {
    if let Ok(p) = std::env::var(env) {
        return Ok(Zeroizing::new(p));
    }
    rpassword::prompt_password(prompt).map(Zeroizing::new).map_err(|e| format!("cannot read password: {e}"))
}

fn new_password(env: &str) -> Result<Password, String> {
    if let Ok(p) = std::env::var(env) {
        return Ok(Zeroizing::new(p));
    }
    let a = read_password("New password: ", "\u{0}")?;
    let b = read_password("Repeat it: ", "\u{0}")?;
    if *a != *b {
        return Err("passwords don't match".into());
    }
    if a.chars().count() < 8 {
        return Err("use at least 8 characters".into());
    }
    Ok(a)
}

fn explain(e: Error) -> String {
    match e {
        Error::WrongPassword => "wrong password".into(),
        Error::NotAVault => "there is no Cinasafe vault there (looked for .cinasafe/vault.json)".into(),
        Error::VaultTooNew(_) => "this vault was made by a newer Cinasafe; update Cinasafe to open it".into(),
        other => other.to_string(),
    }
}

pub fn unlock(root: &Path) -> Result<Vault, String> {
    let pw = read_password("Password: ", "CINASAFE_PASSWORD")?;
    let v = Vault::unlock_with_password(root, &pw).map_err(explain)?;
    if v.library_restored_from_backup() {
        eprintln!("warning: the library file was damaged; opened the previous copy (library.db.enc.bak)");
    }
    Ok(v)
}

fn human(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1000.0 && u < units.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", units[u])
    }
}

pub fn create(root: &Path, recovery: bool) -> Result<(), String> {
    let pw = new_password("CINASAFE_PASSWORD")?;
    eprintln!("Creating vault (deriving the key takes about a second)...");
    let (_vault, words) = Vault::create(root, &pw, recovery).map_err(explain)?;
    println!("Vault created at {}", Vault::vault_dir(root).display());
    if let Some(w) = words {
        println!();
        println!("Recovery key. Write these 24 words down and keep them somewhere safe.");
        println!("They are shown only now. With them, the vault opens if you forget the password.");
        println!();
        for (i, word) in w.words().iter().enumerate() {
            print!("{:>2}. {:<10}", i + 1, word);
            if i % 4 == 3 {
                println!();
            }
        }
    }
    Ok(())
}

pub fn unlock_check(root: &Path) -> Result<(), String> {
    let v = unlock(root)?;
    let files = v.library().media_files().map_err(|e| e.to_string())?;
    println!(
        "Unlocked. {} files, {} on the stick, created {}{}.",
        files.len(),
        human(v.used_bytes().map_err(|e| e.to_string())?),
        v.created_at(),
        if v.has_recovery() { ", recovery key set" } else { ", no recovery key" }
    );
    Ok(())
}

pub fn add(root: &Path, files: &[PathBuf]) -> Result<(), String> {
    let v = unlock(root)?;
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let size = f.metadata().map_err(|e| e.to_string())?.len();
        let tty = io::stderr().is_terminal();
        let id = v
            .write_blob_with(BufReader::with_capacity(4 << 20, f), size, &BlobWriteOptions::media(), &mut |phase, n| {
                if tty {
                    let what = match phase {
                        WritePhase::Encrypting => "encrypting",
                        WritePhase::Verifying => "verifying",
                    };
                    eprint!("\r{name}: {what} {:>3}%", n * 100 / size.max(1));
                }
                true
            })
            .map_err(|e| format!("{}: {}", path.display(), explain(e)))?;
        if tty {
            eprintln!();
        }
        let mf = v
            .library()
            .add_media_file(id, &NewMediaFile { original_name: name.clone(), size_bytes: size, ..Default::default() })
            .map_err(|e| e.to_string())?;
        v.save_library().map_err(explain)?;
        println!("{mf}\t{}\t{name}", id.to_hex());
    }
    Ok(())
}

fn describe(v: &Vault, f: &MediaFile) -> String {
    let lib = v.library();
    if let Ok(Some(m)) = lib.movie_by_media(f.id) {
        return format!("{}{}", m.title, m.year.map(|y| format!(" ({y})")).unwrap_or_default());
    }
    if let Ok(eps) = lib.episodes_by_media(f.id) {
        if let Some(e) = eps.first() {
            let show = lib.show(e.show_id).map(|s| s.title).unwrap_or_default();
            return format!("{show} S{:02}E{:02}", e.season_number, e.number);
        }
    }
    String::new()
}

pub fn ls(root: &Path) -> Result<(), String> {
    let v = unlock(root)?;
    let files = v.library().media_files().map_err(|e| e.to_string())?;
    println!("{:>4}  {:<32}  {:>9}  {:<40}  title", "id", "blob", "size", "original name");
    for f in &files {
        println!(
            "{:>4}  {}  {:>9}  {:<40}  {}",
            f.id,
            f.blob_id.to_hex(),
            human(f.size_bytes),
            f.original_name,
            describe(&v, f)
        );
    }
    eprintln!("{} files", files.len());
    Ok(())
}

/// `id` is a media file id from `ls`, or a blob id (32 hex characters).
fn resolve(v: &Vault, id: &str) -> Result<(BlobId, String), String> {
    if let Some(b) = BlobId::from_hex(id) {
        return Ok((b, b.to_hex()));
    }
    let n: i64 = id.parse().map_err(|_| format!("not a file id or blob id: {id}"))?;
    let f = v.library().media_file(n).map_err(|_| format!("no file with id {n}"))?;
    Ok((f.blob_id, f.original_name))
}

pub fn cat(root: &Path, id: &str) -> Result<(), String> {
    let v = unlock(root)?;
    let (blob, _) = resolve(&v, id)?;
    let mut r = v.open_blob(blob).map_err(explain)?;
    let mut out = io::stdout().lock();
    io::copy(&mut r, &mut out).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())
}

pub fn export(root: &Path, id: &str, dest: &Path, force: bool) -> Result<(), String> {
    let v = unlock(root)?;
    let (blob, name) = resolve(&v, id)?;
    let dest = if dest.is_dir() { dest.join(&name) } else { dest.to_path_buf() };
    if dest.exists() && !force {
        return Err(format!("{} exists (use --force to overwrite)", dest.display()));
    }
    let mut r = v.open_blob(blob).map_err(explain)?;
    let mut f = File::create(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let n = io::copy(&mut r, &mut f).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    println!("exported {} to {} (decrypted, {})", name, dest.display(), human(n));
    Ok(())
}

pub fn passwd(root: &Path) -> Result<(), String> {
    let v = unlock(root)?;
    let new = new_password("CINASAFE_NEW_PASSWORD")?;
    v.change_password(&new).map_err(explain)?;
    println!("Password changed. Your recovery key, if any, still works.");
    Ok(())
}

pub fn recovery(root: &Path, remove: bool) -> Result<(), String> {
    let v = unlock(root)?;
    if remove {
        v.remove_recovery().map_err(explain)?;
        println!("Recovery key removed. Only the password opens this vault now.");
        return Ok(());
    }
    let w: RecoveryWords = v.replace_recovery().map_err(explain)?;
    println!("New recovery key (the old one no longer works):");
    println!();
    for (i, word) in w.words().iter().enumerate() {
        print!("{:>2}. {:<10}", i + 1, word);
        if i % 4 == 3 {
            println!();
        }
    }
    Ok(())
}

pub fn verify(root: &Path) -> Result<bool, String> {
    let v = unlock(root)?;
    let tty = io::stderr().is_terminal();
    let report = v
        .integrity_check(|p| {
            if tty && p.total > 0 {
                eprint!("\rchecking {}/{}", p.done, p.total);
            }
        })
        .map_err(explain)?;
    if tty {
        eprintln!();
    }
    println!("checked {} blobs ({} decrypted)", report.checked, human(report.bytes));
    for (id, why) in &report.corrupt {
        println!("CORRUPT  {}  {why}", id.to_hex());
    }
    for id in &report.missing {
        println!("MISSING  {}", id.to_hex());
    }
    for id in &report.orphaned {
        println!("ORPHAN   {}  (not in the library; left by an interrupted import?)", id.to_hex());
    }
    if report.library_restored_from_backup {
        println!("NOTE     the library was restored from its backup copy");
    }
    println!("{}", if report.is_clean() { "OK" } else { "PROBLEMS FOUND" });
    Ok(report.is_clean())
}
