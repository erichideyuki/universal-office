//! Frontier **W3** (table props; `table-xml.ts` and the legacy generators of `serialize.ts`):
//! `bordersXml`, `shdXml`, `widthXml`, `marXml`, `lookXml`, `tblpPrXml`, the `*_GEN` tables,
//! `genAll`, `tblPrXml`, `tblPrExXml`, `trPrXml`, `tcPrXml`, `deepEq`, its own `setKid`, `patch`,
//! `patchTblPr`, `patchTblPrEx`, `patchTrPr`, `patchTcPr`; serialize.ts `tblPrXml` (legacy),
//! `legacyOrTypedTblPr`, `tcPrXml` (legacy), `borderXml`/`borderSetXml` (legacy, `w:space="0"`).
//!
//! Ported already: [`border_xml`] and [`pg_borders_xml`] (the section surgery), [`grid_xml`].

use uo_opc::xml::raw::{Kid, Split, set_kid};

use crate::js::{JsNumber, Str};
use crate::model::para::BorderEdge;
use crate::model::props::{
    BorderSpec, CellMargins, ShadingSpec, TblBorders, TblLook, TblPr, TblpPr, TcBorders, TcPr, TrHeight, TrPr, WidthSpec,
};
use crate::model::section::PageBorders;
use crate::model::table::{Cell, Table, TableBorders};
use crate::read::props::{read_tbl_pr, read_tc_pr, read_tr_pr};
use crate::write::ctx::WriteCtx;
use crate::write::js::{esc_attr, num_str, opt_num_str, strict_ne, truthy};
use crate::write::lossless::{WResult, split};

/// `attrs(pairs)` of table-xml.ts: ` w:k="esc(String(v))"` for the defined values.
fn attrs(pairs: &[(&str, Option<String>)]) -> String {
    let mut s = String::new();
    for (k, v) in pairs {
        if let Some(v) = v {
            s.push_str(&format!(" w:{k}=\"{}\"", esc_attr(v)));
        }
    }
    s
}

/// `borderXml(tag, b)` (table-xml.ts:48).
#[must_use]
pub fn border_xml(tag: &str, b: &BorderSpec) -> String {
    let nil = b.val.as_ref() == "nil";
    let space = match b.space {
        Some(JsNumber::Num(v)) => Some(num_str(JsNumber::Num(v))),
        Some(JsNumber::Null) | None => (!nil).then(|| "0".to_owned()),
    };
    let color = match b.color.as_deref() {
        Some(c) => Some(c.to_owned()),
        None => (!nil).then(|| "auto".to_owned()),
    };
    let flag = |v: Option<bool>| v.map(|v| if v { "1".to_owned() } else { "0".to_owned() });
    let a = attrs(&[
        ("val", Some(b.val.to_string())),
        ("sz", b.sz.map(num_str)),
        ("space", space),
        ("color", color),
        ("themeColor", b.theme_color.as_deref().map(str::to_owned)),
        ("themeTint", b.theme_tint.as_deref().map(str::to_owned)),
        ("themeShade", b.theme_shade.as_deref().map(str::to_owned)),
        ("shadow", flag(b.shadow)),
        ("frame", flag(b.frame)),
    ]);
    format!("<w:{tag}{a}/>")
}

/// `pgBordersXml(pb)`: `None` = no element.
#[must_use]
pub fn pg_borders_xml(pb: Option<&PageBorders>) -> Option<String> {
    let pb = pb?;
    let inner: String = [("top", &pb.top), ("left", &pb.left), ("bottom", &pb.bottom), ("right", &pb.right)]
        .iter()
        .map(|(s, b)| b.as_ref().map(|b| border_xml(s, b)).unwrap_or_default())
        .collect();
    if inner.is_empty() {
        return None;
    }
    let a = attrs(&[
        ("zOrder", pb.z_order.as_deref().map(str::to_owned)),
        ("display", pb.display.as_deref().map(str::to_owned)),
        ("offsetFrom", pb.offset_from.as_deref().map(str::to_owned)),
    ]);
    Some(format!("<w:pgBorders{a}>{inner}</w:pgBorders>"))
}

/// `gridXml(t)`: `<w:tblGrid>` of the grid columns.
#[must_use]
pub fn grid_xml(t: &Table) -> String {
    let cols: String = t.grid_cols_twips.iter().map(|w| format!("<w:gridCol w:w=\"{}\"/>", num_str(*w))).collect();
    format!("<w:tblGrid>{cols}</w:tblGrid>")
}

