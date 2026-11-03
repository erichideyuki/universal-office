//! Word's property cascade (`docx-layout/src/resolve.ts`, ECMA-376 §17.7.2): the merges of
//! `rPr`/`pPr`/`tblPr`/`trPr`/`tcPr`, the style resolver with its `basedOn` chains, the run cascade
//! with the toggle properties (XOR between style types, §17.7.3), theme fonts, and the colours
//! (`w:color` with theme tint/shade, `w:shd`, highlight, DrawingML specs).
//!
//! Every merge keeps the TS key insertion order ([`super::obj`]), which `JSON.stringify` keys
//! observe downstream (`bdrKey` over `pBdr`, the shaping key over `ot`, K9/LQ20).
//!
//! The resolver's caches are per layout call (K3, LQ18: the TS `CTX` `WeakMap` kept them across
//! calls) and its methods take the [`DocFormatting`] they read, so the resolver is plain data
//! (`Engine` holds it, K18).

use std::sync::Arc;

use indexmap::IndexMap;
use uo_drawingml::dml::{resolve_word_spec, to_hex_upper, word_theme_slot};
use uo_jsnum::math::{floor, max_n, min_n, round};

use super::num::{js_to_radix, parse_int_slice, u16s, upper};
use super::obj::{delete, set, spread, spread_opt};
use crate::js::{JsRecord, NumKey};
use crate::model::color::DrawingColor;
use crate::model::formatting::{DocDefaults, DocFormatting, StyleDef, TblStylePart, Theme};
use crate::model::props::{BorderSpec, ColorSpec, PPr, RFonts, RPr, ShadingSpec, TabSpec, TblPr, TcPr, TrPr, UnderlineSpec};

/// `TOGGLES` (resolve.ts:17): the toggle properties, combined by XOR between style levels.
pub const TOGGLES: [&str; 13] =
    ["b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss", "imprint", "vanish"];

/// The field of toggle `k` (index into [`TOGGLES`]).
fn toggle_mut(r: &mut RPr, k: usize) -> Option<&mut Option<bool>> {
    Some(match k {
        0 => &mut r.b,
        1 => &mut r.b_cs,
        2 => &mut r.i,
        3 => &mut r.i_cs,
        4 => &mut r.caps,
        5 => &mut r.small_caps,
        6 => &mut r.strike,
        7 => &mut r.dstrike,
        8 => &mut r.outline,
        9 => &mut r.shadow,
        10 => &mut r.emboss,
        11 => &mut r.imprint,
        12 => &mut r.vanish,
        _ => return None,
    })
}

fn toggle(r: &RPr, k: usize) -> Option<bool> {
    match k {
        0 => r.b,
        1 => r.b_cs,
        2 => r.i,
        3 => r.i_cs,
        4 => r.caps,
        5 => r.small_caps,
        6 => r.strike,
        7 => r.dstrike,
        8 => r.outline,
        9 => r.shadow,
        10 => r.emboss,
        11 => r.imprint,
        12 => r.vanish,
        _ => None,
    }
}

// ───────────────────────── merges ─────────────────────────

/// One `rFonts` slot: the name key and the theme key (`mergeRFonts`' `slot`).
const RFONT_SLOTS: [(&str, &str); 4] = [("ascii", "asciiTheme"), ("hAnsi", "hAnsiTheme"), ("eastAsia", "eastAsiaTheme"), ("cs", "cstheme")];

fn rfont<'a>(f: &'a RFonts, k: &str) -> Option<&'a crate::js::Str> {
    match k {
        "ascii" => f.ascii.as_ref(),
        "hAnsi" => f.h_ansi.as_ref(),
        "eastAsia" => f.east_asia.as_ref(),
        "cs" => f.cs.as_ref(),
        "asciiTheme" => f.ascii_theme.as_ref(),
        "hAnsiTheme" => f.h_ansi_theme.as_ref(),
        "eastAsiaTheme" => f.east_asia_theme.as_ref(),
        "cstheme" => f.cstheme.as_ref(),
        "hint" => f.hint.as_ref(),
        _ => None,
    }
}

fn rfont_mut<'a>(f: &'a mut RFonts, k: &str) -> Option<&'a mut Option<crate::js::Str>> {
    Some(match k {
        "ascii" => &mut f.ascii,
        "hAnsi" => &mut f.h_ansi,
        "eastAsia" => &mut f.east_asia,
        "cs" => &mut f.cs,
        "asciiTheme" => &mut f.ascii_theme,
        "hAnsiTheme" => &mut f.h_ansi_theme,
        "eastAsiaTheme" => &mut f.east_asia_theme,
        "cstheme" => &mut f.cstheme,
        "hint" => &mut f.hint,
        _ => return None,
    })
}

