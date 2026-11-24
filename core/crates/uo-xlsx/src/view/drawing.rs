//! The drawings of the viewport (frontier C4b): `paintDrawings` (`universal-sheet.ts:3569`), `paintAnchoredChart` (3617),
//! `paintChartsheet` (3633) and the grid bridge of `drawings.ts` (anchor → px, the object re-placed at its anchor, the workbook
//! theme as a DrawingML theme, Excel's text defaults, form controls). Shapes are read and emitted by `uo-dml-render`
//! (`read_drawing_elements` with the `xdr:` prefix, `emit_shapes_into`), charts by `uo-charts` through [`super::chart`], each in
//! a `uo-dml-render` context whose list is the viewport's own (no copy, no splice).

use std::cell::RefCell;
use std::sync::Arc;

use uo_dml_render::emit::{Cx, EmitCaches, EmitParams, Env, emit_host_chart, emit_shapes_into};
use uo_dml_render::model::Shape;
use uo_dml_render::source::{ElementRef, ShapePrefix};
use uo_dml_render::text::{MeasureReq, MeasureTable};
use uo_dml_render::text_style::{LvlRun, TextStyleContext, TextStyleLevel, apply_shape_text_cascade};
use uo_dml_render::theme::{JsRecord, Theme as DmlTheme, ThemeFonts};
use uo_dml_render::{CssColor, Limits, SlidesRules};
use uo_jsnum::math;
use uo_render::slides::{ArcOp, Pair, PathOp};

use super::chart::{ChartCache, attr_escape, sheet_profile, text_escape};
use super::region::Rg;
use super::source::{BookSource, SrcCache};
use crate::book::Book;
use crate::read::model::{AnchorKind, DrawingAnchor, DrawingControl, DrawingObject, XChild, XEl};

/// EMU per px at 96 dpi (zoom 100%).
pub const EMU_PER_PX: f64 = 9525.0;
/// Objects whose read shapes are kept (M5; the TS `parsed` WeakMap holds one entry per object).
pub const MAX_PARSED: usize = 256;
/// The face tag of the charts (`Cx::face_tag`): the shell's font stack of a chart face is `paintChart`'s
/// (`'F', Calibri, 'Liberation Sans', Arial, sans-serif`), not the shapes' (C4b-3).
pub const CHART_FACE_TAG: &str = "\u{1}";

/// A rectangle in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// What the viewer keeps for the drawings of a workbook.
pub struct Drawings {
    /// The parse cache and the media table of the package.
    pub src: RefCell<SrcCache>,
    /// The two-phase measurement table of the shapes and charts (the shell answers it; `uo-dml-render`'s protocol).
    pub table: MeasureTable,
    /// The generated-bitmap caches of the emission.
    pub caches: EmitCaches,
    /// The questions of the last frame.
    pub pending: Vec<MeasureReq>,
    /// The PowerPoint-over-TS switches (C4b-2).
    pub rules: SlidesRules,
    theme: Option<Arc<DmlTheme>>,
    parsed: Vec<Parsed>,
    charts: ChartCache,
    /// sheets whose objects were read once at their file `a:xfrm` (`prepareDrawings`)
    prepared: Vec<usize>,
}

struct Parsed {
    sheet: usize,
    obj: usize,
    key: Option<[u64; 4]>,
    shapes: Arc<Vec<Shape>>,
}

impl Default for Drawings {
    fn default() -> Self {
        Drawings {
            src: RefCell::new(SrcCache::default()),
            table: MeasureTable::default(),
            caches: EmitCaches::new(),
            pending: Vec::new(),
            rules: SlidesRules::default(),
            theme: None,
            parsed: Vec::new(),
            charts: ChartCache::default(),
            prepared: Vec::new(),
        }
    }
}

