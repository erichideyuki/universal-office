//! Frontier R4a: the mapping from the typed OOXML props to the editing props, and the section
//! reader (docx-core `document.ts`): `readBorderEdge`, `runPropsFromRPr`, `themeRefOf`,
//! `revInfo`, `editBaseProps`, `paraPropsFromPPr`, `readSectPr`.
//!
//! Key orders: `RunProps` and `ParaProps` declare their fields in the assignment order of the
//! TS functions, so the order is the declaration order, except in [`edit_base_props`] where a
//! key the base lacks is added after the others (recorded with `mark`, every key, so the
//! recorded list is the whole order). `SectPr` is assigned in a data-dependent order and
//! records every key.
//!
//! The recursion of `runPropsFromRPr`/`paraPropsFromPPr` through `change.rPr`/`change.pPr` is
//! made iterative (the chain is walked down, the result is built from the innermost out).

use uo_jsnum::conv;
use uo_jsnum::parse::parse_int;
use uo_opc::xml::Element;

use crate::js::{JsNumber, JsObject, Nullable, Str};
use crate::model::para::{BorderEdge, NumberingProps, ParaFormatChange, ParaProps, ParaTabStop, ParagraphBorders};
use crate::model::props::{BorderSpec, PPr, RPr, RevisionInfo};
use crate::model::run::{FormatChange, Run, RunProps, ThemeColorAttrs};
use crate::model::section::{ColWidth, Cols, LineNumbering, PageBorders, SectPr};

use super::parts::read_note_props;
use super::plain::default_run_props;
use super::props::read_border;

/// `UNDERLINE_STYLES` (document.ts:22): the `w:u` styles of the editing props (the 17 of Word,
/// less `none`).
pub const UNDERLINE_STYLES: &[&str] = &[
    "single",
    "words",
    "double",
    "thick",
    "dotted",
    "dottedHeavy",
    "dash",
    "dashedHeavy",
    "dashLong",
    "dashLongHeavy",
    "dotDash",
    "dashDotHeavy",
    "dotDotDash",
    "dashDotDotHeavy",
    "wave",
    "wavyHeavy",
    "wavyDouble",
];

/// `TIGHT_WRAP` (document.ts:775).
const TIGHT_WRAP: &[&str] = &["none", "allLines", "firstAndLastLine", "firstLineOnly", "lastLineOnly"];

/// A JS string used as a condition: `Some` when truthy (non-empty).
fn non_empty(s: Option<&Str>) -> Option<&str> {
    s.map(|s| &**s).filter(|s| !s.is_empty())
}

/// `n !== v` for a TS number field that is not `undefined` (`null` is `!== v`, NaN too).
fn ne_num(n: JsNumber, v: f64) -> bool {
    match n {
        JsNumber::Num(x) => x != v,
        JsNumber::Null => true,
    }
}

/// `x ?? d` for a number field (`null` and `undefined` take the default).
fn or_default(n: Option<JsNumber>, d: f64) -> JsNumber {
    match n {
        Some(JsNumber::Num(x)) => JsNumber::Num(x),
        _ => JsNumber::Num(d),
    }
}

/// `-x` for a TS number (`-null` is `-0`).
fn neg(n: JsNumber) -> JsNumber {
    JsNumber::Num(-n.to_number())
}

/// `readBorderEdge` (document.ts:584): a paragraph border edge from a `w:top`/`w:left`/… element;
/// `None` for no element, no `w:val`, `none` or `nil`. `color` is an explicit `undefined` for
/// `auto` or no colour.
#[must_use]
pub fn read_border_edge(el: Option<Element<'_>>) -> Option<BorderEdge> {
    let el = el?;
    let style = non_empty_str(el.attr("w:val"))?;
    if style == "none" || style == "nil" {
        return None;
    }
    let color = el.attr("w:color");
    let mut edge = BorderEdge {
        style: style.into(),
        size_eighth_pt: JsNumber::Num(parse_int(el.attr("w:sz").unwrap_or("4"), 10)),
        color: color.filter(|c| !c.is_empty() && *c != "auto").map(Into::into),
        space_pt: JsNumber::Num(parse_int(el.attr("w:space").unwrap_or("0"), 10)),
        ..BorderEdge::default()
    };
    if edge.color.is_none() {
        edge.mark_undefined("color");
    }
    Some(edge)
}

