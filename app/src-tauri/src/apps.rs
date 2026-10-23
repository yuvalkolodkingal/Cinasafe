//! The player apps on the stick (brief 4.1): copying the running app there
//! ("Update player apps on this stick"), plus `run-linux.sh` and `README.txt`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub const README: &str = include_str!("../stick/README.txt");
pub const RUN_LINUX: &str = include_str!("../stick/run-linux.sh");

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerApp {
    pub platform: String,
    pub file: String,
    pub version: Option<String>,
}

pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-arm64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "other",
    }
}

/// The file name each platform's app has at the stick's root.
fn file_for(platform: &str) -> Option<&'static str> {
    Some(match platform {
        "windows-x86_64" => "Cinasafe-Windows.exe",
        "windows-arm64" => "Cinasafe-Windows-arm64.exe",
        "linux-x86_64" => "Cinasafe-Linux-x86_64.AppImage",
        "linux-aarch64" => "Cinasafe-Linux-aarch64.AppImage",
        _ => return None,
    })
}

const PLATFORMS: [&str; 4] = ["windows-x86_64", "windows-arm64", "linux-x86_64", "linux-aarch64"];

fn versions_file(root: &Path) -> PathBuf {
    root.join(".cinasafe").join("apps.json")
}

fn read_versions(root: &Path) -> serde_json::Map<String, serde_json::Value> {
    fs::read(versions_file(root)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Which player apps are on the stick.
pub fn list(root: &Path) -> Vec<PlayerApp> {
    let versions = read_versions(root);
    PLATFORMS
        .iter()
        .filter_map(|p| {
            let file = file_for(p)?;
            root.join(file).is_file().then(|| PlayerApp {
                platform: (*p).to_owned(),
                file: file.to_owned(),
                version: versions.get(*p).and_then(|v| v.as_str()).map(str::to_owned),
            })
        })
        .collect()
}

/// Copies via a temporary name, so a pulled stick never holds half an app.
fn copy_atomic(src: &Path, dest: &Path) -> io::Result<()> {
    let tmp = dest.with_extension("partial");
    fs::copy(src, &tmp)?;
    fs::File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, dest)
}

/// The app file that is running now: the AppImage itself on Linux, the exe on Windows.
fn running_app() -> io::Result<PathBuf> {
    if let Some(a) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(a));
    }
    if cfg!(windows) {
        return std::env::current_exe();
    }
    Err(io::Error::new(io::ErrorKind::Unsupported, "only the AppImage can be copied to a stick"))
}

/// Puts this platform's app, `run-linux.sh` and `README.txt` on the stick.
pub fn copy_player_apps(root: &Path) -> io::Result<()> {
    fs::write(root.join("README.txt"), README)?;
    fs::write(root.join("run-linux.sh"), RUN_LINUX)?;
    let plat = platform();
    let Some(name) = file_for(plat) else { return Ok(()) };
    let src = running_app()?;
    let dest = root.join(name);
    if src != dest {
        copy_atomic(&src, &dest)?;
    }
    // Windows: libmpv and the DLLs it needs live in `mpv/` next to the exe
    // (portable zip, installer) and in `.cinasafe/runtime/<platform>/` on a
    // stick, which keeps the stick's root tidy. The AppImage carries its own.
    if cfg!(windows) {
        let runtime = src.parent().map(|d| d.join("mpv")).filter(|d| d.is_dir());
        if let Some(from) = runtime {
            let to = root.join(".cinasafe").join("runtime").join(cinasafe_player::ffi::platform_dir());
            fs::create_dir_all(&to)?;
            for e in fs::read_dir(&from)? {
                let e = e?;
                if e.file_type()?.is_file() {
                    copy_atomic(&e.path(), &to.join(e.file_name()))?;
                }
            }
        }
    }
    let mut versions = read_versions(root);
    versions.insert(plat.to_owned(), env!("CARGO_PKG_VERSION").into());
    fs::write(versions_file(root), serde_json::to_vec_pretty(&versions).map_err(io::Error::other)?)?;
    Ok(())
}

pub fn eject(root: &Path) -> Result<(), crate::error::AppError> {
    Ok(cinasafe_core::drives::eject(root)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_readme_and_script() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join(".cinasafe")).unwrap();
        // Not an AppImage in tests: the text files still get written.
        let _ = copy_player_apps(d.path());
        assert!(fs::read_to_string(d.path().join("README.txt")).unwrap().contains("Cinasafe"));
        assert!(fs::read_to_string(d.path().join("run-linux.sh")).unwrap().starts_with("#!/bin/sh"));
        assert!(list(d.path()).is_empty());
        fs::write(d.path().join("Cinasafe-Windows.exe"), b"MZ").unwrap();
        assert_eq!(list(d.path())[0].platform, "windows-x86_64");
    }
}