/// `mergeRFonts` (resolve.ts:24): each slot (name | theme) is replaced whole by the upper level
/// that defines either half.
#[must_use]
pub fn merge_rfonts(base: Option<&RFonts>, over: Option<&RFonts>) -> Option<RFonts> {
    let Some(over) = over else { return base.cloned() };
    let Some(base) = base else { return Some(over.clone()) };
    let mut out = base.clone();
    for (name, theme) in RFONT_SLOTS {
        let (n, t) = (rfont(over, name).cloned(), rfont(over, theme).cloned());
        if n.is_some() || t.is_some() {
            delete(&mut out, name);
            delete(&mut out, theme);
            if let Some(v) = n {
                set(&mut out, name, |o| {
                    if let Some(f) = rfont_mut(o, name) {
                        *f = Some(v);
                    }
                });
            }
            if let Some(v) = t {
                set(&mut out, theme, |o| {
                    if let Some(f) = rfont_mut(o, theme) {
                        *f = Some(v);
                    }
                });
            }
        }
    }
    if let Some(h) = over.hint.clone() {
        set(&mut out, "hint", |o| o.hint = Some(h));
    }
    Some(out)
}

/// `mergeRPr` (resolve.ts:44).
#[must_use]
pub fn merge_rpr(base: Option<&RPr>, over: Option<&RPr>) -> RPr {
    let Some(over) = over else { return base.cloned().unwrap_or_default() };
    let Some(base) = base else { return over.clone() };
    let mut out = spread(base, over);
    if let Some(f) = merge_rfonts(base.r_fonts.as_deref(), over.r_fonts.as_deref()) {
        set(&mut out, "rFonts", |o| o.r_fonts = Some(Box::new(f)));
    }
    if let (Some(b), Some(o)) = (&base.lang, &over.lang) {
        let l = spread(&**b, &**o);
        set(&mut out, "lang", |x| x.lang = Some(Box::new(l)));
    }
    if let (Some(b), Some(o)) = (&base.fx, &over.fx) {
        let f = spread(&**b, &**o);
        set(&mut out, "fx", |x| x.fx = Some(Box::new(f)));
    }
    if let (Some(b), Some(o)) = (&base.ot, &over.ot) {
        let f = spread(&**b, &**o);
        set(&mut out, "ot", |x| x.ot = Some(Box::new(f)));
    }
    out
}

/// The `Map` key of a tab position (SameValueZero; `null` after a JSON round trip is its own key).
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum TabKey {
    Null,
    Num(NumKey),
}

fn tab_key(t: &TabSpec) -> TabKey {
    match t.pos {
        crate::js::JsNumber::Null => TabKey::Null,
        crate::js::JsNumber::Num(x) => TabKey::Num(NumKey::new(x)),
    }
}

/// `mergeTabs` (resolve.ts:57): by position, `clear` removes, sorted by position.
fn merge_tabs(base: Option<&Vec<TabSpec>>, over: Option<&Vec<TabSpec>>) -> Option<Vec<TabSpec>> {
    let Some(over) = over else { return base.cloned() };
    let mut map: IndexMap<TabKey, TabSpec> = IndexMap::new();
    for t in base.map(Vec::as_slice).unwrap_or(&[]) {
        map.insert(tab_key(t), t.clone());
    }
    for t in over {
        if t.val.as_ref() == "clear" {
            map.shift_remove(&tab_key(t));
        } else {
            map.insert(tab_key(t), t.clone());
        }
    }
    let mut v: Vec<TabSpec> = map.into_values().collect();
    super::obj::js_sort(&mut v, |a, b| a.pos.to_number() - b.pos.to_number());
    Some(v)
}

