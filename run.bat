@echo off
setlocal
cd /d "%~dp0"
if not exist "target\release\wectangle.exe" call "%~dp0build.bat" || exit /b 1
start "" "target\release\wectangle.exe"
