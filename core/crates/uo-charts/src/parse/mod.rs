//! `shared/charts/src/parse.ts`: the host's `<c:chartSpace>` tree → [`ChartSpace`], colours through the
//! host's resolver ([`ChartHost::color`]), in the same order as the TS (the order of the host calls is
//! part of the contract: the parity replay answers them from a log).
//!
//! Notes of the port (record `uo-charts.md`, "C-1"):
//! - `undefined` is `Option`; the TS stores a key only when the value is defined, so every spread
//!   `{ ...a, ...b }` is a field-wise `or` ([`TextProps::over`]).
//! - Numbers go through `uo_jsnum::parse` (`parseInt`, `parseFloat`): NaN flows on where the TS lets it
//!   (`ln w`, `c14:style`, gradient `pos`, shadow `dist`/`dir`/`blurRad`, `bodyPr rot`).
//! - The one recursion (`visit` of the chart drawing's groups) is capped at [`MAX_GROUP_DEPTH`] levels
//!   (the TS would die of the stack on a hostile file; declared).
//! - `cx:chartSpace` (Office 2016) is handed to [`crate::chartex`] (frontier C-2).

pub mod xml;

use uo_jsnum::math::{cos, max, min, sin};
use uo_jsnum::parse::{parse_float, parse_int};
use uo_jsnum::sort::js_sort_by;
use uo_jsnum::text::trim;
use uo_opc::xml::Element;

use crate::host::{BlipKey, ChartHost, ColorSource, FileFormat, HostFonts};
use crate::model::{
    Axis, AxisKind, AxisPos, BarDir, BlipTile, ChartColor, ChartSpace, ChartTitle, CrossBetween, Crosses, DLbl, DLbls, DLblsCommon,
    DataPoint, DataTable, DispBlanksAs, FillRect, FmtCode, FracRect, GradPath, GradStop, Group, GroupKind, Grouping, Insets, LabelPart,
    Legend, LegendPos, LineProps, ManualLayout, Marker, NumFmt, OfPie, Paint, PointMap, PointSet, RichPara, RichRun, Series, Shadow,
    ShapeProps, TextAnchor, TextProps, TickLblPos, TickMark, TileFlip, TrendLabel, Trendline, UpDownBars, UserShape, UserText, View3D,
};
use xml::{descend, first, text, truthy, val_num, val_str};

pub use xml::CHART_PREFIXES;

/// Deepest group nesting of a chart drawing that is followed (the TS recurses without a cap).
pub const MAX_GROUP_DEPTH: u32 = 128;

/// The `ChartHost` of [`parse_paint`]: only the colour resolver (`new Parser({ color })`).
struct ColorOnly<'f> {
    f: &'f dyn Fn(ColorSource<'_>) -> Option<ChartColor>,
}

impl ChartHost for ColorOnly<'_> {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        (self.f)(src)
    }
    fn fonts(&self) -> HostFonts<'_> {
        HostFonts::default()
    }
    fn has_fill_style(&self) -> bool {
        false
    }
    fn fill_style(&self, _idx: u32, _ph_clr: &str) -> Option<Paint> {
        None
    }
    fn blip(&self, _blip: Element<'_>) -> Option<BlipKey> {
        None
    }
    fn file_format(&self) -> Option<FileFormat> {
        None
    }
    fn user_shapes(&self) -> Option<Element<'_>> {
        None
    }
    fn theme_line_widths(&self) -> &[f64] {
        &[]
    }
}

/// `parsePaint(el, color)` (parse.ts:61): the fill chosen among the CHILDREN of `el` (`noFill`,
/// `solidFill`, `gradFill`, `pattFill`; the first that decides), with the given colour resolver. A
/// picture fill is ignored (no host).
pub fn parse_paint(el: Element<'_>, color: &dyn Fn(ColorSource<'_>) -> Option<ChartColor>) -> Option<Paint> {
    let host = ColorOnly { f: color };
    Parser::new(&host).paint(Some(el))
}

/// The same for a single fill element (what the TS hosts get by wrapping it in a synthetic parent:
/// `parsePaint({ name: 'w', children: [el] })`, e.g. an entry of the theme's `fillStyleLst`).
pub fn parse_paint_node(el: Element<'_>, color: &dyn Fn(ColorSource<'_>) -> Option<ChartColor>) -> Option<Paint> {
    let host = ColorOnly { f: color };
    Parser::new(&host).paint_one(el).flatten()
}

const COLOR_TAGS: [&str; 6] = ["a:srgbClr", "a:schemeClr", "a:sysClr", "a:prstClr", "a:scrgbClr", "a:hslClr"];

/// `colorChild` (parse.ts:66).
fn color_child<'d>(el: Option<Element<'d>>) -> Option<Element<'d>> {
    el?.child_elements().find(|c| COLOR_TAGS.contains(&c.name()))
}

/// `alphaOf` (parse.ts:71): the LAST `a:alpha` (measured), clamped; NaN stays NaN.
fn alpha_of(clr: Option<Element<'_>>) -> Option<f64> {
    let a = clr?.children_named("a:alpha").next_back()?.attr("val")?;
    Some(max(0.0, min(1.0, parse_int(a, 10) / 100_000.0)))
}

/// `parseInt(s, 10)`.
fn pi(s: &str) -> f64 {
    parse_int(s, 10)
}

/// `Math.min(…)` of an index to the `u32` the host takes (the argument is positive and integral).
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "x >= 1 and capped at u32::MAX before the cast")]
fn to_u32_sat(x: f64) -> u32 {
    if x >= f64::from(u32::MAX) { u32::MAX } else { x as u32 }
}

/// An index parsed by `parseInt` and already checked to be an integer in `0..100000`.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 <= i < 100000, an integer: exact")]
fn to_usize(i: f64) -> usize {
    i as usize
}

/// `||` of two numbers (`a || b`: zero and NaN are falsy).
fn or_num(a: f64, b: f64) -> f64 {
    if a == 0.0 || a.is_nan() { b } else { a }
}

fn truthy_color(c: Option<ChartColor>) -> Option<ChartColor> {
    c.filter(|c| !c.as_str().is_empty())
}

/// The parser of `parse.ts` (class `Parser`): a host and the rules that depend on who wrote the file.
pub struct Parser<'h> {
    pub(crate) host: &'h dyn ChartHost,
}

