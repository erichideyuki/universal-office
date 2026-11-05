//! Surgery on the raw `w:rPr` / `w:pPr` / `w:sectPr` (docx-core `lossless.ts`): the original
//! element with only the differences between the current editing props and the props the
//! element itself implies applied (the `directRPr`/`directPPr` rule of the layout), every other
//! child kept byte for byte in schema order; plus the generators of a fresh `w:rPr`/`w:pPr` from
//! props, `w:cols`, `w:pgNumType` and `w:lnNumType`.
//!
//! The generic raw helpers are uo-opc's (`split_element`, `set_kid`, `join_kids`, `with_attrs`);
//! parses are bounded by the package limits (uo-opc S1) and use the docx preset with positions
//! (the TS `parseXml(raw, { positions: true })`). `sameJson` follows the save's [`SigMode`]
//! (G23). Every value the TS prints with a template literal goes through `String(x)` rules
//! ([`super::js`]); the places the TS does not escape are kept unescaped (named in the comments).

use std::sync::Arc;

use uo_opc::Limits;
use uo_opc::xml::raw::{Kid, Split, join_kids, set_kid, split_element, with_attrs};

use super::ctx::{GenNumIds, WriteCtx};
use super::js::{dynv, esc_attr, num_str, or_num, rel_num, same_json, strict_ne, truthy, truthy_str};
use super::rev::RevKind;
use crate::error::DocxError;
use crate::js::{JsNumber, JsObject, Slot, Str};
use crate::model::para::{ParaProps, Paragraph};
use crate::model::props::{PPr, RPr};
use crate::model::run::{Run, RunProps, ThemeColorAttrs};
use crate::model::section::{Cols, LineNumbering, SectPr};
use crate::read::props::{read_ppr, read_rpr};
use crate::read::props_map::{edit_base_props, para_props_from_ppr, read_sect_pr};

/// Result of the writer.
pub type WResult<T> = Result<T, DocxError>;

/// CT_RPr.
pub const RPR_ORDER: &[&str] = &[
    "w:rStyle",
    "w:rFonts",
    "w:b",
    "w:bCs",
    "w:i",
    "w:iCs",
    "w:caps",
    "w:smallCaps",
    "w:strike",
    "w:dstrike",
    "w:outline",
    "w:shadow",
    "w:emboss",
    "w:imprint",
    "w:noProof",
    "w:snapToGrid",
    "w:vanish",
    "w:webHidden",
    "w:color",
    "w:spacing",
    "w:w",
    "w:kern",
    "w:position",
    "w:sz",
    "w:szCs",
    "w:highlight",
    "w:u",
    "w:effect",
    "w:bdr",
    "w:shd",
    "w:fitText",
    "w:vertAlign",
    "w:rtl",
    "w:cs",
    "w:em",
    "w:lang",
    "w:eastAsianLayout",
    "w:specVanish",
    "w:oMath",
    "w14:glow",
    "w14:shadow",
    "w14:reflection",
    "w14:textOutline",
    "w14:textFill",
    "w14:scene3d",
    "w14:props3d",
    "w14:ligatures",
    "w14:numForm",
    "w14:numSpacing",
    "w14:stylisticSets",
    "w14:cntxtAlts",
    "w:rPrChange",
];

/// CT_PPr.
pub const PPR_ORDER: &[&str] = &[
    "w:pStyle",
    "w:keepNext",
    "w:keepLines",
    "w:pageBreakBefore",
    "w:framePr",
    "w:widowControl",
    "w:numPr",
    "w:suppressLineNumbers",
    "w:pBdr",
    "w:shd",
    "w:tabs",
    "w:suppressAutoHyphens",
    "w:kinsoku",
    "w:wordWrap",
    "w:overflowPunct",
    "w:topLinePunct",
    "w:autoSpaceDE",
    "w:autoSpaceDN",
    "w:bidi",
    "w:adjustRightInd",
    "w:snapToGrid",
    "w:spacing",
    "w:ind",
    "w:contextualSpacing",
    "w:mirrorIndents",
    "w:suppressOverlap",
    "w:jc",
    "w:textDirection",
    "w:textAlignment",
    "w:textboxTightWrap",
    "w:outlineLvl",
    "w:divId",
    "w:cnfStyle",
    "w:rPr",
    "w:sectPr",
    "w:pPrChange",
];

/// CT_SectPr (header/footer references first).
pub const SECT_ORDER: &[&str] = &[
    "w:headerReference",
    "w:footerReference",
    "w:footnotePr",
    "w:endnotePr",
    "w:type",
    "w:pgSz",
    "w:pgMar",
    "w:paperSrc",
    "w:pgBorders",
    "w:lnNumType",
    "w:pgNumType",
    "w:cols",
    "w:formProt",
    "w:vAlign",
    "w:noEndnote",
    "w:titlePg",
    "w:textDirection",
    "w:bidi",
    "w:rtlGutter",
    "w:docGrid",
    "w:printerSettings",
    "w15:footnoteColumns",
    "w:sectPrChange",
];

/// The w14 children of `w:rPr` that text effects and typography write (`W14_RPR`, text-effects.ts).
pub const W14_RPR: &[&str] = &[
    "w14:glow",
    "w14:shadow",
    "w14:reflection",
    "w14:textOutline",
    "w14:textFill",
    "w14:scene3d",
    "w14:props3d",
    "w14:ligatures",
    "w14:numForm",
    "w14:numSpacing",
    "w14:stylisticSets",
    "w14:cntxtAlts",
];

/// `splitElement(raw)`: the docx preset with positions, within `limits`.
pub fn split(raw: &str, limits: &Limits) -> WResult<Split> {
    Ok(split_element(raw, &crate::read::DOCX_POS, limits)?)
}

/// The text a self-closing root becomes as an open tag: `.replace(/\s*\/>$/, '>')` (no
/// trailing white space allowed after `/>`, unlike `splitElement`).
#[must_use]
pub fn self_close_to_open_tail(open: &str) -> String {
    match open.strip_suffix("/>") {
        Some(head) => format!("{}>", uo_jsnum::text::trim_end(head)),
        None => open.to_owned(),
    }
}

/// `withAttrs(raw, name, changes)`: `None` values remove, others are set (escaped). The TS
/// crashes (`!` on a failed exec) when `raw` does not start with a tag: an error here.
pub fn attrs_with(raw: &str, name: &str, changes: &AttrChanges) -> WResult<String> {
    let pairs: Vec<(&str, Option<&str>)> = changes.0.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    with_attrs(raw, name, &pairs).ok_or(DocxError::Write("withAttrs on a text that is not a start tag"))
}

/// The `changes` record of `withAttrs`, in JS object order: a key set again keeps its place.
#[derive(Debug, Default, Clone)]
pub struct AttrChanges(Vec<(&'static str, Option<String>)>);

impl AttrChanges {
    #[must_use]
    pub fn new() -> AttrChanges {
        AttrChanges::default()
    }

    /// `ch[k] = v` (`None` = `undefined`).
    pub fn set(&mut self, k: &'static str, v: Option<String>) {
        if let Some(e) = self.0.iter_mut().find(|e| e.0 == k) {
            e.1 = v;
        } else {
            self.0.push((k, v));
        }
    }

    /// `ch[k] = n` for a number field (`undefined` removes, `null` prints `null`).
    pub fn num(&mut self, k: &'static str, v: Option<JsNumber>) {
        self.set(k, v.map(num_str));
    }
}

fn kid_raw<'k>(kids: &'k [Kid], name: &str) -> &'k str {
    kids.iter().find(|k| k.name == name).map_or("", |k| k.raw.as_str())
}

fn on_off(name: &str, v: bool) -> String {
    if v { format!("<{name}/>") } else { format!("<{name} w:val=\"0\"/>") }
}

/// `w:themeColor`/`w:themeTint`/`w:themeShade` attributes.
fn theme_attrs(t: Option<&ThemeColorAttrs>) -> String {
    let Some(t) = t else { return String::new() };
    let mut s = format!(" w:themeColor=\"{}\"", esc_attr(&t.theme_color));
    if let Some(v) = t.tint.as_deref().filter(|v| !v.is_empty()) {
        s.push_str(&format!(" w:themeTint=\"{}\"", esc_attr(v)));
    }
    if let Some(v) = t.shade.as_deref().filter(|v| !v.is_empty()) {
        s.push_str(&format!(" w:themeShade=\"{}\"", esc_attr(v)));
    }
    s
}

