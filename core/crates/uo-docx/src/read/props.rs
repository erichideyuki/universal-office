//! Frontier R1 (D1a reader, record "Milestone 1 — D1a foundation"): `ooxml/props.ts`:
//! `readRevisionInfo`, `measureTwips`, `onOffVal`, `colorOf`, `readBorder`, `readShading`,
//! `readWidth`, `readRPr`, `readPBdr`, `readPPr`, `readTblPr`, `readTrPr`, `readTcPr` (the
//! declarations are in `core/docs/modules/uo-docx-d1a/R1.md`).
//!
//! The readers are pure functions of an XML element. Every object whose keys the TS assigns in a
//! data-dependent order (`RPr`, `PPr`, `TblPr`, `TrPr`, `TcPr`) or in a code order that differs
//! from the model's declaration order (`SpacingSpec`, `FramePr`, `TblpPr`) records each
//! assignment with `JsObject::mark`.
//!
//! The only recursion of the TS (`readRPr` → `w:rPrChange` → `w:rPr` → …, `readPPr` →
//! `w:pPrChange` → `w:pPr` → …) is a loop here: the chain of nested elements is collected
//! top-down (only the last `w:rPrChange`/`w:pPrChange` of an element survives in the TS, the
//! earlier ones are overwritten) and the objects are built bottom-up.

use uo_jsnum::{conv, math, parse, text};
use uo_opc::xml::Element;

use crate::js::{JsNumber, JsObject, Str};
use crate::model::props::{
    BorderSpec, CellMargins, ColorSpec, FramePr, IndSpec, Lang, NumPr, PBdr, PPr, PPrChange, RFonts, RPr, RPrChange, RevisionInfo,
    ShadingSpec, SpacingSpec, TabSpec, TblBorders, TblLook, TblPr, TblpPr, TcBorders, TcPr, TextFill, TextShadow, TrHeight, TrPr,
    UnderlineSpec, WidthSpec,
};

use super::matchers::{Cur, is_one_of};
use super::text_effects::read_text_effects;

/// `o.field = Some(v)` and records the assignment in the key order.
macro_rules! put {
    ($p:ident, $f:ident, $k:literal, $v:expr) => {{
        let v = $v;
        $p.$f = Some(v);
        $p.mark($k);
    }};
}

fn num(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

/// A JS string used as a condition: `if (s)` (non-empty).
fn truthy(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.is_empty())
}

/// `x || 0`.
fn or0(x: f64) -> f64 {
    if conv::truthy(x) { x } else { 0.0 }
}

/// The `int` helper of `props.ts`: `parseInt(v, 10)` when it is finite.
#[must_use]
pub fn int(v: Option<&str>) -> Option<f64> {
    let v = v?;
    if v.is_empty() {
        return None;
    }
    let n = parse::parse_int(v, 10);
    n.is_finite().then_some(n)
}

/// The units of `measureTwips`, in the alternation order of the regex.
const MEASURE_UNITS: [&str; 6] = ["mm", "cm", "in", "pt", "pc", "pi"];

/// The regex of `measureTwips` (`/^(-?[0-9.]+)(mm|cm|in|pt|pc|pi)?$/`): the number text (group 1)
/// and the unit (group 2). A `Cur` walk (matcher template 5): the digits and the unit letters
/// do not overlap, so the greedy number never gives characters back.
#[must_use]
pub fn split_measure(s: &str) -> Option<(&str, Option<&str>)> {
    let mut c = Cur::new(s);
    c.opt('-');
    c.take_while1(|ch| ch.is_ascii_digit() || ch == '.')?;
    let number = s.get(..c.pos())?;
    let rest = c.rest();
    if rest.is_empty() {
        Some((number, None))
    } else if is_one_of(rest, &MEASURE_UNITS) {
        Some((number, Some(rest)))
    } else {
        None
    }
}

/// `measureTwips`: a "universal measure" in twips (also `12pt`, `1in`…); `None` when absent or
/// not a measure. A match whose number `parseFloat` cannot read (`.`, `..5`) is `Some(NaN)`.
#[must_use]
pub fn measure_twips(v: Option<&str>) -> Option<f64> {
    let v = v?;
    if v.is_empty() {
        return None;
    }
    let (number, unit) = split_measure(text::trim(v))?;
    let n = parse::parse_float(number);
    Some(match unit {
        Some("mm") => math::round(n / 25.4 * 1440.0),
        Some("cm") => math::round(n / 2.54 * 1440.0),
        Some("in") => math::round(n * 1440.0),
        Some("pt") => math::round(n * 20.0),
        Some("pc" | "pi") => math::round(n * 240.0),
        _ => math::round(n),
    })
}

/// `ST_OnOff` of an element: no `w:val` is true; `0`, `false`, `off` and `none` are false.
#[must_use]
pub fn on_off_val(el: Element<'_>) -> bool {
    !matches!(el.attr("w:val"), Some("0" | "false" | "off" | "none"))
}

/// `onOffAttr`: like [`on_off_val`] on an attribute value (without `none`).
fn on_off_attr(v: Option<&str>) -> bool {
    !matches!(v, Some("0" | "false" | "off"))
}