impl Drawings {
    /// Drops every cache (M1): parsed objects, chart models, generated bitmaps, the parse cache.
    pub fn clear_caches(&mut self) {
        self.parsed = Vec::new();
        self.charts.clear();
        self.caches.clear();
        self.src.borrow_mut().clear_xml();
        self.prepared = Vec::new();
        self.theme = None;
    }

    /// Objects and chart models held (M5 tests).
    #[must_use]
    pub fn sizes(&self) -> (usize, usize) {
        (self.parsed.len(), self.charts.len())
    }

    fn need(&mut self, reqs: Vec<MeasureReq>) {
        for r in reqs {
            if !self.pending.contains(&r) {
                self.pending.push(r);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ anchors

/// `anchorRect`: the anchor in px (zoom applied, no scroll). An offset never passes the size of its cell (a hidden column is 0);
/// a NEGATIVE one counts as it is.
#[must_use]
pub fn anchor_rect(a: &DrawingAnchor, edge_x: &dyn Fn(f64) -> f64, edge_y: &dyn Fn(f64) -> f64, zoom: f64) -> Rect {
    let k = zoom / EMU_PER_PX;
    if a.kind == Some(AnchorKind::Absolute) {
        let (px, py) = a.pos.unwrap_or((0.0, 0.0));
        let (cx, cy) = a.ext.unwrap_or((0.0, 0.0));
        return Rect { x: px * k, y: py * k, w: cx * k, h: cy * k };
    }
    let at = |edge: &dyn Fn(f64) -> f64, i: f64, off: f64| -> f64 {
        let s = edge(i);
        s + math::min(off * k, math::max(0.0, edge(i + 1.0) - s))
    };
    let x = at(edge_x, a.from.col, a.from.col_off);
    let y = at(edge_y, a.from.row, a.from.row_off);
    if a.kind == Some(AnchorKind::OneCell) {
        let (cx, cy) = a.ext.unwrap_or((0.0, 0.0));
        return Rect { x, y, w: cx * k, h: cy * k };
    }
    Rect { x, y, w: at(edge_x, a.to.col, a.to.col_off) - x, h: at(edge_y, a.to.row, a.to.row_off) - y }
}

/// Where the object's `a:xfrm` lives: (owner element index path, name of the xfrm).
fn xfrm_holder(el: &XEl) -> Option<(Option<usize>, &'static str)> {
    let child = |name: &str| el.children.iter().position(|c| matches!(c, XChild::El(x) if x.name == name));
    match el.name.as_str() {
        "xdr:grpSp" => Some((Some(child("xdr:grpSpPr")?), "a:xfrm")),
        "xdr:graphicFrame" => Some((None, "xdr:xfrm")),
        _ => Some((Some(child("xdr:spPr")?), "a:xfrm")),
    }
}

fn el_at(el: &XEl, i: Option<usize>) -> Option<&XEl> {
    match i {
        None => Some(el),
        Some(i) => match el.children.get(i) {
            Some(XChild::El(x)) => Some(x),
            _ => None,
        },
    }
}

fn int_attr(e: Option<&XEl>, k: &str) -> f64 {
    let v = uo_jsnum::parse::parse_int(e.and_then(|e| e.attr(k)).unwrap_or("0"), 10);
    if v.is_finite() { v } else { 0.0 }
}

/// The rotation (1/60000 degree) of the object's own `a:xfrm`.
fn top_rot(el: &XEl) -> f64 {
    let Some((h, name)) = xfrm_holder(el) else { return 0.0 };
    int_attr(el_at(el, h).and_then(|h| h.first(name)), "rot")
}

/// `swapsBox`: between 45° and 135° (or 225°–315°) the anchor holds the box of the ROTATED shape — width and height swapped
/// around the same centre (measured in Excel).
#[must_use]
pub fn swaps_box(rot: f64) -> bool {
    let deg = (((rot / 60000.0) % 360.0) + 360.0) % 360.0;
    (45.0..135.0).contains(&deg) || (225.0..315.0).contains(&deg)
}

/// `String(Math.round(x))`.
fn js_round(x: f64) -> String {
    uo_jsnum::format::to_js_string(math::round(x))
}

fn rect_el(name: &str, a: &str, b: &str, x: f64, y: f64) -> XEl {
    XEl { name: name.to_owned(), attrs: vec![(a.to_owned(), js_round(x)), (b.to_owned(), js_round(y))], children: Vec::new() }
}

/// `placeElement`: a copy of the object with its `a:xfrm` at `t` (EMU): the ANCHOR decides position and size (the stored xfrm
/// may differ, measured). In a group the children's space (`chOff/chExt`, absent = the stored off/ext) is carried to the new
/// rectangle and the group's rotation turns around the NEW centre, as in Excel.
fn place_element(el: &XEl, t: Rect) -> Option<XEl> {
    let (hi, name) = xfrm_holder(el)?;
    let holder = el_at(el, hi)?;
    let xi = holder.children.iter().position(|c| matches!(c, XChild::El(x) if x.name == name));
    let xf = xi.and_then(|i| match &holder.children[i] {
        XChild::El(x) => Some(x),
        XChild::Text(_) => None,
    });
    let off = xf.and_then(|x| x.first("a:off"));
    let ext = xf.and_then(|x| x.first("a:ext"));
    let mut kids = vec![XChild::El(rect_el("a:off", "x", "y", t.x, t.y)), XChild::El(rect_el("a:ext", "cx", "cy", t.w, t.h))];
    if el.name == "xdr:grpSp" {
        kids.push(XChild::El(match xf.and_then(|x| x.first("a:chOff")) {
            Some(c) => c.clone(),
            None => rect_el("a:chOff", "x", "y", int_attr(off, "x"), int_attr(off, "y")),
        }));
        kids.push(XChild::El(match xf.and_then(|x| x.first("a:chExt")) {
            Some(c) => c.clone(),
            None => rect_el("a:chExt", "cx", "cy", int_attr(ext, "cx"), int_attr(ext, "cy")),
        }));
    }
    let new_xf = XEl { name: name.to_owned(), attrs: xf.map(|x| x.attrs.clone()).unwrap_or_default(), children: kids };
    let mut new_holder = holder.clone();
    match xi {
        Some(i) => new_holder.children[i] = XChild::El(new_xf),
        None => new_holder.children.insert(0, XChild::El(new_xf)),
    }
    Some(match hi {
        None => new_holder,
        Some(i) => {
            let mut e = el.clone();
            e.children[i] = XChild::El(new_holder);
            e
        }
    })
}

/// An owned element as XML text (the reader parses it with the Slides options).
fn xel_xml(out: &mut String, e: &XEl) {
    out.push('<');
    out.push_str(&e.name);
    for (k, v) in &e.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        attr_escape(out, v);
        out.push('"');
    }
    if e.children.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for c in &e.children {
        match c {
            XChild::El(x) => xel_xml(out, x),
            XChild::Text(t) => text_escape(out, t),
        }
    }
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}

// ------------------------------------------------------------------------------------------------ theme and text defaults

/// `drawingTheme`: the DrawingML theme of the workbook with the default clrMap (a sheet has no master: tx1/bg1 are dk1/lt1);
/// missing slots from the sheet-core theme (which already falls back to the Office palette).
fn drawing_theme(src: &BookSource<'_>, book: &Book) -> DmlTheme {
    let part = crate::read::theme::theme_path(&book.pkg);
    let mut t = uo_dml_render::theme::read_theme(src, &part).unwrap_or_else(|_| DmlTheme { rules: src.rules, ..DmlTheme::default() });
    let st = &book.cx.theme;
    let mut colors: JsRecord<CssColor> = t.colors.clone();
    for (k, v) in &st.by_name {
        if colors.get(k).is_none_or(|c| c.as_str().is_empty()) {
            colors.set(k, CssColor::new(v));
        }
    }
    for (alias, from) in [("tx1", "dk1"), ("bg1", "lt1"), ("tx2", "dk2"), ("bg2", "lt2")] {
        if colors.get(alias).is_none_or(|c| c.as_str().is_empty())
            && let Some(c) = colors.get(from).filter(|c| !c.as_str().is_empty()).cloned()
        {
            colors.set(alias, c);
        }
    }
    let scheme = match &t.scheme {
        Some(s) if !s.is_empty() => s.clone(),
        _ => colors.clone(),
    };
    if t.fonts.is_none() {
        t.fonts = if st.major.is_some() || st.minor.is_some() {
            Some(ThemeFonts { major: st.major.as_deref().map(Box::from), minor: st.minor.as_deref().map(Box::from) })
        } else {
            Some(ThemeFonts { major: Some("Calibri Light".into()), minor: Some("Calibri".into()) })
        };
    }
    t.colors = colors;
    t.scheme = Some(scheme);
    t
}

/// `EXCEL_TEXT`: the shape text default of Excel (no `defaultTextStyle`, no master): the theme minor font at 11 pt.
fn excel_text() -> TextStyleContext {
    let mut ctx = TextStyleContext::default();
    for l in ctx.default_style.iter_mut() {
        *l = TextStyleLevel {
            run: Some(Box::new(LvlRun { sz: Some(11.0), font: Some("+mn-lt".into()), ..LvlRun::default() })),
            ..TextStyleLevel::default()
        };
    }
    ctx
}

/// `finishShape`: Excel's text defaults, then a run whose colour no layer set is `tx1`.
fn finish_shape(sh: &mut Shape, ctx: &TextStyleContext, theme: &DmlTheme) {
    if sh.tx_body.is_none() {
        return;
    }
    apply_shape_text_cascade(sh, ctx, theme, &[], &[]);
    let tx1 = theme.colors.get("tx1").cloned().unwrap_or_else(|| CssColor::new("#000000"));
    if let Some(tb) = sh.tx_body.as_deref_mut() {
        for p in &mut tb.paragraphs {
            for r in &mut p.runs {
                if r.color.is_none() {
                    r.color = Some(tx1.clone());
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ the frame

/// Runs `f` with a `uo-dml-render` context over the viewport list; its questions go to the pending list.
fn with_cx(g: &mut Rg<'_>, f: impl FnOnce(&mut Cx<'_>, &Book)) {
    let book = g.book;
    let dr = &mut *g.dr;
    let src = BookSource { pkg: &book.pkg, cache: &dr.src, rules: dr.rules };
    let env = Env { src: &src, table: &dr.table, rules: dr.rules };
    let view = g.o.list.view.clone();
    let mut cx = Cx::new(&env, &mut *g.o.res, &mut dr.caches, view);
    std::mem::swap(&mut cx.list, &mut *g.o.list);
    f(&mut cx, book);
    std::mem::swap(&mut cx.list, &mut *g.o.list);
    let needs = cx.take_needs();
    drop(cx);
    dr.need(needs);
}

/// The theme of the drawings (built once).
fn theme_of(g: &mut Rg<'_>) -> Arc<DmlTheme> {
    if let Some(t) = &g.dr.theme {
        return Arc::clone(t);
    }
    let src = BookSource { pkg: &g.book.pkg, cache: &g.dr.src, rules: g.dr.rules };
    let t = Arc::new(drawing_theme(&src, g.book));
    g.dr.theme = Some(Arc::clone(&t));
    t
}

/// `shapesAt`: the shapes of object `oi` of sheet `si` placed at `t` (EMU at 100%; `None` = at its file `a:xfrm`), read again
/// only when the rectangle changes.
fn shapes_at(g: &mut Rg<'_>, si: usize, oi: usize, obj: &DrawingObject, t: Option<Rect>) -> Arc<Vec<Shape>> {
    let key = t.map(|t| [math::round(t.x).to_bits(), math::round(t.y).to_bits(), math::round(t.w).to_bits(), math::round(t.h).to_bits()]);
    if let Some(i) = g.dr.parsed.iter().position(|p| p.sheet == si && p.obj == oi) {
        if g.dr.parsed[i].key == key {
            let p = g.dr.parsed.remove(i);
            let s = Arc::clone(&p.shapes);
            g.dr.parsed.push(p);
            return s;
        }
        g.dr.parsed.remove(i);
    }
    let theme = theme_of(g);
    let placed;
    let el = match t {
        Some(t) => match place_element(&obj.el, t) {
            Some(p) => {
                placed = p;
                &placed
            }
            None => &obj.el,
        },
        None => &obj.el,
    };
    let mut text = String::new();
    xel_xml(&mut text, el);
    let mut shapes = match uo_dml_render::source::parse_xml(text) {
        Ok(doc) => {
            let doc = Arc::new(doc);
            let er = ElementRef { element: doc.root().id(), doc: Arc::clone(&doc) };
            let src = BookSource { pkg: &g.book.pkg, cache: &g.dr.src, rules: g.dr.rules };
            uo_dml_render::read::read_drawing_elements(&src, Limits::default(), &obj.part, &[er], &theme, ShapePrefix::Xdr)
                .ok()
                .and_then(|mut v| v.pop())
                .unwrap_or_default()
        }
        Err(_) => Vec::new(),
    };
    let ctx = excel_text();
    for s in &mut shapes {
        finish_shape(s, &ctx, &theme);
    }
    let shapes = Arc::new(shapes);
    if g.dr.parsed.len() >= MAX_PARSED {
        g.dr.parsed.remove(0);
    }
    g.dr.parsed.push(Parsed { sheet: si, obj: oi, key, shapes: Arc::clone(&shapes) });
    shapes
}

/// `prepareDrawings`: the objects of the sheet read once at their file `a:xfrm`; a metafile picture is rasterized at the size
/// of that first read (`warm(sh.image, picDisplaySize(sh))`: the first use decides, as the TS image cache).
fn prepare(g: &mut Rg<'_>, si: usize) {
    if g.dr.prepared.contains(&si) {
        return;
    }
    let ws = g.ws;
    let before = g.dr.pending.len();
    for (oi, o) in ws.drawings.iter().enumerate() {
        let shapes = shapes_at(g, si, oi, o, None);
        with_cx(g, |cx, _| {
            for sh in shapes.iter() {
                let Some(iref) = sh.paint().image.as_deref() else { continue };
                cx.picture_display = sh.xfrm.as_ref().filter(|x| x.cx > 0.0 && x.cy > 0.0).map(|x| {
                    let (fw, fh) = match sh.paint().src_rect {
                        Some(c) => (math::max(0.05, 1.0 - c.l - c.r), math::max(0.05, 1.0 - c.t - c.b)),
                        None => (1.0, 1.0),
                    };
                    (x.cx / EMU_PER_PX / fw, x.cy / EMU_PER_PX / fh)
                });
                let _ = cx.special_img(iref.media);
                cx.picture_display = None;
            }
        });
    }
    if g.dr.pending.len() == before {
        g.dr.prepared.push(si);
    }
}

/// `paintDrawings`: the drawings of the active sheet — charts, pictures, shapes — in file order (z-order), over the grid.
/// `(sx, sy)` = the quadrant's scroll; `view` = the visible area (px) to skip what is outside; `mirror_w` = the width of an RTL
/// camera render (0 = none).
pub(super) fn paint_drawings(g: &mut Rg<'_>, si: usize, sx: f64, sy: f64, view: Rect, mirror_w: f64) {
    let ws = g.ws;
    let start = g.o.list.items.len();
    if ws.charts.is_empty() && ws.drawings.is_empty() {
        let s = u32::try_from(start).unwrap_or(u32::MAX);
        g.spans.push((s, s));
        return;
    }
    if !ws.drawings.is_empty() {
        prepare(g, si);
    }
    // `[...charts, ...drawings]` stably sorted by `order ?? 0`
    let mut items: Vec<(f64, bool, usize)> = Vec::with_capacity(ws.charts.len() + ws.drawings.len());
    items.extend(ws.charts.iter().enumerate().map(|(i, c)| (c.order.unwrap_or(0.0), true, i)));
    items.extend(ws.drawings.iter().enumerate().map(|(i, o)| (o.order, false, i)));
    items.sort_by(|a, b| (a.0 - b.0).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
    let zoom = g.zoom;
    for (_, is_chart, i) in items {
        let anchor = if is_chart { &ws.charts[i].anchor } else { &ws.drawings[i].anchor };
        let r = {
            let (xs, ys, grid) = (g.xs, g.ys, g.grid);
            let ex = |e: f64| edge(xs, grid.default_col_w, zoom, e);
            let ey = |e: f64| edge(ys, grid.default_row_h, zoom, e);
            anchor_rect(anchor, &ex, &ey, zoom)
        };
        let (x, y) = (r.x - sx, r.y - sy);
        // outside the area (room for rotation/shadow: the larger side)
        let m = math::max(r.w, r.h) + 40.0;
        if x > view.x + view.w + m || y > view.y + view.h + m || x + r.w < view.x - m || y + r.h < view.y - m {
            continue;
        }
        // an RTL camera render: the object goes to the mirrored box, its content not mirrored
        let dx = if mirror_w != 0.0 { mirror_w - 2.0 * x - r.w } else { 0.0 };
        if dx != 0.0 {
            g.o.save();
            g.o.translate(dx, 0.0);
        }
        if is_chart {
            let ch = &ws.charts[i];
            if let Some(part) = ch.part.as_deref() {
                paint_anchored_chart(g, part, Some(&ch.chart_xml), Rect { x, y, w: r.w, h: r.h });
            }
        } else {
            paint_object(g, si, i, r, sx, sy, zoom);
        }
        if dx != 0.0 {
            g.o.restore();
        }
    }
    let end = g.o.list.items.len();
    g.spans.push((u32::try_from(start).unwrap_or(u32::MAX), u32::try_from(end).unwrap_or(u32::MAX)));
}

/// `edgeX`/`edgeY`: the left/top edge (px at the zoom, no scroll) of 0-based column/row `i`, past the grid in default sizes.
fn edge(geo: &crate::grid::Geometry, default_px: f64, zoom: f64, i: f64) -> f64 {
    let n = f64::from(geo.len().saturating_sub(1));
    if i <= n {
        let j = math::max(0.0, i);
        return geo.edge(uo_jsnum::conv::to_uint32(j)).unwrap_or(0.0);
    }
    geo.edge(geo.len().saturating_sub(1)).unwrap_or(0.0) + (i - n) * default_px * zoom
}

/// `paintAnchoredChart`: a chart (`chartN.xml`/`chartExN.xml`) in the anchor rectangle (px on the canvas), with the data of the
/// cells and the chart drawing.
fn paint_anchored_chart(g: &mut Rg<'_>, part: &str, xml: Option<&str>, r: Rect) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let book = g.book;
    let Some(model) = g.dr.charts.get(book, part, || match xml {
        Some(x) => Some(x.to_owned()),
        None => book.pkg.find_text(part).ok().flatten().map(std::borrow::Cow::into_owned).filter(|s| !s.is_empty()),
    }) else {
        return;
    };
    let ppt = g.zoom * 96.0 / 72.0;
    with_cx(g, |cx, _| {
        let rect = uo_charts::emit::Rect { x: r.x, y: r.y, w: r.w, h: r.h };
        let _ = emit_host_chart(cx, &model.space, &model.host, rect, ppt, sheet_profile(), CHART_FACE_TAG);
    });
}

/// `SheetDrawings.paint`: one object at its anchor `r` (px at the zoom, grid coordinates WITHOUT scroll); `(sx, sy)` = the
/// quadrant's scroll. The shapes are in EMU of the sheet at 100%: line widths and fonts scale with the zoom only.
fn paint_object(g: &mut Rg<'_>, si: usize, oi: usize, mut r: Rect, sx: f64, sy: f64, zoom: f64) {
    if !(r.w >= 0.0 && r.h >= 0.0) {
        return;
    }
    let ws = g.ws;
    let obj = &ws.drawings[oi];
    // a form control: Excel draws the control; the label comes from the shape
    if let Some(c) = &obj.control {
        r = paint_control(g, c, Rect { x: r.x - sx, y: r.y - sy, w: r.w, h: r.h }, zoom, r);
    }
    let k = zoom / EMU_PER_PX;
    let mut t = Rect { x: r.x / k, y: r.y / k, w: r.w / k, h: r.h / k };
    let rot = top_rot(&obj.el);
    if rot != 0.0 && swaps_box(rot) {
        t = Rect { x: t.x + (t.w - t.h) / 2.0, y: t.y + (t.h - t.w) / 2.0, w: t.h, h: t.w };
    }
    let shapes = shapes_at(g, si, oi, obj, Some(t));
    if shapes.is_empty() {
        return;
    }
    let p = EmitParams { scale: k, ox: -sx, oy: -sy, slide_w: 0.0, slide_h: 0.0, clip_overflow: true, suppress_text: None };
    // a chart inside a group: the Sheet's chart engine (Excel defaults), in the order of its neighbours
    let mut from = 0usize;
    for (i, s) in shapes.iter().enumerate() {
        let chart = s.extras.as_deref().and_then(|e| e.chart.as_deref());
        if let (Some(ch), Some(xf)) = (chart, s.xfrm.as_ref()) {
            if from < i {
                let batch = &shapes[from..i];
                with_cx(g, |cx, _| emit_shapes_into(cx, batch, &p));
            }
            from = i + 1;
            let part = ch.part.clone();
            paint_anchored_chart(g, &part, None, Rect { x: xf.x * k - sx, y: xf.y * k - sy, w: xf.cx * k, h: xf.cy * k });
        }
    }
    if from < shapes.len() {
        let batch = &shapes[from..];
        with_cx(g, |cx, _| emit_shapes_into(cx, batch, &p));
    }
}

/// `paintControl`: a form control as Excel 365 draws it (measured at 100%): check box and option button of 12 px 4.6 px from
/// the left, centred vertically, outline #333 and the label 18 px to the right; command button #F0F0F0 with a 2 px shadow
/// below/right. `p` = the rectangle on the canvas; returns the rectangle (no scroll, as `r`) of the label.
fn paint_control(g: &mut Rg<'_>, c: &DrawingControl, p: Rect, z: f64, r: Rect) -> Rect {
    let o = &mut g.o;
    o.save();
    o.line_width(math::max(1.0, z));
    o.stroke_style("#333333");
    let t = c.r#type.as_str();
    if t == "CheckBox" || t == "Radio" {
        let s = 12.0 * z;
        let (bx, by) = (p.x + 4.6 * z, p.y + p.h / 2.0 - s / 2.0);
        o.fill_style(if c.checked.as_deref() == Some("Mixed") { "#C0C0C0" } else { "#FFFFFF" });
        o.begin_path();
        if t == "Radio" {
            o.path(&[PathOp::Arc(ArcOp {
                cx: bx + s / 2.0,
                cy: by + s / 2.0,
                r: s / 2.0,
                a0: 0.0,
                a1: std::f64::consts::PI * 2.0,
                ccw: false,
            })]);
        } else {
            o.rect(bx, by, s, s);
        }
        o.fill();
        o.stroke();
        if c.checked.as_deref() == Some("Checked") {
            o.fill_style("#333333");
            o.begin_path();
            if t == "Radio" {
                o.path(&[PathOp::Arc(ArcOp {
                    cx: bx + s / 2.0,
                    cy: by + s / 2.0,
                    r: s / 4.0,
                    a0: 0.0,
                    a1: std::f64::consts::PI * 2.0,
                    ccw: false,
                })]);
                o.fill();
            } else {
                o.line_width(1.5 * z);
                o.path(&[
                    PathOp::MoveTo(Pair { x: bx + 0.22 * s, y: by + 0.5 * s }),
                    PathOp::LineTo(Pair { x: bx + 0.42 * s, y: by + 0.72 * s }),
                    PathOp::LineTo(Pair { x: bx + 0.8 * s, y: by + 0.26 * s }),
                ]);
                o.stroke();
            }
        }
        o.restore();
        return Rect { x: r.x + 18.0 * z, y: r.y, w: math::max(0.0, r.w - 18.0 * z), h: r.h };
    }
    if t == "Button" {
        let (w, h) = (math::max(0.0, p.w - z), math::max(0.0, p.h - z));
        o.fill_style("#F0F0F0");
        o.fill_rect(p.x, p.y, w, h);
        o.fill_style("#C6C6C6");
        o.fill_rect(p.x + w - 2.0 * z, p.y, z, h);
        o.fill_rect(p.x, p.y + h - 2.0 * z, w, z);
        o.fill_style("#999999");
        o.fill_rect(p.x + w - z, p.y, z, h);
        o.fill_rect(p.x, p.y + h - z, w, z);
    } else if t == "GBox" {
        o.stroke_style("#BFBFBF");
        o.stroke_rect(
            math::round(p.x) + 0.5,
            math::round(p.y + 7.0 * z) + 0.5,
            math::max(0.0, math::round(p.w) - 1.0),
            math::max(0.0, math::round(p.h - 7.0 * z) - 1.0),
        );
    }
    o.restore();
    r
}

/// `paintChartsheet`: a chart sheet — no grid nor headers, the chart of the absolute anchor at the view's scale (`zoomScale`,
/// measured: Excel's `Chart.Export` comes out at it) or fitted to the window.
pub(super) fn paint_chartsheet(g: &mut Rg<'_>, w: f64, h: f64) {
    let start = g.o.list.items.len();
    // the white page goes in the span: the capture records `paintChartsheet` whole
    g.o.st.reset();
    g.o.fill_style("#FFFFFF");
    g.o.fill_rect(0.0, 0.0, w, h);
    let ws = g.ws;
    let Some(cs) = ws.chartsheet else { return };
    for ch in &ws.charts {
        let Some((cx, cy)) = ch.anchor.ext else { continue };
        if cx.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || cy.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            continue;
        }
        let (px, py) = ch.anchor.pos.map_or((0.0, 0.0), |(x, y)| (x / EMU_PER_PX, y / EMU_PER_PX));
        let (pw, ph) = (cx / EMU_PER_PX, cy / EMU_PER_PX);
        let k = if cs.zoom_to_fit { math::min(w / (pw + 2.0 * px), h / (ph + 2.0 * py)) } else { cs.zoom / 100.0 * g.zoom };
        let Some(part) = ch.part.as_deref() else { continue };
        let book = g.book;
        let Some(model) = g.dr.charts.get(book, part, || Some(ch.chart_xml.clone())) else { continue };
        with_cx(g, |cx2, _| {
            let rect = uo_charts::emit::Rect { x: px * k, y: py * k, w: pw * k, h: ph * k };
            let _ = emit_host_chart(cx2, &model.space, &model.host, rect, k * 96.0 / 72.0, sheet_profile(), CHART_FACE_TAG);
        });
    }
    let end = g.o.list.items.len();
    g.spans.push((u32::try_from(start).unwrap_or(u32::MAX), u32::try_from(end).unwrap_or(u32::MAX)));
}
