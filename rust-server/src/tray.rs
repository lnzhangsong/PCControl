//! Windows 系统托盘图标 + 右键菜单
//!
//! PC 端的常驻界面入口：托盘图标（双击/单击打开控制台）、右键菜单
//! （打开控制台 / 查看配对码 / 退出）。控制台本身是内嵌网页，用浏览器
//! 的应用窗口模式打开，看起来就是一个独立的桌面 App。

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, ShellExecuteW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD,
    NIM_DELETE, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, IMAGE_ICON, LR_LOADFROMFILE};

const TRAY_MSG: u32 = WM_APP + 1;
const ID_OPEN: usize = 1;
const ID_INFO: usize = 2;
const ID_EXIT: usize = 3;
const ID_WEB: usize = 4;

static HWND_TRAY: AtomicIsize = AtomicIsize::new(0);
static ICON_HANDLE: AtomicIsize = AtomicIsize::new(0);
/// 资源管理器重启后会广播 TaskbarCreated，届时必须把图标重新挂上去
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub struct TrayInfo {
    pub port: u16,
    pub pin: String,
    pub name: String,
    pub ip: String,
}

static INFO: OnceLock<TrayInfo> = OnceLock::new();

/// 创建托盘图标（不进入消息循环，主循环由 main 统一跑）
pub fn install(info: TrayInfo) {
    let _ = INFO.set(info);
    unsafe {
        let hinst = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return,
        };
        let cls = w!("PCControlTrayWnd");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: cls,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            cls,
            w!("PC Remote"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };
        HWND_TRAY.store(hwnd.0 as isize, Ordering::SeqCst);

        let hicon = make_icon(32).unwrap_or_default();
        ICON_HANDLE.store(hicon.0 as isize, Ordering::SeqCst);
        let msg = RegisterWindowMessageW(w!("TaskbarCreated"));
        TASKBAR_CREATED.store(msg, Ordering::SeqCst);
        add_icon(hwnd, hicon);
    }
}

unsafe fn add_icon(hwnd: HWND, hicon: HICON) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = TRAY_MSG;
    nid.hIcon = hicon;
    let mut tip = String::from("PC Remote · 控制台运行中");
    if let Some(i) = INFO.get() {
        tip = format!("PC Remote · {} · {}:{}", i.name, i.ip, i.port);
    }
    write_tip(&mut nid.szTip, &tip);
    let ok = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();
    // 必须协商 v4 协议，否则 Windows 11 可能只在溢出区留个空占位、看不到图标
    nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let ver_ok = Shell_NotifyIconW(NIM_SETVERSION, &nid).as_bool();
    crate::util::log(format!(
        "托盘图标{}（v4 协议{}）",
        if ok { "已添加" } else { "添加失败" },
        if ver_ok { "已启用" } else { "未启用" }
    ));
}

/// 消息循环跑起来之后再补挂一次。
/// 在窗口刚创建、WebView2 还在初始化时注册的图标，偶尔会被资源管理器丢掉。
pub fn refresh_later() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        unsafe {
            let hwnd = HWND(HWND_TRAY.load(Ordering::SeqCst) as *mut _);
            let hicon = HICON(ICON_HANDLE.load(Ordering::SeqCst) as *mut _);
            if !hwnd.0.is_null() && !hicon.0.is_null() {
                add_icon(hwnd, hicon);
            }
        }
    });
}


unsafe fn remove_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

