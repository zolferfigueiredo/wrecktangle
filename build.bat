@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul || set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

rem A running copy locks target\release\wrecktangle.exe, so close it first.
taskkill /im "wrecktangle*" >nul 2>nul
ping -n 2 127.0.0.1 >nul
taskkill /f /im "wrecktangle*" >nul 2>nul

cargo build --release || goto :failed
echo.
echo Built: %~dp0target\release\wrecktangle.exe
rem A double-clicked window closes when the script ends, so wait there;
rem not when installer.bat or a terminal runs this.
echo %cmdcmdline% | find /i "%~nx0" >nul && pause
exit /b 0

:failed
echo.
echo Build failed.
pause
exit /b 1