/// `colorXml(p)`.
fn color_xml(p: &RunProps) -> String {
    format!("<w:color w:val=\"{}\"{}/>", esc_attr(p.color.as_deref().unwrap_or("undefined")), theme_attrs(p.color_theme.as_ref()))
}

/// `uXml(p)` (`underlineStyle` unescaped, as the TS).
fn u_xml(p: &RunProps) -> String {
    let color = p.underline_color.as_deref().filter(|c| !c.is_empty()).map(|c| format!(" w:color=\"{}\"", esc_attr(c))).unwrap_or_default();
    format!("<w:u w:val=\"{}\"{color}{}/>", p.underline_style.as_deref().unwrap_or("single"), theme_attrs(p.underline_theme.as_ref()))
}

/// `wrapIn(tag, inner)`: empty → self-closing.
#[must_use]
pub fn wrap_in(tag: &str, inner: &str) -> String {
    if inner.is_empty() { format!("<{tag}/>") } else { format!("<{tag}>{inner}</{tag}>") }
}

/// `innerOf(xml)`: the content between the start and the end tag ("" for self-closing). Without
/// a `>` the TS slices from 0; without a `</`, to one unit before the end (`slice(…, -1)`).
#[must_use]
pub fn inner_of(xml: &str) -> &str {
    if xml.is_empty() {
        return "";
    }
    let a = xml.find('>');
    if let Some(a) = a
        && a > 0
        && xml.as_bytes()[a - 1] == b'/'
    {
        return "";
    }
    let start = a.map_or(0, |a| a + 1);
    let end = xml.rfind("</").unwrap_or_else(|| xml.char_indices().next_back().map_or(0, |(i, _)| i));
    if end <= start { "" } else { &xml[start..end] }
}

/// `rPrFromProps(p, linkAware)`: a `w:rPr` from props (a run created by the edit).
pub fn rpr_from_props(cx: &mut WriteCtx, p: &RunProps, link_aware: bool) -> WResult<String> {
    let mut rpr: Vec<String> = Vec::new();
    if let Some(s) = p.char_style.as_deref().filter(|s| !s.is_empty()) {
        rpr.push(format!("<w:rStyle w:val=\"{}\"/>", esc_attr(s)));
    }
    if let Some(f) = p.font_ascii.as_deref().filter(|s| !s.is_empty()) {
        let f = esc_attr(f);
        rpr.push(format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>"));
    }
    let flags: [(Option<bool>, &str); 9] = [
        (p.bold, "<w:b/>"),
        (p.italic, "<w:i/>"),
        (p.all_caps, "<w:caps/>"),
        (p.small_caps, "<w:smallCaps/>"),
        (p.strike, "<w:strike/>"),
        (p.double_strike, "<w:dstrike/>"),
        (p.no_proof, "<w:noProof/>"),
        (p.hidden, "<w:vanish/>"),
        (p.web_hidden, "<w:webHidden/>"),
    ];
    for (v, x) in flags {
        if v == Some(true) {
            rpr.push(x.to_owned());
        }
    }
    let link = truthy_str(p.link.as_deref());
    let link_look = link_aware && link && !truthy_str(p.underline_style.as_deref()) && !truthy_str(p.underline_color.as_deref());
    if truthy_str(p.color.as_deref()) && !(link_aware && link && p.color.as_deref() == Some("0563C1") && p.color_theme.is_none()) {
        rpr.push(color_xml(p));
    }
    if truthy(p.char_spacing_twips) {
        rpr.push(format!("<w:spacing w:val=\"{}\"/>", super::js::opt_num_str(p.char_spacing_twips)));
    }
    if let Some(s) = p.scale_pct
        && strict_ne(Some(s), Some(JsNumber::Num(100.0)))
    {
        rpr.push(format!("<w:w w:val=\"{}\"/>", num_str(s)));
    }
    if truthy(p.kern_half_pt) {
        rpr.push(format!("<w:kern w:val=\"{}\"/>", super::js::opt_num_str(p.kern_half_pt)));
    }
    if truthy(p.position_half_pt) {
        rpr.push(format!("<w:position w:val=\"{}\"/>", super::js::opt_num_str(p.position_half_pt)));
    }
    if let Some(sz) = p.sz_half_pt {
        let s = num_str(sz);
        rpr.push(format!("<w:sz w:val=\"{s}\"/><w:szCs w:val=\"{s}\"/>"));
    }
    if let Some(h) = p.highlight.as_deref().filter(|s| !s.is_empty()) {
        rpr.push(format!("<w:highlight w:val=\"{}\"/>", esc_attr(h)));
    }
    if p.underline == Some(true) && !link_look {
        rpr.push(u_xml(p));
    }
    if let Some(v) = p.vert_align.as_deref().filter(|s| !s.is_empty()) {
        rpr.push(format!("<w:vertAlign w:val=\"{v}\"/>"));
    }
    let fx = super::emit::fx::text_effects_xml(p.text_fx.as_deref(), p.open_type.as_deref())?;
    if !fx.is_empty() {
        rpr.push(fx);
    }
    if let Some(fmt) = p.fmt.as_deref() {
        let attrs = cx.rev.attrs_of(&fmt.author, fmt.date.as_deref(), fmt.date_utc.as_deref());
        let old = rpr_from_props(cx, &fmt.old, link_aware)?;
        rpr.push(format!("<w:rPrChange{attrs}>{}</w:rPrChange>", wrap_in("w:rPr", inner_of(&old))));
    }
    Ok(if rpr.is_empty() { String::new() } else { format!("<w:rPr>{}</w:rPr>", rpr.concat()) })
}

/// Element of `w:rPr` → field of the typed `RPr` (`RPR_FIELD`).
const RPR_FIELD: &[(&str, &str)] = &[
    ("w:rStyle", "rStyle"),
    ("w:rFonts", "rFonts"),
    ("w:b", "b"),
    ("w:bCs", "bCs"),
    ("w:i", "i"),
    ("w:iCs", "iCs"),
    ("w:caps", "caps"),
    ("w:smallCaps", "smallCaps"),
    ("w:strike", "strike"),
    ("w:dstrike", "dstrike"),
    ("w:outline", "outline"),
    ("w:shadow", "shadow"),
    ("w:emboss", "emboss"),
    ("w:imprint", "imprint"),
    ("w:vanish", "vanish"),
    ("w:color", "color"),
    ("w:spacing", "spacing"),
    ("w:w", "w"),
    ("w:kern", "kern"),
    ("w:position", "position"),
    ("w:sz", "sz"),
    ("w:szCs", "szCs"),
    ("w:highlight", "highlight"),
    ("w:u", "u"),
    ("w:effect", "effect"),
    ("w:bdr", "bdr"),
    ("w:shd", "shd"),
    ("w:vertAlign", "vertAlign"),
    ("w:em", "em"),
    ("w14:glow", "fx"),
    ("w14:shadow", "fx"),
    ("w14:reflection", "fx"),
    ("w14:textOutline", "fx"),
    ("w14:textFill", "fx"),
    ("w14:scene3d", "fx"),
    ("w14:props3d", "fx"),
    ("w14:ligatures", "ot"),
    ("w14:numForm", "ot"),
    ("w14:numSpacing", "ot"),
    ("w14:stylisticSets", "ot"),
    ("w14:cntxtAlts", "ot"),
];

/// Element of `w:pPr` → field of the typed `PPr` (`PPR_FIELD`).
const PPR_FIELD: &[(&str, &str)] = &[
    ("w:keepNext", "keepNext"),
    ("w:keepLines", "keepLines"),
    ("w:pageBreakBefore", "pageBreakBefore"),
    ("w:framePr", "framePr"),
    ("w:widowControl", "widowControl"),
    ("w:numPr", "numPr"),
    ("w:suppressLineNumbers", "suppressLineNumbers"),
    ("w:pBdr", "pBdr"),
    ("w:shd", "shd"),
    ("w:tabs", "tabs"),
    ("w:suppressAutoHyphens", "suppressAutoHyphens"),
    ("w:kinsoku", "kinsoku"),
    ("w:wordWrap", "wordWrap"),
    ("w:overflowPunct", "overflowPunct"),
    ("w:autoSpaceDE", "autoSpaceDE"),
    ("w:autoSpaceDN", "autoSpaceDN"),
    ("w:bidi", "bidi"),
    ("w:adjustRightInd", "adjustRightInd"),
    ("w:snapToGrid", "snapToGrid"),
    ("w:spacing", "spacing"),
    ("w:ind", "ind"),
    ("w:contextualSpacing", "contextualSpacing"),
    ("w:mirrorIndents", "mirrorIndents"),
    ("w:suppressOverlap", "suppressOverlap"),
    ("w:jc", "jc"),
    ("w:textAlignment", "textAlignment"),
    ("w:textboxTightWrap", "textboxTightWrap"),
    ("w:outlineLvl", "outlineLvl"),
    ("w:rPr", "rPr"),
];

/// `PROP_RPR`: editing prop → the `RPr` fields it comes from.
const PROP_RPR: &[(&str, &[&str])] = &[
    ("bold", &["b", "bCs"]),
    ("italic", &["i", "iCs"]),
    ("underline", &["u"]),
    ("underlineStyle", &["u"]),
    ("underlineColor", &["u"]),
    ("underlineTheme", &["u"]),
    ("strike", &["strike"]),
    ("doubleStrike", &["dstrike"]),
    ("allCaps", &["caps"]),
    ("smallCaps", &["smallCaps"]),
    ("vertAlign", &["vertAlign"]),
    ("szHalfPt", &["sz", "szCs"]),
    ("color", &["color"]),
    ("colorTheme", &["color"]),
    ("highlight", &["highlight"]),
    ("fontAscii", &["rFonts"]),
    ("charSpacingTwips", &["spacing"]),
    ("hidden", &["vanish"]),
    ("scalePct", &["w"]),
    ("positionHalfPt", &["position"]),
    ("kernHalfPt", &["kern"]),
    ("textFx", &["fx", "textFill", "textOutline", "textShadow"]),
    ("openType", &["ot", "ligatures"]),
];

/// Whether `o[field] === undefined` (absent or an explicit `undefined`).
fn field_undefined<T: JsObject>(o: &T, field: &str) -> bool {
    match T::key_index(field) {
        Some(i) => !matches!(o.slot(usize::from(i)), Slot::Val(_)),
        None => true,
    }
}

/// `clearedRPr(rpr, fields)`: the rpr without the direct formatting (the editor's "clear
/// formatting"), `rStyle` kept; with `fields`, only those props' elements.
#[must_use]
pub fn cleared_rpr(rpr: Option<&RPr>, fields: Option<&[&str]>) -> RPr {
    let mut out = rpr.cloned().unwrap_or_default();
    let drop: Vec<&str> = match fields {
        Some(fs) => fs.iter().flat_map(|k| PROP_RPR.iter().find(|(p, _)| p == k).map_or(&[][..], |(_, v)| *v)).copied().collect(),
        None => RPR_FIELD.iter().map(|(_, f)| *f).filter(|f| *f != "rStyle").collect(),
    };
    for f in &drop {
        crate::word::obj::delete(&mut out, f);
    }
    if field_undefined(&out, "fx") && !has_own(&out, "fx") {
        for f in ["textFill", "textOutline", "textShadow"] {
            crate::word::obj::delete(&mut out, f);
        }
    }
    if !has_own(&out, "ot") {
        crate::word::obj::delete(&mut out, "ligatures");
    }
    out
}

/// `'k' in o` (an own key, `undefined` included).
fn has_own<T: JsObject>(o: &T, field: &str) -> bool {
    T::key_index(field).is_some_and(|i| !matches!(o.slot(usize::from(i)), Slot::Absent))
}

/// `clearedPPr(ppr)`: the ppr without the direct paragraph formatting (`pStyle`, the mark and
/// the revision stay).
#[must_use]
pub fn cleared_ppr(ppr: Option<&PPr>) -> PPr {
    let mut out = ppr.cloned().unwrap_or_default();
    for (_, f) in PPR_FIELD {
        if *f != "rPr" {
            crate::word::obj::delete(&mut out, f);
        }
    }
    out
}

/// `dropCleared(kids, map, cur)`: drops the kids whose typed field is gone from `cur`.
fn drop_cleared<T: JsObject>(kids: &mut Vec<Kid>, map: &[(&str, &str)], cur: &T) -> bool {
    let before = kids.len();
    kids.retain(|k| !map.iter().find(|(n, _)| *n == k.name).is_some_and(|(_, f)| field_undefined(cur, f)));
    kids.len() != before
}

/// The `w:rPr` a raw `w:rPrChange` keeps (the one before), raw (`changeRPrRaw`).
fn change_rpr_raw(kids: &[Kid], limits: &Limits) -> WResult<Option<String>> {
    let Some(k) = kids.iter().find(|x| x.name == "w:rPrChange") else { return Ok(None) };
    let inner = split(&k.raw, limits)?;
    Ok(Some(inner.kids.iter().find(|x| x.name == "w:rPr").map_or_else(|| "<w:rPr/>".to_owned(), |x| x.raw.clone())))
}

/// The root element of a split as an `Element`.
fn root_of(s: &Split) -> uo_opc::xml::Element<'_> {
    s.doc.root()
}

