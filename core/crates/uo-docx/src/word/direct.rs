//! The effective direct formatting (`docx-layout/src/context.ts` `directRPr` 245, `directPPr`
//! 317, with `setFx`, `setOt`, `uSpec`, `colorSpec`), the paragraph's resolved `pPr`
//! (`para.ts` `resolvePPr` 682) and the ruler helpers of `paraformat.ts` (`inheritedPPr`,
//! `effectivePPr`).
//!
//! The reader keeps the original `w:rPr`/`w:pPr` (`run.rpr`, `para.ppr`) and derives the editing
//! props from them; an edit only changes the props. A prop that differs from what the original
//! implies (`editBaseProps`, `paraPropsFromPPr`, uo-docx R4a) was edited and overrides the
//! original, as direct formatting. The comparisons `eq` are `JSON.stringify` equality
//! ([`json_eq`], K9).

use std::sync::Arc;

use super::numbering::NumberingState;
use super::obj::{delete, json_eq, set, set_undefined, spread_opt};
use super::resolve::{StyleResolver, merge_ppr};
use crate::js::{JsNumber, JsValue, Str};
use crate::model::effects::{OpenTypeProps, TextEffects};
use crate::model::formatting::DocFormatting;
use crate::model::para::ParaProps;
use crate::model::props::{
    BorderSpec, ColorSpec, IndSpec, NumPr, PBdr, PPr, RFonts, RPr, ShadingSpec, SpacingSpec, TabSpec, TextFill, TextShadow, UnderlineSpec,
};
use crate::model::run::{Run, RunProps};
use crate::read::props_map::{edit_base_props, para_props_from_ppr};

/// `a !== b` of two optional numbers of the model (`undefined`, `null` and NaN as in JS).
fn ne_num(a: Option<JsNumber>, b: Option<JsNumber>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(JsNumber::Num(x)), Some(JsNumber::Num(y))) => x != y,
        (Some(JsNumber::Null), Some(JsNumber::Null)) => false,
        _ => true,
    }
}

/// `x ?? d` for a number (`null` and `undefined` give `d`).
fn or_num(a: Option<JsNumber>, d: f64) -> JsNumber {
    match a {
        Some(JsNumber::Num(x)) => JsNumber::Num(x),
        _ => JsNumber::Num(d),
    }
}

fn truthy_num(a: Option<JsNumber>) -> bool {
    a.is_some_and(JsNumber::truthy)
}

fn truthy_str(a: Option<&Str>) -> bool {
    a.is_some_and(|s| !s.is_empty())
}

/// `o.k = v` where `v` may be `undefined` (the key then holds `undefined`).
fn put<T: crate::js::JsObject, V>(o: &mut T, key: &str, v: Option<V>, field: impl Fn(&mut T) -> &mut Option<V>) {
    match v {
        Some(v) => set(o, key, |o| *field(o) = Some(v)),
        None => set_undefined(o, key, |o| *field(o) = None),
    }
}

/// `uSpec(p)` (context.ts:305): the typed `w:u` of the editing props.
fn u_spec(p: &RunProps) -> UnderlineSpec {
    let mut u = UnderlineSpec { val: p.underline_style.clone().unwrap_or_else(|| "single".into()), ..UnderlineSpec::default() };
    if truthy_str(p.underline_color.as_ref()) {
        u.color.clone_from(&p.underline_color);
    }
    if let Some(t) = &p.underline_theme {
        u.theme_color = Some(t.theme_color.clone());
        if truthy_str(t.tint.as_ref()) {
            u.theme_tint.clone_from(&t.tint);
        }
        if truthy_str(t.shade.as_ref()) {
            u.theme_shade.clone_from(&t.shade);
        }
    }
    u
}

/// `colorSpec(p)` (context.ts:311): the typed `w:color` of the editing props.
fn color_spec(p: &RunProps) -> ColorSpec {
    let mut c = ColorSpec { val: p.color.clone(), ..ColorSpec::default() };
    if let Some(t) = &p.color_theme {
        c.theme_color = Some(t.theme_color.clone());
        if truthy_str(t.tint.as_ref()) {
            c.theme_tint.clone_from(&t.tint);
        }
        if truthy_str(t.shade.as_ref()) {
            c.theme_shade.clone_from(&t.shade);
        }
    }
    c
}

