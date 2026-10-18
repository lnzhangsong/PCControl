//! PC 端本地控制面板：原生 Win32 窗口（GDI 自绘）
//!
//! 面向"坐在电脑前"的场景：看服务状态、拿配对码/地址、控制开机自启、看日志。
//! 不是把手机 App 的遥控界面照搬过来。

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::http::AppState;

const CLASS_NAME: PCWSTR = w!("PCControlPanelWnd");

// 颜色（与 App 保持一致）
const C_BG: COLORREF = COLORREF(0x00F7F5F4); // #F4F5F7
const C_CARD: COLORREF = COLORREF(0x00FFFFFF);
const C_LINE: COLORREF = COLORREF(0x00ECE8E6); // #E6E8EC
const C_TEXT: COLORREF = COLORREF(0x001F1814); // #14181F
const C_SUB: COLORREF = COLORREF(0x0094847B); // #7B8494
const C_ACCENT: COLORREF = COLORREF(0x00F56E3B);
const C_ACCENT_SOFT: COLORREF = COLORREF(0x00FCF6E8);
const C_OK: COLORREF = COLORREF(0x0073B61F);

const W_DIP: i32 = 420;
const H_DIP: i32 = 528;

pub struct PanelInfo {
    pub name: String,
    pub port: u16,
    pub pin: String,
    pub ip: String,
    pub dir: PathBuf,
}

#[derive(Default, Clone)]
pub struct Live {
    pub volume: Option<u32>,
    pub muted: bool,
    pub brightness: Option<u32>,
    pub mode: String,
    pub last_client: Option<(String, u64)>, // (ip, 多久之前·秒)
    pub autostart: bool,
}

static INFO: OnceLock<PanelInfo> = OnceLock::new();
static STATE: OnceLock<Arc<AppState>> = OnceLock::new();
static LIVE: OnceLock<Mutex<Live>> = OnceLock::new();
static BTNS: OnceLock<Mutex<Vec<(u32, RECT)>>> = OnceLock::new();
static HOVER: Mutex<u32> = Mutex::new(0);
static COPIED_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);
static CLOSED_HINTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const ID_COPY: u32 = 1;
const ID_WEB: u32 = 2;
const ID_LOG: u32 = 3;
const ID_EXIT: u32 = 4;
const ID_MUTE: u32 = 5;
const ID_VOL_DOWN: u32 = 6;
const ID_VOL_UP: u32 = 7;
const ID_BRI_DOWN: u32 = 8;
const ID_BRI_UP: u32 = 9;
const ID_AUTOSTART: u32 = 10;

/// 已有实例则把它的窗口叫到前台，返回 true
pub fn activate_existing() -> bool {
    unsafe {
        let h = match FindWindowW(CLASS_NAME, PCWSTR::null()) {
            Ok(h) if !h.0.is_null() => h,
            _ => return false,
        };
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
        true
    }
}

pub fn show(info: PanelInfo, state: Arc<AppState>) {
    let _ = INFO.set(info);
    let _ = STATE.set(state);
    let _ = LIVE.set(Mutex::new(Live::default()));
    let _ = BTNS.set(Mutex::new(Vec::new()));

    spawn_poller();

    unsafe {
        let hinst = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return,
        };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let s = scale();
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut r = RECT {
            left: 0,
            top: 0,
            right: (W_DIP as f32 * s).round() as i32,
            bottom: (H_DIP as f32 * s).round() as i32,
        };
        let _ = AdjustWindowRect(&mut r, style, false);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            w!("PC 遥控器 · 电脑端"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(hinst.into()),
            None,
        );
        if let Ok(hwnd) = hwnd {
            if let Some(icon) = crate::tray::make_icon(32) {
                SendMessageW(hwnd, WM_SETICON, Some(WPARAM(1)), Some(LPARAM(icon.0 as isize)));
                SendMessageW(hwnd, WM_SETICON, Some(WPARAM(0)), Some(LPARAM(icon.0 as isize)));
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
            SetTimer(Some(hwnd), 1, 1000, None);
        }
    }
}

