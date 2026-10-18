@echo off
setlocal
set "LNK=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\PC Remote Server.lnk"

if exist "%LNK%" (
    del /f /q "%LNK%"
    echo [OK] Auto-start disabled.
) else (
    echo [INFO] Auto-start was not enabled.
)
echo.
pause
