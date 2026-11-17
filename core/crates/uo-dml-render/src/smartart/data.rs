//! The data model of a SmartArt diagram (`diagram-layout.ts:81–327`): points and connections read from
//! `dgm:dataModel`, the tree of points, and the axis/`forEach`/`if` iteration over it.

use rustc_hash::FxHashMap;
use uo_jsnum::parse::{parse_float, parse_int};
use uo_jsnum::text::{is_js_space, trim};

use super::xt::X;

pub const MAX_PRES: usize = 3000;
pub const MAX_DEPTH: usize = 40;
pub const MAX_ITER: usize = 20000;
/// Points of a data model past which nothing is regenerated (the TS has no bound; declared decision B5a-4).
pub const MAX_MODEL_PTS: usize = 200_000;

pub type PtId = usize;

/// `Pt` (`diagram-layout.ts:81`).
#[derive(Debug)]
pub struct Pt<'a> {
    pub id: String,
    /// doc | node | asst | parTrans | sibTrans | pres
    pub ty: String,
    pub pr_set: Option<&'a X>,
    pub sp_pr: Option<&'a X>,
    pub t: Option<&'a X>,
    pub par: Option<PtId>,
    /// children in model order: `[parTrans, node, sibTrans]` per `parOf` connection
    pub seq: Vec<PtId>,
    pub depth: usize,
    pub src: Option<PtId>,
    pub dst: Option<PtId>,
    pub last: bool,
}

impl Pt<'_> {
    pub fn is_node(&self) -> bool {
        self.ty == "node" || self.ty == "asst"
    }
}

/// `Model`.
pub struct Model {
    pub root: PtId,
    /// `pres` points by `presAssocID \0 presName`
    pub pres: FxHashMap<String, Vec<PtId>>,
}

/// An attribute source: the attributes of an element, or a synthesized record (`presTexts`).
pub trait Ag {
    fn ag(&self, k: &str) -> Option<&str>;
}

impl Ag for X {
    fn ag(&self, k: &str) -> Option<&str> {
        self.attr(k)
    }
}

/// A synthesized attribute record (`Attrs`).
#[derive(Default, Debug, Clone)]
pub struct Attrs(pub Vec<(String, String)>);

impl Attrs {
    pub fn set(&mut self, k: &str, v: &str) {
        match self.0.iter_mut().find(|(n, _)| n == k) {
            Some(e) => e.1 = v.to_string(),
            None => self.0.push((k.to_string(), v.to_string())),
        }
    }
}

