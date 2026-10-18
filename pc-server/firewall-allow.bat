@echo off
chcp 65001 >nul
title PC Remote - Firewall
fltmc >nul 2>&1
if errorlevel 1 (
    echo Requesting administrator rights...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)
echo Adding firewall rules for PC Remote ...
netsh advfirewall firewall delete rule name="PC Remote 8765" >nul 2>&1
netsh advfirewall firewall delete rule name="PC Remote Discovery" >nul 2>&1
netsh advfirewall firewall add rule name="PC Remote 8765" dir=in action=allow protocol=TCP localport=8765 profile=any
netsh advfirewall firewall add rule name="PC Remote Discovery" dir=in action=allow protocol=UDP localport=55667 profile=any
echo.
echo Done.
echo.
pause
