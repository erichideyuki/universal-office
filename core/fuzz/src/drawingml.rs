//! Fuzz targets of `uo-drawingml` (hybrid mode, continuous verification; record
//! `core/docs/modules/uo-drawingml.md`, "Continuous verification").
//!
//! The crate takes preset names, adjust values, custom-guide programs, path segments, mask
//! dimensions, effect parameters, pixel buffers and strings from untrusted documents and runs in
//! wasm and under .NET, where a panic kills the host. Every target decodes the input bytes into
//! the arguments of one family of public functions (`Input`: f64 values come from a tag byte, so
//! NaN, infinities, subnormals, `-0`, `1e308` and plain numbers are all one or two bytes away),
//! runs them under small [`DML_LIMITS`] (or [`TIGHT_LIMITS`] when the selector byte has bit 7) and
//! checks invariants that follow from the plan and the error table:
//!
//! - no panic and no call over [`CALL_BOUND`] (1 s): a loop outside a limit shows up as a slow call
//!   long before it hangs;
//! - the error variant is the one of the table of the record, for the condition the target can
//!   state independently (a mask over `max_mask_pixels`, a kernel over `max_kernel_radius`, a
//!   buffer of the wrong length, a polygon set over `max_fill_work`), and an `Err` leaves its
//!   inputs untouched;
//! - structural facts (parallel vectors of an outline, polygons of at least three points, the
//!   count of path points under its limit, `custom_guides` truncating each list at the limit,
//!   alpha bytes never written by the colour adjusts, a trailing partial pixel never touched);
//! - value ranges where the inputs are clean (coverage in 0..=1, finite parameters): blur,
//!   morph, shift, warp and the four effect masks stay in 0..=1, the distance plane is never
//!   negative or NaN, fill coverage never goes down;
//! - the crate holds nothing between calls: a call repeated with the same arguments gives the
//!   same result.
//!
//! Layouts are written next to each target; [`dml_seeds`] builds the seed corpora with the same
//! encoder (`Enc`), so the seeds are valid inputs, not noise.

use std::time::{Duration, Instant};

use uo_drawingml::{
    Affine, Align, BevelSpec, BoxF, ClrChange, ColorModKind, DEFAULT_TOL, Error, GuideDef, ImageEffects, LimitKind, Limits, Mask, PathSeg,
    PictureAdjust, Polygons, PresetPath, Reflection, ShadowSpec, Subpaths, adjust_brightness_contrast, adjust_saturation, adjust_sharpen,
    adjust_temperature, align_point, apply_color_mods, apply_image_effects, apply_picture_adjust, arc_to_seg, bevel_phi, custom_guides,
    distance_to, effect_decode, effect_line, eval_preset, fill_polygons, flatten_path, gaussian_blur, glow_mask, has_preset_def,
    inner_shadow_mask, light_at, light_face, light_table, lin_to_srgb, morph, outer_shadow_mask, pattern_density, pattern_rows,
    preset_guides, preset_names, preset_outline, preset_path, preset_patterns, preset_text_rect, reflection_alpha, shade_bevel,
    shadow_affine, shift_mask, soft_edge_mask, srgb_to_lin, warp_mask,
};

/// The longest one public call may take under the fuzz limits (they keep each call to a few
/// milliseconds; a missed limit shows up as seconds).
pub const CALL_BOUND: Duration = Duration::from_secs(1);

/// Small limits: every check of `Limits` is reachable with an input of a few hundred bytes and
/// each call stays fast. `max_fill_work` is reachable with a few dozen edges over a 96-row mask.
pub const DML_LIMITS: Limits = Limits {
    max_mask_pixels: 1 << 16,
    max_kernel_radius: 48,
    max_path_points: 1 << 12,
    max_fill_work: 1 << 14,
    max_custom_guides: 24,
    max_image_pixels: 1 << 12,
};

/// Tighter still (selector bit 7): the limit paths of small inputs.
pub const TIGHT_LIMITS: Limits = Limits {
    max_mask_pixels: 600,
    max_kernel_radius: 3,
    max_path_points: 40,
    max_fill_work: 200,
    max_custom_guides: 3,
    max_image_pixels: 64,
};

fn limits_of(sel: u8) -> Limits {
    if sel & 0x80 != 0 { TIGHT_LIMITS } else { DML_LIMITS }
}

fn timed<R>(what: &str, f: impl FnOnce() -> R) -> R {
    let t = Instant::now();
    let r = f();
    let took = t.elapsed();
    assert!(took < CALL_BOUND, "{what} took {took:?}: a loop outside the limits");
    r
}

// ───────────────────────────── decoding ─────────────────────────────

const SPECIAL: [f64; 24] = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    f64::NAN,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::MAX,
    f64::MIN,
    f64::MIN_POSITIVE,
    5e-324,
    1e9,
    -1e9,
    1e5,
    1e6,
    21600.0,
    1e-9,
    100_000.0,
    360.0,
    0.25,
    255.0,
    1e300,
    -1e300,
];

/// A reader over the input bytes: past the end every read gives 0.
struct Input<'a> {
    d: &'a [u8],
    i: usize,
}

impl<'a> Input<'a> {
    fn new(d: &'a [u8]) -> Self {
        Input { d, i: 0 }
    }

    fn u8(&mut self) -> u8 {
        let b = self.d.get(self.i).copied().unwrap_or(0);
        self.i = self.i.saturating_add(1);
        b
    }

    fn bool(&mut self) -> bool {
        self.u8() & 1 != 0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::from(self.u8()) % n.max(1)
    }

    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        for b in &mut out {
            *b = self.u8();
        }
        out
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes())
    }

    fn rest(&mut self) -> &'a [u8] {
        let r = self.d.get(self.i..).unwrap_or(&[]);
        self.i = self.d.len();
        r
    }

    /// A number: tag 0..24 a special value, 24..96 an `i16`, 96..160 an `i32 / 16`, the rest 8
    /// raw bytes (every bit pattern).
    fn f64(&mut self) -> f64 {
        let t = self.u8();
        match t {
            0..=23 => SPECIAL.get(usize::from(t)).copied().unwrap_or(0.0),
            24..=95 => f64::from(i16::from_le_bytes(self.bytes())),
            96..=159 => f64::from(i32::from_le_bytes(self.bytes())) / 16.0,
            _ => f64::from_bits(u64::from_le_bytes(self.bytes())),
        }
    }

    fn opt_f64(&mut self) -> Option<f64> {
        if self.bool() { Some(self.f64()) } else { None }
    }

    /// A number in 0..=1 from one byte (251 and up: the specials of `f64`).
    fn unit(&mut self) -> f64 {
        let b = self.u8();
        if b <= 250 { f64::from(b) / 250.0 } else { self.f64() }
    }

    /// A coordinate in the units of a small mask (a byte / 2), or any number.
    fn coord(&mut self) -> f64 {
        let b = self.u8();
        if b < 0xC0 { f64::from(b) / 2.0 } else { self.f64() }
    }

    /// A string of at most `max` bytes (UTF-8 lossy: invalid bytes become U+FFFD).
    fn str(&mut self, max: usize) -> String {
        let n = usize::from(self.u8()) % (max + 1);
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(self.u8());
        }
        String::from_utf8_lossy(&v).into_owned()
    }
}

/// The encoder of [`Input`] (seeds).
#[derive(Default)]
struct Enc {
    v: Vec<u8>,
}

impl Enc {
    fn u8(&mut self, b: u8) -> &mut Self {
        self.v.push(b);
        self
    }

    fn bool(&mut self, b: bool) -> &mut Self {
        self.u8(u8::from(b))
    }

    fn u32(&mut self, x: u32) -> &mut Self {
        self.v.extend_from_slice(&x.to_le_bytes());
        self
    }

    fn f64(&mut self, x: f64) -> &mut Self {
        if let Some(t) = SPECIAL.iter().position(|s| s.to_bits() == x.to_bits()) {
            return self.u8(u8::try_from(t).unwrap_or(0));
        }
        if x.fract() == 0.0 && x.abs() <= 32767.0 {
            self.u8(24);
            self.v.extend_from_slice(&(x as i16).to_le_bytes());
            return self;
        }
        let q = x * 16.0;
        if q.fract() == 0.0 && q.abs() <= 2_147_483_647.0 {
            self.u8(96);
            self.v.extend_from_slice(&(q as i32).to_le_bytes());
            return self;
        }
        self.u8(160);
        self.v.extend_from_slice(&x.to_bits().to_le_bytes());
        self
    }

    fn opt_f64(&mut self, x: Option<f64>) -> &mut Self {
        match x {
            Some(x) => {
                self.u8(1);
                self.f64(x)
            }
            None => self.u8(0),
        }
    }

    fn unit(&mut self, x: f64) -> &mut Self {
        let b = uo_jsnum::math::round(x * 250.0);
        if (0.0..=250.0).contains(&b) && (b / 250.0 - x).abs() < 1e-12 {
            return self.u8(b as u8);
        }
        self.u8(251);
        self.f64(x)
    }

    fn coord(&mut self, x: f64) -> &mut Self {
        let b = x * 2.0;
        if b.fract() == 0.0 && (0.0..192.0).contains(&b) {
            return self.u8(b as u8);
        }
        self.u8(0xC0);
        self.f64(x)
    }

    fn str(&mut self, s: &str, max: usize) -> &mut Self {
        let b = s.as_bytes();
        let n = b.len().min(max);
        self.u8(n as u8);
        self.v.extend_from_slice(b.get(..n).unwrap_or(&[]));
        self
    }

    fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.v.extend_from_slice(b);
        self
    }

    fn done(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.v)
    }
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn all_finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

// ───────────────────────────── masks ─────────────────────────────

/// A coverage value of one byte: 0..=250 are 0..=1, then 1.5, -1, NaN, +inf, -0.
fn cov(b: u8) -> f32 {
    match b {
        0..=250 => f32::from(b) / 250.0,
        251 => 1.5,
        252 => -1.0,
        253 => f32::NAN,
        254 => f32::INFINITY,
        _ => -0.0,
    }
}

/// A mask of up to `max_side` in each direction (a side byte under 4 is 0..=3, so empty and tiny
/// masks come up) with blocky regions of up to 16 values from the input. Built with `from_vec`:
/// the input mask is not subject to the limits of the call under test.
///
/// Layout: `w`, `h`, `cell`, `n`, then `n` coverage bytes.
fn mask_of(inp: &mut Input<'_>, max_side: u32) -> Mask {
    let side = |inp: &mut Input<'_>| -> u32 {
        let b = inp.u8();
        if b < 4 { u32::from(b) } else { 1 + u32::from(b) % max_side }
    };
    let w = side(inp);
    let h = side(inp);
    let cell = 1 + inp.below(8);
    let n = 1 + inp.below(16);
    let vals: Vec<f32> = (0..n).map(|_| cov(inp.u8())).collect();
    let wu = usize::try_from(w).unwrap_or(0);
    let hu = usize::try_from(h).unwrap_or(0);
    let mut data = Vec::with_capacity(wu * hu);
    for y in 0..hu {
        for x in 0..wu {
            data.push(vals.get((x / cell + 3 * (y / cell)) % n).copied().unwrap_or(0.0));
        }
    }
    Mask::from_vec(w, h, data).expect("the data has w * h values")
}

fn enc_mask(e: &mut Enc, w: u8, h: u8, cell: u8, vals: &[u8]) {
    e.u8(w).u8(h).u8(cell).u8(vals.len().saturating_sub(1) as u8);
    e.raw(vals);
}

/// Every coverage value is finite and in 0..=1.
fn clean(m: &Mask) -> bool {
    m.data().iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}

fn dims(m: &Mask) -> (u32, u32) {
    (m.w(), m.h())
}

fn px(m: &Mask) -> u64 {
    u64::from(m.w()) * u64::from(m.h())
}

fn bits_of(m: &Mask) -> Vec<u32> {
    m.data().iter().map(|v| if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() }).collect()
}

fn in_range(m: &Mask, lo: f32, hi: f32) -> bool {
    m.data().iter().all(|v| *v >= lo && *v <= hi)
}

fn is_zero_mask(m: &Mask) -> bool {
    m.data().iter().all(|v| v.to_bits() == 0)
}

/// The errors a mask operation may return.
fn mask_error_ok(e: Error) -> bool {
    matches!(e, Error::Limit(LimitKind::MaskPixels | LimitKind::KernelRadius) | Error::InvalidArgument)
}

// ───────────────────────────── subpaths ─────────────────────────────

/// `len`, `iter` and `get` agree, and `get` past the end is `None`.
fn subs_consistent(s: &Subpaths) {
    let n = s.len();
    assert_eq!(s.iter().count(), n);
    assert!(s.get(n).is_none());
    assert_eq!(s.is_empty(), n == 0);
    let mut total = 0usize;
    for i in 0..n {
        total += s.get(i).map_or_else(|| panic!("subpath {i} of {n} missing"), <[PathSeg]>::len);
    }
    assert!(total <= s.segs().len());
}

// ───────────────────────────── presets ─────────────────────────────

const EXTRA_NAMES: [&str; 12] = [
    "",
    "nope",
    "constructor",
    "__proto__",
    "toString",
    "hasOwnProperty",
    "valueOf",
    "Rect",
    " rect",
    "rect",
    "actionButton",
    "actionButtonNone",
];
const ADJ_NAMES: [&str; 13] =
    ["adj", "adj1", "adj2", "adj3", "adj4", "adj5", "adj6", "adj7", "adj8", "", "constructor", "__proto__", "nope"];

/// A preset name: 0..=199 a defined preset (wrapping), 200..=211 one of [`EXTRA_NAMES`], 212 none
/// (`undefined`), above that a string from the input.
fn name_of(inp: &mut Input<'_>) -> Option<String> {
    let b = inp.u8();
    match b {
        0..=199 => {
            let n = preset_names().count();
            preset_names().nth(usize::from(b) % n.max(1)).map(str::to_owned)
        }
        200..=211 => EXTRA_NAMES.get(usize::from(b) - 200).map(|s| (*s).to_owned()),
        212 => None,
        _ => Some(inp.str(12)),
    }
}

/// The arguments of the preset functions. Layout: name, `w`, `h`, mode (`% 6`: none, `[]`, values,
/// values + names, values + a different number of names, names only), `n` (`% 9`) values, `n`
/// names from the pool (each a byte).
struct PresetArgs {
    name: Option<String>,
    w: f64,
    h: f64,
    adj: Option<Vec<f64>>,
    names: Option<Vec<String>>,
}

impl PresetArgs {
    fn read(inp: &mut Input<'_>) -> PresetArgs {
        let name = name_of(inp);
        let w = inp.f64();
        let h = inp.f64();
        let mode = inp.below(6);
        let n = inp.below(9);
        let values = |inp: &mut Input<'_>, n: usize| (0..n).map(|_| inp.f64()).collect::<Vec<f64>>();
        let pool = |inp: &mut Input<'_>, n: usize| {
            (0..n).map(|_| ADJ_NAMES.get(inp.below(ADJ_NAMES.len())).copied().unwrap_or("").to_owned()).collect::<Vec<String>>()
        };
        let (adj, names) = match mode {
            0 => (None, None),
            1 => (Some(Vec::new()), None),
            2 => (Some(values(inp, n)), None),
            3 => (Some(values(inp, n)), Some(pool(inp, n))),
            4 => {
                let v = values(inp, n);
                let m = inp.below(11);
                (Some(v), Some(pool(inp, m)))
            }
            _ => (None, Some(pool(inp, n))),
        };
        PresetArgs { name, w, h, adj, names }
    }

    fn names_refs(&self) -> Option<Vec<&str>> {
        self.names.as_ref().map(|v| v.iter().map(String::as_str).collect())
    }

    /// `(name, value)` pairs for `preset_outline` when the lists fit.
    fn pairs(&self) -> Option<Vec<(&str, f64)>> {
        match (&self.names, &self.adj) {
            (Some(n), Some(a)) if n.len() <= a.len() => Some(n.iter().map(String::as_str).zip(a.iter().copied()).collect()),
            _ => None,
        }
    }
}

fn enc_preset_args(e: &mut Enc, name: u8, w: f64, h: f64, adj: Option<(&[f64], Option<&[u8]>)>) {
    e.u8(name).f64(w).f64(h);
    match adj {
        None => {
            e.u8(0).u8(0);
        }
        Some((v, None)) => {
            e.u8(2).u8(v.len() as u8);
            for x in v {
                e.f64(*x);
            }
        }
        Some((v, Some(names))) => {
            e.u8(3).u8(v.len() as u8);
            for x in v {
                e.f64(*x);
            }
            for n in names.iter().take(v.len()) {
                e.u8(*n);
            }
        }
    }
}

/// Layout: selector, [`PresetArgs`]. Calls `eval_preset`, `preset_guides`, `preset_text_rect` and
/// `has_preset_def`: they agree on which names are defined, the geometry's vectors are parallel,
/// a text rectangle is never empty, the built-in variables are always there, and a repeated call
/// gives the same result.
pub fn dml_preset(data: &[u8]) {
    let mut inp = Input::new(data);
    let _sel = inp.u8();
    let a = PresetArgs::read(&mut inp);
    let nm = a.names_refs();
    let (adj, nm) = (a.adj.as_deref(), nm.as_deref());
    let name = a.name.as_deref();
    let defined = has_preset_def(name);
    let g = timed("eval_preset", || name.and_then(|n| eval_preset(n, a.w, a.h, adj, nm)));
    assert_eq!(g.is_some(), defined, "eval_preset and has_preset_def disagree on {name:?}");
    if let Some(g) = &g {
        assert_eq!(g.fill.len(), g.subs.len());
        assert_eq!(g.stroke.len(), g.subs.len());
        subs_consistent(&g.subs);
    }
    let again = name.and_then(|n| eval_preset(n, a.w, a.h, adj, nm));
    assert_eq!(format!("{g:?}"), format!("{again:?}"), "eval_preset is not a function of its arguments");
    let guides = timed("preset_guides", || name.and_then(|n| preset_guides(n, a.w, a.h, adj, nm)));
    assert_eq!(guides.is_some(), defined);
    if let Some(gd) = &guides {
        for builtin in ["l", "t", "r", "b", "w", "h", "hc", "vc", "ss", "ls", "cd2", "cd4", "wd2", "hd2", "ssd2", "wd32"] {
            assert!(gd.get(builtin).is_some(), "built-in {builtin} missing");
        }
        assert!(gd.len() >= 47);
        for n in ["adj", "adj1", "adj2", "x1", "y1", "dx1", "nope", ""] {
            let _ = gd.get(n);
            let _ = gd.val(n);
        }
        let _ = gd.val("1e400");
        let _ = gd.val("0x1A");
    }
    let tr = timed("preset_text_rect", || preset_text_rect(name, a.w, a.h, adj, nm));
    if let Some(r) = tr {
        assert!(defined);
        assert!(r.r > r.l && r.b > r.t, "an empty text rectangle was returned: {r:?}");
    }
    assert!(preset_text_rect(None, a.w, a.h, adj, nm).is_none());
    assert!(preset_text_rect(Some(""), a.w, a.h, adj, nm).is_none());
}

