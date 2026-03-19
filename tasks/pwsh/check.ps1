$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '../..')).Path

if (-not $env:CHECK_MODE) { $env:CHECK_MODE = 'check' }

Set-Location $repoRoot

$cargoArgs = @()

switch ($env:CHECK_MODE) {
    'check' {
        $cargoArgs = @('check', '--workspace', '--all-targets')
    }
    'clippy' {
        $cargoArgs = @('clippy', '--workspace', '--all-targets')
    }
    default {
        throw "Unsupported CHECK_MODE '$($env:CHECK_MODE)'. Expected: check or clippy."
    }
}

Write-Host "+ cargo $($cargoArgs -join ' ')"
& cargo @cargoArgs