use anyhow::{Context as _, Result};
use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "silm-live-server", about = "多平台直播聚合服务端")]
pub struct Cli {
    /// 监听地址
    #[arg(long, env = "SILM_LISTEN")]
    pub listen: Option<String>,
    /// 配置文件路径（默认 ./config.toml）
    #[arg(long, short = 'c', env = "SILM_CONFIG")]
    pub config: Option<PathBuf>,
    /// 数据目录（关注列表 / 设置）
    #[arg(long, env = "SILM_DATA_DIR")]
    pub data_dir: Option<PathBuf>,
    /// 覆盖鉴权开关（true/false）
    #[arg(long, env = "SILM_AUTH_ENABLED")]
    pub auth_enabled: Option<bool>,
    /// 覆盖面板访问密码
    #[arg(long, env = "SILM_AUTH_PASSWORD")]
    pub auth_password: Option<String>,
    /// 上游 HTTP 代理（可选，例如 socks5h://127.0.0.1:1080）
    #[arg(long, env = "SILM_UPSTREAM_PROXY")]
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub upstream: UpstreamConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    /// 静态页面目录。留空则使用内嵌到二进制的页面。
    #[serde(default)]
    pub web_dir: Option<PathBuf>,
    /// 房间详情缓存秒数
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    /// 按需开启；关闭时所有人可直接访问
    #[serde(default)]
    pub enabled: bool,
    /// 面板访问密码
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamConfig {
    /// 所有对外请求的代理，留空直连
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_listen() -> String { "0.0.0.0:8080".into() }
fn default_data_dir() -> PathBuf { PathBuf::from("./data") }
fn default_cache_ttl() -> u64 { 60 }
fn default_timeout() -> u64 { 20 }

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig { listen: default_listen(), web_dir: None, cache_ttl: default_cache_ttl() },
            auth: AuthConfig { enabled: false, password: String::new() },
            storage: StorageConfig { data_dir: default_data_dir() },
            upstream: UpstreamConfig { proxy: None, timeout_secs: default_timeout() },
        }
    }
}
impl Default for ServerConfig { fn default() -> Self { Config::default().server } }
impl Default for AuthConfig { fn default() -> Self { Config::default().auth } }
impl Default for StorageConfig { fn default() -> Self { Config::default().storage } }
impl Default for UpstreamConfig { fn default() -> Self { Config::default().upstream } }

impl Config {
    pub fn load(cli: &Cli) -> Result<Self> {
        let path = cli.config.clone().unwrap_or_else(|| PathBuf::from("config.toml"));
        let mut cfg: Config = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("读取配置 {} 失败", path.display()))?;
            toml::from_str(&text).with_context(|| format!("解析配置 {} 失败", path.display()))?
        } else {
            Config::default()
        };
        if let Some(v) = &cli.listen { cfg.server.listen = v.clone(); }
        if let Some(v) = &cli.data_dir { cfg.storage.data_dir = v.clone(); }
        if let Some(v) = cli.auth_enabled { cfg.auth.enabled = v; }
        if let Some(v) = &cli.auth_password { cfg.auth.password = v.clone(); }
        if let Some(v) = &cli.proxy { cfg.upstream.proxy = Some(v.clone()); }
        if cfg.auth.enabled && cfg.auth.password.trim().is_empty() {
            anyhow::bail!("已开启鉴权但未设置 auth.password / SILM_AUTH_PASSWORD");
        }
        Ok(cfg)
    }
}
