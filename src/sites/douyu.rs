use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use std::sync::OnceLock;

use super::Site;
use crate::http_client::{h, with};
use crate::identity as ident;
use crate::models::{PlayQuality, RoomDetail, SearchItem, SearchResult};
use crate::state::Shared;
use crate::util::{html_unescape, parse_count, random_hex, urlencode, J};

pub struct DouyuSite;

static DID: OnceLock<String> = OnceLock::new();
fn did() -> &'static str { DID.get_or_init(|| random_hex(32)) }

fn request_headers() -> HeaderMap {
    h(&[
        ("referer", "https://www.douyu.com/"),
        ("user-agent", ident::DOUYU_UA),
        ("Cookie", &format!("dy_did={}; acf_did={}", did(), did())),
    ])
}
fn room_headers(room_id: &str) -> HeaderMap {
    with(request_headers(), &[("referer", &format!("https://www.douyu.com/{room_id}"))])
}
fn search_headers() -> HeaderMap {
    h(&[("user-agent", ident::DOUYU_SEARCH_UA), ("referer", "https://www.douyu.com/search/")])
}

fn err_code(v: &Value) -> i64 { v.get("error").map(|x| x.num()).unwrap_or(-1) }

async fn room_info(st: &Shared, room_id: &str) -> Result<Value> {
    let text = st.http.get_text(
        &format!("https://www.douyu.com/betard/{room_id}"),
        &h(&[("referer", &format!("https://www.douyu.com/{room_id}")), ("user-agent", ident::DOUYU_ROOM_UA)]),
    ).await?.0;
    let v: Value = serde_json::from_str(&text)?;
    let room = v.o("room");
    if room.get("room_id").is_none() { return Err(anyhow!("斗鱼房间数据结构变更或房间不存在")); }
    Ok(room)
}

async fn fresh_play_args(st: &Shared, room_id: &str) -> Result<String> {
    let url = format!("https://www.douyu.com/swf_api/homeH5Enc?rids={room_id}");
    let (text, _) = st.http.get_text(&url, &room_headers(room_id)).await?;
    let v: Value = serde_json::from_str(&text)?;
    let script = v.o("data").s(&format!("room{room_id}"));
    if script.is_empty() { return Err(anyhow!("斗鱼加密脚本为空")); }
    let signed = st.sign.douyu_sign(&script, room_id, did()).await?;
    if signed.is_empty() { return Err(anyhow!("斗鱼签名失败")); }
    Ok(signed)
}

async fn post_h5_play(st: &Shared, room_id: &str, body: &str) -> Result<Value> {
    let url = format!("https://www.douyu.com/lapi/live/getH5Play/{room_id}");
    st.http.post_form(&url, body, &room_headers(room_id)).await
}

/// 签名偶发失效时自动重签再试一次
async fn post_fresh(st: &Shared, room_id: &str, suffix: &str) -> Result<Value> {
    let first = post_h5_play(st, room_id, &format!("{}{suffix}", fresh_play_args(st, room_id).await?)).await?;
    if err_code(&first) == 0 { return Ok(first); }
    let second = post_h5_play(st, room_id, &format!("{}{suffix}", fresh_play_args(st, room_id).await?)).await?;
    Ok(second)
}

#[async_trait]
impl Site for DouyuSite {
    fn id(&self) -> &'static str { "douyu" }
    fn name(&self) -> &'static str { "斗鱼直播" }

    async fn room_detail(&self, st: &Shared, room_id: &str) -> Result<RoomDetail> {
        let info = room_info(st, room_id).await?;
        let rid = info.s("room_id");
        let rid = if rid.is_empty() { room_id.to_string() } else { rid };
        let mut d = RoomDetail::new(self.id(), &rid);
        d.title = info.s("room_name");
        d.cover = info.s("room_pic");
        d.user_name = info.s("owner_name");
        d.user_avatar = info.s("owner_avatar");
        d.online = info.o("room_biz_all").u("hot");
        d.status = info.i("show_status") == 1 && info.i("videoLoop") != 1;
        d.url = format!("https://www.douyu.com/{room_id}");
        d.introduction = info.s("show_details");
        d.danmaku_args = json!(rid);
        Ok(d)
    }

