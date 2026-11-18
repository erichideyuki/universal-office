//! `parseSlideTiming` and its readers (`timing.ts:255–530, 735–830`): the slide transition (also inside
//! `mc:AlternateContent`) and the `<p:timing>` tree into the [`TimeArena`], then the steps of the main
//! sequence, the interactive effects, the media nodes and the animated backgrounds.
//!
//! `parseTimeNode`'s recursion is an explicit stack bounded by `Limits::max_time_depth` and
//! `Limits::max_time_nodes` (declared in `uo-pptx.md` "B6"): a subtree that would pass either bound is
//! dropped as if it were not a time node, so hostile trees never overflow the stack.

use std::collections::HashSet;

use uo_dml_render::color::CssColor;
use uo_dml_render::ids::Span;
use uo_dml_render::model::MediaKind;
use uo_jsnum::{format::to_js_string, parse};
use uo_opc::xml::Element;

use crate::limits::Limits;
use crate::timing::{
    AnimStep, AnimTarget, BookmarkTrigger, ChartTarget, EffectTiming, Iterate, Keyframe, MediaNodeOpts, NodeId, SlideMediaInfo, TavValue,
    TimeArena, TimeCond, TimeNode, TimingModel, TransitionInfo,
};
use crate::timing_eval::{EvalCtx, begin_offset, collect_group, first_target, step_from, whole_shape};

/// `ResolveColor` (`timing.ts:387`): a DrawingML colour element to `#RRGGBB`.
pub type ResolveColor<'a> = &'a dyn Fn(Element<'_>) -> Option<CssColor>;

/// The inputs `readSlide` hands to `parseSlideTiming`.
pub struct TimingInputs<'a> {
    /// `countTextUnits(shapes)`.
    pub units: &'a dyn Fn(Option<&AnimTarget>, &str) -> f64,
    /// `colorFromDml(el, theme)`.
    pub color: Option<ResolveColor<'a>>,
    /// `hasBackground(shapes)`; without it the `<p:bldP animBg>` of the file decides.
    pub has_background: Option<&'a dyn Fn(&str) -> bool>,
}

/// `unwrapAlternate` (`timing.ts:258`): the child `name`, else inside the first `mc:Choice`/`mc:Fallback`
/// of an `mc:AlternateContent` that has it.
fn unwrap_alternate<'d>(root: Element<'d>, name: &str) -> Option<Element<'d>> {
    if let Some(d) = root.first_child(name) {
        return Some(d);
    }
    for ac in root.children_named("mc:AlternateContent") {
        for branch in ac.child_elements() {
            if branch.name() != "mc:Choice" && branch.name() != "mc:Fallback" {
                continue;
            }
            if let Some(el) = branch.first_child(name) {
                return Some(el);
            }
        }
    }
    None
}

/// The attributes of a transition element in document order.
type Attrs = Vec<(Box<str>, Box<str>)>;

/// `/^\d+$/`.
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn flag(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true"))
}

/// `parseSlideTransition` (`timing.ts:274`).
pub fn parse_slide_transition(sld: Element<'_>) -> Option<TransitionInfo> {
    let el = unwrap_alternate(sld, "p:transition")?;
    // `SPEED_MS[spd] ?? 500`: an unknown speed is 500 ms (a name of `Object.prototype` would give a
    // function in the TS: declared, `uo-pptx.md` "B6")
    let mut dur_ms = match el.attr("spd").unwrap_or("fast") {
        "fast" => 500.0,
        "med" => 750.0,
        "slow" => 1000.0,
        _ => 500.0,
    };
    if let Some(p) = el.attr("p14:dur")
        && all_digits(p)
    {
        dur_ms = parse::parse_int(p, 10);
    }
    let fx = el.child_elements().find(|c| !c.name().ends_with(":sndAc") && c.name() != "p:extLst");
    let (kind, ns, attrs): (Box<str>, Box<str>, Attrs) = match fx {
        Some(f) => {
            let name = f.name();
            // `name.slice(name.indexOf(':') + 1)` and `name.slice(0, name.indexOf(':'))`: without a colon the
            // index is -1, so the prefix is the name minus its last character
            let (ns, kind) = match name.find(':') {
                Some(i) => (&name[..i], &name[i + 1..]),
                None => (drop_last_char(name), name),
            };
            (kind.into(), ns.into(), f.attrs().map(|a| (a.name.into(), a.value.into())).collect())
        }
        None => ("none".into(), "".into(), Vec::new()),
    };
    let ac = el.attr("advClick");
    let mut info = TransitionInfo { kind, ns, attrs, dur_ms, adv_tm_ms: None, adv_click: ac != Some("0") && ac != Some("false") };
    if let Some(adv) = el.attr("advTm")
        && all_digits(adv)
    {
        info.adv_tm_ms = Some(parse::parse_int(adv, 10));
    }
    Some(info)
}

