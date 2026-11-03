//! Frontier R2 (D1a reader, record "Milestone 1 — D1a foundation"): `styles.ts`:
//! `readStyleRunProps`, `readStyleParaProps`, `readStyles`, `readStylesXml` (the declarations are
//! in `core/docs/modules/uo-docx-d1a/R2.md`).
//!
//! `readStylesXml` resolves each style through its `basedOn` chain with a recursive `resolve`
//! that caches in a `Map` after the base: the map's insertion order is the post-order of the
//! walks. The walk here is a loop over the chain (collected down to a cached style, a missing one
//! or one already seen, then built from the innermost out) which inserts in the same order; the
//! chain is bounded by `DocxLimits::max_based_on_chain` (`DocxError::Limit`, kind `BasedOnChain`,
//! where the TS overflows its stack).
//!
//! `ParaProps`/`RunProps` of a style record every assignment with `mark`; the `{ ...base, ...own }`
//! spread keeps the base's order and appends the keys only `own` has, in its order.

use std::collections::HashSet;

use uo_jsnum::parse::parse_int;
use uo_opc::xml::{Document as XmlDocument, Element};

use crate::error::{DocxError, DocxLimitKind};
use crate::js::{JsMap, JsNumber, JsObject, Str};
use crate::limits::DocxLimits;
use crate::model::formatting::{ResolvedStyle, StylesMap};
use crate::model::para::{ParaProps, ParagraphBorders};
use crate::model::run::RunProps;

use super::ctx::ReadCtx;
use super::parts::part_doc;
use super::props_map::read_border_edge;

