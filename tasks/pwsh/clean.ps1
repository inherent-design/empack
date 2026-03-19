$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '../..')).Path

Set-Location $repoRoot

Write-Host '+ cargo clean'
& cargo clean