/// CT_TblPr (table-xml.ts:24).
const TBLPR_ORDER: &[&str] = &[
    "w:tblStyle",
    "w:tblpPr",
    "w:tblOverlap",
    "w:bidiVisual",
    "w:tblStyleRowBandSize",
    "w:tblStyleColBandSize",
    "w:tblW",
    "w:jc",
    "w:tblCellSpacing",
    "w:tblInd",
    "w:tblBorders",
    "w:shd",
    "w:tblLayout",
    "w:tblCellMar",
    "w:tblLook",
    "w:tblCaption",
    "w:tblDescription",
    "w:tblPrChange",
];
/// CT_TblPrEx (table-xml.ts:30).
const TBLPREX_ORDER: &[&str] = &[
    "w:tblW",
    "w:jc",
    "w:tblCellSpacing",
    "w:tblInd",
    "w:tblBorders",
    "w:shd",
    "w:tblLayout",
    "w:tblCellMar",
    "w:tblLook",
    "w:tblPrExChange",
];
/// CT_TrPr, in the order Word writes (table-xml.ts:35).
const TRPR_ORDER: &[&str] = &[
    "w:cnfStyle",
    "w:divId",
    "w:gridBefore",
    "w:gridAfter",
    "w:wBefore",
    "w:wAfter",
    "w:cantSplit",
    "w:trHeight",
    "w:tblHeader",
    "w:tblCellSpacing",
    "w:jc",
    "w:hidden",
    "w:ins",
    "w:del",
    "w:trPrChange",
];
/// CT_TcPr (table-xml.ts:40).
const TCPR_ORDER: &[&str] = &[
    "w:cnfStyle",
    "w:tcW",
    "w:gridSpan",
    "w:hMerge",
    "w:vMerge",
    "w:tcBorders",
    "w:shd",
    "w:noWrap",
    "w:tcMar",
    "w:textDirection",
    "w:tcFitText",
    "w:vAlign",
    "w:hideMark",
    "w:headers",
    "w:cellIns",
    "w:cellDel",
    "w:cellMerge",
    "w:tcPrChange",
];

/// The `TblPr` keys that count in `w:tblPrEx` (`EX_KEYS`, table-xml.ts:160).
const EX_KEYS: &[&str] = &["tblW", "jc", "tblCellSpacing", "tblInd", "tblBorders", "shd", "tblLayout", "tblCellMar", "tblLook"];

// ───────────────────────── elements ─────────────────────────

/// `bordersXml(tag, b)` over the eight sides in `SIDES` order (table-xml.ts:65): `""` when no side.
fn borders_xml(tag: &str, sides: [(&str, Option<&BorderSpec>); 8]) -> String {
    let inner: String = sides.iter().map(|(s, b)| b.map(|b| border_xml(s, b)).unwrap_or_default()).collect();
    if inner.is_empty() { String::new() } else { format!("<w:{tag}>{inner}</w:{tag}>") }
}

fn tbl_borders_xml(tag: &str, b: &TblBorders) -> String {
    borders_xml(
        tag,
        [
            ("top", b.top.as_deref()),
            ("left", b.left.as_deref()),
            ("bottom", b.bottom.as_deref()),
            ("right", b.right.as_deref()),
            ("insideH", b.inside_h.as_deref()),
            ("insideV", b.inside_v.as_deref()),
            ("tl2br", b.tl2br.as_deref()),
            ("tr2bl", b.tr2bl.as_deref()),
        ],
    )
}

fn tc_borders_xml(tag: &str, b: &TcBorders) -> String {
    borders_xml(
        tag,
        [
            ("top", b.top.as_deref()),
            ("left", b.left.as_deref()),
            ("bottom", b.bottom.as_deref()),
            ("right", b.right.as_deref()),
            ("insideH", b.inside_h.as_deref()),
            ("insideV", b.inside_v.as_deref()),
            ("tl2br", b.tl2br.as_deref()),
            ("tr2bl", b.tr2bl.as_deref()),
        ],
    )
}

/// `shdXml(s)` (table-xml.ts:72).
#[must_use]
pub fn shd_xml(s: &ShadingSpec) -> String {
    let a = attrs(&[
        ("val", Some(s.val.as_deref().unwrap_or("clear").to_owned())),
        ("color", Some(s.color.as_deref().unwrap_or("auto").to_owned())),
        ("themeColor", s.theme_color.as_deref().map(str::to_owned)),
        ("fill", s.fill.as_deref().map(str::to_owned)),
        ("themeFill", s.theme_fill.as_deref().map(str::to_owned)),
        ("themeFillTint", s.theme_fill_tint.as_deref().map(str::to_owned)),
        ("themeFillShade", s.theme_fill_shade.as_deref().map(str::to_owned)),
    ]);
    format!("<w:shd{a}/>")
}

