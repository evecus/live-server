use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use reqwest::header::{HeaderMap, SET_COOKIE};
use serde_json::{json, Value};
use std::sync::OnceLock;

use super::Site;
use crate::http_client::{h, with};
use crate::identity as ident;
use crate::models::{PlayQuality, RoomDetail, SearchItem, SearchResult};
use crate::state::Shared;
use super::arr_str;
use crate::util::{cookie_value, merge_cookies, parse_state_from_page, random_digits, unix_secs, urlencode, J};

pub struct DouyinSite;

static WEB_COOKIES: OnceLock<DashMap<String, (u64, String)>> = OnceLock::new();
fn cookie_cache() -> &'static DashMap<String, (u64, String)> {
    WEB_COOKIES.get_or_init(DashMap::new)
}
const WEB_COOKIE_TTL: u64 = 300;

fn extract_ttwid(cookie: &str) -> Option<String> {
    cookie.split(';').map(|p| p.trim()).find(|p| p.starts_with("ttwid=") && p.len() > 6).map(|p| p.to_string())
}
fn default_playback_cookie() -> String {
    extract_ttwid(ident::DOUYIN_DEFAULT_COOKIE).unwrap_or_else(|| ident::DOUYIN_DEFAULT_COOKIE.into())
}

async fn custom_cookie(st: &Shared) -> String { st.store.settings().await.cookies.douyin.trim().to_string() }
async fn playback_cookie(st: &Shared) -> String {
    let c = custom_cookie(st).await;
    extract_ttwid(&c).unwrap_or_else(default_playback_cookie)
}
async fn search_cookie(st: &Shared) -> String {
    let c = custom_cookie(st).await;
    if c.is_empty() { default_playback_cookie() } else { c }
}

fn request_headers(cookie: &str) -> HeaderMap {
    h(&[
        ("authority", "live.douyin.com"),
        ("referer", "https://live.douyin.com"),
        ("user-agent", ident::DOUYIN_UA),
        ("cookie", cookie),
    ])
}

fn collect_cookies(headers: &HeaderMap) -> String {
    let mut parts = Vec::new();
    for v in headers.get_all(SET_COOKIE).iter() {
        if let Ok(s) = v.to_str() {
            let first = s.split(';').next().unwrap_or("");
            if first.contains("ttwid") || first.contains("__ac_nonce") || first.contains("msToken") {
                parts.push(first.to_string());
            }
        }
    }
    parts.join("; ")
}

async fn get_web_cookie(st: &Shared, web_rid: &str) -> Result<String> {
    let base = playback_cookie(st).await;
    let key = format!("{web_rid}|{base}");
    let now = unix_secs() as u64;
    if let Some(v) = cookie_cache().get(&key) {
        if now - v.0 < WEB_COOKIE_TTL { return Ok(v.1.clone()); }
    }
    let hs = with(request_headers(&base), &[("referer", &format!("https://live.douyin.com/{web_rid}"))]);
    let head = st.http.head(&format!("https://live.douyin.com/{web_rid}"), &hs).await;
    let fresh = match head {
        Ok(h) => collect_cookies(&h),
        Err(_) if !base.is_empty() => String::new(),
        Err(e) => return Err(e),
    };
    let merged = merge_cookies(&[&base, &fresh]);
    cookie_cache().insert(key, (now, merged.clone()));
    Ok(merged)
}

async fn danmaku_cookie(st: &Shared, web_rid: &str) -> String {
    let base = playback_cookie(st).await;
    let web = get_web_cookie(st, web_rid).await.unwrap_or_default();
    if !custom_cookie(st).await.is_empty() { merge_cookies(&[&web, &base]) } else { merge_cookies(&[&base, &web]) }
}

async fn room_data_by_api(st: &Shared, web_rid: &str, cookie: &str) -> Result<Value> {
    let uri = format!(
        "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome&browser_version=125.0.0.0&web_rid={web_rid}&msToken="
    );
    let signed = st.sign.douyin_a_bogus_url(&uri, ident::DOUYIN_UA, None).await?;
    let hs = with(request_headers(cookie), &[("referer", &format!("https://live.douyin.com/{web_rid}"))]);
    let v = st.http.get_json(&signed, &hs).await?;
    let data = v.o("data");
    if data.a("data").is_empty() { return Err(anyhow!("抖音直播间数据为空")); }
    Ok(data)
}

/// 已配置用户 Cookie 但请求失败时，用匿名 ttwid 重试一次
async fn room_data_by_api_checked(st: &Shared, web_rid: &str) -> Result<Value> {
    let cookie = playback_cookie(st).await;
    let custom = !custom_cookie(st).await.is_empty();
    match room_data_by_api(st, web_rid, &cookie).await {
        Ok(v) if !custom || !v.a("data").is_empty() => Ok(v),
        Ok(_) => room_data_by_api(st, web_rid, &default_playback_cookie()).await,
        Err(e) if custom => room_data_by_api(st, web_rid, &default_playback_cookie()).await.or(Err(e)),
        Err(e) => Err(e),
    }
}

