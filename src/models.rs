use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub room_id: String,
    pub title: String,
    pub cover: String,
    pub user_name: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub online: u64,
    #[serde(default)]
    pub live_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub has_more: bool,
    pub items: Vec<SearchItem>,
}

impl SearchResult {
    pub fn empty() -> Self { Self { has_more: false, items: vec![] } }
}

/// 一个清晰度档位，含全部线路的原始 URL
#[derive(Debug, Clone, Default)]
pub struct PlayQuality {
    pub name: String,
    pub key: String,
    pub urls: Vec<String>,
    pub headers: BTreeMap<String, String>,
}

impl PlayQuality {
    pub fn new(name: impl Into<String>, key: impl Into<String>, urls: Vec<String>) -> Self {
        Self { name: name.into(), key: key.into(), urls, headers: BTreeMap::new() }
    }
    pub fn with_headers(mut self, h: &[(&str, &str)]) -> Self {
        for (k, v) in h { self.headers.insert((*k).into(), (*v).into()); }
        self
    }
}

/// 房间详情。`danmaku_args` / `play_data` 只在服务端缓存中使用，不下发前端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomDetail {
    pub platform: String,
    pub room_id: String,
    pub title: String,
    pub cover: String,
    pub user_name: String,
    pub user_avatar: String,
    pub online: u64,
    pub status: bool,
    pub url: String,
    pub introduction: String,
    #[serde(skip)]
    pub danmaku_args: Value,
    #[serde(skip)]
    pub play_data: Value,
}

impl RoomDetail {
    pub fn new(platform: &str, room_id: &str) -> Self {
        Self {
            platform: platform.into(), room_id: room_id.into(),
            title: String::new(), cover: String::new(), user_name: String::new(),
            user_avatar: String::new(), online: 0, status: false,
            url: String::new(), introduction: String::new(),
            danmaku_args: Value::Null, play_data: Value::Null,
        }
    }
    pub fn offline(platform: &str, room_id: &str, title: &str) -> Self {
        let mut d = Self::new(platform, room_id);
        d.title = title.into();
        d
    }
}

/// 推给前端的统一弹幕消息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DanmakuMessage {
    /// chat | online | error | info
    #[serde(rename = "type")]
    pub kind: String,
    pub user_name: String,
    pub message: String,
    pub color: u32,
    pub online: u64,
}

impl DanmakuMessage {
    pub fn chat(user: impl Into<String>, msg: impl Into<String>, color: u32) -> Self {
        Self { kind: "chat".into(), user_name: user.into(), message: msg.into(), color, online: 0 }
    }
    pub fn online_msg(n: u64) -> Self {
        Self { kind: "online".into(), user_name: String::new(), message: String::new(), color: 0, online: n }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Self { kind: "error".into(), user_name: String::new(), message: msg.into(), color: 0, online: 0 }
    }
    pub fn info(msg: impl Into<String>) -> Self {
        Self { kind: "info".into(), user_name: String::new(), message: msg.into(), color: 0, online: 0 }
    }
}
