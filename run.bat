@echo off
setlocal
cd /d "%~dp0"

rem Only one Wectangle can run at a time, so close any running copy first.
taskkill /im "wectangle*" >nul 2>nul
timeout /t 1 /nobreak >nul
taskkill /f /im "wectangle*" >nul 2>nul

if not exist "target\release\wectangle.exe" call "%~dp0build.bat" || exit /b 1
start "" "target\release\wectangle.exe"