fn write_tip(buf: &mut [u16; 128], text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let n = units.len().min(126);
    buf[..n].copy_from_slice(&units[..n]);
    buf[n] = 0;
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 资源管理器重启：把托盘图标重新挂上
    let created = TASKBAR_CREATED.load(Ordering::SeqCst);
    if created != 0 && msg == created {
        let ico = ICON_HANDLE.load(Ordering::SeqCst);
        add_icon(hwnd, HICON(ico as *mut _));
        return LRESULT(0);
    }
    match msg {
        TRAY_MSG => {
            let ev = (lparam.0 as u32) & 0xFFFF;
            match ev {
                WM_LBUTTONUP => open_panel(),
                WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            match id {
                ID_OPEN => open_panel(),
                ID_WEB => open_console(),
                ID_INFO => show_info(hwnd),
                ID_EXIT => {
                    remove_icon(hwnd);
                    std::process::exit(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let Ok(menu) = CreatePopupMenu() else { return };
    let _ = AppendMenuW(menu, MF_STRING, ID_OPEN, w!("打开控制面板"));
    let _ = AppendMenuW(menu, MF_STRING, ID_WEB, w!("网页版控制台"));
    let _ = AppendMenuW(menu, MF_STRING, ID_INFO, w!("配对码 / 访问地址"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("退出"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
}

unsafe fn show_info(hwnd: HWND) {
    let (port, pin, ip) = match INFO.get() {
        Some(i) => (i.port, i.pin.clone(), i.ip.clone()),
        None => (8765, "------".into(), "0.0.0.0".into()),
    };
    let text = format!(
        "手机 App 配对码：{}\n\n控制面板：双击 pccontrol-server.exe 或托盘左键\n网页版：http://127.0.0.1:{}\n局域网访问：http://{}:{}\n\n本机打开不需要配对码。",
        pin, port, ip, port
    );
    let t: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let c: Vec<u16> = "PC Remote".encode_utf16().chain(Some(0)).collect();
    MessageBoxW(
        Some(hwnd),
        PCWSTR(t.as_ptr()),
        PCWSTR(c.as_ptr()),
        MB_OK | MB_ICONINFORMATION,
    );
}

fn open_panel() {
    // 优先唤出面板窗口（WebView2 面板 / 旧版原生面板）
    if crate::webpanel::activate() || crate::panel::activate_existing() {
        return;
    }
    unsafe { open_console() };
}

/// 打开网页版控制台：优先用 Edge/Chrome 的应用窗口模式（无地址栏，像一个独立 App）
unsafe fn open_console() {
    let port = INFO.get().map(|i| i.port).unwrap_or(8765);
    let url = format!("http://127.0.0.1:{}/", port);

    for exe in ["msedge.exe", "chrome.exe"] {
        if let Some(path) = find_browser(exe) {
            let args = format!("--app={}", url);
            let file: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let params: Vec<u16> = args.encode_utf16().chain(Some(0)).collect();
            let h = ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(file.as_ptr()),
                PCWSTR(params.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
            if h.0 as isize > 32 {
                return;
            }
        }
    }

    let u: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), None, None, SW_SHOWNORMAL);
}

fn find_browser(exe: &str) -> Option<String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Ok(dir) = std::env::var(var) {
            roots.push(PathBuf::from(dir));
        }
    }
    for root in roots {
        for sub in [
            "Microsoft\\Edge\\Application",
            "Google\\Chrome\\Application",
        ] {
            let p = root.join(sub).join(exe);
            if p.is_file() {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------- 图标绘制

const ICON_SIZE: u32 = 32;

/// 生成 HICON（16/24/32/48 都可以）
///
/// 优先从程序目录的 pc-remote.ico 读（LoadImage 走系统解码，最稳），
/// 读不到再退回自己拼 DIB 的方式。
pub fn make_icon(size: u32) -> Option<HICON> {
    let size = size.max(8);

    // 1) 优先用嵌在 exe 里的图标资源（由 build.rs 用 rc.exe 编译，格式最可靠）
    unsafe {
        if let Ok(hinst) = GetModuleHandleW(None) {
            if let Ok(h) = LoadImageW(
                Some(hinst.into()),
                PCWSTR(1 as *const u16), // MAKEINTRESOURCE(1)
                IMAGE_ICON,
                size as i32,
                size as i32,
                LR_DEFAULTCOLOR,
            ) {
                if !h.0.is_null() {
                    return Some(HICON(h.0));
                }
                crate::util::log("图标: 资源句柄为空");
            } else {
                crate::util::log(format!("图标: 资源加载失败 {}", unsafe { windows::Win32::Foundation::GetLastError().0 }));
            }
        }
    }

    // 2) 再试程序目录里的 pc-remote.ico
    if let Some(path) = icon_path() {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            if let Ok(h) = LoadImageW(
                None,
                PCWSTR(wide.as_ptr()),
                IMAGE_ICON,
                size as i32,
                size as i32,
                LR_LOADFROMFILE,
            ) {
                if !h.0.is_null() {
                    return Some(HICON(h.0));
                }
                crate::util::log("图标: 资源句柄为空");
            } else {
                crate::util::log(format!("图标: 资源加载失败 {}", unsafe { windows::Win32::Foundation::GetLastError().0 }));
            }
        }
    }
    let px = render_icon(size);
    unsafe { dib_to_icon(&px, size) }
}

fn icon_path() -> Option<String> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let p = dir.join("pc-remote.ico");
    if p.is_file() {
        Some(p.to_string_lossy().to_string())
    } else {
        None
    }
}

/// 画图标像素：蓝色圆角方块 + 白色显示器轮廓
fn render_icon(size: u32) -> Vec<f32> {
    let size = size.max(8);
    let s = ICON_SIZE as f32;
    let mut px = vec![0f32; (ICON_SIZE * ICON_SIZE * 4) as usize];

    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let i = ((y * ICON_SIZE + x) * 4) as usize;

            let d_bg = sd_round_rect(fx, fy, s / 2.0, s / 2.0, s / 2.0, s / 2.0, 7.5);
            let a_bg = cover(d_bg);
            if a_bg <= 0.0 {
                continue;
            }
            let t = fy / s;
            let base = mix((88.0, 138.0, 255.0), (40.0, 86.0, 212.0), t);

            // 显示器：外框 + 支架 + 底座
            let d_outer = sd_round_rect(fx, fy, 16.0, 14.5, 10.2, 6.0, 2.4);
            let ring = (1.2 - d_outer.abs()).clamp(0.0, 1.0);
            let stand = cover(sd_round_rect(fx, fy, 16.0, 22.6, 1.5, 1.6, 0.6));
            let base_bar = cover(sd_round_rect(fx, fy, 16.0, 25.6, 4.8, 1.0, 0.6));
            let a_white = ring.max(stand).max(base_bar);

            let (r, g, b) = (
                base.0 + (255.0 - base.0) * a_white,
                base.1 + (255.0 - base.1) * a_white,
                base.2 + (255.0 - base.2) * a_white,
            );
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            px[i + 3] = a_bg * 255.0;
        }
    }

    // 等比缩放（盒式平均）
    let src = ICON_SIZE as usize;
    let dst = size as usize;
    if dst == src {
        return px;
    }
    let mut out = vec![0f32; dst * dst * 4];
    let k = src as f32 / dst as f32;
    for y in 0..dst {
        for x in 0..dst {
            let (mut r, mut g, mut b, mut a, mut n) = (0f32, 0f32, 0f32, 0f32, 0f32);
            let y0 = (y as f32 * k) as usize;
            let y1 = (((y + 1) as f32 * k) as usize).min(src).max(y0 + 1);
            let x0 = (x as f32 * k) as usize;
            let x1 = (((x + 1) as f32 * k) as usize).min(src).max(x0 + 1);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = (yy * src + xx) * 4;
                    r += px[i];
                    g += px[i + 1];
                    b += px[i + 2];
                    a += px[i + 3];
                    n += 1.0;
                }
            }
            let o = (y * dst + x) * 4;
            out[o] = r / n;
            out[o + 1] = g / n;
            out[o + 2] = b / n;
            out[o + 3] = a / n;
        }
    }
    out
}

/// RGBA（顶部对齐，0-255）→ 图标 DIB（32bpp，底部对齐 + AND 掩码）
///
/// AND 掩码每行必须 4 字节对齐；早先按 w*h/8 写过，16/24 这类尺寸行宽不足，
/// 系统会把它当坏图标，显示成单色剪影。
fn rgba_to_dib(rgba: &[f32], w: usize, h: usize) -> Vec<u8> {
    let mask_stride = (w + 31) / 32 * 4;
    let mut dib = Vec::with_capacity(40 + w * h * 4 + mask_stride * h);
    let mut put = |v: u32| dib.extend_from_slice(&v.to_le_bytes());
    // BITMAPINFOHEADER 必须严格 40 字节（10 个 u32）。早先这里把 biPlanes 和
    // biBitCount 拆成两个 u32，多写了 4 字节，导致整块数据错位、图标变成无效资源，
    // 系统加载失败 → 托盘和标题栏都看不到图标。
    put(40); // biSize
    put(w as u32); // biWidth
    put((h * 2) as u32); // biHeight（XOR 位图 + AND 掩码，共 2 倍高）
    put(1 | (32 << 16)); // biPlanes = 1, biBitCount = 32
    put(0); // biCompression = BI_RGB
    put((w * h * 4 + mask_stride * h) as u32); // biSizeImage
    put(2835); // biXPelsPerMeter
    put(2835); // biYPelsPerMeter
    put(0); // biClrUsed
    put(0); // biClrImportant
    for y in (0..h).rev() {
        for x in 0..w {
            let i = (y * w + x) * 4;
            // DIB 是 BGRA 顺序，源数组是 RGBA
            dib.push(rgba[i + 2].round().clamp(0.0, 255.0) as u8);
            dib.push(rgba[i + 1].round().clamp(0.0, 255.0) as u8);
            dib.push(rgba[i].round().clamp(0.0, 255.0) as u8);
            dib.push(rgba[i + 3].round().clamp(0.0, 255.0) as u8);
        }
    }
    // AND 掩码：按 alpha 置位，透明处 = 1（抠图）。全 0 会让不支持 alpha 的
    // 老式绘制路径把整块画成不透明方块。
    let mut mask = vec![0u8; mask_stride * h];
    for y in 0..h {
        for x in 0..w {
            let a = rgba[(y * w + x) * 4 + 3];
            if a < 128.0 {
                let row = h - 1 - y; // AND 掩码同样是自下而上
                mask[row * mask_stride + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    dib.extend_from_slice(&mask);
    dib
}

unsafe fn dib_to_icon(rgba: &[f32], size: u32) -> Option<HICON> {
    let w = size as usize;
    let dib = rgba_to_dib(rgba, w, w);
    CreateIconFromResourceEx(&dib, true, 0x0003_0000, w as i32, w as i32, LR_DEFAULTCOLOR).ok()
}

/// 导出标准 .ico（16/24/32/48/64，32bpp + alpha），给桌面快捷方式用
pub fn write_ico_file(path: &std::path::Path) -> bool {
    let sizes = [16u32, 24, 32, 48, 64];
    let mut images: Vec<Vec<u8>> = Vec::new();
    for sz in sizes {
        let rgba = render_icon(sz);
        images.push(rgba_to_dib(&rgba, sz as usize, sz as usize));
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    let mut offset = 6 + 16 * images.len() as u32;
    for (idx, data) in images.iter().enumerate() {
        let sz = sizes[idx];
        out.push(sz as u8);
        out.push(sz as u8);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for data in &images {
        out.extend_from_slice(data);
    }
    std::fs::write(path, out).is_ok()
}


fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

fn mix(a: (f32, f32, f32), b: (f32, f32, f32), t: f32) -> (f32, f32, f32) {
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