/// `mergePPr` (resolve.ts:68).
#[must_use]
pub fn merge_ppr(base: Option<&PPr>, over: Option<&PPr>) -> PPr {
    let Some(over) = over else { return base.cloned().unwrap_or_default() };
    let Some(base) = base else {
        let mut out = over.clone();
        if let Some(tabs) = &over.tabs {
            let kept: Vec<TabSpec> = tabs.iter().filter(|t| t.val.as_ref() != "clear").cloned().collect();
            set(&mut out, "tabs", |o| o.tabs = Some(kept));
        }
        return out;
    };
    let mut out = spread(base, over);
    if base.spacing.is_some() || over.spacing.is_some() {
        let sp = spread_opt(base.spacing.as_deref(), over.spacing.as_deref());
        set(&mut out, "spacing", |o| o.spacing = Some(Box::new(sp)));
    }
    if base.ind.is_some() || over.ind.is_some() {
        let mut ind = spread_opt(base.ind.as_deref(), over.ind.as_deref());
        if let Some(oi) = over.ind.as_deref() {
            // firstLine × hanging are one slot
            if oi.hanging.is_some() && oi.first_line.is_none() {
                delete(&mut ind, "firstLine");
            }
            if oi.first_line.is_some() && oi.hanging.is_none() {
                delete(&mut ind, "hanging");
            }
            if oi.hanging_chars.is_some() {
                delete(&mut ind, "firstLineChars");
            }
            if oi.first_line_chars.is_some() {
                delete(&mut ind, "hangingChars");
            }
        }
        set(&mut out, "ind", |o| o.ind = Some(Box::new(ind)));
    }
    if base.p_bdr.is_some() || over.p_bdr.is_some() {
        let b = spread_opt(base.p_bdr.as_deref(), over.p_bdr.as_deref());
        set(&mut out, "pBdr", |o| o.p_bdr = Some(Box::new(b)));
    }
    if base.num_pr.is_some() || over.num_pr.is_some() {
        let n = spread_opt(base.num_pr.as_deref(), over.num_pr.as_deref());
        set(&mut out, "numPr", |o| o.num_pr = Some(Box::new(n)));
    }
    if let Some(tabs) = merge_tabs(base.tabs.as_ref(), over.tabs.as_ref()) {
        set(&mut out, "tabs", |o| o.tabs = Some(tabs));
    }
    if base.r_pr.is_some() || over.r_pr.is_some() {
        let r = merge_rpr(base.r_pr.as_deref(), over.r_pr.as_deref());
        set(&mut out, "rPr", |o| o.r_pr = Some(Box::new(r)));
    }
    if base.frame_pr.is_some() || over.frame_pr.is_some() {
        let f = spread_opt(base.frame_pr.as_deref(), over.frame_pr.as_deref());
        set(&mut out, "framePr", |o| o.frame_pr = Some(Box::new(f)));
    }
    out
}

/// `mergeBorders` / `mergeMargins` (resolve.ts:91, 97).
fn merge_obj<T: crate::js::JsAssign + Clone>(base: Option<&T>, over: Option<&T>) -> Option<T> {
    let Some(over) = over else { return base.cloned() };
    let Some(base) = base else { return Some(over.clone()) };
    Some(spread(base, over))
}

/// `mergeTblPr` (resolve.ts:103).
#[must_use]
pub fn merge_tbl_pr(base: Option<&TblPr>, over: Option<&TblPr>) -> TblPr {
    let Some(over) = over else { return base.cloned().unwrap_or_default() };
    let Some(base) = base else { return over.clone() };
    let mut out = spread(base, over);
    if let Some(b) = merge_obj(base.tbl_borders.as_deref(), over.tbl_borders.as_deref()) {
        set(&mut out, "tblBorders", |o| o.tbl_borders = Some(Box::new(b)));
    }
    if let Some(m) = merge_obj(base.tbl_cell_mar.as_deref(), over.tbl_cell_mar.as_deref()) {
        set(&mut out, "tblCellMar", |o| o.tbl_cell_mar = Some(Box::new(m)));
    }
    if base.tbl_look.is_some() || over.tbl_look.is_some() {
        let l = spread_opt(base.tbl_look.as_deref(), over.tbl_look.as_deref());
        set(&mut out, "tblLook", |o| o.tbl_look = Some(Box::new(l)));
    }
    out
}

/// `mergeTrPr` (resolve.ts:115).
#[must_use]
pub fn merge_tr_pr(base: Option<&TrPr>, over: Option<&TrPr>) -> TrPr {
    spread_opt(base, over)
}

/// `mergeTcPr` (resolve.ts:119).
#[must_use]
pub fn merge_tc_pr(base: Option<&TcPr>, over: Option<&TcPr>) -> TcPr {
    let Some(over) = over else { return base.cloned().unwrap_or_default() };
    let Some(base) = base else { return over.clone() };
    let mut out = spread(base, over);
    if let Some(b) = merge_obj(base.tc_borders.as_deref(), over.tc_borders.as_deref()) {
        set(&mut out, "tcBorders", |o| o.tc_borders = Some(Box::new(b)));
    }
    if let Some(m) = merge_obj(base.tc_mar.as_deref(), over.tc_mar.as_deref()) {
        set(&mut out, "tcMar", |o| o.tc_mar = Some(Box::new(m)));
    }
    out
}

