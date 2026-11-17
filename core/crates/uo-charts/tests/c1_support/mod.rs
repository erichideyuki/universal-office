//! Helpers shared by the C-1 tests that read the TS captures (`c1_replay.rs`, `c1_memory.rs`): `gunzip`, a small JSON
//! reader for the capture's data lines and the host answers, and the hex printer.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::expect_used)]

pub mod corpus;

pub fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn gunzip(b: &[u8]) -> Vec<u8> {
    assert!(b.len() > 18 && b[0] == 0x1f && b[1] == 0x8b && b[3] == 0, "plain gzip from zlib.gzipSync");
    uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap()
}

// ------------------------------------------------------------------------------------------ JSON
#[derive(Clone, Debug)]
pub enum J {
    Null,
    Bool,
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }
    fn value(&mut self) -> J {
        self.ws();
        match self.b[self.i] {
            b'n' => {
                self.i += 4;
                J::Null
            }
            b't' => {
                self.i += 4;
                J::Bool
            }
            b'f' => {
                self.i += 5;
                J::Bool
            }
            b'"' => J::Str(self.string()),
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b']' {
                        self.i += 1;
                        break;
                    }
                    v.push(self.value());
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    }
                }
                J::Arr(v)
            }
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b'}' {
                        self.i += 1;
                        break;
                    }
                    let k = self.string();
                    self.ws();
                    self.i += 1; // ':'
                    let x = self.value();
                    v.push((k, x));
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    }
                }
                // the printer's special numbers
                if let [(k, J::Str(s))] = v.as_slice()
                    && k == "$num"
                {
                    return J::Num(match s.as_str() {
                        "NaN" => f64::NAN,
                        "Infinity" => f64::INFINITY,
                        _ => f64::NEG_INFINITY,
                    });
                }
                J::Obj(v)
            }
            _ => {
                let s = self.i;
                while self.i < self.b.len() && !matches!(self.b[self.i], b',' | b']' | b'}' | b' ' | b'\n') {
                    self.i += 1;
                }
                J::Num(uo_jsnum::parse::parse_float(std::str::from_utf8(&self.b[s..self.i]).unwrap()))
            }
        }
    }
    fn string(&mut self) -> String {
        self.i += 1;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let c = self.b[self.i];
            if c == b'"' {
                self.i += 1;
                break;
            }
            if c == b'\\' {
                let e = self.b[self.i + 1];
                self.i += 2;
                match e {
                    b'u' => {
                        let h = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
                        units.push(u16::from_str_radix(h, 16).unwrap());
                        self.i += 4;
                    }
                    b'n' => units.push(10),
                    b'r' => units.push(13),
                    b't' => units.push(9),
                    b'b' => units.push(8),
                    b'f' => units.push(12),
                    other => units.push(u16::from(other)),
                }
            } else {
                // a UTF-8 sequence
                let len = match c {
                    0..=0x7f => 1,
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    _ => 4,
                };
                let s = std::str::from_utf8(&self.b[self.i..self.i + len]).unwrap();
                units.extend(s.encode_utf16());
                self.i += len;
            }
        }
        String::from_utf16_lossy(&units)
    }
}

pub fn json(s: &str) -> J {
    P { b: s.as_bytes(), i: 0 }.value()
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(v) => v.iter().find(|(x, _)| x == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn s(&self) -> &str {
        match self {
            J::Str(s) => s,
            other => panic!("string expected: {other:?}"),
        }
    }
    pub fn n(&self) -> f64 {
        match self {
            J::Num(n) => *n,
            other => panic!("number expected: {other:?}"),
        }
    }
    pub fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            other => panic!("array expected: {other:?}"),
        }
    }
}
