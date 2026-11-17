//! `diagram-fallback.ts`: a SmartArt WITHOUT the pre-computed drawing and without a regenerable layout
//! definition: rebuilds the diagram from the DATA model (`dgm:ptLst` + `dgm:cxnLst`) with an
//! approximate layout for the application's most common layouts — organization chart, process (simple
//! and curved), lists (basic block, grouped list, vertical blocks), cycle, chevron and matrix. A layout
//! outside the list stays empty. Boxes in the theme's accent1, light text, UNIFORM font size that fits
//! all of them.

use rustc_hash::FxHashMap;
use uo_jsnum::math;
use uo_jsnum::parse::parse_int;
use uo_jsnum::text::{split_js_space, trim, utf16_len};

use super::styler::{DgmStyle, StyleThrow, Styler};
use super::xt::X;
use crate::color::CssColor;
use crate::model::{Fill, FillKind, Paragraph, Run, Shape, TextBody, Xfrm};
use crate::read_dml::solid_fill;
use crate::source::parse_xml;
use crate::theme::{Theme, color_from_dml};

const EMU_PT: f64 = 12700.0;
/// Calls of the recursive org-chart layout past which the diagram is refused (a cyclic or shared
/// graph would recurse for ever or exponentially; the TS ends in a `RangeError` caught by `read.ts`).
const MAX_WORK: usize = 2_000_000;
const MAX_REC: usize = 1500;

struct Pres {
    lbl: String,
    idx: f64,
    cnt: f64,
    sx: f64,
    sp_pr: Option<usize>,
}

struct Node {
    text: Vec<String>,
    children: Vec<usize>,
    pres: Vec<Pres>,
    sp_pr: Option<usize>,
    hier_branch: Option<String>,
}

/// A run recorded with `custT="1"`.
#[derive(Clone)]
struct RunFmt {
    text: String,
    sz: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    font: Option<String>,
}

/// `descendFirst`.
fn descend_first<'a>(e: &'a X, name: &str) -> Option<&'a X> {
    for c in e.kids() {
        if c.name == name {
            return Some(c);
        }
        if let Some(d) = descend_first(c, name) {
            return Some(d);
        }
    }
    None
}

/// `ownSpPr`: `dgm:spPr` with content (manual formatting of the point).
fn own_sp_pr(pt: &X) -> Option<&X> {
    pt.first("dgm:spPr").filter(|e| e.kids().next().is_some())
}

struct Tree<'a> {
    nodes: Vec<Node>,
    root: usize,
    /// the `spPr` elements the nodes/pres refer to by index
    sp_prs: Vec<&'a X>,
}

fn main_pres(n: &Node) -> Option<&Pres> {
    n.pres.iter().find(|p| p.cnt > 0.0 && (p.lbl.starts_with("node") || p.lbl.starts_with("asst"))).or_else(|| {
        n.pres
            .iter()
            .find(|p| p.cnt > 0.0 && !p.lbl.is_empty() && !(p.lbl.contains("Trans") || p.lbl.contains("Acc") || p.lbl.contains("Follow")))
    })
}

fn sub_pres(n: &Node) -> Option<&Pres> {
    n.pres.iter().find(|p| p.cnt > 0.0 && (p.lbl.contains("Acc") || p.lbl.contains("Follow")))
}

