# Builds Teeth in release and its installer, C:\t\instalador\Teeth-Setup-<version>.exe.
# Usage: powershell -ExecutionPolicy Bypass -File tools\build_installer.ps1
# Needs Rust and Inno Setup 6.7 or later (winget install JRSoftware.InnoSetup --scope user).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
# A short target path: with long ones the MSVC linker fails with LNK1104.
$target = 'C:\t\teeth'
$exe = Join-Path $target 'release\teeth.exe'
$out = 'C:\t\instalador'

if (Get-Process -Name teeth -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }) {
    throw "Cierra $exe antes de construir el instalador."
}

$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$env:CARGO_TARGET_DIR = $target
& cargo build --release --manifest-path (Join-Path $root 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'La compilación en release falló.' }

$line = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1
$version = $line.Matches[0].Groups[1].Value

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'No se encontró ISCC.exe. Instala Inno Setup: winget install JRSoftware.InnoSetup --scope user' }

& $iscc /Q "/DAppVersion=$version" "/DExePath=$exe" "/O$out" (Join-Path $root 'installer\teeth.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup no pudo construir el instalador.' }
Write-Output (Join-Path $out "Teeth-Setup-$version.exe")