fn drop_last_char(s: &str) -> &str {
    let mut it = s.chars();
    it.next_back();
    it.as_str()
}

/// `num` (`timing.ts:295`): `indefinite` is ∞, a finite `parseFloat`, else `None`.
fn num(v: Option<&str>) -> Option<f64> {
    let v = v?;
    if v == "indefinite" {
        return Some(f64::INFINITY);
    }
    let n = parse::parse_float(v);
    n.is_finite().then_some(n)
}

fn parse_conds(lst: Option<Element<'_>>) -> Vec<TimeCond> {
    let Some(lst) = lst else { return Vec::new() };
    lst.children_named("p:cond")
        .map(|c| {
            let delay_ms = match c.attr("delay") {
                None => Some(0.0),
                Some("indefinite") => None,
                Some(d) => {
                    // `parseFloat(d) || 0`
                    let v = parse::parse_float(d);
                    Some(if v.is_nan() || v == 0.0 { 0.0 } else { v })
                }
            };
            let mut cond = TimeCond { delay_ms, evt: None, spid: None, bookmark: None };
            if let Some(e) = c.attr("evt").filter(|s| !s.is_empty()) {
                cond.evt = Some(e.into());
            }
            if let Some(tgt) = c.first_child("p:tgtEl") {
                for t in tgt.child_elements() {
                    if let Some(s) = t.attr("spid").filter(|s| !s.is_empty()) {
                        cond.spid = Some(s.into());
                    }
                    if let Some(b) = t.attr("bmkName") {
                        cond.bookmark = Some(b.into());
                    }
                }
            }
            cond
        })
        .collect()
}

