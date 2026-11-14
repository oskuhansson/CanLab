# CanLab builds, from the server to your screen.
#
# The build server cross-compiles the Windows executable (docs/remote.md), and the
# result sits in a cargo target directory on the server. This script is the short
# path from there to a running app: it finds the newest build, copies it when the
# local copy is older, records which commit it came from, and starts it.
#
#   .\scripts\canlab.ps1 status     what the server has, and what you have
#   .\scripts\canlab.ps1 get        copy the newest build here (never starts it)
#   .\scripts\canlab.ps1 run        get if stale, then start it   (the default)
#   .\scripts\canlab.ps1 build      ask the server to rebuild from the pushed commit, then get
#   .\scripts\canlab.ps1 where      the remote path only, for scripting
#
# Options: -Force (copy even if the local file looks current), -Wait (keep the
# terminal attached to the app's output instead of detaching it).
#
# The address comes from $env:CANLAB_REMOTE, else the tailnet default below; the
# key is the same build key `authorise-key.ps1` installs. A sidecar
# `canlab-gui.build.json` is written next to the executable so the app on screen
# can be traced back to a commit without starting it.

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('run', 'get', 'status', 'build', 'where')]
    [string]$Action = 'run',

    [switch]$Force,
    [switch]$Wait,

    [string]$Remote = $(if ($env:CANLAB_REMOTE) { $env:CANLAB_REMOTE } else { 'oskar@100.88.227.20:2220' })
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$cache = Join-Path $root 'target\remote'
$exe = Join-Path $cache 'canlab-gui.exe'
$sidecar = Join-Path $cache 'canlab-gui.build.json'

# ------------------------------------------------------------------ the address

$user = ''
$hostName = $Remote
$port = 22
if ($Remote -match '^([^@]+)@(.+?):(\d+)$') {
    $user, $hostName, $port = $Matches[1], $Matches[2], $Matches[3]
} elseif ($Remote -match '^([^@]+)@(.+)$') {
    $user, $hostName = $Matches[1], $Matches[2]
}
if (-not $user) { throw "no user in -Remote '$Remote'; expected user@host:port" }
$target = "$user@$hostName"

$key = "$env:USERPROFILE\.ssh\id_canlab"
$sshArgs = @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-p', "$port")
if (Test-Path $key) { $sshArgs += @('-i', $key) }

function Invoke-Remote {
    param([string]$Script, [switch]$AllowFailure)
    $output = & ssh @sshArgs $target "{ $Script ; } 2>&1"
    $code = $LASTEXITCODE
    if ($code -ne 0 -and -not $AllowFailure) {
        throw "remote command failed with $code`n$output"
    }
    return ($output | Out-String).Trim()
}

# ------------------------------------------------------------------ discovery

# Where a build can be. The build server keeps one shared target directory, but a
# session that builds in place leaves its own; both are searched and the newest
# file wins, so this tool never depends on which route produced the build.
$searchRoots = @(
    '/home/oskar/canlab-win-target',
    '/home/oskar/canlab-target',
    '/home/oskar/canlab-remote/target',
    '/home/oskar/Documents/deepseek-harness/default-workspace/cas/target'
)

function Find-RemoteBuild {
    $roots = ($searchRoots | ForEach-Object { "'$_'" }) -join ' '
    # `-printf` gives seconds since the epoch, so the newest is one sort away and
    # the answer needs no date parsing on either side.
    # One `find` over every candidate root: a `for` loop cannot be joined into a
    # single line with `;` (the `do` needs a newline), and missing roots only cost
    # a suppressed "No such file" on stderr.
    $script = "find $roots -maxdepth 6 -type f -name 'canlab-gui.exe' -printf '%T@ %s %p\n' 2>/dev/null | sort -nr | head -n1"
    $line = Invoke-Remote $script -AllowFailure
    if (-not $line) { return $null }
    $parts = $line -split ' ', 3
    if ($parts.Count -lt 3) { return $null }
    return [pscustomobject]@{
        Epoch = [double]$parts[0]
        Bytes = [long]$parts[1]
        Path  = $parts[2]
    }
}

function Get-RemoteCommit {
    param([string]$BuildPath)
    # The checkout that produced a build is the nearest `target` above it; the
    # tarball checkout `canlab-remote` has no `.git`, so it is only a last resort
    # and its failure is what "unknown" usually means.
    $candidates = @()
    if ($BuildPath -match '^(.*?)/target/') { $candidates += $Matches[1] }
    $candidates += '/home/oskar/Documents/deepseek-harness/default-workspace/cas'
    $candidates += '/home/oskar/canlab-remote'
    foreach ($checkout in $candidates) {
        $commit = Invoke-Remote "git -C '$checkout' rev-parse --short HEAD" -AllowFailure
        if ($commit -and $commit -match '^[0-9a-f]{7,}$') { return $commit.Trim() }
    }
    return 'unknown'
}

