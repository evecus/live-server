use axum::{
    body::Body,
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, HeaderMap, StatusCode, Uri},
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

use crate::auth;
use crate::danmaku::DanmakuHub;
use crate::error::{ApiResult, AppError, IntoApi};
use crate::http_client::h;
use crate::models::PlayQuality;
use crate::state::{RoomCache, Shared};
use crate::store::Settings;
use crate::util::urlencode;

pub fn router(st: Shared) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/auth/status", get(auth::status))
        .route("/api/auth/login", post(auth::login))
        .route("/api/sites", get(sites))
        .route("/api/parse", post(parse_link))
        .route("/api/search", get(search))
        .route("/api/rooms/:platform/:room", get(room))
        .route("/api/rooms/:platform/:room/play", get(play))
        .route("/api/rooms/:platform/:room/status", get(room_status))
        .route("/api/follows", get(follows).post(add_follow))
        .route("/api/follows/export", get(export_follows))
        .route("/api/follows/import", post(import_follows))
        .route("/api/follows/:id", delete(del_follow).patch(patch_follow))
        .route("/api/settings", get(get_settings).post(set_settings))
        .route("/api/proxy/stream", get(proxy_stream))
        .route("/api/proxy/hls", get(proxy_hls))
        .route("/api/proxy/image", get(proxy_image))
        .route("/ws/danmaku", get(ws_danmaku))
        .fallback(static_files)
        .layer(middleware::from_fn_with_state(st.clone(), auth::middleware))
        .with_state(st)
}

#[derive(rust_embed::RustEmbed)]
#[folder = "frontend/dist/"]
struct WebAssets;

async fn health() -> impl IntoResponse { Json(json!({ "ok": true, "app": "silm-live-server" })) }

async fn sites(State(st): State<Shared>) -> impl IntoResponse {
    Json(json!({ "sites": st.sites.list() }))
}

#[derive(Deserialize)]
struct ParseBody { text: String }

async fn parse_link(State(st): State<Shared>, Json(b): Json<ParseBody>) -> ApiResult<Json<Value>> {
    match st.sites.parse_link(&st, &b.text).await {
        Some((platform, room_id)) => Ok(Json(json!({ "platform": platform, "roomId": room_id }))),
        None => Err(AppError::bad("无法识别该链接 / 房间号，请检查输入")),
    }
}