/// `setFx(base, fx)` (context.ts:226): the edited w14 effects with the older fields the layout
/// reads (`textFill`, `textOutline`, `textShadow`). A solid fill without a colour (the TS throws
/// reading `f.color.mods`) reads as a colour without fields.
fn set_fx(base: &mut RPr, fx: Option<&TextEffects>) {
    delete(base, "textFill");
    delete(base, "textOutline");
    delete(base, "textShadow");
    let Some(fx) = fx else {
        delete(base, "fx");
        return;
    };
    let fxv = fx.clone();
    set(base, "fx", |b| b.fx = Some(Box::new(fxv)));
    let f = fx.fill.as_deref();
    match f.map(|f| f.r#type.as_ref()) {
        Some("none") => {
            let tf = TextFill { alpha: Some(JsNumber::Num(0.0)), ..TextFill::default() };
            set(base, "textFill", |b| b.text_fill = Some(Box::new(tf)));
        }
        Some("solid") => {
            let color = f.and_then(|f| f.color.as_ref());
            let a = color.and_then(|c| c.mods.as_ref()).and_then(|m| m.iter().find(|(k, _)| k.as_ref() == "alpha")).map(|(_, v)| *v);
            let mut tf = TextFill::default();
            if let Some(c) = color {
                if truthy_str(c.rgb.as_ref()) {
                    tf.color.clone_from(&c.rgb);
                }
                if truthy_str(c.scheme.as_ref()) {
                    tf.theme_color.clone_from(&c.scheme);
                }
            }
            if let Some(a) = a {
                tf.alpha = Some(JsNumber::Num(uo_jsnum::math::max(0.0, 1.0 - a.to_number() / 100_000.0)));
            }
            set(base, "textFill", |b| b.text_fill = Some(Box::new(tf)));
        }
        _ => {}
    }
    if fx.outline.as_deref().and_then(|o| o.fill.as_deref()).is_some_and(|f| f.r#type.as_ref() != "none") {
        set(base, "textOutline", |b| b.text_outline = Some(true));
    }
    if let Some(s) = fx.shadow.as_deref() {
        let ts = TextShadow {
            blur: JsNumber::Num(s.blur_rad.to_number() / 12_700.0),
            dist: JsNumber::Num(s.dist.to_number() / 12_700.0),
            dir: JsNumber::Num(s.dir.to_number() / 60_000.0),
            ..TextShadow::default()
        };
        set(base, "textShadow", |b| b.text_shadow = Some(Box::new(ts)));
    }
}

/// `setOt(base, ot)` (context.ts:239).
fn set_ot(base: &mut RPr, ot: Option<&OpenTypeProps>) {
    let Some(ot) = ot else {
        delete(base, "ot");
        delete(base, "ligatures");
        return;
    };
    let otv = ot.clone();
    set(base, "ot", |b| b.ot = Some(Box::new(otv)));
    match ot.ligatures.clone().filter(|s| !s.is_empty()) {
        Some(l) => set(base, "ligatures", |b| b.ligatures = Some(l)),
        None => delete(base, "ligatures"),
    }
}

/// `directRPr(run)` (context.ts:245): the effective direct `rPr` of a run.
#[must_use]
pub fn direct_rpr(run: &Run) -> RPr {
    direct_rpr_with(run, &run.props)
}

