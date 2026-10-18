//! 桌面截屏 —— 给手机端「实时看电脑画面」用
//!
//! 用 GDI 的 StretchBlt 把整个虚拟桌面**直接缩放**到目标尺寸，
//! 只取小图再编码 JPEG，避免每次都搬运 4K 全尺寸像素（那要 33 MB/帧）。
//!
//! 两个关键点（踩过坑）：
//! 1. `GetDIBits` 要求位图**没有被选进任何 DC**，必须先 `SelectObject` 还原再取像素；
//! 2. 截屏是 CPU 密集操作，同一时刻只允许一个线程抓，否则 4 个 HTTP 工作线程会互相拖死。

use std::sync::Mutex;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HGDIOBJ, HALFTONE, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

static CAPTURE_LOCK: Mutex<()> = Mutex::new(());

/// 抓一帧桌面并编码成 JPEG。`max_w` 是输出宽度上限（等比缩放），`quality` 1-100。
pub fn capture_jpeg(max_w: u32, quality: u8) -> Option<Vec<u8>> {
    // 上一帧还在编码就跳过本次请求，宁可掉帧也不要排队堆积
    let _guard = CAPTURE_LOCK.try_lock().ok()?;
    let (w, h, buf) = grab_scaled(max_w.max(320))?;
    to_jpeg(&buf, w, h, quality.clamp(30, 95))
}

/// 截取虚拟桌面并缩放到指定宽度，返回 (宽, 高, 自上而下 BGRA)
fn grab_scaled(max_w: u32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let sw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let sh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if sw <= 0 || sh <= 0 {
            return None;
        }

        // 目标尺寸：宽度不超过 max_w，等比缩放
        let scale = (max_w as f32 / sw as f32).min(1.0);
        let dw = ((sw as f32 * scale).round() as i32).max(1);
        let dh = ((sh as f32 * scale).round() as i32).max(1);

        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, dw, dh);
        if bmp.is_invalid() {
            let _ = ReleaseDC(None, screen);
            return None;
        }
        let old = SelectObject(mem, HGDIOBJ(bmp.0));

        // HALFTONE 让缩小后的画面不会变成锯齿块
        SetStretchBltMode(mem, HALFTONE);
        let _ = SetBrushOrgEx(mem, 0, 0, None);
        // CAPTUREBLT：把分层窗口（播放器、悬浮窗）也一起拍进去
        let _ = StretchBlt(
            mem, 0, 0, dw, dh, Some(screen), vx, vy, sw, sh,
            SRCCOPY | CAPTUREBLT,
        );

        // GetDIBits 要求位图未选入 DC，先把原对象换回去
        SelectObject(mem, old);

        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: dw,
                biHeight: -dh, // 负数 = 自上而下，省一次翻转
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buf = vec![0u8; (dw as usize) * (dh as usize) * 4];
        let got = GetDIBits(
            mem,
            bmp,
            0,
            dh as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut bi,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(None, screen);

        if got == 0 {
            return None;
        }
        Some((dw as u32, dh as u32, buf))
    }
}

fn to_jpeg(bgra: &[u8], w: u32, h: u32, quality: u8) -> Option<Vec<u8>> {
    // BGRA → RGB（JPEG 没有透明通道）
    let mut rgb = Vec::with_capacity((w as usize) * (h as usize) * 3);
    for px in bgra.chunks_exact(4) {
        rgb.push(px[2]);
        rgb.push(px[1]);
        rgb.push(px[0]);
    }
    let img = image::RgbImage::from_raw(w, h, rgb)?;

    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode_image(&img).ok()?;
    Some(out)
}

/// 桌面分辨率，用于给手机端提示
pub fn screen_size() -> (i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

#[allow(dead_code)]
fn _unused(_: HWND) {}
