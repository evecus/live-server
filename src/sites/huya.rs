use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::Rng;
use regex::Regex;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::Site;
use crate::http_client::h;
use crate::identity as ident;
use crate::models::{PlayQuality, RoomDetail, SearchItem, SearchResult};
use crate::state::Shared;
use crate::util::{md5_hex, percent_decode, random_from, to_https, urlencode, J};

pub struct HuyaSite;

fn request_headers() -> HeaderMap {
    h(&[
        ("Origin", "https://m.huya.com"), ("Referer", "https://m.huya.com"),
        ("User-Agent", ident::HUYA_HYSDK_UA),
    ])
}

/// 抽取页面里的 window.HNF_GLOBAL_INIT 对象
fn parse_hnf(html: &str) -> Option<(Value, i64, i64)> {
    let start = html.find("window.HNF_GLOBAL_INIT")?;
    let after = &html[start..];
    let eq = after.find('=')?;
    let mut raw = after[eq + 1..].trim_start().to_string();
    if let Some(p) = raw.find("</script>") { raw.truncate(p); }
    let raw = raw.trim().trim_end_matches(|c: char| c == ';' || c.is_whitespace()).to_string();
    let re = Regex::new(r"function.*?\(.*?\)\s*\{[\s\S]*?\}").ok()?;
    let cleaned = re.replace_all(&raw, "\"\"").to_string();
    let obj: Value = serde_json::from_str(&cleaned).ok()?;

    let number_after = |key: &str| -> Option<i64> {
        let re = Regex::new(&format!(r#"{}"?:([0-9]+)"#, regex::escape(key))).ok()?;
        re.captures(html).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse().ok())
    };
    let mut top = number_after("lChannelId").unwrap_or(0);
    let mut sub = number_after("lSubChannelId").unwrap_or(0);
    if top <= 0 { top = find_positive(&obj, &["lchannelid", "channelid"]); }
    if sub <= 0 { sub = find_positive(&obj, &["lsubchannelid", "subchannelid"]); }
    Some((obj, top, sub))
}

fn find_positive(v: &Value, keys: &[&str]) -> i64 {
    match v {
        Value::Object(map) => {
            for k in keys {
                if let Some(x) = map.get(*k).or_else(|| map.iter().find(|(kk, _)| kk.eq_ignore_ascii_case(k)).map(|(_, v)| v)) {
                    let n = x.num();
                    if n > 0 { return n; }
                }
            }
            for (_, x) in map { let n = find_positive(x, keys); if n > 0 { return n; } }
            0
        }
        Value::Array(arr) => {
            for x in arr { let n = find_positive(x, keys); if n > 0 { return n; } }
            0
        }
        _ => 0,
    }
}

async fn room_data(st: &Shared, room_id: &str) -> Result<(Value, i64, i64)> {
    let (html, _) = st.http.get_text(
        &format!("https://m.huya.com/{room_id}"),
        &h(&[("user-agent", ident::HUYA_UA)]),
    ).await?;
    parse_hnf(&html).ok_or_else(|| anyhow!("虎牙页面数据结构变更或访问受限"))
}

/// anticode 解析：base64 里的 '+' 不能被当作空格
fn parse_anti(q: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for part in q.split('&') {
        let mut it = part.splitn(2, '=');
        let k = it.next().unwrap_or("");
        let v = it.next().unwrap_or("");
        m.insert(k.to_string(), percent_decode(v));
    }
    m
}

fn rotl64(t: i64) -> i64 {
    let u = t as u64;
    let low = u & 0xFFFF_FFFF;
    let rot = ((low << 8) | (low >> 24)) & 0xFFFF_FFFF;
    ((u & 0xFFFF_FFFF_0000_0000) | rot) as i64
}

fn build_anti_code(stream: &str, presenter_uid: i64, anti_code: &str) -> String {
    let map = parse_anti(anti_code);
    if !map.contains_key("fm") { return anti_code.to_string(); }
    let ctype = map.get("ctype").cloned().unwrap_or_else(|| "huya_pc_exe".into());
    let platform_id: i64 = map.get("t").and_then(|s| s.parse().ok()).unwrap_or(0);
    let is_wap = platform_id == 103;
    let now_ms = crate::util::unix_ms() as i64;
    let seq_id = presenter_uid + now_ms;
    let secret_hash = md5_hex(&format!("{seq_id}|{ctype}|{platform_id}"));
    let convert_uid = rotl64(presenter_uid);
    let calc_uid = if is_wap { presenter_uid } else { convert_uid };

    let fm = percent_decode(map.get("fm").map(String::as_str).unwrap_or(""));
    let fm_decoded = STANDARD.decode(fm.as_bytes()).unwrap_or_default();
    let secret_prefix = String::from_utf8_lossy(&fm_decoded).split('_').next().unwrap_or("").to_string();
    let ws_time = map.get("wsTime").cloned().unwrap_or_default();
    let ws_secret = md5_hex(&format!("{secret_prefix}_{calc_uid}_{stream}_{secret_hash}_{ws_time}"));

    let mut rng = rand::thread_rng();
    let wst = i64::from_str_radix(ws_time.trim(), 16).unwrap_or(0) as f64;
    let ct = ((wst + rng.gen::<f64>()) * 1000.0) as u64;
    let uuid = (((ct as f64 % 1e10) + rng.gen::<f64>()) * 1e3 % 0xFFFF_FFFFu64 as f64) as u64;

    let mut parts = vec![
        format!("wsSecret={ws_secret}"),
        format!("wsTime={ws_time}"),
        format!("seqid={seq_id}"),
        format!("ctype={ctype}"),
        "ver=1".to_string(),
        format!("fs={}", map.get("fs").cloned().unwrap_or_default()),
        format!("fm={}", urlencode(map.get("fm").map(String::as_str).unwrap_or(""))),
        format!("t={platform_id}"),
    ];
    if is_wap {
        parts.push(format!("uid={presenter_uid}"));
        parts.push(format!("uuid={uuid}"));
    } else {
        parts.push(format!("u={convert_uid}"));
    }
    parts.join("&")
}

fn ratio_param(bit_rate: i64) -> String {
    if (1..=4000).contains(&bit_rate) { format!("&ratio={bit_rate}") } else { String::new() }
}

#[async_trait]
impl Site for HuyaSite {
    fn id(&self) -> &'static str { "huya" }
    fn name(&self) -> &'static str { "虎牙直播" }

    async fn room_detail(&self, st: &Shared, room_id: &str) -> Result<RoomDetail> {
        let (obj, top_sid, sub_sid) = room_data(st, room_id).await?;
        let room_info = obj.o("roomInfo");
        let live = room_info.o("tLiveInfo");
        let profile = room_info.o("tProfileInfo");
        if live.is_null() || profile.is_null() { return Err(anyhow!("虎牙房间数据结构变更")); }

        let mut lines = Vec::new();
        let stream_values = live.o("tLiveStreamInfo").o("vStreamInfo").a("value");
        for item in &stream_values {
            let flv = item.s("sFlvUrl");
            let hls = item.s("sHlsUrl");
            let name = item.s("sStreamName");
            if (flv.is_empty() && hls.is_empty()) || name.is_empty() { continue; }
            lines.push(json!({
                "line": to_https(&flv),
                "hlsUrl": to_https(&hls),
                "flvAntiCode": item.s("sFlvAntiCode"),
                "hlsAntiCode": item.s("sHlsAntiCode"),
                "streamName": name,
                "presenterUid": if top_sid > 0 { top_sid } else { sub_sid },
            }));
        }
        let mut rates = Vec::new();
        for item in live.o("tLiveStreamInfo").o("vBitRateInfo").a("value") {
            let name = item.s("sDisplayName");
            if name.contains("HDR") { continue; }
            rates.push(json!({ "bitRate": item.i("iBitRate"), "name": name }));
        }

        let mut d = RoomDetail::new(self.id(), &live.s("lProfileRoom"));
        if d.room_id.is_empty() { d.room_id = room_id.into(); }
        d.title = { let t = live.s("sIntroduction"); if t.is_empty() { live.s("sRoomName") } else { t } };
        d.cover = live.s("sScreenshot");
        d.user_name = profile.s("sNick");
        d.user_avatar = profile.s("sAvatar180");
        d.online = live.u("lTotalCount");
        d.status = room_info.i("eLiveStatus") == 2;
        d.url = format!("https://www.huya.com/{room_id}");
        d.introduction = live.s("sIntroduction");
        d.danmaku_args = json!({ "ayyuid": live.i("lYyid"), "topSid": top_sid, "subSid": sub_sid });
        d.play_data = json!({ "lines": lines, "bitRates": rates });
        Ok(d)
    }

    async fn play(&self, _st: &Shared, detail: &RoomDetail) -> Result<Vec<PlayQuality>> {
        let lines = detail.play_data.a("lines");
        let mut rates: Vec<(String, i64)> = detail.play_data.a("bitRates").iter()
            .map(|r| (r.s("name"), r.i("bitRate"))).collect();
        if rates.is_empty() {
            rates = vec![("原画".into(), 0), ("高清".into(), 2000)];
        }
        let mut out = Vec::new();
        for (name, rate) in rates {
            let mut hls_urls = Vec::new();
            let mut flv_urls = Vec::new();
            for line in &lines {
                let stream = line.s("streamName");
                let uid = line.i("presenterUid");
                let hls_base = line.s("hlsUrl");
                if !hls_base.is_empty() {
                    let anti = { let a = line.s("hlsAntiCode"); if a.is_empty() { line.s("flvAntiCode") } else { a } };
                    let built = build_anti_code(&stream, uid, &anti);
                    hls_urls.push(format!("{hls_base}/{stream}.m3u8?{built}&codec=264{}", ratio_param(rate)));
                }
                let flv_base = line.s("line");
                if !flv_base.is_empty() {
                    let built = build_anti_code(&stream, uid, &line.s("flvAntiCode"));
                    flv_urls.push(format!("{flv_base}/{stream}.flv?{built}&codec=264{}", ratio_param(rate)));
                }
            }
            hls_urls.extend(flv_urls);
            if !hls_urls.is_empty() {
                out.push(PlayQuality::new(name, rate.to_string(), hls_urls)
                    .with_headers(&[("user-agent", ident::HUYA_MOBILE_HYSDK_UA)]));
            }
        }
        if out.is_empty() { return Err(anyhow!("虎牙未获取到播放地址（可能未开播或被风控）")); }
        Ok(out)
    }

    async fn live_status(&self, st: &Shared, room_id: &str) -> Result<bool> {
        let (obj, _, _) = room_data(st, room_id).await?;
        Ok(obj.o("roomInfo").i("eLiveStatus") == 2)
    }

    async fn search(&self, st: &Shared, kw: &str, page: u32, anchor: bool) -> Result<SearchResult> {
        let v = st.http.get_json(&format!(
            "https://search.cdn.huya.com/?m=Search&do=getSearchContent&q={}&uid=0&v={}&typ=-5&livestate=0&rows={}&start=0",
            urlencode(kw), if anchor { 1 } else { 4 }, page * 20
        ), &request_headers()).await?;
        let section = v.o("response").o(if anchor { "1" } else { "3" });
        let docs = section.a("docs");
        let skip = ((page - 1) * 20) as usize;
        let items: Vec<SearchItem> = docs.iter().skip(skip).map(|it| {
            if anchor {
                SearchItem {
                    room_id: it.s("room_id"), title: String::new(), cover: it.s("game_avatarUrl180"),
                    user_name: it.s("game_nick"), avatar: it.s("game_avatarUrl180"),
                    online: 0, live_status: it.b("gameLiveOn"),
                }
            } else {
                let mut cover = it.s("game_screenshot");
                if !cover.contains('?') { cover.push_str("?x-oss-process=style/w338_h190&"); }
                let mut title = it.s("game_introduction");
                if title.is_empty() { title = it.s("game_roomName"); }
                SearchItem {
                    room_id: it.s("room_id"), title, cover, user_name: it.s("game_nick"),
                    avatar: String::new(), online: it.u("game_total_count"), live_status: true,
                }
            }
        }).collect();
        let found = section.i("numFound");
        Ok(SearchResult { has_more: found > (page * 20) as i64, items })
    }
}

#[allow(dead_code)]
fn _unused() { let _ = random_from(b"x", 1); }