async fn room_data_by_html(st: &Shared, web_rid: &str) -> Result<Value> {
    let cookie = get_web_cookie(st, web_rid).await?;
    let (html, _) = st.http.get_text(
        &format!("https://live.douyin.com/{web_rid}"),
        &with(request_headers(&cookie), &[("referer", "https://live.douyin.com")]),
    ).await?;
    let state = parse_state_from_page(&html).ok_or_else(|| anyhow!("抖音直播间页面状态数据不可用"))?;
    let room = state.o("roomStore").o("roomInfo").o("room");
    if room.is_null() { return Err(anyhow!("抖音直播间数据异常：room 为空")); }
    Ok(room)
}

async fn room_data_by_room_id(st: &Shared, room_id: &str) -> Result<Value> {
    let url = format!(
        "https://webcast.amemv.com/webcast/room/reflow/info/?type_id=0&live_id=1&room_id={room_id}&sec_user_id=&version_code=99.99.99&app_id=6383"
    );
    let cookie = playback_cookie(st).await;
    let v = st.http.get_json(&url, &request_headers(&cookie)).await
        .or_else(|_| Err(anyhow!("reflow 请求失败")))?;
    let room = v.o("data").o("room");
    if room.is_null() { return Err(anyhow!("抖音直播间数据为空")); }
    Ok(room)
}

async fn build_detail(st: &Shared, room: &Value, web_rid: &str, room_id: &str) -> Result<RoomDetail> {
    let owner = room.o("owner");
    let status = room.i("status") == 2;
    let cover = if status { arr_str(&room.o("cover").o("url_list"), 0) } else { String::new() };
    let rid = if web_rid.is_empty() { room_id } else { web_rid };
    let real_room_id = room.s("id_str");
    let cookie = danmaku_cookie(st, rid).await;
    let mut d = RoomDetail::new("douyin", rid);
    d.title = room.s("title");
    d.cover = cover;
    d.user_name = owner.s("nickname");
    d.user_avatar = arr_str(&owner.o("avatar_thumb").o("url_list"), 0);
    d.status = status;
    d.url = format!("https://live.douyin.com/{rid}");
    d.introduction = owner.s("signature");
    d.play_data = room.o("stream_url");
    d.danmaku_args = json!({
        "webRid": rid,
        "roomId": real_room_id,
        "userId": random_digits(12),
        "cookie": cookie,
    });
    Ok(d)
}

async fn detail_by_web_rid(st: &Shared, web_rid: &str) -> Result<RoomDetail> {
    let api = room_data_by_api_checked(st, web_rid).await;
    match api {
        Ok(data) => {
            let first = data.a("data").first().cloned().unwrap_or(Value::Null);
            let room_id = first.s("id_str");
            let mut d = build_detail(st, &first, web_rid, &room_id).await?;
            d.status = first.i("status") == 2;
            Ok(d)
        }
        Err(e) => {
            tracing::warn!("抖音 enter 接口失败，回退 HTML：{e}");
            let room = room_data_by_html(st, web_rid).await?;
            let room_id = room.s("id_str");
            let mut d = build_detail(st, &room, web_rid, &room_id).await?;
            d.status = room.i("status") == 2;
            Ok(d)
        }
    }
}

#[async_trait]
impl Site for DouyinSite {
    fn id(&self) -> &'static str { "douyin" }
    fn name(&self) -> &'static str { "抖音直播" }

    async fn room_detail(&self, st: &Shared, room_id: &str) -> Result<RoomDetail> {
        if room_id.len() <= 16 { return detail_by_web_rid(st, room_id).await; }
        let room = room_data_by_room_id(st, room_id).await?;
        let web_rid = room.o("owner").s("web_rid");
        if room.i("status") == 4 && !web_rid.is_empty() {
            return detail_by_web_rid(st, &web_rid).await;
        }
        let real = room.s("id_str");
        build_detail(st, &room, &web_rid, real.as_str()).await
    }

