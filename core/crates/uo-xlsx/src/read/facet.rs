//! Canonical JSON of the C1a model (feature `facet`): byte-equal to what `capture-sheet.mjs` prints from the
//! frozen TS model with `canon-json.mjs` (keys sorted by UTF-16 code unit, `String(n)` numbers, `-0` and
//! `{"$num":"NaN"}`, `JSON.stringify` strings, strings over 4096 units digested, `Map` as `{"$map":[[k,v]…]}`,
//! `undefined` keys omitted). Parity harnesses and tests only: the reader does not need it.

use super::model::*;
use super::sst::RunPropsTable;
use uo_calc::cell::InlineText;
use uo_calc::facet::{push_num, push_str};
use uo_calc::strings::JsStr;

/// A JSON object under construction: keys are sorted when it is closed.
#[derive(Default)]
pub struct Obj {
    pairs: Vec<(String, String)>,
}

/// JSON text of a Rust string.
pub fn jstr(s: &str) -> String {
    let mut out = String::new();
    if s.is_ascii() {
        push_str(&mut out, JsStr::Latin1(s.as_bytes()));
    } else {
        let u: Vec<u16> = s.encode_utf16().collect();
        push_str(&mut out, JsStr::Utf16(&u));
    }
    out
}

/// JSON text of a number.
pub fn jnum(x: f64) -> String {
    let mut out = String::new();
    push_num(&mut out, x);
    out
}

/// JSON text of an array of already rendered items.
pub fn jarr(items: impl IntoIterator<Item = String>) -> String {
    let v: Vec<String> = items.into_iter().collect();
    format!("[{}]", v.join(","))
}

fn units_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

impl Obj {
    /// A new object.
    pub fn new() -> Self {
        Obj::default()
    }
    /// A rendered value.
    pub fn raw(&mut self, k: &str, v: String) -> &mut Self {
        self.pairs.push((k.to_owned(), v));
        self
    }
    /// A string.
    pub fn s(&mut self, k: &str, v: &str) -> &mut Self {
        self.raw(k, jstr(v))
    }
    /// An optional string.
    pub fn os(&mut self, k: &str, v: Option<&str>) -> &mut Self {
        if let Some(v) = v {
            self.s(k, v);
        }
        self
    }
    /// A number.
    pub fn n(&mut self, k: &str, v: f64) -> &mut Self {
        self.raw(k, jnum(v))
    }
    /// An optional number.
    pub fn on(&mut self, k: &str, v: Option<f64>) -> &mut Self {
        if let Some(v) = v {
            self.n(k, v);
        }
        self
    }
    /// A boolean the TS sets only when true.
    pub fn t(&mut self, k: &str, v: bool) -> &mut Self {
        if v {
            self.raw(k, "true".to_owned());
        }
        self
    }
    /// A boolean always present.
    pub fn b(&mut self, k: &str, v: bool) -> &mut Self {
        self.raw(k, if v { "true" } else { "false" }.to_owned())
    }
    /// An optional boolean.
    pub fn ob(&mut self, k: &str, v: Option<bool>) -> &mut Self {
        if let Some(v) = v {
            self.b(k, v);
        }
        self
    }
    /// An optional rendered value.
    pub fn oraw(&mut self, k: &str, v: Option<String>) -> &mut Self {
        if let Some(v) = v {
            self.raw(k, v);
        }
        self
    }
    /// Closes the object.
    pub fn done(mut self) -> String {
        self.pairs.sort_by(|a, b| units_cmp(&a.0, &b.0));
        let mut out = String::from("{");
        for (i, (k, v)) in self.pairs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&jstr(k));
            out.push(':');
            out.push_str(v);
        }
        out.push('}');
        out
    }
}

fn strs(v: &[String]) -> String {
    jarr(v.iter().map(|s| jstr(s)))
}

fn nums(v: &[f64]) -> String {
    jarr(v.iter().map(|&x| jnum(x)))
}

// ---------------------------------------------------------------- xml subtrees

/// `XmlElement` as `{attrs, children, name}`.
pub fn xel(e: &XEl) -> String {
    let mut o = Obj::new();
    let mut a = Obj::new();
    for (k, v) in &e.attrs {
        a.s(k, v);
    }
    o.raw("attrs", a.done());
    o.raw(
        "children",
        jarr(e.children.iter().map(|c| match c {
            XChild::El(x) => xel(x),
            XChild::Text(t) => jstr(t),
        })),
    );
    o.s("name", &e.name);
    o.done()
}