/// `readRevisionInfo`.
#[must_use]
pub fn read_revision_info(el: Element<'_>) -> RevisionInfo {
    let mut info = RevisionInfo { author: el.attr("w:author").unwrap_or("").into(), ..RevisionInfo::default() };
    if let Some(d) = truthy(el.attr("w:date")) {
        info.date = Some(d.into());
    }
    if let Some(d) = truthy(el.attr("w16du:dateUtc")) {
        info.date_utc = Some(d.into());
    }
    if matches!(el.name(), "w:moveFrom" | "w:moveTo") {
        info.r#move = Some(true);
    }
    info
}

fn color_of(el: Element<'_>) -> ColorSpec {
    ColorSpec {
        val: el.attr("w:val").map(Str::from),
        theme_color: truthy(el.attr("w:themeColor")).map(Str::from),
        theme_tint: truthy(el.attr("w:themeTint")).map(Str::from),
        theme_shade: truthy(el.attr("w:themeShade")).map(Str::from),
        ..ColorSpec::default()
    }
}

fn border_of(el: Element<'_>) -> BorderSpec {
    BorderSpec {
        val: el.attr("w:val").unwrap_or("single").into(),
        sz: int(el.attr("w:sz")).map(num),
        space: int(el.attr("w:space")).map(num),
        color: truthy(el.attr("w:color")).map(Str::from),
        theme_color: truthy(el.attr("w:themeColor")).map(Str::from),
        theme_tint: truthy(el.attr("w:themeTint")).map(Str::from),
        theme_shade: truthy(el.attr("w:themeShade")).map(Str::from),
        shadow: el.attr("w:shadow").map(|v| on_off_attr(Some(v))),
        frame: el.attr("w:frame").map(|v| on_off_attr(Some(v))),
        ..BorderSpec::default()
    }
}

/// `readBorder`.
#[must_use]
pub fn read_border(el: Option<Element<'_>>) -> Option<BorderSpec> {
    el.map(border_of)
}

fn shading_of(el: Element<'_>) -> ShadingSpec {
    let g = |k: &str| el.attr(k).map(Str::from);
    ShadingSpec {
        val: g("w:val"),
        color: g("w:color"),
        fill: g("w:fill"),
        theme_fill: g("w:themeFill"),
        theme_fill_tint: g("w:themeFillTint"),
        theme_fill_shade: g("w:themeFillShade"),
        theme_color: g("w:themeColor"),
        ..ShadingSpec::default()
    }
}

/// `readShading`.
#[must_use]
pub fn read_shading(el: Option<Element<'_>>) -> Option<ShadingSpec> {
    el.map(shading_of)
}