fn num(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

/// `onOff` of styles.ts (the element exists): `0`, `false` and `none` are false.
fn on_off(el: Element<'_>) -> bool {
    !matches!(el.attr("w:val"), Some("0" | "false" | "none"))
}

/// `o.field = Some(v)` and records the assignment in the key order.
macro_rules! put {
    ($p:ident, $f:ident, $k:literal, $v:expr) => {{
        let v = $v;
        $p.$f = Some(v);
        $p.mark($k);
    }};
}

/// `readStyleRunProps` (styles.ts:34): the run props a style sets (the few the style list uses).
#[must_use]
pub fn read_style_run_props(rpr: Option<Element<'_>>) -> RunProps {
    let mut p = RunProps::default();
    let Some(rpr) = rpr else { return p };
    for el in rpr.child_elements() {
        let val = el.attr("w:val");
        match el.name() {
            "w:b" => put!(p, bold, "bold", on_off(el)),
            "w:i" => put!(p, italic, "italic", on_off(el)),
            "w:caps" => put!(p, all_caps, "allCaps", on_off(el)),
            "w:smallCaps" => put!(p, small_caps, "smallCaps", on_off(el)),
            "w:strike" => put!(p, strike, "strike", on_off(el)),
            "w:sz" => {
                if let Some(v) = non_empty(val) {
                    put!(p, sz_half_pt, "szHalfPt", num(parse_int(v, 10)));
                }
            }
            "w:color" => {
                if let Some(v) = non_empty(val).filter(|v| *v != "auto") {
                    put!(p, color, "color", Str::from(v));
                }
            }
            "w:rFonts" => {
                if let Some(v) = non_empty(el.attr("w:ascii")) {
                    put!(p, font_ascii, "fontAscii", Str::from(v));
                }
            }
            "w:u" => put!(p, underline, "underline", val != Some("none")),
            _ => {}
        }
    }
    p
}

/// `readStyleParaProps` (styles.ts:52): the paragraph props a style sets.
#[must_use]
pub fn read_style_para_props(ppr: Option<Element<'_>>) -> ParaProps {
    let mut p = ParaProps::default();
    let Some(ppr) = ppr else { return p };
    if let Some(jc @ ("center" | "right" | "both" | "left")) = ppr.first_child("w:jc").and_then(|e| e.attr("w:val")) {
        put!(p, alignment, "alignment", Str::from(jc));
    }
    if let Some(spacing) = ppr.first_child("w:spacing") {
        if let Some(line) = non_empty(spacing.attr("w:line"))
            && spacing.attr("w:lineRule") == Some("auto")
        {
            put!(p, line_twentieths, "lineTwentieths", num(parse_int(line, 10)));
        }
        if let Some(v) = non_empty(spacing.attr("w:after")) {
            put!(p, space_after_twips, "spaceAfterTwips", num(parse_int(v, 10)));
        }
        if let Some(v) = non_empty(spacing.attr("w:before")) {
            put!(p, space_before_twips, "spaceBeforeTwips", num(parse_int(v, 10)));
        }
    }
    let ind = ppr.first_child("w:ind");
    if let Some(v) = non_empty(ind.and_then(|e| e.attr("w:left"))) {
        put!(p, indent_left_twips, "indentLeftTwips", num(parse_int(v, 10)));
    }
    if let Some(v) = non_empty(ind.and_then(|e| e.attr("w:right"))) {
        put!(p, indent_right_twips, "indentRightTwips", num(parse_int(v, 10)));
    }
    if ppr.first_child("w:pageBreakBefore").is_some() {
        put!(p, page_break_before, "pageBreakBefore", true);
    }
    if let Some(bdr) = ppr.first_child("w:pBdr") {
        let mut borders = ParagraphBorders::default();
        let mut any = false;
        let sides = [("w:top", "top"), ("w:left", "left"), ("w:bottom", "bottom"), ("w:right", "right")];
        for (name, key) in sides {
            let Some(edge) = read_border_edge(bdr.first_child(name)) else { continue };
            any = true;
            match key {
                "top" => borders.top = Some(edge),
                "left" => borders.left = Some(edge),
                "bottom" => borders.bottom = Some(edge),
                _ => borders.right = Some(edge),
            }
            borders.mark(key);
        }
        if any {
            put!(p, borders, "borders", Box::new(borders));
        }
    }
    if let Some(shd) = non_empty(ppr.first_child("w:shd").and_then(|e| e.attr("w:fill"))).filter(|s| *s != "auto") {
        put!(p, shading_hex, "shadingHex", Str::from(shd));
    }
    p
}

macro_rules! spread {
    ($out:ident, $over:ident, [$($f:ident),*]) => {
        $( if let Some(v) = &$over.$f { $out.$f = Some(v.clone()); } )*
        $over.for_each_in_order(&mut |k, _| $out.mark(k));
    };
}

/// `{ ...base, ...over }` of the paragraph props a style reader can set: the base's order, then
/// the keys only `over` has.
fn spread_para(base: &ParaProps, over: &ParaProps) -> ParaProps {
    let mut out = base.clone();
    spread!(
        out,
        over,
        [
            alignment,
            line_twentieths,
            space_after_twips,
            space_before_twips,
            indent_left_twips,
            indent_right_twips,
            page_break_before,
            borders,
            shading_hex
        ]
    );
    out
}

/// `{ ...base, ...over }` of the run props a style reader can set.
fn spread_run(base: &RunProps, over: &RunProps) -> RunProps {
    let mut out = base.clone();
    spread!(out, over, [bold, italic, all_caps, small_caps, strike, sz_half_pt, color, font_ascii, underline]);
    out
}

/// A style before its chain is resolved (`Raw` of `readStylesXml`).
struct Raw {
    based_on: Option<Str>,
    name: Option<Str>,
    character: bool,
    para: ParaProps,
    run: RunProps,
}

/// The base a chain is built on.
enum Base<'a> {
    Defaults,
    Cached(&'a str),
}

/// `readStylesXml` (styles.ts:81): every paragraph and character style resolved through its
/// `basedOn` chain over the document defaults, by styleId in the TS `Map` order, and the
/// reserved `docDefaults` entry last.
pub fn read_styles_xml(doc: Option<&XmlDocument>, limits: &DocxLimits) -> Result<StylesMap, DocxError> {
    let mut out = StylesMap::new();
    let Some(doc) = doc else { return Ok(out) };
    let root = doc.root();

    let defaults = root.first_child("w:docDefaults");
    let def_run = read_style_run_props(defaults.and_then(|d| d.first_child("w:rPrDefault").unwrap_or(d).first_child("w:rPr")));
    let def_para = read_style_para_props(defaults.and_then(|d| d.first_child("w:pPrDefault").unwrap_or(d).first_child("w:pPr")));

    let mut raw: JsMap<Str, Raw> = JsMap::new();
    for st in root.children_named("w:style") {
        let type_ = st.attr("w:type");
        if type_ != Some("paragraph") && type_ != Some("character") {
            continue;
        }
        let Some(id) = non_empty(st.attr("w:styleId")) else { continue };
        let val = |n: &str| st.first_child(n).and_then(|c| c.attr("w:val")).map(Str::from);
        raw.insert(
            id.into(),
            Raw {
                based_on: val("w:basedOn"),
                name: val("w:name"),
                character: type_ == Some("character"),
                para: read_style_para_props(st.first_child("w:pPr")),
                run: read_style_run_props(st.first_child("w:rPr")),
            },
        );
    }

    for id in raw.keys() {
        if out.contains_key(id) {
            continue;
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let mut chain: Vec<&str> = Vec::new();
        let mut cur: &str = id;
        let base = loop {
            if out.contains_key(cur) {
                break Base::Cached(cur);
            }
            let Some(r) = raw.get(cur) else { break Base::Defaults };
            if !seen.insert(cur) {
                break Base::Defaults;
            }
            chain.push(cur);
            match non_empty(r.based_on.as_deref()) {
                Some(b) => cur = b,
                None => break Base::Defaults,
            }
            if chain.len() > usize::try_from(limits.max_based_on_chain).unwrap_or(usize::MAX) {
                return Err(DocxError::Limit { kind: DocxLimitKind::BasedOnChain, max: u64::from(limits.max_based_on_chain), part: None });
            }
        };
        let (mut para, mut run) = match base {
            Base::Cached(c) => match out.get(c) {
                Some(b) => (b.para.clone(), b.run.clone()),
                None => (def_para.clone(), def_run.clone()),
            },
            Base::Defaults => (def_para.clone(), def_run.clone()),
        };
        for &sid in chain.iter().rev() {
            let Some(r) = raw.get(sid) else { continue };
            para = spread_para(&para, &r.para);
            run = spread_run(&run, &r.run);
            let mut resolved = ResolvedStyle {
                para: para.clone(),
                run: run.clone(),
                name: r.name.clone(),
                r#type: Some(if r.character { "character" } else { "paragraph" }.into()),
                ..ResolvedStyle::default()
            };
            if r.name.is_none() {
                resolved.mark_undefined("name");
            }
            out.insert(sid.into(), resolved);
        }
    }
    out.insert("docDefaults".into(), ResolvedStyle { para: def_para, run: def_run, ..ResolvedStyle::default() });
    Ok(out)
}

/// `readStylesXml(xml)` over a text (the empty text is no part).
pub fn read_styles_text(xml: &str, limits: &DocxLimits) -> Result<StylesMap, DocxError> {
    read_styles_xml(super::parts::parse_text(xml)?.as_ref(), limits)
}

/// `readStyles(pkg)` (styles.ts:77): the resolved styles of `word/styles.xml`.
pub fn read_styles(ctx: &ReadCtx<'_>) -> Result<StylesMap, DocxError> {
    read_styles_xml(part_doc(ctx, "word/styles.xml")?.as_ref(), &ctx.limits).map_err(|e| e.in_part("word/styles.xml"))
}
