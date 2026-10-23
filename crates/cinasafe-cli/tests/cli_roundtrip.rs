//! Brief 13: `create → add → ls → cat | sha256` equals the original, plus the
//! other M1 commands, through the real binary.

use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

fn run(root: &Path, args: &[&str], password: &str, new_password: Option<&str>) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_cinasafe"));
    c.args(args).env("CINASAFE_PASSWORD", password).current_dir(root);
    if let Some(n) = new_password {
        c.env("CINASAFE_NEW_PASSWORD", n);
    }
    c.output().unwrap()
}

fn ok(o: &Output) -> String {
    assert!(
        o.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout.clone()).unwrap()
}

#[test]
fn create_add_ls_cat_export_passwd_verify() {
    let stick = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let root = stick.path().to_str().unwrap();
    let pw = "correct horse battery";

    let out = ok(&run(work.path(), &["create", root], pw, None));
    assert!(out.contains("Recovery key"));
    let words: Vec<&str> = out
        .lines()
        .flat_map(|l| l.split_whitespace())
        .filter(|w| w.chars().all(|c| c.is_ascii_lowercase()) && w.len() >= 3)
        .collect();
    assert!(words.len() >= 24);

    let data: Vec<u8> = (0..2_500_000u32).map(|i| (i * 31 % 251) as u8).collect();
    let src = work.path().join("Movie.Name.2014.1080p.mkv");
    std::fs::write(&src, &data).unwrap();
    let out = ok(&run(work.path(), &["add", root, src.to_str().unwrap()], pw, None));
    let id = out.split('\t').next().unwrap().trim().to_owned();

    let ls = ok(&run(work.path(), &["ls", root], pw, None));
    assert!(ls.contains("Movie.Name.2014.1080p.mkv"));

    let cat = run(work.path(), &["cat", root, &id], pw, None);
    assert!(cat.status.success());
    assert_eq!(Sha256::digest(&cat.stdout), Sha256::digest(&data), "cat | sha256 equals the original");

    let dest = work.path().join("out");
    std::fs::create_dir(&dest).unwrap();
    ok(&run(work.path(), &["export", root, &id, dest.to_str().unwrap()], pw, None));
    assert_eq!(std::fs::read(dest.join("Movie.Name.2014.1080p.mkv")).unwrap(), data);

    // Wrong password fails cleanly.
    let bad = run(work.path(), &["ls", root], "nope", None);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("wrong password"));

    ok(&run(work.path(), &["passwd", root], pw, Some("a brand new password")));
    assert!(!run(work.path(), &["unlock", root], pw, None).status.success());
    ok(&run(work.path(), &["unlock", root], "a brand new password", None));

    let v = ok(&run(work.path(), &["verify", root], "a brand new password", None));
    assert!(v.trim_end().ends_with("OK"), "{v}");

    let r = ok(&run(work.path(), &["recovery", root], "a brand new password", None));
    assert!(r.contains("New recovery key"));

    // No plaintext anywhere on the "stick".
    let needle = &data[1000..1064];
    let mut stack = vec![stick.path().to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            if e.file_type().unwrap().is_dir() {
                stack.push(e.path());
            } else {
                let raw = std::fs::read(e.path()).unwrap();
                assert!(!raw.windows(needle.len()).any(|w| w == needle), "plaintext in {:?}", e.path());
                assert!(!raw.windows(5).any(|w| w == b"Movie"), "file name in {:?}", e.path());
            }
        }
    }
}
