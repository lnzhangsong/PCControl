//! 显示器亮度（DDC/CI 真实背光）与 HDR 状态
//!
//! 优先级：DDC/CI 硬件背光。HDR 开启时显示器固件会锁死背光，此时如实返回
//! “不可写 + 原因”，由 App 提示用户关闭 HDR，不做假调光。

use std::sync::Mutex;
use std::time::Duration;

use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo,
    GetDisplayConfigBufferSizes, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, QueryDisplayConfig,
    SetVCPFeature, DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE, PHYSICAL_MONITOR, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{HANDLE, POINT};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};

const VCP_BRIGHTNESS: u8 = 0x10;

const INFO_GET_ADVANCED_COLOR: i32 = 9;
const INFO_SET_ADVANCED_COLOR: i32 = 10;

struct RawHandle(HANDLE);
// HANDLE 只在持有锁时使用，这里手工承诺可跨线程传递
unsafe impl Send for RawHandle {}

pub struct Display {
    inner: Mutex<Inner>,
}

struct Inner {
    mon: Option<RawHandle>,
    mode: String,
    reason: String,
}

impl Display {
    pub fn new() -> Self {
        let d = Display {
            inner: Mutex::new(Inner {
                mon: None,
                mode: "none".into(),
                reason: "未探测".into(),
            }),
        };
        d.probe();
        d
    }

    /// HDR 切换后重新探测背光通道是否可写
    pub fn new_probe_needed(&self) {
        self.probe();
    }

    /// 打开物理显示器句柄并验证背光通道是否真的可写
    fn probe(&self) {
        // 释放旧句柄，避免重复探测时泄漏
        if let Some(old) = self.inner.lock().unwrap().mon.take() {
            let _ = unsafe { DestroyPhysicalMonitor(old.0) };
        }

        let handle = open_monitor();
        let Some(h) = handle else {
            let mut g = self.inner.lock().unwrap();
            g.mode = "none".into();
            g.reason = "未找到支持 DDC/CI 的显示器".into();
            return;
        };

        let read = read_vcp(h.0);
        match read {
            Some((cur, max)) if max > 0 => {
                // 写回当前值，验证固件是否真的接受
                let writable = unsafe { SetVCPFeature(h.0, VCP_BRIGHTNESS, cur) != 0 };
                std::thread::sleep(Duration::from_millis(250));
                let after = read_vcp(h.0).map(|(c, _)| c);

                let (mode, reason) = if !writable {
                    ("none", "显示器拒绝了 DDC/CI 写入（部分型号在 OSD 中关闭了 DDC/CI）".to_string())
                } else if after != Some(cur) {
                    ("none", "背光被锁定：HDR 开启时显示器会忽略 DDC/CI 写入".to_string())
                } else {
                    ("ddc", "通过 DDC/CI 直接控制显示器背光".to_string())
                };

                let mut g = self.inner.lock().unwrap();
                g.mode = mode.into();
                g.reason = reason;
                g.mon = Some(h);
            }
            _ => {
                let _ = unsafe { DestroyPhysicalMonitor(h.0) };
                let mut g = self.inner.lock().unwrap();
                g.mode = "none".into();
                g.reason = "显示器不支持 DDC/CI 亮度读取".into();
            }
        }
    }

    pub fn mode(&self) -> String {
        self.inner.lock().unwrap().mode.clone()
    }

    pub fn reason(&self) -> String {
        let g = self.inner.lock().unwrap();
        if self.hdr_locked() {
            return "HDR 已开启，显示器背光被锁定；关闭 HDR 后可真实调节".to_string();
        }
        g.reason.clone()
    }

    fn hdr_locked(&self) -> bool {
        self.hdr()
    }

    /// 当前亮度百分比
    pub fn get(&self) -> Option<u32> {
        let h = {
            let mut g = self.inner.lock().unwrap();
            if g.mon.is_none() {
                g.mon = open_monitor();
            }
            g.mon.as_ref().map(|m| m.0)
        }?;
        let (cur, max) = read_vcp(h)?;
        if max == 0 {
            return None;
        }
        Some(((cur as f32 / max as f32) * 100.0).round().clamp(0.0, 100.0) as u32)
    }

