use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use super::Site;
use crate::http_client::h;
use crate::identity as ident;
use crate::models::{PlayQuality, RoomDetail, SearchItem, SearchResult};
use crate::state::Shared;
use super::arr_str;
use crate::util::{json_quote, md5_hex, query_string, strip_em_tags, urlencode, unix_secs, J};

pub struct BiliSite;

#[derive(Default, Clone)]
struct Keys { buvid3: String, buvid4: String, img_key: String, sub_key: String }
static KEYS: OnceLock<RwLock<Keys>> = OnceLock::new();
fn keys() -> &'static RwLock<Keys> { KEYS.get_or_init(|| RwLock::new(Keys::default())) }

const MIXIN_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49,
    33, 9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61,
    26, 17, 0, 1, 60, 51, 30, 4, 22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36,
    20, 34, 44, 52,
];

fn default_headers() -> HeaderMap {
    h(&[("user-agent", ident::BILI_UA), ("referer", "https://live.bilibili.com/")])
}

async fn ensure_buvid(st: &Shared) -> Result<()> {
    if !keys().read().unwrap().buvid3.is_empty() { return Ok(()); }
    let v = st.http.get_json("https://api.bilibili.com/x/frontend/finger/spi", &default_headers()).await?;
    let b3 = v.o("data").s("b_3");
    let b4 = v.o("data").s("b_4");
    let mut k = keys().write().unwrap();
    k.buvid3 = b3; k.buvid4 = b4;
    Ok(())
}

async fn headers(st: &Shared) -> Result<HeaderMap> {
    ensure_buvid(st).await.ok();
    let k = keys().read().unwrap().clone();
    Ok(h(&[
        ("user-agent", ident::BILI_UA),
        ("referer", "https://live.bilibili.com/"),
        ("cookie", &format!("buvid3={};buvid4={};", k.buvid3, k.buvid4)),
    ]))
}

async fn ensure_wbi(st: &Shared) -> Result<(String, String)> {
    let cached = { let k = keys().read().unwrap(); (k.img_key.clone(), k.sub_key.clone()) };
    if !cached.0.is_empty() && !cached.1.is_empty() { return Ok(cached); }
    let v = st.http.get_json("https://api.bilibili.com/x/web-interface/nav", &default_headers()).await?;
    let wbi = v.o("data").o("wbi_img");
    let file_name = |u: String| -> String {
        let p = u.rsplit('/').next().unwrap_or("");
        p.split('.').next().unwrap_or("").to_string()
    };
    let img = file_name(wbi.s("img_url"));
    let sub = file_name(wbi.s("sub_url"));
    { let mut k = keys().write().unwrap(); k.img_key = img.clone(); k.sub_key = sub.clone(); }
    Ok((img, sub))
}

/// WBI 签名，返回可直接拼到 URL 后的完整参数表
async fn wbi_sign(st: &Shared, pairs: &[(&str, &str)]) -> Result<BTreeMap<String, String>> {
    let (img, sub) = ensure_wbi(st).await.unwrap_or_default();
    let raw = format!("{img}{sub}");
    let chars: Vec<char> = raw.chars().collect();
    let mixin: String = MIXIN_TAB.iter().filter_map(|&i| chars.get(i).copied()).take(32).collect();

    let mut params: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in pairs { params.insert((*k).into(), (*v).into()); }
    params.insert("wts".into(), unix_secs().to_string());
    let query = params.iter().map(|(k, v)| {
        let cleaned: String = v.chars().filter(|c| !"!'()*".contains(*c)).collect();
        format!("{k}={}", urlencode(&cleaned))
    }).collect::<Vec<_>>().join("&");
    params.insert("w_rid".into(), md5_hex(&format!("{query}{mixin}")));
    Ok(params)
}

async fn wbi_get(st: &Shared, base: &str, pairs: &[(&str, &str)]) -> Result<Value> {
    let params = wbi_sign(st, pairs).await?;
    let url = format!("{base}?{}", query_string(&params));
    st.http.get_json(&url, &headers(st).await?).await
}

async fn room_play_info(st: &Shared, pairs: &[(&str, &str)]) -> Result<Value> {
    let mut params: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in pairs { params.insert((*k).into(), (*v).into()); }
    let url = format!("https://api.live.bilibili.com/xlive/web-room/v2/index/getRoomPlayInfo?{}", query_string(&params));
    st.http.get_json(&url, &headers(st).await?).await
}

fn read_play_url(v: &Value) -> Result<Value> {
    let p = v.o("data").o("playurl_info").o("playurl");
    if p.is_null() { return Err(anyhow!("B站播放信息响应异常")); }
    Ok(p)
}

#[async_trait]
impl Site for BiliSite {
    fn id(&self) -> &'static str { "bilibili" }
    fn name(&self) -> &'static str { "哔哩哔哩直播" }

