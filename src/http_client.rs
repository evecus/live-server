use anyhow::{anyhow, Context as _, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::config::UpstreamConfig;

#[derive(Clone)]
pub struct Http {
    pub client: reqwest::Client,
}

/// 由 &[(&str, &str)] 构造请求头
pub fn h(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut m = HeaderMap::new();
    for (k, v) in pairs {
        if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            m.insert(name, val);
        }
    }
    m
}
pub fn with(mut base: HeaderMap, pairs: &[(&str, &str)]) -> HeaderMap {
    for (k, v) in pairs {
        if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
            base.insert(name, val);
        }
    }
    base
}

impl Http {
    pub fn new(cfg: &UpstreamConfig) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            // 与原 Android 一致强制 HTTP/1.1，降低抖音 WAF 对 h2 指纹的敏感度
            .http1_only()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(crate::identity::BILI_UA);
        if let Some(proxy) = cfg.proxy.as_ref().filter(|p| !p.trim().is_empty()) {
            builder = builder.proxy(reqwest::Proxy::all(proxy).context("上游代理地址无效")?);
        }
        Ok(Self { client: builder.build()? })
    }

    pub async fn get_text(&self, url: &str, headers: &HeaderMap) -> Result<(String, HeaderMap)> {
        let resp = self.client.get(url).headers(headers.clone()).send().await?;
        let hs = resp.headers().clone();
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() { return Err(anyhow!("HTTP {} {}", status.as_u16(), &body[..body.len().min(160)])); }
        Ok((body, hs))
    }

    pub async fn get_json(&self, url: &str, headers: &HeaderMap) -> Result<Value> {
        let (body, _) = self.get_text(url, headers).await?;
        if body.trim().is_empty() { return Err(anyhow!("空响应（疑似被风控）")); }
        let v: Value = serde_json::from_str(&body)
            .with_context(|| format!("JSON 解析失败：{}", &body[..body.len().min(160)]))?;
        Ok(v)
    }

    pub async fn get_bytes(&self, url: &str, headers: &HeaderMap) -> Result<Vec<u8>> {
        let resp = self.client.get(url).headers(headers.clone()).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() { return Err(anyhow!("HTTP {}", status.as_u16())); }
        Ok(bytes.to_vec())
    }

    pub async fn post_form(&self, url: &str, form: &str, headers: &HeaderMap) -> Result<Value> {
        let mut hs = headers.clone();
        hs.insert(reqwest::header::CONTENT_TYPE, HeaderValue::from_static("application/x-www-form-urlencoded"));
        let resp = self.client.post(url).headers(hs).body(form.to_string()).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() { return Err(anyhow!("HTTP {}", status.as_u16())); }
        if body.trim().is_empty() { return Ok(Value::Null); }
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn head(&self, url: &str, headers: &HeaderMap) -> Result<HeaderMap> {
        let resp = self.client.head(url).headers(headers.clone()).send().await?;
        Ok(resp.headers().clone())
    }

    /// 跟随重定向后返回最终 URL（用于分享短链展开）
    pub async fn resolve(&self, url: &str, headers: &HeaderMap) -> Result<String> {
        let resp = self.client.get(url).headers(headers.clone()).send().await?;
        Ok(resp.url().to_string())
    }

    /// 流式响应（流代理用，不落地不缓冲整段）
    pub async fn stream(&self, url: &str, headers: &HeaderMap) -> Result<reqwest::Response> {
        let resp = self.client.get(url).headers(headers.clone()).send().await?;
        Ok(resp)
    }
}