fn parse_ctn(tag: &str, c_tn: Element<'_>) -> TimeNode {
    let g = |k: &str| c_tn.attr(k);
    let nonempty = |k: &str| c_tn.attr(k).filter(|s| !s.is_empty()).map(Box::<str>::from);
    let mut node = TimeNode { tag: tag.into(), st: parse_conds(c_tn.first_child("p:stCondLst")), ..TimeNode::default() };
    node.id = nonempty("id");
    node.dur = num(g("dur"));
    if let Some(rc) = num(g("repeatCount")) {
        // absurd counts (FLT_MAX from binary files) are also "indefinite"
        node.ctn_x_mut().repeat = Some(if rc == f64::INFINITY || rc >= 1e12 { f64::INFINITY } else { rc / 1000.0 });
    }
    let ends: Vec<Box<str>> =
        parse_conds(c_tn.first_child("p:endCondLst")).into_iter().filter_map(|c| c.evt).filter(|e| !e.is_empty()).collect();
    if !ends.is_empty() {
        node.ctn_x_mut().end_evts = Some(ends);
    }
    if flag(g("autoRev")) {
        node.ctn_x_mut().auto_rev = Some(true);
    }
    if let Some(a) = num(g("accel"))
        && uo_jsnum::conv::truthy(a)
    {
        node.ctn_x_mut().accel = Some(a / 100_000.0);
    }
    if let Some(d) = num(g("decel"))
        && uo_jsnum::conv::truthy(d)
    {
        node.ctn_x_mut().decel = Some(d / 100_000.0);
    }
    node.fill = nonempty("fill");
    node.node_type = nonempty("nodeType");
    node.preset_class = nonempty("presetClass");
    node.preset_id = num(g("presetID"));
    node.preset_subtype = num(g("presetSubtype"));
    node.grp_id = g("grpId").map(Into::into);
    if let Some(s) = num(g("spd"))
        && s > 0.0
    {
        node.ctn_x_mut().spd = Some(s / 100_000.0);
    }
    if let Some(f) = nonempty("tmFilter") {
        let pts: Vec<(f64, f64)> = f
            .split(';')
            .filter_map(|pair| {
                let v: Vec<f64> = pair.split(',').map(parse::parse_float).collect();
                (v.len() == 2 && v[0].is_finite() && v[1].is_finite()).then(|| (v[0], v[1]))
            })
            .collect();
        if pts.len() >= 2 {
            node.ctn_x_mut().tm_filter = Some(pts);
        }
    }
    if let Some(it) = c_tn.first_child("p:iterate") {
        let kind: Box<str> = it.attr("type").unwrap_or("el").into();
        let mut iter = Iterate { kind, backwards: flag(it.attr("backwards")), pct: None, abs_ms: None };
        let pct = it.first_child("p:tmPct");
        let abs = it.first_child("p:tmAbs");
        if let Some(v) = pct.and_then(|p| p.attr("val")) {
            iter.pct = Some(parse::parse_float(v) / 100_000.0);
        }
        if let Some(v) = abs.and_then(|p| p.attr("val")) {
            iter.abs_ms = Some(parse::parse_float(v));
        }
        node.ctn_x_mut().iterate = Some(iter);
    }
    node
}

fn parse_int10(v: Option<&str>, dflt: &str) -> f64 {
    parse::parse_int(v.unwrap_or(dflt), 10)
}

fn parse_target(tgt_el: Option<Element<'_>>) -> Option<AnimTarget> {
    let tgt_el = tgt_el?;
    // ink: `<p:inkTgt spid>` is a SIBLING of `<p:spTgt>` in the `<p:tgtEl>`
    if let Some(ink) = tgt_el.first_child("p:inkTgt")
        && let Some(s) = ink.attr("spid").filter(|s| !s.is_empty())
    {
        return Some(AnimTarget { spid: s.into(), sub: Some("inkTgt".into()), ..AnimTarget::default() });
    }
    let sp = tgt_el.first_child("p:spTgt")?;
    let spid = sp.attr("spid").filter(|s| !s.is_empty())?;
    let mut t = AnimTarget { spid: spid.into(), ..AnimTarget::default() };
    if let Some(tx) = sp.first_child("p:txEl") {
        if let Some(p) = tx.first_child("p:pRg") {
            t.p_range = Some((parse_int10(p.attr("st"), "0"), parse_int10(p.attr("end"), "0")));
        }
        if let Some(c) = tx.first_child("p:charRg") {
            t.char_range = Some((parse_int10(c.attr("st"), "0"), parse_int10(c.attr("end"), "0")));
        }
    }
    for k in ["p:bg", "p:subSp", "p:oleChartEl", "p:graphicEl"] {
        if sp.first_child(k).is_some() {
            t.sub = Some(k[2..].into());
        }
    }
    if let Some(ge) = sp.first_child("p:graphicEl") {
        let ch = ge.first_child("a:chart");
        let dg = ge.first_child("a:dgm");
        if let Some(ch) = ch {
            t.chart = Some(ChartTarget {
                series: parse_int10(ch.attr("seriesIdx"), "-1"),
                category: parse_int10(ch.attr("categoryIdx"), "-1"),
                step: ch.attr("bldStep").unwrap_or("allPts").into(),
            });
        }
        if let Some(id) = dg.and_then(|d| d.attr("id")).filter(|s| !s.is_empty()) {
            t.dgm = Some(id.into());
        }
    }
    Some(t)
}

const BEHAVIORS: [&str; 8] = ["set", "anim", "animEffect", "animMotion", "animRot", "animScale", "animClr", "cmd"];