fn non_empty_str(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// `themeRefOf` (document.ts:743): the theme reference of a `w:color` or `w:u`.
fn theme_ref_of(theme_color: Option<&Str>, tint: Option<&Str>, shade: Option<&Str>) -> Option<ThemeColorAttrs> {
    let theme_color = non_empty(theme_color)?;
    Some(ThemeColorAttrs {
        theme_color: theme_color.into(),
        tint: non_empty(tint).map(Into::into),
        shade: non_empty(shade).map(Into::into),
        ..ThemeColorAttrs::default()
    })
}

/// `revInfo` (document.ts:749): only the authorship of a revision.
#[must_use]
pub fn rev_info(r: &RevisionInfo) -> RevisionInfo {
    rev_info_of(&r.author, r.date.as_ref(), r.date_utc.as_ref(), r.r#move)
}

fn rev_info_of(author: &Str, date: Option<&Str>, date_utc: Option<&Str>, mv: Option<bool>) -> RevisionInfo {
    RevisionInfo {
        author: author.clone(),
        date: date.cloned(),
        date_utc: date_utc.cloned(),
        r#move: (mv == Some(true)).then_some(true),
        ..RevisionInfo::default()
    }
}

/// `runPropsFromRPr` (document.ts:706): the editing props of a `w:rPr`.
#[must_use]
pub fn run_props_from_rpr(rpr: Option<&RPr>) -> RunProps {
    let mut chain: Vec<&RPr> = Vec::new();
    let mut cur = rpr;
    while let Some(r) = cur {
        chain.push(r);
        cur = r.change.as_deref().map(|c| &*c.r_pr);
    }
    let mut inner: Option<RunProps> = None;
    for r in chain.into_iter().rev() {
        let mut p = flat_run_props(r);
        if let Some(ch) = r.change.as_deref() {
            let old = inner.take().unwrap_or_else(default_run_props);
            let info = rev_info_of(&ch.author, ch.date.as_ref(), ch.date_utc.as_ref(), ch.r#move);
            p.fmt = Some(Box::new(FormatChange {
                author: info.author,
                date: info.date,
                date_utc: info.date_utc,
                r#move: info.r#move,
                old: Box::new(old),
                ..FormatChange::default()
            }));
        }
        inner = Some(p);
    }
    inner.unwrap_or_else(default_run_props)
}

/// The body of `runPropsFromRPr` without the `change` recursion.
fn flat_run_props(rpr: &RPr) -> RunProps {
    let mut p = default_run_props();
    if let Some(b) = rpr.b {
        p.bold = Some(b);
    }
    if let Some(i) = rpr.i {
        p.italic = Some(i);
    }
    if let Some(u) = rpr.u.as_deref() {
        let val: &str = &u.val;
        p.underline = Some(val != "none" && !val.is_empty());
        if val != "single" && UNDERLINE_STYLES.contains(&val) {
            p.underline_style = Some(val.into());
        }
        if let Some(c) = non_empty(u.color.as_ref()).filter(|c| *c != "auto") {
            p.underline_color = Some(c.into());
        }
        if let Some(t) = theme_ref_of(u.theme_color.as_ref(), u.theme_tint.as_ref(), u.theme_shade.as_ref()) {
            p.underline_theme = Some(t);
        }
    }
    if let Some(c) = rpr.color.as_deref()
        && let Some(t) = theme_ref_of(c.theme_color.as_ref(), c.theme_tint.as_ref(), c.theme_shade.as_ref())
    {
        p.color_theme = Some(t);
    }
    if let Some(v) = rpr.vanish {
        p.hidden = Some(v);
    }
    if let Some(w) = rpr.w
        && ne_num(w, 100.0)
    {
        p.scale_pct = Some(w);
    }
    if let Some(pos) = rpr.position
        && pos.truthy()
    {
        p.position_half_pt = Some(pos);
    }
    if let Some(k) = rpr.kern
        && k.truthy()
    {
        p.kern_half_pt = Some(k);
    }
    if let Some(v) = rpr.strike {
        p.strike = Some(v);
    }
    if let Some(v) = rpr.dstrike {
        p.double_strike = Some(v);
    }
    if let Some(v) = rpr.caps {
        p.all_caps = Some(v);
    }
    if let Some(v) = rpr.small_caps {
        p.small_caps = Some(v);
    }
    if let Some(v) = rpr.vert_align.as_deref()
        && (v == "superscript" || v == "subscript")
    {
        p.vert_align = Some(v.into());
    }
    if let Some(sz) = rpr.sz {
        p.sz_half_pt = Some(sz);
    }
    if let Some(c) = rpr.color.as_deref()
        && let Some(v) = non_empty(c.val.as_ref()).filter(|v| *v != "auto")
    {
        p.color = Some(v.into());
    }
    if let Some(h) = non_empty(rpr.highlight.as_ref()).filter(|h| *h != "none") {
        p.highlight = Some(h.into());
    }
    if let Some(f) = rpr.r_fonts.as_deref() {
        p.font_ascii = f.ascii.clone();
        if p.font_ascii.is_none() {
            p.mark_undefined("fontAscii");
        }
    }
    if let Some(s) = rpr.spacing
        && ne_num(s, 0.0)
    {
        p.char_spacing_twips = Some(s);
    }
    if let Some(s) = non_empty(rpr.r_style.as_ref()) {
        p.char_style = Some(s.into());
    }
    if let Some(fx) = &rpr.fx {
        p.text_fx = Some(fx.clone());
    }
    if let Some(ot) = &rpr.ot {
        p.open_type = Some(ot.clone());
    }
    p
}

/// Records the current key order of `p` (every present key, in order), before a key is added
/// after the others.
fn freeze_order(p: &mut RunProps) {
    let names: Vec<&'static str> = p.slots_in_order().into_iter().map(|(k, _)| k).collect();
    for k in names {
        p.mark(k);
    }
}

/// `editBaseProps` (document.ts:766): the base the editing of a run is measured against: the
/// props the `w:rPr` implies plus what the reader forces (hyperlink underline and colour, the
/// superscript of a note mark, the font of a `w:sym`).
#[must_use]
pub fn edit_base_props(run: &Run, rpr: Option<&RPr>) -> RunProps {
    let mut base = run_props_from_rpr(rpr);
    if non_empty(run.props.link.as_ref()).is_some() && base.underline == Some(false) {
        base.underline = Some(true);
        if base.color.is_none() {
            freeze_order(&mut base);
            base.color = Some("0563C1".into());
            base.mark("color");
        }
    }
    let ext = run.ext();
    if ext.is_some_and(|e| e.note_ref.is_some()) && base.vert_align.is_none() {
        freeze_order(&mut base);
        base.vert_align = Some("superscript".into());
        base.mark("vertAlign");
    }
    if let Some(sym) = ext.and_then(|e| non_empty(e.sym_font.as_ref())) {
        freeze_order(&mut base);
        base.font_ascii = Some(sym.into());
        if let Some(i) = RunProps::key_index("fontAscii") {
            base.cleared.clear(u32::from(i));
        }
        base.mark("fontAscii");
    }
    base
}

/// `paraPropsFromPPr` (document.ts:778): the editing props of a `w:pPr`.
#[must_use]
pub fn para_props_from_ppr(ppr: Option<&PPr>) -> ParaProps {
    let mut chain: Vec<&PPr> = Vec::new();
    let mut cur = ppr;
    while let Some(p) = cur {
        chain.push(p);
        cur = p.change.as_deref().map(|c| &*c.p_pr);
    }
    let mut inner: Option<ParaProps> = None;
    for p in chain.into_iter().rev() {
        let mut props = flat_para_props(p);
        if let Some(ch) = p.change.as_deref() {
            let old = inner.take().unwrap_or_default();
            let info = rev_info_of(&ch.author, ch.date.as_ref(), ch.date_utc.as_ref(), ch.r#move);
            props.pfmt = Some(Box::new(ParaFormatChange {
                author: info.author,
                date: info.date,
                date_utc: info.date_utc,
                r#move: info.r#move,
                old: Box::new(old),
                ..ParaFormatChange::default()
            }));
        }
        inner = Some(props);
    }
    inner.unwrap_or_default()
}

/// The body of `paraPropsFromPPr` without the `change` recursion.
fn flat_para_props(ppr: &PPr) -> ParaProps {
    let mut props = ParaProps::default();
    match ppr.jc.as_deref() {
        Some(jc @ ("center" | "right" | "both" | "left")) => props.alignment = Some(jc.into()),
        Some("start") => props.alignment = Some("left".into()),
        Some("end") => props.alignment = Some("right".into()),
        Some("distribute") => props.alignment = Some("both".into()),
        _ => {}
    }
    if let Some(s) = &ppr.p_style {
        props.style_id = Some(s.clone());
    }
    if let Some(np) = ppr.num_pr.as_deref() {
        let num_id = or_default(np.num_id, 0.0);
        props.numbering = Some(NumberingProps {
            kind: if num_id == JsNumber::Num(1002.0) { "decimal" } else { "bullet" }.into(),
            ilvl: or_default(np.ilvl, 0.0),
            num_id: Some(num_id),
            ..NumberingProps::default()
        });
    }
    if let Some(ind) = ppr.ind.as_deref() {
        if let Some(v) = ind.left {
            props.indent_left_twips = Some(v);
        }
        if let Some(v) = ind.right {
            props.indent_right_twips = Some(v);
        }
        if let Some(v) = ind.first_line {
            props.indent_first_line_twips = Some(v);
        }
        if let Some(v) = ind.hanging {
            props.indent_first_line_twips = Some(neg(v));
        }
    }
    if let Some(sp) = ppr.spacing.as_deref() {
        if let Some(line) = sp.line
            && sp.line_rule.as_deref() == Some("auto")
        {
            props.line_twentieths = Some(line);
        }
        if let Some(v) = sp.after {
            props.space_after_twips = Some(v);
        }
        if let Some(v) = sp.before {
            props.space_before_twips = Some(v);
        }
        if let Some(v) = sp.before_autospacing {
            props.space_before_auto = Some(v);
        }
        if let Some(v) = sp.after_autospacing {
            props.space_after_auto = Some(v);
        }
    }
    if let Some(shd) = ppr.shd.as_deref()
        && let Some(f) = non_empty(shd.fill.as_ref()).filter(|f| *f != "auto")
    {
        props.shading_hex = Some(f.into());
    }
    if let Some(tabs) = ppr.tabs.as_ref().filter(|t| !t.is_empty()) {
        props.tab_stops = Some(
            tabs.iter()
                .map(|t| ParaTabStop {
                    pos_twips: t.pos,
                    align: t.val.clone(),
                    leader: non_empty(t.leader.as_ref()).filter(|l| *l != "none").map(Into::into),
                    ..ParaTabStop::default()
                })
                .collect(),
        );
    }
    if let Some(bdr) = ppr.p_bdr.as_deref() {
        props.box_border = Some(true);
        let mut borders = ParagraphBorders::default();
        let sides = [(&bdr.top, 0), (&bdr.left, 1), (&bdr.bottom, 2), (&bdr.right, 3), (&bdr.between, 4), (&bdr.bar, 5)];
        let mut any = false;
        for (b, side) in sides {
            let Some(b) = b.as_deref() else { continue };
            if &*b.val == "none" || &*b.val == "nil" {
                continue;
            }
            let edge = border_edge_of(b);
            any = true;
            match side {
                0 => borders.top = Some(edge),
                1 => borders.left = Some(edge),
                2 => borders.bottom = Some(edge),
                3 => borders.right = Some(edge),
                4 => borders.between = Some(edge),
                _ => borders.bar = Some(edge),
            }
        }
        if any {
            props.borders = Some(Box::new(borders));
        }
    }
    if let Some(v) = ppr.page_break_before {
        props.page_break_before = Some(v);
    }
    if let Some(v) = ppr.keep_next {
        props.keep_next = Some(v);
    }
    if let Some(v) = ppr.keep_lines {
        props.keep_lines = Some(v);
    }
    if let Some(v) = ppr.widow_control {
        props.widow_control = Some(v);
    }
    if let Some(v) = ppr.suppress_line_numbers {
        props.suppress_line_numbers = Some(v);
    }
    if let Some(v) = ppr.suppress_auto_hyphens {
        props.suppress_auto_hyphens = Some(v);
    }
    if let Some(w) = non_empty(ppr.textbox_tight_wrap.as_ref())
        && TIGHT_WRAP.contains(&w)
    {
        props.textbox_tight_wrap = Some(w.into());
    }
    if let Some(v) = ppr.outline_lvl {
        props.outline_lvl = Some(v);
    }
    if let Some(fp) = ppr.frame_pr.as_deref()
        && non_empty(fp.drop_cap.as_ref()).is_some_and(|d| d != "none")
    {
        props.drop_cap = Some(true);
    }
    if let Some(m) = ppr.mark_ins.as_deref() {
        props.mark_ins = Some(Box::new(rev_info(m)));
    }
    if let Some(m) = ppr.mark_del.as_deref() {
        props.mark_del = Some(Box::new(rev_info(m)));
    }
    props
}

/// The edge `{ style: b.val, sizeEighthPt: b.sz ?? 4, color, spacePt: b.space ?? 0 }` of
/// `paraPropsFromPPr` (document.ts:818).
fn border_edge_of(b: &BorderSpec) -> BorderEdge {
    let mut edge = BorderEdge {
        style: b.val.clone(),
        size_eighth_pt: or_default(b.sz, 4.0),
        color: non_empty(b.color.as_ref()).filter(|c| *c != "auto").map(Into::into),
        space_pt: or_default(b.space, 0.0),
        ..BorderEdge::default()
    };
    if edge.color.is_none() {
        edge.mark_undefined("color");
    }
    edge
}

/// `tw` (document.ts:2208): `parseInt` of an attribute, or the fallback when it is missing.
fn tw(el: Option<Element<'_>>, attr: &str, fallback: f64) -> JsNumber {
    match el.and_then(|e| e.attr(attr)) {
        None => JsNumber::Num(fallback),
        Some(v) => JsNumber::Num(parse_int(v, 10)),
    }
}

/// `readSectPr` (document.ts:2226): the geometry of a `w:sectPr` (missing element: the A4 page
/// of the TS defaults). Every key is recorded with `mark` (the assignment order differs from the
/// declaration order).
#[must_use]
pub fn read_sect_pr(sect_el: Option<Element<'_>>) -> SectPr {
    let pg_sz = sect_el.and_then(|e| e.first_child("w:pgSz"));
    let pg_mar = sect_el.and_then(|e| e.first_child("w:pgMar"));
    let cols_el = sect_el.and_then(|e| e.first_child("w:cols"));
    let mut sect = SectPr {
        page_width_twips: tw(pg_sz, "w:w", 11906.0),
        page_height_twips: tw(pg_sz, "w:h", 16838.0),
        margin_left_twips: tw(pg_mar, "w:left", 1701.0),
        margin_right_twips: tw(pg_mar, "w:right", 1701.0),
        margin_top_twips: tw(pg_mar, "w:top", 1417.0),
        margin_bottom_twips: tw(pg_mar, "w:bottom", 1417.0),
        header_distance_twips: Some(tw(pg_mar, "w:header", 720.0)),
        footer_distance_twips: Some(tw(pg_mar, "w:footer", 720.0)),
        ..SectPr::default()
    };
    for k in [
        "pageWidthTwips",
        "pageHeightTwips",
        "marginLeftTwips",
        "marginRightTwips",
        "marginTopTwips",
        "marginBottomTwips",
        "headerDistanceTwips",
        "footerDistanceTwips",
    ] {
        sect.mark(k);
    }
    let ty = sect_el.and_then(|e| e.first_child("w:type")).and_then(|t| t.attr("w:val"));
    if let Some(t @ ("continuous" | "evenPage" | "oddPage" | "nextColumn")) = ty {
        sect.r#type = Some(t.into());
        sect.mark("type");
    }
    let num = match cols_el {
        Some(c) => parse_int(c.attr("w:num").unwrap_or("1"), 10),
        None => 1.0,
    };
    if let Some(cols_el) = cols_el
        && num > 1.0
    {
        let mut cols = Cols {
            count: JsNumber::Num(num),
            space_twips: JsNumber::Num(parse_int(cols_el.attr("w:space").unwrap_or("720"), 10)),
            sep: matches!(cols_el.attr("w:sep"), Some("true" | "1")),
            ..Cols::default()
        };
        let eq = cols_el.attr("w:equalWidth");
        let defs: Vec<Element<'_>> = cols_el.children_named("w:col").collect();
        if matches!(eq, Some("0" | "false"))
            && let Some(n) = conv::js_index(num, defs.len().saturating_add(1))
        {
            cols.widths = Some(
                defs.iter()
                    .take(n)
                    .map(|c| ColWidth {
                        width_twips: or_zero(parse_int(c.attr("w:w").unwrap_or("0"), 10)),
                        space_twips: or_zero(parse_int(c.attr("w:space").unwrap_or("0"), 10)),
                        ..ColWidth::default()
                    })
                    .collect(),
            );
        }
        sect.cols = Some(cols);
        sect.mark("cols");
    }
    if let Some(t) = sect_el.and_then(|e| e.first_child("w:titlePg"))
        && !matches!(t.attr("w:val"), Some("0" | "false"))
    {
        sect.title_pg = Some(true);
        sect.mark("titlePg");
    }
    if let Some(pb) = sect_el.and_then(|e| e.first_child("w:pgBorders")) {
        let offset_from = pb.attr("w:offsetFrom").filter(|v| matches!(*v, "page" | "text"));
        let display = pb.attr("w:display").filter(|v| matches!(*v, "allPages" | "firstPage" | "notFirstPage"));
        let z_order = pb.attr("w:zOrder").filter(|v| matches!(*v, "front" | "back"));
        let out = PageBorders {
            offset_from: offset_from.map(Into::into),
            display: display.map(Into::into),
            z_order: z_order.map(Into::into),
            top: read_border(pb.first_child("w:top")),
            left: read_border(pb.first_child("w:left")),
            bottom: read_border(pb.first_child("w:bottom")),
            right: read_border(pb.first_child("w:right")),
            ..PageBorders::default()
        };
        sect.pg_borders = Some(Nullable::Val(Box::new(out)));
        sect.mark("pgBorders");
    }
    let pg_num_type = sect_el.and_then(|e| e.first_child("w:pgNumType"));
    if let Some(start) = pg_num_type.and_then(|p| p.attr("w:start")) {
        let n = parse_int(start, 10);
        if !n.is_nan() {
            sect.pg_num_start = Some(JsNumber::Num(n));
            sect.mark("pgNumStart");
        }
    }
    if let Some(fmt) = non_empty_str(pg_num_type.and_then(|p| p.attr("w:fmt"))) {
        sect.pg_num_fmt = Some(fmt.into());
        sect.mark("pgNumFmt");
    }
    let chap_style = parse_int(pg_num_type.and_then(|p| p.attr("w:chapStyle")).unwrap_or(""), 10);
    if chap_style > 0.0 {
        sect.pg_num_chap_style = Some(JsNumber::Num(chap_style));
        sect.mark("pgNumChapStyle");
        if let Some(sep @ ("period" | "colon" | "emDash" | "enDash")) = pg_num_type.and_then(|p| p.attr("w:chapSep")) {
            sect.pg_num_chap_sep = Some(sep.into());
            sect.mark("pgNumChapSep");
        }
    }
    if let Some(n) = read_note_props(sect_el.and_then(|e| e.first_child("w:footnotePr"))) {
        sect.footnote_pr = Some(n);
        sect.mark("footnotePr");
    }
    if let Some(n) = read_note_props(sect_el.and_then(|e| e.first_child("w:endnotePr"))) {
        sect.endnote_pr = Some(n);
        sect.mark("endnotePr");
    }
    if let Some(fn_cols) = sect_el.and_then(|e| e.first_child("w15:footnoteColumns")).and_then(|c| c.attr("w:val"))
        && parse_int(fn_cols, 10) > 0.0
    {
        sect.footnote_columns = Some(JsNumber::Num(parse_int(fn_cols, 10)));
        sect.mark("footnoteColumns");
    }
    if let Some(ln) = sect_el.and_then(|e| e.first_child("w:lnNumType")) {
        let int = |k: &str| {
            let n = parse_int(ln.attr(k).unwrap_or(""), 10);
            (!n.is_nan()).then_some(JsNumber::Num(n))
        };
        let restart = ln.attr("w:restart").filter(|v| matches!(*v, "newSection" | "continuous"));
        sect.ln_num_type = Some(LineNumbering {
            count_by: int("w:countBy"),
            start: int("w:start"),
            distance: int("w:distance"),
            restart: restart.map(Into::into),
            ..LineNumbering::default()
        });
        sect.mark("lnNumType");
    }
    if let Some(va @ ("center" | "both" | "bottom")) = sect_el.and_then(|e| e.first_child("w:vAlign")).and_then(|v| v.attr("w:val")) {
        sect.v_align = Some(va.into());
        sect.mark("vAlign");
    }
    sect
}

/// `x || 0`.
fn or_zero(x: f64) -> JsNumber {
    JsNumber::Num(if conv::truthy(x) { x } else { 0.0 })
}
