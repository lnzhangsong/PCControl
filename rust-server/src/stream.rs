//! 实时画面串流 —— ffmpeg(gdigrab + NVENC) 出 MPEG-TS，HTTP 直接转发给手机
//!
//! 为什么不用逐帧 JPEG：4K 桌面每帧要 100 ms 以上编码，撑死 6~8 fps，看视频会一顿一顿。
//! 走 H.264 硬件编码后，1080p30 只占 GPU 的一小块，手机端 ExoPlayer 硬解播放，画质和流畅度都是另一个量级。
//!
//! 每个客户端连接对应一个 ffmpeg 子进程，连接断开（响应写失败）时进程随之结束。

use std::io::{Read, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 最多两路并发：切换画质时旧连接可能还没完全断开，留一路余量；再多会拖垮采集
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

pub struct StreamGuard {
    child: Child,
    out: ChildStdout,
}

impl Read for StreamGuard {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.out.read(buf)
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// ffmpeg 可执行文件：优先用 exe 同目录的 ffmpeg.exe，其次靠 PATH
pub fn ffmpeg_path() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("ffmpeg.exe");
            if p.is_file() {
                return Some(p);
            }
            // 源码目录调试时的位置
            let p2 = dir.join("..").join("ffmpeg.exe");
            if p2.is_file() {
                return Some(p2);
            }
        }
    }
    // PATH 里找
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join("ffmpeg.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn available() -> bool {
    ffmpeg_path().is_some()
}

/// 桌面分辨率的偶数缩放，H.264 要求宽高都是偶数
fn even(v: i32) -> i32 {
    if v % 2 == 0 {
        v
    } else {
        v - 1
    }
}

/// 打开一路串流。`width` 是目标宽度上限，`bitrate_kbps` 是码率，`fps` 是帧率。
pub fn open(width: u32, bitrate_kbps: u32, fps: u32) -> Option<StreamGuard> {
    let bin = ffmpeg_path()?;
    if ACTIVE.fetch_add(1, Ordering::SeqCst) >= 2 {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
        return None; // 已有另一路在跑，再多会拖垮采集
    }

    let (sw, _sh) = crate::screen::screen_size();
    let target = width.clamp(480, 2560).min(sw.max(480) as u32);

    // 优先 ddagrab（DXGI 桌面复制，画面直接留在显存里，延迟和 CPU 都比 GDI 抓屏低一截）；
    // 远程桌面、多显卡切换之类的环境下它可能起不来，再退回 gdigrab。
    for mode in [Capture::DesktopDuplication, Capture::Gdi] {
        if let Some(g) = spawn(bin.clone(), mode, target, bitrate_kbps, fps) {
            return Some(g);
        }
    }

    ACTIVE.fetch_sub(1, Ordering::SeqCst);
    None
}

#[derive(Clone, Copy, PartialEq)]
enum Capture {
    DesktopDuplication,
    Gdi,
}

fn spawn(
    bin: std::path::PathBuf,
    mode: Capture,
    target: u32,
    bitrate_kbps: u32,
    fps: u32,
) -> Option<StreamGuard> {
    let size = format!("scale={}:{}:flags=fast_bilinear", even(target as i32), -2);
    let mut cmd = Command::new(&bin);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);

    match mode {
        Capture::DesktopDuplication => {
            cmd.args(["-f", "lavfi", "-i", &format!("ddagrab=0:framerate={}", fps)]);
            // ddagrab 出的是显存里的帧，先下载再交给 CPU 缩放，最后转 NV12 直接喂编码器
            cmd.args(["-vf", &format!("hwdownload,format=bgra,{},format=nv12", size)]);
        }
        Capture::Gdi => {
            cmd.args(["-f", "gdigrab", "-framerate", &fps.to_string(), "-i", "desktop"]);
            cmd.args(["-vf", &size]);
        }
    }

    cmd.args([
        // NVENC 低延迟：p1 最快、无 B 帧、CBR 固定码率
        "-c:v", "h264_nvenc",
        "-preset", "p1",
        "-tune", "ll",
        "-rc", "cbr",
        "-b:v", &format!("{}k", bitrate_kbps),
        "-maxrate", &format!("{}k", bitrate_kbps),
        "-bufsize", &format!("{}k", (bitrate_kbps / 4).max(400)),
        "-g", &fps.to_string(),
        "-bf", "0",
        "-pix_fmt", "yuv420p",
        // 每一帧立刻吐出来，不在封装层攒包
        "-flush_packets", "1",
        "-f", "mpegts",
        "-muxdelay", "0",
        "-muxpreload", "0",
        "pipe:1",
    ]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().ok()?;

    // 给采集端一点时间：能撑过 350ms 就认为链路正常，否则换下一种抓屏方式
    std::thread::sleep(std::time::Duration::from_millis(350));
    if !matches!(child.try_wait(), Ok(None)) {
        return None;
    }

    // stderr 必须抽走，否则管道写满会把 ffmpeg 卡死
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut err = err;
            let mut buf = [0u8; 2048];
            let mut tail = String::new();
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                tail.push_str(&String::from_utf8_lossy(&buf[..n]));
                if tail.len() > 4000 {
                    let keep = tail.len() - 2000;
                    tail = tail[keep..].to_string();
                }
            }
            if !tail.trim().is_empty() {
                crate::util::log(format!("串流退出: {}", tail.trim().lines().last().unwrap_or("")));
            }
        });
    }

    let out = child.stdout.take()?;
    crate::util::log(format!(
        "串流开始（{}）：{}x自动 · {}fps · {}kbps",
        if mode == Capture::DesktopDuplication { "GPU 抓屏" } else { "GDI 抓屏" },
        target, fps, bitrate_kbps
    ));
    Some(StreamGuard { child, out })
}

/// 给网页控制台/诊断用：把一帧 TS 数据丢弃，只检查能不能出画面
pub fn probe() -> bool {
    let mut g = match open(1280, 4000, 30) {
        Some(g) => g,
        None => return false,
    };
    let mut buf = [0u8; 8192];
    let ok = matches!(g.read(&mut buf), Ok(n) if n > 0);
    drop(g);
    ok
}

#[allow(dead_code)]
fn _keep(_: &mut dyn Write) {}
