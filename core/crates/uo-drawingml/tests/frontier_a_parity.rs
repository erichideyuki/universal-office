//! Parity replay of frontier A against `tests/captures/frontier-a.capture`, written by
//! `tools/capture-frontier-a.mjs` from the frozen TS (`shared/drawingml/dist`,
//! `write/packages/docx-layout/dist/images.js`, Node 24.15.0 linux-x64 with the FMA variant of
//! glibc's `pow` masked, the pow reference of the uo-jsnum gate): the colour modifiers over 20,000
//! generated sequences, the sRGB curve over ~41,000 values, the preset patterns, the four image
//! adjustments over 57 generated images and the 25 x 25 brightness/contrast grid, and the four
//! Word blip functions. The inputs are regenerated here exactly as in the script (the testkit
//! `Rng`, exact arithmetic only); the outputs (NaN canonicalized, `-0` distinguished, buffers as
//! bytes) are hashed with the same canonical stream and compared digest by digest.
//!
//! `apply_picture_adjust` is compared with the `Uint8ClampedArray` run of the TS (Rust clamps every
//! store, G7); the script already proved that the `Uint8Array` run (the PDF path wraps) is equal
//! wherever no store leaves 0..=255, and counts the others in its metadata.
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_drawingml::{
    ClrChange, ColorModKind, ImageEffects, Limits, PictureAdjust, adjust_brightness_contrast, adjust_saturation, adjust_sharpen,
    adjust_temperature, apply_color_mods, apply_image_effects, apply_picture_adjust, effect_decode, effect_line, lin_to_srgb,
    pattern_density, pattern_rows, preset_patterns, srgb_to_lin,
};
use uo_jsnum::conv::canonicalize_nan;
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec, Replay, Stream};

const CAPTURE: &str = include_str!("captures/frontier-a.capture");

fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
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

fn f(s: &mut Stream, v: f64) {
    s.f64(canonicalize_nan(v));
}

