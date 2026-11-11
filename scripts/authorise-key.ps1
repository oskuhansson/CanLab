# Authorise the build key on the server, from Windows.
#
# Windows OpenSSH ships `ssh`, `scp` and `ssh-keygen` but **not** `ssh-copy-id`,
# so double-clicking it opens the "how do you want to open this file" dialog. This
# is what that command does, in three lines of PowerShell.
#
# It makes a **dedicated** key, `~/.ssh/id_canlab`, with the comment `canlab-agent`.
# Your own `id_ed25519` never goes to the server, so revoking never touches the
# access you rely on:
#
#   # on the server
#   sed -i.bak '/canlab-agent/d' ~/.ssh/authorized_keys
#
# Usage, from the repository root, and with the server's password to type once:
#
#   .\scripts\authorise-key.ps1

[CmdletBinding()]
param(
    [string]$Remote = $(if ($env:CANLAB_REMOTE) { $env:CANLAB_REMOTE } else { 'oskar@100.88.227.20:2220' }),
    [string]$Key = "$env:USERPROFILE\.ssh\id_canlab",
    [string]$Comment = 'canlab-agent'
)

$ErrorActionPreference = 'Stop'

$user = ''
$hostName = $Remote
$port = 22
if ($Remote -match '^([^@]+)@(.+?):(\d+)$') {
    $user, $hostName, $port = $Matches[1], $Matches[2], $Matches[3]
} elseif ($Remote -match '^([^@]+)@(.+)$') {
    $user, $hostName = $Matches[1], $Matches[2]
}
if (-not $user) { throw "no user in -Remote '$Remote'; expected user@host:port" }

if (-not (Test-Path "$Key.pub")) {
    Write-Host "creating $Key ..." -ForegroundColor Cyan
    # An empty passphrase: the key is a build credential on this machine, and a
    # passphrase the agent cannot type would only make the script prompt twice.
    & ssh-keygen -q -t ed25519 -f $Key -C $Comment -N '""'
    if ($LASTEXITCODE -ne 0) { throw "ssh-keygen failed with $LASTEXITCODE" }
}

$public = (Get-Content "$Key.pub" -Raw).Trim()
Write-Host "the key to authorise:" -ForegroundColor Cyan
Write-Host "  $public" -ForegroundColor DarkGray
Write-Host ""
Write-Host "connecting to ${user}@${hostName}:$port -- type the password once:" -ForegroundColor Cyan

# `mkdir`/`chmod` first, because sshd refuses a key file anyone else can write.
# The trailing `sort -u` keeps the file clean if this is run twice.
$remote = "mkdir -p ~/.ssh && chmod 700 ~/.ssh && cat >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && sort -u -o ~/.ssh/authorized_keys ~/.ssh/authorized_keys && echo added"
$public | & ssh -o StrictHostKeyChecking=accept-new -p $port "${user}@${hostName}" $remote
if ($LASTEXITCODE -ne 0) {
    throw @"
ssh failed with $LASTEXITCODE.
If the server does not accept a password at all, paste the line above into
  ~/.ssh/authorized_keys
on the server yourself; it is the same thing this script was about to do.
"@
}

Write-Host ""
Write-Host "done. Check it with:" -ForegroundColor Green
Write-Host "  ssh -o BatchMode=yes -i `"$Key`" -p $port ${user}@${hostName} 'uname -a'" -ForegroundColor DarkGray
Write-Host "and then run:" -ForegroundColor Green
Write-Host "  .\scripts\remote.ps1 status" -ForegroundColor DarkGray
