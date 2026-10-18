# -*- coding: utf-8 -*-
"""
display_ctrl.py —— 通过 Windows DisplayConfig API 读取/设置 HDR 与 SDR 白点亮度

为什么需要它：
  显示器开启 HDR 后，背光由 HDR 元数据自动管理，DDC/CI(0x10) 写入会被显示器忽略。
  这时 Windows 自己用的亮度通道是 "SDR 白点亮度"(SDRWhiteLevel)，
  对应 设置 > 系统 > 显示 > HDR > "SDR 内容亮度" 滑块。
  本模块直接调用同名 API，效果与系统滑块一致，是真实的色彩管线调节。
"""
from __future__ import annotations

import ctypes
from ctypes import wintypes as wt

# ---- DisplayConfig 常量 ----
QDC_ONLY_ACTIVE_PATHS = 0x00000002
ERROR_SUCCESS = 0
GET_ADVANCED_COLOR_INFO = 9
SET_ADVANCED_COLOR_STATE = 10
GET_SDR_WHITE_LEVEL = 11
SET_SDR_WHITE_LEVEL = 12


class LUID(ctypes.Structure):
    _fields_ = [("LowPart", wt.DWORD), ("HighPart", wt.LONG)]


class RATIONAL(ctypes.Structure):
    _fields_ = [("Numerator", ctypes.c_uint32), ("Denominator", ctypes.c_uint32)]


class PATH_SOURCE_INFO(ctypes.Structure):
    _fields_ = [("adapterId", LUID), ("id", ctypes.c_uint32),
                ("modeInfoIdx", ctypes.c_uint32), ("statusFlags", ctypes.c_uint32)]


class PATH_TARGET_INFO(ctypes.Structure):
    _fields_ = [("adapterId", LUID), ("id", ctypes.c_uint32), ("modeInfoIdx", ctypes.c_uint32),
                ("outputTechnology", ctypes.c_uint32), ("rotation", ctypes.c_uint32),
                ("scaling", ctypes.c_uint32), ("refreshRate", RATIONAL),
                ("scanLineOrdering", ctypes.c_uint32), ("targetAvailable", ctypes.c_int),
                ("statusFlags", ctypes.c_uint32)]


class PATH_INFO(ctypes.Structure):
    _fields_ = [("sourceInfo", PATH_SOURCE_INFO),
                ("targetInfo", PATH_TARGET_INFO),
                ("flags", ctypes.c_uint32)]


class DEVICE_INFO_HEADER(ctypes.Structure):
    _fields_ = [("type", ctypes.c_uint32), ("size", ctypes.c_uint32),
                ("adapterId", LUID), ("id", ctypes.c_uint32)]


class ADVANCED_COLOR_INFO(ctypes.Structure):
    _fields_ = [("header", DEVICE_INFO_HEADER),
                ("value", ctypes.c_uint32),
                ("colorEncoding", ctypes.c_uint32),
                ("bitsPerColorChannel", ctypes.c_uint32)]


class SDR_WHITE_LEVEL(ctypes.Structure):
    _fields_ = [("header", DEVICE_INFO_HEADER), ("SDRWhiteLevel", ctypes.c_uint32)]


user32 = ctypes.windll.user32
try:
    user32.GetDisplayConfigBufferSizes.argtypes = [ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32), ctypes.POINTER(ctypes.c_uint32)]
    user32.QueryDisplayConfig.argtypes = [ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32), ctypes.POINTER(PATH_INFO),
                                          ctypes.POINTER(ctypes.c_uint32), ctypes.c_void_p, ctypes.c_void_p]
    user32.DisplayConfigGetDeviceInfo.argtypes = [ctypes.c_void_p]
    user32.DisplayConfigSetDeviceInfo.argtypes = [ctypes.c_void_p]
except Exception:
    pass


def _paths():
    """返回当前所有活动显示链路 (源 -> 目标)"""
    n_path, n_mode = ctypes.c_uint32(), ctypes.c_uint32()
    if user32.GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS,
                                          ctypes.byref(n_path), ctypes.byref(n_mode)) != 0:
        return []
    if n_path.value == 0:
        return []
    paths = (PATH_INFO * n_path.value)()
    # 模式数组不能传 NULL，否则返回 ERROR_INVALID_PARAMETER；x64 下每项 64 字节
    modes = ctypes.create_string_buffer(64 * max(1, n_mode.value))
    if user32.QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, ctypes.byref(n_path),
                                 paths, ctypes.byref(n_mode),
                                 ctypes.cast(modes, ctypes.c_void_p), None) != 0:
        return []
    return list(paths)[: n_path.value]


