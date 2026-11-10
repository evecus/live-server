//! 弹幕层：服务端维护各平台 WebSocket 上游连接，把统一消息广播给多个网页客户端。
//! 同一房间多个浏览器复用一条上游连接。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use futures_util::{lock::Mutex, SinkExt, StreamExt};
use http::{HeaderName, HeaderValue};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::{self, Sender};
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

use crate::models::DanmakuMessage;
use crate::state::Shared;

pub mod bili;
pub mod douyin;
pub mod douyu;
pub mod huya;
pub mod kuaishou;
pub mod xhs;

type RawWs = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[async_trait]
pub trait DanmakuSource: Send {
    async fn run(self: Box<Self>, tx: Sender<DanmakuMessage>) -> Result<()>;
}

pub fn build_source(st: Shared, platform: &str, _room: &str, args: Value) -> Result<Box<dyn DanmakuSource>> {
    match platform {
        "bilibili" => Ok(Box::new(bili::BiliSource { st, args })),
        "douyu" => Ok(Box::new(douyu::DouyuSource { st, args })),
        "huya" => Ok(Box::new(huya::HuyaSource { st, args })),
        "douyin" => Ok(Box::new(douyin::DouyinSource { st, args })),
        "kuaishou" => Ok(Box::new(kuaishou::KuaishouSource { st, args })),
        "xiaohongshu" => Ok(Box::new(xhs::XhsSource)),
        _ => Err(anyhow!("不支持弹幕的平台：{platform}")),
    }
}

pub fn emit(tx: &Sender<DanmakuMessage>, msg: DanmakuMessage) { let _ = tx.send(msg); }

pub fn gunzip(data: &[u8]) -> Vec<u8> {
    if data.len() < 2 || data[0] != 0x1f || data[1] != 0x8b { return data.to_vec(); }
    use std::io::Read;
    let mut out = Vec::new();
    match flate2::read::GzDecoder::new(data).read_to_end(&mut out) {
        Ok(_) => out,
        Err(_) => data.to_vec(),
    }
}

pub fn zlib_decode(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    match flate2::read::ZlibDecoder::new(data).read_to_end(&mut out) {
        Ok(_) => out,
        Err(_) => data.to_vec(),
    }
}

pub fn brotli_decode(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    match brotli::Decompressor::new(data, 4096).read_to_end(&mut out) {
        Ok(_) => out,
        Err(_) => data.to_vec(),
    }
}

/// 通用 WS 循环：进房包 -> 固定间隔心跳 -> 帧解析（可返回需要回送的字节）
pub async fn ws_loop<F>(
    url: String,
    headers: Vec<(String, String)>,
    heartbeat_ms: u64,
    heartbeat: Vec<u8>,
    join: Vec<Vec<u8>>,
    tx: Sender<DanmakuMessage>,
    mut on_frame: F,
) -> Result<()>
where
    F: FnMut(&[u8], &Sender<DanmakuMessage>, &mut Vec<Vec<u8>>) + Send + 'static,
{
    let mut req = url.as_str().into_client_request()?;
    for (k, v) in headers {
        if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(&v)) {
            req.headers_mut().insert(name, val);
        }
    }
    let (socket, _) = connect_async(req).await?;
    let ws = Arc::new(Mutex::new(socket));
    for j in join {
        ws.lock().await.send(Message::Binary(j)).await?;
    }

    let hb_dur = Duration::from_millis(heartbeat_ms.max(1000));
    let mut next_hb = Instant::now() + hb_dur;
    loop {
        let dur = next_hb.saturating_duration_since(Instant::now());
        let frame: Option<std::result::Result<Message, tokio_tungstenite::tungstenite::Error>> = {
            let mut g = ws.lock().await;
            match tokio::time::timeout(dur, g.next()).await {
                Ok(v) => v,
                Err(_) => None,
            }
        };
        if Instant::now() >= next_hb {
            ws.lock().await.send(Message::Binary(heartbeat.clone())).await?;
            next_hb = Instant::now() + hb_dur;
        }
        match frame {
            Some(Ok(Message::Binary(b))) => {
                let mut replies = Vec::new();
                on_frame(&b, &tx, &mut replies);
                for r in replies { ws.lock().await.send(Message::Binary(r)).await?; }
            }
            Some(Ok(Message::Text(t))) => {
                let mut replies = Vec::new();
                on_frame(t.as_bytes(), &tx, &mut replies);
                for r in replies { ws.lock().await.send(Message::Binary(r)).await?; }
            }
            Some(Ok(Message::Ping(p))) => { ws.lock().await.send(Message::Pong(p)).await?; }
            Some(Ok(Message::Close(_))) => break,
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e.into()),
            None => {
                // 超时分支：下一轮继续等待
                continue;
            }
        }
    }
    Ok(())
}

