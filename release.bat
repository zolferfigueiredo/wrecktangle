@echo off
setlocal
rem release.bat 0.4.1  opens a pull request that bumps the version to 0.4.1.
rem release.bat        publishes the version on main, or, once that is
rem                    released, asks for the next one. See scripts\release.ps1.
where gh >nul 2>nul || set "PATH=%ProgramFiles%\GitHub CLI;%PATH%"
where cargo >nul 2>nul || set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\release.ps1" %*
set "CODE=%ERRORLEVEL%"
rem A double-clicked window closes when the script ends, so wait there.
echo %cmdcmdline% | find /i "%~nx0" >nul && pause
exit /b %CODE%
