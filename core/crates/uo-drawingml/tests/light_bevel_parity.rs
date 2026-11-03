//! Parity replay of `light3d` and `bevel` against `tests/captures/light_bevel.capture`, written by
//! `tools/capture-light-bevel.mjs` from the frozen TS (`shared/drawingml/dist`, Node 24.15.0 linux-x64,
//! glibc 2.39 with the FMA variant of `pow` masked): `light_table`, `light_at`, `light_face`,
//! `bevel_phi` and `shade_bevel` over the corpus of the script (every light rig, material,
//! direction, hostile azimuths and polar angles; every Word bevel table and condition; generated
//! silhouettes and RGBA). The inputs are regenerated here exactly as in the script; the outputs
//! (NaN canonicalized, `-0` distinguished, RGBA as bytes) are hashed with the same canonical stream
//! and compared digest by digest.
//!
//! The light facets call no `pow`; the bevel facets read `SRGB_LIN`/`LIN_SRGB`, generated in the
//! same environment as this capture (hence the platform requirement below).
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_drawingml::{BevelSpec, Limits, Mask, bevel_phi, light_at, light_face, light_table, shade_bevel};
use uo_jsnum::conv::canonicalize_nan;
use uo_jsnum::math::{max, min};
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec, Replay, Stream};

const CAPTURE: &str = include_str!("captures/light_bevel.capture");

// ── the corpus (mirrored from tools/capture-light-bevel.mjs) ────────────────────────────────────

type O = Option<&'static str>;

const RIGS: [O; 20] = [
    None,
    Some(""),
    Some("threePt"),
    Some("balanced"),
    Some("soft"),
    Some("harsh"),
    Some("glow"),
    Some("twoPt"),
    Some("freezing"),
    Some("contrasting"),
    Some("flood"),
    Some("chilly"),
    Some("morning"),
    Some("sunrise"),
    Some("sunset"),
    Some("brightRoom"),
    Some("flat"),
    Some("nope"),
    Some("threePt|matte"),
    Some("Soft"),
];
const MATS: [O; 14] = [
    None,
    Some(""),
    Some("warmMatte"),
    Some("matte"),
    Some("plastic"),
    Some("metal"),
    Some("flat"),
    Some("softEdge"),
    Some("powder"),
    Some("dkEdge"),
    Some("softmetal"),
    Some("nope"),
    Some("clear"),
    Some("Matte"),
];
const DIRS: [O; 12] =
    [None, Some(""), Some("t"), Some("tl"), Some("l"), Some("bl"), Some("b"), Some("br"), Some("r"), Some("tr"), Some("nope"), Some("T")];
const KEYS: [(&str, O); 23] = [
    ("threePt", None),
    ("threePt", Some("matte")),
    ("threePt", Some("plastic")),
    ("threePt", Some("metal")),
    ("threePt", Some("flat")),
    ("threePt", Some("softEdge")),
    ("threePt", Some("powder")),
    ("balanced", None),
    ("soft", None),
    ("harsh", None),
    ("glow", None),
    ("twoPt", None),
    ("freezing", None),
    ("contrasting", None),
    ("flood", None),
    ("chilly", None),
    ("morning", None),
    ("sunrise", None),
    ("sunset", None),
    ("threePt", Some("dkEdge")),
    ("threePt", Some("softmetal")),
    ("brightRoom", None),
    ("flat", None),
];
const WORD_PRSTS: [&str; 12] =
    ["relaxedInset", "circle", "slope", "cross", "angle", "softRound", "convex", "coolSlant", "divot", "riblet", "hardEdge", "artDeco"];

fn prsts() -> Vec<O> {
    let mut v: Vec<O> = vec![None, Some("")];
    v.extend(WORD_PRSTS.iter().map(|p| Some(*p)));
    v.push(Some("nope"));
    v.push(Some("Circle"));
    v
}

fn label(x: O) -> String {
    match x {
        None => "u".to_owned(),
        Some("") => "e".to_owned(),
        Some(s) => s.to_owned(),
    }
}