fn summary(s: Stream) -> String {
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

fn rng_f(rng: &mut Rng) -> f64 {
    f64::from(rng.next_u32()) / 4_294_967_296.0
}

fn below(rng: &mut Rng, n: usize) -> u32 {
    u32::try_from(rng.below(n)).unwrap()
}

// ── colour ──────────────────────────────────────────────────────────────────────────────────────

const CH_PALETTE: [f64; 17] = [
    0.0,
    1.0,
    0.5,
    0.25,
    0.75,
    0.04045,
    0.0031308,
    0.0031309,
    0.04046,
    0.999999,
    1e-9,
    0.2126,
    0.7152,
    0.0722,
    2.0 / 3.0,
    1.0 / 3.0,
    1.0 / 6.0,
];
const CH_EDGE: [f64; 9] = [-0.0, -0.25, 1.5, 2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308, 5e-324];
const MOD_NAMES: [&str; 20] = [
    "gamma", "invGamma", "comp", "inv", "gray", "shade", "tint", "lumMod", "lumOff", "lum", "satMod", "satOff", "sat", "hueMod", "hueOff",
    "hue", "red", "blueMod", "bogus", "",
];

fn gen_channel(rng: &mut Rng, prev: f64) -> f64 {
    let k = rng.below(20);
    if k < 10 {
        rng_f(rng)
    } else if k < 16 {
        CH_PALETTE[rng.below(CH_PALETTE.len())]
    } else if k < 18 {
        prev
    } else if k == 18 {
        CH_EDGE[rng.below(CH_EDGE.len())]
    } else {
        1.0 - rng_f(rng)
    }
}

fn gen_raw(rng: &mut Rng) -> Option<f64> {
    let m = rng.below(24);
    Some(match m {
        0 => return None,
        1 => f64::NAN,
        2 => f64::INFINITY,
        3 => f64::NEG_INFINITY,
        4 => 0.0,
        5 => -0.0,
        6 => 100_000.0,
        7 => 50_000.0,
        8 => 75_000.0,
        9 => 25_000.0,
        10 => 200_000.0,
        11 => -50_000.0,
        12 => 1e9,
        13 => 1e-9,
        14 => 12_345.678,
        15 => 21_600_000.0,
        16 => 5_400_000.0,
        17 => 1e308,
        18..=20 => f64::from(below(rng, 300_001)),
        _ => (rng_f(rng) - 0.5) * 400_000.0,
    })
}

struct Sequence {
    c: [f64; 3],
    mods: Vec<(&'static str, Option<f64>)>,
}

fn gen_sequence(i: u32) -> Sequence {
    let mut rng = Rng::new(0xc010_0000 + i);
    let c0 = gen_channel(&mut rng, 0.5);
    let c1 = gen_channel(&mut rng, c0);
    let c2 = gen_channel(&mut rng, c1);
    let n = rng.below(7);
    let mut mods = Vec::new();
    for _ in 0..n {
        let name = MOD_NAMES[rng.below(MOD_NAMES.len())];
        mods.push((name, gen_raw(&mut rng)));
    }
    Sequence { c: [c0, c1, c2], mods }
}

fn ulp_step(v: f64, k: i64) -> f64 {
    let bits = i64::try_from(v.to_bits()).unwrap() + k;
    f64::from_bits(u64::try_from(bits).unwrap())
}

fn lin_inputs() -> Vec<f64> {
    let mut v: Vec<f64> = (0..=1000).map(|i| f64::from(i) / 1000.0).collect();
    for base in [0.04045, 0.0031308] {
        for k in -4..=4 {
            v.push(ulp_step(base, k));
        }
    }
    v.extend([0.0, -0.0, 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 2.0, 1e-320, 1e308, 5e-324, 0.5]);
    let mut rng = Rng::new(0x1111_1111);
    for _ in 0..20_000 {
        v.push(rng_f(&mut rng));
    }
    let mut rb = Rng::new(0x2222_2222);
    for _ in 0..20_000 {
        let hi = u64::from(rb.next_u32());
        let lo = u64::from(rb.next_u32());
        v.push(f64::from_bits((hi << 32) | lo));
    }
    v
}

#[test]
fn colour_modifiers_match_the_ts_bit_for_bit() {
    let cap = capture().subset(|k| k.starts_with("color/"));
    let mut replay = Replay::new(&cap);
    const SEQUENCES: u32 = 20_000;
    const BLOCK: u32 = 500;
    for b in 0..SEQUENCES / BLOCK {
        let mut s = Stream::new();
        for i in b * BLOCK..(b + 1) * BLOCK {
            let q = gen_sequence(i);
            let mods: Vec<(ColorModKind, Option<f64>)> =
                q.mods.iter().filter_map(|&(n, raw)| ColorModKind::from_name(n).map(|k| (k, raw))).collect();
            for v in apply_color_mods(q.c, &mods) {
                f(&mut s, v);
            }
        }
        replay.check("color/mods", &format!("b{b}"), &summary(s));
    }
    let inputs = lin_inputs();
    for (b, block) in inputs.chunks(5000).enumerate() {
        let mut s = Stream::new();
        for &x in block {
            f(&mut s, srgb_to_lin(x));
            f(&mut s, lin_to_srgb(x));
        }
        replay.check("color/lin", &format!("b{b}"), &summary(s));
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

// ── patterns ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn patterns_match_the_ts() {
    let cap = capture().subset(|k| k == "patterns");
    let mut replay = Replay::new(&cap);
    let mut s = Stream::new();
    for (name, rows) in preset_patterns() {
        s.str(name);
        assert_eq!(pattern_rows(name), Some(*rows));
        for b in rows {
            s.u32(u32::from(*b));
        }
        f(&mut s, pattern_density(name));
        let hex: String = rows.iter().map(|b| format!("{b:02x}")).collect();
        s.str(&hex);
    }
    replay.check("patterns", "all", &summary(s));
    let mut k = Stream::new();
    for (name, _) in preset_patterns() {
        k.str(name);
    }
    replay.check("patterns", "keys", &summary(k));
    let mut u = Stream::new();
    for name in ["", "Pct5", " pct5", "pct5 ", "nope", "zigZagX", "PCT5", "pct", "horz\u{0}", "5pct"] {
        match pattern_rows(name) {
            None => {
                u.tag(b'n');
            }
            Some(rows) => {
                u.tag(b'r');
                for b in rows {
                    u.u32(u32::from(b));
                }
            }
        }
        f(&mut u, pattern_density(name));
    }
    replay.check("patterns", "unknown", &summary(u));
    let report = replay.finish();
    report.assert_clean();
    for name in ["constructor", "__proto__", "toString", "hasOwnProperty", "valueOf"] {
        assert_eq!(pattern_rows(name), None, "{name} is an unknown key (G7: the TS throws a TypeError)");
        assert!((pattern_density(name) - 0.5).abs() < f64::EPSILON);
    }
    assert!(capture().meta("patterns-prototype-names").unwrap().contains("throws_TypeError"));
}

// ── adjust ──────────────────────────────────────────────────────────────────────────────────────

fn gen_image(kind: u32, w: usize, h: usize, seed: u32) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let mut d = vec![0u8; w * h * 4];
    let wm = (w - 1).max(1);
    let hm = (h - 1).max(1);
    let c0 = [below(&mut rng, 256), below(&mut rng, 256), below(&mut rng, 256)];
    let byte = |v: usize| u8::try_from(v & 255).unwrap();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            match kind {
                0 => {
                    d[i] = byte(x * 255 / wm);
                    d[i + 1] = byte(y * 255 / hm);
                    d[i + 2] = byte(x * 37 + y * 91);
                    d[i + 3] = 255;
                }
                1 => {
                    for c in 0..4 {
                        d[i + c] = u8::try_from(rng.below(256)).unwrap();
                    }
                }
                2 => {
                    if seed & 1 == 1 {
                        for c in 0..3 {
                            d[i + c] = u8::try_from(c0[c]).unwrap();
                        }
                    } else {
                        for c in 0..3 {
                            d[i + c] = u8::try_from(c0[0]).unwrap();
                        }
                    }
                    d[i + 3] = 255;
                }
                3 => {
                    let col = if y % 5 == 4 {
                        [250, 250, 250]
                    } else if x * 2 < w {
                        [200, 30, 30]
                    } else {
                        [30, 180, 40]
                    };
                    d[i..i + 3].copy_from_slice(&col);
                    d[i + 3] = 200;
                }
                _ => {
                    let v = (x * x + y * 3 + usize::try_from(seed).unwrap()) % 256;
                    d[i] = byte(v);
                    d[i + 1] = byte(v * 5 + x);
                    d[i + 2] = byte(255 - v + y);
                    d[i + 3] = byte(255 - ((x + y) & 7));
                }
            }
        }
    }
    d
}

