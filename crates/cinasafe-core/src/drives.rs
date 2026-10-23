//! Removable drives (brief 5.3–5.4): listing, free space, removal watching
//! and formatting to exFAT.
//!
//! - Windows: `GetLogicalDrives` + `GetDriveTypeW == DRIVE_REMOVABLE` +
//!   `GetVolumeInformationW`; formatting runs `format` elevated (UAC prompt).
//! - Linux: UDisks2 over D-Bus, filtering `Removable` drives that aren't
//!   `HintSystem`; formatting goes through UDisks2 (polkit prompt), falling
//!   back to `pkexec mkfs.exfat`.
//!
//! Removal is detected by polling the vault folder every 250 ms, which works
//! the same everywhere and meets the "lock within 1 s" requirement.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum DriveError {
    #[error("drive listing is not available: {0}")]
    Unavailable(String),
    #[error("formatting failed: {0}")]
    FormatFailed(String),
    #[error("formatting was cancelled")]
    FormatCancelled,
    #[error("invalid volume label (use 1-11 letters, digits, - or _)")]
    BadLabel,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DriveStatus {
    /// exFAT and mounted: can hold a vault as is.
    Ready,
    /// Another file system (FAT32, NTFS, ...) or none: must be formatted first.
    NeedsFormatting,
    /// No mounted file system we can see.
    NotMounted,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    /// Stable id for this session: device path on Linux, `E:` on Windows.
    pub id: String,
    /// What to show: label, else vendor and model.
    pub name: String,
    pub label: Option<String>,
    pub size_bytes: u64,
    pub free_bytes: Option<u64>,
    /// `exfat`, `vfat`, `ntfs`, ... in lowercase.
    pub filesystem: Option<String>,
    pub mount_path: Option<PathBuf>,
    pub has_vault: bool,
    pub status: DriveStatus,
}

pub fn status_for(filesystem: Option<&str>, mounted: bool) -> DriveStatus {
    match (filesystem.map(|f| f.to_ascii_lowercase()), mounted) {
        (Some(f), true) if f == "exfat" => DriveStatus::Ready,
        (Some(_), true) => DriveStatus::NeedsFormatting,
        (Some(_), false) => DriveStatus::NotMounted,
        (None, _) => DriveStatus::NeedsFormatting,
    }
}

/// exFAT labels: up to 11 characters; kept to a portable, shell-safe set.
pub fn valid_label(label: &str) -> bool {
    (1..=11).contains(&label.chars().count())
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn with_vault_flag(mut d: Drive) -> Drive {
    d.has_vault = d.mount_path.as_deref().is_some_and(|p| p.join(".cinasafe").join("vault.json").exists());
    d
}

/// Free bytes on the file system holding `path`.
pub fn free_space(path: &Path) -> std::io::Result<u64> {
    imp::free_space(path)
}

/// Removable drives, mounted or not.
pub fn list_removable() -> Result<Vec<Drive>, DriveError> {
    let mut drives: Vec<Drive> = imp::list()?.into_iter().map(with_vault_flag).collect();
    drives.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(drives)
}

/// Formats `drive` as exFAT with `label` and returns where it is mounted.
/// Asks the OS for administrator rights (UAC / polkit). The caller must have
/// made the user type the drive's name to confirm (brief 5.4).
pub fn format_exfat(drive: &Drive, label: &str) -> Result<PathBuf, DriveError> {
    if !valid_label(label) {
        return Err(DriveError::BadLabel);
    }
    imp::format_exfat(drive, label)
}

/// Unmounts the volume mounted at `mount` and powers its drive off, so it
/// can be pulled safely. Linux only; on Windows the vault's writes are
/// already flushed (every save is fsynced) and this does nothing.
pub fn eject(mount: &Path) -> Result<(), DriveError> {
    imp::eject(mount)
}

/// Calls `on_removed` once when `vault_root/.cinasafe` stops being reachable.
pub struct RemovalWatch {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RemovalWatch {
    pub const INTERVAL: Duration = Duration::from_millis(250);

    pub fn start(vault_root: &Path, on_removed: impl FnOnce() + Send + 'static) -> RemovalWatch {
        let stop = Arc::new(AtomicBool::new(false));
        let marker = vault_root.join(".cinasafe").join("vault.json");
        let s = stop.clone();
        let handle = std::thread::Builder::new()
            .name("drive-watch".into())
            .spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    if std::fs::metadata(&marker).is_err() {
                        on_removed();
                        return;
                    }
                    std::thread::sleep(Self::INTERVAL);
                }
            })
            .expect("spawning a thread");
        RemovalWatch { stop, handle: Some(handle) }
    }
}