impl Ag for Attrs {
    fn ag(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

/// `readModel` (`diagram-layout.ts:103`). The points go into `pts`; `None` when there is no `dgm:ptLst`
/// or no `doc` point.
pub fn read_model<'a>(data: &'a X, pts: &mut Vec<Pt<'a>>) -> Option<Model> {
    let pt_lst = data.first("dgm:ptLst")?;
    let mut by_id: FxHashMap<&'a str, PtId> = FxHashMap::default();
    let mut pres: FxHashMap<String, Vec<PtId>> = FxHashMap::default();
    let mut root: Option<PtId> = None;
    for e in pt_lst.kids_named("dgm:pt") {
        let Some(id) = e.attr("modelId") else { continue };
        if by_id.contains_key(id) {
            continue;
        }
        if pts.len() >= MAX_MODEL_PTS {
            return None;
        }
        let sp = e.first("dgm:spPr");
        let p = Pt {
            id: id.to_string(),
            ty: e.attr("type").unwrap_or("node").to_string(),
            pr_set: e.first("dgm:prSet"),
            sp_pr: sp.filter(|s| s.kids().next().is_some()),
            t: e.first("dgm:t"),
            par: None,
            seq: Vec::new(),
            depth: 0,
            src: None,
            dst: None,
            last: false,
        };
        let pid = pts.len();
        let is_doc = p.ty == "doc";
        let is_pres = p.ty == "pres";
        let pr_set = p.pr_set;
        pts.push(p);
        by_id.insert(id, pid);
        if is_doc && root.is_none() {
            root = Some(pid);
        }
        if is_pres && let Some(ps) = pr_set {
            let k = format!("{}\u{0}{}", ps.attr("presAssocID").unwrap_or(""), ps.attr("presName").unwrap_or(""));
            pres.entry(k).or_default().push(pid);
        }
    }
    let root = root?;
    // connections of type parOf, by source
    struct Cx<'x> {
        ord: f64,
        i: usize,
        e: &'x X,
    }
    let mut by_src: FxHashMap<&str, Vec<Cx<'_>>> = FxHashMap::default();
    let mut i = 0usize;
    if let Some(cxn) = data.first("dgm:cxnLst") {
        for c in cxn.kids_named("dgm:cxn") {
            if c.attr("type").unwrap_or("parOf") != "parOf" {
                continue;
            }
            let s = c.attr("srcId").unwrap_or("");
            let ord0 = parse_int(c.attr("srcOrd").unwrap_or("0"), 10);
            // `parseInt(...) || 0`
            let ord = if ord0 == 0.0 || ord0.is_nan() { 0.0 } else { ord0 };
            by_src.entry(s).or_default().push(Cx { ord, i, e: c });
            i += 1;
        }
    }
    // tree from the root (visited: cyclic/duplicate connections are ignored)
    let mut seen: rustc_hash::FxHashSet<PtId> = rustc_hash::FxHashSet::default();
    seen.insert(root);
    let mut stack: Vec<PtId> = vec![root];
    let mut guard = 0usize;
    while !stack.is_empty() && {
        let g = guard;
        guard += 1;
        g < MAX_ITER
    } {
        let Some(p) = stack.pop() else { break };
        let pid_key = pts.get(p).map(|q| q.id.clone()).unwrap_or_default();
        let mut cx: Vec<&Cx<'_>> = by_src.get(pid_key.as_str()).map(|v| v.iter().collect()).unwrap_or_default();
        cx.sort_by(|a, b| a.ord.partial_cmp(&b.ord).unwrap_or(std::cmp::Ordering::Equal).then(a.i.cmp(&b.i)));
        let mut nodes: Vec<PtId> = Vec::new();
        let p_depth = pts.get(p).map_or(0, |q| q.depth);
        for c in cx {
            let e = c.e;
            let Some(&d) = by_id.get(e.attr("destId").unwrap_or("")) else { continue };
            let (dty, ddid) = match pts.get(d) {
                Some(q) => (q.ty.as_str(), d),
                None => continue,
            };
            if seen.contains(&ddid) || dty == "pres" || dty == "doc" {
                continue;
            }
            seen.insert(d);
            if let Some(q) = pts.get_mut(d) {
                q.par = Some(p);
                q.depth = p_depth + 1;
            }
            let pt = by_id.get(e.attr("parTransId").unwrap_or("")).copied();
            let st = by_id.get(e.attr("sibTransId").unwrap_or("")).copied();
            if let Some(ptid) = pt
                && pts.get(ptid).is_some_and(|q| q.ty == "parTrans")
            {
                if let Some(q) = pts.get_mut(ptid) {
                    q.par = Some(p);
                    q.depth = p_depth + 1;
                    q.src = Some(p);
                    q.dst = Some(d);
                }
                if let Some(pp) = pts.get_mut(p) {
                    pp.seq.push(ptid);
                }
            }
            if let Some(pp) = pts.get_mut(p) {
                pp.seq.push(d);
            }
            if let Some(stid) = st
                && pts.get(stid).is_some_and(|q| q.ty == "sibTrans")
            {
                if let Some(q) = pts.get_mut(stid) {
                    q.par = Some(p);
                    q.depth = p_depth + 1;
                    q.src = Some(d);
                }
                if let Some(pp) = pts.get_mut(p) {
                    pp.seq.push(stid);
                }
            }
            nodes.push(d);
        }
        // destination of the sibling transition: the next sibling (the last one links to the first)
        let mut pos: FxHashMap<PtId, usize> = FxHashMap::default();
        for (k, &nd) in nodes.iter().enumerate() {
            pos.entry(nd).or_insert(k);
        }
        let seq_len = pts.get(p).map_or(0, |q| q.seq.len());
        for k in 0..seq_len {
            let Some(s) = pts.get(p).and_then(|q| q.seq.get(k)).copied() else { continue };
            let Some((is_sib, src)) = pts.get(s).map(|q| (q.ty == "sibTrans", q.src)) else { continue };
            if !is_sib {
                continue;
            }
            let Some(src) = src else { continue };
            // `nodes.indexOf(s.src)`
            let at: Option<usize> = pos.get(&src).copied();
            let next = match at {
                Some(a) => nodes.get(a + 1).or_else(|| nodes.first()).copied(),
                None => nodes.first().copied(),
            };
            let is_last = match at {
                Some(a) => a + 1 == nodes.len(),
                None => false,
            };
            if let Some(q) = pts.get_mut(s) {
                q.dst = next;
                if is_last {
                    q.last = true;
                }
            }
        }
        for k in (0..nodes.len()).rev() {
            if let Some(&nd) = nodes.get(k) {
                stack.push(nd);
            }
        }
    }
    Some(Model { root, pres })
}