const IMG_SIZES: [(usize, usize); 11] = [(1, 1), (2, 2), (2, 3), (3, 2), (5, 5), (7, 3), (9, 9), (16, 16), (33, 17), (64, 48), (130, 70)];

struct Img {
    kind: u32,
    w: usize,
    h: usize,
    seed: u32,
}

fn image_list() -> Vec<Img> {
    let mut list = Vec::new();
    let mut n = 0u32;
    for (w, h) in IMG_SIZES {
        for kind in 0..5 {
            list.push(Img { kind, w, h, seed: 0xad00_0000 + n });
            n += 1;
        }
    }
    for kind in [1, 3] {
        list.push(Img { kind, w: 300, h: 200, seed: 0xad00_0000 + n });
        n += 1;
    }
    list
}

fn levels_image() -> Vec<u8> {
    let mut d = vec![0u8; 256 * 4];
    for v in 0..256usize {
        d[v * 4] = u8::try_from(v).unwrap();
        d[v * 4 + 1] = u8::try_from(255 - v).unwrap();
        d[v * 4 + 2] = u8::try_from((v * 37) & 255).unwrap();
        d[v * 4 + 3] = u8::try_from(v).unwrap();
    }
    d
}

const NAN: f64 = f64::NAN;
const INF: f64 = f64::INFINITY;
const NINF: f64 = f64::NEG_INFINITY;
const SAT: [f64; 17] = [0.0, 0.25, 0.5, 0.75, 0.999_999_5, 1.0, 1.000_000_1, 1.5, 2.0, 3.0, 5.0, 1e9, -1.0, NAN, INF, NINF, 1e-7];
const TEMP: [f64; 23] = [
    3000.0, 4700.0, 5000.0, 5300.0, 5900.0, 6000.0, 6499.5, 6500.0, 6501.0, 7000.0, 7200.0, 8000.0, 8800.0, 10000.0, 11200.0, 20000.0,
    500.0, 1000.0, 0.0, -5.0, NAN, INF, NINF,
];
const SHARP: [f64; 21] =
    [0.0, -0.0, NAN, 0.01, 0.1, 0.25, 0.5, 0.75, 1.0, 1.7, -0.01, -0.1, -0.5, -1.0, -3.0, INF, NINF, 1e-300, -1e-300, 1e-8, 0.3333];
