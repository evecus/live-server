use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::util::{unix_secs, J};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Follow {
    pub id: String,
    pub platform: String,
    pub room_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub user_name: String,
    #[serde(default)]
    pub add_time: u64,
    #[serde(default)]
    pub top: bool,
    #[serde(default)]
    pub live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountCookies {
    #[serde(default)]
    pub douyin: String,
    #[serde(default)]
    pub kuaishou: String,
}

/// 面板访问验证：可由设置页运行时开启 / 关闭，改动立即生效并持久化
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelAuthSettings {
    pub enabled: bool,
    pub password: String,
}

/// 弹幕显示设置（前端解释并渲染，服务端只负责持久化，多设备共享）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DanmakuSettings {
    pub enabled: bool,
    /// 字号 px
    pub font_size: u32,
    /// 显示区域占播放器高度的百分比
    pub area: u32,
    /// 行间距 px
    pub line_gap: u32,
    /// 同一轨道相邻弹幕的最小水平间距 px
    pub item_gap: u32,
    /// 弹幕横穿屏幕所需秒数（越小越快）
    pub duration: f32,
    /// platform | custom | white | rainbow
    pub color_mode: String,
    /// colorMode=custom 时使用的颜色
    pub color: String,
    /// 文字不透明度 0.2 ~ 1
    pub opacity: f32,
    pub bold: bool,
    /// 描边 / 阴影，提升在浅色画面上的可读性
    pub stroke: bool,
    /// 是否显示用户名前缀
    pub show_user: bool,
}

impl Default for DanmakuSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            font_size: 22,
            area: 60,
            line_gap: 10,
            item_gap: 64,
            duration: 9.0,
            color_mode: "platform".into(),
            color: "#ffffff".into(),
            opacity: 1.0,
            bold: true,
            stroke: true,
            show_user: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub cookies: AccountCookies,
    /// None 表示尚未由面板管理，启动时以 config.toml / CLI 为准
    pub panel_auth: Option<PanelAuthSettings>,
    pub danmaku: DanmakuSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Data {
    #[serde(default)]
    follows: Vec<Follow>,
    #[serde(default)]
    settings: Settings,
}

pub struct Store {
    path: PathBuf,
    inner: RwLock<Data>,
}

fn follow_id(platform: &str, room_id: &str) -> String { format!("{platform}:{room_id}") }

impl Store {
    pub async fn open(dir: &Path) -> Result<Arc<Store>> {
        tokio::fs::create_dir_all(dir).await.context("创建数据目录失败")?;
        let path = dir.join("silm.json");
        let data: Data = if path.exists() {
            let text = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            serde_json::from_str(&text).unwrap_or_default()
        } else { Data::default() };
        Ok(Arc::new(Store { path, inner: RwLock::new(data) }))
    }

    async fn persist(&self, data: &Data) -> Result<()> {
        let text = serde_json::to_string_pretty(data)?;
        let tmp = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp, text).await?;
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(())
    }

    pub async fn follows(&self) -> Vec<Follow> {
        let d = self.inner.read().await;
        let mut list = d.follows.clone();
        list.sort_by(|a, b| b.top.cmp(&a.top).then(b.add_time.cmp(&a.add_time)));
        list
    }

    pub async fn add(&self, platform: &str, room_id: &str, title: &str, cover: &str, user_name: &str) -> Result<Follow> {
        let mut d = self.inner.write().await;
        let id = follow_id(platform, room_id);
        if let Some(old) = d.follows.iter_mut().find(|f| f.id == id) {
            if !title.is_empty() { old.title = title.into(); }
            if !cover.is_empty() { old.cover = cover.into(); }
            if !user_name.is_empty() { old.user_name = user_name.into(); }
            let out = old.clone();
            self.persist(&d).await?;
            return Ok(out);
        }
        let f = Follow {
            id, platform: platform.into(), room_id: room_id.into(),
            title: title.into(), cover: cover.into(), user_name: user_name.into(),
            add_time: unix_secs() as u64, top: false, live: false,
        };
        d.follows.push(f.clone());
        self.persist(&d).await?;
        Ok(f)
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        let mut d = self.inner.write().await;
        let before = d.follows.len();
        d.follows.retain(|f| f.id != id);
        if d.follows.len() == before { anyhow::bail!("关注不存在：{id}"); }
        self.persist(&d).await?;
        Ok(())
    }

    pub async fn set_top(&self, id: &str, top: bool) -> Result<()> {
        let mut d = self.inner.write().await;
        let f = d.follows.iter_mut().find(|f| f.id == id).ok_or_else(|| anyhow::anyhow!("关注不存在：{id}"))?;
        f.top = top;
        self.persist(&d).await?;
        Ok(())
    }

    pub async fn set_live(&self, id: &str, live: bool) {
        let mut d = self.inner.write().await;
        if let Some(f) = d.follows.iter_mut().find(|f| f.id == id) {
            if f.live != live {
                f.live = live;
                let _ = self.persist(&d).await;
            }
        }
    }

    pub async fn settings(&self) -> Settings { self.inner.read().await.settings.clone() }

    pub async fn update_settings(&self, s: Settings) -> Result<()> {
        let mut d = self.inner.write().await;
        d.settings = s;
        self.persist(&d).await?;
        Ok(())
    }

    pub async fn export_json(&self) -> Value {
        let d = self.inner.read().await;
        json!({
            "app": "silm-live",
            "version": 1,
            "exportedAt": unix_secs(),
            "follows": d.follows,
        })
    }

    /// 兼容数组 / {follows:[...]} / 安卓端字段名
    pub async fn import_json(&self, v: Value) -> Result<usize> {
        let arr: Vec<Value> = if v.is_array() {
            serde_json::from_value(v)?
        } else {
            let cand = v.get("follows").or_else(|| v.get("data").and_then(|d| d.get("follows")));
            match cand {
                Some(Value::Array(a)) => a.clone(),
                _ => anyhow::bail!("导入格式无法识别：需要关注列表数组"),
            }
        };
        let mut d = self.inner.write().await;
        let mut count = 0;
        for item in arr {
            let platform = first_str(&item, &["platform", "site", "siteId"]);
            let room_id = first_str(&item, &["roomId", "room_id", "roomid", "rid"]);
            if platform.is_empty() || room_id.is_empty() { continue; }
            let id = follow_id(&platform, &room_id);
            if d.follows.iter().any(|f| f.id == id) { continue; }
            d.follows.push(Follow {
                id,
                platform,
                room_id,
                title: first_str(&item, &["title", "roomName", "room_name"]),
                cover: first_str(&item, &["cover", "roomPic", "avatar"]),
                user_name: first_str(&item, &["userName", "user_name", "nickName", "ownerName"]),
                add_time: unix_secs() as u64,
                top: item.b("top"),
                live: false,
            });
            count += 1;
        }
        self.persist(&d).await?;
        Ok(count)
    }
}

fn first_str(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        let s = v.s(k);
        if !s.is_empty() { return s; }
    }
    String::new()
}
