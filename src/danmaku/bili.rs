use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::broadcast::Sender;

use super::{brotli_decode, emit, ws_loop, DanmakuSource};
use crate::models::DanmakuMessage;
use crate::state::Shared;
use crate::util::J;

pub struct BiliSource { pub st: Shared, pub args: Value }

fn encode(msg: &str, action: u32) -> Vec<u8> {
    let data = msg.as_bytes();
    let len = data.len() + 16;
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.extend_from_slice(&16u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&action.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(data);
    out
}

fn decode(data: &[u8], tx: &Sender<DanmakuMessage>) {
    let mut pos = 0usize;
    while pos + 16 <= data.len() {
        let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        if len < 16 || pos + len > data.len() { break; }
        decode_packet(&data[pos..pos + len], tx);
        pos += len;
    }
}

fn decode_packet(p: &[u8], tx: &Sender<DanmakuMessage>) {
    let proto = u16::from_be_bytes([p[6], p[7]]);
    let op = u32::from_be_bytes([p[8], p[9], p[10], p[11]]);
    let body = &p[16..];
    match op {
        3 => {
            if body.len() >= 4 {
                let online = u32::from_be_bytes(body[..4].try_into().unwrap()) as u64;
                emit(tx, DanmakuMessage::online_msg(online));
            }
        }
        5 => {
            let decoded = match proto {
                2 => super::zlib_decode(body),
                3 => brotli_decode(body),
                _ => body.to_vec(),
            };
            let text = String::from_utf8_lossy(&decoded);
            for part in text.split(|c: char| c.is_control()) {
                let part = part.trim();
                if part.len() > 2 && part.starts_with('{') { parse_message(part, tx); }
            }
        }
        _ => {}
    }
}

fn parse_message(s: &str, tx: &Sender<DanmakuMessage>) {
    let v: Value = match serde_json::from_str(s) { Ok(v) => v, Err(_) => return };
    if !v.s("cmd").contains("DANMU_MSG") { return; }
    let info = match v.get("info").and_then(|x| x.as_array()) { Some(a) => a, None => return };
    if info.len() < 3 { return; }
    let message = info[1].txt();
    let color = info[0].as_array().and_then(|a| a.get(3)).map(|x| x.num()).unwrap_or(0);
    let username = info[2].as_array().and_then(|a| a.get(1)).map(|x| x.txt()).unwrap_or_default();
    emit(tx, DanmakuMessage::chat(username, message, (color.max(0) as u32) & 0xFFFFFF));
}

#[async_trait]
impl DanmakuSource for BiliSource {
    async fn run(self: Box<Self>, tx: Sender<DanmakuMessage>) -> Result<()> {
        let _ = &self.st;
        let token = self.args.s("token");
        if token.is_empty() { bail!("B站弹幕 token 为空（房间可能未开播）"); }
        let host = { let h = self.args.s("serverHost"); if h.is_empty() { "broadcastlv.chat.bilibili.com".into() } else { h } };
        let join = json!({
            "uid": self.args.i("uid"),
            "roomid": self.args.i("roomId"),
            "protover": 3,
            "buvid": self.args.s("buvid"),
            "platform": "web",
            "type": 2,
            "key": token,
        }).to_string();
        ws_loop(
            format!("wss://{host}/sub"), vec![], 60_000,
            encode("", 2), vec![encode(&join, 7)], tx,
            |data, tx, _| decode(data, tx),
        ).await
    }
}
