//! Shared by the `g_*.rs` parity tests of frontier G: the pools and generators that mirror
//! `tools/capture-g.mjs`, the canonical stream helpers and the capture loader. Every generator here
//! consumes the `Rng` in the same order as its JS twin.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use uo_jsnum::conv::canonicalize_nan;
use uo_render::types::PathCmd;
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec, Stream};

pub const NAN: f64 = f64::NAN;
pub const INF: f64 = f64::INFINITY;

pub const GOOD: [f64; 19] =
    [0.0, 1.0, 2.0, 5.0, 10.0, 16.5, 50.0, 100.0, 150.25, 240.0, 720.0, 1440.0, 3000.0, 5760.0, 11906.0, 0.5, 0.25, 7.0, 33.333];
pub const BAD: [f64; 11] = [-0.0, -1.0, -5.5, 1e-7, 1e9, 1e15, NAN, INF, -INF, 1e308, -1e308];
pub const INF_NEG: f64 = f64::NEG_INFINITY;

pub const COLORS: [&str; 20] = [
    "FF0000", "00FF00", "0000FF", "FFFFFF", "000000", "123456", "abcdef", "ABCDEF", "", "zz", "12", "-1f0a0b", " 5a5a5a", "0x1234", "ff",
    "+f+f+f", "7f7f7f", "ffffff00", "FF00", "8080GG",
];
pub const VALID: [&str; 6] = ["000000", "FF0000", "808080", "12AB34", "FFFFFF", "00B050"];
pub const POS: [f64; 10] = [0.0, 1.0, 0.5, 0.25, 0.75, -0.5, 1.5, NAN, 0.3333, 0.999];

pub fn pick<T: Copy>(r: &mut Rng, a: &[T]) -> T {
    let i = r.below(a.len());
    a[i]
}

pub fn num(r: &mut Rng) -> f64 {
    num_bad(r, 12)
}

pub fn num_bad(r: &mut Rng, bad: usize) -> f64 {
    if r.below(100) < bad { pick_bad(r) } else { pick(r, &GOOD) }
}

fn pick_bad(r: &mut Rng) -> f64 {
    pick(r, &BAD)
}

pub fn boolean(r: &mut Rng) -> bool {
    r.below(2) == 1
}

pub fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn f(s: &mut Stream, v: f64) {
    s.f64(canonicalize_nan(v));
}

pub fn dig(build: impl FnOnce(&mut Stream)) -> String {
    let mut s = Stream::new();
    build(&mut s);
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

pub fn cmd_out(s: &mut Stream, cmds: &[PathCmd]) {
    s.u32(u32::try_from(cmds.len()).unwrap());
    for c in cmds {
        match *c {
            PathCmd::M { x, y } => {
                s.tag(b'M');
                f(s, x);
                f(s, y);
            }
            PathCmd::L { x, y } => {
                s.tag(b'L');
                f(s, x);
                f(s, y);
            }
            PathCmd::C { x1, y1, x2, y2, x, y } => {
                s.tag(b'C');
                for v in [x1, y1, x2, y2, x, y] {
                    f(s, v);
                }
            }
            PathCmd::Z => {
                s.tag(b'Z');
            }
        }
    }
}

/// The capture, checked against the platform the replay needs: V8 13.6, x64 (math) and, for the
/// facets that call `pow`, the glibc environment of the pow reference.
pub fn capture(text: &str) -> Capture {
    let cap = Capture::parse(text).unwrap();
    let spec = PlatformSpec {
        v8_prefix: Some("13.6.233.17".into()),
        arch: Some("x64".into()),
        os: Some("linux".into()),
        glibc_prefix: Some("2.39".into()),
        tunables_contain: Some("glibc.cpu.hwcaps=-FMA,-AVX2".into()),
        flags: Some("none".into()),
    };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    cap
}

pub fn u32n(n: usize) -> u32 {
    u32::try_from(n).unwrap()
}
