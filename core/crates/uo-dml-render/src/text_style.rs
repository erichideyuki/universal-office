//! The text-style cascade (`text-style.ts`): a level of paragraph/run defaults, nine levels, the
//! context the cascade reads and its mechanics (`parseTxStyleLevels`, `mergeStyleLevels`,
//! `applyTextStyles`, `applyShapeTextCascade`). The PowerPoint context is built by `uo-pptx`
//! (`buildTextStyleContext`), the Excel one by Sheet (`EXCEL_TEXT`, drawings.ts).

use std::sync::Arc;

use uo_jsnum::math;
use uo_jsnum::sort::js_sort_by;
use uo_opc::xml::Element;

use crate::color::CssColor;
use crate::model::{ImageRef, OuterShadow, Paragraph, Shape, TabStop, TextBody};
use crate::source::{int_attr, pint, ta};
use crate::theme::{Theme, color_from_dml, text_fill_color};
use uo_opc::xml::Element as El;

/// `BlipResolver` (`text-style.ts:5`): the `<a:blip>` of a picture bullet → the image, through the
/// rels of the part the style lives in.
pub type BlipResolver<'a> = &'a dyn for<'d> Fn(Element<'d>) -> Option<ImageRef>;

/// One level (`lvlNpPr` + `defRPr`) of a text style (`TextStyleLevel`, `text-style.ts:40`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TextStyleLevel {
    pub tabs: Option<Box<[TabStop]>>,
    pub def_tab_sz: Option<f64>,
    pub sz: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub color: Option<CssColor>,
    /// May be a theme alias (`+mj-lt`, `+mn-lt`), resolved when applied.
    pub font: Option<Box<str>>,
    pub align: Option<Box<str>>,
    pub bu_char: Option<Box<str>>,
    pub bu_auto_num: Option<Box<str>>,
    pub bu_none: Option<bool>,
    pub mar_l: Option<f64>,
    pub indent: Option<f64>,
    pub shadow: Option<OuterShadow>,
    pub line_spacing: Option<f64>,
    pub line_spacing_pt: Option<f64>,
    pub space_before_pt: Option<f64>,
    pub space_after_pt: Option<f64>,
    pub space_before_pct: Option<f64>,
    pub space_after_pct: Option<f64>,
    pub bu_font: Option<Box<str>>,
    pub bu_clr: Option<CssColor>,
    pub bu_sz_pct: Option<f64>,
    pub bu_sz_pt: Option<f64>,
    /// `<a:buBlip>`: the image (the master's pictures resolve through the master's rels).
    pub bu_blip: Option<Box<ImageRef>>,
    pub baseline: Option<f64>,
    pub cap: Option<Box<str>>,
    pub strike: Option<bool>,
    pub spc: Option<f64>,
    pub kern: Option<f64>,
}

/// Nine levels, index 0 = `lvl1pPr` (`TextStyleLevels`).
pub type StyleLevels = [TextStyleLevel; 9];

/// Nine empty levels (`emptyLevels`, read.ts:602).
pub fn empty_levels() -> Box<StyleLevels> {
    Box::new(core::array::from_fn(|_| TextStyleLevel::default()))
}

/// A placeholder map keyed by the TS **strings** (`phKey`: `title#3`, `body`, `idx:3`; read.ts:569):
/// collisions such as a `type="body#3"` are behaviour, so the key is not a struct. Ordered, with the
/// write semantics of each TS site applied by the caller (overwrite in place, or set if absent).
#[derive(Clone, PartialEq, Debug)]
pub struct PhMap<V> {
    pub entries: Vec<(Box<str>, V)>,
}

impl<V> Default for PhMap<V> {
    fn default() -> Self {
        PhMap { entries: Vec::new() }
    }
}

impl<V> PhMap<V> {
    /// `map.get(key)`.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| &**k == key).map(|(_, v)| v)
    }

    /// `map.has(key)`.
    pub fn has(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| &**k == key)
    }

    /// `map.set(key, v)`: an existing key keeps its position.
    pub fn set(&mut self, key: &str, v: V) {
        match self.entries.iter_mut().find(|(k, _)| &**k == key) {
            Some(e) => e.1 = v,
            None => self.entries.push((key.into(), v)),
        }
    }
}

