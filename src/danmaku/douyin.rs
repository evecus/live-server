use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::broadcast::Sender;
use url::Url;

use super::{emit, gunzip, try_urls, DanmakuSource};
use crate::identity as ident;
use crate::models::DanmakuMessage;
use crate::pb::{Pb, PbWriter};
use crate::sign::douyin_ms_stub;
use crate::state::Shared;
use crate::util::{cookie_value, unix_ms, urlencode, J};

pub struct DouyinSource { pub st: Shared, pub args: Value }

const SERVER_URL: &str = "wss://webcast3-ws-web-lq.douyin.com/webcast/im/push/v2/";

struct ImContext { push_server: String, cursor: String, internal_ext: String, heartbeat: u64 }

fn qs(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v))).collect::<Vec<_>>().join("&")
}

fn build_query(room_id: &str, user_id: &str, cursor: Option<&str>, internal_ext: Option<&str>, heartbeat: u64) -> Vec<(String, String)> {
    let ts = unix_ms();
    let mut q: Vec<(String, String)> = vec![
        ("app_name", "douyin_web"), ("version_code", "180800"),
        ("webcast_sdk_version", "1.3.0"), ("update_version_code", "1.3.0"),
        ("compress", "gzip"), ("resp_content_type", "protobuf"),
        ("cursor", cursor.unwrap_or("")),
    ].into_iter().map(|(k, v)| (k.into(), v.into())).collect();
    if q[6].1.is_empty() { q[6].1 = format!("h-1_t-{ts}_r-1_d-1_u-1"); }
    let extra: Vec<(&str, &str)> = vec![
        ("host", "https://live.douyin.com"), ("aid", "6383"), ("live_id", "1"),
        ("did_rule", "3"), ("debug", "false"), ("maxCacheMessageNumber", "20"),
        ("endpoint", "live_pc"), ("support_wrds", "1"),
        ("im_path", "/webcast/im/fetch/"), ("user_unique_id", user_id),
        ("device_platform", "web"), ("cookie_enabled", "true"),
        ("screen_width", "1920"), ("screen_height", "1080"),
        ("browser_language", "zh-CN"), ("browser_platform", "Win32"),
        ("browser_name", "Mozilla"),
        ("browser_version", ident::DOUYIN_UA.trim_start_matches("Mozilla/")),
        ("browser_online", "true"), ("tz_name", "Asia/Shanghai"),
        ("identity", "audience"), ("room_id", room_id),
    ];
    q.extend(extra.into_iter().map(|(k, v)| (k.into(), v.into())));
    q.push(("heartbeatDuration".into(), heartbeat.to_string()));
    if let Some(ext) = internal_ext.filter(|s| !s.is_empty()) { q.push(("internal_ext".into(), ext.into())); }
    q
}

fn socket_headers(web_rid: &str, cookie: &str) -> Vec<(String, String)> {
    vec![
        ("accept".into(), "application/json, text/plain, */*".into()),
        ("accept-language".into(), "zh-CN,zh;q=0.9,en;q=0.8".into()),
        ("cache-control".into(), "no-cache".into()),
        ("user-agent".into(), ident::DOUYIN_UA.into()),
        ("cookie".into(), cookie.into()),
        ("origin".into(), "https://live.douyin.com".into()),
        ("referer".into(), format!("https://live.douyin.com/{web_rid}")),
    ]
}

fn normalize_push_server(value: &str) -> String {
    let mut server = value.trim().to_string();
    if server.is_empty() { return SERVER_URL.into(); }
    if !server.contains("://") { server = format!("wss://{server}"); }
    let mut u = match Url::parse(&server) { Ok(u) => u, Err(_) => return SERVER_URL.into() };
    let _ = u.set_scheme("wss");
    if u.path().is_empty() || u.path() == "/" { u.set_path("/webcast/im/push/v2/"); }
    u.to_string()
}

async fn fetch_im_context(st: &Shared, room_id: &str, user_id: &str, cookie: &str, ms_token: Option<&str>) -> Result<ImContext> {
    let q = build_query(room_id, user_id, None, None, 0);
    let stub = douyin_ms_stub(room_id, user_id);
    let signature = st.sign.douyin_signature(&stub, ident::DOUYIN_UA).await?;
    let unsigned = format!("https://live.douyin.com/webcast/im/fetch/?{}&signature={}", qs(&q), urlencode(&signature));
    let signed = st.sign.douyin_a_bogus_url(&unsigned, ident::DOUYIN_UA, ms_token).await?;
    let headers = crate::http_client::h(&[
        ("accept", "application/json, text/plain, */*"),
        ("user-agent", ident::DOUYIN_UA),
        ("cookie", cookie),
        ("origin", "https://live.douyin.com"),
        ("referer", "https://live.douyin.com"),
    ]);
    let bytes = st.http.get_bytes(&signed, &headers).await?;
    if bytes.is_empty() || bytes[0] == b'{' { bail!("IM 预取响应不可用"); }
    let resp = Pb::parse(&gunzip(&bytes));
    let cursor = { let c = resp.str(2); if c.is_empty() { resp.str(11) } else { c } };
    if cursor.is_empty() { bail!("IM 预取缺少动态游标"); }
    Ok(ImContext {
        push_server: resp.str(10),
        cursor,
        internal_ext: resp.str(5),
        heartbeat: resp.int(8),
    })
}

