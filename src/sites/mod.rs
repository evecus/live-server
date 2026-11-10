use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use url::Url;

use crate::error::{ApiResult, AppError};
use crate::http_client::h;
use crate::models::{PlayQuality, RoomDetail, SearchResult};
use crate::state::Shared;
use crate::util::{first_url, parse_query, J};

pub mod bili;
pub mod douyin;
pub mod douyu;
pub mod huya;
pub mod kuaishou;
pub mod xhs;

#[async_trait]
pub trait Site: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn searchable(&self) -> bool { true }
    fn danmaku_supported(&self) -> bool { true }

    async fn room_detail(&self, st: &Shared, room_id: &str) -> anyhow::Result<RoomDetail>;
    async fn play(&self, st: &Shared, detail: &RoomDetail) -> anyhow::Result<Vec<PlayQuality>>;
    async fn live_status(&self, st: &Shared, room_id: &str) -> anyhow::Result<bool>;
    async fn search(&self, st: &Shared, kw: &str, page: u32, anchor: bool) -> anyhow::Result<SearchResult>;
}

pub struct SiteManager {
    pub sites: Vec<Arc<dyn Site>>,
}

impl SiteManager {
    pub fn new() -> Self {
        Self {
            sites: vec![
                Arc::new(bili::BiliSite),
                Arc::new(douyu::DouyuSite),
                Arc::new(huya::HuyaSite),
                Arc::new(douyin::DouyinSite),
                Arc::new(kuaishou::KuaishouSite),
                Arc::new(xhs::XhsSite),
            ],
        }
    }

    pub fn by_id(&self, id: &str) -> Option<Arc<dyn Site>> {
        self.sites.iter().find(|s| s.id() == id).cloned()
    }

    pub fn list(&self) -> Vec<Value> {
        self.sites.iter().map(|s| json!({
            "id": s.id(),
            "name": s.name(),
            "searchable": s.searchable(),
            "danmaku": s.danmaku_supported(),
        })).collect()
    }

    pub fn site_or_err(&self, id: &str) -> ApiResult<Arc<dyn Site>> {
        self.by_id(id).ok_or_else(|| AppError::bad(format!("不支持的平台：{id}")))
    }

    /// 解析分享文本/链接 -> (平台 id, 房间号)
    pub async fn parse_link(&self, st: &Shared, text: &str) -> Option<(String, String)> {
        if let Some(url) = first_url(text) {
            let mut current = url;
            for _ in 0..3 {
                if let Some(found) = match_room(&current) { return Some(found); }
                if !is_short_link(&current) { return None; }
                let expanded = st.http.resolve(&current, &h(&[("user-agent", crate::identity::DOUYIN_UA)])).await.ok()?;
                if expanded == current { return None; }
                current = expanded;
            }
            return match_room(&current);
        }
        // 纯文本：平台名 + 数字房间号
        for s in &self.sites {
            let short_name = s.name().trim_end_matches("直播");
            if text.to_lowercase().contains(s.id()) || text.contains(short_name) {
                let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() { return Some((s.id().into(), digits)); }
            }
        }
        None
    }
}

fn is_short_link(url: &str) -> bool {
    let host = Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_lowercase()));
    matches!(host.as_deref(), Some("b23.tv") | Some("v.douyin.com") | Some("v.kuaishou.com") | Some("xhslink.com"))
}

fn valid_room(s: &str, numeric_only: bool) -> bool {
    if s.is_empty() { return false; }
    if numeric_only { return s.chars().all(|c| c.is_ascii_digit()); }
    s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn room(platform: &str, id: Option<&str>, numeric: bool) -> Option<(String, String)> {
    let id = id?;
    if !valid_room(id, numeric) { return None; }
    Some((platform.into(), id.into()))
}

fn match_room(url: &str) -> Option<(String, String)> {
    let u = Url::parse(url).ok()?;
    let host = u.host_str()?.to_lowercase();
    let segs: Vec<&str> = u.path().split('/').filter(|s| !s.is_empty()).collect();
    let q = parse_query(u.query().unwrap_or(""));

    if host == "live.kuaishou.com" {
        if segs.len() == 2 && segs[0] == "u" { return room("kuaishou", Some(segs[1]), false); }
        return None;
    }
    if host == "m.chenzhongtech.com" || host.ends_with(".m.chenzhongtech.com") {
        if segs.len() == 3 && segs[0] == "fw" && segs[1] == "live" {
            return room("kuaishou", Some(segs[2]), false);
        }
        return None;
    }
    if host == "live.bilibili.com" {
        return room("bilibili", segs.first().copied(), false);
    }
    if host == "douyu.com" || host.ends_with(".douyu.com") {
        if segs.first() == Some(&"topic") { return room("douyu", q.get("rid").map(String::as_str), true); }
        return room("douyu", segs.first().copied(), false);
    }
    if host == "huya.com" || host.ends_with(".huya.com") {
        return room("huya", segs.first().copied(), false);
    }
    if host == "live.douyin.com" {
        return room("douyin", segs.first().copied(), false);
    }
    if host == "douyin.com" || host.ends_with(".douyin.com") {
        let rid = if segs.len() == 2 && segs[0] == "live" { Some(segs[1]) }
            else if segs.len() == 3 && segs[0] == "follow" && segs[1] == "live" { Some(segs[2]) }
            else { None };
        return room("douyin", rid, false);
    }
    if host.ends_with(".amemv.com") {
        let rid = q.get("room_id").map(String::as_str).or_else(|| {
            segs.iter().position(|s| *s == "reflow").and_then(|i| segs.get(i + 1).copied())
        });
        return room("douyin", rid, true);
    }
    if host == "xiaohongshu.com" || host.ends_with(".xiaohongshu.com") {
        if segs.first() == Some(&"livestream") && (2..=3).contains(&segs.len()) {
            return room("xiaohongshu", segs.last().copied(), true);
        }
        return None;
    }
    None
}

/// 常用：从 JSON 数组里取第 n 个字符串
pub fn arr_str(v: &Value, idx: usize) -> String {
    v.as_array().and_then(|a| a.get(idx)).map(|x| x.txt()).unwrap_or_default()
}
