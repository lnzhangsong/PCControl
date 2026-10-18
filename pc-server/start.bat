@echo off
chcp 65001 >nul
cd /d "%~dp0"
title PC Remote Server

if not exist ".venv" (
    echo [1/2] 首次运行，正在创建运行环境并安装依赖……
    python -m venv .venv
    if errorlevel 1 (
        echo.
        echo 未找到 python，请先安装 Python 3.8+ 并勾选 Add to PATH
        pause & exit /b 1
    )
    ".venv\Scripts\python.exe" -m pip install --upgrade pip -q
    ".venv\Scripts\python.exe" -m pip install -r requirements.txt -q
    echo [2/2] 依赖安装完成
)

".venv\Scripts\python.exe" server.py %*
pause
