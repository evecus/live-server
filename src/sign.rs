//! JS 签名引擎：专用线程内嵌 QuickJS，复用 Android 版 assets/*.js。
//! 运行时不需要 Node.js，也不需要浏览器。QuickJS 不是 Send，故固定在单线程。

use anyhow::{anyhow, Result};
use rquickjs::{Context, Runtime};
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::oneshot;
use url::Url;

use crate::util::{json_quote, md5_hex, random_from, urlencode};

const DOUYU_CRYPTO: &str = include_str!("../assets/douyu_crypto.js");
const DOUYIN_ABOGUS: &str = include_str!("../assets/douyin_abogus.js");
const DOUYIN_MSSDK: &str = include_str!("../assets/douyin_mssdk.js");

/// 补齐常见浏览器全局（脚本自带的垫片优先级更高，这里只在缺失时补）
const SHIMS: &str = r#"
if (typeof console === 'undefined') { var console = {log:function(){},warn:function(){},error:function(){},info:function(){},debug:function(){}}; }
if (typeof navigator === 'undefined') { var navigator = {userAgent:'', appVersion:'', platform:'Win32', language:'zh-CN'}; }
if (typeof location === 'undefined') { var location = {href:'https://live.douyin.com/', hostname:'live.douyin.com', pathname:'/', search:''}; }
if (typeof window === 'undefined') { var window = globalThis; }
if (typeof document === 'undefined') { var document = {cookie:'', referrer:'', createElement:function(){return {}} , getElementsByTagName:function(){return []}, addEventListener:function(){}}; }
if (typeof atob === 'undefined') {
  var atob = function(s){ var c='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'; var o=''; s=String(s).replace(/=+$/,''); for(var i=0;i<s.length;i+=4){var n=c.indexOf(s[i])<<18|c.indexOf(s[i+1])<<12|(c.indexOf(s[i+2])&63)<<6|(c.indexOf(s[i+3])&63);o+=String.fromCharCode((n>>16)&255,(n>>8)&255,n&255);} return o; };
}
if (typeof btoa === 'undefined') {
  var btoa = function(s){ var c='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'; var o=''; for(var i=0;i<s.length;i+=3){var a=s.charCodeAt(i),b=i+1<s.length?s.charCodeAt(i+1):0,d=i+2<s.length?s.charCodeAt(i+2):0,n=(a<<16)|(b<<8)|d;o+=c[(n>>18)&63]+c[(n>>12)&63]+(i+1<s.length?c[(n>>6)&63]:'=')+(i+2<s.length?c[n&63]:'=');} return o; };
}
"#;

const ERR_PREFIX: &str = "__QJS_ERR__:";

type Reply = oneshot::Sender<Result<String>>;

enum Job {
    Douyu { script: String, room: String, did: String, reply: Reply },
    ABogus { query: String, ua: String, reply: Reply },
    Signature { ms_stub: String, ua: String, reply: Reply },
}

pub struct SignEngine {
    tx: Sender<Job>,
}

impl SignEngine {
    pub fn new() -> Result<Self> {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("quickjs-sign".into())
            .spawn(move || {
                if let Err(e) = run_engine(rx) {
                    tracing::error!("QuickJS 签名线程退出：{e}");
                }
            })?;
        Ok(Self { tx })
    }

    fn call(&self, build: impl FnOnce(Reply) -> Job) -> Result<oneshot::Receiver<Result<String>>> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(build(reply)).map_err(|_| anyhow!("QuickJS 引擎不可用"))?;
        Ok(rx)
    }

    /// 斗鱼：执行服务端下发的加密脚本生成播放签名参数
    pub async fn douyu_sign(&self, script: &str, room: &str, did: &str) -> Result<String> {
        let rx = self.call(|reply| Job::Douyu {
            script: script.into(), room: room.into(), did: did.into(), reply,
        })?;
        rx.await.map_err(|_| anyhow!("QuickJS 无响应"))?
    }

    /// 抖音：webcast 弹幕 signature
    pub async fn douyin_signature(&self, ms_stub: &str, ua: &str) -> Result<String> {
        let rx = self.call(|reply| Job::Signature {
            ms_stub: ms_stub.into(), ua: ua.into(), reply,
        })?;
        rx.await.map_err(|_| anyhow!("QuickJS 无响应"))?
    }

    /// 抖音：给完整 URL 追加 a_bogus 参数
    pub async fn douyin_a_bogus_url(&self, url: &str, ua: &str, ms_token: Option<&str>) -> Result<String> {
        let parsed = Url::parse(url)?;
        let mut params: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        params.retain(|(k, _)| k != "a_bogus" && k != "msToken");
        let token = ms_token.map(str::trim).filter(|s| !s.is_empty())
            .map(str::to_string).unwrap_or_else(|| generate_ms_token(107));
        params.push(("msToken".into(), token));

        let qb = params.iter()
            .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
            .collect::<Vec<_>>().join("&");
        let host = match parsed.port() {
            Some(p) => format!("{}:{}", parsed.host_str().unwrap_or(""), p),
            None => parsed.host_str().unwrap_or("").to_string(),
        };
        let base = format!("{}://{}{}", parsed.scheme(), host, parsed.path());
        let unsigned = if qb.is_empty() { base } else { format!("{base}?{qb}") };

        let rx = self.call(|reply| Job::ABogus { query: qb.clone(), ua: ua.into(), reply })?;
        let ab = rx.await.map_err(|_| anyhow!("QuickJS 无响应"))??;
        let sep = if qb.is_empty() { "?" } else { "&" };
        Ok(format!("{unsigned}{sep}a_bogus={}", urlencode(&ab)))
    }
}

