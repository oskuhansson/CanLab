# The build server, as one command.
#
# The machine this repository is developed on runs with `-j 1`: a clean Bevy
# build needs more memory than it has, and `target/` is already 42 GiB. The
# server on the tailnet (the `ai` node) has the cores and the memory, so the
# expensive work belongs there:
#
#   .\scripts\remote.ps1 status                     # toolchains, cores, RAM, disk
#   .\scripts\remote.ps1 test -p canlab-solver -p canlab-poly
#   .\scripts\remote.ps1 build                      # canlab-gui.exe via cargo-xwin
#   .\scripts\remote.ps1 snapshots                  # the GUI frames, rendered there
#   .\scripts\remote.ps1 setup                      # rustup target + cargo-xwin
#
# Configuration, in this order: an explicit -Remote, else $env:CANLAB_REMOTE,
# else `oskar@100.88.227.20:2220` (the tailnet IP; `ai.ragnarhansson.com` does
# not resolve to a reachable address from every network). The checkout is
# `~/canlab-remote` and the build directory `~/canlab-target` is *shared*, so a
# second run is incremental - that is the point of the exercise.
#
# The transport is plain `ssh`/`scp` with the key in `~/.ssh/id_ed25519`. To
# authorise it once, from this machine and with the server's password:
#
#   ssh-copy-id -p 2220 -i ~/.ssh/id_ed25519.pub oskar@100.88.227.20

[CmdletBinding()]
param(
    # What to do. See the header for the four actions.
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('status', 'test', 'build', 'snapshots', 'setup')]
    [string]$Action,

    # Cargo package selection for `test`: -p canlab-core -p canlab-solver.
    [string[]]$Package = @(),

    # `test` runs release by default on the server: it is slower to compile once
    # and much faster to run, and the server is not the machine anybody waits on.
    [switch]$DebugBuild,

    # How many compile jobs. The server has the memory for the real number.
    [int]$Jobs = 0,

    # `user@host:port`, or `user@host`.
    [string]$Remote = $(if ($env:CANLAB_REMOTE) { $env:CANLAB_REMOTE } else { 'oskar@100.88.227.20:2220' }),

    # Where the checkout lives on the server.
    [string]$RemotePath = '',

    # Where the shared build directory lives on the server.
    [string]$RemoteTarget = '',

    # Where a built artifact lands here.
    [string]$Out = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# ---------------------------------------------------------------- the address

$user = ''
$hostName = $Remote
$port = 22
if ($Remote -match '^([^@]+)@(.+?):(\d+)$') {
    $user, $hostName, $port = $Matches[1], $Matches[2], $Matches[3]
} elseif ($Remote -match '^([^@]+)@(.+)$') {
    $user, $hostName = $Matches[1], $Matches[2]
} elseif ($Remote -match '^(.+?):(\d+)$') {
    $hostName, $port = $Matches[1], $Matches[2]
}
if (-not $user) { throw "no user in -Remote '$Remote'; expected user@host:port" }
$target = "$user@$hostName"
if (-not $RemotePath) { $RemotePath = "/home/$user/canlab-remote" }
if (-not $RemoteTarget) { $RemoteTarget = "/home/$user/canlab-target" }
$sshBase = @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10',
    '-o', 'StrictHostKeyChecking=accept-new', '-p', "$port")

# Prefer the dedicated build key that `authorise-key.ps1` creates: revoking it
# never touches the key you log in with by hand.
$identity = "$env:USERPROFILE\.ssh\id_canlab"
$ioArgs = @()
if (Test-Path $identity) {
    $ioArgs = @('-i', $identity)
    $sshBase += $ioArgs
}

