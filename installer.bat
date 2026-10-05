@echo off
rem Builds the installer dist\Wrecktangle-<version>-x64-setup.exe on this PC. Needs Inno Setup 6.
setlocal
cd /d "%~dp0"

set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if exist "%ISCC%" goto have_iscc
set "ISCC=%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe"
if exist "%ISCC%" goto have_iscc
echo Inno Setup isn't installed: winget install JRSoftware.InnoSetup (then run this again).
exit /b 1
:have_iscc

set "VALUEPART="
for /f "tokens=1,* delims==" %%a in ('findstr /r /c:"^version = " "Cargo.toml"') do set "VALUEPART=%%b"
set VERSION=%VALUEPART:"=%
set VERSION=%VERSION: =%
if not "%VERSION%"=="" goto have_version
echo Couldn't read the version from Cargo.toml.
exit /b 1
:have_version

call "%~dp0build.bat"
if errorlevel 1 exit /b 1

"%ISCC%" /Q "/DAppVersion=%VERSION%" installer\wrecktangle.iss
if errorlevel 1 exit /b 1

echo Built: dist\Wrecktangle-%VERSION%-x64-setup.exe
