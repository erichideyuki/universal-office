//! Shared by the replay tests: the capture, the locales of `js/capture-numfmt.mjs` (mirrored by name),
//! the value/option decoders and the encoders of the results (the same text the script writes).
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use uo_jsnum::conv::canonicalize_nan;
use uo_jsnum::json::push_string_utf16;
use uo_numfmt::{FmtPart, FormatLocale, FormatOptions, FormatParts, Value, pt_br};
use uo_testkit::{Capture, PlatformSpec};

pub const CAPTURE: &str = include_str!("../captures/numfmt.capture");

/// The capture, after checking where it was taken (V8 13.6.233.17 on x64: `log10` and `pow` are
/// the fdlibm bits of that V8).
pub fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
    let spec = PlatformSpec { v8_prefix: Some("13.6.233.17".into()), arch: Some("x64".into()), ..PlatformSpec::default() };
    cap.require_platform(&spec).unwrap();
    cap
}

pub fn units_of(s: &str) -> Vec<u16> {
    if s == "-" {
        return Vec::new();
    }
    s.split(',').map(|h| u16::from_str_radix(h, 16).unwrap()).collect()
}

pub fn f64_of(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).unwrap())
}

pub fn f64_hex(v: f64) -> String {
    format!("{:016x}", canonicalize_nan(v).to_bits())
}

/// A decoded value: `n<f64 bits>` or `s<units>`.
pub enum Val {
    Num(f64),
    Str(Vec<u16>),
}

impl Val {
    pub fn parse(s: &str) -> Val {
        match s.split_at(1) {
            ("n", h) => Val::Num(f64_of(h)),
            ("s", u) => Val::Str(units_of(u)),
            _ => panic!("bad value {s}"),
        }
    }
    pub fn value(&self) -> Value<'_> {
        match self {
            Val::Num(x) => Value::Num(*x),
            Val::Str(s) => Value::Str(s),
        }
    }
}

fn strs<const N: usize>(f: impl Fn(usize) -> String) -> [String; N] {
    std::array::from_fn(f)
}

fn english() -> FormatLocale {
    let a = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let ms = a(&["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]);
    let ml = a(&["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]);
    let ds = a(&["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]);
    let dl = a(&["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]);
    FormatLocale {
        decimal: ".".into(),
        group: ",".into(),
        months_short: strs(|i| ms[i].clone()),
        months_long: strs(|i| ml[i].clone()),
        days_short: strs(|i| ds[i].clone()),
        days_long: strs(|i| dl[i].clone()),
        ampm: ["AM".into(), "PM".into()],
        builtins: Vec::new(),
        sys_date: Some("dddd, mmmm d, yyyy".into()),
        sys_time: Some("h:mm:ss AM/PM".into()),
    }
}

fn odd() -> FormatLocale {
    FormatLocale {
        decimal: "::".into(),
        group: "'".into(),
        months_short: strs(|i| format!("m{}", i + 1)),
        months_long: strs(|i| format!("month-{}", i + 1)),
        days_short: strs(|i| format!("d{i}")),
        days_long: strs(|i| format!("day-{i}")),
        ampm: ["vm".into(), "Nm".into()],
        builtins: vec![(14, "dd-mm".into()), (5, "0.0".into())],
        sys_date: Some("yyyy\"/\"mm".into()),
        sys_time: Some("[$-409]hh".into()),
    }
}

/// The locales of the capture by name (`default` = no locale = PT_BR).
pub struct Locales {
    ptclone: FormatLocale,
    en: FormatLocale,
    odd: FormatLocale,
    blank: FormatLocale,
}

impl Locales {
    pub fn new() -> Locales {
        let en = english();
        let blank = FormatLocale { decimal: String::new(), group: String::new(), ..en.clone() };
        Locales { ptclone: pt_br().clone(), en, odd: odd(), blank }
    }
    pub fn get(&self, name: &str) -> Option<&FormatLocale> {
        match name {
            "default" => None,
            "ptbr" => Some(pt_br()),
            "ptclone" => Some(&self.ptclone),
            "en" => Some(&self.en),
            "odd" => Some(&self.odd),
            "blank" => Some(&self.blank),
            _ => panic!("unknown locale {name}"),
        }
    }
}

/// `d1904 showZeros [,width bits]`
pub fn opts_of<'a>(s: &str, locale: Option<&'a FormatLocale>) -> FormatOptions<'a> {
    let b = s.as_bytes();
    FormatOptions {
        locale,
        date1904: b.first() == Some(&b'1'),
        show_zeros: b.get(1) != Some(&b'0'),
        col_width_chars: s.split_once(',').map(|(_, h)| f64_of(h)),
    }
}

pub fn json(units: &[u16]) -> String {
    let mut s = String::new();
    push_string_utf16(&mut s, units);
    s
}

pub fn parts_text(r: &FormatParts) -> String {
    let ps: Vec<String> = r
        .parts
        .iter()
        .map(|p| match p {
            FmtPart::Text(s) => format!("T{}", json(s)),
            FmtPart::Skip(c) => format!("S{c}"),
            FmtPart::Fill(c) => format!("F{c}"),
            FmtPart::General(v) => format!("G{}", f64_hex(*v)),
        })
        .collect();
    let mut s = ps.join(" ");
    if let Some(c) = &r.color {
        s.push('!');
        s.push_str(c);
    }
    if r.overflow {
        s.push('#');
    }
    s
}
