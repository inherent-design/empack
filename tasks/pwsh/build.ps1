$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '../..')).Path

if (-not $env:BUILD_MODE) { $env:BUILD_MODE = 'debug' }
if (-not $env:RUN_AFTER_BUILD) { $env:RUN_AFTER_BUILD = '0' }

$binPath = $null
$buildArgs = @('build')
$logLevel = $null

Set-Location $repoRoot

switch ($env:BUILD_MODE) {
    'debug' {
        $binPath = Join-Path $repoRoot 'target/debug/empack.exe'
        $logLevel = '3'
    }
    'profile' {
        $binPath = Join-Path $repoRoot 'target/release/empack.exe'
        $buildArgs += '--release'
        $logLevel = '2'
    }
    'release' {
        $binPath = Join-Path $repoRoot 'target/release/empack.exe'
        $buildArgs += '--release'
        $logLevel = '0'
    }
    default {
        throw "Unsupported BUILD_MODE '$($env:BUILD_MODE)'. Expected: debug, profile, release."
    }
}

Write-Host "+ cargo $($buildArgs -join ' ')"
& cargo @buildArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

switch ($env:RUN_AFTER_BUILD) {
    '0' {
    }
    '1' {
        $env:EMPACK_LOG_LEVEL = $logLevel
        Write-Host "+ EMPACK_LOG_LEVEL=$($env:EMPACK_LOG_LEVEL) & $binPath $($args -join ' ')"
        & $binPath @args
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    default {
        throw "Unsupported RUN_AFTER_BUILD '$($env:RUN_AFTER_BUILD)'. Expected: 0 or 1."
    }
}