/// `patchRPr(raw, run)`: the run's raw `w:rPr` with the differences of its props applied.
pub fn patch_rpr(cx: &mut WriteCtx, raw: Option<&str>, run: &Run) -> WResult<String> {
    let p = &*run.props;
    let Some(raw0) = raw else { return rpr_from_props(cx, p, true) };
    let limits = cx.limits.clone();
    let mode = cx.mode;
    let mut raw: String = raw0.to_owned();
    // the TS recursion of a rejected tracked change (`return patchRPr(old.raw, run)`) as a loop
    let (sp, edited, base) = loop {
        let sp = split(&raw, &limits)?;
        let edited = if run.rpr_edited == Some(true) { run.rpr.as_deref() } else { None };
        let read = read_rpr(Some(root_of(&sp)));
        let base = edit_base_props(run, edited.or(read.as_ref()));
        if base.fmt.is_some()
            && p.fmt.is_none()
            && let Some(old_raw) = change_rpr_raw(&sp.kids, &limits)?
        {
            let old = split(&old_raw, &limits)?;
            let old_base = edit_base_props(run, read_rpr(Some(root_of(&old))).as_ref());
            let a = run_formatting(p);
            let b = run_formatting(&old_base);
            if same_json(Some(&a), Some(&b), mode) {
                raw = old_raw;
                continue;
            }
        }
        break (sp, edited.cloned(), base);
    };
    let Split { open, close, mut kids, .. } = sp;
    let mut changed = match edited.as_ref() {
        Some(e) => drop_cleared(&mut kids, RPR_FIELD, e),
        None => false,
    };
    let mut set = |kids: &mut Vec<Kid>, name: &str, xml: Option<String>| {
        changed = true;
        set_kid(kids, RPR_ORDER, name, xml.as_deref());
    };
    if p.bold != base.bold {
        let b = p.bold == Some(true);
        set(&mut kids, "w:b", Some(on_off("w:b", b)));
        set(&mut kids, "w:bCs", Some(on_off("w:bCs", b)));
    }
    if p.italic != base.italic {
        let b = p.italic == Some(true);
        set(&mut kids, "w:i", Some(on_off("w:i", b)));
        set(&mut kids, "w:iCs", Some(on_off("w:iCs", b)));
    }
    if p.underline != base.underline
        || p.underline_style != base.underline_style
        || p.underline_color != base.underline_color
        || !same_json(dynv(p.underline_theme.as_ref()), dynv(base.underline_theme.as_ref()), mode)
    {
        set(&mut kids, "w:u", Some(if p.underline == Some(true) { u_xml(p) } else { "<w:u w:val=\"none\"/>".to_owned() }));
    }
    if (p.hidden == Some(true)) != (base.hidden == Some(true)) {
        set(&mut kids, "w:vanish", (p.hidden == Some(true)).then(|| "<w:vanish/>".to_owned()));
    }
    if or_num(p.scale_pct, 100.0) != or_num(base.scale_pct, 100.0) {
        let x = match p.scale_pct {
            Some(s) if strict_ne(Some(s), Some(JsNumber::Num(100.0))) => Some(format!("<w:w w:val=\"{}\"/>", num_str(s))),
            _ => None,
        };
        set(&mut kids, "w:w", x);
    }
    if or_num(p.kern_half_pt, 0.0) != or_num(base.kern_half_pt, 0.0) {
        let x = truthy(p.kern_half_pt).then(|| format!("<w:kern w:val=\"{}\"/>", super::js::opt_num_str(p.kern_half_pt)));
        set(&mut kids, "w:kern", x);
    }
    if or_num(p.position_half_pt, 0.0) != or_num(base.position_half_pt, 0.0) {
        let x = truthy(p.position_half_pt).then(|| format!("<w:position w:val=\"{}\"/>", super::js::opt_num_str(p.position_half_pt)));
        set(&mut kids, "w:position", x);
    }
    if p.strike != base.strike {
        set(&mut kids, "w:strike", Some(on_off("w:strike", p.strike == Some(true))));
    }
    if (p.double_strike == Some(true)) != (base.double_strike == Some(true)) {
        set(&mut kids, "w:dstrike", Some(on_off("w:dstrike", p.double_strike == Some(true))));
    }
    if p.all_caps != base.all_caps {
        set(&mut kids, "w:caps", Some(on_off("w:caps", p.all_caps == Some(true))));
    }
    if p.small_caps != base.small_caps {
        set(&mut kids, "w:smallCaps", Some(on_off("w:smallCaps", p.small_caps == Some(true))));
    }
    if p.vert_align != base.vert_align {
        // unescaped, as the TS
        set(&mut kids, "w:vertAlign", Some(format!("<w:vertAlign w:val=\"{}\"/>", p.vert_align.as_deref().unwrap_or("baseline"))));
    }
    if strict_ne(p.sz_half_pt, base.sz_half_pt) {
        let sz = p.sz_half_pt.map(num_str);
        set(&mut kids, "w:sz", sz.as_ref().map(|s| format!("<w:sz w:val=\"{s}\"/>")));
        set(&mut kids, "w:szCs", sz.as_ref().map(|s| format!("<w:szCs w:val=\"{s}\"/>")));
    }
    if p.color != base.color || !same_json(dynv(p.color_theme.as_ref()), dynv(base.color_theme.as_ref()), mode) {
        set(&mut kids, "w:color", p.color.is_some().then(|| color_xml(p)));
    }
    if p.highlight != base.highlight {
        set(&mut kids, "w:highlight", p.highlight.as_deref().map(|h| format!("<w:highlight w:val=\"{}\"/>", esc_attr(h))));
    }
    if p.font_ascii != base.font_ascii {
        let x = match p.font_ascii.as_deref() {
            None => None,
            Some(f) => {
                let mut ch = AttrChanges::new();
                for k in ["w:ascii", "w:hAnsi", "w:cs"] {
                    ch.set(k, Some(f.to_owned()));
                }
                for k in ["w:asciiTheme", "w:hAnsiTheme", "w:cstheme"] {
                    ch.set(k, None);
                }
                Some(attrs_with(kid_raw(&kids, "w:rFonts"), "w:rFonts", &ch)?)
            }
        };
        set(&mut kids, "w:rFonts", x);
    }
    if p.char_style != base.char_style {
        let x = p.char_style.as_deref().filter(|s| !s.is_empty()).map(|s| format!("<w:rStyle w:val=\"{}\"/>", esc_attr(s)));
        set(&mut kids, "w:rStyle", x);
    }
    if or_num(p.char_spacing_twips, 0.0) != or_num(base.char_spacing_twips, 0.0) {
        let x = truthy(p.char_spacing_twips).then(|| format!("<w:spacing w:val=\"{}\"/>", super::js::opt_num_str(p.char_spacing_twips)));
        set(&mut kids, "w:spacing", x);
    }
    if !same_json(dynv(p.text_fx.as_deref()), dynv(base.text_fx.as_deref()), mode)
        || !same_json(dynv(p.open_type.as_deref()), dynv(base.open_type.as_deref()), mode)
    {
        let fx = super::emit::fx::text_effects_xml(p.text_fx.as_deref(), p.open_type.as_deref())?;
        let now = split(&format!("<w:rPr>{fx}</w:rPr>"), &limits)?;
        for n in W14_RPR {
            let x = now.kids.iter().find(|k| k.name == *n).map(|k| k.raw.clone());
            set(&mut kids, n, x);
        }
    }
    if !same_json(dynv(p.fmt.as_deref()), dynv(base.fmt.as_deref()), mode) {
        match p.fmt.as_deref() {
            None => set(&mut kids, "w:rPrChange", None),
            Some(fmt) => {
                let rest: Vec<Kid> = kids.iter().filter(|k| k.name != "w:rPrChange").cloned().collect();
                let before = if rest.is_empty() {
                    rpr_from_props(cx, &fmt.old, true)?
                } else {
                    let mut old_props = (*fmt.old).clone();
                    if let Some(l) = p.link.as_deref().filter(|l| !l.is_empty()) {
                        crate::word::obj::set(&mut old_props, "link", |o| o.link = Some(Str::from(l)));
                    }
                    let mut old_run = run.clone();
                    old_run.props = Arc::new(old_props);
                    patch_rpr(cx, Some(&join_kids(&open, &rest, &close)), &old_run)?
                };
                let attrs = cx.rev.attrs_of(&fmt.author, fmt.date.as_deref(), fmt.date_utc.as_deref());
                set(&mut kids, "w:rPrChange", Some(format!("<w:rPrChange{attrs}>{}</w:rPrChange>", wrap_in("w:rPr", inner_of(&before)))));
            }
        }
    }
    Ok(if changed { join_kids(&open, &kids, &close) } else { raw })
}

