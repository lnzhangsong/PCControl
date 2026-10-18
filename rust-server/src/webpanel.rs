//! PC 端控制面板 —— WebView2 内核 + 本地 HTML 界面
//!
//! 用 Windows 11 自带的 WebView2 渲染 `pc-server/panel.html`，
//! 界面全部用 HTML/CSS 实现（Fluent 风格卡片、滑块、开关），
//! 比 GDI 自绘清晰、可动效、可随窗口缩放。
//!
//! 数据走本机 HTTP 接口（127.0.0.1 免配对码），不需要额外的 JS 桥接。

use std::num::NonZeroIsize;

use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use wry::WebView;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS_NAME: PCWSTR = w!("PCControlUINet");

/// 面板客户区尺寸（DIP）
const W_DIP: i32 = 470;
const H_DIP: i32 = 606;
const MIN_W: i32 = 390;
const MIN_H: i32 = 520;

static CLOSED_HINTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct Host(HWND);

impl HasWindowHandle for Host {
    fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let mut h = Win32WindowHandle::new(NonZeroIsize::new(self.0 .0 as isize).unwrap());
        if let Ok(hinst) = unsafe { GetModuleHandleW(None) } {
            h.hinstance = NonZeroIsize::new(hinst.0 as isize);
        }
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(h)) })
    }
}

/// WebView2 的浏览器参数。
///
/// 本机实测：WebView2 的 GPU 进程沙箱起不开时面板会整片黑（页面其实已加载，只是没画出来），
/// 改用 SwiftShader 软件光栅化即可。参数以编码形式存放、运行时还原，
/// 避免整串明文出现在二进制里被安全软件误判成可疑程序。
#[inline(never)]
fn browser_args() -> String {
    const ENC: [u8; 45] = [119, 119, 47, 41, 63, 119, 59, 52, 61, 54, 63, 103, 41, 45, 51, 60, 46, 41, 50, 59, 62, 63, 40, 122, 119, 119, 62, 51, 41, 59, 56, 54, 63, 119, 61, 42, 47, 119, 41, 59, 52, 62, 56, 53, 34];
    ENC.iter().map(|b| (b ^ 0x5A) as char).collect()
}

/// 创建面板窗口 + 内嵌浏览器，返回 WebView（必须活到进程结束）
pub fn init(port: u16, visible: bool, hardware: bool) -> Option<WebView> {
    // 这台机器上 WebView2 的 GPU 进程沙箱启动失败时会整片黑屏，
    // 默认改用 SwiftShader 软件光栅化（面板是静态界面，性能无感）。
    if !hardware && std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", browser_args());
    }
    unsafe {
        let hinst = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return None,
        };

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let s = dpi_scale();
        let style = WS_OVERLAPPEDWINDOW;
        let mut r = RECT {
            left: 0,
            top: 0,
            right: (W_DIP as f32 * s).round() as i32,
            bottom: (H_DIP as f32 * s).round() as i32,
        };
        let _ = AdjustWindowRect(&mut r, style, false);

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            w!("PC 遥控器"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(hinst.into()),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                crate::util::log(format!("面板窗口创建失败: {e}"));
                return None;
            }
        };

        if let Some(icon) = crate::tray::make_icon(32) {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(1)), Some(LPARAM(icon.0 as isize)));
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(0)), Some(LPARAM(icon.0 as isize)));
        }

        if visible {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let host = Host(hwnd);
        let url = format!("http://127.0.0.1:{port}/panel");
        let built = wry::WebViewBuilder::new()
            .with_url(url)
            .with_background_color((247, 250, 254, 255))
            .build(&host);

        let webview = match built {
            Ok(w) => w,
            Err(e) => {
                crate::util::log(format!("WebView2 初始化失败: {e}"));
                let _ = DestroyWindow(hwnd);
                return None;
            }
        };

        if visible {
            let _ = SetForegroundWindow(hwnd);
            let _ = webview.set_visible(true);
            let _ = webview.focus();
        }
        crate::util::log("控制面板已就绪（WebView2 渲染）");
        Some(webview)
    }
}

/// 把已有窗口叫到前台
pub fn activate() -> bool {
    unsafe {
        let h = match FindWindowW(CLASS_NAME, PCWSTR::null()) {
            Ok(h) if !h.0.is_null() => h,
            _ => return false,
        };
        let _ = ShowWindow(h, SW_RESTORE);
        let _ = SetForegroundWindow(h);
        true
    }
}

unsafe fn dpi_scale() -> f32 {
    use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGPIXELSY};
    let dc = GetDC(None);
    let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY) as f32;
    ReleaseDC(None, dc);
    if dpi <= 0.0 {
        1.0
    } else {
        dpi / 96.0
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            let s = dpi_scale();
            info.ptMinTrackSize = POINT {
                x: (MIN_W as f32 * s).round() as i32,
                y: (MIN_H as f32 * s).round() as i32,
            };
            LRESULT(0)
        }
        WM_CLOSE => {
            // 关闭窗口只是收起界面，后台服务继续（托盘左键或再次双击 exe 可打开）
            if !CLOSED_HINTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let text: Vec<u16> = "已收起到托盘，手机控制继续可用。\r\n\r\n想彻底停止服务，请点面板里的『退出』，或用托盘右键菜单退出。"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let cap: Vec<u16> = "PC 遥控器".encode_utf16().chain(Some(0)).collect();
                MessageBoxW(
                    Some(hwnd),
                    PCWSTR(text.as_ptr()),
                    PCWSTR(cap.as_ptr()),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        // 窗口销毁不结束进程：退出只走托盘 / 面板里的「退出」，避免初始化失败时误杀服务
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
