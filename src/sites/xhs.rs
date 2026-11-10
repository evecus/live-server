use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::Site;
use crate::http_client::h;
use crate::identity as ident;
use crate::models::{PlayQuality, RoomDetail, SearchResult};
use crate::state::Shared;
use crate::util::{extract_initial_state, parse_count, percent_decode, to_https, J};

pub struct XhsSite;

fn page_headers() -> HeaderMap {
    h(&[
        ("user-agent", ident::XHS_MOBILE_UA),
        ("referer", "https://www.xiaohongshu.com/"),
        ("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
        ("accept-language", "zh-CN,zh;q=0.9"),
    ])
}

fn decode_param(link: &str, key: &str) -> String {
    let q = link.split_once('?').map(|(_, q)| q).unwrap_or("");
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            if k == key { return percent_decode(v); }
        }
    }
    String::new()
}

fn quality_rank(t: &str) -> i64 {
    match t.to_uppercase().as_str() {
        "UHD" | "4K" => 4, "HD" => 3, "SD" => 2, "LD" => 1, _ => 0,
    }
}

/// pullConfig 是 JSON 字符串：{"h264":[{master_url,quality_type,quality_type_name}]}
fn parse_pull_config(raw: &Value) -> Vec<Value> {
    let text = match raw {
        Value::String(s) => s.clone(),
        Value::Object(_) => raw.to_string(),
        _ => return vec![],
    };
    if text.trim().is_empty() { return vec![]; }
    let obj: Value = match serde_json::from_str(&text) { Ok(v) => v, Err(_) => return vec![] };
    let mut out = Vec::new();
    for codec in ["h264", "h265"] {
        for item in obj.a(codec) {
            let url = item.s("master_url");
            if url.is_empty() { continue; }
            let quality_type = { let q = item.s("quality_type"); if q.is_empty() { "HD".to_string() } else { q } };
            out.push(json!({
                "codec": codec,
                "qualityType": quality_type,
                "qualityName": item.s("quality_type_name"),
                "url": to_https(&url),
            }));
        }
    }
    out
}

fn fallback_streams(room_id: &str) -> Vec<Value> {
    vec![
        json!({"codec":"h264","qualityType":"HD","qualityName":"原画","url":format!("https://live-source-play-hw.xhscdn.com/live/{room_id}.m3u8")}),
        json!({"codec":"h264","qualityType":"HD","qualityName":"原画","url":format!("https://live-source-play-hw.xhscdn.com/live/{room_id}.flv")}),
    ]
}

#[async_trait]
impl Site for XhsSite {
    fn id(&self) -> &'static str { "xiaohongshu" }
    fn name(&self) -> &'static str { "小红书直播" }
    fn searchable(&self) -> bool { false }
    fn danmaku_supported(&self) -> bool { false }

    async fn room_detail(&self, st: &Shared, room_id: &str) -> Result<RoomDetail> {
        let (html, _) = st.http.get_text(
            &format!("https://www.xiaohongshu.com/livestream/{room_id}"), &page_headers(),
        ).await?;
        let state = extract_initial_state(&html).ok_or_else(|| anyhow!("小红书直播间页面解析失败"))?;
        let info = state.o("liveStream").o("roomData").o("roomInfo");
        if info.is_null() || info.s("roomId").is_empty() {
            let mut d = RoomDetail::offline("xiaohongshu", room_id, &format!("小红书直播 {room_id}"));
            d.user_name = "小红书主播".into();
            d.play_data = json!({ "streams": [] });
            return Ok(d);
        }
        let status = info.i("status") == 2;
        let mut streams = parse_pull_config(&info.o("pullConfig"));
        if status && streams.is_empty() { streams = fallback_streams(room_id); }

        let mut d = RoomDetail::new("xiaohongshu", room_id);
        d.title = { let t = info.s("roomTitle"); if t.is_empty() { format!("小红书直播 {room_id}") } else { t } };
        d.cover = info.s("roomCover");
        d.user_name = { let n = decode_param(&info.s("deeplink"), "host_nickname"); if n.is_empty() { "小红书主播".into() } else { n } };
        d.online = parse_count(&info.s("displayViewerCount"));
        d.status = status;
        d.url = format!("https://www.xiaohongshu.com/livestream/{room_id}");
        d.play_data = json!({ "streams": streams });
        Ok(d)
    }

    async fn play(&self, _st: &Shared, detail: &RoomDetail) -> Result<Vec<PlayQuality>> {
        let streams = detail.play_data.a("streams");
        if streams.is_empty() { return Err(anyhow!("小红书主播未开播或未获取到播放地址")); }
        let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
        for s in &streams { groups.entry(s.s("qualityType")).or_default().push(s); }
        let mut out: Vec<PlayQuality> = groups.into_iter().map(|(t, items)| {
            let name = { let n = items.first().map(|v| v.s("qualityName")).unwrap_or_default(); if n.is_empty() { t.clone() } else { n } };
            let mut sorted = items.clone();
            sorted.sort_by_key(|v| if v.s("url").contains(".m3u8") { 0 } else { 1 });
            let urls: Vec<String> = sorted.iter().map(|v| v.s("url")).filter(|u| !u.is_empty()).collect();
            PlayQuality::new(name, quality_rank(&t).to_string(), urls)
                .with_headers(&[("user-agent", ident::XHS_MOBILE_UA), ("referer", "https://www.xiaohongshu.com/")])
        }).collect();
        out.sort_by(|a, b| b.key.parse::<i64>().unwrap_or(0).cmp(&a.key.parse::<i64>().unwrap_or(0)));
        Ok(out)
    }

    async fn live_status(&self, st: &Shared, room_id: &str) -> Result<bool> {
        Ok(self.room_detail(st, room_id).await.map(|d| d.status).unwrap_or(false))
    }

    async fn search(&self, _st: &Shared, _kw: &str, _page: u32, _anchor: bool) -> Result<SearchResult> {
        Err(anyhow!("小红书暂不支持搜索，可通过分享链接添加直播间"))
    }
}
