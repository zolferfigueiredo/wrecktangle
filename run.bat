@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul || set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

rem Only one Wrecktangle can run at a time, and a running copy locks the exe.
taskkill /im "wrecktangle*" >nul 2>nul
ping -n 2 127.0.0.1 >nul
taskkill /f /im "wrecktangle*" >nul 2>nul

rem A debug build, which recompiles only what changed in seconds; build.bat and the installer make the
rem optimized release. Cargo takes a second when nothing changed.
cargo build || goto :failed
start "" "target\debug\wrecktangle.exe"
exit /b 0

:failed
echo.
echo Build failed.
pause
exit /b 1