// ---------------------------------------------------------------- workbook, theme

/// `Workbook`.
pub fn workbook(w: &Workbook) -> String {
    let mut o = Obj::new();
    o.raw(
        "sheets",
        jarr(w.sheets.iter().map(|s| {
            let mut x = Obj::new();
            x.s("name", &s.name)
                .s("sheetId", &s.sheet_id)
                .s("rId", &s.r_id)
                .os("state", s.state.as_deref())
                .os("target", s.target.as_deref());
            x.done()
        })),
    );
    o.on("activeTab", w.active_tab).t("date1904", w.date1904);
    if !w.defined_names.is_empty() {
        o.raw(
            "definedNames",
            jarr(w.defined_names.iter().map(|d| {
                let mut x = Obj::new();
                x.s("name", &d.name).s("formula", &d.formula).on("localSheetId", d.local_sheet_id).t("hidden", d.hidden);
                x.done()
            })),
        );
    }
    if let Some(c) = &w.calc_pr {
        let mut x = Obj::new();
        x.on("calcId", c.calc_id)
            .os("calcMode", c.calc_mode.as_deref())
            .t("fullCalcOnLoad", c.full_calc_on_load)
            .t("forceFullCalc", c.force_full_calc)
            .t("iterate", c.iterate)
            .on("iterateCount", c.iterate_count)
            .on("iterateDelta", c.iterate_delta)
            .os("refMode", c.ref_mode.as_deref());
        o.raw("calcPr", x.done());
    }
    o.os("appName", w.app_name.as_deref());
    if !w.external_links.is_empty() {
        o.raw(
            "externalLinks",
            jarr(w.external_links.iter().map(|l| {
                let mut x = Obj::new();
                x.os("target", l.target.as_deref()).raw("sheetNames", strs(&l.sheet_names));
                x.raw(
                    "definedNames",
                    jarr(l.defined_names.iter().map(|d| {
                        let mut y = Obj::new();
                        y.s("name", &d.name).os("refersTo", d.refers_to.as_deref()).on("sheetId", d.sheet_id);
                        y.done()
                    })),
                );
                x.raw(
                    "sheetData",
                    jarr(l.sheet_data.iter().map(|sd| {
                        let mut y = Obj::new();
                        y.n("sheetId", sd.sheet_id).raw(
                            "cells",
                            jarr(sd.cells.iter().map(|c| {
                                let mut z = Obj::new();
                                z.s("ref", &c.r#ref).os("t", c.t.as_deref()).os("v", c.v.as_deref());
                                z.done()
                            })),
                        );
                        y.done()
                    })),
                );
                x.done()
            })),
        );
    }
    if !w.dynamic_array_cm.is_empty() {
        o.raw("dynamicArrayCm", nums(&w.dynamic_array_cm));
    }
    o.done()
}

/// `Theme` (`ColorContext.theme`).
pub fn theme(t: &Theme) -> String {
    let mut o = Obj::new();
    let mut by = Obj::new();
    for (k, v) in &t.by_name {
        by.s(k, v);
    }
    o.raw("byName", by.done()).raw("colors", strs(&t.colors));
    if t.major.is_some() || t.minor.is_some() {
        let mut f = Obj::new();
        f.os("major", t.major.as_deref()).os("minor", t.minor.as_deref());
        o.raw("fonts", f.done());
    }
    if let Some(fs) = &t.fill_styles {
        o.raw("fillStyles", jarr(fs.iter().map(xel)));
    }
    if let Some(l) = &t.line_widths {
        o.raw("lineWidths", nums(l));
    }
    o.done()
}

// ---------------------------------------------------------------- styles

fn edge(e: &Option<BorderEdge>) -> Option<String> {
    e.as_ref().map(|e| {
        let mut o = Obj::new();
        o.s("style", &e.style).s("color", &e.color);
        o.done()
    })
}

/// `Border`.
pub fn border(b: &Border) -> String {
    let mut o = Obj::new();
    o.oraw("left", edge(&b.left))
        .oraw("right", edge(&b.right))
        .oraw("top", edge(&b.top))
        .oraw("bottom", edge(&b.bottom))
        .oraw("diagonal", edge(&b.diagonal))
        .t("diagonalUp", b.diagonal_up)
        .t("diagonalDown", b.diagonal_down)
        .oraw("vertical", edge(&b.vertical))
        .oraw("horizontal", edge(&b.horizontal));
    o.done()
}

/// `Fill`.
pub fn fill(f: &Fill) -> String {
    let mut o = Obj::new();
    o.os("fgColor", f.fg_color.as_deref()).os("bgColor", f.bg_color.as_deref()).os("pattern", f.pattern.as_deref());
    if let Some(g) = &f.gradient {
        let mut x = Obj::new();
        x.n("degree", g.degree).raw(
            "stops",
            jarr(g.stops.iter().map(|s| {
                let mut y = Obj::new();
                y.n("position", s.position).s("color", &s.color);
                y.done()
            })),
        );
        if let Some([l, r, t, b]) = g.path {
            x.s("type", "path").n("left", l).n("right", r).n("top", t).n("bottom", b);
        }
        o.raw("gradient", x.done());
    }
    o.done()
}

/// `Dxf`.
pub fn dxf(d: &Dxf) -> String {
    let mut o = Obj::new();
    o.os("fillColor", d.fill_color.as_deref()).oraw("fill", d.fill.as_ref().map(fill)).os("fontColor", d.font_color.as_deref());
    o.ob("bold", d.bold).ob("italic", d.italic).ob("underline", d.underline).ob("strike", d.strike);
    o.oraw("border", d.border.as_ref().map(border)).on("numFmtId", d.num_fmt_id).os("numFmtCode", d.num_fmt_code.as_deref());
    o.done()
}

/// `Styles`.
pub fn styles(s: &Styles) -> String {
    let mut o = Obj::new();
    o.raw(
        "numFmts",
        format!("{{\"$map\":[{}]}}", s.num_fmts.iter().map(|(k, v)| format!("[{},{}]", jnum(*k), jstr(v))).collect::<Vec<_>>().join(",")),
    );
    o.raw(
        "cellXfs",
        jarr(s.cell_xfs.iter().map(|x| {
            let mut y = Obj::new();
            y.n("numFmtId", x.num_fmt_id).n("fontId", x.font_id).n("fillId", x.fill_id).n("borderId", x.border_id).n("xfId", x.xf_id);
            y.t("applyNumberFormat", x.apply_number_format)
                .t("applyFont", x.apply_font)
                .t("applyFill", x.apply_fill)
                .t("applyBorder", x.apply_border)
                .t("applyAlignment", x.apply_alignment)
                .t("pivotButton", x.pivot_button)
                .os("horizontal", x.horizontal.as_deref())
                .os("vertical", x.vertical.as_deref())
                .t("wrapText", x.wrap_text)
                .on("indent", x.indent)
                .on("textRotation", x.text_rotation)
                .t("shrinkToFit", x.shrink_to_fit)
                .on("readingOrder", x.reading_order)
                .t("justifyLastLine", x.justify_last_line);
            y.done()
        })),
    );
    o.raw(
        "fonts",
        jarr(s.fonts.iter().map(|f| {
            let mut y = Obj::new();
            y.t("bold", f.bold)
                .t("italic", f.italic)
                .t("underline", f.underline)
                .os("underlineStyle", f.underline_style)
                .t("strike", f.strike);
            y.on("size", f.size)
                .os("color", f.color.as_deref())
                .os("name", f.name.as_deref())
                .os("altName", f.alt_name.as_deref())
                .os("vertAlign", f.vert_align);
            y.done()
        })),
    );
    o.raw("fills", jarr(s.fills.iter().map(fill)))
        .raw("borders", jarr(s.borders.iter().map(border)))
        .raw("dxfs", jarr(s.dxfs.iter().map(dxf)));
    if let Some(ts) = &s.table_styles {
        o.raw(
            "tableStyles",
            jarr(ts.iter().map(|t| {
                let mut y = Obj::new();
                y.s("name", &t.name).b("pivot", t.pivot).b("table", t.table).raw(
                    "elements",
                    jarr(t.elements.iter().map(|e| {
                        let mut z = Obj::new();
                        z.s("type", &e.r#type).on("dxfId", e.dxf_id).on("size", e.size);
                        z.done()
                    })),
                );
                y.done()
            })),
        );
    }
    o.os("defaultTableStyle", s.default_table_style.as_deref()).os("defaultPivotStyle", s.default_pivot_style.as_deref());
    o.done()
}

// ---------------------------------------------------------------- shared strings

/// `RunProps` (the `props` of a rich run).
pub fn run_props_json(p: &RunProps) -> String {
    let mut o = Obj::new();
    o.ob("bold", p.bold).ob("italic", p.italic).ob("underline", p.underline).ob("strike", p.strike).on("size", p.size);
    o.os("color", p.color.as_deref()).os("font", p.font.as_deref()).os("vertAlign", p.vert_align);
    o.done()
}

/// A `SharedString` (`{runs?, text?}`).
pub fn shared_string(it: &InlineText, props: &RunPropsTable) -> String {
    let mut o = Obj::new();
    if let Some(runs) = &it.runs {
        o.raw(
            "runs",
            jarr(runs.iter().map(|r| {
                let mut x = Obj::new();
                x.raw("text", js_json(r.text.as_js()));
                if let Some(rpr) = &r.rpr {
                    let mut a = Obj::new();
                    for (k, v) in rpr.iter() {
                        a.raw(&k.as_js().to_string_lossy(), js_json(v.as_js()));
                    }
                    x.raw("rPr", a.done());
                }
                x.oraw("props", r.props.and_then(|p| props.get(p)).map(run_props_json));
                x.done()
            })),
        );
    }
    o.oraw("text", it.text.as_ref().map(|t| js_json(t.as_js())));
    o.done()
}

/// JSON text of a JS string (lone surrogates as `\udXXX`).
pub fn js_json(s: JsStr<'_>) -> String {
    let mut out = String::new();
    push_str(&mut out, s);
    out
}

// ---------------------------------------------------------------- worksheet

fn pos(p: &AnchorPos) -> String {
    let mut o = Obj::new();
    o.n("col", p.col).n("row", p.row).n("colOff", p.col_off).n("rowOff", p.row_off);
    o.done()
}

fn anchor(a: &DrawingAnchor) -> String {
    let mut o = Obj::new();
    o.raw("from", pos(&a.from)).raw("to", pos(&a.to));
    o.os("kind", a.kind.map(|k| if k == AnchorKind::OneCell { "oneCell" } else { "absolute" })).os("editAs", a.edit_as.as_deref());
    if let Some((cx, cy)) = a.ext {
        let mut e = Obj::new();
        e.n("cx", cx).n("cy", cy);
        o.raw("ext", e.done());
    }
    if let Some((x, y)) = a.pos {
        let mut e = Obj::new();
        e.n("x", x).n("y", y);
        o.raw("pos", e.done());
    }
    o.done()
}

fn cfvo(c: &Cfvo) -> String {
    let mut o = Obj::new();
    o.s("type", &c.r#type).os("val", c.val.as_deref()).on("value", c.value).ob("gte", c.gte);
    o.done()
}

fn rule(r: &CondRule) -> String {
    let mut o = Obj::new();
    o.s("type", &r.r#type).n("priority", r.priority).t("stopIfTrue", r.stop_if_true);
    if let Some(st) = &r.stops {
        o.raw(
            "stops",
            jarr(st.iter().map(|s| {
                let mut x = Obj::new();
                x.s("type", &s.r#type).on("value", s.value).os("val", s.val.as_deref()).s("color", &s.color);
                x.done()
            })),
        );
    }
    o.os("barColor", r.bar_color.as_deref())
        .on("barMinLen", r.bar_min_len)
        .on("barMaxLen", r.bar_max_len)
        .ob("barBorder", r.bar_border)
        .ob("barGradient", r.bar_gradient);
    o.os("barBorderColor", r.bar_border_color.as_deref())
        .os("barNegColor", r.bar_neg_color.as_deref())
        .os("barNegBorderColor", r.bar_neg_border_color.as_deref());
    o.t("barNegSame", r.bar_neg_same)
        .t("barNegBorderSame", r.bar_neg_border_same)
        .os("barAxis", r.bar_axis.as_deref())
        .os("barAxisColor", r.bar_axis_color.as_deref());
    o.os("barDirection", r.bar_direction.as_deref());
    if let Some(c) = &r.cfvos {
        o.raw("cfvos", jarr(c.iter().map(cfvo)));
    }
    o.ob("showValue", r.show_value)
        .t("x14", r.x14)
        .os("x14Id", r.x14_id.as_deref())
        .os("iconSet", r.icon_set.as_deref())
        .t("iconReverse", r.icon_reverse);
    if let Some(ic) = &r.icons {
        o.raw(
            "icons",
            jarr(ic.iter().map(|i| {
                let mut x = Obj::new();
                x.s("set", &i.set).n("id", i.id);
                x.done()
            })),
        );
    }
    o.os("operator", r.operator.as_deref());
    if let Some(f) = &r.formula {
        o.raw("formula", strs(f));
    }
    o.on("dxfId", r.dxf_id)
        .oraw("dxf", r.dxf.as_ref().map(dxf))
        .os("text", r.text.as_deref())
        .os("timePeriod", r.time_period.as_deref())
        .on("rank", r.rank);
    o.t("percent", r.percent)
        .t("bottom", r.bottom)
        .ob("aboveAverage", r.above_average)
        .t("equalAverage", r.equal_average)
        .on("stdDev", r.std_dev);
    o.done()
}

fn auto_filter(a: &AutoFilterInfo) -> String {
    let mut o = Obj::new();
    o.s("ref", &a.r#ref);
    if !a.hidden.is_empty() {
        o.raw("hidden", nums(&a.hidden));
    }
    if !a.active.is_empty() {
        o.raw("active", nums(&a.active));
    }
    o.done()
}

fn pivot_items(v: &[PivotItem]) -> String {
    jarr(v.iter().map(|i| {
        let mut o = Obj::new();
        o.os("t", i.t.as_deref()).n("r", i.r).n("x", i.x);
        o.done()
    }))
}

/// A worksheet model without rows (`ws<i>` of the capture).
pub fn worksheet(w: &Worksheet) -> String {
    let mut o = Obj::new();
    if !w.merges.is_empty() {
        o.raw("merges", strs(&w.merges));
        o.raw(
            "mergedAreas",
            format!(
                "{{\"$map\":[{}]}}",
                w.merged_areas.iter().map(|(k, v)| format!("[{},{}]", jstr(k), jstr(v))).collect::<Vec<_>>().join(",")
            ),
        );
    }
    if !w.cols.is_empty() {
        o.raw(
            "cols",
            jarr(w.cols.iter().map(|c| {
                let mut x = Obj::new();
                x.n("min", c.min)
                    .n("max", c.max)
                    .on("width", c.width)
                    .t("customWidth", c.custom_width)
                    .t("hidden", c.hidden)
                    .on("style", c.style);
                x.done()
            })),
        );
    }
    if let Some(p) = &w.pane {
        let mut x = Obj::new();
        x.n("xSplit", p.x_split).n("ySplit", p.y_split).os("topLeftCell", p.top_left_cell.as_deref()).s("state", &p.state);
        o.raw("pane", x.done());
    }
    if w.show_grid_lines_off {
        o.b("showGridLines", false);
    }
    if w.show_zeros_off {
        o.b("showZeros", false);
    }
    o.t("rightToLeft", w.right_to_left);
    if !w.ignore_number_as_text.is_empty() {
        o.raw("ignoreNumberAsText", strs(&w.ignore_number_as_text));
    }
    if !w.cond_formats.is_empty() {
        o.raw(
            "condFormats",
            jarr(w.cond_formats.iter().map(|c| {
                let mut x = Obj::new();
                x.s("sqref", &c.sqref).raw("rules", jarr(c.rules.iter().map(rule)));
                x.done()
            })),
        );
    }
    if !w.comments.is_empty() {
        o.raw(
            "comments",
            jarr(w.comments.iter().map(|c| {
                let mut x = Obj::new();
                x.s("ref", &c.r#ref).s("text", &c.text).os("author", c.author.as_deref());
                x.done()
            })),
        );
    }
    if !w.charts.is_empty() {
        o.raw(
            "charts",
            jarr(w.charts.iter().map(|c| {
                let mut x = Obj::new();
                x.raw("anchor", anchor(&c.anchor))
                    .s("chartXml", &c.chart_xml)
                    .on("order", c.order)
                    .os("name", c.name.as_deref())
                    .os("part", c.part.as_deref());
                if c.chartex {
                    x.s("kind", "chartex");
                }
                x.done()
            })),
        );
    }
    if !w.drawings.is_empty() {
        o.raw(
            "drawings",
            jarr(w.drawings.iter().map(|d| {
                let mut x = Obj::new();
                x.raw("anchor", anchor(&d.anchor)).n("order", d.order).raw("el", xel(&d.el)).s("part", &d.part);
                if let Some(c) = &d.control {
                    let mut y = Obj::new();
                    y.s("type", &c.r#type).os("checked", c.checked.as_deref());
                    x.raw("control", y.done());
                }
                x.done()
            })),
        );
    }
    if let Some(c) = &w.chartsheet {
        let mut x = Obj::new();
        x.n("zoom", c.zoom).b("zoomToFit", c.zoom_to_fit);
        o.raw("chartsheet", x.done());
    }
    if !w.hyperlinks.is_empty() {
        o.raw(
            "hyperlinks",
            jarr(w.hyperlinks.iter().map(|h| {
                let mut x = Obj::new();
                x.s("ref", &h.r#ref)
                    .os("target", h.target.as_deref())
                    .os("location", h.location.as_deref())
                    .os("display", h.display.as_deref())
                    .os("tooltip", h.tooltip.as_deref());
                x.done()
            })),
        );
    }
    if let Some(s) = &w.sheet_format_pr {
        let mut x = Obj::new();
        x.on("defaultColWidth", s.default_col_width)
            .on("baseColWidth", s.base_col_width)
            .on("defaultRowHeight", s.default_row_height)
            .t("customHeight", s.custom_height)
            .t("zeroHeight", s.zero_height);
        o.raw("sheetFormatPr", x.done());
    }
    if !w.tables.is_empty() {
        o.raw(
            "tables",
            jarr(w.tables.iter().map(|t| {
                let mut x = Obj::new();
                x.s("ref", &t.r#ref).os("name", t.name.as_deref()).os("displayName", t.display_name.as_deref());
                x.on("headerRowCount", t.header_row_count).on("totalsRowCount", t.totals_row_count);
                if let Some(cols) = &t.columns {
                    x.raw(
                        "columns",
                        jarr(cols.iter().map(|c| {
                            let mut y = Obj::new();
                            y.s("name", &c.name)
                                .os("calculatedColumnFormula", c.calculated_column_formula.as_deref())
                                .os("totalsRowFunction", c.totals_row_function.as_deref())
                                .os("totalsRowLabel", c.totals_row_label.as_deref())
                                .os("totalsRowFormula", c.totals_row_formula.as_deref());
                            y.done()
                        })),
                    );
                }
                x.b("headerRow", t.header_row)
                    .b("totalsRow", t.totals_row)
                    .b("stripes", t.stripes)
                    .os("styleName", t.style_name.as_deref())
                    .b("autoFilter", t.auto_filter);
                x.t("colStripes", t.col_stripes).t("firstColumn", t.first_column).t("lastColumn", t.last_column);
                x.oraw("filter", t.filter.as_ref().map(auto_filter)).on("borderDxfId", t.border_dxf_id);
                x.done()
            })),
        );
    }
    if let Some(a) = &w.auto_filter {
        o.raw("autoFilter", auto_filter(a));
    }
    o.t("filterMode", w.filter_mode);
    if !w.pivots.is_empty() {
        o.raw(
            "pivots",
            jarr(w.pivots.iter().map(|p| {
                let mut x = Obj::new();
                x.s("ref", &p.r#ref)
                    .n("firstHeaderRow", p.first_header_row)
                    .n("firstDataRow", p.first_data_row)
                    .n("firstDataCol", p.first_data_col)
                    .n("rowPageCount", p.row_page_count)
                    .n("colPageCount", p.col_page_count)
                    .n("pageFieldCount", p.page_field_count);
                if let Some(a) = &p.page_field_active {
                    x.raw("pageFieldActive", jarr(a.iter().map(|b| b.to_string())));
                }
                x.raw("rowFields", nums(&p.row_fields))
                    .raw("colFields", nums(&p.col_fields))
                    .raw("rowItems", pivot_items(&p.row_items))
                    .raw("colItems", pivot_items(&p.col_items));
                x.b("compact", p.compact)
                    .b("outline", p.outline)
                    .b("showHeaders", p.show_headers)
                    .b("rowGrandTotals", p.row_grand_totals)
                    .b("colGrandTotals", p.col_grand_totals);
                x.os("styleName", p.style_name.as_deref());
                x.b("showRowHeaders", p.show_row_headers)
                    .b("showColHeaders", p.show_col_headers)
                    .b("showRowStripes", p.show_row_stripes)
                    .b("showColStripes", p.show_col_stripes)
                    .b("showLastColumn", p.show_last_column);
                if !p.filtered_fields.is_empty() {
                    x.raw("filteredFields", nums(&p.filtered_fields));
                }
                x.done()
            })),
        );
    }
    if !w.list_validations.is_empty() {
        o.raw("listValidations", strs(&w.list_validations));
    }
    o.done()
}