/// What the cascade reads (`TextStyleContext`, `text-style.ts:322`).
#[derive(Clone, PartialEq, Debug)]
pub struct TextStyleContext {
    /// `<p:defaultTextStyle>` (PowerPoint) or the Excel default (theme minor font, 11 pt).
    pub default_style: Box<StyleLevels>,
    pub title_style: Box<StyleLevels>,
    pub body_style: Box<StyleLevels>,
    pub other_style: Box<StyleLevels>,
    /// `lstStyle` of the master's placeholders by `phKey`.
    pub master_ph: PhMap<Arc<StyleLevels>>,
    /// `lstStyle` of the layout's placeholders by `phKey`.
    pub layout_ph: PhMap<Arc<StyleLevels>>,
}

impl Default for TextStyleContext {
    fn default() -> Self {
        TextStyleContext {
            default_style: empty_levels(),
            title_style: empty_levels(),
            body_style: empty_levels(),
            other_style: empty_levels(),
            master_ph: PhMap::default(),
            layout_ph: PhMap::default(),
        }
    }
}

/// baseline/cap/strike/spc/kern of an `rPr` (or `defRPr`) — what `readRunExtras` writes into a run
/// or a level.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RunExtraProps {
    pub baseline: Option<f64>,
    pub cap: Option<Box<str>>,
    pub strike: Option<bool>,
    pub spc: Option<f64>,
    pub kern: Option<f64>,
}

/// `readRunExtras` (`text-style.ts:27`).
pub fn read_run_extras(rpr: El<'_>) -> RunExtraProps {
    let mut out = RunExtraProps::default();
    if let Some(bl) = rpr.attr("baseline") {
        out.baseline = Some(pint(bl) / 100000.0);
    }
    if let Some(cap @ ("all" | "small" | "none")) = rpr.attr("cap") {
        out.cap = Some(cap.into());
    }
    if let Some(st) = rpr.attr("strike") {
        out.strike = Some(st == "sngStrike" || st == "dblStrike");
    }
    if let Some(spc) = rpr.attr("spc") {
        out.spc = Some(pint(spc) / 100.0);
    }
    if let Some(kern) = rpr.attr("kern").filter(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit())) {
        out.kern = Some(pint(kern) / 100.0);
    }
    out
}

const LEVEL_NAMES: [&str; 9] =
    ["a:lvl1pPr", "a:lvl2pPr", "a:lvl3pPr", "a:lvl4pPr", "a:lvl5pPr", "a:lvl6pPr", "a:lvl7pPr", "a:lvl8pPr", "a:lvl9pPr"];

/// `{...base, ...over}`: the defined fields of `over` replace those of `base`.
fn spread(base: &TextStyleLevel, over: TextStyleLevel) -> TextStyleLevel {
    macro_rules! pick {
        ($($f:ident),*) => { TextStyleLevel { $($f: over.$f.or_else(|| base.$f.clone())),* } };
    }
    pick!(
        tabs,
        def_tab_sz,
        sz,
        bold,
        italic,
        color,
        font,
        align,
        bu_char,
        bu_auto_num,
        bu_none,
        mar_l,
        indent,
        shadow,
        line_spacing,
        line_spacing_pt,
        space_before_pt,
        space_after_pt,
        space_before_pct,
        space_after_pct,
        bu_font,
        bu_clr,
        bu_sz_pct,
        bu_sz_pt,
        bu_blip,
        baseline,
        cap,
        strike,
        spc,
        kern
    )
}

/// `parseTxStyleLevels` (`text-style.ts:102`): a levels container (`<a:lstStyle>`, `<p:titleStyle>`,
/// `<p:bodyStyle>`, `<p:otherStyle>`, `<p:defaultTextStyle>`) → 9 levels. `<a:defPPr>` is the base
/// of every level.
pub fn parse_tx_style_levels(el: Option<El<'_>>, theme: &Theme, blip: Option<BlipResolver<'_>>) -> Box<StyleLevels> {
    let mut levels = empty_levels();
    let Some(el) = el else { return levels };
    let base = el.first_child("a:defPPr").map(|d| parse_ppr_level(d, theme, blip)).unwrap_or_default();
    for (i, name) in LEVEL_NAMES.iter().enumerate() {
        let lvl = match el.first_child(name) {
            Some(l) => spread(&base, parse_ppr_level(l, theme, blip)),
            None => base.clone(),
        };
        if let Some(slot) = levels.get_mut(i) {
            *slot = lvl;
        }
    }
    levels
}

