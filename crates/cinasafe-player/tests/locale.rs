//! A desktop in German, Hebrew, French, ... runs with a decimal-comma
//! `LC_NUMERIC` (GTK sets the locale at start-up), and libmpv's `mpv_create`
//! returns NULL under any locale but "C". `Mpv::new` must cope.
//!
//! Needs `localedef` and the `de_DE` source locale; skipped without them.
#![cfg(target_os = "linux")]

use std::ffi::{c_char, c_int};

use cinasafe_player::Mpv;

const LC_NUMERIC: c_int = 1;

unsafe extern "C" {
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

#[test]
fn mpv_starts_under_a_decimal_comma_locale() {
    let required = std::env::var_os("CINASAFE_REQUIRE_LIBMPV").is_some();
    if let Err(e) = cinasafe_player::ffi::api() {
        assert!(!required, "libmpv required but not loadable: {e}");
        eprintln!("skipped: {e}");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let made = std::process::Command::new("localedef")
        .args(["-i", "de_DE", "-f", "UTF-8"])
        .arg(dir.path().join("de_DE.UTF-8"))
        .output();
    // localedef warns about missing locale parts and exits 1 even when it wrote the locale.
    if !dir.path().join("de_DE.UTF-8").join("LC_NUMERIC").exists() {
        eprintln!("skipped: could not build a de_DE locale: {made:?}");
        return;
    }
    // SAFETY: this test binary has a single test, so nothing else reads the environment.
    unsafe { std::env::set_var("LOCPATH", dir.path()) };
    let set = unsafe { setlocale(LC_NUMERIC, c"de_DE.UTF-8".as_ptr()) };
    assert!(!set.is_null(), "setlocale(de_DE.UTF-8) failed");

    Mpv::new().expect("Mpv::new under a non-C LC_NUMERIC");
}
