//! Animations of PowerPoint 2002+ stored in the `.ppt` → `<p:timing>` (`ppt/anim.ts`).
//!
//! Where they live: SlideContainer → ProgTags → ProgBinaryTag "___PPT10" → BinaryTagDataBlob
//! (PP10SlideBinaryTagExtension) → ExtTimeNodeContainer (root) + BuildListContainer ([MS-PPT]
//! 2.8). The tree is the same SMIL as PresentationML, node by node. Text targets come as a range
//! of CHARACTERS (VisualShapeAtom) and become a range of PARAGRAPHS (`<p:pRg>`) when they match
//! paragraph boundaries. The ids of the cTn are renumbered in pre-order (as PowerPoint writes).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use uo_jsnum::math::round;
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{is_js_space, trim};

use super::rd::{Rec, RecRead, children, recs_in, rt};
use super::reader::PptSlide;
use super::roundtrip::{RoundTrip, parse_xml};
use super::xmlw::{Attrs, V, el, el_a, esc_attr, n, s};

/// The value of a `TimeVariant` (bool/int/float/string).
#[derive(Debug, Clone, PartialEq)]
enum Variant {
    Bool(bool),
    Num(f64),
    Str(String),
}

/// A variant that remembers "it is a float" (`FloatVal`): written as `<p:fltVal>` even when the
/// value is an integer.
#[derive(Debug, Clone, PartialEq)]
enum TVar {
    Float(f64),
    V(Variant),
}

fn read_variant(rec: &Rec<'_>) -> Option<Variant> {
    let o = rec.start;
    match rec.u8(o) {
        0 => Some(Variant::Bool(rec.u8(o + 1) != 0)),
        1 => Some(Variant::Num(f64::from(rec.i32(o + 1)))),
        2 => Some(Variant::Num(rec.f32(o + 1))),
        3 => {
            let mut units = super::rd::utf16_units(rec.data, o + 1, rec.end);
            if let Some(z) = units.iter().position(|&u| u == 0) {
                units.truncate(z);
            }
            Some(Variant::Str(String::from_utf16_lossy(&units)))
        }
        _ => None,
    }
}

/// Like `read_variant`, but keeps "is a float" for animation values.
fn read_typed_variant(rec: &Rec<'_>) -> Option<TVar> {
    if rec.u8(rec.start) == 2 { Some(TVar::Float(rec.f32(rec.start + 1))) } else { read_variant(rec).map(TVar::V) }
}

/// What the animations need to know of the part (`AnimCtx`).
pub struct AnimCtx<'c> {
    /// the text (with '\r' between paragraphs) of the shape `spid`, if any
    pub texts: &'c HashMap<u32, Rc<Vec<u16>>>,
    /// the spids present in the slide (a target that does not exist drops the behavior)
    pub spids: &'c HashSet<u32>,
    /// the ORIGINAL OOXML id (round-trip, keyed by the bits of the number) → the emitted spid
    pub ooxml_ids: &'c HashMap<u64, f64>,
}

impl AnimCtx<'_> {
    fn has_shape(&self, spid: f64) -> bool {
        // spids are `u32` in the set; any other number is never there
        let v = uo_jsnum::conv::to_uint32(spid);
        f64::from(v) == spid && self.spids.contains(&v)
    }
}

const FILL: [&str; 5] = ["", "remove", "freeze", "hold", "transition"];
const RESTART: [&str; 4] = ["", "always", "whenNotActive", "never"];
const NODE_TYPE: [&str; 10] =
    ["", "clickEffect", "withEffect", "afterEffect", "mainSeq", "interactiveSeq", "clickPar", "withGroup", "afterGroup", "tmRoot"];
const PRESET_CLASS: [&str; 7] = ["", "entr", "exit", "emph", "path", "verb", "mediacall"];
const TRIGGER_EVT: [&str; 12] =
    ["", "onBegin", "onEnd", "begin", "end", "onClick", "onDblClick", "onMouseOver", "onMouseOut", "onNext", "onPrev", "onStopAudio"];
/// Color indices of the scheme of the .ppt → colour names of the generated theme.
const SCHEME_NAMES: [&str; 8] = ["bg1", "tx1", "bg2", "tx2", "accent1", "accent2", "hlink", "folHlink"];

struct Writer<'c> {
    next_id: u32,
    ctx: &'c AnimCtx<'c>,
    /// `lastStarts`: the paragraph starts of the last text asked for
    last_starts: Option<(Rc<Vec<u16>>, Vec<u64>)>,
}

impl Writer<'_> {
    fn id(&mut self) -> f64 {
        let v = self.next_id;
        self.next_id += 1;
        f64::from(v)
    }
}

/// `fmtNum`: integers as they are, others rounded to 3 decimals.
fn fmt_num(v: f64) -> String {
    use uo_jsnum::format::to_js_string;
    if uo_jsnum::conv::is_integer(v) { to_js_string(v) } else { to_js_string(round(v * 1000.0) / 1000.0) }
}

/// A TS array lookup with a number that may not be an index: `None` for what `arr[v]` would not find.
fn at<'t>(arr: &[&'t str], v: f64) -> Option<&'t str> {
    if v.fract() != 0.0 || v < 0.0 {
        return None;
    }
    arr.get(uo_jsnum::conv::js_index(v, arr.len())?).copied()
}

