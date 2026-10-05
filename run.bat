@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul || set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

rem Only one Wectangle can run at a time, and a running copy locks the exe.
taskkill /im "wectangle*" >nul 2>nul
ping -n 2 127.0.0.1 >nul
taskkill /f /im "wectangle*" >nul 2>nul

rem Cargo only recompiles what changed, so this takes a second when nothing did.
cargo build --release || goto :failed
start "" "target\release\wectangle.exe"
exit /b 0

:failed
echo.
echo Build failed.
pause
exit /b 1
