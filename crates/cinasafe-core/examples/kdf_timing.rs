//! Times the default Argon2id unlock cost: `cargo run --release --example kdf_timing`.
fn main() {
    let p = cinasafe_core::crypto::KdfParams::new_default().unwrap();
    let t = std::time::Instant::now();
    let _k = cinasafe_core::crypto::password_kek(b"correct horse battery staple", &p).unwrap();
    println!("argon2id m={} KiB t={} p={}: {:.2} s", p.m_kib, p.t, p.p, t.elapsed().as_secs_f64());
}
