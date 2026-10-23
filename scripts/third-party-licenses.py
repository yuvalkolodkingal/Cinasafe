#!/usr/bin/env python3
"""Generates THIRD_PARTY_LICENSES.md (brief section 12).

Sources:
  * Rust crates linked into the shipped binaries (cinasafe-app, cinasafe-cli)
    on every release target, from `cargo metadata` (normal dependencies only;
    build and dev dependencies aren't shipped).
  * npm packages in the UI bundle (app/ui production dependencies, from
    package-lock.json; their copyright lines from node_modules).
  * Native libraries and fonts the packages ship (fixed list below).

Each package's licence expression is checked: an unknown licence, a missing
one, or one with no permissive choice (GPL, AGPL, ...) fails the run. For
packages offering a choice, the first acceptable licence in PREFERENCE is
recorded. Copyright lines and NOTICE files are taken from each package's own
licence files, and the full text of every licence used is appended from
scripts/licenses/.

Usage:
  scripts/third-party-licenses.py           write THIRD_PARTY_LICENSES.md
  scripts/third-party-licenses.py --check   fail if the file is out of date

Needs: cargo (network for the first `cargo metadata` run, to fetch sources),
and `npm ci` already run in app/ui.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_LICENSES.md"
TEXTS = ROOT / "scripts" / "licenses"
UI = ROOT / "app" / "ui"

SHIPPED_CRATES = ["cinasafe-app", "cinasafe-cli"]
TARGETS = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
]

# Acceptable licences, most preferred first. When a package offers a choice,
# the earliest one here is the one recorded.
PREFERENCE = [
    "Apache-2.0",
    "MIT",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "MPL-2.0",
    "CC0-1.0",
    "0BSD",
    "MIT-0",
    "Unlicense",
    "BSL-1.0",
    "Apache-2.0 WITH LLVM-exception",
    "OFL-1.1",
]
# Known but not acceptable for code linked into the app.
REJECTED = re.compile(r"^(A?GPL|LGPL|SSPL|EUPL|CDDL|OSL|CC-BY-NC|BUSL|Commons-Clause)", re.I)

# Native components shipped in the packages (not visible to cargo or npm).
NATIVE = [
    ("libmpv", "LGPL-2.1-or-later", "built with `-Dgpl=false`; https://mpv.io/ (source: https://github.com/mpv-player/mpv)"),
    ("FFmpeg", "LGPL-2.1-or-later", "built without `--enable-gpl` / `--enable-nonfree`; https://ffmpeg.org/"),
    ("libplacebo", "LGPL-2.1-or-later", "https://code.videolan.org/videolan/libplacebo"),
    ("libass", "ISC", "https://github.com/libass/libass"),
    ("FriBidi", "LGPL-2.1-or-later", "https://github.com/fribidi/fribidi"),
    ("HarfBuzz", "MIT (\"Old MIT\")", "https://github.com/harfbuzz/harfbuzz"),
    ("FreeType", "FTL", "dual-licensed FTL or GPL-2.0; used under the FTL; https://freetype.org/"),
    ("dav1d", "BSD-2-Clause", "https://code.videolan.org/videolan/dav1d"),
    ("zlib", "Zlib", "Windows build; https://zlib.net/"),
    ("shaderc", "Apache-2.0", "Windows build (Direct3D 11 shaders); https://github.com/google/shaderc"),
    ("glslang", "BSD-3-Clause", "Windows build, via shaderc; also BSD-2-Clause, MIT and Apache-2.0 parts, and a Bison grammar under GPL-3.0 with the Bison exception, which permits this use; https://github.com/KhronosGroup/glslang"),
    ("SPIRV-Tools", "Apache-2.0", "Windows build, via shaderc; https://github.com/KhronosGroup/SPIRV-Tools"),
    ("SPIRV-Headers", "MIT", "Windows build; https://github.com/KhronosGroup/SPIRV-Headers"),
    ("SPIRV-Cross", "Apache-2.0", "Windows build; https://github.com/KhronosGroup/SPIRV-Cross"),
    ("Rubik (UI font)", "OFL-1.1", "https://github.com/googlefonts/rubik"),
    ("Noto Sans, Noto Sans Hebrew, Noto Sans Arabic (UI and subtitle fonts)", "OFL-1.1", "https://notofonts.github.io/"),
]
NATIVE_TEXTS = ["LGPL-2.1-or-later", "ISC", "MIT", "FTL", "BSD-2-Clause", "BSD-3-Clause", "Apache-2.0", "Zlib", "OFL-1.1"]

LICENSE_FILE = re.compile(r"^(LICEN[SC]E|COPYING|COPYRIGHT|NOTICE|UNLICENSE)", re.I)
# "Copyright 2019 X", "COPYRIGHT (c) X", "© 2020 X", "(c) 2018 X"; not licence prose
# such as "copyright license to reproduce" or "(c) You must retain".
COPYRIGHT_LINE = re.compile(r"^\s*((Copyright|COPYRIGHT)\b(?!\s+(license|notice|and\b|owner|holder))|©|\([cC]\)\s*\d{4})")
TEMPLATE_LINE = re.compile(r"\[yyyy\]|<year>|\{yyyy\}|copyright owner\]|\[name of|<copyright holders>|copyright notice", re.I)


class Fail(Exception):
    pass


# ---------- SPDX expressions ----------

def tokens(expr: str) -> list[str]:
    expr = expr.replace("/", " OR ")
    return re.findall(r"\(|\)|[A-Za-z0-9.+\-]+", expr)


def parse(expr: str) -> list[frozenset[str]]:
    """Disjunctive normal form: a list of alternatives, each a set of licences that all apply."""
    toks = tokens(expr)
    pos = 0

    def peek() -> str | None:
        return toks[pos] if pos < len(toks) else None

    def take() -> str:
        nonlocal pos
        t = toks[pos]
        pos += 1
        return t

    def primary() -> list[frozenset[str]]:
        if peek() == "(":
            take()
            r = disj()
            if take() != ")":
                raise Fail(f"bad licence expression: {expr}")
            return r
        ident = take()
        if peek() == "WITH":
            take()
            ident = f"{ident} WITH {take()}"
        return [frozenset([ident])]

    def conj() -> list[frozenset[str]]:
        r = primary()
        while peek() == "AND":
            take()
            rhs = primary()
            r = [a | b for a in r for b in rhs]
        return r

    def disj() -> list[frozenset[str]]:
        r = conj()
        while peek() == "OR":
            take()
            r = r + conj()
        return r

    result = disj()
    if pos != len(toks):
        raise Fail(f"bad licence expression: {expr}")
    return result


def rank(lic: str) -> int:
    return PREFERENCE.index(lic) if lic in PREFERENCE else len(PREFERENCE)


def choose(name: str, expr: str | None) -> list[str]:
    """The licences to record for a package (all of them apply)."""
    if not expr:
        raise Fail(f"{name}: no licence declared")
    options = parse(expr)
    for alt in options:
        for lic in alt:
            if lic not in PREFERENCE and not REJECTED.match(lic):
                raise Fail(f"{name}: unknown licence {lic!r} in {expr!r}; add it to PREFERENCE or REJECTED")
    ok = [alt for alt in options if all(l in PREFERENCE for l in alt)]
    if not ok:
        raise Fail(f"{name}: no permissive choice in {expr!r}")
    best = min(ok, key=lambda alt: (max(rank(l) for l in alt), len(alt)))
    return sorted(best, key=rank)


# ---------- notices ----------

def notices(directory: Path) -> tuple[list[str], list[str]]:
    """Copyright lines from a package's licence files, and its NOTICE files' contents."""
    lines: list[str] = []
    notice_texts: list[str] = []
    if not directory.is_dir():
        return lines, notice_texts
    for f in sorted(p for p in directory.iterdir() if p.is_file() and LICENSE_FILE.match(p.name)):
        try:
            text = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        if f.name.upper().startswith("NOTICE"):
            body = text.strip()
            if body:
                notice_texts.append(body)
            continue
        for line in text.splitlines():
            line = " ".join(line.split())
            if COPYRIGHT_LINE.match(line) and not TEMPLATE_LINE.search(line) and line not in lines:
                lines.append(line)
    return lines[:12], notice_texts


# ---------- sources ----------

def rust_packages() -> list[dict]:
    seen: dict[str, dict] = {}
    for target in TARGETS:
        out = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        meta = json.loads(out)
        packages = {p["id"]: p for p in meta["packages"]}
        nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
        workspace = set(meta["workspace_members"])
        stack = [p["id"] for p in meta["packages"] if p["name"] in SHIPPED_CRATES and p["id"] in workspace]
        if len(stack) != len(SHIPPED_CRATES):
            raise Fail(f"shipped crates not found in the workspace: {SHIPPED_CRATES}")
        visited: set[str] = set()
        while stack:
            pid = stack.pop()
            if pid in visited:
                continue
            visited.add(pid)
            if pid not in workspace:
                seen[pid] = packages[pid]
            for dep in nodes[pid]["deps"]:
                if any(k["kind"] is None for k in dep["dep_kinds"]):
                    stack.append(dep["pkg"])
    out = []
    for p in seen.values():
        lic = choose(f"{p['name']} {p['version']}", p.get("license"))
        copyright_lines, notice_texts = notices(Path(p["manifest_path"]).parent)
        out.append(
            {
                "name": p["name"],
                "version": p["version"],
                "expr": p.get("license") or "",
                "chosen": lic,
                "source": p.get("repository") or f"https://crates.io/crates/{p['name']}",
                "copyright": copyright_lines,
                "notices": notice_texts,
            }
        )
    return sorted(out, key=lambda p: (p["name"].lower(), p["version"]))


def npm_packages() -> list[dict]:
    lock = json.loads((UI / "package-lock.json").read_text())
    modules = UI / "node_modules"
    if not modules.is_dir():
        raise Fail("app/ui/node_modules is missing: run `npm ci` in app/ui first")
    out = []
    for key, p in lock["packages"].items():
        if not key or p.get("dev") or p.get("devOptional"):
            continue
        name = key.split("node_modules/")[-1]
        lic = choose(f"{name} {p.get('version')}", p.get("license"))
        copyright_lines, notice_texts = notices(modules / name)
        out.append(
            {
                "name": name,
                "version": p.get("version", ""),
                "expr": p.get("license", ""),
                "chosen": lic,
                "source": f"https://www.npmjs.com/package/{name}",
                "copyright": copyright_lines,
                "notices": notice_texts,
            }
        )
    return sorted(out, key=lambda p: p["name"].lower())


# ---------- output ----------

def text_files(lic: str) -> list[Path]:
    parts = lic.split(" WITH ")
    files = [TEXTS / f"{parts[0]}.txt"] + ([TEXTS / f"{parts[1]}.txt"] if len(parts) > 1 else [])
    for f in files:
        if not f.is_file():
            raise Fail(f"no licence text for {lic}: add {f.relative_to(ROOT)} (from spdx/license-list-data)")
    return files


def table(rows: list[dict]) -> list[str]:
    out = ["| Package | Version | Licence used | Declared | Source |", "| --- | --- | --- | --- | --- |"]
    for r in rows:
        declared = r["expr"].replace("|", "\\|")
        out.append(f"| {r['name']} | {r['version']} | {' AND '.join(r['chosen'])} | {declared} | {r['source']} |")
    return out


def notice_section(rows: list[dict]) -> list[str]:
    out = []
    for r in rows:
        if not r["copyright"] and not r["notices"]:
            continue
        out.append(f"**{r['name']} {r['version']}**")
        out.append("")
        out.extend(f"    {line}" for line in r["copyright"])
        for n in r["notices"]:
            if r["copyright"]:
                out.append("")
            out.extend(f"    {line}" if line else "" for line in n.splitlines())
        out.append("")
    return out


def render() -> str:
    rust = rust_packages()
    npm = npm_packages()
    used: Counter[str] = Counter()
    for r in rust + npm:
        for lic in r["chosen"]:
            used[lic] += 1
    families = sorted(set(used) | set(n for n in NATIVE_TEXTS), key=lambda l: (rank(l), l))

    md: list[str] = [
        "# Third-party licences",
        "",
        "Cinasafe is licensed under the [Apache License 2.0](LICENSE). The software below is",
        "included in or linked into its builds under its own licence.",
        "",
        "<!-- Generated by scripts/third-party-licenses.py; don't edit by hand. -->",
        "",
        "## Summary",
        "",
        "| Licence | Rust crates and npm packages |",
        "| --- | --- |",
    ]
    md += [f"| {lic} | {n} |" for lic, n in sorted(used.items(), key=lambda kv: (-kv[1], kv[0]))]
    md += [
        "",
        "Where a package offers a choice of licences, the first permissive one in this order is",
        f"used: {', '.join(PREFERENCE)}.",
        "",
        "## Native libraries and fonts",
        "",
        "Compiled into the app (`cinasafe-app`, and the `cinasafe` CLI) in every package: libmpv",
        "and the libraries below it. On Linux, FreeType, FriBidi, HarfBuzz and fontconfig come",
        "from the system (the AppImage bundles them); on Windows they are compiled in too. Also",
        "shipped: the fonts in the app's resources and the UI bundle.",
        "",
        "| Component | Licence | Notes |",
        "| --- | --- | --- |",
    ]
    md += [f"| {name} | {lic} | {note} |" for name, lic, note in NATIVE]
    md += [
        "",
        "The LGPL libraries are linked statically. As LGPL-2.1 section 6 requires, you can",
        "relink the app against modified versions of them: its complete source is public, and",
        "the scripts that build the libraries are in [`third_party/mpv`](third_party/mpv).",
        "Each release attaches their exact source code (`libmpv-sources-*.tar.gz`), and each",
        "build's `SOURCES.txt` lists the commits.",
        "",
        f"## Rust crates ({len(rust)})",
        "",
        "Linked into `cinasafe-app` and the `cinasafe` CLI on "
        + ", ".join(f"`{t}`" for t in TARGETS)
        + ".",
        "",
    ]
    md += table(rust)
    md += ["", f"## JavaScript packages in the UI bundle ({len(npm)})", ""]
    md += table(npm)
    md += ["", "## Copyright notices", "", "From each package's own licence and NOTICE files.", ""]
    md += notice_section(rust + npm)
    md += ["## Licence texts", ""]
    for lic in families:
        md += [f"### {lic}", ""]
        for f in text_files(lic):
            md += ["```text", f.read_text(encoding="utf-8").strip(), "```", ""]
    return "\n".join(md).rstrip() + "\n"


def main() -> int:
    try:
        text = render()
    except Fail as e:
        print(f"third-party-licenses: {e}", file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as e:
        print(f"third-party-licenses: cargo metadata failed:\n{e.stderr}", file=sys.stderr)
        return 1
    if "--check" in sys.argv[1:]:
        if not OUT.is_file() or OUT.read_text(encoding="utf-8") != text:
            print("THIRD_PARTY_LICENSES.md is out of date: run scripts/third-party-licenses.py", file=sys.stderr)
            return 1
        print("THIRD_PARTY_LICENSES.md is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