/// Layout: selector (bit 0: also flatten the outline), [`PresetArgs`]. Calls `preset_path` and
/// `preset_outline`: the vectors of a generic path and of an outline are parallel to their
/// subpaths, an outline always has at least one subpath (the rectangle is the fallback), and the
/// results do not change between calls.
pub fn dml_preset_path(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let a = PresetArgs::read(&mut inp);
    let nm = a.names_refs();
    let (adj, nm) = (a.adj.as_deref(), nm.as_deref());
    let name = a.name.as_deref();
    let p = timed("preset_path", || preset_path(name, a.w, a.h, adj, nm));
    match &p {
        PresetPath::Poly(points) => assert!(!points.is_empty()),
        PresetPath::Path(shape) => {
            subs_consistent(&shape.subs);
            if let Some(l) = &shape.sub_lum {
                assert_eq!(l.len(), shape.subs.len());
            }
            if let Some(s) = &shape.sub_stroke {
                assert_eq!(s.len(), shape.subs.len());
            }
        }
        _ => {}
    }
    assert_eq!(format!("{p:?}"), format!("{:?}", preset_path(name, a.w, a.h, adj, nm)), "preset_path is not a function of its arguments");
    let pairs = a.pairs();
    let o = timed("preset_outline", || preset_outline(name, a.w, a.h, pairs.as_deref()));
    subs_consistent(&o.subs);
    assert!(!o.subs.is_empty(), "an outline without subpaths");
    assert_eq!(o.fill.len(), o.subs.len());
    assert_eq!(o.stroke.len(), o.subs.len());
    assert_eq!(format!("{o:?}"), format!("{:?}", preset_outline(name, a.w, a.h, pairs.as_deref())));
    if sel & 1 != 0 {
        let lim = limits_of(sel);
        if let Ok(polys) = timed("flatten the outline", || flatten_path(&o.subs, &Affine::IDENTITY, DEFAULT_TOL, &lim)) {
            polygons_ok(&polys, lim.max_path_points);
        }
    }
}

// ───────────────────────────── custom guides ─────────────────────────────

const CG_NAMES: [&str; 14] = ["a", "b", "c", "d", "x1", "y1", "", "0", "w", "h", "l", "ss", "wd2", "adj1"];
const CG_OPS: [&str; 21] = [
    "val", "*/", "+-", "+/", "?:", "abs", "at2", "cat2", "sat2", "cos", "sin", "tan", "max", "min", "mod", "pin", "sqrt", "bogus", "",
    "VAL", "+ -",
];
const CG_TOKENS: [&str; 40] = [
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
    "wd2",
    "hd4",
    "cd2",
    "3cd4",
    "a",
    "c",
    "d",
    "x1",
    "y1",
    "adj1",
    "\u{a0} 7\u{a0}",
];

/// A token: 0..=99 from the pool, 100..=139 an `i16` written as text, above that a short string.
fn token(inp: &mut Input<'_>) -> String {
    let b = inp.u8();
    match b {
        0..=99 => CG_TOKENS.get(usize::from(b) % CG_TOKENS.len()).copied().unwrap_or("0").to_owned(),
        100..=139 => i16::from_le_bytes(inp.bytes()).to_string(),
        _ => inp.str(6),
    }
}

fn enc_token_pool(e: &mut Enc, i: usize) {
    e.u8(u8::try_from(i % CG_TOKENS.len()).unwrap_or(0));
}

type Program = Vec<(String, Vec<String>)>;

/// One list of guides: a count byte (`% max_count`), then per guide the name (a byte under 100:
/// the pool; else a short string), the operator (a byte: the pool of operators, or a string above
/// 40), the argument count (`% 6`) and the tokens.
fn guide_list(inp: &mut Input<'_>, max_count: usize) -> Program {
    let n = inp.below(max_count);
    (0..n)
        .map(|_| {
            let nb = inp.u8();
            let name = if nb < 100 { CG_NAMES.get(usize::from(nb) % CG_NAMES.len()).copied().unwrap_or("").to_owned() } else { inp.str(4) };
            let ob = inp.u8();
            let op = if ob < 40 { CG_OPS.get(usize::from(ob) % CG_OPS.len()).copied().unwrap_or("").to_owned() } else { inp.str(4) };
            let argc = inp.below(6);
            let mut fmla = vec![op];
            for _ in 0..argc {
                fmla.push(token(inp));
            }
            (name, fmla)
        })
        .collect()
}

fn defs<'a>(p: &'a Program, fmla: &'a [Vec<&'a str>]) -> Vec<GuideDef<'a>> {
    p.iter().zip(fmla).map(|((name, _), f)| GuideDef { name, fmla: f }).collect()
}

fn fmla_refs(p: &Program) -> Vec<Vec<&str>> {
    p.iter().map(|(_, f)| f.iter().map(String::as_str).collect()).collect()
}

/// Layout: selector, `w`, `h`, the `av` list (up to 8 guides), the `gd` list (up to 64), then
/// the six numbers of an `arc_to_seg`. Checks: the guides defined stay under builtins + 2 x the
/// limit (each list is cut at `max_custom_guides`), evaluating the lists already cut gives the
/// same values for every name and token, a repeated call gives the same values, and `arc_to_seg`
/// answers `None` exactly for a non-positive radius or a zero sweep.
pub fn dml_custom_guides(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let lim = limits_of(sel);
    let (w, h) = (inp.f64(), inp.f64());
    let av = guide_list(&mut inp, 8);
    let gd = guide_list(&mut inp, 64);
    let (avf, gdf) = (fmla_refs(&av), fmla_refs(&gd));
    let (avd, gdd) = (defs(&av, &avf), defs(&gd, &gdf));
    let g = timed("custom_guides", || custom_guides(&avd, &gdd, w, h, &lim));
    let max = usize::try_from(lim.max_custom_guides).unwrap_or(usize::MAX);
    let base = custom_guides(&[], &[], w, h, &lim).len();
    assert!(g.len() <= base + 2 * max, "{} guides over {base} built-ins and 2 x {max}", g.len());
    let cut = custom_guides(avd.get(..max.min(avd.len())).unwrap_or(&[]), gdd.get(..max.min(gdd.len())).unwrap_or(&[]), w, h, &lim);
    let again = custom_guides(&avd, &gdd, w, h, &lim);
    let mut queries: Vec<&str> = av.iter().chain(gd.iter()).map(|(n, _)| n.as_str()).collect();
    queries.extend(CG_TOKENS);
    queries.extend(["w", "h", "ss", "wd2", "cd4"]);
    for q in queries {
        assert!(same(g.val(q), cut.val(q)), "the lists are not cut at the limit: {q:?} {} vs {}", g.val(q), cut.val(q));
        assert!(same(g.val(q), again.val(q)));
        assert_eq!(g.get(q).is_some(), cut.get(q).is_some());
    }
    let (cx, cy, rx, ry, st, sw) = (inp.f64(), inp.f64(), inp.f64(), inp.f64(), inp.f64(), inp.f64());
    let arc = timed("arc_to_seg", || arc_to_seg(cx, cy, rx, ry, st, sw));
    let guard = rx <= 0.0 || ry <= 0.0 || sw == 0.0 || (sw * (std::f64::consts::PI / (180.0 * 60000.0))) == 0.0;
    assert_eq!(arc.is_none(), guard, "arc_to_seg guard: rx {rx} ry {ry} sw {sw}");
    if let Some((seg, _, _)) = arc {
        assert!(matches!(seg, PathSeg::E { .. }));
    }
}

// ───────────────────────────── colour, patterns ─────────────────────────────

const MOD_NAMES: [&str; 20] = [
    "gamma", "invGamma", "comp", "inv", "gray", "shade", "tint", "lumMod", "lumOff", "lum", "satMod", "satOff", "sat", "hueMod", "hueOff",
    "hue", "alpha", "alphaMod", "red", "Shade",
];

fn is_hsl(k: ColorModKind) -> bool {
    use ColorModKind::*;
    matches!(k, Comp | LumMod | LumOff | Lum | SatMod | SatOff | Sat | HueMod | HueOff | Hue)
}

fn is_valued(k: ColorModKind) -> bool {
    use ColorModKind::*;
    !matches!(k, Gamma | InvGamma | Comp | Inv | Gray)
}

