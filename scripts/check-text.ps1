<#
.SYNOPSIS
    Fail when a tracked text file contains a control character.

.DESCRIPTION
    Twice now a letter has disappeared into a control code because a file was
    written through a shell string where `\a`, `\v` or `\0` is an escape:

      * `main.rs` once held a NUL and read as binary to every tool that touched it;
      * four prose characters in `lessons.md` and `native.rs` became U+0007 and
        U+000B, so a paragraph said "wrong about " and a comment said "in ar".

    No compiler sees this: comments are free text and Markdown is not compiled.
    This script is the reading that catches it. Run it before a commit; it exits
    1 and names every offending file, or 0 when the tree is clean.

.EXAMPLE
    .\scripts\check-text.ps1
#>
[CmdletBinding()]
param(
    # Where to look. Defaults to the repository root, i.e. the parent of scripts/.
    [string] $Root = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'

# Tab, newline and carriage return are legitimate; everything below 0x20 is not.
$forbidden = [regex]'[\x00-\x08\x0B\x0C\x0E-\x1F]'
$extensions = @('*.md', '*.rs', '*.toml', '*.ps1', '*.json', '*.txt', '*.gitignore')

$files = Get-ChildItem -Path $Root -Recurse -File -Include $extensions -Force |
    Where-Object { $_.FullName -notmatch '\\(target|\.git|node_modules)\\' }

$offenders = 0
foreach ($file in $files) {
    $text = [System.IO.File]::ReadAllText($file.FullName)
    $matches = $forbidden.Matches($text)
    if ($matches.Count -eq 0) {
        continue
    }
    $offenders++
    $line = 1
    $lineStart = 0
    foreach ($match in $matches) {
        # Count the lines before this match so the report points at a line.
        $segment = $text.Substring($lineStart, $match.Index - $lineStart)
        $line += ($segment -split "`n").Count - 1
        $lineStart = $match.Index
        $code = [int][char]$match.Value
        Write-Output ("{0}:{1}: control character U+{2:X4}" -f $file.FullName, $line, $code)
    }
}

if ($offenders -gt 0) {
    Write-Output ("{0} file(s) contain control characters; edit them with the file tools, not a shell string." -f $offenders)
    exit 1
}

Write-Output ("{0} text file(s) checked: no control characters." -f $files.Count)
exit 0