/// `runFormatting(p)` (revisions.ts:44): `{ ...p }` without `RUN_META`.
fn run_formatting(p: &RunProps) -> RunProps {
    let mut out = p.clone();
    for k in ["src", "link", "commentIds", "isMath", "ins", "del", "fmt"] {
        crate::word::obj::delete(&mut out, k);
    }
    out
}

/// `paraFormatting(p)` (revisions.ts:51): `{ ...p }` without `PARA_META`.
fn para_formatting(p: &ParaProps) -> ParaProps {
    let mut out = p.clone();
    for k in ["src", "fmtSrc", "sectionBreak", "markIns", "markDel", "pfmt", "tocInstr", "tocEntry", "blockSdt"] {
        crate::word::obj::delete(&mut out, k);
    }
    out
}

/// `FLOW_FLAGS(pp)`.
fn flow_flags(pp: &ParaProps) -> [(&'static str, Option<bool>); 6] {
    [
        ("w:keepNext", pp.keep_next),
        ("w:keepLines", pp.keep_lines),
        ("w:pageBreakBefore", pp.page_break_before),
        ("w:widowControl", pp.widow_control),
        ("w:suppressLineNumbers", pp.suppress_line_numbers),
        ("w:suppressAutoHyphens", pp.suppress_auto_hyphens),
    ]
}

/// The parts of a `w:pPr` the editing props control (`pprBits`).
struct PprBits {
    p_style: Option<String>,
    flags: Vec<(&'static str, Option<String>)>,
    textbox_tight_wrap: Option<String>,
    tabs: Option<String>,
    num_pr: Option<String>,
    p_bdr: Option<String>,
    shd: Option<String>,
}

impl PprBits {
    fn flag(&self, name: &str) -> Option<String> {
        self.flags.iter().find(|(n, _)| *n == name).and_then(|(_, x)| x.clone())
    }
}

fn border_side(side: &str, e: &crate::model::para::BorderEdge) -> String {
    let space = if side == "bar" { String::new() } else { format!(" w:space=\"{}\"", num_str(e.space_pt)) };
    let color = e.color.as_deref().filter(|c| !c.is_empty()).map_or_else(|| "auto".to_owned(), esc_attr);
    format!("<w:{side} w:val=\"{}\" w:sz=\"{}\"{space} w:color=\"{color}\"/>", esc_attr(&e.style), num_str(e.size_eighth_pt))
}

fn ppr_bits(pp: &ParaProps, ids: GenNumIds) -> PprBits {
    let p_style = pp.style_id.as_deref().filter(|s| !s.is_empty()).map(|s| format!("<w:pStyle w:val=\"{}\"/>", esc_attr(s)));
    let flags = flow_flags(pp)
        .into_iter()
        .map(|(name, v)| (name, v.map(|v| if v { format!("<{name}/>") } else { format!("<{name} w:val=\"0\"/>") })))
        .collect();
    let textbox_tight_wrap =
        pp.textbox_tight_wrap.as_deref().filter(|s| !s.is_empty()).map(|v| format!("<w:textboxTightWrap w:val=\"{v}\"/>"));
    let tabs = pp.tab_stops.as_ref().filter(|t| !t.is_empty()).map(|ts| {
        let mut s = String::from("<w:tabs>");
        for t in ts {
            let leader = t.leader.as_deref().filter(|l| !l.is_empty()).map(|l| format!(" w:leader=\"{l}\"")).unwrap_or_default();
            s.push_str(&format!("<w:tab w:val=\"{}\"{leader} w:pos=\"{}\"/>", t.align, num_str(t.pos_twips)));
        }
        s.push_str("</w:tabs>");
        s
    });
    let num_pr = pp.numbering.as_ref().map(|n| {
        // `numId ?? (…)`: `null` is nullish too
        let num_id = match n.num_id {
            Some(JsNumber::Num(id)) => uo_jsnum::format::to_js_string(id),
            _ => uo_jsnum::format::to_js_string(if n.kind.as_ref() == "decimal" { ids.decimal } else { ids.bullet }),
        };
        format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>", num_str(n.ilvl))
    });
    let p_bdr = if let Some(b) = pp.borders.as_deref() {
        let mut inner = String::new();
        for (side, e) in
            [("top", &b.top), ("left", &b.left), ("bottom", &b.bottom), ("right", &b.right), ("between", &b.between), ("bar", &b.bar)]
        {
            if let Some(e) = e {
                inner.push_str(&border_side(side, e));
            }
        }
        (!inner.is_empty()).then(|| format!("<w:pBdr>{inner}</w:pBdr>"))
    } else if pp.box_border == Some(true) {
        let b = |side: &str| format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"auto\"/>");
        Some(format!("<w:pBdr>{}{}{}{}</w:pBdr>", b("top"), b("left"), b("bottom"), b("right")))
    } else {
        None
    };
    let shd = pp
        .shading_hex
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|h| format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", esc_attr(h)));
    PprBits { p_style, flags, textbox_tight_wrap, tabs, num_pr, p_bdr, shd }
}

/// A generated `w:pPr` and whether a `w:numPr` points at the new lists (`PPrOut`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PPrOut {
    pub xml: String,
    pub gen_numbering: bool,
}