/// Layout: selector, 3 channels (`unit`), the modifier count (`% 17`), per modifier a name byte
/// (under 20: [`MOD_NAMES`]; else a string) and an optional raw value; then 3 numbers for the
/// sRGB curves. Checks: unknown names have no kind, the colour is a function of its arguments,
/// when the last modifier that changes the colour is an HSL one every channel is NaN or in 0..=1,
/// and `lin_to_srgb(srgb_to_lin(v))` returns `v` on 0..=1.
pub fn dml_color(data: &[u8]) {
    let mut inp = Input::new(data);
    let _sel = inp.u8();
    let c = [inp.unit(), inp.unit(), inp.unit()];
    let n = inp.below(17);
    let mut mods: Vec<(ColorModKind, Option<f64>)> = Vec::new();
    for _ in 0..n {
        let b = inp.u8();
        let name = if b < 20 { MOD_NAMES.get(usize::from(b)).copied().unwrap_or("").to_owned() } else { inp.str(8) };
        let raw = inp.opt_f64();
        if let Some(kind) = ColorModKind::from_name(&name) {
            mods.push((kind, raw));
        } else {
            assert!(!matches!(name.as_str(), "gamma" | "invGamma" | "comp" | "inv" | "gray" | "shade" | "tint"), "{name} has no kind");
        }
    }
    let out = timed("apply_color_mods", || apply_color_mods(c, &mods));
    let again = apply_color_mods(c, &mods);
    assert!(out.iter().zip(&again).all(|(a, b)| same(*a, *b)));
    let last = mods
        .iter()
        .rev()
        .find(|(k, raw)| !matches!(k, ColorModKind::Gamma | ColorModKind::InvGamma) && (!is_valued(*k) || raw.is_some_and(f64::is_finite)));
    if let Some((k, _)) = last
        && is_hsl(*k)
    {
        for v in out {
            assert!(v.is_nan() || (0.0..=1.0).contains(&v), "{k:?} left a channel at {v}");
        }
    }
    for _ in 0..3 {
        let x = inp.f64();
        let (l, s) = (srgb_to_lin(x), lin_to_srgb(x));
        let _ = (l, s);
    }
    let v = inp.unit();
    if (0.0..=1.0).contains(&v) {
        let round = lin_to_srgb(srgb_to_lin(v));
        assert!((round - v).abs() < 1e-9, "sRGB round trip: {v} -> {round}");
        assert!((0.0..=1.0).contains(&srgb_to_lin(v)));
    }
}

/// Layout: selector (under 128: a defined pattern by index; else the rest is the name). Checks the
/// lookup against the table, the density against the popcount, 0.5 for unknown names.
pub fn dml_patterns(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let all = preset_patterns();
    assert!(!all.is_empty());
    let name = if sel < 128 {
        all.get(usize::from(sel) % all.len()).map(|(n, _)| (*n).to_owned()).unwrap_or_default()
    } else {
        String::from_utf8_lossy(inp.rest()).into_owned()
    };
    let rows = timed("pattern_rows", || pattern_rows(&name));
    let expect = all.iter().find(|(n, _)| *n == name).map(|(_, r)| *r);
    assert_eq!(rows, expect);
    let d = pattern_density(&name);
    match expect {
        Some(r) => {
            let ones: u32 = r.iter().map(|b| b.count_ones()).sum();
            assert!(same(d, f64::from(ones) / 64.0));
        }
        None => assert!(same(d, 0.5)),
    }
    let mut seen = std::collections::HashSet::new();
    for (n, _) in all {
        assert!(seen.insert(*n), "{n} listed twice");
    }
}

// ───────────────────────────── adjusts and blip ─────────────────────────────

/// The pixels of a call: `len` bytes cycled from the input (or a gradient when it has none).
fn pixels(inp: &mut Input<'_>, len: usize) -> Vec<u8> {
    let src = inp.rest();
    (0..len).map(|i| if src.is_empty() { (i * 37 % 256) as u8 } else { src.get(i % src.len()).copied().unwrap_or(0) }).collect()
}

fn alpha_same(a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        assert_eq!(x.get(3), y.get(3), "an alpha byte changed");
    }
}

fn tail_same(a: &[u8], b: &[u8]) {
    let whole = a.len() / 4 * 4;
    assert_eq!(a.get(whole..), b.get(whole..), "a trailing partial pixel changed");
}

/// Layout: selector (bit 7 tight limits; `% 4`: saturation, temperature, brightness and contrast,
/// sharpen/soften), `w`, `h` (`% 80`), the buffer mode (`% 4`: exactly `4 w h`, longer, shorter,
/// as many bytes as the input has), the extra length (`% 7`), two numbers, then the pixels. The
/// three pointwise adjusts never change alpha or a trailing partial pixel. `adjust_sharpen`
/// follows the table of the record: no-op (`!amount`, `w < 2`, `h < 2`), then `BufferSize`, then
/// `Limit(ImagePixels)`, all before touching the buffer; alpha never changes.
pub fn dml_adjust(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let lim = limits_of(sel);
    let w = u32::from(inp.u8() % 80);
    let h = u32::from(inp.u8() % 80);
    let mode = inp.below(4);
    let extra = inp.below(7);
    let (a, b) = (inp.f64(), inp.f64());
    let exact = 4 * usize::try_from(w * h).unwrap_or(0);
    let len = match mode {
        0 => exact,
        1 => exact + extra,
        2 => exact.saturating_sub(extra),
        _ => inp.d.len().saturating_sub(inp.i),
    };
    let src = pixels(&mut inp, len);
    let mut buf = src.clone();
    match sel % 4 {
        0 => timed("adjust_saturation", || adjust_saturation(&mut buf, a)),
        1 => timed("adjust_temperature", || adjust_temperature(&mut buf, a)),
        2 => timed("adjust_brightness_contrast", || adjust_brightness_contrast(&mut buf, a, b)),
        _ => {
            let r = timed("adjust_sharpen", || adjust_sharpen(&mut buf, w, h, a, &lim));
            let noop = a == 0.0 || a.is_nan() || w < 2 || h < 2;
            let n = u64::from(w) * u64::from(h);
            let expect = if noop {
                Ok(())
            } else if u64::try_from(src.len()).ok() != Some(4 * n) {
                Err(Error::BufferSize)
            } else if n > lim.max_image_pixels {
                Err(Error::Limit(LimitKind::ImagePixels))
            } else {
                Ok(())
            };
            assert_eq!(r, expect, "adjust_sharpen {w}x{h} amount {a} on {} bytes", src.len());
            if r.is_err() || noop {
                assert_eq!(buf, src, "adjust_sharpen changed a buffer it refused");
            }
        }
    }
    alpha_same(&src, &buf);
    tail_same(&src, &buf);
}

const HEX: [&str; 12] =
    ["ff0000", "00FF00", "0000ff", "808080", "fff", "", "zz0011", "-f0000", "0x1234", "  12ab", "\u{1F600}ab12", "12\u{e9}\u{e9}ab"];

/// A hex-ish string: a byte under 12 picks [`HEX`], else a short string.
fn hex_of(inp: &mut Input<'_>) -> String {
    let b = inp.u8();
    if usize::from(b) < HEX.len() { HEX.get(usize::from(b)).copied().unwrap_or("").to_owned() } else { inp.str(10) }
}

/// Layout: selector (bit 0: also `effect_decode` ...), the effects (gray, gain, bright, floor,
/// bi-level), the picture adjust (alpha, duotone present + 2 strings, colour change present + 2
/// strings + `to_alpha`), the components for the decode, then the pixels (any length). Lengths do
/// not change, alpha is never written by the effects, a trailing partial pixel is never touched,
/// the decode array has 2 or 6 numbers and exists only when there is a line.
pub fn dml_blip(data: &[u8]) {
    let mut inp = Input::new(data);
    let _sel = inp.u8();
    let fx = ImageEffects { gray: inp.bool(), gain: inp.opt_f64(), bright: inp.opt_f64(), floor: inp.bool(), bi_level: inp.opt_f64() };
    let alpha = inp.opt_f64();
    let duo = if inp.bool() { Some((hex_of(&mut inp), hex_of(&mut inp))) } else { None };
    let chg = if inp.bool() { Some((hex_of(&mut inp), hex_of(&mut inp), inp.f64())) } else { None };
    let components = u32::from(inp.u8() % 6);
    let src = inp.rest().to_vec();
    let line = timed("effect_line", || effect_line(&fx));
    let decode = timed("effect_decode", || effect_decode(&fx, components));
    if let Some(d) = &decode {
        assert!(line.is_some());
        assert_eq!(d.len(), if components == 1 { 2 } else { 6 });
    }
    if line.is_none() {
        assert!(decode.is_none());
    }
    let mut a = src.clone();
    timed("apply_image_effects", || apply_image_effects(&mut a, &fx));
    alpha_same(&src, &a);
    tail_same(&src, &a);
    let adj = PictureAdjust {
        alpha,
        duotone: duo.as_ref().map(|(x, y)| (x.as_str(), y.as_str())),
        clr_change: chg.as_ref().map(|(from, to, to_alpha)| ClrChange { from, to, to_alpha: *to_alpha }),
    };
    let mut b = src.clone();
    timed("apply_picture_adjust", || apply_picture_adjust(&mut b, &adj));
    assert_eq!(b.len(), src.len());
    tail_same(&src, &b);
    if alpha.is_none() && chg.is_none() {
        alpha_same(&src, &b);
    }
    let mut c = src.clone();
    apply_picture_adjust(&mut c, &adj);
    assert_eq!(b, c);
}

// ───────────────────────────── raster ─────────────────────────────

fn polygons_ok(p: &Polygons, max_points: u64) {
    let mut points = 0u64;
    for poly in p.iter() {
        assert!(poly.len() % 2 == 0 && poly.len() >= 6, "a polygon of {} numbers", poly.len());
        points += (poly.len() / 2) as u64;
    }
    assert!(points <= max_points, "{points} points over the limit {max_points}");
    assert_eq!(p.iter().count(), p.len());
}