impl<'h> Parser<'h> {
    pub(crate) fn new(host: &'h dyn ChartHost) -> Parser<'h> {
        Parser { host }
    }

    /// Office 2007 file (`AppVersion` ≤ 12), see [`ChartHost::file_format`].
    pub(crate) fn v07(&self) -> bool {
        self.host.file_format() == Some(FileFormat::Mso2007)
    }

    /// Office 2010+ file.
    pub(crate) fn modern(&self) -> bool {
        self.host.file_format() == Some(FileFormat::Modern)
    }

    /// CT_Boolean: `<c:x/>` without `val` is true (spec, 2010+); in Office 2007 it is false (measured).
    pub(crate) fn bool_of(&self, el: Option<Element<'_>>, name: &str) -> Option<bool> {
        let c = first(el, name)?;
        match c.attr("val") {
            None => Some(!self.v07()),
            Some(v) => Some(v == "1" || v == "true"),
        }
    }

    pub(crate) fn color(&self, clr: Option<Element<'_>>) -> Option<ChartColor> {
        clr.and_then(|c| self.host.color(ColorSource::Element(c)))
    }

    /// The fill chosen among the children of `el` (`spPr`, `ln`, `rPr`…).
    pub(crate) fn paint(&self, el: Option<Element<'_>>) -> Option<Paint> {
        for c in el?.child_elements() {
            if let Some(r) = self.paint_one(c) {
                return r;
            }
        }
        None
    }

    /// One child of `paint`'s loop: `None` = not a fill element (the loop goes on), `Some(x)` = the
    /// loop returns `x`.
    fn paint_one(&self, c: Element<'_>) -> Option<Option<Paint>> {
        match c.name() {
            "a:noFill" => Some(Some(Paint::None)),
            "a:solidFill" => {
                let clr = color_child(Some(c));
                let Some(col) = truthy_color(self.color(clr)) else { return Some(None) };
                let a = alpha_of(clr);
                Some(Some(Paint::Solid { color: col, alpha: a.filter(|a| *a < 1.0) }))
            }
            "a:gradFill" => Some(self.grad_fill(c)),
            "a:pattFill" => {
                let fg = self.color(color_child(first(Some(c), "a:fgClr"))).unwrap_or_else(|| ChartColor::new("#000000"));
                let bg = self.color(color_child(first(Some(c), "a:bgClr"))).unwrap_or_else(|| ChartColor::new("#FFFFFF"));
                Some(Some(Paint::Patt { fg, bg, prst: c.attr("prst").unwrap_or("pct50").into() }))
            }
            "a:blipFill" => Some(self.blip_fill(c)),
            _ => None,
        }
    }

    fn grad_fill(&self, c: Element<'_>) -> Option<Paint> {
        let mut stops: Vec<GradStop> = Vec::new();
        if let Some(gs) = first(Some(c), "a:gsLst") {
            for s in gs.children_named("a:gs") {
                let clr = color_child(Some(s));
                let Some(col) = truthy_color(self.color(clr)) else { continue };
                let a = alpha_of(clr);
                stops.push(GradStop { pos: pi(s.attr("pos").unwrap_or("0")) / 100_000.0, color: col, alpha: a.filter(|a| *a < 1.0) });
            }
        }
        if stops.is_empty() {
            return None;
        }
        js_sort_by(&mut stops, |a, b| a.pos - b.pos);
        let lin = first(Some(c), "a:lin");
        let path = truthy(first(Some(c), "a:path").and_then(|p| p.attr("path"))).map(|p| match p {
            "circle" => GradPath::Circle,
            "rect" => GradPath::Rect,
            "shape" => GradPath::Shape,
            other => GradPath::Other(other.into()),
        });
        let angle = match lin {
            Some(l) => pi(l.attr("ang").unwrap_or("0")) / 60_000.0,
            None => 90.0,
        };
        Some(Paint::Grad { stops: stops.into_boxed_slice(), angle, path })
    }

    /// `a:blipFill` → picture stretched (`a:stretch`, inset by `a:fillRect`) or tiled (`a:tile`; with
    /// neither, PowerPoint tiles at natural size too, as for shapes, tdf#128596). Only the host resolves
    /// the picture (the rels of the chart part).
    fn blip_fill(&self, bf: Element<'_>) -> Option<Paint> {
        let blip = first(Some(bf), "a:blip");
        let key = self.host.blip(blip?)?;
        let mut alpha = None;
        if let Some(amt) = first(blip, "a:alphaModFix").and_then(|e| e.attr("amt")) {
            let a = pi(amt) / 100_000.0;
            if a.is_finite() && a < 1.0 {
                alpha = Some(max(0.0, a));
            }
        }
        let tile_el = first(Some(bf), "a:tile");
        let stretch = first(Some(bf), "a:stretch");
        let mut tile = None;
        let mut fill_rect = None;
        if tile_el.is_some() || stretch.is_none() {
            let n = |k: &str, d: f64| {
                let x = tile_el.and_then(|t| t.attr(k)).map_or(f64::NAN, pi);
                if x.is_finite() { x } else { d }
            };
            let flip = match tile_el.and_then(|t| t.attr("flip")) {
                Some("x") => Some(TileFlip::X),
                Some("y") => Some(TileFlip::Y),
                Some("xy") => Some(TileFlip::Xy),
                _ => None,
            };
            tile = Some(BlipTile {
                sx: n("sx", 100_000.0) / 100_000.0,
                sy: n("sy", 100_000.0) / 100_000.0,
                tx: n("tx", 0.0) / 12700.0,
                ty: n("ty", 0.0) / 12700.0,
                algn: tile_el.and_then(|t| t.attr("algn")).unwrap_or("tl").into(),
                flip,
            });
        } else {
            let fr = first(stretch, "a:fillRect");
            let v = |k: &str| {
                let x = fr.and_then(|f| f.attr(k)).map_or(0.0, |s| pi(s) / 100_000.0);
                if x.is_finite() { x } else { 0.0 }
            };
            if fr.is_some() && (v("l") != 0.0 || v("t") != 0.0 || v("r") != 0.0 || v("b") != 0.0) {
                fill_rect = Some(FillRect { l: v("l"), t: v("t"), r: v("r"), b: v("b") });
            }
        }
        Some(Paint::Blip { key, tile: tile.map(Box::new), fill_rect: fill_rect.map(Box::new), alpha })
    }

    pub(crate) fn line(&self, ln: Option<Element<'_>>) -> Option<Box<LineProps>> {
        let ln = ln?;
        let paint = self.paint(Some(ln));
        Some(Box::new(LineProps {
            paint,
            w: truthy(ln.attr("w")).map(|w| pi(w) / 12700.0),
            dash: truthy(val_str(Some(ln), "a:prstDash")).map(Into::into),
            cap: truthy(ln.attr("cap")).map(Into::into),
        }))
    }

    pub(crate) fn shape(&self, sp_pr: Option<Element<'_>>) -> ShapeProps {
        let Some(sp_pr) = sp_pr else { return ShapeProps::default() };
        let fill = self.paint(Some(sp_pr));
        let line = self.line(first(Some(sp_pr), "a:ln"));
        let mut out = ShapeProps { fill, line, shadow: None };
        if let Some(sh) = descend(first(Some(sp_pr), "a:effectLst"), "a:outerShdw") {
            let clr = color_child(Some(sh));
            let dist = pi(sh.attr("dist").unwrap_or("0")) / 12700.0;
            let dir = pi(sh.attr("dir").unwrap_or("0")) / 60_000.0 * std::f64::consts::PI / 180.0;
            let color = self.color(clr).unwrap_or_else(|| ChartColor::new("#000000"));
            out.shadow = Some(Box::new(Shadow {
                color,
                alpha: alpha_of(clr).unwrap_or(1.0),
                dx: dist * cos(dir),
                dy: dist * sin(dir),
                blur: pi(sh.attr("blurRad").unwrap_or("0")) / 12700.0,
            }));
        }
        out
    }

    pub(crate) fn run_props(&self, rpr: Option<Element<'_>>) -> TextProps {
        let mut out = TextProps::default();
        let Some(rpr) = rpr else { return out };
        if let Some(sz) = rpr.attr("sz")
            && !sz.is_empty()
            && sz.bytes().all(|b| b.is_ascii_digit())
        {
            out.sz = Some(pi(sz) / 100.0);
        }
        let flag = |v: &str| v == "1" || v == "true";
        if let Some(b) = rpr.attr("b") {
            out.b = Some(flag(b));
        }
        if let Some(i) = rpr.attr("i") {
            out.i = Some(flag(i));
        }
        if let Some(u) = rpr.attr("u") {
            out.u = Some(u != "none");
        }
        match self.paint(Some(rpr)) {
            Some(Paint::Solid { color, alpha }) => {
                out.color = Some(color);
                out.alpha = alpha;
            }
            Some(Paint::Grad { stops, .. }) => out.color = stops.first().map(|s| s.color.clone()),
            _ => {}
        }
        let face = first(Some(rpr), "a:latin").and_then(|l| l.attr("typeface"));
        let face: Option<&str> = match face {
            Some("+mn-lt") => self.host.fonts().minor,
            Some("+mj-lt") => self.host.fonts().major,
            other => other,
        };
        if let Some(f) = truthy(face)
            && !f.starts_with('+')
        {
            out.font = Some(f.into());
        }
        out
    }

    /// `c:txPr` / `c:rich`: the body (rotation) + the first `a:defRPr`.
    pub(crate) fn text_props(&self, tx: Option<Element<'_>>) -> TextProps {
        let Some(tx) = tx else { return TextProps::default() };
        let mut out = TextProps::default();
        let body = first(Some(tx), "a:bodyPr");
        if let Some(rot) = body.and_then(|b| b.attr("rot")) {
            out.rot = Some(pi(rot) / 60_000.0);
        }
        if let Some(vert) = truthy(body.and_then(|b| b.attr("vert")))
            && vert != "horz"
        {
            out.vert = Some(vert.into());
        }
        let p = first(Some(tx), "a:p");
        let def = p.and_then(|p| first(first(Some(p), "a:pPr"), "a:defRPr"));
        out.over(self.run_props(def))
    }

    pub(crate) fn rich(&self, rich: Element<'_>) -> Vec<RichPara> {
        let mut out = Vec::new();
        for p in rich.children_named("a:p") {
            let ppr = first(Some(p), "a:pPr");
            let def = first(ppr, "a:defRPr");
            let mut para = RichPara { runs: Vec::new(), props: self.run_props(def), align: None, no_def_rpr: def.is_none() };
            if let Some(a) = truthy(ppr.and_then(|e| e.attr("algn"))) {
                para.align = Some(a.into());
            }
            for r in p.child_elements() {
                match r.name() {
                    "a:r" | "a:fld" => {
                        let t = first(Some(r), "a:t");
                        let txt = t.map_or_else(String::new, text);
                        para.runs.push(RichRun { text: txt.into(), props: self.run_props(first(Some(r), "a:rPr")) });
                    }
                    "a:br" => para.runs.push(RichRun { text: "\n".into(), props: self.run_props(first(Some(r), "a:rPr")) }),
                    _ => {}
                }
            }
            out.push(para);
        }
        out
    }

    pub(crate) fn layout(&self, el: Option<Element<'_>>) -> Option<ManualLayout> {
        let ml = first(first(el, "c:layout"), "c:manualLayout")?;
        let target = match val_str(Some(ml), "c:layoutTarget") {
            Some("inner") => Some(crate::model::LayoutTarget::Inner),
            Some("outer") => Some(crate::model::LayoutTarget::Outer),
            _ => None,
        };
        let mode = |s: Option<&str>| match s {
            Some("edge") => Some(crate::model::LayoutMode::Edge),
            Some("factor") => Some(crate::model::LayoutMode::Factor),
            _ => None,
        };
        Some(ManualLayout {
            target,
            x_mode: mode(val_str(Some(ml), "c:xMode")),
            y_mode: mode(val_str(Some(ml), "c:yMode")),
            x: val_num(Some(ml), "c:x"),
            y: val_num(Some(ml), "c:y"),
            w: val_num(Some(ml), "c:w"),
            h: val_num(Some(ml), "c:h"),
        })
    }

    pub(crate) fn title(&self, el: Option<Element<'_>>) -> Option<ChartTitle> {
        let el = el?;
        let overlay = self.bool_of(Some(el), "c:overlay").unwrap_or(false);
        let sp = self.shape(first(Some(el), "c:spPr"));
        let mut t = ChartTitle { paras: None, text: None, overlay, layout: None, sp, tx_pr: self.text_props(first(Some(el), "c:txPr")) };
        let tx = first(Some(el), "c:tx");
        if let Some(rich) = first(tx, "c:rich") {
            t.paras = Some(self.rich(rich));
            // rotation/defRPr of the rich body count as txPr
            let base = self.text_props(Some(rich));
            t.tx_pr = std::mem::take(&mut t.tx_pr).over(base);
        } else if tx.is_some()
            && let Some(v) = descend(tx, "c:v")
        {
            t.text = Some(text(v).into());
        }
        t.layout = self.layout(Some(el));
        Some(t)
    }

    pub(crate) fn num_fmt(&self, el: Option<Element<'_>>) -> Option<NumFmt> {
        let nf = first(el, "c:numFmt")?;
        Some(NumFmt {
            code: FmtCode::new(nf.attr("formatCode").unwrap_or("General")),
            linked: matches!(nf.attr("sourceLinked"), Some("1" | "true")),
        })
    }

    pub(crate) fn d_lbls(&self, el: Option<Element<'_>>) -> Option<DLbls> {
        let el = el?;
        let mut d = DLbls { common: self.d_lbl_common(el), points: None };
        for p in el.children_named("c:dLbl") {
            let Some(idx) = val_num(Some(p), "c:idx") else { continue };
            let mut pd = DLbl { common: self.d_lbl_common(p), ..DLbl::default() };
            if let Some(rich) = first(first(Some(p), "c:tx"), "c:rich") {
                let paras = self.rich(rich);
                pd.text =
                    Some(paras.iter().map(|pp| pp.runs.iter().map(|r| &*r.text).collect::<String>()).collect::<Vec<_>>().join("\n").into());
                // fields (a:fld type=CATEGORYNAME/VALUE/…): replaced by the point's value at paint time
                let mut flds: Vec<LabelPart> = Vec::new();
                for (k, pp) in rich.children_named("a:p").enumerate() {
                    if k > 0 {
                        flds.push(LabelPart { text: "\n".into(), fld: None });
                    }
                    for r in pp.child_elements().filter(|r| matches!(r.name(), "a:r" | "a:fld" | "a:br")) {
                        if r.name() == "a:br" {
                            flds.push(LabelPart { text: "\n".into(), fld: None });
                        } else {
                            let t = first(Some(r), "a:t").map_or_else(String::new, text);
                            let fld = if r.name() == "a:fld" { truthy(r.attr("type")).map(Box::<str>::from) } else { None };
                            flds.push(LabelPart { text: t.into(), fld });
                        }
                    }
                }
                if flds.iter().any(|f| f.fld.is_some()) {
                    pd.parts = Some(flds);
                }
                let rp = paras.iter().flat_map(|pp| pp.runs.iter()).find(|r| !trim(&r.text).is_empty()).map(|r| r.props.clone());
                if let Some(rp) = rp
                    && !rp.is_empty()
                {
                    pd.run_props = Some(rp);
                }
            }
            pd.layout = self.layout(Some(p));
            let c15 = descend(first(Some(p), "c:extLst"), "c15:layout");
            let ml = c15.and_then(|c| c.first_child("c:manualLayout"));
            if let (Some(w), Some(h)) = (val_num(ml, "c:w"), val_num(ml, "c:h"))
                && w > 0.0
                && h > 0.0
            {
                pd.size = Some((w, h));
            }
            d.points.get_or_insert_with(PointMap::default).set(idx, pd);
        }
        Some(d)
    }

    fn d_lbl_common(&self, el: Element<'_>) -> DLblsCommon {
        let tx_pr = self.text_props(first(Some(el), "c:txPr"));
        let sp = self.shape(first(Some(el), "c:spPr"));
        let mut d = DLblsCommon { tx_pr, sp, ..DLblsCommon::default() };
        d.delete = self.bool_of(Some(el), "c:delete");
        // 2010+: an absent show* in a dLbls/dLbl shows (measured, syn-a-mso16 #13)
        let b = |k: &str| self.bool_of(Some(el), k).or(if self.modern() { Some(true) } else { None });
        d.show_val = b("c:showVal");
        d.show_cat = b("c:showCatName");
        d.show_ser = b("c:showSerName");
        d.show_pct = b("c:showPercent");
        d.show_bubble = b("c:showBubbleSize");
        d.show_legend_key = b("c:showLegendKey");
        d.pos = truthy(val_str(Some(el), "c:dLblPos")).map(Into::into);
        d.sep = first(Some(el), "c:separator").map(|s| text(s).into());
        d.num_fmt = self.num_fmt(Some(el));
        // leader lines: c:showLeaderLines or the c15 extension (2013+), with its style
        let ext = first(Some(el), "c:extLst");
        let sll = self.bool_of(Some(el), "c:showLeaderLines").unwrap_or(false)
            || descend(ext, "c15:showLeaderLines").is_some_and(|e| e.attr("val") != Some("0"));
        if sll {
            let ll = descend(ext, "c15:leaderLines").or_else(|| first(Some(el), "c:leaderLines"));
            d.leader_lines = Some(self.shape(first(ll, "c:spPr")));
        }
        d
    }

    pub(crate) fn marker(&self, el: Option<Element<'_>>) -> Option<Marker> {
        let el = el?;
        let sp = self.shape(first(Some(el), "c:spPr"));
        Some(Marker { symbol: truthy(val_str(Some(el), "c:symbol")).map(Into::into), size: val_num(Some(el), "c:size"), sp })
    }

    /// The numeric cache of a `c:val`/`c:cat`/… (Ref or Lit).
    pub(crate) fn nums(&self, el: Option<Element<'_>>) -> Option<Nums> {
        let el = el?;
        let cache = el.descend("c:numCache").or_else(|| el.descend("c:numLit"))?;
        let n = min(100_000.0, val_num(Some(cache), "c:ptCount").unwrap_or(0.0));
        let mut vals: Vec<Option<f64>> = Vec::new();
        let mut pt_fmts: Option<Vec<Option<FmtCode>>> = None;
        for pt in cache.children_named("c:pt") {
            let i = pi(pt.attr("idx").unwrap_or("-1"));
            if !(0.0..100_000.0).contains(&i) {
                continue;
            }
            let i = to_usize(i);
            let v = first(Some(pt), "c:v");
            let x = v.map_or(f64::NAN, |v| parse_float(&text(v)));
            while vals.len() < i {
                vals.push(None);
            }
            let value = x.is_finite().then_some(x);
            match vals.get_mut(i) {
                Some(slot) => *slot = value,
                None => vals.push(value),
            }
            if let Some(pf) = truthy(pt.attr("formatCode")) {
                let f = pt_fmts.get_or_insert_with(Vec::new);
                if f.len() <= i {
                    f.resize(i + 1, None);
                }
                if let Some(slot) = f.get_mut(i) {
                    *slot = Some(FmtCode::new(pf));
                }
            }
        }
        while (vals.len() as f64) < n {
            vals.push(None);
        }
        let fmt = first(Some(cache), "c:formatCode").map(|fc| FmtCode::new(&text(fc)));
        Some(Nums { vals, fmt, pt_fmts })
    }

    pub(crate) fn strs(&self, el: Option<Element<'_>>) -> Option<Vec<Box<str>>> {
        let el = el?;
        let cache = el.descend("c:strCache").or_else(|| el.descend("c:strLit"))?;
        Some(self.pt_strings(cache, None))
    }

    /// `ptStrings(cache)`; `count_from` is the element whose `c:ptCount` stands in for a missing one (the
    /// level of a `multiLvlStrCache` borrows its parent's: the TS appends it to the level's children).
    fn pt_strings(&self, cache: Element<'_>, count_from: Option<Element<'_>>) -> Vec<Box<str>> {
        let pc = cache.first_child("c:ptCount").or_else(|| count_from.and_then(|c| c.first_child("c:ptCount")));
        let n = min(100_000.0, ptcount_val(pc).unwrap_or(0.0));
        let mut out: Vec<Box<str>> = Vec::new();
        for pt in cache.children_named("c:pt") {
            let i = pi(pt.attr("idx").unwrap_or("-1"));
            if !(0.0..100_000.0).contains(&i) {
                continue;
            }
            let i = to_usize(i);
            let v = first(Some(pt), "c:v");
            while out.len() < i {
                out.push("".into());
            }
            let s: Box<str> = v.map_or_else(String::new, text).into();
            match out.get_mut(i) {
                Some(slot) => *slot = s,
                None => out.push(s),
            }
        }
        while (out.len() as f64) < n {
            out.push("".into());
        }
        out
    }

    pub(crate) fn series(&self, ser: Element<'_>, kind: GroupKind) -> Series {
        let tx = first(Some(ser), "c:tx");
        let mut name = String::new();
        if let Some(v) = descend(tx, "c:v") {
            name = text(v);
        }
        let mut s = Series {
            idx: val_num(Some(ser), "c:idx").unwrap_or(0.0),
            order: val_num(Some(ser), "c:order").unwrap_or(0.0),
            name: name.into(),
            values: Vec::new(),
            cats: None,
            cat_nums: None,
            cat_levels: None,
            cat_fmt: None,
            x_vals: None,
            bubble: None,
            fmt: None,
            pt_fmts: None,
            sp: self.shape(first(Some(ser), "c:spPr")),
            marker: None,
            smooth: None,
            invert_if_negative: None,
            explosion: None,
            d_pts: PointMap::default(),
            d_lbls: None,
            trendlines: Vec::new(),
            box_opts: None,
            err_bars: Vec::new(),
        };
        let xy = kind == GroupKind::Scatter || kind == GroupKind::Bubble;
        let val = self.nums(first(Some(ser), if xy { "c:yVal" } else { "c:val" }));
        if let Some(val) = val {
            s.values = val.vals;
            if let Some(f) = val.fmt.filter(|f| !f.units().is_empty()) {
                s.fmt = Some(f);
            }
            if val.pt_fmts.is_some() {
                s.pt_fmts = val.pt_fmts;
            }
        }
        let cat_el = first(Some(ser), if xy { "c:xVal" } else { "c:cat" });
        if let Some(cat_el) = cat_el {
            let ml = cat_el.descend("c:multiLvlStrCache").or_else(|| cat_el.descend("c:multiLvlStrLit"));
            if let Some(ml) = ml {
                let mut lvls: Vec<Vec<Box<str>>> = ml.children_named("c:lvl").map(|l| self.pt_strings(l, Some(ml))).collect();
                if !lvls.is_empty() {
                    let rest = lvls.split_off(1);
                    s.cats = lvls.pop();
                    if !rest.is_empty() {
                        s.cat_levels = Some(rest);
                    }
                }
            } else {
                let st = self.strs(Some(cat_el));
                let nm = if st.is_some() { None } else { self.nums(Some(cat_el)) };
                if let Some(st) = st {
                    s.cats = Some(st);
                } else if let Some(nm) = nm {
                    s.cat_nums = Some(nm.vals);
                    if let Some(f) = nm.fmt.filter(|f| !f.units().is_empty()) {
                        s.cat_fmt = Some(f);
                    }
                }
            }
            if xy && let Some(nm) = self.nums(Some(cat_el)) {
                s.x_vals = Some(nm.vals);
            }
        }
        if kind == GroupKind::Bubble
            && let Some(b) = self.nums(first(Some(ser), "c:bubbleSize"))
        {
            s.bubble = Some(b.vals);
        }
        s.marker = self.marker(first(Some(ser), "c:marker")).map(Box::new);
        // Office 2007: no <c:smooth> = straight (2010+: smooth, the paint default)
        s.smooth = self.bool_of(Some(ser), "c:smooth").or(if self.v07() { Some(false) } else { None });
        // 2010+: a bar series without <c:invertIfNegative> inverts negatives (measured, syn-a-misc3b #2)
        s.invert_if_negative =
            self.bool_of(Some(ser), "c:invertIfNegative").or(if self.modern() && kind == GroupKind::Bar { Some(true) } else { None });
        s.explosion = val_num(Some(ser), "c:explosion");
        for dp in ser.children_named("c:dPt") {
            let Some(i) = val_num(Some(dp), "c:idx") else { continue };
            let mut p = DataPoint { sp: self.shape(first(Some(dp), "c:spPr")), ..DataPoint::default() };
            p.marker = self.marker(first(Some(dp), "c:marker")).map(Box::new);
            p.explosion = val_num(Some(dp), "c:explosion");
            p.invert_if_negative = self.bool_of(Some(dp), "c:invertIfNegative");
            s.d_pts.set(i, p);
        }
        s.d_lbls = self.d_lbls(first(Some(ser), "c:dLbls")).map(Box::new);
        for tl in ser.children_named("c:trendline") {
            let kind = val_str(Some(tl), "c:trendlineType").unwrap_or("linear");
            let sp = self.shape(first(Some(tl), "c:spPr"));
            let mut t = Trendline {
                kind: kind.into(),
                sp,
                order: None,
                period: None,
                name: None,
                forward: None,
                backward: None,
                intercept: None,
                disp_eq: false,
                disp_r_sqr: false,
                label: None,
            };
            t.name = first(Some(tl), "c:name").map(|n| text(n).into());
            t.order = val_num(Some(tl), "c:order");
            t.period = val_num(Some(tl), "c:period");
            t.forward = val_num(Some(tl), "c:forward");
            t.backward = val_num(Some(tl), "c:backward");
            t.intercept = val_num(Some(tl), "c:intercept");
            // 2010+: absent = shows the equation and R² (measured, syn trend2 and lo-trendline)
            t.disp_eq = self.bool_of(Some(tl), "c:dispEq").unwrap_or_else(|| self.modern());
            t.disp_r_sqr = self.bool_of(Some(tl), "c:dispRSqr").unwrap_or_else(|| self.modern());
            if let Some(lb) = first(Some(tl), "c:trendlineLbl") {
                let tx_pr = self.text_props(first(Some(lb), "c:txPr"));
                let sp = self.shape(first(Some(lb), "c:spPr"));
                let layout = self.layout(Some(lb));
                let num_fmt = self.num_fmt(Some(lb));
                t.label = Some(TrendLabel { layout, num_fmt, tx_pr, sp });
            }
            s.trendlines.push(t);
        }
        for eb in ser.children_named("c:errBars") {
            let kind = match val_str(Some(eb), "c:errValType") {
                Some("percentage") => crate::model::ErrValType::Percentage,
                Some("stdDev") => crate::model::ErrValType::StdDev,
                Some("stdErr") => crate::model::ErrValType::StdErr,
                Some("cust") => crate::model::ErrValType::Custom,
                _ => crate::model::ErrValType::Fixed,
            };
            let ty = val_str(Some(eb), "c:errBarType").unwrap_or("both");
            s.err_bars.push(crate::model::ErrBars {
                x_dir: val_str(Some(eb), "c:errDir").map(|d| d == "x"),
                plus: ty != "minus",
                minus: ty != "plus",
                kind,
                val: val_num(Some(eb), "c:val"),
                no_end_cap: self.bool_of(Some(eb), "c:noEndCap").unwrap_or(false),
                plus_vals: self.nums(first(Some(eb), "c:plus")).map(|n| n.vals),
                minus_vals: self.nums(first(Some(eb), "c:minus")).map(|n| n.vals),
                sp: self.shape(first(Some(eb), "c:spPr")),
            });
        }
        s
    }

    pub(crate) fn group(&self, el: Element<'_>, index: u32) -> Option<Group> {
        let (kind, three_d) = match el.name() {
            "c:barChart" => (GroupKind::Bar, false),
            "c:bar3DChart" => (GroupKind::Bar, true),
            "c:lineChart" => (GroupKind::Line, false),
            "c:line3DChart" => (GroupKind::Line, true),
            "c:areaChart" => (GroupKind::Area, false),
            "c:area3DChart" => (GroupKind::Area, true),
            "c:pieChart" => (GroupKind::Pie, false),
            "c:pie3DChart" => (GroupKind::Pie, true),
            "c:doughnutChart" => (GroupKind::Doughnut, false),
            "c:ofPieChart" => (GroupKind::OfPie, false),
            "c:scatterChart" => (GroupKind::Scatter, false),
            "c:radarChart" => (GroupKind::Radar, false),
            "c:bubbleChart" => (GroupKind::Bubble, false),
            "c:stockChart" => (GroupKind::Stock, false),
            "c:surfaceChart" => (GroupKind::Surface, false),
            "c:surface3DChart" => (GroupKind::Surface, true),
            _ => return None,
        };
        let grouping = match val_str(Some(el), "c:grouping").unwrap_or(if kind == GroupKind::Bar { "clustered" } else { "standard" }) {
            "clustered" => Grouping::Clustered,
            "stacked" => Grouping::Stacked,
            "percentStacked" => Grouping::PercentStacked,
            "standard" => Grouping::Standard,
            other => Grouping::Other(other.into()),
        };
        let stacked = matches!(grouping, Grouping::Stacked | Grouping::PercentStacked);
        let pie_like = matches!(kind, GroupKind::Pie | GroupKind::Doughnut | GroupKind::OfPie);
        let dir = if val_str(Some(el), "c:barDir") == Some("bar") { BarDir::Bar } else { BarDir::Col };
        let vary_colors = self.bool_of(Some(el), "c:varyColors").unwrap_or(self.modern() || pie_like);
        let gap_width = val_num(Some(el), "c:gapWidth").unwrap_or(150.0);
        let overlap = val_num(Some(el), "c:overlap").unwrap_or(if stacked { 100.0 } else { 0.0 });
        let hole_size = val_num(Some(el), "c:holeSize").unwrap_or(10.0);
        let first_slice_ang = val_num(Some(el), "c:firstSliceAng").unwrap_or(0.0);
        let show_marker = if kind == GroupKind::Line {
            self.bool_of(Some(el), "c:marker").unwrap_or(self.host.file_format().is_some())
        } else {
            kind == GroupKind::Scatter || kind == GroupKind::Radar
        };
        let series: Vec<Series> = el.children_named("c:ser").map(|s| self.series(s, kind)).collect();
        let ax_ids: Vec<Box<str>> = el.children_named("c:axId").map(|a| a.attr("val").unwrap_or("").into()).collect();
        let mut g = Group {
            kind,
            three_d,
            index,
            dir,
            grouping,
            vary_colors,
            gap_width,
            overlap,
            hole_size,
            first_slice_ang,
            scatter_style: truthy(val_str(Some(el), "c:scatterStyle")).map(Into::into),
            radar_style: truthy(val_str(Some(el), "c:radarStyle")).map(Into::into),
            show_marker,
            series,
            ax_ids,
            d_lbls: None,
            drop_lines: None,
            hi_low_lines: None,
            up_down_bars: None,
            ser_lines: None,
            bubble_scale: None,
            gap_depth: None,
            shape: None,
            of_pie_type: None,
            of_pie: None,
            parent_label: None,
        };
        g.d_lbls = self.d_lbls(first(Some(el), "c:dLbls")).map(Box::new);
        if let Some(d) = first(Some(el), "c:dropLines") {
            g.drop_lines = Some(self.shape(first(Some(d), "c:spPr")));
        }
        if let Some(h) = first(Some(el), "c:hiLowLines") {
            g.hi_low_lines = Some(self.shape(first(Some(h), "c:spPr")));
        }
        if let Some(s) = first(Some(el), "c:serLines") {
            g.ser_lines = Some(self.shape(first(Some(s), "c:spPr")));
        }
        if let Some(ud) = first(Some(el), "c:upDownBars") {
            let gap_width = val_num(Some(ud), "c:gapWidth").unwrap_or(150.0);
            let up = self.shape(first(first(Some(ud), "c:upBars"), "c:spPr"));
            let down = self.shape(first(first(Some(ud), "c:downBars"), "c:spPr"));
            g.up_down_bars = Some(UpDownBars { gap_width, up, down });
        }
        g.gap_depth = val_num(Some(el), "c:gapDepth").filter(|v| *v >= 0.0);
        g.bubble_scale = val_num(Some(el), "c:bubbleScale");
        g.shape = truthy(val_str(Some(el), "c:shape")).map(Into::into);
        g.of_pie_type = truthy(val_str(Some(el), "c:ofPieType")).map(Into::into);
        if kind == GroupKind::OfPie {
            let split_pos = val_num(Some(el), "c:splitPos");
            let cust: Vec<f64> = first(Some(el), "c:custSplit")
                .map(|c| c.children_named("c:secondPiePt").map(|p| pi(p.attr("val").unwrap_or("-1"))).filter(|v| *v >= 0.0).collect())
                .unwrap_or_default();
            g.of_pie = Some(OfPie {
                split_type: val_str(Some(el), "c:splitType").unwrap_or("auto").into(),
                split_pos,
                cust_split: if cust.is_empty() { None } else { Some(cust) },
                second_pie_size: val_num(Some(el), "c:secondPieSize").unwrap_or(75.0),
            });
        }
        js_sort_by(&mut g.series, |a, b| a.order - b.order);
        Some(g)
    }

    pub(crate) fn axis(&self, el: Element<'_>) -> Option<Axis> {
        let kind = match el.name() {
            "c:catAx" => AxisKind::Cat,
            "c:valAx" => AxisKind::Val,
            "c:dateAx" => AxisKind::Date,
            "c:serAx" => AxisKind::Ser,
            _ => return None,
        };
        let sc = first(Some(el), "c:scaling");
        // absent in Office 2007: major "out", minor none (measured, gdx-545393 and syn-a-mso07 #1);
        // otherwise "cross" (the schema default)
        let tick = |k: &str| -> TickMark {
            let s = val_str(Some(el), k).unwrap_or(if self.v07() && first(Some(el), k).is_none() {
                if k == "c:majorTickMark" { "out" } else { "none" }
            } else {
                "cross"
            });
            match s {
                "none" => TickMark::None,
                "in" => TickMark::In,
                "out" => TickMark::Out,
                "cross" => TickMark::Cross,
                other => TickMark::Other(other.into()),
            }
        };
        let pos = match val_str(Some(el), "c:axPos").unwrap_or("b") {
            "b" => AxisPos::B,
            "l" => AxisPos::L,
            "r" => AxisPos::R,
            "t" => AxisPos::T,
            other => AxisPos::Other(other.into()),
        };
        let crosses = match val_str(Some(el), "c:crosses").unwrap_or("autoZero") {
            "autoZero" => Crosses::AutoZero,
            "min" => Crosses::Min,
            "max" => Crosses::Max,
            other => Crosses::Other(other.into()),
        };
        let tick_lbl_pos = match val_str(Some(el), "c:tickLblPos").unwrap_or("nextTo") {
            "nextTo" => TickLblPos::NextTo,
            "low" => TickLblPos::Low,
            "high" => TickLblPos::High,
            "none" => TickLblPos::None,
            other => TickLblPos::Other(other.into()),
        };
        let mut a = Axis {
            id: val_str(Some(el), "c:axId").unwrap_or("").into(),
            kind,
            pos,
            deleted: self.bool_of(Some(el), "c:delete").unwrap_or(self.modern()),
            reversed: val_str(sc, "c:orientation") == Some("maxMin"),
            min: None,
            max: None,
            log_base: None,
            major_unit: None,
            minor_unit: None,
            cross_ax: None,
            crosses,
            crosses_at: None,
            cross_between: None,
            major_grid: None,
            minor_grid: None,
            major_tick: tick("c:majorTickMark"),
            minor_tick: tick("c:minorTickMark"),
            tick_lbl_pos,
            num_fmt: None,
            title: None,
            tx_pr: self.text_props(first(Some(el), "c:txPr")),
            sp: self.shape(first(Some(el), "c:spPr")),
            lbl_offset: val_num(Some(el), "c:lblOffset").unwrap_or(100.0),
            tick_lbl_skip: None,
            tick_mark_skip: None,
            no_multi_lvl_lbl: self.bool_of(Some(el), "c:noMultiLvlLbl").unwrap_or(false),
            disp_units: None,
            base_time_unit: None,
            major_time_unit: None,
        };
        a.min = val_num(sc, "c:min");
        a.max = val_num(sc, "c:max");
        a.log_base = val_num(sc, "c:logBase").filter(|lb| *lb >= 2.0 && *lb <= 1000.0);
        a.major_unit = val_num(Some(el), "c:majorUnit").filter(|v| *v > 0.0);
        a.minor_unit = val_num(Some(el), "c:minorUnit").filter(|v| *v > 0.0);
        a.cross_ax = truthy(val_str(Some(el), "c:crossAx")).map(Into::into);
        a.crosses_at = val_num(Some(el), "c:crossesAt");
        a.cross_between = match val_str(Some(el), "c:crossBetween") {
            Some("between") => Some(CrossBetween::Between),
            Some("midCat") => Some(CrossBetween::MidCat),
            _ => None,
        };
        if let Some(mg) = first(Some(el), "c:majorGridlines") {
            a.major_grid = Some(self.shape(first(Some(mg), "c:spPr")));
        }
        if let Some(ng) = first(Some(el), "c:minorGridlines") {
            a.minor_grid = Some(self.shape(first(Some(ng), "c:spPr")));
        }
        a.num_fmt = self.num_fmt(Some(el));
        a.title = self.title(first(Some(el), "c:title")).map(Box::new);
        a.tick_lbl_skip = val_num(Some(el), "c:tickLblSkip").filter(|v| *v >= 1.0);
        a.tick_mark_skip = val_num(Some(el), "c:tickMarkSkip").filter(|v| *v >= 1.0);
        if let Some(du) = first(Some(el), "c:dispUnits") {
            let b = truthy(val_str(Some(du), "c:builtInUnit"));
            let c = val_num(Some(du), "c:custUnit");
            let f = match b {
                Some(b) => builtin_unit(b),
                None => c,
            };
            a.disp_units = f.filter(|f| *f > 0.0);
        }
        a.base_time_unit = truthy(val_str(Some(el), "c:baseTimeUnit")).map(Into::into);
        a.major_time_unit = truthy(val_str(Some(el), "c:majorTimeUnit")).map(Into::into);
        Some(a)
    }

    pub(crate) fn legend(&self, el: Option<Element<'_>>) -> Option<Legend> {
        let el = el?;
        let pos = match val_str(Some(el), "c:legendPos").unwrap_or("r") {
            "b" => LegendPos::B,
            "t" => LegendPos::T,
            "l" => LegendPos::L,
            "r" => LegendPos::R,
            "tr" => LegendPos::Tr,
            other => LegendPos::Other(other.into()),
        };
        // 2010+: a legend without <c:overlay> OVERLAPS the plot (measured in Excel, syn ff16)
        let overlay = self.bool_of(Some(el), "c:overlay").unwrap_or(self.modern());
        let tx_pr = self.text_props(first(Some(el), "c:txPr"));
        let sp = self.shape(first(Some(el), "c:spPr"));
        let mut lg = Legend { pos, overlay, layout: None, deleted: PointSet::default(), tx_pr, sp };
        for e in el.children_named("c:legendEntry") {
            if let Some(i) = val_num(Some(e), "c:idx")
                && self.bool_of(Some(e), "c:delete").unwrap_or(false)
            {
                lg.deleted.add(i);
            }
        }
        lg.layout = self.layout(Some(el));
        Some(lg)
    }

    /// `c:userShapes` → shapes: every anchor (`relSizeAnchor` from/to as fractions, `absSizeAnchor` from +
    /// ext in EMU) with `sp`/`pic`/`cxnSp`; groups flattened with the child → anchor box transformation
    /// (as the spreadsheet drawing).
    pub(crate) fn user_shapes(&self, root: Element<'_>) -> Vec<UserShape> {
        let mut out = Vec::new();
        let num = |el: Option<Element<'_>>, name: &str| -> f64 {
            let v = first(el, name).map_or(f64::NAN, |c| parse_float(&text(c)));
            if v.is_finite() { v } else { 0.0 }
        };
        for an in root.child_elements() {
            let rel = an.name() == "cdr:relSizeAnchor";
            let abs = an.name() == "cdr:absSizeAnchor";
            if !rel && !abs {
                continue;
            }
            let from = first(Some(an), "cdr:from");
            let to = first(Some(an), "cdr:to");
            let ext = first(Some(an), "cdr:ext");
            let x = num(from, "cdr:x");
            let y = num(from, "cdr:y");
            let (w, h) = if rel {
                (num(to, "cdr:x") - x, num(to, "cdr:y") - y)
            } else {
                (attr_num(ext, "cx") / 12700.0, attr_num(ext, "cy") / 12700.0)
            };
            if !(w > 0.0 && h > 0.0) {
                continue;
            }
            let bx = AnchorBox { rect: FracRect { x, y, w, h }, abs };
            self.visit_shapes(an, None, &bx, 0, &mut out);
        }
        out
    }

