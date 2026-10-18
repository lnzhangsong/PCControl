//! HTTP 接口层（与 Android App 的协议保持一致）

use std::sync::Arc;

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::display::Display;
use crate::{audio, power, util};

/// 网页控制台直接编译进可执行文件，单文件 exe 也能用
const WEBUI: &str = include_str!("../../pc-server/webui.html");
const PANEL: &str = include_str!("../../pc-server/panel.html");

pub struct AppState {
    pub pin: String,
    pub port: u16,
    pub name: String,
    pub display: Display,
    pub start: std::time::Instant,
    /// 最近一次非本机请求（手机端）的 IP 与时间，面板上显示"手机已连接"
    pub last_client: std::sync::Mutex<Option<(String, std::time::Instant)>>,
}

pub fn serve(state: Arc<AppState>, server: Arc<Server>, workers: usize) {
    for _ in 0..workers {
        let st = state.clone();
        let srv = server.clone();
        std::thread::spawn(move || loop {
            match srv.recv() {
                Ok(req) => handle(&st, req),
                Err(_) => break,
            }
        });
    }
}

fn handle(st: &AppState, mut req: Request) {
    let method = req.method().clone();
    let raw_url = req.url().to_string();
    let (path_raw, query) = match raw_url.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (raw_url.clone(), String::new()),
    };
    let path = {
        let t = path_raw.trim_end_matches('/');
        if t.is_empty() {
            "/".to_string()
        } else {
            t.to_string()
        }
    };

    if method == Method::Options {
        return respond_text(req, 204, "", "text/plain");
    }

    // 网页控制台不需要配对码（页面内部再填 PIN）
    if method == Method::Get && (path == "/" || path == "/index.html" || path == "/webui") {
        return respond_text(req, 200, WEBUI, "text/html; charset=utf-8");
    }

    // PC 端面板页面（只给本机 WebView2 用，同样免配对码）
    if method == Method::Get && path == "/panel" {
        return respond_text(req, 200, PANEL, "text/html; charset=utf-8");
    }

    if path == "/api/ping" {
        return respond_json(
            req,
            200,
            json!({
                "ok": true, "app": "pccontrol", "version": env!("CARGO_PKG_VERSION"),
                "needPin": true, "name": st.name, "port": st.port
            }),
        );
    }

    // 本机（127.0.0.1）访问免配对码：PC 端控制台打开即用；局域网仍需 PIN
    if !authorized(st, &req, &query) && !is_loopback(&req) {
        return respond_json(req, 401, json!({"ok": false, "error": "配对码错误"}));
    }

    // 文件上传：请求体就是文件本体，不能先当 JSON 读掉
    if method == Method::Post && path == "/api/upload" {
        handle_upload(req, &query);
        return;
    }

    let mut body = String::new();
    if method == Method::Post {
        let _ = req.as_reader().read_to_string(&mut body);
    }
    let payload: Value = if body.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&body).unwrap_or_else(|_| json!({}))
    };

    if !is_loopback(&req) {
        if let Some(addr) = req.remote_addr() {
            if let Ok(mut g) = st.last_client.lock() {
                *g = Some((addr.ip().to_string(), std::time::Instant::now()));
            }
        }
    }

    match (method.as_str(), path.as_str()) {
        ("GET", "/api/status") => {
            let local = is_loopback(&req);
            respond_json(req, 200, status(st, local))
        }

        // 实时画面：返回一帧 JPEG。w 是输出宽度上限，q 是 JPEG 质量
        ("GET", "/api/screen") => {
            let mut w = 1280u32;
            let mut q = 72u8;
            for pair in query.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    match k {
                        "w" => w = v.parse().unwrap_or(w),
                        "q" => q = v.parse().unwrap_or(q),
                        _ => {}
                    }
                }
            }
            match crate::screen::capture_jpeg(w.clamp(320, 2560), q) {
                Some(jpg) => respond_bytes(req, 200, jpg, "image/jpeg"),
                None => respond_json(
                    req,
                    503,
                    json!({"ok": false, "error": "上一帧还在处理，稍后再试"}),
                ),
            }
        }

        ("GET", "/api/screen/meta") => {
            let (w, h) = crate::screen::screen_size();
            respond_json(req, 200, json!({"ok": true, "width": w, "height": h}))
        }

        // 剪贴板：手机和电脑共用一块
        ("GET", "/api/clipboard") => match crate::clipboard::get_text() {
            Some(text) => respond_json(req, 200, json!({"ok": true, "text": text})),
            None => respond_json(
                req,
                200,
                json!({"ok": false, "error": "电脑剪贴板是空的，或正被其他程序占用"}),
            ),
        },

        ("POST", "/api/clipboard") => {
            let text = payload.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let ok = crate::clipboard::set_text(text);
            respond_json(
                req,
                200,
                if ok {
                    json!({"ok": true, "length": text.chars().count()})
                } else {
                    json!({"ok": false, "error": "写入剪贴板失败"})
                },
            )
        }

        // 实时画面串流：ffmpeg H.264 硬编 → MPEG-TS，直接推给播放器
        ("GET", "/api/stream") => {
            let mut w = 1920u32;
            let mut br = 8000u32;
            let mut fps = 30u32;
            for pair in query.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    match k {
                        "w" => w = v.parse().unwrap_or(w),
                        "br" => br = v.parse().unwrap_or(br),
                        "fps" => fps = v.parse().unwrap_or(fps),
                        _ => {}
                    }
                }
            }
            match crate::stream::open(w, br, fps.clamp(10, 60)) {
                Some(guard) => {
                    let mut resp = Response::new(StatusCode(200), Vec::new(), guard, None, None);
                    if let Ok(h) =
                        Header::from_bytes(&b"Content-Type"[..], &b"video/mp2t"[..])
                    {
                        resp.add_header(h);
                    }
                    if let Ok(h) = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]) {
                        resp.add_header(h);
                    }
                    if let Ok(h) = Header::from_bytes(&b"Connection"[..], &b"close"[..]) {
                        resp.add_header(h);
                    }
                    let _ = req.respond(resp);
                }
                None => respond_json(
                    req,
                    503,
                    json!({"ok": false, "error": "串流启动失败：已有另一路在播放，或找不到 ffmpeg"}),
                ),
            }
        }

        ("GET", "/api/volume") => match audio::get() {
            Some((vol, muted)) => {
                respond_json(req, 200, json!({"ok": true, "volume": vol, "muted": muted}))
            }
            None => respond_json(req, 200, json!({"ok": false, "error": "音量控制不可用"})),
        },

        ("POST", "/api/volume") => {
            let Some(v) = payload.get("value").and_then(|x| x.as_i64()) else {
                return respond_json(req, 400, json!({"ok": false, "error": "缺少 value"}));
            };
            if !audio::set(v.clamp(0, 100) as u32) {
                return respond_json(req, 200, json!({"ok": false, "error": "音量控制不可用"}));
            }
            let (vol, muted) = audio::get().unwrap_or((v.clamp(0, 100) as u32, false));
            respond_json(req, 200, json!({"ok": true, "volume": vol, "muted": muted}))
        }

        ("POST", "/api/volume/step") => {
            let delta = payload.get("delta").and_then(|x| x.as_i64()).unwrap_or(5);
            let (cur, _) = audio::get().unwrap_or((50, false));
            let target = (cur as i64 + delta).clamp(0, 100) as u32;
            if !audio::set(target) {
                return respond_json(req, 200, json!({"ok": false, "error": "音量控制不可用"}));
            }
            let (vol, muted) = audio::get().unwrap_or((target, false));
            respond_json(req, 200, json!({"ok": true, "volume": vol, "muted": muted}))
        }

        ("POST", "/api/mute") => {
            let explicit = payload.get("value").and_then(|x| x.as_bool());
            if !audio::set_mute(explicit) {
                return respond_json(req, 200, json!({"ok": false, "error": "音量控制不可用"}));
            }
            let (vol, muted) = audio::get().unwrap_or((0, false));
            respond_json(req, 200, json!({"ok": true, "volume": vol, "muted": muted}))
        }

        ("POST", "/api/brightness") => {
            let Some(v) = payload.get("value").and_then(|x| x.as_i64()) else {
                return respond_json(req, 400, json!({"ok": false, "error": "缺少 value"}));
            };
            let ok = st.display.set(v.clamp(0, 100) as u32);
            respond_json(
                req,
                200,
                json!({
                    "ok": ok,
                    "brightness": st.display.get(),
                    "mode": st.display.mode(),
                    "reason": st.display.reason(),
                }),
            )
        }

        ("POST", "/api/hdr") => {
            let enabled = payload
                .get("enabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let ok = st.display.set_hdr(enabled);
            st.display.new_probe_needed();
            respond_json(
                req,
                200,
                json!({
                    "ok": ok,
                    "hdr": st.display.hdr(),
                    "brightness": st.display.get(),
                    "brightnessMode": st.display.mode(),
                    "brightnessReason": st.display.reason(),
                }),
            )
        }

        ("POST", "/api/media") => {
            let action = payload.get("action").and_then(|x| x.as_str()).unwrap_or("");
            respond_json(req, 200, json!({"ok": power::media(action)}))
        }

        ("POST", "/api/autostart") => {
            let on = payload.get("value").and_then(|x| x.as_bool()).unwrap_or(false);
            crate::panel::set_autostart(on);
            respond_json(req, 200, json!({"ok": true, "autostart": crate::panel::autostart_enabled()}))
        }

        ("POST", "/api/panel") => {
            let action = payload.get("action").and_then(|x| x.as_str()).unwrap_or("");
            match action {
                "web" => {
                    let ip = util::lan_ip();
                    util::open_url(&format!("http://{ip}:{}/", st.port));
                    respond_json(req, 200, json!({"ok": true}))
                }
                "log" => {
                    let log = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|d| d.join("logs")))
                        .unwrap_or_default();
                    let _ = std::fs::create_dir_all(&log);
                    util::open_path(&log.join("server.log").to_string_lossy());
                    respond_json(req, 200, json!({"ok": true}))
                }
                "quit" => {
                    respond_json(req, 200, json!({"ok": true}));
                    std::thread::spawn(|| {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        crate::util::log("从面板退出，服务停止");
                        std::process::exit(0);
                    });
                }
                _ => respond_json(req, 400, json!({"ok": false, "error": "未知操作"})),
            }
        }

        ("POST", "/api/power") => {
            let action = payload.get("action").and_then(|x| x.as_str()).unwrap_or("");
            respond_json(req, 200, json!({"ok": power::act(action)}))
        }

        _ => respond_json(req, 404, json!({"ok": false, "error": "unknown endpoint"})),
    }
}

