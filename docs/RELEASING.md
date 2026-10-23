# Building and releasing

Everything is built by GitHub Actions (`.github/workflows/build.yml`). Each push builds
packages as workflow artifacts. Pushing a tag `v*` publishes them as a pre-release.

## What a release contains

| File | What |
| --- | --- |
| `Cinasafe-stick-kit.zip` | Everything for a stick's root (brief 4.1): `Cinasafe-Windows.exe`, `Cinasafe-Linux-x86_64.AppImage`, `run-linux.sh`, `README.txt`, and the licences in `.cinasafe/runtime/`. Unzip onto an exFAT stick, run the app from it, and the create-vault wizard does the rest. |
| `Cinasafe-Windows-<arch>.exe` | The portable Windows app: one file, libmpv compiled in, nothing to install. Needs only the WebView2 runtime that Windows 11 and updated Windows 10 have. |
| `Cinasafe-Linux-<arch>.AppImage` | The portable Linux app: one file, libmpv compiled in. |
| `cinasafe_<version>_amd64.deb`, `cinasafe-<version>-1.x86_64.rpm` | Install on a computer; libmpv is compiled in. |
| `Cinasafe_<version>_x64-setup.exe` | Windows installer (current user, no admin). |
| `cinasafe-cli-linux-<arch>.tar.gz`, `cinasafe-cli-windows-<arch>.exe` | The `cinasafe` command-line tool. |
| `libmpv-sources-*.tar.gz` | The exact sources of mpv, FFmpeg and the other libraries compiled into the app: the LGPL source offer. |
| `THIRD_PARTY_LICENSES.md` | Every bundled component and its licence. |
| `SHA256SUMS.txt`, `SHA256SUMS.txt.minisig` | Checksums of all of the above, and their signature. |

## libmpv

libmpv is compiled into the app: the package jobs build with the cargo feature
`static-mpv`, which links static mpv, FFmpeg, libplacebo, libass and dav1d into
the binary (`crates/cinasafe-player/build.rs`). No package carries or looks for
a libmpv file. CI checks this: the Linux job fails if `cinasafe-app` links any
of them dynamically, and the Windows job fails if the exe needs a DLL that
isn't part of Windows.

`third_party/mpv/build-linux.sh` and `build-windows.ps1` build those libraries
from pinned sources as **LGPL**: mpv with `-Dgpl=false`, FFmpeg without `gpl`,
`version3` or `nonfree`. Both scripts stop if either ends up GPL or anything
links a GPL-only library. CI caches the result keyed on the scripts (and
Windows' wrap files), so changing a version rebuilds it: about 15 minutes on
Linux, and longer on Windows, which builds every dependency from source.
Each build's `SOURCES.txt` lists the exact commits.

Development builds without the feature load the system's libmpv at run time,
looking in `CINASAFE_LIBMPV_DIR`, then next to the executable, then on the
system search path.

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