fn is_color_name(n: &str) -> bool {
    matches!(n, "a:srgbClr" | "a:schemeClr" | "a:sysClr" | "a:prstClr" | "a:scrgbClr" | "a:hslClr")
}

/// First colour child (`a:*Clr`) of an element.
fn color_child(el: Option<Element<'_>>) -> Option<Element<'_>> {
    el?.child_elements().find(|c| is_color_name(c.name()))
}

/// `x`/`y` of `<p:by x y>` etc. divided by `div`.
fn read_xy(el: Option<Element<'_>>, div: f64) -> Option<(f64, f64)> {
    let el = el?;
    let x = num(el.attr("x"));
    let y = num(el.attr("y"));
    if x.is_none() && y.is_none() {
        return None;
    }
    Some((x.unwrap_or(0.0) / div, y.unwrap_or(0.0) / div))
}

enum ValKind {
    Str(Box<str>),
    Num(f64),
}

/// `readVal`: the value of `<p:val>`/`<p:to>` (strVal | fltVal | intVal | boolVal | clrVal).
fn read_val(el: Option<Element<'_>>, color: Option<ResolveColor<'_>>) -> (Option<ValKind>, Option<CssColor>) {
    let Some(el) = el else { return (None, None) };
    for c in el.child_elements() {
        match c.name() {
            "p:strVal" => return (Some(ValKind::Str(c.attr("val").unwrap_or("").into())), None),
            "p:fltVal" | "p:intVal" => return (Some(ValKind::Num(parse::parse_float(c.attr("val").unwrap_or("0")))), None),
            "p:boolVal" => return (Some(ValKind::Num(if flag(c.attr("val")) { 1.0 } else { 0.0 })), None),
            "p:clrVal" => {
                let ce = color_child(Some(c));
                let hex = match (ce, color) {
                    (Some(ce), Some(f)) => f(ce).filter(|h| !h.as_str().is_empty()),
                    _ => None,
                };
                return (None, hex);
            }
            _ => {}
        }
    }
    (None, None)
}

fn local_tag(el: Element<'_>) -> &str {
    let n = el.name();
    n.find(':').map_or(n, |i| &n[i + 1..])
}

/// The behaviour or media node of a leaf element, or `None` (`parseTimeNode` without the containers).
fn parse_leaf(el: Element<'_>, tag: &str, color: Option<ResolveColor<'_>>) -> Option<TimeNode> {
    if BEHAVIORS.contains(&tag) {
        let c_bhvr = el.first_child("p:cBhvr")?;
        let c_tn = c_bhvr.first_child("p:cTn")?;
        let mut node = parse_ctn(tag, c_tn);
        node.bhvr_mut().target = parse_target(c_bhvr.first_child("p:tgtEl"));
        if let Some(names) = c_bhvr.first_child("p:attrNameLst") {
            node.bhvr_mut().attr_names = Some(
                names.children_named("p:attrName").map(|n| n.children().filter_map(|c| c.as_text()).collect::<String>().into()).collect(),
            );
        }
        node.bhvr_mut().additive = c_bhvr.attr("additive").filter(|s| !s.is_empty()).map(Into::into);
        let nz = |k: &str| el.attr(k).filter(|s| !s.is_empty()).map(Box::<str>::from);
        match tag {
            "animEffect" => {
                node.bhvr_mut().filter = nz("filter");
                node.bhvr_mut().transition = nz("transition");
                if let Some(p) = nz("prLst") {
                    node.bhvr_x_mut().pr_lst = Some(p);
                }
            }
            "set" => {
                let (v, c) = read_val(el.first_child("p:to"), color);
                match v {
                    Some(ValKind::Str(s)) => node.bhvr_mut().set_to = Some(s),
                    Some(ValKind::Num(n)) => node.bhvr_mut().set_to = Some(to_js_string(n).into()),
                    None => {}
                }
                if c.is_some() {
                    node.bhvr_mut().to_color = c;
                }
            }
            "anim" => {
                node.bhvr_mut().calcmode = nz("calcmode");
                node.bhvr_mut().value_type = nz("valueType");
                node.bhvr_mut().from = el.attr("from").map(Into::into);
                node.bhvr_mut().to = el.attr("to").map(Into::into);
                node.bhvr_mut().by = el.attr("by").map(Into::into);
                if let Some(lst) = el.first_child("p:tavLst") {
                    let mut tav: Vec<Keyframe> = lst
                        .children_named("p:tav")
                        .map(|t| {
                            let (v, c) = read_val(t.first_child("p:val"), color);
                            Keyframe {
                                tm: t.attr("tm").map_or(f64::NAN, |s| parse::parse_float(s) / 100_000.0),
                                val: v.map(|v| match v {
                                    ValKind::Str(s) => TavValue::Str(s),
                                    ValKind::Num(n) => TavValue::Num(n),
                                }),
                                fmla: t.attr("fmla").filter(|s| !s.is_empty()).map(Into::into),
                                color: c,
                            }
                        })
                        .collect();
                    // tm absent: spread uniformly (SMIL implicit keyTimes)
                    let n = tav.len();
                    for (i, k) in tav.iter_mut().enumerate() {
                        if !k.tm.is_finite() {
                            k.tm = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
                        }
                    }
                    node.bhvr_mut().tav = Some(tav);
                }
            }
            "animScale" => {
                node.bhvr_x_mut().by_xy = read_xy(el.first_child("p:by"), 100_000.0);
                node.bhvr_x_mut().from_xy = read_xy(el.first_child("p:from"), 100_000.0);
                node.bhvr_x_mut().to_xy = read_xy(el.first_child("p:to"), 100_000.0);
            }
            "animRot" => {
                node.bhvr_x_mut().rot_by = num(el.attr("by")).map(|v| v / 60000.0);
                node.bhvr_x_mut().rot_from = num(el.attr("from")).map(|v| v / 60000.0);
                node.bhvr_x_mut().rot_to = num(el.attr("to")).map(|v| v / 60000.0);
            }
            "animMotion" => {
                node.bhvr_x_mut().path = nz("path");
                node.bhvr_x_mut().origin = nz("origin");
                node.bhvr_x_mut().path_edit_mode = nz("pathEditMode");
                node.bhvr_x_mut().r_ang = num(el.attr("rAng")).map(|v| v / 60000.0);
                node.bhvr_x_mut().by_xy = read_xy(el.first_child("p:by"), 100_000.0);
                node.bhvr_x_mut().from_xy = read_xy(el.first_child("p:from"), 100_000.0);
                node.bhvr_x_mut().to_xy = read_xy(el.first_child("p:to"), 100_000.0);
            }
            "cmd" => {
                node.bhvr_x_mut().cmd = el.attr("cmd").map(Into::into);
                node.bhvr_x_mut().cmd_type = nz("type");
            }
            "animClr" => {
                node.bhvr_x_mut().clr_spc = nz("clrSpc");
                node.bhvr_x_mut().clr_dir = nz("dir");
                let to_el = color_child(el.first_child("p:to"));
                let from_el = color_child(el.first_child("p:from"));
                if let (Some(t), Some(f)) = (to_el, color)
                    && let Some(c) = f(t).filter(|c| !c.as_str().is_empty())
                {
                    node.bhvr_mut().to_color = Some(c);
                }
                if let (Some(t), Some(f)) = (from_el, color)
                    && let Some(c) = f(t).filter(|c| !c.as_str().is_empty())
                {
                    node.bhvr_x_mut().from_color = Some(c);
                }
                if let Some(hsl) = el.first_child("p:by").and_then(|b| b.first_child("p:hsl")) {
                    node.bhvr_x_mut().by_hsl = Some((
                        num(hsl.attr("h")).unwrap_or(0.0) / 60000.0,
                        num(hsl.attr("s")).unwrap_or(0.0) / 100_000.0,
                        num(hsl.attr("l")).unwrap_or(0.0) / 100_000.0,
                    ));
                }
            }
            _ => {}
        }
        return Some(node);
    }
    if tag == "audio" || tag == "video" {
        let media = el.first_child("p:cMediaNode")?;
        let c_tn = media.first_child("p:cTn")?;
        let mut node = parse_ctn(tag, c_tn);
        node.bhvr_mut().target = parse_target(media.first_child("p:tgtEl"));
        let mut info = MediaNodeOpts::default();
        if let Some(v) = media.attr("vol") {
            info.vol = Some(parse::parse_int(v, 10) / 100_000.0);
        }
        if let Some(v) = media.attr("mute") {
            info.mute = Some(flag(Some(v)));
        }
        if let Some(v) = media.attr("showWhenStopped") {
            info.show_when_stopped = Some(!matches!(v, "0" | "false"));
        }
        if let Some(v) = media.attr("numSld") {
            info.num_sld = Some(parse::parse_int(v, 10));
        }
        if tag == "video"
            && let Some(v) = el.attr("fullScrn")
        {
            info.full_scrn = Some(flag(Some(v)));
        }
        node.bhvr_x_mut().media = Some(info);
        return Some(node);
    }
    None
}

struct Frame<'d> {
    id: NodeId,
    kids_src: Vec<Element<'d>>,
    next: usize,
    kids: Vec<NodeId>,
}

