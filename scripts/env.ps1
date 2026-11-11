# Keep a build inside the workspace.
#
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass -Force
#   . .\scripts\env.ps1
#   cargo test -p canlab-core --offline -j 1
#
# The first line is needed because this machine's execution policy refuses an
# unsigned script, and a process-scoped override needs no administrator. It is a
# one-liner to paste, not a permanent setting.
#
# Why: the file sandbox allows writes under this workspace only. Cargo itself is
# fine there - an offline build already worked without it - but rustc and the
# test binaries inherit TMP and TEMP, and a test that writes to
# `std::env::temp_dir()` fails with "Access is denied". That is a property of the
# machine, not of the code, and it cost two red tests before anyone noticed.
#
# `CARGO_TARGET_TMPDIR` already keeps integration tests inside `target/`. This
# keeps everything else there too, and points the app's session file at the
# workspace so opening a window needs no approval either.
#
# A dependency addition is the one thing that still needs the network:
#   $env:CARGO_NET_OFFLINE = 'false'; cargo fetch
# and then set it back. Nothing else here reaches outside.

$root = Split-Path -Parent $PSScriptRoot
$tmp = Join-Path $root 'target\tmp'
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

$env:TMP = $tmp
$env:TEMP = $tmp
$env:CARGO_NET_OFFLINE = 'true'
$env:CANLAB_SESSION = Join-Path $tmp 'session.canlab'

$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ($env:PATH -notlike "*$cargoBin*") {
    $env:PATH = "$cargoBin;$env:PATH"
}

# `rust-lld` is not on `PATH`, and `.cargo/config.toml` points the linker at it.
# Same directory the toolchain keeps it in, for the same reason: linking is where
# a Bevy build spends its time.
$lld = Get-ChildItem (Join-Path $env:USERPROFILE '.rustup\toolchains') -Recurse `
    -Filter 'rust-lld.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($lld) {
    $lldDir = $lld.DirectoryName
    if ($env:PATH -notlike "*$lldDir*") {
        $env:PATH = "$lldDir;$env:PATH"
    }
}
