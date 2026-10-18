@echo off
cd /d "%~dp0"
echo ============================================
echo   PC Remote Server (Rust)
echo ============================================
echo   Console : http://127.0.0.1:8765/
echo   Tray icon: bottom-right of the taskbar, click to open.
echo.
pccontrol-server.exe %*
echo.
echo Server stopped.
pause