function Invoke-Remote {
    param([string]$Script, [switch]$AllowFailure)
    Write-Verbose "remote: $Script"
    # `2>&1` must wrap the whole command and be applied *on the server*: cargo
    # writes progress to stderr, and PowerShell turns any stderr line from a
    # native command into an error record - so a working build reported exit 1.
    # Appending the redirect instead of wrapping it would bind to the last command
    # of an `a && b && c` chain and leave the rest unredirected.
    & ssh @sshBase $target "{ $Script ; } 2>&1"
    $code = $LASTEXITCODE
    if ($code -ne 0 -and -not $AllowFailure) {
        throw "remote command failed with $code"
    }
    return $code
}

function Sync-Tree {
    # The server builds sources, not history: no `target/`, no `.git/`, no editor
    # state. A tarball rather than a clone because the tree may hold work that is
    # not committed yet, and because a clone would need the server to authenticate
    # to GitHub for a private repository.
    $stamp = [guid]::NewGuid().ToString('N').Substring(0, 12)
    $remoteTarball = "$RemotePath-$stamp.tar.gz"
    $localTarball = Join-Path ([System.IO.Path]::GetTempPath()) "canlab-$stamp.tar.gz"
    Write-Host "packing the tree ..." -ForegroundColor Cyan
    Push-Location $root
    try {
        # `git archive`, not `tar`. The archive comes from the commit, so it holds
        # only tracked files, carries git's own file modes, and has none of
        # bsdtar's `SCHILY.fflags` extended attributes - which is what made the
        # server refuse some paths with "Cannot mkdir: Permission denied". It also
        # means the server builds a named commit rather than whatever the tree
        # happened to hold, so a failing remote run can be reproduced.
        & git archive --format=tar.gz --output=$localTarball HEAD
        if ($LASTEXITCODE -ne 0) { throw "git archive failed with $LASTEXITCODE" }
        $dirty = (& git status --porcelain) -join "`n"
        if ($dirty) {
            Write-Host "warning: the tree is dirty; only the commit is sent" -ForegroundColor Yellow
        }
    } finally {
        Pop-Location
    }
    $size = [math]::Round((Get-Item $localTarball).Length / 1MB, 2)
    Write-Host "copying $size MiB to ${target}:$remoteTarball ..." -ForegroundColor Cyan
    & scp @ioArgs -o BatchMode=yes -o ConnectTimeout=10 -P "$port" $localTarball "${target}:$remoteTarball"
    if ($LASTEXITCODE -ne 0) { throw "scp failed with $LASTEXITCODE" }
    Remove-Item $localTarball -ErrorAction SilentlyContinue
    $unpack = @(
        "set -e"
        "mkdir -p '$RemotePath'"
        # A previous archive left the checkout root mode 0555, so the next
        # extraction could not create a file inside it ("Cannot open: Permission
        # denied", on .cargo/config.toml because it sorts first). Make the tree
        # writable before unpacking, and do not let the archive's own modes make
        # it read-only again.
        "chmod -R u+w '$RemotePath' 2>/dev/null || true"
        "tar --no-same-permissions --no-same-owner -xzf '$remoteTarball' -C '$RemotePath'"
        "rm -f '$remoteTarball'"
    ) -join ' && '
    Invoke-Remote $unpack | Out-Null
    Write-Host "the server has the tree at $RemotePath" -ForegroundColor Cyan
}

# The environment every cargo call needs: rustup's PATH, and one shared target
# directory so the second run reuses the first one's objects.
$cargoPrelude = @(
    "set -e"
    # `~/.cargo/env` does not exist on every install, and the server's `$HOME`
    # arrives wrong through this shell, so the cargo bin directory is added by
    # absolute path from the login name. Without it `cargo xwin` is "missing" in
    # one probe and found in another, which is a failure waiting to happen.
    "export PATH=`"/home/`$(id -un)/.cargo/bin:`$PATH`""
    ". `"`$HOME/.cargo/env`" 2>/dev/null || true"
    "export CARGO_TARGET_DIR='$RemoteTarget'"
    "cd '$RemotePath'"
) -join ' && '

# ------------------------------------------------------------------- the actions

