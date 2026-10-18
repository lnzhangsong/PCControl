//! 本机信息与日志

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::net::UdpSocket;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersInfo, IP_ADAPTER_INFO};
use windows::Win32::System::SystemInformation::GetLocalTime;

static LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn init_log(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("server.log");
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    let _ = LOG_FILE.set(Mutex::new(file));
}

/// 输出到控制台，同时追加到日志文件（开机自启时窗口是隐藏的）
pub fn log(msg: impl AsRef<str>) {
    let line = format!("[{}] {}\n", now_hms(), msg.as_ref());
    print!("{}", line);
    let _ = std::io::stdout().flush();
    if let Some(lock) = LOG_FILE.get() {
        if let Ok(mut guard) = lock.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
    }
}

fn now_hms() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

/// 本机在局域网中的地址：靠一次不会真正发出去的 UDP connect 取出口网卡
pub fn lan_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("223.5.5.5:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

/// 第一块带默认网关的网卡 MAC（用于网络唤醒）
pub fn mac_address() -> String {
    unsafe {
        let mut size = 0u32;
        GetAdaptersInfo(None, &mut size);
        if size == 0 {
            return String::new();
        }
        let mut buf = vec![0u8; size as usize];
        let ret = GetAdaptersInfo(Some(buf.as_mut_ptr() as *mut IP_ADAPTER_INFO), &mut size);
        if ret != 0 {
            return String::new();
        }
        let mut p = buf.as_ptr() as *const IP_ADAPTER_INFO;
        while !p.is_null() {
            let a = &*p;
            let gw = &a.GatewayList.IpAddress.String;
            let has_gateway = gw[0] != 0 && gw[0] as u8 != b'0';
            if has_gateway && a.AddressLength == 6 {
                let mac = (0..6)
                    .map(|i| format!("{:02X}", a.Address[i] as u8))
                    .collect::<Vec<_>>()
                    .join(":");
                return mac;
            }
            p = a.Next;
        }
        String::new()
    }
}

// ---------------------------------------------------------------- 系统交互

/// 复制文本到剪贴板
pub fn set_clipboard(text: &str) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    unsafe {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(h);
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p as *mut u16, wide.len());
                let _ = GlobalUnlock(h);
            }
            // CF_UNICODETEXT = 13
            let _ = SetClipboardData(13, Some(HANDLE(h.0)));
        }
        let _ = CloseClipboard();
    }
}

fn shell_open(target: &str) {
    use windows::core::w;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            windows::core::PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

pub fn open_url(url: &str) {
    shell_open(url);
}

pub fn open_path(path: &str) {
    shell_open(path);
}