#[derive(Deserialize)]
struct SearchQuery { platform: String, kw: String, #[serde(default = "one")] page: u32, #[serde(default)] anchor: bool }
fn one() -> u32 { 1 }

async fn search(State(st): State<Shared>, Query(q): Query<SearchQuery>) -> ApiResult<Json<Value>> {
    let site = st.sites.site_or_err(&q.platform)?;
    if !site.searchable() { return Err(AppError::bad(format!("{}暂不支持搜索", site.name()))); }
    let r = site.search(&st, &q.kw, q.page.max(1), q.anchor).await.api("搜索")?;
    Ok(Json(json!({ "hasMore": r.has_more, "items": r.items })))
}

async fn cached_detail(st: &Shared, platform: &str, room: &str) -> ApiResult<crate::models::RoomDetail> {
    let site = st.sites.site_or_err(platform)?;
    let key = RoomCache::key(platform, room);
    if let Some(d) = st.cache.get(&key) { return Ok(d); }
    let d = site.room_detail(st, room).await.api("获取房间信息")?;
    st.cache.put(key, d.clone());
    Ok(d)
}

async fn room(State(st): State<Shared>, Path((platform, room)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let d = cached_detail(&st, &platform, &room).await?;
    Ok(Json(json!({ "detail": d })))
}

fn proxy_params(raw: &str, q: &PlayQuality) -> Vec<(String, String)> {
    let mut p = vec![("u".to_string(), raw.to_string())];
    for (k, v) in &q.headers {
        let kl = k.to_lowercase();
        if matches!(kl.as_str(), "referer" | "user-agent" | "cookie") { p.push((kl, v.clone())); }
    }
    p
}

fn proxy_url(raw: &str, q: &PlayQuality, hls: bool) -> String {
    let endpoint = if hls || raw.contains(".m3u8") { "/api/proxy/hls" } else { "/api/proxy/stream" };
    let query = proxy_params(raw, q).iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>().join("&");
    format!("{endpoint}?{query}")
}

async fn play(State(st): State<Shared>, Path((platform, room)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let site = st.sites.site_or_err(&platform)?;
    let detail = cached_detail(&st, &platform, &room).await?;
    let qualities = site.play(&st, &detail).await.api("获取播放地址")?;
    let list: Vec<Value> = qualities.iter().map(|q| {
        let lines: Vec<Value> = q.urls.iter().enumerate().map(|(i, u)| {
            let is_hls = u.contains(".m3u8");
            json!({
                "name": format!("线路 {}", i + 1),
                "type": if is_hls { "hls" } else { "flv" },
                "url": proxy_url(u, q, is_hls),
            })
        }).collect();
        json!({ "name": q.name, "key": q.key, "lines": lines })
    }).collect();
    Ok(Json(json!({
        "qualities": list,
        "danmaku": site.danmaku_supported(),
        "roomId": detail.room_id,
    })))
}

async fn room_status(State(st): State<Shared>, Path((platform, room)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    let site = st.sites.site_or_err(&platform)?;
    let live = site.live_status(&st, &room).await.api("查询开播状态")?;
    Ok(Json(json!({ "live": live })))
}

// ---------------- 关注 ----------------

async fn follows(State(st): State<Shared>) -> impl IntoResponse {
    Json(json!({ "items": st.store.follows().await }))
}

#[derive(Deserialize)]
struct AddFollow { platform: String, room_id: String, #[serde(default)] title: String, #[serde(default)] cover: String, #[serde(default)] user_name: String }

async fn add_follow(State(st): State<Shared>, Json(b): Json<AddFollow>) -> ApiResult<Json<Value>> {
    if b.room_id.trim().is_empty() { return Err(AppError::bad("房间号不能为空")); }
    st.sites.site_or_err(&b.platform)?;
    let f = st.store.add(&b.platform, &b.room_id, &b.title, &b.cover, &b.user_name).await
        .map_err(|e| AppError::internal(e.to_string()))?;
    st.cache.clear();
    Ok(Json(json!({ "item": f })))
}

async fn del_follow(State(st): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    st.store.remove(&id).await.map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PatchFollow { #[serde(default)] top: Option<bool> }

async fn patch_follow(State(st): State<Shared>, Path(id): Path<String>, Json(b): Json<PatchFollow>) -> ApiResult<Json<Value>> {
    if let Some(t) = b.top { st.store.set_top(&id, t).await.map_err(|e| AppError::internal(e.to_string()))?; }
    Ok(Json(json!({ "ok": true })))
}

async fn export_follows(State(st): State<Shared>) -> impl IntoResponse { Json(st.store.export_json().await) }

async fn import_follows(State(st): State<Shared>, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    let n = st.store.import_json(v).await.map_err(|e| AppError::bad(e.to_string()))?;
    st.cache.clear();
    Ok(Json(json!({ "imported": n })))
}

// ---------------- 设置 ----------------

async fn get_settings(State(st): State<Shared>) -> impl IntoResponse { Json(st.store.settings().await) }

async fn set_settings(State(st): State<Shared>, Json(s): Json<Settings>) -> ApiResult<Json<Value>> {
    st.store.update_settings(s).await.map_err(|e| AppError::internal(e.to_string()))?;
    st.cache.clear();
    Ok(Json(json!({ "ok": true })))
}

// ---------------- 流代理 ----------------

#[derive(Deserialize)]
struct ProxyQuery {
    u: String,
    #[serde(default)] referer: String,
    #[serde(default)] ua: String,
    #[serde(default)] cookie: String,
    #[serde(default)] token: Option<String>,
}

fn safe_url(url: &str) -> ApiResult<Url> {
    let u = Url::parse(url).map_err(|_| AppError::bad("代理地址无效"))?;
    if !matches!(u.scheme(), "http" | "https") { return Err(AppError::bad("仅支持 http/https 代理")); }
    let host = u.host_str().unwrap_or("").to_lowercase();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "0.0.0.0" | "169.254.169.254" | "::1") || host.starts_with("127.") {
        return Err(AppError::bad("禁止代理本机地址"));
    }
    Ok(u)
}

fn upstream_headers(q: &ProxyQuery, incoming: &HeaderMap) -> HeaderMap {
    let mut pairs: Vec<(&str, &str)> = vec![
        ("user-agent", if q.ua.is_empty() { crate::identity::BILI_ROOM_UA } else { &q.ua }),
        ("referer", &q.referer),
    ];
    if !q.cookie.is_empty() { pairs.push(("cookie", &q.cookie)); }
    let mut hs = h(&pairs);
    if let Some(range) = incoming.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        if let Ok(v) = axum::http::HeaderValue::from_str(range) { hs.insert(header::RANGE, v); }
    }
    hs
}

fn proxy_header_query(q: &ProxyQuery) -> String {
    let mut parts = vec![("referer".to_string(), q.referer.clone()), ("ua".to_string(), q.ua.clone()), ("cookie".to_string(), q.cookie.clone())];
    if let Some(t) = &q.token { parts.push(("token".to_string(), t.clone())); }
    parts.into_iter().filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{}={}", urlencode(&k), urlencode(&v))).collect::<Vec<_>>().join("&")
}

async fn proxy_stream(
    State(st): State<Shared>, Query(q): Query<ProxyQuery>, headers: HeaderMap,
) -> ApiResult<Response> {
    safe_url(&q.u)?;
    let resp = st.http.stream(&q.u, &upstream_headers(&q, &headers)).await
        .map_err(|e| AppError::upstream(e.to_string()))?;
    let status = resp.status();
    let ct = resp.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
    let mut builder = Response::builder().status(status)
        .header(header::CONTENT_TYPE, ct)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    if let Some(len) = resp.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()) {
        builder = builder.header(header::CONTENT_LENGTH, len);
    }
    builder.body(Body::from_stream(resp.bytes_stream()))
        .map_err(|e| AppError::internal(e.to_string()))
}

/// HLS：重写 m3u8 里的分片 / 密钥地址，全部走本地代理并带上鉴权与请求头
async fn proxy_hls(
    State(st): State<Shared>, Query(q): Query<ProxyQuery>, headers: HeaderMap,
) -> ApiResult<Response> {
    let base = safe_url(&q.u)?;
    let (text, _) = st.http.get_text(&q.u, &upstream_headers(&q, &headers)).await
        .map_err(|e| AppError::upstream(e.to_string()))?;
    let extra = proxy_header_query(&q);
    let make = |raw: &str| -> String {
        let absolute = base.join(raw).unwrap_or_else(|_| base.clone()).to_string();
        format!("/api/proxy/stream?u={}&{}", urlencode(&absolute), extra)
    };
    let mut out = String::with_capacity(text.len() + 256);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#") {
            // 处理 EXT-X-KEY / EXT-X-MAP 里的 URI="..."
            if let Some(start) = trimmed.find("URI=\"") {
                let from = start + 5;
                if let Some(end) = trimmed[from..].find('"') {
                    let uri = &trimmed[from..from + end];
                    let replaced = format!("URI=\"{}\"", make(uri));
                    out.push_str(&trimmed[..start]);
                    out.push_str(&replaced);
                    out.push_str(&trimmed[from + end + 1..]);
                    out.push('\n');
                    continue;
                }
            }
            out.push_str(line); out.push('\n');
        } else if !trimmed.is_empty() {
            out.push_str(&make(trimmed)); out.push('\n');
        } else { out.push('\n'); }
    }
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Body::from(out)).map_err(|e| AppError::internal(e.to_string()))?)
}