fn at_u(arr: &[&'static str], v: u32) -> Option<&'static str> {
    arr.get(usize::try_from(v).ok()?).copied()
}

const VISUAL_SHAPE_ATOM: u16 = 0x2afb;
const VISUAL_PAGE_ATOM: u16 = 0x2b01;

/// Target (`<p:tgtEl>`) from a `TimeClientVisualElementContainer`: `None` = the shape does not exist.
fn target_xml(cve: Option<&Rec<'_>>, w: &mut Writer<'_>) -> Option<String> {
    let Some(cve) = cve else { return Some(String::new()) };
    for k in children(cve) {
        if k.rec_type == VISUAL_PAGE_ATOM {
            return Some("<p:tgtEl><p:sldTgt/></p:tgtEl>".to_owned());
        }
        if k.rec_type != VISUAL_SHAPE_ATOM {
            continue;
        }
        let o = k.start;
        let (ty, ref_type, spid, d1, d2) = (k.u32(o), k.u32(o + 4), k.u32(o + 8), k.i32(o + 12), k.i32(o + 16));
        if ref_type == 2 {
            return Some(String::new()); // sound (sndTgt) — no equivalent media in the generated package
        }
        if !w.ctx.has_shape(f64::from(spid)) {
            return None;
        }
        let inner = match ty {
            2 => {
                let text = w.ctx.texts.get(&spid).cloned();
                text_range_xml(text, f64::from(d1), f64::from(d2), w)
            }
            6 => "<p:bg/>".to_owned(),
            5 => {
                let t = usize::try_from(d1)
                    .ok()
                    .and_then(|i| ["gridLegend", "series", "category", "ptInSeries", "ptInCategory"].get(i).copied())
                    .unwrap_or("gridLegend");
                el("p:oleChartEl", vec![("type", s(t)), ("lvl", if d2 > 0 { n(d2) } else { None })], "")
            }
            _ => String::new(),
        };
        return Some(format!("<p:tgtEl>{}</p:tgtEl>", el("p:spTgt", vec![("spid", n(spid))], &inner)));
    }
    Some(String::new())
}

/// Character range [st, end) → `<p:txEl>` with pRg (paragraphs) or charRg.
fn text_range_xml(text: Option<Rc<Vec<u16>>>, st: f64, end: f64, w: &mut Writer<'_>) -> String {
    let Some(text) = text else {
        return el("p:txEl", vec![], &el("p:charRg", vec![("st", n(st)), ("end", n(end))], ""));
    };
    // start of each paragraph (the '\r' belongs to the previous one) — remembered per text and
    // with a binary search: many targets in a large text were O(targets x text)
    let hit = w.last_starts.as_ref().is_some_and(|(t, _)| Rc::ptr_eq(t, &text));
    if !hit {
        let mut starts = vec![0u64];
        for (i, &u) in text.iter().enumerate() {
            if u == 0x0d {
                starts.push(i as u64 + 1);
            }
        }
        w.last_starts = Some((Rc::clone(&text), starts));
    }
    let starts: &[u64] = w.last_starts.as_ref().map_or(&[], |(_, st)| st.as_slice());
    let para_at = |pos: f64| -> usize {
        let (mut lo, mut hi) = (0usize, starts.len().saturating_sub(1));
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if starts.get(mid).is_some_and(|&v| (v as f64) <= pos) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    };
    let first = para_at(st);
    let last = para_at(uo_jsnum::math::max(st, end - 1.0));
    let last_end = starts.get(last + 1).map_or(text.len() as f64 + 1.0, |&v| v as f64);
    if starts.get(first).is_some_and(|&v| (v as f64) == st) && (end >= last_end || end == text.len() as f64) {
        return el("p:txEl", vec![], &el("p:pRg", vec![("st", nu_(first)), ("end", nu_(last))], ""));
    }
    el("p:txEl", vec![], &el("p:charRg", vec![("st", n(st)), ("end", n(end))], ""))
}

fn nu_(v: usize) -> Option<V> {
    super::xmlw::nu(v)
}

/// A condition (`TimeConditionContainer` → `<p:cond>`).
fn cond_xml(c: &Rec<'_>, w: &mut Writer<'_>, tag: &str) -> Option<String> {
    let kids = children(c);
    let atom = kids.iter().find(|k| k.rec_type == rt::TIME_CONDITION)?;
    let o = atom.start;
    let (trig_obj, evt, id, delay) = (atom.u32(o), atom.u32(o + 4), atom.u32(o + 8), atom.i32(o + 12));
    let mut inner = String::new();
    if trig_obj == 1 {
        let cve = kids.iter().find(|k| k.rec_type == rt::TIME_CLIENT_VISUAL_ELEMENT);
        inner = target_xml(cve, w)?;
    } else if trig_obj == 2 {
        inner = el("p:tn", vec![("val", n(id))], "");
    } else if trig_obj == 3 {
        let v = at_u(&["first", "last", "all"], id).unwrap_or("all");
        inner = el("p:rtn", vec![("val", s(v))], "");
    }
    let ev = at_u(&TRIGGER_EVT, evt).filter(|e| !e.is_empty());
    Some(el(tag, vec![("evt", ev.and_then(s)), ("delay", if delay == -1 { s("indefinite") } else { n(delay) })], &inner))
}

fn cond_list_xml(conds: &[Rec<'_>], tag: &str, w: &mut Writer<'_>) -> String {
    let items: Vec<String> = conds.iter().filter_map(|c| cond_xml(c, w, "p:cond")).filter(|x| !x.is_empty()).collect();
    if items.is_empty() { String::new() } else { el(tag, vec![], &items.concat()) }
}

/// A decoded node of an `ExtTimeNodeContainer` (or `SubEffectContainer`).
#[derive(Default)]
struct TNode<'a> {
    restart: u32,
    type_: u32,
    fill: u32,
    dur: i32,
    flags: u32,
    props: HashMap<u16, Variant>,
    behavior: Option<Rec<'a>>,
    iterate: Option<Rec<'a>>,
    seq_data: Option<Rec<'a>>,
    conds: HashMap<u16, Vec<Rec<'a>>>,
    modifiers: Vec<(u32, f64)>,
    sub: Vec<TNode<'a>>,
    kids: Vec<TNode<'a>>,
    cve: Option<Rec<'a>>,
}

/// Nesting of the time tree (protection against corrupt files).
const MAX_NODE_DEPTH: usize = 64;

fn read_node<'a>(rec: &Rec<'a>, depth: usize) -> TNode<'a> {
    let mut n = TNode::default();
    if depth > MAX_NODE_DEPTH {
        return n;
    }
    for c in children(rec) {
        match c.rec_type {
            rt::TIME_NODE => {
                let o = c.start;
                n.restart = c.u32(o + 4);
                n.type_ = c.u32(o + 8);
                n.fill = c.u32(o + 12);
                n.dur = c.i32(o + 24);
                n.flags = c.u32(o + 28);
            }
            rt::TIME_PROPERTY_LIST => {
                for v in children(&c) {
                    if v.rec_type == rt::TIME_VARIANT
                        && let Some(val) = read_variant(&v)
                    {
                        n.props.insert(v.inst, val);
                    }
                }
            }
            rt::TIME_SET_BEHAVIOR_CONTAINER
            | rt::TIME_ANIMATE_BEHAVIOR_CONTAINER
            | rt::TIME_EFFECT_BEHAVIOR_CONTAINER
            | rt::TIME_MOTION_BEHAVIOR_CONTAINER
            | rt::TIME_ROTATION_BEHAVIOR_CONTAINER
            | rt::TIME_SCALE_BEHAVIOR_CONTAINER
            | rt::TIME_COLOR_BEHAVIOR_CONTAINER
            | rt::TIME_COMMAND_BEHAVIOR_CONTAINER => n.behavior = Some(c),
            rt::TIME_ITERATE_DATA => n.iterate = Some(c),
            rt::TIME_SEQUENCE_DATA => n.seq_data = Some(c),
            rt::TIME_CONDITION_CONTAINER => n.conds.entry(c.inst).or_default().push(c),
            rt::TIME_MODIFIER => n.modifiers.push((c.u32(c.start), c.f32(c.start + 4))),
            rt::SUB_EFFECT_CONTAINER => n.sub.push(read_node(&c, depth + 1)),
            rt::EXT_TIME_NODE_CONTAINER => n.kids.push(read_node(&c, depth + 1)),
            rt::TIME_CLIENT_VISUAL_ELEMENT => n.cve = Some(c),
            _ => {}
        }
    }
    n
}