fn spacing_xml(pp: &ParaProps) -> Option<String> {
    if pp.line_twentieths.is_none()
        && pp.space_after_twips.is_none()
        && pp.space_before_twips.is_none()
        && pp.space_before_auto.is_none()
        && pp.space_after_auto.is_none()
    {
        return None;
    }
    let line = pp.line_twentieths.map(|l| format!(" w:line=\"{}\" w:lineRule=\"auto\"", num_str(l))).unwrap_or_default();
    let before = pp.space_before_twips.map(|v| format!(" w:before=\"{}\"", num_str(v))).unwrap_or_default()
        + &pp.space_before_auto.map(|v| format!(" w:beforeAutospacing=\"{}\"", u8::from(v))).unwrap_or_default();
    let after = pp.space_after_twips.map(|v| format!(" w:after=\"{}\"", num_str(v))).unwrap_or_default()
        + &pp.space_after_auto.map(|v| format!(" w:afterAutospacing=\"{}\"", u8::from(v))).unwrap_or_default();
    Some(format!("<w:spacing{before}{after}{line}/>"))
}

fn ind_xml(pp: &ParaProps) -> Option<String> {
    if pp.indent_left_twips.is_none() && pp.indent_right_twips.is_none() && pp.indent_first_line_twips.is_none() {
        return None;
    }
    let mut attrs = String::new();
    if let Some(v) = pp.indent_left_twips {
        attrs.push_str(&format!(" w:left=\"{}\"", num_str(v)));
    }
    if let Some(v) = pp.indent_right_twips {
        attrs.push_str(&format!(" w:right=\"{}\"", num_str(v)));
    }
    if let Some(f) = pp.indent_first_line_twips {
        if f.to_number() >= 0.0 {
            attrs.push_str(&format!(" w:firstLine=\"{}\"", num_str(f)));
        } else {
            attrs.push_str(&format!(" w:hanging=\"{}\"", uo_jsnum::format::to_js_string(-f.to_number())));
        }
    }
    Some(format!("<w:ind{attrs}/>"))
}

/// `markXml(pp)`: the revisions of the paragraph mark.
fn mark_xml(cx: &mut WriteCtx, pp: &ParaProps) -> String {
    let mut s = String::new();
    if let Some(r) = pp.mark_ins.as_deref() {
        s.push_str(&cx.rev.mark_xml(RevKind::Ins, r));
    }
    if let Some(r) = pp.mark_del.as_deref() {
        s.push_str(&cx.rev.mark_xml(RevKind::Del, r));
    }
    s
}

/// `pPrChangeXml(pp, oldPPr)`.
fn ppr_change_xml(cx: &mut WriteCtx, pp: &ParaProps, old_ppr: &str) -> String {
    let attrs = match pp.pfmt.as_deref() {
        Some(f) => cx.rev.attrs_of(&f.author, f.date.as_deref(), f.date_utc.as_deref()),
        None => cx.rev.attrs_of("undefined", None, None),
    };
    format!("<w:pPrChange{attrs}>{}</w:pPrChange>", wrap_in("w:pPr", inner_of(old_ppr)))
}

/// `pPrFromProps(pp, sectPrXml, ids)`: a `w:pPr` from props (a paragraph created by the edit).
pub fn ppr_from_props(cx: &mut WriteCtx, pp: &ParaProps, sect_pr_xml: &str, ids: GenNumIds) -> WResult<PPrOut> {
    let bits = ppr_bits(pp, ids);
    let mut ppr: Vec<String> = Vec::new();
    let mut add = |x: Option<String>| {
        if let Some(x) = x.filter(|x| !x.is_empty()) {
            ppr.push(x);
        }
    };
    add(bits.p_style.clone());
    add(bits.flag("w:keepNext"));
    add(bits.flag("w:keepLines"));
    add(bits.flag("w:pageBreakBefore"));
    add(bits.flag("w:widowControl"));
    add(bits.num_pr.clone());
    add(bits.flag("w:suppressLineNumbers"));
    add(bits.p_bdr.clone());
    add(bits.shd.clone());
    add(bits.tabs.clone());
    add(bits.flag("w:suppressAutoHyphens"));
    add(spacing_xml(pp));
    add(ind_xml(pp));
    if let Some(a) = pp.alignment.as_deref().filter(|a| !a.is_empty() && *a != "left") {
        add(Some(format!("<w:jc w:val=\"{a}\"/>")));
    }
    add(bits.textbox_tight_wrap.clone());
    if let Some(l) = pp.outline_lvl {
        add(Some(format!("<w:outlineLvl w:val=\"{}\"/>", num_str(l))));
    }
    let mark = mark_xml(cx, pp);
    if !mark.is_empty() {
        ppr.push(format!("<w:rPr>{mark}</w:rPr>"));
    }
    if !sect_pr_xml.is_empty() {
        ppr.push(sect_pr_xml.to_owned());
    }
    if let Some(pf) = pp.pfmt.as_deref() {
        let old = ppr_from_props(cx, &pf.old, "", ids)?;
        ppr.push(ppr_change_xml(cx, pp, &old.xml));
    }
    let generated = pp.numbering.as_ref().is_some_and(|n| n.num_id.is_none())
        || pp.pfmt.as_deref().and_then(|f| f.old.numbering.as_ref()).is_some_and(|n| n.num_id.is_none());
    Ok(PPrOut { xml: if ppr.is_empty() { String::new() } else { format!("<w:pPr>{}</w:pPr>", ppr.concat()) }, gen_numbering: generated })
}