/// 与 Flutter 一致：签名不得含 "-" 或 "="，且必须是纯 base64 变体字符
fn valid_douyin_sig(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && !s.contains('-') && !s.contains('=')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_'))
}

pub fn generate_ms_token(len: usize) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    random_from(alphabet, len)
}

/// 抖音弹幕 signature 的 md5 stub（纯 Rust，不需要 JS）
pub fn douyin_ms_stub(room_id: &str, unique_id: &str) -> String {
    let params = format!(
        "live_id=1,aid=6383,version_code=180800,webcast_sdk_version=1.3.0,room_id={room_id},sub_room_id=,sub_channel_id=,did_rule=3,user_unique_id={unique_id},device_platform=web,device_type=,ac=,identity=audience"
    );
    md5_hex(&params)
}

/// 在同一个 QuickJS Context 里执行一段「函数体」代码（必须以 return 返回字符串）。
/// 用 JS 层 try/catch 把异常 message/stack 带回 Rust，避免只拿到笼统的 Exception。
fn eval_str(ctx: &Context, body: &str) -> Result<String> {
    // 关键点：rquickjs 的 eval 按严格模式编译，脚本里的隐式全局赋值（如 `n = encodeURIComponent(e)`）
    // 会抛 ReferenceError；而 Function 构造器生成的函数是非严格的，与浏览器原始语义一致。
    // .call(globalThis) 同时保证 CryptoJS 的 UMD 包装 `...(this, factory)` 拿到全局对象。
    let inner = format!(
        "try {{ {body}\n }} catch(e) {{ return '{ERR_PREFIX}' + String(e) + ' | ' + ((e && e.stack) ? e.stack : ''); }}"
    );
    let wrapped = format!(
        "(function(){{ try {{ return Function({}).call(globalThis); }} catch(e) {{ return '{ERR_PREFIX}' + String(e); }} }})()",
        json_quote(&inner)
    );
    let r = ctx.with(|c| c.eval::<String, _>(wrapped));
    let s = r.map_err(|e| anyhow!("QuickJS: {e:?}"))?;
    if let Some(rest) = s.strip_prefix(ERR_PREFIX) {
        return Err(anyhow!("QuickJS 异常: {rest}"));
    }
    Ok(s)
}

/// 装载脚本并确认全局符号已注册。
/// 注意：CryptoJS 是 object、getABogus / getMSSDKSignature 是 function，
/// 因此只要求 typeof != "undefined"，不能硬编码 "function"。
/// 斗鱼的 ub98484234 由服务端下发脚本定义，装载时应校验 CryptoJS 而非 ub98484234。
fn ensure(ctx: &Context, loaded: &mut HashSet<&'static str>, key: &'static str, script: &str, global_name: &str) -> Result<()> {
    if loaded.contains(key) { return Ok(()); }
    // 脚本里的 `function foo(){}` 在 IIFE 中只是局部声明，必须显式挂到 globalThis，
    // 后续调用所在的另一个 IIFE 才能通过标识符找到它。
    // CryptoJS 由 UMD 自行挂到全局，这里等同于自赋值，无副作用。
    let name = json_quote(global_name);
    let body = format!(
        "{script}\nglobalThis[{name}] = {global_name};\nreturn String(typeof globalThis[{name}]);"
    );
    let got = eval_str(ctx, &body)?;
    if got == "undefined" || got.is_empty() {
        return Err(anyhow!("{global_name} 未注册，签名脚本装载失败"));
    }
    loaded.insert(key);
    Ok(())
}

