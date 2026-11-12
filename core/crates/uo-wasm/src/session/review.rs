//! The answers of the wave-5 frontier R queries (the editor switch: review and document; record
//! `core/docs/modules/uo-bindings.md` "Wave 5 — R"): the tracking state and markup mode, the runs of a
//! paragraph (the shell's field/comment overlays, links, comments and the spelling spans), the style
//! list and the style look (`listStyles`/`styleLook` of styles.ts), the default paragraph style, the run
//! props at a position and the styles version. `query.rs` dispatches each `QueryOp` arm here.
//!
//! `lookOf` (styles.ts 252) is ported here over uo-docx's resolver (`StyleResolver`, `cascade_rpr`,
//! `merge_ppr`, `resolve_color`), as `shell_state.rs` ports its two-field half; the style chain's
//! formatting (with `withLatent`) comes from uo-docx `Session::style_look_formatting` (ED-301).

use uo_docx::js::JsNumber;
use uo_docx::model::formatting::{DocFormatting, Theme};
use uo_docx::model::para::Paragraph;
use uo_docx::model::props::{BorderSpec, RPr};
use uo_docx::model::run::Run;
use uo_docx::session::Session;
use uo_docx::session::ops::find::SpellLangs;
use uo_docx::session::ops::find::engine::FindEnv;
use uo_docx::session::text::{DocPosition, props_at};
use uo_docx::word::resolve::{ColorRef, FontSlot, StyleResolver, cascade_rpr, font_of_slot, merge_ppr, resolve_color};

use super::codec as w;

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn num(n: Option<JsNumber>) -> Option<f64> {
    n.and_then(JsNumber::as_f64)
}

/// `skipRun(r)` of spell.ts: fields, note marks, plugin inlines, objects, math, form fields, deletions.
fn skip_run(r: &Run) -> bool {
    if r.props.is_math == Some(true) || r.props.del.is_some() {
        return true;
    }
    r.ext().is_some_and(|e| {
        e.field.as_deref().is_some_and(|f| !f.is_empty())
            || e.field_result.is_some()
            || e.note_ref.is_some()
            || e.custom.is_some()
            || e.image.is_some()
            || e.textbox.is_some()
            || e.shape.is_some()
            || e.group.is_some()
            || e.math.is_some()
            || e.form_field.is_some()
    })
}

/// The `noProof` half of `LangResolver` (spell.ts 60): direct → character style → paragraph style →
/// the defaults; a style's value through its `basedOn` chain (a style already seen counts as none).
fn no_proof(fmt: &DocFormatting, run: &Run, para: &Paragraph) -> bool {
    let sheet = &fmt.styles;
    let style = |id: Option<&str>| -> Option<bool> {
        let mut seen: Vec<String> = Vec::new();
        let mut id = id.filter(|s| !s.is_empty()).map(str::to_owned);
        while let Some(i) = id {
            if seen.contains(&i) {
                return None;
            }
            let def = sheet.styles.get(&i)?;
            if let Some(v) = def.r_pr.as_deref().and_then(|r| r.no_proof) {
                return Some(v);
            }
            id = def.based_on.as_deref().filter(|s| !s.is_empty()).map(str::to_owned);
            seen.push(i);
        }
        None
    };
    let rpr = run.rpr.as_deref();
    if let Some(v) = rpr.and_then(|r| r.no_proof) {
        return v;
    }
    if let Some(v) = style(rpr.and_then(|r| r.r_style.as_deref())) {
        return v;
    }
    let pid = para
        .props
        .style_id
        .as_deref()
        .or_else(|| para.ppr.as_deref().and_then(|p| p.p_style.as_deref()))
        .or(sheet.defaults.paragraph.as_deref());
    if let Some(v) = style(pid) {
        return v;
    }
    sheet.doc_defaults.r_pr.as_deref().and_then(|r| r.no_proof).unwrap_or(false)
}

