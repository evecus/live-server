//! 可选鉴权：默认关闭。可在「设置」面板中运行时开启 / 关闭 / 改密码，状态持久化到 silm.json。
//! 开启后所有 /api 与 /ws 需要 Cookie 或 Bearer Token；静态页面放行（登录界面在前端）。

use axum::{
    extract::{Query, State},
    http::{header::AUTHORIZATION, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

use crate::config::AuthConfig;
use crate::error::{ApiResult, AppError};
use crate::state::Shared;
use crate::util::{cookie_value, random_hex, sha256_hex};

pub const COOKIE_NAME: &str = "silm_session";

struct AuthInner {
    enabled: bool,
    password: String,
    token: String,
}

/// 可运行时更新的鉴权状态（RwLock 保证多请求并发读，设置页保存时写入）
#[derive(Clone)]
pub struct Auth {
    secret: String,
    inner: Arc<RwLock<AuthInner>>,
}

impl Auth {
    pub fn new(cfg: &AuthConfig) -> Self {
        let secret = random_hex(32);
        let enabled = cfg.enabled && !cfg.password.trim().is_empty();
        let token = Self::derive(&secret, &cfg.password);
        Self {
            secret,
            inner: Arc::new(RwLock::new(AuthInner {
                enabled,
                password: cfg.password.clone(),
                token,
            })),
        }
    }

    fn derive(secret: &str, password: &str) -> String {
        sha256_hex(&format!("{secret}:{password}"))
    }

    pub fn enabled(&self) -> bool {
        self.inner.read().map(|i| i.enabled).unwrap_or(false)
    }

    pub fn has_password(&self) -> bool {
        self.inner.read().map(|i| !i.password.trim().is_empty()).unwrap_or(false)
    }

    pub fn password(&self) -> String {
        self.inner.read().map(|i| i.password.clone()).unwrap_or_default()
    }

    pub fn cookie(&self) -> String {
        self.inner.read().map(|i| i.token.clone()).unwrap_or_default()
    }

    /// 运行时更新；开启时必须带非空密码，密码变化会让旧 Cookie 立即失效
    pub fn update(&self, enabled: bool, password: &str) -> Result<(), String> {
        if enabled && password.trim().is_empty() {
            return Err("开启访问验证时必须设置访问密码".into());
        }
        let token = Self::derive(&self.secret, password);
        let mut inner = self.inner.write().map_err(|_| "鉴权状态更新失败".to_string())?;
        inner.enabled = enabled;
        inner.password = password.to_string();
        inner.token = token;
        Ok(())
    }

    pub fn verify(&self, headers: &HeaderMap, query_token: Option<&str>) -> bool {
        let inner = match self.inner.read() { Ok(i) => i, Err(_) => return false };
        if !inner.enabled { return true; }
        if let Some(t) = query_token { if t == inner.token { return true; } }
        if let Some(v) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
            let t = v.strip_prefix("Bearer ").unwrap_or(v).trim();
            if t == inner.token { return true; }
        }
        if let Some(v) = headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()) {
            if cookie_value(v, COOKIE_NAME).as_deref() == Some(inner.token.as_str()) { return true; }
        }
        false
    }
}

/// 静态页面放行（登录界面在前端），API / WS 受保护
pub async fn middleware(State(st): State<Shared>, req: Request<axum::body::Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let protected = path.starts_with("/api/") || path.starts_with("/ws/");
    let public = !protected
        || path == "/api/auth/status"
        || path == "/api/auth/login"
        || path == "/api/auth/logout"
        || path == "/health";
    if public { return next.run(req).await; }

    let query_token = req.uri().query().and_then(|q| {
        q.split('&').find_map(|p| p.strip_prefix("token=").map(crate::util::urldecode))
    });
    if st.auth.verify(req.headers(), query_token.as_deref()) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": "未登录或登录已失效" }))).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginBody { pub password: String }

pub async fn status(State(st): State<Shared>, headers: HeaderMap) -> impl IntoResponse {
    Json(json!({
        "enabled": st.auth.enabled(),
        "authenticated": st.auth.verify(&headers, None),
    }))
}

pub async fn login(
    State(st): State<Shared>,
    Json(body): Json<LoginBody>,
) -> ApiResult<impl IntoResponse> {
    if !st.auth.enabled() {
        return Ok(Json(json!({ "ok": true, "enabled": false })).into_response());
    }
    if body.password != st.auth.password() {
        return Err(AppError::Unauthorized("密码错误".into()));
    }
    let cookie = format!(
        "{COOKIE_NAME}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000",
        st.auth.cookie()
    );
    let mut resp = Json(json!({ "ok": true, "enabled": true })).into_response();
    resp.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        axum::http::HeaderValue::from_str(&cookie).map_err(|e| AppError::internal(e.to_string()))?,
    );
    Ok(resp)
}

/// 退出登录：清除会话 Cookie（HttpOnly Cookie 需由服务端过期）
pub async fn logout() -> impl IntoResponse {
    let cookie = format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    let mut resp = Json(json!({ "ok": true })).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&cookie) {
        resp.headers_mut().insert(axum::http::header::SET_COOKIE, v);
    }
    resp
}

/// WS / 流代理可带 ?token= 的辅助参数（前端默认走 Cookie，无需使用）
#[allow(dead_code)]
pub fn token_from_query(q: &Query<std::collections::HashMap<String, String>>) -> Option<String> {
    q.get("token").cloned()
}
