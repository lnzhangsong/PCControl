//! 系统音量控制（Windows Core Audio）

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

/// 每次操作都重新取默认输出设备，这样插拔耳机 / 切换设备后依然正确
fn endpoint() -> Option<IAudioEndpointVolume> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev: IMMDevice = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

pub fn available() -> bool {
    endpoint().is_some()
}

/// 返回 (百分比 0-100, 是否静音)
pub fn get() -> Option<(u32, bool)> {
    let vol = endpoint()?;
    unsafe {
        let scalar = vol.GetMasterVolumeLevelScalar().ok()?;
        let muted = vol.GetMute().ok()?.as_bool();
        Some(((scalar * 100.0).round().clamp(0.0, 100.0) as u32, muted))
    }
}

pub fn set(percent: u32) -> bool {
    let percent = percent.min(100);
    match endpoint() {
        Some(vol) => unsafe {
            vol.SetMasterVolumeLevelScalar(percent as f32 / 100.0, std::ptr::null())
                .is_ok()
        },
        None => false,
    }
}

/// explicit 为 Some 时按其值设置，None 表示切换
pub fn set_mute(explicit: Option<bool>) -> bool {
    let vol = match endpoint() {
        Some(v) => v,
        None => return false,
    };
    unsafe {
        let current = vol.GetMute().ok().map(|b| b.as_bool()).unwrap_or(false);
        let target = explicit.unwrap_or(!current);
        vol.SetMute(target, std::ptr::null()).is_ok()
    }
}