/// The `<a:tabLst>` of a `pPr`, sorted by position (`.sort((a, b) => a.pos - b.pos)`).
pub fn read_tabs(tl: El<'_>) -> Box<[TabStop]> {
    let mut v: Vec<TabStop> = tl
        .children_named("a:tab")
        .map(|t| TabStop { pos: t.attr("pos").map_or(0.0, pint), algn: t.attr("algn").unwrap_or("l").into() })
        .collect();
    js_sort_by(&mut v, |a, b| a.pos - b.pos);
    v.into_boxed_slice()
}

/// The text shadow of a `defRPr`/`rPr` (`<a:effectLst><a:outerShdw>`), with the default alpha of
/// the caller (0.6 in a style level, 1 in a run).
pub fn read_text_shadow(shd: El<'_>, theme: &Theme, default_alpha: f64) -> OuterShadow {
    let color_el = shd.child_elements().next();
    let mut color = CssColor::new("#000000");
    let mut alpha = default_alpha;
    if let Some(ce) = color_el {
        if let Some(c) = color_from_dml(Some(ce), theme) {
            color = c;
        }
        if let Some(a) = ce.last_child("a:alpha").and_then(|a| ta(a, "val")) {
            alpha = pint(a) / 100000.0;
        }
    }
    OuterShadow {
        color,
        blur_rad: ta(shd, "blurRad").map_or(0.0, pint),
        dist: ta(shd, "dist").map_or(0.0, pint),
        dir_deg: ta(shd, "dir").map_or(5400000.0, pint) / 60000.0,
        alpha,
        sx: None,
        sy: None,
        algn: None,
    }
}