/// `widthXml(tag, w)` (table-xml.ts:79).
fn width_xml(tag: &str, w: &WidthSpec) -> String {
    let a = attrs(&[("w", Some(num_str(w.w))), ("type", Some(w.r#type.to_string()))]);
    format!("<w:{tag}{a}/>")
}

/// `marXml(tag, m)` (table-xml.ts:81): `""` when no side.
fn mar_xml(tag: &str, m: &CellMargins) -> String {
    let inner: String = [("top", &m.top), ("left", &m.left), ("bottom", &m.bottom), ("right", &m.right)]
        .iter()
        .map(|(k, w)| w.as_ref().map(|w| width_xml(k, w)).unwrap_or_default())
        .collect();
    if inner.is_empty() { String::new() } else { format!("<w:{tag}>{inner}</w:{tag}>") }
}

/// `lookXml(l)` (table-xml.ts:87): the legacy hexadecimal and the attributes.
#[must_use]
pub fn look_xml(l: &TblLook) -> String {
    let on = |v: Option<bool>| v == Some(true);
    let bits: u32 = (if on(l.first_row) { 0x20 } else { 0 })
        | (if on(l.last_row) { 0x40 } else { 0 })
        | (if on(l.first_column) { 0x80 } else { 0 })
        | (if on(l.last_column) { 0x100 } else { 0 })
        | (if on(l.no_h_band) { 0x200 } else { 0 })
        | (if on(l.no_v_band) { 0x400 } else { 0 });
    let b = |v: Option<bool>| u8::from(on(v));
    format!(
        "<w:tblLook w:val=\"{bits:04X}\" w:firstRow=\"{}\" w:lastRow=\"{}\" w:firstColumn=\"{}\" w:lastColumn=\"{}\" w:noHBand=\"{}\" w:noVBand=\"{}\"/>",
        b(l.first_row),
        b(l.last_row),
        b(l.first_column),
        b(l.last_column),
        b(l.no_h_band),
        b(l.no_v_band)
    )
}

fn tblp_pr_xml(p: &TblpPr) -> String {
    let n = |v: Option<JsNumber>| v.map(num_str);
    let s = |v: &Option<Str>| v.as_deref().map(str::to_owned);
    let a = attrs(&[
        ("leftFromText", n(p.left_from_text)),
        ("rightFromText", n(p.right_from_text)),
        ("topFromText", n(p.top_from_text)),
        ("bottomFromText", n(p.bottom_from_text)),
        ("vertAnchor", s(&p.vert_anchor)),
        ("horzAnchor", s(&p.horz_anchor)),
        ("tblpXSpec", s(&p.tblp_x_spec)),
        ("tblpX", n(p.tblp_x)),
        ("tblpYSpec", s(&p.tblp_y_spec)),
        ("tblpY", n(p.tblp_y)),
    ]);
    format!("<w:tblpPr{a}/>")
}

fn on_off(tag: &str, v: bool) -> String {
    if v { format!("<w:{tag}/>") } else { format!("<w:{tag} w:val=\"0\"/>") }
}

fn val(tag: &str, v: &str) -> String {
    format!("<w:{tag} w:val=\"{}\"/>", esc_attr(v))
}

fn val_num(tag: &str, v: JsNumber) -> String {
    val(tag, &num_str(v))
}

// ───────────────────────── the generator tables ─────────────────────────

/// One entry of a `Gen<T>` table: the TS key, the schema name, the structural equality of the
/// field (`deepEq`) and its XML (`None` for `undefined` or `null`, which `genAll` skips).
struct Gen<T> {
    key: &'static str,
    name: &'static str,
    eq: fn(&T, &T) -> bool,
    xml: fn(&T) -> Option<String>,
}

/// A number field of `genAll`: `null` is skipped like `undefined`.
fn num_v(v: Option<JsNumber>) -> Option<JsNumber> {
    v.filter(|v| matches!(v, JsNumber::Num(_)))
}

fn eq_str(a: &Option<Str>, b: &Option<Str>) -> bool {
    a == b
}

/// `deepEq` of two `number | null | undefined`: `===`, so NaN differs from itself.
fn eq_num(a: Option<JsNumber>, b: Option<JsNumber>) -> bool {
    !strict_ne(a, b)
}

fn eq_width(a: &Option<WidthSpec>, b: &Option<WidthSpec>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => eq_num(Some(a.w), Some(b.w)) && a.r#type == b.r#type,
        _ => false,
    }
}

fn eq_border(a: Option<&BorderSpec>, b: Option<&BorderSpec>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.val == b.val
                && eq_num(a.sz, b.sz)
                && eq_num(a.space, b.space)
                && a.color == b.color
                && a.theme_color == b.theme_color
                && a.theme_tint == b.theme_tint
                && a.theme_shade == b.theme_shade
                && a.shadow == b.shadow
                && a.frame == b.frame
        }
        _ => false,
    }
}

fn eq_tbl_borders(a: &Option<Box<TblBorders>>, b: &Option<Box<TblBorders>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            eq_border(a.top.as_deref(), b.top.as_deref())
                && eq_border(a.left.as_deref(), b.left.as_deref())
                && eq_border(a.bottom.as_deref(), b.bottom.as_deref())
                && eq_border(a.right.as_deref(), b.right.as_deref())
                && eq_border(a.inside_h.as_deref(), b.inside_h.as_deref())
                && eq_border(a.inside_v.as_deref(), b.inside_v.as_deref())
                && eq_border(a.tl2br.as_deref(), b.tl2br.as_deref())
                && eq_border(a.tr2bl.as_deref(), b.tr2bl.as_deref())
        }
        _ => false,
    }
}

