//! 极简 protobuf 读写（覆盖抖音 / 快手弹幕所需：varint 与 length-delimited）

#[derive(Debug, Clone)]
pub enum PbVal {
    Varint(u64),
    Fixed64([u8; 8]),
    Bytes(Vec<u8>),
    Fixed32([u8; 4]),
}

#[derive(Debug, Clone)]
pub struct Pb {
    pub fields: Vec<(u32, u8, PbVal)>,
}

impl Pb {
    pub fn parse(data: &[u8]) -> Pb {
        let mut fields = Vec::new();
        let mut pos = 0usize;
        while pos < data.len() {
            let (tag, n) = match read_varint(data, pos) { Some(v) => v, None => break };
            pos += n;
            let field = (tag >> 3) as u32;
            let wire = (tag & 7) as u8;
            match wire {
                0 => {
                    let (v, n) = match read_varint(data, pos) { Some(v) => v, None => break };
                    pos += n;
                    fields.push((field, wire, PbVal::Varint(v)));
                }
                1 => {
                    if pos + 8 > data.len() { break; }
                    let mut b = [0u8; 8]; b.copy_from_slice(&data[pos..pos + 8]); pos += 8;
                    fields.push((field, wire, PbVal::Fixed64(b)));
                }
                2 => {
                    let (len, n) = match read_varint(data, pos) { Some(v) => v, None => break };
                    pos += n;
                    let len = len as usize;
                    if pos + len > data.len() { break; }
                    fields.push((field, wire, PbVal::Bytes(data[pos..pos + len].to_vec())));
                    pos += len;
                }
                5 => {
                    if pos + 4 > data.len() { break; }
                    let mut b = [0u8; 4]; b.copy_from_slice(&data[pos..pos + 4]); pos += 4;
                    fields.push((field, wire, PbVal::Fixed32(b)));
                }
                _ => break,
            }
        }
        Pb { fields }
    }

    pub fn int(&self, f: u32) -> u64 {
        self.fields.iter().find(|(n, w, _)| *n == f && *w == 0)
            .and_then(|(_, _, v)| if let PbVal::Varint(x) = v { Some(*x) } else { None })
            .unwrap_or(0)
    }
    pub fn str(&self, f: u32) -> String {
        String::from_utf8_lossy(&self.bytes(f)).to_string()
    }
    pub fn bytes(&self, f: u32) -> Vec<u8> {
        self.fields.iter().find(|(n, w, _)| *n == f && *w == 2)
            .and_then(|(_, _, v)| if let PbVal::Bytes(x) = v { Some(x.clone()) } else { None })
            .unwrap_or_default()
    }
    pub fn msgs(&self, f: u32) -> Vec<Pb> {
        self.fields.iter().filter(|(n, w, _)| *n == f && *w == 2)
            .filter_map(|(_, _, v)| if let PbVal::Bytes(x) = v { Some(Pb::parse(x)) } else { None })
            .collect()
    }
    pub fn has(&self, f: u32) -> bool { self.fields.iter().any(|(n, _, _)| *n == f) }
}

fn read_varint(data: &[u8], pos: usize) -> Option<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0u32;
    let mut i = pos;
    while i < data.len() && shift < 64 {
        let b = data[i];
        i += 1;
        result |= ((b & 0x7F) as u64).checked_shl(shift).unwrap_or(0);
        if b & 0x80 == 0 { return Some((result, i - pos)); }
        shift += 7;
    }
    None
}

#[derive(Default)]
pub struct PbWriter { out: Vec<u8> }

impl PbWriter {
    pub fn new() -> Self { Self::default() }

    pub fn varint(&mut self, mut v: u64) -> &mut Self {
        loop {
            if v & !0x7F == 0 { self.out.push(v as u8); break; }
            self.out.push(((v & 0x7F) | 0x80) as u8);
            v >>= 7;
        }
        self
    }
    pub fn tag(&mut self, field: u32, wire: u8) -> &mut Self {
        self.varint(((field << 3) | wire as u32) as u64)
    }
    pub fn str(&mut self, field: u32, value: &str) -> &mut Self {
        self.bytes(field, value.as_bytes())
    }
    pub fn bytes(&mut self, field: u32, value: &[u8]) -> &mut Self {
        self.tag(field, 2).varint(value.len() as u64);
        self.out.extend_from_slice(value);
        self
    }
    pub fn uint64(&mut self, field: u32, value: u64) -> &mut Self {
        self.tag(field, 0).varint(value)
    }
    pub fn finish(&self) -> Vec<u8> { self.out.clone() }
}