/// `parsePPrLevel` (`text-style.ts:119`): ONE level (`a:lvlNpPr` / `a:defPPr` / `a:pPr`).
pub fn parse_ppr_level(ppr: El<'_>, theme: &Theme, blip: Option<BlipResolver<'_>>) -> TextStyleLevel {
    let mut lvl = TextStyleLevel::default();
    if let Some(a) = ta(ppr, "algn") {
        lvl.align = Some(a.into());
    }
    lvl.mar_l = int_attr(ppr, "marL");
    lvl.indent = int_attr(ppr, "indent");
    lvl.def_tab_sz = int_attr(ppr, "defTabSz");
    if let Some(tl) = ppr.first_child("a:tabLst") {
        lvl.tabs = Some(read_tabs(tl));
    }
    // inheritable line and paragraph spacing (a compact template title = 75%)
    if let Some(ln_spc) = ppr.first_child("a:lnSpc") {
        let pct = ln_spc.first_child("a:spcPct").and_then(|e| ta(e, "val"));
        let pts = ln_spc.first_child("a:spcPts").and_then(|e| ta(e, "val"));
        if let Some(v) = pct {
            lvl.line_spacing = Some(pint(v) / 100000.0);
        } else if let Some(v) = pts {
            lvl.line_spacing_pt = Some(pint(v) / 100.0);
        }
    }
    let spc = |name: &str, pts_name: &str, pct_name: &str| -> (Option<f64>, Option<f64>) {
        let _ = (pts_name, pct_name);
        let Some(el) = ppr.first_child(name) else { return (None, None) };
        let pts = el.first_child("a:spcPts").and_then(|e| ta(e, "val")).map(|v| pint(v) / 100.0);
        let pct = el.first_child("a:spcPct").and_then(|e| ta(e, "val")).map(|v| pint(v) / 100000.0);
        (pts, pct)
    };
    (lvl.space_before_pt, lvl.space_before_pct) = spc("a:spcBef", "", "");
    (lvl.space_after_pt, lvl.space_after_pct) = spc("a:spcAft", "", "");
    if let Some(c) = ppr.first_child("a:buChar").and_then(|e| ta(e, "char")) {
        lvl.bu_char = Some(c.into());
    }
    if let Some(b) = ppr.first_child("a:buAutoNum") {
        lvl.bu_auto_num = Some(b.attr("type").unwrap_or("arabicPeriod").into());
    }
    if ppr.first_child("a:buNone").is_some() {
        lvl.bu_none = Some(true);
    }
    if let (Some(bb), Some(res)) = (ppr.first_child("a:buBlip").and_then(|e| e.first_child("a:blip")), blip) {
        lvl.bu_blip = res(bb).map(Box::new);
    }
    if let Some(f) = ppr.first_child("a:buFont").and_then(|e| ta(e, "typeface")) {
        lvl.bu_font = Some(f.into());
    }
    if let Some(bc) = ppr.first_child("a:buClr") {
        lvl.bu_clr = color_from_dml(bc.child_elements().next(), theme);
    }
    let pct = ppr.first_child("a:buSzPct").and_then(|e| ta(e, "val"));
    let pts = ppr.first_child("a:buSzPts").and_then(|e| ta(e, "val"));
    if let Some(v) = pct {
        lvl.bu_sz_pct = Some(pint(v) / 100000.0);
    } else if let Some(v) = pts {
        lvl.bu_sz_pt = Some(pint(v) / 100.0);
    }
    if let Some(def) = ppr.first_child("a:defRPr") {
        let ex = read_run_extras(def);
        lvl.baseline = ex.baseline;
        lvl.cap = ex.cap;
        lvl.strike = ex.strike;
        lvl.spc = ex.spc;
        lvl.kern = ex.kern;
        if let Some(sz) = ta(def, "sz") {
            lvl.sz = Some(pint(sz) / 100.0);
        }
        if let Some(b) = def.attr("b") {
            lvl.bold = Some(b == "1");
        }
        if let Some(i) = def.attr("i") {
            lvl.italic = Some(i == "1");
        }
        if let Some(tc) = text_fill_color(def, theme) {
            lvl.color = Some(tc);
        }
        if let Some(f) = def.first_child("a:latin").and_then(|e| ta(e, "typeface")) {
            lvl.font = Some(f.into());
        }
        // default text shadow of the level (template titles over a photo)
        if let Some(shd) = def.first_child("a:effectLst").and_then(|e| e.first_child("a:outerShdw")) {
            lvl.shadow = Some(read_text_shadow(shd, theme, 0.6));
        }
    }
    lvl
}