/// 依次尝试候选地址，全部失败才返回错误
pub async fn try_urls<F>(
    urls: Vec<String>,
    headers: Vec<(String, String)>,
    heartbeat_ms: u64,
    heartbeat: Vec<u8>,
    join: Vec<Vec<u8>>,
    tx: Sender<DanmakuMessage>,
    on_frame: F,
) -> Result<()>
where
    F: Fn(&[u8], &Sender<DanmakuMessage>, &mut Vec<Vec<u8>>) + Send + Sync + Clone + 'static,
{
    let mut last = anyhow!("无可用弹幕地址");
    for url in urls {
        let f = on_frame.clone();
        match ws_loop(url.clone(), headers.clone(), heartbeat_ms, heartbeat.clone(), join.clone(), tx.clone(), f).await {
            Ok(()) => return Ok(()),
            Err(e) => { tracing::warn!("弹幕地址 {url} 失败：{e}"); last = e; }
        }
    }
    Err(last)
}

struct RoomChannel {
    tx: Sender<DanmakuMessage>,
    task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    users: AtomicUsize,
    dead: AtomicBool,
}

pub struct DanmakuHub {
    rooms: DashMap<String, Arc<RoomChannel>>,
}

impl DanmakuHub {
    pub fn new() -> Self { Self { rooms: DashMap::new() } }

    pub async fn subscribe(
        hub: Arc<DanmakuHub>,
        st: Shared,
        platform: &str,
        room: &str,
        args: Value,
    ) -> Result<Subscription> {
        let key = format!("{platform}:{room}");
        let channel = hub.rooms.entry(key.clone()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(512);
            Arc::new(RoomChannel {
                tx,
                task: tokio::sync::Mutex::new(None),
                users: AtomicUsize::new(0),
                dead: AtomicBool::new(false),
            })
        }).clone();

        let mut task = channel.task.lock().await;
        if task.is_none() && !channel.dead.load(Ordering::SeqCst) {
            let source = build_source(st, platform, room, args)?;
            let tx = channel.tx.clone();
            let ch = channel.clone();
            *task = Some(tokio::spawn(async move {
                if let Err(e) = source.run(tx.clone()).await {
                    let _ = tx.send(DanmakuMessage::error(format!("弹幕连接失败：{e}")));
                }
                ch.dead.store(true, Ordering::SeqCst);
            }));
        } else if channel.dead.load(Ordering::SeqCst) {
            return Err(anyhow!("弹幕连接已结束，请刷新页面重试"));
        }
        drop(task);

        channel.users.fetch_add(1, Ordering::SeqCst);
        let rx = channel.tx.subscribe();
        Ok(Subscription { hub, key, channel, rx })
    }
}

pub struct Subscription {
    hub: Arc<DanmakuHub>,
    key: String,
    channel: Arc<RoomChannel>,
    rx: broadcast::Receiver<DanmakuMessage>,
}

impl Subscription {
    pub async fn recv(&mut self) -> Option<DanmakuMessage> {
        loop {
            match self.rx.recv().await {
                Ok(m) => return Some(m),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let left = self.channel.users.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
        if left == 0 {
            if let Ok(mut g) = self.channel.task.try_lock() {
                if let Some(t) = g.take() { t.abort(); }
            }
            self.hub.rooms.remove(&self.key);
        }
    }
}
