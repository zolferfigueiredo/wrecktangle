@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul || set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

rem A running copy locks target\release\wectangle.exe, so close it first.
taskkill /im "wectangle*" >nul 2>nul
timeout /t 1 /nobreak >nul
taskkill /f /im "wectangle*" >nul 2>nul

cargo build --release || goto :failed
exit /b 0

:failed
echo.
echo Build failed.
pause
exit /b 1
