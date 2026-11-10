//! 可选鉴权：默认关闭。开启后所有 /api 与 /ws 需要 Cookie 或 Bearer Token。

use axum::{
    extract::{Query, State},
    http::{header::AUTHORIZATION, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::config::AuthConfig;
use crate::error::{AppError, ApiResult};
use crate::state::Shared;
use crate::util::{cookie_value, random_hex, sha256_hex};

pub const COOKIE_NAME: &str = "silm_session";

#[derive(Clone)]
pub struct Auth {
    pub enabled: bool,
    /// 服务端根据启动随机密钥与密码派生，无需保存会话
    token: String,
}

impl Auth {
    pub fn new(cfg: &AuthConfig) -> Self {
        let secret = random_hex(32);
        let token = sha256_hex(&format!("{secret}:{}", cfg.password));
        Self { enabled: cfg.enabled, token }
    }

    pub fn cookie(&self) -> String { self.token.clone() }

    pub fn verify(&self, headers: &HeaderMap, query_token: Option<&str>) -> bool {
        if !self.enabled { return true; }
        if let Some(t) = query_token { if t == self.token { return true; } }
        if let Some(v) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
            let t = v.strip_prefix("Bearer ").unwrap_or(v).trim();
            if t == self.token { return true; }
        }
        if let Some(v) = headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()) {
            if cookie_value(v, COOKIE_NAME).as_deref() == Some(self.token.as_str()) { return true; }
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
        "enabled": st.auth.enabled,
        "authenticated": st.auth.verify(&headers, None),
    }))
}

pub async fn login(
    State(st): State<Shared>,
    Json(body): Json<LoginBody>,
) -> ApiResult<impl IntoResponse> {
    if !st.auth.enabled {
        return Ok(Json(json!({ "ok": true, "enabled": false })).into_response());
    }
    if body.password != st.cfg.auth.password {
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

/// WS / 流代理可带 ?token= 的辅助参数（前端默认走 Cookie，无需使用）
#[allow(dead_code)]
pub fn token_from_query(q: &Query<std::collections::HashMap<String, String>>) -> Option<String> {
    q.get("token").cloned()
}