// ───────────────────────── styles ─────────────────────────

/// `ResolvedParaStyle` (resolve.ts:132). `r_pr` is shared with [`StyleResolver::char_style`] of a
/// linked style (the TS returns the same object).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedParaStyle {
    pub p_pr: PPr,
    pub r_pr: Arc<RPr>,
}

/// `ResolvedTableStyle` (resolve.ts:137).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedTableStyle {
    pub tbl_pr: TblPr,
    pub tr_pr: TrPr,
    pub tc_pr: TcPr,
    pub p_pr: PPr,
    pub r_pr: RPr,
    /// conditional formatting per `TblStyleType` (every part present); read through
    /// [`COND_ORDER`], so a type name such as `__proto__` (which the TS assignment turns into the
    /// object's prototype) is just an unused key here
    pub cond: JsRecord<TblStylePart>,
    /// `hasStyle`
    pub has_style: bool,
}

/// `COND_ORDER` (resolve.ts:150): the order conditional formats apply (the last wins).
pub const COND_ORDER: [&str; 13] = [
    "wholeTable",
    "band1Vert",
    "band2Vert",
    "band1Horz",
    "band2Horz",
    "firstCol",
    "lastCol",
    "firstRow",
    "lastRow",
    "nwCell",
    "neCell",
    "swCell",
    "seCell",
];

/// `own(rec, k)` (resolve.ts:297): an own key of a record (`undefined` key → none).
#[must_use]
pub fn own<'a, T>(rec: &'a JsRecord<T>, k: Option<&str>) -> Option<&'a T> {
    rec.get(k?)
}

/// `chain(id, type)` (resolve.ts:169): the `basedOn` chain, root first, at most 32 styles, stopping
/// at a style of another type or a cycle.
#[must_use]
pub fn style_chain<'f>(fmt: &'f DocFormatting, id: Option<&str>, ty: &str) -> Vec<&'f StyleDef> {
    let styles = &fmt.styles.styles;
    let mut out: Vec<&StyleDef> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut cur = id.filter(|s| !s.is_empty()).and_then(|i| styles.get(i));
    while let Some(c) = cur {
        if seen.contains(&&*c.style_id) || out.len() >= 32 {
            break;
        }
        if c.r#type.as_ref() != ty {
            break;
        }
        seen.push(&c.style_id);
        out.push(c);
        cur = c.based_on.as_deref().filter(|s| !s.is_empty()).and_then(|b| styles.get(b));
    }
    out.reverse();
    out
}

/// `StyleResolver` (resolve.ts:155): resolved paragraph, character and table styles, cached per
/// style id for one layout call.
#[derive(Debug, Clone, Default)]
pub struct StyleResolver {
    para_cache: IndexMap<Box<str>, Arc<ResolvedParaStyle>>,
    char_cache: IndexMap<Box<str>, Arc<RPr>>,
    table_cache: IndexMap<Box<str>, Arc<ResolvedTableStyle>>,
}

impl StyleResolver {
    #[must_use]
    pub fn new() -> StyleResolver {
        StyleResolver::default()
    }

    /// `docDefaults`.
    #[must_use]
    pub fn doc_defaults(fmt: &DocFormatting) -> &DocDefaults {
        &fmt.styles.doc_defaults
    }

