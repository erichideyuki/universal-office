//! Parity replay of the preset engine against `tests/captures/presets.capture`, written by
//! `tools/capture-presets.mjs` from the frozen TS (`shared/drawingml/dist`, Node 24.15.0, V8
//! 13.6): `preset_path`, `eval_preset`, `has_preset_def`, `preset_guides`, `preset_text_rect`,
//! `preset_outline`, `custom_guides` and `arc_to_seg` over the corpus of the script (every
//! preset, sizes and hostile adjust sets, 5,000 custom guide programs, an arc grid). The inputs
//! are regenerated here exactly as in the script; the outputs (NaN canonicalized, `-0`
//! distinguished) are hashed with the same canonical stream and compared digest by digest.
//!
//! None of the facets calls `pow` (sin, cos, tan, atan2 and sqrt only): they are valid from any
//! Node x64, and the replay checks V8, Node and the architecture only.
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_drawingml::{
    GuideDef, Limits, PathSeg, PresetPath, Subpaths, arc_to_seg, custom_guides, eval_preset, has_preset_def, preset_guides, preset_names,
    preset_outline, preset_path, preset_text_rect,
};
use uo_jsnum::conv::canonicalize_nan;
use uo_jsnum::parse::to_number;
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec, Replay, Report, Stream};

pub const CAPTURE: &str = include_str!("captures/presets.capture");

// ── the corpus (mirrored from tools/capture-presets.mjs) ────────────────────────────────────────

const SIZES: [(f64, f64); 7] = [(200.0, 100.0), (100.0, 200.0), (100.0, 100.0), (0.0, 0.0), (1.0, 1e6), (21600.0, 21600.0), (13.37, 0.5)];
const PERTURB: [f64; 9] = [0.0, -1.0, 1e5, 1e9, -1e9, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308];
const DIVISORS: [u32; 10] = [2, 3, 4, 5, 6, 8, 10, 12, 16, 32];
const PROBE_NAMES: [&str; 5] = ["nope", "", "0", "constructor", "__proto__"];
const PROBE_TOKENS: [&str; 10] = ["0", "abc", "0x10", "l", " 7 ", "1e400", "Infinity", "-5", "", "1_0"];
const HAND_DRAWN: [&str; 49] = [
    "frame",
    "ellipse",
    "roundRect",
    "triangle",
    "rtTriangle",
    "diamond",
    "parallelogram",
    "trapezoid",
    "pentagon",
    "hexagon",
    "star5",
    "rightArrow",
    "leftArrow",
    "upArrow",
    "downArrow",
    "chevron",
    "pie",
    "line",
    "straightConnector1",
    "bentConnector2",
    "bentConnector3",
    "bentConnector4",
    "curvedConnector2",
    "curvedConnector3",
    "curvedConnector4",
    "curvedConnector5",
    "arc",
    "blockArc",
    "octagon",
    "donut",
    "moon",
    "heart",
    "lightningBolt",
    "sun",
    "can",
    "cube",
    "rect",
    "actionButtonBlank",
    "actionButtonHome",
    "actionButtonHelp",
    "actionButtonInformation",
    "actionButtonBackPrevious",
    "actionButtonForwardNext",
    "actionButtonBeginning",
    "actionButtonEnd",
    "actionButtonReturn",
    "actionButtonDocument",
    "actionButtonSound",
    "actionButtonMovie",
];
const EXTRA_NAMES: [(&str, Option<&str>); 13] = [
    ("undef", None),
    ("empty", Some("")),
    ("nope", Some("nope")),
    ("constructor", Some("constructor")),
    ("proto", Some("__proto__")),
    ("toString", Some("toString")),
    ("hasOwnProperty", Some("hasOwnProperty")),
    ("valueOf", Some("valueOf")),
    ("Frame", Some("Frame")),
    ("actionButton", Some("actionButton")),
    ("actionButtonNone", Some("actionButtonNone")),
    ("rect", Some("rect")),
    ("space", Some(" rect")),
];

