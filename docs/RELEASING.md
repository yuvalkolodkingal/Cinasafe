# Building and releasing

Everything is built by GitHub Actions (`.github/workflows/build.yml`). Each push builds
packages as workflow artifacts. Pushing a tag `v*` publishes them as a pre-release.

## What a release contains

| File | What |
| --- | --- |
| `Cinasafe-stick-kit.zip` | Everything for a stick's root (brief 4.1): `Cinasafe-Windows.exe`, `Cinasafe-Linux-x86_64.AppImage`, `run-linux.sh`, `README.txt`, and Windows' libmpv in `.cinasafe/runtime/windows-x86_64/`. Unzip onto an exFAT stick, run the app from it, and the create-vault wizard does the rest. |
| `Cinasafe-Linux-<arch>.AppImage` | Runs from anywhere and carries its own libmpv. |
| `cinasafe_<version>_amd64.deb`, `cinasafe-<version>-1.x86_64.rpm` | Install on a computer; use the distribution's libmpv. |
| `Cinasafe_<version>_x64-setup.exe` | Windows installer (current user, no admin). |
| `Cinasafe-Windows-<arch>-portable.zip` | The exe, the CLI and `mpv\`. |
| `cinasafe-cli-linux-<arch>.tar.gz` | The `cinasafe` command-line tool. |
| `libmpv-sources-*.tar.gz` | The exact FFmpeg, mpv (and on Linux libplacebo, dav1d) sources of the bundled libmpv: the LGPL source offer. |
| `THIRD_PARTY_LICENSES.md` | Every bundled component and its licence. |
| `SHA256SUMS.txt`, `SHA256SUMS.txt.minisig` | Checksums of all of the above, and their signature. |

## libmpv

`third_party/mpv/build-linux.sh` and `build-windows.sh` build libmpv from pinned
commits as **LGPL**: mpv with `-Dgpl=false`, FFmpeg without `--enable-gpl` or
`--enable-nonfree`. Both scripts stop if either ends up GPL. CI caches the result
keyed on the script, so bumping a version in the script rebuilds it (about 15
minutes on Linux, longer on Windows).

Each build's `SOURCES.txt` lists the exact commits and, on Windows, the MSYS2
package versions of the other DLLs. The `.deb` and `.rpm` use the
distribution's libmpv; that's the distribution's licensing.

The app looks for libmpv in this order:
1. `CINASAFE_LIBMPV_DIR`;
2. `mpv/` next to the executable;
3. `.cinasafe/runtime/<platform>/` next to it, which is the stick layout;
4. `../lib` from the executable, which is inside the AppImage;
5. the system search path.

## Signing

Release checksums are signed with [minisign](https://jedisct1.github.io/minisign/). Keyless
Sigstore signing would publish the repository's identity to a public transparency log, which
a private repository may not want.

One-time setup by the owner:

```sh
minisign -G -W -p cinasafe.pub -s cinasafe.key    # -W: no password, CI can't type one
```

1. Put the contents of `cinasafe.key` in the repository secret `MINISIGN_SECRET_KEY`, then
   delete the file, or keep it offline.
2. Publish `cinasafe.pub` in the README and on the website. Users then verify with:

```sh
minisign -Vm SHA256SUMS.txt -p cinasafe.pub && sha256sum -c SHA256SUMS.txt --ignore-missing
```

Without the secret, releases still get `SHA256SUMS.txt`, but not the signature, and the
workflow warns.

**Windows code signing** (SmartScreen) needs a certificate. Apply to a free open-source
programme such as SignPath Foundation, then add a signing step after "Build installer".
Until then, README.txt explains the SmartScreen warning.

## ARM

GitHub's ARM runners (`ubuntu-22.04-arm`, `windows-11-arm`) are free for public repositories
only. Set the repository variable `ENABLE_ARM_RUNNERS` to `true` once the repository is
public, or once the account pays for them. The same jobs then also build linux-aarch64
(AppImage, .deb, .rpm) and windows-arm64 (portable zip, installer). Windows on ARM also runs
the x86_64 build under emulation.

## Reproducibility

- **Pinned inputs:** actions are pinned by commit, libmpv sources by commit, npm packages by
  `package-lock.json`, and Rust crates by `Cargo.lock`.
- **Rust toolchain:** CI uses the current stable. Pin it with a `rust-toolchain.toml`
  before 1.0 so a rebuild uses the same compiler.
- **Not yet bit-for-bit:** Tauri bundles embed timestamps, and NSIS and linuxdeploy aren't
  deterministic. To check a release, rebuild from the tag and compare the binaries inside
  the packages (`target/release/cinasafe-app`, the libmpv libraries), not the packages
  themselves.

## Cutting a release

1. Make sure CI is green on `main`, including the disk-trace and screenshot jobs.
2. Update the version in `Cargo.toml` (workspace), `app/src-tauri/tauri.conf.json` and
   `app/ui/package.json`.
3. Regenerate the licence list: `python3 scripts/third-party-licenses.py`.
4. Tag and push: `git tag -s v0.2.0 -m "Cinasafe 0.2.0" && git push origin v0.2.0`.
   Or, without pushing a tag: run the **Build packages** workflow by hand on `main`
   (Actions → Build packages → Run workflow) with `release_tag` set to `v0.2.0`. The
   release job then creates the tag on the commit it built. Either way the tag must match
   the workspace version, or the release job fails.
5. The release is created as a **pre-release**. Check the stick kit on a real stick on
   Windows and Linux, then mark it as a full release.