/// `ParaRuns`: the runs of paragraph `para` of the edit story.
pub fn para_runs(s: &Session, para: u32, fallback: &str) -> Vec<w::RunSpan> {
    let Some(p) = s.paras().and_then(|ps| usize::try_from(para).ok().and_then(|i| ps.get(i))) else { return Vec::new() };
    let fmt = &*s.document().formatting;
    let langs = SpellLangs { sheet: Some(&fmt.styles), fallback };
    p.runs
        .iter()
        .map(|r| {
            let ext = r.ext();
            let field = ext.is_some_and(|e| e.field.as_deref().is_some_and(|f| !f.is_empty()) || e.field_result.is_some());
            w::RunSpan {
                len: u32::try_from(r.text.len()).unwrap_or(u32::MAX),
                link: r.props.link.as_deref().map(utf16),
                link_tip: r.props.link_tip.as_deref().map(utf16),
                field,
                comment_ids: r.props.comment_ids.as_ref().map(|v| v.iter().map(|n| n.as_f64().unwrap_or(f64::NAN)).collect()),
                del: r.props.del.is_some(),
                spell: !skip_run(r) && !no_proof(fmt, r, p),
                lang: utf16(&langs.lang(r, p).unwrap_or_else(|| fallback.to_owned())),
                char_style: r.props.char_style.as_deref().map(utf16),
            }
        })
        .collect()
}

/// `ListStyles`: `listStyles(…, 'all')` with the use flags, before the shell's filter and sort.
pub fn list_styles(s: &Session) -> Vec<w::StyleListItem> {
    s.style_list_entries()
        .into_iter()
        .map(|e| w::StyleListItem {
            id: utf16(&e.id),
            name: utf16(&e.name),
            raw_name: utf16(&e.raw_name),
            kind: utf16(e.kind),
            built_in: e.built_in,
            defined: e.defined,
            quick: e.quick,
            hidden: e.hidden,
            priority: e.priority,
            in_use: e.in_use,
            is_default: e.is_default,
            based_on: e.based_on.as_deref().map(utf16),
            next: e.next.as_deref().map(utf16),
            link: e.link.as_deref().map(utf16),
            link_name: e.link_name.as_deref().map(utf16),
            auto_update: e.auto_update,
        })
        .collect()
}

/// `borderCss(b, theme)` of styles.ts.
fn border_css(b: Option<&BorderSpec>, theme: &Theme) -> Option<String> {
    let b = b?;
    if &*b.val == "nil" || &*b.val == "none" {
        return None;
    }
    let c = resolve_color(Some(ColorRef::of_border(b)), theme).unwrap_or_else(|| "000000".to_owned());
    let sz = num(b.sz).unwrap_or(4.0);
    // `Math.max(1, Math.round(sz / 8 * 4 / 3))`
    let px = uo_jsnum::math::max(1.0, uo_jsnum::math::round(((sz / 8.0) * 4.0) / 3.0));
    let style = match &*b.val {
        "double" => "double",
        "dotted" => "dotted",
        "dashed" => "dashed",
        _ => "solid",
    };
    Some(format!("{}px {style} #{c}", uo_jsnum::format::to_js_string(px)))
}

