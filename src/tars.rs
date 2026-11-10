//! 最小 TARS(JCE) 编解码，覆盖虎牙弹幕进房包与推送消息

use anyhow::{bail, Result};

const BYTE: u8 = 0;
const SHORT: u8 = 1;
const INT: u8 = 2;
const LONG: u8 = 3;
const FLOAT: u8 = 4;
const DOUBLE: u8 = 5;
const STRING1: u8 = 6;
const STRING4: u8 = 7;
const MAP: u8 = 8;
const LIST: u8 = 9;
const STRUCT_BEGIN: u8 = 10;
const STRUCT_END: u8 = 11;
const ZERO_TAG: u8 = 12;
const SIMPLE_LIST: u8 = 13;

#[derive(Default)]
pub struct TarsWriter { out: Vec<u8> }

impl TarsWriter {
    pub fn new() -> Self { Self::default() }
    fn head(&mut self, ty: u8, tag: u8) {
        if tag < 15 { self.out.push((tag << 4) | ty); }
        else { self.out.push((15 << 4) | ty); self.out.push(tag); }
    }
    pub fn byte(&mut self, b: i8, tag: u8) {
        if b == 0 { self.head(ZERO_TAG, tag); }
        else { self.head(BYTE, tag); self.out.push(b as u8); }
    }
    pub fn int(&mut self, n: i64, tag: u8) {
        if (-128..=127).contains(&n) { self.byte(n as i8, tag); }
        else if (-32768..=32767).contains(&n) {
            self.head(SHORT, tag);
            self.out.extend_from_slice(&(n as i16).to_be_bytes());
        } else if (i32::MIN as i64..=i32::MAX as i64).contains(&n) {
            self.head(INT, tag);
            self.out.extend_from_slice(&(n as i32).to_be_bytes());
        } else {
            self.head(LONG, tag);
            self.out.extend_from_slice(&n.to_be_bytes());
        }
    }
    pub fn boolean(&mut self, b: bool, tag: u8) { self.byte(if b { 1 } else { 0 }, tag); }
    pub fn string(&mut self, s: &str, tag: u8) {
        let bytes = s.as_bytes();
        if bytes.len() > 255 {
            self.head(STRING4, tag);
            self.out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        } else {
            self.head(STRING1, tag);
            self.out.push(bytes.len() as u8);
        }
        self.out.extend_from_slice(bytes);
    }
    pub fn bytes(&mut self, data: &[u8], tag: u8) {
        self.head(SIMPLE_LIST, tag);
        self.head(BYTE, 0);
        self.int(data.len() as i64, 0);
        self.out.extend_from_slice(data);
    }
    pub fn finish(&self) -> Vec<u8> { self.out.clone() }
}

#[derive(Clone, Copy)]
struct Head { ty: u8, tag: u8 }

pub struct TarsReader<'a> { data: &'a [u8], pos: usize }