async fn socket_url(st: &Shared, base: &str, room_id: &str, user_id: &str, ctx: Option<&ImContext>) -> Result<String> {
    let q = build_query(room_id, user_id, ctx.map(|c| c.cursor.as_str()), ctx.map(|c| c.internal_ext.as_str()), ctx.map(|c| c.heartbeat).unwrap_or(0));
    let stub = douyin_ms_stub(room_id, user_id);
    let signature = st.sign.douyin_signature(&stub, ident::DOUYIN_UA).await?;
    Ok(format!("{base}?{}&signature={}", qs(&q), urlencode(&signature)))
}

fn decode(data: &[u8], tx: &Sender<DanmakuMessage>, replies: &mut Vec<Vec<u8>>) {
    let frame = Pb::parse(data);
    let payload_type = frame.str(7);
    let log_id = frame.int(2);
    let payload = frame.bytes(8);
    if payload_type != "msg" && !payload_type.is_empty() { return; }
    let resp = Pb::parse(&gunzip(&payload));
    if resp.has(9) && resp.int(9) == 1 && log_id != 0 {
        let mut w = PbWriter::new();
        w.str(7, "ack").uint64(2, log_id).bytes(8, resp.str(5).as_bytes());
        replies.push(w.finish());
    }
    for msg in resp.msgs(1) {
        match msg.str(1).as_str() {
            "WebcastChatMessage" => unpack_chat(&msg.bytes(2), tx),
            "WebcastRoomUserSeqMessage" => {
                let m = Pb::parse(&msg.bytes(2));
                emit(tx, DanmakuMessage::online_msg(m.int(3)));
            }
            _ => {}
        }
    }
}

fn unpack_chat(payload: &[u8], tx: &Sender<DanmakuMessage>) {
    let chat = Pb::parse(payload);
    let nick = chat.msgs(2).first().map(|u| u.str(3)).unwrap_or_default();
    let mut content = chat.str(3).trim().to_string();
    if content.is_empty() {
        if let Some(rtf) = chat.msgs(22).first() {
            let mut sb = String::new();
            for piece in rtf.msgs(4) {
                let mut v = piece.str(3);
                if v.is_empty() { v = piece.msgs(7).first().map(|x| x.str(2)).unwrap_or_default(); }
                sb.push_str(&v);
            }
            content = sb.trim().to_string();
        }
    }
    if content.is_empty() { return; }
    emit(tx, DanmakuMessage::chat(nick, content, 0));
}

#[async_trait]
impl DanmakuSource for DouyinSource {
    async fn run(self: Box<Self>, tx: Sender<DanmakuMessage>) -> Result<()> {
        let st = self.st;
        let a = self.args;
        let web_rid = a.s("webRid");
        let room_id = a.s("roomId");
        let user_id = a.s("userId");
        let cookie = a.s("cookie");
        if room_id.is_empty() { bail!("抖音真实房间号为空"); }
        let ms_token = cookie_value(&cookie, "msToken");

        let ctx = match fetch_im_context(&st, &room_id, &user_id, &cookie, ms_token.as_deref()).await {
            Ok(c) => Some(c),
            Err(e) => { tracing::warn!("抖音 IM 预取失败，使用静态地址：{e}"); None }
        };
        let hb_ms = ctx.as_ref().map(|c| c.heartbeat).filter(|d| (1000..=120_000).contains(d)).unwrap_or(10_000);
        let static_url = socket_url(&st, SERVER_URL, &room_id, &user_id, ctx.as_ref()).await?;
        let mut urls: Vec<String> = Vec::new();
        if let Some(c) = &ctx {
            let base = normalize_push_server(&c.push_server);
            urls.push(socket_url(&st, &base, &room_id, &user_id, Some(c)).await?);
        }
        urls.push(static_url.clone());
        for alt in ["webcast5-ws-web-lf", "webcast5-ws-web-hl", "webcast3-ws-web-hl", "webcast3-ws-web-lf"] {
            urls.push(static_url.replace("webcast3-ws-web-lq", alt));
        }

        let mut w = PbWriter::new();
        w.str(7, "hb");
        let heartbeat = w.finish();
        try_urls(urls, socket_headers(&web_rid, &cookie), hb_ms, heartbeat, vec![], tx,
            |data, tx, replies| decode(data, tx, replies)).await
    }
}