switch ($Action) {
    'status' {
        $script = @(
            "export PATH=`"/home/`$(id -un)/.cargo/bin:`$PATH`""
            "uname -srm"
            "nproc"
            "free -g | awk 'NR==1 || NR==2'"
            "df -h `$HOME | tail -1"
            ". `"`$HOME/.cargo/env`" 2>/dev/null || true"
            "cargo --version"
            "rustc --version"
            "rustup target list --installed"
            "command -v cargo-xwin || echo 'cargo-xwin: missing'"
            "nvidia-smi --query-gpu=name,memory.total --format=csv,noheader 2>/dev/null || echo 'nvidia-smi: missing'"
        ) -join '; '
        Invoke-Remote $script -AllowFailure
        exit $LASTEXITCODE
    }

    'setup' {
        $script = @(
            "set -e"
            ". `"`$HOME/.cargo/env`""
            "rustup target add x86_64-pc-windows-msvc"
            "cargo xwin --version 2>/dev/null || cargo install cargo-xwin --locked"
        ) -join ' && '
        Invoke-Remote $script | Out-Null
    }

    'test' {
        Sync-Tree
        $packages = ($Package | ForEach-Object { "-p $_" }) -join ' '
        if (-not $packages) { throw "test needs at least one -Package, e.g. -Package canlab-solver" }
        $profile = if ($DebugBuild) { '' } else { ' --release' }
        $jobs = if ($Jobs -gt 0) { $Jobs } else { 8 }
        $script = "$cargoPrelude && cargo test $packages$profile -j $jobs"
        Invoke-Remote $script
        exit $LASTEXITCODE
    }

    'build' {
        Sync-Tree
        $jobs = if ($Jobs -gt 0) { $Jobs } else { 8 }
        $script = "$cargoPrelude && cargo xwin build -p canlab-gui --release --target x86_64-pc-windows-msvc -j $jobs"
        Invoke-Remote $script
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        if (-not $Out) { $Out = Join-Path $root 'target\remote' }
        New-Item -ItemType Directory -Force -Path $Out | Out-Null
        $artifact = "$RemoteTarget/x86_64-pc-windows-msvc/release/canlab-gui.exe"
        $local = Join-Path $Out 'canlab-gui.exe'
        Write-Host "copying the executable back to $local ..." -ForegroundColor Cyan
        & scp @ioArgs -o BatchMode=yes -o ConnectTimeout=10 -P "$port" "${target}:$artifact" $local
        if ($LASTEXITCODE -ne 0) { throw "the executable did not come back: check $artifact" }
        Write-Host ("built: {0:N1} MiB" -f ((Get-Item $local).Length / 1MB)) -ForegroundColor Green
    }

    'snapshots' {
        Sync-Tree
        $jobs = if ($Jobs -gt 0) { $Jobs } else { 8 }
        # The frames render headlessly through wgpu; UPDATE_SNAPSHOTS writes them
        # rather than comparing, because this machine's reference images were made
        # on a different adapter and a pixel diff across GPUs is not a bug.
        $script = "$cargoPrelude && UPDATE_SNAPSHOTS=1 cargo test -p canlab-gui --release -j $jobs snapshot"
        Invoke-Remote $script
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        if (-not $Out) { $Out = Join-Path $root 'target\remote\snapshots' }
        New-Item -ItemType Directory -Force -Path $Out | Out-Null
        Write-Host "copying the frames back to $Out ..." -ForegroundColor Cyan
        & scp @ioArgs -o BatchMode=yes -o ConnectTimeout=10 -P "$port" "${target}:$RemotePath/crates/canlab-gui/tests/snapshots/*.png" $Out
        if ($LASTEXITCODE -ne 0) { throw "no frames came back" }
        Get-ChildItem $Out -Filter *.png | ForEach-Object {
            Write-Host ("  {0} ({1:N0} KiB)" -f $_.Name, ($_.Length / 1KB))
        }
    }
}