fn conds_of<'a>(n: &TNode<'a>, inst: u16) -> Vec<Rec<'a>> {
    n.conds.get(&inst).cloned().unwrap_or_default()
}

fn num_prop(n: &TNode<'_>, k: u16) -> Option<f64> {
    match n.props.get(&k) {
        Some(Variant::Num(v)) => Some(*v),
        _ => None,
    }
}

/// Attributes of the `<p:cTn>`.
fn c_tn_attrs(n: &TNode<'_>, id: f64) -> Attrs {
    let mut a = Attrs::new();
    a.set("id", super::xmlw::n(id));
    let (pc, pid, pst) = (num_prop(n, 0x0b), num_prop(n, 0x09), num_prop(n, 0x0a));
    if let Some(pid) = pid {
        a.set("presetID", self::n(pid));
    }
    if let Some(pc) = pc {
        a.set("presetClass", at(&PRESET_CLASS, pc).and_then(s));
    }
    if let Some(pst) = pst {
        a.set("presetSubtype", self::n(pst));
    }
    if n.flags & 0x2 != 0 {
        a.set("restart", at_u(&RESTART, n.restart).filter(|x| !x.is_empty()).and_then(s));
    }
    if n.flags & 0x10 != 0 {
        a.set("dur", if n.dur == -1 { s("indefinite") } else { self::n(n.dur) });
    }
    // TimeModifierAtom: default values (1 repetition, speed 1) are not written in the XML.
    // "Indefinite" is FLT_MAX (3.4e38) in the binary — measured: repeat until the end of the
    // slide / the next click saved by PowerPoint
    let indefinite = |v: f64| v < 0.0 || !v.is_finite() || v >= 1e30;
    for &(ty, value) in &n.modifiers {
        match ty {
            0 => {
                if value != 1.0 {
                    a.set("repeatCount", if indefinite(value) { s("indefinite") } else { self::n(round(value * 1000.0)) });
                }
            }
            1 => {
                a.set("repeatDur", if indefinite(value) { s("indefinite") } else { self::n(round(value)) });
            }
            2 => {
                if value != 1.0 {
                    a.set("spd", self::n(round(value * 100_000.0)));
                }
            }
            3 => {
                if truthy(value) {
                    a.set("accel", self::n(round(value * 100_000.0)));
                }
            }
            4 => {
                if truthy(value) {
                    a.set("decel", self::n(round(value * 100_000.0)));
                }
            }
            5 if truthy(value) => {
                a.set("autoRev", self::n(1));
            }
            _ => {}
        }
    }
    if n.flags & 0x1 != 0 {
        a.set("fill", at_u(&FILL, n.fill).filter(|x| !x.is_empty()).and_then(s));
    }
    if let Some(Variant::Str(tf)) = n.props.get(&0x10) {
        a.set("tmFilter", s(tf.clone()));
    }
    if let Some(Variant::Str(ef)) = n.props.get(&0x11) {
        a.set("evtFilter", s(ef.clone()));
    }
    // TL_TPID_Display: 1 (or false) = do not show (effects "after the animation")
    match n.props.get(&0x02) {
        Some(Variant::Num(v)) if *v == 1.0 => {
            a.set("display", self::n(0));
        }
        Some(Variant::Bool(false)) => {
            a.set("display", self::n(0));
        }
        _ => {}
    }
    if let Some(mr) = num_prop(n, 0x05) {
        a.set("masterRel", at(&["sameClick", "lastClick", "nextClick"], mr).and_then(s));
    }
    if let Some(g) = num_prop(n, 0x13) {
        a.set("grpId", self::n(g));
    }
    if n.props.get(&0x0d) == Some(&Variant::Bool(true)) {
        a.set("afterEffect", self::n(1));
    }
    if let Some(nt) = num_prop(n, 0x14) {
        a.set("nodeType", at(&NODE_TYPE, nt).and_then(s));
    }
    if n.props.get(&0x15) == Some(&Variant::Bool(true)) {
        a.set("nodePh", self::n(1));
    }
    a
}