unsafe fn scale() -> f32 {
    let dc = GetDC(None);
    let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY) as f32;
    ReleaseDC(None, dc);
    if dpi <= 0.0 {
        1.0
    } else {
        dpi / 96.0
    }
}

fn spawn_poller() {
    std::thread::spawn(|| {
        let mut tick = 0u32;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            tick = tick.wrapping_add(1);
            let mut live = Live::default();

            if let Some((vol, muted)) = crate::audio::get() {
                live.volume = Some(vol);
                live.muted = muted;
            }
            if let Some(st) = STATE.get() {
                if tick % 3 == 1 {
                    live.brightness = st.display.get();
                    live.mode = st.display.mode();
                } else {
                    let prev = snapshot();
                    live.brightness = prev.brightness;
                    live.mode = prev.mode;
                }
                if let Some((ip, at)) = st.last_client.lock().unwrap().clone() {
                    live.last_client = Some((ip, at.elapsed().as_secs()));
                }
            }
            if tick % 4 == 2 {
                live.autostart = autostart_enabled();
            } else {
                live.autostart = snapshot().autostart;
            }

            if let Some(m) = LIVE.get() {
                *m.lock().unwrap() = live;
            }
            if let Some(hwnd) = panel_hwnd() {
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
        }
    });
}

fn panel_hwnd() -> Option<HWND> {
    unsafe {
        match FindWindowW(CLASS_NAME, PCWSTR::null()) {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    }
}

fn snapshot() -> Live {
    LIVE.get()
        .map(|m| m.lock().unwrap().clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- 自启开关

fn startup_lnk() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    PathBuf::from(appdata)
        .join("Microsoft\\Windows\\Start Menu\\Programs\\Startup")
        .join("PC Remote Server.lnk")
}

/// 早期版本会写一个占位文件冒充快捷方式，这里识别并清掉
fn is_placeholder(p: &std::path::Path) -> bool {
    let placeholder = std::fs::read(p)
        .map(|b| b.starts_with(b"enabled-by-panel"))
        .unwrap_or(false);
    if placeholder {
        let _ = std::fs::remove_file(p);
    }
    placeholder
}

/// 是否已开启开机自启
pub fn autostart_enabled() -> bool {
    if startup_lnk().is_file() && !is_placeholder(&startup_lnk()) {
        return true;
    }
    unsafe {
        let mut key = HKEY::default();
        let sub = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let opened = RegOpenKeyExW(HKEY_CURRENT_USER, sub, Some(0), KEY_QUERY_VALUE, &mut key).is_ok();
        let mut size = 0u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            sub,
            w!("PCControlServer"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        );
        if opened {
            let _ = RegCloseKey(key);
        }
        r.is_ok() && size > 0
    }
}

/// 打开 / 关闭开机自启（写 HKCU Run 项，登录后用隐藏脚本拉起）
pub fn set_autostart(on: bool) {
    unsafe { set_autostart_inner(on) }
}

unsafe fn set_autostart_inner(on: bool) {
    let sub = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    if !on {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub, Some(0), KEY_SET_VALUE, &mut key).is_ok() {
            let _ = RegDeleteValueW(key, w!("PCControlServer"));
            let _ = RegCloseKey(key);
        }
        let _ = std::fs::remove_file(startup_lnk());
        return;
    }

    let dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| INFO.get().map(|i| i.dir.clone()).unwrap_or_default());
    let vbs = dir.join("start-hidden.vbs");
    let cmd = format!(
        "\"{}\\wscript.exe\" \"{}\"",
        std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()),
        vbs.display()
    );

    let mut key = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, sub, Some(0), KEY_SET_VALUE, &mut key).is_ok() {
        let wide: Vec<u16> = cmd.encode_utf16().chain(Some(0)).collect();
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let _ = RegSetValueExW(key, w!("PCControlServer"), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
    }
}

// ---------------------------------------------------------------- 绘制

