//! PC Remote Server —— Rust 版
//!
//! 单文件可执行程序，无运行库依赖，适合开机自启。
//! 协议与 Android App / 网页控制台完全兼容。

// 关键：用 windows 子系统，双击只出现程序窗口，不再额外弹一个黑框终端。
// 代价是没有 stdout，日志一律走 exe 同级 logs/server.log（util::log 已处理）。
#![cfg_attr(windows, windows_subsystem = "windows")]

mod audio;
mod clipboard;
mod desktop;
mod discovery;
mod display;
mod http;
mod panel;
mod power;
mod screen;
mod stream;
mod tray;
mod util;
mod webpanel;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use tiny_http::Server;

use display::Display;

const DEFAULT_PORT: u16 = 8765;

fn main() {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    util::init_log(&exe_dir.join("logs"));
    enable_dpi_awareness();
    init_com_sta();

    // 顺手导出图标文件，方便做桌面快捷方式
    let _ = tray::write_ico_file(&exe_dir.join("pc-remote.ico"));

    // 只导出图标然后退出（用于重新生成 assets/app.ico）
    if let Some(p) = std::env::args().find_map(|a| a.strip_prefix("--write-icon=").map(String::from)) {
        let ok = tray::write_ico_file(std::path::Path::new(&p));
        util::log(format!("{} {}", if ok { "已写出" } else { "写出失败" }, p));
        return;
    }

    // 一键整理桌面：给 bat 脚本 / 快捷方式用的，整理完弹个结果就退出
    if std::env::args().any(|a| a == "--tidy-desktop") {
        let (title, text) = match desktop::tidy() {
            Ok(v) => {
                let n = v.get("moved").and_then(|x| x.as_u64()).unwrap_or(0);
                let path = v.get("desktop").and_then(|x| x.as_str()).unwrap_or("");
                util::log(format!("桌面整理完成，移动 {n} 个文件"));
                (
                    "桌面已整理",
                    format!("把 {n} 个文件按类型归好了。\n\n桌面：{path}\n\n想还原的话，在 PC Remote 面板里点「撤销上次」。"),
                )
            }
            Err(e) => {
                util::log(format!("桌面整理失败: {e}"));
                ("整理失败", e)
            }
        };
        show_message(&title, &text);
        return;
    }

    let mut port = DEFAULT_PORT;
    let mut pin_arg: Option<String> = None;
    let mut no_tray = false;
    let mut hidden = false;
    let mut no_webview = false;
    let mut hardware = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if let Some(v) = args.get(i + 1).and_then(|v| v.parse::<u16>().ok()) {
                    port = v;
                    i += 1;
                }
            }
            "--pin" => {
                if let Some(v) = args.get(i + 1) {
                    pin_arg = Some(v.clone());
                    i += 1;
                }
            }
            "--no-tray" => {
                no_tray = true;
            }
            "--hidden" | "--silent" => {
                hidden = true;
            }
            "--no-webview" | "--gdi" => {
                no_webview = true;
            }
            "--hw" => {
                hardware = true;
            }
            "--version" | "-V" => {
                util::log(format!("pccontrol-server {}", env!("CARGO_PKG_VERSION")));
                return;
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                if let Some(v) = other.strip_prefix("--port=") {
                    port = v.parse().unwrap_or(DEFAULT_PORT);
                } else if let Some(v) = other.strip_prefix("--pin=") {
                    pin_arg = Some(v.to_string());
                }
            }
        }
        i += 1;
    }

    let pin = pin_arg.unwrap_or_else(|| load_or_create_pin(&exe_dir));
    let name = util::hostname();

    // 已经有一个实例在跑：把它的窗口叫到前台就退出，避免端口冲突
    if !hidden && (webpanel::activate() || panel::activate_existing()) {
        return;
    }

    util::log(format!(
        "PC Remote Server (Rust) {} 启动",
        env!("CARGO_PKG_VERSION")
    ));

    let display = Display::new();
    util::log(format!(
        "亮度控制: {} — {}",
        display.mode(),
        display.reason()
    ));
    util::log(format!(
        "音量控制: {}",
        if audio::available() { "可用" } else { "不可用" }
    ));

    let state = Arc::new(http::AppState {
        pin: pin.clone(),
        port,
        name: name.clone(),
        display,
        start: Instant::now(),
        last_client: std::sync::Mutex::new(None),
    });

    let server = match Server::http(("0.0.0.0", port)) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            util::log(format!("端口 {port} 监听失败: {e}（是否已被另一个实例占用？）"));
            std::process::exit(1);
        }
    };

    let ip = util::lan_ip();
    util::log(format!("监听地址: http://{ip}:{port}"));
    util::log(format!("本机控制台: http://127.0.0.1:{port}/"));
    util::log(format!("配对码: {pin}"));
    util::log(format!("本机名称: {name}"));

    let stop = Arc::new(AtomicBool::new(false));
    discovery::spawn(port, name.clone(), ip.clone(), stop.clone());
    http::serve(state.clone(), server, 4);

    let show_panel = !hidden;
    let mut keep_alive: Option<wry::WebView> = None;
    if !no_webview {
        keep_alive = webpanel::init(port, show_panel, hardware);
    }
    let need_gdi_panel = show_panel && keep_alive.is_none();
    if need_gdi_panel {
        panel::show(
            panel::PanelInfo {
                name: name.clone(),
                port,
                pin: pin.clone(),
                ip: ip.clone(),
                dir: exe_dir.clone(),
            },
            state.clone(),
        );
    }
    if !no_tray {
        tray::install(tray::TrayInfo {
            port,
            pin,
            name,
            ip,
        });
        tray::refresh_later();
    }

    if show_panel || !no_tray || keep_alive.is_some() {
        let _hold = keep_alive;
        message_loop();
    }

    // 纯后台：常驻
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// 弹一个系统提示框（一键脚本没有控制台，只能用窗口告诉用户结果）
fn show_message(title: &str, text: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    let t: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let m: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = MessageBoxW(None, PCWSTR(m.as_ptr()), PCWSTR(t.as_ptr()), MB_OK | MB_ICONINFORMATION);
    }
}