# When the commit a build probably came from was made. A build older than its own
# checkout is a build worth rebuilding, and saying so is more useful than printing
# a hash and leaving the reader to guess.
function Get-RemoteCommitTime {
    param([string]$BuildPath)
    if ($BuildPath -match '^(.*?)/target/') {
        $checkout = $Matches[1]
    } else {
        $checkout = '/home/oskar/Documents/deepseek-harness/default-workspace/cas'
    }
    $stamp = Invoke-Remote "git -C '$checkout' log -1 --format=%ct" -AllowFailure
    if ($stamp -match '^\d+$') {
        return [DateTimeOffset]::FromUnixTimeSeconds([long]$stamp).ToLocalTime()
    }
    return $null
}

# The build's own record, written beside it by `remote.ps1 build`. A hash read
# from the checkout is a guess about the past; this is the fact.
function Get-RemoteStamp {
    param([string]$BuildPath)
    $dir = $BuildPath -replace '/[^/]*$', ''
    $text = Invoke-Remote "cat '$dir/canlab-gui.stamp'" -AllowFailure
    if (-not $text -or $text -match 'No such file') { return $null }
    $record = @{}
    foreach ($line in ($text -split "`n")) {
        if ($line -match '^\s*([a-z_]+)=(.*)$') { $record[$Matches[1]] = $Matches[2].Trim() }
    }
    if (-not $record.ContainsKey('commit')) { return $null }
    return [pscustomobject]@{ commit = $record['commit']; built_at = $record['built_at'] }
}

