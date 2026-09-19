# SAP Login Manager (GPUI) build script
# Usage: .\build.ps1 [check|build|run] [debug|release]
#
# Toolchain strategy: prefer rust-dev (C:\Users\<user>\rust-dev);
# when it is blocked by AV policy (os error 5 / access denied),
# automatically retry with the Temp toolchain (%LOCALAPPDATA%\Temp\rust).

param(
    [Parameter(Position=0)]
    [ValidateSet('check','build','run')]
    [string]$Action = 'check',

    [Parameter(Position=1)]
    [ValidateSet('debug','release')]
    [string]$Mode = 'debug'
)

function Get-Cargo([string]$root) {
    $env:RUSTUP_HOME = Join-Path $root 'rustup'
    $env:CARGO_HOME  = Join-Path $root 'cargo'
    return (Join-Path $root 'cargo\bin\cargo.exe')
}

$rustDev  = 'C:\Users\Yuqing.Zhang\rust-dev'
$tempRust = Join-Path $env:LOCALAPPDATA 'Temp\rust'

$cargo = $null
if (Test-Path (Join-Path $rustDev 'cargo\bin\cargo.exe')) {
    $cargo = Get-Cargo $rustDev
} elseif (Test-Path (Join-Path $tempRust 'cargo\bin\cargo.exe')) {
    $cargo = Get-Cargo $tempRust
} else {
    Write-Host 'ERROR: no cargo found (neither rust-dev nor Temp toolchain)' -ForegroundColor Red
    exit 1
}

$cargoArgs = @($Action, '-p', 'sap-login-manager-gpui')
if ($Mode -eq 'release') { $cargoArgs += @('--release') }

Write-Host "Rust toolchain: $((& $cargo --version) 2>&1)" -ForegroundColor Cyan
Write-Host "RUSTUP_HOME = $env:RUSTUP_HOME"
Write-Host "CARGO_HOME  = $env:CARGO_HOME"
Write-Host "Workdir     = $PSScriptRoot"
Write-Host "Command     : cargo $($cargoArgs -join ' ')" -ForegroundColor Green
Write-Host '---'

Set-Location $PSScriptRoot
& $cargo @cargoArgs
$exitCode = $LASTEXITCODE

# rust-dev blocked by AV? retry once with the Temp toolchain
if ($exitCode -ne 0 -and (Test-Path (Join-Path $tempRust 'cargo\bin\cargo.exe'))) {
    Write-Host ''
    Write-Host 'Build failed - retrying with Temp toolchain ...' -ForegroundColor Yellow
    $cargo = Get-Cargo $tempRust
    & $cargo @cargoArgs
    $exitCode = $LASTEXITCODE
}

Write-Host ''
if ($exitCode -eq 0) {
    $exe = 'target\release\sap-login-manager-gpui.exe'
    if ($Mode -eq 'release' -and $Action -ne 'check' -and (Test-Path $exe)) {
        $info = Get-Item $exe
        Write-Host "BUILD OK: $($info.Name), $([math]::Round($info.Length/1MB,1)) MB, $($info.LastWriteTime)" -ForegroundColor Green
    } else {
        Write-Host 'BUILD OK' -ForegroundColor Green
    }
} else {
    Write-Host "BUILD FAILED (exit $exitCode)" -ForegroundColor Red
    Write-Host 'If the cargo registry cache is corrupted, remove %CARGO_HOME%\registry\src and retry.' -ForegroundColor Yellow
}
exit $exitCode
