use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, SET_COOKIE};
use serde_json::{json, Value};
use std::sync::{OnceLock, RwLock};

use super::Site;
use crate::http_client::{h, with};
use crate::identity as ident;
use crate::models::{PlayQuality, RoomDetail, SearchItem, SearchResult};
use crate::state::Shared;
use crate::util::{extract_initial_state, merge_cookies, parse_count, percent_decode, random_from, urlencode, J};

pub struct KuaishouSite;

static COOKIE: OnceLock<RwLock<String>> = OnceLock::new();
fn cookie_store() -> &'static RwLock<String> { COOKIE.get_or_init(|| RwLock::new(String::new())) }
fn cookie() -> String { cookie_store().read().unwrap().clone() }

fn base_headers() -> HeaderMap {
    h(&[
        ("user-agent", ident::KUAISHOU_UA),
        ("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,image/apng,*/*;q=0.8"),
        ("accept-language", "zh-CN,zh;q=0.9"),
        ("sec-ch-ua", "Google Chrome;v=120, Chromium;v=120, Not=A?Brand;v=24"),
        ("sec-ch-ua-platform", "Windows"),
        ("Sec-Fetch-Dest", "document"),
        ("Sec-Fetch-Mode", "navigate"),
        ("Sec-Fetch-Site", "same-origin"),
    ])
}
fn headers() -> HeaderMap {
    let c = cookie();
    if c.is_empty() { base_headers() } else { with(base_headers(), &[("cookie", &c)]) }
}

fn save_cookie(hs: &HeaderMap) {
    let mut map = std::collections::BTreeMap::new();
    for part in cookie().split(';') {
        let mut it = part.splitn(2, '=');
        if let (Some(k), Some(v)) = (it.next(), it.next()) { map.insert(k.trim().to_string(), v.trim().to_string()); }
    }
    for v in hs.get_all(SET_COOKIE).iter() {
        if let Ok(s) = v.to_str() {
            let first = s.split(';').next().unwrap_or("");
            let mut it = first.splitn(2, '=');
            if let (Some(k), Some(val)) = (it.next(), it.next()) { map.insert(k.trim().to_string(), val.trim().to_string()); }
        }
    }
    let joined = map.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
    *cookie_store().write().unwrap() = joined;
}

fn merge_user_cookie(raw: &str) {
    let parts: Vec<String> = raw.split(';').filter_map(|p| {
        let mut it = p.trim().splitn(2, '=');
        match (it.next(), it.next()) {
            (Some(k), Some(v)) if !k.trim().is_empty() && !v.trim().is_empty() => Some(format!("{}={}", k.trim(), v.trim())),
            _ => None,
        }
    }).collect();
    if parts.is_empty() { return; }
    let mut hs = HeaderMap::new();
    for p in parts { if let Ok(v) = axum::http::HeaderValue::from_str(&p) { hs.append(SET_COOKIE, v); } }
    save_cookie(&hs);
}

fn is_image(url: &str) -> bool {
    let ext = url.split('?').next().unwrap_or("").rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "ico" | "avif" | "tiff" | "tif" | "jfif" | "pjpeg" | "pjp" | "svgz" | "xbm")
}