fn eq_tc_borders(a: &Option<Box<TcBorders>>, b: &Option<Box<TcBorders>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            eq_border(a.top.as_deref(), b.top.as_deref())
                && eq_border(a.left.as_deref(), b.left.as_deref())
                && eq_border(a.bottom.as_deref(), b.bottom.as_deref())
                && eq_border(a.right.as_deref(), b.right.as_deref())
                && eq_border(a.inside_h.as_deref(), b.inside_h.as_deref())
                && eq_border(a.inside_v.as_deref(), b.inside_v.as_deref())
                && eq_border(a.tl2br.as_deref(), b.tl2br.as_deref())
                && eq_border(a.tr2bl.as_deref(), b.tr2bl.as_deref())
        }
        _ => false,
    }
}

fn eq_shd(a: &Option<Box<ShadingSpec>>, b: &Option<Box<ShadingSpec>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.val == b.val
                && a.color == b.color
                && a.fill == b.fill
                && a.theme_fill == b.theme_fill
                && a.theme_fill_tint == b.theme_fill_tint
                && a.theme_fill_shade == b.theme_fill_shade
                && a.theme_color == b.theme_color
        }
        _ => false,
    }
}

fn eq_mar(a: &Option<Box<CellMargins>>, b: &Option<Box<CellMargins>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            eq_width(&a.top, &b.top) && eq_width(&a.left, &b.left) && eq_width(&a.bottom, &b.bottom) && eq_width(&a.right, &b.right)
        }
        _ => false,
    }
}

fn eq_look(a: &Option<Box<TblLook>>, b: &Option<Box<TblLook>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.first_row == b.first_row
                && a.last_row == b.last_row
                && a.first_column == b.first_column
                && a.last_column == b.last_column
                && a.no_h_band == b.no_h_band
                && a.no_v_band == b.no_v_band
        }
        _ => false,
    }
}

fn eq_tblp(a: &Option<Box<TblpPr>>, b: &Option<Box<TblpPr>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            eq_num(a.left_from_text, b.left_from_text)
                && eq_num(a.right_from_text, b.right_from_text)
                && eq_num(a.top_from_text, b.top_from_text)
                && eq_num(a.bottom_from_text, b.bottom_from_text)
                && a.vert_anchor == b.vert_anchor
                && a.horz_anchor == b.horz_anchor
                && a.tblp_x_spec == b.tblp_x_spec
                && eq_num(a.tblp_x, b.tblp_x)
                && a.tblp_y_spec == b.tblp_y_spec
                && eq_num(a.tblp_y, b.tblp_y)
        }
        _ => false,
    }
}

fn eq_tr_height(a: &Option<TrHeight>, b: &Option<TrHeight>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => eq_num(Some(a.val), Some(b.val)) && a.h_rule == b.h_rule,
        _ => false,
    }
}

