//! A small JSON reader for the op scripts of the capture (numbers go through `Number()` of
//! uo-jsnum, as JS reads them).

use uo_jsnum::parse::to_number;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            other => panic!("not an array: {other:?}"),
        }
    }

    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }

    pub fn bool(&self) -> bool {
        match self {
            Json::Bool(b) => *b,
            other => panic!("not a bool: {other:?}"),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A number as the scripts write it: a JSON number, or one of the strings of the specials.
    pub fn num(&self) -> f64 {
        match self {
            Json::Num(n) => *n,
            Json::Str(s) => match s.as_str() {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                "-0" => -0.0,
                other => panic!("not a number: {other}"),
            },
            other => panic!("not a number: {other:?}"),
        }
    }

    pub fn nums(&self) -> Vec<f64> {
        self.arr().iter().map(Json::num).collect()
    }

    pub fn uint(&self) -> u32 {
        let n = self.num();
        assert!(n >= 0.0 && n.fract() == 0.0 && n < 4_294_967_296.0, "not a u32: {n}");
        n as u32
    }

    pub fn int(&self) -> i64 {
        let n = self.num();
        assert!(n.fract() == 0.0 && n.abs() < 9e15, "not an integer: {n}");
        n as i64
    }
}

pub fn parse(text: &str) -> Json {
    let mut p = Parser { b: text.as_bytes(), i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, p.b.len(), "trailing JSON");
    v
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        assert_eq!(self.b.get(self.i), Some(&c), "expected {} at {}", char::from(c), self.i);
        self.i += 1;
    }

    fn value(&mut self) -> Json {
        self.ws();
        match self.b.get(self.i) {
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Json::Arr(v);
                }
                loop {
                    v.push(self.value());
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Json::Arr(v);
                        }
                        other => panic!("bad array separator {other:?}"),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Json::Obj(v);
                }
                loop {
                    self.ws();
                    let Json::Str(k) = self.value() else { panic!("object key") };
                    self.ws();
                    self.eat(b':');
                    let val = self.value();
                    v.push((k, val));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Json::Obj(v);
                        }
                        other => panic!("bad object separator {other:?}"),
                    }
                }
            }
            Some(b'"') => Json::Str(self.string()),
            Some(b't') => {
                self.i += 4;
                Json::Bool(true)
            }
            Some(b'f') => {
                self.i += 5;
                Json::Bool(false)
            }
            Some(b'n') => {
                self.i += 4;
                Json::Null
            }
            _ => {
                let start = self.i;
                while matches!(self.b.get(self.i), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.b[start..self.i]).unwrap();
                assert!(!text.is_empty(), "unexpected byte at {start}");
                Json::Num(to_number(text))
            }
        }
    }

    fn hex4(&mut self) -> u32 {
        let h = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
        self.i += 4;
        u32::from_str_radix(h, 16).unwrap()
    }

    fn string(&mut self) -> String {
        self.eat(b'"');
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.b[self.i];
                    self.i += 1;
                    let ch = match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'/' => '/',
                        b'\\' => '\\',
                        b'"' => '"',
                        b'u' => {
                            let hi = self.hex4();
                            if (0xd800..0xdc00).contains(&hi) && self.b.get(self.i) == Some(&b'\\') && self.b.get(self.i + 1) == Some(&b'u')
                            {
                                self.i += 2;
                                let lo = self.hex4();
                                char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)).unwrap()
                            } else {
                                char::from_u32(hi).expect("lone surrogate in a script")
                            }
                        }
                        other => panic!("bad escape {other}"),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                other => out.push(other),
            }
        }
        String::from_utf8(out).unwrap()
    }
}
