//! The output of the SmartArt layout engine (`diagram-layout.ts:1744–2497`): connectors, the text fit,
//! the shapes as a `dsp:spTree`, and the `layoutDiagram` driver. Text metrics and the preset text
//! rectangles are here too (`textHeight`, `textRect`).

use rustc_hash::{FxHashMap, FxHashSet};
use uo_jsnum::format::to_js_string;
use uo_jsnum::math;
use uo_jsnum::parse::{parse_float, parse_int};
use uo_jsnum::text::is_js_space;

use super::data::{Ag, Attrs, PtId, read_model};
use super::engine::{Conn, EMU_PT, Eng, FitCtx, NId, TextPara, marg_fonts, norm360, params, quarter, usize_to_f};
use super::metrics::{DiagramFont, DiagramMeasure};
use super::xt::{X, XC, mk, mk_s, norm_ns};

/// DrawingML colour element → `#rrggbb`.
pub type ResolveColor<'a> = dyn Fn(&X) -> Option<String> + 'a;

/// The input of [`layout_diagram`] (`DiagramLayoutInput`, `diagram-layout.ts:25`).
pub struct DiagramInput<'a> {
    pub data: &'a X,
    pub layout: &'a X,
    pub style: Option<&'a X>,
    pub colors: Option<&'a X>,
    /// frame size, EMU
    pub cx: f64,
    pub cy: f64,
    /// DrawingML colour → `#rrggbb` (to interpolate `span` lists)
    pub resolve_color: Option<&'a ResolveColor<'a>>,
    pub measure: Option<&'a dyn DiagramMeasure>,
    /// the theme's body font (the SmartArt text uses +mn-lt)
    pub minor_font: Option<&'a str>,
}

// ---------------------------------------------------------------------------------------------
// text
// ---------------------------------------------------------------------------------------------

/// A run of a paragraph's text: text and its `a:rPr`.
pub struct RunT<'a> {
    pub text: String,
    pub r_pr: Option<&'a X>,
}

/// `paraText` (`diagram-layout.ts:1894`).
pub fn para_text<'a>(p: Option<&'a X>) -> Vec<RunT<'a>> {
    let mut runs = Vec::new();
    let Some(p) = p else { return runs };
    for r in p.kids() {
        if r.name == "a:r" || r.name == "a:fld" {
            let t = r.first("a:t");
            runs.push(RunT { text: t.map(X::text).unwrap_or_default(), r_pr: r.first("a:rPr") });
        } else if r.name == "a:br" {
            runs.push(RunT { text: "\n".to_string(), r_pr: None });
        }
    }
    runs
}

enum Sp {
    Pct(f64),
    Pts(f64),
}

/// `userSpacing`: spacing recorded in the paragraph's pPr: fraction (spcPct) or points (spcPts).
fn user_spacing(p: Option<&X>, name: &str) -> Option<Sp> {
    let ppr = p?.first("a:pPr")?;
    let e = ppr.first(name)?;
    let pc = e.first("a:spcPct");
    let pt = e.first("a:spcPts");
    let v = parse_int(pc.or(pt).and_then(|x| x.attr("val")).unwrap_or(""), 10);
    if !v.is_finite() || v < 0.0 || v > 1e7 {
        return None;
    }
    if pc.is_some() {
        Some(Sp::Pct(v / 100000.0))
    } else if pt.is_some() {
        Some(Sp::Pts(v / 100.0))
    } else {
        None
    }
}

fn bullet_indent(sz_pt: f64) -> f64 {
    sz_pt * EMU_PT * 0.5
}

/// `textHeight`: the height (EMU) of the text at the given size, breaking lines at `width`; infinity if a word does not fit.
pub fn text_height(
    paras: &[TextPara<'_>],
    prim: f64,
    sec: f64,
    width: f64,
    measure: &dyn DiagramMeasure,
    family: Option<&str>,
    line_k: f64,
) -> f64 {
    let mut hgt = 0.0_f64;
    for (pi, tp) in paras.iter().enumerate() {
        let sz = tp.csz.unwrap_or(if tp.level == 0.0 { prim } else { sec });
        let avail = width - if tp.level > 0.0 { bullet_indent(sz) * tp.level } else { 0.0 };
        let runs = para_text(tp.p);
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        let font = DiagramFont {
            family,
            bold: runs.iter().any(|r| r.r_pr.and_then(|x| x.attr("b")) == Some("1")),
            italic: runs.iter().any(|r| r.r_pr.and_then(|x| x.attr("i")) == Some("1")),
        };
        let mut lines = 0.0_f64;
        for seg in text.split('\n') {
            lines += 1.0;
            let mut cur = 0.0_f64;
            // `seg.split(/(\s+)/)`: maximal runs of white space and of anything else
            let mut tokens: Vec<(&str, bool)> = Vec::new();
            let mut start = 0usize;
            let mut cur_ws: Option<bool> = None;
            for (i, ch) in seg.char_indices() {
                let ws = is_js_space(ch);
                match cur_ws {
                    Some(w) if w == ws => {}
                    Some(w) => {
                        tokens.push((seg.get(start..i).unwrap_or(""), w));
                        start = i;
                        cur_ws = Some(ws);
                    }
                    None => cur_ws = Some(ws),
                }
            }
            if let Some(w) = cur_ws {
                tokens.push((seg.get(start..).unwrap_or(""), w));
            }
            for (w, is_ws) in tokens {
                let ww = measure.width(w, sz, &font);
                if is_ws {
                    cur += ww;
                    continue;
                }
                if ww > avail + 1.0 {
                    return f64::INFINITY;
                }
                if cur > 0.0 && cur + ww > avail + 1.0 {
                    lines += 1.0;
                    cur = ww;
                } else {
                    cur += ww;
                }
            }
        }
        // spacing recorded by the user (pPr of the data) counts as in the saved drawing
        let ln = user_spacing(tp.p, "a:lnSpc");
        hgt += lines
            * match ln {
                Some(Sp::Pts(p)) => p * EMU_PT,
                Some(Sp::Pct(p)) => sz * EMU_PT * line_k * p,
                None => sz * EMU_PT * line_k * 0.9,
            };
        if pi + 1 < paras.len() {
            let nxt = &paras[pi + 1];
            let aft = user_spacing(tp.p, "a:spcAft");
            let bef = user_spacing(nxt.p, "a:spcBef");
            let nsz = nxt.csz.unwrap_or(if nxt.level == 0.0 { prim } else { sec });
            hgt += match aft {
                Some(Sp::Pts(p)) => p * EMU_PT,
                Some(Sp::Pct(p)) => sz * EMU_PT * 1.2 * p,
                None => sz * EMU_PT * 1.2 * (if tp.level == 0.0 { 0.35 } else { 0.15 }),
            };
            match bef {
                Some(Sp::Pts(p)) => hgt += p * EMU_PT,
                Some(Sp::Pct(p)) => hgt += nsz * EMU_PT * 1.2 * p,
                None => {}
            }
        }
    }
    hgt
}

/// The names of the guides of each preset's text rectangle (ECMA-376 presetShapeDefinitions `<a:rect>`).
fn text_rect_names(prst: &str) -> Option<[&'static str; 4]> {
    Some(match prst {
        "roundRect" | "plaque" => ["il", "il", "ir", "ib"],
        "round1Rect" | "homePlate" => ["l", "t", "ir", "b"],
        "round2SameRect" => ["il", "tdx", "ir", "ib"],
        "snip1Rect" => ["l", "it", "ir", "b"],
        "snip2SameRect" => ["il", "it", "ir", "ib"],
        "ellipse" | "flowChartConnector" | "pie" | "donut" | "blockArc" | "circularArrow" | "leftCircularArrow" | "teardrop"
        | "hexagon" | "parallelogram" => ["il", "it", "ir", "ib"],
        "chevron" => ["il", "t", "ir", "b"],
        "pentagon" => ["x2", "it", "x3", "y2"],
        "octagon" => ["il", "il", "ir", "ib"],
        "triangle" => ["x1", "vc", "x3", "b"],
        "trapezoid" | "nonIsoscelesTrapezoid" => ["il", "it", "ir", "b"],
        "diamond" => ["wd4", "hd4", "ir", "ib"],
        "rightArrow" => ["l", "y1", "x2", "y2"],
        "leftArrow" => ["x1", "y1", "r", "y2"],
        "upArrow" => ["x1", "y1", "x2", "b"],
        "downArrow" => ["x1", "t", "x2", "y2"],
        "leftRightArrow" => ["x1", "y1", "x4", "y2"],
        "flowChartManualOperation" => ["x3", "t", "x4", "b"],
        "flowChartExtract" => ["wd4", "vc", "x2", "b"],
        "can" => ["l", "y2", "r", "y3"],
        "cube" => ["l", "y1", "x4", "b"],
        "upArrowCallout" => ["l", "y2", "r", "b"],
        "downArrowCallout" => ["l", "t", "r", "y2"],
        "rightArrowCallout" => ["l", "t", "x2", "b"],
        "leftArrowCallout" => ["x2", "t", "r", "b"],
        "mathPlus" => ["x1", "y2", "x4", "y3"],
        "mathMinus" => ["x1", "y1", "x2", "y2"],
        "mathEqual" => ["x1", "y1", "x2", "y4"],
        "leftBrace" => ["il", "it", "r", "ib"],
        "rightBrace" => ["l", "it", "ir", "ib"],
        _ => return None,
    })
}

/// `textRect`: the text rectangle of the preset in a w×h box.
pub fn text_rect(prst: &str, w: f64, h: f64, adj: &[(String, f64)]) -> (f64, f64, f64, f64) {
    let Some(names) = text_rect_names(prst) else { return (0.0, 0.0, w, h) };
    let vals: Vec<f64> = adj.iter().map(|a| a.1).collect();
    let nms: Vec<&str> = adj.iter().map(|a| a.0.as_str()).collect();
    let Some(g) = uo_drawingml::preset_guides(prst, w, h, Some(&vals), Some(&nms)) else { return (0.0, 0.0, w, h) };
    let v = |k: &str, d: f64| match g.get(k) {
        Some(x) if x.is_finite() => x,
        _ => d,
    };
    (v(names[0], 0.0), v(names[1], 0.0), v(names[2], w), v(names[3], h))
}

// ---------------------------------------------------------------------------------------------
// geometry of the nodes
// ---------------------------------------------------------------------------------------------

type P2 = (f64, f64);

struct Geom {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    prst: String,
    adj: Vec<(String, f64)>,
    rot: f64,
}

struct TextJob<'a> {
    tn: NId,
    paras: std::rc::Rc<Vec<TextPara<'a>>>,
    /// box of the text (visual, in the frame)
    bx: (f64, f64, f64, f64),
    /// final rotation of the text (degrees): 0 = upright
    rot: f64,
}