const TBLPR_GEN: &[Gen<TblPr>] = &[
    Gen {
        key: "tblStyle",
        name: "w:tblStyle",
        eq: |a, b| eq_str(&a.tbl_style, &b.tbl_style),
        xml: |p| p.tbl_style.as_deref().map(|v| val("tblStyle", v)),
    },
    Gen { key: "tblpPr", name: "w:tblpPr", eq: |a, b| eq_tblp(&a.tblp_pr, &b.tblp_pr), xml: |p| p.tblp_pr.as_deref().map(tblp_pr_xml) },
    Gen {
        key: "tblOverlap",
        name: "w:tblOverlap",
        eq: |a, b| eq_str(&a.tbl_overlap, &b.tbl_overlap),
        xml: |p| p.tbl_overlap.as_deref().map(|v| val("tblOverlap", v)),
    },
    Gen {
        key: "bidiVisual",
        name: "w:bidiVisual",
        eq: |a, b| a.bidi_visual == b.bidi_visual,
        xml: |p| p.bidi_visual.map(|v| on_off("bidiVisual", v)),
    },
    Gen {
        key: "tblStyleRowBandSize",
        name: "w:tblStyleRowBandSize",
        eq: |a, b| eq_num(a.tbl_style_row_band_size, b.tbl_style_row_band_size),
        xml: |p| num_v(p.tbl_style_row_band_size).map(|v| val_num("tblStyleRowBandSize", v)),
    },
    Gen {
        key: "tblStyleColBandSize",
        name: "w:tblStyleColBandSize",
        eq: |a, b| eq_num(a.tbl_style_col_band_size, b.tbl_style_col_band_size),
        xml: |p| num_v(p.tbl_style_col_band_size).map(|v| val_num("tblStyleColBandSize", v)),
    },
    Gen { key: "tblW", name: "w:tblW", eq: |a, b| eq_width(&a.tbl_w, &b.tbl_w), xml: |p| p.tbl_w.as_ref().map(|v| width_xml("tblW", v)) },
    Gen { key: "jc", name: "w:jc", eq: |a, b| eq_str(&a.jc, &b.jc), xml: |p| p.jc.as_deref().map(|v| val("jc", v)) },
    Gen {
        key: "tblCellSpacing",
        name: "w:tblCellSpacing",
        eq: |a, b| eq_width(&a.tbl_cell_spacing, &b.tbl_cell_spacing),
        xml: |p| p.tbl_cell_spacing.as_ref().map(|v| width_xml("tblCellSpacing", v)),
    },
    Gen {
        key: "tblInd",
        name: "w:tblInd",
        eq: |a, b| eq_width(&a.tbl_ind, &b.tbl_ind),
        xml: |p| p.tbl_ind.as_ref().map(|v| width_xml("tblInd", v)),
    },
    Gen {
        key: "tblBorders",
        name: "w:tblBorders",
        eq: |a, b| eq_tbl_borders(&a.tbl_borders, &b.tbl_borders),
        xml: |p| p.tbl_borders.as_deref().map(|v| tbl_borders_xml("tblBorders", v)),
    },
    Gen { key: "shd", name: "w:shd", eq: |a, b| eq_shd(&a.shd, &b.shd), xml: |p| p.shd.as_deref().map(shd_xml) },
    Gen {
        key: "tblLayout",
        name: "w:tblLayout",
        eq: |a, b| eq_str(&a.tbl_layout, &b.tbl_layout),
        xml: |p| p.tbl_layout.as_deref().map(|v| format!("<w:tblLayout w:type=\"{v}\"/>")),
    },
    Gen {
        key: "tblCellMar",
        name: "w:tblCellMar",
        eq: |a, b| eq_mar(&a.tbl_cell_mar, &b.tbl_cell_mar),
        xml: |p| p.tbl_cell_mar.as_deref().map(|v| mar_xml("tblCellMar", v)),
    },
    Gen { key: "tblLook", name: "w:tblLook", eq: |a, b| eq_look(&a.tbl_look, &b.tbl_look), xml: |p| p.tbl_look.as_deref().map(look_xml) },
    Gen {
        key: "tblCaption",
        name: "w:tblCaption",
        eq: |a, b| eq_str(&a.tbl_caption, &b.tbl_caption),
        xml: |p| p.tbl_caption.as_deref().map(|v| val("tblCaption", v)),
    },
    Gen {
        key: "tblDescription",
        name: "w:tblDescription",
        eq: |a, b| eq_str(&a.tbl_description, &b.tbl_description),
        xml: |p| p.tbl_description.as_deref().map(|v| val("tblDescription", v)),
    },
];

const TRPR_GEN: &[Gen<TrPr>] = &[
    Gen {
        key: "cnfStyle",
        name: "w:cnfStyle",
        eq: |a, b| eq_str(&a.cnf_style, &b.cnf_style),
        xml: |p| p.cnf_style.as_deref().map(|v| val("cnfStyle", v)),
    },
    Gen {
        key: "gridBefore",
        name: "w:gridBefore",
        eq: |a, b| eq_num(a.grid_before, b.grid_before),
        xml: |p| num_v(p.grid_before).map(|v| val_num("gridBefore", v)),
    },
    Gen {
        key: "gridAfter",
        name: "w:gridAfter",
        eq: |a, b| eq_num(a.grid_after, b.grid_after),
        xml: |p| num_v(p.grid_after).map(|v| val_num("gridAfter", v)),
    },
    Gen {
        key: "wBefore",
        name: "w:wBefore",
        eq: |a, b| eq_width(&a.w_before, &b.w_before),
        xml: |p| p.w_before.as_ref().map(|v| width_xml("wBefore", v)),
    },
    Gen {
        key: "wAfter",
        name: "w:wAfter",
        eq: |a, b| eq_width(&a.w_after, &b.w_after),
        xml: |p| p.w_after.as_ref().map(|v| width_xml("wAfter", v)),
    },
    Gen {
        key: "cantSplit",
        name: "w:cantSplit",
        eq: |a, b| a.cant_split == b.cant_split,
        xml: |p| p.cant_split.map(|v| on_off("cantSplit", v)),
    },
    Gen {
        key: "trHeight",
        name: "w:trHeight",
        eq: |a, b| eq_tr_height(&a.tr_height, &b.tr_height),
        xml: |p| {
            p.tr_height.as_ref().map(|v| {
                let rule = if &*v.h_rule == "atLeast" { String::new() } else { format!(" w:hRule=\"{}\"", v.h_rule) };
                format!("<w:trHeight{rule} w:val=\"{}\"/>", num_str(v.val))
            })
        },
    },
    Gen {
        key: "tblHeader",
        name: "w:tblHeader",
        eq: |a, b| a.tbl_header == b.tbl_header,
        xml: |p| p.tbl_header.map(|v| on_off("tblHeader", v)),
    },
    Gen {
        key: "tblCellSpacing",
        name: "w:tblCellSpacing",
        eq: |a, b| eq_width(&a.tbl_cell_spacing, &b.tbl_cell_spacing),
        xml: |p| p.tbl_cell_spacing.as_ref().map(|v| width_xml("tblCellSpacing", v)),
    },
    Gen { key: "jc", name: "w:jc", eq: |a, b| eq_str(&a.jc, &b.jc), xml: |p| p.jc.as_deref().map(|v| val("jc", v)) },
    Gen { key: "hidden", name: "w:hidden", eq: |a, b| a.hidden == b.hidden, xml: |p| p.hidden.map(|v| on_off("hidden", v)) },
];