/// An affine: a flag byte (0 identity, 1 a scale and offset of small numbers, else 6 numbers).
fn affine_of(inp: &mut Input<'_>) -> Affine {
    match inp.u8() {
        0 => Affine::IDENTITY,
        1 => {
            let (s, t) = (f64::from(inp.u8() % 32) / 8.0, f64::from(inp.u8() % 64) - 16.0);
            Affine([s, 0.0, 0.0, s, t, t])
        }
        _ => Affine([inp.f64(), inp.f64(), inp.f64(), inp.f64(), inp.f64(), inp.f64()]),
    }
}

fn seg_of(inp: &mut Input<'_>) -> (PathSeg, bool) {
    let k = inp.u8();
    let end = k & 8 != 0;
    let seg = match k % 6 {
        0 => PathSeg::M { x: inp.coord(), y: inp.coord() },
        1 => PathSeg::L { x: inp.coord(), y: inp.coord() },
        2 => PathSeg::C { x1: inp.coord(), y1: inp.coord(), x2: inp.coord(), y2: inp.coord(), x: inp.coord(), y: inp.coord() },
        3 => PathSeg::Q { x1: inp.coord(), y1: inp.coord(), x: inp.coord(), y: inp.coord() },
        4 => {
            PathSeg::E { cx: inp.coord(), cy: inp.coord(), rx: inp.coord(), ry: inp.coord(), a0: inp.f64(), a1: inp.f64(), ccw: inp.bool() }
        }
        _ => PathSeg::Z,
    };
    (seg, end)
}

/// Layout: selector (bit 0 even-odd, bit 1 start from a non-empty mask, bit 7 tight), the affine,
/// `tol` (a flag byte: 0 the default 0.35, else a number), the mask (`mask_of` with up to 64 px
/// sides), the segment count (`% 48`) and the segments (a kind byte: `% 6` M L C Q E Z, bit 3 ends
/// the subpath). `flatten_path`: `Err` is `Limit(PathPoints)`; polygons have at least 3 points and
/// together at most `max_path_points`. `fill_polygons`: `Err` is `Limit(FillWork)` and leaves the
/// mask as it was; with finite polygons and a clean mask the coverage stays in 0..=1 and never
/// goes down.
pub fn dml_flatten(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let lim = limits_of(sel);
    let m = affine_of(&mut inp);
    let tol = if inp.u8() == 0 { DEFAULT_TOL } else { inp.f64() };
    let mut mask = mask_of(&mut inp, 64);
    if sel & 2 == 0 {
        mask = Mask::from_vec(mask.w(), mask.h(), vec![0.0; mask.data().len()]).unwrap_or(mask);
    }
    let n = inp.below(48);
    let mut subs = Subpaths::new();
    let mut open = false;
    for _ in 0..n {
        let (seg, end) = seg_of(&mut inp);
        subs.push(seg);
        open = true;
        if end {
            subs.end_sub();
            open = false;
        }
    }
    if open {
        subs.end_sub();
    }
    subs_consistent(&subs);
    let polys = match timed("flatten_path", || flatten_path(&subs, &m, tol, &lim)) {
        Ok(p) => p,
        Err(e) => {
            assert_eq!(e, Error::Limit(LimitKind::PathPoints));
            return;
        }
    };
    polygons_ok(&polys, lim.max_path_points);
    let before = mask.clone();
    let finite = polys.iter().all(all_finite);
    let was_clean = clean(&before);
    let r = timed("fill_polygons", || fill_polygons(&mut mask, &polys, sel & 1 != 0, &lim));
    match r {
        Ok(()) => {
            assert_eq!(dims(&mask), dims(&before));
            if finite && was_clean {
                assert!(clean(&mask), "fill left the 0..=1 range");
                for (a, b) in before.data().iter().zip(mask.data()) {
                    assert!(*b >= *a - 1e-6, "fill lowered coverage {a} -> {b}");
                }
            }
        }
        Err(e) => {
            assert_eq!(e, Error::Limit(LimitKind::FillWork));
            assert_eq!(bits_of(&mask), bits_of(&before), "a refused fill changed the mask");
        }
    }
}

/// Layout: selector (bit 7 tight; `% 6`: blur, distance, morph, shift, warp, mask limits), the mask
/// (up to 96 px sides), then the parameters of the operation. The expected variant of every `Err`
/// is computed here from the parameters (a kernel over the limit, a padded plane or a target over
/// `max_mask_pixels`, a NaN radius on a non-empty mask); a clean mask stays in 0..=1.
pub fn dml_raster(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let lim = limits_of(sel);
    let op = sel % 6;
    if op == 5 {
        let (w, h) = (inp.u32(), inp.u32());
        let r = Mask::new(w, h, &lim);
        let n = u64::from(w) * u64::from(h);
        match r {
            Ok(m) => {
                assert!(n <= lim.max_mask_pixels);
                assert_eq!(m.data().len() as u64, n);
                assert!(is_zero_mask(&m));
            }
            Err(e) => {
                assert!(n > lim.max_mask_pixels);
                assert_eq!(e, Error::Limit(LimitKind::MaskPixels));
            }
        }
        let len = inp.below(40);
        let v = Mask::from_vec(w % 7, h % 7, vec![0.0; len]);
        assert_eq!(v.is_ok(), u64::from(w % 7) * u64::from(h % 7) == len as u64);
        return;
    }
    let m = mask_of(&mut inp, 96);
    let was_clean = clean(&m);
    let (w, h) = dims(&m);
    match op {
        0 => {
            let sigma = inp.f64();
            let r = timed("gaussian_blur", || gaussian_blur(&m, sigma, &lim));
            if sigma < 0.05 {
                assert_eq!(r.map(|o| bits_of(&o)), Ok(bits_of(&m)));
            } else if sigma.is_nan() {
                let o = r.unwrap_or_else(|e| panic!("NaN sigma: {e:?}"));
                assert_eq!(dims(&o), (w, h));
                assert!(is_zero_mask(&o));
            } else {
                let c = (sigma * 3.0).ceil();
                let radius = if c > 1.0 { c } else { 1.0 };
                match r {
                    Ok(o) => {
                        assert!(radius <= f64::from(lim.max_kernel_radius), "sigma {sigma}: kernel radius {radius} over the limit");
                        assert_eq!(dims(&o), (w, h));
                        if was_clean {
                            assert!(in_range(&o, -1e-5, 1.0 + 1e-4), "blur left the range");
                        }
                    }
                    Err(e) => {
                        assert_eq!(e, Error::Limit(LimitKind::KernelRadius));
                        assert!(radius > f64::from(lim.max_kernel_radius));
                    }
                }
            }
        }
        1 => {
            let inside = inp.bool();
            let d = timed("distance_to", || distance_to(&m, inside));
            if px(&m) == 0 {
                assert!(d.is_empty());
            } else {
                assert_eq!(d.len() as u64, px(&m));
                for (a, dv) in m.data().iter().zip(&d) {
                    assert!(*dv >= 0.0, "a negative or NaN distance {dv}");
                    if (f64::from(*a) >= 0.5) == inside {
                        assert!(dv.to_bits() == 0, "a pixel of the set at distance {dv}");
                    }
                }
            }
        }
        2 => {
            let r = inp.f64();
            let res = timed("morph", || morph(&m, r, &lim));
            if r.abs() < 0.01 {
                assert_eq!(res.map(|o| bits_of(&o)), Ok(bits_of(&m)));
            } else if r.is_nan() {
                if m.data().is_empty() {
                    assert_eq!(res.map(|o| dims(&o)), Ok((w, h)));
                } else {
                    assert_eq!(res.map(|o| dims(&o)), Err(Error::InvalidArgument));
                }
            } else {
                let p = r.abs().ceil() + 2.0;
                let padded = (f64::from(w) + 2.0 * p) * (f64::from(h) + 2.0 * p);
                match res {
                    Ok(o) => {
                        assert!(r.is_finite() && padded <= lim.max_mask_pixels as f64, "morph {r}: padded {padded} over the limit");
                        assert_eq!(dims(&o), (w, h));
                        if was_clean {
                            assert!(in_range(&o, 0.0, 1.0));
                            for (a, b) in m.data().iter().zip(o.data()) {
                                assert!(if r > 0.0 { *b >= *a } else { *b <= *a }, "morph {r}: {a} -> {b}");
                            }
                        }
                    }
                    Err(e) => {
                        assert_eq!(e, Error::Limit(LimitKind::MaskPixels));
                        assert!(!r.is_finite() || padded > lim.max_mask_pixels as f64, "morph {r}: refused at {padded}");
                    }
                }
            }
        }
        3 => {
            let (dx, dy) = (inp.f64(), inp.f64());
            let size = match inp.u8() % 3 {
                0 => None,
                1 => Some((u32::from(inp.u8() % 80), u32::from(inp.u8() % 80))),
                _ => Some((inp.u32(), inp.u32())),
            };
            let (tw, th) = size.unwrap_or((w, h));
            let res = timed("shift_mask", || shift_mask(&m, dx, dy, size, &lim));
            target_check(res, (tw, th), &lim, was_clean && all_finite(&[dx, dy]) && dx.abs() < 1e9 && dy.abs() < 1e9);
        }
        _ => {
            let a = affine_of(&mut inp);
            let (tw, th) = match inp.u8() % 3 {
                0 => (w, h),
                1 => (u32::from(inp.u8() % 80), u32::from(inp.u8() % 80)),
                _ => (inp.u32(), inp.u32()),
            };
            let res = timed("warp_mask", || warp_mask(&m, &a, tw, th, &lim));
            target_check(res, (tw, th), &lim, was_clean && a.0.iter().all(|v| v.is_finite() && v.abs() < 1e6));
        }
    }
}