fn nzf(v: f64) -> f64 {
    if v == 0.0 || v.is_nan() { 0.0 } else { v }
}

impl<'a> Eng<'a> {
    fn box_pt(&self, n: NId, k: &str) -> P2 {
        let nd = &self.nodes[n];
        let (x, y, w, h) = (nd.x, nd.y, nd.w, nd.h);
        match k {
            "tL" => (x, y),
            "tR" => (x + w, y),
            "bL" => (x, y + h),
            "bR" => (x + w, y + h),
            "tCtr" => (x + w / 2.0, y),
            "bCtr" => (x + w / 2.0, y + h),
            "midL" => (x, y + h / 2.0),
            "midR" => (x + w, y + h / 2.0),
            _ => (x + w / 2.0, y + h / 2.0),
        }
    }

    /// The point where the line centre→target leaves the node's box (or ellipse).
    fn exit_pt(&self, n: NId, to: P2) -> P2 {
        let nd = &self.nodes[n];
        let cx = nd.x + nd.w / 2.0;
        let cy = nd.y + nd.h / 2.0;
        let dx = to.0 - cx;
        let dy = to.1 - cy;
        if (dx == 0.0 || dx.is_nan()) && (dy == 0.0 || dy.is_nan()) {
            return (cx, cy);
        }
        let ty = nd.shape.and_then(|s| s.attr("type"));
        if ty == Some("ellipse") || ty == Some("circle") {
            let a = nd.w / 2.0;
            let b = nd.h / 2.0;
            let t = 1.0 / math::sqrt((dx * dx) / nz1(a * a) + (dy * dy) / nz1(b * b));
            return (cx + dx * t, cy + dy * t);
        }
        let t = math::min(
            if nzf(dx) != 0.0 { nd.w / 2.0 / math::abs(dx) } else { f64::INFINITY },
            if nzf(dy) != 0.0 { nd.h / 2.0 / math::abs(dy) } else { f64::INFINITY },
        );
        (cx + dx * t, cy + dy * t)
    }

    /// The name of the connection point picked from a list (the closest to the other side).
    fn picked_name(&self, k: NId, spec: &str, other: P2) -> String {
        let opts: Vec<&str> = spec.split(is_js_space).filter(|s| !s.is_empty()).collect();
        if opts.len() <= 1 {
            return opts.first().copied().unwrap_or("auto").to_string();
        }
        let mut best = opts[0];
        let mut bd = f64::INFINITY;
        for o in &opts {
            let q = self.box_pt(k, o);
            let d = math::hypot(q.0 - other.0, q.1 - other.1);
            if d < bd {
                bd = d;
                best = o;
            }
        }
        best.to_string()
    }

    fn conn_ends(&self, n: NId) -> Option<(NId, NId)> {
        let p = params(self.nodes[n].alg);
        let geo = |k: NId| {
            k != n
                && self.alg_type(k) != "conn"
                && self.nodes[k].shape.and_then(|s| s.attr("type")).is_some_and(|t| !t.is_empty() && t != "none")
        };
        let sibs: Vec<NId> = self.nodes[n].parent.map(|pp| self.nodes[pp].kids.clone()).unwrap_or_default();
        let all: Vec<NId> = (0..self.nodes.len()).collect();
        let find = |pt: Option<PtId>, name: Option<&str>| -> Option<NId> {
            let pt = pt?;
            if let Some(name) = name.filter(|s| !s.is_empty()) {
                return all
                    .iter()
                    .copied()
                    .find(|&k| self.nodes[k].pt == pt && self.nodes[k].name == name)
                    .or_else(|| sibs.iter().copied().find(|&k| self.nodes[k].name == name && self.nodes[k].pt == pt));
            }
            // without a name: the sibling with the same end (lin/cycle); else any shape of that end
            sibs.iter()
                .copied()
                .find(|&k| self.nodes[k].pt == pt && geo(k))
                .or_else(|| {
                    let base = sibs.iter().copied().find(|&k| self.nodes[k].pt == pt).unwrap_or(n);
                    let mut d = Vec::new();
                    self.des_nodes(base, &mut d, 0);
                    d.into_iter().find(|&k| self.nodes[k].pt == pt && geo(k))
                })
                .or_else(|| all.iter().copied().find(|&k| self.nodes[k].pt == pt && geo(k)))
        };
        let pt_n = &self.pts[self.nodes[n].pt];
        let mut src = find(pt_n.src, p.ag("srcNode"));
        let mut dst = find(pt_n.dst, p.ag("dstNode"));
        if src.is_none() || dst.is_none() {
            let sb: Vec<NId> = sibs.iter().copied().filter(|&k| !self.is_trans(k) || k == n).collect();
            let i: isize = sb.iter().position(|&x| x == n).map_or(-1, |p| isize::try_from(p).unwrap_or(0));
            let at = |j: isize| -> Option<NId> { usize::try_from(j).ok().and_then(|j| sb.get(j).copied()) };
            if src.is_none() {
                src = at(i - 1);
            }
            if dst.is_none() {
                dst = at(i + 1);
            }
        }
        match (src, dst) {
            (Some(s), Some(d)) => Some((s, d)),
            _ => None,
        }
    }