fn merge_xml(tag: &str, v: &str) -> String {
    if v == "restart" { val(tag, "restart") } else { format!("<w:{tag}/>") }
}

const TCPR_GEN: &[Gen<TcPr>] = &[
    Gen {
        key: "cnfStyle",
        name: "w:cnfStyle",
        eq: |a, b| eq_str(&a.cnf_style, &b.cnf_style),
        xml: |p| p.cnf_style.as_deref().map(|v| val("cnfStyle", v)),
    },
    Gen { key: "tcW", name: "w:tcW", eq: |a, b| eq_width(&a.tc_w, &b.tc_w), xml: |p| p.tc_w.as_ref().map(|v| width_xml("tcW", v)) },
    Gen {
        key: "gridSpan",
        name: "w:gridSpan",
        eq: |a, b| eq_num(a.grid_span, b.grid_span),
        xml: |p| num_v(p.grid_span).map(|v| val_num("gridSpan", v)),
    },
    Gen {
        key: "hMerge",
        name: "w:hMerge",
        eq: |a, b| eq_str(&a.h_merge, &b.h_merge),
        xml: |p| p.h_merge.as_deref().map(|v| merge_xml("hMerge", v)),
    },
    Gen {
        key: "vMerge",
        name: "w:vMerge",
        eq: |a, b| eq_str(&a.v_merge, &b.v_merge),
        xml: |p| p.v_merge.as_deref().map(|v| merge_xml("vMerge", v)),
    },
    Gen {
        key: "tcBorders",
        name: "w:tcBorders",
        eq: |a, b| eq_tc_borders(&a.tc_borders, &b.tc_borders),
        xml: |p| p.tc_borders.as_deref().map(|v| tc_borders_xml("tcBorders", v)),
    },
    Gen { key: "shd", name: "w:shd", eq: |a, b| eq_shd(&a.shd, &b.shd), xml: |p| p.shd.as_deref().map(shd_xml) },
    Gen { key: "noWrap", name: "w:noWrap", eq: |a, b| a.no_wrap == b.no_wrap, xml: |p| p.no_wrap.map(|v| on_off("noWrap", v)) },
    Gen {
        key: "tcMar",
        name: "w:tcMar",
        eq: |a, b| eq_mar(&a.tc_mar, &b.tc_mar),
        xml: |p| p.tc_mar.as_deref().map(|v| mar_xml("tcMar", v)),
    },
    Gen {
        key: "textDirection",
        name: "w:textDirection",
        eq: |a, b| eq_str(&a.text_direction, &b.text_direction),
        xml: |p| p.text_direction.as_deref().map(|v| val("textDirection", v)),
    },
    Gen {
        key: "tcFitText",
        name: "w:tcFitText",
        eq: |a, b| a.tc_fit_text == b.tc_fit_text,
        xml: |p| p.tc_fit_text.map(|v| on_off("tcFitText", v)),
    },
    Gen {
        key: "vAlign",
        name: "w:vAlign",
        eq: |a, b| eq_str(&a.v_align, &b.v_align),
        xml: |p| p.v_align.as_deref().map(|v| val("vAlign", v)),
    },
    Gen { key: "hideMark", name: "w:hideMark", eq: |a, b| a.hide_mark == b.hide_mark, xml: |p| p.hide_mark.map(|v| on_off("hideMark", v)) },
];

/// `order.indexOf(name)`.
fn rank(order: &[&str], name: &str) -> isize {
    order.iter().position(|o| *o == name).map_or(-1, |i| i as isize)
}