/// `shift_mask` and `warp_mask`: the target size decides `Ok` or `Limit(MaskPixels)`; a clean
/// mask and tame parameters give coverage in 0..=1.
fn target_check(res: Result<Mask, Error>, target: (u32, u32), lim: &Limits, tame: bool) {
    let n = u64::from(target.0) * u64::from(target.1);
    match res {
        Ok(o) => {
            assert!(n <= lim.max_mask_pixels);
            assert_eq!(dims(&o), target);
            if tame {
                assert!(in_range(&o, -1e-6, 1.0 + 1e-6), "a resampled mask left 0..=1");
            }
        }
        Err(e) => {
            assert_eq!(e, Error::Limit(LimitKind::MaskPixels));
            assert!(n > lim.max_mask_pixels);
        }
    }
}

// ───────────────────────────── effects ─────────────────────────────

const ALGNS: [Option<&str>; 13] = [
    None,
    Some("tl"),
    Some("t"),
    Some("tr"),
    Some("l"),
    Some("ctr"),
    Some("r"),
    Some("bl"),
    Some("b"),
    Some("br"),
    Some(""),
    Some("xx"),
    Some("TL"),
];

fn spec_of(inp: &mut Input<'_>) -> ShadowSpec {
    let algn = ALGNS.get(inp.below(ALGNS.len())).copied().flatten();
    ShadowSpec {
        blur_pt: inp.f64(),
        dist_pt: inp.f64(),
        dir_deg: inp.f64(),
        sx: inp.opt_f64(),
        sy: inp.opt_f64(),
        kx_deg: inp.opt_f64(),
        ky_deg: inp.opt_f64(),
        algn: Align::from_attr(algn),
    }
}

/// Finite parameters of a moderate size. A scale that is subnormal but not zero is left out: the
/// determinant of the transform then underflows, `invertAffine` divides by it (`det || 1e-12` only
/// replaces an exact 0) and the inverse overflows to infinity, so NaN coverage is the TS result
/// too.
fn spec_tame(s: &ShadowSpec, ppt: f64) -> bool {
    let scale = |x: Option<f64>| x.is_none_or(|v| v == 0.0 || v.abs() >= 1e-30);
    let v =
        [s.blur_pt, s.dist_pt, s.dir_deg, ppt, s.sx.unwrap_or(1.0), s.sy.unwrap_or(1.0), s.kx_deg.unwrap_or(0.0), s.ky_deg.unwrap_or(0.0)];
    v.iter().all(|x| x.is_finite() && x.abs() < 1e6) && scale(s.sx) && scale(s.sy)
}

fn effect_check(what: &str, ctx: &dyn Fn() -> String, res: Result<Mask, Error>, sil: &Mask, tame: bool) {
    match res {
        Ok(o) => {
            assert_eq!(dims(&o), dims(sil));
            if tame && clean(sil) {
                let (lo, hi) = o
                    .data()
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), v| (if *v < l { *v } else { l }, if *v > h { *v } else { h }));
                assert!(in_range(&o, -1e-5, 1.0 + 1e-4), "{what} left 0..=1: min {lo}, max {hi}; {}x{} {}", sil.w(), sil.h(), ctx());
            }
        }
        Err(e) => assert!(mask_error_ok(e), "unexpected error {e:?}"),
    }
}

/// Layout: selector (bit 7 tight; `% 6`: outer shadow, inner shadow, glow, soft edge, the shadow
/// transform and the alignment point, the reflection gradient), the silhouette (up to 96 px sides),
/// then the parameters. The four masks have the size of the silhouette, allow only the errors of
/// the calls they compose, and stay in 0..=1 for a clean silhouette and tame parameters. An outer
/// or inner shadow over a silhouette past `max_mask_pixels` is `Limit(MaskPixels)`.
pub fn dml_effects(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let lim = limits_of(sel);
    let op = sel % 6;
    if op >= 4 {
        let (ax, ay, aw, ah) = (inp.f64(), inp.f64(), inp.f64(), inp.f64());
        let spec = spec_of(&mut inp);
        let bx = BoxF { x: ax, y: ay, w: aw, h: ah };
        let ppt = inp.f64();
        let t = timed("shadow_affine", || shadow_affine(&spec, &bx, ppt));
        assert!(same(t.0[0], shadow_affine(&spec, &bx, ppt).0[0]));
        let (px_, py_) = align_point(spec.algn, ax, ay, aw, ah);
        let _ = (px_, py_);
        let r = Reflection { st_a: inp.f64(), st_pos: inp.f64(), end_a: inp.f64(), end_pos: inp.f64() };
        let x = inp.f64();
        let v = timed("reflection_alpha", || reflection_alpha(x, &r));
        assert!(same(v, reflection_alpha(x, &r)));
        return;
    }
    let sil = mask_of(&mut inp, 96);
    let over = px(&sil) > lim.max_mask_pixels;
    match op {
        0 => {
            let bx = BoxF { x: inp.coord(), y: inp.coord(), w: inp.coord(), h: inp.coord() };
            let spec = spec_of(&mut inp);
            let ppt = inp.f64();
            let res = timed("outer_shadow_mask", || outer_shadow_mask(&sil, &bx, &spec, ppt, &lim));
            if over {
                assert_eq!(res.as_ref().err(), Some(&Error::Limit(LimitKind::MaskPixels)));
            }
            effect_check(
                "outer_shadow_mask",
                &|| format!("{bx:?} {spec:?} ppt {ppt}"),
                res,
                &sil,
                spec_tame(&spec, ppt) && [bx.x, bx.y, bx.w, bx.h].iter().all(|v| v.is_finite() && v.abs() < 1e6),
            );
        }
        1 => {
            let (blur, dist, dir, ppt) = (inp.f64(), inp.f64(), inp.f64(), inp.f64());
            let res = timed("inner_shadow_mask", || inner_shadow_mask(&sil, blur, dist, dir, ppt, &lim));
            if over {
                assert_eq!(res.as_ref().err(), Some(&Error::Limit(LimitKind::MaskPixels)));
            }
            effect_check(
                "inner_shadow_mask",
                &|| format!("blur {blur} dist {dist} dir {dir} ppt {ppt}"),
                res,
                &sil,
                [blur, dist, dir, ppt].iter().all(|x| x.is_finite() && x.abs() < 1e6),
            );
        }
        2 => {
            let (rad, ppt) = (inp.f64(), inp.f64());
            let res = timed("glow_mask", || glow_mask(&sil, rad, ppt, &lim));
            effect_check(
                "glow_mask",
                &|| format!("rad {rad} ppt {ppt}"),
                res,
                &sil,
                [rad, ppt].iter().all(|x| x.is_finite() && x.abs() < 1e6),
            );
        }
        _ => {
            let (rad, ppt) = (inp.f64(), inp.f64());
            let res = timed("soft_edge_mask", || soft_edge_mask(&sil, rad, ppt, &lim));
            effect_check(
                "soft_edge_mask",
                &|| format!("rad {rad} ppt {ppt}"),
                res,
                &sil,
                [rad, ppt].iter().all(|x| x.is_finite() && x.abs() < 1e6),
            );
        }
    }
}

// ───────────────────────────── light, bevel ─────────────────────────────

const RIGS: [&str; 14] = [
    "threePt",
    "balanced",
    "soft",
    "harsh",
    "glow",
    "twoPt",
    "freezing",
    "contrasting",
    "flood",
    "chilly",
    "morning",
    "sunrise",
    "brightRoom",
    "flat",
];
const MATERIALS: [&str; 10] = ["matte", "plastic", "metal", "flat", "softEdge", "powder", "dkEdge", "softmetal", "warmMatte", "nope"];
const DIRECTIONS: [&str; 11] = ["t", "tl", "l", "bl", "b", "br", "r", "tr", "nope", "T", "constructor"];
const PRESETS: [&str; 16] = [
    "relaxedInset",
    "circle",
    "slope",
    "cross",
    "angle",
    "softRound",
    "convex",
    "coolSlant",
    "divot",
    "riblet",
    "hardEdge",
    "artDeco",
    "nope",
    "",
    "__proto__",
    "toString",
];

/// An optional name from a pool: 0 none, 1 `""`, 2..=2+len a pool entry, above that a string.
fn pool_name(inp: &mut Input<'_>, pool: &[&str]) -> Option<String> {
    let b = usize::from(inp.u8());
    match b {
        0 => None,
        1 => Some(String::new()),
        _ if b < 2 + pool.len() => pool.get(b - 2).map(|s| (*s).to_owned()),
        _ => Some(inp.str(10)),
    }
}

/// Layout: selector (`% 2`: the light functions, the bevel profile), then the names, the angles or
/// `t` and `hw`. Every table is the same size (23 table keys of 1,060 bytes each), `light_at` and
/// `light_face` give finite values in 0..=6 for finite angles, `bevel_phi` stays within +-90
/// degrees or is NaN, and a repeated call gives the same bits.
pub fn dml_light(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    if sel.is_multiple_of(2) {
        let rig = pool_name(&mut inp, &RIGS);
        let mat = pool_name(&mut inp, &MATERIALS);
        let dir = pool_name(&mut inp, &DIRECTIONS);
        let (theta, phi) = (inp.f64(), inp.f64());
        let t = timed("light_table", || light_table(rig.as_deref(), mat.as_deref()));
        assert_eq!(t.bytes().len(), light_table(None, None).bytes().len(), "a table of another size");
        let v = timed("light_at", || light_at(&t, dir.as_deref(), theta, phi));
        let again = light_at(&t, dir.as_deref(), theta, phi);
        assert!(v.iter().zip(&again).all(|(a, b)| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())));
        if theta.is_finite() && phi.is_finite() {
            for x in v {
                assert!(x.is_finite() && (0.0..=6.0).contains(&x), "light_at({theta}, {phi}) gave {x}");
            }
        }
        let f = timed("light_face", || light_face(&t));
        for x in f {
            assert!(x.is_finite() && (0.0..=6.0).contains(&x), "light_face gave {x}");
        }
    } else {
        let prst = pool_name(&mut inp, &PRESETS);
        let (t, hw) = (inp.f64(), inp.f64());
        let phi = timed("bevel_phi", || bevel_phi(prst.as_deref(), t, hw));
        assert!(same(phi, bevel_phi(prst.as_deref(), t, hw)));
        assert!(phi.is_nan() || phi.abs() <= 90.0 + 1e-9, "bevel_phi({prst:?}, {t}, {hw}) = {phi}");
    }
}