struct P<'a> {
    hdc: HDC,
    s: f32,
    fonts: &'a mut Vec<HFONT>,
    btns: &'a mut Vec<(u32, RECT)>,
}

fn d(v: f32, s: f32) -> i32 {
    (v * s).round() as i32
}

impl<'a> P<'a> {
    fn font(&mut self, size: f32, weight: i32) -> HFONT {
        let h = -((size * self.s * 1.34).round() as i32);
        let f = unsafe {
            CreateFontW(
                h, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH.0 as u32, w!("Segoe UI"),
            )
        };
        self.fonts.push(f);
        f
    }

    fn text(&mut self, s: &str, x: i32, y: i32, wd: i32, ht: i32, size: f32, weight: i32, color: COLORREF, center: bool) {
        unsafe {
            let f = self.font(size, weight);
            let old = SelectObject(self.hdc, f.into());
            SetBkMode(self.hdc, TRANSPARENT);
            SetTextColor(self.hdc, color);
            let mut buf: Vec<u16> = s.encode_utf16().collect();
            let mut r = RECT { left: x, top: y, right: x + wd, bottom: y + ht };
            let fmt = if center { DT_CENTER | DT_VCENTER | DT_SINGLELINE } else { DT_LEFT | DT_VCENTER | DT_SINGLELINE };
            DrawTextW(self.hdc, &mut buf, &mut r, fmt);
            SelectObject(self.hdc, old);
        }
    }

    fn fill(&mut self, x: i32, y: i32, wd: i32, ht: i32, color: COLORREF) {
        unsafe {
            let br = CreateSolidBrush(color);
            let mut r = RECT { left: x, top: y, right: x + wd, bottom: y + ht };
            FillRect(self.hdc, &r, br);
            let _ = DeleteObject(br.into());
        }
    }

    fn round(&mut self, x: i32, y: i32, wd: i32, ht: i32, radius: i32, fill: COLORREF, border: Option<COLORREF>) {
        unsafe {
            let br = CreateSolidBrush(fill);
            let pen = match border {
                Some(c) => CreatePen(PS_SOLID, 1, c),
                None => CreatePen(PS_NULL, 0, COLORREF(0)),
            };
            let ob = SelectObject(self.hdc, br.into());
            let op = SelectObject(self.hdc, pen.into());
            let _ = RoundRect(self.hdc, x, y, x + wd, y + ht, radius * 2, radius * 2);
            SelectObject(self.hdc, ob);
            SelectObject(self.hdc, op);
            let _ = DeleteObject(br.into());
            let _ = DeleteObject(pen.into());
        }
    }

    fn button(&mut self, id: u32, x: i32, y: i32, wd: i32, ht: i32, label: &str, primary: bool) {
        let hovered = *HOVER.lock().unwrap() == id;
        let fill = if primary {
            if hovered { COLORREF(0x00E85B2D) } else { C_ACCENT }
        } else if hovered {
            COLORREF(0x00F5F3F1)
        } else {
            COLORREF(0x00FBFAF9)
        };
        self.round(x, y, wd, ht, d(10.0, self.s), fill, Some(C_LINE));
        let color = if primary { COLORREF(0x00FFFFFF) } else { C_TEXT };
        let size = 11.5;
        self.text(label, x, y, wd, ht, size, 600, color, true);
        self.btns.push((id, RECT { left: x, top: y, right: x + wd, bottom: y + ht }));
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let (w, h) = (rc.right, rc.bottom);

    // 双缓冲
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem, bmp.into());

    let s = scale();
    let info = INFO.get();
    let lv = snapshot();