/// One element: a container (needs a frame), a leaf (done) or nothing.
enum Parsed<'d> {
    Container(NodeId, Vec<Element<'d>>),
    Leaf(NodeId),
    Nothing,
}

fn is_container(tag: &str) -> bool {
    tag == "par" || tag == "seq" || tag == "excl"
}

fn push_node(arena: &mut TimeArena, n: TimeNode, max_nodes: usize) -> Option<NodeId> {
    if arena.nodes.len() >= max_nodes {
        return None;
    }
    let id = NodeId(u32::try_from(arena.nodes.len()).ok()?);
    arena.nodes.push(n);
    Some(id)
}

fn open_node<'d>(arena: &mut TimeArena, el: Element<'d>, color: Option<ResolveColor<'_>>, max_nodes: usize) -> Parsed<'d> {
    let tag = local_tag(el);
    if is_container(tag) {
        let Some(c_tn) = el.first_child("p:cTn") else { return Parsed::Nothing };
        let node = parse_ctn(tag, c_tn);
        let kids_src: Vec<Element<'d>> = c_tn.first_child("p:childTnLst").map(|l| l.child_elements().collect()).unwrap_or_default();
        return match push_node(arena, node, max_nodes) {
            Some(id) => Parsed::Container(id, kids_src),
            None => Parsed::Nothing,
        };
    }
    match parse_leaf(el, tag, color).and_then(|n| push_node(arena, n, max_nodes)) {
        Some(id) => Parsed::Leaf(id),
        None => Parsed::Nothing,
    }
}