/// `genAll(p, gen, order, only)` (table-xml.ts:162): the children in schema order, the stable
/// sort of the entries in `Object.keys(gen)` order.
fn gen_all<T>(p: &T, table: &[Gen<T>], order: &[&str], only: Option<&[&str]>) -> String {
    let mut parts: Vec<(isize, String)> = Vec::new();
    for g in table {
        if only.is_some_and(|o| !o.contains(&g.key)) {
            continue;
        }
        if let Some(x) = (g.xml)(p) {
            parts.push((rank(order, g.name), x));
        }
    }
    parts.sort_by_key(|p| p.0);
    parts.into_iter().map(|p| p.1).collect()
}

/// `tblPrXml(p)` (typed, table-xml.ts:174).
#[must_use]
pub fn tbl_pr_xml(p: &TblPr) -> String {
    format!("<w:tblPr>{}</w:tblPr>", gen_all(p, TBLPR_GEN, TBLPR_ORDER, None))
}

/// `tblPrExXml(p)` (typed, table-xml.ts:175): `""` when nothing of `EX_KEYS` is set.
pub fn tbl_pr_ex_xml(_cx: &mut WriteCtx, p: &TblPr) -> WResult<String> {
    let inner = gen_all(p, TBLPR_GEN, TBLPREX_ORDER, Some(EX_KEYS));
    Ok(if inner.is_empty() { String::new() } else { format!("<w:tblPrEx>{inner}</w:tblPrEx>") })
}

/// `trPrXml(p)` (typed, table-xml.ts:179): `""` when nothing is set.
pub fn tr_pr_xml(_cx: &mut WriteCtx, p: &TrPr) -> WResult<String> {
    let inner = gen_all(p, TRPR_GEN, TRPR_ORDER, None);
    Ok(if inner.is_empty() { String::new() } else { format!("<w:trPr>{inner}</w:trPr>") })
}

/// `tcPrXml(p)` (typed, table-xml.ts:183).
pub fn tc_pr_xml(_cx: &mut WriteCtx, p: &TcPr) -> WResult<String> {
    Ok(format!("<w:tcPr>{}</w:tcPr>", gen_all(p, TCPR_GEN, TCPR_ORDER, None)))
}

// ───────────────────────── patch of the original XML ─────────────────────────

/// How `patch` reads the typed props of the original element (`readTblPr` and its siblings).
type ReadProps<T> = for<'a> fn(Option<uo_opc::xml::Element<'a>>) -> Option<T>;

/// `patch(raw, cur, read, gen, order, keys, emptyOk)` (table-xml.ts:219): only the children whose
/// typed value changed are regenerated (`setKid`: an empty XML removes, as `null`); the rest stays
/// byte for byte. Nothing changed: `raw` itself.
struct PatchSpec<'s, T> {
    read: ReadProps<T>,
    table: &'s [Gen<T>],
    order: &'s [&'s str],
    keys: Option<&'s [&'s str]>,
    empty_ok: bool,
}

fn patch<T: Default>(cx: &WriteCtx, raw: &str, cur: Option<&T>, spec: &PatchSpec<'_, T>) -> WResult<String> {
    let sp = split(raw, &cx.limits)?;
    let base = (spec.read)(Some(sp.doc.root())).unwrap_or_default();
    let default = T::default();
    let now = cur.unwrap_or(&default);
    let Split { open, close, mut kids, .. } = sp;
    let mut changed = false;
    for g in spec.table {
        if spec.keys.is_some_and(|k| !k.contains(&g.key)) {
            continue;
        }
        if (g.eq)(&base, now) {
            continue;
        }
        changed = true;
        let xml = (g.xml)(now);
        set_kid(&mut kids, spec.order, g.name, xml.as_deref().filter(|x| !x.is_empty()));
    }
    if !changed {
        return Ok(raw.to_owned());
    }
    if kids.is_empty() && !spec.empty_ok {
        return Ok(String::new());
    }
    let mut out = String::with_capacity(open.len() + close.len() + kids.iter().map(|k: &Kid| k.raw.len()).sum::<usize>());
    out.push_str(&open);
    for k in &kids {
        out.push_str(&k.raw);
    }
    out.push_str(&close);
    Ok(out)
}

/// `patchTblPr(raw, cur)`: the original `w:tblPr` with the differences of the current `TblPr`.
pub fn patch_tbl_pr(cx: &mut WriteCtx, raw: &str, cur: Option<&TblPr>) -> WResult<String> {
    let spec = PatchSpec { read: read_tbl_pr, table: TBLPR_GEN, order: TBLPR_ORDER, keys: None, empty_ok: true };
    patch(cx, raw, cur, &spec)
}