fn width_of(el: Element<'_>) -> WidthSpec {
    let ty = el.attr("w:type").unwrap_or("dxa");
    let raw = el.attr("w:w").unwrap_or("0");
    let w = if raw.ends_with('%') {
        math::round(parse::parse_float(raw) * 50.0)
    } else if ty == "dxa" {
        measure_twips(Some(raw)).unwrap_or(0.0)
    } else {
        int(Some(raw)).unwrap_or(0.0)
    };
    let ty = if matches!(ty, "pct" | "auto" | "nil") { ty } else { "dxa" };
    WidthSpec { w: num(w), r#type: ty.into(), ..WidthSpec::default() }
}

fn read_width(el: Option<Element<'_>>) -> Option<WidthSpec> {
    el.map(width_of)
}

/// `readRPr`: a `w:rPr` (or any element with the same children) as the sparse run properties.
#[must_use]
pub fn read_rpr(rpr: Option<Element<'_>>) -> Option<RPr> {
    rpr.map(rpr_of)
}

fn rpr_of(rpr: Element<'_>) -> RPr {
    let mut chain: Vec<(Element<'_>, Option<Element<'_>>)> = Vec::new();
    let mut cur = Some(rpr);
    while let Some(e) = cur.take() {
        let change = e.children_named("w:rPrChange").next_back();
        chain.push((e, change));
        cur = change.and_then(|c| c.first_child("w:rPr"));
    }
    let mut below: Option<RPr> = None;
    for (e, change) in chain.into_iter().rev() {
        let mut p = rpr_level(e);
        if let Some(c) = change {
            let info = read_revision_info(c);
            p.change = Some(Box::new(RPrChange {
                author: info.author,
                date: info.date,
                date_utc: info.date_utc,
                r#move: info.r#move,
                r_pr: Box::new(below.take().unwrap_or_default()),
                ..RPrChange::default()
            }));
        }
        below = Some(p);
    }
    below.unwrap_or_default()
}

/// One `w:rPr` without the value of `change` (set by [`rpr_of`]; the key is recorded here).
fn rpr_level(rpr: Element<'_>) -> RPr {
    let mut p = RPr::default();
    for el in rpr.child_elements() {
        let val = el.attr("w:val");
        match el.name() {
            "w:rStyle" => {
                if let Some(v) = truthy(val) {
                    put!(p, r_style, "rStyle", v.into());
                }
            }
            "w:rFonts" => {
                let g = |k: &str| el.attr(k).map(Str::from);
                let f = RFonts {
                    ascii: g("w:ascii"),
                    h_ansi: g("w:hAnsi"),
                    east_asia: g("w:eastAsia"),
                    cs: g("w:cs"),
                    ascii_theme: g("w:asciiTheme"),
                    h_ansi_theme: g("w:hAnsiTheme"),
                    east_asia_theme: g("w:eastAsiaTheme"),
                    cstheme: g("w:cstheme"),
                    hint: g("w:hint"),
                    ..RFonts::default()
                };
                put!(p, r_fonts, "rFonts", Box::new(f));
            }
            "w:b" => put!(p, b, "b", on_off_val(el)),
            "w:bCs" => put!(p, b_cs, "bCs", on_off_val(el)),
            "w:i" => put!(p, i, "i", on_off_val(el)),
            "w:iCs" => put!(p, i_cs, "iCs", on_off_val(el)),
            "w:caps" => put!(p, caps, "caps", on_off_val(el)),
            "w:smallCaps" => put!(p, small_caps, "smallCaps", on_off_val(el)),
            "w:strike" => put!(p, strike, "strike", on_off_val(el)),
            "w:dstrike" => put!(p, dstrike, "dstrike", on_off_val(el)),
            "w:outline" => put!(p, outline, "outline", on_off_val(el)),
            "w:shadow" => put!(p, shadow, "shadow", on_off_val(el)),
            "w:emboss" => put!(p, emboss, "emboss", on_off_val(el)),
            "w:imprint" => put!(p, imprint, "imprint", on_off_val(el)),
            "w:vanish" => put!(p, vanish, "vanish", on_off_val(el)),
            "w:specVanish" => put!(p, spec_vanish, "specVanish", on_off_val(el)),
            "w:webHidden" => put!(p, web_hidden, "webHidden", on_off_val(el)),
            "w:noProof" => put!(p, no_proof, "noProof", on_off_val(el)),
            "w:snapToGrid" => put!(p, snap_to_grid, "snapToGrid", on_off_val(el)),
            "w:rtl" => put!(p, rtl, "rtl", on_off_val(el)),
            "w:cs" => put!(p, cs, "cs", on_off_val(el)),
            "w:color" => put!(p, color, "color", Box::new(color_of(el))),
            "w14:textFill" => {
                let solid = el.first_child("w14:solidFill");
                let clr = solid.and_then(|s| s.first_child("w14:srgbClr").or_else(|| s.first_child("w14:schemeClr")));
                if let Some(clr) = clr {
                    let mut tf = TextFill::default();
                    let v = clr.attr("w14:val");
                    if clr.name() == "w14:srgbClr"
                        && let Some(v) = truthy(v)
                    {
                        tf.color = Some(v.to_uppercase().into());
                    } else if let Some(v) = truthy(v) {
                        tf.theme_color = Some(v.into());
                    }
                    if let Some(a) = clr.first_child("w14:alpha").and_then(|a| a.attr("w14:val")) {
                        tf.alpha = Some(num(math::max(0.0, 1.0 - or0(parse::parse_int(a, 10)) / 100_000.0)));
                    }
                    put!(p, text_fill, "textFill", Box::new(tf));
                } else if el.first_child("w14:noFill").is_some() {
                    put!(p, text_fill, "textFill", Box::new(TextFill { alpha: Some(num(0.0)), ..TextFill::default() }));
                }
            }
            "w14:ligatures" => put!(p, ligatures, "ligatures", el.attr("w14:val").unwrap_or("standard").into()),
            "w:rPrChange" => p.mark("change"),
            "w14:shadow" => {
                let emu = |k: &str| or0(parse::parse_int(el.attr(k).unwrap_or("0"), 10)) / 12700.0;
                let sh = TextShadow {
                    blur: num(emu("w14:blurRad")),
                    dist: num(emu("w14:dist")),
                    dir: num(or0(parse::parse_int(el.attr("w14:dir").unwrap_or("0"), 10)) / 60000.0),
                    ..TextShadow::default()
                };
                put!(p, text_shadow, "textShadow", Box::new(sh));
            }
            "w14:textOutline" => {
                let outline = el.first_child("w14:noFill").is_none()
                    && (el.first_child("w14:solidFill").is_some() || el.first_child("w14:gradFill").is_some());
                put!(p, text_outline, "textOutline", outline);
            }
            "w:spacing" => {
                if let Some(n) = measure_twips(val) {
                    put!(p, spacing, "spacing", num(n));
                }
            }
            "w:w" => {
                let v = val.map(|v| v.replacen('%', "", 1));
                if let Some(n) = int(v.as_deref()) {
                    put!(p, w, "w", num(n));
                }
            }
            "w:kern" => {
                if let Some(n) = int(val) {
                    put!(p, kern, "kern", num(n));
                }
            }
            "w:position" => {
                if let Some(n) = int(val) {
                    put!(p, position, "position", num(n));
                }
            }
            "w:sz" => {
                if let Some(n) = int(val) {
                    put!(p, sz, "sz", num(n));
                }
            }
            "w:szCs" => {
                if let Some(n) = int(val) {
                    put!(p, sz_cs, "szCs", num(n));
                }
            }
            "w:highlight" => {
                if let Some(v) = truthy(val) {
                    put!(p, highlight, "highlight", v.into());
                }
            }
            "w:u" => {
                let u = UnderlineSpec {
                    val: val.unwrap_or("single").into(),
                    color: truthy(el.attr("w:color")).map(Str::from),
                    theme_color: truthy(el.attr("w:themeColor")).map(Str::from),
                    theme_tint: truthy(el.attr("w:themeTint")).map(Str::from),
                    theme_shade: truthy(el.attr("w:themeShade")).map(Str::from),
                    ..UnderlineSpec::default()
                };
                put!(p, u, "u", Box::new(u));
            }
            "w:effect" => {
                if let Some(v) = truthy(val) {
                    put!(p, effect, "effect", v.into());
                }
            }
            "w:bdr" => put!(p, bdr, "bdr", Box::new(border_of(el))),
            "w:shd" => put!(p, shd, "shd", Box::new(shading_of(el))),
            "w:vertAlign" => {
                if let Some(v @ ("superscript" | "subscript" | "baseline")) = val {
                    put!(p, vert_align, "vertAlign", v.into());
                }
            }
            "w:em" => {
                if let Some(v) = truthy(val) {
                    put!(p, em, "em", v.into());
                }
            }
            "w:lang" => {
                let l = Lang {
                    val: truthy(el.attr("w:val")).map(Str::from),
                    east_asia: truthy(el.attr("w:eastAsia")).map(Str::from),
                    bidi: truthy(el.attr("w:bidi")).map(Str::from),
                    ..Lang::default()
                };
                put!(p, lang, "lang", Box::new(l));
            }
            _ => {}
        }
    }
    let (fx, ot) = read_text_effects(rpr);
    if let Some(fx) = fx {
        put!(p, fx, "fx", Box::new(fx));
    }
    if let Some(ot) = ot {
        put!(p, ot, "ot", Box::new(ot));
    }
    p
}

fn read_pbdr(el: Element<'_>) -> PBdr {
    let side = |name: &str, alt: Option<&str>| {
        let e = el.first_child(name).or_else(|| alt.and_then(|a| el.first_child(a)));
        read_border(e).map(Box::new)
    };
    PBdr {
        top: side("w:top", None),
        left: side("w:left", Some("w:start")),
        bottom: side("w:bottom", None),
        right: side("w:right", Some("w:end")),
        between: side("w:between", None),
        bar: side("w:bar", None),
        ..PBdr::default()
    }
}

const TAB_VALS: [&str; 9] = ["left", "center", "right", "decimal", "bar", "clear", "num", "start", "end"];
const LEADERS: [&str; 6] = ["none", "dot", "hyphen", "underscore", "heavy", "middleDot"];

/// `readPPr`: a `w:pPr` as the sparse paragraph properties.
#[must_use]
pub fn read_ppr(ppr: Option<Element<'_>>) -> Option<PPr> {
    ppr.map(ppr_of)
}

fn ppr_of(ppr: Element<'_>) -> PPr {
    let mut chain: Vec<(Element<'_>, Option<Element<'_>>)> = Vec::new();
    let mut cur = Some(ppr);
    while let Some(e) = cur.take() {
        let change = e.children_named("w:pPrChange").next_back();
        chain.push((e, change));
        cur = change.and_then(|c| c.first_child("w:pPr"));
    }
    let mut below: Option<PPr> = None;
    for (e, change) in chain.into_iter().rev() {
        let mut p = ppr_level(e);
        if let Some(c) = change {
            let info = read_revision_info(c);
            p.change = Some(Box::new(PPrChange {
                author: info.author,
                date: info.date,
                date_utc: info.date_utc,
                r#move: info.r#move,
                p_pr: Box::new(below.take().unwrap_or_default()),
                ..PPrChange::default()
            }));
        }
        below = Some(p);
    }
    below.unwrap_or_default()
}

/// One `w:pPr` without the value of `change` (set by [`ppr_of`]; the key is recorded here).
fn ppr_level(ppr: Element<'_>) -> PPr {
    let mut p = PPr::default();
    for el in ppr.child_elements() {
        let val = el.attr("w:val");
        match el.name() {
            "w:pStyle" => {
                if let Some(v) = truthy(val) {
                    put!(p, p_style, "pStyle", v.into());
                }
            }
            "w:keepNext" => put!(p, keep_next, "keepNext", on_off_val(el)),
            "w:keepLines" => put!(p, keep_lines, "keepLines", on_off_val(el)),
            "w:pageBreakBefore" => put!(p, page_break_before, "pageBreakBefore", on_off_val(el)),
            "w:widowControl" => put!(p, widow_control, "widowControl", on_off_val(el)),
            "w:suppressLineNumbers" => put!(p, suppress_line_numbers, "suppressLineNumbers", on_off_val(el)),
            "w:suppressAutoHyphens" => put!(p, suppress_auto_hyphens, "suppressAutoHyphens", on_off_val(el)),
            "w:kinsoku" => put!(p, kinsoku, "kinsoku", on_off_val(el)),
            "w:wordWrap" => put!(p, word_wrap, "wordWrap", on_off_val(el)),
            "w:overflowPunct" => put!(p, overflow_punct, "overflowPunct", on_off_val(el)),
            "w:autoSpaceDE" => put!(p, auto_space_de, "autoSpaceDE", on_off_val(el)),
            "w:autoSpaceDN" => put!(p, auto_space_dn, "autoSpaceDN", on_off_val(el)),
            "w:bidi" => put!(p, bidi, "bidi", on_off_val(el)),
            "w:adjustRightInd" => put!(p, adjust_right_ind, "adjustRightInd", on_off_val(el)),
            "w:snapToGrid" => put!(p, snap_to_grid, "snapToGrid", on_off_val(el)),
            "w:contextualSpacing" => put!(p, contextual_spacing, "contextualSpacing", on_off_val(el)),
            "w:mirrorIndents" => put!(p, mirror_indents, "mirrorIndents", on_off_val(el)),
            "w:suppressOverlap" => put!(p, suppress_overlap, "suppressOverlap", on_off_val(el)),
            "w:jc" => {
                if let Some(v) = truthy(val) {
                    put!(p, jc, "jc", v.into());
                }
            }
            "w:textboxTightWrap" => {
                if let Some(v) = truthy(val) {
                    put!(p, textbox_tight_wrap, "textboxTightWrap", v.into());
                }
            }
            "w:textAlignment" => {
                if let Some(v) = truthy(val) {
                    put!(p, text_alignment, "textAlignment", v.into());
                }
            }
            "w:outlineLvl" => {
                if let Some(n) = int(val) {
                    put!(p, outline_lvl, "outlineLvl", num(n));
                }
            }
            "w:numPr" => {
                let n = NumPr {
                    num_id: int(el.first_child("w:numId").and_then(|e| e.attr("w:val"))).map(num),
                    ilvl: int(el.first_child("w:ilvl").and_then(|e| e.attr("w:val"))).map(num),
                    ..NumPr::default()
                };
                put!(p, num_pr, "numPr", Box::new(n));
            }
            "w:pBdr" => put!(p, p_bdr, "pBdr", Box::new(read_pbdr(el))),
            "w:shd" => put!(p, shd, "shd", Box::new(shading_of(el))),
            "w:tabs" => {
                let mut tabs: Vec<TabSpec> = Vec::new();
                for t in el.children_named("w:tab") {
                    let v = match t.attr("w:val").unwrap_or("left") {
                        "start" => "left",
                        "end" => "right",
                        v => v,
                    };
                    if !TAB_VALS.contains(&v) {
                        continue;
                    }
                    let Some(pos) = measure_twips(t.attr("w:pos")) else { continue };
                    let leader = truthy(t.attr("w:leader")).filter(|l| LEADERS.contains(l));
                    tabs.push(TabSpec { val: v.into(), pos: num(pos), leader: leader.map(Str::from), ..TabSpec::default() });
                }
                put!(p, tabs, "tabs", tabs);
            }
            "w:spacing" => {
                let mut s = SpacingSpec::default();
                if let Some(n) = measure_twips(el.attr("w:before")) {
                    put!(s, before, "before", num(n));
                }
                if let Some(n) = measure_twips(el.attr("w:after")) {
                    put!(s, after, "after", num(n));
                }
                if let Some(n) = int(el.attr("w:beforeLines")) {
                    put!(s, before_lines, "beforeLines", num(n));
                }
                if let Some(n) = int(el.attr("w:afterLines")) {
                    put!(s, after_lines, "afterLines", num(n));
                }
                if let Some(v) = el.attr("w:beforeAutospacing") {
                    put!(s, before_autospacing, "beforeAutospacing", on_off_attr(Some(v)));
                }
                if let Some(v) = el.attr("w:afterAutospacing") {
                    put!(s, after_autospacing, "afterAutospacing", on_off_attr(Some(v)));
                }
                if let Some(n) = measure_twips(el.attr("w:line")) {
                    put!(s, line, "line", num(n));
                }
                if let Some(rule @ ("auto" | "exact" | "atLeast")) = el.attr("w:lineRule") {
                    put!(s, line_rule, "lineRule", rule.into());
                }
                put!(p, spacing, "spacing", Box::new(s));
            }
            "w:ind" => {
                let g = |k: &str| el.attr(k);
                let s = IndSpec {
                    left: measure_twips(g("w:left").or_else(|| g("w:start"))).map(num),
                    right: measure_twips(g("w:right").or_else(|| g("w:end"))).map(num),
                    hanging: measure_twips(g("w:hanging")).map(num),
                    first_line: measure_twips(g("w:firstLine")).map(num),
                    left_chars: int(g("w:leftChars").or_else(|| g("w:startChars"))).map(num),
                    right_chars: int(g("w:rightChars").or_else(|| g("w:endChars"))).map(num),
                    hanging_chars: int(g("w:hangingChars")).map(num),
                    first_line_chars: int(g("w:firstLineChars")).map(num),
                    ..IndSpec::default()
                };
                put!(p, ind, "ind", Box::new(s));
            }
            "w:framePr" => {
                let a = |k: &str| el.attr(k);
                let mut f = FramePr::default();
                if let Some(dc @ ("none" | "drop" | "margin")) = a("w:dropCap") {
                    put!(f, drop_cap, "dropCap", dc.into());
                }
                if let Some(n) = int(a("w:lines")) {
                    put!(f, lines, "lines", num(n));
                }
                if let Some(n) = measure_twips(a("w:w")) {
                    put!(f, w, "w", num(n));
                }
                if let Some(n) = measure_twips(a("w:h")) {
                    put!(f, h, "h", num(n));
                }
                if let Some(n) = measure_twips(a("w:hSpace")) {
                    put!(f, h_space, "hSpace", num(n));
                }
                if let Some(n) = measure_twips(a("w:vSpace")) {
                    put!(f, v_space, "vSpace", num(n));
                }
                if let Some(n) = measure_twips(a("w:x")) {
                    put!(f, x, "x", num(n));
                }
                if let Some(n) = measure_twips(a("w:y")) {
                    put!(f, y, "y", num(n));
                }
                if let Some(hr @ ("auto" | "exact" | "atLeast")) = a("w:hRule") {
                    put!(f, h_rule, "hRule", hr.into());
                }
                if let Some(v) = truthy(a("w:wrap")) {
                    put!(f, wrap, "wrap", v.into());
                }
                if let Some(v) = truthy(a("w:hAnchor")) {
                    put!(f, h_anchor, "hAnchor", v.into());
                }
                if let Some(v) = truthy(a("w:vAnchor")) {
                    put!(f, v_anchor, "vAnchor", v.into());
                }
                if let Some(v) = truthy(a("w:xAlign")) {
                    put!(f, x_align, "xAlign", v.into());
                }
                if let Some(v) = truthy(a("w:yAlign")) {
                    put!(f, y_align, "yAlign", v.into());
                }
                if let Some(v) = a("w:anchorLock") {
                    put!(f, anchor_lock, "anchorLock", on_off_attr(Some(v)));
                }
                put!(p, frame_pr, "framePr", Box::new(f));
            }
            "w:rPr" => {
                put!(p, r_pr, "rPr", Box::new(rpr_of(el)));
                for c in el.child_elements() {
                    match c.name() {
                        "w:del" | "w:moveFrom" => {
                            put!(p, mark_deleted, "markDeleted", true);
                            put!(p, mark_del, "markDel", Box::new(read_revision_info(c)));
                        }
                        "w:ins" | "w:moveTo" => put!(p, mark_ins, "markIns", Box::new(read_revision_info(c))),
                        _ => {}
                    }
                }
            }
            "w:pPrChange" => p.mark("change"),
            _ => {}
        }
    }
    p
}

/// `readBorders`: `w:tblBorders` / `w:tcBorders`; the first of each name list that exists.
fn borders_of(el: Element<'_>) -> TcBorders {
    let side = |names: &[&str]| names.iter().find_map(|n| read_border(el.first_child(n))).map(Box::new);
    TcBorders {
        top: side(&["w:top"]),
        left: side(&["w:left", "w:start"]),
        bottom: side(&["w:bottom"]),
        right: side(&["w:right", "w:end"]),
        inside_h: side(&["w:insideH"]),
        inside_v: side(&["w:insideV"]),
        tl2br: side(&["w:tl2br"]),
        tr2bl: side(&["w:tr2bl"]),
        ..TcBorders::default()
    }
}

/// `w:tblBorders` into the model's `TblBorders`: the TS reads it with `readBorders`, so the
/// diagonals are stored there too.
fn tbl_borders_of(el: Element<'_>) -> TblBorders {
    let b = borders_of(el);
    TblBorders {
        top: b.top,
        left: b.left,
        bottom: b.bottom,
        right: b.right,
        inside_h: b.inside_h,
        inside_v: b.inside_v,
        tl2br: b.tl2br,
        tr2bl: b.tr2bl,
        ..TblBorders::default()
    }
}

fn cell_mar_of(el: Element<'_>) -> CellMargins {
    CellMargins {
        top: read_width(el.first_child("w:top")),
        left: read_width(el.first_child("w:left").or_else(|| el.first_child("w:start"))),
        bottom: read_width(el.first_child("w:bottom")),
        right: read_width(el.first_child("w:right").or_else(|| el.first_child("w:end"))),
        ..CellMargins::default()
    }
}

/// `readTblPr`: a `w:tblPr` (or `w:tblPrEx`).
#[must_use]
pub fn read_tbl_pr(tbl_pr: Option<Element<'_>>) -> Option<TblPr> {
    let tbl_pr = tbl_pr?;
    Some(tbl_pr_of(tbl_pr.child_elements()))
}

/// WordCraft study (tables, the TCond aside; measured `oracle_wc/tblorder`): Word reads `w:tblPr` in order, and a
/// `w:tblStyle` naming an existing table style discards every direct property read before it. The properties Word
/// keeps when the (last) `w:tblStyle` is NOT the first child: that element and the ones after it; `None` when it is
/// first or absent (Word's view = the read one). Whether the style exists is the layout's check.
#[must_use]
pub fn read_tbl_pr_word(tbl_pr: Option<Element<'_>>) -> Option<TblPr> {
    let tbl_pr = tbl_pr?;
    let k = tbl_pr.child_elements().collect::<Vec<_>>().iter().rposition(|e| e.name() == "w:tblStyle")?;
    if k == 0 {
        return None;
    }
    Some(tbl_pr_of(tbl_pr.child_elements().skip(k)))
}

fn tbl_pr_of<'d>(children: impl Iterator<Item = Element<'d>>) -> TblPr {
    let mut p = TblPr::default();
    for el in children {
        let val = el.attr("w:val");
        match el.name() {
            "w:tblStyle" => {
                if let Some(v) = truthy(val) {
                    put!(p, tbl_style, "tblStyle", v.into());
                }
            }
            "w:tblpPr" => {
                let a = |k: &str| el.attr(k);
                let mut t = TblpPr::default();
                if let Some(n) = measure_twips(a("w:leftFromText")) {
                    put!(t, left_from_text, "leftFromText", num(n));
                }
                if let Some(n) = measure_twips(a("w:rightFromText")) {
                    put!(t, right_from_text, "rightFromText", num(n));
                }
                if let Some(n) = measure_twips(a("w:topFromText")) {
                    put!(t, top_from_text, "topFromText", num(n));
                }
                if let Some(n) = measure_twips(a("w:bottomFromText")) {
                    put!(t, bottom_from_text, "bottomFromText", num(n));
                }
                if let Some(n) = measure_twips(a("w:tblpX")) {
                    put!(t, tblp_x, "tblpX", num(n));
                }
                if let Some(n) = measure_twips(a("w:tblpY")) {
                    put!(t, tblp_y, "tblpY", num(n));
                }
                if let Some(va @ ("text" | "margin" | "page")) = a("w:vertAnchor") {
                    put!(t, vert_anchor, "vertAnchor", va.into());
                }
                if let Some(ha @ ("text" | "margin" | "page")) = a("w:horzAnchor") {
                    put!(t, horz_anchor, "horzAnchor", ha.into());
                }
                if let Some(v) = truthy(a("w:tblpXSpec")) {
                    put!(t, tblp_x_spec, "tblpXSpec", v.into());
                }
                if let Some(v) = truthy(a("w:tblpYSpec")) {
                    put!(t, tblp_y_spec, "tblpYSpec", v.into());
                }
                put!(p, tblp_pr, "tblpPr", Box::new(t));
            }
            "w:tblOverlap" => {
                if let Some(v) = truthy(val) {
                    put!(p, tbl_overlap, "tblOverlap", v.into());
                }
            }
            "w:bidiVisual" => put!(p, bidi_visual, "bidiVisual", on_off_val(el)),
            "w:tblStyleRowBandSize" => {
                if let Some(n) = int(val) {
                    put!(p, tbl_style_row_band_size, "tblStyleRowBandSize", num(n));
                }
            }
            "w:tblStyleColBandSize" => {
                if let Some(n) = int(val) {
                    put!(p, tbl_style_col_band_size, "tblStyleColBandSize", num(n));
                }
            }
            "w:tblW" => put!(p, tbl_w, "tblW", width_of(el)),
            "w:jc" => {
                if let Some(v) = truthy(val) {
                    put!(p, jc, "jc", v.into());
                }
            }
            "w:tblCellSpacing" => put!(p, tbl_cell_spacing, "tblCellSpacing", width_of(el)),
            "w:tblInd" => put!(p, tbl_ind, "tblInd", width_of(el)),
            "w:tblBorders" => put!(p, tbl_borders, "tblBorders", Box::new(tbl_borders_of(el))),
            "w:shd" => put!(p, shd, "shd", Box::new(shading_of(el))),
            "w:tblLayout" => {
                put!(p, tbl_layout, "tblLayout", if el.attr("w:type") == Some("fixed") { "fixed".into() } else { "autofit".into() })
            }
            "w:tblCaption" => {
                if let Some(v) = val {
                    put!(p, tbl_caption, "tblCaption", v.into());
                }
            }
            "w:tblDescription" => {
                if let Some(v) = val {
                    put!(p, tbl_description, "tblDescription", v.into());
                }
            }
            "w:tblCellMar" => put!(p, tbl_cell_mar, "tblCellMar", Box::new(cell_mar_of(el))),
            "w:tblLook" => {
                let a = |k: &str| el.attr(k);
                let mut look = TblLook::default();
                let hex = parse::parse_int(a("w:val").unwrap_or(""), 16);
                if hex.is_finite() {
                    let bits = conv::to_int32(hex);
                    look.first_row = Some(bits & 0x20 != 0);
                    look.last_row = Some(bits & 0x40 != 0);
                    look.first_column = Some(bits & 0x80 != 0);
                    look.last_column = Some(bits & 0x100 != 0);
                    look.no_h_band = Some(bits & 0x200 != 0);
                    look.no_v_band = Some(bits & 0x400 != 0);
                }
                if let Some(v) = a("w:firstRow") {
                    look.first_row = Some(on_off_attr(Some(v)));
                }
                if let Some(v) = a("w:lastRow") {
                    look.last_row = Some(on_off_attr(Some(v)));
                }
                if let Some(v) = a("w:firstColumn") {
                    look.first_column = Some(on_off_attr(Some(v)));
                }
                if let Some(v) = a("w:lastColumn") {
                    look.last_column = Some(on_off_attr(Some(v)));
                }
                if let Some(v) = a("w:noHBand") {
                    look.no_h_band = Some(on_off_attr(Some(v)));
                }
                if let Some(v) = a("w:noVBand") {
                    look.no_v_band = Some(on_off_attr(Some(v)));
                }
                put!(p, tbl_look, "tblLook", Box::new(look));
            }
            _ => {}
        }
    }
    p
}

