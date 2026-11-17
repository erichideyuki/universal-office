//! The animation engine (`anim-engine.ts`): interprets the SMIL behaviours the application records in
//! `<p:timing>` (`set`, `anim` with keyframes and formulas, `animEffect`, `animScale`, `animRot`,
//! `animMotion`, `animClr`) and returns, for an instant, the visual state of each animated part of a
//! shape. There is no preset table: every effect plays from its own definition in the file.
//!
//! Time of each node (SMIL 2.0): local time → (`spd`) → repetition → auto-reverse → accel/decel →
//! `tmFilter` → progress. After the active end `fill="hold"` freezes the last value; without it the
//! behaviour stops applying.
//!
//! The TS is lazy (`ShapeAnimHooks`): the painter calls the hooks with the box of each part. [`ShapeHooks`]
//! is the same structure; the caches (`pathCache`, the formula cache) live in [`AnimCaches`], owned by the
//! caller and capped (M5).

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use uo_dml_render::color::CssColor;
use uo_jsnum::{conv, format::to_js_string, math, parse, sort::js_sort_by};

use crate::anim::{AnimBase, AnimFilter, PartState, UnitRef};
use crate::anim_formula::{FormulaCache, FormulaVars};
use crate::playback::PlacedEffect;
use crate::timing::{AnimTarget, NodeId, TavValue, TimeArena, TimeNode};
use crate::timing_eval::{STEP_PENDING, begin_offset};

/// Motion paths kept by [`AnimCaches`] (`uo-pptx.md` M5 table: 256).
pub const PATH_CACHE_CAP: usize = 256;
/// Longest `animMotion` path text parsed (characters); a longer one is the empty path (declared).
pub const MAX_PATH_LEN: usize = 65_536;
/// Most sampled points of one path (declared).
pub const MAX_PATH_POINTS: usize = 262_144;

// ───────────────────────── time ─────────────────────────

/// SMIL 2.0 accel/decel over the fraction `f` of the simple time.
pub fn accel_decel(f: f64, a: f64, d: f64) -> f64 {
    let (mut a, mut d) = (a, d);
    if !conv::truthy(a) && !conv::truthy(d) {
        return f;
    }
    if a + d > 1.0 {
        let k = 1.0 / (a + d);
        a *= k;
        d *= k;
    }
    let r = 1.0 / (1.0 - a / 2.0 - d / 2.0);
    if f < a {
        return (r * f * f) / (2.0 * a);
    }
    if f <= 1.0 - d {
        return r * (f - a / 2.0);
    }
    let td = 1.0 - f;
    1.0 - (r * td * td) / (2.0 * d)
}

/// `tmFilter`: piecewise linear map `[[x,y]…]` (x increasing).
fn tm_filter_map(f: f64, pts: &[(f64, f64)]) -> f64 {
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return f };
    if f <= first.0 {
        return first.1;
    }
    for w in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if f <= x1 {
            return if x1 > x0 { y0 + ((y1 - y0) * (f - x0)) / (x1 - x0) } else { y1 };
        }
    }
    last.1
}

/// Simple duration WITHOUT the `<p:iterate>` expansion (one unit).
pub fn simple_dur_of(a: &TimeArena, n: &TimeNode) -> f64 {
    if let Some(d) = n.dur {
        return d;
    }
    if &*n.tag == "seq" {
        let mut t = 0.0;
        for c in a.kids(n) {
            let cn = a.node(*c);
            let Some(b) = begin_offset(cn) else { break };
            t += b + active_dur_of(a, cn);
        }
        return t;
    }
    let mut m = 0.0;
    for c in a.kids(n) {
        let cn = a.node(*c);
        let Some(b) = begin_offset(cn) else { continue };
        m = math::max(m, b + active_dur_of(a, cn));
    }
    m
}

/// Active duration of the engine: unlike `timing.ts` it divides by `spd` (the visual is accelerated).
pub fn active_dur_of(a: &TimeArena, n: &TimeNode) -> f64 {
    let mut d = simple_dur_of(a, n) * if n.auto_rev == Some(true) { 2.0 } else { 1.0 };
    if let Some(r) = n.repeat {
        d = if r == f64::INFINITY { f64::INFINITY } else { d * r };
    }
    d / n.spd.unwrap_or(1.0)
}

/// Local time (ms since the node began) → fraction 0..1 of the simple time, with repetition,
/// auto-reverse, accel/decel and `tmFilter`. `None` = ended without holding. Indefinite → 0.
fn progress_of(n: &TimeNode, local: f64, simple: f64) -> Option<f64> {
    if !simple.is_finite() {
        return Some(0.0);
    }
    let t = local * n.spd.unwrap_or(1.0);
    let auto_rev = n.auto_rev == Some(true);
    let period = simple * if auto_rev { 2.0 } else { 1.0 };
    let reps = n.repeat.unwrap_or(1.0);
    let active = if reps == f64::INFINITY { f64::INFINITY } else { period * reps };
    let it = if t >= active {
        if n.fill.as_deref() != Some("hold") && n.fill.as_deref() != Some("freeze") {
            return None;
        }
        let frac = reps - math::floor(reps);
        if frac > 1e-9 { frac * period } else { period }
    } else if period > 0.0 {
        math::rem(t, period)
    } else {
        0.0
    };
    let s = if auto_rev && it > simple { 2.0 * simple - it } else { math::min(it, simple) };
    let mut f = if simple > 0.0 { s / simple } else { 1.0 };
    f = accel_decel(f, n.accel.unwrap_or(0.0), n.decel.unwrap_or(0.0));
    if let Some(pts) = &n.tm_filter {
        f = tm_filter_map(f, pts);
    }
    Some(f)
}

// ───────────────────────── colours ─────────────────────────

