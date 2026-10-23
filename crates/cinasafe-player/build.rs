//! With the `static-mpv` feature, links the static libmpv built by
//! `third_party/mpv/build-{linux,windows}.*` into whatever binary uses this
//! crate. That build leaves `link.txt` beside the libraries: one entry per
//! line, in link order, `static=<file in that folder>` or `dylib=<system library>`.
//! The folder is `$CINASAFE_MPV_STATIC_DIR`, or `third_party/mpv/<os>-<arch>`.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CINASAFE_MPV_STATIC_DIR");
    if std::env::var_os("CARGO_FEATURE_STATIC_MPV").is_none() {
        return;
    }
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" if os == "windows" => "arm64".to_owned(),
        other => other.to_owned(),
    };
    let dir = std::env::var_os("CINASAFE_MPV_STATIC_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../third_party/mpv")
            .join(format!("{os}-{arch}"))
    });
    let list = dir.join("link.txt");
    println!("cargo:rerun-if-changed={}", list.display());
    let text = std::fs::read_to_string(&list).unwrap_or_else(|e| {
        panic!(
            "static-mpv: can't read {} ({e}). Build it with third_party/mpv/build-{}, or set CINASAFE_MPV_STATIC_DIR.",
            list.display(),
            if os == "windows" { "windows.ps1" } else { "linux.sh" }
        )
    });
    let dir = dir.canonicalize().unwrap();
    println!("cargo:rustc-link-search=native={}", dir.display());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        match line.split_once('=') {
            Some(("static", file)) => {
                assert!(dir.join(file).is_file(), "static-mpv: {file} is listed but missing from {}", dir.display());
                println!("cargo:rerun-if-changed={}", dir.join(file).display());
                // Unbundled, by exact file name: the final link gets the archive itself, in this order.
                println!("cargo:rustc-link-lib=static:-bundle,+verbatim={file}");
            }
            Some(("dylib", name)) => println!("cargo:rustc-link-lib=dylib={name}"),
            _ => panic!("static-mpv: bad line in {}: {line}", list.display()),
        }
    }
}