fn string_array(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.txt()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

fn is_live_flag(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_i64().unwrap_or(0) == 1,
        Value::String(s) => s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn resolve_live_status(room: &Value) -> bool {
    if is_live_flag(&room.o("isLiving")) || is_live_flag(&room.o("living")) { return true; }
    !room.o("liveStream").s("id").is_empty() || !room.s("id").is_empty()
}

fn generate_page_id() -> String {
    random_from(b"useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict", 16)
}

fn resolve_server_kww(cookie: &str) -> String {
    for part in cookie.split(';') {
        let item = part.trim();
        if let Some(v) = item.strip_prefix("kwfv1=") {
            if !v.trim().is_empty() { return format!("{}###ssrc", percent_decode(v.trim())); }
        }
    }
    String::new()
}

async fn fetch_ws_info(st: &Shared, room_id: &str, live_stream_id: &str) -> (String, Vec<String>) {
    let c = cookie();
    let mut hs = with(headers(), &[
        ("accept", "application/json, text/plain, */*"),
        ("referer", &format!("https://live.kuaishou.com/u/{room_id}")),
        ("Sec-Fetch-Dest", "empty"),
        ("Sec-Fetch-Mode", "cors"),
    ]);
    let kww = resolve_server_kww(&c);
    if !kww.is_empty() { hs = with(hs, &[("Kww", &kww)]); }
    let url = format!("https://live.kuaishou.com/live_api/liveroom/websocketinfo?liveStreamId={live_stream_id}");
    match st.http.get_json(&url, &hs).await {
        Ok(v) => {
            let d = v.o("data");
            let urls = { let u = string_array(&d.o("websocketUrls")); if u.is_empty() { string_array(&d.o("webSocketAddresses")) } else { u } };
            (d.s("token"), urls)
        }
        Err(_) => (String::new(), vec![]),
    }
}

async fn parse_detail(st: &Shared, html: &str, room_id: &str) -> Result<RoomDetail> {
    let state = extract_initial_state(html).ok_or_else(|| anyhow!("快手页面状态解析失败"))?;
    let liveroom = state.o("liveroom");
    let play_list = liveroom.a("playList");
    if play_list.is_empty() { return Ok(RoomDetail::offline("kuaishou", room_id, "")); }

    let matched: Vec<&Value> = play_list.iter().filter(|r| r.o("author").s("id").trim() == room_id.trim()).collect();
    let candidates: Vec<&Value> = if matched.is_empty() { play_list.iter().collect() } else { matched };
    let first = candidates.iter().copied().find(|r| resolve_live_status(r)).or_else(|| candidates.first().copied())
        .ok_or_else(|| anyhow!("快手直播间为空"))?;

    let live_stream = first.o("liveStream");
    let author = first.o("author");
    let game = first.o("gameInfo");
    let is_living = resolve_live_status(first);
    let live_stream_id = live_stream.s("id");
    let real_id = { let x = author.s("id"); if x.is_empty() { room_id.to_string() } else { x } };
    let mut ws_urls = string_array(&liveroom.o("websocketUrls"));
    let mut token = liveroom.s("token");
    if let Some(ws_info) = first.get("websocketInfo") {
        if token.is_empty() { token = ws_info.s("token"); }
        if ws_urls.is_empty() {
            ws_urls = string_array(&ws_info.o("websocketUrls"));
            if ws_urls.is_empty() { ws_urls = string_array(&ws_info.o("webSocketAddresses")); }
        }
    }
    if is_living && !live_stream_id.is_empty() && (token.is_empty() || ws_urls.is_empty()) {
        let (t, u) = fetch_ws_info(st, &real_id, &live_stream_id).await;
        if token.is_empty() { token = t; }
        if ws_urls.is_empty() { ws_urls = u; }
    }
    let mut cover = live_stream.s("poster");
    if !cover.is_empty() && !is_image(&cover) { cover.push_str(".jpg"); }
    let title = [first.s("caption"), first.s("title"), live_stream.s("caption"), live_stream.s("title"), game.s("name"), author.s("name")]
        .into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| format!("{}的直播间", author.s("name")));

    let mut d = RoomDetail::new("kuaishou", &real_id);
    d.title = title;
    d.cover = cover;
    d.user_name = { let n = author.s("name"); if n.is_empty() { room_id.into() } else { n } };
    d.user_avatar = author.s("avatar");
    d.online = if is_living { parse_count(&game.s("watchingCount")) } else { 0 };
    d.status = is_living;
    d.url = format!("https://live.kuaishou.com/u/{real_id}");
    d.introduction = author.s("description");
    d.danmaku_args = json!({
        "roomId": real_id, "liveStreamId": live_stream_id, "token": token,
        "websocketUrls": ws_urls, "pageId": generate_page_id(),
        "expTag": live_stream.s("expTag"), "attach": first.s("expTag"),
        "cookie": cookie(), "userAgent": ident::KUAISHOU_UA,
    });
    d.play_data = live_stream.o("playUrls");
    Ok(d)
}

async fn warm_up(st: &Shared, kw: &str) {
    let user_cookie = st.store.settings().await.cookies.kuaishou;
    if !user_cookie.trim().is_empty() { merge_user_cookie(&user_cookie); }
    if cookie().is_empty() {
        if let Ok((_, hs)) = st.http.get_text(
            &format!("https://live.kuaishou.com/search?keyword={}", urlencode(kw)), &headers(),
        ).await { save_cookie(&hs); }
    }
}

fn search_headers(kw: &str) -> HeaderMap {
    with(headers(), &[
        ("accept", "application/json, text/plain, */*"),
        ("referer", &format!("https://live.kuaishou.com/search?keyword={}", urlencode(kw))),
        ("Sec-Fetch-Dest", "empty"),
        ("Sec-Fetch-Mode", "cors"),
    ])
}

fn parse_room_item(it: &Value) -> SearchItem {
    let author = it.o("author");
    let game = it.o("gameInfo");
    let mut cover = [it.s("poster"), it.s("coverUrl"), game.s("poster")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
    if !cover.is_empty() && !is_image(&cover) { cover.push_str(".jpg"); }
    let user_name = author.s("name");
    let title = if !it.s("caption").is_empty() { it.s("caption") }
        else if !it.s("title").is_empty() { it.s("title") } else { user_name.clone() };
    let room_id = [author.s("id"), it.s("authorId"), it.s("userId")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
    SearchItem { room_id, title, cover, user_name, avatar: String::new(), online: parse_count(&it.s("watchingCount")), live_status: true }
}

#[async_trait]
impl Site for KuaishouSite {
    fn id(&self) -> &'static str { "kuaishou" }
    fn name(&self) -> &'static str { "快手直播" }

    async fn room_detail(&self, st: &Shared, room_id: &str) -> Result<RoomDetail> {
        let url = format!("https://live.kuaishou.com/u/{room_id}");
        let mut anonymous: Option<RoomDetail> = None;
        if let Ok((body, hs)) = st.http.get_text(&url, &base_headers()).await {
            save_cookie(&hs);
            anonymous = parse_detail(st, &body, room_id).await.ok();
        }
        if let Some(a) = &anonymous { if a.status { return Ok(a.clone()); } }
        if !cookie().is_empty() {
            if let Ok((body, _)) = st.http.get_text(&url, &headers()).await {
                if let Ok(d) = parse_detail(st, &body, room_id).await {
                    if d.status || anonymous.is_none() { return Ok(d); }
                }
            }
        }
        anonymous.ok_or_else(|| anyhow!("快手直播间状态暂时不可用"))
    }

    async fn play(&self, _st: &Shared, detail: &RoomDetail) -> Result<Vec<PlayQuality>> {
        let play_urls = &detail.play_data;
        let h264 = if play_urls.o("h264").is_object() { play_urls.o("h264") }
            else if let Some(v) = play_urls.a("h264").first() { v.clone() } else { Value::Null };
        let reps = h264.o("adaptationSet").a("representation");
        if reps.is_empty() { return Err(anyhow!("快手未获取到清晰度列表")); }
        let mut out = Vec::new();
        for rep in &reps {
            if rep.b("hidden") { continue; }
            let url = rep.s("url");
            if url.is_empty() { continue; }
            let level = rep.i("level");
            let mut urls = vec![url];
            for hevc in play_urls.o("hevc").o("adaptationSet").a("representation") {
                if hevc.i("level") == level {
                    let u = hevc.s("url");
                    if !u.is_empty() { urls.push(u); }
                    break;
                }
            }
            out.push(PlayQuality::new(rep.s("name"), level.to_string(), urls)
                .with_headers(&[("referer", "https://live.kuaishou.com/")]));
        }
        out.sort_by(|a, b| b.key.parse::<i64>().unwrap_or(0).cmp(&a.key.parse::<i64>().unwrap_or(0)));
        if out.is_empty() { return Err(anyhow!("快手未获取到清晰度列表")); }
        Ok(out)
    }

    async fn live_status(&self, st: &Shared, room_id: &str) -> Result<bool> {
        Ok(self.room_detail(st, room_id).await.map(|d| d.status).unwrap_or(false))
    }

    async fn search(&self, st: &Shared, kw: &str, page: u32, anchor: bool) -> Result<SearchResult> {
        warm_up(st, kw).await;
        let k = urlencode(kw);
        if !anchor {
            let primary = st.http.get_json(
                &format!("https://live.kuaishou.com/live_api/search/liveStream?keyword={k}&page={page}&ussid="),
                &search_headers(kw),
            ).await.ok();
            if let Some(v) = primary {
                let d = v.o("data");
                if d.i("result") == 1 {
                    let items: Vec<SearchItem> = d.a("list").iter().map(parse_room_item).filter(|i| !i.room_id.is_empty()).collect();
                    if !items.is_empty() || page > 1 { return Ok(SearchResult { has_more: !items.is_empty(), items }); }
                }
            }
            if page > 1 { return Ok(SearchResult::empty()); }
            // overview 兜底
            if let Ok(v) = st.http.get_json(&format!("https://live.kuaishou.com/live_api/search/overview?keyword={k}&ussid="), &search_headers(kw)).await {
                for section in v.o("data").a("list") {
                    if section.s("type") == "liveStreams" {
                        let items: Vec<SearchItem> = section.a("list").iter().map(parse_room_item).filter(|i| !i.room_id.is_empty()).collect();
                        return Ok(SearchResult { has_more: false, items });
                    }
                }
            }
            Ok(SearchResult::empty())
        } else {
            let primary = st.http.get_json(
                &format!("https://live.kuaishou.com/live_api/search/author?key={k}&keyword={k}&page={page}&count=15&ussid=&lssid="),
                &search_headers(kw),
            ).await.ok();
            if let Some(v) = primary {
                let d = v.o("data");
                if d.i("result") == 1 {
                    let items: Vec<SearchItem> = d.a("list").iter().map(|a| SearchItem {
                        room_id: a.s("id"), title: String::new(), cover: a.s("avatar"),
                        user_name: a.s("name"), avatar: a.s("avatar"), online: 0,
                        live_status: is_live_flag(&a.o("living")),
                    }).filter(|i| !i.room_id.is_empty()).collect();
                    if !items.is_empty() || page > 1 { return Ok(SearchResult { has_more: items.len() >= 15, items }); }
                }
            }
            if page > 1 { return Ok(SearchResult::empty()); }
            if let Ok(v) = st.http.get_json(&format!("https://live.kuaishou.com/live_api/search/overview?keyword={k}&ussid="), &search_headers(kw)).await {
                for section in v.o("data").a("list") {
                    if section.s("type") == "authors" {
                        let items: Vec<SearchItem> = section.a("list").iter().map(|a| SearchItem {
                            room_id: a.s("id"), title: String::new(), cover: a.s("avatar"),
                            user_name: a.s("name"), avatar: a.s("avatar"), online: 0,
                            live_status: is_live_flag(&a.o("living")),
                        }).filter(|i| !i.room_id.is_empty()).collect();
                        return Ok(SearchResult { has_more: false, items });
                    }
                }
            }
            Ok(SearchResult::empty())
        }
    }
}

#[allow(dead_code)]
fn _unused() { let _ = merge_cookies(&[]); }