    /// `paraStyleId(id)` (resolve.ts:188): the paragraph style in effect (absent or not a
    /// paragraph style → the default paragraph style).
    #[must_use]
    pub fn para_style_id<'a>(fmt: &'a DocFormatting, id: Option<&'a str>) -> Option<&'a str> {
        if let Some(i) = id.filter(|s| !s.is_empty())
            && fmt.styles.styles.get(i).is_some_and(|s| s.r#type.as_ref() == "paragraph")
        {
            return Some(i);
        }
        fmt.styles.defaults.paragraph.as_deref()
    }

    /// `paraChain(id)` (resolve.ts:183).
    #[must_use]
    pub fn para_chain<'f>(fmt: &'f DocFormatting, id: Option<&'f str>) -> Vec<&'f StyleDef> {
        style_chain(fmt, Some(Self::para_style_id(fmt, id).unwrap_or("")), "paragraph")
    }

    /// `paraStyle(id)` (resolve.ts:193).
    pub fn para_style(&mut self, fmt: &DocFormatting, id: Option<&str>) -> Arc<ResolvedParaStyle> {
        let key = Self::para_style_id(fmt, id).unwrap_or("");
        if let Some(r) = self.para_cache.get(key) {
            return Arc::clone(r);
        }
        let mut p_pr = PPr::default();
        let mut r_pr = RPr::default();
        for s in style_chain(fmt, Some(key), "paragraph") {
            p_pr = merge_ppr(Some(&p_pr), s.p_pr.as_deref());
            r_pr = merge_rpr(Some(&r_pr), s.r_pr.as_deref());
        }
        let r = Arc::new(ResolvedParaStyle { p_pr, r_pr: Arc::new(r_pr) });
        self.para_cache.insert(key.into(), Arc::clone(&r));
        r
    }

    /// `charStyle(id)` (resolve.ts:209): a linked paragraph style used as `rStyle` gives that
    /// style's `rPr`.
    pub fn char_style(&mut self, fmt: &DocFormatting, id: Option<&str>) -> Arc<RPr> {
        let Some(id) = id.filter(|s| !s.is_empty()) else { return Arc::new(RPr::default()) };
        if let Some(r) = self.char_cache.get(id) {
            return Arc::clone(r);
        }
        let chain = style_chain(fmt, Some(id), "character");
        let mut r = RPr::default();
        for s in &chain {
            r = merge_rpr(Some(&r), s.r_pr.as_deref());
        }
        let mut r = Arc::new(r);
        if chain.is_empty() && fmt.styles.styles.get(id).is_some_and(|s| s.r#type.as_ref() == "paragraph") {
            r = Arc::clone(&self.para_style(fmt, Some(id)).r_pr);
        }
        self.char_cache.insert(id.into(), Arc::clone(&r));
        r
    }

    /// `tableStyle(id)` (resolve.ts:222).
    pub fn table_style(&mut self, fmt: &DocFormatting, id: Option<&str>) -> Arc<ResolvedTableStyle> {
        let styles = &fmt.styles.styles;
        let key: &str = match id.filter(|s| !s.is_empty()) {
            Some(i) if styles.get(i).is_some_and(|s| s.r#type.as_ref() == "table") => i,
            _ => fmt.styles.defaults.table.as_deref().unwrap_or(""),
        };
        if let Some(r) = self.table_cache.get(key) {
            return Arc::clone(r);
        }
        let chain = style_chain(fmt, Some(key), "table");
        let mut r = ResolvedTableStyle { has_style: !chain.is_empty(), ..ResolvedTableStyle::default() };
        for s in chain {
            r.tbl_pr = merge_tbl_pr(Some(&r.tbl_pr), s.tbl_pr.as_deref());
            r.tr_pr = merge_tr_pr(Some(&r.tr_pr), s.tr_pr.as_deref());
            r.tc_pr = merge_tc_pr(Some(&r.tc_pr), s.tc_pr.as_deref());
            r.p_pr = merge_ppr(Some(&r.p_pr), s.p_pr.as_deref());
            r.r_pr = merge_rpr(Some(&r.r_pr), s.r_pr.as_deref());
            if let Some(parts) = &s.tbl_style_pr {
                for (t, part) in parts.iter() {
                    let prev = r.cond.get(t);
                    let next = TblStylePart {
                        p_pr: Some(Box::new(merge_ppr(prev.and_then(|p| p.p_pr.as_deref()), part.p_pr.as_deref()))),
                        r_pr: Some(Box::new(merge_rpr(prev.and_then(|p| p.r_pr.as_deref()), part.r_pr.as_deref()))),
                        tbl_pr: Some(Box::new(merge_tbl_pr(prev.and_then(|p| p.tbl_pr.as_deref()), part.tbl_pr.as_deref()))),
                        tr_pr: Some(Box::new(merge_tr_pr(prev.and_then(|p| p.tr_pr.as_deref()), part.tr_pr.as_deref()))),
                        tc_pr: Some(Box::new(merge_tc_pr(prev.and_then(|p| p.tc_pr.as_deref()), part.tc_pr.as_deref()))),
                        ..TblStylePart::default()
                    };
                    r.cond.insert(t, next);
                }
            }
        }
        let r = Arc::new(r);
        self.table_cache.insert(key.into(), Arc::clone(&r));
        r
    }

    /// Drops the caches (`clear_caches`, M1).
    pub fn clear(&mut self) {
        *self = StyleResolver::default();
    }
}

/// `cascadeRPr` (resolve.ts:256): docDefaults as the base, each style level (already resolved in
/// its chain) with the toggles combined by XOR, the direct formatting absolute.
#[must_use]
pub fn cascade_rpr(doc_defaults: Option<&RPr>, style_levels: &[Option<&RPr>], direct: Option<&RPr>) -> RPr {
    let mut out = doc_defaults.cloned().unwrap_or_default();
    let mut toggled: [Option<bool>; 13] = [None; 13];
    for lvl in style_levels.iter().flatten() {
        let mut rest = (*lvl).clone();
        for (k, name) in TOGGLES.iter().enumerate() {
            let Some(v) = toggle(lvl, k) else { continue };
            delete(&mut rest, name);
            if let Some(t) = toggled.get_mut(k) {
                if v {
                    *t = Some(!t.unwrap_or(false));
                } else if t.is_none() {
                    *t = Some(false);
                }
            }
        }
        out = merge_rpr(Some(&out), Some(&rest));
    }
    for (k, name) in TOGGLES.iter().enumerate() {
        let Some(t) = toggled.get(k).copied().flatten() else { continue };
        let base = doc_defaults.and_then(|d| toggle(d, k)).unwrap_or(false);
        let v = if t { !base } else { base };
        set(&mut out, name, |o| {
            if let Some(f) = toggle_mut(o, k) {
                *f = Some(v);
            }
        });
    }
    merge_rpr(Some(&out), direct)
}

// ───────────────────────── theme fonts ─────────────────────────

/// An `rFonts` slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSlot {
    Ascii,
    HAnsi,
    EastAsia,
    Cs,
}

/// `fontOfSlot(rf, slot, theme)` (resolve.ts:282): the theme font has priority over the name.
#[must_use]
pub fn font_of_slot<'a>(rf: Option<&'a RFonts>, slot: FontSlot, theme: &'a Theme) -> Option<&'a str> {
    let rf = rf?;
    let (t, name) = match slot {
        FontSlot::Ascii => (&rf.ascii_theme, &rf.ascii),
        FontSlot::HAnsi => (&rf.h_ansi_theme, &rf.h_ansi),
        FontSlot::EastAsia => (&rf.east_asia_theme, &rf.east_asia),
        FontSlot::Cs => (&rf.cstheme, &rf.cs),
    };
    if let Some(t) = t.as_deref().filter(|s| !s.is_empty()) {
        let set = if t.starts_with("major") { &theme.major } else { &theme.minor };
        let n = if t.ends_with("EastAsia") {
            &set.ea
        } else if t.ends_with("Bidi") {
            &set.cs
        } else {
            &set.latin
        };
        if let Some(n) = n.as_deref().filter(|s| !s.is_empty()) {
            return Some(n);
        }
    }
    name.as_deref()
}

// ───────────────────────── colours ─────────────────────────

const HIGHLIGHT: [(&str, &str); 16] = [
    ("yellow", "FFFF00"),
    ("green", "00FF00"),
    ("cyan", "00FFFF"),
    ("magenta", "FF00FF"),
    ("blue", "0000FF"),
    ("red", "FF0000"),
    ("darkBlue", "000080"),
    ("darkCyan", "008080"),
    ("darkGreen", "008000"),
    ("darkMagenta", "800080"),
    ("darkRed", "800000"),
    ("darkYellow", "808000"),
    ("darkGray", "808080"),
    ("lightGray", "C0C0C0"),
    ("black", "000000"),
    ("white", "FFFFFF"),
];

/// `highlightHex(name)` (resolve.ts:305).
#[must_use]
pub fn highlight_hex(name: Option<&str>) -> Option<&'static str> {
    let n = name?;
    HIGHLIGHT.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
}

