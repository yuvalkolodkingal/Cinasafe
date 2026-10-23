//! The libmpv that will ship loads, accepts every mandatory option (brief
//! 6.2) and initialises. Skipped when no libmpv is found, unless
//! `CINASAFE_REQUIRE_LIBMPV=1` (CI sets it, with `CINASAFE_LIBMPV_DIR`
//! pointing at the bundled build).

use cinasafe_player::Mpv;

#[test]
fn bundled_libmpv_loads_and_initialises() {
    let required = std::env::var_os("CINASAFE_REQUIRE_LIBMPV").is_some();
    if let Err(e) = cinasafe_player::ffi::api() {
        assert!(!required, "libmpv required but not loadable: {e}");
        eprintln!("skipped: {e}");
        return;
    }
    // Mpv::new applies SAFE_OPTIONS and fails if any is rejected.
    let mpv = Mpv::new().expect("mpv with the mandatory options");
    for (k, v) in [("vo", "null"), ("ao", "null"), ("idle", "yes")] {
        mpv.set_option(k, v).unwrap();
    }
    mpv.initialize().expect("initialize");
    let version = mpv.get_string("mpv-version").unwrap();
    let ffmpeg = mpv.get_string("ffmpeg-version").unwrap_or_default();
    eprintln!("{version}, FFmpeg {ffmpeg}");
    assert!(version.starts_with("mpv "), "{version}");
}
