@echo off
rem ===================================================================
rem  Open the PC console in app-window mode (no address bar).
rem  Starts the background server first if it is not running.
rem ===================================================================
setlocal
set "BASE=%~dp0"
set "URL=http://127.0.0.1:8765/"

tasklist /FI "IMAGENAME eq pccontrol-server.exe" 2>nul | find /I "pccontrol-server.exe" >nul
if errorlevel 1 (
  start "" "%BASE%pccontrol-server.exe"
  timeout /t 2 >nul
)

set "BROWSER=%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"
if not exist "%BROWSER%" set "BROWSER=%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"
if not exist "%BROWSER%" set "BROWSER=%ProgramFiles%\Google\Chrome\Application\chrome.exe"
if not exist "%BROWSER%" set "BROWSER=%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe"

if exist "%BROWSER%" (
  start "" "%BROWSER%" --app=%URL%
) else (
  start "" "%URL%"
)