fn thetas() -> Vec<f64> {
    let mut v = Vec::new();
    for k in -50..=50 {
        v.push(f64::from(k) * 7.3);
    }
    for k in -2..=25 {
        v.push(f64::from(k) * 15.0);
    }
    v.extend([f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 1e300, 359.99999, 360.0, -1e-20, -1e-300, 1e-320, 720.0000001]);
    v
}

fn phis() -> Vec<f64> {
    let mut v = vec![
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -0.0,
        0.0,
        -1.0,
        1.0,
        5.0,
        10.0,
        10.0000001,
        20.0,
        33.3,
        40.0,
        50.0,
        60.0,
        70.0,
        74.999,
        75.0,
        80.0,
        84.999,
        85.0,
        85.0001,
        90.0,
        180.0,
        1e9,
        1e300,
        -1e300,
    ];
    for k in 0..=36 {
        v.push(f64::from(k) * 2.5);
    }
    v
}

fn phi_ts() -> Vec<f64> {
    let mut v = vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 0.0, 1e-4, 1e-3, 2.0, 1e9, -1.0, 1.0];
    for k in 0..=60 {
        v.push(f64::from(k) / 50.0);
    }
    for k in 0..24 {
        v.push((f64::from(k) + 0.5) / 24.0);
    }
    v
}

const HWS: [f64; 11] = [0.0, 0.25, 0.5, 1.0, 2.0, 4.0, f64::NAN, -1.0, f64::INFINITY, 1e-9, 1e9];

/// `(kind, W, H, seed)` of a silhouette by name.
fn mask_def(name: &str) -> (&'static str, u32, u32, u32) {
    match name {
        "e1" => ("ellipse", 1, 1, 0),
        "e2" => ("ellipse", 2, 2, 0),
        "e3" => ("ellipse", 3, 5, 0),
        "e17" => ("ellipse", 17, 9, 0),
        "e40" => ("ellipse", 40, 40, 0),
        "e64" => ("ellipse", 64, 48, 0),
        "rr" => ("rrect", 50, 30, 0),
        "ring" => ("ring", 60, 60, 0),
        "blobs" => ("blobs", 70, 50, 7),
        "full" => ("full", 24, 18, 0),
        "empty" => ("empty", 10, 10, 0),
        "noise" => ("noise", 32, 24, 3),
        "line" => ("ellipse", 100, 1, 0),
        "col" => ("ellipse", 1, 100, 0),
        "big_e" => ("ellipse", 256, 256, 0),
        "big_blobs" => ("blobs", 200, 150, 11),
        "big_rr" => ("rrect", 300, 120, 0),
        "nan" => ("noisenan", 24, 18, 5),
        other => panic!("no mask {other}"),
    }
}

const NOISE_PAL: [f64; 8] = [0.0, 0.25, 0.5, 1.0, 1.0, 1.0, 1.5, -1.0];
const NAN_PAL: [f64; 8] = [0.0, 0.25, 0.5, 1.0, 1.0, 1.0, f64::NAN, -1.0];

fn ell(x: f64, y: f64, ex: f64, ey: f64, erx: f64, ery: f64) -> f64 {
    let dx = (x + 0.5 - ex) / erx;
    let dy = (y + 0.5 - ey) / ery;
    let d = (dx * dx + dy * dy).sqrt();
    max(0.0, min(1.0, (1.0 - d) * min(erx, ery) + 0.5))
}

#[allow(clippy::cast_possible_truncation)]
fn st(v: f64) -> f32 {
    v as f32
}