    fn visit_shapes(
        &self,
        el: Element<'_>,
        xf: Option<&dyn Fn(f64, f64) -> (f64, f64)>,
        bx: &AnchorBox,
        depth: u32,
        out: &mut Vec<UserShape>,
    ) {
        for c in el.child_elements() {
            if c.name() == "cdr:grpSp" {
                if depth >= MAX_GROUP_DEPTH {
                    continue;
                }
                let gx = first(first(Some(c), "cdr:grpSpPr"), "a:xfrm");
                let off = first(gx, "a:off");
                let e = first(gx, "a:ext");
                let c_off = first(gx, "a:chOff");
                let c_ext = first(gx, "a:chExt");
                let cw = or_num(or_num(attr_num(c_ext, "cx"), attr_num(e, "cx")), 1.0);
                let ch = or_num(or_num(attr_num(c_ext, "cy"), attr_num(e, "cy")), 1.0);
                let ew = or_num(attr_num(e, "cx"), cw);
                let eh = or_num(attr_num(e, "cy"), ch);
                let (ox, oy) = (attr_num(off, "x"), attr_num(off, "y"));
                let (cx, cy) = (attr_num(c_off, "x"), attr_num(c_off, "y"));
                match xf {
                    Some(p) => {
                        let g = |px: f64, py: f64| p(ox + (px - cx) * ew / cw, oy + (py - cy) * eh / ch);
                        self.visit_shapes(c, Some(&g), bx, depth + 1, out);
                    }
                    None => {
                        let g = |px: f64, py: f64| ((px - cx) / cw, (py - cy) / ch);
                        self.visit_shapes(c, Some(&g), bx, depth + 1, out);
                    }
                }
                continue;
            }
            if !matches!(c.name(), "cdr:sp" | "cdr:pic" | "cdr:cxnSp") {
                continue;
            }
            let nv = c.descend("cdr:cNvPr");
            if matches!(nv.and_then(|n| n.attr("hidden")), Some("1" | "true")) {
                continue;
            }
            let sp_pr = first(Some(c), "cdr:spPr");
            let xfrm = first(sp_pr, "a:xfrm");
            let geom: Box<str> = match first(sp_pr, "a:prstGeom").and_then(|g| g.attr("prst")) {
                Some(p) => p.into(),
                None => (if first(sp_pr, "a:custGeom").is_some() { "custom" } else { "rect" }).into(),
            };
            let mut u = UserShape {
                rect: bx.rect,
                abs: bx.abs,
                sub: None,
                geom,
                rot: None,
                flip_h: false,
                flip_v: false,
                sp: self.shape(sp_pr),
                text: None,
                blip: None,
            };
            if let Some(xf) = xf {
                let off = first(xfrm, "a:off");
                let e = first(xfrm, "a:ext");
                let (ox, oy) = (attr_num(off, "x"), attr_num(off, "y"));
                let (x0, y0) = xf(ox, oy);
                let (x1, y1) = xf(ox + attr_num(e, "cx"), oy + attr_num(e, "cy"));
                if !(x1 > x0 && y1 > y0) {
                    continue;
                }
                u.sub = Some(FracRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
            }
            let rot = attr_num(xfrm, "rot") / 60_000.0;
            if rot != 0.0 {
                u.rot = Some(rot);
            }
            u.flip_h = xfrm.and_then(|x| x.attr("flipH")) == Some("1");
            u.flip_v = xfrm.and_then(|x| x.attr("flipV")) == Some("1");
            // style (cdr:style): lnRef/fillRef/fontRef with the theme colour
            let st = first(Some(c), "cdr:style");
            let style_ref = |k: &str| -> Option<(f64, Option<ChartColor>)> {
                let r = first(st, k)?;
                let idx = or_num(pi(r.attr("idx").unwrap_or("0")), 0.0);
                Some((idx, self.color(color_child(Some(r)))))
            };
            let fr = style_ref("a:fillRef");
            let lr = style_ref("a:lnRef");
            let fo = style_ref("a:fontRef");
            if u.sp.fill.is_none()
                && let Some((idx, Some(color))) = &fr
                && *idx > 0.0
                && !color.as_str().is_empty()
            {
                let themed = if self.host.has_fill_style() { self.host.fill_style(to_u32_sat(*idx), color.as_str()) } else { None };
                u.sp.fill = Some(themed.unwrap_or_else(|| Paint::Solid { color: color.clone(), alpha: None }));
            }
            if let Some((idx, Some(color))) = &lr
                && *idx > 0.0
                && !color.as_str().is_empty()
            {
                let widths = [0.0, 0.75, 1.0, 1.5];
                let w = widths.get(to_u32_sat(min(3.0, *idx)) as usize).copied().unwrap_or(0.0);
                let own = u.sp.line.take().unwrap_or_default();
                u.sp.line = Some(Box::new(LineProps {
                    paint: own.paint.or_else(|| Some(Paint::Solid { color: color.clone(), alpha: None })),
                    w: own.w.or(Some(w)),
                    dash: own.dash,
                    cap: own.cap,
                }));
            }
            if c.name() == "cdr:cxnSp" && &*u.geom == "rect" {
                u.geom = "line".into();
            }
            if c.name() == "cdr:pic" {
                let blip = descend(first(Some(c), "cdr:blipFill"), "a:blip");
                u.blip = blip.and_then(|b| self.host.blip(b));
            }
            let tb = first(Some(c), "cdr:txBody");
            let has_text = tb.is_some_and(|tb| tb.children_named("a:p").any(|p| p.descend("a:t").is_some_and(|t| !text(t).is_empty())));
            if has_text && let Some(tb) = tb {
                let body = first(Some(tb), "a:bodyPr");
                let ins = |k: &str, d: f64| match body.and_then(|b| b.attr(k)) {
                    Some(_) => attr_num(body, k) / 12700.0,
                    None => d,
                };
                let mut def = self.run_props(descend(first(first(Some(tb), "a:lstStyle"), "a:lvl1pPr"), "a:defRPr"));
                if def.color.is_none()
                    && let Some((_, Some(fc))) = &fo
                    && !fc.as_str().is_empty()
                {
                    def.color = Some(fc.clone());
                }
                let vert = body.and_then(|b| b.attr("vert"));
                let paras = self.rich(tb);
                let mut t = UserText {
                    paras,
                    ins: Insets { l: ins("lIns", 7.2), t: ins("tIns", 3.6), r: ins("rIns", 7.2), b: ins("bIns", 3.6) },
                    anchor: match body.and_then(|b| b.attr("anchor")) {
                        Some("ctr") => TextAnchor::Ctr,
                        Some("b") => TextAnchor::B,
                        _ => TextAnchor::T,
                    },
                    wrap: body.and_then(|b| b.attr("wrap")) != Some("none"),
                    clip: body.and_then(|b| b.attr("vertOverflow")) == Some("clip"),
                    rot: None,
                    def,
                };
                let base = match vert {
                    Some("vert" | "eaVert") => 90.0,
                    Some("vert270") => -90.0,
                    _ => 0.0,
                };
                let tr = base + attr_num(body, "rot") / 60_000.0;
                if tr != 0.0 {
                    t.rot = Some(tr);
                }
                u.text = Some(t);
            }
            out.push(u);
        }
    }
}

/// The box of an anchor of the chart drawing: fractions of the chart (`relSizeAnchor`) or the corner in
/// fractions and the size in pt (`absSizeAnchor`).
struct AnchorBox {
    rect: FracRect,
    abs: bool,
}

/// `attr(el, k)` of `userShapes`: `parseFloat` of the attribute (default `0`), finite or 0.
fn attr_num(el: Option<Element<'_>>, k: &str) -> f64 {
    let v = el.map_or(0.0, |e| parse_float(e.attr(k).unwrap_or("0")));
    if v.is_finite() { v } else { 0.0 }
}

/// `parseFloat` of a `c:ptCount`-like child's `val`, finite only (`valNum` over the element itself).
fn ptcount_val(el: Option<Element<'_>>) -> Option<f64> {
    let v = parse_float(el?.attr("val")?);
    v.is_finite().then_some(v)
}

/// `UNITS[b]` of `dispUnits` (parse.ts:566).
fn builtin_unit(b: &str) -> Option<f64> {
    Some(match b {
        "hundreds" => 1e2,
        "thousands" => 1e3,
        "tenThousands" => 1e4,
        "hundredThousands" => 1e5,
        "millions" => 1e6,
        "tenMillions" => 1e7,
        "hundredMillions" => 1e8,
        "billions" => 1e9,
        "trillions" => 1e12,
        _ => return None,
    })
}

/// The numeric cache of a series part (the TS `nums` result).
pub(crate) struct Nums {
    pub vals: Vec<Option<f64>>,
    pub fmt: Option<FmtCode>,
    pub pt_fmts: Option<Vec<Option<FmtCode>>>,
}

/// `styleOf(root)` (parse.ts:757): `c:style`, else the AlternateContent's `c14:style` − 100 (any
/// number: NaN included), else 2.
fn style_of(root: Element<'_>) -> f64 {
    if let Some(st) = val_num(Some(root), "c:style") {
        return st;
    }
    for ac in root.children_named("mc:AlternateContent") {
        let v = ac.descend("c14:style").and_then(|e| e.attr("val")).or_else(|| ac.descend("c:style").and_then(|e| e.attr("val")));
        if let Some(v) = v {
            let n = pi(v);
            return if n > 100.0 { n - 100.0 } else { n };
        }
    }
    2.0
}

/// `parseChartSpace(root, host)` (parse.ts:767): the chart part's tree (canonical prefixes) → the model.
/// A root that is not a chart space gives the empty model; a `cx:chartSpace` is converted by
/// [`crate::chartex`].
pub fn parse_chart_space(root: Element<'_>, host: &dyn ChartHost) -> ChartSpace {
    let p = Parser::new(host);
    let scheme = |v: &str| host.color(ColorSource::Scheme(v)).unwrap_or_else(|| ChartColor::new("#000000"));
    let style = style_of(root);
    // default grey lines (tx1 75%): chart with c14:style OR an Office 2007 deck (measured, syn-a-mso07/ticks07)
    let office2010 = p.v07() || root.children_named("mc:AlternateContent").any(|ac| ac.descend("c14:style").is_some());
    // absent = rounded corners (the Office draws so on the frame of styles 33–40); in Office 2007, absent = straight
    let rounded_corners = p.bool_of(Some(root), "c:roundedCorners").unwrap_or(!p.v07());
    let sp = p.shape(first(Some(root), "c:spPr"));
    let tx_pr = p.text_props(first(Some(root), "c:txPr"));
    let accents = [scheme("accent1"), scheme("accent2"), scheme("accent3"), scheme("accent4"), scheme("accent5"), scheme("accent6")];
    let tx1 = scheme("tx1");
    let bg1 = scheme("bg1");
    let fonts = host.fonts();
    let widths = host.theme_line_widths();
    let mut cs = ChartSpace {
        style,
        office2010,
        rounded_corners,
        title: None,
        auto_title_deleted: false,
        auto_title_written: false,
        groups: Vec::new(),
        axes: Vec::new(),
        plot_layout: None,
        plot_sp: ShapeProps::default(),
        legend: None,
        sp,
        tx_pr,
        disp_blanks_as: DispBlanksAs::Gap,
        d_table: None,
        view_3d: None,
        floor_sp: None,
        back_wall_sp: None,
        side_wall_sp: None,
        accents,
        tx1,
        bg1,
        minor_font: truthy(fonts.minor).map(Into::into),
        major_font: truthy(fonts.major).map(Into::into),
        has_theme_fill: host.has_fill_style(),
        theme_ln: if widths.is_empty() { None } else { Some(widths.into()) },
        user_shapes: None,
    };
    // Office 2016 chart (cx:): converted into the engine's model
    if root.name() == "cx:chartSpace" {
        return crate::chartex::parse_chart_ex(root, &p, cs);
    }
    let Some(chart) = first(Some(root), "c:chart") else { return cs };
    cs.title = p.title(first(Some(chart), "c:title"));
    cs.auto_title_deleted = p.bool_of(Some(chart), "c:autoTitleDeleted").unwrap_or(false);
    cs.auto_title_written = first(Some(chart), "c:autoTitleDeleted").is_some();
    if let Some(v3) = first(Some(chart), "c:view3D") {
        cs.view_3d = Some(View3D {
            rot_x: val_num(Some(v3), "c:rotX"),
            rot_y: val_num(Some(v3), "c:rotY"),
            r_ang_ax: p.bool_of(Some(v3), "c:rAngAx").unwrap_or(true),
            perspective: val_num(Some(v3), "c:perspective").unwrap_or(30.0),
            depth_percent: val_num(Some(v3), "c:depthPercent").unwrap_or(100.0),
            h_percent: val_num(Some(v3), "c:hPercent"),
        });
    }
    if let Some(e) = first(Some(chart), "c:floor") {
        cs.floor_sp = Some(p.shape(first(Some(e), "c:spPr")));
    }
    if let Some(e) = first(Some(chart), "c:backWall") {
        cs.back_wall_sp = Some(p.shape(first(Some(e), "c:spPr")));
    }
    if let Some(e) = first(Some(chart), "c:sideWall") {
        cs.side_wall_sp = Some(p.shape(first(Some(e), "c:spPr")));
    }
    if let Some(plot) = first(Some(chart), "c:plotArea") {
        cs.plot_layout = p.layout(Some(plot));
        cs.plot_sp = p.shape(first(Some(plot), "c:spPr"));
        if let Some(dt) = first(Some(plot), "c:dTable") {
            cs.d_table = Some(DataTable {
                horz: p.bool_of(Some(dt), "c:showHorzBorder").unwrap_or(false),
                vert: p.bool_of(Some(dt), "c:showVertBorder").unwrap_or(false),
                outline: p.bool_of(Some(dt), "c:showOutline").unwrap_or(false),
                keys: p.bool_of(Some(dt), "c:showKeys").unwrap_or(false),
                tx_pr: p.text_props(first(Some(dt), "c:txPr")),
            });
        }
        let mut gi = 0u32;
        for c in plot.child_elements() {
            if let Some(g) = p.group(c, gi) {
                cs.groups.push(g);
                gi += 1;
                continue;
            }
            if let Some(a) = p.axis(c) {
                cs.axes.push(a);
            }
        }
    }
    fix_axes(&mut cs);
    // Office 2007: a single series, no <c:title> and automatic title not deleted = a title with the series
    // name (measured, syn-a-mso07 #10; in 2010+ only with <c:title>)
    if p.v07() && cs.title.is_none() && !cs.auto_title_deleted && cs.groups.iter().map(|g| g.series.len()).sum::<usize>() == 1 {
        cs.title = Some(ChartTitle {
            paras: None,
            text: None,
            overlay: false,
            layout: None,
            sp: ShapeProps::default(),
            tx_pr: TextProps::default(),
        });
    }
    cs.legend = p.legend(first(Some(chart), "c:legend"));
    cs.disp_blanks_as = match val_str(Some(chart), "c:dispBlanksAs") {
        Some("zero") => DispBlanksAs::Zero,
        Some("span") => DispBlanksAs::Span,
        _ => DispBlanksAs::Gap,
    };
    if let Some(us) = host.user_shapes() {
        let shapes = p.user_shapes(us);
        if !shapes.is_empty() {
            cs.user_shapes = Some(shapes);
        }
    }
    cs
}

/// `fixAxes(cs)` (parse.ts:795): groups with absent or unmatched axes (minimal XML, simple generators):
/// links to the plot's only pair of axes or creates the default pair (categories at the bottom, values on
/// the left with grid), as the Office does on opening.
fn fix_axes(cs: &mut ChartSpace) {
    let base = |id: String, kind: AxisKind, pos: AxisPos| -> Axis {
        let val = kind == AxisKind::Val;
        Axis {
            id: id.into(),
            kind,
            pos,
            deleted: false,
            reversed: false,
            min: None,
            max: None,
            log_base: None,
            major_unit: None,
            minor_unit: None,
            cross_ax: None,
            crosses: Crosses::AutoZero,
            crosses_at: None,
            cross_between: if val { Some(CrossBetween::Between) } else { None },
            major_grid: if val { Some(ShapeProps::default()) } else { None },
            minor_grid: None,
            major_tick: TickMark::Out,
            minor_tick: TickMark::None,
            tick_lbl_pos: TickLblPos::NextTo,
            num_fmt: None,
            title: None,
            tx_pr: TextProps::default(),
            sp: ShapeProps::default(),
            lbl_offset: 100.0,
            tick_lbl_skip: None,
            tick_mark_skip: None,
            no_multi_lvl_lbl: false,
            disp_units: None,
            base_time_unit: None,
            major_time_unit: None,
        }
    };
    for gi in 0..cs.groups.len() {
        let Some(g) = cs.groups.get(gi) else { break };
        if matches!(g.kind, GroupKind::Pie | GroupKind::Doughnut | GroupKind::OfPie) {
            continue;
        }
        let found = g.ax_ids.iter().filter(|id| cs.axes.iter().any(|a| a.id == **id)).count();
        if found >= 2 {
            continue;
        }
        let xy = matches!(g.kind, GroupKind::Scatter | GroupKind::Bubble);
        let first_kind = if xy { AxisKind::Val } else { AxisKind::Cat };
        let bar = g.dir == BarDir::Bar;
        let index = g.index;
        let mut i0 = cs
            .axes
            .iter()
            .position(|a| if xy { a.kind == AxisKind::Val && matches!(a.pos, AxisPos::B | AxisPos::T) } else { a.kind != AxisKind::Val });
        let mut i1 = cs.axes.iter().enumerate().position(|(k, a)| a.kind == AxisKind::Val && Some(k) != i0);
        if i0.is_none() {
            cs.axes.push(base(format!("__x{index}"), first_kind, if bar { AxisPos::L } else { AxisPos::B }));
            i0 = Some(cs.axes.len() - 1);
        }
        if i1.is_none() {
            cs.axes.push(base(format!("__y{index}"), AxisKind::Val, if bar { AxisPos::B } else { AxisPos::L }));
            i1 = Some(cs.axes.len() - 1);
        }
        let (Some(i0), Some(i1)) = (i0, i1) else { continue };
        if let Some(a) = cs.axes.get_mut(i0)
            && a.id.is_empty()
        {
            a.id = format!("_x{index}").into();
        }
        if let Some(a) = cs.axes.get_mut(i1)
            && a.id.is_empty()
        {
            a.id = format!("_y{index}").into();
        }
        let id0 = cs.axes.get(i0).map(|a| a.id.clone()).unwrap_or_default();
        let id1 = cs.axes.get(i1).map(|a| a.id.clone()).unwrap_or_default();
        if let Some(a0) = cs.axes.get_mut(i0) {
            a0.cross_ax.get_or_insert_with(|| id1.clone());
        }
        if let Some(a1) = cs.axes.get_mut(i1) {
            a1.cross_ax.get_or_insert_with(|| id0.clone());
        }
        if let Some(g) = cs.groups.get_mut(gi) {
            g.ax_ids = vec![id0, id1];
        }
    }
}