/// Layout: selector (bit 7 tight), the silhouette (up to 48 px sides), the buffer mode (`% 4`:
/// exactly `4 w h`, longer, shorter, whatever is left of the input), the extra length (`% 9`), `w`
/// and `h` of the bevel, the preset, rig, direction and material (`pool_name`), the Word flag, then
/// the pixels. Follows the table of the record: `!(w > 0.5)` is a no-op, then `BufferSize`, then
/// `Limit(MaskPixels)` for the padded plane or the kernel, and an `Err` or a no-op leaves the
/// pixels as they were; alpha is never written, and pixels outside the silhouette keep their
/// colour.
pub fn dml_bevel(data: &[u8]) {
    let mut inp = Input::new(data);
    let sel = inp.u8();
    let lim = limits_of(sel);
    let sil = mask_of(&mut inp, 48);
    let (w, h) = dims(&sil);
    let exact = 4 * usize::try_from(px(&sil)).unwrap_or(0);
    let mode = inp.below(4);
    let extra = 1 + inp.below(9);
    let (bw, bh) = (inp.f64(), inp.f64());
    let prst = pool_name(&mut inp, &PRESETS);
    let rig = pool_name(&mut inp, &RIGS);
    let dir = pool_name(&mut inp, &DIRECTIONS);
    let material = pool_name(&mut inp, &MATERIALS);
    let word = inp.bool();
    let len = match mode {
        0 => exact,
        1 => exact + extra,
        2 => exact.saturating_sub(extra),
        _ => inp.d.len().saturating_sub(inp.i),
    };
    let src = pixels(&mut inp, len);
    let spec =
        BevelSpec { w: bw, h: bh, prst: prst.as_deref(), rig: rig.as_deref(), dir: dir.as_deref(), material: material.as_deref(), word };
    let mut buf = src.clone();
    let r = timed("shade_bevel", || shade_bevel(&mut buf, &sil, &spec, &lim));
    let noop = bw.is_nan() || bw <= 0.5;
    if noop {
        assert_eq!(r, Ok(()));
        assert_eq!(buf, src, "a no-op bevel changed the pixels");
        return;
    }
    if src.len() != exact {
        assert_eq!(r, Err(Error::BufferSize));
    } else if u64::from(w) + 2 > u64::from(u32::MAX) || (u64::from(w) + 2) * (u64::from(h) + 2) > lim.max_mask_pixels {
        assert_eq!(r, Err(Error::Limit(LimitKind::MaskPixels)));
    } else {
        match r {
            Ok(()) => {}
            Err(e) => assert_eq!(e, Error::Limit(LimitKind::KernelRadius), "unexpected error {e:?}"),
        }
    }
    if r.is_err() {
        assert_eq!(buf, src, "a refused bevel changed the pixels");
        return;
    }
    alpha_same(&src, &buf);
    for (i, (a, b)) in src.chunks_exact(4).zip(buf.chunks_exact(4)).enumerate() {
        let c = sil.data().get(i).copied().unwrap_or(0.0);
        if c <= 0.0 {
            assert_eq!(a, b, "a pixel outside the silhouette changed");
        }
    }
}

// ───────────────────────────── seeds ─────────────────────────────