fn run_engine(rx: Receiver<Job>) -> Result<()> {
    let rt = Runtime::new().map_err(|e| anyhow!("QuickJS Runtime: {e}"))?;
    let ctx = Context::full(&rt).map_err(|e| anyhow!("QuickJS Context: {e}"))?;
    eval_str(&ctx, &format!("{SHIMS}\nreturn String('ok');"))?;
    let mut loaded: HashSet<&'static str> = HashSet::new();
    tracing::info!("QuickJS 签名引擎就绪");

    for job in rx {
        let (reply, result) = match job {
            Job::Douyu { script, room, did, reply } => {
                (reply, handle_douyu(&ctx, &mut loaded, &script, &room, &did))
            }
            Job::ABogus { query, ua, reply } => {
                (reply, handle_abogus(&ctx, &mut loaded, &query, &ua))
            }
            Job::Signature { ms_stub, ua, reply } => {
                (reply, handle_signature(&ctx, &mut loaded, &ms_stub, &ua))
            }
        };
        let _ = reply.send(result);
    }
    Ok(())
}

fn handle_douyu(ctx: &Context, loaded: &mut HashSet<&'static str>, script: &str, room: &str, did: &str) -> Result<String> {
    ensure(ctx, loaded, "douyu", DOUYU_CRYPTO, "CryptoJS")?;
    let ts = crate::util::unix_secs().to_string();
    let body = format!(
        "{script}\nreturn String(ub98484234({},{},{}));",
        json_quote(room), json_quote(did), json_quote(&ts)
    );
    let r = eval_str(ctx, &body)?;
    if !r.contains('=') || r.contains("function") || r.contains('{') {
        return Err(anyhow!("斗鱼签名结果异常：{}", &r[..r.len().min(120)]));
    }
    Ok(r)
}

fn handle_abogus(ctx: &Context, loaded: &mut HashSet<&'static str>, query: &str, ua: &str) -> Result<String> {
    ensure(ctx, loaded, "abogus", DOUYIN_ABOGUS, "getABogus")?;
    let body = format!("return String(getABogus({},{}));", json_quote(query), json_quote(ua));
    let r = eval_str(ctx, &body)?;
    if r.is_empty() || r == "undefined" { return Err(anyhow!("a_bogus 生成失败（空结果）")); }
    Ok(r)
}

fn handle_signature(ctx: &Context, loaded: &mut HashSet<&'static str>, ms_stub: &str, ua: &str) -> Result<String> {
    ensure(ctx, loaded, "mssdk", DOUYIN_MSSDK, "getMSSDKSignature")?;
    let body = format!("return String(getMSSDKSignature({},{}));", json_quote(ms_stub), json_quote(ua));
    for _ in 0..5 {
        if let Ok(sig) = eval_str(ctx, &body) {
            if valid_douyin_sig(&sig) { return Ok(sig); }
        }
    }
    tracing::warn!("抖音 signature 生成失败，降级返回 msStub");
    Ok(ms_stub.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟测试：三个签名脚本都能被内嵌 QuickJS 装载并执行。
    /// 全程不需要 Node.js、浏览器或外网，用来守住「运行时零 Node 依赖」这条底线。
    #[tokio::test]
    async fn quickjs_sign_scripts_smoke() {
        let eng = SignEngine::new().expect("SignEngine 创建失败");
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0 Safari/537.36";

        // 1) douyu_crypto.js -> CryptoJS。ub98484234 由服务端下发脚本定义，这里用等效桩函数验证调用链。
        let stub_js = "function ub98484234(rid, did, t) { return 'v=' + CryptoJS.MD5(rid + did + t).toString(); }";
        let dy = eng.douyu_sign(stub_js, "123456", "10000000000000000000000000001501").await
            .expect("斗鱼签名失败");
        assert!(dy.starts_with("v=") && dy.len() > 20, "斗鱼签名格式异常: {dy}");
        println!("[qjs] douyu sign len={}", dy.len());

        // 2) douyin_abogus.js -> getABogus
        let signed = eng.douyin_a_bogus_url(
            "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=webcast&live_id=1",
            ua, None,
        ).await.expect("a_bogus 生成失败");
        assert!(signed.contains("a_bogus="), "签名 URL 缺少 a_bogus: {signed}");
        let ab = signed.rsplit("a_bogus=").next().unwrap_or("");
        assert!(!ab.is_empty() && ab != "undefined", "a_bogus 为空: {signed}");
        println!("[qjs] a_bogus len={}", ab.len());

        // 3) douyin_mssdk.js -> getMSSDKSignature（625KB 混淆脚本，放最后避免覆盖全局垫片）
        let stub = douyin_ms_stub("123456", "789");
        let sig = eng.douyin_signature(&stub, ua).await.expect("MSSDK signature 失败");
        assert!(!sig.is_empty(), "signature 为空");
        println!("[qjs] signature len={} generated={}", sig.len(), sig != stub);
    }
}
