//! UDP 自动发现：手机端广播 PCREMOTE_DISCOVER，服务端回一条 JSON

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

pub const DISCOVERY_PORT: u16 = 55667;
const MAGIC: &[u8] = b"PCREMOTE_DISCOVER";

pub fn spawn(http_port: u16, name: String, ip: String, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let sock = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) {
            Ok(s) => s,
            Err(e) => {
                crate::util::log(format!("UDP 发现端口被占用，已跳过自动发现: {e}"));
                return;
            }
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(800)));
        let reply = json!({
            "app": "pccontrol",
            "name": name,
            "port": http_port,
            "ip": ip,
        })
        .to_string();
        crate::util::log(format!("UDP 自动发现已开启（端口 {DISCOVERY_PORT}）"));

        let mut buf = [0u8; 256];
        while !stop.load(Ordering::Relaxed) {
            match sock.recv_from(&mut buf) {
                Ok((n, addr)) => {
                    if buf[..n].trim_ascii() == MAGIC {
                        let _ = sock.send_to(reply.as_bytes(), addr);
                    }
                }
                Err(_) => continue,
            }
        }
    });
}