/// `buildTree`: the tree of nodes of the data model (root = pt type="doc").
fn build_tree<'a>(data: &'a X, fmt: &mut FxHashMap<String, Vec<RunFmt>>) -> Option<Tree<'a>> {
    let pt_lst = data.first("dgm:ptLst")?;
    let cxn_lst = data.first("dgm:cxnLst");
    let mut nodes: Vec<Node> = Vec::new();
    let mut by_id: FxHashMap<String, usize> = FxHashMap::default();
    let mut root: Option<usize> = None;
    let mut sp_prs: Vec<&'a X> = Vec::new();
    let add_sp = |e: Option<&'a X>, sp_prs: &mut Vec<&'a X>| -> Option<usize> {
        e.map(|e| {
            sp_prs.push(e);
            sp_prs.len() - 1
        })
    };
    for pt in pt_lst.kids_named("dgm:pt") {
        let ty = pt.attr("type").unwrap_or("node");
        if ty != "node" && ty != "doc" && ty != "asst" {
            continue;
        }
        let t = pt.first("dgm:t");
        let text: Vec<String> = match t {
            Some(t) => t
                .kids_named("a:p")
                .map(|p| {
                    p.kids()
                        .filter(|r| r.name == "a:r" || r.name == "a:fld")
                        .map(|r| r.first("a:t").map(X::text).unwrap_or_default())
                        .collect::<String>()
                })
                .filter(|s| !trim(s).is_empty())
                .collect(),
            None => Vec::new(),
        };
        let sp = add_sp(own_sp_pr(pt), &mut sp_prs);
        let id = pt.attr("modelId").unwrap_or("").to_string();
        let n = Node { text, children: Vec::new(), pres: Vec::new(), sp_pr: sp, hier_branch: None };
        // custom text (custT): keeps the runs with the size/bold/italic of each line
        if let Some(t) = t
            && pt.first("dgm:prSet").and_then(|s| s.attr("custT")) == Some("1")
        {
            for p in t.kids_named("a:p") {
                let mut runs: Vec<RunFmt> = Vec::new();
                for r in p.kids() {
                    if r.name != "a:r" && r.name != "a:fld" {
                        continue;
                    }
                    let tt = r.first("a:t");
                    let rp = r.first("a:rPr");
                    let mut run = RunFmt {
                        text: tt.map(X::text).unwrap_or_default(),
                        sz: None,
                        bold: None,
                        italic: None,
                        underline: None,
                        font: None,
                    };
                    let sz = parse_int(rp.and_then(|x| x.attr("sz")).unwrap_or(""), 10);
                    if sz > 0.0 {
                        run.sz = Some(sz / 100.0);
                    }
                    if rp.and_then(|x| x.attr("b")) == Some("1") {
                        run.bold = Some(true);
                    }
                    if rp.and_then(|x| x.attr("i")) == Some("1") {
                        run.italic = Some(true);
                    }
                    if let Some(u) = rp.and_then(|x| x.attr("u"))
                        && !u.is_empty()
                        && u != "none"
                    {
                        run.underline = Some(true);
                    }
                    let face = rp.and_then(|x| x.first("a:latin")).and_then(|l| l.attr("typeface")).filter(|f| !f.is_empty());
                    if let Some(f) = face {
                        run.font = Some(f.to_string());
                    }
                    runs.push(run);
                }
                let line: String = runs.iter().map(|r| r.text.as_str()).collect();
                if !trim(&line).is_empty() && runs.iter().any(|r| r.sz.is_some_and(|s| s != 0.0)) {
                    fmt.insert(line, runs);
                }
            }
        }
        let ix = nodes.len();
        nodes.push(n);
        by_id.insert(id, ix);
        if ty == "doc" {
            root = Some(ix);
        }
    }
    for pt in pt_lst.kids_named("dgm:pt") {
        if pt.attr("type") != Some("pres") {
            continue;
        }
        let Some(pr) = pt.first("dgm:prSet") else { continue };
        let Some(&nix) = by_id.get(pr.attr("presAssocID").unwrap_or("")) else { continue };
        let hb = descend_first(pr, "dgm:hierBranch").and_then(|e| e.attr("val")).filter(|v| !v.is_empty());
        if let Some(hb) = hb
            && nodes[nix].hier_branch.as_deref().is_none_or(str::is_empty)
        {
            nodes[nix].hier_branch = Some(hb.to_string());
        }
        let num = |k: &str, d: f64| {
            let v = parse_int(pr.attr(k).unwrap_or(""), 10);
            if v.is_finite() { v } else { d }
        };
        let sp = add_sp(own_sp_pr(pt), &mut sp_prs);
        nodes[nix].pres.push(Pres {
            lbl: pr.attr("presStyleLbl").unwrap_or("").to_string(),
            idx: num("presStyleIdx", 0.0),
            cnt: num("presStyleCnt", 0.0),
            sx: num("custScaleX", 100000.0) / 100000.0,
            sp_pr: sp,
        });
    }
    let root = root?;
    let mut cx: Vec<&X> = match cxn_lst {
        Some(c) => c.kids_named("dgm:cxn").filter(|c| c.attr("type").unwrap_or("parOf") == "parOf").collect(),
        None => Vec::new(),
    };
    uo_jsnum::sort::js_sort_by(&mut cx, |a, b| {
        parse_int(a.attr("srcOrd").unwrap_or("0"), 10) - parse_int(b.attr("srcOrd").unwrap_or("0"), 10)
    });
    for c in cx {
        let p = by_id.get(c.attr("srcId").unwrap_or("")).copied();
        let ch = by_id.get(c.attr("destId").unwrap_or("")).copied();
        if let (Some(p), Some(ch)) = (p, ch)
            && ch != root
        {
            nodes[p].children.push(ch);
        }
    }
    Some(Tree { nodes, root, sp_prs })
}

/// `familyOf`: covered layouts (final name of the `uniqueId`); the rest gives an empty list.
fn family_of(unique_id: &str) -> Option<&'static str> {
    let last = unique_id.rsplit('/').next().unwrap_or("");
    // `.replace(/#\d+$/, '')`
    let k = match last.rfind('#') {
        Some(i) if i + 1 < last.len() && last.get(i + 1..).is_some_and(|t| t.bytes().all(|b| b.is_ascii_digit())) => {
            last.get(..i).unwrap_or(last)
        }
        _ => last,
    };
    Some(match k {
        "orgChart1" => "org",
        "process5" => "bend",
        "process1" => "process",
        "list1" => "list1",
        "chevron1" => "chevron",
        "vList3" => "circleList",
        "hList1" | "hierarchy4" => "row",
        "vList5" | "vProcess5" => "column",
        "cycle2" => "cycle",
        "default" => "block",
        "matrix1" => "grid",
        _ => return None,
    })
}

/// `fitPt`: the size (pt) that makes `lines` fit the box (estimate ~0.55 em per character).
fn fit_pt(lines: &[String], w: f64, h: f64, max: f64) -> f64 {
    if lines.is_empty() {
        return max;
    }
    let w_pt = w / EMU_PT * 0.92;
    let h_pt = h / EMU_PT * 0.95;
    let mut sz = max;
    while sz >= 6.0 {
        let per_line = math::max(1.0, math::floor(w_pt / (sz * 0.5)));
        let mut rows = 0.0_f64;
        let mut ok = true;
        for l in lines {
            let words: Vec<&str> = split_js_space(l).collect();
            if words.iter().any(|wd| uf(utf16_len(wd)) > per_line) {
                ok = false;
                break;
            }
            let mut cur = 0.0_f64;
            rows += 1.0;
            for wd in words {
                let wl = uf(utf16_len(wd));
                if cur != 0.0 && cur + 1.0 + wl > per_line {
                    rows += 1.0;
                    cur = wl;
                } else {
                    cur += (if cur != 0.0 { 1.0 } else { 0.0 }) + wl;
                }
            }
        }
        if ok && rows * sz * 1.08 <= h_pt {
            return sz;
        }
        sz -= 1.0;
    }
    6.0
}