/// `mergeStyleLevels` (`text-style.ts:201`): later layers win (defined props overwrite); a bullet is
/// exclusive: a layer defining buChar/buAutoNum/buNone/buBlip replaces the earlier layer's whole bullet.
pub fn merge_style_levels(layers: &[&StyleLevels]) -> Box<StyleLevels> {
    let mut out = empty_levels();
    for layer in layers {
        for (i, src) in layer.iter().enumerate() {
            let Some(dst) = out.get_mut(i) else { continue };
            if src.sz.is_some() {
                dst.sz = src.sz;
            }
            if src.bold.is_some() {
                dst.bold = src.bold;
            }
            if src.italic.is_some() {
                dst.italic = src.italic;
            }
            if src.color.is_some() {
                dst.color = src.color.clone();
            }
            if src.font.is_some() {
                dst.font = src.font.clone();
            }
            if src.align.is_some() {
                dst.align = src.align.clone();
            }
            if src.shadow.is_some() {
                dst.shadow = src.shadow.clone();
            }
            if src.line_spacing.is_some() {
                dst.line_spacing = src.line_spacing;
                dst.line_spacing_pt = None;
            }
            if src.line_spacing_pt.is_some() {
                dst.line_spacing_pt = src.line_spacing_pt;
                dst.line_spacing = None;
            }
            // points × percent: the more specific level wins (one erases the other)
            if src.space_before_pt.is_some() {
                dst.space_before_pt = src.space_before_pt;
                dst.space_before_pct = None;
            }
            if src.space_before_pct.is_some() {
                dst.space_before_pct = src.space_before_pct;
                dst.space_before_pt = None;
            }
            if src.space_after_pt.is_some() {
                dst.space_after_pt = src.space_after_pt;
                dst.space_after_pct = None;
            }
            if src.space_after_pct.is_some() {
                dst.space_after_pct = src.space_after_pct;
                dst.space_after_pt = None;
            }
            if src.mar_l.is_some() {
                dst.mar_l = src.mar_l;
            }
            if src.tabs.is_some() {
                dst.tabs = src.tabs.clone();
            }
            if src.def_tab_sz.is_some() {
                dst.def_tab_sz = src.def_tab_sz;
            }
            if src.indent.is_some() {
                dst.indent = src.indent;
            }
            if src.bu_font.is_some() {
                dst.bu_font = src.bu_font.clone();
            }
            if src.bu_clr.is_some() {
                dst.bu_clr = src.bu_clr.clone();
            }
            if src.bu_sz_pct.is_some() {
                dst.bu_sz_pct = src.bu_sz_pct;
                dst.bu_sz_pt = None;
            }
            if src.bu_sz_pt.is_some() {
                dst.bu_sz_pt = src.bu_sz_pt;
                dst.bu_sz_pct = None;
            }
            if src.baseline.is_some() {
                dst.baseline = src.baseline;
            }
            if src.cap.is_some() {
                dst.cap = src.cap.clone();
            }
            if src.strike.is_some() {
                dst.strike = src.strike;
            }
            if src.spc.is_some() {
                dst.spc = src.spc;
            }
            if src.kern.is_some() {
                dst.kern = src.kern;
            }
            if src.bu_char.is_some() || src.bu_auto_num.is_some() || src.bu_none == Some(true) || src.bu_blip.is_some() {
                dst.bu_char = None;
                dst.bu_auto_num = None;
                dst.bu_none = None;
                dst.bu_blip = None;
                if src.bu_blip.is_some() {
                    dst.bu_blip = src.bu_blip.clone();
                }
                if src.bu_char.is_some() {
                    dst.bu_char = src.bu_char.clone();
                }
                if src.bu_auto_num.is_some() {
                    dst.bu_auto_num = src.bu_auto_num.clone();
                }
                if src.bu_none == Some(true) {
                    dst.bu_none = Some(true);
                }
            }
        }
    }
    out
}

/// `resolveFontAlias` (`text-style.ts:248`): `+mj…` → the theme's major face, `+mn…` → minor; any
/// other name is itself.
pub fn resolve_font_alias(font: &str, theme: &Theme) -> Option<Box<str>> {
    if font.starts_with("+mj") {
        theme.fonts.as_ref().and_then(|f| f.major.clone())
    } else if font.starts_with("+mn") {
        theme.fonts.as_ref().and_then(|f| f.minor.clone())
    } else {
        Some(font.into())
    }
}

/// `applyTextStyles` (`text-style.ts:259`): applies the effective levels to a body, filling ONLY what
/// is absent (direct XML properties always win). Mutates in place.
pub fn apply_text_styles(tb: &mut TextBody, levels: &StyleLevels, theme: &Theme) {
    let empty = TextStyleLevel::default();
    for p in &mut tb.paragraphs {
        let lvl = level_at(levels, p.level).unwrap_or(&empty);
        apply_paragraph(p, lvl, theme);
    }
}

fn level_at(levels: &StyleLevels, level: Option<f64>) -> Option<&TextStyleLevel> {
    // `levels[Math.min(p.level ?? 0, 8)]`: NaN and negative levels select nothing
    let i = math::min(level.unwrap_or(0.0), 8.0);
    if i.is_nan() || i < 0.0 { None } else { levels.get(uo_jsnum::conv::js_index(i, 9)?) }
}

