use anyhow::Result;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::auth::Auth;
use crate::config::Config;
use crate::danmaku::DanmakuHub;
use crate::http_client::Http;
use crate::models::RoomDetail;
use crate::sites::SiteManager;
use crate::sign::SignEngine;
use crate::store::{PanelAuthSettings, Store};

pub type Shared = Arc<AppState>;

pub struct AppState {
    pub cfg: Config,
    pub http: Http,
    pub sign: SignEngine,
    pub store: Arc<Store>,
    pub sites: SiteManager,
    pub cache: RoomCache,
    pub danmaku: Arc<DanmakuHub>,
    pub auth: Auth,
}

impl AppState {
    pub async fn build(cfg: Config) -> Result<Shared> {
        let http = Http::new(&cfg.upstream)?;
        let sign = SignEngine::new()?;
        let store = Store::open(&cfg.storage.data_dir).await?;
        let cache = RoomCache::new(cfg.server.cache_ttl);
        let auth = Auth::new(&cfg.auth);

        // 面板设置优先：若用户已在设置页管理过访问验证，重启后沿用；否则把配置文件的值回写，便于面板展示
        let mut saved = store.settings().await;
        match saved.panel_auth.clone() {
            Some(pa) => {
                let _ = auth.update(pa.enabled && !pa.password.trim().is_empty(), &pa.password);
            }
            None => {
                saved.panel_auth = Some(PanelAuthSettings {
                    enabled: cfg.auth.enabled,
                    password: cfg.auth.password.clone(),
                });
                store.update_settings(saved).await?;
            }
        }

        Ok(Arc::new(AppState {
            cfg,
            http,
            sign,
            store,
            sites: SiteManager::new(),
            cache,
            danmaku: Arc::new(DanmakuHub::new()),
            auth,
        }))
    }
}

/// 房间详情缓存：避免播放 / 弹幕接口重复请求平台
pub struct RoomCache {
    ttl: Duration,
    map: DashMap<String, (Instant, RoomDetail)>,
}

impl RoomCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self { ttl: Duration::from_secs(ttl_secs.max(1)), map: DashMap::new() }
    }
    pub fn key(platform: &str, room: &str) -> String { format!("{platform}:{room}") }
    pub fn get(&self, key: &str) -> Option<RoomDetail> {
        self.map.get(key).and_then(|v| {
            if v.0.elapsed() < self.ttl { Some(v.1.clone()) } else { None }
        })
    }
    pub fn put(&self, key: String, detail: RoomDetail) {
        self.map.insert(key, (Instant::now(), detail));
    }
    pub fn clear(&self) { self.map.clear(); }
}
