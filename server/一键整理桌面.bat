@echo off
chcp 65001 >nul
rem 一键整理桌面：调用 PC Remote 的桌面分类功能，整理完会弹窗告诉你结果
rem 想还原就打开 PC Remote 面板，点「撤销上次」
cd /d "%~dp0"
start "" /b pccontrol-server.exe --tidy-desktop
