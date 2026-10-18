@echo off
setlocal
set "PCDIR=%~dp0"
set "LNK=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\PC Remote Server.lnk"

if not exist "%PCDIR%pccontrol-server.exe" (
    echo [ERROR] pccontrol-server.exe not found next to this script.
    goto end
)

powershell -NoProfile -ExecutionPolicy Bypass -Command "$ws=New-Object -ComObject WScript.Shell; $sc=$ws.CreateShortcut($env:APPDATA+'\Microsoft\Windows\Start Menu\Programs\Startup\PC Remote Server.lnk'); $sc.TargetPath=$env:WINDIR+'\System32\wscript.exe'; $sc.Arguments=[char]34+$env:PCDIR+'start-hidden.vbs'+[char]34; $sc.WorkingDirectory=$env:PCDIR; $sc.WindowStyle=7; $sc.Description='PC Remote Server'; $sc.Save()"

if exist "%LNK%" (
    echo [OK] Auto-start enabled. The server will run silently after you log in.
) else (
    echo [FAILED] Could not create the startup shortcut.
)

:end
echo.
pause