/// `patchTblPrEx(raw, cur)`.
pub fn patch_tbl_pr_ex(cx: &mut WriteCtx, raw: &str, cur: Option<&TblPr>) -> WResult<String> {
    let spec = PatchSpec { read: read_tbl_pr, table: TBLPR_GEN, order: TBLPREX_ORDER, keys: Some(EX_KEYS), empty_ok: false };
    patch(cx, raw, cur, &spec)
}

/// `patchTrPr(raw, cur)`.
pub fn patch_tr_pr(cx: &mut WriteCtx, raw: &str, cur: Option<&TrPr>) -> WResult<String> {
    let spec = PatchSpec { read: read_tr_pr, table: TRPR_GEN, order: TRPR_ORDER, keys: None, empty_ok: false };
    patch(cx, raw, cur, &spec)
}

/// `patchTcPr(raw, cur)`.
pub fn patch_tc_pr(cx: &mut WriteCtx, raw: &str, cur: Option<&TcPr>) -> WResult<String> {
    let spec = PatchSpec { read: read_tc_pr, table: TCPR_GEN, order: TCPR_ORDER, keys: None, empty_ok: true };
    patch(cx, raw, cur, &spec)
}

// ───────────────────────── legacy generators (serialize.ts) ─────────────────────────

/// `borderXml(tag, e)` of serialize.ts:1066 (`w:space="0"`, `auto` colour).
fn border_edge_xml(tag: &str, e: &BorderEdge) -> String {
    let color = match e.color.as_deref() {
        Some(c) if !c.is_empty() => esc_attr(c),
        _ => "auto".to_owned(),
    };
    format!("<w:{tag} w:val=\"{}\" w:sz=\"{}\" w:space=\"0\" w:color=\"{color}\"/>", esc_attr(&e.style), num_str(e.size_eighth_pt))
}

/// `borderSetXml(wrap, b, sides)` (serialize.ts:1071).
fn border_set_xml(wrap: &str, b: &TableBorders, sides: &[&str]) -> String {
    let mut inner = String::new();
    for side in sides {
        let e = match *side {
            "top" => b.top.as_ref(),
            "left" => b.left.as_ref(),
            "bottom" => b.bottom.as_ref(),
            "right" => b.right.as_ref(),
            "insideH" => b.inside_h.as_ref(),
            "insideV" => b.inside_v.as_ref(),
            _ => None,
        };
        if let Some(e) = e {
            inner.push_str(&border_edge_xml(side, e));
        }
    }
    if inner.is_empty() { String::new() } else { format!("<w:{wrap}>{inner}</w:{wrap}>") }
}

const TBL_SIDES: &[&str] = &["top", "left", "bottom", "right", "insideH", "insideV"];
const CELL_SIDES: &[&str] = &["top", "left", "bottom", "right"];

/// `tcPrXml(cell)` (legacy, serialize.ts:1164): from the simple fields of a cell built outside the
/// reader.
pub fn tc_pr_xml_legacy(_cx: &mut WriteCtx, cell: &Cell) -> WResult<String> {
    let mut s = String::from("<w:tcPr>");
    if truthy(cell.width_twips) {
        s.push_str(&format!("<w:tcW w:w=\"{}\" w:type=\"dxa\"/>", opt_num_str(cell.width_twips)));
    }
    if cell.grid_span.to_number() > 1.0 {
        s.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", num_str(cell.grid_span)));
    }
    if let Some(b) = cell.borders.as_deref() {
        s.push_str(&border_set_xml("tcBorders", b, CELL_SIDES));
    }
    if let Some(h) = cell.shading_hex.as_deref().filter(|h| !h.is_empty()) {
        s.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", esc_attr(h)));
    }
    s.push_str("</w:tcPr>");
    Ok(s)
}

/// `tblPrXml(t)` (legacy, serialize.ts:1153).
fn tbl_pr_xml_legacy(t: &Table) -> String {
    let total = t.grid_cols_twips.iter().fold(0.0, |a, b| a + b.to_number());
    let mut s = String::from("<w:tblPr>");
    if total != 0.0 && !total.is_nan() {
        s.push_str(&format!("<w:tblW w:w=\"{}\" w:type=\"dxa\"/>", num_str(JsNumber::Num(total))));
    }
    if let Some(b) = t.borders.as_deref() {
        s.push_str(&border_set_xml("tblBorders", b, TBL_SIDES));
    }
    s.push_str("</w:tblPr>");
    s
}

/// `legacyOrTypedTblPr(t)` (serialize.ts:1149): the typed props when the table has them.
pub fn legacy_or_typed_tbl_pr(_cx: &mut WriteCtx, t: &Table) -> WResult<String> {
    Ok(match t.tbl_pr.as_deref() {
        Some(p) => tbl_pr_xml(p),
        None => tbl_pr_xml_legacy(t),
    })
}