    fn layout_conn(&mut self, n: NId) {
        let Some((src, dst)) = self.conn_ends(n) else { return };
        let p = params(self.nodes[n].alg);
        let arc_ok = p.ag("connRout") == Some("curve") && p.ag("dim").unwrap_or("2D") == "1D";
        if arc_ok
            && let Some(cyc) = self.nodes[n].parent.and_then(|pp| self.nodes[pp].cyc)
            && self.arc_conn(n, src, dst, cyc)
        {
            return;
        }
        let sc = (self.nodes[src].x + self.nodes[src].w / 2.0, self.nodes[src].y + self.nodes[src].h / 2.0);
        let dc = (self.nodes[dst].x + self.nodes[dst].w / 2.0, self.nodes[dst].y + self.nodes[dst].h / 2.0);
        let bp = p.ag("begPts").unwrap_or("auto").to_string();
        let ep = p.ag("endPts").unwrap_or("auto").to_string();
        let pick = |this: &Self, k: NId, spec: &str, other: P2| -> P2 {
            let opts: Vec<&str> = spec.split(is_js_space).filter(|s| !s.is_empty()).collect();
            if opts.is_empty() || opts[0] == "auto" || opts[0] == "radial" {
                return this.exit_pt(k, other);
            }
            // several candidates: the closest to the other side
            let mut best = this.box_pt(k, opts[0]);
            let mut bd = f64::INFINITY;
            for o in &opts {
                let q = this.box_pt(k, o);
                let d = math::hypot(q.0 - other.0, q.1 - other.1);
                if d < bd {
                    bd = d;
                    best = q;
                }
            }
            best
        };
        let a = pick(self, src, &bp, dc);
        let b = pick(self, dst, &ep, sc);
        let dist = math::hypot(b.0 - a.0, b.1 - a.1);
        // begPad/endPad relative to the distance (connDist)
        self.nodes[n].v.set("connDist", dist);
        self.apply_conn_dist(n);
        // 2D arrow without begPad/endPad: insets of 22% and 25% of the distance (measured in cycle2)
        let two_d = p.ag("dim").unwrap_or("2D") != "1D";
        let beg = self.nodes[n].v.val("begPad").unwrap_or(if two_d { 0.22 * dist } else { 0.0 });
        let end = self.nodes[n].v.val("endPad").unwrap_or(if two_d { 0.25 * dist } else { 0.0 });
        let len = math::max(0.0, dist - beg - end);
        let truthy = |v: f64| v != 0.0 && !v.is_nan();
        let ux = if truthy(dist) { (b.0 - a.0) / dist } else { 1.0 };
        let uy = if truthy(dist) { (b.1 - a.1) / dist } else { 0.0 };
        let ang = math::atan2(uy, ux) * 180.0 / std::f64::consts::PI;
        let dim = p.ag("dim").unwrap_or("2D");
        if dim == "1D" {
            // line (straight or elbow) as a free path in the bounding box
            let s: P2 = (a.0 + ux * beg, a.1 + uy * beg);
            let e: P2 = (b.0 - ux * end, b.1 - uy * end);
            let mut pts: Vec<P2> = vec![s];
            if p.ag("connRout") == Some("bend") && math::abs(s.0 - e.0) > 1.0 && math::abs(s.1 - e.1) > 1.0 {
                // direction of leaving/arriving by the connection points (middle of the side: perpendicular to it)
                let pn_out = self.picked_name(src, &bp, dc);
                let pn_in = self.picked_name(dst, &ep, sc);
                let is_tb = |s: &str| s == "tCtr" || s == "bCtr";
                let is_mid = |s: &str| s == "midL" || s == "midR";
                let vert_dom = math::abs(e.1 - s.1) >= math::abs(e.0 - s.0);
                let v_out = is_tb(&pn_out) || (!is_mid(&pn_out) && vert_dom);
                let v_in = is_tb(&pn_in) || (!is_mid(&pn_in) && vert_dom);
                let bd = self.nodes[n].v.val("bendDist");
                if v_out && v_in {
                    let my = s.1
                        + match bd {
                            Some(bd) => math::sign(e.1 - s.1) * bd,
                            None => (e.1 - s.1) / 2.0,
                        };
                    pts.push((s.0, my));
                    pts.push((e.0, my));
                } else if !v_out && !v_in {
                    let mx = s.0
                        + match bd {
                            Some(bd) => math::sign(e.0 - s.0) * bd,
                            None => (e.0 - s.0) / 2.0,
                        };
                    pts.push((mx, s.1));
                    pts.push((mx, e.1));
                } else {
                    pts.push(if v_out { (s.0, e.1) } else { (e.0, s.1) });
                }
            }
            pts.push(e);
            let mut x0 = math::min_n(&pts.iter().map(|q| q.0).collect::<Vec<_>>());
            let mut y0 = math::min_n(&pts.iter().map(|q| q.1).collect::<Vec<_>>());
            let x1 = math::max_n(&pts.iter().map(|q| q.0).collect::<Vec<_>>());
            let y1 = math::max_n(&pts.iter().map(|q| q.1).collect::<Vec<_>>());
            let mut w = x1 - x0;
            let mut h = y1 - y0;
            // straight vertical/horizontal line: the application writes a 0.1" box centred on the line
            if w < 1.0 {
                w = 91440.0;
                x0 -= 45720.0;
            }
            if h < 1.0 {
                h = 91440.0;
                y0 -= 45720.0;
            }
            let ch: Vec<XC> = pts
                .iter()
                .enumerate()
                .map(|(i, q)| XC::E(mk(if i > 0 { "a:lnTo" } else { "a:moveTo" }, &[], vec![XC::E(pt_el(q.0 - x0, q.1 - y0))])))
                .collect();
            let path = mk_s("a:path", vec![("w", jsr(math::max(1.0, math::round(w)))), ("h", jsr(math::max(1.0, math::round(h))))], ch);
            self.nodes[n].conn =
                Some(Conn { prst: "cust".into(), x: x0, y: y0, w, h, rot: 0.0, flip_h: false, adj: None, path: Some(path), line: true });
            return;
        }
        // 2D arrow: length = distance minus the insets; height = the node's
        let hh = self.nodes[n].h;
        let cx = a.0 + ux * (beg + len / 2.0);
        let cy = a.1 + uy * (beg + len / 2.0);
        let rot = math::rem(math::rem(math::round(ang * 1e4) / 1e4, 360.0) + 360.0, 360.0);
        let beg_arr = p.ag("begSty") == Some("arr");
        let end_arr = p.ag("endSty").unwrap_or("arr") == "arr";
        let prst = if beg_arr && end_arr {
            "leftRightArrow"
        } else if beg_arr {
            "leftArrow"
        } else if end_arr {
            "rightArrow"
        } else {
            "rect"
        };
        self.nodes[n].conn = Some(Conn {
            prst: prst.into(),
            x: cx - len / 2.0,
            y: cy - hh / 2.0,
            w: len,
            h: hh,
            rot,
            flip_h: false,
            adj: if prst == "rightArrow" { Some(vec![("adj1".to_string(), 60000.0), ("adj2".to_string(), 50000.0)]) } else { None },
            path: None,
            line: false,
        });
    }

    /// The constraints of a connector without the empty `connDist` (`keep.filter(...)`), "self" ones only.
    fn apply_conn_dist(&mut self, n: NId) {
        let list: Vec<_> =
            self.nodes[n].constrs.iter().filter(|c| !(c.ty == "connDist" && c.ref_type == "none" && !c.has_val)).cloned().collect();
        self.apply_constrs(n, &list, None, true, false);
    }