/// `rgb(hex)` (resolve.ts:312) over UTF-16 units: `parseInt` of each pair.
fn rgb(hex: &[u16]) -> [f64; 3] {
    [parse_int_slice(hex, 0, 2, 16), parse_int_slice(hex, 2, 4, 16), parse_int_slice(hex, 4, 6, 16)]
}

/// `applyTintShade(hex, tint, shade)` (resolve.ts:327): Word's theme tint/shade on the HSL
/// lightness, channels truncated.
#[must_use]
pub fn apply_tint_shade(hex: &str, tint: Option<&str>, shade: Option<&str>) -> String {
    let tint = tint.filter(|s| !s.is_empty());
    let shade = shade.filter(|s| !s.is_empty());
    if tint.is_none() && shade.is_none() {
        return upper(hex);
    }
    let [r, g, b] = rgb(&u16s(hex)).map(|v| v / 255.0);
    let mx = max_n(&[r, g, b]);
    let mn = min_n(&[r, g, b]);
    let mut l = (mx + mn) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if mx != mn {
        let d = mx - mn;
        s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
        h = (if mx == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if mx == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        }) / 6.0;
    }
    if let Some(sh) = shade {
        l *= uo_jsnum::parse::parse_int(sh, 16) / 255.0;
    }
    if let Some(ti) = tint {
        let t = uo_jsnum::parse::parse_int(ti, 16) / 255.0;
        l = l * t + 1.0 - t;
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let ch = |x: f64| -> f64 {
        let mut x = x;
        if x < 0.0 {
            x += 1.0;
        }
        if x > 1.0 {
            x -= 1.0;
        }
        let v = if x < 1.0 / 6.0 {
            p + (q - p) * 6.0 * x
        } else if x < 1.0 / 2.0 {
            q
        } else if x < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - x) * 6.0
        } else {
            p
        };
        floor(v * 255.0)
    };
    to_hex_upper(ch(h + 1.0 / 3.0), ch(h), ch(h - 1.0 / 3.0))
}