fn apply_paragraph(p: &mut Paragraph, lvl: &TextStyleLevel, theme: &Theme) {
    if p.align.is_none() && lvl.align.is_some() {
        p.align = lvl.align.clone();
    }
    if p.line_spacing.is_none() && p.line_spacing_pt.is_none() {
        if lvl.line_spacing.is_some() {
            p.line_spacing = lvl.line_spacing;
        } else if lvl.line_spacing_pt.is_some() {
            p.line_spacing_pt = lvl.line_spacing_pt;
        }
    }
    if p.space_before_pt.is_none() && p.space_before_pct.is_none() {
        if lvl.space_before_pt.is_some() {
            p.space_before_pt = lvl.space_before_pt;
        } else if lvl.space_before_pct.is_some() {
            p.space_before_pct = lvl.space_before_pct;
        }
    }
    if p.space_after_pt.is_none() && p.space_after_pct.is_none() {
        if lvl.space_after_pt.is_some() {
            p.space_after_pt = lvl.space_after_pt;
        } else if lvl.space_after_pct.is_some() {
            p.space_after_pct = lvl.space_after_pct;
        }
    }
    if p.mar_l.is_none() && lvl.mar_l.is_some() {
        p.mar_l = lvl.mar_l;
    }
    if p.tabs.is_none() && lvl.tabs.is_some() {
        p.tabs = lvl.tabs.clone();
    }
    if p.def_tab_sz.is_none() && lvl.def_tab_sz.is_some() {
        p.def_tab_sz = lvl.def_tab_sz;
    }
    if p.indent.is_none() && lvl.indent.is_some() {
        p.indent = lvl.indent;
    }
    if p.bullet_font.is_none() && lvl.bu_font.is_some() {
        if let Some(f) = lvl.bu_font.as_deref() {
            p.bullet_font = Some(resolve_font_alias(f, theme).unwrap_or_else(|| f.into()));
        }
    } else if let Some(bf) = p.bullet_font.as_deref().filter(|b| b.starts_with('+')) {
        p.bullet_font = Some(resolve_font_alias(bf, theme).unwrap_or_else(|| bf.into()));
    }
    if p.bullet_color.is_none() && lvl.bu_clr.is_some() {
        p.bullet_color = lvl.bu_clr.clone();
    }
    if p.bullet_size_pct.is_none() && p.bullet_size_pt.is_none() {
        if lvl.bu_sz_pct.is_some() {
            p.bullet_size_pct = lvl.bu_sz_pct;
        } else if lvl.bu_sz_pt.is_some() {
            p.bullet_size_pt = lvl.bu_sz_pt;
        }
    }
    if p.end_sz.is_none() && lvl.sz.is_some() {
        p.end_sz = lvl.sz;
    }
    // bullet: inherited only when the pPr declared nothing (bullet === undefined)
    match p.bullet {
        None => {
            if lvl.bu_blip.is_some() {
                p.bullet = Some(true);
                p.bullet_image = lvl.bu_blip.clone();
                p.bullet_char = Some(lvl.bu_char.clone().unwrap_or_else(|| "\u{2022}".into()));
            } else if lvl.bu_char.is_some() {
                p.bullet = Some(true);
                p.bullet_char = lvl.bu_char.clone();
            } else if lvl.bu_auto_num.is_some() {
                p.bullet = Some(true);
                p.bullet_auto_num = lvl.bu_auto_num.clone();
            } else if lvl.bu_none == Some(true) {
                p.bullet = Some(false);
            }
        }
        Some(true) if p.bullet_char.is_none() && p.bullet_auto_num.is_none() => {
            // the pPr had buChar/buAutoNum without a usable char/type: complete from the cascade
            if lvl.bu_char.is_some() {
                p.bullet_char = lvl.bu_char.clone();
            } else if lvl.bu_auto_num.is_some() {
                p.bullet_auto_num = lvl.bu_auto_num.clone();
            }
        }
        Some(_) => {}
    }
    for r in &mut p.runs {
        if r.sz.is_none() && lvl.sz.is_some() {
            r.sz = lvl.sz;
        }
        if r.bold.is_none() && lvl.bold.is_some() {
            r.bold = lvl.bold;
        }
        if r.italic.is_none() && lvl.italic.is_some() {
            r.italic = lvl.italic;
        }
        if r.color.is_none() && lvl.color.is_some() {
            r.color = lvl.color.clone();
        }
        let ex_shadow = r.extras.as_ref().is_some_and(|e| e.shadow.is_some());
        let ex_baseline = r.extras.as_ref().is_some_and(|e| e.baseline.is_some());
        let ex_cap = r.extras.as_ref().is_some_and(|e| e.cap.is_some());
        let ex_strike = r.extras.as_ref().is_some_and(|e| e.strike.is_some());
        let ex_spc = r.extras.as_ref().is_some_and(|e| e.spc.is_some());
        let ex_kern = r.extras.as_ref().is_some_and(|e| e.kern.is_some());
        if (!ex_shadow && lvl.shadow.is_some())
            || (!ex_baseline && lvl.baseline.is_some())
            || (!ex_cap && lvl.cap.is_some())
            || (!ex_strike && lvl.strike.is_some())
            || (!ex_spc && lvl.spc.is_some())
            || (!ex_kern && lvl.kern.is_some())
        {
            let ex = r.extras.get_or_insert_with(Default::default);
            if !ex_shadow && lvl.shadow.is_some() {
                ex.shadow = lvl.shadow.clone();
            }
            if !ex_baseline && lvl.baseline.is_some() {
                ex.baseline = lvl.baseline;
            }
            if !ex_cap && lvl.cap.is_some() {
                ex.cap = lvl.cap.clone();
            }
            if !ex_strike && lvl.strike.is_some() {
                ex.strike = lvl.strike;
            }
            if !ex_spc && lvl.spc.is_some() {
                ex.spc = lvl.spc;
            }
            if !ex_kern && lvl.kern.is_some() {
                ex.kern = lvl.kern;
            }
        }
        if r.font.is_none() && lvl.font.is_some() {
            if let Some(f) = lvl.font.as_deref().and_then(|f| resolve_font_alias(f, theme)) {
                r.font = Some(f);
            }
        } else if let Some(rf) = r.font.as_deref().filter(|f| f.starts_with('+')) {
            // a run with a direct alias ("+mn-lt") resolves too
            if let Some(f) = resolve_font_alias(rf, theme) {
                r.font = Some(f);
            }
        }
    }
}

