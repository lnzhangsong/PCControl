//! 媒体键、锁屏 / 休眠 / 关屏

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, HWND_BROADCAST, WM_SYSCOMMAND};

const SC_MONITORPOWER: usize = 0xF170;
const MONITOR_OFF: isize = 2;

pub fn media(action: &str) -> bool {
    let vk: u8 = match action {
        "play_pause" => 0xB3,
        "next" => 0xB0,
        "prev" => 0xB1,
        "stop" => 0xB2,
        "vol_up" => 0xAF,
        "vol_down" => 0xAE,
        "mute_key" => 0xAD,
        _ => return false,
    };
    unsafe {
        keybd_event(vk, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(vk, 0, KEYBD_EVENT_FLAGS(2), 0);
    }
    true
}

pub fn act(action: &str) -> bool {
    match action {
        "lock" => unsafe { LockWorkStation().is_ok() },
        "sleep" => unsafe { SetSuspendState(false, true, false) },
        "screen_off" => {
            unsafe {
                SendMessageW(
                    HWND_BROADCAST,
                    WM_SYSCOMMAND,
                    Some(WPARAM(SC_MONITORPOWER)),
                    Some(LPARAM(MONITOR_OFF)),
                );
            }
            true
        }
        "shutdown" => shutdown(),
        _ => false,
    }
}

/// 关机。普通用户默认没有启用 SeShutdownPrivilege，必须先自己打开。
fn shutdown() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Shutdown::{ExitWindowsEx, EWX_FORCEIFHUNG, EWX_POWEROFF};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return false;
        }

        let name: Vec<u16> = "SeShutdownPrivilege\0".encode_utf16().collect();
        let mut luid = Default::default();
        if LookupPrivilegeValueW(None, PCWSTR(name.as_ptr()), &mut luid).is_ok() {
            let mut tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let _ = AdjustTokenPrivileges(token, false, Some(&mut tp), 0, None, None);
        }
        let _ = CloseHandle(token);

        ExitWindowsEx(EWX_POWEROFF | EWX_FORCEIFHUNG, Default::default()).is_ok()
    }
}
