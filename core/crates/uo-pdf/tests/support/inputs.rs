//! The inputs of the scalar facets, generated as `capture_writer.mjs` generates them (the same
//! xorshift32 and the same order of draws).

use uo_testkit::generate::Rng;

fn unit(r: &mut Rng) -> f64 {
    f64::from(r.next_u32()) / 4_294_967_296.0
}

fn double_from_bits(r: &mut Rng) -> f64 {
    let lo = u64::from(r.next_u32());
    let hi = u64::from(r.next_u32());
    f64::from_bits(lo | (hi << 32))
}

pub fn fmt_inputs() -> Vec<f64> {
    let mut out = vec![
        0.0,
        -0.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e9,
        -1e9,
        1e9 + 0.5,
        1e10,
        0.0005,
        0.0015,
        -0.0005,
        0.0004999999999999999,
        0.0025,
        1.0005,
        2.5,
        -2.5,
        0.1 + 0.2,
        1.0 / 3.0,
        123456.78912,
        5e-324,
        1e300,
        -1e300,
        999999999.9996,
        0.9995,
        0.99949999,
    ];
    let mut r = Rng::new(0x5eed);
    for _ in 0..20000 {
        out.push(double_from_bits(&mut r));
    }
    for _ in 0..20000 {
        let a = r.next_u32() % 2_000_001;
        let tie = if r.below(3) == 0 { 0.0005 } else { 0.0 };
        out.push((f64::from(a) - 1_000_000.0) / 1000.0 + tie);
    }
    for _ in 0..20000 {
        out.push(unit(&mut r) * 2000.0 - 1000.0);
    }
    for _ in 0..5000 {
        let a = r.next_u32() % 4_000_000_000;
        let scale = if r.below(2) == 0 { 1.0 } else { 0.001 };
        out.push((f64::from(a) - 2_000_000_000.0) * scale);
    }
    out
}

const HEX_PIECES: [&str; 25] = [
    "#",
    "0",
    "9",
    "a",
    "F",
    "g",
    "x",
    " ",
    "-",
    "+",
    "\t",
    "\u{e9}",
    "\u{1F600}",
    "au",
    "FF",
    "12",
    "zz",
    "0x",
    "\u{a0}",
    "\u{feff}",
    "\u{2003}",
    "E",
    "e",
    ".",
    "#",
];

pub fn hex_inputs() -> Vec<String> {
    let mut out: Vec<String> = [
        "",
        "#",
        "auto",
        "#FF8800",
        "ff8800",
        "#ff00",
        "abcdef12",
        " ff0000",
        "-1 1 1",
        "# f0f0f",
        "0x1020 30",
        "#12#34#56",
        "\u{e9}1\u{e9}2\u{e9}3",
        "\u{1F600}\u{1F600}\u{1F600}",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let mut r = Rng::new(0xc010);
    for _ in 0..3000 {
        let mut s = String::new();
        for _ in 0..r.below(8) {
            s.push_str(HEX_PIECES[r.below(HEX_PIECES.len())]);
        }
        out.push(s);
    }
    out
}

const URI_SCHEMES: [&str; 7] = ["http://", "https://", "HTTPS://", "mailto:", "MailTo:", "ftp://", "javascript:"];
const URI_PIECES: [&str; 37] = [
    "http://",
    "https://",
    "HTTPS://",
    "mailto:",
    "MailTo:",
    "ftp://",
    "javascript:",
    " ",
    "\t",
    "\u{a0}",
    "\u{feff}",
    "\u{2003}",
    "a",
    "Z",
    "0",
    "(",
    ")",
    "\\",
    "\"",
    "<",
    ">",
    "%",
    "\u{e9}",
    "\u{20ac}",
    "\u{1F600}",
    "\u{85}",
    "\u{180e}",
    "\u{200b}",
    "\n",
    "\u{7f}",
    ".",
    "/",
    "?",
    "#",
    "&",
    "K",
    "\u{17f}",
];

pub fn uri_inputs() -> Vec<String> {
    let mut out: Vec<String> = [
        "",
        " ",
        "http://",
        "mailto:",
        "http:/x",
        "HtTp://a b",
        "  https://x  ",
        "https://example.com/path?q=a b&c=(d)",
        "\u{17f}http://x",
        "htKp://x",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let mut r = Rng::new(0x0ff1);
    for _ in 0..4000 {
        let mut s = if r.below(3) == 0 { String::new() } else { URI_SCHEMES[r.below(URI_SCHEMES.len())].to_owned() };
        for _ in 0..r.below(10) {
            s.push_str(URI_PIECES[r.below(URI_PIECES.len())]);
        }
        out.push(s);
    }
    out
}