/// `directRPr({ ...run, props })`: the same with other editing props (the Original revision view,
/// revisions.ts:298); the run gives `rpr` and the reader's forced props (`editBaseProps`).
#[must_use]
pub fn direct_rpr_with(run: &Run, p: &RunProps) -> RPr {
    let mut base: RPr = run.rpr.as_deref().cloned().unwrap_or_default();
    let orig = edit_base_props(run, run.rpr.as_deref());
    if run.rpr.is_none() {
        // a run created by editing: the props are the direct formatting
        if p.bold == Some(true) {
            set(&mut base, "b", |b| b.b = Some(true));
        }
        if p.italic == Some(true) {
            set(&mut base, "i", |b| b.i = Some(true));
        }
        if p.underline == Some(true)
            && !(truthy_str(p.link.as_ref()) && !truthy_str(p.underline_style.as_ref()) && !truthy_str(p.underline_color.as_ref()))
        {
            let u = u_spec(p);
            set(&mut base, "u", |b| b.u = Some(Box::new(u)));
        }
        if p.strike == Some(true) {
            set(&mut base, "strike", |b| b.strike = Some(true));
        }
        if p.double_strike == Some(true) {
            set(&mut base, "dstrike", |b| b.dstrike = Some(true));
        }
        if p.all_caps == Some(true) {
            set(&mut base, "caps", |b| b.caps = Some(true));
        }
        if p.small_caps == Some(true) {
            set(&mut base, "smallCaps", |b| b.small_caps = Some(true));
        }
        if truthy_str(p.vert_align.as_ref()) && p.vert_align != orig.vert_align {
            let v = p.vert_align.clone();
            set(&mut base, "vertAlign", |b| b.vert_align = v);
        }
        if let Some(sz) = p.sz_half_pt {
            set(&mut base, "sz", |b| b.sz = Some(sz));
        }
        if truthy_str(p.color.as_ref()) && !(truthy_str(p.link.as_ref()) && p.color.as_deref() == Some("0563C1") && p.color_theme.is_none())
        {
            let c = color_spec(p);
            set(&mut base, "color", |b| b.color = Some(Box::new(c)));
        }
        if p.hidden == Some(true) {
            set(&mut base, "vanish", |b| b.vanish = Some(true));
        }
        if let Some(w) = p.scale_pct
            && (w.is_null() || w.as_f64() != Some(100.0))
        {
            set(&mut base, "w", |b| b.w = Some(w));
        }
        if truthy_num(p.position_half_pt) {
            let v = p.position_half_pt;
            set(&mut base, "position", |b| b.position = v);
        }
        if truthy_num(p.kern_half_pt) {
            let v = p.kern_half_pt;
            set(&mut base, "kern", |b| b.kern = v);
        }
        if truthy_str(p.highlight.as_ref()) {
            let v = p.highlight.clone();
            set(&mut base, "highlight", |b| b.highlight = v);
        }
        if truthy_str(p.font_ascii.as_ref()) && p.font_ascii != orig.font_ascii {
            let a = p.font_ascii.clone();
            let f = RFonts { ascii: a.clone(), h_ansi: a.clone(), cs: a, ..RFonts::default() };
            set(&mut base, "rFonts", |b| b.r_fonts = Some(Box::new(f)));
        }
        if truthy_num(p.char_spacing_twips) {
            let v = p.char_spacing_twips;
            set(&mut base, "spacing", |b| b.spacing = v);
        }
        if truthy_str(p.char_style.as_ref()) {
            let v = p.char_style.clone();
            set(&mut base, "rStyle", |b| b.r_style = v);
        }
        if let Some(fx) = p.text_fx.as_deref() {
            set_fx(&mut base, Some(fx));
        }
        if let Some(ot) = p.open_type.as_deref() {
            set_ot(&mut base, Some(ot));
        }
        return base;
    }
    if !json_eq(p.text_fx.as_deref().map(|v| v as &dyn JsValue), orig.text_fx.as_deref().map(|v| v as &dyn JsValue)) {
        set_fx(&mut base, p.text_fx.as_deref());
    }
    if !json_eq(p.open_type.as_deref().map(|v| v as &dyn JsValue), orig.open_type.as_deref().map(|v| v as &dyn JsValue)) {
        set_ot(&mut base, p.open_type.as_deref());
    }
    if p.bold != orig.bold {
        put(&mut base, "b", p.bold, |b| &mut b.b);
    }
    if p.italic != orig.italic {
        put(&mut base, "i", p.italic, |b| &mut b.i);
    }
    if p.underline != orig.underline
        || p.underline_style != orig.underline_style
        || p.underline_color != orig.underline_color
        || !json_eq(p.underline_theme.as_ref().map(|v| v as &dyn JsValue), orig.underline_theme.as_ref().map(|v| v as &dyn JsValue))
    {
        let u = if p.underline == Some(true) { u_spec(p) } else { UnderlineSpec { val: "none".into(), ..UnderlineSpec::default() } };
        set(&mut base, "u", |b| b.u = Some(Box::new(u)));
    }
    if (p.hidden == Some(true)) != (orig.hidden == Some(true)) {
        let v = p.hidden == Some(true);
        set(&mut base, "vanish", |b| b.vanish = Some(v));
    }
    if ne_num(Some(or_num(p.scale_pct, 100.0)), Some(or_num(orig.scale_pct, 100.0))) {
        match p.scale_pct {
            None => delete(&mut base, "w"),
            Some(w) => set(&mut base, "w", |b| b.w = Some(w)),
        }
    }
    if ne_num(Some(or_num(p.position_half_pt, 0.0)), Some(or_num(orig.position_half_pt, 0.0))) {
        if truthy_num(p.position_half_pt) {
            let v = p.position_half_pt;
            set(&mut base, "position", |b| b.position = v);
        } else {
            delete(&mut base, "position");
        }
    }
    if ne_num(Some(or_num(p.kern_half_pt, 0.0)), Some(or_num(orig.kern_half_pt, 0.0))) {
        if truthy_num(p.kern_half_pt) {
            let v = p.kern_half_pt;
            set(&mut base, "kern", |b| b.kern = v);
        } else {
            delete(&mut base, "kern");
        }
    }
    if p.strike != orig.strike {
        put(&mut base, "strike", p.strike, |b| &mut b.strike);
    }
    if p.double_strike != orig.double_strike {
        let v = p.double_strike == Some(true);
        set(&mut base, "dstrike", |b| b.dstrike = Some(v));
    }
    if p.all_caps != orig.all_caps {
        put(&mut base, "caps", p.all_caps, |b| &mut b.caps);
    }
    if p.small_caps != orig.small_caps {
        put(&mut base, "smallCaps", p.small_caps, |b| &mut b.small_caps);
    }
    if p.vert_align != orig.vert_align {
        let v = p.vert_align.clone().unwrap_or_else(|| "baseline".into());
        set(&mut base, "vertAlign", |b| b.vert_align = Some(v));
    }
    if ne_num(p.sz_half_pt, orig.sz_half_pt) {
        match p.sz_half_pt {
            None => delete(&mut base, "sz"),
            Some(v) => set(&mut base, "sz", |b| b.sz = Some(v)),
        }
    }
    if p.color != orig.color
        || !json_eq(p.color_theme.as_ref().map(|v| v as &dyn JsValue), orig.color_theme.as_ref().map(|v| v as &dyn JsValue))
    {
        if p.color.is_none() {
            delete(&mut base, "color");
        } else {
            let c = color_spec(p);
            set(&mut base, "color", |b| b.color = Some(Box::new(c)));
        }
    }
    if p.highlight != orig.highlight {
        match p.highlight.clone() {
            None => delete(&mut base, "highlight"),
            Some(h) => set(&mut base, "highlight", |b| b.highlight = Some(h)),
        }
    }
    if p.font_ascii != orig.font_ascii {
        match p.font_ascii.clone() {
            None => delete(&mut base, "rFonts"),
            Some(a) => {
                let mut f = base.r_fonts.as_deref().cloned().unwrap_or_default();
                set(&mut f, "ascii", |f| f.ascii = Some(a.clone()));
                set(&mut f, "hAnsi", |f| f.h_ansi = Some(a.clone()));
                set(&mut f, "cs", |f| f.cs = Some(a));
                set_undefined(&mut f, "asciiTheme", |f| f.ascii_theme = None);
                set_undefined(&mut f, "hAnsiTheme", |f| f.h_ansi_theme = None);
                set_undefined(&mut f, "cstheme", |f| f.cstheme = None);
                set(&mut base, "rFonts", |b| b.r_fonts = Some(Box::new(f)));
            }
        }
    }
    if ne_num(p.char_spacing_twips, orig.char_spacing_twips) {
        if truthy_num(p.char_spacing_twips) {
            let v = p.char_spacing_twips;
            set(&mut base, "spacing", |b| b.spacing = v);
        } else {
            delete(&mut base, "spacing");
        }
    }
    if p.char_style != orig.char_style {
        match p.char_style.clone() {
            None => delete(&mut base, "rStyle"),
            Some(s) => set(&mut base, "rStyle", |b| b.r_style = Some(s)),
        }
    }
    base
}

