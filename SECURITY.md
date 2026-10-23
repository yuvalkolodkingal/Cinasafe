# Security policy

Cinasafe protects media and viewing history on a USB stick that may be lost
or stolen. What it does and doesn't protect against is written down in
[`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md); the on-disk format is in
[`docs/FORMAT.md`](docs/FORMAT.md).

## Reporting a vulnerability

Please report vulnerabilities **privately**, through GitHub's private
vulnerability reporting: on the repository page, open **Security** ›
**Report a vulnerability**. Don't open a public issue or pull request.

Include what you found, how to reproduce it, which version or commit you
tested, and what an attacker gains. A proof-of-concept vault or file helps.

You'll get an acknowledgement within a week. Fixes for confirmed issues are
released as soon as they are ready, with credit to you unless you prefer
otherwise.

## In scope

- Anything that lets someone holding the stick, without the password or
  recovery key, learn titles, subtitles, artwork, watch history or media content.
- Tampering with the stick's files that goes undetected, or that makes
  Cinasafe misbehave when it opens them (crashes, code execution, writes
  outside the stick).
- Decrypted data reaching the host's disk: temp files, caches, logs, crash
  dumps, "recent files" entries.
- Key handling: keys or passwords kept after locking, weak key derivation,
  nonce reuse.

## Out of scope

As stated in the threat model: malware, keyloggers or memory dumps on the
host computer while the vault is open, someone watching the screen, and the
size and file-count information the format deliberately leaves visible.

## Supported versions

Cinasafe is pre-1.0. Only the latest release and `main` get security fixes.
