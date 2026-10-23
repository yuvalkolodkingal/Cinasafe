# Builds a static, LGPL libmpv for Windows with clang targeting the MSVC ABI,
# so it links straight into the app's MSVC-built exe (cargo feature
# `static-mpv`): no libmpv-2.dll or any other DLL ships beside the app.
#
# This follows mpv's own native Windows build (mpv ci/build-win32.ps1): meson
# builds mpv and every dependency from source as subprojects (pinned by the
# wrap files in subprojects-windows/), all static, with the static C/C++
# runtime (/MT) that Tauri's static VC runtime expects.
#
# Output in $Out (default third_party/mpv/windows-<arch>): the static
# libraries and link.txt, read by crates/cinasafe-player/build.rs (one line
# per library: `static=<file in $Out>` or `dylib=<Windows import library>`),
# plus SOURCES.txt, the licences, and ..\libmpv-sources-windows-<arch>.tar.gz
# (the LGPL source offer).
#
# Licensing (docs/OPEN_QUESTIONS.md Q2): mpv -Dgpl=false and FFmpeg without
# gpl/version3/nonfree, so the result is LGPL-2.1-or-later; the script fails
# otherwise. Static linking meets LGPL-2.1 section 6 because the app's
# complete source is public, so anyone can relink it against a modified
# library.
#
# Needs: Visual Studio 2022 (or Build Tools) with the Windows SDK, LLVM
# (clang, lld, llvm-rc, llvm-ar), Python 3 with meson and ninja, CMake, NASM,
# Git. GitHub's windows-latest runner has all of them except NASM and meson.