/// `markRPr(raw, pp)`: the mark's `w:rPr` with its revisions replaced (`None` = gone).
fn mark_rpr(cx: &mut WriteCtx, raw: &str, pp: &ParaProps) -> WResult<Option<String>> {
    let mark = mark_xml(cx, pp);
    if raw.is_empty() {
        return Ok((!mark.is_empty()).then(|| format!("<w:rPr>{mark}</w:rPr>")));
    }
    let sp = split(raw, &cx.limits)?;
    let rest: String = sp
        .kids
        .iter()
        .filter(|k| !matches!(k.name.as_str(), "w:ins" | "w:del" | "w:moveFrom" | "w:moveTo"))
        .map(|k| k.raw.as_str())
        .collect();
    Ok((!mark.is_empty() || !rest.is_empty()).then(|| format!("{}{mark}{rest}{}", sp.open, sp.close)))
}

/// The `w:sectPr` argument of [`patch_ppr`]: keep the raw one, remove it, or this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectArg {
    /// `undefined`: keep the raw one as it is
    Keep,
    /// `null`: none
    Remove,
    Set(String),
}

impl SectArg {
    fn as_xml(&self) -> &str {
        match self {
            SectArg::Set(s) => s,
            _ => "",
        }
    }
}

/// `patchPPr(raw, para, sect, ids)`: the paragraph's raw `w:pPr` with the differences applied.
pub fn patch_ppr(cx: &mut WriteCtx, raw: Option<&str>, para: &Paragraph, sect: &SectArg, ids: GenNumIds) -> WResult<PPrOut> {
    patch_ppr_props(cx, raw, &para.props, if para.ppr_edited == Some(true) { para.ppr.as_deref() } else { None }, sect, ids)
}

fn patch_ppr_props(
    cx: &mut WriteCtx,
    raw: Option<&str>,
    p: &ParaProps,
    edited: Option<&PPr>,
    sect: &SectArg,
    ids: GenNumIds,
) -> WResult<PPrOut> {
    let Some(raw0) = raw else { return ppr_from_props(cx, p, sect.as_xml(), ids) };
    let limits = cx.limits.clone();
    let mode = cx.mode;
    let mut raw = raw0.to_owned();
    let (sp, base) = loop {
        let sp = split(&raw, &limits)?;
        let read = read_ppr(Some(root_of(&sp)));
        let base = para_props_from_ppr(edited.or(read.as_ref()));
        if base.pfmt.is_some()
            && p.pfmt.is_none()
            && let Some(ch) = sp.kids.iter().find(|k| k.name == "w:pPrChange")
        {
            let chs = split(&ch.raw, &limits)?;
            let old = chs.kids.iter().find(|k| k.name == "w:pPr");
            let old_props = match old {
                Some(o) => {
                    let os = split(&o.raw, &limits)?;
                    para_props_from_ppr(read_ppr(Some(root_of(&os))).as_ref())
                }
                // `{ name: 'w:pPr', attrs: {}, children: [] }`
                None => {
                    let os = split("<w:pPr/>", &limits)?;
                    para_props_from_ppr(read_ppr(Some(root_of(&os))).as_ref())
                }
            };
            let a = para_formatting(p);
            let b = para_formatting(&old_props);
            if same_json(Some(&a), Some(&b), mode) {
                let old_kids = match old {
                    Some(o) => split(&o.raw, &limits)?.kids,
                    None => Vec::new(),
                };
                let keep = sp.kids.iter().filter(|k| k.name == "w:rPr" || k.name == "w:sectPr");
                let all: Vec<&Kid> = old_kids.iter().chain(keep).collect();
                if all.is_empty() {
                    return ppr_from_props(cx, p, sect.as_xml(), ids);
                }
                raw = format!("<w:pPr>{}</w:pPr>", all.iter().map(|k| k.raw.as_str()).collect::<String>());
                continue;
            }
        }
        break (sp, base);
    };
    let root_num_id: Option<String> = {
        let root = root_of(&sp);
        let num_pr = root.child_elements().find(|c| c.name() == "w:numPr");
        num_pr.and_then(|n| n.children_named("w:numId").next()).and_then(|e| e.attr("w:val")).map(str::to_owned)
    };
    let Split { open, close, mut kids, .. } = sp;
    let mut changed = match edited {
        Some(e) => drop_cleared(&mut kids, PPR_FIELD, e),
        None => false,
    };
    let mut gen_numbering = false;
    let mut set = |kids: &mut Vec<Kid>, name: &str, xml: Option<String>| {
        changed = true;
        set_kid(kids, PPR_ORDER, name, xml.as_deref());
    };
    let bits = ppr_bits(p, ids);
    if p.style_id != base.style_id {
        set(&mut kids, "w:pStyle", bits.p_style.clone());
    }
    let base_flags = flow_flags(&base);
    for (name, v) in flow_flags(p) {
        if base_flags.iter().find(|(n, _)| *n == name).map(|(_, b)| *b) != Some(v) {
            set(&mut kids, name, bits.flag(name));
        }
    }
    if p.textbox_tight_wrap != base.textbox_tight_wrap {
        set(&mut kids, "w:textboxTightWrap", bits.textbox_tight_wrap.clone());
    }
    if !same_json(dynv(p.tab_stops.as_ref()), dynv(base.tab_stops.as_ref()), mode) {
        set(&mut kids, "w:tabs", bits.tabs.clone());
    }
    if !same_json(dynv(p.borders.as_deref()), dynv(base.borders.as_deref()), mode)
        || (p.box_border == Some(true)) != (base.box_border == Some(true))
    {
        set(&mut kids, "w:pBdr", bits.p_bdr.clone());
    }
    if p.shading_hex != base.shading_hex {
        set(&mut kids, "w:shd", bits.shd.clone());
    }
    if !same_json(dynv(p.numbering.as_ref()), dynv(base.numbering.as_ref()), mode) {
        match p.numbering.as_ref() {
            None => set(&mut kids, "w:numPr", None),
            Some(n) => {
                if let Some(id) = n.num_id {
                    set(
                        &mut kids,
                        "w:numPr",
                        Some(format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{}\"/></w:numPr>", num_str(n.ilvl), num_str(id))),
                    );
                } else if let Some(nid) = root_num_id.as_deref().filter(|v| !v.is_empty() && *v != "0")
                    && base.numbering.as_ref().is_some_and(|b| b.kind == n.kind)
                {
                    set(
                        &mut kids,
                        "w:numPr",
                        Some(format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{nid}\"/></w:numPr>", num_str(n.ilvl))),
                    );
                } else {
                    set(&mut kids, "w:numPr", bits.num_pr.clone());
                    gen_numbering = true;
                }
            }
        }
    }
    if strict_ne(p.indent_left_twips, base.indent_left_twips)
        || strict_ne(p.indent_right_twips, base.indent_right_twips)
        || strict_ne(p.indent_first_line_twips, base.indent_first_line_twips)
    {
        let f = p.indent_first_line_twips;
        let mut ch = AttrChanges::new();
        ch.num("w:left", p.indent_left_twips);
        ch.set("w:start", None);
        ch.num("w:right", p.indent_right_twips);
        ch.set("w:end", None);
        ch.num("w:firstLine", f.filter(|f| f.to_number() >= 0.0));
        ch.set("w:hanging", f.filter(|f| f.to_number() < 0.0).map(|f| uo_jsnum::format::to_js_string(-f.to_number())));
        for k in ["w:firstLineChars", "w:hangingChars", "w:leftChars", "w:rightChars"] {
            ch.set(k, None);
        }
        let ind = attrs_with(kid_raw(&kids, "w:ind"), "w:ind", &ch)?;
        let x = (!super::matchers::is_empty_element(&ind, "w:ind")).then_some(ind);
        set(&mut kids, "w:ind", x);
    }
    if strict_ne(p.line_twentieths, base.line_twentieths)
        || strict_ne(p.space_after_twips, base.space_after_twips)
        || strict_ne(p.space_before_twips, base.space_before_twips)
        || p.space_before_auto != base.space_before_auto
        || p.space_after_auto != base.space_after_auto
    {
        let mut ch = AttrChanges::new();
        let truth = |v: Option<bool>| v == Some(true);
        if truth(p.space_before_auto) != truth(base.space_before_auto) {
            ch.set("w:beforeAutospacing", truth(p.space_before_auto).then(|| "1".to_owned()));
        }
        if truth(p.space_after_auto) != truth(base.space_after_auto) {
            ch.set("w:afterAutospacing", truth(p.space_after_auto).then(|| "1".to_owned()));
        }
        if strict_ne(p.line_twentieths, base.line_twentieths) {
            ch.num("w:line", p.line_twentieths);
            ch.set("w:lineRule", p.line_twentieths.map(|_| "auto".to_owned()));
        }
        let auto = |v: Option<bool>| v.map(|v| if v { "1".to_owned() } else { "0".to_owned() });
        if strict_ne(p.space_after_twips, base.space_after_twips) {
            ch.num("w:after", p.space_after_twips);
            ch.set("w:afterLines", None);
            ch.set("w:afterAutospacing", auto(p.space_after_auto));
        }
        if strict_ne(p.space_before_twips, base.space_before_twips) {
            ch.num("w:before", p.space_before_twips);
            ch.set("w:beforeLines", None);
            ch.set("w:beforeAutospacing", auto(p.space_before_auto));
        }
        if p.space_before_auto != base.space_before_auto {
            ch.set("w:beforeAutospacing", auto(p.space_before_auto));
        }
        if p.space_after_auto != base.space_after_auto {
            ch.set("w:afterAutospacing", auto(p.space_after_auto));
        }
        let sp = attrs_with(kid_raw(&kids, "w:spacing"), "w:spacing", &ch)?;
        let x = (!super::matchers::is_empty_element(&sp, "w:spacing")).then_some(sp);
        set(&mut kids, "w:spacing", x);
    }
    if p.alignment != base.alignment {
        let x = p.alignment.as_deref().filter(|a| !a.is_empty()).map(|a| format!("<w:jc w:val=\"{a}\"/>"));
        set(&mut kids, "w:jc", x);
    }
    if strict_ne(p.outline_lvl, base.outline_lvl) {
        set(&mut kids, "w:outlineLvl", p.outline_lvl.map(|l| format!("<w:outlineLvl w:val=\"{}\"/>", num_str(l))));
    }
    if *sect != SectArg::Keep {
        let cur = kid_raw(&kids, "w:sectPr").to_owned();
        if sect.as_xml() != cur {
            set(
                &mut kids,
                "w:sectPr",
                match sect {
                    SectArg::Set(s) => Some(s.clone()),
                    _ => None,
                },
            );
        }
    }
    if !same_json(dynv(p.mark_ins.as_deref()), dynv(base.mark_ins.as_deref()), mode)
        || !same_json(dynv(p.mark_del.as_deref()), dynv(base.mark_del.as_deref()), mode)
    {
        let cur = kid_raw(&kids, "w:rPr").to_owned();
        let x = mark_rpr(cx, &cur, p)?;
        set(&mut kids, "w:rPr", x);
    }
    if !same_json(dynv(p.pfmt.as_deref()), dynv(base.pfmt.as_deref()), mode) {
        match p.pfmt.as_deref() {
            None => set(&mut kids, "w:pPrChange", None),
            Some(pf) => {
                let rest: Vec<Kid> =
                    kids.iter().filter(|k| !matches!(k.name.as_str(), "w:rPr" | "w:sectPr" | "w:pPrChange")).cloned().collect();
                let before = if rest.is_empty() {
                    ppr_from_props(cx, &pf.old, "", ids)?
                } else {
                    patch_ppr_props(cx, Some(&join_kids(&open, &rest, &close)), &pf.old, None, &SectArg::Keep, ids)?
                };
                if before.gen_numbering {
                    gen_numbering = true;
                }
                let x = ppr_change_xml(cx, p, &before.xml);
                set(&mut kids, "w:pPrChange", Some(x));
            }
        }
    }
    Ok(PPrOut { xml: if changed { join_kids(&open, &kids, &close) } else { raw }, gen_numbering })
}

