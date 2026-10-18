//! 剪贴板读写（手机 ↔ 电脑共用一块剪贴板）
//!
//! 剪贴板是全系统共享资源，随时可能被别的程序占着，
//! `OpenClipboard` 失败很正常，重试几次就好，不能一次失败就报错。

use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// CF_UNICODETEXT。这个常量不在 DataExchange 模块里，直接写值（13）
const CF_UNICODETEXT: u32 = 13;

/// 读电脑剪贴板里的文本
pub fn get_text() -> Option<String> {
    unsafe {
        let mut opened = false;
        for _ in 0..6 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if !opened {
            return None;
        }

        let result = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
            if handle.is_invalid() {
                return None;
            }
            let ptr = GlobalLock(HGLOBAL(handle.0));
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            let p = ptr as *const u16;
            // 上限保护，避免异常数据把内存扫穿
            while len < 1_000_000 && *p.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(HGLOBAL(handle.0));
            Some(text)
        })();

        let _ = CloseClipboard();
        result
    }
}

/// 把文本写进电脑剪贴板
pub fn set_text(text: &str) -> bool {
    unsafe {
        let mut opened = false;
        for _ in 0..6 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if !opened {
            return false;
        }

        let ok = (|| {
            let _ = EmptyClipboard();
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            wide.push(0);
            let bytes = wide.len() * 2;

            let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
            let ptr = GlobalLock(hmem);
            if ptr.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, bytes);
            let _ = GlobalUnlock(hmem);

            // 成功后内存归系统所有，不能再释放
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hmem.0)))
                .ok()
                .map(|_| ())
        })()
        .is_some();

        let _ = CloseClipboard();
        ok
    }
}

/// 上传文件统一放在这里，方便用户找
pub fn upload_dir() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
    let downloads = std::path::Path::new(&home).join("Downloads");
    let base = if downloads.is_dir() {
        downloads
    } else {
        std::path::Path::new(&home).to_path_buf()
    };
    base.join("PC Remote")
}

/// 只取文件名，去掉路径分隔符和 Windows 不允许的字符，防目录穿越
pub fn safe_file_name(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let cleaned: String = base
        .chars()
        .filter(|c| !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'))
        .take(120)
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        "upload.bin".to_string()
    } else {
        cleaned
    }
}

/// 同名文件不覆盖，自动加 (1)(2) 后缀
pub fn unique_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..10_000 {
        let p = dir.join(format!("{stem} ({i}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{ext}", std::process::id()))
}
