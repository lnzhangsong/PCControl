//! 桌面图标分类整理
//!
//! 把桌面顶层散着的文件按类型挪进分类文件夹（快捷方式 / 文档 / 图片 …）。
//! **文件夹一律不动**，只动文件；系统文件、隐藏文件、自己建的分类文件夹都跳过。
//! 每次整理都会记一份清单，可以一键撤销。

use serde_json::json;
use std::path::{Path, PathBuf};

/// 分类文件夹名（顺序即中文系统里的默认排序，改名要同步改 undo 的清理逻辑）
pub const CATEGORIES: [&str; 10] = [
    "游戏", "软件", "文档", "图片", "视频", "音乐", "压缩包", "安装包", "代码", "其他文件",
];

pub struct Item {
    pub name: String,
    pub category: String,
}

/// 桌面目录：优先 USERPROFILE\Desktop，其次 OneDrive 重定向，再退到公共桌面
pub fn desktop_dir() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").ok()?;
    let public_desktop = std::env::var("PUBLIC")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .unwrap_or_default();
    [
        PathBuf::from(&home).join("Desktop"),
        PathBuf::from(&home).join("OneDrive").join("Desktop"),
        public_desktop,
    ]
    .into_iter()
    .find(|p| !p.as_os_str().is_empty() && p.is_dir())
}

/// 游戏平台关键词：命中就归到「游戏」，不命中就是普通软件
const GAME_HINTS: [&str; 11] = [
    "steamapps", "steam://", "steam.exe", "epic games", "wegame", "ubisoft",
    "gog galaxy", "battle.net", "riot games", "mihoyo", "genshin",
];

/// 快捷方式指向的是不是游戏：.url 直接读文本，.lnk 从二进制里捞 UTF-16 路径片段
fn is_game(path: &Path, name: &str) -> bool {
    let mut text = name.to_lowercase();
    let lower = text.clone();
    if let Ok(bytes) = std::fs::read(path) {
        if lower.ends_with(".url") {
            text.push_str(&String::from_utf8_lossy(&bytes).to_lowercase());
        } else if lower.ends_with(".lnk") {
            let wide: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            text.push_str(&String::from_utf16_lossy(&wide).to_lowercase());
        }
    }
    GAME_HINTS.iter().any(|k| text.contains(k))
}

fn category_of(path: &Path, name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "lnk" | "url" | "website" => {
            if is_game(path, name) {
                "游戏"
            } else {
                "软件"
            }
        }
        "txt" | "md" | "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "csv"
        | "rtf" | "epub" | "mobi" | "log" | "ini" => "文档",
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg" | "ico" | "heic" | "tif"
        | "tiff" | "psd" | "ai" => "图片",
        "mp4" | "mkv" | "avi" | "mov" | "flv" | "wmv" | "rmvb" | "m2ts" | "webm" => "视频",
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "wma" => "音乐",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" => "压缩包",
        "exe" | "msi" | "appx" | "msix" => "安装包",
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "java" | "kt" | "c" | "cpp" | "h" | "hpp"
        | "cs" | "go" | "json" | "html" | "css" | "xml" | "yml" | "yaml" | "sh" | "bat" | "ps1"
        | "toml" => "代码",
        _ => "其他文件",
    }
}

/// 这个文件要不要参与整理
fn should_touch(path: &Path, name: &str) -> bool {
    if name.starts_with('.') || name.eq_ignore_ascii_case("desktop.ini") {
        return false;
    }
    // 文件夹不动（用户自己摆的目录不随意挪），包括上次整理出来的分类文件夹
    if path.is_dir() {
        return false;
    }
    true
}

/// 扫描桌面顶层，只列出"会被整理走"的文件
pub fn scan() -> Vec<Item> {
    let dir = match desktop_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut items: Vec<Item> = rd
        .flatten()
        .filter(|e| should_touch(&e.path(), &e.file_name().to_string_lossy()))
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let category = category_of(&e.path(), &name).to_string();
            Item { name, category }
        })
        .collect();
    items.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
    items
}

