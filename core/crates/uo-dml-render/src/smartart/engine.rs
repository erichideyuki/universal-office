//! The SmartArt layout engine (`diagram-layout.ts`, ECMA-376 Part 1 §21.4): interprets the layout
//! definition (`layoutNode`, `forEach`, `choose/if`, `presOf`, `constrLst`, `ruleLst`, `alg`, `shape`)
//! over the data model. Port 1:1 of lines 17–1744 of the frozen TS: the presentation tree, constraints,
//! the algorithms (`lin`, `snake`, `cycle`, `pyra`, `composite`, hierarchy) and the text rules.
//!
//! Where the TS holds object references the port holds indices (`NId` into `Eng::nodes`, `PtId` into
//! `Eng::pts`); the module-level mutable state of the TS (`FIT`, `FRAME`, `SCALE_BUDGET`) is the
//! `Eng` struct's fields, so the engine is re-entrant. Hostile-input bounds are the TS's own
//! (`MAX_PRES`, `MAX_DEPTH`, `MAX_ITER`, the scale budget).

use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};
use uo_jsnum::conv::to_int32;
use uo_jsnum::math;
use uo_jsnum::parse::{parse_float, parse_int};

use super::data::{Ag, Attrs, MAX_DEPTH, MAX_ITER, MAX_PRES, Model, Pt, PtId, iterate, list_attr, parse_num, pt_match};
use super::metrics::DiagramMeasure;
use super::xt::X;

pub type NId = usize;

pub const EMU_MM: f64 = 36000.0;
pub const EMU_PT: f64 = 12700.0;

// ---------------------------------------------------------------------------------------------
// small ordered string maps (JS `Record`s with a handful of keys)
// ---------------------------------------------------------------------------------------------

/// A record of a few string keys (`Record<string, T>`): insertion ordered, overwrite in place.
#[derive(Clone, Debug)]
pub struct SMap<T>(pub Vec<(Box<str>, T)>);

impl<T> Default for SMap<T> {
    fn default() -> Self {
        SMap(Vec::new())
    }
}

impl<T> SMap<T> {
    pub fn get(&self, k: &str) -> Option<&T> {
        self.0.iter().find(|(n, _)| &**n == k).map(|(_, v)| v)
    }
    pub fn set(&mut self, k: &str, v: T) {
        match self.0.iter_mut().find(|(n, _)| &**n == k) {
            Some(e) => e.1 = v,
            None => self.0.push((k.into(), v)),
        }
    }
    pub fn remove(&mut self, k: &str) {
        self.0.retain(|(n, _)| &**n != k);
    }
    pub fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.0.iter().map(|(k, v)| (&**k, v))
    }
}

/// `Record<string, number>`.
pub type Rec = SMap<f64>;

impl Rec {
    pub fn val(&self, k: &str) -> Option<f64> {
        self.get(k).copied()
    }
    /// `{ ...self, ...other }`.
    pub fn spread(&mut self, other: &Rec) {
        for (k, v) in other.iter() {
            self.set(k, *v);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------------------------

/// `Constr` (`diagram-layout.ts:316`).
#[derive(Clone, Debug)]
pub struct Constr {
    pub ty: String,
    pub for_: String,
    pub for_name: Option<String>,
    pub pt_type: Option<String>,
    pub ref_type: String,
    pub ref_for: String,
    pub ref_for_name: Option<String>,
    pub ref_pt_type: Option<String>,
    pub op: String,
    pub val: f64,
    pub has_val: bool,
    pub fact: f64,
}

/// `Rule`.
#[derive(Clone, Debug)]
pub struct Rule {
    pub ty: String,
    pub for_: String,
    pub for_name: Option<String>,
    pub pt_type: Option<String>,
    pub val: f64,
    pub fact: f64,
    pub max: f64,
}

type ConstrList = Rc<Vec<Rc<Constr>>>;

/// `lin`'s scale in force (`PNode.ls`).
#[derive(Clone, Copy, Debug)]
pub struct Ls {
    pub axis_w: bool,
    pub s: f64,
    pub all: bool,
}

/// A limit (`lte`/`desLte`): reference node (`None` = the detached copy of `evalKidsOnly`, whose font
/// size is never set), reference type and factor.
type Lte = (Option<NId>, String, f64);
/// Margin proportional to a font (`margF`): factor, font type, reference node.
type MargF = (f64, String, Option<NId>);

/// The circle of a cycle (`PNode.cyc`).
#[derive(Clone, Copy, Debug)]
pub struct Cyc {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub cw: bool,
}

/// A connector's geometry (`PNode.conn`).
#[derive(Clone, Debug)]
pub struct Conn {
    pub prst: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rot: f64,
    pub flip_h: bool,
    pub adj: Option<Vec<(String, f64)>>,
    pub path: Option<X>,
    pub line: bool,
}

/// `TextPara`: `p` is the `a:p` element (`None` = an empty paragraph made by `mk('a:p')`).
#[derive(Clone, Debug)]
pub struct TextPara<'a> {
    pub p: Option<&'a X>,
    pub level: f64,
    pub csz: Option<f64>,
}

/// `PNode` (`diagram-layout.ts:259`).
#[derive(Debug)]
pub struct PNode<'a> {
    pub uid: usize,
    pub name: String,
    pub pt: PtId,
    pub parent: Option<NId>,
    pub kids: Vec<NId>,
    pub alg: Option<&'a X>,
    pub shape: Option<&'a X>,
    pub pres_of: Option<&'a X>,
    pub constrs: ConstrList,
    pub rules: Vec<Rule>,
    pub lvars: Vec<(String, String)>,
    pub style_lbl: Option<String>,
    pub pres: Option<PtId>,
    pub style_idx: usize,
    pub style_cnt: usize,
    pub v: Rec,
    pub des_v: Rec,
    pub des_c: Option<SMap<Rc<Constr>>>,
    pub des_o: Option<SMap<NId>>,
    pub des_r: Option<SMap<NId>>,
    pub ls: Option<Ls>,
    pub ov: Rec,
    pub from_parent: Option<FxHashSet<String>>,
    pub hi: Option<Rec>,
    pub gw: Option<Rec>,
    pub shr: Option<Rec>,
    pub cw: Option<(f64, f64)>,
    pub link: Option<SMap<(String, f64)>>,
    pub preplaced: bool,
    pub adj_ov: Option<Vec<(String, f64)>>,
    pub fmax0: Option<f64>,
    pub des_lte: Option<SMap<Lte>>,
    pub cyc: Option<Cyc>,
    pub paras: Option<Rc<Vec<TextPara<'a>>>>,
    pub marg_f: SMap<MargF>,
    pub equ: SMap<String>,
    pub lte: SMap<Lte>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rx: Option<f64>,
    pub ry: Option<f64>,
    pub rw: Option<f64>,
    pub rh: Option<f64>,
    pub rot: f64,
    pub conn: Option<Conn>,
    pub font_pt: Option<f64>,
    pub sec_pt: Option<f64>,
}

/// `FitCtx`.
pub struct FitCtx<'a> {
    pub measure: &'a dyn DiagramMeasure,
    pub family: Option<&'a str>,
    pub line_k: f64,
    pub root: PtId,
}

/// The engine: the points, the presentation tree and the state the TS keeps in module variables.
pub struct Eng<'a> {
    pub pts: Vec<Pt<'a>>,
    pub model: Model,
    pub nodes: Vec<PNode<'a>>,
    pub fit: Option<FitCtx<'a>>,
    pub frame: Option<(f64, f64)>,
    pub scale_budget: i64,
    // Builder
    for_each_by_name: FxHashMap<String, &'a X>,
    used: FxHashSet<PtId>,
    iter: usize,
    // the whole-layout run (`layoutDiagram`'s `run`)
    pub root: NId,
    pub cx: f64,
    pub cy: f64,
}

// ---------------------------------------------------------------------------------------------
// value classes
// ---------------------------------------------------------------------------------------------

pub fn is_len(k: &str) -> bool {
    matches!(
        k,
        "w" | "h"
            | "l"
            | "t"
            | "r"
            | "b"
            | "ctrX"
            | "ctrY"
            | "lOff"
            | "tOff"
            | "rOff"
            | "bOff"
            | "ctrXOff"
            | "ctrYOff"
            | "wOff"
            | "hOff"
            | "sp"
            | "sibSp"
            | "secSibSp"
            | "begPad"
            | "endPad"
            | "diam"
            | "connDist"
            | "bendDist"
            | "wArH"
            | "hArH"
            | "stemThick"
            | "begMarg"
            | "endMarg"
            | "lMarg"
            | "rMarg"
            | "tMarg"
            | "bMarg"
    )
}
pub fn is_marg(k: &str) -> bool {
    matches!(k, "lMarg" | "rMarg" | "tMarg" | "bMarg")
}
pub fn is_font(k: &str) -> bool {
    matches!(k, "primFontSz" | "secFontSz")
}
fn is_x_key(k: &str) -> bool {
    matches!(k, "w" | "l" | "r" | "ctrX" | "lOff" | "rOff" | "ctrXOff" | "wOff")
}
fn is_y_key(k: &str) -> bool {
    matches!(k, "h" | "t" | "b" | "ctrY" | "tOff" | "bOff" | "ctrYOff" | "hOff")
}
/// `axisOf`: `true` = the width axis.
fn axis_of(key: &str, rf: &str) -> bool {
    if is_x_key(key) {
        true
    } else if is_y_key(key) {
        false
    } else {
        rf == "w"
    }
}

fn nz_str(o: &Option<String>) -> Option<&str> {
    o.as_deref().filter(|s| !s.is_empty())
}

/// `Number.isFinite(f) ? f : 1`.
fn fact_or1(f: f64) -> f64 {
    if f.is_finite() { f } else { 1.0 }
}

const VAR_DEFAULTS: [(&str, &str); 9] = [
    ("dir", "norm"),
    ("hierBranch", "std"),
    ("bulletEnabled", "0"),
    ("animLvl", "none"),
    ("animOne", "one"),
    ("resizeHandles", "rel"),
    ("orgChart", "0"),
    ("chMax", "-1"),
    ("chPref", "-1"),
];
fn var_default(k: &str) -> Option<&'static str> {
    VAR_DEFAULTS.iter().find(|(n, _)| *n == k).map(|(_, v)| *v)
}

fn parse_constr(e: &X) -> Constr {
    let a = |k: &str| e.attr(k);
    Constr {
        ty: a("type").unwrap_or("none").to_string(),
        for_: a("for").unwrap_or("self").to_string(),
        for_name: a("forName").map(str::to_string),
        pt_type: a("ptType").map(str::to_string),
        ref_type: a("refType").unwrap_or("none").to_string(),
        ref_for: a("refFor").unwrap_or("self").to_string(),
        ref_for_name: a("refForName").map(str::to_string),
        ref_pt_type: a("refPtType").map(str::to_string),
        op: a("op").unwrap_or("none").to_string(),
        val: parse_num(a("val"), 0.0),
        has_val: a("val").is_some(),
        fact: parse_num(a("fact"), 1.0),
    }
}

fn parse_rule(e: &X) -> Rule {
    let a = |k: &str| e.attr(k);
    Rule {
        ty: a("type").unwrap_or("none").to_string(),
        for_: a("for").unwrap_or("self").to_string(),
        for_name: a("forName").map(str::to_string),
        pt_type: a("ptType").map(str::to_string),
        val: parse_num(a("val"), f64::NAN),
        fact: parse_num(a("fact"), f64::NAN),
        max: parse_num(a("max"), f64::NAN),
    }
}

/// `params(alg)`: the `dgm:param` type → val record.
pub fn params(alg: Option<&X>) -> Attrs {
    let mut o = Attrs::default();
    if let Some(alg) = alg {
        for p in alg.kids_named("dgm:param") {
            if let Some(t) = p.attr("type").filter(|t| !t.is_empty()) {
                o.set(t, p.attr("val").unwrap_or(""));
            }
        }
    }
    o
}

/// `axis(v, x, dflt)`: one axis (x: l/r/ctrX/w, y: t/b/ctrY/h, with the `*Off` offsets): any two values
/// give the rest. Returns (start, size).
pub fn axis(v: &Rec, x: bool, dflt: Option<f64>) -> (Option<f64>, Option<f64>) {
    let (lo, hi, ctr, sz, lo_o, hi_o, ctr_o, sz_o) = if x {
        ("l", "r", "ctrX", "w", "lOff", "rOff", "ctrXOff", "wOff")
    } else {
        ("t", "b", "ctrY", "h", "tOff", "bOff", "ctrYOff", "hOff")
    };
    let off = |k: &str| v.val(k).unwrap_or(0.0);
    let l = v.val(lo).map(|a| a + off(lo_o));
    let r = v.val(hi).map(|a| a + off(hi_o));
    let c = v.val(ctr).map(|a| a + off(ctr_o));
    let mut z = v.val(sz).map(|a| a + off(sz_o));
    if z.is_none() {
        z = if let (Some(l), Some(r)) = (l, r) {
            Some(r - l)
        } else if let (Some(l), Some(c)) = (l, c) {
            Some(2.0 * (c - l))
        } else if let (Some(r), Some(c)) = (r, c) {
            Some(2.0 * (r - c))
        } else {
            dflt.map(|d| d + off(sz_o))
        };
    }
    let s = l.or_else(|| match (c, z, r) {
        (Some(c), Some(z), _) => Some(c - z / 2.0),
        (_, Some(z), Some(r)) => Some(r - z),
        _ => None,
    });
    (s, z)
}

/// `getVal(n, t)`: a constraint value of the node (position values derived from l/t/w/h).
pub fn get_val_of(v: &Rec, t: &str) -> Option<f64> {
    if let Some(x) = v.val(t)
        && !t.ends_with("Off")
        && !matches!(t, "l" | "r" | "t" | "b" | "ctrX" | "ctrY")
    {
        return Some(x);
    }
    match t {
        "l" | "r" | "ctrX" | "w" => {
            let (s, z) = axis(v, true, None);
            let Some(s) = s else { return if t == "w" { z } else { v.val(t) } };
            match t {
                "l" => Some(s),
                "r" => z.map(|z| s + z).or_else(|| v.val(t)),
                "ctrX" => z.map(|z| s + z / 2.0).or_else(|| v.val(t)),
                _ => z,
            }
        }
        "t" | "b" | "ctrY" | "h" => {
            let (s, z) = axis(v, false, None);
            let Some(s) = s else { return if t == "h" { z } else { v.val(t) } };
            match t {
                "t" => Some(s),
                "b" => z.map(|z| s + z).or_else(|| v.val(t)),
                "ctrY" => z.map(|z| s + z / 2.0).or_else(|| v.val(t)),
                _ => z,
            }
        }
        _ => v.val(t),
    }
}

/// The scale `evalKids` passes to `applyConstrs` (a closure in the TS).
#[derive(Clone, Copy, Debug)]
pub enum Scale {
    /// `algLin`: the scale `s`, in mode `all` or `main`, along the width (`main_w`) or height axis.
    Lin { s: f64, all: bool, main_w: bool },
    /// `algSnake`: `() => s`.
    Const(f64),
}

impl Scale {
    fn apply(self, c: &Constr) -> f64 {
        match self {
            Scale::Const(s) => s,
            Scale::Lin { s, all, main_w } => {
                let a = (c.ref_type == "w" || c.ref_type == "h") && (all || axis_of(&c.ty, &c.ref_type) == main_w);
                let b = !all
                    && c.ref_type == "none"
                    && c.op != "equ"
                    && is_len(&c.ty)
                    && !is_marg(&c.ty)
                    && axis_of(&c.ty, "none") == main_w
                    && (is_x_key(&c.ty) || is_y_key(&c.ty));
                if a || b { s } else { 1.0 }
            }
        }
    }
}

/// A reference node of a constraint: a real node or the detached copy `evalKidsOnly` makes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rf {
    N(NId),
    Copy,
}

#[derive(Clone, Copy, Debug, Default)]
struct Sz {
    w: f64,
    h: f64,
    full: Option<(f64, f64)>,
    grow: Option<(f64, f64)>,
}