const BC: [f64; 25] = [
    -1.5, -1.0, -0.9, -0.8, -0.75, -0.5, -0.3, -0.25, -0.1, -5e-7, 0.0, -0.0, 0.05, 0.1, 0.15, 0.25, 0.5, 0.7, 0.8, 0.9, 1.0, 1.5, NAN,
    INF, NINF,
];

#[test]
fn adjustments_match_the_ts_bit_for_bit() {
    let cap = capture().subset(|k| k.starts_with("adjust/"));
    let mut replay = Replay::new(&cap);
    let images = image_list();
    assert_eq!(capture().meta("adjust-images").unwrap(), images.len().to_string());
    let limits = Limits::DEFAULT;
    for (k, im) in images.iter().enumerate() {
        let src = gen_image(im.kind, im.w, im.h, im.seed);
        let (w, h) = (u32::try_from(im.w).unwrap(), u32::try_from(im.h).unwrap());
        let (mut sat, mut temp, mut sharp) = (Stream::new(), Stream::new(), Stream::new());
        for v in SAT {
            let mut d = src.clone();
            adjust_saturation(&mut d, v);
            sat.bytes(&d);
        }
        for v in TEMP {
            let mut d = src.clone();
            adjust_temperature(&mut d, v);
            temp.bytes(&d);
        }
        for v in SHARP {
            let mut d = src.clone();
            adjust_sharpen(&mut d, w, h, v, &limits).unwrap();
            sharp.bytes(&d);
        }
        replay.check("adjust/sat", &format!("i{k}"), &summary(sat));
        replay.check("adjust/temp", &format!("i{k}"), &summary(temp));
        replay.check("adjust/sharp", &format!("i{k}"), &summary(sharp));
    }
    let lv = levels_image();
    for (bi, bright) in BC.iter().enumerate() {
        let mut s = Stream::new();
        for contrast in BC {
            let mut d = lv.clone();
            adjust_brightness_contrast(&mut d, *bright, contrast);
            s.bytes(&d);
        }
        replay.check("adjust/bc", &format!("b{bi}"), &summary(s));
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

// ── blip ────────────────────────────────────────────────────────────────────────────────────────

const GAINS: [Option<f64>; 8] = [None, Some(0.0), Some(1.0), Some(1.5), Some(-1.0), Some(NAN), Some(INF), Some(0.3)];
const BRIGHTS: [Option<f64>; 9] = [None, Some(0.0), Some(-0.0), Some(NAN), Some(0.1), Some(-0.3), Some(1.0), Some(-1.0), Some(1e9)];
const BILEVELS: [Option<f64>; 8] = [None, Some(0.0), Some(0.3), Some(0.5), Some(1.0), Some(NAN), Some(-1.0), Some(2.0)];

fn fx(gray: bool, gain: Option<f64>, bright: Option<f64>, floor: bool, bi_level: Option<f64>) -> ImageEffects {
    ImageEffects { gray, gain, bright, floor, bi_level }
}

fn blip_images() -> Vec<Vec<u8>> {
    vec![levels_image(), gen_image(1, 32, 32, 0xb100_0001), gen_image(0, 17, 13, 0xb100_0002)]
}

const ALPHAS: [Option<f64>; 11] =
    [None, Some(0.0), Some(0.5), Some(1.0), Some(1.0 / 3.0), Some(1.5), Some(-0.5), Some(NAN), Some(2.0), Some(1e9), Some(0.002)];
const DUOTONES: [Option<(&str, &str)>; 12] = [
    None,
    Some(("000000", "ffffff")),
    Some(("ff0000", "0000ff")),
    Some(("12", "abcdef")),
    Some(("-f0000", "zz")),
    Some(("0x1a2b3c", "gg1122")),
    Some(("  ", "ÄÖ12")),
    Some(("\u{1F600}ff00ff", "00ff00")),
    Some(("ffff", "ff")),
    Some(("FFaa00", "+f+f+f")),
    Some(("00ff00", " f f f")),
    Some(("", "")),
];
const CLRCHANGES: [Option<ClrChange<'static>>; 12] = [
    None,
    Some(ClrChange { from: "ff0000", to: "00ff00", to_alpha: 0.0 }),
    Some(ClrChange { from: "808080", to: "ffffff", to_alpha: 1.0 }),
    Some(ClrChange { from: "000000", to: "-f0000", to_alpha: 0.5 }),
    Some(ClrChange { from: "FfFfFf", to: "123456", to_alpha: 0.25 }),
    Some(ClrChange { from: "zz", to: "00ff00", to_alpha: 0.0 }),
    Some(ClrChange { from: "ff0000", to: "zz", to_alpha: 1.0 }),
    Some(ClrChange { from: "808080", to: "0x0000", to_alpha: 1.5 }),
    Some(ClrChange { from: "808080", to: "f0f0f0", to_alpha: -1.0 }),
    Some(ClrChange { from: "000000", to: "+f+f+f", to_alpha: NAN }),
    Some(ClrChange { from: "ffffff", to: "\u{1F600}\u{1F600}", to_alpha: 0.7 }),
    Some(ClrChange { from: "", to: "", to_alpha: 1.0 }),
];

fn picture_image() -> Vec<u8> {
    let mut rng = Rng::new(0xb200_0001);
    let near: [[i32; 3]; 5] = [[255, 0, 0], [128, 128, 128], [0, 0, 0], [255, 255, 255], [18, 52, 86]];
    let mut d = vec![0u8; 16 * 16 * 4];
    for p in 0..256 {
        if rng.below(2) == 0 {
            let c = near[rng.below(near.len())];
            for k in 0..3 {
                let v = c[k] + i32::try_from(rng.below(7)).unwrap() - 3;
                d[p * 4 + k] = u8::try_from(v.clamp(0, 255)).unwrap();
            }
        } else {
            for k in 0..3 {
                d[p * 4 + k] = u8::try_from(rng.below(256)).unwrap();
            }
        }
        d[p * 4 + 3] = u8::try_from(rng.below(256)).unwrap();
    }
    d
}

#[test]
fn blip_effects_match_the_ts_bit_for_bit() {
    let cap = capture().subset(|k| k.starts_with("blip/"));
    let mut replay = Replay::new(&cap);
    let mut s = Stream::new();
    for gain in GAINS {
        for bright in BRIGHTS {
            match effect_line(&fx(false, gain, bright, false, None)) {
                None => {
                    s.tag(b'n');
                }
                Some((slope, off)) => {
                    s.tag(b'l');
                    f(&mut s, slope);
                    f(&mut s, off);
                }
            }
        }
    }
    replay.check("blip/line", "v", &summary(s));
    let mut d = Stream::new();
    for components in [1u32, 3, 4, 2, 0] {
        for gray in [false, true] {
            for bi_level in [None, Some(0.5)] {
                for gain in GAINS {
                    for bright in BRIGHTS {
                        match effect_decode(&fx(gray, gain, bright, false, bi_level), components) {
                            None => {
                                d.tag(b'n');
                            }
                            Some(r) => {
                                d.tag(b'd');
                                d.u32(u32::try_from(r.len()).unwrap());
                                for v in r {
                                    f(&mut d, v);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    replay.check("blip/decode", "v", &summary(d));
    for (k, src) in blip_images().iter().enumerate() {
        let mut s = Stream::new();
        for gray in [false, true] {
            for floor in [false, true] {
                for gain in GAINS {
                    for bright in BRIGHTS {
                        for bi_level in BILEVELS {
                            let mut a = src.clone();
                            apply_image_effects(&mut a, &fx(gray, gain, bright, floor, bi_level));
                            s.bytes(&a);
                        }
                    }
                }
            }
        }
        replay.check("blip/effects", &format!("i{k}"), &summary(s));
    }
    let sources = [picture_image(), gen_image(1, 16, 16, 0xb100_0003), levels_image()];
    for (k, src) in sources.iter().enumerate() {
        let mut s = Stream::new();
        for alpha in ALPHAS {
            for duotone in DUOTONES {
                for clr_change in CLRCHANGES {
                    let mut a = src.clone();
                    apply_picture_adjust(&mut a, &PictureAdjust { alpha, duotone, clr_change });
                    s.bytes(&a);
                }
            }
        }
        replay.check("blip/adjust", &format!("i{k}"), &summary(s));
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}