fn status(st: &AppState, local: bool) -> Value {
    let audio_state = audio::get();
    let brightness = st.display.get();
    let (volume, muted) = match audio_state {
        Some((v, m)) => (json!(v), json!(m)),
        None => (Value::Null, Value::Null),
    };
    let (last_ip, last_ago) = st
        .last_client
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .map(|(ip, at)| (Value::String(ip), json!(at.elapsed().as_secs())))
        .unwrap_or((Value::Null, Value::Null));
    json!({
        "ok": true,
        "app": "PC Remote Server",
        "version": env!("CARGO_PKG_VERSION"),
        "name": st.name,
        "os": "Windows",
        "ip": util::lan_ip(),
        "port": st.port,
        "uptime": st.start.elapsed().as_secs(),
        "volume": volume,
        "muted": muted,
        "brightness": brightness,
        "brightnessMode": st.display.mode(),
        "brightnessReason": st.display.reason(),
        "hdr": st.display.hdr(),
        "canVolume": audio_state.is_some(),
        "canBrightness": brightness.is_some(),
        "mac": util::mac_address(),
        "pin": if local { json!(st.pin) } else { Value::Null },
        "lastClientIp": last_ip,
        "lastClientAgo": last_ago,
        "autostart": crate::panel::autostart_enabled(),
        "features": ["volume", "brightness", "media", "lock", "sleep", "screen_off", "shutdown", "screen", "stream", "upload", "clipboard"],
        "streamReady": crate::stream::available(),
    })
}