/// `readTrPr`: a `w:trPr`.
#[must_use]
pub fn read_tr_pr(tr_pr: Option<Element<'_>>) -> Option<TrPr> {
    let tr_pr = tr_pr?;
    let mut p = TrPr::default();
    for el in tr_pr.child_elements() {
        let val = el.attr("w:val");
        match el.name() {
            "w:cnfStyle" => {
                if let Some(v) = truthy(val) {
                    put!(p, cnf_style, "cnfStyle", v.into());
                }
            }
            "w:gridBefore" => {
                if let Some(n) = int(val) {
                    put!(p, grid_before, "gridBefore", num(n));
                }
            }
            "w:gridAfter" => {
                if let Some(n) = int(val) {
                    put!(p, grid_after, "gridAfter", num(n));
                }
            }
            "w:wBefore" => put!(p, w_before, "wBefore", width_of(el)),
            "w:wAfter" => put!(p, w_after, "wAfter", width_of(el)),
            "w:cantSplit" => put!(p, cant_split, "cantSplit", on_off_val(el)),
            "w:trHeight" => {
                let v = measure_twips(val).unwrap_or(0.0);
                let rule = el.attr("w:hRule").filter(|r| matches!(*r, "exact" | "auto")).unwrap_or("atLeast");
                put!(p, tr_height, "trHeight", TrHeight { val: num(v), h_rule: rule.into(), ..TrHeight::default() });
            }
            "w:tblHeader" => put!(p, tbl_header, "tblHeader", on_off_val(el)),
            "w:tblCellSpacing" => put!(p, tbl_cell_spacing, "tblCellSpacing", width_of(el)),
            "w:jc" => {
                if let Some(v) = truthy(val) {
                    put!(p, jc, "jc", v.into());
                }
            }
            "w:hidden" => put!(p, hidden, "hidden", on_off_val(el)),
            _ => {}
        }
    }
    Some(p)
}

