#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PC Remote Server —— 让手机控制这台 Windows 电脑
================================================
功能：音量调节 / 静音 / 屏幕亮度 / 媒体播放键 / 锁屏 / 休眠 / 关屏
通信：局域网 HTTP + 6 位配对码，支持 UDP 自动发现

启动：  python server.py
可选参数：
        --port 8765          监听端口
        --pin 123456         固定配对码（默认随机生成并写入 pin.txt）
        --no-discovery       关闭 UDP 自动发现
"""
from __future__ import annotations

import argparse
import atexit
import ctypes
import json
import os
import re
import secrets
import socket
import subprocess
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

APP_NAME = "PC Remote Server"
APP_VERSION = "1.0"
DISCOVERY_PORT = 55667
DISCOVERY_MAGIC = b"PCREMOTE_DISCOVER"
BASE_DIR = os.path.dirname(os.path.abspath(__file__))
IS_WIN = sys.platform == "win32"
CREATE_NO_WINDOW = 0x08000000 if IS_WIN else 0

# 高 DPI 缩放会让覆盖层窗口盖不满屏幕，必须在创建窗口前声明 DPI 感知
if IS_WIN:
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)   # PER_MONITOR_DPI_AWARE
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass


# ----------------------------------------------------------------------------
# 工具
# ----------------------------------------------------------------------------
def log(*args):
    print(time.strftime("[%H:%M:%S]"), *args, flush=True)


def get_lan_ip() -> str:
    """取本机在局域网中的 IPv4 地址"""
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.connect(("8.8.8.8", 80))
        return s.getsockname()[0]
    except Exception:
        return "127.0.0.1"
    finally:
        s.close()


def get_os_name() -> str:
    if not IS_WIN:
        return sys.platform
    try:
        v = sys.getwindowsversion()
        return f"Windows {v.major}.{v.minor} (build {v.build})"
    except Exception:
        return "Windows"


def run_ps(cmd: str, timeout: float = 8.0) -> str:
    """执行 PowerShell 命令（不弹黑窗）。中文系统输出编码不统一，这里容错解码。"""
    try:
        r = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", cmd],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            timeout=timeout, creationflags=CREATE_NO_WINDOW,
        )
        return (r.stdout or "").strip()
    except Exception as e:
        log("PowerShell 执行失败:", e)
        return ""


_thread_local = threading.local()


def ensure_com():
    """HTTP 是多线程处理，每个线程都要初始化一次 COM，否则 pycaw 会报错"""
    if getattr(_thread_local, "com_ready", False):
        return
    try:
        ctypes.windll.ole32.CoInitializeEx(None, 0x2)  # APARTMENTTHREADED
    except Exception:
        pass
    _thread_local.com_ready = True


# ----------------------------------------------------------------------------
# 音量控制
# ----------------------------------------------------------------------------
class AudioController:
    """优先用 pycaw（精确设置 0-100），失败则降级为媒体按键步进"""

    def __init__(self):
        self.ok = False
        self.error = ""
        self._pycaw = None
        try:
            from pycaw.pycaw import AudioUtilities
            self._pycaw = AudioUtilities
            self._endpoint()  # 试一次，确认可用
            self.ok = True
            log("音量控制：pycaw 就绪")
        except Exception as e:
            self.error = str(e)
            log("音量控制：pycaw 不可用 ->", e)

    # --- 底层：拿到 IAudioEndpointVolume ---
    def _endpoint(self):
        dev = self._pycaw.GetSpeakers()
        if hasattr(dev, "volume_percent"):  # pycaw >= 20240210 新接口
            return dev, getattr(dev, "EndpointVolume", None)
        from ctypes import cast, POINTER
        from comtypes import CLSCTX_ALL
        from pycaw.pycaw import IAudioEndpointVolume
        iface = dev.Activate(IAudioEndpointVolume._iid_, CLSCTX_ALL, None)
        return dev, cast(iface, POINTER(IAudioEndpointVolume))

    def get(self):
        """返回 (volume 0-100, muted)"""
        ensure_com()
        if not self.ok:
            return None, None
        try:
            dev, iface = self._endpoint()
            if hasattr(dev, "volume_percent"):
                return int(round(dev.volume_percent)), bool(iface.GetMute())
            return int(round(iface.GetMasterVolumeLevelScalar() * 100)), bool(iface.GetMute())
        except Exception as e:
            log("读取音量失败:", e)
            return None, None

    def set(self, value: int):
        ensure_com()
        value = max(0, min(100, int(value)))
        if not self.ok:
            return False
        try:
            dev, iface = self._endpoint()
            if hasattr(dev, "volume_percent"):
                dev.volume_percent = value
                if iface.GetMute() and value > 0:
                    iface.SetMute(0, None)
                return True
            iface.SetMasterVolumeLevelScalar(value / 100.0, None)
            if iface.GetMute() and value > 0:
                iface.SetMute(0, None)
            return True
        except Exception as e:
            log("设置音量失败:", e)
            return False

    def toggle_mute(self, target=None):
        ensure_com()
        if not self.ok:
            return False
        try:
            _, iface = self._endpoint()
            now = bool(iface.GetMute())
            m = (not now) if target is None else bool(target)
            iface.SetMute(1 if m else 0, None)
            return True
        except Exception as e:
            log("静音切换失败:", e)
            return False


# ----------------------------------------------------------------------------
# 软件调光：铺一层全屏、可穿透点击的半透明黑色窗口
# 比 gamma 表可靠得多（很多显卡驱动、或开启 HDR 后，Windows 会忽略 SetDeviceGammaRamp）
# ----------------------------------------------------------------------------
class ScreenDimmer:
    def __init__(self):
        self.hwnd = None
        self.ok = False
        self.ratio = 1.0
        self._ready = threading.Event()
        self._proc = None

    def _run(self):
        from ctypes import wintypes as wt

        user32 = ctypes.windll.user32
        kernel32 = ctypes.windll.kernel32
        LRESULT, WPARAM, LPARAM = ctypes.c_ssize_t, ctypes.c_size_t, ctypes.c_ssize_t
        WNDPROC = ctypes.WINFUNCTYPE(LRESULT, wt.HWND, wt.UINT, WPARAM, LPARAM)

        # 64 位下必须显式声明原型，否则句柄会被截断成 32 位
        user32.DefWindowProcW.argtypes = [wt.HWND, wt.UINT, WPARAM, LPARAM]
        user32.DefWindowProcW.restype = LRESULT
        user32.CreateWindowExW.argtypes = [
            wt.DWORD, wt.LPCWSTR, wt.LPCWSTR, wt.DWORD,
            ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
            wt.HWND, wt.HMENU, wt.HINSTANCE, ctypes.c_void_p]
        user32.CreateWindowExW.restype = wt.HWND
        user32.SetWindowPos.argtypes = [wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int,
                                        ctypes.c_int, ctypes.c_int, wt.UINT]
        user32.SetLayeredWindowAttributes.argtypes = [wt.HWND, wt.DWORD, wt.BYTE, wt.DWORD]
        user32.ShowWindow.argtypes = [wt.HWND, ctypes.c_int]
        user32.GetMessageW.argtypes = [ctypes.POINTER(wt.MSG), wt.HWND, wt.UINT, wt.UINT]
        user32.PostMessageW.argtypes = [wt.HWND, wt.UINT, WPARAM, LPARAM]
        user32.DestroyWindow.argtypes = [wt.HWND]

        class WNDCLASSW(ctypes.Structure):
            _fields_ = [
                ("style", wt.UINT), ("lpfnWndProc", WNDPROC),
                ("cbClsExtra", ctypes.c_int), ("cbWndExtra", ctypes.c_int),
                ("hInstance", wt.HINSTANCE), ("hIcon", wt.HICON),
                ("hCursor", wt.HANDLE), ("hbrBackground", wt.HBRUSH),
                ("lpszMenuName", wt.LPCWSTR), ("lpszClassName", wt.LPCWSTR),
            ]

        def wndproc(hwnd, msg, wp, lp):
            if msg == 0x0010:            # WM_CLOSE
                user32.DestroyWindow(hwnd)
                return 0
            if msg == 0x0002:            # WM_DESTROY
                user32.PostQuitMessage(0)
                return 0
            return user32.DefWindowProcW(hwnd, msg, wp, lp)

        self._proc = WNDPROC(wndproc)
        cls = "PCRemoteDimmerWnd"
        wc = WNDCLASSW()
        wc.lpfnWndProc = self._proc
        wc.hInstance = kernel32.GetModuleHandleW(None)
        wc.lpszClassName = cls
        try:
            user32.RegisterClassW(ctypes.byref(wc))
        except Exception:
            pass

        # 覆盖整个虚拟桌面（多显示器）
        x, y = user32.GetSystemMetrics(76), user32.GetSystemMetrics(77)
        w, h = user32.GetSystemMetrics(78), user32.GetSystemMetrics(79)
        WS_POPUP = 0x80000000
        EX = 0x80000 | 0x20 | 0x80 | 0x08000000   # LAYERED|TRANSPARENT|TOOLWINDOW|NOACTIVATE
        self.hwnd = user32.CreateWindowExW(
            EX, cls, "PC Remote Dimmer", WS_POPUP, x, y, w, h,
            None, None, kernel32.GetModuleHandleW(None), None)
        if not self.hwnd:
            log("覆盖层窗口创建失败:", ctypes.GetLastError())
            self._ready.set()
            return
        user32.SetWindowPos(self.hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)
        user32.ShowWindow(self.hwnd, 4)   # SW_SHOWNOACTIVATE
        self.ok = True
        self._apply(self.ratio)
        self._ready.set()

        msg = wt.MSG()
        while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))

    def _apply(self, ratio: float):
        if not self.hwnd:
            return
        user32 = ctypes.windll.user32
        alpha = int(round(max(0.0, min(1.0, 1.0 - ratio)) * 235))
        try:
            if alpha > 0:
                user32.SetLayeredWindowAttributes(self.hwnd, 0, alpha, 0x2)  # LWA_ALPHA
                user32.ShowWindow(self.hwnd, 4)
                user32.SetWindowPos(self.hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)
            else:
                user32.ShowWindow(self.hwnd, 0)  # SW_HIDE
        except Exception as e:
            log("覆盖层调光失败:", e)

    # ---- 对外接口 ----
    def start(self) -> bool:
        threading.Thread(target=self._run, daemon=True).start()
        self._ready.wait(3.0)
        return self.ok

    def set_ratio(self, ratio: float):
        self.ratio = ratio
        self._apply(ratio)

    def stop(self):
        if self.hwnd:
            try:
                ctypes.windll.user32.PostMessageW(self.hwnd, 0x0010, 0, 0)
            except Exception:
                pass
            self.hwnd = None


# ----------------------------------------------------------------------------
# 亮度控制：按"真实能力"分级，而不是盲目降级
#   1) DDC/CI  —— 直接写显示器背光（真·亮度，HDR 关闭时可用）
#   2) WMI      —— 笔记本内置屏背光
#   3) SDR 白点 —— HDR 模式下的等效通道
#   4) 覆盖层   —— 仅在前三者都不可用时兜底
# ----------------------------------------------------------------------------
class BrightnessController:
    MODE_DDC = "ddc"
    MODE_WMI = "wmi"
    MODE_SDR = "sdr"
    MODE_SOFT = "soft"

    def __init__(self):
        self.mode = None
        self.level = 100
        self.hdr = False
        self.reason = ""
        self._mon = None
        self._dimmer = None
        self._lock = threading.RLock()
        self._backend = None
        if IS_WIN:
            self.refresh()
        else:
            self.mode = self.MODE_SOFT
            self._backend = ScreenDimmer()
            self._backend.start()

    # ---------------- HDR ----------------
    def _read_hdr(self) -> bool:
        try:
            import display_ctrl
            outs = display_ctrl.list_outputs()
            return bool(outs) and bool(outs[0].get("hdrEnabled"))
        except Exception:
            return False

    def set_hdr(self, enabled: bool) -> bool:
        """开关 HDR（关掉 HDR 后显示器才接受 DDC 背光写入）"""
        try:
            import display_ctrl
            ok = display_ctrl.set_hdr_enabled(bool(enabled))
            time.sleep(2.0)      # 显示器重新协商模式需要时间
            self.refresh()
            log(f"HDR 已{'开启' if enabled else '关闭'}；亮度控制方式 -> {self.mode}")
            return ok
        except Exception as e:
            log("切换 HDR 失败:", e)
            return False

    # ---------------- 探测 ----------------
    def refresh(self):
        with self._lock:
            self.hdr = self._read_hdr()
            self.reason = ""
            self.mode = None
            if self._probe_wmi() or self._probe_ddc() or (self.hdr and self._probe_sdr()):
                return
            self._probe_soft()

    def _probe_wmi(self) -> bool:
        try:
            out = run_ps("(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness "
                         "-ErrorAction Stop | Select-Object -First 1).CurrentBrightness", timeout=8)
            if re.fullmatch(r"\d+", out or ""):
                self.mode = self.MODE_WMI
                self.level = int(out)
                log(f"亮度控制：WMI 内置屏背光可用（当前 {out}）")
                return True
        except Exception:
            pass
        return False

    def _probe_ddc(self) -> bool:
        try:
            import monitorcontrol
            mons = monitorcontrol.get_monitors()
            if not mons:
                return False
            mon = mons[0]
            with mon:
                cur, mx = mon.vcp.get_vcp_feature(0x10)
                if not mx:
                    return False
                test = cur - 2 if cur >= 3 else cur + 2
                mon.vcp.set_vcp_feature(0x10, test)
                time.sleep(0.45)
                after, _ = mon.vcp.get_vcp_feature(0x10)
                mon.vcp.set_vcp_feature(0x10, cur)      # 探针回滚
            if after != cur:
                self._mon = mon
                self.mode = self.MODE_DDC
                self.level = cur
                log(f"亮度控制：DDC/CI 显示器背光可用（{cur}/{mx}，HDR {'开' if self.hdr else '关'}）")
                return True
            self.reason = ("显示器在 HDR 模式下拒绝 DDC 背光写入，关掉 HDR 即可恢复"
                           if self.hdr else "显示器拒绝了 DDC/CI 写入")
            log("亮度控制：" + self.reason)
        except Exception as e:
            self.reason = f"DDC/CI 不可用：{e}"
            log("亮度控制：", self.reason)
        return False

    def _probe_sdr(self) -> bool:
        try:
            import display_ctrl
            cur = display_ctrl.get_sdr_white()
            if cur and display_ctrl.set_sdr_white(cur):
                self.mode = self.MODE_SDR
                self.level = display_ctrl.white_to_percent(cur)
                log("亮度控制：HDR SDR 白点亮度可用")
                return True
        except Exception:
            pass
        return False

    def _probe_soft(self):
        if self._dimmer is None:
            self._dimmer = ScreenDimmer()
        if self._dimmer.ok or self._dimmer.start():
            self.mode = self.MODE_SOFT
            self.level = 100
            self.reason = self.reason or "显示器不支持硬件调光"
            log("亮度控制：使用覆盖层软件调光（画面上叠暗一层，非真实背光）")
        else:
            self.mode = self.MODE_SOFT
            self.level = 100
            log("亮度控制：覆盖层不可用，亮度功能受限")

    # ---------------- 读 ----------------
    def get(self) -> int:
        with self._lock:
            if self.mode == self.MODE_WMI:
                try:
                    out = run_ps("(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness "
                                 "-ErrorAction Stop | Select-Object -First 1).CurrentBrightness", timeout=5)
                    if re.fullmatch(r"\d+", out or ""):
                        self.level = int(out)
                except Exception:
                    pass
            elif self.mode == self.MODE_DDC and self._mon is not None:
                try:
                    with self._mon:
                        cur, _ = self._mon.vcp.get_vcp_feature(0x10)
                    self.level = int(cur)
                except Exception:
                    pass
            return self.level

    # ---------------- 写 ----------------
    def set(self, value: int) -> bool:
        value = max(0, min(100, int(value)))
        with self._lock:
            # 用户可能在系统里手动开关了 HDR，检测到变化就重新探测
            if self._read_hdr() != self.hdr:
                self.refresh()

            if self.mode == self.MODE_WMI:
                try:
                    run_ps(f"(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods "
                           f"-ErrorAction Stop).WmiSetBrightness(1,{value})", timeout=5)
                    self.level = value
                    return True
                except Exception as e:
                    log("WMI 设置亮度失败:", e)
                    return False

            if self.mode == self.MODE_DDC and self._mon is not None:
                try:
                    with self._mon:
                        self._mon.vcp.set_vcp_feature(0x10, value)
                    self.level = value
                    return True
                except Exception as e:
                    log("DDC 设置亮度失败:", e)
                    return False

            if self.mode == self.MODE_SDR:
                try:
                    import display_ctrl
                    ok = display_ctrl.set_sdr_white(display_ctrl.percent_to_white(value))
                    self.level = value
                    return ok
                except Exception as e:
                    log("设置 SDR 白点失败:", e)
                    return False

            # 软件调光
            self.level = value
            if self._dimmer:
                self._dimmer.set_ratio(value / 100.0)
            return True

    def restore(self):
        """退出时只还原软件层；真实背光属于用户设置，保留不动"""
        if self._dimmer:
            self._dimmer.set_ratio(1.0)
            self._dimmer.stop()


# ----------------------------------------------------------------------------
# 媒体键 / 电源
# ----------------------------------------------------------------------------
VK = {"play_pause": 0xB3, "next": 0xB0, "prev": 0xB1, "stop": 0xB2,
      "vol_up": 0xAF, "vol_down": 0xAE, "mute_key": 0xAD}


class MediaController:
    def press(self, action: str) -> bool:
        if not IS_WIN or action not in VK:
            return False
        code = VK[action]
        try:
            ctypes.windll.user32.keybd_event(code, 0, 0, 0)
            ctypes.windll.user32.keybd_event(code, 0, 2, 0)
            return True
        except Exception as e:
            log("媒体键失败:", e)
            return False


class PowerController:
    def act(self, action: str) -> bool:
        if not IS_WIN:
            return False
        try:
            if action == "lock":
                return bool(ctypes.windll.user32.LockWorkStation())
            if action == "sleep":
                # hibernate=0, force=1, disableWake=0
                return bool(ctypes.windll.powrprof.SetSuspendState(0, 1, 0))
            if action == "screen_off":
                HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER = 0xFFFF, 0x0112, 0xF170
                ctypes.windll.user32.SendMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER, 2)
                return True
        except Exception as e:
            log("电源操作失败:", e)
        return False


AUDIO = AudioController() if IS_WIN else None
BRIGHT = BrightnessController() if IS_WIN else None
MEDIA = MediaController()
POWER = PowerController()

PIN = "000000"          # main() 里赋值或由 --pin 指定
PORT = 8765
SERVER_START = time.time()
_op_lock = threading.Lock()


# ----------------------------------------------------------------------------
# HTTP 处理
# ----------------------------------------------------------------------------
class Handler(BaseHTTPRequestHandler):
    server_version = f"{APP_NAME}/{APP_VERSION}"
    protocol_version = "HTTP/1.1"

    # ---------- 基础 ----------
    def log_message(self, fmt, *args):
        pass  # 静音默认日志，避免刷屏

    def _send(self, code: int, body, ctype="application/json; charset=utf-8"):
        if isinstance(body, (dict, list)):
            data = json.dumps(body, ensure_ascii=False).encode("utf-8")
        elif isinstance(body, str):
            data = body.encode("utf-8")
        else:
            data = body
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, X-Pin")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        try:
            self.wfile.write(data)
        except Exception:
            pass

    def _authorized(self, qs) -> bool:
        given = self.headers.get("X-Pin") or (qs.get("pin", [""])[0])
        return bool(given) and secrets.compare_digest(str(given), PIN)

    def _body(self) -> dict:
        try:
            n = int(self.headers.get("Content-Length") or 0)
            if n <= 0:
                return {}
            return json.loads(self.rfile.read(n).decode("utf-8") or "{}")
        except Exception:
            return {}

    # ---------- 路由 ----------
    def do_OPTIONS(self):
        self._send(204, b"")

    def do_GET(self):
        u = urlparse(self.path)
        qs = parse_qs(u.query)
        path = u.path.rstrip("/") or "/"

        # 网页控制台不需要配对码（页面内部再填 PIN）
        if path in ("/", "/index.html", "/webui"):
            return self._serve_webui()
        if path == "/api/ping":
            return self._send(200, {"ok": True, "app": APP_NAME, "version": APP_VERSION,
                                    "needPin": True, "name": socket.gethostname()})

        if not self._authorized(qs):
            return self._send(401, {"ok": False, "error": "配对码错误"})

        if path == "/api/status":
            return self._send(200, self._status())
        if path == "/api/volume":
            if AUDIO and AUDIO.ok:
                return self._send(200, {"ok": True, "volume": AUDIO.get()[0]})
            return self._send(200, {"ok": False, "error": "音量控制不可用"})

        return self._send(404, {"ok": False, "error": "unknown endpoint"})

    def do_POST(self):
        u = urlparse(self.path)
        qs = parse_qs(u.query)
        path = u.path.rstrip("/")
        if not self._authorized(qs):
            return self._send(401, {"ok": False, "error": "配对码错误"})
        body = self._body()

        with _op_lock:
            if path == "/api/volume":
                v = body.get("value")
                if v is None:
                    return self._send(400, {"ok": False, "error": "缺少 value"})
                ok = AUDIO.set(int(v)) if AUDIO and AUDIO.ok else False
                if not ok:
                    return self._send(200, {"ok": False, "error": "音量控制不可用"})
                vol, muted = AUDIO.get()
                return self._send(200, {"ok": True, "volume": vol, "muted": muted})

            if path == "/api/volume/step":
                if not (AUDIO and AUDIO.ok):
                    return self._send(200, {"ok": False, "error": "音量控制不可用"})
                cur, _ = AUDIO.get()
                cur = cur if cur is not None else 50
                AUDIO.set(cur + int(body.get("delta", 5)))
                vol, muted = AUDIO.get()
                return self._send(200, {"ok": True, "volume": vol, "muted": muted})

            if path == "/api/mute":
                if not (AUDIO and AUDIO.ok):
                    return self._send(200, {"ok": False, "error": "音量控制不可用"})
                AUDIO.toggle_mute(body.get("value"))
                vol, muted = AUDIO.get()
                return self._send(200, {"ok": True, "volume": vol, "muted": muted})

            if path == "/api/brightness":
                v = body.get("value")
                if v is None:
                    return self._send(400, {"ok": False, "error": "缺少 value"})
                ok = BRIGHT.set(int(v)) if BRIGHT else False
                return self._send(200, {"ok": ok, "brightness": BRIGHT.get() if BRIGHT else None,
                                        "mode": BRIGHT.mode if BRIGHT else None})

            if path == "/api/hdr":
                en = bool(body.get("enabled"))
                ok = BRIGHT.set_hdr(en) if BRIGHT else False
                return self._send(200, {
                    "ok": ok,
                    "hdr": BRIGHT.hdr if BRIGHT else None,
                    "brightness": BRIGHT.get() if BRIGHT else None,
                    "brightnessMode": BRIGHT.mode if BRIGHT else None,
                    "brightnessReason": BRIGHT.reason if BRIGHT else "",
                })

            if path == "/api/media":
                a = body.get("action", "")
                ok = MEDIA.press(a)
                return self._send(200, {"ok": ok})

            if path == "/api/power":
                a = body.get("action", "")
                ok = POWER.act(a)
                return self._send(200, {"ok": ok})

        return self._send(404, {"ok": False, "error": "unknown endpoint"})

    # ---------- 数据 ----------
    def _status(self):
        vol, muted = (AUDIO.get() if AUDIO and AUDIO.ok else (None, None))
        mac = uuid.getnode()
        return {
            "ok": True,
            "app": APP_NAME,
            "version": APP_VERSION,
            "name": socket.gethostname(),
            "os": get_os_name(),
            "ip": get_lan_ip(),
            "port": PORT,
            "uptime": int(time.time() - SERVER_START),
            "volume": vol,
            "muted": muted,
            "brightness": BRIGHT.get() if BRIGHT else None,
            "brightnessMode": BRIGHT.mode if BRIGHT else None,
            "brightnessReason": (BRIGHT.reason if BRIGHT else ""),
            "hdr": BRIGHT.hdr if BRIGHT else False,
            "canVolume": bool(AUDIO and AUDIO.ok),
            "canBrightness": BRIGHT is not None,
            "mac": ":".join(f"{(mac >> i) & 0xFF:02X}" for i in range(40, -1, -8)),
            "features": ["volume", "brightness", "media", "lock", "sleep", "screen_off"],
        }

    def _serve_webui(self):
        p = os.path.join(BASE_DIR, "webui.html")
        if not os.path.exists(p):
            return self._send(200, "<h1>PC Remote Server</h1><p>缺少 webui.html</p>",
                              "text/html; charset=utf-8")
        with open(p, "rb") as f:
            self._send(200, f.read(), "text/html; charset=utf-8")


# ----------------------------------------------------------------------------
# UDP 自动发现
# ----------------------------------------------------------------------------
def discovery_worker(port: int, stop: threading.Event):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind(("0.0.0.0", DISCOVERY_PORT))
    except OSError as e:
        log("UDP 发现端口被占用，已跳过自动发现:", e)
        return
    s.settimeout(1.0)
    log(f"UDP 自动发现已开启（端口 {DISCOVERY_PORT}）")
    reply = json.dumps({"app": "pccontrol", "name": socket.gethostname(),
                        "port": port, "ip": get_lan_ip()}).encode()
    while not stop.is_set():
        try:
            data, addr = s.recvfrom(256)
        except socket.timeout:
            continue
        except OSError:
            break
        if data.strip() == DISCOVERY_MAGIC:
            try:
                s.sendto(reply, addr)
            except OSError:
                pass
    s.close()


# ----------------------------------------------------------------------------
# main
# ----------------------------------------------------------------------------
def load_or_create_pin(fixed: str | None) -> str:
    if fixed:
        return fixed
    p = os.path.join(BASE_DIR, "pin.txt")
    if os.path.exists(p):
        t = open(p, encoding="utf-8").read().strip()
        if re.fullmatch(r"\d{6}", t):
            return t
    pin = f"{secrets.randbelow(900000) + 100000}"
    try:
        open(p, "w", encoding="utf-8").write(pin)
    except Exception:
        pass
    return pin


def main():
    global PIN, PORT
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--pin", default=None)
    ap.add_argument("--no-discovery", action="store_true")
    args = ap.parse_args()

    PORT = args.port
    PIN = load_or_create_pin(args.pin)

    if not IS_WIN:
        log("警告：当前不是 Windows，控制功能只会返回占位数据")

    stop = threading.Event()
    if not args.no_discovery:
        threading.Thread(target=discovery_worker, args=(PORT, stop), daemon=True).start()

    atexit.register(lambda: BRIGHT and BRIGHT.restore())

    srv = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    srv.daemon_threads = True

    ip = get_lan_ip()
    line = "=" * 52
    print(f"\n{line}")
    print(f"  {APP_NAME} v{APP_VERSION}  已启动")
    print(f"{line}")
    print(f"  电脑名称 : {socket.gethostname()}")
    print(f"  访问地址 : http://{ip}:{PORT}")
    print(f"  配对码   : {PIN}   (已保存到 pin.txt)")
    print(f"  音量控制 : {'可用' if AUDIO and AUDIO.ok else '不可用'}")
    print(f"  亮度控制 : {BRIGHT.mode if BRIGHT else '-'}"
          + (f"   HDR: {'开' if BRIGHT.hdr else '关'}" if BRIGHT else ""))
    if BRIGHT and BRIGHT.reason:
        print(f"  提示     : {BRIGHT.reason}")
    print(f"{line}")
    print("  手机与电脑需连同一个 Wi-Fi；手机浏览器可直接打开上面的地址。")
    print("  按 Ctrl+C 停止服务。\n")

    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print()
        log("正在停止…")
    finally:
        stop.set()
        if BRIGHT:
            BRIGHT.restore()
        srv.shutdown()
        log("已停止，屏幕亮度已恢复")


if __name__ == "__main__":
    main()