fn gen_mask(name: &str) -> Mask {
    let (kind, w, h, seed) = mask_def(name);
    let (wu, hu) = (w as usize, h as usize);
    let mut a = vec![0.0f32; wu * hu];
    let (wf, hf) = (f64::from(w), f64::from(h));
    let (cx, cy) = (wf / 2.0, hf / 2.0);
    let rx = max(wf / 2.0 - 0.5, 0.5);
    let ry = max(hf / 2.0 - 0.5, 0.5);
    let mut rng = Rng::new(seed);
    if kind == "blobs" {
        let mut bl = Vec::new();
        for _ in 0..3 {
            let ex = rng.below(wu);
            let ey = rng.below(hu);
            let erx = 2 + rng.below((w >> 1).max(1) as usize);
            let ery = 2 + rng.below((h >> 1).max(1) as usize);
            bl.push([ex as f64, ey as f64, erx as f64, ery as f64]);
        }
        for y in 0..hu {
            for x in 0..wu {
                let mut v = 0.0;
                for b in &bl {
                    v = max(v, ell(x as f64, y as f64, b[0], b[1], b[2], b[3]));
                }
                a[y * wu + x] = st(v);
            }
        }
    } else if kind == "noise" || kind == "noisenan" {
        let pal = if kind == "noise" { &NOISE_PAL } else { &NAN_PAL };
        for slot in &mut a {
            *slot = st(pal[rng.below(pal.len())]);
        }
    } else {
        let r = min(wf, hf) / 4.0;
        for y in 0..hu {
            for x in 0..wu {
                let (xf, yf) = (x as f64, y as f64);
                let v = match kind {
                    "ellipse" => ell(xf, yf, cx, cy, rx, ry),
                    "ring" => ell(xf, yf, cx, cy, rx, ry) * (1.0 - ell(xf, yf, cx, cy, rx / 2.0, ry / 2.0)),
                    "rrect" => {
                        let qx = max((xf + 0.5 - cx).abs() - (cx - r), 0.0);
                        let qy = max((yf + 0.5 - cy).abs() - (cy - r), 0.0);
                        max(0.0, min(1.0, 0.5 - ((qx * qx + qy * qy).sqrt() - r)))
                    }
                    "full" => 1.0,
                    _ => 0.0,
                };
                a[y * wu + x] = st(v);
            }
        }
    }
    Mask::from_vec(w, h, a).unwrap()
}

fn gen_rgba(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    (0..w as usize * h as usize * 4).map(|_| ((rng.next_u32() >> 8) & 255) as u8).collect()
}

// ── the replay ──────────────────────────────────────────────────────────────────────────────────

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