/// The six line/page-break flags of `directPPr` (context.ts:369).
const FLAGS: [&str; 6] = ["pageBreakBefore", "keepNext", "keepLines", "widowControl", "suppressLineNumbers", "suppressAutoHyphens"];

fn para_flag(p: &ParaProps, k: usize) -> Option<bool> {
    match k {
        0 => p.page_break_before,
        1 => p.keep_next,
        2 => p.keep_lines,
        3 => p.widow_control,
        4 => p.suppress_line_numbers,
        5 => p.suppress_auto_hyphens,
        _ => None,
    }
}

fn ppr_flag(p: &mut PPr, k: usize) -> Option<&mut Option<bool>> {
    Some(match k {
        0 => &mut p.page_break_before,
        1 => &mut p.keep_next,
        2 => &mut p.keep_lines,
        3 => &mut p.widow_control,
        4 => &mut p.suppress_line_numbers,
        5 => &mut p.suppress_auto_hyphens,
        _ => return None,
    })
}

/// `directPPr(para)` (context.ts:317) over the paragraph's `ppr` and editing props.
#[must_use]
pub fn direct_ppr(ppr: Option<&PPr>, p: &ParaProps) -> PPr {
    let mut base: PPr = ppr.cloned().unwrap_or_default();
    let orig = para_props_from_ppr(ppr);
    if p.alignment != orig.alignment {
        match p.alignment.clone().filter(|s| !s.is_empty()) {
            Some(a) => set(&mut base, "jc", |b| b.jc = Some(a)),
            None => delete(&mut base, "jc"),
        }
    }
    if p.style_id != orig.style_id {
        match p.style_id.clone().filter(|s| !s.is_empty()) {
            Some(s) => set(&mut base, "pStyle", |b| b.p_style = Some(s)),
            None => delete(&mut base, "pStyle"),
        }
    }
    if !json_eq(p.numbering.as_ref().map(|v| v as &dyn JsValue), orig.numbering.as_ref().map(|v| v as &dyn JsValue)) {
        let n = match &p.numbering {
            None => NumPr { num_id: Some(JsNumber::Num(0.0)), ..NumPr::default() },
            Some(nb) if nb.num_id.is_some() => NumPr { num_id: nb.num_id, ilvl: Some(nb.ilvl), ..NumPr::default() },
            Some(nb) => match ppr.and_then(|x| x.num_pr.as_deref()) {
                Some(old) if old.num_id.is_some_and(JsNumber::truthy) => {
                    let mut x = old.clone();
                    set(&mut x, "ilvl", |x| x.ilvl = Some(nb.ilvl));
                    x
                }
                _ => NumPr {
                    num_id: Some(JsNumber::Num(if nb.kind.as_ref() == "decimal" { 1002.0 } else { 1001.0 })),
                    ilvl: Some(nb.ilvl),
                    ..NumPr::default()
                },
            },
        };
        set(&mut base, "numPr", |b| b.num_pr = Some(Box::new(n)));
    }
    if ne_num(p.indent_left_twips, orig.indent_left_twips)
        || ne_num(p.indent_right_twips, orig.indent_right_twips)
        || ne_num(p.indent_first_line_twips, orig.indent_first_line_twips)
    {
        let mut ind: IndSpec = base.ind.as_deref().cloned().unwrap_or_default();
        match p.indent_left_twips {
            Some(v) => set(&mut ind, "left", |i| i.left = Some(v)),
            None => delete(&mut ind, "left"),
        }
        match p.indent_right_twips {
            Some(v) => set(&mut ind, "right", |i| i.right = Some(v)),
            None => delete(&mut ind, "right"),
        }
        delete(&mut ind, "firstLine");
        delete(&mut ind, "hanging");
        if let Some(f) = p.indent_first_line_twips {
            if f.to_number() < 0.0 && !f.is_null() {
                let h = JsNumber::Num(-f.to_number());
                set(&mut ind, "hanging", |i| i.hanging = Some(h));
            } else {
                set(&mut ind, "firstLine", |i| i.first_line = Some(f));
            }
        }
        set(&mut base, "ind", |b| b.ind = Some(Box::new(ind)));
    }
    if ne_num(p.line_twentieths, orig.line_twentieths)
        || ne_num(p.space_after_twips, orig.space_after_twips)
        || ne_num(p.space_before_twips, orig.space_before_twips)
        || p.space_before_auto != orig.space_before_auto
        || p.space_after_auto != orig.space_after_auto
    {
        let mut sp: SpacingSpec = base.spacing.as_deref().cloned().unwrap_or_default();
        if p.space_before_auto != orig.space_before_auto {
            match p.space_before_auto {
                None => delete(&mut sp, "beforeAutospacing"),
                Some(v) => set(&mut sp, "beforeAutospacing", |s| s.before_autospacing = Some(v)),
            }
        }
        if p.space_after_auto != orig.space_after_auto {
            match p.space_after_auto {
                None => delete(&mut sp, "afterAutospacing"),
                Some(v) => set(&mut sp, "afterAutospacing", |s| s.after_autospacing = Some(v)),
            }
        }
        if ne_num(p.line_twentieths, orig.line_twentieths) {
            match p.line_twentieths {
                Some(v) => {
                    set(&mut sp, "line", |s| s.line = Some(v));
                    set(&mut sp, "lineRule", |s| s.line_rule = Some("auto".into()));
                }
                None => {
                    delete(&mut sp, "line");
                    delete(&mut sp, "lineRule");
                }
            }
        }
        if ne_num(p.space_after_twips, orig.space_after_twips) {
            match p.space_after_twips {
                Some(v) => set(&mut sp, "after", |s| s.after = Some(v)),
                None => delete(&mut sp, "after"),
            }
        }
        if ne_num(p.space_before_twips, orig.space_before_twips) {
            match p.space_before_twips {
                Some(v) => set(&mut sp, "before", |s| s.before = Some(v)),
                None => delete(&mut sp, "before"),
            }
        }
        set(&mut base, "spacing", |b| b.spacing = Some(Box::new(sp)));
    }
    if p.shading_hex != orig.shading_hex {
        match p.shading_hex.clone().filter(|s| !s.is_empty()) {
            Some(h) => {
                let shd = ShadingSpec { val: Some("clear".into()), fill: Some(h), ..ShadingSpec::default() };
                set(&mut base, "shd", |b| b.shd = Some(Box::new(shd)));
            }
            None => delete(&mut base, "shd"),
        }
    }
    if !json_eq(p.borders.as_deref().map(|v| v as &dyn JsValue), orig.borders.as_deref().map(|v| v as &dyn JsValue)) {
        match p.borders.as_deref() {
            None => delete(&mut base, "pBdr"),
            Some(bs) => {
                let edge = |e: &crate::model::para::BorderEdge| -> Box<BorderSpec> {
                    let mut b =
                        BorderSpec { val: e.style.clone(), sz: Some(e.size_eighth_pt), space: Some(e.space_pt), ..BorderSpec::default() };
                    if truthy_str(e.color.as_ref()) {
                        b.color.clone_from(&e.color);
                    }
                    Box::new(b)
                };
                let pb = PBdr {
                    top: bs.top.as_ref().map(edge),
                    left: bs.left.as_ref().map(edge),
                    bottom: bs.bottom.as_ref().map(edge),
                    right: bs.right.as_ref().map(edge),
                    between: bs.between.as_ref().map(edge),
                    bar: bs.bar.as_ref().map(edge),
                    ..PBdr::default()
                };
                set(&mut base, "pBdr", |b| b.p_bdr = Some(Box::new(pb)));
            }
        }
    }
    if !json_eq(p.tab_stops.as_ref().map(|v| v as &dyn JsValue), orig.tab_stops.as_ref().map(|v| v as &dyn JsValue)) {
        let tabs: Vec<TabSpec> = p
            .tab_stops
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|t| {
                let mut s = TabSpec { val: t.align.clone(), pos: t.pos_twips, ..TabSpec::default() };
                if truthy_str(t.leader.as_ref()) {
                    s.leader.clone_from(&t.leader);
                }
                s
            })
            .collect();
        set(&mut base, "tabs", |b| b.tabs = Some(tabs));
    }
    // line and page breaks: absent = inherited, false = off against the inherited
    for (k, name) in FLAGS.iter().enumerate() {
        let v = para_flag(p, k);
        if v != para_flag(&orig, k) {
            match v {
                None => delete(&mut base, name),
                Some(v) => set(&mut base, name, |b| {
                    if let Some(f) = ppr_flag(b, k) {
                        *f = Some(v);
                    }
                }),
            }
        }
    }
    if p.textbox_tight_wrap != orig.textbox_tight_wrap {
        match p.textbox_tight_wrap.clone().filter(|s| !s.is_empty()) {
            Some(t) => set(&mut base, "textboxTightWrap", |b| b.textbox_tight_wrap = Some(t)),
            None => delete(&mut base, "textboxTightWrap"),
        }
    }
    base
}