/// 让进程按物理像素渲染（Per-Monitor V2）。
/// 不做这一步，在 150%/200% 缩放的高分屏上整个窗口会被系统位图拉伸，文字发虚。
fn enable_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// 主线程（UI 线程）必须是 STA 公寓，否则 WebView2 无法创建。
/// 音量模块在首次调用时会尝试 MTA，这里先占住 STA，它那边失败但不影响后续使用。
fn init_com_sta() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

fn message_loop() {
    unsafe {
        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        loop {
            let r = windows::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, None, 0, 0);
            if r.0 == -1 || r.0 == 0 {
                break;
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
            windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
        }
    }
    std::process::exit(0);
}

fn print_help() {
    util::log(format!("pccontrol-server {}", env!("CARGO_PKG_VERSION")));
    util::log("用法: pccontrol-server [--port 8765] [--pin 123456]");
    util::log("  --port, -p <端口>   HTTP 端口，默认 8765");
    util::log("  --pin <6位数字>     配对码，不传则沿用同目录 pin.txt 或随机生成");
    util::log("  --no-tray          不显示托盘图标（纯后台服务）");
    util::log("  --hidden           启动时不显示控制面板，只在后台运行");
    util::log("  --no-webview       不使用 WebView2 面板，回退到旧版原生面板");
    util::log("  --hw               面板走 GPU 渲染（默认软件渲染，兼容性更好）");
    util::log("  --tidy-desktop     直接整理一次桌面图标然后退出（可写进 bat 一键整理）");
}

fn load_or_create_pin(dir: &std::path::Path) -> String {
    let path = dir.join("pin.txt");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let t = text.trim().to_string();
        if t.len() == 6 && t.chars().all(|c| c.is_ascii_digit()) {
            return t;
        }
    }
    // 用系统时间做种子，避免引入随机数库
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pin = format!("{:06}", 100000 + (seed % 900000));
    let _ = std::fs::write(&path, &pin);
    pin
}