    async fn room_detail(&self, st: &Shared, room_id: &str) -> Result<RoomDetail> {
        let info = wbi_get(st, "https://api.live.bilibili.com/xlive/web-room/v1/index/getInfoByRoom", &[("room_id", room_id)]).await?
            .o("data");
        let ri = info.o("room_info");
        let real_id = ri.s("room_id");
        let rid = if real_id.is_empty() { room_id.to_string() } else { real_id };

        let mut token = String::new();
        let mut server_host = "broadcastlv.chat.bilibili.com".to_string();
        if let Ok(dm) = wbi_get(st, "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo", &[("id", rid.as_str())]).await {
            let d = dm.o("data");
            token = d.s("token");
            if let Some(host) = d.a("host_list").first() {
                let hh = host.s("host");
                if !hh.is_empty() { server_host = hh; }
            }
        }
        let base = info.o("anchor_info").o("base_info");
        let buvid = keys().read().unwrap().buvid3.clone();

        let mut d = RoomDetail::new(self.id(), &rid);
        d.title = ri.s("title");
        d.cover = ri.s("cover");
        d.user_name = base.s("uname");
        d.user_avatar = format!("{}@100w.jpg", base.s("face"));
        d.online = ri.u("online");
        d.status = ri.i("live_status") == 1;
        d.url = format!("https://live.bilibili.com/{room_id}");
        d.introduction = ri.s("description");
        d.danmaku_args = json!({
            "roomId": rid.parse::<u64>().unwrap_or(0),
            "uid": 0,
            "token": token,
            "serverHost": server_host,
            "buvid": buvid,
        });
        Ok(d)
    }

    async fn play(&self, st: &Shared, detail: &RoomDetail) -> Result<Vec<PlayQuality>> {
        let p = read_play_url(&room_play_info(st, &[
            ("room_id", detail.room_id.as_str()), ("protocol", "0,1"), ("format", "0,1,2"),
            ("codec", "0,1"), ("platform", "web"),
        ]).await?)?;
        let mut names: BTreeMap<i64, String> = BTreeMap::new();
        for q in p.a("g_qn_desc") { names.insert(q.i("qn"), q.s("desc")); }
        let mut accepts: Vec<i64> = Vec::new();
        if let Some(first) = p.a("stream").first() {
            if let Some(fmt) = first.a("format").first() {
                if let Some(codec) = fmt.a("codec").first() {
                    accepts = codec.a("accept_qn").iter().map(|v| v.num()).collect();
                }
            }
        }
        if accepts.is_empty() { return Err(anyhow!("B站暂时无法获取播放清晰度")); }

        let mut out = Vec::new();
        for qn in accepts {
            let qn_s = qn.to_string();
            let pp = read_play_url(&room_play_info(st, &[
                ("room_id", detail.room_id.as_str()), ("protocol", "0,1"), ("format", "0,2"),
                ("codec", "0"), ("platform", "web"), ("qn", qn_s.as_str()),
            ]).await?)?;
            let mut urls = Vec::new();
            for stream in pp.a("stream") {
                for fmt in stream.a("format") {
                    for codec in fmt.a("codec") {
                        let base = codec.s("base_url");
                        for info in codec.a("url_info") {
                            urls.push(format!("{}{base}{}", info.s("host"), info.s("extra")));
                        }
                    }
                }
            }
            urls.sort_by_key(|u| (!u.contains("m3u8"), u.contains("mcdn")));
            out.push(PlayQuality::new(names.get(&qn).cloned().unwrap_or_else(|| "未知清晰度".into()), qn.to_string(), urls)
                .with_headers(&[("referer", "https://live.bilibili.com"), ("user-agent", ident::BILI_ROOM_UA)]));
        }
        Ok(out)
    }

    async fn live_status(&self, st: &Shared, room_id: &str) -> Result<bool> {
        let v = st.http.get_json(
            &format!("https://api.live.bilibili.com/room/v1/Room/get_info?room_id={room_id}"),
            &default_headers(),
        ).await?;
        Ok(v.o("data").i("live_status") == 1)
    }

    async fn search(&self, st: &Shared, kw: &str, page: u32, anchor: bool) -> Result<SearchResult> {
        let kind = if anchor { "live_user" } else { "live" };
        let url = format!(
            "https://api.bilibili.com/x/web-interface/search/type?context=&search_type={kind}&cover_type=user_cover&order=&keyword={}&category_id=&__refresh__=&_extra=&highlight=0&single_column=0&page={page}",
            urlencode(kw)
        );
        let v = st.http.get_json(&url, &headers(st).await?).await?;
        let items: Vec<SearchItem> = if anchor {
            v.o("data").a("result").iter().map(|it| SearchItem {
                room_id: it.s("roomid"), title: String::new(),
                cover: format!("https:{}@400w.jpg", it.s("uface")),
                user_name: strip_em_tags(&it.s("uname")), avatar: String::new(),
                online: 0, live_status: it.b("is_live"),
            }).collect()
        } else {
            v.o("data").o("result").a("live_room").iter().map(|it| SearchItem {
                room_id: it.s("roomid"), title: strip_em_tags(&it.s("title")),
                cover: format!("https:{}@400w.jpg", it.s("cover")),
                user_name: it.s("uname"), avatar: String::new(),
                online: it.u("online"), live_status: true,
            }).collect()
        };
        Ok(SearchResult { has_more: items.len() >= 40, items })
    }
}

#[allow(dead_code)]
fn _unused() { let _ = json_quote(""); let _ = arr_str(&Value::Null, 0); }