    async fn play(&self, _st: &Shared, detail: &RoomDetail) -> Result<Vec<PlayQuality>> {
        let stream_url = &detail.play_data;
        let pull = stream_url.o("live_core_sdk_data").o("pull_data");
        let qualities = pull.o("options").a("qualities");
        if qualities.is_empty() { return Ok(vec![]); }
        let stream_data = pull.s("stream_data");
        let mut out: Vec<PlayQuality> = Vec::new();

        if !stream_data.starts_with('{') {
            let flv: Vec<String> = match stream_url.o("flv_pull_url") {
                Value::Object(m) => m.values().map(|v| v.txt()).filter(|s| !s.is_empty()).collect(),
                _ => vec![],
            };
            let hls: Vec<String> = match stream_url.o("hls_pull_url_map") {
                Value::Object(m) => m.values().map(|v| v.txt()).filter(|s| !s.is_empty()).collect(),
                _ => vec![],
            };
            for q in &qualities {
                let level = q.i("level") as usize;
                let mut urls = Vec::new();
                if hls.len() >= level && level > 0 { if let Some(u) = hls.get(hls.len() - level) { urls.push(u.clone()); } }
                if flv.len() >= level && level > 0 { if let Some(u) = flv.get(flv.len() - level) { urls.push(u.clone()); } }
                if !urls.is_empty() { out.push(PlayQuality::new(q.s("name"), level.to_string(), urls)); }
            }
        } else {
            let data: Value = serde_json::from_str(&stream_data).unwrap_or(Value::Null);
            let quality_data = data.o("data");
            for q in &qualities {
                let sdk = q.s("sdk_key");
                let main = quality_data.o(&sdk).o("main");
                let mut urls = Vec::new();
                let hls = main.s("hls"); if !hls.is_empty() { urls.push(hls); }
                let flv = main.s("flv"); if !flv.is_empty() { urls.push(flv); }
                if !urls.is_empty() { out.push(PlayQuality::new(q.s("name"), q.i("level").to_string(), urls)); }
            }
        }
        out.sort_by(|a, b| b.key.parse::<i64>().unwrap_or(0).cmp(&a.key.parse::<i64>().unwrap_or(0)));
        Ok(out)
    }

    async fn live_status(&self, st: &Shared, room_id: &str) -> Result<bool> {
        match self.room_detail(st, room_id).await {
            Ok(d) => Ok(d.status),
            Err(_) => Ok(false),
        }
    }

    async fn search(&self, st: &Shared, kw: &str, page: u32, anchor: bool) -> Result<SearchResult> {
        let offset = (page - 1) * 10;
        let url = format!(
            "https://www.douyin.com/aweme/v1/web/live/search/?device_platform=webapp&aid=6383&channel=channel_pc_web&search_channel=aweme_live&keyword={}&search_source=switch_tab&query_correct_type=1&is_filter_search=0&from_group_id=&offset={offset}&count=10&pc_client_type=1&version_code=170400&version_name=17.4.0&cookie_enabled=true&screen_width=1980&screen_height=1080&browser_language=zh-CN&browser_platform=Win32&browser_name=Edge&browser_version=125.0.0.0&browser_online=true&engine_name=Blink&engine_version=125.0.0.0&os_name=Windows&os_version=10&cpu_core_num=12&device_memory=8&platform=PC&downlink=10&effective_type=4g&round_trip_time=100&webid=7382872326016435738",
            urlencode(kw)
        );
        let mut dy_cookie = format!("{};", search_cookie(st).await);
        if let Ok(hs) = st.http.head("https://live.douyin.com", &request_headers(&playback_cookie(st).await)).await {
            let extra = collect_cookies(&hs);
            if !extra.is_empty() { dy_cookie.push_str(&extra); }
        }
        let headers = h(&[
            ("authority", "www.douyin.com"),
            ("accept", "application/json, text/plain, */*"),
            ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8"),
            ("cookie", &dy_cookie),
            ("referer", &format!("https://www.douyin.com/search/{}?type=live", urlencode(kw))),
            ("user-agent", ident::DOUYIN_UA),
        ]);
        let (text, _) = st.http.get_text(&url, &headers).await?;
        if text.trim().is_empty() || text.trim() == "blocked" {
            return Err(anyhow!("抖音直播搜索被限制，请稍后再试"));
        }
        let v: Value = serde_json::from_str(&text)?;
        if v.i("status_code") == 2483 { return Err(anyhow!("抖音搜索需要登录：请在设置中填写抖音 Cookie")); }
        let mut items: Vec<SearchItem> = Vec::new();
        for row in v.a("data") {
            let raw = row.o("lives").s("rawdata");
            if raw.is_empty() { continue; }
            let live: Value = match serde_json::from_str(&raw) { Ok(x) => x, Err(_) => continue };
            let owner = live.o("owner");
            items.push(SearchItem {
                room_id: owner.s("web_rid"), title: live.s("title"),
                cover: arr_str(&live.o("cover").o("url_list"), 0),
                user_name: owner.s("nickname"), avatar: String::new(),
                online: live.o("stats").u("total_user"), live_status: true,
            });
        }
        if anchor {
            let k = kw.trim().to_lowercase();
            items.sort_by(|a, b| {
                b.user_name.to_lowercase().contains(&k).cmp(&a.user_name.to_lowercase().contains(&k))
                    .then(b.online.cmp(&a.online))
            });
        }
        Ok(SearchResult { has_more: items.len() >= 10, items })
    }
}

#[allow(dead_code)]
fn _unused() { let _ = cookie_value("", ""); }