/// 各类别数量（给 UI 显示"桌面有 12 个快捷方式、3 个文档…"）
fn summary_of(items: &[Item]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for c in CATEGORIES {
        let n = items.iter().filter(|i| i.category == c).count();
        if n > 0 {
            map.insert(c.to_string(), json!(n));
        }
    }
    serde_json::Value::Object(map)
}

pub fn status() -> serde_json::Value {
    let items = scan();
    json!({
        "ok": true,
        "desktop": desktop_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "total": items.len(),
        "summary": summary_of(&items),
        "items": items.iter().map(|i| json!({"name": i.name, "category": i.category})).collect::<Vec<_>>(),
    })
}

/// 重名时自动加 (1)(2)
fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let parent = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
    for i in 1..1000 {
        let cand = parent.join(format!("{stem}({i}){ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    p
}

fn undo_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("desktop-undo.json"))
}

/// 整理桌面。返回移动了哪些文件，出错只记录不中断
pub fn tidy() -> Result<serde_json::Value, String> {
    let dir = desktop_dir().ok_or_else(|| "找不到桌面目录".to_string())?;
    let mut moved: Vec<serde_json::Value> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    for item in scan() {
        let from = dir.join(&item.name);
        let target_dir = dir.join(&item.category);
        if !target_dir.exists() && std::fs::create_dir(&target_dir).is_err() {
            failed.push(item.name.clone());
            continue;
        }
        let to = unique_path(target_dir.join(&item.name));
        match std::fs::rename(&from, &to) {
            Ok(_) => moved.push(json!({
                "name": item.name,
                "category": item.category,
                "from": from.to_string_lossy(),
                "to": to.to_string_lossy(),
            })),
            Err(_) => failed.push(item.name.clone()),
        }
    }

    if let Some(p) = undo_path() {
        let _ = std::fs::write(p, json!({"desktop": dir.to_string_lossy(), "moved": moved}).to_string());
    }

    Ok(json!({
        "ok": true,
        "desktop": dir.to_string_lossy(),
        "moved": moved.len(),
        "failed": failed,
        "summary": summary_of(&scan_before_tidy(&moved)),
    }))
}

/// 从移动记录反推"整理前"的清单，用来给前端显示整理结果概览
fn scan_before_tidy(moved: &[serde_json::Value]) -> Vec<Item> {
    moved
        .iter()
        .map(|m| Item {
            name: m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            category: m
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("其他文件")
                .to_string(),
        })
        .collect()
}

/// 撤销上一次整理：文件搬回桌面原位，空掉的分类文件夹顺手删掉
pub fn undo() -> Result<serde_json::Value, String> {
    let path = undo_path().ok_or_else(|| "无法确定撤销记录位置".to_string())?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| "没有可撤销的整理记录".to_string())?;
    let data: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| "撤销记录已损坏".to_string())?;
    let desktop = data
        .get("desktop")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let moved = data.get("moved").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut restored = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for m in &moved {
        let from = m.get("from").and_then(|v| v.as_str()).unwrap_or_default();
        let to = m.get("to").and_then(|v| v.as_str()).unwrap_or_default();
        if from.is_empty() || to.is_empty() {
            continue;
        }
        if std::fs::rename(Path::new(to), Path::new(from)).is_ok() {
            restored += 1;
        } else {
            failed.push(m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string());
        }
    }

    // 分类文件夹空了就删掉，别在桌面留一堆空壳。
    // 资源管理器 / OneDrive 可能还握着句柄，删不掉就隔一会儿再试几次
    if !desktop.is_empty() {
        let base = PathBuf::from(&desktop);
        for c in CATEGORIES {
            let d = base.join(c);
            if !d.is_dir() {
                continue;
            }
            for _ in 0..5 {
                let empty = std::fs::read_dir(&d)
                    .map(|mut r| r.next().is_none())
                    .unwrap_or(false);
                if !empty {
                    break;
                }
                if std::fs::remove_dir(&d).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }
    let _ = std::fs::remove_file(&path);

    Ok(json!({"ok": true, "restored": restored, "failed": failed}))
}
