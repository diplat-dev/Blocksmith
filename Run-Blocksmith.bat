@echo off
setlocal

cd /d "%~dp0"
title Blocksmith Launcher

if exist "%USERPROFILE%\.cargo\bin" (
  set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

where npm >nul 2>nul
if errorlevel 1 goto :missing_npm

if not exist "node_modules" (
  echo Installing frontend dependencies...
  call npm.cmd install --cache .\.npm-cache
  if errorlevel 1 goto :failed
)

echo Starting Blocksmith...
call npm.cmd run tauri dev
if errorlevel 1 goto :failed

exit /b 0

:missing_npm
echo npm was not found on PATH.
echo Install Node.js, then try again.
pause
exit /b 1

:failed
echo.
echo Blocksmith failed to start.
echo Review the messages above, then press any key to close this window.
pause
exit /b 1