fn is_loopback(req: &Request) -> bool {
    matches!(req.remote_addr(), Some(addr) if addr.ip().is_loopback())
}

fn authorized(st: &AppState, req: &Request, query: &str) -> bool {
    let mut given = String::new();
    for h in req.headers() {
        if h.field.as_str().as_str().eq_ignore_ascii_case("x-pin") {
            given = h.value.as_str().to_string();
        }
    }
    if given.is_empty() {
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "pin" {
                    given = v.to_string();
                }
            }
        }
    }
    !given.is_empty() && given == st.pin
}

fn respond_json(req: Request, code: u16, value: Value) {
    respond_text(req, code, &value.to_string(), "application/json; charset=utf-8")
}

/// 接收手机传来的文件，落到「下载\PC Remote」下
fn handle_upload(mut req: Request, query: &str) {
    let raw_name = query
        .split('&')
        .find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == "name").map(|(_, v)| v))
        .map(url_decode)
        .unwrap_or_default();

    let dir = crate::clipboard::upload_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        respond_json(req, 500, json!({"ok": false, "error": format!("创建目录失败: {e}")}));
        return;
    }

    let name = crate::clipboard::safe_file_name(&raw_name);
    let path = crate::clipboard::unique_path(&dir, &name);

    let result: Result<u64, String> = (|| {
        let mut reader = req.as_reader();
        let mut file = std::fs::File::create(&path).map_err(|e| format!("创建文件失败: {e}"))?;
        std::io::copy(&mut reader, &mut file).map_err(|e| format!("写入失败: {e}"))
    })();

    match result {
        Ok(size) => {
            let shown = path.display().to_string();
            crate::util::log(format!("收到文件 {shown}（{} KB）", size / 1024));
            respond_json(
                req,
                200,
                json!({"ok": true, "path": shown, "size": size, "name": path.file_name().map(|s| s.to_string_lossy().to_string())}),
            );
        }
        Err(e) => respond_json(req, 500, json!({"ok": false, "error": e})),
    }
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn respond_text(req: Request, code: u16, body: &str, content_type: &str) {
    respond_bytes(req, code, body.as_bytes().to_vec(), content_type)
}

fn respond_bytes(req: Request, code: u16, body: Vec<u8>, content_type: &str) {
    let mut resp = Response::from_data(body).with_status_code(code);
    if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()) {
        resp.add_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]) {
        resp.add_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]) {
        resp.add_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"*"[..]) {
        resp.add_header(h);
    }
    if let Ok(h) = Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET,POST,OPTIONS"[..]) {
        resp.add_header(h);
    }
    let _ = req.respond(resp);
}