function Show-Status {
    $build = Find-RemoteBuild
    if (-not $build) {
        Write-Host "no canlab-gui.exe found on ${target}" -ForegroundColor Yellow
        Write-Host "  looked under: $($searchRoots -join ', ')"
        Write-Host "  build one with: .\scripts\remote.ps1 build"
        return
    }
    $when = [DateTimeOffset]::FromUnixTimeSeconds([long]$build.Epoch).ToLocalTime()
    $stamp = Get-RemoteStamp -BuildPath $build.Path
    if ($stamp) {
        $commit = $stamp.commit
    } else {
        $commit = 'unknown (this build has no stamp)'
    }
    $commitTime = Get-RemoteCommitTime -BuildPath $build.Path
    Write-Host "server build" -ForegroundColor Cyan
    Write-Host ("  commit : {0}" -f $commit)
    Write-Host ("  built  : {0:yyyy-MM-dd HH:mm:ss}" -f $when)
    Write-Host ("  size   : {0:N1} MiB" -f ($build.Bytes / 1MB))
    Write-Host ("  path   : {0}:{1}" -f $target, $build.Path)
    if ($commitTime -and $when -lt $commitTime.AddMinutes(-1)) {
        Write-Host ("  behind : the checkout is at a commit from {0:yyyy-MM-dd HH:mm}; rebuild with .\scripts\remote.ps1 build" -f $commitTime) -ForegroundColor Yellow
    }

    Write-Host "local copy" -ForegroundColor Cyan
    if (-not (Test-Path $exe)) {
        Write-Host "  none yet (run .\scripts\canlab.ps1 get)"
        return
    }
    $local = Get-Item $exe
    Write-Host ("  file   : {0}  ({1:N1} MiB, {2:yyyy-MM-dd HH:mm:ss})" -f $local.FullName, ($local.Length / 1MB), $local.LastWriteTime)
    if (Test-Path $sidecar) {
        $record = Get-Content $sidecar -Raw | ConvertFrom-Json
        Write-Host ("  commit : {0}  (fetched {1})" -f $record.commit, $record.fetched_at)
        if ($commit -match "^unknown") {
            # The server build predates stamping; the local record cannot be
            # compared with anything, so say that instead of guessing.
            Write-Host "  provenance: the server build has no stamp, so its commit is unknown" -ForegroundColor Yellow
        } elseif ($record.commit -ne $commit) {
            Write-Host "  stale  : the server has a newer build ($commit)" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  commit : unknown (no sidecar)"
    }
}

# ------------------------------------------------------------------ actions

function Get-Build {
    param([switch]$Quiet)
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    $build = Find-RemoteBuild
    if (-not $build) {
        throw "no canlab-gui.exe on ${target}; run .\scripts\remote.ps1 build first"
    }
    $remoteWhen = [DateTimeOffset]::FromUnixTimeSeconds([long]$build.Epoch).ToLocalTime()
    $upToDate = $false
    if ((Test-Path $exe) -and -not $Force) {
        $local = Get-Item $exe
        $upToDate = ($local.Length -eq $build.Bytes) -and ([DateTimeOffset]$local.LastWriteTime -ge $remoteWhen.AddSeconds(-2))
    }
    if ($upToDate -and -not $Force) {
        # The file can be current while the record is missing (a copy made by hand,
        # or by an older version of this script); writing it then is what keeps
        # "which commit is on screen" answerable.
        if (-not (Test-Path $sidecar)) {
            $commit = Get-RemoteCommit -BuildPath $build.Path
            [pscustomobject]@{
                commit     = $commit
                built_at   = $remoteWhen.ToString('o')
                fetched_at = (Get-Date).ToString('o')
                source     = "${target}:$($build.Path)"
                bytes      = $build.Bytes
            } | ConvertTo-Json | Set-Content -Path $sidecar -Encoding utf8
        }
        if (-not $Quiet) { Write-Host "already current: $((Get-Item $exe).LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss'))" -ForegroundColor Green }
        return $exe
    }

    $stamp = Get-RemoteStamp -BuildPath $build.Path
    if ($stamp) {
        $commit = $stamp.commit
    } else {
        # No stamp: say so rather than label the binary with whatever the checkout
        # is at now, which is how a day-old build got a newer commit's name.
        $commit = Get-RemoteCommit -BuildPath $build.Path
    }
    if (-not $Quiet) {
        Write-Host ("fetching {0:N1} MiB from {1} ({2}) ..." -f ($build.Bytes / 1MB), $target, $commit) -ForegroundColor Cyan
    }
    $scpArgs = @('-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-P', "$port")
    if (Test-Path $key) { $scpArgs += @('-i', $key) }
    & scp @scpArgs "${target}:$($build.Path)" $exe
    if ($LASTEXITCODE -ne 0) { throw "scp failed with $LASTEXITCODE" }
    # Keep the server's timestamp: the next comparison is against the server, and a
    # local clock that is a second behind would re-download on every run.
    (Get-Item $exe).LastWriteTime = $remoteWhen.LocalDateTime
    $record = [pscustomobject]@{
        commit     = $commit
        built_at   = $remoteWhen.ToString('o')
        fetched_at = (Get-Date).ToString('o')
        source     = "${target}:$($build.Path)"
        bytes      = $build.Bytes
    }
    $record | ConvertTo-Json | Set-Content -Path $sidecar -Encoding utf8
    if (-not $Quiet) { Write-Host "fetched: $exe" -ForegroundColor Green }
    return $exe
}

switch ($Action) {
    'status' { Show-Status }
    'where' { $build = Find-RemoteBuild; if ($build) { $build.Path } else { exit 2 } }
    'get' { Get-Build | Out-Null }
    'build' {
        # "The latest build" means the newest *code*, and the newest code is the
        # server's checkout - which is where the other session works, commits and
        # (often) leaves uncommitted. Building the tarball of this machine's HEAD
        # would produce a binary from an older tree and call it current, which is
        # exactly the confusion this command exists to remove.
        $checkout = '/home/oskar/Documents/deepseek-harness/default-workspace/cas'
        $targetDir = '/home/oskar/canlab-win-target'
        # Single-quoted here-string: the remote shell's own `$(...)` and `$COMMIT`
        # must survive, and PowerShell has no backslash escapes - `\"` here is a
        # literal backslash followed by the quote that ends the string, which is a
        # parse error, not an escape.
        $template = @'
set -e
cd {checkout}
export PATH="/home/$(id -un)/.cargo/bin:$PATH"
export CARGO_TARGET_DIR={target}
cargo xwin build -p canlab-gui --release --target x86_64-pc-windows-msvc -j 8
COMMIT=$(git rev-parse --short HEAD)
if ! git diff-index --quiet HEAD --; then COMMIT="$COMMIT-dirty"; fi
mkdir -p {target}/x86_64-pc-windows-msvc/release
{ echo "commit=$COMMIT"; echo "built_at=$(date -u +%FT%TZ)"; } > {target}/x86_64-pc-windows-msvc/release/canlab-gui.stamp
echo "built $COMMIT"
'@
        $script = $template.Replace('{checkout}', $checkout).Replace('{target}', $targetDir)
        Write-Host "building on ${target} from $checkout (the long one on a cold tree)" -ForegroundColor Cyan
        Invoke-Remote $script
        Get-Build -Force | Out-Null
    }
    'run' {
        $path = Get-Build
        Get-Process canlab-gui -ErrorAction SilentlyContinue | Stop-Process -Force
        Start-Sleep -Milliseconds 400
        if ($Wait) {
            & $path
        } else {
            Start-Process -FilePath $path -WorkingDirectory $root | Out-Null
            Start-Sleep -Seconds 2
            Get-Process canlab-gui -ErrorAction SilentlyContinue | ForEach-Object {
                Write-Host ("running: pid {0}  {1}" -f $_.Id, $path) -ForegroundColor Green
            }
        }
    }
}