/// `colsXml(cols, raw)`: `w:cols` as Word writes it.
pub fn cols_xml(cols: Option<&Cols>, raw: Option<&str>, limits: &Limits) -> WResult<String> {
    if let Some(c) = cols
        && rel_num(Some(c.count)) > 1.0
    {
        let head = format!(
            "<w:cols w:num=\"{}\"{} w:space=\"{}\"",
            num_str(c.count),
            if c.sep { " w:sep=\"1\"" } else { "" },
            num_str(c.space_twips)
        );
        let Some(ws) = c.widths.as_ref().filter(|w| !w.is_empty()) else { return Ok(format!("{head}/>")) };
        let cols: String =
            ws.iter().map(|w| format!("<w:col w:w=\"{}\" w:space=\"{}\"/>", num_str(w.width_twips), num_str(w.space_twips))).collect();
        return Ok(format!("{head} w:equalWidth=\"0\">{cols}</w:cols>"));
    }
    let (sep, space) = match raw.filter(|r| !r.is_empty()) {
        Some(r) => {
            let sp = split(r, limits)?;
            let root = root_of(&sp);
            (root.attr("w:sep").map(str::to_owned), root.attr("w:space").map(str::to_owned))
        }
        None => (None, None),
    };
    let sep = sep.as_deref().is_some_and(|s| !s.is_empty() && s != "0" && s != "false");
    Ok(format!("<w:cols{} w:space=\"{}\"/>", if sep { " w:sep=\"1\"" } else { "" }, esc_attr(space.as_deref().unwrap_or("708"))))
}

/// `pgNumTypeXml(s, raw)`: `None` when no attribute is left.
pub fn pg_num_type_xml(s: &SectPr, raw: &str) -> WResult<Option<String>> {
    let chap = s.pg_num_chap_style.filter(|c| truthy(Some(*c)) && c.to_number() > 0.0);
    let base = if raw.is_empty() { String::new() } else { super::matchers::strip_pg_num_attrs(raw) };
    let mut ch = AttrChanges::new();
    ch.set("w:fmt", s.pg_num_fmt.as_deref().filter(|f| !f.is_empty() && *f != "decimal").map(str::to_owned));
    ch.num("w:start", s.pg_num_start);
    ch.num("w:chapStyle", chap);
    ch.set(
        "w:chapSep",
        if chap.is_some() { s.pg_num_chap_sep.as_deref().filter(|v| !v.is_empty() && *v != "hyphen").map(str::to_owned) } else { None },
    );
    let xml = attrs_with(&base, "w:pgNumType", &ch)?;
    Ok((!super::matchers::is_empty_element(&xml, "w:pgNumType")).then_some(xml))
}

/// `lnNumTypeXml(ln)`.
#[must_use]
pub fn ln_num_type_xml(ln: &LineNumbering) -> String {
    let count_by = match ln.count_by {
        Some(JsNumber::Num(v)) => uo_jsnum::format::to_js_string(v),
        _ => "1".to_owned(),
    };
    let start = if truthy(ln.start) { format!(" w:start=\"{}\"", super::js::opt_num_str(ln.start)) } else { String::new() };
    let distance = if truthy(ln.distance) { format!(" w:distance=\"{}\"", super::js::opt_num_str(ln.distance)) } else { String::new() };
    let restart = ln.restart.as_deref().filter(|r| !r.is_empty()).map(|r| format!(" w:restart=\"{r}\"")).unwrap_or_default();
    format!("<w:lnNumType w:countBy=\"{count_by}\"{start}{distance}{restart}/>")
}

