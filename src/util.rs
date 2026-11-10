use md5::Digest as Md5Digest;
use rand::Rng;
use serde_json::Value;
use sha2::Digest as ShaDigest;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn md5_hex(s: &str) -> String {
    let mut h = md5::Md5::new();
    Md5Digest::update(&mut h, s.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(s: &str) -> String {
    let mut h = sha2::Sha256::new();
    ShaDigest::update(&mut h, s.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unix_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}
pub fn unix_secs() -> u64 { (unix_ms() / 1000) as u64 }

/// 与 Java URLEncoder 一致：空格 -> %20，保留 - _ . ~，大写十六进制（逐字节编码）
pub fn urlencode(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// 与 Java URLDecoder 一致：+ -> 空格
pub fn urldecode(s: &str) -> String {
    percent_encoding::percent_decode_str(s.replace('+', " ").as_str())
        .decode_utf8_lossy()
        .to_string()
}

/// 纯百分号解码，保留 '+'（用于 base64 / 虎牙 anticode）
pub fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().to_string()
}

pub fn query_string(map: &BTreeMap<String, String>) -> String {
    map.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v))).collect::<Vec<_>>().join("&")
}

pub fn parse_query(q: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for part in q.split('&') {
        if part.is_empty() { continue; }
        let mut it = part.splitn(2, '=');
        let k = it.next().unwrap_or("");
        let v = it.next().unwrap_or("");
        out.insert(urldecode(k), urldecode(v));
    }
    out
}

pub fn json_quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// 把 JS 的 undefined 字面量替换为 null（仅处理紧跟在分隔符后的情况）
pub fn replace_undefined(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        out.push(c);
        i += 1;
        if matches!(c, ':' | '{' | ',' | '[' | '(') {
            let mut j = i;
            while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() { j += 1; }
            if src[j..].starts_with("undefined") {
                out.push_str(&src[i..j]);
                out.push_str("null");
                i = j + "undefined".len();
            }
        }
    }
    out
}

/// serde_json::Value 链式取值助手
pub trait J {
    fn s(&self, k: &str) -> String;
    fn i(&self, k: &str) -> i64;
    fn u(&self, k: &str) -> u64;
    fn b(&self, k: &str) -> bool;
    fn o(&self, k: &str) -> Value;
    fn a(&self, k: &str) -> Vec<Value>;
    fn txt(&self) -> String;
    fn num(&self) -> i64;
}

impl J for Value {
    fn s(&self, k: &str) -> String { self.get(k).map(|v| v.txt()).unwrap_or_default() }
    fn i(&self, k: &str) -> i64 { self.get(k).map(|v| v.num()).unwrap_or(0) }
    fn u(&self, k: &str) -> u64 { self.i(k).max(0) as u64 }
    fn b(&self, k: &str) -> bool {
        match self.get(k) {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0) == 1,
            Some(Value::String(s)) => s == "1" || s.eq_ignore_ascii_case("true"),
            _ => false,
        }
    }
    fn o(&self, k: &str) -> Value { self.get(k).cloned().unwrap_or(Value::Null) }
    fn a(&self, k: &str) -> Vec<Value> {
        self.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default()
    }
    fn txt(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => String::new(),
            _ => String::new(),
        }
    }
    fn num(&self) -> i64 {
        match self {
            Value::Number(n) => n.as_i64().unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
            Value::String(s) => s.trim().parse().unwrap_or(0),
            Value::Bool(b) => if *b { 1 } else { 0 },
            _ => 0,
        }
    }
}

pub fn strip_em_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' { in_tag = true; continue; }
        if c == '>' { in_tag = false; continue; }
        if !in_tag { out.push(c); }
    }
    out
}

pub fn parse_count(raw: &str) -> u64 {
    let text = raw.trim().replace(',', "");
    if text.is_empty() { return 0; }
    let has_wan = text.contains('万');
    let cleaned = text.replace('万', "").replace('+', "");
    match cleaned.parse::<f64>() {
        Ok(v) => ((if has_wan { v * 10000.0 } else { v }).max(0.0)) as u64,
        Err(_) => 0,
    }
}

pub fn html_unescape(input: &str) -> String {
    input
        .replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&#39;", "'")
        .replace("&apos;", "'").replace("&nbsp;", " ")
}

pub fn to_https(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else { url.to_string() }
}

/// 提取 k=v; k2=v2 中的某个值
pub fn cookie_value(cookie: &str, name: &str) -> Option<String> {
    cookie.split(';').map(|p| p.trim()).find_map(|part| {
        let mut it = part.splitn(2, '=');
        let k = it.next()?.trim();
        let v = it.next()?.trim();
        if k == name && !v.is_empty() { Some(v.to_string()) } else { None }
    })
}

pub fn merge_cookies(parts: &[&str]) -> String {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for c in parts {
        for part in c.split(';') {
            let mut it = part.trim().splitn(2, '=');
            if let (Some(k), Some(v)) = (it.next(), it.next()) {
                if !k.trim().is_empty() { map.insert(k.trim().into(), v.trim().into()); }
            }
        }
    }
    map.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
}

pub fn random_digits(len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len).map(|_| char::from(b'0' + rng.gen_range(0..10))).collect()
}
pub fn random_hex(len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len).map(|_| format!("{:x}", rng.gen_range(0..16))).collect()
}
pub fn random_from(alphabet: &[u8], len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len).map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char).collect()
}

/// 括号配平截取 `window.__INITIAL_STATE__=` 后的 JSON（快手 / 小红书）
pub fn extract_initial_state(html: &str) -> Option<Value> {
    let key = "window.__INITIAL_STATE__";
    let start = html.find(key)? + key.len();
    let mut i = start;
    let bytes = html.as_bytes();
    while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() { i += 1; }
    if i < bytes.len() && bytes[i] == b'=' { i += 1; }
    while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() { i += 1; }
    if i >= bytes.len() || bytes[i] != b'{' { return None; }
    let begin = i;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if esc { esc = false; }
            else if c == '\\' { esc = true; }
            else if c == '"' { in_str = false; }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => { depth -= 1; if depth == 0 { i += 1; break; } }
                _ => {}
            }
        }
        i += 1;
    }
    if depth != 0 { return None; }
    let raw = &html[begin..i];
    let fixed = replace_undefined(raw);
    serde_json::from_str(&fixed).ok()
}

/// 抖音新版页面把 state 藏在 `\"state\":{...}` 转义字符串里
pub fn parse_state_from_page(page: &str) -> Option<Value> {
    let key = "\\\"state\\\":{";
    let idx = page.find(key)?;
    let start = idx + key.len() - 1; // 指向 '{'
    let bytes = page.as_bytes();
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'\\' { i += 2; continue; }
        match bytes[i] as char {
            '{' => depth += 1,
            '}' => { depth -= 1; if depth == 0 {
                let raw = &page[start..=i];
                let fixed = raw.replace("\\\"", "\"").replace("\\\\", "\\");
                return serde_json::from_str(&fixed).ok();
            } }
            _ => {}
        }
        i += 1;
    }
    None
}

/// 从文本中抓第一个 http(s) 链接
pub fn first_url(text: &str) -> Option<String> {
    let idx = text.find("http://").or_else(|| text.find("https://"))?;
    let rest = &text[idx..];
    let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | '，' | '。'))
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}
