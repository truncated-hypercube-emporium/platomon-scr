@echo off
setlocal EnableExtensions
rem Moves platomon.scr and platomon-config.exe (from this script's folder)
rem into %SystemRoot%\System32 so platomon shows up in Screen Saver Settings,
rem then adds Windows Defender exclusions for both installed files.

set "SRC=%~dp0"
set "DEST=%SystemRoot%\System32"
rem A 32-bit cmd sees System32 redirected to SysWOW64; Sysnative is the real one.
if exist "%SystemRoot%\Sysnative\" set "DEST=%SystemRoot%\Sysnative"

rem Writing to System32 needs administrator rights: relaunch elevated if needed.
net session >nul 2>&1
if errorlevel 1 (
    echo Requesting administrator rights...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

set "FAIL=0"
for %%F in (platomon.scr platomon-config.exe) do (
    if exist "%SRC%%%F" (
        move /Y "%SRC%%%F" "%DEST%\" >nul && (
            echo Moved %%F to %SystemRoot%\System32
        ) || (
            echo Failed to move %%F ^(is the screensaver or config utility running?^)
            set "FAIL=1"
        )
    ) else (
        echo Not found: %SRC%%%F
        set "FAIL=1"
    )
)

rem Exclude the installed files from Windows Defender scans. Defender sees the
rem real System32 path; a 64-bit PowerShell is used so Test-Path sees it too.
set "PS=powershell"
if exist "%SystemRoot%\Sysnative\WindowsPowerShell\v1.0\powershell.exe" set "PS=%SystemRoot%\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
echo.
"%PS%" -NoProfile -ExecutionPolicy Bypass -Command "$f = @('%SystemRoot%\System32\platomon.scr', '%SystemRoot%\System32\platomon-config.exe') | Where-Object { Test-Path -LiteralPath $_ }; if (-not $f) { 'No installed files to exclude from Windows Defender.'; exit 1 }; try { Add-MpPreference -ExclusionPath $f -ErrorAction Stop; $f | ForEach-Object { 'Windows Defender exclusion added: ' + $_ } } catch { 'Could not add Windows Defender exclusions: ' + $_.Exception.Message; exit 1 }"
if errorlevel 1 set "FAIL=1"

echo.
if "%FAIL%"=="0" (
    echo Done. Pick "platomon" in Settings ^> Personalization ^> Lock screen ^> Screen saver.
) else (
    echo Finished with errors.
)
pause
endlocal
