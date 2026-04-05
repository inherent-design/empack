$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '../..')).Path

Set-Location $repoRoot

Write-Host "+ cargo build --release -p empack"
cargo build --release -p empack

$env:EMPACK_E2E_BIN = Join-Path $repoRoot 'target\release\empack.exe'

$filter = if ($args.Count -gt 0) { $args[0] } else { 'e2e_' }

Write-Host "+ cargo nextest run -p empack-tests -E 'test(~$filter)'"
cargo nextest run -p empack-tests -E "test(~$filter)"