/// JS truthiness of a number.
fn truthy(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

/// `<p:iterate>`: TimeIterateDataAtom = interval (f32: seconds or %), type (0 element, 1 word, 2
/// letter), direction (1 = reverse, only with the flag), interval type (0 = absolute, 1 = %),
/// use flags.
fn iterate_xml(rec: Option<&Rec<'_>>) -> String {
    let Some(rec) = rec else { return String::new() };
    let o = rec.start;
    let (interval, ty, dir, itype, f) = (rec.f32(o), rec.u32(o + 4), rec.u32(o + 8), rec.u32(o + 12), rec.u32(o + 16));
    let val = n(round(interval * 1000.0));
    let kid = if itype == 1 { el("p:tmPct", vec![("val", val)], "") } else { el("p:tmAbs", vec![("val", val)], "") };
    let t = at_u(&["el", "wd", "lt"], ty).unwrap_or("el");
    el("p:iterate", vec![("type", s(t)), ("backwards", if f & 1 != 0 && dir == 1 { n(1) } else { None })], &kid)
}

/// Body of the `<p:cTn>` (conditions, iterate, children, subTnLst).
fn c_tn_xml(n: &TNode<'_>, w: &mut Writer<'_>) -> String {
    let id = w.id();
    let a = c_tn_attrs(n, id);
    let mut body = cond_list_xml(&conds_of(n, 1), "p:stCondLst", w);
    body.push_str(&cond_list_xml(&conds_of(n, 2), "p:endCondLst", w));
    if let Some(es) = conds_of(n, 5).first()
        && let Some(x) = cond_xml(es, w, "p:endSync")
        && !x.is_empty()
    {
        body.push_str(&x);
    }
    body.push_str(&iterate_xml(n.iterate.as_ref()));
    let kids: String = n.kids.iter().map(|k| node_xml(k, w)).filter(|x| !x.is_empty()).collect();
    if !kids.is_empty() {
        body.push_str(&el("p:childTnLst", vec![], &kids));
    }
    let sub: String = n.sub.iter().map(|k| node_xml(k, w)).filter(|x| !x.is_empty()).collect();
    if !sub.is_empty() {
        body.push_str(&el("p:subTnLst", vec![], &sub));
    }
    el_a("p:cTn", &a, &body)
}

/// A node → XML (container or behavior); empty if the target does not exist.
fn node_xml(n: &TNode<'_>, w: &mut Writer<'_>) -> String {
    match n.type_ {
        0 => el("p:par", vec![], &c_tn_xml(n, w)),
        1 => {
            let mut a = Attrs::new();
            if let Some(sd) = &n.seq_data {
                let o = sd.start;
                let (conc, next, prev, f) = (sd.u32(o), sd.u32(o + 4), sd.u32(o + 8), sd.u32(o + 16));
                if f & 1 != 0 {
                    a.set("concurrent", self::n(u32::from(conc != 0)));
                }
                if f & 4 != 0 {
                    a.set("prevAc", s(if prev == 1 { "skipTimed" } else { "none" }));
                }
                if f & 2 != 0 {
                    a.set("nextAc", s(if next == 1 { "seek" } else { "none" }));
                }
            }
            let mut inner = c_tn_xml(n, w);
            inner.push_str(&cond_list_xml(&conds_of(n, 4), "p:prevCondLst", w));
            inner.push_str(&cond_list_xml(&conds_of(n, 3), "p:nextCondLst", w));
            el_a("p:seq", &a, &inner)
        }
        2 => el("p:excl", vec![], &c_tn_xml(n, w)),
        3 => match &n.behavior {
            Some(b) => behavior_xml(n, b, w),
            None => String::new(),
        },
        4 => media_xml(n, w),
        _ => String::new(),
    }
}

/// Properties of the behavior (TimePropertyList of the TimeBehaviorContainer): 1 = prLst
/// (animEffect), 2 = execution context (rctx), 3 = relative path, 4 = colour space (0 rgb, 1 hsl),
/// 5 = colour direction (0 cw, 1 ccw), 6 = override (1 = childStyle), 10 = path point types.
type BhvrProps = HashMap<u16, Variant>;

/// `<p:cBhvr>` (cTn + target + attributes) from the TimeBehaviorContainer; `None` = target missing.
fn c_bhvr_xml(n: &TNode<'_>, bc: Option<&Rec<'_>>, w: &mut Writer<'_>, props: &BhvrProps) -> Option<String> {
    let id = w.id();
    let mut tgt = String::new();
    let mut names: Vec<String> = Vec::new();
    let mut a = Attrs::new();
    if let Some(bc) = bc {
        for k in children(bc) {
            if k.rec_type == rt::TIME_BEHAVIOR {
                let o = k.start;
                let (f, add, acc, xf) = (k.u32(o), k.u32(o + 4), k.u32(o + 8), k.u32(o + 12));
                if f & 1 != 0 {
                    a.set("additive", at_u(&["base", "sum", "repl", "mult", "none"], add).and_then(s));
                }
                if f & 2 != 0 {
                    a.set("accumulate", s(if acc == 1 { "none" } else { "always" }));
                }
                if f & 8 != 0 {
                    a.set("xfrmType", s(if xf == 1 { "img" } else { "pt" }));
                }
            } else if k.rec_type == rt::TIME_VARIANT_LIST {
                names = children(&k)
                    .iter()
                    .filter(|v| v.rec_type == rt::TIME_VARIANT)
                    .filter_map(read_variant)
                    .filter_map(|v| if let Variant::Str(x) = v { Some(x) } else { None })
                    .collect();
            } else if k.rec_type == rt::TIME_CLIENT_VISUAL_ELEMENT {
                tgt = target_xml(Some(&k), w)?;
            }
        }
    }
    if let Some(Variant::Str(rctx)) = props.get(&2)
        && !rctx.is_empty()
    {
        a.set("rctx", s(rctx.clone()));
    }
    if props.get(&6) == Some(&Variant::Num(1.0)) {
        a.set("override", s("childStyle"));
    }
    let mut body = el_a(
        "p:cTn",
        &c_tn_attrs(n, id),
        &(cond_list_xml(&conds_of(n, 1), "p:stCondLst", w) + &cond_list_xml(&conds_of(n, 2), "p:endCondLst", w)),
    );
    body.push_str(&tgt);
    if !names.is_empty() {
        let list: String = names.iter().map(|x| format!("<p:attrName>{}</p:attrName>", esc_attr(x))).collect();
        body.push_str(&el("p:attrNameLst", vec![], &list));
    }
    Some(el_a("p:cBhvr", &a, &body))
}

/// The TimePropertyList of the TimeBehaviorContainer.
fn behavior_props(bc: Option<&Rec<'_>>) -> BhvrProps {
    let mut m = HashMap::new();
    let Some(bc) = bc else { return m };
    for k in children(bc) {
        if k.rec_type != rt::TIME_PROPERTY_LIST {
            continue;
        }
        for v in children(&k) {
            let val = if v.rec_type == rt::TIME_VARIANT { read_variant(&v) } else { None };
            if let Some(val) = val {
                m.insert(v.inst, val);
            }
        }
    }
    m
}

/// `/^#([0-9a-fA-F]{6})(?:\s*\[(\d+)\])?$/` over a trimmed string: the six digits and the index.
fn color_text(t: &str) -> Option<(String, Option<String>)> {
    let rest = t.strip_prefix('#')?;
    let hex = rest.get(..6)?;
    if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let tail = rest.get(6..)?;
    if tail.is_empty() {
        return Some((hex.to_owned(), None));
    }
    let after_space = tail.trim_start_matches(is_js_space);
    let inner = after_space.strip_prefix('[')?.strip_suffix(']')?;
    if !inner.is_empty() && inner.bytes().all(|c| c.is_ascii_digit()) { Some((hex.to_owned(), Some(inner.to_owned()))) } else { None }
}

/// Animation value (`<p:val>`/`<p:to>`) from a TimeVariant. Colours come as the text "#rrggbb [i]"
/// (i = index in the scheme) → `<p:clrVal>`.
fn variant_val(tag: &str, v: Option<&TVar>) -> String {
    let Some(v) = v else { return String::new() };
    match v {
        TVar::Float(f) => el(tag, vec![], &el("p:fltVal", vec![("val", s(fmt_num(*f)))], "")),
        TVar::V(Variant::Bool(b)) => el(tag, vec![], &el("p:boolVal", vec![("val", n(u32::from(*b)))], "")),
        TVar::V(Variant::Num(x)) => {
            let inner = if uo_jsnum::conv::is_integer(*x) {
                el("p:intVal", vec![("val", n(*x))], "")
            } else {
                el("p:fltVal", vec![("val", s(fmt_num(*x)))], "")
            };
            el(tag, vec![], &inner)
        }
        TVar::V(Variant::Str(text)) => {
            if let Some((hex, idx)) = color_text(trim(text)) {
                let idx = idx.map(|i| parse_int(&i, 10));
                let name = idx.and_then(|i| at(&SCHEME_NAMES, i)).filter(|x| !x.is_empty());
                let clr = match name {
                    Some(nm) => el("a:schemeClr", vec![("val", s(nm))], ""),
                    None => el("a:srgbClr", vec![("val", s(hex.to_uppercase()))], ""),
                };
                return el(tag, vec![], &el("p:clrVal", vec![], &clr));
            }
            el(tag, vec![], &el("p:strVal", vec![("val", s(text.clone()))], ""))
        }
    }
}

fn pct(v: f64) -> f64 {
    round(v * 1000.0)
}

fn frac(v: f64) -> f64 {
    round(v * 100_000.0)
}

/// `TimeColorStruct` (16 bytes: model + 3 components) → DrawingML colour.
fn color_struct_xml(r: &Rec<'_>, o: u64, for_by: bool) -> String {
    let (model, c0, c1, c2) = (r.u32(o), r.i32(o + 4), r.i32(o + 8), r.i32(o + 12));
    let (f0, f1, f2) = (f64::from(c0), f64::from(c1), f64::from(c2));
    if model == 1 && for_by {
        // HSL: offsets with components 0..255 (hue 255 = 360°)
        return el(
            "p:hsl",
            vec![
                ("h", n(round(((f0 * 360.0) / 255.0) * 60000.0))),
                ("s", n(round((f1 / 255.0) * 100_000.0))),
                ("l", n(round((f2 / 255.0) * 100_000.0))),
            ],
            "",
        );
    }
    if model == 2 {
        let v = usize::try_from(c0).ok().and_then(|i| SCHEME_NAMES.get(i).copied()).unwrap_or("tx1");
        return el("a:schemeClr", vec![("val", s(v))], "");
    }
    if for_by {
        return el(
            "p:rgb",
            vec![
                ("r", n(round((f0 / 255.0) * 100_000.0))),
                ("g", n(round((f1 / 255.0) * 100_000.0))),
                ("b", n(round((f2 / 255.0) * 100_000.0))),
            ],
            "",
        );
    }
    let h = |v: i32| format!("{:02X}", v.clamp(0, 255));
    el("a:srgbClr", vec![("val", s(h(c0) + &h(c1) + &h(c2)))], "")
}

/// Behavior → `<p:set>`/`<p:anim>`/…
fn behavior_xml(n: &TNode<'_>, rec: &Rec<'_>, w: &mut Writer<'_>) -> String {
    let kids = children(rec);
    let bc = kids.iter().find(|k| k.rec_type == rt::TIME_BEHAVIOR_CONTAINER).copied();
    let atom = kids
        .iter()
        .find(|k| k.rec_type >= rt::TIME_BEHAVIOR && k.rec_type <= rt::TIME_COMMAND_BEHAVIOR && k.rec_type != rt::TIME_BEHAVIOR)
        .copied();
    let mut vars: HashMap<u16, Variant> = HashMap::new();
    let mut typed: HashMap<u16, TVar> = HashMap::new();
    for k in &kids {
        if k.rec_type != rt::TIME_VARIANT {
            continue;
        }
        if let Some(v) = read_variant(k) {
            vars.insert(k.inst, v);
        }
        if let Some(t) = read_typed_variant(k) {
            typed.insert(k.inst, t);
        }
    }
    let mut props = behavior_props(bc.as_ref());
    let o = atom.map_or(0, |a| a.start);
    let u32a = |off: u64| atom.map(|a| a.u32(o + off));
    let f32v = |i: u64| atom.map_or(0.0, |a| a.f32(o + 4 + i * 4));
    let bcr = bc.as_ref();
    match rec.rec_type {
        rt::TIME_SET_BEHAVIOR_CONTAINER => {
            let cb = c_bhvr_xml(n, bcr, w, &props);
            match cb {
                None => String::new(),
                Some(cb) => el("p:set", vec![], &(cb + &variant_val("p:to", typed.get(&1)))),
            }
        }
        rt::TIME_EFFECT_BEHAVIOR_CONTAINER => {
            let f = u32a(0).unwrap_or(0);
            let tr = u32a(4).unwrap_or(0);
            // the execution context may also come in the container itself (var 3)
            if let Some(Variant::Str(rc)) = vars.get(&3)
                && !rc.is_empty()
                && !props.contains_key(&2)
            {
                props.insert(2, Variant::Str(rc.clone()));
            }
            let Some(cb) = c_bhvr_xml(n, bcr, w, &props) else { return String::new() };
            let filter = vars.get(&1);
            let progress = vars.get(&2);
            let pr_lst = props.get(&1);
            let transition = if f & 1 != 0 { at_u(&["in", "out", "none"], tr).and_then(s) } else { None };
            let filter_s = if let Some(Variant::Str(x)) = filter { s(x.clone()) } else { None };
            let pr = if let Some(Variant::Str(x)) = pr_lst
                && !x.is_empty()
            {
                s(x.clone())
            } else {
                None
            };
            let prog = if let Some(Variant::Num(p)) = progress {
                el("p:progress", vec![], &el("p:fltVal", vec![("val", s(fmt_num(*p)))], ""))
            } else {
                String::new()
            };
            el("p:animEffect", vec![("transition", transition), ("filter", filter_s), ("prLst", pr)], &(cb + &prog))
        }
        rt::TIME_ANIMATE_BEHAVIOR_CONTAINER => {
            let calc = u32a(0).unwrap_or(1);
            let f = u32a(4).unwrap_or(0);
            let vt = u32a(8).unwrap_or(1);
            let mut a = Attrs::new();
            a.set("calcmode", if f & 8 != 0 { at_u(&["discrete", "lin", "fmla"], calc).and_then(s) } else { None });
            a.set("valueType", if f & 32 != 0 { at_u(&["str", "num", "clr"], vt).and_then(s) } else { None });
            if let Some(Variant::Str(x)) = vars.get(&1) {
                a.set("by", s(x.clone()));
            }
            if let Some(Variant::Str(x)) = vars.get(&2) {
                a.set("from", s(x.clone()));
            }
            if let Some(Variant::Str(x)) = vars.get(&3) {
                a.set("to", s(x.clone()));
            }
            let Some(cb) = c_bhvr_xml(n, bcr, w, &props) else { return String::new() };
            let mut tav = String::new();
            if let Some(list) = kids.iter().find(|k| k.rec_type == rt::TIME_ANIMATION_VALUE_LIST) {
                struct Item {
                    tm: f64,
                    val: Option<TVar>,
                    fmla: Option<String>,
                }
                let mut items: Vec<Item> = Vec::new();
                for k in children(list) {
                    if k.rec_type == rt::TIME_ANIMATION_VALUE {
                        items.push(Item { tm: f64::from(k.i32(k.start)), val: None, fmla: None });
                    } else if k.rec_type == rt::TIME_VARIANT
                        && let Some(cur) = items.last_mut()
                    {
                        if k.inst == 0 {
                            cur.val = read_typed_variant(&k);
                        } else if k.inst == 1
                            && let Some(Variant::Str(f)) = read_variant(&k)
                            && !f.is_empty()
                        {
                            cur.fmla = Some(f);
                        }
                    }
                }
                let body: String = items
                    .iter()
                    .map(|it| {
                        el(
                            "p:tav",
                            vec![("tm", self::n(it.tm * 100.0)), ("fmla", it.fmla.as_deref().and_then(s))],
                            &variant_val("p:val", it.val.as_ref()),
                        )
                    })
                    .collect();
                tav = el("p:tavLst", vec![], &body);
            }
            el_a("p:anim", &a, &(cb + &tav))
        }
        rt::TIME_SCALE_BEHAVIOR_CONTAINER => {
            let f = u32a(0).unwrap_or(0);
            let v = f32v;
            let zoom = atom.map_or(0, |a| a.u8(o + 28));
            let Some(cb) = c_bhvr_xml(n, bcr, w, &props) else { return String::new() };
            let mut inner = cb;
            if f & 1 != 0 {
                inner.push_str(&el("p:by", vec![("x", self::n(pct(v(0)))), ("y", self::n(pct(v(1))))], ""));
            }
            if f & 2 != 0 {
                inner.push_str(&el("p:from", vec![("x", self::n(pct(v(2)))), ("y", self::n(pct(v(3))))], ""));
            }
            if f & 4 != 0 {
                inner.push_str(&el("p:to", vec![("x", self::n(pct(v(4)))), ("y", self::n(pct(v(5))))], ""));
            }
            el("p:animScale", vec![("zoomContents", if f & 8 != 0 { self::n(u32::from(zoom != 0)) } else { None })], &inner)
        }
        rt::TIME_ROTATION_BEHAVIOR_CONTAINER => {
            let f = u32a(0).unwrap_or(0);
            let v = f32v;
            let Some(cb) = c_bhvr_xml(n, bcr, w, &props) else { return String::new() };
            el(
                "p:animRot",
                vec![
                    ("by", if f & 1 != 0 { self::n(round(v(0) * 60000.0)) } else { None }),
                    ("from", if f & 2 != 0 { self::n(round(v(1) * 60000.0)) } else { None }),
                    ("to", if f & 4 != 0 { self::n(round(v(2) * 60000.0)) } else { None }),
                ],
                &cb,
            )
        }
        rt::TIME_MOTION_BEHAVIOR_CONTAINER => {
            let f = u32a(0).unwrap_or(0);
            let v = f32v;
            let origin = u32a(28).unwrap_or(0);
            let path = vars.get(&1);
            let pts = props.get(&10);
            let rel = props.get(&3);
            let Some(cb) = c_bhvr_xml(n, bcr, w, &props) else { return String::new() };
            let mut inner = cb;
            if f & 1 != 0 {
                inner.push_str(&el("p:by", vec![("x", self::n(frac(v(0)))), ("y", self::n(frac(v(1))))], ""));
            }
            if f & 2 != 0 {
                inner.push_str(&el("p:from", vec![("x", self::n(frac(v(2)))), ("y", self::n(frac(v(3))))], ""));
            }
            if f & 4 != 0 {
                inner.push_str(&el("p:to", vec![("x", self::n(frac(v(4)))), ("y", self::n(frac(v(5))))], ""));
            }
            let path_edit = rel.map(|r| if variant_truthy(r) { "relative" } else { "fixed" });
            el(
                "p:animMotion",
                vec![
                    ("origin", if f & 8 != 0 { s(if origin == 2 { "layout" } else { "parent" }) } else { None }),
                    ("path", if let Some(Variant::Str(x)) = path { s(x.clone()) } else { None }),
                    ("pathEditMode", path_edit.and_then(s)),
                    ("ptsTypes", if let Some(Variant::Str(x)) = pts { s(x.clone()) } else { None }),
                ],
                &inner,
            )
        }
        rt::TIME_COLOR_BEHAVIOR_CONTAINER => {
            let f = u32a(0).unwrap_or(0);
            let Some(cb) = c_bhvr_xml(n, bcr, w, &props) else { return String::new() };
            let Some(r) = atom else { return String::new() };
            let mut inner = cb;
            if f & 1 != 0 {
                inner.push_str(&el("p:by", vec![], &color_struct_xml(&r, o + 4, true)));
            }
            if f & 2 != 0 {
                inner.push_str(&el("p:from", vec![], &color_struct_xml(&r, o + 20, false)));
            }
            if f & 4 != 0 {
                inner.push_str(&el("p:to", vec![], &color_struct_xml(&r, o + 36, false)));
            }
            let spc = props.get(&4);
            let dir = props.get(&5);
            el(
                "p:animClr",
                vec![
                    ("clrSpc", spc.map(|v| if *v == Variant::Num(1.0) { "hsl" } else { "rgb" }).and_then(s)),
                    ("dir", dir.map(|v| if *v == Variant::Num(1.0) { "ccw" } else { "cw" }).and_then(s)),
                ],
                &inner,
            )
        }
        rt::TIME_COMMAND_BEHAVIOR_CONTAINER => {
            let f = u32a(0).unwrap_or(0);
            let t = u32a(4).unwrap_or(0);
            let cmd = vars.get(&2).or_else(|| vars.get(&1));
            let Some(cb) = c_bhvr_xml(n, bcr, w, &props) else { return String::new() };
            el(
                "p:cmd",
                vec![
                    ("type", if f & 1 != 0 { at_u(&["evt", "call", "verb"], t).and_then(s) } else { None }),
                    ("cmd", if let Some(Variant::Str(x)) = cmd { s(x.clone()) } else { None }),
                ],
                &cb,
            )
        }
        _ => String::new(),
    }
}

/// JS truthiness of a variant.
fn variant_truthy(v: &Variant) -> bool {
    match v {
        Variant::Bool(b) => *b,
        Variant::Num(x) => truthy(*x),
        Variant::Str(x) => !x.is_empty(),
    }
}

/// Media node (TimeNodeAtom type 4) → `<p:video>`/`<p:audio>`.
fn media_xml(n: &TNode<'_>, w: &mut Writer<'_>) -> String {
    let mut kind = "p:video";
    let mut tgt = String::new();
    if let Some(cve) = &n.cve {
        for k in children(cve) {
            if k.rec_type == VISUAL_SHAPE_ATOM && k.u32(k.start) == 3 {
                kind = "p:audio";
            }
        }
        let Some(t) = target_xml(Some(cve), w) else { return String::new() };
        tgt = t;
    }
    let vol = n.props.get(&0x16);
    let mute = n.props.get(&0x17);
    let hide = n.props.get(&0x12);
    let sld = n.props.get(&0x0f);
    let attrs = vec![
        ("vol", if let Some(Variant::Num(v)) = vol { self::n(round(v * 100_000.0)) } else { None }),
        ("mute", if mute == Some(&Variant::Bool(true)) { self::n(1) } else { None }),
        ("numSld", if let Some(Variant::Num(v)) = sld { if *v != 1.0 { self::n(*v) } else { None } } else { None }),
        ("showWhenStopped", if hide == Some(&Variant::Bool(true)) { self::n(0) } else { None }),
    ];
    let c = c_tn_xml(n, w);
    let cm = el("p:cMediaNode", attrs, &(c + &tgt));
    el(kind, vec![], &cm)
}

/// BuildListContainer → `<p:bldLst>`.
fn build_list_xml(rec: &Rec<'_>, ctx: &AnimCtx<'_>) -> String {
    let mut out: Vec<String> = Vec::new();
    for b in children(rec) {
        let kids = children(&b);
        let Some(ba) = kids.iter().find(|k| k.rec_type == rt::BUILD_ATOM) else { continue };
        // BuildAtom: type, grpId, spid, fExpanded (byte), fUIExpanded (byte)
        let grp_id = ba.u32(ba.start + 4);
        let spid = ba.u32(ba.start + 8);
        let ui_exp = ba.u8(ba.start + 13);
        if !ctx.has_shape(f64::from(spid)) {
            continue;
        }
        match b.rec_type {
            rt::PARA_BUILD => {
                let pa = kids.iter().find(|k| k.rec_type == rt::PARA_BUILD_ATOM);
                let mut a = Attrs::new();
                a.set("spid", n(spid));
                a.set("grpId", n(grp_id));
                if ui_exp != 0 {
                    a.set("uiExpand", n(1));
                }
                if let Some(pa) = pa {
                    // ParaBuildAtom: mode (0 all at once, 1 paragraphs, 2 custom, 3 whole object =
                    // default), level, flags as bytes (background, reverse, …, automatic), delay
                    let o = pa.start;
                    let (build, lvl, delay) = (pa.u32(o), pa.u32(o + 4), pa.u32(o + 12));
                    a.set("build", if build == 3 { None } else { at_u(&["allAtOnce", "p", "cust"], build).and_then(s) });
                    if lvl > 1 {
                        a.set("bldLvl", n(lvl));
                    }
                    if pa.u8(o + 8) != 0 {
                        a.set("animBg", n(1));
                    }
                    if pa.u8(o + 9) != 0 {
                        a.set("rev", n(1));
                    }
                    if pa.u8(o + 11) != 0 {
                        a.set("advAuto", n(delay));
                    }
                }
                out.push(el_a("p:bldP", &a, ""));
            }
            rt::CHART_BUILD => {
                // ChartBuildAtom: mode (0 = whole object → bldAsOne) + animate background (byte)
                let ca = kids.iter().find(|k| k.rec_type == rt::CHART_BUILD_ATOM);
                let t = ca.map_or(0, |c| c.u32(c.start));
                let bg = ca.map_or(0, |c| c.u8(c.start + 4));
                let bld = at_u(&["", "series", "category", "seriesEl", "categoryEl"], t).filter(|x| !x.is_empty());
                let inner = match bld {
                    Some(bld) => {
                        el("p:bldSub", vec![], &el("a:bldChart", vec![("bld", s(bld)), ("animBg", if bg != 0 { None } else { n(0) })], ""))
                    }
                    None => "<p:bldAsOne/>".to_owned(),
                };
                out.push(el("p:bldGraphic", vec![("spid", n(spid)), ("grpId", n(grp_id))], &inner));
            }
            rt::DIAGRAM_BUILD => {
                let da = kids.iter().find(|k| k.rec_type == rt::DIAGRAM_BUILD_ATOM);
                let t = da.map_or(0, |d| d.u32(d.start));
                let names = [
                    "",
                    "depthByNode",
                    "depthByBranch",
                    "breadthByNode",
                    "breadthByLvl",
                    "cw",
                    "cwIn",
                    "cwOut",
                    "ccw",
                    "ccwIn",
                    "ccwOut",
                    "inByRing",
                    "outByRing",
                    "up",
                    "down",
                    "allAtOnce",
                    "cust",
                ];
                let bld = at_u(&names, t).filter(|x| !x.is_empty());
                let inner = match bld {
                    Some(bld) => el("p:bldSub", vec![], &el("a:bldDgm", vec![("bld", s(bld))], "")),
                    None => "<p:bldAsOne/>".to_owned(),
                };
                out.push(el("p:bldGraphic", vec![("spid", n(spid)), ("grpId", n(grp_id))], &inner));
            }
            _ => out.push(el("p:bldOleChart", vec![("spid", n(spid)), ("grpId", n(grp_id))], "")),
        }
    }
    if out.is_empty() { String::new() } else { el("p:bldLst", vec![], &out.concat()) }
}

/// Number of effects (cTn with presetClass) in a timing XML: `xml.match(/presetClass="/g).length`.
fn effect_count(xml: &str) -> usize {
    xml.matches("presetClass=\"").count()
}

/// The ORIGINAL `<p:timing>` of the round trip (RoundTripAnimationAtom12 → `drs/timingInfo.xml`),
/// with the OOXML spids replaced by the emitted ones; `None` without round trip or if some target
/// does not exist any more in the slide.
fn round_trip_timing_xml(slide: &PptSlide<'_>, ctx: &AnimCtx<'_>, rtc: &mut RoundTrip) -> Option<String> {
    let xml = rtc.zip_part_text(slide.round_trip_timing, "timingInfo.xml")?;
    let close = "</p:timing>";
    let i = xml.find("<p:timing")?;
    let j = xml.rfind(close)?;
    if j < i {
        return None;
    }
    // only the `<p:timing>…</p:timing>` element, and only if it is well-formed XML — the rest of
    // the file (or garbage) would enter the slide raw and break it
    let raw = xml.get(i..j + close.len())?;
    parse_xml(raw).ok()?;
    let mut ok = true;
    let mut out = String::with_capacity(raw.len());
    let b = raw.as_bytes();
    let mut run = 0usize;
    let mut p = 0usize;
    while let Some(k) = raw.get(p..).and_then(|r| r.find("spid=\"")) {
        let at = p + k;
        // `\b`: no word character before
        let word_before = at > 0 && b.get(at - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
        let ds = at + 6;
        let nd = raw.get(ds..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
        if !word_before && nd > 0 && b.get(ds + nd) == Some(&b'"') {
            let v = raw.get(ds..ds + nd).unwrap_or("");
            let id = parse_int(v, 10);
            let mapped = ctx.ooxml_ids.get(&id.to_bits()).copied().or_else(|| if ctx.has_shape(id) { Some(id) } else { None });
            out.push_str(raw.get(run..at).unwrap_or(""));
            match mapped {
                None => {
                    ok = false;
                    out.push_str(&format!("spid=\"{v}\""));
                }
                Some(m) => out.push_str(&format!("spid=\"{}\"", uo_jsnum::format::to_js_string(m))),
            }
            p = ds + nd + 1;
            run = p;
        } else {
            p = at + 1;
        }
    }
    out.push_str(raw.get(run..).unwrap_or(""));
    if ok { Some(out) } else { None }
}

/// Does the tree match `/<p:(set|anim|animEffect|animMotion|animRot|animScale|animClr|cmd|video|audio)\b/`?
fn has_behavior_tag(tree: &str) -> bool {
    const NAMES: [&str; 10] = ["set", "anim", "animEffect", "animMotion", "animRot", "animScale", "animClr", "cmd", "video", "audio"];
    let b = tree.as_bytes();
    let mut p = 0usize;
    while let Some(k) = tree.get(p..).and_then(|r| r.find("<p:")) {
        let st = p + k + 3;
        let len = b.get(st..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'_').count());
        let word = tree.get(st..st + len).unwrap_or("");
        if NAMES.contains(&word) {
            return true;
        }
        p = st;
    }
    false
}

/// `<p:timing>` of the slide (an empty string when there are no animations).
pub fn build_timing_xml(slide: &PptSlide<'_>, ctx: &AnimCtx<'_>, rtc: &mut RoundTrip) -> String {
    let recs: Vec<Rec<'_>> = match slide.binary_tags.get("___PPT10") {
        Some(blob) => recs_in(blob.data, blob.start, blob.end).collect(),
        None => Vec::new(),
    };
    let root = recs.iter().find(|r| r.rec_type == rt::EXT_TIME_NODE_CONTAINER);
    let mut out = String::new();
    if let Some(root) = root {
        let mut w = Writer { next_id: 1, ctx, last_starts: None };
        let tree = node_xml(&read_node(root, 0), &mut w);
        if !tree.is_empty() && has_behavior_tag(&tree) {
            let bl = recs.iter().find(|r| r.rec_type == rt::BUILD_LIST);
            let bld = bl.map_or(String::new(), |b| build_list_xml(b, ctx));
            out = el("p:timing", vec![], &(el("p:tnLst", vec![], &tree) + &bld));
        }
    }
    // round trip: the application keeps the original XML when the binary does not represent the
    // animation (chart/SmartArt by parts, text in columns…) — it applies when it has MORE effects
    // than the conversion of the PPT10
    if let Some(rt) = round_trip_timing_xml(slide, ctx, rtc)
        && effect_count(&rt) > effect_count(&out)
    {
        return rt;
    }
    out
}
