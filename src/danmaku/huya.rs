use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::broadcast::Sender;

use super::{emit, ws_loop, DanmakuSource};
use crate::models::DanmakuMessage;
use crate::state::Shared;
use crate::tars::{parse_hy_message, parse_hy_push, TarsReader, TarsWriter};
use crate::util::J;

pub struct HuyaSource { pub st: Shared, pub args: Value }

fn join_data(ayyuid: i64, tid: i64, sid: i64) -> Vec<u8> {
    let mut inner = TarsWriter::new();
    inner.int(ayyuid, 0);
    inner.boolean(true, 1);
    inner.string("", 2);
    inner.string("", 3);
    inner.int(tid, 4);
    inner.int(sid, 5);
    inner.int(0, 6);
    inner.int(0, 7);
    let mut outer = TarsWriter::new();
    outer.int(1, 0);
    outer.bytes(&inner.finish(), 1);
    outer.finish()
}

fn decode(data: &[u8], tx: &Sender<DanmakuMessage>) {
    let mut s = TarsReader::new(data);
    if s.read_int(0).unwrap_or(0) != 7 { return; }
    let inner = s.read_bytes(1).unwrap_or_default();
    let (uri, msg) = match parse_hy_push(&inner) { Ok(v) => v, Err(_) => return };
    match uri {
        1400 => {
            if let Ok((nick, content, color)) = parse_hy_message(&msg) {
                let c = if color <= 0 { 0 } else { (color as u32) & 0xFFFFFF };
                emit(tx, DanmakuMessage::chat(nick, content, c));
            }
        }
        8006 => {
            let online = TarsReader::new(&msg).read_int(0).unwrap_or(0).max(0) as u64;
            emit(tx, DanmakuMessage::online_msg(online));
        }
        _ => {}
    }
}

#[async_trait]
impl DanmakuSource for HuyaSource {
    async fn run(self: Box<Self>, tx: Sender<DanmakuMessage>) -> Result<()> {
        let _ = &self.st;
        let ayyuid = self.args.i("ayyuid");
        let top = self.args.i("topSid");
        // 与 Android 版一致：tid / sid 均使用 topSid
        let join = join_data(ayyuid, top, top);
        let heartbeat = vec![0x00, 0x14, 0x1D, 0x00, 0x0C, 0x2C, 0x36, 0x00, 0x4C];
        ws_loop(
            "wss://cdnws.api.huya.com".into(),
            vec![("Origin".into(), "https://www.huya.com".into())],
            60_000, heartbeat, vec![join], tx,
            |data, tx, _| decode(data, tx),
        ).await
    }
}
