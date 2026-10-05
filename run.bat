@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul || set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

rem A running copy locks target\release\wectangle.exe, and a second copy would
rem only open the first one's Settings, so close it before building.
taskkill /im "wectangle*" >nul 2>nul
timeout /t 1 /nobreak >nul
taskkill /f /im "wectangle*" >nul 2>nul

cargo build --release
if errorlevel 1 (
    echo.
    echo Build failed.
    pause
    exit /b 1
)

start "" "target\release\wectangle.exe"