fn uf(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Clone, Debug, Default)]
struct SubBox {
    r: Rect,
    lines: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct BoxF {
    r: Rect,
    lines: Vec<String>,
    nd: Option<usize>,
    geom: Option<&'static str>,
    acc: bool,
    sub: Option<SubBox>,
}

struct Glyph {
    geom: &'static str,
    r: Rect,
}

/// `fitUnits`: the bounding box in units → the frame (uniform scale, centred).
struct FitUnits {
    x0: f64,
    y0: f64,
    s: f64,
    ox: f64,
    oy: f64,
}

impl FitUnits {
    fn new(items: &[Rect], frame: &Xfrm) -> FitUnits {
        let x0 = math::min_n(&items.iter().map(|r| r.x).collect::<Vec<_>>());
        let y0 = math::min_n(&items.iter().map(|r| r.y).collect::<Vec<_>>());
        let w = math::max_n(&items.iter().map(|r| r.x + r.w).collect::<Vec<_>>()) - x0;
        let h = math::max_n(&items.iter().map(|r| r.y + r.h).collect::<Vec<_>>()) - y0;
        let s = math::min(frame.cx / w, frame.cy / h);
        let ox = frame.x + (frame.cx - w * s) / 2.0;
        let oy = frame.y + (frame.cy - h * s) / 2.0;
        FitUnits { x0, y0, s, ox, oy }
    }
    fn map(&self, r: Rect) -> Rect {
        Rect { x: self.ox + (r.x - self.x0) * self.s, y: self.oy + (r.y - self.y0) * self.s, w: r.w * self.s, h: r.h * self.s }
    }
}

fn scheme(theme: &Theme, val: &str, modx: &str) -> CssColor {
    let txt = format!("<a:schemeClr xmlns:a=\"a\" val=\"{val}\">{modx}</a:schemeClr>");
    let c = parse_xml(txt).ok().and_then(|d| color_from_dml(Some(d.root()), theme));
    c.unwrap_or_else(|| CssColor::new("#4472C4"))
}

/// The org-chart layout (a nested set of closures in the TS).
struct Org<'t> {
    nodes: &'t [Node],
    boxes: Vec<BoxF>,
    lines: Vec<[f64; 4]>,
    work: usize,
}

const OW: f64 = 1.0;
const OH: f64 = 0.5;
const OHG: f64 = 0.15;
const OVG: f64 = 0.3;
const OHANG: f64 = 0.25;

impl Org<'_> {
    fn nw(&self, n: usize) -> f64 {
        OW * self.nodes.get(n).and_then(main_pres).map_or(1.0, |p| p.sx)
    }

    fn hangs(&self, n: usize, depth: usize) -> bool {
        let Some(nd) = self.nodes.get(n) else { return false };
        let hb = nd.hier_branch.as_deref().unwrap_or("");
        !nd.children.is_empty()
            && (matches!(hb, "r" | "l" | "hang")
                || (hb == "init" && depth >= 2 && nd.children.iter().all(|&c| self.nodes.get(c).is_none_or(|k| k.children.is_empty()))))
    }

    fn width(&mut self, n: usize, depth: usize) -> Option<f64> {
        self.work += 1;
        if self.work > MAX_WORK || depth > MAX_REC {
            return None;
        }
        let ch: Vec<usize> = self.nodes.get(n)?.children.clone();
        if ch.is_empty() {
            return Some(self.nw(n));
        }
        if self.hangs(n, depth) {
            let m = math::max_n(&ch.iter().map(|&c| self.nw(c)).collect::<Vec<_>>());
            return Some(math::max(self.nw(n), OHANG + m));
        }
        let mut sum = 0.0;
        for &c in &ch {
            sum += self.width(c, depth + 1)?;
        }
        Some(math::max(self.nw(n), sum + OHG * uf(ch.len() - 1)))
    }

    fn place(&mut self, n: usize, depth: usize, x0: f64, y: f64) -> Option<Rect> {
        self.work += 1;
        if self.work > MAX_WORK || depth > MAX_REC {
            return None;
        }
        let span = self.width(n, depth)?;
        let ch: Vec<usize> = self.nodes.get(n)?.children.clone();
        let text = self.nodes.get(n)?.text.clone();
        if self.hangs(n, depth) {
            let me = Rect { x: x0, y, w: self.nw(n), h: OH };
            self.boxes.push(BoxF { r: me, lines: text, nd: Some(n), ..BoxF::default() });
            let lx = x0 + OW * 0.06;
            let mut cy = y + OH + OVG;
            for &c in &ch {
                let ct = self.nodes.get(c)?.text.clone();
                let cw = self.nw(c);
                self.boxes.push(BoxF { r: Rect { x: x0 + OHANG, y: cy, w: cw, h: OH }, lines: ct, nd: Some(c), ..BoxF::default() });
                self.lines.push([lx, cy + OH / 2.0, x0 + OHANG, cy + OH / 2.0]);
                cy += OH + OVG;
            }
            self.lines.push([lx, y + OH, lx, cy - OVG - OH / 2.0]);
            return Some(me);
        }
        let mut widths: Vec<f64> = Vec::new();
        for &c in &ch {
            widths.push(self.width(c, depth + 1)?);
        }
        let total: f64 = widths.iter().sum::<f64>() + OHG * uf(ch.len().saturating_sub(1));
        let mut cx0 = x0 + (span - total) / 2.0;
        let mut kids: Vec<Rect> = Vec::new();
        for (i, &c) in ch.iter().enumerate() {
            kids.push(self.place(c, depth + 1, cx0, y + OH + OVG)?);
            cx0 += widths.get(i).copied().unwrap_or(0.0) + OHG;
        }
        let mid = match (kids.first(), kids.last()) {
            (Some(f), Some(l)) => (f.x + f.w / 2.0 + l.x + l.w / 2.0) / 2.0,
            _ => x0 + span / 2.0,
        };
        let nwn = self.nw(n);
        let me = Rect { x: mid - nwn / 2.0, y, w: nwn, h: OH };
        self.boxes.push(BoxF { r: me, lines: text, nd: Some(n), ..BoxF::default() });
        if !kids.is_empty() {
            let by = y + OH + OVG / 2.0;
            self.lines.push([mid, y + OH, mid, by]);
            let xs: Vec<f64> = kids.iter().map(|k| k.x + k.w / 2.0).collect();
            if kids.len() > 1 {
                self.lines.push([math::min_n(&xs), by, math::max_n(&xs), by]);
            }
            for k in &kids {
                self.lines.push([k.x + k.w / 2.0, by, k.x + k.w / 2.0, k.y]);
            }
        }
        Some(me)
    }
}

