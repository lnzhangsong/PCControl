//! 把图标嵌进 exe，让资源管理器 / 任务栏 / 开机自启的快捷方式都能显示图标。
//!
//! 做法：用 Windows SDK 的 rc.exe 把 assets/app.ico 编译成 .res，
//! 再交给链接器一起链接。找不到 rc.exe 就跳过（不影响编译，只是 exe 没图标）。

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=build.rs");

    let ico = Path::new("assets/app.ico");
    if !ico.exists() {
        return;
    }
    let Some(rc) = find_rc() else {
        println!("cargo:warning=找不到 rc.exe，跳过图标嵌入");
        return;
    };

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let rc_file = out_dir.join("app.rc");
    let res_file = out_dir.join("app.res");

    // rc.exe 不接受 \\?\ 前缀，用普通绝对路径
    let ico_abs = std::env::current_dir()
        .map(|d| d.join(ico))
        .unwrap_or_else(|_| ico.to_path_buf());
    // rc 会把路径里的 \\ 当转义符（例如 \\2026 变成八进制），必须写成双反斜杠
    let ico_escaped = ico_abs.display().to_string().replace('\\', "\\\\");
    let script = format!("1 ICON \"{}\"\n", ico_escaped);
    if std::fs::write(&rc_file, script).is_err() {
        return;
    }

    let output = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .output();

    match output {
        Ok(o) if o.status.success() && res_file.exists() => {
            println!("cargo:rustc-link-arg={}", res_file.display());
        }
        Ok(o) => println!(
            "cargo:warning=rc.exe 失败: {} {}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => println!("cargo:warning=无法执行 rc.exe: {e}"),
    }
}

/// 在 Windows SDK / VS 常见位置找 rc.exe（优先 x64 且版本号最大）
fn find_rc() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = std::fs::read_dir(kits) {
        for e in entries.flatten() {
            let p = e.path().join("x64").join("rc.exe");
            if p.exists() {
                candidates.push(p);
            }
        }
    }
    // 版本号大的排前面
    candidates.sort();
    candidates.reverse();
    candidates.into_iter().next()
}
