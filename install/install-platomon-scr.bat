@echo off
setlocal EnableExtensions
rem Moves platomon.scr and platomon-config.exe (from this script's folder)
rem into %SystemRoot%\System32 so platomon shows up in Screen Saver Settings.

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

echo.
if "%FAIL%"=="0" (
    echo Done. Pick "platomon" in Settings ^> Personalization ^> Lock screen ^> Screen saver.
) else (
    echo Finished with errors.
)
pause
endlocal
