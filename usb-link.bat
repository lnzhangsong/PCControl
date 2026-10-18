@echo off
chcp 65001 >nul
title PC Remote - USB 通道
set ADB=%LOCALAPPDATA%\Android\Sdk\platform-tools\adb.exe
if not exist "%ADB%" (
    echo 找不到 adb，请确认已安装 Android SDK Platform-Tools
    pause & exit /b 1
)
echo 建立 USB 反向隧道 127.0.0.1:8765 -^> 电脑:8765 ...
"%ADB%" reverse tcp:8765 tcp:8765
"%ADB%" reverse --list
echo.
echo 正在打开 App ...
"%ADB%" shell am start -n com.workbuddy.pccontrol/.MainActivity
echo.
echo 提示：App 里把电脑 IP 填 127.0.0.1，端口 8765，配对码见服务端窗口。
echo       拔掉数据线隧道就失效，重新双击本文件即可。
pause
