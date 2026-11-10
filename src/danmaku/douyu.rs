use anyhow::{bail, Result};
use async_trait::async_trait;
use rand::seq::SliceRandom;
use serde_json::Value;
use tokio::sync::broadcast::Sender;

use super::{emit, try_urls, DanmakuSource};
use crate::models::DanmakuMessage;
use crate::state::Shared;
use crate::util::J;

pub struct DouyuSource { pub st: Shared, pub args: Value }

fn serialize(body: &str) -> Vec<u8> {
    let b = body.as_bytes();
    let length = 9 + b.len();
    let mut out = Vec::with_capacity(13 + b.len());
    out.extend_from_slice(&(length as u32).to_le_bytes());
    out.extend_from_slice(&(length as u32).to_le_bytes());
    out.extend_from_slice(&689u16.to_le_bytes());
    out.extend_from_slice(&[0u8, 0u8]);
    out.extend_from_slice(b);
    out.push(0);
    out
}

fn stt_map(s: &str) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    for field in s.split('/') {
        if field.is_empty() { continue; }
        let mut it = field.splitn(2, "@=");
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            if v.contains("@=") || v.contains("//") { continue; }
            m.insert(k.to_string(), v.replace("@S", "/").replace("@A", "@"));
        }
    }
    m
}

fn col_color(t: i64) -> u32 {
    match t {
        1 => 0xFF0000, 2 => 0x1E87F0, 3 => 0x7AC84B,
        4 => 0xFF7F00, 5 => 0x9B39F4, 6 => 0xFF69B4, _ => 0,
    }
}

fn decode(data: &[u8], tx: &Sender<DanmakuMessage>) {
    if data.len() < 13 { return; }
    let full = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let body_len = full.saturating_sub(9);
    let pos = 12usize;
    if body_len == 0 || pos + body_len > data.len() { return; }
    let body = String::from_utf8_lossy(&data[pos..pos + body_len]);
    let m = stt_map(&body);
    if m.get("type").map(String::as_str) != Some("chatmsg") { return; }
    let col = m.get("col").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    emit(tx, DanmakuMessage::chat(
        m.get("nn").cloned().unwrap_or_default(),
        m.get("txt").cloned().unwrap_or_default(),
        col_color(col),
    ));
}

#[async_trait]
impl DanmakuSource for DouyuSource {
    async fn run(self: Box<Self>, tx: Sender<DanmakuMessage>) -> Result<()> {
        let _ = &self.st;
        let rid = self.args.txt();
        if rid.is_empty() { bail!("斗鱼房间号为空"); }
        let mut ports: Vec<u16> = (8501..=8506).collect();
        ports.shuffle(&mut rand::thread_rng());
        let urls: Vec<String> = ports.iter().map(|p| format!("wss://danmuproxy.douyu.com:{p}")).collect();
        let join = vec![
            serialize(&format!("type@=loginreq/roomid@={rid}/")),
            serialize(&format!("type@=joingroup/rid@={rid}/gid@=-9999/")),
        ];
        try_urls(urls, vec![], 45_000, serialize("type@=mrkl/"), join, tx,
            |data, tx, _| decode(data, tx)).await
    }
}