    let mut fonts: Vec<HFONT> = Vec::new();
    let mut btns: Vec<(u32, RECT)> = Vec::new();
    {
        let mut p = P { hdc: mem, s, fonts: &mut fonts, btns: &mut btns };

        let pad = d(18.0, s);
        let cw = w - pad * 2;
        p.fill(0, 0, w, h, C_BG);

        // ---- 卡片 1：状态
        let c1 = d(18.0, s);
        let h1 = d(92.0, s);
        p.round(pad, c1, cw, h1, d(16.0, s), C_CARD, Some(C_LINE));

        let recent = match &lv.last_client {
            Some((ip, secs)) if *secs < 90 => format!("手机已连接 · {} · {} 秒前", ip, secs),
            Some((ip, secs)) => format!("上次连接 {} · {} 分钟前", ip, secs / 60),
            None => "还没有手机连过".to_string(),
        };
        let is_live = matches!(&lv.last_client, Some((_, s)) if *s < 90);
        let dot = d(4.5, s);
        p.round(pad + d(18.0, s), c1 + d(28.0, s), dot * 2, dot * 2, dot, C_OK, None);
        p.text("服务运行中", pad + d(34.0, s), c1 + d(14.0, s), cw - d(120.0, s), d(24.0, s), 15.0, 700, C_TEXT, false);
        let uptime = STATE.get().map(|st| st.start.elapsed().as_secs()).unwrap_or(0);
        let up = if uptime < 60 {
            format!("{} 秒", uptime)
        } else if uptime < 3600 {
            format!("{} 分钟", uptime / 60)
        } else {
            format!("{} 小时 {} 分", uptime / 3600, uptime % 3600 / 60)
        };
        p.text(
            &format!("{} · 端口 {} · 已运行 {}", info.map(|i| i.name.as_str()).unwrap_or("-"), info.map(|i| i.port).unwrap_or(8765), up),
            pad + d(34.0, s),
            c1 + d(38.0, s),
            cw - d(40.0, s),
            d(20.0, s),
            11.5,
            400,
            C_SUB,
            false,
        );
        p.text(&recent, pad + d(34.0, s), c1 + d(58.0, s), cw - d(40.0, s), d(20.0, s), 11.5, 400, if is_live { C_OK } else { C_SUB }, false);

        // ---- 卡片 2：连接信息
        let c2 = c1 + h1 + d(12.0, s);
        let h2 = d(168.0, s);
        p.round(pad, c2, cw, h2, d(16.0, s), C_CARD, Some(C_LINE));
        p.text("手机 App 连接用", pad + d(16.0, s), c2 + d(12.0, s), cw - d(32.0, s), d(18.0, s), 11.5, 400, C_SUB, false);

        let addr = format!("{}:{}", info.map(|i| i.ip.as_str()).unwrap_or("-"), info.map(|i| i.port).unwrap_or(8765));
        p.text(&addr, pad + d(16.0, s), c2 + d(32.0, s), cw - d(110.0, s), d(30.0, s), 17.0, 700, C_TEXT, false);
        let bw = d(72.0, s);
        let copied = COPIED_AT
            .lock()
            .unwrap()
            .map(|t| t.elapsed().as_millis() < 1500)
            .unwrap_or(false);
        p.button(
            ID_COPY,
            pad + cw - bw - d(14.0, s),
            c2 + d(32.0, s),
            bw,
            d(30.0, s),
            if copied { "已复制" } else { "复制" },
            false,
        );

        p.text("配对码", pad + d(16.0, s), c2 + d(72.0, s), d(60.0, s), d(18.0, s), 11.5, 400, C_SUB, false);
        p.text(
            info.map(|i| i.pin.as_str()).unwrap_or("------"),
            pad + d(16.0, s),
            c2 + d(88.0, s),
            cw - d(32.0, s),
            d(44.0, s),
            29.0,
            700,
            C_ACCENT,
            false,
        );
        p.text(
            "同一网络下，手机填这个地址和配对码即可。",
            pad + d(16.0, s),
            c2 + d(134.0, s),
            cw - d(32.0, s),
            d(20.0, s),
            10.5,
            400,
            C_SUB,
            false,
        );

        // ---- 卡片 3：实时状态（音量 / 亮度）
        let c3 = c2 + h2 + d(12.0, s);
        let h3 = d(96.0, s);
        p.round(pad, c3, cw, h3, d(16.0, s), C_CARD, Some(C_LINE));
        let row_h = d(40.0, s);

        // 音量行
        let y1 = c3 + d(8.0, s);
        p.text("音量", pad + d(16.0, s), y1, d(50.0, s), row_h, 12.5, 500, C_SUB, false);
        let vol_txt = match lv.volume {
            Some(v) if lv.muted => format!("{v}% · 已静音"),
            Some(v) => format!("{v}%"),
            None => "不可用".to_string(),
        };
        let btn = d(30.0, s);
        let gap = d(6.0, s);
        let mute_w = d(52.0, s);
        let right = pad + cw - d(14.0, s);
        p.button(ID_VOL_UP, right - btn, y1 + d(5.0, s), btn, d(28.0, s), "+", false);
        p.button(ID_VOL_DOWN, right - btn * 2 - gap, y1 + d(5.0, s), btn, d(28.0, s), "−", false);
        p.button(ID_MUTE, right - btn * 2 - gap * 2 - mute_w, y1 + d(5.0, s), mute_w, d(28.0, s), if lv.muted { "取消静音" } else { "静音" }, false);
        p.text(&vol_txt, pad + d(80.0, s), y1, cw - d(230.0, s), row_h, 14.0, 700, C_TEXT, false);

        // 亮度行
        let y2 = y1 + row_h + d(2.0, s);
        p.text("亮度", pad + d(16.0, s), y2, d(50.0, s), row_h, 12.5, 500, C_SUB, false);
        let bri_txt = match lv.brightness {
            Some(v) => format!("{v}%"),
            None => "不可用".to_string(),
        };
        p.button(ID_BRI_UP, right - btn, y2 + d(5.0, s), btn, d(28.0, s), "+", false);
        p.button(ID_BRI_DOWN, right - btn * 2 - gap, y2 + d(5.0, s), btn, d(28.0, s), "−", false);
        p.text(&bri_txt, pad + d(80.0, s), y2, cw - d(180.0, s), row_h, 14.0, 700, C_TEXT, false);

        // ---- 卡片 4：开机自启
        let c4 = c3 + h3 + d(12.0, s);
        let h4 = d(46.0, s);
        p.round(pad, c4, cw, h4, d(14.0, s), C_CARD, Some(C_LINE));
        p.text("开机自启", pad + d(16.0, s), c4, d(74.0, s), h4, 12.0, 600, C_TEXT, false);
        p.text(
            if lv.autostart { "登录后自动运行" } else { "已关闭" },
            pad + d(102.0, s),
            c4,
            cw - d(200.0, s),
            h4,
            11.0,
            400,
            C_SUB,
            false,
        );
        let tw = d(44.0, s);
        let th = d(24.0, s);
        let tx = pad + cw - tw - d(14.0, s);
        let ty = c4 + (h4 - th) / 2;
        p.round(tx, ty, tw, th, th / 2, if lv.autostart { C_ACCENT } else { COLORREF(0x00DDD9D5) }, None);
        let knob = th - d(6.0, s);
        let kx = if lv.autostart { tx + tw - knob - d(3.0, s) } else { tx + d(3.0, s) };
        p.round(kx, ty + d(3.0, s), knob, knob, knob, COLORREF(0x00FFFFFF), None);
        p.btns.push((ID_AUTOSTART, RECT { left: tx - d(80.0, s), top: c4, right: tx + tw, bottom: c4 + h4 }));

        // ---- 底部按钮
        let by = c4 + h4 + d(14.0, s);
        let bh = d(38.0, s);
        let bg = d(8.0, s);
        let n = 4;
        let bwid = (cw - bg * (n - 1)) / n;
        let labels = ["复制信息", "网页版", "打开日志", "退出"];
        let ids = [ID_COPY, ID_WEB, ID_LOG, ID_EXIT];
        for (i, (lb, id)) in labels.iter().zip(ids.iter()).enumerate() {
            p.button(*id, pad + (bwid + bg) * i as i32, by, bwid, bh, lb, *id == ID_WEB);
        }
    }