/// `styleLook(ds, theme, resolveStyleId(xml, id))` (styles.ts 285): `styleProps` then `lookOf`.
pub fn style_look(s: &Session, id: &str) -> w::StyleLookW {
    let (rid, fmt) = s.style_look_formatting(id);
    let mut res = StyleResolver::new();
    let dd = StyleResolver::doc_defaults(&fmt).clone();
    let character = fmt.styles.styles.get(&rid).is_some_and(|d| &*d.r#type == "character");
    let (p_pr, r_pr): (_, RPr) = if character {
        let para = res.para_style(&fmt, None);
        let ch = res.char_style(&fmt, Some(&rid));
        (merge_ppr(dd.p_pr.as_deref(), Some(&para.p_pr)), cascade_rpr(dd.r_pr.as_deref(), &[Some(&para.r_pr), Some(&ch)], None))
    } else {
        let ps = res.para_style(&fmt, Some(&rid));
        (merge_ppr(dd.p_pr.as_deref(), Some(&ps.p_pr)), cascade_rpr(dd.r_pr.as_deref(), &[Some(&ps.r_pr)], None))
    };
    let theme = &fmt.theme;
    let font_family = font_of_slot(r_pr.r_fonts.as_deref(), FontSlot::Ascii, theme)
        .or_else(|| font_of_slot(r_pr.r_fonts.as_deref(), FontSlot::HAnsi, theme))
        .or_else(|| font_of_slot(dd.r_pr.as_deref().and_then(|r| r.r_fonts.as_deref()), FontSlot::Ascii, theme))
        .or(theme.minor.latin.as_deref())
        .unwrap_or("Times New Roman")
        .to_owned();
    let jc = p_pr.jc.as_deref();
    let alignment = match jc {
        Some("center") => "center",
        Some("right" | "end") => "right",
        Some("both" | "distribute") => "both",
        _ => "left",
    };
    let sp = p_pr.spacing.as_deref();
    let ind = p_pr.ind.as_deref();
    let rule = sp.and_then(|x| x.line_rule.as_deref());
    let hanging = ind.and_then(|i| num(i.hanging));
    w::StyleLookW {
        font_family: utf16(&font_family),
        size_pt: num(r_pr.sz).unwrap_or(20.0) / 2.0,
        bold: r_pr.b == Some(true),
        italic: r_pr.i == Some(true),
        underline: r_pr.u.as_deref().is_some_and(|u| &*u.val != "none"),
        strike: r_pr.strike == Some(true),
        all_caps: r_pr.caps == Some(true),
        small_caps: r_pr.small_caps == Some(true),
        color: resolve_color(r_pr.color.as_deref().map(ColorRef::of_color), theme).as_deref().map(utf16),
        char_spacing_twips: num(r_pr.spacing).unwrap_or(0.0),
        alignment: utf16(alignment),
        space_before_twips: sp.and_then(|x| num(x.before)).unwrap_or(0.0),
        space_after_twips: sp.and_then(|x| num(x.after)).unwrap_or(0.0),
        line_twentieths: if matches!(rule, Some("exact" | "atLeast")) { None } else { Some(sp.and_then(|x| num(x.line)).unwrap_or(240.0)) },
        indent_left_twips: ind.and_then(|i| num(i.left)).unwrap_or(0.0),
        indent_right_twips: ind.and_then(|i| num(i.right)).unwrap_or(0.0),
        // `ind.hanging ? -ind.hanging : ind.firstLine ?? 0` (0 hanging is falsy)
        indent_first_line_twips: match hanging {
            Some(h) if h != 0.0 && !h.is_nan() => -h,
            _ => ind.and_then(|i| num(i.first_line)).unwrap_or(0.0),
        },
        shading_hex: p_pr.shd.as_deref().and_then(|s| s.fill.as_deref()).filter(|f| !f.is_empty() && *f != "auto").map(utf16),
        border_top: border_css(p_pr.p_bdr.as_deref().and_then(|b| b.top.as_deref()), theme).as_deref().map(utf16),
        border_bottom: border_css(p_pr.p_bdr.as_deref().and_then(|b| b.bottom.as_deref()), theme).as_deref().map(utf16),
    }
}

/// `RunPropsAt`: `propsAt(model, pos)` of the edit story (the default run props without one).
pub fn run_props_at(s: &Session, pos: DocPosition) -> uo_docx::model::run::RunProps {
    match s.paras() {
        Some(ps) => props_at(ps, pos).props,
        None => uo_docx::read::plain::default_run_props(),
    }
}

/// `ParaPropsAt`: the stored paragraph props of paragraph `para` of the edit story (the default props without one).
pub fn para_props_at(s: &Session, para: u32) -> uo_docx::model::para::ParaProps {
    s.paras().and_then(|ps| usize::try_from(para).ok().and_then(|i| ps.get(i))).map(|p| (*p.props).clone()).unwrap_or_default()
}