impl<'a> TarsReader<'a> {
    pub fn new(data: &'a [u8]) -> Self { Self { data, pos: 0 } }
    fn read(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() { bail!("tars: read to end"); }
        let b = self.data[self.pos]; self.pos += 1; Ok(b)
    }
    fn read_head(&mut self) -> Result<Head> {
        let b = self.read()?;
        let mut h = Head { ty: b & 0x0F, tag: b >> 4 };
        if h.tag == 15 { h.tag = self.read()?; }
        Ok(h)
    }
    fn peek_head(&self) -> Result<(Head, usize)> {
        let mut tmp = TarsReader { data: self.data, pos: self.pos };
        let h = tmp.read_head()?;
        Ok((h, tmp.pos - self.pos))
    }
    fn skip(&mut self, n: usize) { self.pos = (self.pos + n).min(self.data.len()); }
    fn read_raw_int(&mut self, len: usize) -> Result<i64> {
        if self.pos + len > self.data.len() { bail!("tars: raw int overflow"); }
        let mut result = 0i64;
        if len == 1 {
            result = self.data[self.pos] as i64;
            if result >= 0x80 { result -= 0x100; }
        } else {
            for i in 0..len { result = (result << 8) | (self.data[self.pos + i] as i64); }
            if self.data[self.pos] & 0x80 != 0 { result -= 1i64 << (8 * len); }
        }
        self.pos += len;
        Ok(result)
    }
    fn skip_field_with_type(&mut self, ty: u8) -> Result<()> {
        match ty {
            BYTE => self.skip(1),
            SHORT | FLOAT => self.skip(2),
            INT => self.skip(4),
            LONG | DOUBLE => self.skip(8),
            STRING1 => { let n = self.read()? as usize; self.skip(n); }
            STRING4 => { let n = self.read_raw_int(4)? as usize; self.skip(n); }
            MAP => { let n = self.read_int(0)?; for _ in 0..(n * 2) { self.skip_field()?; } }
            LIST => { let n = self.read_int(0)?; for _ in 0..n { self.skip_field()?; } }
            SIMPLE_LIST => {
                self.read_head()?;
                let n = self.read_int(0)?;
                self.skip(n as usize);
            }
            STRUCT_BEGIN => self.skip_to_struct_end()?,
            _ => {}
        }
        Ok(())
    }
    fn skip_field(&mut self) -> Result<()> {
        let h = self.read_head()?;
        self.skip_field_with_type(h.ty)
    }
    fn skip_to_struct_end(&mut self) -> Result<()> {
        loop {
            let h = self.read_head()?;
            self.skip_field_with_type(h.ty)?;
            if h.ty == STRUCT_END { break; }
        }
        Ok(())
    }
    fn skip_to_tag(&mut self, tag: u8) -> bool {
        loop {
            let (h, len) = match self.peek_head() { Ok(v) => v, Err(_) => return false };
            if tag <= h.tag || h.ty == STRUCT_END { return tag == h.tag; }
            self.skip(len);
            if self.skip_field_with_type(h.ty).is_err() { return false; }
        }
    }

    pub fn read_int(&mut self, tag: u8) -> Result<i64> {
        if !self.skip_to_tag(tag) { return Ok(0); }
        let h = self.read_head()?;
        match h.ty {
            ZERO_TAG => Ok(0),
            BYTE => self.read_raw_int(1),
            SHORT => self.read_raw_int(2),
            INT => self.read_raw_int(4),
            LONG => self.read_raw_int(8),
            _ => bail!("tars: type mismatch int"),
        }
    }
    pub fn read_string(&mut self, tag: u8) -> Result<String> {
        if !self.skip_to_tag(tag) { return Ok(String::new()); }
        let h = self.read_head()?;
        let len = match h.ty {
            STRING1 => self.read()? as usize,
            STRING4 => u32::from_be_bytes(self.data.get(self.pos..self.pos + 4).ok_or_else(|| anyhow::anyhow!("tars: str4"))?.try_into()?) as usize,
            _ => bail!("tars: type mismatch string"),
        };
        if h.ty == STRING4 { self.pos += 4; }
        if self.pos + len > self.data.len() { bail!("tars: string overflow"); }
        let s = String::from_utf8_lossy(&self.data[self.pos..self.pos + len]).to_string();
        self.pos += len;
        Ok(s)
    }
    pub fn read_bytes(&mut self, tag: u8) -> Result<Vec<u8>> {
        if !self.skip_to_tag(tag) { return Ok(vec![]); }
        let h = self.read_head()?;
        match h.ty {
            SIMPLE_LIST => {
                self.read_head()?;
                let n = self.read_int(0)? as usize;
                if self.pos + n > self.data.len() { bail!("tars: bytes overflow"); }
                let out = self.data[self.pos..self.pos + n].to_vec();
                self.pos += n;
                Ok(out)
            }
            LIST => {
                let n = self.read_int(0)? as usize;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n { out.push(self.read_int(0)? as u8); }
                Ok(out)
            }
            _ => bail!("tars: type mismatch bytes"),
        }
    }
    pub fn read_struct<F>(&mut self, tag: u8, body: F) -> Result<bool>
    where F: FnOnce(&mut TarsReader<'a>) -> Result<()> {
        if !self.skip_to_tag(tag) { return Ok(false); }
        let h = self.read_head()?;
        if h.ty != STRUCT_BEGIN { bail!("tars: type mismatch struct"); }
        body(self)?;
        self.skip_to_struct_end()?;
        Ok(true)
    }
}

/// 虎牙推送消息：pushType=0, uri=1, msg=2, protocolType=3（顶层字段）
pub fn parse_hy_push(data: &[u8]) -> Result<(i64, Vec<u8>)> {
    let mut s = TarsReader::new(data);
    let _push_type = s.read_int(0)?;
    let uri = s.read_int(1)?;
    let msg = s.read_bytes(2)?;
    Ok((uri, msg))
}

/// 虎牙弹幕内容：userInfo=struct(0)、content=3、bulletFormat=struct(6)
pub fn parse_hy_message(data: &[u8]) -> Result<(String, String, i64)> {
    let mut s = TarsReader::new(data);
    let mut nick = String::new();
    s.read_struct(0, |st| {
        st.read_int(0)?;
        st.read_int(0)?;
        nick = st.read_string(2)?;
        Ok(())
    })?;
    let content = s.read_string(3)?;
    let mut color = 0i64;
    s.read_struct(6, |st| { color = st.read_int(0)?; Ok(()) })?;
    Ok((nick, content, color))
}