impl Drop for RemovalWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            // Don't join from inside the callback's own thread.
            if h.thread().id() != std::thread::current().id() {
                let _ = h.join();
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    use super::*;

    const DEST: &str = "org.freedesktop.UDisks2";
    type Props = HashMap<String, OwnedValue>;
    type Objects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

    pub fn free_space(path: &Path) -> std::io::Result<u64> {
        let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| std::io::ErrorKind::InvalidInput)?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: valid C string and out-pointer.
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(s.f_bavail as u64 * s.f_frsize as u64)
    }

    fn connect() -> Result<Connection, DriveError> {
        Connection::system().map_err(|e| DriveError::Unavailable(e.to_string()))
    }

    fn objects(conn: &Connection) -> Result<Objects, DriveError> {
        let p = Proxy::new(conn, DEST, "/org/freedesktop/UDisks2", "org.freedesktop.DBus.ObjectManager")
            .map_err(|e| DriveError::Unavailable(e.to_string()))?;
        let raw: HashMap<OwnedObjectPath, HashMap<zbus::names::OwnedInterfaceName, Props>> =
            p.call("GetManagedObjects", &()).map_err(|e| DriveError::Unavailable(e.to_string()))?;
        Ok(raw.into_iter().map(|(k, v)| (k, v.into_iter().map(|(i, p)| (i.to_string(), p)).collect())).collect())
    }

    fn bytes(v: &Value<'_>) -> Option<Vec<u8>> {
        match v {
            Value::Array(a) => a.iter().map(|x| if let Value::U8(b) = x { Some(*b) } else { None }).collect(),
            _ => None,
        }
    }

    /// `ay` strings are NUL-terminated.
    fn byte_string(v: &Value<'_>) -> Option<String> {
        let mut b = bytes(v)?;
        while b.last() == Some(&0) {
            b.pop();
        }
        Some(String::from_utf8_lossy(&b).into_owned())
    }

    fn get<'a>(p: &'a Props, k: &str) -> Option<&'a Value<'static>> {
        p.get(k).map(|v| &**v)
    }

    fn string(p: &Props, k: &str) -> Option<String> {
        match get(p, k)? {
            Value::Str(s) => Some(s.to_string()).filter(|s| !s.is_empty()),
            _ => None,
        }
    }

    fn boolean(p: &Props, k: &str) -> bool {
        matches!(get(p, k), Some(Value::Bool(true)))
    }

    fn u64_of(p: &Props, k: &str) -> u64 {
        match get(p, k) {
            Some(Value::U64(n)) => *n,
            _ => 0,
        }
    }

    fn object_path(p: &Props, k: &str) -> Option<String> {
        match get(p, k)? {
            Value::ObjectPath(o) => Some(o.to_string()).filter(|s| s != "/"),
            _ => None,
        }
    }

    fn mount_points(p: &Props) -> Vec<PathBuf> {
        match get(p, "MountPoints") {
            Some(Value::Array(a)) => a.iter().filter_map(byte_string).map(PathBuf::from).collect(),
            _ => Vec::new(),
        }
    }

    pub fn list() -> Result<Vec<Drive>, DriveError> {
        let conn = connect()?;
        let objs = objects(&conn)?;
        let mut out = Vec::new();
        for ifaces in objs.values() {
            let Some(block) = ifaces.get("org.freedesktop.UDisks2.Block") else { continue };
            if boolean(block, "HintSystem") || boolean(block, "HintIgnore") {
                continue;
            }
            let Some(drive_path) = object_path(block, "Drive") else { continue };
            let Some(drive) = objs
                .iter()
                .find(|(k, _)| k.as_str() == drive_path)
                .and_then(|(_, i)| i.get("org.freedesktop.UDisks2.Drive"))
            else {
                continue;
            };
            if !(boolean(drive, "Removable") || boolean(drive, "MediaRemovable")) {
                continue;
            }
            // Whole-disk blocks that carry a partition table aren't volumes themselves.
            if ifaces.contains_key("org.freedesktop.UDisks2.PartitionTable") {
                continue;
            }
            let device = get(block, "PreferredDevice")
                .and_then(byte_string)
                .or_else(|| get(block, "Device").and_then(byte_string));
            let Some(device) = device else { continue };
            let fs = string(block, "IdType");
            let mounts = ifaces.get("org.freedesktop.UDisks2.Filesystem").map(mount_points).unwrap_or_default();
            let mount = mounts.into_iter().next();
            let label = string(block, "IdLabel");
            let vendor_model =
                [string(drive, "Vendor"), string(drive, "Model")].into_iter().flatten().collect::<Vec<_>>().join(" ");
            out.push(Drive {
                name: label
                    .clone()
                    .unwrap_or_else(|| if vendor_model.is_empty() { device.clone() } else { vendor_model }),
                id: device,
                label,
                size_bytes: u64_of(block, "Size"),
                free_bytes: mount.as_deref().and_then(|m| free_space(m).ok()),
                status: status_for(fs.as_deref(), mount.is_some()),
                filesystem: fs.map(|f| f.to_ascii_lowercase()),
                mount_path: mount,
                has_vault: false,
            });
        }
        Ok(out)
    }

    fn block_path(objs: &Objects, device: &str) -> Option<String> {
        objs.iter().find_map(|(k, ifaces)| {
            let b = ifaces.get("org.freedesktop.UDisks2.Block")?;
            let dev = get(b, "Device").and_then(byte_string);
            let pref = get(b, "PreferredDevice").and_then(byte_string);
            (dev.as_deref() == Some(device) || pref.as_deref() == Some(device)).then(|| k.to_string())
        })
    }

    fn opts() -> HashMap<&'static str, Value<'static>> {
        HashMap::new()
    }

    pub fn eject(mount: &Path) -> Result<(), DriveError> {
        let conn = connect()?;
        let objs = objects(&conn)?;
        let (path, drive) = objs
            .iter()
            .find_map(|(k, ifaces)| {
                let fs = ifaces.get("org.freedesktop.UDisks2.Filesystem")?;
                let block = ifaces.get("org.freedesktop.UDisks2.Block")?;
                mount_points(fs).iter().any(|m| m == mount).then(|| (k.to_string(), object_path(block, "Drive")))
            })
            .ok_or_else(|| DriveError::Unavailable("not a mounted removable volume".into()))?;
        let fail = |e: zbus::Error| DriveError::Unavailable(e.to_string());
        Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.UDisks2.Filesystem")
            .map_err(fail)?
            .call_method("Unmount", &(opts(),))
            .map_err(fail)?;
        if let Some(d) = drive {
            // Not every drive can be powered off; unmounted is enough to pull it.
            if let Ok(p) = Proxy::new(&conn, DEST, d.as_str(), "org.freedesktop.UDisks2.Drive") {
                let _ = p.call_method("PowerOff", &(opts(),));
            }
        }
        Ok(())
    }

    pub fn format_exfat(drive: &Drive, label: &str) -> Result<PathBuf, DriveError> {
        let conn = connect()?;
        let objs = objects(&conn)?;
        let path = block_path(&objs, &drive.id).ok_or_else(|| DriveError::FormatFailed("drive not found".into()))?;
        let fail = |e: zbus::Error| {
            let msg = e.to_string();
            if msg.contains("NotAuthorized") || msg.contains("Cancelled") {
                DriveError::FormatCancelled
            } else {
                DriveError::FormatFailed(msg)
            }
        };
        if drive.mount_path.is_some() {
            let fsys = Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.UDisks2.Filesystem").map_err(fail)?;
            let _: () = fsys.call("Unmount", &(opts(),)).map_err(fail)?;
        }
        let block = Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.UDisks2.Block").map_err(fail)?;
        let mut o = opts();
        o.insert("label", Value::from(label.to_owned()));
        o.insert("update-partition-type", Value::from(true));
        let formatted: Result<(), _> = block.call("Format", &("exfat", o));
        if let Err(e) = formatted {
            if matches!(fail(e), DriveError::FormatCancelled) {
                return Err(DriveError::FormatCancelled);
            }
            // UDisks without exFAT support: fall back to mkfs.exfat via pkexec.
            let st = std::process::Command::new("pkexec")
                .args(["mkfs.exfat", "-n", label, drive.id.as_str()])
                .status()
                .map_err(|e| DriveError::FormatFailed(format!("pkexec: {e}")))?;
            match st.code() {
                Some(0) => {}
                Some(126) | Some(127) => return Err(DriveError::FormatCancelled),
                _ => return Err(DriveError::FormatFailed(format!("mkfs.exfat exited with {st}"))),
            }
        }
        // The new file system shows up on the same block object; mount it.
        for _ in 0..20 {
            if let Ok(fsys) = Proxy::new(&conn, DEST, path.as_str(), "org.freedesktop.UDisks2.Filesystem") {
                if let Ok(mount) = fsys.call::<_, _, String>("Mount", &(opts(),)) {
                    return Ok(PathBuf::from(mount));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        Err(DriveError::FormatFailed("formatted, but mounting failed; reinsert the stick".into()))
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    use super::*;

    const DRIVE_REMOVABLE: u32 = 2;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(b: &[u16]) -> String {
        let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
        String::from_utf16_lossy(&b[..end])
    }

    pub fn free_space(path: &Path) -> std::io::Result<u64> {
        let p: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
        // SAFETY: valid wide string and out-pointers.
        if unsafe { GetDiskFreeSpaceExW(p.as_ptr(), &mut free, &mut total, &mut total_free) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(free)
    }

    fn total_space(root: &[u16]) -> u64 {
        let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
        // SAFETY: as above.
        unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut free, &mut total, &mut total_free) };
        total
    }

    pub fn list() -> Result<Vec<Drive>, DriveError> {
        let mask = unsafe { GetLogicalDrives() };
        let mut out = Vec::new();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = format!("{letter}:\\");
            let w = wide(&root);
            // SAFETY: NUL-terminated wide string.
            if unsafe { GetDriveTypeW(w.as_ptr()) } != DRIVE_REMOVABLE {
                continue;
            }
            let mut label = [0u16; 261];
            let mut fs = [0u16; 261];
            // SAFETY: buffers sized as passed.
            let ok = unsafe {
                GetVolumeInformationW(
                    w.as_ptr(),
                    label.as_mut_ptr(),
                    label.len() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    fs.as_mut_ptr(),
                    fs.len() as u32,
                ) != 0
            };
            let (label, fs) = if ok {
                (Some(from_wide(&label)).filter(|s| !s.is_empty()), Some(from_wide(&fs).to_ascii_lowercase()))
            } else {
                (None, None)
            };
            let mount = ok.then(|| PathBuf::from(&root));
            out.push(Drive {
                id: format!("{letter}:"),
                name: label.clone().unwrap_or_else(|| format!("Removable drive ({letter}:)")),
                label,
                size_bytes: if ok { total_space(&w) } else { 0 },
                free_bytes: mount.as_deref().and_then(|m| free_space(m).ok()),
                status: status_for(fs.as_deref(), ok),
                filesystem: fs,
                mount_path: mount,
                has_vault: false,
            });
        }
        Ok(out)
    }

    pub fn eject(_: &Path) -> Result<(), DriveError> {
        Ok(())
    }

    pub fn format_exfat(drive: &Drive, label: &str) -> Result<PathBuf, DriveError> {
        // Only "X:" ever reaches the command line.
        let b = drive.id.as_bytes();
        if !(b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':') {
            return Err(DriveError::FormatFailed("not a drive letter".into()));
        }
        let verb = wide("runas");
        let file = wide("cmd.exe");
        let params = wide(&format!("/c format {} /FS:exFAT /Q /V:{label} /Y", drive.id));
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        // SAFETY: info is fully initialised and its strings outlive the call.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            return Err(if unsafe { GetLastError() } == ERROR_CANCELLED {
                DriveError::FormatCancelled
            } else {
                DriveError::FormatFailed(std::io::Error::last_os_error().to_string())
            });
        }
        let mut code = 1u32;
        // SAFETY: hProcess is valid because of SEE_MASK_NOCLOSEPROCESS.
        unsafe {
            WaitForSingleObject(info.hProcess, INFINITE);
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
        }
        if code != 0 {
            return Err(DriveError::FormatFailed(format!("format exited with code {code}")));
        }
        Ok(PathBuf::from(format!("{}\\", drive.id)))
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::*;

    pub fn free_space(_: &Path) -> std::io::Result<u64> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    pub fn list() -> Result<Vec<Drive>, DriveError> {
        Err(DriveError::Unavailable("not supported on this platform".into()))
    }

    pub fn format_exfat(_: &Drive, _: &str) -> Result<PathBuf, DriveError> {
        Err(DriveError::Unavailable("not supported on this platform".into()))
    }

    pub fn eject(_: &Path) -> Result<(), DriveError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_and_labels() {
        assert_eq!(status_for(Some("exfat"), true), DriveStatus::Ready);
        assert_eq!(status_for(Some("exFAT"), true), DriveStatus::Ready);
        assert_eq!(status_for(Some("vfat"), true), DriveStatus::NeedsFormatting);
        assert_eq!(status_for(Some("ntfs"), true), DriveStatus::NeedsFormatting);
        assert_eq!(status_for(Some("exfat"), false), DriveStatus::NotMounted);
        assert_eq!(status_for(None, false), DriveStatus::NeedsFormatting);
        assert!(valid_label("CINASAFE"));
        assert!(!valid_label(""));
        assert!(!valid_label("TWELVECHARS1"));
        assert!(!valid_label("A & del"), "no shell metacharacters");
    }

    #[test]
    fn free_space_of_temp() {
        assert!(free_space(&std::env::temp_dir()).unwrap() > 0);
    }

    #[test]
    fn removal_is_noticed_within_a_second() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".cinasafe")).unwrap();
        std::fs::write(root.path().join(".cinasafe/vault.json"), b"{}").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let _w = RemovalWatch::start(root.path(), move || tx.send(std::time::Instant::now()).unwrap());
        std::thread::sleep(Duration::from_millis(300));
        assert!(rx.try_recv().is_err(), "no false alarm while present");
        let gone = std::time::Instant::now();
        std::fs::remove_dir_all(root.path().join(".cinasafe")).unwrap();
        let fired = rx.recv_timeout(Duration::from_secs(2)).expect("removal noticed");
        assert!(fired.duration_since(gone) < Duration::from_secs(1));
    }

    #[test]
    fn listing_never_panics() {
        // In CI there may be no D-Bus or no removable drives; either way, no panic.
        let _ = list_removable();
    }
}
