use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::broadcast::Sender;

use super::{emit, gunzip, try_urls, DanmakuSource};
use crate::models::DanmakuMessage;
use crate::pb::{Pb, PbWriter};
use crate::state::Shared;
use crate::util::{unix_ms, J};

pub struct KuaishouSource { pub st: Shared, pub args: Value }

fn encode_socket_message(payload_type: u64, payload: &[u8]) -> Vec<u8> {
    let mut w = PbWriter::new();
    w.uint64(1, payload_type).bytes(3, payload);
    w.finish()
}

fn decode_comment(c: &Pb) -> Option<DanmakuMessage> {
    let content = c.str(3);
    if c.int(7) == 2 || content.is_empty() { return None; }
    let user = c.msgs(2).first().map(|u| u.str(2)).unwrap_or_default();
    let text = c.str(6).trim().trim_start_matches('#').to_string();
    let color = if text.len() == 6 {
        u32::from_str_radix(&text, 16).map(|x| x & 0xFFFFFF).unwrap_or(0)
    } else { 0 };
    Some(DanmakuMessage::chat(user, content, color))
}

fn decode_feed(payload: &[u8], tx: &Sender<DanmakuMessage>) {
    let push = Pb::parse(payload);
    for comment in push.msgs(5) {
        let msg = decode_comment(&comment).or_else(|| decode_comment(&Pb::parse(&comment.bytes(1))));
        if let Some(m) = msg { emit(tx, m); }
    }
}

fn decode(data: &[u8], tx: &Sender<DanmakuMessage>) {
    let frame = Pb::parse(data);
    let payload_type = frame.int(1);
    let compression = frame.int(2);
    let mut payload = frame.bytes(3);
    if payload.is_empty() { return; }
    if compression == 2 { payload = gunzip(&payload); }
    else if compression == 3 { return; }
    match payload_type {
        103 => {
            let err = Pb::parse(&payload);
            let msg = err.str(2);
            let code = err.int(1);
            if !msg.is_empty() || code != 0 {
                emit(tx, DanmakuMessage::error(if msg.is_empty() { format!("快手弹幕错误：{code}") } else { msg }));
            }
        }
        310 => decode_feed(&payload, tx),
        _ => {}
    }
}

#[async_trait]
impl DanmakuSource for KuaishouSource {
    async fn run(self: Box<Self>, tx: Sender<DanmakuMessage>) -> Result<()> {
        let _ = &self.st;
        let a = self.args;
        let urls: Vec<String> = a.a("websocketUrls").iter().map(|v| v.txt()).filter(|s| !s.is_empty()).collect();
        let token = a.s("token");
        let live_stream_id = a.s("liveStreamId");
        if urls.is_empty() || token.is_empty() || live_stream_id.is_empty() {
            bail!("快手弹幕需要登录态凭证（token / websocketUrls 缺失）");
        }
        let mut p = PbWriter::new();
        p.str(1, &token)
            .str(2, &live_stream_id)
            .uint64(3, 0)
            .uint64(4, 0)
            .str(5, &a.s("expTag"))
            .str(6, &a.s("attach"))
            .str(7, &a.s("pageId"));
        let join = encode_socket_message(200, &p.finish());

        let mut hbw = PbWriter::new();
        hbw.uint64(1, unix_ms() as u64);
        let heartbeat = encode_socket_message(1, &hbw.finish());

        let room_id = a.s("roomId");
        let ua = { let u = a.s("userAgent"); if u.is_empty() { crate::identity::KUAISHOU_UA.into() } else { u } };
        let cookie = a.s("cookie");
        let mut headers = vec![
            ("User-Agent".into(), ua),
            ("Origin".into(), "https://live.kuaishou.com".into()),
            ("Referer".into(), format!("https://live.kuaishou.com/u/{room_id}")),
        ];
        if !cookie.is_empty() { headers.push(("Cookie".into(), cookie)); }

        try_urls(urls, headers, 20_000, heartbeat, vec![join], tx,
            |data, tx, _| decode(data, tx)).await
    }
}