/// `resolvePPr`'s answer (para.ts:682).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPPr {
    pub ppr: PPr,
    /// the paragraph style in effect
    pub style_id: Option<Box<str>>,
    /// the paragraph style's run properties
    pub style_rpr: Arc<RPr>,
}

/// `resolvePPr(ctx, para, env)` (para.ts:682): docDefaults → table → style → numbering → direct.
/// The numbering level's `pPr` enters at the level of whoever sets the numbering: direct, between
/// the style and the direct formatting; from a style, just before the style that brings the
/// `numId`. `table_ppr` is `env.tablePPr`.
pub fn resolve_ppr(
    fmt: &DocFormatting,
    resolver: &mut StyleResolver,
    ppr: Option<&PPr>,
    props: &ParaProps,
    table_ppr: Option<&PPr>,
) -> ResolvedPPr {
    let dd = &fmt.styles.doc_defaults;
    let direct = direct_ppr(ppr, props);
    let style_id: Option<Box<str>> = StyleResolver::para_style_id(fmt, direct.p_style.as_deref()).map(Into::into);
    let st = resolver.para_style(fmt, style_id.as_deref());
    let mut out = merge_ppr(dd.p_pr.as_deref(), table_ppr);
    let num_pr: NumPr = spread_opt(st.p_pr.num_pr.as_deref(), direct.num_pr.as_deref());
    let lvl = if num_pr.num_id.is_some_and(JsNumber::truthy) {
        let ilvl = match num_pr.ilvl {
            Some(JsNumber::Num(x)) => JsNumber::Num(x),
            _ => JsNumber::Num(0.0),
        };
        NumberingState::level_in(fmt, num_pr.num_id.unwrap_or(JsNumber::Null), ilvl)
    } else {
        None
    };
    let add_lvl = |ppr: &mut PPr| {
        if let Some(lp) = lvl.and_then(|l| l.p_pr.as_deref()) {
            let mut x = lp.clone();
            set_undefined(&mut x, "tabs", |x| x.tabs = None);
            *ppr = merge_ppr(Some(ppr), Some(&x));
            if let Some(tabs) = &lp.tabs {
                let t = PPr { tabs: Some(tabs.clone()), ..PPr::default() };
                *ppr = merge_ppr(Some(ppr), Some(&t));
            }
        }
    };
    if num_pr.num_id.is_some_and(JsNumber::truthy) && direct.num_pr.as_deref().is_none_or(|n| n.num_id.is_none()) {
        let chain = StyleResolver::para_chain(fmt, style_id.as_deref());
        let mut at: Option<usize> = None;
        for (i, s) in chain.iter().enumerate() {
            if s.p_pr.as_deref().and_then(|p| p.num_pr.as_deref()).is_some_and(|n| n.num_id.is_some()) {
                at = Some(i);
            }
        }
        for (i, s) in chain.iter().enumerate() {
            if Some(i) == at {
                add_lvl(&mut out);
            }
            out = merge_ppr(Some(&out), s.p_pr.as_deref());
        }
        if at.is_none() {
            add_lvl(&mut out);
        }
    } else {
        out = merge_ppr(Some(&out), Some(&st.p_pr));
        if num_pr.num_id.is_some_and(JsNumber::truthy) {
            add_lvl(&mut out);
        }
    }
    out = merge_ppr(Some(&out), Some(&direct));
    set(&mut out, "numPr", |o| o.num_pr = Some(Box::new(num_pr)));
    ResolvedPPr { ppr: out, style_id, style_rpr: Arc::clone(&st.r_pr) }
}

/// `inheritedPPr(fmt, para)` (paraformat.ts:20): the `pPr` the paragraph inherits (docDefaults →
/// style → numbering), without direct formatting beyond the style and the list — the base Word
/// compares the ruler's edits against.
pub fn inherited_ppr(fmt: &DocFormatting, resolver: &mut StyleResolver, ppr: Option<&PPr>, props: &ParaProps) -> PPr {
    let d = direct_ppr(ppr, props);
    let mut base = PPr::default();
    if let Some(s) = d.p_style.clone() {
        set(&mut base, "pStyle", |b| b.p_style = Some(s));
    }
    if let Some(n) = d.num_pr.clone() {
        set(&mut base, "numPr", |b| b.num_pr = Some(n));
    }
    let props = para_props_from_ppr(Some(&base));
    resolve_ppr(fmt, resolver, Some(&base), &props, None).ppr
}

/// `effectivePPr(fmt, para)` (paraformat.ts:29): the paragraph's effective `pPr`.
pub fn effective_ppr(fmt: &DocFormatting, resolver: &mut StyleResolver, ppr: Option<&PPr>, props: &ParaProps) -> PPr {
    resolve_ppr(fmt, resolver, ppr, props, None).ppr
}