/// `masterStyleFor` (`text-style.ts:336`): the master style by the placeholder's family.
pub fn master_style_for<'c>(ctx: &'c TextStyleContext, ph_type: Option<&str>) -> &'c StyleLevels {
    match ph_type {
        Some("title" | "ctrTitle") => &ctx.title_style,
        Some("body" | "subTitle") => &ctx.body_style,
        _ => &ctx.other_style,
    }
}

/// `applyShapeTextCascade` (`text-style.ts:347`): the complete cascade on a shape (item A plus the
/// fontRef of item B). `ph_keys` are the candidate keys of the placeholder in preference order (the
/// first present in the map wins); `master_keys` the same for the master (by type only).
pub fn apply_shape_text_cascade(sh: &mut Shape, ctx: &TextStyleContext, theme: &Theme, ph_keys: &[String], master_keys: &[String]) {
    let Some(tb) = sh.tx_body.as_deref_mut() else { return };
    // EXCLUSIVE layers (measured): a non-placeholder shape uses only the presentation's
    // defaultTextStyle; a placeholder only the master's style
    let is_ph = sh.ph_type.is_some() || sh.ph_idx.is_some();
    let mut layers: Vec<&StyleLevels> = vec![if is_ph { master_style_for(ctx, sh.ph_type.as_deref()) } else { &ctx.default_style }];
    // fontRef of `<p:style>`: colour/font default of the themed shape — above the master txStyles,
    // below the placeholder lstStyles
    let font_layer;
    if sh.style_font_color.is_some() || sh.style_font_idx.is_some() {
        let font = match sh.style_font_idx.as_deref() {
            Some("major") => theme.fonts.as_ref().and_then(|f| f.major.clone()),
            Some("minor") => theme.fonts.as_ref().and_then(|f| f.minor.clone()),
            _ => None,
        };
        let lvl = TextStyleLevel { color: sh.style_font_color.clone(), font, ..TextStyleLevel::default() };
        font_layer = Box::new(core::array::from_fn::<TextStyleLevel, 9, _>(|_| lvl.clone()));
        layers.push(&font_layer);
    }
    // the layout matches by idx/type; the MASTER only by type (the master's idx is another numbering)
    for (map, keys) in [(&ctx.master_ph, master_keys), (&ctx.layout_ph, ph_keys)] {
        for key in keys {
            if let Some(ls) = map.get(key) {
                layers.push(ls);
                break;
            }
        }
    }
    if let Some(ls) = tb.lst_style.as_deref() {
        layers.push(ls);
    }
    let merged = merge_style_levels(&layers);
    apply_text_styles(tb, &merged, theme);
}