/// `parseTimeNode` (`timing.ts:420`) over an explicit stack: `el` and its subtree into `arena`.
/// Returns the root's id, or `None` when `el` is not a time node (or is over the limits).
pub fn parse_time_tree(arena: &mut TimeArena, el: Element<'_>, color: Option<ResolveColor<'_>>, limits: &Limits) -> Option<NodeId> {
    let max_depth = limits.max_time_depth as usize;
    let max_nodes = limits.max_time_nodes as usize;
    let root = match open_node(arena, el, color, max_nodes) {
        Parsed::Container(id, kids_src) => Frame { id, kids_src, next: 0, kids: Vec::new() },
        Parsed::Leaf(id) => return Some(id),
        Parsed::Nothing => return None,
    };
    let root_id = root.id;
    let mut stack = vec![root];
    while let Some(top) = stack.last_mut() {
        if top.next < top.kids_src.len() {
            let child = top.kids_src[top.next];
            top.next += 1;
            // the root frame is depth 1: a container child would be depth `stack.len() + 1`
            if stack.len() >= max_depth && is_container(local_tag(child)) {
                continue;
            }
            match open_node(arena, child, color, max_nodes) {
                Parsed::Container(id, kids_src) => stack.push(Frame { id, kids_src, next: 0, kids: Vec::new() }),
                Parsed::Leaf(id) => {
                    if let Some(t) = stack.last_mut() {
                        t.kids.push(id);
                    }
                }
                Parsed::Nothing => {}
            }
        } else if let Some(done) = stack.pop() {
            let start = u32::try_from(arena.children.len()).ok()?;
            let len = u32::try_from(done.kids.len()).ok()?;
            arena.children.extend_from_slice(&done.kids);
            if let Some(n) = arena.nodes.get_mut(done.id.0 as usize) {
                n.children = Span::new(start, len);
            }
            if let Some(parent) = stack.last_mut() {
                parent.kids.push(done.id);
            }
        }
    }
    Some(root_id)
}

