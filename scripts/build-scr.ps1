# Build the Windows screensaver and its configuration utility:
#   dist\platomon.scr         the screensaver (self-contained)
#   dist\platomon-config.exe  opens the screensaver's settings dialog
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-scr.ps1
#
# Install: right-click dist\platomon.scr > Install, or copy it to
# C:\Windows\System32 and pick "platomon" in Screen Saver Settings.
# Settings (from either Settings... or platomon-config.exe) are stored in
# %APPDATA%\platomon\screensaver.ini.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }

    cargo build --release --bin platomon-scr --bin platomon-config --features screensaver
    if ($LASTEXITCODE -ne 0) { throw 'build failed' }

    New-Item -ItemType Directory -Force (Join-Path $root 'dist') | Out-Null
    foreach ($pair in @(@('platomon-scr.exe', 'platomon.scr'), @('platomon-config.exe', 'platomon-config.exe'))) {
        $out = Join-Path $root "dist\$($pair[1])"
        Copy-Item (Join-Path $target "release\$($pair[0])") $out -Force
        Write-Host "built $out ($([math]::Round((Get-Item $out).Length / 1MB, 1)) MB)"
    }
}
finally {
    Pop-Location
}