fn summary(s: Stream) -> String {
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

fn f(s: &mut Stream, v: f64) {
    s.f64(canonicalize_nan(v));
}

fn f4(s: &mut Stream, v: [f32; 4]) {
    for x in v {
        f(s, f64::from(x));
    }
}

#[test]
fn light_tables_match_the_ts_bit_for_bit() {
    let cap = capture().subset(|k| k == "light/table");
    let mut replay = Replay::new(&cap);
    for rig in RIGS {
        let mut s = Stream::new();
        for mat in MATS {
            s.bytes(light_table(rig, mat).bytes());
        }
        replay.check("light/table", &format!("r_{}", label(rig)), &summary(s));
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn light_at_and_face_match_the_ts_bit_for_bit() {
    let cap = capture().subset(|k| k == "light/at" || k == "light/face");
    let mut replay = Replay::new(&cap);
    let (th, ph) = (thetas(), phis());
    let mut sf = Stream::new();
    for (rig, mat) in KEYS {
        let t = light_table(Some(rig), mat);
        let mut s = Stream::new();
        for dir in DIRS {
            for &a in &th {
                for &p in &ph {
                    f4(&mut s, light_at(&t, dir, a, p));
                }
            }
        }
        replay.check("light/at", &format!("k_{rig}_{}", label(mat)), &summary(s));
        f4(&mut sf, light_face(&t));
    }
    replay.check("light/face", "all", &summary(sf));
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

#[test]
fn bevel_phi_matches_the_ts_bit_for_bit() {
    let cap = capture().subset(|k| k == "light/phi");
    let mut replay = Replay::new(&cap);
    let ts = phi_ts();
    for prst in prsts() {
        let mut s = Stream::new();
        for &t in &ts {
            for hw in HWS {
                f(&mut s, bevel_phi(prst, t, hw));
            }
        }
        replay.check("light/phi", &format!("p_{}", label(prst)), &summary(s));
    }
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}

struct Shader {
    call: u32,
    masks: std::collections::HashMap<&'static str, Mask>,
}

impl Shader {
    fn new() -> Shader {
        Shader { call: 0, masks: std::collections::HashMap::new() }
    }

    fn shade(&mut self, s: &mut Stream, mask: &'static str, spec: &BevelSpec<'_>) {
        let sil = self.masks.entry(mask).or_insert_with(|| gen_mask(mask));
        let mut rgba = gen_rgba(sil.w(), sil.h(), 1000 + 7919 * self.call);
        shade_bevel(&mut rgba, sil, spec, &Limits::DEFAULT).unwrap();
        s.u32(self.call).bytes(&rgba);
        self.call += 1;
    }
}

fn spec<'a>(w: f64, h: f64, prst: O, rig: O, dir: O, material: O, word: bool) -> BevelSpec<'a> {
    BevelSpec { w, h, prst, rig, dir, material, word }
}

#[test]
fn shade_bevel_matches_the_ts_bit_for_bit() {
    let cap = capture().subset(|k| k.starts_with("bevel/"));
    let mut replay = Replay::new(&cap);
    let mut sh = Shader::new();
    let all_prsts = prsts();
    for key in ["bevel-calls", "uint8-vs-clamped-checked"] {
        assert!(cap.meta(key).is_some(), "{key}");
    }

    // A: the Word tables and the conditions that select or reject them
    let cond: [(O, O, O); 6] = [
        (None, None, None),
        (Some("threePt"), Some("t"), Some("warmMatte")),
        (Some(""), Some(""), Some("")),
        (Some("balanced"), None, None),
        (None, Some("tl"), None),
        (None, None, Some("matte")),
    ];
    let ws = [0.4, 0.6, 3.0, 12.0, 80.0, f64::NAN];
    for &prst in &all_prsts {
        let mut s = Stream::new();
        for (rig, dir, material) in cond {
            for w in ws {
                for hr in [1.0, 0.5] {
                    for m in ["e17", "e40", "rr", "ring", "blobs", "noise"] {
                        sh.shade(&mut s, m, &spec(w, w * hr, prst, rig, dir, material, true));
                    }
                }
            }
        }
        replay.check("bevel/word", &format!("p_{}", label(prst)), &summary(s));
    }

    // B: the light rigs x materials x presets, directions cycling
    let bm = ["e17", "ring", "blobs", "noise"];
    for (ri, rig) in RIGS.into_iter().enumerate() {
        let mut s = Stream::new();
        for (pi, &prst) in all_prsts.iter().enumerate() {
            for (mi, material) in MATS.into_iter().enumerate() {
                let w = [3.0, 12.0][(pi + mi) % 2];
                let hr = [1.0, 0.5, 2.0][(ri + mi) % 3];
                sh.shade(&mut s, bm[(pi * 14 + mi) % 4], &spec(w, w * hr, prst, rig, DIRS[(ri + pi + mi) % 12], material, false));
            }
        }
        replay.check("bevel/light", &format!("r_{}", label(rig)), &summary(s));
    }

    // B2: every direction x preset on small masks and several rigs
    for dir in DIRS {
        let mut s = Stream::new();
        for (pi, &prst) in all_prsts.iter().enumerate() {
            for (mi, m) in ["e17", "blobs", "noise"].into_iter().enumerate() {
                for w in [3.0, 12.0] {
                    sh.shade(&mut s, m, &spec(w, w, prst, RIGS[(pi * 3 + mi) % 20], dir, None, false));
                }
            }
        }
        replay.check("bevel/dir", &format!("d_{}", label(dir)), &summary(s));
    }

    // C: larger masks, Word table on and off
    let mut s = Stream::new();
    for m in ["big_e", "big_blobs", "big_rr"] {
        for prst in [None, Some("circle"), Some("relaxedInset"), Some("angle"), Some("softRound"), Some("convex")] {
            for word in [false, true] {
                for w in [6.0, 24.0, 100.0] {
                    sh.shade(&mut s, m, &spec(w, w, prst, None, None, None, word));
                }
            }
        }
    }
    replay.check("bevel/big", "all", &summary(s));

    // D: a silhouette with NaN coverage
    let mut s = Stream::new();
    for word in [false, true] {
        for prst in [None, Some("relaxedInset"), Some("angle")] {
            for w in [3.0, 12.0] {
                sh.shade(&mut s, "nan", &spec(w, w, prst, None, None, None, word));
            }
        }
    }
    replay.check("bevel/nan", "all", &summary(s));

    assert_eq!(cap.meta("bevel-calls").unwrap(), sh.call.to_string());
    let report = replay.finish();
    eprintln!("{}", report.summary());
    report.assert_clean();
}