/// `parseSlideTiming` (`timing.ts:741`).
pub fn parse_slide_timing(sld: Element<'_>, inp: &TimingInputs<'_>, limits: &Limits) -> TimingModel {
    let show = sld.attr("show");
    let mut model = TimingModel { hidden: show == Some("0") || show == Some("false"), ..TimingModel::default() };
    model.transition = parse_slide_transition(sld);
    let timing = unwrap_alternate(sld, "p:timing");
    let tn_lst = timing.and_then(|t| t.first_child("p:tnLst"));
    let root_el = tn_lst.and_then(|l| l.child_elements().next());
    let mut arena = TimeArena::default();
    let root = root_el.and_then(|e| parse_time_tree(&mut arena, e, inp.color, limits));
    let Some(root) = root else {
        model.nodes = arena;
        return model;
    };
    // shapes that animate the BACKGROUND with the text (bldLst)
    let mut bg_set: HashSet<String> = HashSet::new();
    if let Some(bld) = timing.and_then(|t| t.first_child("p:bldLst")) {
        for b in bld.children_named("p:bldP") {
            if flag(b.attr("animBg")) {
                // `${spid}|${grpId ?? '0'}` (an absent spid prints as "undefined")
                bg_set.insert(format!("{}|{}", b.attr("spid").unwrap_or("undefined"), b.attr("grpId").unwrap_or("0")));
            }
        }
    }
    let anim_bg = |spid: &str, grp: Option<&str>| -> bool {
        match inp.has_background {
            Some(h) => h(spid),
            None => bg_set.contains(&format!("{}|{}", spid, grp.unwrap_or("0"))),
        }
    };
    let ctx = EvalCtx { units: inp.units, anim_bg: &anim_bg };

    // shapes whose background becomes a unit: whole targets of per-letter/per-word effects
    let mut bg_ids: Vec<Box<str>> = Vec::new();
    // media nodes (outside the mainSeq) and the sequences, in pre-order
    let mut media: Vec<SlideMediaInfo> = Vec::new();
    let mut seqs: Vec<NodeId> = Vec::new();
    {
        // pre-order walk: iter/media over the whole tree, the sequences stop at the first `seq` of a branch
        let mut stack: Vec<(NodeId, bool)> = vec![(root, true)];
        // `findIter` and `findMedia` visit every node; `findSeqs` does not enter a `seq`. Three separate
        // pre-order walks keep the TS orders of their results.
        let mut order: Vec<NodeId> = Vec::new();
        while let Some((id, _)) = stack.pop() {
            order.push(id);
            let n = arena.node(id);
            for c in arena.kids(n).iter().rev() {
                stack.push((*c, true));
            }
        }
        for id in &order {
            let n = arena.node(*id);
            if let Some(it) = &n.ctn_x().iterate
                && (&*it.kind == "wd" || &*it.kind == "lt")
                && let Some(t) = first_target(&arena, *id)
                && whole_shape(t)
                && (ctx.anim_bg)(&t.spid, n.grp_id.as_deref())
                && !bg_ids.iter().any(|b| **b == *t.spid)
            {
                bg_ids.push(t.spid.clone());
            }
        }
        for id in &order {
            let n = arena.node(*id);
            if (&*n.tag == "video" || &*n.tag == "audio")
                && let Some(t) = &n.bhvr().target
            {
                media.push(SlideMediaInfo {
                    spid: t.spid.clone(),
                    kind: if &*n.tag == "video" { MediaKind::Video } else { MediaKind::Audio },
                    vol: n.bhvr_x().media.as_ref().and_then(|m| m.vol),
                    mute: n.bhvr_x().media.as_ref().and_then(|m| m.mute),
                    show_when_stopped: n.bhvr_x().media.as_ref().and_then(|m| m.show_when_stopped),
                    rewind: (n.fill.as_deref() == Some("remove")).then_some(true),
                    loop_: (n.ctn_x().repeat == Some(f64::INFINITY)).then_some(true),
                    full_screen: n.bhvr_x().media.as_ref().is_some_and(|m| m.full_scrn == Some(true)).then_some(true),
                });
            }
        }
        // findSeqs: do not descend into a seq
        let mut st: Vec<NodeId> = vec![root];
        while let Some(id) = st.pop() {
            let n = arena.node(id);
            if &*n.tag == "seq" {
                seqs.push(id);
                continue;
            }
            for c in arena.kids(n).iter().rev() {
                st.push(*c);
            }
        }
    }
    if !bg_ids.is_empty() {
        model.anim_bg = Some(bg_ids);
    }
    if !media.is_empty() {
        model.media = Some(media);
    }

    let mut steps: Vec<AnimStep> = Vec::new();
    let mut interactive: Vec<EffectTiming> = Vec::new();
    let mut triggers: Vec<BookmarkTrigger> = Vec::new();
    for seq in seqs {
        let sn = arena.node(seq);
        match sn.node_type.as_deref() {
            Some("mainSeq") => {
                // steps: [1st automatic (onBegin)] + one step per clickable child
                let mut cur: Vec<(NodeId, f64)> = Vec::new();
                let mut cur_auto = true;
                for (i, c) in arena.kids(sn).iter().enumerate() {
                    let cn = arena.node(*c);
                    // automatic only the 1st step with onBegin; the rest is click (the application ignores
                    // a numeric delay on a mainSeq child — measured on the showcase)
                    let auto = i == 0 && cn.st.iter().any(|k| k.evt.as_deref() == Some("onBegin"));
                    if !auto {
                        if !cur.is_empty() {
                            steps.push(step_from(&arena, &cur, cur_auto, &ctx));
                        }
                        cur.clear();
                        cur_auto = false;
                    }
                    cur.push((*c, if auto { begin_offset(cn).unwrap_or(0.0) } else { 0.0 }));
                    if !auto {
                        steps.push(step_from(&arena, &cur, cur_auto, &ctx));
                        cur.clear();
                    }
                }
                if !cur.is_empty() {
                    steps.push(step_from(&arena, &cur, cur_auto, &ctx));
                }
            }
            Some("interactiveSeq") => {
                let bmk = sn.st.iter().find(|c| c.evt.as_deref() == Some("onMediaBookmark") && c.spid.is_some() && c.bookmark.is_some());
                if let Some(b) = bmk {
                    // media bookmark: fires by itself when the media passes the point — an interactive
                    // sequence advances ONE click group per event: only the 1st plays on the pass; the
                    // others stay pending (the initial state holds)
                    let mut effects: Vec<EffectTiming> = Vec::new();
                    for (i, c) in arena.kids(sn).iter().enumerate() {
                        collect_group(&arena, *c, 0.0, &ctx, if i == 0 { &mut effects } else { &mut interactive });
                    }
                    triggers.push(BookmarkTrigger {
                        spid: b.spid.clone().unwrap_or_default(),
                        bookmark: b.bookmark.clone().unwrap_or_default(),
                        time_ms: None,
                        trim_start_ms: None,
                        effects,
                    });
                } else {
                    for c in arena.kids(sn) {
                        collect_group(&arena, *c, 0.0, &ctx, &mut interactive);
                    }
                }
            }
            _ => {}
        }
    }
    model.steps = steps;
    model.interactive = interactive;
    if !triggers.is_empty() {
        model.bookmark_triggers = Some(triggers);
    }
    model.nodes = arena;
    model
}