    /// Curved connector (connRout="curve") in a cycle: an arc of the circle itself.
    fn arc_conn(&mut self, n: NId, src: NId, dst: NId, cyc: super::engine::Cyc) -> bool {
        let (cx, cy, r) = (cyc.cx, cyc.cy, cyc.r);
        if r.is_nan() || r <= 0.0 {
            return false;
        }
        let at = |this: &Self, k: NId| {
            math::atan2(this.nodes[k].x + this.nodes[k].w / 2.0 - cx, -(this.nodes[k].y + this.nodes[k].h / 2.0 - cy))
        }; // 0 = 12 o'clock, clockwise
        let pt = |phi: f64| -> P2 { (cx + r * math::sin(phi), cy - r * math::cos(phi)) };
        let inside = |this: &Self, k: NId, q: P2| {
            let nd = &this.nodes[k];
            q.0 > nd.x && q.0 < nd.x + nd.w && q.1 > nd.y && q.1 < nd.y + nd.h
        };
        let dir = if cyc.cw { 1.0 } else { -1.0 };
        let a0 = at(self, src);
        let mut a1 = at(self, dst);
        let mut guard = 0;
        while dir * (a1 - a0) <= 0.0 {
            a1 += dir * 2.0 * std::f64::consts::PI;
            guard += 1;
            if guard > 1_000_000 {
                break;
            }
        }
        let span = math::abs(a1 - a0);
        // exit from the source / entry into the destination along the circle (fine step + bisection)
        let edge = |this: &Self, k: NId, from: f64, sgn: f64| -> f64 {
            let (mut lo, mut hi) = (0.0_f64, 0.0_f64);
            let mut d = 0.0_f64;
            let mut it = 0usize;
            while d <= span {
                if !inside(this, k, pt(from + sgn * d)) {
                    hi = d;
                    break;
                }
                lo = d;
                hi = d;
                d += span / 200.0;
                it += 1;
                if it > 100_000 {
                    break;
                }
            }
            for _ in 0..30 {
                let m = (lo + hi) / 2.0;
                if inside(this, k, pt(from + sgn * m)) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            hi
        };
        let e0 = a0 + dir * edge(self, src, a0, dir);
        let e1 = a1 - dir * edge(self, dst, a1, -dir);
        let arc = math::abs(e1 - e0);
        self.nodes[n].v.set("connDist", arc * r);
        self.apply_conn_dist(n);
        let bp = self.nodes[n].v.val("begPad").unwrap_or(0.0) / r;
        let ep = self.nodes[n].v.val("endPad").unwrap_or(0.0) / r;
        let s0 = e0 + dir * bp;
        let s1 = e1 - dir * ep;
        if dir * (s1 - s0) <= 0.0 {
            return false;
        }
        // cubic Béziers in pieces of up to 45°
        let x0 = cx - r;
        let y0 = cy - r;
        let mut segs: Vec<XC> = Vec::new();
        let pp = |q: P2| XC::E(pt_el(q.0 - x0, q.1 - y0));
        segs.push(XC::E(mk("a:moveTo", &[], vec![pp(pt(s0))])));
        let parts = math::max(1.0, math::ceil(math::abs(s1 - s0) / (std::f64::consts::PI / 4.0)));
        let parts_n = super::engine::f64_to_usize(parts);
        for i in 0..parts_n {
            let fi = usize_to_f(i);
            let t0 = s0 + ((s1 - s0) * fi) / parts;
            let t1 = s0 + ((s1 - s0) * (fi + 1.0)) / parts;
            let k = (4.0 / 3.0) * math::tan((t1 - t0) / 4.0) * r;
            let p0 = pt(t0);
            let p3 = pt(t1);
            let d0 = (math::cos(t0), math::sin(t0));
            let d1 = (math::cos(t1), math::sin(t1));
            segs.push(XC::E(mk(
                "a:cubicBezTo",
                &[],
                vec![pp((p0.0 + k * d0.0, p0.1 + k * d0.1)), pp((p3.0 - k * d1.0, p3.1 - k * d1.1)), pp(p3)],
            )));
        }
        let path =
            mk_s("a:path", vec![("w", jsr(math::round(2.0 * r))), ("h", jsr(math::round(2.0 * r))), ("fill", "none".to_string())], segs);
        self.nodes[n].conn = Some(Conn {
            prst: "cust".into(),
            x: x0,
            y: y0,
            w: 2.0 * r,
            h: 2.0 * r,
            rot: 0.0,
            flip_h: false,
            adj: None,
            path: Some(path),
            line: true,
        });
        true
    }

    // ----- geometry of a node and its text ----------------------------------------------------

    fn geom_of(&self, n: NId) -> Geom {
        let nd = &self.nodes[n];
        let shape_rot = nzf(parse_float(nd.shape.and_then(|s| s.attr("rot")).unwrap_or("0")));
        if let Some(g) = &nd.conn {
            let rot = norm360(g.rot + shape_rot + nd.rot);
            return Geom {
                x: g.x,
                y: g.y,
                w: g.w,
                h: g.h,
                prst: if g.prst == "cust" { "rect".to_string() } else { g.prst.clone() },
                adj: g.adj.clone().unwrap_or_default(),
                rot,
            };
        }
        let rot = norm360(0.0 + shape_rot + nd.rot);
        let ty = nd.shape.and_then(|s| s.attr("type")).unwrap_or("rect");
        Geom {
            x: nd.x,
            y: nd.y,
            w: nd.w,
            h: nd.h,
            prst: if ty == "conn" { "rightArrow".to_string() } else { ty.to_string() },
            adj: nd.adj_ov.clone().unwrap_or_else(|| self.adj_of(n)),
            rot,
        }
    }

    /// The text rotation by the text node's autoTxRot: upr (upright), grav (follows without being upside down), none.
    fn text_rot(&self, tn: NId, shape_rot: f64) -> f64 {
        let p = params(self.nodes[tn].alg);
        let m = p.ag("autoTxRot").unwrap_or("upr");
        if m == "none" {
            return shape_rot;
        }
        if m == "grav" {
            let r = norm360(shape_rot);
            return if r > 90.0 && r <= 270.0 { norm360(r - 180.0) } else { r };
        }
        0.0
    }

    /// The text rectangle of the geometry, rotated with the shape, as a visual box.
    fn geom_text_box(&self, n: NId) -> (f64, f64, f64, f64) {
        let g = self.geom_of(n);
        let sw = quarter(g.rot);
        // a shape rotated 90°/270° has the sides swapped (it takes the layout's box)
        let (lw, lh) = if sw { (g.h, g.w) } else { (g.w, g.h) };
        let (l, t, r, b) = text_rect(&g.prst, lw, lh, &g.adj);
        let ox = (l + r) / 2.0 - lw / 2.0;
        let oy = (t + b) / 2.0 - lh / 2.0;
        let th = (g.rot * std::f64::consts::PI) / 180.0;
        let dx = ox * math::cos(th) - oy * math::sin(th);
        let dy = ox * math::sin(th) + oy * math::cos(th);
        let (tw, tht) = if sw { (b - t, r - l) } else { (r - l, b - t) };
        let cx = g.x + g.w / 2.0 + dx;
        let cy = g.y + g.h / 2.0 + dy;
        (cx - tw / 2.0, cy - tht / 2.0, tw, tht)
    }

    fn hidden(&self, n: NId) -> bool {
        matches!(self.nodes[n].shape.and_then(|s| s.attr("hideGeom")), Some("1" | "true"))
    }

    /// `fitText`: the largest size (integer pt) at which the text fits the box, going down to the rules' minimum.
    fn fit_text(&mut self, job: &TextJob<'a>, measure: &dyn DiagramMeasure, family: Option<&str>, line_k: f64) {
        let n = job.tn;
        let paras = job.paras.clone();
        let has_prim = paras.iter().any(|p| p.level == 0.0);
        let rule_min = |this: &Self, t: &str, d: f64| -> f64 {
            let l: Vec<f64> = this.nodes[n].rules.iter().filter(|r| r.ty == t && r.val.is_finite()).map(|r| r.val).collect();
            if l.is_empty() { d } else { math::min_n(&l) }
        };
        let prim_max = self.nodes[n].v.val("primFontSz").or_else(|| self.nodes[n].v.val("secFontSz")).unwrap_or(65.0);
        let sec_fixed = self.nodes[n].v.val("secFontSz");
        let (_, _, bw, bh) = job.bx;
        let (text_w, text_h) = if quarter(job.rot) { (bh, bw) } else { (bw, bh) };
        let fits = |this: &Self, prim: f64, sec: f64| -> bool {
            let (mp, ms) = marg_fonts(&paras, prim, sec);
            let w_a = text_w - this.margin(n, "lMarg", mp, ms) - this.margin(n, "rMarg", mp, ms);
            let h_a = text_h - this.margin(n, "tMarg", mp, ms) - this.margin(n, "bMarg", mp, ms);
            text_height(&paras, prim, sec, w_a, measure, family, line_k) <= h_a + 1.0
        };
        // all text custom: nothing shrinks (the layout's font variable stays at the maximum)
        if !paras.is_empty() && paras.iter().all(|p| p.csz.is_some()) {
            self.nodes[n].font_pt = Some(math::round(prim_max));
            self.nodes[n].sec_pt = Some(math::round(sec_fixed.unwrap_or(if has_prim { prim_max * 0.78 } else { prim_max })));
            return;
        }
        // `for (; s > Math.max(1, min); s--)` with the starting size bounded (a 1e9 pt font would loop for ever)
        let cap = |x: f64| math::min(x, 1_000_000.0);
        if !has_prim {
            let max = cap(math::round(sec_fixed.unwrap_or(prim_max)));
            let min = math::round(rule_min(self, "secFontSz", rule_min(self, "primFontSz", max)));
            let mut s = max;
            while s > math::max(1.0, min) {
                if fits(self, s, s) {
                    break;
                }
                s -= 1.0;
            }
            self.nodes[n].sec_pt = Some(s);
            if self.nodes[n].font_pt.is_none() {
                self.nodes[n].font_pt = Some(s);
            }
            return;
        }
        let max = cap(math::round(prim_max));
        let min = math::round(rule_min(self, "primFontSz", max));
        let sec_of = |p: f64| match sec_fixed {
            Some(f) => math::min(math::round(f), p),
            None => math::round(p * 0.78),
        };
        let mut p = max;
        while p > math::max(1.0, min) {
            if fits(self, p, sec_of(p)) {
                break;
            }
            p -= 1.0;
        }
        self.nodes[n].font_pt = Some(p);
        self.nodes[n].sec_pt = Some(sec_of(p));
    }

    // ----- the spTree ---------------------------------------------------------------------------

    /// `emit`.
    pub fn emit(&mut self, style: Option<&X>, colors: Option<&X>, inp: &DiagramInput<'_>, measure: &dyn DiagramMeasure) -> X {
        let qs = style_lbls(style);
        let cs = style_lbls(colors);
        let family = inp.minor_font;
        let line_k = measure.line_factor(family);
        let all: Vec<NId> = (0..self.nodes.len()).collect();
        for &n in &all {
            if self.alg_type(n) == "conn" {
                self.layout_conn(n);
            }
        }
        // z-order: each shape enters at the position (current size + zOrderOff)
        let mut order: Vec<NId> = Vec::new();
        for &n in &all {
            let Some(sh) = self.nodes[n].shape else { continue };
            let ty = sh.attr("type");
            if ty.is_none_or(|t| t.is_empty() || t == "none") {
                continue;
            }
            let off = nzf(parse_int(sh.attr("zOrderOff").unwrap_or("0"), 10));
            let len_f = usize_to_f(order.len());
            let at = math::max(0.0, math::min(len_f, len_f + off));
            order.insert(super::engine::f64_to_usize(at), n);
        }
        // the text presented by each node
        let root_pt = self.model.root;
        let mut texts: FxHashMap<NId, (Vec<PtId>, std::rc::Rc<Vec<TextPara<'a>>>)> = FxHashMap::default();
        for &n in &order {
            let pts = self.pres_texts(n, root_pt);
            if pts.is_empty() {
                continue;
            }
            let paras = self.node_paras(n);
            texts.insert(n, (pts.iter().map(|x| x.0).collect(), paras));
        }
        // a text node without geometry (hideGeom) that presents the same point as a shape: the text goes
        // in the shape (txXfrm = the text node's box) — the application writes one shape
        let mut jobs: Vec<(NId, TextJob<'a>)> = Vec::new();
        let mut jobs_ix: FxHashMap<NId, usize> = FxHashMap::default();
        let mut merged: FxHashSet<NId> = FxHashSet::default();
        // a hidden node WITHOUT a text algorithm (rootConnector…) carries no text
        for &n in &order {
            if self.hidden(n) && self.alg_type(n) != "tx" && texts.contains_key(&n) {
                texts.remove(&n);
            }
        }
        macro_rules! set_job {
            ($k:expr, $job:expr) => {{
                let k: NId = $k;
                let job = $job;
                match jobs_ix.get(&k) {
                    Some(&i) => jobs[i] = (k, job),
                    None => {
                        jobs_ix.insert(k, jobs.len());
                        jobs.push((k, job));
                    }
                }
            }};
        }
        for &n in &order {
            let Some(t) = texts.get(&n) else { continue };
            if !self.hidden(n) {
                continue;
            }
            let t0 = t.0[0];
            let t_paras = t.1.clone();
            // the partner shape cannot be another text node (tx) with its own text
            let same = |this: &Self, k: NId| -> bool {
                k != n
                    && !this.hidden(k)
                    && ((texts.get(&k).is_some_and(|tk| tk.0[0] == t0) && this.alg_type(k) != "tx")
                        || (!texts.contains_key(&k) && this.nodes[k].pt == t0 && this.alg_type(k) == "conn"))
            };
            let par = self.nodes[n].parent;
            let partner: Option<NId> = par
                .filter(|pp| order.contains(pp) && same(self, *pp))
                .or_else(|| order.iter().copied().find(|&k| self.nodes[k].parent == par && same(self, k)))
                .or_else(|| order.iter().copied().find(|&k| same(self, k)));
            let Some(partner) = partner else { continue };
            merged.insert(n);
            let pr = self.geom_of(partner).rot;
            let rot = self.text_rot(n, pr);
            if self.nodes[partner].conn.is_some() {
                let bx = self.geom_text_box(partner);
                set_job!(partner, TextJob { tn: n, paras: t_paras, bx, rot });
            } else {
                let nd = &self.nodes[n];
                let bx = (nd.x, nd.y, nd.w, nd.h);
                set_job!(partner, TextJob { tn: n, paras: t_paras, bx, rot });
            }
        }
        for &n in &order {
            if jobs_ix.contains_key(&n) || merged.contains(&n) {
                continue;
            }
            let Some(t) = texts.get(&n) else { continue };
            let t_paras = t.1.clone();
            let bx = if self.hidden(n) {
                let nd = &self.nodes[n];
                (nd.x, nd.y, nd.w, nd.h)
            } else {
                self.geom_text_box(n)
            };
            let rot = self.text_rot(n, self.geom_of(n).rot);
            set_job!(n, TextJob { tn: n, paras: t_paras, bx, rot });
        }
        // font size: the largest that fits (primFontSz/secFontSz rules), then equality by group
        for ix in 0..jobs.len() {
            let Some((_, j)) = jobs.get(ix) else { continue };
            let job = TextJob { tn: j.tn, paras: j.paras.clone(), bx: j.bx, rot: j.rot };
            self.fit_text(&job, measure, family, line_k);
        }
        for key in ["primFontSz", "secFontSz"] {
            let mut groups: FxHashMap<String, Vec<NId>> = FxHashMap::default();
            let mut gorder: Vec<String> = Vec::new();
            for (_, job) in &jobs {
                // explicit equality (op="equ") or a font rule of an ancestor that holds for all the targets
                let tn = job.tn;
                let g = self.nodes[tn]
                    .equ
                    .get(key)
                    .cloned()
                    .or_else(|| if key == "primFontSz" { self.rules_for(tn).1 } else { self.nodes[tn].equ.get("primFontSz").cloned() });
                let have = if key == "primFontSz" { self.nodes[tn].font_pt.is_some() } else { self.nodes[tn].sec_pt.is_some() };
                if let Some(g) = g.filter(|g| !g.is_empty())
                    && have
                {
                    if !groups.contains_key(&g) {
                        gorder.push(g.clone());
                    }
                    groups.entry(g).or_default().push(tn);
                }
            }
            for g in gorder {
                let Some(list) = groups.get(&g) else { continue };
                let vals: Vec<f64> = list
                    .iter()
                    .map(|&nn| if key == "primFontSz" { self.nodes[nn].font_pt } else { self.nodes[nn].sec_pt }.unwrap_or(f64::NAN))
                    .collect();
                let m = math::min_n(&vals);
                for &nn in list {
                    if key == "primFontSz" {
                        self.nodes[nn].font_pt = Some(m);
                    } else {
                        self.nodes[nn].sec_pt = Some(m);
                    }
                }
            }
        }
        for (_, job) in &jobs {
            let n = job.tn;
            let l = self.nodes[n].lte.get("primFontSz").cloned();
            if let Some((Some(r), _, fct)) = l
                && let Some(rf) = self.nodes[r].font_pt
                && let Some(nf) = self.nodes[n].font_pt
            {
                self.nodes[n].font_pt = Some(math::min(nf, math::floor(rf * fct)));
            }
        }

        let mut out: Vec<XC> = Vec::new();
        for &n in &order {
            if merged.contains(&n) {
                continue;
            }
            let Some(ty) = self.nodes[n].shape.and_then(|s| s.attr("type")) else { continue };
            let hide = self.hidden(n);
            let job = jobs_ix.get(&n).map(|&i| &jobs[i].1);
            if hide && job.is_none() {
                continue;
            }
            if self.nodes[n].conn.is_none() && self.nodes[n].w <= 0.0 && self.nodes[n].h <= 0.0 {
                continue;
            }
            if self.alg_type(n) == "conn" && self.nodes[n].conn.is_none() {
                continue;
            }
            out.push(XC::E(self.shape_xml(n, ty, hide, job, &qs, &cs, inp)));
        }
        let mut kids: Vec<XC> = vec![
            XC::E(mk(
                "dsp:nvGrpSpPr",
                &[],
                vec![XC::E(mk("dsp:cNvPr", &[("id", "0"), ("name", "")], vec![])), XC::E(mk("dsp:cNvGrpSpPr", &[], vec![]))],
            )),
            XC::E(mk("dsp:grpSpPr", &[], vec![])),
        ];
        kids.extend(out);
        mk("dsp:spTree", &[], kids)
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_xml(
        &self,
        n: NId,
        ty: &str,
        hide: bool,
        job: Option<&TextJob<'a>>,
        qs: &FxHashMap<String, &X>,
        cs: &FxHashMap<String, &X>,
        inp: &DiagramInput<'_>,
    ) -> X {
        let nd = &self.nodes[n];
        let lbl = nd.style_lbl.as_deref().unwrap_or("node1");
        let q = qs.get(lbl).copied();
        let c = cs.get(lbl).copied();
        let idx = nd.style_idx;
        let cnt = if nd.style_cnt == 0 { 1 } else { nd.style_cnt };
        let resolve = inp.resolve_color;
        let fill_c = pick_color(c.and_then(|c| c.first("dgm:fillClrLst")), idx, cnt, resolve);
        let line_c = pick_color(c.and_then(|c| c.first("dgm:linClrLst")), idx, cnt, resolve);
        let eff_c = pick_color(c.and_then(|c| c.first("dgm:effectClrLst")), idx, cnt, resolve);
        let tx_c = pick_color(c.and_then(|c| c.first("dgm:txFillClrLst")), idx, cnt, resolve);
        let st = q.and_then(|q| q.first("dgm:style"));
        let ref_el = |name: &str, color: Option<X>, d_idx: &str| -> X {
            let r = st.and_then(|s| s.first(name));
            let mut base = match r {
                Some(r) => r.clone(),
                None => mk(name, &[("idx", d_idx)], vec![]),
            };
            let own: Option<X> = base.kids().next().cloned();
            base.children = vec![XC::E(color.or(own).unwrap_or_else(|| mk("a:scrgbClr", &[("r", "0"), ("g", "0"), ("b", "0")], vec![])))];
            base
        };
        let g = self.geom_of(n);
        let sw = quarter(g.rot);
        // rotated 90°/270°: the shape has the sides swapped and the same centre as the box
        let (w, h) = if sw { (g.h, g.w) } else { (g.w, g.h) };
        let x = g.x + g.w / 2.0 - w / 2.0;
        let y = g.y + g.h / 2.0 - h / 2.0;
        let mut xfrm_attrs: Vec<(&str, String)> = Vec::new();
        if g.rot != 0.0 && !g.rot.is_nan() {
            xfrm_attrs.push(("rot", jsr(math::round(g.rot * 60000.0))));
        }
        if nd.conn.as_ref().is_some_and(|c| c.flip_h) {
            xfrm_attrs.push(("flipH", "1".to_string()));
        }
        let xfrm = mk_s(
            "a:xfrm",
            xfrm_attrs,
            vec![
                XC::E(mk_s("a:off", vec![("x", jsr(math::round(x))), ("y", jsr(math::round(y)))], vec![])),
                XC::E(mk_s("a:ext", vec![("cx", jsr(math::round(w))), ("cy", jsr(math::round(h)))], vec![])),
            ],
        );
        let geom = match nd.conn.as_ref().and_then(|c| c.path.as_ref()) {
            Some(path) => mk(
                "a:custGeom",
                &[],
                vec![
                    XC::E(mk("a:avLst", &[], vec![])),
                    XC::E(mk("a:gdLst", &[], vec![])),
                    XC::E(mk("a:ahLst", &[], vec![])),
                    XC::E(mk("a:cxnLst", &[], vec![])),
                    XC::E(mk("a:rect", &[("l", "0"), ("t", "0"), ("r", "0"), ("b", "0")], vec![])),
                    XC::E(mk("a:pathLst", &[], vec![XC::E(path.clone())])),
                ],
            ),
            None => mk_s(
                "a:prstGeom",
                vec![("prst", g.prst.clone())],
                vec![XC::E(mk(
                    "a:avLst",
                    &[],
                    g.adj
                        .iter()
                        .map(|(nm, v)| {
                            XC::E(mk_s("a:gd", vec![("name", nm.clone()), ("fmla", format!("val {}", to_js_string(*v)))], vec![]))
                        })
                        .collect(),
                ))],
            ),
        };
        let mut sp_pr_kids: Vec<X> = vec![xfrm, geom];
        let is_line = nd.conn.as_ref().is_some_and(|c| c.line);
        if hide {
            sp_pr_kids.push(mk("a:noFill", &[], vec![]));
            sp_pr_kids.push(mk("a:ln", &[], vec![XC::E(mk("a:noFill", &[], vec![]))]));
        } else {
            if is_line {
                sp_pr_kids.push(mk("a:noFill", &[], vec![]));
                let ends = line_ends(&params(nd.alg));
                if !ends.is_empty() {
                    sp_pr_kids.push(mk("a:ln", &[], ends.into_iter().map(XC::E).collect()));
                }
            } else if let Some(fc) = &fill_c
                && fc
                    .kids()
                    .any(|m| m.name == "a:alpha" || m.name == "a:alphaMod" || (m.name == "a:alphaOff" && m.attr("val") != Some("0")))
            {
                // colour with transparency (vennNode1 of radial3/venn: alpha 50%): the application writes
                // the explicit fill in the spPr — through the fillRef the reader would lose the alpha
                let fr = st.and_then(|s| s.first("a:fillRef"));
                if fr.and_then(|f| f.attr("idx")).unwrap_or("1") == "1" {
                    sp_pr_kids.push(mk("a:solidFill", &[], vec![XC::E(fc.clone())]));
                }
            }
            // the user's formatting (data point and presentation point) over the style
            let own_lists: [Option<&X>; 2] = [self.pts[nd.pt].sp_pr, nd.pres.and_then(|p| self.pts[p].sp_pr)];
            for own in own_lists.into_iter().flatten() {
                for k in own.kids() {
                    if k.name == "a:xfrm" || k.name == "a:prstGeom" || k.name == "a:custGeom" {
                        continue;
                    }
                    let i = sp_pr_kids.iter().position(|e| e.name == k.name || (e.name.ends_with("Fill") && k.name.ends_with("Fill")));
                    if let Some(i) = i
                        && i >= 2
                    {
                        sp_pr_kids.remove(i);
                    }
                    sp_pr_kids.push(k.clone());
                }
            }
        }
        let style_el = mk(
            "dsp:style",
            &[],
            vec![
                XC::E(ref_el("a:lnRef", line_c, "2")),
                XC::E(ref_el("a:fillRef", if is_line { None } else { fill_c }, "1")),
                XC::E(ref_el("a:effectRef", eff_c, "0")),
                XC::E(ref_el("a:fontRef", tx_c, "minor")),
            ],
        );
        let mut sp: Vec<XC> = vec![
            XC::E(mk(
                "dsp:nvSpPr",
                &[],
                vec![XC::E(mk("dsp:cNvPr", &[("id", "0"), ("name", "")], vec![])), XC::E(mk("dsp:cNvSpPr", &[], vec![]))],
            )),
            XC::E(mk("dsp:spPr", &[], sp_pr_kids.into_iter().map(XC::E).collect())),
            XC::E(style_el),
        ];
        if let Some(job) = job {
            sp.extend(self.text_xml(job, g.rot).into_iter().map(XC::E));
        }
        let model_id = match nd.pres {
            Some(p) => self.pts[p].id.clone(),
            None => format!("{{dgm-{}}}", nd.uid),
        };
        let _ = ty;
        mk_s("dsp:sp", vec![("modelId", model_id)], sp)
    }

    fn text_xml(&self, job: &TextJob<'a>, shape_rot: f64) -> Vec<X> {
        let n = job.tn;
        let nd = &self.nodes[n];
        let paras = &job.paras;
        let sz = nd.font_pt.unwrap_or(65.0);
        let sec = nd.sec_pt.unwrap_or_else(|| math::round(sz * 0.78));
        let (mp, ms) = marg_fonts(paras, sz, sec);
        let m = |k: &str| jsr(math::round(self.margin(n, k, mp, ms)));
        let p = params(nd.alg);
        let has_ch = paras.iter().any(|tp| tp.level > 0.0);
        let anchor = (if has_ch { p.ag("txAnchorVertCh") } else { None }).or_else(|| p.ag("txAnchorVert")).unwrap_or("mid");
        let body_pr = mk_s(
            "a:bodyPr",
            vec![
                ("spcFirstLastPara", "0".to_string()),
                ("vert", "horz".to_string()),
                ("wrap", "square".to_string()),
                ("lIns", m("lMarg")),
                ("tIns", m("tMarg")),
                ("rIns", m("rMarg")),
                ("bIns", m("bMarg")),
                ("numCol", "1".to_string()),
                ("spcCol", "1270".to_string()),
                (
                    "anchor",
                    if anchor == "t" {
                        "t"
                    } else if anchor == "b" {
                        "b"
                    } else {
                        "ctr"
                    }
                    .to_string(),
                ),
                ("anchorCtr", "0".to_string()),
            ],
            vec![XC::E(mk("a:noAutofit", &[], vec![]))],
        );
        let par_algn = p.ag("parTxLTRAlign").unwrap_or(if has_ch { "l" } else { "ctr" });
        let ch_algn = p.ag("shpTxLTRAlignCh").unwrap_or("l");
        let bul = self.var_of(n, "bulletEnabled");
        let bullets = bul == "1" || bul == "true";
        let nzi = |v: f64| if v == 0.0 || v.is_nan() { 1.0 } else { v };
        let st_bullet = nzi(parse_int(p.ag("stBulletLvl").unwrap_or("1"), 10));
        let mut ps: Vec<XC> = Vec::new();
        for tp in paras.iter() {
            let src = tp.p;
            let level = tp.level;
            let size = tp.csz.unwrap_or(if level == 0.0 { sz } else { sec });
            let lvl = math::min(8.0, level);
            let bu = bullets && level >= st_bullet;
            let ind = if bu { bullet_mar_l(size) } else { 0.0 };
            // paragraph properties recorded in the data (the user centred it, removed the spacing…) win over the
            // layout's — gdx-265011: algn="ctr" in the list1 parentText
            let user = src.and_then(|s| s.first("a:pPr"));
            let uk = |names: &[&str]| -> Vec<XC> {
                user.map(|u| u.kids().filter(|c| names.contains(&c.name.as_str())).map(|c| XC::E(c.clone())).collect()).unwrap_or_default()
            };
            let one = |name: &str, dflt: X| -> XC { XC::E(user.and_then(|u| u.first(name)).cloned().unwrap_or(dflt)) };
            let ubu = uk(&["a:buNone", "a:buAutoNum", "a:buChar", "a:buBlip"]);
            let mut attrs = Attrs::default();
            attrs.set("marL", &jsr(ind * math::max(1.0, level - st_bullet + 1.0) * if bu { 1.0 } else { 0.0 }));
            attrs.set("lvl", &jsr(lvl));
            attrs.set("indent", &jsr(-ind));
            attrs.set(
                "algn",
                if level != 0.0 {
                    if ch_algn == "r" {
                        "r"
                    } else if ch_algn == "ctr" {
                        "ctr"
                    } else {
                        "l"
                    }
                } else if par_algn == "l" {
                    "l"
                } else if par_algn == "r" {
                    "r"
                } else {
                    "ctr"
                },
            );
            attrs.set("defTabSz", &jsr(math::round(size * 3.5 * EMU_PT)));
            if let Some(u) = user {
                for (k, v) in &u.attrs {
                    if k != "lvl" {
                        attrs.set(k, v);
                    }
                }
            }
            let mut kids: Vec<XC> = vec![
                one("a:lnSpc", mk("a:lnSpc", &[], vec![XC::E(mk("a:spcPct", &[("val", "90000")], vec![]))])),
                one("a:spcBef", mk("a:spcBef", &[], vec![XC::E(mk("a:spcPct", &[("val", "0")], vec![]))])),
                one(
                    "a:spcAft",
                    mk("a:spcAft", &[], vec![XC::E(mk("a:spcPct", &[("val", if level != 0.0 { "15000" } else { "35000" })], vec![]))]),
                ),
            ];
            kids.extend(uk(&["a:buClrTx", "a:buClr", "a:buSzTx", "a:buSzPct", "a:buSzPts", "a:buFontTx", "a:buFont"]));
            if let Some(f) = ubu.first() {
                kids.push(f.clone());
            } else {
                kids.push(XC::E(if bu { mk("a:buChar", &[("char", "\u{2022}")], vec![]) } else { mk("a:buNone", &[], vec![]) }));
            }
            kids.extend(uk(&["a:tabLst", "a:defRPr"]));
            let p_pr = X { name: "a:pPr".to_string(), attrs: attrs.0, children: kids };
            let mut runs: Vec<XC> = Vec::new();
            let mut end_rpr: Option<&X> = None;
            if let Some(src) = src {
                for r in src.kids() {
                    if r.name == "a:r" || r.name == "a:fld" {
                        let mut rr = r.clone();
                        let r_pr = rr.first("a:rPr").cloned();
                        // custT: each run keeps the recorded size
                        let own = if tp.csz.is_some() { r_pr.as_ref().and_then(|x| x.attr("sz")).map(str::to_string) } else { None };
                        let mut a = Attrs::default();
                        a.set("lang", "en-US");
                        if let Some(rp) = &r_pr {
                            for (k, v) in &rp.attrs {
                                a.set(k, v);
                            }
                        }
                        a.set("sz", &own.unwrap_or_else(|| jsr(math::round(size * 100.0))));
                        a.set("kern", r_pr.as_ref().and_then(|x| x.attr("kern")).unwrap_or("1200"));
                        let nr = X { name: "a:rPr".to_string(), attrs: a.0, children: r_pr.map(|x| x.children).unwrap_or_default() };
                        rr.children.retain(|k| match k {
                            XC::T(_) => true,
                            XC::E(e) => e.name != "a:rPr",
                        });
                        rr.children.insert(0, XC::E(nr));
                        runs.push(XC::E(rr));
                    } else if r.name == "a:br" {
                        runs.push(XC::E(r.clone()));
                    } else if r.name == "a:endParaRPr" {
                        end_rpr = Some(r);
                    }
                }
            }
            let mut ea = Attrs::default();
            ea.set("lang", "en-US");
            if let Some(e) = end_rpr {
                for (k, v) in &e.attrs {
                    ea.set(k, v);
                }
            }
            ea.set("sz", &jsr(math::round(size * 100.0)));
            ea.set("kern", "1200");
            let mut pk: Vec<XC> = vec![XC::E(p_pr)];
            pk.extend(runs);
            pk.push(XC::E(X { name: "a:endParaRPr".to_string(), attrs: ea.0, children: vec![] }));
            ps.push(XC::E(mk("a:p", &[], pk)));
        }
        let (bx, by, bw, bh) = job.bx;
        // the reader adds the shape's rotation to txXfrm's: writes the difference
        let rel = norm360(job.rot - shape_rot);
        let qr = quarter(job.rot);
        // text rotated 90°: the recorded box has the sides swapped (rotates around the centre)
        let (tw, th) = if qr { (bh, bw) } else { (bw, bh) };
        let tx = bx + bw / 2.0 - tw / 2.0;
        let ty = by + bh / 2.0 - th / 2.0;
        let mut body_kids: Vec<XC> = vec![XC::E(body_pr), XC::E(mk("a:lstStyle", &[], vec![]))];
        body_kids.extend(ps);
        let tx_attrs: Vec<(&str, String)> = if rel != 0.0 && !rel.is_nan() {
            vec![("rot", jsr(math::round((if rel > 180.0 { rel - 360.0 } else { rel }) * 60000.0)))]
        } else {
            vec![]
        };
        vec![
            mk("dsp:txBody", &[], body_kids),
            mk_s(
                "dsp:txXfrm",
                tx_attrs,
                vec![
                    XC::E(mk_s("a:off", vec![("x", jsr(math::round(tx))), ("y", jsr(math::round(ty)))], vec![])),
                    XC::E(mk_s("a:ext", vec![("cx", jsr(math::round(tw))), ("cy", jsr(math::round(th)))], vec![])),
                ],
            ),
        ]
    }
}

fn nz1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

/// `String(x)`.
fn jsr(x: f64) -> String {
    to_js_string(x)
}

fn pt_el(x: f64, y: f64) -> X {
    mk_s("a:pt", vec![("x", jsr(math::round(x))), ("y", jsr(math::round(y)))], vec![])
}

/// Arrow heads of the 1D line (endSty default = arrow; begSty default = none).
fn line_ends(p: &Attrs) -> Vec<X> {
    let mut o = Vec::new();
    if p.ag("begSty") == Some("arr") {
        o.push(mk("a:headEnd", &[("type", "arrow")], vec![]));
    }
    if p.ag("endSty").unwrap_or("arr") == "arr" {
        o.push(mk("a:tailEnd", &[("type", "arrow")], vec![]));
    }
    o
}

/// Indent of the bullets: multiples of 4.5 pt by the size (measured), up to 22.5 pt.
fn bullet_mar_l(sz_pt: f64) -> f64 {
    math::min(5.0, math::max(1.0, math::floor((sz_pt * 0.85) / 4.5))) * 57150.0
}

// ---------------------------------------------------------------------------------------------
// colours and style
// ---------------------------------------------------------------------------------------------

/// `styleLbls`.
fn style_lbls(root: Option<&X>) -> FxHashMap<String, &X> {
    let mut m: FxHashMap<String, &X> = FxHashMap::default();
    if let Some(r) = root {
        for e in r.kids_named("dgm:styleLbl") {
            if let Some(nm) = e.attr("name").filter(|s| !s.is_empty()) {
                m.entry(nm.to_string()).or_insert(e);
            }
        }
    }
    m
}

fn hsl_of(hex: &str) -> (f64, f64, f64) {
    let n = parse_int(&hex.replacen('#', "", 1), 16);
    let i = uo_jsnum::conv::to_int32(n);
    let r = f64::from((i >> 16) & 255) / 255.0;
    let g = f64::from((i >> 8) & 255) / 255.0;
    let b = f64::from(i & 255) / 255.0;
    let mx = math::max(math::max(r, g), b);
    let mn = math::min(math::min(r, g), b);
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn with_offsets(mut e: X, dh: f64, ds: f64, dl: f64, da: f64) -> X {
    if matches!(e.name.as_str(), "a:schemeClr" | "a:srgbClr" | "a:sysClr" | "a:prstClr" | "a:scrgbClr" | "a:hslClr") {
        e.children.push(XC::E(mk_s("a:hueOff", vec![("val", jsr(math::round(dh * 60000.0)))], vec![])));
        e.children.push(XC::E(mk_s("a:satOff", vec![("val", jsr(math::round(ds * 100000.0)))], vec![])));
        e.children.push(XC::E(mk_s("a:lumOff", vec![("val", jsr(math::round(dl * 100000.0)))], vec![])));
        e.children.push(XC::E(mk_s("a:alphaOff", vec![("val", jsr(math::round(da * 100000.0)))], vec![])));
    }
    e
}

/// `pickColor`: colour `idx` of a colour list (repeat/cycle/span) — span interpolates in HSL like the
/// application (hueOff/satOff/lumOff/alphaOff).
fn pick_color(lst: Option<&X>, idx: usize, cnt: usize, resolve: Option<&ResolveColor<'_>>) -> Option<X> {
    let lst = lst?;
    let cs: Vec<&X> = lst.kids().filter(|c| c.name.ends_with("Clr")).collect();
    if cs.is_empty() {
        return None;
    }
    let meth = lst.attr("meth").unwrap_or("span");
    let len = cs.len();
    if len == 1 {
        return Some(with_offsets(cs[0].clone(), 0.0, 0.0, 0.0, 0.0));
    }
    if meth == "repeat" {
        return Some(with_offsets(cs[idx % len].clone(), 0.0, 0.0, 0.0, 0.0));
    }
    if meth == "cycle" {
        let per = 2 * len - 2;
        let k = idx % per.max(1);
        let at = if k < len { k } else { per - k };
        return Some(with_offsets((*cs.get(at)?).clone(), 0.0, 0.0, 0.0, 0.0));
    }
    // span: from the first to the last along cnt items (hueDir cw/ccw)
    let t = if cnt > 1 { math::min(1.0, usize_to_f(idx) / (usize_to_f(cnt) - 1.0)) * (usize_to_f(len) - 1.0) } else { 0.0 };
    let i_f = math::min(usize_to_f(len) - 2.0, math::floor(t));
    let u = t - i_f;
    let i = super::engine::f64_to_usize(i_f);
    let a = cs[i];
    let b = cs[i + 1];
    let ca = resolve.and_then(|r| r(a));
    let cb = resolve.and_then(|r| r(b));
    let (Some(ca), Some(cb)) = (ca, cb) else { return Some(with_offsets((if u > 0.999 { b } else { a }).clone(), 0.0, 0.0, 0.0, 0.0)) };
    if u == 0.0 {
        return Some(with_offsets((if u > 0.999 { b } else { a }).clone(), 0.0, 0.0, 0.0, 0.0));
    }
    let ha = hsl_of(&ca);
    let hb = hsl_of(&cb);
    let mut dh = hb.0 - ha.0;
    let cw = lst.attr("hueDir").unwrap_or("cw") == "cw";
    if cw && dh < 0.0 {
        dh += 360.0;
    } else if !cw && dh > 0.0 {
        dh -= 360.0;
    }
    let alpha = |e: &X| -> f64 {
        match e.kids().find(|m| m.name == "a:alpha") {
            Some(al) => parse_int(al.attr("val").unwrap_or("100000"), 10) / 100000.0,
            None => 1.0,
        }
    };
    Some(with_offsets(a.clone(), dh * u, (hb.1 - ha.1) * u, (hb.2 - ha.2) * u, (alpha(b) - alpha(a)) * u))
}

// ---------------------------------------------------------------------------------------------
// the driver
// ---------------------------------------------------------------------------------------------

/// `assignStyles`: presStyleLbl/Idx/Cnt: the node's label (or the default by type/depth) and its order inside the label.
fn assign_styles(eng: &mut Eng<'_>) {
    let mut cnt: FxHashMap<String, usize> = FxHashMap::default();
    for i in 0..eng.nodes.len() {
        if eng.nodes[i].style_lbl.as_deref().is_none_or(str::is_empty) {
            eng.nodes[i].style_lbl = default_lbl(eng, i);
        }
        let Some(lbl) = eng.nodes[i].style_lbl.clone().filter(|l| !l.is_empty()) else { continue };
        let c = cnt.get(&lbl).copied().unwrap_or(0);
        eng.nodes[i].style_idx = c;
        cnt.insert(lbl, c + 1);
    }
    for i in 0..eng.nodes.len() {
        if let Some(lbl) = eng.nodes[i].style_lbl.clone().filter(|l| !l.is_empty()) {
            eng.nodes[i].style_cnt = cnt.get(&lbl).copied().unwrap_or(1);
        }
    }
}

fn default_lbl(eng: &Eng<'_>, i: usize) -> Option<String> {
    let n = &eng.nodes[i];
    if n.shape.and_then(|s| s.attr("type")).is_none_or(|t| t.is_empty() || t == "none") {
        // inherits from the parent with a label (text nodes without geometry of their own)
        return None;
    }
    let pt = &eng.pts[n.pt];
    let d = pt.depth.clamp(1, 4);
    let dim = if params(n.alg).ag("dim") == Some("1D") { "1D" } else { "2D" };
    match pt.ty.as_str() {
        "sibTrans" => Some(format!("sibTrans{dim}1")),
        "parTrans" => Some(format!("parChTrans{dim}{d}")),
        "asst" => Some(format!("asst{d}")),
        "node" => Some(format!("node{d}")),
        _ => None,
    }
}

/// `layoutDiagram` (`diagram-layout.ts:1744`): the generated `dsp:spTree`, or `None`.
pub fn layout_diagram(inp: &DiagramInput<'_>) -> Option<X> {
    let data = norm_ns(inp.data);
    let layout = norm_ns(inp.layout);
    let mut pts = Vec::new();
    let model = read_model(&data, &mut pts)?;
    let root_def = layout.first("dgm:layoutNode")?;
    let root_pt = model.root;
    let mut eng = Eng::new(pts, model);
    eng.index(&layout, 0);
    let root = eng.build_node(root_def, root_pt, None, 0)?;
    assign_styles(&mut eng);
    let family = inp.minor_font;
    let default_measure = super::metrics::ApproxMeasure;
    let measure: &dyn DiagramMeasure = inp.measure.unwrap_or(&default_measure);
    eng.fit = Some(FitCtx { measure, family, line_k: super::metrics::line_factor(family), root: root_pt });
    eng.scale_budget = 20000;
    eng.root = root;
    eng.cx = inp.cx;
    eng.cy = inp.cy;
    eng.relayout();
    // rules: shrinks the font / grows the shapes until the text fits, and redoes the layout
    eng.run_text_rules();
    let style = inp.style.map(norm_ns);
    let colors = inp.colors.map(norm_ns);
    Some(eng.emit(style.as_ref(), colors.as_ref(), inp, measure))
}

/// `supported` (`diagram-layout.ts:1501`): only layouts whose algorithms the engine implements.
pub fn supported(layout: &X, depth: usize) -> bool {
    if depth > 200 {
        return false;
    }
    for c in layout.kids() {
        if c.name.ends_with(":alg")
            && !matches!(
                c.attr("type").unwrap_or(""),
                "composite" | "lin" | "snake" | "tx" | "sp" | "conn" | "hierRoot" | "hierChild" | "cycle" | "pyra"
            )
        {
            return false;
        }
        if !supported(c, depth + 1) {
            return false;
        }
    }
    true
}