/// `diagramFallbackShapes(data, layoutUniqueId, frame, theme, styler)`. `Err` = the TS throws somewhere
/// (the caller catches it and keeps no shapes).
pub fn diagram_fallback_shapes(
    data: &X,
    layout_unique_id: &str,
    frame: &Xfrm,
    theme: &Theme,
    styler: Option<&Styler<'_>>,
) -> Result<Vec<Shape>, StyleThrow> {
    let Some(fam) = family_of(layout_unique_id) else { return Ok(Vec::new()) };
    let mut fmt: FxHashMap<String, Vec<RunFmt>> = FxHashMap::default();
    let Some(tree) = build_tree(data, &mut fmt) else { return Ok(Vec::new()) };
    let nodes = &tree.nodes;
    let Some(root) = nodes.get(tree.root) else { return Ok(Vec::new()) };
    if root.children.is_empty() {
        return Ok(Vec::new());
    }
    // font: the run's or the theme's body (the SmartArt text uses +mn-lt)
    let body: Option<&str> = theme.fonts.as_ref().and_then(|f| f.minor.as_deref());
    let major: Option<&str> = theme.fonts.as_ref().and_then(|f| f.major.as_deref());
    let face = |f: Option<&str>| -> Option<String> {
        match f {
            Some("+mn-lt") => body.map(str::to_string),
            Some("+mj-lt") => major.or(body).map(str::to_string),
            Some(x) => Some(x.to_string()),
            None => body.map(str::to_string),
        }
    };
    let runs_of = |t: &str, size: f64, color: &CssColor| -> Vec<Run> {
        match fmt.get(t) {
            Some(rs) => rs
                .iter()
                .map(|r| {
                    let f = face(r.font.as_deref()).filter(|s| !s.is_empty());
                    Run {
                        text: r.text.as_str().into(),
                        sz: Some(r.sz.unwrap_or(size)),
                        bold: r.bold,
                        italic: r.italic,
                        underline: r.underline,
                        color: Some(color.clone()),
                        font: f.map(|s| s.into()).or_else(|| r.font.as_deref().map(Into::into)),
                        ..Run::default()
                    }
                })
                .collect(),
            None => vec![Run { text: t.into(), sz: Some(size), color: Some(color.clone()), font: body.map(Into::into), ..Run::default() }],
        }
    };
    let fill_c = scheme(theme, "accent1", "");
    let txt = scheme(theme, "lt1", "");
    let dark = scheme(theme, "tx1", "");
    let mut boxes: Vec<BoxF> = Vec::new();
    let mut lines: Vec<[f64; 4]> = Vec::new();
    let mut glyphs: Vec<Glyph> = Vec::new();
    let (fx, fy, fw, fh) = (frame.x, frame.y, frame.cx, frame.cy);
    let tops: &[usize] = &root.children;
    let sub_of = |n: usize| -> Vec<String> {
        nodes.get(n).map(|nd| nd.children.iter().filter_map(|&c| nodes.get(c)).flat_map(|c| c.text.clone()).collect()).unwrap_or_default()
    };
    let text_of = |n: usize| -> Vec<String> { nodes.get(n).map(|nd| nd.text.clone()).unwrap_or_default() };
    let mut box_line: Option<CssColor> = Some(txt.clone());
    // subStyle: fill, alpha, line, geom
    let mut sub_fill = scheme(theme, "accent1", "<a:alpha val=\"90000\"/><a:tint val=\"40000\"/>");
    let mut sub_alpha: Option<f64> = Some(0.9);
    let mut sub_line: Option<CssColor> = None;
    let mut sub_geom: &str = "roundRect";
    let nt = tops.len();
    let ntf = uf(nt);

    if fam == "org" {
        // organization chart: box 1 × 0.5; siblings 0.15 apart; levels 0.3. Below the 2nd level,
        // children that are all leaves "hang" to the right (hierBranch init) when the default tree would be too wide.
        let mut org = Org { nodes, boxes: Vec::new(), lines: Vec::new(), work: 0 };
        let mut x0 = 0.0_f64;
        for &t in tops {
            if org.place(t, 1, x0, 0.0).is_none() {
                return Err(StyleThrow);
            }
            let Some(w) = org.width(t, 1) else { return Err(StyleThrow) };
            x0 += w + OHG;
        }
        boxes = org.boxes;
        lines = org.lines;
        let rects: Vec<Rect> = boxes.iter().map(|b| b.r).collect();
        let map = FitUnits::new(&rects, frame);
        for b in &mut boxes {
            b.r = map.map(b.r);
            b.geom = Some("rect");
        }
        for l in &mut lines {
            let a = map.map(Rect { x: l[0], y: l[1], w: 0.0, h: 0.0 });
            let b = map.map(Rect { x: l[2], y: l[3], w: 0.0, h: 0.0 });
            *l = [a.x, a.y, b.x, b.y];
        }
    } else if fam == "bend" || fam == "process" {
        // process: nodes 1 × 0.6 with arrows between them; curved, zig-zag rows with as many columns as make the node biggest
        let (w, h, hg, vg) = (1.0, 0.6, 0.43, 0.45);
        let mut best_cols = ntf;
        let mut best_s = 0.0_f64;
        let mut cols = if fam == "bend" { 1.0 } else { ntf };
        while cols <= ntf {
            let rows = math::ceil(ntf / cols);
            let s = math::min(fw / (cols * w + (cols - 1.0) * hg), fh / (rows * h + (rows - 1.0) * vg));
            if s > best_s * 1.0001 {
                best_cols = cols;
                best_s = s;
            }
            cols += 1.0;
        }
        let cols = best_cols;
        let cell: Vec<Rect> = (0..nt)
            .map(|i| {
                let fi = uf(i);
                let r = math::floor(fi / cols);
                let c0 = math::rem(fi, cols);
                let c = if math::rem(r, 2.0) != 0.0 { cols - 1.0 - c0 } else { c0 };
                Rect { x: c * (w + hg), y: r * (h + vg), w, h }
            })
            .collect();
        let aw = hg * 0.5;
        let ah = h * 0.35;
        let mut arrows: Vec<Glyph> = Vec::new();
        for i in 0..nt.saturating_sub(1) {
            let (Some(a), Some(b)) = (cell.get(i), cell.get(i + 1)) else { continue };
            if a.y == b.y {
                let left = b.x < a.x;
                let gx = if left { b.x + w + (hg - aw) / 2.0 } else { a.x + w + (hg - aw) / 2.0 };
                arrows.push(Glyph {
                    geom: if left { "leftArrow" } else { "rightArrow" },
                    r: Rect { x: gx, y: a.y + (h - ah) / 2.0, w: aw, h: ah },
                });
            } else {
                arrows.push(Glyph { geom: "downArrow", r: Rect { x: a.x + (w - ah) / 2.0, y: a.y + h + (vg - aw) / 2.0, w: ah, h: aw } });
            }
        }
        let map = FitUnits::new(&cell, frame);
        for (i, &t) in tops.iter().enumerate() {
            let Some(c) = cell.get(i) else { continue };
            boxes.push(BoxF { r: map.map(*c), lines: text_of(t), nd: Some(t), geom: Some("roundRect"), ..BoxF::default() });
        }
        for g in arrows {
            glyphs.push(Glyph { geom: g.geom, r: map.map(g.r) });
        }
    } else if fam == "list1" {
        // grouped list: outlined rectangle with the children and a label over the top edge
        let gap = fh * 0.05;
        let ih = (fh - gap * (ntf - 1.0)) / ntf;
        let hh = math::min(ih * 0.2, fh * 0.08);
        // custom text (custT) does not shrink: the label grows until the lines fit at the recorded size
        let cust_h = |t: usize| -> f64 {
            let txts = text_of(t);
            let pts: Vec<f64> = txts
                .iter()
                .map(|l| {
                    math::max_n(
                        &[
                            vec![0.0],
                            fmt.get(l).map(|rs| rs.iter().map(|r| r.sz.unwrap_or(0.0)).collect::<Vec<f64>>()).unwrap_or_else(|| vec![0.0]),
                        ]
                        .concat(),
                    )
                })
                .collect();
            if !pts.is_empty() && pts.iter().all(|p| *p > 0.0) {
                pts.iter().fold(0.0, |a, p| a + p * 1.2) * EMU_PT + 2.0 * 45720.0
            } else {
                0.0
            }
        };
        for (i, &t) in tops.iter().enumerate() {
            let y = fy + uf(i) * (ih + gap);
            let h = math::min(ih * 0.5, math::max(hh, cust_h(t)));
            let sx = nodes.get(t).and_then(main_pres).map(|p| p.sx);
            let w = fw
                * match sx {
                    Some(sx) if sx != 1.0 => math::min(0.7 * sx, 0.96),
                    _ => 0.9,
                };
            // label: 70% of the width in the layout; stretched by the user (custScaleX) up to the edge (measured: ×1.91 → 96%)
            boxes.push(BoxF {
                r: Rect { x: fx + fw * 0.04, y, w, h },
                lines: text_of(t),
                nd: Some(t),
                geom: Some("roundRect"),
                sub: Some(SubBox { r: Rect { x: fx, y: y + h / 2.0, w: fw, h: ih - h / 2.0 }, lines: sub_of(t) }),
                ..BoxF::default()
            });
        }
        sub_fill = scheme(theme, "lt1", "");
        sub_alpha = None;
        sub_line = Some(fill_c.clone());
        sub_geom = "rect";
        box_line = None;
    } else if fam == "block" {
        // basic block: rectangles 1 × 0.6, gaps 0.12; columns that make the block biggest; last row centred
        let (w, h, g) = (1.0, 0.6, 0.12);
        let mut best_cols = ntf;
        let mut best_s = 0.0_f64;
        let mut cols = 1.0;
        while cols <= ntf {
            let rows = math::ceil(ntf / cols);
            let sc = math::min(fw / (cols * w + (cols - 1.0) * g), fh / (rows * h + (rows - 1.0) * g));
            if sc > best_s * 1.0001 {
                best_cols = cols;
                best_s = sc;
            }
            cols += 1.0;
        }
        let cols = best_cols;
        let cells: Vec<Rect> = (0..nt)
            .map(|i| {
                let fi = uf(i);
                let r = math::floor(fi / cols);
                let c = math::rem(fi, cols);
                let in_row = math::min(cols, ntf - r * cols);
                Rect { x: ((cols - in_row) * (w + g)) / 2.0 + c * (w + g), y: r * (h + g), w, h }
            })
            .collect();
        let map = FitUnits::new(&cells, frame);
        for (i, &t) in tops.iter().enumerate() {
            let Some(c) = cells.get(i) else { continue };
            boxes.push(BoxF { r: map.map(*c), lines: text_of(t), nd: Some(t), geom: Some("rect"), ..BoxF::default() });
        }
    } else if fam == "chevron" {
        // chevron process: 1 × 0.4, step 0.91 (nested); children below
        let (w, h, step) = (1.0, 0.4, 0.91);
        let has_sub = tops.iter().any(|&t| !sub_of(t).is_empty());
        let cells: Vec<Rect> = (0..nt).map(|i| Rect { x: uf(i) * step, y: 0.0, w, h }).collect();
        let mut all = cells.clone();
        if has_sub {
            all.push(Rect { x: 0.0, y: 0.0, w: (ntf - 1.0) * step + w, h: h * 2.6 });
        }
        let map = FitUnits::new(&all, frame);
        for (i, &t) in tops.iter().enumerate() {
            let Some(c) = cells.get(i) else { continue };
            let r = map.map(*c);
            boxes.push(BoxF {
                r,
                lines: text_of(t),
                nd: Some(t),
                geom: Some("chevron"),
                sub: Some(SubBox { r: Rect { x: r.x + r.w * 0.1, y: r.y + r.h * 1.1, w: r.w * 0.75, h: r.h * 1.5 }, lines: sub_of(t) }),
                ..BoxF::default()
            });
        }
        sub_fill = scheme(theme, "accent1", "<a:tint val=\"40000\"/>");
        sub_alpha = Some(0.9);
        sub_geom = "rect";
    } else if fam == "circleList" {
        // vertical list with a circle: the node's rectangle (≈5.3 × 1) and a circle of 0.85 on the left
        let (h, gap, rw, d) = (1.0, 0.34, 5.26, 0.85);
        let rects: Vec<Rect> = (0..nt).map(|i| Rect { x: d - 0.1, y: uf(i) * (h + gap), w: rw, h }).collect();
        let circles: Vec<Rect> = (0..nt).map(|i| Rect { x: 0.0, y: uf(i) * (h + gap) + (h - d) / 2.0, w: d, h: d }).collect();
        let all: Vec<Rect> = rects.iter().chain(circles.iter()).copied().collect();
        let map = FitUnits::new(&all, frame);
        for (i, &t) in tops.iter().enumerate() {
            let (Some(c), Some(r)) = (circles.get(i), rects.get(i)) else { continue };
            boxes.push(BoxF { r: map.map(*c), lines: Vec::new(), geom: Some("ellipse"), acc: true, nd: Some(t), ..BoxF::default() });
            boxes.push(BoxF { r: map.map(*r), lines: text_of(t), nd: Some(t), geom: Some("rect"), ..BoxF::default() });
        }
    } else if fam == "row" {
        let gap = fw * 0.04;
        let bw = (fw - gap * (ntf - 1.0)) / ntf;
        let has_sub = tops.iter().any(|&t| !sub_of(t).is_empty());
        let bh = if has_sub { fh * 0.35 } else { fh * 0.6 };
        for (i, &t) in tops.iter().enumerate() {
            let x = fx + uf(i) * (bw + gap);
            let y = if has_sub { fy } else { fy + (fh - bh) / 2.0 };
            let sy = y + bh + fh * 0.02;
            boxes.push(BoxF {
                r: Rect { x, y, w: bw, h: bh },
                lines: text_of(t),
                nd: Some(t),
                sub: Some(SubBox { r: Rect { x, y: sy, w: bw, h: fy + fh - sy }, lines: sub_of(t) }),
                ..BoxF::default()
            });
        }
    } else if fam == "column" {
        let gap = fh * 0.04;
        let bh = (fh - gap * (ntf - 1.0)) / ntf;
        let has_sub = tops.iter().any(|&t| !sub_of(t).is_empty());
        let bw = if has_sub { fw * 0.36 } else { fw };
        for (i, &t) in tops.iter().enumerate() {
            let y = fy + uf(i) * (bh + gap);
            let sx = fx + bw;
            boxes.push(BoxF {
                r: Rect { x: fx, y, w: bw, h: bh },
                lines: text_of(t),
                nd: Some(t),
                sub: Some(SubBox { r: Rect { x: sx, y: y + bh * 0.05, w: fx + fw - sx, h: bh * 0.9 }, lines: sub_of(t) }),
                ..BoxF::default()
            });
        }
    } else if fam == "cycle" {
        let n = math::max(1.0, ntf);
        let cx = fx + fw / 2.0;
        let cy = fy + fh / 2.0;
        let r = math::min(fw, fh) * 0.36;
        let bs = math::min(fw, fh) * math::min(0.3, 1.6 / (n + 1.0));
        for (i, &t) in tops.iter().enumerate() {
            let a = -std::f64::consts::PI / 2.0 + (2.0 * std::f64::consts::PI * uf(i)) / n;
            let x = cx + r * math::cos(a);
            let y = cy + r * math::sin(a);
            boxes.push(BoxF {
                r: Rect { x: x - bs / 2.0, y: y - bs / 2.0, w: bs, h: bs },
                lines: text_of(t),
                nd: Some(t),
                geom: Some("ellipse"),
                ..BoxF::default()
            });
        }
    } else {
        // basic block: grid of rectangles 1 × 0.6, rows centred
        let truthy_or1 = |x: f64| if x == 0.0 || x.is_nan() { 1.0 } else { x };
        let cols = truthy_or1(math::ceil(math::sqrt((ntf * fw) / math::max(1.0, fh) * 0.6)));
        let rows = math::ceil(ntf / cols);
        let gx = fw * 0.03;
        let gy = fh * 0.04;
        let bw = (fw - gx * (cols - 1.0)) / cols;
        let bh = math::min((fh - gy * (rows - 1.0)) / rows, bw * 0.65);
        for (i, &t) in tops.iter().enumerate() {
            let fi = uf(i);
            let r = math::floor(fi / cols);
            let c = math::rem(fi, cols);
            let in_row = math::min(cols, ntf - r * cols);
            let off = (fw - (in_row * bw + (in_row - 1.0) * gx)) / 2.0;
            boxes.push(BoxF {
                r: Rect { x: fx + off + c * (bw + gx), y: fy + (fh - (rows * bh + (rows - 1.0) * gy)) / 2.0 + r * (bh + gy), w: bw, h: bh },
                lines: text_of(t),
                nd: Some(t),
                ..BoxF::default()
            });
        }
    }

    // UNIFORM size (the application equalizes the text of the nodes of the same level)
    let ins = 45720.0_f64;
    let sz = math::min_n(
        &boxes
            .iter()
            .filter(|b| !b.lines.is_empty())
            .map(|b| fit_pt(&b.lines, b.r.w - 2.0 * ins, b.r.h - 2.0 * ins, 65.0))
            .collect::<Vec<_>>(),
    );
    let subs: Vec<&BoxF> = boxes.iter().filter(|b| b.sub.as_ref().is_some_and(|s| !s.lines.is_empty())).collect();
    let sub_sz = if subs.is_empty() {
        0.0
    } else {
        let mut v = vec![math::round(sz * 0.8)];
        v.extend(subs.iter().filter_map(|b| b.sub.as_ref()).map(|s| fit_pt(&s.lines, s.r.w - 0.25 * 914400.0, s.r.h - 2.0 * ins, 65.0)));
        math::min_n(&v)
    };
    // style by label (colours + quick style of the diagram); without the parts, the default accent1_2
    let look = |lbl: &str, idx: f64, cnt: f64, sp_pr: Option<usize>| -> Result<Option<DgmStyle>, StyleThrow> {
        match styler {
            Some(s) => s.style(lbl, idx, cnt, sp_pr.and_then(|i| tree.sp_prs.get(i).copied())),
            None => Ok(None),
        }
    };
    let mut out: Vec<Shape> = Vec::new();
    let mut k = 0usize;
    let all_pres = |pred: &dyn Fn(&Pres) -> bool| -> Option<&Pres> {
        for b in &boxes {
            if let Some(nd) = b.nd.and_then(|n| nodes.get(n)) {
                for p in &nd.pres {
                    if pred(p) {
                        return Some(p);
                    }
                }
            }
        }
        None
    };
    let conn = all_pres(&|p| p.lbl.starts_with("parChTrans"));
    let conn_st = look(conn.map_or("parChTrans1D2", |c| c.lbl.as_str()), 0.0, 1.0, None)?;
    let line_color = conn_st.as_ref().and_then(|s| s.line.clone()).unwrap_or_else(|| scheme(theme, "accent1", "<a:shade val=\"60000\"/>"));
    for [x1, y1, x2, y2] in &lines {
        let (x1, y1, x2, y2) = (*x1, *y1, *x2, *y2);
        let id = format!("dgmfb-l{k}");
        k += 1;
        out.push(Shape {
            id: id.into(),
            name: "conector".into(),
            geom: Some("line".into()),
            xfrm: Some(Xfrm {
                x: math::min(x1, x2),
                y: math::min(y1, y2),
                cx: math::abs(x2 - x1),
                cy: math::abs(y2 - y1),
                rot: None,
                flip_h: None,
                flip_v: if (x2 < x1) != (y2 < y1) { Some(true) } else { None },
            }),
            line_color: Some(line_color.clone()),
            line_width: Some(conn_st.as_ref().and_then(|s| s.line_width).unwrap_or(12700.0)),
            ..Shape::default()
        });
    }
    for (gi, g) in glyphs.iter().enumerate() {
        let st = look("sibTrans2D1", uf(gi), uf(glyphs.len()), None)?;
        let id = format!("dgmfb-a{k}");
        k += 1;
        out.push(Shape {
            id: id.into(),
            name: "seta".into(),
            geom: Some(g.geom.into()),
            adj: Some(vec![60000.0, 50000.0].into()),
            xfrm: Some(Xfrm {
                x: math::round(g.r.x),
                y: math::round(g.r.y),
                cx: math::round(g.r.w),
                cy: math::round(g.r.h),
                rot: None,
                flip_h: None,
                flip_v: None,
            }),
            fill: Some(Box::new(
                st.and_then(|s| s.fill).unwrap_or_else(|| solid_fill(scheme(theme, "accent1", "<a:tint val=\"60000\"/>"))),
            )),
            ..Shape::default()
        });
    }
    let para = |t: &str, size: f64, color: &CssColor| -> Paragraph {
        Paragraph {
            align: Some("ctr".into()),
            bullet: Some(false),
            mar_l: Some(0.0),
            indent: Some(0.0),
            line_spacing: Some(0.9),
            space_before_pt: Some(0.0),
            space_after_pt: Some(0.0),
            runs: runs_of(t, size, color),
            ..Paragraph::default()
        }
    };
    let bullet_ind = math::round(0.12 * 914400.0);
    for (bi, b) in boxes.iter().enumerate() {
        let bi_f = uf(bi);
        let nb = uf(boxes.len());
        let nd = b.nd.and_then(|n| nodes.get(n));
        if let Some(s) = b.sub.as_ref().filter(|s| !s.lines.is_empty()) {
            // children of the 2nd level: light box behind/beside with bullets
            let sp = nd.and_then(sub_pres);
            let st = look(
                sp.map_or(if fam == "list1" { "bgAccFollowNode1" } else { "alignAccFollowNode1" }, |p| p.lbl.as_str()),
                sp.map_or(bi_f, |p| p.idx),
                sp.map_or(nb, |p| p.cnt),
                sp.and_then(|p| p.sp_pr),
            )?;
            let top_ins = if fam == "list1" { math::round(b.r.h / 2.0 + ins) } else { ins };
            let sfill: Fill = st.as_ref().and_then(|s| s.fill.clone()).unwrap_or_else(|| solid_fill(sub_fill.clone()));
            let fill_alpha = match &st {
                Some(s) if s.fill_alpha.is_some() => s.fill_alpha,
                None if sub_alpha.is_some() => sub_alpha,
                _ => None,
            };
            let (lc, lw): (Option<CssColor>, Option<f64>) = match &st {
                Some(s) => match &s.line {
                    Some(l) => (Some(l.clone()), Some(s.line_width.unwrap_or(12700.0))),
                    None => (None, None),
                },
                None => match &sub_line {
                    Some(l) => (Some(l.clone()), Some(12700.0)),
                    None => (None, None),
                },
            };
            let text_color = st.as_ref().and_then(|s| s.text.clone()).unwrap_or_else(|| dark.clone());
            let id = format!("dgmfb-{k}");
            k += 1;
            out.push(Shape {
                id: id.into(),
                name: "sub".into(),
                geom: Some(sub_geom.into()),
                xfrm: Some(Xfrm {
                    x: math::round(s.r.x),
                    y: math::round(s.r.y),
                    cx: math::round(s.r.w),
                    cy: math::round(s.r.h),
                    rot: None,
                    flip_h: None,
                    flip_v: None,
                }),
                fill: Some(Box::new(sfill)),
                fill_alpha,
                line_color: lc,
                line_width: lw,
                tx_body: Some(Box::new(TextBody {
                    // grouped list: bullets at ~8% of the width (measured); children with 15% before (as the application's drawing)
                    anchor: Some(if fam == "column" { "ctr" } else { "t" }.into()),
                    ins_l: Some(if fam == "list1" { math::round(s.r.w * 0.079) } else { ins * 2.0 }),
                    ins_r: Some(ins),
                    ins_t: Some(top_ins),
                    ins_b: Some(ins),
                    paragraphs: s
                        .lines
                        .iter()
                        .map(|t| Paragraph {
                            align: Some("l".into()),
                            bullet: Some(true),
                            bullet_char: Some("\u{2022}".into()),
                            mar_l: Some(bullet_ind),
                            indent: Some(-bullet_ind),
                            line_spacing: Some(0.9),
                            space_before_pt: Some(0.0),
                            space_before_pct: Some(0.15),
                            runs: runs_of(t, sub_sz, &text_color),
                            ..Paragraph::default()
                        })
                        .collect(),
                    ..TextBody::default()
                })),
                ..Shape::default()
            });
        }
        // accessory box (the circle of vList3): the node's accent label
        let mp: Option<&Pres> = match nd {
            Some(nd) => {
                if b.acc {
                    let mn = main_pres(nd).map(std::ptr::from_ref);
                    nd.pres.iter().find(|p| p.cnt > 0.0 && Some(std::ptr::from_ref(*p)) != mn)
                } else {
                    main_pres(nd)
                }
            }
            None => None,
        };
        let st = if b.acc {
            look(
                mp.map_or("fgImgPlace1", |p| p.lbl.as_str()),
                mp.map_or(bi_f, |p| p.idx),
                mp.map_or(nb, |p| p.cnt),
                mp.and_then(|p| p.sp_pr),
            )?
        } else {
            let sp = nd.and_then(|n| n.sp_pr).or_else(|| mp.and_then(|p| p.sp_pr));
            look(mp.map_or("node1", |p| p.lbl.as_str()), mp.map_or(bi_f, |p| p.idx), mp.map_or(nb, |p| p.cnt), sp)?
        };
        let (lc, lw): (Option<CssColor>, Option<f64>) = match &st {
            Some(s) => match &s.line {
                Some(l) => (Some(l.clone()), Some(s.line_width.unwrap_or(12700.0))),
                None => (None, None),
            },
            None => match &box_line {
                Some(l) => (Some(l.clone()), Some(12700.0)),
                None => (None, None),
            },
        };
        let text_color = st.as_ref().and_then(|s| s.text.clone()).unwrap_or_else(|| txt.clone());
        let id = format!("dgmfb-{k}");
        k += 1;
        out.push(Shape {
            id: id.into(),
            name: "n\u{00f3}".into(),
            geom: Some(b.geom.unwrap_or("roundRect").into()),
            xfrm: Some(Xfrm {
                x: math::round(b.r.x),
                y: math::round(b.r.y),
                cx: math::round(b.r.w),
                cy: math::round(b.r.h),
                rot: None,
                flip_h: None,
                flip_v: None,
            }),
            fill: Some(Box::new(st.as_ref().and_then(|s| s.fill.clone()).unwrap_or_else(|| solid_fill(fill_c.clone())))),
            fill_alpha: st.as_ref().and_then(|s| s.fill_alpha),
            line_color: lc,
            line_width: lw,
            tx_body: Some(Box::new(TextBody {
                anchor: Some("ctr".into()),
                ins_l: Some(ins),
                ins_r: Some(ins),
                ins_t: Some(ins),
                ins_b: Some(ins),
                paragraphs: b.lines.iter().map(|t| para(t, sz, &text_color)).collect(),
                ..TextBody::default()
            })),
            ..Shape::default()
        });
    }
    Ok(out)
}

/// `FillKind` kept in scope for the fill helpers of the TS (`{ type: 'none' }`).
#[allow(dead_code)]
fn _fill_none() -> Fill {
    Fill::of_kind(FillKind::None)
}