    let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
    SelectObject(mem, old_bmp);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
    for f in fonts {
        let _ = DeleteObject(f.into());
    }
    if let Some(b) = BTNS.get() {
        *b.lock().unwrap() = btns;
    }
    let _ = EndPaint(hwnd, &ps);
}

fn hit(pt: windows::Win32::Foundation::POINT) -> u32 {
    if let Some(b) = BTNS.get() {
        for (id, r) in b.lock().unwrap().iter() {
            if pt.x >= r.left && pt.x <= r.right && pt.y >= r.top && pt.y <= r.bottom {
                return *id;
            }
        }
    }
    0
}

unsafe fn on_click(hwnd: HWND, id: u32) {
    match id {
        ID_COPY => {
            let info = INFO.get();
            let text = format!(
                "地址 {}:{}\n配对码 {}",
                info.map(|i| i.ip.as_str()).unwrap_or("-"),
                info.map(|i| i.port).unwrap_or(8765),
                info.map(|i| i.pin.as_str()).unwrap_or("")
            );
            crate::util::set_clipboard(&text);
            *COPIED_AT.lock().unwrap() = Some(std::time::Instant::now());
        }
        ID_WEB => {
            let port = INFO.get().map(|i| i.port).unwrap_or(8765);
            let url = format!("http://127.0.0.1:{}/", port);
            crate::util::open_url(&url);
        }
        ID_LOG => {
            if let Some(i) = INFO.get() {
                crate::util::open_path(&i.dir.join("logs").display().to_string());
            }
        }
        ID_EXIT => {
            std::process::exit(0);
        }
        ID_MUTE => {
            let live = crate::audio::get();
            let target = live.map(|(_, m)| !m);
            let _ = crate::audio::set_mute(target);
        }
        ID_VOL_DOWN | ID_VOL_UP => {
            let delta = if id == ID_VOL_UP { 5 } else { -5 };
            if let Some((v, _)) = crate::audio::get() {
                let _ = crate::audio::set((v as i32 + delta).clamp(0, 100) as u32);
            }
        }
        ID_BRI_DOWN | ID_BRI_UP => {
            let delta = if id == ID_BRI_UP { 5 } else { -5 };
            if let Some(st) = STATE.get() {
                if let Some(v) = st.display.get() {
                    let _ = st.display.set((v as i32 + delta).clamp(0, 100) as u32);
                }
            }
        }
        ID_AUTOSTART => {
            let on = !autostart_enabled();
            set_autostart(on);
        }
        _ => {}
    }
    let _ = InvalidateRect(Some(hwnd), None, false);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let pt = windows::Win32::Foundation::POINT {
                x: (lp.0 & 0xFFFF) as i16 as i32,
                y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32,
            };
            let id = hit(pt);
            if id != 0 {
                on_click(hwnd, id);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let pt = windows::Win32::Foundation::POINT {
                x: (lp.0 & 0xFFFF) as i16 as i32,
                y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32,
            };
            let id = hit(pt);
            let mut h = HOVER.lock().unwrap();
            if *h != id {
                *h = id;
                drop(h);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut p = windows::Win32::Foundation::POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = ScreenToClient(hwnd, &mut p);
            let id = hit(p);
            if id != 0 {
                SetCursor(Some(LoadCursorW(None, IDC_HAND).unwrap_or_default()));
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            // 关闭窗口只是收起界面，后台服务继续（托盘左键或再次双击 exe 可打开）
            if !CLOSED_HINTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let t: Vec<u16> = "已收起到托盘，手机控制继续可用。\n\n想彻底停止服务，请点面板里的「退出」。"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let c: Vec<u16> = "PC 遥控器".encode_utf16().chain(Some(0)).collect();
                MessageBoxW(
                    Some(hwnd),
                    PCWSTR(t.as_ptr()),
                    PCWSTR(c.as_ptr()),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