/// A header/footer reference to write in a `w:sectPr` (`HfRefXml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfRefXml {
    /// header | footer
    pub kind: Str,
    pub r#type: Str,
    pub rid: Str,
}

/// JS default sort of strings (UTF-16 code units).
fn js_sort_strings(v: &mut [String]) {
    v.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
}

/// `patchSectPr(raw, cur, refs)`: the raw `w:sectPr` with the current geometry and references.
pub fn patch_sect_pr(cx: &mut WriteCtx, raw: &str, cur: &SectPr, refs: Option<&[HfRefXml]>) -> WResult<String> {
    let limits = cx.limits.clone();
    let mode = cx.mode;
    let sp = split(raw, &limits)?;
    let base = read_sect_pr(Some(root_of(&sp)));
    let ref_keys: Vec<String> = sp
        .kids
        .iter()
        .filter(|k| k.name == "w:headerReference" || k.name == "w:footerReference")
        .map(|k| {
            let el = k.el.and_then(|id| sp.doc.element(id));
            let ty = el.and_then(|e| e.attr("w:type")).unwrap_or("default");
            let rid = el.and_then(|e| e.attr("r:id")).unwrap_or("undefined");
            format!("{}|{ty}|{rid}", k.name)
        })
        .collect();
    let Split { open, close, mut kids, .. } = sp;
    let mut changed = false;
    let mut set = |kids: &mut Vec<Kid>, name: &str, xml: Option<String>| {
        changed = true;
        set_kid(kids, SECT_ORDER, name, xml.as_deref());
    };
    if let Some(refs) = refs {
        let mut now = ref_keys;
        js_sort_strings(&mut now);
        let mut want: Vec<String> = refs.iter().map(|r| format!("w:{}Reference|{}|{}", r.kind, r.r#type, r.rid)).collect();
        js_sort_strings(&mut want);
        if now.join(",") != want.join(",") {
            set(&mut kids, "w:headerReference", None);
            set(&mut kids, "w:footerReference", None);
            let new: Vec<Kid> = refs
                .iter()
                .map(|r| Kid {
                    name: format!("w:{}Reference", r.kind),
                    raw: format!("<w:{}Reference w:type=\"{}\" r:id=\"{}\"/>", r.kind, r.r#type, r.rid),
                    el: None,
                })
                .collect();
            kids.splice(0..0, new);
        }
    }
    if strict_ne(Some(cur.page_width_twips), Some(base.page_width_twips))
        || strict_ne(Some(cur.page_height_twips), Some(base.page_height_twips))
    {
        let mut ch = AttrChanges::new();
        ch.num("w:w", Some(cur.page_width_twips));
        ch.num("w:h", Some(cur.page_height_twips));
        ch.set("w:orient", (cur.page_width_twips.to_number() > cur.page_height_twips.to_number()).then(|| "landscape".to_owned()));
        let x = attrs_with(kid_raw(&kids, "w:pgSz"), "w:pgSz", &ch)?;
        set(&mut kids, "w:pgSz", Some(x));
    }
    let hd = or_num(cur.header_distance_twips, 720.0);
    let fd = or_num(cur.footer_distance_twips, 720.0);
    if strict_ne(Some(cur.margin_top_twips), Some(base.margin_top_twips))
        || strict_ne(Some(cur.margin_bottom_twips), Some(base.margin_bottom_twips))
        || strict_ne(Some(cur.margin_left_twips), Some(base.margin_left_twips))
        || strict_ne(Some(cur.margin_right_twips), Some(base.margin_right_twips))
        || strict_ne(Some(JsNumber::Num(hd)), base.header_distance_twips)
        || strict_ne(Some(JsNumber::Num(fd)), base.footer_distance_twips)
    {
        let pg_mar = kid_raw(&kids, "w:pgMar").to_owned();
        let mut ch = AttrChanges::new();
        ch.num("w:top", Some(cur.margin_top_twips));
        ch.num("w:right", Some(cur.margin_right_twips));
        ch.num("w:bottom", Some(cur.margin_bottom_twips));
        ch.num("w:left", Some(cur.margin_left_twips));
        ch.set("w:header", Some(uo_jsnum::format::to_js_string(hd)));
        ch.set("w:footer", Some(uo_jsnum::format::to_js_string(fd)));
        if pg_mar.is_empty() {
            ch.set("w:gutter", Some("0".to_owned()));
        }
        let x = attrs_with(&pg_mar, "w:pgMar", &ch)?;
        set(&mut kids, "w:pgMar", Some(x));
    }
    if cur.r#type.as_deref().unwrap_or("nextPage") != base.r#type.as_deref().unwrap_or("nextPage") {
        let x = cur.r#type.as_deref().filter(|t| !t.is_empty() && *t != "nextPage").map(|t| format!("<w:type w:val=\"{t}\"/>"));
        set(&mut kids, "w:type", x);
    }
    if !same_json(dynv(cur.cols.as_ref()), dynv(base.cols.as_ref()), mode) {
        let raw_cols = kid_raw(&kids, "w:cols").to_owned();
        let x = cols_xml(cur.cols.as_ref(), (!raw_cols.is_empty()).then_some(raw_cols.as_str()), &limits)?;
        set(&mut kids, "w:cols", Some(x));
    }
    if (cur.title_pg == Some(true)) != (base.title_pg == Some(true)) {
        set(&mut kids, "w:titlePg", (cur.title_pg == Some(true)).then(|| "<w:titlePg/>".to_owned()));
    }
    if let Some(pb) = cur.pg_borders.as_ref() {
        let cur_pb = pb.as_val().map(|b| &**b);
        let base_pb = base.pg_borders.as_ref().and_then(|b| b.as_val()).map(|b| &**b);
        if !same_json(dynv(cur_pb), dynv(base_pb), mode) {
            set(&mut kids, "w:pgBorders", super::emit::table::pg_borders_xml(cur_pb));
        }
    }
    if strict_ne(cur.pg_num_start, base.pg_num_start)
        || cur.pg_num_fmt != base.pg_num_fmt
        || strict_ne(cur.pg_num_chap_style, base.pg_num_chap_style)
        || cur.pg_num_chap_sep != base.pg_num_chap_sep
    {
        let raw_pg = kid_raw(&kids, "w:pgNumType").to_owned();
        let x = pg_num_type_xml(cur, &raw_pg)?;
        set(&mut kids, "w:pgNumType", x);
    }
    use super::emit::notes::{note_props_xml, same_note_props};
    if !same_note_props(cur.footnote_pr.as_ref(), base.footnote_pr.as_ref()) {
        let x = note_props_xml("footnote", cur.footnote_pr.as_ref());
        set(&mut kids, "w:footnotePr", (!x.is_empty()).then_some(x));
    }
    if !same_note_props(cur.endnote_pr.as_ref(), base.endnote_pr.as_ref()) {
        let x = note_props_xml("endnote", cur.endnote_pr.as_ref());
        set(&mut kids, "w:endnotePr", (!x.is_empty()).then_some(x));
    }
    if or_num(cur.footnote_columns, 0.0) != or_num(base.footnote_columns, 0.0) {
        let x = truthy(cur.footnote_columns)
            .then(|| format!("<w15:footnoteColumns w:val=\"{}\"/>", super::js::opt_num_str(cur.footnote_columns)));
        set(&mut kids, "w15:footnoteColumns", x);
    }
    if !same_json(dynv(cur.ln_num_type.as_ref()), dynv(base.ln_num_type.as_ref()), mode) {
        set(&mut kids, "w:lnNumType", cur.ln_num_type.as_ref().map(ln_num_type_xml));
    }
    if cur.v_align != base.v_align {
        let x = cur.v_align.as_deref().filter(|v| !v.is_empty()).map(|v| format!("<w:vAlign w:val=\"{v}\"/>"));
        set(&mut kids, "w:vAlign", x);
    }
    if !changed {
        return Ok(raw.to_owned());
    }
    let j = join_kids(&open, &kids, &close);
    Ok(if j.is_empty() { format!("{open}{close}") } else { j })
}