fn hex_rgb(h: &str) -> [f64; 3] {
    // `parseInt(h.replace('#', ''), 16)` then `(n >> 16) & 255`…
    let t = h.replacen('#', "", 1);
    let n = conv::to_int32(parse::parse_int(&t, 16));
    [f64::from((n >> 16) & 255), f64::from((n >> 8) & 255), f64::from(n & 255)]
}

/// `(v).toString(16).padStart(2, '0')` of `Math.max(0, Math.min(255, Math.round(v)))` (NaN prints "NaN").
fn hex2(v: f64, out: &mut String) {
    let c = math::max(0.0, math::min(255.0, math::round(v)));
    if c.is_nan() {
        out.push_str("NaN");
        return;
    }
    const D: &[u8; 16] = b"0123456789abcdef";
    // c is an integer in 0..=255 here
    let n = conv::to_uint32(c) & 255;
    out.push(char::from(D[(n >> 4) as usize]));
    out.push(char::from(D[(n & 15) as usize]));
}

fn rgb_hex(r: f64, g: f64, b: f64) -> String {
    let mut s = String::from("#");
    hex2(r, &mut s);
    hex2(g, &mut s);
    hex2(b, &mut s);
    s
}

fn rgb_hsl(rgb: [f64; 3]) -> [f64; 3] {
    let (r, g, b) = (rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0);
    let mx = math::max(math::max(r, g), b);
    let mn = math::min(math::min(r, g), b);
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let mut h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h *= 60.0;
    [h, s, l]
}