async fn proxy_image(State(st): State<Shared>, Query(q): Query<ProxyQuery>, headers: HeaderMap) -> ApiResult<Response> {
    safe_url(&q.u)?;
    let resp = st.http.client.get(&q.u).headers(upstream_headers(&q, &headers)).send().await
        .map_err(|e| AppError::upstream(e.to_string()))?;
    let ct = resp.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("image/jpeg").to_string();
    let bytes = resp.bytes().await.map_err(|e| AppError::upstream(e.to_string()))?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, ct)
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(bytes)).map_err(|e| AppError::internal(e.to_string()))?)
}

// ---------------- 静态页面（内嵌单文件） ----------------

async fn static_files(State(st): State<Shared>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // 可选：外部静态目录优先（开发时无需重新编译）
    if let Some(dir) = &st.cfg.server.web_dir {
        let file = dir.join(path);
        if let Ok(bytes) = tokio::fs::read(&file).await {
            return serve_bytes(path, bytes);
        }
    }
    if let Some(f) = WebAssets::get(path) {
        return serve_bytes(path, f.data.into_owned());
    }
    // SPA 回退
    if let Some(f) = WebAssets::get("index.html") {
        return serve_bytes("index.html", f.data.into_owned());
    }
    (StatusCode::NOT_FOUND, "前端未构建：请先执行 pnpm build").into_response()
}

fn serve_bytes(path: &str, bytes: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes)).unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ---------------- 弹幕 WebSocket ----------------

#[derive(Deserialize)]
struct DanmakuQuery { platform: String, room: String }

async fn ws_danmaku(
    ws: WebSocketUpgrade, State(st): State<Shared>, Query(q): Query<DanmakuQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_danmaku_socket(socket, st, q))
}

async fn handle_danmaku_socket(socket: WebSocket, st: Shared, q: DanmakuQuery) {
    let (mut sink, mut stream) = socket.split();
    let detail = match cached_detail(&st, &q.platform, &q.room).await {
        Ok(d) => d,
        Err(e) => {
            let _ = sink.send(WsMessage::Text(json!({ "type": "error", "message": e.to_string() }).to_string())).await;
            return;
        }
    };
    let mut sub = match DanmakuHub::subscribe(st.danmaku.clone(), st.clone(), &q.platform, &q.room, detail.danmaku_args).await {
        Ok(s) => s,
        Err(e) => {
            let _ = sink.send(WsMessage::Text(json!({ "type": "error", "message": e.to_string() }).to_string())).await;
            return;
        }
    };
    let _ = sink.send(WsMessage::Text(json!({ "type": "info", "message": "弹幕已连接" }).to_string())).await;

    loop {
        tokio::select! {
            msg = sub.recv() => match msg {
                Some(m) => {
                    let text = serde_json::to_string(&m).unwrap_or_default();
                    if sink.send(WsMessage::Text(text)).await.is_err() { break; }
                }
                None => break,
            },
            incoming = stream.next() => match incoming {
                Some(Ok(WsMessage::Close(_))) | None => break,
                Some(Ok(WsMessage::Ping(p))) => { if sink.send(WsMessage::Pong(p)).await.is_err() { break; } }
                Some(Err(_)) => break,
                _ => {}
            }
        }
    }
}
