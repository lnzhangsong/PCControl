@echo off
taskkill /F /IM pccontrol-server.exe >nul 2>&1
if errorlevel 1 (
    echo [INFO] Server is not running.
) else (
    echo [OK] Server stopped.
)
timeout /t 2 >nul