/// `ptMatch` (`diagram-layout.ts:171`).
pub fn pt_match(p: &Pt<'_>, t: Option<&str>) -> bool {
    match t.unwrap_or("all") {
        "all" => true,
        "node" => p.ty == "node" || p.ty == "asst",
        "norm" | "nonAsst" => p.ty == "node",
        "nonNorm" | "asst" => p.ty == "asst",
        "doc" => p.ty == "doc",
        "parTrans" => p.ty == "parTrans",
        "sibTrans" => p.ty == "sibTrans",
        "pres" => p.ty == "pres",
        _ => false,
    }
}

/// `descendants` (`diagram-layout.ts:189`).
pub fn descendants(pts: &[Pt<'_>], p: PtId, out: &mut Vec<PtId>, depth: usize) {
    if depth > MAX_DEPTH || out.len() > MAX_ITER {
        return;
    }
    let Some(seq) = pts.get(p).map(|q| q.seq.clone()) else { return };
    for c in seq {
        out.push(c);
        if pts.get(c).is_some_and(Pt::is_node) {
            descendants(pts, c, out, depth + 1);
        }
    }
}

fn desc_of(pts: &[Pt<'_>], p: PtId) -> Vec<PtId> {
    let mut out = Vec::new();
    descendants(pts, p, &mut out, 0);
    out
}

/// `axisPts` (`diagram-layout.ts:195`).
pub fn axis_pts(pts: &[Pt<'_>], p: PtId, axis: &str, root: PtId) -> Vec<PtId> {
    let Some(me) = pts.get(p) else { return Vec::new() };
    let siblings = |q: &Pt<'_>| -> Vec<PtId> { q.par.and_then(|pp| pts.get(pp)).map(|pp| pp.seq.clone()).unwrap_or_default() };
    match axis {
        "self" => vec![p],
        "ch" => me.seq.clone(),
        "des" => desc_of(pts, p),
        "desOrSelf" => {
            let mut o = vec![p];
            o.extend(desc_of(pts, p));
            o
        }
        "par" => me.par.into_iter().collect(),
        "ancst" => {
            let mut o = Vec::new();
            let mut q = me.par;
            while let Some(x) = q {
                if o.len() >= MAX_DEPTH {
                    break;
                }
                o.push(x);
                q = pts.get(x).and_then(|y| y.par);
            }
            o
        }
        "ancstOrSelf" => {
            let mut o = vec![p];
            let mut q = me.par;
            while let Some(x) = q {
                if o.len() >= MAX_DEPTH {
                    break;
                }
                o.push(x);
                q = pts.get(x).and_then(|y| y.par);
            }
            o
        }
        "followSib" => {
            let s = siblings(me);
            match s.iter().position(|&x| x == p) {
                Some(i) => s.get(i + 1..).unwrap_or(&[]).to_vec(),
                None => Vec::new(),
            }
        }
        "precedSib" => {
            let s = siblings(me);
            match s.iter().position(|&x| x == p) {
                Some(i) => s.get(..i).unwrap_or(&[]).to_vec(),
                None => Vec::new(),
            }
        }
        "follow" => {
            let s = siblings(me);
            match s.iter().position(|&x| x == p) {
                Some(i) => s
                    .get(i + 1..)
                    .unwrap_or(&[])
                    .iter()
                    .flat_map(|&q| {
                        let mut v = vec![q];
                        v.extend(desc_of(pts, q));
                        v
                    })
                    .collect(),
                None => Vec::new(),
            }
        }
        "preced" => {
            let s = siblings(me);
            match s.iter().position(|&x| x == p) {
                Some(i) => s
                    .get(..i)
                    .unwrap_or(&[])
                    .iter()
                    .flat_map(|&q| {
                        let mut v = vec![q];
                        v.extend(desc_of(pts, q));
                        v
                    })
                    .collect(),
                None => Vec::new(),
            }
        }
        "root" => vec![root],
        _ => Vec::new(),
    }
}

/// `listAttr` (`diagram-layout.ts:213`): the attribute split on white space.
pub fn list_attr(a: &dyn Ag, k: &str) -> Vec<String> {
    trim(a.ag(k).unwrap_or("")).split(is_js_space).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// A JS number as an index into a list of `len` entries (`0 ≤ i < len`).
pub fn idx(i: f64, len: usize) -> Option<usize> {
    uo_jsnum::conv::js_index(i, len)
}

/// `iterate` (`diagram-layout.ts:216`): the points of an iteration (`forEach`/`presOf`/`if`): chained axes
/// with a filter, start, count and step.
pub fn iterate(pts: &[Pt<'_>], p: PtId, a: &dyn Ag, root: PtId, def_axis: &str) -> Vec<PtId> {
    let mut axes = list_attr(a, "axis");
    if axes.is_empty() {
        axes.push(def_axis.to_string());
    }
    let types = list_attr(a, "ptType");
    let sts = list_attr(a, "st");
    let cnts = list_attr(a, "cnt");
    let steps = list_attr(a, "step");
    let hlts = list_attr(a, "hideLastTrans");
    let mut cur: Vec<PtId> = vec![p];
    for (k, axis) in axes.iter().enumerate() {
        let mut next: Vec<PtId> = Vec::new();
        let ty: &str = types.get(k).or_else(|| types.last()).map_or("all", String::as_str);
        let hl = hlts.get(k).or_else(|| hlts.last());
        let hide = hl.map_or("true", String::as_str) != "0" && hl.map(String::as_str) != Some("false");
        let nz = |v: f64, d: f64| if v == 0.0 || v.is_nan() { d } else { v };
        let st = nz(parse_int(sts.get(k).map_or("1", String::as_str), 10), 1.0);
        let cnt = nz(parse_int(cnts.get(k).map_or("0", String::as_str), 10), 0.0);
        let mut step = nz(parse_int(steps.get(k).map_or("1", String::as_str), 10), 1.0);
        for &q in &cur {
            let mut l: Vec<PtId> =
                axis_pts(pts, q, axis, root).into_iter().filter(|&x| pts.get(x).is_some_and(|px| pt_match(px, Some(ty)))).collect();
            if hide {
                l.retain(|&x| !pts.get(x).is_some_and(|px| px.ty == "sibTrans" && px.last));
            }
            let len_f = f64::from(u32::try_from(l.len()).unwrap_or(u32::MAX));
            let start = if st < 0.0 { len_f + st } else { st - 1.0 };
            if step == 0.0 {
                step = 1.0;
            }
            let mut taken = 0.0_f64;
            let mut i = start;
            while i >= 0.0 && i < len_f {
                if let Some(ix) = idx(i, l.len())
                    && let Some(&v) = l.get(ix)
                {
                    next.push(v);
                }
                if cnt != 0.0 {
                    taken += 1.0;
                    if taken >= cnt {
                        break;
                    }
                }
                if next.len() > MAX_ITER {
                    break;
                }
                i += step;
            }
        }
        cur = next;
    }
    cur
}

/// `parseNum` (`diagram-layout.ts:329`).
pub fn parse_num(s: Option<&str>, d: f64) -> f64 {
    let Some(s) = s else { return d };
    if s == "INF" {
        return f64::INFINITY;
    }
    if s == "NaN" {
        return f64::NAN;
    }
    let v = parse_float(s);
    if v.is_finite() { v } else { d }
}