/// The four fields `resolveColor` reads (`{ val?, themeColor?, themeTint?, themeShade? }`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorRef<'a> {
    pub val: Option<&'a str>,
    pub theme_color: Option<&'a str>,
    pub theme_tint: Option<&'a str>,
    pub theme_shade: Option<&'a str>,
}

impl<'a> ColorRef<'a> {
    /// A `w:color`.
    #[must_use]
    pub fn of_color(c: &'a ColorSpec) -> ColorRef<'a> {
        ColorRef {
            val: c.val.as_deref(),
            theme_color: c.theme_color.as_deref(),
            theme_tint: c.theme_tint.as_deref(),
            theme_shade: c.theme_shade.as_deref(),
        }
    }

    /// A `w:u` (its colour attributes).
    #[must_use]
    pub fn of_underline(u: &'a UnderlineSpec) -> ColorRef<'a> {
        ColorRef {
            val: u.color.as_deref(),
            theme_color: u.theme_color.as_deref(),
            theme_tint: u.theme_tint.as_deref(),
            theme_shade: u.theme_shade.as_deref(),
        }
    }

    /// A border (`BorderSpec`: `color`, `themeColor`, `themeTint`, `themeShade`).
    #[must_use]
    pub fn of_border(b: &'a BorderSpec) -> ColorRef<'a> {
        ColorRef {
            val: b.color.as_deref(),
            theme_color: b.theme_color.as_deref(),
            theme_tint: b.theme_tint.as_deref(),
            theme_shade: b.theme_shade.as_deref(),
        }
    }
}

/// `resolveColor(spec, theme)` (resolve.ts:351): RRGGBB, or `None` for automatic.
#[must_use]
pub fn resolve_color(spec: Option<ColorRef<'_>>, theme: &Theme) -> Option<String> {
    let spec = spec?;
    if let Some(tc) = spec.theme_color.filter(|s| !s.is_empty()) {
        let key = word_theme_slot(tc);
        if let Some(base) = theme.colors.get(key).filter(|s| !s.is_empty()) {
            return Some(apply_tint_shade(base, spec.theme_tint, spec.theme_shade));
        }
    }
    let v = spec.val.filter(|s| !s.is_empty())?;
    if v == "auto" {
        return None;
    }
    uo_drawingml::dml::is_hex6(v).then(|| v.to_ascii_uppercase())
}

const SHD_COVER: [(&str, f64); 16] = [
    ("horzStripe", 0.5),
    ("vertStripe", 0.5),
    ("reverseDiagStripe", 0.5),
    ("diagStripe", 0.5),
    ("horzCross", 0.5),
    ("diagCross", 0.75),
    ("thinHorzStripe", 0.25),
    ("thinVertStripe", 0.25),
    ("thinReverseDiagStripe", 0.25),
    ("thinDiagStripe", 0.25),
    ("thinHorzCross", 0.4375),
    ("thinDiagCross", 0.4375),
    ("pct12", 0.125),
    ("pct37", 0.375),
    ("pct62", 0.625),
    ("pct87", 0.875),
];

/// `/^pct\d+$/`.
fn is_pct(v: &str) -> bool {
    v.strip_prefix("pct").is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// `shadingFill(shd, theme)` (resolve.ts:379): the fill, the solid colour or the mix of the
/// pattern over the fill; `None` = no shading.
#[must_use]
pub fn shading_fill(shd: Option<&ShadingSpec>, theme: &Theme) -> Option<String> {
    let shd = shd?;
    if shd.val.as_deref() == Some("nil") {
        return None;
    }
    let fg = || -> String {
        resolve_color(Some(ColorRef { val: shd.color.as_deref(), theme_color: shd.theme_color.as_deref(), ..ColorRef::default() }), theme)
            .unwrap_or_else(|| "000000".into())
    };
    if shd.val.as_deref() == Some("solid") {
        return Some(fg());
    }
    let fill = if shd.theme_fill.as_deref().is_some_and(|s| !s.is_empty()) {
        resolve_color(
            Some(ColorRef {
                val: None,
                theme_color: shd.theme_fill.as_deref(),
                theme_tint: shd.theme_fill_tint.as_deref(),
                theme_shade: shd.theme_fill_shade.as_deref(),
            }),
            theme,
        )
    } else {
        match shd.fill.as_deref() {
            Some(f) if !f.is_empty() && f != "auto" => Some(upper(f)),
            _ => None,
        }
    };
    let v = shd.val.as_deref().unwrap_or("clear");
    let cover = match SHD_COVER.iter().find(|(k, _)| *k == v) {
        Some((_, c)) => Some(*c),
        None if is_pct(v) => Some(uo_jsnum::parse::parse_int(v.get(3..).unwrap_or(""), 10) / 100.0),
        None => None,
    };
    let Some(cover) = cover else { return fill };
    let f = u16s(&fg());
    let bg = u16s(fill.as_deref().unwrap_or("FFFFFF"));
    let mix = |i: usize| -> f64 { round(parse_int_slice(&bg, i, i + 2, 16) * (1.0 - cover) + parse_int_slice(&f, i, i + 2, 16) * cover) };
    let mut out = String::new();
    for i in [0usize, 2, 4] {
        out.push_str(&super::num::pad_start_zero(&js_to_radix(mix(i), 16), 2));
    }
    Some(upper(&out))
}

/// `resolveDrawingColor(spec, theme)` (resolve.ts:398), through uo-drawingml's
/// `resolve_word_spec` over the structured spec (its named difference: modifier values holding
/// `|`, `,` or `=`).
#[must_use]
pub fn resolve_drawing_color(spec: Option<&DrawingColor>, theme: &Theme) -> Option<String> {
    let s = &spec?.0;
    if s.to_ts_string().is_empty() {
        return None;
    }
    resolve_word_spec(&s.head, s.parsed_mods(), &theme.colors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::{JsObject, JsonWriter};

    fn json(v: &dyn crate::js::JsValue) -> String {
        JsonWriter::stringify(v, None).unwrap_or_default()
    }

    #[test]
    fn tint_and_shade_match_word() {
        // the measured examples of resolve.ts:323
        assert_eq!(apply_tint_shade("156082", Some("33"), None), "C1E4F5");
        assert_eq!(apply_tint_shade("156082", Some("99"), None), "45B0E1");
        assert_eq!(apply_tint_shade("156082", None, Some("BF")), "0F4761");
        assert_eq!(apply_tint_shade("zz", Some("10"), None), "NANNANNAN");
    }

    #[test]
    fn merge_ppr_ind_slots_and_key_order() {
        let mut base = PPr::default();
        let mut ind = crate::model::props::IndSpec { first_line: Some(100.0.into()), left: Some(10.0.into()), ..Default::default() };
        ind.mark("firstLine");
        ind.mark("left");
        base.ind = Some(Box::new(ind));
        let over = PPr {
            ind: Some(Box::new(crate::model::props::IndSpec { hanging: Some(50.0.into()), ..Default::default() })),
            ..PPr::default()
        };
        let m = merge_ppr(Some(&base), Some(&over));
        assert_eq!(json(&m), r#"{"ind":{"left":10,"hanging":50}}"#);
    }

    #[test]
    fn cascade_toggles_xor() {
        let dd = RPr { b: Some(true), ..RPr::default() };
        let style = RPr { b: Some(true), i: Some(false), ..RPr::default() };
        let ch = RPr { b: Some(true), ..RPr::default() };
        let r = cascade_rpr(Some(&dd), &[Some(&style), Some(&ch)], None);
        // b toggled twice -> false -> base; i toggled to false -> base false
        assert_eq!(r.b, Some(true));
        assert_eq!(r.i, Some(false));
    }

    #[test]
    fn shading_mix() {
        let theme = Theme::default();
        let shd =
            ShadingSpec { val: Some("pct50".into()), color: Some("000000".into()), fill: Some("ffffff".into()), ..Default::default() };
        // 255 * 0.5 + 0 * 0.5 = 127.5 -> 128 = 80
        assert_eq!(shading_fill(Some(&shd), &theme).as_deref(), Some("808080"));
        let shd2 = ShadingSpec { val: Some("clear".into()), fill: Some("auto".into()), ..Default::default() };
        assert_eq!(shading_fill(Some(&shd2), &theme), None);
        assert_eq!(RPr::keys().len(), 43);
    }
}