def _header(ptype: int, size: int, target: PATH_TARGET_INFO) -> DEVICE_INFO_HEADER:
    h = DEVICE_INFO_HEADER()
    h.type = ptype
    h.size = size
    h.adapterId = target.adapterId
    h.id = target.id
    return h


def list_outputs():
    """每个活动输出：HDR 是否支持/开启、当前 SDR 白点亮度"""
    out = []
    for i, p in enumerate(_paths()):
        t = p.targetInfo
        info = ADVANCED_COLOR_INFO()
        info.header = _header(GET_ADVANCED_COLOR_INFO, ctypes.sizeof(info), t)
        hdr_supported = hdr_enabled = None
        if user32.DisplayConfigGetDeviceInfo(ctypes.byref(info)) == ERROR_SUCCESS:
            hdr_supported = bool(info.value & 0x1)
            hdr_enabled = bool(info.value & 0x2)
        white = None
        w = SDR_WHITE_LEVEL()
        w.header = _header(GET_SDR_WHITE_LEVEL, ctypes.sizeof(w), t)
        if user32.DisplayConfigGetDeviceInfo(ctypes.byref(w)) == ERROR_SUCCESS:
            white = int(w.SDRWhiteLevel)
        out.append({"index": i, "hdrSupported": hdr_supported,
                    "hdrEnabled": hdr_enabled, "sdrWhiteLevel": white})
    return out


def any_hdr_enabled() -> bool:
    return any(o["hdrEnabled"] for o in list_outputs())


def get_sdr_white() -> int | None:
    for o in list_outputs():
        if o["sdrWhiteLevel"]:
            return o["sdrWhiteLevel"]
    return None


def set_sdr_white(value: int) -> bool:
    """对所有开启 HDR 的输出设置 SDR 白点亮度（单位：1/80 nit，1000 = 80 nit）"""
    ok = False
    for p in _paths():
        w = SDR_WHITE_LEVEL()
        w.header = _header(SET_SDR_WHITE_LEVEL, ctypes.sizeof(w), p.targetInfo)
        w.SDRWhiteLevel = int(value)
        if user32.DisplayConfigSetDeviceInfo(ctypes.byref(w)) == ERROR_SUCCESS:
            ok = True
    return ok


class _SetAdvancedColorState(ctypes.Structure):
    _fields_ = [("header", DEVICE_INFO_HEADER), ("value", ctypes.c_uint32)]


def set_hdr_enabled(enabled: bool) -> bool:
    """开启/关闭 HDR。HDR 关闭后显示器才接受 DDC/CI 背光写入。"""
    ok = False
    for p in _paths():
        s = _SetAdvancedColorState()
        s.header = _header(SET_ADVANCED_COLOR_STATE, ctypes.sizeof(s), p.targetInfo)
        s.value = 0x1 if enabled else 0x0
        if user32.DisplayConfigSetDeviceInfo(ctypes.byref(s)) == ERROR_SUCCESS:
            ok = True
    return ok


# ---- 百分比 <-> SDR 白点亮度 ----
NITS_MIN = 80.0     # 0%   对应 80 nit
NITS_MAX = 480.0    # 100% 对应 480 nit
UNIT = 80.0 / 1000.0   # 1 单位 = 0.08 nit


def percent_to_white(pct: float) -> int:
    pct = max(0.0, min(100.0, float(pct)))
    nits = NITS_MIN * (NITS_MAX / NITS_MIN) ** (pct / 100.0)
    return int(round(nits / UNIT))


def white_to_percent(white: int) -> int:
    if not white:
        return 100
    nits = white * UNIT
    import math
    pct = 100.0 * math.log(nits / NITS_MIN) / math.log(NITS_MAX / NITS_MIN)
    return int(round(max(0.0, min(100.0, pct))))


if __name__ == "__main__":
    print("活动输出:", list_outputs())
    print("HDR 是否开启:", any_hdr_enabled())
    cur = get_sdr_white()
    print("当前 SDR 白点:", cur, "→", white_to_percent(cur) if cur else None, "%")
    if cur:
        test = percent_to_white(max(0, white_to_percent(cur) - 15))
        print("尝试写入", test, "→", set_sdr_white(test))
        import time
        time.sleep(0.3)
        print("回读:", get_sdr_white())
        print("还原 →", set_sdr_white(cur), "回读:", get_sdr_white())