/// Seed inputs of the `dml_*` targets: valid inputs of each layout (every preset at a few sizes
/// and adjust sets, hostile numbers, a program for every guide operator, every modifier,
/// every pattern, blocky masks with every operation, every rig and bevel preset), built by `Enc`.
pub fn dml_seeds() -> Vec<(&'static str, Vec<u8>)> {
    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    let n_presets = preset_names().count();
    let sizes: [(f64, f64); 6] = [(200.0, 100.0), (100.0, 200.0), (0.0, 0.0), (1.0, 1e6), (21600.0, 21600.0), (13.37, 0.5)];
    let adjs: [&[f64]; 5] = [&[], &[50000.0, 25000.0], &[f64::NAN, 1e9], &[-1e9, f64::INFINITY, 7.0], &[0.0, -1.0, 1e5, 1e308]];
    for i in 0..n_presets {
        let idx = u8::try_from(i % 200).unwrap_or(0);
        let (w, h) = sizes.get(i % sizes.len()).copied().unwrap_or((100.0, 100.0));
        let adj = adjs.get(i % adjs.len()).copied().unwrap_or(&[]);
        for target in ["dml_preset", "dml_preset_path"] {
            let mut e = Enc::default();
            e.u8(if target == "dml_preset_path" { 1 } else { 0 });
            enc_preset_args(&mut e, idx, w, h, if i % 3 == 0 { None } else { Some((adj, None)) });
            out.push((target, e.done()));
            let mut e = Enc::default();
            e.u8(0);
            enc_preset_args(&mut e, idx, 100.0, 100.0, Some((&[50000.0, 50000.0], Some(&[1, 2]))));
            out.push((target, e.done()));
        }
    }
    for (k, n) in EXTRA_NAMES.iter().enumerate() {
        let _ = n;
        for target in ["dml_preset", "dml_preset_path"] {
            let mut e = Enc::default();
            e.u8(0);
            enc_preset_args(&mut e, 200 + k as u8, 200.0, 100.0, Some((&[1.0, 2.0], None)));
            out.push((target, e.done()));
        }
    }
    for (w, h) in sizes {
        let mut e = Enc::default();
        e.u8(0);
        enc_preset_args(&mut e, 213, w, h, None);
        out.push(("dml_preset", e.done()));
    }

    // custom guides: one program per operator, plus the arc inputs
    for (k, op) in CG_OPS.iter().enumerate() {
        let mut e = Enc::default();
        e.u8(0).f64(200.0).f64(100.0);
        e.u8(1);
        e.u8(0).u8(k as u8).u8(2);
        enc_token_pool(&mut e, 33);
        enc_token_pool(&mut e, 34);
        e.u8(8);
        for g in 0..6u8 {
            e.u8(g).u8(((k + usize::from(g)) % CG_OPS.len()) as u8).u8(3);
            for t in 0..3usize {
                enc_token_pool(&mut e, k + usize::from(g) * 3 + t);
            }
        }
        e.f64(0.0).f64(0.0).f64(50.0).f64(40.0).f64(5_400_000.0).f64(-5_400_000.0);
        let _ = op;
        out.push(("dml_custom_guides", e.done()));
    }
    for sw in [0.0, 5_400_000.0, f64::NAN, 1e308, f64::NEG_INFINITY] {
        let mut e = Enc::default();
        e.u8(0x80).f64(10.0).f64(10.0).u8(0).u8(60);
        for g in 0..60u8 {
            e.u8(g % 14).u8(g % 21).u8(2).u8(g % 40).u8(1);
        }
        e.f64(10.5).f64(-3.0).f64(5.0).f64(f64::NAN).f64(0.0).f64(sw);
        out.push(("dml_custom_guides", e.done()));
    }

    // colour: every modifier alone and in sequences, raw values in and out of range
    for (k, name) in MOD_NAMES.iter().enumerate() {
        let _ = name;
        for raw in [Some(50_000.0), Some(120_000.0), Some(f64::NAN), Some(-20_000.0), None, Some(1e9)] {
            let mut e = Enc::default();
            e.u8(0).unit(0.2).unit(0.5).unit(0.8).u8(2);
            e.u8(k as u8).opt_f64(raw);
            e.u8(((k + 7) % 20) as u8).opt_f64(Some(35_000.0));
            e.f64(0.5).f64(f64::NAN).f64(2.0).unit(0.3);
            out.push(("dml_color", e.done()));
        }
    }
    let mut e = Enc::default();
    e.u8(0).unit(1.0).unit(0.0).unit(0.0).u8(16);
    for k in 0..16u8 {
        e.u8(k).opt_f64(Some(f64::from(k) * 7_000.0));
    }
    e.f64(0.0).f64(0.0).f64(0.0).unit(0.5);
    out.push(("dml_color", e.done()));

    // patterns: every pattern by index and unknown names
    for i in 0..preset_patterns().len() {
        out.push(("dml_patterns", vec![i as u8]));
    }
    for name in ["constructor", "__proto__", "pct5 ", "PCT5", "", "horz\0", "\u{1F600}"] {
        let mut v = vec![0x80];
        v.extend_from_slice(name.as_bytes());
        out.push(("dml_patterns", v));
    }

    // adjusts: each operation on a few shapes and lengths
    let px: Vec<u8> = (0..=255u8).collect();
    for op in 0..4u8 {
        for (w, h, mode, a, b) in [
            (8u8, 6u8, 0u8, 0.5, 0.25),
            (1, 1, 0, -1.0, 0.0),
            (30, 20, 0, 1.0, -0.5),
            (5, 5, 1, 2.0, 2.0),
            (4, 4, 2, f64::NAN, f64::INFINITY),
            (79, 79, 0, -0.5, 1e9),
            (2, 2, 3, 0.8, 0.0),
        ] {
            for lim in [0u8, 0x80] {
                let mut e = Enc::default();
                e.u8(op | lim).u8(w).u8(h).u8(mode).u8(3).f64(a).f64(b).raw(&px);
                out.push(("dml_adjust", e.done()));
            }
        }
    }

    // blip: effects, decode and picture adjusts
    for (gray, gain, bright, floor, bi) in [
        (false, Some(1.5), Some(0.1), false, None),
        (true, None, None, true, Some(0.5)),
        (false, Some(0.0), Some(f64::NAN), false, Some(1.0)),
        (true, Some(-1.0), Some(-0.3), true, Some(f64::NAN)),
        (false, None, Some(1e9), false, None),
    ] {
        for (alpha, duo, chg) in [
            (Some(0.5), None, None),
            (None, Some(("102030", "f0e0d0")), None),
            (Some(2.0), None, Some(("808080", "ffffff", 0.5))),
            (Some(-1.0), Some(("-f0000", "zz0011")), Some(("\u{1F600}ab12", "12\u{e9}\u{e9}ab", f64::NAN))),
        ] {
            let mut e = Enc::default();
            e.u8(0).bool(gray).opt_f64(gain).opt_f64(bright).bool(floor).opt_f64(bi);
            e.opt_f64(alpha);
            match duo {
                Some((x, y)) => e.u8(1).u8(0xFF).str(x, 10).u8(0xFF).str(y, 10),
                None => e.u8(0),
            };
            match chg {
                Some((x, y, a)) => e.u8(1).u8(0xFF).str(x, 10).u8(0xFF).str(y, 10).f64(a),
                None => e.u8(0),
            };
            e.u8(3).raw(&px);
            out.push(("dml_blip", e.done()));
        }
    }

    // flatten and fill: the outlines of the presets (at most 47 segments), each rule, both limits
    for (i, name) in preset_names().enumerate().step_by(3) {
        let o = preset_outline(Some(name), 48.0, 32.0, None);
        let mut segs: Vec<(PathSeg, bool)> = Vec::new();
        for sub in o.subs.iter() {
            let last = sub.len().saturating_sub(1);
            segs.extend(sub.iter().enumerate().map(|(k, s)| (*s, k == last)));
        }
        segs.truncate(47);
        let mut e = Enc::default();
        e.u8(((i / 3) % 2) as u8 | (((i / 6) % 2) as u8) << 1 | if i % 5 == 0 { 0x80 } else { 0 });
        e.u8(if i % 4 == 0 { 1 } else { 0 }).u8(if i % 4 == 0 { 3 } else { 0 }).u8(5);
        e.u8(if i % 4 == 0 { 8 } else { 0 });
        enc_mask(&mut e, 48, 32, 4, &[0, 250, 100]);
        e.u8(segs.len() as u8);
        for (seg, end) in &segs {
            let flag = if *end { 8 } else { 0 };
            match *seg {
                PathSeg::M { x, y } => e.u8(flag).coord(x).coord(y),
                PathSeg::L { x, y } => e.u8(1 | flag).coord(x).coord(y),
                PathSeg::C { x1, y1, x2, y2, x, y } => e.u8(2 | flag).coord(x1).coord(y1).coord(x2).coord(y2).coord(x).coord(y),
                PathSeg::Q { x1, y1, x, y } => e.u8(3 | flag).coord(x1).coord(y1).coord(x).coord(y),
                PathSeg::E { cx, cy, rx, ry, a0, a1, ccw } => {
                    e.u8(4 | flag).coord(cx).coord(cy).coord(rx).coord(ry).f64(a0).f64(a1).bool(ccw)
                }
                PathSeg::Z => e.u8(5 | flag),
            };
        }
        out.push(("dml_flatten", e.done()));
    }

    // raster: blocky masks, every operation and a few parameters
    let mask_vals: [&[u8]; 4] = [&[0, 250, 250, 0, 125], &[250], &[0, 250, 251, 252, 253, 254, 255], &[10, 60, 120, 200, 250, 250]];
    for (mi, vals) in mask_vals.iter().enumerate() {
        for (w, h) in [(16u8, 16u8), (2, 2), (40, 12), (90, 90)] {
            for op in 0..5u8 {
                for lim in [0u8, 0x80] {
                    let mut e = Enc::default();
                    e.u8(op | lim);
                    enc_mask(&mut e, w, h, 3 + mi as u8, vals);
                    match op {
                        0 => e.f64([1.0, 2.5, 0.3, 0.04, 33.3, f64::NAN, f64::INFINITY, -1.0][(w as usize + mi) % 8]),
                        1 => e.u8(w & 1),
                        2 => e.f64([3.7, -3.7, 0.5, 20.0, f64::NAN, f64::INFINITY, -0.005, 1e9][(h as usize + mi) % 8]),
                        3 => e.f64(1.5).f64(-2.25).u8((w + h) % 3).u8(w).u8(h).u32(1 << 20).u32(1 << 20),
                        _ => e.u8(1).u8(8).u8(3).u8(w).u8(h).u32(5).u32(7),
                    };
                    out.push(("dml_raster", e.done()));
                }
            }
        }
    }
    for (w, h) in [(100u32, 100u32), (1 << 20, 1 << 20), (u32::MAX, 0), (0, u32::MAX), (7, 5)] {
        let mut e = Enc::default();
        e.u8(5).u32(w).u32(h).u8(7);
        out.push(("dml_raster", e.done()));
    }

    // effects: every mask on a blob, shadow transform and reflection
    for op in 0..6u8 {
        for lim in [0u8, 0x80] {
            for (mi, vals) in mask_vals.iter().enumerate() {
                let mut e = Enc::default();
                e.u8(op | lim);
                if op >= 4 {
                    e.f64(0.0).f64(0.0).f64(100.0).f64(50.0);
                    e.f64(6.0).f64(3.0).f64(45.0).opt_f64(Some(1.0)).opt_f64(None).opt_f64(Some(30.0)).opt_f64(Some(89.9)).u8(5);
                    e.f64(1.0).f64(0.0).f64(0.0).f64(1.0);
                    e.f64(0.5);
                    out.push(("dml_effects", e.done()));
                    continue;
                }
                enc_mask(&mut e, 40, 30, 4 + mi as u8, vals);
                match op {
                    0 => {
                        e.coord(0.0).coord(0.0).coord(30.0).coord(20.0);
                        e.f64(6.0).f64(3.0).f64(45.0).opt_f64(Some(1.0)).opt_f64(Some(0.5)).opt_f64(Some(30.0)).opt_f64(None).u8(5);
                        e.f64(1.0);
                    }
                    1 => {
                        e.f64(6.0).f64(3.0).f64(45.0).f64(1.0);
                    }
                    _ => {
                        e.f64(if mi % 2 == 0 { 10.0 } else { f64::NAN }).f64(1.0);
                    }
                }
                out.push(("dml_effects", e.done()));
            }
        }
    }

    // light and bevel
    for (ri, _) in RIGS.iter().enumerate() {
        for (mi, _) in MATERIALS.iter().enumerate().step_by(3) {
            let mut e = Enc::default();
            e.u8(0).u8(2 + ri as u8).u8(2 + mi as u8).u8(2 + (ri % 8) as u8).f64(37.5).f64(30.0);
            out.push(("dml_light", e.done()));
        }
    }
    for (pi, _) in PRESETS.iter().enumerate() {
        for (t, hw) in [(0.5, 1.0), (0.0, 0.25), (1.0, 4.0), (f64::NAN, 1.0), (0.3, f64::INFINITY)] {
            let mut e = Enc::default();
            e.u8(1).u8(2 + pi as u8).f64(t).f64(hw);
            out.push(("dml_light", e.done()));
        }
    }
    for (pi, _) in PRESETS.iter().enumerate() {
        for (word, rig, mode, lim) in
            [(true, 0u8, 0u8, 0u8), (false, 2 + 3, 0, 0), (true, 0, 1, 0), (false, 0, 2, 0x80), (true, 0, 0, 0x80)]
        {
            let mut e = Enc::default();
            e.u8(lim);
            enc_mask(&mut e, 24, 18, 4, &[0, 250, 250, 250, 125]);
            e.u8(mode).u8(3).f64([6.0, 3.0, 12.0, 0.3, f64::NAN][pi % 5]).f64(6.0);
            e.u8(2 + pi as u8).u8(rig).u8(0).u8(0).bool(word);
            e.raw(&px);
            out.push(("dml_bevel", e.done()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_run_clean() {
        for (name, data) in dml_seeds() {
            let f: fn(&[u8]) = match name {
                "dml_preset" => dml_preset,
                "dml_preset_path" => dml_preset_path,
                "dml_custom_guides" => dml_custom_guides,
                "dml_color" => dml_color,
                "dml_patterns" => dml_patterns,
                "dml_adjust" => dml_adjust,
                "dml_flatten" => dml_flatten,
                "dml_blip" => dml_blip,
                "dml_raster" => dml_raster,
                "dml_effects" => dml_effects,
                "dml_light" => dml_light,
                "dml_bevel" => dml_bevel,
                other => panic!("{other}"),
            };
            f(&data);
        }
    }
}