fn builtins() -> Vec<String> {
    let mut v: Vec<String> = ["l", "t", "r", "b", "w", "h", "hc", "vc", "ss", "ls", "cd2", "cd4", "cd8", "3cd4", "3cd8", "5cd8", "7cd8"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    for d in DIVISORS {
        v.push(format!("wd{d}"));
        v.push(format!("hd{d}"));
        v.push(format!("ssd{d}"));
    }
    v
}

struct AdjSet {
    adj: Option<Vec<f64>>,
    names: Option<Vec<String>>,
}

fn adj_sets(names: &[String], defs: &[f64]) -> Vec<AdjSet> {
    let mut sets = Vec::new();
    let mut add = |adj: Option<Vec<f64>>, nm: Option<Vec<String>>| sets.push(AdjSet { adj, names: nm });
    add(None, None);
    add(Some(vec![]), None);
    add(Some(vec![]), Some(vec![]));
    add(Some(defs.to_vec()), None);
    add(Some(defs.to_vec()), Some(names.to_vec()));
    for i in 0..names.len() {
        for p in PERTURB {
            let mut d = defs.to_vec();
            d[i] = p;
            add(Some(d.clone()), None);
            add(Some(d.clone()), Some(names.to_vec()));
            add(Some(vec![p]), Some(vec![names[i].clone()]));
            let mut dr = d.clone();
            dr.reverse();
            let mut nr = names.to_vec();
            nr.reverse();
            add(Some(dr), Some(nr));
            add(Some(vec![p, defs[i]]), Some(vec![names[i].clone(), names[i].clone()]));
        }
    }
    for p in PERTURB {
        add(Some(vec![p]), None);
        add(Some(vec![p]), Some(vec!["adj".to_owned()]));
        let mut long = defs.to_vec();
        long.push(p);
        long.push(7.0);
        add(Some(long), None);
    }
    add(Some(vec![1.0, 2.0, 3.0]), Some(vec!["adj".to_owned()]));
    add(Some(vec![1.0]), Some(vec!["nope".to_owned()]));
    sets
}

// ── encoders (mirrored from the script) ─────────────────────────────────────────────────────────

fn f(s: &mut Stream, v: f64) {
    s.f64(canonicalize_nan(v));
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap()
}

fn seg(s: &mut Stream, g: &PathSeg) {
    match *g {
        PathSeg::M { x, y } => {
            s.tag(b'M');
            f(s, x);
            f(s, y);
        }
        PathSeg::L { x, y } => {
            s.tag(b'L');
            f(s, x);
            f(s, y);
        }
        PathSeg::C { x1, y1, x2, y2, x, y } => {
            s.tag(b'C');
            for v in [x1, y1, x2, y2, x, y] {
                f(s, v);
            }
        }
        PathSeg::Q { x1, y1, x, y } => {
            s.tag(b'Q');
            for v in [x1, y1, x, y] {
                f(s, v);
            }
        }
        PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => {
            s.tag(b'E');
            for v in [cx, cy, rx, ry, a0, a1] {
                f(s, v);
            }
            s.bool(ccw);
        }
        PathSeg::Z => {
            s.tag(b'Z');
        }
    }
}

fn segs(s: &mut Stream, subs: &Subpaths) {
    s.u32(count(subs.len()));
    for sub in subs.iter() {
        s.u32(count(sub.len()));
        for g in sub {
            seg(s, g);
        }
    }
}

fn lums(s: &mut Stream, list: &[Option<f64>]) {
    s.u32(count(list.len()));
    for v in list {
        match v {
            None => {
                s.tag(b'n');
            }
            Some(v) => {
                s.tag(b'f');
                f(s, *v);
            }
        }
    }
}

fn enc_path(s: &mut Stream, p: &PresetPath) {
    match p {
        PresetPath::Poly(points) => {
            s.tag(b'P');
            s.u32(count(points.len()));
            for q in points {
                f(s, q.x);
                f(s, q.y);
            }
        }
        PresetPath::Line { x1, y1, x2, y2 } => {
            s.tag(b'L');
            for v in [x1, y1, x2, y2] {
                f(s, *v);
            }
        }
        PresetPath::Pie { start_deg, end_deg } => {
            s.tag(b'I');
            f(s, *start_deg);
            f(s, *end_deg);
        }
        PresetPath::Rect => {
            s.tag(b'R');
        }
        PresetPath::Ellipse => {
            s.tag(b'E');
        }
        PresetPath::RoundRect { r } => {
            s.tag(b'O');
            f(s, *r);
        }
        PresetPath::Path(shape) => {
            s.tag(b'A');
            s.bool(shape.even_odd);
            segs(s, &shape.subs);
            s.bool(shape.sub_lum.is_some());
            if let Some(l) = &shape.sub_lum {
                lums(s, l);
            }
            s.bool(shape.sub_stroke.is_some());
            if let Some(st) = &shape.sub_stroke {
                s.u32(count(st.len()));
                for v in st {
                    s.bool(*v);
                }
            }
        }
    }
}

fn bools(s: &mut Stream, list: &[bool]) {
    s.u32(count(list.len()));
    for v in list {
        s.bool(*v);
    }
}

fn summary(s: Stream) -> String {
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

// ── the replay ──────────────────────────────────────────────────────────────────────────────────

fn capture() -> Capture {
    let cap = Capture::parse(CAPTURE).unwrap();
    let spec = PlatformSpec { v8_prefix: Some("13.6.233.17".into()), arch: Some("x64".into()), ..PlatformSpec::default() };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    cap
}

fn split(list: &str) -> Vec<String> {
    if list.is_empty() { Vec::new() } else { list.split(',').map(str::to_owned).collect() }
}

fn run_preset(replay: &mut Replay<'_>, key: &str, name: Option<&str>, av: &[String], defs: &[f64], guide_names: &[String]) {
    let sets = adj_sets(av, defs);
    for (si, &(w, h)) in SIZES.iter().enumerate() {
        let (mut sp, mut se, mut sg, mut sr, mut so) = (Stream::new(), Stream::new(), Stream::new(), Stream::new(), Stream::new());
        for (k, set) in sets.iter().enumerate() {
            for s in [&mut sp, &mut se, &mut sg, &mut sr, &mut so] {
                s.u32(count(k));
            }
            let adj = set.adj.as_deref();
            let nm_refs: Option<Vec<&str>> = set.names.as_ref().map(|v| v.iter().map(String::as_str).collect());
            let nm = nm_refs.as_deref();
            enc_path(&mut sp, &preset_path(name, w, h, adj, nm));
            match name {
                None => {
                    se.tag(b'u');
                    sg.tag(b'u');
                }
                Some(n) => {
                    match eval_preset(n, w, h, adj, nm) {
                        None => {
                            se.tag(b'n');
                        }
                        Some(g) => {
                            se.tag(b'g');
                            segs(&mut se, &g.subs);
                            lums(&mut se, &g.fill);
                            bools(&mut se, &g.stroke);
                        }
                    }
                    match preset_guides(n, w, h, adj, nm) {
                        None => {
                            sg.tag(b'n');
                        }
                        Some(g) => {
                            sg.tag(b'm');
                            sg.u32(count(g.len()));
                            for name in guide_names {
                                match g.get(name) {
                                    None => {
                                        sg.tag(b'u');
                                    }
                                    Some(v) => {
                                        sg.tag(b'v');
                                        f(&mut sg, v);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            match preset_text_rect(name, w, h, adj, nm) {
                None => {
                    sr.tag(b'n');
                }
                Some(r) => {
                    sr.tag(b'r');
                    for v in [r.l, r.t, r.r, r.b] {
                        f(&mut sr, v);
                    }
                }
            }
            match (&set.names, &set.adj) {
                (None, _) => enc_outline(&mut so, name, w, h, None),
                (Some(n), Some(a)) if n.len() <= a.len() => {
                    let mut pairs: Vec<(&str, f64)> = Vec::new();
                    for (i, n) in n.iter().enumerate() {
                        match pairs.iter_mut().find(|p| p.0 == n.as_str()) {
                            Some(p) => p.1 = a[i],
                            None => pairs.push((n.as_str(), a[i])),
                        }
                    }
                    enc_outline(&mut so, name, w, h, Some(&pairs));
                }
                _ => {
                    so.tag(b's');
                }
            }
        }
        replay.check(key, &format!("path.s{si}"), &summary(sp));
        replay.check(key, &format!("eval.s{si}"), &summary(se));
        replay.check(key, &format!("guides.s{si}"), &summary(sg));
        replay.check(key, &format!("rect.s{si}"), &summary(sr));
        replay.check(key, &format!("outline.s{si}"), &summary(so));
    }
    let mut sh = Stream::new();
    sh.bool(has_preset_def(name));
    replay.check(key, "has", &summary(sh));
}

fn enc_outline(s: &mut Stream, name: Option<&str>, w: f64, h: f64, adj: Option<&[(&str, f64)]>) {
    let o = preset_outline(name, w, h, adj);
    s.tag(b'o');
    segs(s, &o.subs);
    bools(s, &o.fill);
    bools(s, &o.stroke);
}

#[test]
fn presets_match_the_ts_bit_for_bit() {
    let report = replay_presets(&capture(), 1);
    eprintln!("{}", report.summary());
    report.assert_clean();
}

/// The replay of the presets (the wasm self-test of `uo-wasm` calls it too): every `every`-th
/// defined preset and every `every`-th hand-drawn name outside the table, all the names without a
/// definition. `every == 1` is the whole corpus; the facets not replayed show as `missing`.
pub fn replay_presets(cap: &Capture, every: usize) -> Report {
    let every = every.max(1);
    let cap = cap.subset(|k| !k.starts_with("custom/") && !k.starts_with("arc/"));
    let mut replay = Replay::new(&cap);
    let names: Vec<&str> = preset_names().collect();
    assert_eq!(names.len(), 187);
    let builtin = builtins();
    for (index, name) in names.iter().enumerate() {
        if !index.is_multiple_of(every) {
            continue;
        }
        let key = format!("preset/{name}");
        let rec = cap.record(&key).unwrap_or_else(|| panic!("no capture record {key}"));
        let av = split(rec.get("av").unwrap());
        let defs: Vec<f64> = split(rec.get("avd").unwrap()).iter().map(|v| to_number(v)).collect();
        assert_eq!(av.len(), defs.len());
        let all = split(rec.get("guides").unwrap());
        let mut guide_names = builtin.clone();
        guide_names.extend(all);
        guide_names.extend(PROBE_NAMES.iter().map(|s| (*s).to_owned()));
        for k in ["av", "avd", "guides"] {
            replay.skip(&key, k);
        }
        run_preset(&mut replay, &key, Some(name), &av, &defs, &guide_names);
    }
    let two = ["adj1".to_owned(), "adj2".to_owned()];
    let mut guide_names = builtin.clone();
    guide_names.extend(two.iter().cloned());
    guide_names.extend(PROBE_NAMES.iter().map(|s| (*s).to_owned()));
    for (label, name) in EXTRA_NAMES {
        run_preset(&mut replay, &format!("extra/{label}"), name, &two, &[50000.0, 50000.0], &guide_names);
    }
    for (index, name) in HAND_DRAWN.into_iter().enumerate() {
        if !names.contains(&name) && index % every == 0 {
            run_preset(&mut replay, &format!("hand/{name}"), Some(name), &two, &[50000.0, 50000.0], &guide_names);
        }
    }
    let mut s = Stream::new();
    let all_names: Vec<Option<&str>> =
        names.iter().map(|n| Some(*n)).chain(HAND_DRAWN.iter().map(|n| Some(*n))).chain(EXTRA_NAMES.iter().map(|e| e.1)).collect();
    for n in all_names {
        s.bool(has_preset_def(n));
    }
    replay.check("has/all", "v", &summary(s));
    replay.finish()
}

// ── custom guides ───────────────────────────────────────────────────────────────────────────────

const CG_DIMS: [f64; 9] = [200.0, 100.0, 0.0, 1e6, 21600.0, 13.37, f64::NAN, -5.0, f64::INFINITY];
const CG_NAMES: [&str; 18] = ["a", "b", "c", "d", "x1", "y1", "", "0", "w", "h", "l", "ss", "wd2", "adj1", "adj2", "val", "1e3", " 12"];
const CG_OPS: [&str; 21] = [
    "val", "*/", "+-", "+/", "?:", "abs", "at2", "cat2", "sat2", "cos", "sin", "tan", "max", "min", "mod", "pin", "sqrt", "bogus", "",
    "VAL", "+ -",
];
const CG_TOKENS: [&str; 52] = [
    "0",
    "1",
    "-1",
    "100000",
    "21600000",
    "5400000",
    "2.5",
    "0x1A",
    "1e400",
    "1e-400",
    " 12",
    "Infinity",
    "-Infinity",
    "NaN",
    "+.5",
    "5.",
    "0b11",
    "0o17",
    "1_0",
    "abc",
    "",
    "l",
    "t",
    "r",
    "b",
    "w",
    "h",
    "hc",
    "vc",
    "ss",
    "ls",
    "wd2",
    "hd4",
    "ssd8",
    "cd2",
    "cd4",
    "3cd4",
    "a",
    "c",
    "d",
    "x1",
    "y1",
    "adj1",
    "adj2",
    "\u{a0} 7\u{a0}",
    "\u{feff}3",
    "\u{663}",
    "e5",
    ".",
    "-",
    "+",
    "1e3",
];
const CG_PROGRAMS: u32 = 5000;
const CG_BLOCK: u32 = 250;

struct Program {
    w: f64,
    h: f64,
    av: Vec<(&'static str, Vec<&'static str>)>,
    gd: Vec<(&'static str, Vec<&'static str>)>,
}

fn program(i: u32) -> Program {
    let mut rng = Rng::new(0xc0de_0000 + i);
    let w = CG_DIMS[rng.below(CG_DIMS.len())];
    let h = CG_DIMS[rng.below(CG_DIMS.len())];
    let nav = rng.below(4);
    let ngd = rng.below(10);
    let mut guide = || {
        let name = CG_NAMES[rng.below(CG_NAMES.len())];
        let opn = rng.below(CG_OPS.len() + 1);
        if opn == CG_OPS.len() {
            return (name, Vec::new());
        }
        let argc = rng.below(5);
        let mut fmla = vec![CG_OPS[opn]];
        for _ in 0..argc {
            fmla.push(CG_TOKENS[rng.below(CG_TOKENS.len())]);
        }
        (name, fmla)
    };
    let av: Vec<_> = (0..nav).map(|_| guide()).collect();
    let gd: Vec<_> = (0..ngd).map(|_| guide()).collect();
    Program { w, h, av, gd }
}

fn cg_query() -> Vec<String> {
    let mut q: Vec<String> = Vec::new();
    let all = CG_TOKENS
        .iter()
        .map(|s| (*s).to_owned())
        .chain(CG_NAMES.iter().map(|s| (*s).to_owned()))
        .chain(builtins())
        .chain(PROBE_TOKENS.iter().map(|s| (*s).to_owned()));
    for t in all {
        if !q.contains(&t) {
            q.push(t);
        }
    }
    q
}

#[test]
fn custom_guides_match_the_ts_bit_for_bit() {
    let report = replay_custom_guides(&capture(), 1);
    eprintln!("{}", report.summary());
    report.assert_clean();
}

/// The replay of the custom guide programs: every `every`-th block of 250.
pub fn replay_custom_guides(cap: &Capture, every: usize) -> Report {
    let every = every.max(1);
    let cap = cap.subset(|k| k.starts_with("custom/"));
    let mut replay = Replay::new(&cap);
    let query = cg_query();
    let limits = Limits::DEFAULT;
    for b in 0..CG_PROGRAMS / CG_BLOCK {
        if !usize::try_from(b).unwrap().is_multiple_of(every) {
            continue;
        }
        let mut s = Stream::new();
        for i in b * CG_BLOCK..(b + 1) * CG_BLOCK {
            let p = program(i);
            let av: Vec<GuideDef<'_>> = p.av.iter().map(|(n, fm)| GuideDef { name: n, fmla: fm }).collect();
            let gd: Vec<GuideDef<'_>> = p.gd.iter().map(|(n, fm)| GuideDef { name: n, fmla: fm }).collect();
            let g = custom_guides(&av, &gd, p.w, p.h, &limits);
            s.u32(i);
            for t in &query {
                f(&mut s, g.val(t));
            }
        }
        replay.check(&format!("custom/{b}"), "v", &summary(s));
    }
    replay.finish()
}

#[test]
fn arc_to_seg_matches_the_ts_bit_for_bit() {
    replay_arc(&capture()).assert_clean();
}

/// The replay of the `arc_to_seg` grid (24,948 cases, one facet).
pub fn replay_arc(cap: &Capture) -> Report {
    let cap = cap.subset(|k| k.starts_with("arc/"));
    let mut replay = Replay::new(&cap);
    let centres = [0.0, 10.5, -3.0];
    let radii = [0.0, -1.0, 5.0, 100.0, f64::NAN, f64::INFINITY];
    let starts = [0.0, 5_400_000.0, 16_200_000.0, -5_400_000.0, 1e9, f64::NAN, f64::INFINITY];
    let sweeps =
        [0.0, 5_400_000.0, -5_400_000.0, 21_600_000.0, -21_600_000.0, 30_000_000.0, 1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308];
    let mut s = Stream::new();
    for cx in centres {
        for cy in centres {
            for rx in radii {
                for ry in radii {
                    for st in starts {
                        for sw in sweeps {
                            match arc_to_seg(cx, cy, rx, ry, st, sw) {
                                None => {
                                    s.tag(b'n');
                                }
                                Some((g, x, y)) => {
                                    s.tag(b'a');
                                    s.u32(1);
                                    s.u32(1);
                                    seg(&mut s, &g);
                                    f(&mut s, x);
                                    f(&mut s, y);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    replay.check("arc/grid", "v", &summary(s));
    replay.finish()
}