/// The memoized result of one `lin` run.
#[derive(Clone, Copy, Debug)]
struct LinRes {
    len: f64,
    sec: f64,
    lo: f64,
}

struct LinSt {
    horiz: bool,
    main_w: bool,
    mode_all: bool,
    measuring: bool,
    sizes: Vec<Sz>,
    starts: Vec<f64>,
    last_key: Option<(u64, bool, Vec<Option<u64>>)>,
    last_res: Option<LinRes>,
}

fn norm_bits(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

/// `custScale(k, axis)`.
fn cust_scale_of(eng: &Eng<'_>, k: NId, ax: char) -> f64 {
    let key = if ax == 'X' { "custScaleX" } else { "custScaleY" };
    let s = eng.nodes.get(k).and_then(|n| n.pres).and_then(|p| eng.pts.get(p)).and_then(|p| p.pr_set).and_then(|ps| ps.attr(key));
    let v = match s {
        Some(s) => parse_int(s, 10) / 100000.0,
        None => 1.0,
    };
    if v.is_finite() && v > 0.0 && v < 100.0 { v } else { 1.0 }
}

#[derive(Clone, Copy, Debug)]
struct Bx {
    n: NId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    inner: Option<(f64, f64, f64, f64)>,
}

struct Arr {
    boxes: Vec<Bx>,
    anchor: Option<usize>,
    span: Option<(f64, f64)>,
}

fn bbox(bs: &[Bx]) -> (f64, f64, f64, f64) {
    if bs.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for b in bs {
        x0 = math::min(x0, b.x);
        y0 = math::min(y0, b.y);
        x1 = math::max(x1, b.x + b.w);
        y1 = math::max(y1, b.y + b.h);
    }
    (x0, y0, x1 - x0, y1 - y0)
}

fn shift(a: &mut Arr, dx: f64, dy: f64) {
    for b in &mut a.boxes {
        b.x += dx;
        b.y += dy;
    }
}

/// `quantScale`: the application finds the scale by 12-step bisection: a multiple of 1/4096 that fits.
fn quant_scale(s: f64) -> f64 {
    if !s.is_finite() || s <= 0.0 {
        return 1.0;
    }
    if s >= 1.0 {
        return s;
    }
    math::max(1.0 / 4096.0, math::floor(s * 4096.0) / 4096.0)
}

/// `norm360`.
pub fn norm360(a: f64) -> f64 {
    math::rem(math::rem(a, 360.0) + 360.0, 360.0)
}

/// `quarter`.
pub fn quarter(a: f64) -> bool {
    math::abs(math::rem(norm360(a), 180.0) - 90.0) < 0.01
}

// ---------------------------------------------------------------------------------------------
// the engine
// ---------------------------------------------------------------------------------------------

impl<'a> Eng<'a> {
    pub fn new(pts: Vec<Pt<'a>>, model: Model) -> Eng<'a> {
        Eng {
            pts,
            model,
            nodes: Vec::new(),
            fit: None,
            frame: None,
            scale_budget: 0,
            for_each_by_name: FxHashMap::default(),
            used: FxHashSet::default(),
            iter: 0,
            root: 0,
            cx: 0.0,
            cy: 0.0,
        }
    }

    // ----- builder ---------------------------------------------------------------------------

    /// `Builder.index`.
    pub fn index(&mut self, e: &'a X, depth: usize) {
        if depth > 200 {
            return;
        }
        for c in e.kids() {
            if c.name == "dgm:forEach"
                && let Some(nm) = c.attr("name").filter(|s| !s.is_empty())
                && c.attr("ref").is_none_or(str::is_empty)
            {
                self.for_each_by_name.insert(nm.to_string(), c);
            }
            self.index(c, depth + 1);
        }
    }

    /// `Builder.node`.
    pub fn build_node(&mut self, def: &'a X, pt: PtId, parent: Option<NId>, depth: usize) -> Option<NId> {
        if self.nodes.len() >= MAX_PRES || depth > MAX_DEPTH {
            return None;
        }
        let name = def.attr("name").unwrap_or("").to_string();
        let uid = self.nodes.len();
        self.nodes.push(PNode {
            uid,
            name: name.clone(),
            pt,
            parent,
            kids: Vec::new(),
            alg: None,
            shape: None,
            pres_of: None,
            constrs: Rc::new(Vec::new()),
            rules: Vec::new(),
            lvars: Vec::new(),
            style_lbl: def.attr("styleLbl").map(str::to_string),
            pres: None,
            style_idx: 0,
            style_cnt: 0,
            v: Rec::default(),
            des_v: Rec::default(),
            des_c: None,
            des_o: None,
            des_r: None,
            ls: None,
            ov: Rec::default(),
            from_parent: None,
            hi: None,
            gw: None,
            shr: None,
            cw: None,
            link: None,
            preplaced: false,
            adj_ov: None,
            fmax0: None,
            des_lte: None,
            cyc: None,
            paras: None,
            marg_f: SMap::default(),
            equ: SMap::default(),
            lte: SMap::default(),
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            rx: None,
            ry: None,
            rw: None,
            rh: None,
            rot: 0.0,
            conn: None,
            font_pt: None,
            sec_pt: None,
        });
        // the recorded presentation point (user adjustments: variables, scale, formatting)
        let key = format!("{}\u{0}{}", self.pts[pt].id, name);
        if let Some(l) = self.model.pres.get(&key)
            && let Some(&p) = l.iter().find(|x| !self.used.contains(x))
        {
            self.used.insert(p);
            self.nodes[uid].pres = Some(p);
        }
        self.content(def, uid, pt, depth);
        Some(uid)
    }

    /// `Builder.content`.
    fn content(&mut self, container: &'a X, n: NId, pt: PtId, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for c in container.kids() {
            self.iter += 1;
            if self.iter > MAX_ITER * 4 {
                return;
            }
            match c.name.as_str() {
                "dgm:alg" => self.nodes[n].alg = Some(c),
                "dgm:shape" => self.nodes[n].shape = Some(c),
                "dgm:presOf" => self.nodes[n].pres_of = Some(c),
                "dgm:constrLst" => {
                    for k in c.kids_named("dgm:constr") {
                        let rc = Rc::new(parse_constr(k));
                        Rc::make_mut(&mut self.nodes[n].constrs).push(rc);
                    }
                }
                "dgm:ruleLst" => {
                    for k in c.kids_named("dgm:rule") {
                        self.nodes[n].rules.push(parse_rule(k));
                    }
                }
                "dgm:varLst" => {
                    for v in c.kids() {
                        let k = v.name.strip_prefix("dgm:").unwrap_or(&v.name);
                        let val = v.attr("val").or_else(|| var_default(k)).unwrap_or("");
                        let lv = &mut self.nodes[n].lvars;
                        match lv.iter_mut().find(|(a, _)| a == k) {
                            Some(e) => e.1 = val.to_string(),
                            None => lv.push((k.to_string(), val.to_string())),
                        }
                    }
                }
                "dgm:layoutNode" => {
                    if let Some(k) = self.build_node(c, pt, Some(n), depth + 1) {
                        self.nodes[n].kids.push(k);
                    }
                }
                "dgm:forEach" => {
                    let fe: &'a X = match c.attr("ref").filter(|s| !s.is_empty()) {
                        Some(r) => self.for_each_by_name.get(r).copied().unwrap_or(c),
                        None => c,
                    };
                    let root = self.model.root;
                    for q in iterate(&self.pts, pt, fe, root, "none") {
                        self.content(fe, n, q, depth + 1);
                    }
                }
                "dgm:choose" => {
                    for br in c.kids() {
                        let take = if br.name == "dgm:if" { self.test(br, n, pt) } else { br.name == "dgm:else" };
                        if take {
                            self.content(br, n, pt, depth + 1);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// `Builder.test`.
    fn test(&self, br: &X, n: NId, pt: PtId) -> bool {
        let func = br.attr("func").unwrap_or("cnt");
        let op = br.attr("op").unwrap_or("equ");
        let want = br.attr("val").unwrap_or("");
        let root = self.model.root;
        enum Got {
            N(f64),
            S(String),
        }
        let pts = &self.pts;
        let got: Got = match func {
            "cnt" => Got::N(f64::from(u32::try_from(iterate(pts, pt, br, root, "none").len()).unwrap_or(u32::MAX))),
            "pos" | "revPos" | "posEven" | "posOdd" => {
                let base = iterate(pts, pt, br, root, "self").first().copied().unwrap_or(pt);
                let bty = pts[base].ty.as_str();
                let sibs_all: Vec<PtId> = match pts[base].par {
                    Some(pp) => pts[pp].seq.clone(),
                    None => vec![base],
                };
                let sib: Vec<PtId> = sibs_all.into_iter().filter(|&x| pts[x].ty == bty).collect();
                let i: i64 = sib.iter().position(|&x| x == base).map_or(0, |p| i64::try_from(p).unwrap_or(0) + 1);
                let len = i64::try_from(sib.len()).unwrap_or(0);
                let v = match func {
                    "pos" => i,
                    "revPos" => len - i + 1,
                    "posEven" => i64::from(i % 2 == 0),
                    _ => i64::from(i % 2 == 1),
                };
                Got::N(f64::from(i32::try_from(v).unwrap_or(0)))
            }
            "depth" => {
                let base = iterate(pts, pt, br, root, "self").first().copied().unwrap_or(pt);
                Got::N(f64::from(u32::try_from(pts[base].depth).unwrap_or(u32::MAX)))
            }
            "maxDepth" => {
                let l = iterate(pts, pt, br, root, "self");
                let mut m = 0.0_f64;
                let from = f64::from(u32::try_from(pts[pt].depth).unwrap_or(u32::MAX));
                for q in l {
                    if pts[q].is_node() {
                        m = math::max(m, f64::from(u32::try_from(pts[q].depth).unwrap_or(u32::MAX)) - from);
                    }
                }
                Got::N(m)
            }
            "var" => Got::S(self.var_of(n, br.attr("arg").unwrap_or(""))),
            _ => return false,
        };
        let got = match got {
            Got::S(s) => {
                let g = if s == "true" {
                    "1".to_string()
                } else if s == "false" {
                    "0".to_string()
                } else {
                    s
                };
                let w = if want == "true" {
                    "1"
                } else if want == "false" {
                    "0"
                } else {
                    want
                };
                let gn = parse_float(&g);
                let wn = parse_float(w);
                if gn.is_finite() && wn.is_finite() {
                    gn
                } else {
                    return match op {
                        "equ" => g == w,
                        "neq" => g != w,
                        _ => false,
                    };
                }
            }
            Got::N(n) => n,
        };
        let w = parse_float(want);
        match op {
            "equ" => got == w,
            "neq" => got != w,
            "gt" => got > w,
            "lt" => got < w,
            "gte" => got >= w,
            "lte" => got <= w,
            _ => false,
        }
    }

    /// `varOf`: layout variable: the user's adjustment (presLayoutVars) → the node's varLst → ancestors → default.
    pub fn var_of(&self, n: NId, k: &str) -> String {
        let mut q = Some(n);
        let key = format!("dgm:{k}");
        while let Some(i) = q {
            let nd = &self.nodes[i];
            let pv = nd.pres.and_then(|p| self.pts[p].pr_set).and_then(|ps| ps.first("dgm:presLayoutVars"));
            let from_pres = pv.and_then(|pv| pv.kids().find(|e| e.name == key));
            if let Some(fp) = from_pres {
                return fp.attr("val").or_else(|| var_default(k)).unwrap_or("").to_string();
            }
            if let Some((_, v)) = nd.lvars.iter().find(|(a, _)| a == k) {
                return v.clone();
            }
            q = nd.parent;
        }
        var_default(k).unwrap_or("").to_string()
    }

    // ----- constraints ----------------------------------------------------------------------

    pub(super) fn des_nodes(&self, n: NId, out: &mut Vec<NId>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for &k in &self.nodes[n].kids {
            out.push(k);
            self.des_nodes(k, out, depth + 1);
        }
    }

    /// `select` (`diagram-layout.ts:491`).
    fn select(&self, owner: NId, f: &str, name: Option<&str>, pt_type: Option<&str>) -> Vec<NId> {
        let base: Vec<NId> = if f == "ch" {
            self.nodes[owner].kids.clone()
        } else if f == "des" {
            let mut o = Vec::new();
            self.des_nodes(owner, &mut o, 0);
            o
        } else {
            vec![owner]
        };
        let name = name.filter(|s| !s.is_empty());
        let pt_type = pt_type.filter(|s| !s.is_empty());
        if name.is_none() && pt_type.is_none() {
            return base;
        }
        base.into_iter()
            .filter(|&k| {
                let nd = &self.nodes[k];
                name.is_none_or(|nm| nd.name == nm)
                    && pt_type.is_none_or(|ty| pt_match(&self.pts[nd.pt], Some(ty)) || self.pres_pt_match(k, ty))
            })
            .collect()
    }

    /// `presPtMatch`: a node made outside any forEach (context = the document) that presents a point by
    /// its presOf counts as that point in the ptType filter.
    fn pres_pt_match(&self, k: NId, pt_type: &str) -> bool {
        let nd = &self.nodes[k];
        let Some(fit) = &self.fit else { return false };
        if nd.pres_of.is_none() || self.pts[nd.pt].ty != "doc" {
            return false;
        }
        let p = self.pres_texts(k, fit.root).first().map(|x| x.0);
        p.is_some_and(|p| pt_match(&self.pts[p], Some(pt_type)))
    }

    fn get_val(&self, n: NId, t: &str) -> Option<f64> {
        get_val_of(&self.nodes[n].v, t)
    }

    fn rf_val(&self, rf: Rf, owner: NId, t: &str) -> Option<f64> {
        match rf {
            Rf::N(n) => self.get_val(n, t),
            Rf::Copy => self.get_val(owner, t),
        }
    }

    /// `refNode`.
    fn ref_node(&self, owner: NId, c: &Constr, target: NId, copy: bool) -> Option<Rf> {
        if c.ref_for == "self" {
            return Some(if copy { Rf::Copy } else { Rf::N(owner) });
        }
        let cands = self.select(owner, &c.ref_for, c.ref_for_name.as_deref(), c.ref_pt_type.as_deref());
        if cands.contains(&target) {
            return Some(Rf::N(target));
        }
        cands.first().map(|&x| Rf::N(x))
    }

    /// `virtualVal`: the value the owner's constraint would give the absent node `refForName`.
    fn virtual_val(&self, owner: NId, constrs: &[Rc<Constr>], c: &Constr) -> Option<f64> {
        let d = constrs
            .iter()
            .find(|x| x.for_name == c.ref_for_name && x.ty == c.ref_type && x.op != "lte" && x.op != "gte" && !std::ptr::eq(&***x, c))?;
        if d.ref_type == "none" {
            return if d.has_val { Some(if is_len(&d.ty) { d.val * EMU_MM } else { d.val }) } else { None };
        }
        if !d.ref_for.is_empty() && d.ref_for != "self" {
            return None;
        }
        let v = self.get_val(owner, &d.ref_type)?;
        Some(v * fact_or1(d.fact))
    }

    /// `markEqu`.
    fn mark_equ(&mut self, t: NId, c: &Constr, owner_uid: usize) {
        let key = format!("{}:{}:{}:{}", owner_uid, c.ty, c.for_name.as_deref().unwrap_or(""), c.pt_type.as_deref().unwrap_or(""));
        let font_eq = is_font(&c.ty)
            && c.for_ != "self"
            && nz_str(&c.for_name).is_some()
            && c.ref_type == "none"
            && c.has_val
            && !self.nodes[t].equ.has(&c.ty);
        if c.op == "equ" || font_eq {
            self.nodes[t].equ.set(&c.ty, key);
        }
    }

    /// `rangePair`: a lte+gte pair for the SAME target/type from the parent = a band used by the rules,
    /// not a value.
    fn range_pair(constrs: &[Rc<Constr>], c: &Rc<Constr>) -> bool {
        if c.for_ == "self" || (c.op != "lte" && c.op != "gte") || is_font(&c.ty) {
            return false;
        }
        let other = if c.op == "lte" { "gte" } else { "lte" };
        constrs.iter().any(|d| {
            !Rc::ptr_eq(d, c) && d.op == other && d.ty == c.ty && d.for_ == c.for_ && d.for_name == c.for_name && d.pt_type == c.pt_type
        })
    }

    /// `applyConstrs` (`diagram-layout.ts:576`). `copy`: the owner is the detached copy of `evalKidsOnly`.
    pub(super) fn apply_constrs(&mut self, owner: NId, constrs: &[Rc<Constr>], scale: Option<Scale>, only_self: bool, copy: bool) {
        let owner_uid = self.nodes[owner].uid;
        for c in constrs {
            if only_self && c.for_ != "self" {
                continue;
            }
            if Self::range_pair(constrs, c) {
                continue;
            }
            let targets = self.select(owner, &c.for_, c.for_name.as_deref(), c.pt_type.as_deref());
            for t in targets {
                let c: &Rc<Constr> = c;
                // margin proportional to a font: resolved after the text is fitted
                if is_marg(&c.ty) && is_font(&c.ref_type) {
                    let r = if !c.ref_for.is_empty() && c.ref_for != "self" { self.ref_node(owner, c, t, copy) } else { None };
                    let rr = match r {
                        Some(Rf::N(x)) if x != t => Some(x),
                        _ => None,
                    };
                    self.nodes[t].marg_f.set(&c.ty, (fact_or1(c.fact), c.ref_type.clone(), rr));
                    continue;
                }
                // a value decided by the rules wins over the constraint
                let decided = {
                    let nd = &self.nodes[t];
                    nd.ov.has(&c.ty) || nd.gw.as_ref().is_some_and(|g| g.has(&c.ty)) || nd.shr.as_ref().is_some_and(|g| g.has(&c.ty))
                };
                if decided {
                    self.mark_equ(t, c, owner_uid);
                    continue;
                }
                let mut value: f64;
                let mut rf: Option<Rf> = None;
                if c.ref_type != "none" {
                    rf = self.ref_node(owner, c, t, copy);
                    let mut rv = rf.and_then(|r| self.rf_val(r, owner, &c.ref_type));
                    // a reference to a node that does not exist: the owner's own constraint gives the value
                    if rv.is_none() && rf.is_none() && nz_str(&c.ref_for_name).is_some() {
                        rv = self.virtual_val(owner, constrs, c);
                        if rv.is_some() {
                            rf = Some(if copy { Rf::Copy } else { Rf::N(owner) });
                        }
                    }
                    let Some(rv) = rv else { continue };
                    value = rv * fact_or1(c.fact);
                    if is_marg(&c.ty) && is_len(&c.ref_type) && !is_marg(&c.ref_type) {
                        value *= EMU_PT / EMU_MM;
                    } else if is_len(&c.ty) && is_font(&c.ref_type) {
                        value *= EMU_MM;
                    } else if is_font(&c.ty) && is_len(&c.ref_type) {
                        value /= EMU_MM;
                    }
                    if c.op == "lte" || c.op == "gte" {
                        if c.op == "lte"
                            && let Some(r) = rf
                        {
                            let ent: Lte = (if let Rf::N(x) = r { Some(x) } else { None }, c.ref_type.clone(), fact_or1(c.fact));
                            self.nodes[t].lte.set(&c.ty, ent.clone());
                            // a ceiling from an ancestor (for="des") survives the parent's re-evaluation
                            if c.for_ == "des" {
                                self.nodes[t].des_lte.get_or_insert_with(SMap::default).set(&c.ty, ent);
                            }
                        }
                        // numeric ceiling (also limits the growth by the rules)
                        if c.op == "lte" && value.is_finite() {
                            let hi = self.nodes[t].hi.get_or_insert_with(SMap::default);
                            let cur = hi.val(&c.ty).unwrap_or(f64::INFINITY);
                            hi.set(&c.ty, math::min(cur, value));
                        }
                        let cur = self.nodes[t].v.val(&c.ty);
                        if let Some(cur) = cur
                            && value.is_finite()
                        {
                            let nv = if c.op == "lte" { math::min(cur, value) } else { math::max(cur, value) };
                            self.nodes[t].v.set(&c.ty, nv);
                            // h "equ" w of the node itself (the circle of vList3): the ceiling holds for both
                            let lk = self.nodes[t].link.as_ref().and_then(|l| l.get(&c.ty)).cloned();
                            if let Some((lt, lf)) = lk
                                && nv != cur
                                && lf != 0.0
                                && !lf.is_nan()
                            {
                                self.nodes[t].v.set(&lt, nv / lf);
                            }
                        } else if cur.is_none() && c.op == "gte" {
                            self.nodes[t].v.set(&c.ty, value);
                        }
                        continue;
                    }
                } else if c.has_val {
                    value = if is_marg(&c.ty) {
                        c.val * EMU_PT
                    } else if is_len(&c.ty) {
                        c.val * EMU_MM
                    } else {
                        c.val
                    };
                } else if c.op != "none" {
                    if c.op == "equ" {
                        let key = format!(
                            "{}:{}:{}:{}",
                            owner_uid,
                            c.ty,
                            c.for_name.as_deref().unwrap_or(""),
                            c.pt_type.as_deref().unwrap_or("")
                        );
                        self.nodes[t].equ.set(&c.ty, key);
                    }
                    continue;
                } else {
                    value = 0.0;
                }
                if value.is_nan() {
                    continue;
                }
                // the node's own constraint does not override a value imposed by an ancestor
                if t == owner && c.op == "none" && self.nodes[t].from_parent.as_ref().is_some_and(|f| f.contains(&c.ty)) {
                    continue;
                }
                if t != owner {
                    self.nodes[t].from_parent.get_or_insert_with(FxHashSet::default).insert(c.ty.clone());
                }
                if let Some(sc) = scale
                    && t != owner
                    && ((rf == Some(Rf::N(owner)) && (c.ref_type == "w" || c.ref_type == "h")) || (c.ref_type == "none" && is_len(&c.ty)))
                {
                    value *= sc.apply(c);
                }
                self.mark_equ(t, c, owner_uid);
                self.nodes[t].v.set(&c.ty, value);
                if c.for_ == "des" {
                    let nd = &mut self.nodes[t];
                    nd.des_v.set(&c.ty, value);
                    nd.des_c.get_or_insert_with(SMap::default).set(&c.ty, c.clone());
                    nd.des_o.get_or_insert_with(SMap::default).set(&c.ty, owner);
                    match rf {
                        Some(Rf::N(r)) => nd.des_r.get_or_insert_with(SMap::default).set(&c.ty, r),
                        Some(Rf::Copy) => {}
                        None => {
                            if let Some(dr) = nd.des_r.as_mut() {
                                dr.remove(&c.ty);
                            }
                        }
                    }
                }
                // a value tied to another of the SAME node by equality (h = w): ceilings keep the ratio
                if c.op == "equ" && rf == Some(Rf::N(t)) && is_len(&c.ref_type) {
                    self.nodes[t].link.get_or_insert_with(SMap::default).set(&c.ty, (c.ref_type.clone(), fact_or1(c.fact)));
                }
            }
        }
    }

    fn apply_own(&mut self, owner: NId, scale: Option<Scale>, only_self: bool) {
        let list = self.nodes[owner].constrs.clone();
        self.apply_constrs(owner, &list, scale, only_self, false);
    }

    /// `evalKids`: the owner's constraints plus each child's "self" ones (the children's ideal size).
    fn eval_kids(&mut self, n: NId, scale: Option<Scale>) {
        let kids = self.nodes[n].kids.clone();
        // values imposed from above (for="des" of ancestors) keep holding
        for &k in &kids {
            let (mut v, lte, from_parent) = {
                let nd = &self.nodes[k];
                let mut v = nd.des_v.clone();
                v.spread(&nd.ov);
                if let Some(s) = &nd.shr {
                    v.spread(s);
                }
                if let Some(g) = &nd.gw {
                    v.spread(g);
                }
                let lte = nd.des_lte.clone().unwrap_or_default();
                let fp: FxHashSet<String> = nd.des_v.iter().map(|(k, _)| k.to_string()).collect();
                (v, lte, fp)
            };
            // a size imposed by an ancestor shrinks with the lin between the owner of the constraint
            // and the node, along the value's axis
            let des_c = self.nodes[k].des_c.clone();
            if let Some(des_c) = des_c {
                for (key, c) in des_c.iter() {
                    if !(is_len(key) || key.starts_with("user")) || is_marg(key) {
                        continue;
                    }
                    {
                        let nd = &self.nodes[k];
                        if nd.ov.has(key)
                            || nd.gw.as_ref().is_some_and(|g| g.has(key))
                            || nd.shr.as_ref().is_some_and(|g| g.has(key))
                            || !nd.des_v.has(key)
                        {
                            continue;
                        }
                    }
                    if c.ref_type != "w" && c.ref_type != "h" {
                        continue;
                    }
                    let ax = axis_of(key, &c.ref_type);
                    let o = self.nodes[k].des_o.as_ref().and_then(|m| m.get(key)).copied();
                    let r = self.nodes[k].des_r.as_ref().and_then(|m| m.get(key)).copied();
                    // only if the value comes from the owner's size or from a node that a lin of the same axis shrinks
                    let mut scaled_ref = r == o;
                    let mut p = r.and_then(|r| self.nodes[r].parent);
                    let mut d = 0usize;
                    while !scaled_ref && let Some(pi) = p {
                        if Some(pi) == o || d >= MAX_DEPTH {
                            break;
                        }
                        if let Some(ls) = &self.nodes[pi].ls
                            && (ls.all || ls.axis_w == ax)
                        {
                            scaled_ref = true;
                        }
                        p = self.nodes[pi].parent;
                        d += 1;
                    }
                    let mut f = 1.0;
                    if scaled_ref {
                        let mut q = Some(n);
                        let mut d = 0usize;
                        while let Some(qi) = q {
                            if Some(qi) == o || d >= MAX_DEPTH {
                                break;
                            }
                            if let Some(ls) = &self.nodes[qi].ls
                                && (ls.all || ls.axis_w == ax)
                            {
                                f *= ls.s;
                            }
                            q = self.nodes[qi].parent;
                            d += 1;
                        }
                    }
                    if f != 1.0 {
                        let dv = self.nodes[k].des_v.val(key).unwrap_or(f64::NAN);
                        v.set(key, dv * f);
                    }
                }
            }
            let nd = &mut self.nodes[k];
            nd.v = std::mem::take(&mut v);
            nd.lte = lte;
            nd.hi = None;
            nd.link = None;
            nd.from_parent = Some(from_parent);
        }
        self.apply_own(n, scale, false);
        for &k in &kids {
            self.apply_own(k, None, true);
        }
        // second pass: the parent's references to values a child adjusted
        self.apply_own(n, scale, false);
        for &k in &kids {
            self.apply_own(k, None, true);
        }
    }

    // ----- algorithms -----------------------------------------------------------------------

    pub(super) fn alg_type(&self, n: NId) -> &str {
        self.nodes[n].alg.and_then(|a| a.attr("type")).unwrap_or("none")
    }

    pub(super) fn is_trans(&self, n: NId) -> bool {
        let t = &self.pts[self.nodes[n].pt].ty;
        t == "sibTrans" || t == "parTrans"
    }

    pub fn is_trans_or_conn(&self, n: NId) -> bool {
        self.is_trans(n) || self.alg_type(n) == "conn"
    }

    fn cust_scale(&self, k: NId, ax: char) -> f64 {
        cust_scale_of(self, k, ax)
    }

    /// `kidSize`.
    fn kid_size(&self, k: NId, n: NId) -> (f64, f64) {
        let nd = &self.nodes[n];
        let kv = &self.nodes[k].v;
        let w = axis(kv, true, Some(nd.w)).1.unwrap_or(nd.w);
        let h = axis(kv, false, Some(nd.h)).1.unwrap_or(nd.h);
        let sx = self.cust_scale(k, 'X');
        let sy = self.cust_scale(k, 'Y');
        (w * sx, h * sy)
    }

    fn place(&mut self, k: NId, x: f64, y: f64, w: f64, h: f64) {
        let nd = &mut self.nodes[k];
        nd.rx = Some(x);
        nd.ry = Some(y);
        nd.rw = Some(w);
        nd.rh = Some(h);
    }

    fn alg_composite(&mut self, n: NId) {
        let p = params(self.nodes[n].alg);
        let ar = parse_float(p.ag("ar").unwrap_or(""));
        {
            let nd = &mut self.nodes[n];
            if ar.is_finite() && ar > 0.0 && nd.w > 0.0 && nd.h > 0.0 {
                // fixed ratio: shrinks the side that is left over, centred
                if nd.w / nd.h > ar {
                    let w = nd.h * ar;
                    nd.x += (nd.w - w) / 2.0;
                    nd.w = w;
                } else {
                    let h = nd.w / ar;
                    nd.y += (nd.h - h) / 2.0;
                    nd.h = h;
                }
                let (w, h) = (nd.w, nd.h);
                nd.v.set("w", w);
                nd.v.set("h", h);
            }
        }
        self.eval_kids(n, None);
        let (nw, nh) = (self.nodes[n].w, self.nodes[n].h);
        let kids = self.nodes[n].kids.clone();
        for k in kids {
            let (ax, ay) = {
                let kv = &self.nodes[k].v;
                (axis(kv, true, Some(nw)), axis(kv, false, Some(nh)))
            };
            let w0 = ax.1.unwrap_or(nw);
            let h0 = ay.1.unwrap_or(nh);
            let sx = self.cust_scale(k, 'X');
            let sy = self.cust_scale(k, 'Y');
            let w = w0 * sx;
            let h = h0 * sy;
            // the user's scale around the centre of the computed box
            let x = ax.0.unwrap_or(0.0) + (w0 - w) / 2.0;
            let y = ay.0.unwrap_or(0.0) + (h0 - h) / 2.0;
            self.place(k, x, y, w, h);
        }
    }

    fn lin_main_w(&self, n: NId) -> bool {
        let p = params(self.nodes[n].alg);
        let d = p.ag("linDir").unwrap_or("fromL");
        d == "fromL" || d == "fromR"
    }

    /// `naturalSize`: the natural size (bounding box of the children) of a container put in a w×h box.
    fn natural_size(&mut self, k: NId, w: f64, h: f64, along: Option<bool>) -> (f64, f64) {
        let keep = {
            let nd = &self.nodes[k];
            (nd.x, nd.y, nd.w, nd.h)
        };
        let t = self.alg_type(k).to_string();
        let mut nv = self.nodes[k].v.clone();
        nv.set("w", w);
        nv.set("h", h);
        let keep_v = std::mem::replace(&mut self.nodes[k].v, nv);
        {
            let nd = &mut self.nodes[k];
            nd.x = 0.0;
            nd.y = 0.0;
            nd.w = w;
            nd.h = h;
        }
        // a lin perpendicular to the axis the parent measures: its own scale only moves its axis, which
        // it fits in the box by itself
        let km: Option<bool> = if t == "lin" { Some(self.lin_main_w(k)) } else { None };
        let flat = along.is_some() && km.is_some() && km != along;
        if t == "lin" {
            self.alg_lin(k, true, flat);
        } else if t == "snake" {
            self.alg_snake(k);
        } else {
            self.alg_composite(k);
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for &c in &self.nodes[k].kids {
            let cn = &self.nodes[c];
            let (Some(rw), Some(rh)) = (cn.rw, cn.rh) else { continue };
            if !rw.is_finite() || !rh.is_finite() {
                continue;
            }
            x0 = math::min(x0, cn.rx.unwrap_or(0.0));
            y0 = math::min(y0, cn.ry.unwrap_or(0.0));
            x1 = math::max(x1, cn.rx.unwrap_or(0.0) + rw);
            y1 = math::max(y1, cn.ry.unwrap_or(0.0) + rh);
        }
        {
            let nd = &mut self.nodes[k];
            nd.x = keep.0;
            nd.y = keep.1;
            nd.w = keep.2;
            nd.h = keep.3;
            nd.v = keep_v;
        }
        if x0 == f64::INFINITY {
            return (0.0, 0.0);
        }
        let mut out = (x1 - x0, y1 - y0);
        // what grows on its own axis (horzOne with w INF) does not fit: the content counts
        let km_name = km.map(|m| if m { "w" } else { "h" });
        let grows = self.nodes[k]
            .rules
            .iter()
            .any(|r| r.for_ == "self" && nz_str(&r.for_name).is_none() && Some(r.ty.as_str()) == km_name && r.val == f64::INFINITY);
        if flat && !grows && km == Some(true) && w.is_finite() {
            out.0 = math::min(out.0, w);
        }
        if flat && !grows && km == Some(false) && h.is_finite() {
            out.1 = math::min(out.1, h);
        }
        out
    }

    /// `contentExtent`: the extent of a composite put in a w×h box (only content anchored at the origin).
    fn content_extent(&mut self, k: NId, w: f64, h: f64) -> (f64, f64) {
        // fixed ratio (ar): the composite already fits and centres itself
        if params(self.nodes[k].alg).ag("ar").is_some() {
            return (w, h);
        }
        let keep = {
            let nd = &self.nodes[k];
            (nd.x, nd.y, nd.w, nd.h)
        };
        let mut nv = self.nodes[k].v.clone();
        nv.set("w", w);
        nv.set("h", h);
        let keep_v = std::mem::replace(&mut self.nodes[k].v, nv);
        {
            let nd = &mut self.nodes[k];
            nd.x = 0.0;
            nd.y = 0.0;
            nd.w = w;
            nd.h = h;
        }
        self.alg_composite(k);
        let (mut x1, mut y1, mut x0, mut y0) = (0.0_f64, 0.0_f64, f64::INFINITY, f64::INFINITY);
        for &c in &self.nodes[k].kids {
            let cn = &self.nodes[c];
            let (Some(rw), Some(rh)) = (cn.rw, cn.rh) else { continue };
            x0 = math::min(x0, cn.rx.unwrap_or(0.0));
            y0 = math::min(y0, cn.ry.unwrap_or(0.0));
            x1 = math::max(x1, cn.rx.unwrap_or(0.0) + rw);
            y1 = math::max(y1, cn.ry.unwrap_or(0.0) + rh);
        }
        {
            let nd = &mut self.nodes[k];
            nd.x = keep.0;
            nd.y = keep.1;
            nd.w = keep.2;
            nd.h = keep.3;
            nd.v = keep_v;
        }
        if x0 == f64::INFINITY {
            return (w, h);
        }
        // content that does not start at the origin (centred etc.) does not shrink
        (if x0 > 1.0 { w } else { x1 }, if y0 > 1.0 { h } else { y1 })
    }

    /// `sizeOf` of `algLin`.
    fn lin_size_of(&mut self, k: NId, n: NId, st: &LinSt, w_big: f64, h_big: f64) -> Sz {
        let (zw, zh) = self.kid_size(k, n);
        let main = Some(st.main_w);
        // a spacer without size (reference to a node that does not exist): takes nothing
        if self.alg_type(k) == "sp" && self.nodes[k].shape.and_then(|s| s.attr("type")).is_none_or(str::is_empty) {
            return Sz {
                w: if self.get_val(k, "w").is_none() { 0.0 } else { zw },
                h: if self.get_val(k, "h").is_none() { 0.0 } else { zh },
                ..Sz::default()
            };
        }
        // infinite size (h="INF" of list1): the natural size of the content
        if !zw.is_finite() || !zh.is_finite() {
            let nat = self.natural_size(k, if zw.is_finite() { zw } else { w_big }, if zh.is_finite() { zh } else { h_big }, main);
            return Sz { w: if zw.is_finite() { zw } else { nat.0 }, h: if zh.is_finite() { zh } else { nat.1 }, ..Sz::default() };
        }
        let t = self.alg_type(k).to_string();
        if t == "lin" || t == "snake" {
            // child lin/snake: the content decides — without a constraint on the secondary axis the
            // natural size counts; overflowing the box, it grows
            let nat = self.natural_size(k, zw, zh, main);
            let fw = self.get_val(k, "w").is_none();
            let fh = self.get_val(k, "h").is_none();
            let w = if fw { nat.0 } else { math::max(zw, nat.0) };
            let h = if fh { nat.1 } else { math::max(zh, nat.1) };
            if t == "lin" {
                let km = self.lin_main_w(k);
                if km && !fw && nat.0 > 0.0 && nat.0 < zw - 1.0 && nat.1 <= zh + 1.0 {
                    return Sz { w: nat.0, h, full: Some((zw, zh)), grow: None };
                }
                if !km && !fh && nat.1 > 0.0 && nat.1 < zh - 1.0 && nat.0 <= zw + 1.0 {
                    return Sz { w, h: nat.1, full: Some((zw, zh)), grow: None };
                }
            }
            let grow = if (!fw && nat.0 > zw + 1.0) || (!fh && nat.1 > zh + 1.0) { Some((w, h)) } else { None };
            return Sz { w, h, full: None, grow };
        }
        // composite: on the secondary axis the content counts (process3 has h = 1000 × w)
        if t == "composite" {
            let c = self.content_extent(k, zw, zh);
            if st.horiz && c.1 > 0.0 && c.1 < zh - 1.0 {
                return Sz { w: zw, h: c.1, full: Some((zw, zh)), grow: None };
            }
            if !st.horiz && c.0 > 0.0 && c.0 < zw - 1.0 {
                return Sz { w: c.0, h: zh, full: Some((zw, zh)), grow: None };
            }
        }
        Sz { w: zw, h: zh, full: None, grow: None }
    }

    /// `run` (memoized) of `algLin`.
    fn lin_run(&mut self, n: NId, st: &mut LinSt, s: f64) -> LinRes {
        let main = if st.main_w { "w" } else { "h" };
        let key = (
            norm_bits(s),
            st.mode_all,
            self.nodes[n].kids.iter().map(|&k| self.nodes[k].shr.as_ref().and_then(|r| r.val(main)).map(norm_bits)).collect::<Vec<_>>(),
        );
        if let (Some(lr), Some(lk)) = (st.last_res, &st.last_key)
            && *lk == key
        {
            return lr;
        }
        let r = self.lin_run_now(n, st, s);
        st.last_res = Some(r);
        st.last_key = Some(key);
        r
    }

    fn lin_run_now(&mut self, n: NId, st: &mut LinSt, s: f64) -> LinRes {
        let order = self.nodes[n].kids.clone();
        for &k in &order {
            self.nodes[k].gw = None;
        }
        self.nodes[n].ls = Some(Ls { axis_w: st.main_w, s, all: st.mode_all });
        let scale = Scale::Lin { s, all: st.mode_all, main_w: st.main_w };
        self.eval_kids(n, Some(scale));
        let (w_big, h_big) = (self.nodes[n].w, self.nodes[n].h);
        let mut sizes: Vec<Sz> = Vec::with_capacity(order.len());
        for &k in &order {
            let z = self.lin_size_of(k, n, st, w_big, h_big);
            sizes.push(z);
        }
        // a child that grew: the siblings' constraints that refer to it see the grown size
        if !st.measuring && sizes.iter().any(|z| z.grow.is_some()) {
            for (i, &k) in order.iter().enumerate() {
                if let Some(g) = sizes.get(i).and_then(|z| z.grow) {
                    let mut r = Rec::default();
                    r.set("w", g.0);
                    r.set("h", g.1);
                    self.nodes[k].gw = Some(r);
                }
            }
            self.eval_kids(n, Some(scale));
            sizes.clear();
            for &k in &order {
                let z = self.lin_size_of(k, n, st, w_big, h_big);
                sizes.push(z);
            }
        }
        // positions in sequence (a negative spacer moves the cursor back); the extent is the bounding box
        let sp = self.nodes[n].v.val("sp").unwrap_or(0.0);
        let (mut cur, mut lo, mut hi, mut sec) = (0.0_f64, f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
        let mut starts: Vec<f64> = Vec::with_capacity(sizes.len());
        for (i, z) in sizes.iter().enumerate() {
            if i > 0 {
                cur += sp;
            }
            let a = cur;
            let d = if st.horiz { z.w } else { z.h };
            cur += d;
            lo = math::min(lo, math::min(a, a + d));
            hi = math::max(hi, math::max(a, a + d));
            sec = math::max(sec, if st.horiz { z.h } else { z.w });
            starts.push(a);
        }
        if lo == f64::INFINITY {
            lo = 0.0;
            hi = 0.0;
        }
        st.sizes = sizes;
        st.starts = starts;
        LinRes { len: hi - lo, sec, lo }
    }

    /// `bestScale`: the largest scale in (0, 1] that fits, 12-step bisection like the application (the
    /// result is a multiple of 1/4096); a global step budget keeps nested lins bounded.
    fn best_scale(&mut self, n: NId, st: &mut LinSt, lim: f64) -> f64 {
        if self.lin_run(n, st, 1.0).len <= lim {
            return 1.0;
        }
        self.scale_budget -= 1;
        if self.scale_budget < 0 {
            return 1e-6;
        }
        // shortcut: linear extension in the scale (the common case)
        if lim.is_finite() {
            let l1 = self.lin_run(n, st, 1.0).len;
            let lh = self.lin_run(n, st, 0.5).len;
            let a = (l1 - lh) * 2.0;
            let b = l1 - a;
            if a > 0.0 && a.is_finite() && b.is_finite() {
                let q = math::floor(math::min(1.0, math::max(0.0, (lim - b) / a)) * 4096.0) / 4096.0;
                if q > 0.0 && q < 1.0 && self.lin_run(n, st, q).len <= lim && self.lin_run(n, st, q + 1.0 / 4096.0).len > lim {
                    return q;
                }
            }
        }
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for _ in 0..12 {
            let m = (lo + hi) / 2.0;
            if self.lin_run(n, st, m).len <= lim {
                lo = m;
            } else {
                hi = m;
            }
        }
        if lo == 0.0 || lo.is_nan() { 1e-6 } else { lo }
    }

    fn alg_lin(&mut self, n: NId, measuring: bool, no_scale: bool) {
        let p = params(self.nodes[n].alg);
        let dir = p.ag("linDir").unwrap_or("fromL");
        let horiz = dir == "fromL" || dir == "fromR";
        let rev = dir == "fromR" || dir == "fromB";
        let (w_, h_) = (self.nodes[n].w, self.nodes[n].h);
        let main_w = horiz;
        let main = if main_w { "w" } else { "h" };
        let order = self.nodes[n].kids.clone();
        let mut st =
            LinSt { horiz, main_w, mode_all: false, measuring, sizes: Vec::new(), starts: Vec::new(), last_key: None, last_res: None };
        let avail_s = if horiz { h_ } else { w_ };
        let lim = (if horiz { w_ } else { h_ }) * 1.0001;
        // a rule to grow on the main axis (horzOne of hierarchy4: w INF): does not shrink, grows
        let grow_prim =
            self.nodes[n].rules.iter().any(|r| r.for_ == "self" && nz_str(&r.for_name).is_none() && r.ty == main && r.val == f64::INFINITY);
        // a rule to SHRINK children on the main axis (rule w for="ch" forName="linV" val="0" of hProcess3)
        for &k in &order {
            self.nodes[k].shr = None;
        }
        if !grow_prim && !no_scale && self.lin_run(n, &mut st, 1.0).len > lim {
            let rules = self.nodes[n].rules.clone();
            for r in &rules {
                if r.for_ != "ch" || nz_str(&r.for_name).is_none() || r.ty != main || !r.val.is_finite() {
                    continue;
                }
                let tg: Vec<NId> = order.iter().copied().filter(|&k| Some(self.nodes[k].name.as_str()) == r.for_name.as_deref()).collect();
                if tg.is_empty() {
                    continue;
                }
                let cur = math::max_n(
                    &tg.iter()
                        .map(|&k| {
                            let i = order.iter().position(|&o| o == k).unwrap_or(usize::MAX);
                            let z = st.sizes.get(i);
                            z.map_or(0.0, |z| if horiz { z.w } else { z.h })
                        })
                        .collect::<Vec<_>>(),
                );
                let lo = math::max(0.0, r.val * EMU_MM);
                if cur <= lo || cur.is_nan() || lo.is_nan() {
                    continue;
                }
                let set = |this: &mut Self, x: f64| {
                    for &k in &tg {
                        let mut rr = Rec::default();
                        rr.set(main, x);
                        this.nodes[k].shr = Some(rr);
                    }
                };
                set(self, lo);
                if self.lin_run(n, &mut st, 1.0).len > lim {
                    continue; // not even at the minimum: stays at the minimum and the scale solves the rest
                }
                let (mut a, mut b) = (lo, cur);
                for _ in 0..12 {
                    let m = (a + b) / 2.0;
                    set(self, m);
                    if self.lin_run(n, &mut st, 1.0).len <= lim {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                set(self, a);
                break;
            }
        }
        let mut s = if grow_prim || no_scale { 1.0 } else { self.best_scale(n, &mut st, lim) };
        if s <= 1e-5 {
            st.mode_all = true;
            s = self.best_scale(n, &mut st, lim);
        }
        // not even shrinking fits (absolute sizes): stays as is and overflows
        if s <= 1e-5 {
            st.mode_all = false;
            s = 1.0;
        }
        let LinRes { len, mut sec, lo } = self.lin_run(n, &mut st, s);
        if !measuring && avail_s.is_finite() && sec > avail_s * 1.0001 {
            // the user's scale (custScale) stays out of the cut: the given size is what is cut
            let mut sizes = std::mem::take(&mut st.sizes);
            for (i, z) in sizes.iter_mut().enumerate() {
                let Some(&k) = order.get(i) else { continue };
                let cs = self.cust_scale(k, if horiz { 'Y' } else { 'X' });
                if horiz {
                    if z.h / cs > avail_s {
                        z.h = avail_s * cs;
                    }
                } else if z.w / cs > avail_s {
                    z.w = avail_s * cs;
                }
            }
            let mut all = vec![avail_s];
            all.extend(sizes.iter().map(|z| if horiz { z.h } else { z.w }));
            sec = math::max_n(&all);
            st.sizes = sizes;
        }
        // the box shrunk to the content keeps the inner layout relative to the given size
        for (i, &k) in order.iter().enumerate() {
            self.nodes[k].cw = st.sizes.get(i).and_then(|z| z.full);
        }
        let h_al = p.ag("nodeHorzAlign").unwrap_or("ctr");
        let v_al = p.ag("nodeVertAlign").unwrap_or("mid");
        // alignment of the set (horzAlign/vertAlign of the lin) on both axes; the node's inside the set's band
        let grp = if horiz { p.ag("horzAlign") } else { p.ag("vertAlign") };
        let room = if (if horiz { w_ } else { h_ }).is_finite() { (if horiz { w_ } else { h_ }) - len } else { 0.0 };
        let base = (if matches!(grp, Some("l" | "t")) {
            0.0
        } else if matches!(grp, Some("r" | "b")) {
            room
        } else {
            room / 2.0
        }) - lo;
        let sg = if horiz { p.ag("vertAlign") } else { p.ag("horzAlign") };
        let sec_room = if (if horiz { h_ } else { w_ }).is_finite() { (if horiz { h_ } else { w_ }) - sec } else { 0.0 };
        let band = if matches!(sg, Some("t" | "l")) {
            0.0
        } else if matches!(sg, Some("b" | "r")) {
            sec_room
        } else {
            sec_room / 2.0
        };
        for (i, &k) in order.iter().enumerate() {
            let Some(z) = st.sizes.get(i).copied() else { continue };
            let pos = base + st.starts.get(i).copied().unwrap_or(f64::NAN);
            if horiz {
                let y = band
                    + if v_al == "t" {
                        0.0
                    } else if v_al == "b" {
                        sec - z.h
                    } else {
                        (sec - z.h) / 2.0
                    };
                self.place(k, if rev { w_ - pos - z.w } else { pos }, y, z.w, z.h);
            } else {
                let x = band
                    + if h_al == "l" {
                        0.0
                    } else if h_al == "r" {
                        sec - z.w
                    } else {
                        (sec - z.w) / 2.0
                    };
                self.place(k, x, if rev { h_ - pos - z.h } else { pos }, z.w, z.h);
            }
        }
    }

    fn alg_snake(&mut self, n: NId) {
        let p = params(self.nodes[n].alg);
        let gr_dir = p.ag("grDir").unwrap_or("tL");
        let flow_col = p.ag("flowDir") == Some("col");
        let rev_dir = p.ag("contDir") == Some("revDir");
        let (w_, h_) = (self.nodes[n].w, self.nodes[n].h);
        let items: Vec<NId> = self.nodes[n].kids.iter().copied().filter(|&k| !self.is_trans(k)).collect();
        let count = items.len();
        if count == 0 {
            self.eval_kids(n, None);
            return;
        }
        self.eval_kids(n, None);
        let ideal: Vec<(f64, f64)> = items.iter().map(|&k| self.kid_size(k, n)).collect();
        let iw = math::max_n(&ideal.iter().map(|z| z.0).collect::<Vec<_>>());
        let ih = math::max_n(&ideal.iter().map(|z| z.1).collect::<Vec<_>>());
        let isp = self.nodes[n].v.val("sp").unwrap_or(0.0);
        // in the flow the gap between items is the transition's spacer (sibTrans w = 0.08 × node of bList2);
        // between lines the sp (0.16 × node)
        let trans: Option<NId> = self.nodes[n].kids.iter().copied().find(|&k| self.is_trans(k) && self.alg_type(k) == "sp");
        let gap_of = |this: &Self, sp: f64| -> f64 {
            let Some(tr) = trans else { return sp };
            let z = this.kid_size(tr, n);
            let g = if flow_col { z.1 } else { z.0 };
            if g.is_finite() && g > 0.0 && this.get_val(tr, if flow_col { "h" } else { "w" }).is_some() { g } else { sp }
        };
        let igap = gap_of(self, isp);
        let (mut best_c, mut best_s) = (usize_to_f(count), 0.0_f64);
        let fixed = if p.ag("bkpt") == Some("fixed") { parse_int(p.ag("bkPtFixedVal").unwrap_or(""), 10) } else { f64::NAN };
        let nf = usize_to_f(count);
        for c in 1..=count {
            let cf = usize_to_f(c);
            if fixed.is_finite() && fixed > 0.0 && cf != math::min(nf, fixed) {
                continue;
            }
            let r = math::ceil(nf / cf);
            let (cols, rows) = if flow_col { (r, cf) } else { (cf, r) };
            let tw = cols * iw + (cols - 1.0) * (if flow_col { isp } else { igap });
            let th = rows * ih + (rows - 1.0) * (if flow_col { igap } else { isp });
            let s = math::min(if tw > 0.0 { w_ / tw } else { f64::INFINITY }, if th > 0.0 { h_ / th } else { f64::INFINITY });
            // a tie on the scale: more items per line (bList2 with 5: 4 + 1, measured)
            if s >= best_s * 0.999999 {
                best_c = cf;
                best_s = s;
            }
        }
        let s = math::min(1.0, best_s);
        self.eval_kids(n, Some(Scale::Const(s)));
        let sizes: Vec<(f64, f64)> = items.iter().map(|&k| self.kid_size(k, n)).collect();
        let w = math::max_n(&sizes.iter().map(|z| z.0).collect::<Vec<_>>());
        let h = math::max_n(&sizes.iter().map(|z| z.1).collect::<Vec<_>>());
        let sp = self.nodes[n].v.val("sp").unwrap_or(0.0);
        let gap = gap_of(self, sp);
        let per = best_c;
        let lines = math::ceil(nf / per);
        let (cols, rows) = if flow_col { (lines, per) } else { (per, lines) };
        let tw = cols * w + (cols - 1.0) * (if flow_col { sp } else { gap });
        let th = rows * h + (rows - 1.0) * (if flow_col { gap } else { sp });
        let (ox, oy) = ((w_ - tw) / 2.0, (h_ - th) / 2.0);
        let from_r = gr_dir == "tR" || gr_dir == "bR";
        let from_b = gr_dir == "bL" || gr_dir == "bR";
        for (i, &k) in items.iter().enumerate() {
            let fi = usize_to_f(i);
            let line = math::floor(fi / per);
            let mut at = math::rem(fi, per);
            let in_line = math::min(per, nf - line * per);
            if rev_dir && math::rem(line, 2.0) == 1.0 {
                at = per - 1.0 - at;
            }
            // an incomplete line is centred only with off="ctr" (bList2); without the parameter it starts on
            // the side the line starts (process5: the 3rd under the 2nd on the way back; measured)
            let sh = if p.ag("off") != Some("ctr") { 0.0 } else { ((per - in_line) * ((if flow_col { h } else { w }) + gap)) / 2.0 };
            let (mut cx, mut cy);
            let shs = if rev_dir && math::rem(line, 2.0) == 1.0 { -sh } else { sh };
            if !flow_col {
                cx = at * (w + gap) + shs;
                cy = line * (h + sp);
            } else {
                cy = at * (h + gap) + shs;
                cx = line * (w + sp);
            }
            if from_r {
                cx = tw - cx - w;
            }
            if from_b {
                cy = th - cy - h;
            }
            let Some(z) = sizes.get(i).copied() else { continue };
            self.place(k, ox + cx + (w - z.0) / 2.0, oy + cy + (h - z.1) / 2.0, z.0, z.1);
        }
        // transitions: between the nodes (positioned by the connector, if any)
        let kids = self.nodes[n].kids.clone();
        for k in kids {
            if self.is_trans(k) {
                let z = self.kid_size(k, n);
                self.place(k, 0.0, 0.0, z.0, z.1);
            }
        }
    }

    /// Cycle: nodes on a circle from stAng (0 = 12 o'clock, clockwise), step spanAng/n; radius such that
    /// neighbouring nodes are sibSp apart (on the chord) and the central one (ctrShpMap="fNode") sp from
    /// the ring. The ideal set scales to fit.
    fn alg_cycle(&mut self, n: NId) {
        let p = params(self.nodes[n].alg);
        let nzf = |v: f64, d: f64| if v == 0.0 || v.is_nan() { d } else { v };
        let st = nzf(parse_float(p.ag("stAng").unwrap_or("0")), 0.0);
        let span = nzf(parse_float(p.ag("spanAng").unwrap_or("360")), 360.0);
        let ctr_map = p.ag("ctrShpMap") == Some("fNode");
        let along = p.ag("rotPath") == Some("alongPath");
        self.eval_kids(n, None);
        let items: Vec<NId> = self.nodes[n].kids.iter().copied().filter(|&k| !self.is_trans_or_conn(k)).collect();
        if items.is_empty() {
            return;
        }
        let center: Option<NId> = if ctr_map { items.first().copied() } else { None };
        let ring: Vec<NId> = if ctr_map { items.iter().skip(1).copied().collect() } else { items.clone() };
        let sz: FxHashMap<NId, (f64, f64)> = items.iter().map(|&k| (k, self.kid_size(k, n))).collect();
        let szf = |k: NId| sz.get(&k).copied().unwrap_or((f64::NAN, f64::NAN));
        let nn = ring.len();
        let nf = usize_to_f(nn);
        let sib = self.nodes[n].v.val("sibSp").unwrap_or(0.0);
        let sp = self.nodes[n].v.val("sp").unwrap_or(0.0);
        let full = math::abs(span) >= 359.999;
        let step = if nn > 0 {
            if full {
                span / nf
            } else if nn > 1 {
                span / (nf - 1.0)
            } else {
                0.0
            }
        } else {
            0.0
        };
        // radius: neighbours on the ring sibSp apart on the chord (average width of the pair + sibSp)
        let mut r = 0.0_f64;
        if nn > 1 {
            let half = math::min(std::f64::consts::PI / 2.0, (math::abs(step) * std::f64::consts::PI) / 360.0);
            let mut need = 0.0_f64;
            for i in 0..nn {
                if !full && i == nn - 1 {
                    break;
                }
                let a = szf(ring.get(i).copied().unwrap_or(0));
                let b = szf(ring.get((i + 1) % nn).copied().unwrap_or(0));
                need = math::max(need, (a.0 + b.0) / 2.0 + sib);
            }
            let hs = if half == 0.0 || half.is_nan() { std::f64::consts::PI / 2.0 } else { half };
            r = need / (2.0 * math::sin(hs));
        }
        let mut dws = vec![0.0];
        dws.extend(ring.iter().map(|&k| szf(k).0));
        let d = math::max_n(&dws);
        if let Some(c) = center
            && nn > 0
        {
            let cz = szf(c);
            r = math::max(r, math::max(cz.0, cz.1) / 2.0 + sp + d / 2.0);
        }
        struct CB {
            k: NId,
            x: f64,
            y: f64,
            w: f64,
            h: f64,
            rot: f64,
        }
        let mut boxes: Vec<CB> = Vec::new();
        if let Some(c) = center {
            let cz = szf(c);
            boxes.push(CB { k: c, x: -cz.0 / 2.0, y: -cz.1 / 2.0, w: cz.0, h: cz.1, rot: 0.0 });
        }
        for (i, &k) in ring.iter().enumerate() {
            let z = szf(k);
            let a = ((st + usize_to_f(i) * step) * std::f64::consts::PI) / 180.0;
            let cx = r * math::sin(a);
            let cy = -r * math::cos(a);
            boxes.push(CB {
                k,
                x: cx - z.0 / 2.0,
                y: cy - z.1 / 2.0,
                w: z.0,
                h: z.1,
                rot: if along { st + usize_to_f(i) * step } else { 0.0 },
            });
        }
        let x0 = math::min_n(&boxes.iter().map(|b| b.x).collect::<Vec<_>>());
        let y0 = math::min_n(&boxes.iter().map(|b| b.y).collect::<Vec<_>>());
        let bw = math::max_n(&boxes.iter().map(|b| b.x + b.w).collect::<Vec<_>>()) - x0;
        let bh = math::max_n(&boxes.iter().map(|b| b.y + b.h).collect::<Vec<_>>()) - y0;
        let (nw, nh) = (self.nodes[n].w, self.nodes[n].h);
        let s = quant_scale(math::min(if bw > 0.0 { nw / bw } else { f64::INFINITY }, if bh > 0.0 { nh / bh } else { f64::INFINITY }));
        let ox = (nw - bw * s) / 2.0;
        let oy = (nh - bh * s) / 2.0;
        // the cycle's circle (arc connectors follow it)
        let (nx, ny) = (self.nodes[n].x, self.nodes[n].y);
        self.nodes[n].cyc = Some(Cyc { cx: nx + ox - x0 * s, cy: ny + oy - y0 * s, r: r * s, cw: span >= 0.0 });
        for b in &boxes {
            self.place(b.k, ox + (b.x - x0) * s, oy + (b.y - y0) * s, b.w * s, b.h * s);
            self.nodes[b.k].rot = b.rot;
        }
        // connectors and ideal length values follow the set's scale
        let kids = self.nodes[n].kids.clone();
        for k in kids {
            if !self.is_trans_or_conn(k) {
                continue;
            }
            let z = self.kid_size(k, n);
            self.place(k, 0.0, 0.0, z.0 * s, z.1 * s);
            let old = std::mem::take(&mut self.nodes[k].des_v);
            let mut nv = Rec::default();
            for (key, v) in old.iter() {
                nv.set(key, if is_len(key) { v * s } else { *v });
            }
            self.nodes[k].des_v = nv;
        }
        self.nodes[n].v.set("sibSp", sib * s);
        self.nodes[n].v.set("sp", sp * s);
    }

    /// Pyramid (pyra): N levels of height H/N; level i has width (i+1)/N of the base (fromB: the 1st on
    /// top), a trapezoid with the slope that fits the neighbours; the level's text takes the level's width.
    /// With accounts (children) the pyramid takes (1 − pyraAcctRatio) of the width.
    fn alg_pyra(&mut self, n: NId) {
        let p = params(self.nodes[n].alg);
        self.eval_kids(n, None);
        let comps: Vec<NId> = self.nodes[n].kids.iter().copied().filter(|&k| !self.is_trans_or_conn(k)).collect();
        let count = comps.len();
        if count == 0 {
            return;
        }
        let nf = usize_to_f(count);
        let lvl_name = p.ag("pyraLvlNode").unwrap_or("level").to_string();
        let bg_name = p.ag("pyraAcctBkgdNode").map(str::to_string);
        let tx_name = p.ag("pyraAcctTxNode").map(str::to_string);
        let mut has_acct = false;
        for &c in &comps {
            let kids = self.nodes[c].kids.clone();
            for k in kids {
                if Some(self.nodes[k].name.as_str()) == bg_name.as_deref() && !self.node_paras(k).is_empty() {
                    has_acct = true;
                }
            }
            if has_acct {
                break;
            }
        }
        let ratio = if has_acct { math::min(0.9, math::max(0.0, self.nodes[n].v.val("pyraAcctRatio").unwrap_or(0.0))) } else { 0.0 };
        let (w, h) = (self.nodes[n].w, self.nodes[n].h);
        let pyr_w = w * (1.0 - ratio);
        let lh = h / nf;
        let half = pyr_w / (2.0 * nf);
        let aft = p.ag("pyraAcctPos").unwrap_or("aft") != "bef";
        let inv = p.ag("linDir") == Some("fromT");
        let x0 = if has_acct && !aft { w - pyr_w } else { 0.0 };
        for (i, &c) in comps.iter().enumerate() {
            let fi = usize_to_f(i);
            let step = if inv { nf - fi } else { fi + 1.0 };
            let wi = (pyr_w * step) / nf;
            let xi = x0 + (pyr_w - wi) / 2.0;
            self.place(c, 0.0, fi * lh, w, lh);
            self.nodes[c].preplaced = true;
            self.nodes[c].v.set("w", w);
            self.nodes[c].v.set("h", lh);
            // children of the level: positions dictated by the algorithm (the composite's constraints are symbolic)
            let ckids = self.nodes[c].kids.clone();
            let lvl: Option<NId> = ckids.iter().copied().find(|&k| self.nodes[k].name == lvl_name);
            if let Some(lvl) = lvl {
                self.place(lvl, xi, 0.0, wi, lh);
                self.nodes[lvl].adj_ov =
                    Some(vec![("adj".to_string(), math::round((half / math::max(1.0, math::min(wi, lh))) * 100000.0))]);
                if inv {
                    self.nodes[lvl].rot = 180.0;
                }
                let v = &mut self.nodes[lvl].v;
                v.set("l", xi);
                v.set("t", 0.0);
                v.set("w", wi);
                v.set("h", lh);
            }
            for &k in &ckids {
                if Some(k) == lvl {
                    continue;
                }
                let kname = self.nodes[k].name.clone();
                if Some(kname.as_str()) == bg_name.as_deref() {
                    // from the top corner of the level (account side) to the edge
                    let l = if aft { xi + wi - half } else { 0.0 };
                    let kw = if aft { w - l } else { xi + half };
                    self.place(k, l, 0.0, kw, lh);
                    self.nodes[k].adj_ov = Some(vec![
                        ("adj1".to_string(), 0.0),
                        ("adj2".to_string(), math::round((half / math::max(1.0, math::min(kw, lh))) * 100000.0)),
                    ]);
                    self.nodes[k].rot = if aft != inv { 180.0 } else { 0.0 };
                } else if Some(kname.as_str()) == tx_name.as_deref() {
                    let l = if aft { xi + wi } else { 0.0 };
                    let kw = if aft { w - l } else { xi };
                    self.place(k, l, 0.0, kw, lh);
                } else if let Some(lv) = lvl {
                    // the level's text: by the composite's constraints relative to the level (w = 0.65 × level…)
                    self.eval_kids(c, None);
                    let v = &mut self.nodes[lv].v;
                    v.set("l", xi);
                    v.set("t", 0.0);
                    v.set("w", wi);
                    v.set("h", lh);
                    self.eval_kids_only(c, k);
                    let (ax, ay) = {
                        let kv = &self.nodes[k].v;
                        (axis(kv, true, Some(wi)), axis(kv, false, Some(lh)))
                    };
                    let kw = ax.1.unwrap_or(wi);
                    let kh = ay.1.unwrap_or(lh);
                    self.place(k, ax.0.unwrap_or(xi + (wi - kw) / 2.0), ay.0.unwrap_or(0.0), kw, kh);
                }
            }
        }
    }

    /// `evalKidsOnly`: re-evaluates only the composite's constraints that aim at `target`.
    fn eval_kids_only(&mut self, c: NId, target: NId) {
        {
            let nd = &self.nodes[target];
            let mut v = nd.des_v.clone();
            v.spread(&nd.ov);
            self.nodes[target].v = v;
        }
        let list = self.nodes[c].constrs.clone();
        let tname = self.nodes[target].name.clone();
        for k in list.iter() {
            if k.for_ != "ch" || (nz_str(&k.for_name).is_some() && k.for_name.as_deref() != Some(tname.as_str())) {
                continue;
            }
            let one = [k.clone()];
            self.apply_constrs(c, &one, None, false, true);
        }
    }

    // ----- main loop ------------------------------------------------------------------------

    fn layout_node(&mut self, n: NId, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        // a composite shrunk to the content: the inner layout uses the original size
        let cw = self.nodes[n].cw;
        let boxed = cw.map(|_| (self.nodes[n].w, self.nodes[n].h));
        if let Some((w, h)) = cw {
            self.nodes[n].w = w;
            self.nodes[n].h = h;
        }
        let (w, h) = (self.nodes[n].w, self.nodes[n].h);
        self.nodes[n].v.set("w", w);
        self.nodes[n].v.set("h", h);
        self.layout_node_alg(n, depth);
        if let Some((bw, bh)) = boxed {
            self.nodes[n].w = bw;
            self.nodes[n].h = bh;
        }
    }

    fn layout_node_alg(&mut self, n: NId, depth: usize) {
        let ty = if self.nodes[n].preplaced { "pre".to_string() } else { self.alg_type(n).to_string() };
        match ty.as_str() {
            "pre" => {}
            "lin" => {
                self.alg_lin(n, false, false);
                // a lin shrunk to the content (inner box = the given one): the content starts at the box
                if self.nodes[n].cw.is_some() {
                    let km = self.lin_main_w(n);
                    let ks: Vec<NId> = self.nodes[n]
                        .kids
                        .iter()
                        .copied()
                        .filter(|&k| {
                            let kn = &self.nodes[k];
                            matches!((kn.rw, kn.rh), (Some(rw), Some(rh)) if rw > 0.0 || rh > 0.0)
                        })
                        .collect();
                    if !ks.is_empty() {
                        let m = math::min_n(
                            &ks.iter()
                                .map(|&k| if km { self.nodes[k].rx.unwrap_or(0.0) } else { self.nodes[k].ry.unwrap_or(0.0) })
                                .collect::<Vec<_>>(),
                        );
                        if m.is_finite() && m > 0.0 {
                            let kids = self.nodes[n].kids.clone();
                            for k in kids {
                                let kn = &mut self.nodes[k];
                                if km {
                                    kn.rx = Some(kn.rx.unwrap_or(0.0) - m);
                                } else {
                                    kn.ry = Some(kn.ry.unwrap_or(0.0) - m);
                                }
                            }
                        }
                    }
                }
            }
            "snake" => self.alg_snake(n),
            "hierChild" | "hierRoot" => {
                self.layout_hier(n, depth);
                return;
            }
            "cycle" => self.alg_cycle(n),
            "pyra" => self.alg_pyra(n),
            _ => self.alg_composite(n),
        }
        let kids = self.nodes[n].kids.clone();
        for k in kids {
            let (nx, ny, nw, nh) = {
                let nd = &self.nodes[n];
                (nd.x, nd.y, nd.w, nd.h)
            };
            {
                let kn = &mut self.nodes[k];
                kn.x = nx + kn.rx.unwrap_or(0.0);
                kn.y = ny + kn.ry.unwrap_or(0.0);
                kn.w = kn.rw.unwrap_or(nw);
                kn.h = kn.rh.unwrap_or(nh);
            }
            self.cust_shift(k, n);
            self.layout_node(k, depth + 1);
        }
    }

    fn cust_fact(&self, k: NId, name: &str) -> f64 {
        let s = self.nodes[k].pres.and_then(|p| self.pts[p].pr_set).and_then(|ps| ps.attr(name));
        let v = match s {
            Some(s) => parse_int(s, 10) / 100000.0,
            None => 0.0,
        };
        if v.is_finite() && math::abs(v) < 1000.0 { v } else { 0.0 }
    }

    /// `custShift`: the user's displacement (custLinFactNeighborX/Y): in a lin, a fraction of the NEIGHBOUR's
    /// size (the previous sibling; the first uses the next) on the axis; in a composite, of the node itself.
    fn cust_shift(&mut self, k: NId, n: NId) {
        let fx = self.cust_fact(k, "custLinFactNeighborX");
        let fy = self.cust_fact(k, "custLinFactNeighborY");
        if (fx == 0.0 || fx.is_nan()) && (fy == 0.0 || fy.is_nan()) {
            return;
        }
        let kids = &self.nodes[n].kids;
        let i = kids.iter().position(|&x| x == k);
        let t = self.alg_type(n);
        let nb: Option<NId> = if t == "lin" || t == "snake" {
            // n.kids[i - 1] ?? n.kids[i + 1]
            let prev = i.and_then(|i| i.checked_sub(1)).and_then(|j| kids.get(j)).copied();
            prev.or_else(|| i.and_then(|i| kids.get(i + 1)).copied())
        } else {
            Some(k)
        };
        let Some(nb) = nb else { return };
        let clamp_d = |p: f64, s: f64, d: f64, l: Option<f64>| -> f64 {
            if d == 0.0 || d.is_nan() {
                return d;
            }
            let Some(l) = l else { return d };
            let t = math::min(math::max(p + d, math::min(p, 0.0)), math::max(p, l - s));
            t - p
        };
        // a neighbour without size on the axis (spacer of hProcess9 without h): the parent's counts
        let dim = |this: &Self, key_w: bool| -> f64 {
            let nbn = &this.nodes[nb];
            let r = math::abs(if key_w { nbn.rw } else { nbn.rh }.unwrap_or(0.0));
            if r > 0.0 || this.get_val(nb, if key_w { "w" } else { "h" }).is_some() {
                r
            } else if key_w {
                this.nodes[n].w
            } else {
                this.nodes[n].h
            }
        };
        let fr = self.frame;
        if fx != 0.0 && !fx.is_nan() {
            let d = fx * dim(self, true);
            let (kx, kw) = (self.nodes[k].x, self.nodes[k].w);
            let add = clamp_d(kx, kw, d, fr.map(|f| f.0));
            self.nodes[k].x += add;
        }
        if fy != 0.0 && !fy.is_nan() {
            let d = fy * dim(self, false);
            let (ky, kh) = (self.nodes[k].y, self.nodes[k].h);
            let add = clamp_d(ky, kh, d, fr.map(|f| f.1));
            self.nodes[k].y += add;
        }
    }

    // ----- hierarchy --------------------------------------------------------------------------

    /// `hierBox`: the box of a hierarchy node with the user's scale (custScaleX/Y) of the composite's children.
    fn hier_box(&mut self, n: NId, z: (f64, f64)) -> Bx {
        let mut b = Bx { n, x: 0.0, y: 0.0, w: z.0, h: z.1, inner: None };
        let kids = self.nodes[n].kids.clone();
        if self.alg_type(n) != "composite"
            || self.nodes[n].preplaced
            || !kids.iter().any(|&c| self.cust_scale(c, 'X') != 1.0 || self.cust_scale(c, 'Y') != 1.0)
        {
            return b;
        }
        let keep = {
            let nd = &self.nodes[n];
            (nd.x, nd.y, nd.w, nd.h)
        };
        let mut nv = self.nodes[n].v.clone();
        nv.set("w", z.0);
        nv.set("h", z.1);
        let keep_v = std::mem::replace(&mut self.nodes[n].v, nv);
        {
            let nd = &mut self.nodes[n];
            nd.x = 0.0;
            nd.y = 0.0;
            nd.w = z.0;
            nd.h = z.1;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (0.0_f64, 0.0_f64, z.0, z.1);
        self.alg_composite(n);
        for &c in &kids {
            let cn = &self.nodes[c];
            let (Some(rw), Some(rh)) = (cn.rw, cn.rh) else { continue };
            if !rw.is_finite() || !rh.is_finite() || rw <= 0.0 || rh <= 0.0 {
                continue;
            }
            x0 = math::min(x0, cn.rx.unwrap_or(0.0));
            y0 = math::min(y0, cn.ry.unwrap_or(0.0));
            x1 = math::max(x1, cn.rx.unwrap_or(0.0) + rw);
            y1 = math::max(y1, cn.ry.unwrap_or(0.0) + rh);
        }
        {
            let nd = &mut self.nodes[n];
            nd.x = keep.0;
            nd.y = keep.1;
            nd.w = keep.2;
            nd.h = keep.3;
            nd.v = keep_v;
        }
        if x0 > -1.0 && y0 > -1.0 && x1 < z.0 + 1.0 && y1 < z.1 + 1.0 {
            return b;
        }
        b = Bx { n, x: 0.0, y: 0.0, w: x1 - x0, h: y1 - y0, inner: Some((-x0, -y0, z.0, z.1)) };
        b
    }

    fn is_hier(&self, n: NId) -> bool {
        let t = self.alg_type(n);
        t == "hierChild" || t == "hierRoot"
    }

    fn arrange(&mut self, n: NId, depth: usize, parent: Option<NId>) -> Arr {
        if depth > MAX_DEPTH {
            return Arr { boxes: Vec::new(), anchor: None, span: None };
        }
        let t = self.alg_type(n).to_string();
        if t == "hierRoot" {
            return self.arrange_root(n, depth);
        }
        if t == "hierChild" {
            return self.arrange_child(n, depth);
        }
        let z = match parent {
            Some(p) => self.kid_size(n, p),
            None => (self.nodes[n].w, self.nodes[n].h),
        };
        let b = self.hier_box(n, z);
        Arr { boxes: vec![b], anchor: Some(0), span: None }
    }

    fn is_asst_block(&self, a: &Arr) -> bool {
        !a.boxes.is_empty()
            && a.boxes.iter().all(|b| self.pts[self.nodes[b.n].pt].ty == "asst" || self.is_trans_or_conn(b.n))
            && a.boxes.iter().any(|b| self.pts[self.nodes[b.n].pt].ty == "asst")
    }

    fn arrange_root(&mut self, hr: NId, depth: usize) -> Arr {
        self.eval_kids(hr, None);
        let p = params(self.nodes[hr].alg);
        let align = uo_jsnum::text::to_lowercase(p.ag("hierAlign").unwrap_or("tCtrCH"));
        let sp = self.nodes[hr].v.val("sp").unwrap_or(0.0);
        let off = self.nodes[hr].v.val("alignOff").unwrap_or(0.0);
        let kids = self.nodes[hr].kids.clone();
        let root_k = kids.iter().copied().find(|&k| !self.is_hier(k) && self.alg_type(k) != "conn");
        let ch_ks: Vec<NId> = kids.iter().copied().filter(|&k| self.alg_type(k) == "hierChild").collect();
        let mut boxes: Vec<Bx> = Vec::new();
        let mut anchor: Option<usize> = None;
        if let Some(rk) = root_k {
            let z = self.kid_size(rk, hr);
            self.nodes[rk].w = z.0;
            self.nodes[rk].h = z.1;
            let a = self.hier_box(rk, z);
            boxes.push(a);
            anchor = Some(0);
        }
        // the other non-hierarchical nodes (connectors stay for later)
        let mut blocks: Vec<Arr> = Vec::new();
        for &k in &ch_ks {
            let a = self.arrange(k, depth + 1, Some(hr));
            if !a.boxes.is_empty() {
                blocks.push(a);
            }
        }
        // assistants (hierChild whose children are assistants) come first, stuck to the root
        let mut asst: Vec<Arr> = Vec::new();
        let mut rest: Vec<Arr> = Vec::new();
        for a in blocks {
            if self.is_asst_block(&a) {
                asst.push(a);
            } else {
                rest.push(a);
            }
        }
        asst.extend(rest);
        let mut blocks = asst;
        let side = align.chars().next().unwrap_or('t');
        let vert = side == 't' || side == 'b';
        let (rw, rh) = anchor.and_then(|i| boxes.get(i)).map_or((0.0, 0.0), |a| (a.w, a.h));
        let mut cursor = (if vert { rh } else { rw }) + sp;
        let mode: String = align.chars().skip(1).collect();
        for a in &mut blocks {
            let bb = bbox(&a.boxes);
            // main axis (distance from the root)
            if vert {
                shift(a, 0.0, cursor - bb.1);
            } else {
                shift(a, cursor - bb.0, 0.0);
            }
            cursor += (if vert { bb.3 } else { bb.2 }) + sp;
            // secondary axis: the root's alignment in relation to the block
            let span: Option<(f64, f64)> = a.span.or_else(|| {
                a.anchor
                    .and_then(|i| a.boxes.get(i))
                    .map(|an| if vert { (an.x + an.w / 2.0, an.x + an.w / 2.0) } else { (an.y + an.h / 2.0, an.y + an.h / 2.0) })
            });
            let nb = bbox(&a.boxes);
            if vert {
                if mode == "l" {
                    shift(a, off * rw - nb.0, 0.0);
                } else if mode == "r" {
                    shift(a, rw - off * rw - (nb.0 + nb.2), 0.0);
                } else if let Some(sp2) = span {
                    shift(a, rw / 2.0 - (sp2.0 + sp2.1) / 2.0, 0.0);
                } else {
                    shift(a, rw / 2.0 - (nb.0 + nb.2 / 2.0), 0.0);
                }
            } else if mode == "t" {
                shift(a, 0.0, off * rh - nb.1);
            } else if mode == "b" {
                shift(a, 0.0, rh - off * rh - (nb.1 + nb.3));
            } else if let Some(sp2) = span {
                shift(a, 0.0, rh / 2.0 - (sp2.0 + sp2.1) / 2.0);
            } else {
                shift(a, 0.0, rh / 2.0 - (nb.1 + nb.3 / 2.0));
            }
            boxes.extend(a.boxes.iter().copied());
        }
        // root at the bottom / right: mirrors the main axis
        if side == 'b' || side == 'r' {
            let bb = bbox(&boxes);
            for b in &mut boxes {
                if side == 'b' {
                    b.y = bb.1 + bb.3 - (b.y - bb.1) - b.h;
                } else {
                    b.x = bb.0 + bb.2 - (b.x - bb.0) - b.w;
                }
            }
        }
        Arr { boxes, anchor, span: None }
    }

    fn arrange_child(&mut self, hc: NId, depth: usize) -> Arr {
        self.eval_kids(hc, None);
        let p = params(self.nodes[hc].alg);
        let dir = p.ag("linDir").unwrap_or("fromL");
        let horiz = dir == "fromL" || dir == "fromR";
        let rev = dir == "fromR" || dir == "fromB";
        let ch_align = p.ag("chAlign").unwrap_or(if horiz { "t" } else { "l" }).to_string();
        let sib = self.nodes[hc].v.val("sibSp").unwrap_or(0.0);
        let sec = self.nodes[hc].v.val("secSibSp").unwrap_or(sib);
        let two = p.ag("secLinDir").is_some_and(|s| !s.is_empty());
        let kids: Vec<NId> = self.nodes[hc].kids.iter().copied().filter(|&k| !self.is_trans_or_conn(k)).collect();
        let mut subs: Vec<Arr> = Vec::new();
        for k in kids {
            let a = self.arrange(k, depth + 1, Some(hc));
            if !a.boxes.is_empty() {
                subs.push(a);
            }
        }
        let mut placed: Vec<Bx> = Vec::new();
        let mut anchors: Vec<usize> = Vec::new();
        // in 2D (assistants, "hang"): two columns, rows top to bottom
        let per_row = if two {
            2
        } else if subs.is_empty() {
            1
        } else {
            subs.len()
        };
        let mut row_start = 0.0_f64;
        let mut r = 0usize;
        while r * per_row < subs.len() {
            let lo = r * per_row;
            let hi = math::min(usize_to_f((r + 1) * per_row), usize_to_f(subs.len()));
            let hi = f64_to_usize(hi);
            let mut row_boxes: Vec<Bx> = Vec::new();
            let placed_base = placed.len();
            for (i, a) in subs.iter_mut().enumerate().skip(lo).take(hi.saturating_sub(lo)) {
                let ri = i - lo;
                let bb = bbox(&a.boxes);
                // perpendicular alignment
                if horiz {
                    let d = if ch_align == "b" {
                        -(bb.1 + bb.3)
                    } else if ch_align == "ctr" || ch_align == "mid" {
                        -(bb.1 + bb.3 / 2.0)
                    } else {
                        -bb.1
                    };
                    shift(a, 0.0, d + row_start);
                } else {
                    let d = if ch_align == "r" {
                        -(bb.0 + bb.2)
                    } else if ch_align == "ctr" {
                        -(bb.0 + bb.2 / 2.0)
                    } else {
                        -bb.0
                    };
                    shift(a, d + row_start, 0.0);
                }
                let nb = bbox(&a.boxes);
                if ri == 0 {
                    if horiz {
                        shift(a, -nb.0, 0.0);
                    } else {
                        shift(a, 0.0, -nb.1);
                    }
                } else {
                    // the smallest displacement that keeps sibSp from every box already placed that overlaps on the other axis
                    let mut d = f64::NEG_INFINITY;
                    for q in &row_boxes {
                        for b in &a.boxes {
                            let ov = if horiz {
                                math::min(q.y + q.h, b.y + b.h) - math::max(q.y, b.y)
                            } else {
                                math::min(q.x + q.w, b.x + b.w) - math::max(q.x, b.x)
                            };
                            if ov <= 0.0 {
                                continue;
                            }
                            d = math::max(d, if horiz { q.x + q.w + sib - b.x } else { q.y + q.h + sib - b.y });
                        }
                    }
                    if d == f64::NEG_INFINITY {
                        let pb = bbox(&row_boxes);
                        d = if horiz { pb.0 + pb.2 + sib - nb.0 } else { pb.1 + pb.3 + sib - nb.1 };
                    }
                    if horiz {
                        shift(a, d, 0.0);
                    } else {
                        shift(a, 0.0, d);
                    }
                }
                let base = row_boxes.len();
                row_boxes.extend(a.boxes.iter().copied());
                if let Some(an) = a.anchor {
                    anchors.push(placed_base + base + an);
                }
            }
            placed.extend(row_boxes.iter().copied());
            let rb = bbox(&row_boxes);
            row_start = (if horiz { rb.1 + rb.3 } else { rb.0 + rb.2 }) + sec;
            r += 1;
        }
        if rev {
            let bb = bbox(&placed);
            for b in &mut placed {
                if horiz {
                    b.x = bb.0 + bb.2 - (b.x - bb.0) - b.w;
                } else {
                    b.y = bb.1 + bb.3 - (b.y - bb.1) - b.h;
                }
            }
        }
        let mut out = Arr { boxes: placed, anchor: None, span: None };
        if !anchors.is_empty() {
            let c: Vec<f64> =
                anchors.iter().filter_map(|&i| out.boxes.get(i)).map(|b| if horiz { b.x + b.w / 2.0 } else { b.y + b.h / 2.0 }).collect();
            out.span = Some((math::min_n(&c), math::max_n(&c)));
        }
        out
    }

    /// `layoutHier`: the top of a hierarchy: arranges everything in ideal units, scales to fit, centres.
    fn layout_hier(&mut self, top: NId, depth: usize) {
        let a = self.arrange(top, depth, None);
        if a.boxes.is_empty() {
            return;
        }
        let bb = bbox(&a.boxes);
        let (tw, th, tx, ty) = {
            let t = &self.nodes[top];
            (t.w, t.h, t.x, t.y)
        };
        let s = math::min(if bb.2 > 0.0 { tw / bb.2 } else { f64::INFINITY }, if bb.3 > 0.0 { th / bb.3 } else { f64::INFINITY });
        let k = if s.is_finite() && s > 0.0 { s } else { 1.0 };
        let ox = tx + (tw - bb.2 * k) / 2.0;
        let oy = ty + (th - bb.3 * k) / 2.0;
        // ideal length values (sp, bendDist…) follow the scale
        let mut d = Vec::new();
        self.des_nodes(top, &mut d, 0);
        for n in d {
            let keys: Vec<String> = self.nodes[n].v.iter().map(|(k, _)| k.to_string()).collect();
            for key in keys {
                if is_len(&key) && key != "w" && key != "h" && !is_marg(&key) {
                    let cur = self.nodes[n].v.val(&key).unwrap_or(f64::NAN);
                    self.nodes[n].v.set(&key, cur * k);
                }
            }
        }
        for b in &a.boxes {
            let n = b.n;
            {
                let nd = &mut self.nodes[n];
                nd.x = ox + (b.x - bb.0) * k;
                nd.y = oy + (b.y - bb.1) * k;
                nd.w = b.w * k;
                nd.h = b.h * k;
                // a composite with a child scaled by the user: the inner layout uses the original box
                if let Some((dx, dy, iw, ih)) = b.inner {
                    nd.x += dx * k;
                    nd.y += dy * k;
                    nd.w = iw * k;
                    nd.h = ih * k;
                }
            }
            self.layout_node(n, depth + 1);
        }
    }

    // ----- text rules -------------------------------------------------------------------------

    /// `presTexts`: the points of a node's text (`presOf`) and each paragraph's LEVEL: relative to the
    /// anchor — the point the last `des`/`desOrSelf` step starts from.
    pub fn pres_texts(&self, n: NId, root: PtId) -> Vec<(PtId, f64)> {
        let nd = &self.nodes[n];
        let Some(po) = nd.pres_of else { return Vec::new() };
        let axes = list_attr(po, "axis");
        let last = axes.last().cloned();
        let mut out: Vec<(PtId, f64)> = Vec::new();
        let keep = |p: PtId| {
            let q = &self.pts[p];
            q.t.is_some() || q.ty == "node" || q.ty == "asst"
        };
        if let Some(last) = last.as_deref()
            && (last == "des" || last == "desOrSelf")
        {
            let k = axes.len() - 1;
            let pick = |key: &str| -> Option<(String, String)> {
                let l = list_attr(po, key);
                if l.is_empty() { None } else { Some((key.to_string(), l.iter().take(k).cloned().collect::<Vec<_>>().join(" "))) }
            };
            let mut head = Attrs::default();
            if k > 0 {
                head.set("axis", &axes.iter().take(k).cloned().collect::<Vec<_>>().join(" "));
                for key in ["ptType", "st", "cnt", "step", "hideLastTrans"] {
                    if let Some((a, b)) = pick(key) {
                        head.set(&a, &b);
                    }
                }
            }
            let anchors: Vec<PtId> = if k > 0 { iterate(&self.pts, nd.pt, &head, root, "none") } else { vec![nd.pt] };
            let mut last_attrs = Attrs::default();
            last_attrs.set("axis", last);
            for key in ["ptType", "st", "cnt", "step", "hideLastTrans"] {
                if let Some(v) = list_attr(po, key).get(k) {
                    last_attrs.set(key, v);
                }
            }
            for an in anchors {
                for p in iterate(&self.pts, an, &last_attrs, root, "none") {
                    if keep(p) {
                        let lv = math::max(0.0, usize_to_f(self.pts[p].depth) - usize_to_f(self.pts[an].depth));
                        out.push((p, lv));
                    }
                }
            }
            return out;
        }
        for p in iterate(&self.pts, nd.pt, po, root, "none") {
            if keep(p) {
                out.push((p, 0.0));
            }
        }
        out
    }

    /// `nodeParas`.
    pub fn node_paras(&mut self, n: NId) -> Rc<Vec<TextPara<'a>>> {
        if let Some(p) = &self.nodes[n].paras {
            return p.clone();
        }
        let mut out: Vec<TextPara<'a>> = Vec::new();
        if let Some(root) = self.fit.as_ref().map(|f| f.root) {
            for (pt, level) in self.pres_texts(n, root) {
                let ptn = &self.pts[pt];
                let ps: Vec<&'a X> = ptn.t.map(|t| t.kids_named("a:p").collect()).unwrap_or_default();
                // custom text (custT): the size recorded in the runs counts and does not shrink
                let cust = matches!(ptn.pr_set.and_then(|s| s.attr("custT")), Some("1" | "true"));
                let list: Vec<Option<&'a X>> = if ps.is_empty() { vec![None] } else { ps.into_iter().map(Some).collect() };
                for p in list {
                    let mut csz: Option<f64> = None;
                    if cust && let Some(p) = p {
                        for r in p.kids() {
                            let rp = if r.name == "a:r" || r.name == "a:fld" {
                                r.first("a:rPr")
                            } else if r.name == "a:endParaRPr" {
                                Some(r)
                            } else {
                                None
                            };
                            let v = parse_int(rp.and_then(|x| x.attr("sz")).unwrap_or(""), 10);
                            if v > 0.0 {
                                csz = Some(math::max(csz.unwrap_or(0.0), v / 100.0));
                            }
                        }
                    }
                    out.push(TextPara { p, level, csz: csz.filter(|c| *c != 0.0) });
                }
            }
        }
        let rc = Rc::new(out);
        self.nodes[n].paras = Some(rc.clone());
        rc
    }

    fn has_text_alg(&self, n: NId) -> bool {
        self.alg_type(n) == "tx" && self.nodes[n].pres_of.is_some()
    }

    /// `rulesFor`: the node's own rules (self) and those of ancestors that reach it (ch/des), inside out.
    /// `group`: an ancestor's rule holds for all the targets TOGETHER.
    pub(super) fn rules_for(&self, n: NId) -> (Vec<Rule>, Option<String>) {
        let mut rules: Vec<Rule> =
            self.nodes[n].rules.iter().filter(|r| r.for_ == "self" && nz_str(&r.for_name).is_none()).cloned().collect();
        let mut group: Option<String> = None;
        let mut q = self.nodes[n].parent;
        let mut d = 1usize;
        let name = &self.nodes[n].name;
        while let Some(qi) = q {
            for r in &self.nodes[qi].rules {
                if r.for_ == "self" || (r.for_ == "ch" && d > 1) {
                    continue;
                }
                if nz_str(&r.for_name).is_some_and(|f| f != name) {
                    continue;
                }
                if nz_str(&r.pt_type).is_some_and(|t| !pt_match(&self.pts[self.nodes[n].pt], Some(t))) {
                    continue;
                }
                rules.push(r.clone());
                if group.is_none() && is_font(&r.ty) {
                    group = Some(format!(
                        "r{}:{}:{}:{}",
                        self.nodes[qi].uid,
                        r.ty,
                        r.for_name.as_deref().unwrap_or(""),
                        r.pt_type.as_deref().unwrap_or("")
                    ));
                }
            }
            q = self.nodes[qi].parent;
            d += 1;
        }
        (rules, group)
    }

    /// `margin`: the text margins in EMU at the given size (proportional to the font or absolute).
    pub fn margin(&self, n: NId, k: &str, prim: f64, sec: f64) -> f64 {
        let nd = &self.nodes[n];
        if let Some(f) = nd.marg_f.get(k) {
            let rv = f.2.and_then(|r| self.nodes[r].v.val(&f.1).or_else(|| self.nodes[r].v.val("primFontSz")));
            if let Some(rv) = rv
                && rv.is_finite()
            {
                return f.0 * rv * EMU_PT;
            }
            return f.0 * (if f.1 == "secFontSz" { sec } else { prim }) * EMU_PT;
        }
        // without a constraint: 0.56 × font (measured in pList2/hList1)
        nd.v.val(k).unwrap_or(0.56 * prim * EMU_PT)
    }

    /// `boxFits`: the node's text fits the box w×h (geometry, margins, word wrapping) with the given fonts.
    fn box_fits(&mut self, n: NId, w: f64, h: f64, prim: f64, sec: f64) -> bool {
        let paras = self.node_paras(n);
        if paras.is_empty() || self.fit.is_none() {
            return true;
        }
        let shape = self.nodes[n].shape;
        let ty = shape.and_then(|s| s.attr("type")).unwrap_or("rect").to_string();
        let hide = matches!(shape.and_then(|s| s.attr("hideGeom")), Some("1" | "true")) || ty == "none";
        let nzf = |v: f64| if v == 0.0 || v.is_nan() { 0.0 } else { v };
        let sw = quarter(nzf(parse_float(shape.and_then(|s| s.attr("rot")).unwrap_or("0"))));
        let (lw, lh) = if sw { (h, w) } else { (w, h) };
        let (l, t, r, b) = if hide {
            (0.0, 0.0, lw, lh)
        } else {
            super::emit::text_rect(if ty == "conn" { "rightArrow" } else { &ty }, lw, lh, &self.adj_of(n))
        };
        let (tw, th) = if sw { (b - t, r - l) } else { (r - l, b - t) };
        let (mp, ms) = marg_fonts(&paras, prim, sec);
        let w_a = tw - self.margin(n, "lMarg", mp, ms) - self.margin(n, "rMarg", mp, ms);
        let h_a = th - self.margin(n, "tMarg", mp, ms) - self.margin(n, "bMarg", mp, ms);
        let Some(fit) = &self.fit else { return true };
        super::emit::text_height(&paras, prim, sec, w_a, fit.measure, fit.family, fit.line_k) <= h_a + 1.0
    }

    /// `growLimit`.
    fn grow_limit(r: Option<&Rule>, cur: f64, hi: Option<f64>) -> f64 {
        let Some(r) = r else { return cur };
        let mut l = if r.fact.is_finite() {
            cur * r.fact
        } else if r.val == f64::INFINITY {
            f64::INFINITY
        } else if r.val.is_finite() {
            math::max(cur, r.val * EMU_MM)
        } else {
            f64::INFINITY
        };
        if r.max.is_finite() {
            l = math::min(l, r.max * EMU_MM);
        }
        if let Some(h) = hi {
            l = math::min(l, h); // growing does not pass the lte ceiling (h ≤ 0.4 w)
        }
        math::max(cur, l)
    }

    /// `needBox`: the smallest box (up to the growth rules' limits) in which m's text fits; `None` = does not fit.
    fn need_box(&mut self, m: NId, f: f64, sec: f64, g_h: Option<&Rule>, g_w: Option<&Rule>) -> Option<(f64, f64)> {
        let (bw, bh) = (self.nodes[m].w, self.nodes[m].h);
        if self.box_fits(m, bw, bh, f, sec) {
            return Some((bw, bh));
        }
        let hi_w = self.nodes[m].hi.as_ref().and_then(|h| h.val("w"));
        let hi_h = self.nodes[m].hi.as_ref().and_then(|h| h.val("h"));
        let w_ = Self::grow_limit(g_w, bw, hi_w);
        let h_ = Self::grow_limit(g_h, bh, hi_h);
        if w_ <= bw + 1.0 && h_ <= bh + 1.0 {
            return None;
        }
        let cap_w = if w_.is_finite() { w_ } else { bw * 20.0 };
        let cap_h = if h_.is_finite() { h_ } else { bh * 50.0 + EMU_PT * 400.0 };
        if !self.box_fits(m, cap_w, cap_h, f, sec) {
            return None;
        }
        let (mut w, mut h) = (bw, bh);
        if cap_w > bw + 1.0 {
            let (mut lo, mut hi) = (bw, cap_w);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if self.box_fits(m, mid, cap_h, f, sec) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            w = hi;
        }
        if cap_h > bh + 1.0 {
            let (mut lo, mut hi) = (bh, cap_h);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if self.box_fits(m, w, mid, f, sec) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            h = hi;
        }
        Some((w, h))
    }

    /// `withinFrame`: the visible shapes fit in the frame (rounding slack)?
    fn within_frame(&self, w: f64, h: f64) -> bool {
        let tw = w * 0.0001 + 1.0;
        let th = h * 0.0001 + 1.0;
        for n in &self.nodes {
            let Some(sh) = n.shape else { continue };
            let ty = sh.attr("type");
            if ty.is_none_or(|t| t.is_empty() || t == "none" || t == "conn") || sh.attr("hideGeom") == Some("1") {
                continue;
            }
            if not_pos(n.w) || not_pos(n.h) {
                continue;
            }
            if n.x < -tw || n.y < -th || n.x + n.w > w + tw || n.y + n.h > h + th {
                return false;
            }
        }
        true
    }

    /// The whole layout again with the decided values (`run` of `layoutDiagram`).
    pub(super) fn relayout(&mut self) {
        for i in 0..self.nodes.len() {
            let nd = &mut self.nodes[i];
            nd.v = nd.ov.clone();
            nd.des_v = Rec::default();
            nd.des_c = None;
            nd.des_o = None;
            nd.des_r = None;
            nd.ls = None;
            nd.shr = None;
            nd.des_lte = None;
            nd.marg_f = SMap::default();
            nd.equ = SMap::default();
            nd.lte = SMap::default();
            nd.from_parent = None;
            nd.conn = None;
            nd.cyc = None;
            nd.preplaced = false;
            nd.adj_ov = None;
            nd.rot = 0.0;
            nd.rx = None;
            nd.ry = None;
            nd.rw = None;
            nd.rh = None;
        }
        let root = self.root;
        {
            let r = &mut self.nodes[root];
            r.x = 0.0;
            r.y = 0.0;
            r.w = self.cx;
            r.h = self.cy;
        }
        self.frame = Some((self.cx, self.cy));
        // budget of scale searches PER redo (the rules redo the layout several times)
        self.scale_budget = 20000;
        self.layout_node(root, 0);
    }

    /// `runTextRules` (`diagram-layout.ts:1610`): with text that does not fit, the application applies the
    /// rules IN ORDER — shrinks the font to the limit, grows the height/width to the factor/limit… — and
    /// redoes the layout with the decided values (which win over the constraints).
    pub fn run_text_rules(&mut self) {
        let (frame_w, frame_h) = (self.cx, self.cy);
        let nodes: Vec<NId> = (0..self.nodes.len()).collect();
        let mut group_order: Vec<String> = Vec::new();
        let mut groups: FxHashMap<String, Vec<NId>> = FxHashMap::default();
        for &n in &nodes {
            if !self.has_text_alg(n) || self.node_paras(n).is_empty() || not_pos(self.nodes[n].w) || not_pos(self.nodes[n].h) {
                continue;
            }
            let paras = self.node_paras(n);
            let any_text = paras.iter().any(|p| super::emit::para_text(p.p).iter().any(|r| !uo_jsnum::text::trim(&r.text).is_empty()));
            if !any_text {
                continue;
            }
            let g = self.nodes[n]
                .equ
                .get("primFontSz")
                .cloned()
                .or_else(|| self.nodes[n].equ.get("secFontSz").cloned())
                .or_else(|| self.rules_for(n).1)
                .unwrap_or_else(|| format!("u{}", self.nodes[n].uid));
            if !groups.contains_key(&g) {
                group_order.push(g.clone());
            }
            groups.entry(g).or_default().push(n);
        }
        // layout that depends on the font (a length referred to primFontSz): only fits by redoing
        let font_dependent = nodes.iter().any(|&n| self.nodes[n].constrs.iter().any(|c| is_len(&c.ty) && is_font(&c.ref_type)));
        // groups with an ancestor's font rule first (they define the font the others refer to)
        let mut ordered: Vec<Vec<NId>> = group_order.iter().filter_map(|g| groups.get(g).cloned()).collect();
        {
            let keyed: Vec<(bool, Vec<NId>)> = ordered.drain(..).map(|g| (self.rules_for(g[0]).1.is_none(), g)).collect();
            let (a, b): (Vec<_>, Vec<_>) = keyed.into_iter().partition(|(none, _)| !*none);
            ordered = a.into_iter().chain(b).map(|(_, g)| g).collect();
        }
        // two passes: the second sees the growth decided for the other groups
        let passes: Vec<usize> = if font_dependent && ordered.len() > 1 {
            (0..ordered.len()).chain(0..ordered.len()).collect()
        } else {
            (0..ordered.len()).collect()
        };
        for gi in passes {
            let g = ordered[gi].clone();
            let n0 = g[0];
            let rules = self.rules_for(n0).0;
            let growth = rules.iter().any(|r| r.ty == "h" || r.ty == "w");
            if !growth && !font_dependent {
                continue; // only font: the final fit solves it
            }
            let has_prim = self.node_paras(n0).iter().any(|p| p.level == 0.0);
            let key: &str = if has_prim { "primFontSz" } else { "secFontSz" };
            // the constraint's font (before any rule) and the current one (the 2nd pass starts from it)
            let f_now = math::round(if has_prim {
                self.nodes[n0].v.val("primFontSz").unwrap_or(65.0)
            } else {
                self.nodes[n0].v.val("secFontSz").or_else(|| self.nodes[n0].v.val("primFontSz")).unwrap_or(65.0)
            });
            if self.nodes[n0].fmax0.is_none() {
                self.nodes[n0].fmax0 = Some(f_now);
            }
            // without a font rule of its own the font comes from another node (already decided): the current counts
            let own_font = rules.iter().any(|r| is_font(&r.ty));
            let fmax = if own_font { self.nodes[n0].fmax0.unwrap_or(f_now) } else { f_now };
            let fcur = if own_font { math::round(self.nodes[n0].ov.val(key).unwrap_or(fmax)) } else { fmax };
            let mut use_frame = font_dependent;
            let custom = g.iter().all(|&m| {
                let ps = self.node_paras(m);
                !ps.is_empty() && ps.iter().all(|p| p.csz.is_some())
            });
            // groups that only GROW (no font rule of their own): in the frame check they grow along
            let followers: Vec<Vec<NId>> = if font_dependent && own_font {
                ordered
                    .iter()
                    .enumerate()
                    .filter(|(hi, h)| {
                        let fr = self.rules_for(h[0]).0;
                        *hi != gi && !fr.iter().any(|r| is_font(&r.ty)) && fr.iter().any(|r| r.ty == "h" || r.ty == "w")
                    })
                    .map(|(_, h)| h.clone())
                    .collect()
            } else {
                Vec::new()
            };
            let mut rs = RuleSt {
                g: &g,
                followers: &followers,
                key,
                has_prim,
                fmax,
                n0,
                custom,
                growth,
                text_ok: false,
                frame_ok: false,
                nodes: &nodes,
                frame_w,
                frame_h,
            };
            let mut st = State { f: fcur, g_h: None, g_w: None };
            if self.rule_apply(&mut rs, &st, use_frame) {
                continue;
            }
            // only the frame overflows even at the smallest font of the rules: not a font case
            if use_frame {
                let mut fl: Vec<f64> = rules.iter().filter(|r| is_font(&r.ty) && r.val.is_finite()).map(|r| r.val).collect();
                fl.push(fmax);
                let min_f = math::min_n(&fl);
                let gh = if rules.iter().any(|r| r.ty == "h") { rules.iter().rev().find(|r| r.ty == "h").cloned() } else { None };
                let last = State { f: math::max(1.0, math::round(min_f)), g_h: gh, g_w: None };
                self.rule_apply(&mut rs, &last, use_frame);
                if !rs.frame_ok {
                    use_frame = false;
                    if self.rule_apply(&mut rs, &st, use_frame) {
                        continue;
                    }
                }
            }
            for r in &rules {
                if is_font(&r.ty) {
                    // custom text does not shrink: the font rule only helps if the layout depends on the font
                    if !use_frame && custom {
                        continue;
                    }
                    let lo = math::max(1.0, math::round(if r.val.is_finite() { r.val } else { 1.0 }));
                    if lo >= st.f {
                        continue;
                    }
                    if !self.rule_apply(&mut rs, &State { f: lo, ..st.clone() }, use_frame) {
                        st = State { f: lo, ..st.clone() };
                        continue;
                    }
                    // the largest font that fits (binary search)
                    let (mut a, mut b, mut best) = (lo + 1.0, st.f - 1.0, lo);
                    while a <= b {
                        let mid = f64::from(to_int32(a + b) >> 1);
                        if self.rule_apply(&mut rs, &State { f: mid, ..st.clone() }, use_frame) {
                            best = mid;
                            a = mid + 1.0;
                        } else {
                            b = mid - 1.0;
                        }
                    }
                    st = State { f: best, ..st.clone() };
                    break;
                } else if r.ty == "h" || r.ty == "w" {
                    st = if r.ty == "h" {
                        State { g_h: Some(r.clone()), ..st.clone() }
                    } else {
                        State { g_w: Some(r.clone()), ..st.clone() }
                    };
                    if self.rule_apply(&mut rs, &st, use_frame) {
                        break;
                    }
                }
            }
            self.rule_apply(&mut rs, &st, use_frame);
        }
    }

    /// `secOf` of `runTextRules`.
    fn sec_of(&self, n0: NId, has_prim: bool, f: f64) -> f64 {
        if has_prim {
            match self.nodes[n0].v.val("secFontSz") {
                Some(s) => math::min(f, math::round(s)),
                None => math::round(f * 0.78),
            }
        } else {
            f
        }
    }

    /// `apply` of `runTextRules`: the state (font + growth), redoes the layout and says whether everything fits.
    fn rule_apply(&mut self, rs: &mut RuleSt<'_>, s: &State, use_frame: bool) -> bool {
        for &m in rs.g {
            let nd = &mut self.nodes[m];
            nd.ov.remove("h");
            nd.ov.remove("w");
            if s.f < rs.fmax {
                nd.ov.set(rs.key, s.f);
                if !rs.has_prim {
                    nd.ov.set("primFontSz", s.f);
                }
            } else {
                nd.ov.remove(rs.key);
                if !rs.has_prim {
                    nd.ov.remove("primFontSz");
                }
            }
        }
        for h in rs.followers {
            for &m in h {
                let nd = &mut self.nodes[m];
                nd.ov.remove("h");
                nd.ov.remove("w");
            }
        }
        self.relayout();
        let sec = self.sec_of(rs.n0, rs.has_prim, s.f);
        let mut sizes: Vec<Option<(f64, f64)>> = Vec::new();
        for &m in rs.g {
            sizes.push(self.need_box(m, s.f, sec, s.g_h.as_ref(), s.g_w.as_ref()));
        }
        rs.text_ok = !sizes.iter().any(Option::is_none);
        if rs.text_ok {
            let eq_h = if self.nodes[rs.n0].equ.get("h").is_some_and(|e| !e.is_empty()) {
                Some(math::max_n(&sizes.iter().map(|z| z.map_or(f64::NAN, |z| z.1)).collect::<Vec<_>>()))
            } else {
                None
            };
            let mut grew = false;
            for (i, &m) in rs.g.iter().enumerate() {
                let Some(z) = sizes.get(i).copied().flatten() else { continue };
                let h = eq_h.unwrap_or(z.1);
                if h > self.nodes[m].h + 1.0 {
                    self.nodes[m].ov.set("h", h);
                    grew = true;
                }
                if z.0 > self.nodes[m].w + 1.0 {
                    self.nodes[m].ov.set("w", z.0);
                    grew = true;
                }
            }
            if grew {
                self.relayout();
            }
        }
        let follow_ok = if use_frame && !rs.followers.is_empty() { self.grow_followers(rs) } else { true };
        rs.frame_ok = follow_ok && (!use_frame || self.within_frame(rs.frame_w, rs.frame_h));
        // custom text does not depend on the layout's font: only the frame decides — but the growth rules hold
        (rs.text_ok || (rs.custom && !rs.growth)) && rs.frame_ok
    }

    /// `growFollowers`: false if the text of some follower does not fit even growing.
    fn grow_followers(&mut self, rs: &RuleSt<'_>) -> bool {
        let (mut grew, mut ok) = (false, true);
        for h in rs.followers {
            let fr = self.rules_for(h[0]).0;
            let g_h = fr.iter().rev().find(|r| r.ty == "h").cloned();
            let g_w = fr.iter().rev().find(|r| r.ty == "w").cloned();
            for &m in h {
                if self.node_paras(m).is_empty() || not_pos(self.nodes[m].w) || not_pos(self.nodes[m].h) {
                    continue;
                }
                let prim = math::round(self.nodes[m].v.val("primFontSz").unwrap_or(65.0));
                let sec = math::round(self.nodes[m].v.val("secFontSz").unwrap_or(prim * 0.78));
                let Some(z) = self.need_box(m, prim, sec, g_h.as_ref(), g_w.as_ref()) else {
                    ok = false;
                    continue;
                };
                if z.1 > self.nodes[m].h + 1.0 {
                    self.nodes[m].ov.set("h", z.1);
                    grew = true;
                }
                if z.0 > self.nodes[m].w + 1.0 {
                    self.nodes[m].ov.set("w", z.0);
                    grew = true;
                }
            }
        }
        if grew {
            self.relayout();
        }
        ok
    }

    /// `adjOf`: the adjust values of the node's `dgm:adjLst`.
    pub fn adj_of(&self, n: NId) -> Vec<(String, f64)> {
        let Some(shape) = self.nodes[n].shape else { return Vec::new() };
        let Some(al) = shape.first("dgm:adjLst") else { return Vec::new() };
        let ty = shape.attr("type").unwrap_or("");
        let mut o: Vec<Option<(String, f64)>> = Vec::new();
        for a in al.kids_named("dgm:adj") {
            let i = parse_int(a.attr("idx").unwrap_or("1"), 10);
            let v = parse_float(a.attr("val").unwrap_or("0"));
            if !(1.0..=8.0).contains(&i) || !v.is_finite() {
                continue;
            }
            let ix = f64_to_usize(i) - 1;
            while o.len() <= ix {
                o.push(None);
            }
            if let Some(slot) = o.get_mut(ix) {
                *slot = Some((adj_name(ty, ix + 1), math::round(v * 100000.0)));
            }
        }
        o.into_iter().flatten().collect()
    }
}

/// `margFonts`: the margins' fonts: the custom size (custT) of the 1st paragraph of each level, else the computed one.
pub fn marg_fonts(paras: &[TextPara<'_>], prim: f64, sec: f64) -> (f64, f64) {
    let p0 = paras.iter().find(|p| p.level == 0.0);
    let p1 = paras.iter().find(|p| p.level > 0.0);
    (
        p0.and_then(|p| p.csz).unwrap_or(if p0.is_some() { prim } else { p1.and_then(|p| p.csz).unwrap_or(prim) }),
        p1.and_then(|p| p.csz).unwrap_or(sec),
    )
}

const ONE_ADJ: [&str; 29] = [
    "roundRect",
    "chevron",
    "homePlate",
    "hexagon",
    "octagon",
    "plaque",
    "can",
    "cube",
    "bevel",
    "donut",
    "noSmoking",
    "trapezoid",
    "triangle",
    "parallelogram",
    "snip1Rect",
    "round1Rect",
    "foldedCorner",
    "frame",
    "halfFrame",
    "smileyFace",
    "sun",
    "moon",
    "flowChartOffpageConnector",
    "leftBrace",
    "rightBrace",
    "leftBracket",
    "rightBracket",
    "plus",
    "pieWedge",
];

fn adj_name(ty: &str, i: usize) -> String {
    if ONE_ADJ.contains(&ty) && i == 1 { "adj".to_string() } else { format!("adj{i}") }
}

#[derive(Clone, Debug)]
struct State {
    f: f64,
    g_h: Option<Rule>,
    g_w: Option<Rule>,
}

struct RuleSt<'g> {
    g: &'g [NId],
    followers: &'g [Vec<NId>],
    key: &'static str,
    has_prim: bool,
    fmax: f64,
    n0: NId,
    custom: bool,
    growth: bool,
    text_ok: bool,
    frame_ok: bool,
    #[allow(dead_code)]
    nodes: &'g [NId],
    frame_w: f64,
    frame_h: f64,
}

/// A count as a JS number.
pub fn usize_to_f(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// A non-negative integral JS number as an index (saturating).
pub fn f64_to_usize(x: f64) -> usize {
    uo_jsnum::conv::js_index(x, usize::MAX).unwrap_or(0)
}

/// `!(x > 0)`: true for NaN too.
fn not_pos(x: f64) -> bool {
    x.is_nan() || x <= 0.0
}