    async fn play(&self, st: &Shared, detail: &RoomDetail) -> Result<Vec<PlayQuality>> {
        let rid = &detail.room_id;
        let base = post_fresh(st, rid, "&cdn=&rate=-1&ver=Douyu_223061205&iar=1&ive=1&hevc=0&fa=0").await?;
        if err_code(&base) != 0 {
            return Err(anyhow!("斗鱼播放质量请求失败：{}", base.s("msg")));
        }
        let payload = base.o("data");
        let mut cdns: Vec<String> = payload.a("cdnsWithName").iter().map(|c| c.s("cdn")).filter(|s| !s.is_empty()).collect();
        cdns.sort_by_key(|c| c.starts_with("scdn"));
        let rates: Vec<(String, i64)> = payload.a("multirates").iter()
            .map(|r| (r.s("name"), r.i("rate"))).collect();
        if rates.is_empty() { return Err(anyhow!("斗鱼返回了无效的播放质量")); }

        let mut out = Vec::new();
        for (name, rate) in rates {
            let mut urls = Vec::new();
            for _attempt in 0..2 {
                if let Ok(args) = fresh_play_args(st, rid).await {
                    for cdn in &cdns {
                        let body = format!("{args}&cdn={cdn}&rate={rate}");
                        if let Ok(res) = post_h5_play(st, rid, &body).await {
                            if err_code(&res) != 0 { continue; }
                            let data = res.o("data");
                            let rtmp_url = data.s("rtmp_url");
                            let rtmp_live = html_unescape(&data.s("rtmp_live"));
                            if rtmp_url.is_empty() || rtmp_live.is_empty() { continue; }
                            let mut url = format!("{rtmp_url}/{rtmp_live}");
                            if url.contains("expire=300") && url.contains("fcdn=ws") { url.push_str("&expire=0"); }
                            urls.push(url);
                        }
                    }
                }
                if !urls.is_empty() { break; }
            }
            if !urls.is_empty() { out.push(PlayQuality::new(name, rate.to_string(), urls)); }
        }
        if out.is_empty() { return Err(anyhow!("斗鱼未获取到播放地址")); }
        Ok(out)
    }

    async fn live_status(&self, st: &Shared, room_id: &str) -> Result<bool> {
        let info = room_info(st, room_id).await?;
        Ok(info.i("show_status") == 1 && info.i("videoLoop") != 1)
    }

    async fn search(&self, st: &Shared, kw: &str, page: u32, anchor: bool) -> Result<SearchResult> {
        let k = urlencode(kw);
        if !anchor {
            let v = st.http.get_json(
                &format!("https://www.douyu.com/japi/search/api/searchShow?kw={k}&page={page}&pageSize=20"),
                &search_headers(),
            ).await?;
            if err_code(&v) != 0 { return Err(anyhow!("{}", v.s("msg"))); }
            let items: Vec<SearchItem> = v.o("data").a("relateShow").iter().map(|it| SearchItem {
                room_id: it.s("rid"), title: it.s("roomName"), cover: it.s("roomSrc"),
                user_name: it.s("nickName"), avatar: String::new(),
                online: parse_count(&it.s("hot")), live_status: true,
            }).collect();
            Ok(SearchResult { has_more: !items.is_empty(), items })
        } else {
            let v = st.http.get_json(
                &format!("https://www.douyu.com/japi/search/api/searchUser?kw={k}&page={page}&pageSize=20&filterType=1"),
                &search_headers(),
            ).await?;
            let items: Vec<SearchItem> = v.o("data").a("relateUser").iter().filter_map(|it| {
                let a = it.o("anchorInfo");
                if a.is_null() { return None; }
                Some(SearchItem {
                    room_id: a.s("rid"), title: String::new(), cover: a.s("avatar"),
                    user_name: a.s("nickName"), avatar: a.s("avatar"), online: 0,
                    live_status: a.i("isLive") == 1 && a.i("roomType") == 0,
                })
            }).collect();
            Ok(SearchResult { has_more: !items.is_empty(), items })
        }
    }
}
