# Send the working tree to the build server and run the tests there.
#
#   .\scripts\remote.ps1 -HostName canlab.example -User oskuh -RemotePath /home/oskuh/canlab
#   .\scripts\remote.ps1 -HostName ... -Filter "-p canlab-core -p canlab-solver"
#   .\scripts\remote.ps1 -HostName ... -Bench
#
# See scripts/remote.md for why this is a tarball and not a clone or rsync.
#
# What it does, in order, and nothing else:
#   1. packs the working tree (no target/, no .git/) into one tar.gz
#   2. copies it to <RemotePath>.tar.gz and extracts it into <RemotePath>
#   3. runs `cargo test` there with the filter you passed
#
# Every step is an ssh/scp call, which needs network access. One approval per
# run, not one per command.

param(
    [Parameter(Mandatory = $true)][string]$HostName,
    [string]$User = $env:USERNAME,
    [Parameter(Mandatory = $true)][string]$RemotePath,
    [string]$Filter = "--workspace",
    [switch]$Bench,
    [switch]$KeepTarball
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = "$User@$HostName"
$tarball = Join-Path $env:TEMP "canlab-remote.tar.gz"

Write-Host "packing $root ..." -ForegroundColor Cyan
Push-Location $root
try {
    # `tar` ships with Windows (bsdtar). Exclude the build directory, the git
    # database and the editor config: the server needs sources, not history.
    tar -czf $tarball --exclude=target --exclude=./target --exclude=.git --exclude=.obsidian .
    if ($LASTEXITCODE -ne 0) { throw "tar failed with $LASTEXITCODE" }
} finally {
    Pop-Location
}
$size = [math]::Round((Get-Item $tarball).Length / 1MB, 2)
Write-Host "packed: $size MiB" -ForegroundColor Cyan

Write-Host "copying to ${target}:$RemotePath.tar.gz ..." -ForegroundColor Cyan
scp $tarball "${target}:$RemotePath.tar.gz"
if ($LASTEXITCODE -ne 0) { throw "scp failed with $LASTEXITCODE" }

Write-Host "extracting and testing on the server ..." -ForegroundColor Cyan
$commands = @(
    "mkdir -p '$RemotePath'"
    "tar -xzf '$RemotePath.tar.gz' -C '$RemotePath'"
    "cd '$RemotePath'"
    ". `"`$HOME/.cargo/env`""
    "cargo test $Filter --offline -j 8"
)
if ($Bench) {
    $commands += "cargo run -p canlab-cli --release --offline -- bench --write"
}
$remote = ($commands -join " && ")
ssh $target $remote
$code = $LASTEXITCODE

if (-not $KeepTarball) { Remove-Item $tarball -ErrorAction SilentlyContinue }
if ($code -ne 0) {
    Write-Host "remote command failed with $code" -ForegroundColor Red
    exit $code
}
Write-Host "remote tests passed" -ForegroundColor Green