param(
    [string]$Out = "",
    [string]$Work = ""
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$MpvTag = "v0.41.0"
$MpvCommit = "41f6a645068483470267271e1d09966ca3b9f413"
$ShadercTag = "v2026.4"
$ShadercCommit = "b64fd42c4a2a9d501c47aca5d3571f7673a5e9cc"

$here = $PSScriptRoot
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x86_64" }
$vsArch = if ($arch -eq "arm64") { "arm64" } else { "amd64" }
if (-not $Out) { $Out = Join-Path $here "windows-$arch" }
if (-not $Work) { $Work = Join-Path $here "build-windows-$arch" }
New-Item -ItemType Directory -Force -Path $Work, $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Work = (Resolve-Path $Work).Path

# The Visual Studio developer environment (Windows SDK and MSVC libraries).
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
& (Join-Path $vs "Common7\Tools\Launch-VsDevShell.ps1") -Arch $vsArch -HostArch $vsArch -SkipAutomaticLocation | Out-Null

$env:CC = "clang"
$env:CXX = "clang++"
$env:CC_LD = "lld"
$env:CXX_LD = "lld"
$env:WINDRES = "llvm-rc"
$env:AR = "llvm-ar"
clang --version

function Fetch([string]$Dir, [string]$Url, [string]$Tag, [string]$Commit) {
    if (-not (Test-Path (Join-Path $Dir ".git"))) {
        git clone --quiet --depth 1 --branch $Tag $Url $Dir
    }
    $got = git -C $Dir rev-parse HEAD
    if ($got -ne $Commit) { throw "$Url $Tag is $got, expected $Commit" }
}

$src = Join-Path $Work "mpv"
Fetch $src "https://github.com/mpv-player/mpv.git" $MpvTag $MpvCommit
$sub = Join-Path $src "subprojects"
New-Item -ItemType Directory -Force -Path $sub | Out-Null
Copy-Item (Join-Path $here "subprojects-windows\*.wrap") $sub -Force

# shaderc (GLSL to SPIR-V, for Direct3D 11) through meson's CMake module, as
# mpv's CI does it: its git-sync-deps pins glslang, SPIRV-Tools and SPIRV-Headers.
$shadercSrc = Join-Path $sub "shaderc_cmake"
Fetch $shadercSrc "https://github.com/google/shaderc.git" $ShadercTag $ShadercCommit
$tp = Join-Path $shadercSrc "third_party\CMakeLists.txt"
$cm = Get-Content -Raw $tp
$expr = 'set(GLSLANG_ENABLE_INSTALL $<NOT:${SKIP_GLSLANG_INSTALL}>)'
if ($cm.Contains($expr)) {
    # Meson's CMake interpreter can't evaluate this generator expression.
    $cm = $cm.Replace($expr, "if (SKIP_GLSLANG_INSTALL)`n      set(GLSLANG_ENABLE_INSTALL OFF)`n    else()`n      set(GLSLANG_ENABLE_INSTALL ON)`n    endif()")
    Set-Content -NoNewline -Path $tp -Value $cm
}
if (-not (Test-Path (Join-Path $shadercSrc "third_party\glslang"))) {
    python (Join-Path $shadercSrc "utils\git-sync-deps")
}
# SPIRV-Tools generates build-version.inc in a CMake custom target, which
# meson's CMake module doesn't run. Generate it up front; the SPIRV-Tools
# source root is on the include path.
$spvTools = Join-Path $shadercSrc "third_party\spirv-tools"
python (Join-Path $spvTools "utils\update_build_version.py") (Join-Path $spvTools "CHANGES") (Join-Path $spvTools "build-version.inc")
New-Item -ItemType Directory -Force -Path (Join-Path $sub "shaderc") | Out-Null
Set-Content -Path (Join-Path $sub "shaderc\meson.build") -Value @"
project('shaderc', 'cpp', version: '2026.4')
cmake = import('cmake')
opts = cmake.subproject_options()
opts.add_cmake_defines({
    'CMAKE_MSVC_RUNTIME_LIBRARY': 'MultiThreaded',
    'CMAKE_POLICY_DEFAULT_CMP0091': 'NEW',
    'SHADERC_SKIP_INSTALL': 'ON',
    'SHADERC_SKIP_TESTS': 'ON',
    'SHADERC_SKIP_EXAMPLES': 'ON',
    'SHADERC_SKIP_COPYRIGHT_CHECK': 'ON',
    'SPIRV_SKIP_TESTS': 'ON',
    'SPIRV_SKIP_EXECUTABLES': 'ON',
    'ENABLE_GLSLANG_BINARIES': 'OFF',
})
shaderc_proj = cmake.subproject('shaderc_cmake', options: opts)
shaderc_dep = declare_dependency(dependencies: [
    shaderc_proj.dependency('shaderc'),
    shaderc_proj.dependency('shaderc_util'),
    shaderc_proj.dependency('SPIRV-Tools-static'),
    shaderc_proj.dependency('SPIRV-Tools-opt'),
    shaderc_proj.dependency('glslang'),
])
meson.override_dependency('shaderc', shaderc_dep)
"@

# SPIRV-Cross (SPIR-V to HLSL), static, through the CMake module.
New-Item -ItemType Directory -Force -Path (Join-Path $sub "spirv-cross-c-shared") | Out-Null
Set-Content -Path (Join-Path $sub "spirv-cross-c-shared\meson.build") -Value @"
project('spirv-cross', 'cpp', version: '1.4.363')
cmake = import('cmake')
opts = cmake.subproject_options()
opts.add_cmake_defines({
    'CMAKE_MSVC_RUNTIME_LIBRARY': 'MultiThreaded',
    'CMAKE_POLICY_DEFAULT_CMP0091': 'NEW',
    'SPIRV_CROSS_EXCEPTIONS_TO_ASSERTIONS': 'ON',
    'SPIRV_CROSS_CLI': 'OFF',
    'SPIRV_CROSS_ENABLE_TESTS': 'OFF',
    'SPIRV_CROSS_ENABLE_MSL': 'OFF',
    'SPIRV_CROSS_ENABLE_CPP': 'OFF',
    'SPIRV_CROSS_ENABLE_REFLECT': 'OFF',
    'SPIRV_CROSS_ENABLE_UTIL': 'OFF',
})
spirv_cross_proj = cmake.subproject('spirv-cross', options: opts)
spirv_cross_c_dep = declare_dependency(dependencies: [
    spirv_cross_proj.dependency('spirv-cross-c'),
    spirv_cross_proj.dependency('spirv-cross-core'),
    spirv_cross_proj.dependency('spirv-cross-glsl'),
    spirv_cross_proj.dependency('spirv-cross-hlsl'),
])
meson.override_dependency('spirv-cross-c-shared', spirv_cross_c_dep)
"@

$build = Join-Path $Work "build"
$opts = @(
    "--wrap-mode=forcefallback", "--buildtype=release", "-Ddefault_library=static", "-Db_vscrt=mt",
    # mpv: libmpv only, LGPL, Direct3D 11 output, nothing that phones home or shows media controls.
    "-Dlibmpv=true", "-Dcplayer=true", "-Dgpl=false", "-Dtests=false",
    "-Dd3d11=enabled", "-Dshaderc=enabled", "-Dspirv-cross=enabled", "-Dvulkan=disabled",
    "-Dlua=disabled", "-Djavascript=disabled", "-Dlcms2=disabled", "-Diconv=disabled", "-Duchardet=disabled",
    "-Dlibarchive=disabled", "-Drubberband=disabled", "-Dvapoursynth=disabled", "-Dzimg=disabled",
    "-Djpeg=disabled", "-Dcdda=disabled", "-Ddvdnav=disabled", "-Dlibbluray=disabled", "-Dlibavdevice=disabled",
    "-Dwin32-smtc=disabled", "-Dzlib=enabled",
    "-Dmanpage-build=disabled", "-Dhtml-build=disabled", "-Dpdf-build=disabled",
    # FFmpeg: decoding only, LGPL.
    "-Dffmpeg:gpl=disabled", "-Dffmpeg:version3=disabled", "-Dffmpeg:nonfree=disabled",
    "-Dffmpeg:programs=disabled", "-Dffmpeg:tests=disabled", "-Dffmpeg:avdevice=disabled",
    "-Dffmpeg:postproc=disabled", "-Dffmpeg:network=disabled", "-Dffmpeg:muxers=disabled",
    "-Dffmpeg:encoders=disabled", "-Dffmpeg:png_encoder=enabled", "-Dffmpeg:mjpeg_encoder=enabled",
    "-Dffmpeg:libdav1d=enabled", "-Dffmpeg:zlib=enabled", "-Dffmpeg:vulkan=disabled",
    "-Dffmpeg:d3d11va=enabled", "-Dffmpeg:dxva2=enabled",
    # libplacebo: Direct3D 11 and OpenGL, no colour management (lcms2).
    "-Dlibplacebo:demos=false", "-Dlibplacebo:tests=false", "-Dlibplacebo:vulkan=disabled",
    "-Dlibplacebo:d3d11=enabled", "-Dlibplacebo:shaderc=enabled", "-Dlibplacebo:lcms=disabled",
    # libass with DirectWrite for system fonts.
    "-Dlibass:fontconfig=disabled", "-Dlibass:directwrite=enabled", "-Dlibass:test=disabled",
    "-Dlibass:compare=disabled", "-Dlibass:profile=disabled", "-Dlibass:fuzz=disabled",
    "-Dlibass:checkasm=disabled", "-Dlibass:libunibreak=disabled",
    "-Dharfbuzz:tests=disabled", "-Dharfbuzz:docs=disabled", "-Dharfbuzz:utilities=disabled",
    "-Dharfbuzz:glib=disabled", "-Dharfbuzz:gobject=disabled", "-Dharfbuzz:cairo=disabled",
    "-Dharfbuzz:icu=disabled", "-Dharfbuzz:freetype=enabled",
    "-Dfreetype2:harfbuzz=disabled", "-Dfreetype2:png=disabled", "-Dfreetype2:brotli=disabled",
    "-Dfreetype2:bzip2=disabled", "-Dfreetype2:zlib=disabled",
    "-Dfribidi:docs=false", "-Dfribidi:tests=false", "-Dfribidi:bin=false"
)
if (Test-Path (Join-Path $build "build.ninja")) {
    meson setup --reconfigure $build $src @opts
} else {
    meson setup $build $src @opts
}
# mpv.exe is built too: its link line lists every library libmpv needs.
ninja -C $build libmpv.a mpv.exe

# Licence checks.
$ffconfig = Get-ChildItem -Recurse -Path (Join-Path $build "subprojects") -Filter config.h |
    Where-Object { Select-String -Quiet -Path $_.FullName -Pattern "FFMPEG_LICENSE" } | Select-Object -First 1
if (-not $ffconfig) { throw "FFmpeg's config.h not found" }
if (-not (Select-String -Quiet -Path $ffconfig.FullName -Pattern '#define FFMPEG_LICENSE "LGPL version 2.1 or later"')) {
    throw "FFmpeg was not configured as LGPL 2.1 or later"
}
if (Select-String -Quiet -Path (Join-Path $build "config.h") -Pattern "#define HAVE_GPL 1") {
    throw "mpv was configured as GPL"
}

# The link list, from mpv.exe's link line in build.ninja: every static archive
# it links (copied to $Out, libmpv's own objects as libmpv.a) and every
# Windows import library.
Remove-Item -Recurse -Force (Join-Path $Out "*") -ErrorAction SilentlyContinue
$ninja = Get-Content -Raw (Join-Path $build "build.ninja")
$m = [regex]::Match($ninja, '(?ms)^build mpv\.exe: \S+ (.*?)\n(?:\s+\w+ = .*\n)*?\s+LINK_ARGS = (.*?)\n')
if (-not $m.Success) { throw "mpv.exe's link line not found in build.ninja" }
$inputs = ($m.Groups[1].Value -replace '\$\s*\n', ' ') -split '\s+' | Where-Object { $_ }
$linkArgs = ($m.Groups[2].Value) -split '\s+' | Where-Object { $_ }
$lines = New-Object System.Collections.Generic.List[string]
# Meson writes subprojects' internal libraries as thin archives (references to
# object files), which link.exe can't read. Those are rebuilt as regular
# archives from the objects they name; regular ones are copied as they are.
# (llvm-lib can convert them too, but crashed on SPIRV-Tools.)
function Copy-Archive([string]$From, [string]$To) {
    $head = [System.Text.Encoding]::ASCII.GetString([byte[]](Get-Content -AsByteStream -TotalCount 8 $From))
    if ($head -ne "!<thin>`n") {
        Copy-Item $From $To
        return
    }
    # llvm-ar lists a thin archive's members relative to the current directory.
    $cwd = (Get-Location).Path
    $rsp = "$To.rsp"
    llvm-ar t $From | ForEach-Object { '"' + [IO.Path]::GetFullPath($_, $cwd) + '"' } | Set-Content -Path $rsp
    Remove-Item -ErrorAction SilentlyContinue $To
    llvm-ar qcs $To "@$rsp"
    Remove-Item $rsp
}
Copy-Archive (Join-Path $build "libmpv.a") (Join-Path $Out "libmpv.a")
$lines.Add("static=libmpv.a")
$taken = @{ "libmpv.a" = $true }
$seen = @{ ([IO.Path]::GetFullPath((Join-Path $build "libmpv.a"))) = $true }
foreach ($tok in @($inputs + $linkArgs)) {
    $t = $tok.Trim('"') -replace '^\|+', ''
    if ($t -match '\.(a|lib)$' -and (Test-Path (Join-Path $build $t))) {
        $full = [IO.Path]::GetFullPath((Join-Path $build $t))
        if ($seen.ContainsKey($full)) { continue }
        $seen[$full] = $true
        $name = Split-Path -Leaf $t
        if ($taken.ContainsKey($name)) {
            $i = 2
            while ($taken.ContainsKey("$i-$name")) { $i++ }
            $name = "$i-$name"
        }
        $taken[$name] = $true
        Copy-Archive $full (Join-Path $Out $name)
        $lines.Add("static=$name")
    } elseif ($t -match '^-l(.+)$') {
        $lines.Add("dylib=$($Matches[1])")
    } elseif ($t -match '^([\w-]+)\.lib$') {
        $lines.Add("dylib=$($Matches[1])")
    }
}
$lines = $lines | Select-Object -Unique
Set-Content -Path (Join-Path $Out "link.txt") -Value $lines

$gplOnly = 'rubberband|x264|x265|xvidcore|postproc|cdio|dvdnav|dvdread|smbclient|fdk-aac'
$bad = $lines | Where-Object { $_ -match "^(static|dylib)=(\d+-)?(lib)?($gplOnly)" }
if ($bad) { throw "GPL-only or non-free libraries are linked: $bad" }

# Licences, sources and the source archive (LGPL source offer).
$lic = Join-Path $Out "licenses"
New-Item -ItemType Directory -Force -Path $lic | Out-Null
Copy-Item (Join-Path $src "LICENSE.LGPL") (Join-Path $lic "mpv-LICENSE.LGPL")
Get-ChildItem -Directory $sub | ForEach-Object {
    foreach ($f in @("COPYING", "COPYING.LGPLv2.1", "LICENSE", "LICENSE.txt", "LICENSE.md", "COPYING.LIB", "docs\FTL.TXT")) {
        $p = Join-Path $_.FullName $f
        if (Test-Path $p) { Copy-Item $p (Join-Path $lic "$($_.Name)-$(Split-Path -Leaf $f)") }
    }
}
$sources = @("Static LGPL libmpv for Cinasafe (Windows $arch, clang/MSVC ABI), built by third_party/mpv/build-windows.ps1",
    "mpv     $MpvTag $MpvCommit https://github.com/mpv-player/mpv",
    "shaderc $ShadercTag $ShadercCommit https://github.com/google/shaderc",
    "", "Subprojects (third_party/mpv/subprojects-windows):")
foreach ($w in Get-ChildItem (Join-Path $here "subprojects-windows\*.wrap")) {
    $text = Get-Content -Raw $w.FullName
    $url = [regex]::Match($text, '(?m)^(url|source_url) = (.*)$').Groups[2].Value
    $rev = [regex]::Match($text, '(?m)^(revision|source_hash) = (.*)$').Groups[2].Value
    $sources += "  $($w.BaseName): $url $rev"
}
$sources += @("", "Source code: libmpv-sources-windows-$arch.tar.gz next to this build.",
    "The app links these statically; its own source (to relink against a modified",
    "library, LGPL-2.1 section 6) is at https://github.com/yuvalkolodkingal/cinasafe.")
Set-Content -Path (Join-Path $Out "SOURCES.txt") -Value $sources
tar -czf (Join-Path (Split-Path $Out) "libmpv-sources-windows-$arch.tar.gz") --exclude=.git --exclude=packagecache `
    -C $Work mpv -C $here build-windows.ps1 subprojects-windows

Write-Host "Static libmpv (LGPL) is in ${Out}:"
Get-Content (Join-Path $Out "link.txt")
