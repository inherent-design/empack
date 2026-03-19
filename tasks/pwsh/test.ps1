$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '../..')).Path

if (-not $env:BUILD_MODE) { $env:BUILD_MODE = 'debug' }

Set-Location $repoRoot

$cargoArgs = @('nextest', 'run')

switch ($env:BUILD_MODE) {
    'debug' {
    }
    'profile' {
        $cargoArgs += '--release'
    }
    'release' {
        $cargoArgs += '--release'
    }
    default {
        throw "Unsupported BUILD_MODE '$($env:BUILD_MODE)'. Expected: debug, profile, release."
    }
}

$cargoArgs += @('-p', 'empack-lib', '--features', 'test-utils', '-p', 'empack-tests')

Write-Host "+ cargo $($cargoArgs -join ' ')"
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