fn hsl_hex(h: f64, s: f64, l: f64) -> String {
    let h = math::rem(math::rem(h, 360.0) + 360.0, 360.0);
    let c = (1.0 - math::abs(2.0 * l - 1.0)) * s;
    let x = c * (1.0 - math::abs(math::rem(h / 60.0, 2.0) - 1.0));
    let m = l - c / 2.0;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    rgb_hex((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

fn mix_color(a: &str, b: &str, f: f64, n: &TimeNode) -> String {
    if n.clr_spc.as_deref() == Some("hsl") {
        let [h1, s1, l1] = rgb_hsl(hex_rgb(a));
        let [h2, s2, l2] = rgb_hsl(hex_rgb(b));
        let mut dh = h2 - h1;
        if n.clr_dir.as_deref() == Some("ccw") {
            if dh > 0.0 {
                dh -= 360.0;
            }
        } else if dh < 0.0 {
            dh += 360.0;
        }
        return hsl_hex(h1 + dh * f, s1 + (s2 - s1) * f, l1 + (l2 - l1) * f);
    }
    let [r1, g1, b1] = hex_rgb(a);
    let [r2, g2, b2] = hex_rgb(b);
    rgb_hex(r1 + (r2 - r1) * f, g1 + (g2 - g1) * f, b1 + (b2 - b1) * f)
}

// ───────────────────────── motion paths ─────────────────────────

/// A sampled `animMotion` path (fractions of the slide) with cumulative lengths.
#[derive(Debug)]
pub struct MotionPath {
    pts: Vec<[f64; 2]>,
    cum: Vec<f64>,
    total: f64,
}

/// The caches of the engine, owned by the caller (the deck view; `Deck::clear_caches` empties them):
/// `pathCache` and the formula cache of `anim-formula.ts`, both capped (M5).
#[derive(Debug)]
pub struct AnimCaches {
    paths: HashMap<Box<str>, Rc<MotionPath>>,
    path_order: VecDeque<Box<str>>,
    path_cap: usize,
    /// The parsed formulas.
    pub formulas: FormulaCache,
}

impl Default for AnimCaches {
    fn default() -> Self {
        AnimCaches::new()
    }
}

impl AnimCaches {
    pub fn new() -> AnimCaches {
        AnimCaches { paths: HashMap::new(), path_order: VecDeque::new(), path_cap: PATH_CACHE_CAP, formulas: FormulaCache::new() }
    }

    /// Caches with other caps (tests of the fill behaviour).
    pub fn with_caps(path_cap: usize, formula_cap: usize) -> AnimCaches {
        AnimCaches {
            paths: HashMap::new(),
            path_order: VecDeque::new(),
            path_cap: path_cap.max(1),
            formulas: FormulaCache::with_cap(formula_cap),
        }
    }

    /// Paths held.
    pub fn path_len(&self) -> usize {
        self.paths.len()
    }

    /// Drops everything (and the maps' capacity: live bytes return to the start, M1).
    pub fn clear(&mut self) {
        self.paths = HashMap::new();
        self.path_order = VecDeque::new();
        self.formulas.clear();
    }

    fn motion_path(&mut self, src: &str) -> Rc<MotionPath> {
        if let Some(p) = self.paths.get(src) {
            return p.clone();
        }
        let mp = Rc::new(parse_motion_path(src));
        while self.paths.len() >= self.path_cap {
            match self.path_order.pop_front() {
                Some(old) => {
                    self.paths.remove(&old);
                }
                None => break,
            }
        }
        self.paths.insert(src.into(), mp.clone());
        self.path_order.push_back(src.into());
        mp
    }
}

/// The tokens of `/[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?|[MLCZEmlcze]/g`.
fn path_tokens(src: &str) -> Vec<&str> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let digits = |from: usize| b[from.min(b.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    while i < b.len() {
        // alternative 1: a number
        let mut j = i;
        if matches!(b[j], b'-' | b'+') {
            j += 1;
        }
        let int = digits(j);
        let mut ok = false;
        if int > 0 {
            j += int;
            if b.get(j) == Some(&b'.') {
                j += 1;
                j += digits(j);
            }
            ok = true;
        } else if b.get(j) == Some(&b'.') {
            let f = digits(j + 1);
            if f > 0 {
                j += 1 + f;
                ok = true;
            }
        }
        if ok {
            if matches!(b.get(j), Some(b'e' | b'E')) {
                let mut k = j + 1;
                if matches!(b.get(k), Some(b'-' | b'+')) {
                    k += 1;
                }
                let e = digits(k);
                if e > 0 {
                    j = k + e;
                }
            }
            if let Some(t) = src.get(i..j) {
                out.push(t);
            }
            i = j;
            continue;
        }
        // alternative 2: a command letter
        if matches!(b[i], b'M' | b'L' | b'C' | b'Z' | b'E' | b'm' | b'l' | b'c' | b'z' | b'e')
            && let Some(t) = src.get(i..=i)
        {
            out.push(t);
        }
        i += 1;
    }
    out
}

/// Samples the `animMotion` path (fractions of the slide) for constant pace. As in SVG, capitals are absolute
/// and lower case RELATIVE to the current point.
fn parse_motion_path(src: &str) -> MotionPath {
    let tok = if src.len() > MAX_PATH_LEN { Vec::new() } else { path_tokens(src) };
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let mut i = 0usize;
    let mut cmd = b'M';
    let mut rel = false;
    let mut cur = [0.0, 0.0];
    let mut start = [0.0, 0.0];
    let n = |i: &mut usize| -> f64 {
        let v = parse::parse_float(tok.get(*i).copied().unwrap_or("0"));
        *i += 1;
        v
    };
    let pt = |i: &mut usize, rel: bool, o: [f64; 2]| -> [f64; 2] {
        let x = n(i);
        let y = n(i);
        if rel { [o[0] + x, o[1] + y] } else { [x, y] }
    };
    while i < tok.len() && pts.len() < MAX_PATH_POINTS {
        let t = tok[i];
        if t.len() == 1 && t.as_bytes()[0].is_ascii_alphabetic() {
            let c = t.as_bytes()[0];
            i += 1;
            cmd = c.to_ascii_uppercase();
            rel = c != cmd;
        }
        // Z has no operands: a stray number after it is dropped
        if cmd == b'Z' {
            pts.push(start);
            cur = start;
            cmd = 0;
            continue;
        }
        if cmd == b'E' {
            break;
        }
        if cmd == b'M' {
            cur = pt(&mut i, rel, cur);
            start = cur;
            pts.push(cur);
            cmd = b'L';
            continue;
        }
        if cmd == b'L' {
            cur = pt(&mut i, rel, cur);
            pts.push(cur);
            continue;
        }
        if cmd == b'C' {
            let p0 = cur;
            let p1 = pt(&mut i, rel, p0);
            let p2 = pt(&mut i, rel, p0);
            let p3 = pt(&mut i, rel, p0);
            for k in 1..=16u32 {
                let t = f64::from(k) / 16.0;
                let u = 1.0 - t;
                pts.push([
                    u * u * u * p0[0] + 3.0 * u * u * t * p1[0] + 3.0 * u * t * t * p2[0] + t * t * t * p3[0],
                    u * u * u * p0[1] + 3.0 * u * u * t * p1[1] + 3.0 * u * t * t * p2[1] + t * t * t * p3[1],
                ]);
            }
            cur = p3;
            continue;
        }
        i += 1; // unknown token
    }
    if pts.is_empty() {
        pts.push([0.0, 0.0]);
    }
    let mut cum = vec![0.0];
    for k in 1..pts.len() {
        let prev = cum[k - 1];
        cum.push(prev + math::hypot(pts[k][0] - pts[k - 1][0], pts[k][1] - pts[k - 1][1]));
    }
    let total = cum[cum.len() - 1];
    MotionPath { pts, cum, total }
}

/// The point on the path at the fraction `f` of the LENGTH (`animMotion` is paced).
fn point_on_path(mp: &MotionPath, f: f64) -> [f64; 2] {
    if mp.total <= 0.0 || mp.pts.len() < 2 {
        return mp.pts[0];
    }
    let d = math::max(0.0, math::min(1.0, f)) * mp.total;
    let mut k = 1;
    while k < mp.cum.len() - 1 && mp.cum[k] < d {
        k += 1;
    }
    let (d0, d1) = (mp.cum[k - 1], mp.cum[k]);
    let t = if d1 > d0 { (d - d0) / (d1 - d0) } else { 0.0 };
    let (a, b) = (mp.pts[k - 1], mp.pts[k]);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

// ───────────────────────── behaviours ─────────────────────────

fn vars(base: &AnimBase) -> FormulaVars {
    // EXIT effects write the variables without '#': "ppt_w", "1+ppt_h/2"
    FormulaVars { x: base.x, y: base.y, w: base.w, h: base.h, r: base.rot.unwrap_or(0.0), dollar: None }
}

/// Numeric attributes (the values of `set` can be formulas: "(#ppt_y+0.4)").
fn is_numeric_attr(a: &str) -> bool {
    matches!(a, "ppt_x" | "ppt_y" | "ppt_w" | "ppt_h" | "style.rotation" | "r" | "style.opacity" | "style.fontSize")
}

/// A value of an attribute: number or text.
#[derive(Clone, PartialEq, Debug)]
enum V {
    Num(f64),
    Str(String),
}

fn num_val(v: Option<&TavValue>, vr: &FormulaVars, c: &mut AnimCaches) -> f64 {
    match v {
        None => f64::NAN,
        Some(TavValue::Num(n)) => *n,
        Some(TavValue::Str(s)) => c.formulas.eval(s, vr),
    }
}

fn num_val_str(v: &str, vr: &FormulaVars, c: &mut AnimCaches) -> f64 {
    c.formulas.eval(v, vr)
}

/// The CURRENT value of an attribute (the start of an `anim` without `from`).
fn current_of(attr: &str, st: &PartState) -> Option<V> {
    Some(match attr {
        "ppt_x" => V::Num(st.x),
        "ppt_y" => V::Num(st.y),
        "ppt_w" => V::Num(st.w),
        "ppt_h" => V::Num(st.h),
        "style.rotation" | "r" => V::Num(st.rot),
        "style.opacity" => V::Num(st.opacity),
        "style.fontSize" => V::Num(st.font_scale.unwrap_or(1.0)),
        "style.visibility" => V::Str(if st.visible { "visible" } else { "hidden" }.into()),
        _ => return None,
    })
}

fn v_string(v: &V) -> String {
    match v {
        V::Num(n) => to_js_string(*n),
        V::Str(s) => s.clone(),
    }
}

/// Assigns `v` to the animated attribute.
fn assign(attr: &str, v: &V, st: &mut PartState) {
    let n = match v {
        V::Num(n) => *n,
        V::Str(s) => parse::parse_float(s),
    };
    match attr {
        "ppt_x" => {
            if n.is_finite() {
                st.x = n;
            }
        }
        "ppt_y" => {
            if n.is_finite() {
                st.y = n;
            }
        }
        "ppt_w" => {
            if n.is_finite() {
                st.w = n;
            }
        }
        "ppt_h" => {
            if n.is_finite() {
                st.h = n;
            }
        }
        "style.rotation" | "r" => {
            if n.is_finite() {
                st.rot = n;
            }
        }
        "style.opacity" => {
            if n.is_finite() {
                st.opacity = math::max(0.0, math::min(1.0, n));
            }
        }
        "style.visibility" => st.visible = v_string(v) != "hidden",
        "style.fontSize" => {
            if n.is_finite() {
                st.font_scale = Some(n);
            }
        }
        "style.fontWeight" => st.font_weight = Some(v_string(v).into()),
        "style.fontStyle" => st.font_style = Some(v_string(v).into()),
        "style.textDecorationUnderline" => st.underline = Some(v_string(v) == "true"),
        "style.fontFamily" => st.font_family = Some(v_string(v).into()),
        "fillcolor" | "fillColor" | "fill.color" => st.fill_color = Some(CssColor::new(&v_string(v))),
        "stroke.color" | "strokecolor" => st.line_color = Some(CssColor::new(&v_string(v))),
        "style.color" => st.text_color = Some(CssColor::new(&v_string(v))),
        _ => {} // fill.on, fill.type, stroke.on, ppt_c…: no visual effect of their own
    }
}

fn color_of(attr: &str, st: &PartState, base: &AnimBase) -> Option<String> {
    match attr {
        "fillcolor" | "fillColor" | "fill.color" => st.fill_color.as_ref().or(base.fill.as_ref()).map(|c| c.as_str().to_owned()),
        "stroke.color" | "strokecolor" => st.line_color.as_ref().or(base.line.as_ref()).map(|c| c.as_str().to_owned()),
        "style.color" => st.text_color.as_ref().or(base.text.as_ref()).map(|c| c.as_str().to_owned()),
        _ => None,
    }
}

fn clamp01(v: f64) -> f64 {
    math::max(0.0, math::min(1.0, v))
}

fn truthy_color(c: &Option<CssColor>) -> Option<&str> {
    c.as_ref().map(CssColor::as_str).filter(|s| !s.is_empty())
}

/// `/opacity:\s*([\d.]+)/.exec(s)`'s first group.
fn image_opacity(s: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(p) = s[from..].find("opacity:") {
        let after = from + p + "opacity:".len();
        let rest = s[after..].trim_start_matches(uo_jsnum::text::is_js_space);
        let n = rest.bytes().take_while(|c| c.is_ascii_digit() || *c == b'.').count();
        if n > 0 {
            return Some(&rest[..n]);
        }
        from = after;
    }
    None
}

/// Applies ONE behaviour at progress `p` to the state.
fn apply_behavior(n: &TimeNode, p: f64, st: &mut PartState, base: &AnimBase, c: &mut AnimCaches) {
    let attrs: &[Box<str>] = n.attr_names.as_deref().unwrap_or(&[]);
    let vr = vars(base);
    match &*n.tag {
        "set" => {
            for a in attrs {
                if let Some(tc) = truthy_color(&n.to_color) {
                    assign(a, &V::Str(tc.to_owned()), st);
                } else if let Some(set_to) = &n.set_to {
                    let v = if is_numeric_attr(a) { num_val_str(set_to, &vr, c) } else { f64::NAN };
                    if v.is_finite() {
                        assign(a, &V::Num(v), st);
                    } else {
                        assign(a, &V::Str(set_to.to_string()), st);
                    }
                }
            }
        }
        "anim" => {
            for a in attrs {
                let mut v: Option<V> = None;
                match n.tav.as_deref().filter(|t| !t.is_empty()) {
                    Some(tav) => {
                        // at the end (p ≥ last tm) the LAST SEGMENT stays at f=1: the formula `fmla` lives on
                        // the START frame of the segment
                        let mut i = tav.len().saturating_sub(2);
                        for k in 0..tav.len() - 1 {
                            if p < tav[k + 1].tm {
                                i = k;
                                break;
                            }
                        }
                        let ka = &tav[i];
                        let kb = &tav[(i + 1).min(tav.len() - 1)];
                        // DISCRETE (and text): the LAST frame with tm ≤ p
                        let mut kd = 0;
                        for (k, key) in tav.iter().enumerate().skip(1) {
                            if key.tm <= p {
                                kd = k;
                            }
                        }
                        let kq = &tav[kd];
                        let discrete = n.calcmode.as_deref() == Some("discrete") || n.value_type.as_deref() == Some("str");
                        if truthy_color(&ka.color).is_some() || truthy_color(&kb.color).is_some() {
                            let ca = truthy_color(&ka.color)
                                .map(str::to_owned)
                                .or_else(|| color_of(a, st, base))
                                .unwrap_or_else(|| "#000000".into());
                            let cb = truthy_color(&kb.color).map_or_else(|| ca.clone(), str::to_owned);
                            let f = if kb.tm > ka.tm { (p - ka.tm) / (kb.tm - ka.tm) } else { 0.0 };
                            v = Some(V::Str(if discrete {
                                truthy_color(&kq.color)
                                    .map(str::to_owned)
                                    .or_else(|| color_of(a, st, base))
                                    .unwrap_or_else(|| "#000000".into())
                            } else {
                                mix_color(&ca, &cb, clamp01(f), n)
                            }));
                        } else {
                            let na = num_val(ka.val.as_ref(), &vr, c);
                            let nb = num_val(kb.val.as_ref(), &vr, c);
                            if discrete || !na.is_finite() {
                                // discrete / text (bold/normal/visible…): the active frame, no interpolation
                                let xq = num_val(kq.val.as_ref(), &vr, c);
                                v = if !xq.is_finite() {
                                    kq.val.as_ref().map(|x| match x {
                                        TavValue::Num(n) => V::Num(*n),
                                        TavValue::Str(s) => V::Str(s.to_string()),
                                    })
                                } else if let Some(f) = kq.fmla.as_deref().filter(|f| !f.is_empty()) {
                                    Some(V::Num(c.formulas.eval(f, &FormulaVars { dollar: Some(xq), ..vr })))
                                } else {
                                    Some(V::Num(xq))
                                };
                            } else {
                                let f = if kb.tm > ka.tm { clamp01((p - ka.tm) / (kb.tm - ka.tm)) } else { 0.0 };
                                let mut x = if !nb.is_finite() { na } else { na + (nb - na) * f };
                                if let Some(fm) = ka.fmla.as_deref().filter(|f| !f.is_empty()) {
                                    x = c.formulas.eval(fm, &FormulaVars { dollar: Some(x), ..vr });
                                }
                                v = Some(V::Num(x));
                            }
                        }
                    }
                    None => {
                        let cur = current_of(a, st);
                        let from = match &n.from {
                            Some(f) => num_val_str(f, &vr, c),
                            None => match &cur {
                                Some(V::Num(x)) => *x,
                                _ => f64::NAN,
                            },
                        };
                        if let Some(to_s) = &n.to {
                            let to = num_val_str(to_s, &vr, c);
                            v = if to.is_finite() && from.is_finite() {
                                Some(V::Num(from + (to - from) * p))
                            } else if p > 0.0 {
                                Some(V::Str(to_s.to_string()))
                            } else {
                                Some(cur.unwrap_or_else(|| V::Str(to_s.to_string())))
                            };
                        } else if let Some(by_s) = &n.by {
                            let by = num_val_str(by_s, &vr, c);
                            if by.is_finite() && from.is_finite() {
                                v = Some(V::Num(from + by * p));
                            }
                        }
                    }
                }
                if let Some(v) = v
                    && (matches!(v, V::Str(_)) || matches!(v, V::Num(x) if x.is_finite()))
                {
                    assign(a, &v, st);
                }
            }
        }
        "animEffect" => {
            let Some(filter) = n.filter.as_deref().filter(|f| !f.is_empty()) else { return };
            if filter == "image" {
                if let Some(m) = image_opacity(n.pr_lst.as_deref().unwrap_or("")) {
                    st.opacity = math::min(st.opacity, parse::parse_float(m));
                }
                return;
            }
            st.filters.push(AnimFilter { filter: filter.into(), transition: n.transition.clone().unwrap_or_else(|| "in".into()), p });
        }
        "animScale" => {
            let from = n.from_xy.unwrap_or((st.sx, st.sy));
            let to = n.to_xy.unwrap_or_else(|| n.by_xy.map_or(from, |b| (from.0 * b.0, from.1 * b.1)));
            st.sx = from.0 + (to.0 - from.0) * p;
            st.sy = from.1 + (to.1 - from.1) * p;
        }
        "animRot" => {
            if n.rot_from.is_some() || n.rot_to.is_some() {
                let from = n.rot_from.unwrap_or(st.rot);
                let to = n.rot_to.unwrap_or(from);
                st.rot = from + (to - from) * p;
            } else if let Some(by) = n.rot_by {
                st.rot += by * p;
            }
        }
        "animMotion" => {
            if let Some(path) = n.path.as_deref().filter(|p| !p.is_empty()) {
                // pathEditMode relative: the path IS ALREADY an offset from the original position
                let mp = c.motion_path(path);
                let [mut px, mut py] = point_on_path(&mp, p);
                if n.r_ang.is_some_and(conv::truthy) {
                    let a = (n.r_ang.unwrap_or(0.0) * core::f64::consts::PI) / 180.0;
                    let (cs, sn) = (math::cos(a), math::sin(a));
                    (px, py) = (px * cs - py * sn, px * sn + py * cs);
                }
                st.mx += px;
                st.my += py;
            } else {
                let from = n.from_xy.unwrap_or((0.0, 0.0));
                let to = n.to_xy.unwrap_or_else(|| n.by_xy.map_or(from, |b| (from.0 + b.0, from.1 + b.1)));
                st.mx += from.0 + (to.0 - from.0) * p;
                st.my += from.1 + (to.1 - from.1) * p;
            }
        }
        "animClr" => {
            for a in attrs {
                let from_c =
                    truthy_color(&n.from_color).map(str::to_owned).or_else(|| color_of(a, st, base)).unwrap_or_else(|| "#000000".into());
                let mut v: Option<String> = None;
                if let Some(to) = truthy_color(&n.to_color) {
                    v = Some(mix_color(&from_c, to, p, n));
                } else if let Some((bh, bs, bl)) = n.by_hsl {
                    let [h, s, l] = rgb_hsl(hex_rgb(&from_c));
                    let sign = if n.clr_dir.as_deref() == Some("ccw") { -1.0 } else { 1.0 };
                    v = Some(hsl_hex(h + sign * bh * p, clamp01(s + bs * p), clamp01(l + bl * p)));
                }
                if let Some(v) = v.filter(|s| !s.is_empty()) {
                    assign(a, &V::Str(v), st);
                }
            }
        }
        _ => {} // cmd (media), audio/video
    }
}

/// Evaluates a node (container or behaviour) at the local time `local`.
fn eval_node(a: &TimeArena, id: NodeId, local: f64, st: &mut PartState, base: &AnimBase, c: &mut AnimCaches) {
    let n = a.node(id);
    let container = matches!(&*n.tag, "par" | "seq" | "excl");
    let simple = simple_dur_of(a, n);
    let Some(f) = progress_of(n, local, simple) else { return };
    if !container {
        apply_behavior(n, f, st, base, c);
        return;
    }
    let t = if simple.is_finite() { f * simple } else { local };
    if &*n.tag == "seq" {
        let mut c0 = 0.0;
        for k in a.kids(n) {
            let kn = a.node(*k);
            let Some(b) = begin_offset(kn) else { break };
            c0 += b;
            if t < c0 {
                break;
            }
            eval_node(a, *k, t - c0, st, base, c);
            c0 += active_dur_of(a, kn);
            // an indefinite child: the following ones never begin (Infinity − Infinity = NaN)
            if !c0.is_finite() {
                break;
            }
        }
        return;
    }
    for k in a.kids(n) {
        let Some(b) = begin_offset(a.node(*k)) else { continue };
        if t < b {
            continue;
        }
        eval_node(a, *k, t - b, st, base, c);
    }
}

// ───────────────────────── chart parts ─────────────────────────

/// The selection of a chart part's drawing (`ChartSel` of `chart-engine.ts`): the background and/or the
/// data marks `(series, category)` that satisfy [`ChartData::matches`].
#[derive(Clone, PartialEq, Debug)]
pub struct ChartSel {
    pub key: Option<Box<str>>,
    pub bg: bool,
    pub data: ChartData,
}

/// The `data` predicate of a [`ChartSel`].
#[derive(Clone, PartialEq, Debug)]
pub enum ChartData {
    /// `null`: no data marks
    None,
    /// `() => true`
    All,
    /// `(s) => s === series`
    Series(f64),
    /// `(_s, k) => k === category`
    Category(f64),
    /// `(s, k) => (series < 0 || s === series) && (category < 0 || k === category)`
    Pt { series: f64, category: f64 },
    /// `(s, k) => !covered(s, k)`: no other part's predicate holds
    Uncovered(Vec<ChartData>),
}

impl ChartData {
    pub fn matches(&self, s: f64, k: f64) -> bool {
        match self {
            ChartData::None => false,
            ChartData::All => true,
            ChartData::Series(c) => s == *c,
            ChartData::Category(c) => k == *c,
            ChartData::Pt { series, category } => (*series < 0.0 || s == *series) && (*category < 0.0 || k == *category),
            ChartData::Uncovered(parts) => !parts.iter().any(|d| d.matches(s, k)),
        }
    }
}

/// `chartSel` (`anim-engine.ts:518`): a chart part (`<a:chart seriesIdx categoryIdx bldStep>`) → drawing selection.
fn chart_sel(series: f64, category: f64, step: &str) -> ChartSel {
    let key: Box<str> = format!("{step}|{}|{}", to_js_string(series), to_js_string(category)).into();
    match step {
        "gridLegend" => ChartSel { key: Some(key), bg: true, data: ChartData::None },
        "series" => ChartSel { key: Some(key), bg: false, data: ChartData::Series(series) },
        "category" => ChartSel { key: Some(key), bg: false, data: ChartData::Category(category) },
        "ptInSeries" | "ptInCategory" => ChartSel { key: Some(key), bg: false, data: ChartData::Pt { series, category } },
        // allPts: the whole chart
        _ => ChartSel { key: Some(key), bg: true, data: ChartData::All },
    }
}

// ───────────────────────── layers / initial visibility ─────────────────────────

/// First `set style.visibility` of the effect: 'visible' (entrance) | 'hidden' (exit) | none.
fn first_visibility(a: &TimeArena, id: NodeId) -> Option<&str> {
    fn walk<'a>(a: &'a TimeArena, id: NodeId, t0: f64, best: &mut Option<(f64, &'a str)>) {
        let x = a.node(id);
        if &*x.tag == "set"
            && x.attr_names.as_ref().is_some_and(|n| n.iter().any(|s| &**s == "style.visibility"))
            && let Some(to) = x.set_to.as_deref().filter(|s| !s.is_empty())
        {
            if best.is_none_or(|(bt, _)| t0 < bt) {
                *best = Some((t0, to));
            }
            return;
        }
        for c in a.kids(x) {
            if let Some(b) = begin_offset(a.node(*c)) {
                walk(a, *c, t0 + b, best);
            }
        }
    }
    let mut best = None;
    walk(a, id, 0.0, &mut best);
    best.map(|(_, v)| v)
}

/// The neutral state of a part (before any effect).
fn neutral(base: &AnimBase, visible: bool) -> PartState {
    PartState { visible, opacity: 1.0, x: base.x, y: base.y, w: base.w, h: base.h, sx: 1.0, sy: 1.0, ..PartState::default() }
}

#[derive(Clone, Copy)]
struct Track<'a> {
    e: &'a PlacedEffect,
    /// Extra start offset (ms) — staggered units.
    shift: f64,
}

/// Evaluates a list of effects (ordered by start) for a part.
fn eval_tracks(
    a: &TimeArena,
    tracks: &[Track<'_>],
    anim_ms: f64,
    base: &AnimBase,
    initially_visible: bool,
    c: &mut AnimCaches,
) -> PartState {
    let mut st = neutral(base, initially_visible);
    for tr in tracks {
        if tr.e.t0 >= STEP_PENDING {
            continue; // pending (click/trigger): only the initial state
        }
        let mut local = anim_ms - tr.e.t0 - tr.shift;
        if local < 0.0 {
            continue;
        }
        // forced end ("until next click"): freezes at the instant of the end (fill remove = gone for good)
        if let Some(end_at) = tr.e.end_at
            && anim_ms >= end_at + tr.shift
        {
            if tr.e.fx.fill.as_deref() == Some("remove") {
                continue;
            }
            local = end_at - tr.e.t0;
        }
        eval_node(a, tr.e.fx.node, local, &mut st, base, c);
    }
    st
}

/// Initial visibility: hidden if the first visibility change (in time) is an ENTRANCE.
fn initial_visible(a: &TimeArena, tracks: &[Track<'_>]) -> bool {
    let mut sorted: Vec<Track<'_>> = tracks.to_vec();
    js_sort_by(&mut sorted, |x, y| (x.e.t0 + x.shift) - (y.e.t0 + y.shift));
    for tr in &sorted {
        if let Some(v) = first_visibility(a, tr.e.fx.node) {
            return v != "visible";
        }
    }
    true
}

// ───────────────────────── hooks ─────────────────────────

/// `ShapeAnimContext`: the shape's background animation test, the SmartArt drawing shape and the paragraph count.
#[derive(Default)]
pub struct ShapeAnimContext<'a> {
    /// The shape's background is animated with the text in per-letter/per-word effects (`bldP animBg`).
    pub anim_bg: Option<&'a dyn Fn(&str) -> bool>,
    /// The drawing shape of a SmartArt (`dsp:sp modelId`): `<a:dgm id>` effects of other shapes do not apply.
    pub dgm_id: Option<&'a str>,
    /// Paragraphs of the txBody (not used by the TS engine either).
    pub para_count: Option<u32>,
}

struct UnitEffect<'a> {
    e: &'a PlacedEffect,
    range: Option<(f64, f64)>,
    bg_first: bool,
}

/// One chart part's hook: its drawing selection and its state function (`None` = static layer).
pub struct ChartPartHook<'a> {
    pub sel: ChartSel,
    tracks: Option<(Vec<Track<'a>>, bool)>,
}

impl ChartPartHook<'_> {
    /// Whether the part has an animated state.
    pub fn is_animated(&self) -> bool {
        self.tracks.is_some()
    }
}

/// The text-unit hook (`ShapeAnimHooks.unit`): the granularity and whether a word effect exists too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitInfo {
    /// `lt` when some effect is per letter, else `wd`.
    pub letters: bool,
    pub words: bool,
}

/// The lazy hooks of an animated shape (`ShapeAnimHooks`). Borrows the placed effects and the node arena.
pub struct ShapeHooks<'a> {
    arena: &'a TimeArena,
    anim_ms: f64,
    all: Vec<Track<'a>>,
    bg: Option<(Vec<Track<'a>>, bool)>,
    paras: Vec<(Track<'a>, (f64, f64))>,
    units: Vec<UnitEffect<'a>>,
    unit_info: Option<UnitInfo>,
    chart: Vec<ChartPartHook<'a>>,
}

fn target_matches(t: &AnimTarget, ids: &[&str], dgm: Option<&str>) -> bool {
    ids.contains(&&*t.spid) && t.dgm.as_deref().is_none_or(|d| d.is_empty() || Some(d) == dgm)
}

/// `shapeAnimHooks` (`anim-engine.ts:604`): the hooks of a shape at `anim_ms`. `ids` = the shape's id + the
/// ids of the groups that contain it. `None` when no effect of the slide touches the shape.
pub fn shape_anim_hooks<'a>(
    effects: &'a [PlacedEffect],
    arena: &'a TimeArena,
    ids: &[&str],
    anim_ms: f64,
    ctx: &ShapeAnimContext<'_>,
) -> Option<ShapeHooks<'a>> {
    let mut mine: Vec<&'a PlacedEffect> =
        effects.iter().filter(|e| e.fx.target.as_ref().is_some_and(|t| target_matches(t, ids, ctx.dgm_id))).collect();
    js_sort_by(&mut mine, |x, y| x.t0 - y.t0);
    if mine.is_empty() {
        return None;
    }
    let mut chart: Vec<(Box<str>, ChartSel, Vec<Track<'a>>)> = Vec::new();
    let mut all: Vec<Track<'a>> = Vec::new();
    let mut bg: Vec<Track<'a>> = Vec::new();
    let mut paras: Vec<(Track<'a>, (f64, f64))> = Vec::new();
    let mut units: Vec<UnitEffect<'a>> = Vec::new();
    for e in mine {
        let Some(t) = e.fx.target.as_ref() else { continue };
        let it = arena.node(e.fx.node).iterate.as_ref();
        if let Some(ch) = &t.chart {
            let key: Box<str> = format!("{}|{}|{}", ch.step, to_js_string(ch.series), to_js_string(ch.category)).into();
            match chart.iter_mut().find(|(k, _, _)| *k == key) {
                Some((_, _, tr)) => tr.push(Track { e, shift: 0.0 }),
                None => chart.push((key, chart_sel(ch.series, ch.category, &ch.step), vec![Track { e, shift: 0.0 }])),
            }
        } else if t.sub.as_deref() == Some("bg") {
            bg.push(Track { e, shift: 0.0 });
        } else if it.is_some_and(|i| &*i.kind == "wd" || &*i.kind == "lt") {
            // the background unit only with the WHOLE SHAPE as the target (as in the timing model): a range
            // of paragraphs/characters does not take the background
            let bg_first = t.p_range.is_none() && t.char_range.is_none() && t.sub.is_none() && ctx.anim_bg.is_some_and(|f| f(&t.spid));
            units.push(UnitEffect { e, range: t.p_range, bg_first });
            if bg_first {
                bg.push(Track { e, shift: 0.0 });
            }
        } else if let Some(r) = t.p_range {
            paras.push((Track { e, shift: 0.0 }, r));
        } else {
            all.push(Track { e, shift: 0.0 });
        }
    }
    let bg_hook = if bg.is_empty() {
        None
    } else {
        let vis = initial_visible(arena, &bg);
        Some((bg, vis))
    };
    let mut chart_hooks: Vec<ChartPartHook<'a>> = Vec::new();
    if !chart.is_empty() {
        // background/marks without their own effect stay static (extra layers)
        if !chart.iter().any(|(_, s, _)| s.bg) {
            chart_hooks.push(ChartPartHook { sel: ChartSel { key: None, bg: true, data: ChartData::None }, tracks: None });
        }
        let covered: Vec<ChartData> = chart.iter().filter(|(_, s, _)| s.data != ChartData::None).map(|(_, s, _)| s.data.clone()).collect();
        chart_hooks.push(ChartPartHook { sel: ChartSel { key: None, bg: false, data: ChartData::Uncovered(covered) }, tracks: None });
        for (_, sel, tracks) in chart {
            let vis = initial_visible(arena, &tracks);
            chart_hooks.push(ChartPartHook { sel, tracks: Some((tracks, vis)) });
        }
    }
    let unit_info = if units.is_empty() {
        None
    } else {
        // per letter AND per word in the same shape: draws per letter; the per-word effect staggers by word
        let kind_of = |u: &UnitEffect<'a>| arena.node(u.e.fx.node).iterate.as_ref().map(|i| i.kind.clone());
        let has_lt = units.iter().any(|u| kind_of(u).as_deref() == Some("lt"));
        let has_wd = units.iter().any(|u| kind_of(u).as_deref() == Some("wd"));
        Some(UnitInfo { letters: has_lt, words: has_lt && has_wd })
    };
    Some(ShapeHooks { arena, anim_ms, all, bg: bg_hook, paras, units, unit_info, chart: chart_hooks })
}

impl<'a> ShapeHooks<'a> {
    /// The whole shape (background + text).
    pub fn all(&self, base: &AnimBase, c: &mut AnimCaches) -> PartState {
        eval_tracks(self.arena, &self.all, self.anim_ms, base, initial_visible(self.arena, &self.all), c)
    }

    /// Whether a background hook exists (`hooks.bg`).
    pub fn has_bg(&self) -> bool {
        self.bg.is_some()
    }

    /// The background only, over `all`.
    pub fn bg(&self, base: &AnimBase, c: &mut AnimCaches) -> Option<PartState> {
        let (tracks, vis) = self.bg.as_ref()?;
        Some(eval_tracks(self.arena, tracks, self.anim_ms, base, *vis, c))
    }

    /// Whether a paragraph hook exists (`hooks.para`).
    pub fn has_para(&self) -> bool {
        !self.paras.is_empty()
    }

    /// The text of paragraph `i`, `None` = no effect on it.
    pub fn para(&self, i: u32, base: &AnimBase, c: &mut AnimCaches) -> Option<PartState> {
        let i = f64::from(i);
        let tr: Vec<Track<'a>> = self.paras.iter().filter(|(_, r)| i >= r.0 && i <= r.1).map(|(t, _)| *t).collect();
        if tr.is_empty() {
            return None;
        }
        Some(eval_tracks(self.arena, &tr, self.anim_ms, base, initial_visible(self.arena, &tr), c))
    }

    /// The unit hook's granularity (`hooks.unit.type`, `.words`), `None` = no unit effect.
    pub fn unit_info(&self) -> Option<UnitInfo> {
        self.unit_info
    }

    /// The state of a letter/word (`hooks.unit.state`); `word` is the word of a letter when `words` is set.
    pub fn unit_state(&self, unit: UnitRef, base: &AnimBase, word: Option<UnitRef>, c: &mut AnimCaches) -> Option<PartState> {
        let info = self.unit_info?;
        let mut tr: Vec<Track<'a>> = Vec::new();
        for u in &self.units {
            let Some(it) = self.arena.node(u.e.fx.node).iterate.as_ref() else { continue };
            let per_letter = &*it.kind == "lt";
            let r = if per_letter == info.letters { unit } else { word.unwrap_or(unit) };
            if let Some(rg) = u.range
                && (f64::from(r.para) < rg.0 || f64::from(r.para) > rg.1)
            {
                continue;
            }
            let simple = simple_dur_of(self.arena, self.arena.node(u.e.fx.node));
            let gap = it.abs_ms.unwrap_or_else(|| it.pct.unwrap_or(0.0) * simple);
            // an effect restricted to ONE paragraph counts inside it; otherwise over the whole txBody
            let local = u.range.is_some_and(|rg| rg.0 == rg.1);
            let (k, count) = if local { (r.k_para, r.count_para) } else { (r.k, r.count) };
            let (k, count) = (f64::from(k), f64::from(count));
            let idx = if it.backwards { count - 1.0 - k } else { k };
            tr.push(Track { e: u.e, shift: idx * gap + if u.bg_first { simple } else { 0.0 } });
        }
        if tr.is_empty() {
            return None;
        }
        Some(eval_tracks(self.arena, &tr, self.anim_ms, base, initial_visible(self.arena, &tr), c))
    }

    /// The chart parts (`hooks.chartParts`), empty without a chart effect.
    pub fn chart_parts(&self) -> &[ChartPartHook<'a>] {
        &self.chart
    }

    /// The state of chart part `j` for `base` (`chartParts[j].state(base)`); `None` = static layer.
    pub fn chart_state(&self, j: usize, base: &AnimBase, c: &mut AnimCaches) -> Option<PartState> {
        let (tracks, vis) = self.chart.get(j)?.tracks.as_ref()?;
        Some(eval_tracks(self.arena, tracks, self.anim_ms, base, *vis, c))
    }
}
