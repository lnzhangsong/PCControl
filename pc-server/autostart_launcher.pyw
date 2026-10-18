# -*- coding: utf-8 -*-
"""
开机自启启动器（无黑色命令行窗口）

由 Windows「启动」文件夹里的快捷方式调用，用 pythonw.exe 运行，
所以不会出现任何控制台窗口。

职责：
  1. 先探测端口，若服务已在运行则直接退出（避免两个进程抢同一端口）
  2. 读取同目录可选的 autostart.json 覆盖端口/配对码
  3. 以无窗口方式拉起 server.py，输出写入 logs/server-YYYYMMDD.log
"""
import json
import os
import socket
import subprocess
import sys
import time
from datetime import datetime

BASE = os.path.dirname(os.path.abspath(__file__))
CONF = os.path.join(BASE, "autostart.json")
LOG_DIR = os.path.join(BASE, "logs")

DEFAULTS = {"port": 8765, "pin": "123456", "extraArgs": []}


def load_conf():
    conf = dict(DEFAULTS)
    if os.path.isfile(CONF):
        try:
            with open(CONF, "r", encoding="utf-8") as f:
                conf.update(json.load(f) or {})
        except Exception:
            pass
    return conf


def port_in_use(port):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(0.6)
        return s.connect_ex(("127.0.0.1", int(port))) == 0


def log_path():
    os.makedirs(LOG_DIR, exist_ok=True)
    return os.path.join(LOG_DIR, "server-%s.log" % datetime.now().strftime("%Y%m%d"))


def main():
    conf = load_conf()
    port = int(conf.get("port", 8765))

    # 已有实例在跑，直接退出（开机自启 + 手动启动同时存在时不会冲突）
    if port_in_use(port):
        return

    # 用同一个 pythonw.exe 启动，保持"无窗口"并复用已装好的依赖
    pythonw = sys.executable
    if not pythonw.lower().endswith("pythonw.exe"):
        candidate = os.path.join(os.path.dirname(pythonw), "pythonw.exe")
        if os.path.isfile(candidate):
            pythonw = candidate

    cmd = [pythonw, os.path.join(BASE, "server.py"),
           "--port", str(port), "--pin", str(conf.get("pin", "123456"))]
    cmd += [str(a) for a in (conf.get("extraArgs") or [])]

    logfile = open(log_path(), "ab", buffering=0)
    logfile.write(("\n===== 启动 %s =====\n" % datetime.now().strftime("%Y-%m-%d %H:%M:%S")).encode("utf-8"))

    flags = 0
    if os.name == "nt":
        flags = 0x00000008 | 0x00000200 | 0x08000000  # DETACHED_PROCESS|NEW_PROCESS_GROUP|CREATE_NO_WINDOW

    try:
        subprocess.Popen(
            cmd, cwd=BASE, stdout=logfile, stderr=logfile, stdin=subprocess.DEVNULL,
            creationflags=flags, close_fds=True,
        )
    except Exception as e:
        logfile.write(("启动失败: %r\n" % (e,)).encode("utf-8"))
        return
    finally:
        # 给子进程一点时间把日志刷出来
        time.sleep(0.2)
        logfile.close()


if __name__ == "__main__":
    main()