/// `readTcPr`: a `w:tcPr`.
#[must_use]
pub fn read_tc_pr(tc_pr: Option<Element<'_>>) -> Option<TcPr> {
    let tc_pr = tc_pr?;
    let mut p = TcPr::default();
    for el in tc_pr.child_elements() {
        let val = el.attr("w:val");
        match el.name() {
            "w:cnfStyle" => {
                if let Some(v) = truthy(val) {
                    put!(p, cnf_style, "cnfStyle", v.into());
                }
            }
            "w:tcW" => put!(p, tc_w, "tcW", width_of(el)),
            "w:gridSpan" => {
                if let Some(n) = int(val) {
                    put!(p, grid_span, "gridSpan", num(n));
                }
            }
            "w:hMerge" => put!(p, h_merge, "hMerge", if val == Some("restart") { "restart".into() } else { "continue".into() }),
            "w:vMerge" => put!(p, v_merge, "vMerge", if val == Some("restart") { "restart".into() } else { "continue".into() }),
            "w:tcBorders" => put!(p, tc_borders, "tcBorders", Box::new(borders_of(el))),
            "w:shd" => put!(p, shd, "shd", Box::new(shading_of(el))),
            "w:noWrap" => put!(p, no_wrap, "noWrap", on_off_val(el)),
            "w:tcMar" => put!(p, tc_mar, "tcMar", Box::new(cell_mar_of(el))),
            "w:textDirection" => {
                if let Some(v) = truthy(val) {
                    put!(p, text_direction, "textDirection", v.into());
                }
            }
            "w:tcFitText" => put!(p, tc_fit_text, "tcFitText", on_off_val(el)),
            "w:vAlign" => {
                if let Some(v @ ("top" | "center" | "bottom" | "both")) = val {
                    put!(p, v_align, "vAlign", v.into());
                }
            }
            "w:hideMark" => put!(p, hide_mark, "hideMark", on_off_val(el)),
            _ => {}
        }
    }
    Some(p)
}