    /// 设置亮度百分比，返回是否真正写入了硬件
    pub fn set(&self, percent: u32) -> bool {
        let percent = percent.min(100);
        if self.hdr() {
            return false;
        }
        let h = {
            let mut g = self.inner.lock().unwrap();
            if g.mon.is_none() {
                g.mon = open_monitor();
            }
            g.mon.as_ref().map(|m| m.0)
        };
        let Some(h) = h else { return false };
        let Some((_, max)) = read_vcp(h) else {
            // 句柄可能因显示器休眠失效，重建一次再试
            let mut g = self.inner.lock().unwrap();
            if let Some(old) = g.mon.take() {
                let _ = unsafe { DestroyPhysicalMonitor(old.0) };
            }
            drop(g);
            let Some(h2) = open_monitor() else { return false };
            let Some((_, max2)) = read_vcp(h2.0) else { return false };
            let target = (percent as f32 / 100.0 * max2 as f32).round() as u32;
            let ok = unsafe { SetVCPFeature(h2.0, VCP_BRIGHTNESS, target) != 0 };
            self.inner.lock().unwrap().mon = Some(h2);
            return ok;
        };
        let target = (percent as f32 / 100.0 * max as f32).round() as u32;
        unsafe { SetVCPFeature(h, VCP_BRIGHTNESS, target) != 0 }
    }

    /// 是否开启 HDR
    pub fn hdr(&self) -> bool {
        let paths = active_paths();
        let Some(p) = paths.first() else { return false };
        let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
        info.header = header(
            INFO_GET_ADVANCED_COLOR,
            std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
            p,
        );
        let r = unsafe { DisplayConfigGetDeviceInfo(&mut info.header as *mut _) };
        let bits = unsafe { info.Anonymous.Anonymous._bitfield };
        r == 0 && (bits >> 1) & 1 == 1
    }

    /// 开 / 关 HDR
    pub fn set_hdr(&self, enabled: bool) -> bool {
        let paths = active_paths();
        let mut ok = false;
        for p in &paths {
            let mut s = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE::default();
            s.header = header(
                INFO_SET_ADVANCED_COLOR,
                std::mem::size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>() as u32,
                p,
            );
            s.Anonymous.Anonymous._bitfield = if enabled { 1 } else { 0 };
            let r = unsafe { DisplayConfigSetDeviceInfo(&s.header as *const _) };
            if r == 0 {
                ok = true;
            }
        }
        // 显示器切换 HDR 需要一两秒重新协商
        std::thread::sleep(Duration::from_millis(1500));
        ok
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        if let Some(m) = self.inner.lock().unwrap().mon.take() {
            let _ = unsafe { DestroyPhysicalMonitor(m.0) };
        }
    }
}

fn open_monitor() -> Option<RawHandle> {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut count = 0u32;
        if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count).is_err() || count == 0 {
            return None;
        }
        let mut mons: Vec<PHYSICAL_MONITOR> = vec![Default::default(); count as usize];
        if GetPhysicalMonitorsFromHMONITOR(hmon, &mut mons).is_err() {
            return None;
        }
        for m in mons.iter().skip(1) {
            let _ = DestroyPhysicalMonitor(m.hPhysicalMonitor);
        }
        Some(RawHandle(mons[0].hPhysicalMonitor))
    }
}

fn read_vcp(h: HANDLE) -> Option<(u32, u32)> {
    unsafe {
        let mut cur = 0u32;
        let mut max = 0u32;
        let ok = GetVCPFeatureAndVCPFeatureReply(h, VCP_BRIGHTNESS, None, &mut cur, Some(&mut max));
        if ok != 0 {
            Some((cur, max))
        } else {
            None
        }
    }
}

fn header(
    ty: i32,
    size: u32,
    p: &DISPLAYCONFIG_PATH_INFO,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(ty),
        size,
        adapterId: p.targetInfo.adapterId,
        id: p.targetInfo.id,
    }
}

fn active_paths() -> Vec<DISPLAYCONFIG_PATH_INFO> {
    unsafe {
        let mut np = 0u32;
        let mut nm = 0u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return vec![];
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return vec![];
        }
        paths.truncate(np as usize);
        paths
    }
}
