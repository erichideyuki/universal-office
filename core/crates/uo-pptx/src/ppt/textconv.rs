//! Binary text → DrawingML (`<a:p>`/`<a:r>`), with ALL the effective properties written
//! explicitly — the result does not depend on the style cascade of the reader (`ppt/textconv.ts`).
//!
//! The cascade of the binary PowerPoint, from the lowest to the highest priority:
//! 1. the document defaults (`TextCF/PFExceptionAtom` of the Environment);
//! 2. the master style of the text TYPE (`TextMasterStyleAtom` of the master), chained levels: level
//!    n inherits from n−1 (the atoms only carry differences);
//! 3. derived styles (centered body/title, half/quarter body: instances 5–8) over the base type;
//! 4. the block's ruler (`TextRulerAtom`: margins per level);
//! 5. the run exceptions (`StyleTextPropAtom`) + PP9 (numbering).
//!
//! Units: margins in master units (576/inch → EMU × 1587.5); line spacing and spaces ≥ 0 = %,
//! < 0 = −master units (8 per point).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use uo_jsnum::math::round;

use super::rd::rt;
use super::reader::{MasterStyles, PptDocument};
use super::text::{ColorIndex, MasterStyleLevel, TextBlock, TextCF, TextPF};
use super::xmlw::{Attrs, attrs, el, el_a, esc_text, esc_text_units, n, s};

pub const MU_TO_EMU: f64 = 1587.5;

/// `emu(mu)`: master units to EMU.
#[must_use]
pub fn emu(mu: f64) -> f64 {
    round(mu * MU_TO_EMU)
}

/// The header/footer texts of the master's placeholders (`TextConvCtx.hf`).
#[derive(Debug, Clone, Default)]
pub struct HfText {
    pub footer: Option<String>,
    pub header: Option<String>,
    pub user_date: Option<String>,
}

/// What a part lends to the text conversion: the relation of a hyperlink and the one of a picture
/// bullet (`TextConvCtx.linkRel`, `bulletBlipRel`).
pub trait TextHost {
    /// A relation for an external hyperlink target → rId.
    fn link_rel(&mut self, target: &str) -> String;
    /// The relation of the picture bullet `bulletBlipRef` → rId, when there is such a picture.
    fn bullet_blip_rel(&mut self, bullet_ref: i32) -> Option<String>;
}

/// The text context of a part (`TextConvCtx`).
pub struct TextConvCtx<'a> {
    pub doc: &'a PptDocument<'a>,
    /// the master styles (by instance) of the slide's main master
    pub styles: Option<Rc<MasterStyles>>,
    /// the effective colour scheme (8 colours 'RRGGBB')
    pub scheme: Vec<String>,
    /// the slide number (slide number fields)
    pub slide_number: Option<f64>,
    /// header/footer texts (meta characters of the master's placeholders)
    pub hf: Option<HfText>,
    /// today, for the automatic date field: year, month (1..12), day
    pub today: (i32, u32, u32),
    /// the counter of `a:fld` GUIDs of the conversion (`fieldSeq`, shared by every part)
    pub field_seq: Rc<Cell<u32>>,
    cache: RefCell<HashMap<(u32, i32), Rc<LevelStyle>>>,
}

impl<'a> TextConvCtx<'a> {
    #[must_use]
    pub fn new(doc: &'a PptDocument<'a>, scheme: Vec<String>, today: (i32, u32, u32), field_seq: Rc<Cell<u32>>) -> Self {
        TextConvCtx { doc, styles: None, scheme, slide_number: None, hf: None, today, field_seq, cache: RefCell::new(HashMap::new()) }
    }
}

/// The effective style of a level.
#[derive(Debug, Clone, Default)]
pub struct LevelStyle {
    pub pf: TextPF,
    pub cf: TextCF,
}

const JUMPS: [&str; 7] = ["", "nextslide", "previousslide", "firstslide", "lastslide", "lastslideviewed", "endshow"];

/// The base type of the derived styles (instances 5–8).
fn base_type(t: u32) -> u32 {
    match t {
        6 => 0,
        5 | 7 | 8 => 1,
        other => other,
    }
}

/// Level `lvl` of a list of styles (instances ≥ 5 carry the level number).
fn level_of(levels: Option<&Vec<MasterStyleLevel>>, lvl: usize) -> Option<&MasterStyleLevel> {
    let levels = levels?;
    if levels.iter().any(|l| l.level.is_some()) {
        return levels.iter().find(|l| l.level.map(usize::from) == Some(lvl));
    }
    levels.get(lvl)
}

fn style_of(styles: Option<&Rc<MasterStyles>>, key: u32) -> Option<&Vec<MasterStyleLevel>> {
    u16::try_from(key).ok().and_then(|k| styles?.get(&k))
}

/// The effective style of a text type at a level (0–4; levels above 4 use 4).
#[must_use]
pub fn resolve_level(ctx: &TextConvCtx<'_>, text_type: u32, level: i32) -> Rc<LevelStyle> {
    let lvl = level.clamp(0, 4);
    let key = (text_type, lvl);
    if let Some(hit) = ctx.cache.borrow().get(&key) {
        return Rc::clone(hit);
    }
    let mut pf = ctx.doc.doc_pf.clone();
    let mut cf = ctx.doc.doc_cf.clone();
    let base = base_type(text_type);
    // "other" text (text boxes/autoshapes): the style of the DOCUMENT (Environment — it becomes
    // the defaultTextStyle; the application does NOT use the master's "other" for that text,
    // measured); the other types: the master's, with the master's "other" and finally the
    // document's as fallbacks
    let base_levels = if base == 4 {
        ctx.doc.doc_other_style.as_ref().or_else(|| style_of(ctx.styles.as_ref(), 4))
    } else {
        style_of(ctx.styles.as_ref(), base).or_else(|| style_of(ctx.styles.as_ref(), 4)).or(ctx.doc.doc_other_style.as_ref())
    };
    let derived = if base == text_type { None } else { style_of(ctx.styles.as_ref(), text_type) };
    let top = usize::try_from(lvl).unwrap_or(0);
    for l in 0..=top {
        if let Some(b) = level_of(base_levels, l) {
            pf.assign(&b.pf);
            cf.assign(&b.cf);
        }
    }
    for l in 0..=top {
        if let Some(d) = level_of(derived, l) {
            pf.assign(&d.pf);
            cf.assign(&d.cf);
        }
    }
    let out = Rc::new(LevelStyle { pf, cf });
    ctx.cache.borrow_mut().insert(key, Rc::clone(&out));
    out
}

/// Text/bullet colour → 'RRGGBB' (index 0xFE = RGB; 0..7 = scheme).
#[must_use]
pub fn color_index_hex(ci: Option<&ColorIndex>, scheme: &[String]) -> Option<String> {
    let ci = ci?;
    if ci.index == 0xfe {
        return Some(hex3(f64::from(ci.r), f64::from(ci.g), f64::from(ci.b)));
    }
    if ci.index <= 7 {
        return scheme.get(usize::from(ci.index)).cloned();
    }
    None
}

/// `hex3(r, g, b)`: three components (0..255) as 'RRGGBB' upper case.
#[must_use]
pub fn hex3(r: f64, g: f64, b: f64) -> String {
    [r, g, b].iter().map(|v| to_hex2(*v)).collect()
}

/// `v.toString(16).padStart(2, '0').toUpperCase()` of an integer in 0..255.
fn to_hex2(v: f64) -> String {
    let x = uo_jsnum::conv::to_int32(v);
    format!("{x:02X}")
}

const ALIGN: [&str; 7] = ["l", "ctr", "r", "just", "dist", "thaiDist", "justLow"];

/// Automatic numbering schemes (`TextAutoNumberScheme`) → `ST_TextAutonumberScheme`.
const AUTONUM: [&str; 43] = [
    "alphaLcPeriod",
    "alphaUcPeriod",
    "arabicParenR",
    "arabicPeriod",
    "romanLcParenBoth",
    "romanLcParenR",
    "romanLcPeriod",
    "romanUcPeriod",
    "alphaLcParenBoth",
    "alphaLcParenR",
    "alphaUcParenBoth",
    "alphaUcParenR",
    "arabicParenBoth",
    "arabicPlain",
    "romanUcParenBoth",
    "romanUcParenR",
    "ea1ChsPlain",
    "ea1ChsPeriod",
    "circleNumDbPlain",
    "circleNumWdWhitePlain",
    "circleNumWdBlackPlain",
    "thaiAlphaPeriod",
    "thaiAlphaParenR",
    "thaiAlphaParenBoth",
    "thaiNumPeriod",
    "thaiNumParenR",
    "thaiNumParenBoth",
    "hindiAlphaPeriod",
    "hindiNumPeriod",
    "ea1JpnChsDbPeriod",
    "hindiNumParenR",
    "hindiAlpha1Period",
    "ea1ChtPlain",
    "ea1ChtPeriod",
    "arabicDbPlain",
    "arabicDbPeriod",
    "ea1JpnKorPlain",
    "ea1JpnKorPeriod",
    "arabic1Minus",
    "arabic2Minus",
    "hebrew2Minus",
    "thaiAlphaPeriod",
    "thaiAlphaParenR",
];

/// Spacing (line spacing / before / after): ≥ 0 → `spcPct`; < 0 → `spcPts`.
fn spacing_xml(tag: &str, v: Option<i32>) -> String {
    let Some(v) = v else { return String::new() };
    if v >= 0 {
        return el(tag, vec![], &el("a:spcPct", vec![("val", n(round(f64::from(v) * 1000.0)))], ""));
    }
    el(tag, vec![], &el("a:spcPts", vec![("val", n(round((-f64::from(v) / 8.0) * 100.0)))], ""))
}

fn font_name(ctx: &TextConvCtx<'_>, idx: Option<i32>) -> Option<String> {
    let idx = idx?;
    if idx == 0xffff {
        return None;
    }
    let f = ctx.doc.fonts.get(usize::try_from(idx).ok()?)?.as_ref()?;
    if f.name.is_empty() { None } else { Some(f.name.clone()) }
}

/// The bullet character: symbol fonts use the private area F0xx.
fn bullet_char_str(code: i32) -> String {
    let c = if (0xf000..=0xf0ff).contains(&code) { code - 0xf000 } else { code };
    let c = if c == 0 { 0x2022 } else { c };
    String::from_utf16_lossy(&[u16::try_from(c & 0xffff).unwrap_or(0)])
}

/// `<a:pPr>` (or `<a:lvlNpPr>`) with the effective paragraph properties.
pub fn p_pr_xml(tag: &str, pf: &TextPF, cf: &TextCF, ctx: &TextConvCtx<'_>, lvl: i32, host: &mut Option<&mut dyn TextHost>) -> String {
    let left = f64::from(pf.left_margin.unwrap_or(0));
    let ind = f64::from(pf.indent.unwrap_or(0));
    let mut a = Attrs::new();
    a.set("marL", n(emu(left)));
    a.set("lvl", if lvl != 0 { n(lvl) } else { None });
    a.set("indent", n(emu(ind - left)));
    let algn = usize::try_from(pf.align.unwrap_or(0)).ok().and_then(|i| ALIGN.get(i)).copied().unwrap_or("l");
    a.set("algn", s(algn));
    if let Some(d) = pf.default_tab_size {
        a.set("defTabSz", n(emu(f64::from(d))));
    }
    if pf.text_direction == Some(1) {
        a.set("rtl", s("1"));
    }
    if let Some(fa) = pf.font_align {
        let v = usize::try_from(fa).ok().and_then(|i| ["base", "t", "ctr", "b"].get(i).copied());
        a.set("fontAlgn", v.and_then(s));
    }
    let mut kids = String::new();
    kids.push_str(&spacing_xml("a:lnSpc", pf.line_spacing));
    kids.push_str(&spacing_xml("a:spcBef", pf.space_before));
    kids.push_str(&spacing_xml("a:spcAft", pf.space_after));
    if pf.has_bullet == Some(true) {
        if pf.bullet_has_color == Some(true)
            && let Some(c) = color_index_hex(pf.bullet_color.as_ref(), &ctx.scheme)
        {
            kids.push_str(&el("a:buClr", vec![], &el("a:srgbClr", vec![("val", s(c))], "")));
        }
        if pf.bullet_has_size == Some(true)
            && let Some(bs) = pf.bullet_size
            && bs != 0
        {
            if bs > 0 {
                kids.push_str(&el("a:buSzPct", vec![("val", n(round(f64::from(bs) * 1000.0)))], ""));
            } else {
                kids.push_str(&el("a:buSzPts", vec![("val", n(round(-f64::from(bs) * 100.0)))], ""));
            }
        }
        let bf = if pf.bullet_has_font == Some(true) { font_name(ctx, pf.bullet_font) } else { None };
        if let Some(bf) = bf {
            kids.push_str(&el("a:buFont", vec![("typeface", s(bf))], ""));
        }
        let blip_rid = match (pf.has_auto_num == Some(true), pf.bullet_blip_ref, host.as_mut()) {
            (false, Some(r), Some(h)) if r >= 0 => h.bullet_blip_rel(r),
            _ => None,
        };
        if let Some(rid) = blip_rid {
            kids.push_str(&el("a:buBlip", vec![], &el("a:blip", vec![("r:embed", s(rid))], "")));
        } else if pf.has_auto_num == Some(true) {
            let ty = usize::try_from(pf.auto_num_scheme.unwrap_or(3)).ok().and_then(|i| AUTONUM.get(i)).copied().unwrap_or("arabicPeriod");
            let start = pf.auto_num_start.filter(|&x| x > 1);
            kids.push_str(&el("a:buAutoNum", vec![("type", s(ty)), ("startAt", start.and_then(n))], ""));
        } else {
            kids.push_str(&el("a:buChar", vec![("char", s(bullet_char_str(pf.bullet_char.unwrap_or(0x2022))))], ""));
        }
    } else {
        kids.push_str("<a:buNone/>");
    }
    if let Some(ts) = &pf.tab_stops
        && !ts.is_empty()
    {
        let tabs: String = ts
            .iter()
            .map(|t| {
                let algn = ["l", "ctr", "r", "dec"].get(usize::from(t.type_)).copied().unwrap_or("l");
                el("a:tab", vec![("pos", n(emu(f64::from(t.pos)))), ("algn", s(algn))], "")
            })
            .collect();
        kids.push_str(&el("a:tabLst", vec![], &tabs));
    }
    if tag != "a:pPr" {
        kids.push_str(&r_pr_xml("a:defRPr", cf, ctx, None));
    }
    el_a(tag, &a, &kids)
}

/// LCIDs → language tag (BCP 47) for `<a:rPr lang>`.
fn lcid(id: u16) -> Option<&'static str> {
    Some(match id {
        0x0416 => "pt-BR",
        0x0816 => "pt-PT",
        0x0409 => "en-US",
        0x0809 => "en-GB",
        0x0c09 => "en-AU",
        0x1009 => "en-CA",
        0x0c0a => "es-ES",
        0x040a => "es-ES",
        0x080a => "es-MX",
        0x2c0a => "es-AR",
        0x040c => "fr-FR",
        0x0c0c => "fr-CA",
        0x0407 => "de-DE",
        0x0807 => "de-CH",
        0x0c07 => "de-AT",
        0x0410 => "it-IT",
        0x0413 => "nl-NL",
        0x0813 => "nl-BE",
        0x0419 => "ru-RU",
        0x0415 => "pl-PL",
        0x0405 => "cs-CZ",
        0x040e => "hu-HU",
        0x041d => "sv-SE",
        0x0406 => "da-DK",
        0x0414 => "nb-NO",
        0x040b => "fi-FI",
        0x0408 => "el-GR",
        0x041f => "tr-TR",
        0x0411 => "ja-JP",
        0x0412 => "ko-KR",
        0x0804 => "zh-CN",
        0x0404 => "zh-TW",
        0x040d => "he-IL",
        0x0401 => "ar-SA",
        0x041e => "th-TH",
        0x042a => "vi-VN",
        0x0418 => "ro-RO",
        0x0422 => "uk-UA",
        0x0402 => "bg-BG",
        0x041a => "hr-HR",
        0x041b => "sk-SK",
        0x0424 => "sl-SI",
        0x0403 => "ca-ES",
        0x0421 => "id-ID",
        0x0439 => "hi-IN",
        _ => return None,
    })
}

/// The hyperlink of a run (`<a:hlinkClick>`).
#[derive(Debug, Clone)]
pub struct RunLink {
    pub rid: String,
    pub action: Option<String>,
}

/// `<a:rPr>`/`<a:defRPr>`/`<a:endParaRPr>` with the effective properties.
#[must_use]
pub fn r_pr_xml(tag: &str, cf: &TextCF, ctx: &TextConvCtx<'_>, link: Option<&RunLink>) -> String {
    let mut a = Attrs::new();
    // the application writes kern="1200" in the styles (master and defaultTextStyle) when it
    // converts a .ppt: the text of a .ppt has kerning from 12 pt (oracle/ppt2pptx)
    let def = tag == "a:defRPr";
    if !def {
        a.set("lang", ctx.doc.doc_lcid.and_then(lcid).and_then(s));
    }
    a.set(
        "sz",
        match cf.size {
            Some(sz) if sz > 0 => n(round(f64::from(sz) * 100.0)),
            _ => None,
        },
    );
    a.set("b", s(if cf.bold == Some(true) { "1" } else { "0" }));
    a.set("i", s(if cf.italic == Some(true) { "1" } else { "0" }));
    a.set("u", if cf.underline == Some(true) { s("sng") } else { None });
    a.set(
        "baseline",
        match cf.position {
            Some(p) if p != 0 => n(round(f64::from(p) * 1000.0)),
            _ => None,
        },
    );
    if def {
        a.set("kern", n(1200));
    }
    let mut kids = String::new();
    let color = color_index_hex(cf.color.as_ref(), &ctx.scheme).or_else(|| ctx.scheme.get(1).cloned());
    if let Some(c) = color.filter(|c| !c.is_empty()) {
        kids.push_str(&el("a:solidFill", vec![], &el("a:srgbClr", vec![("val", s(c))], "")));
    }
    if cf.shadow == Some(true) {
        kids.push_str(
            "<a:effectLst><a:outerShdw blurRad=\"38100\" dist=\"38100\" dir=\"2700000\" algn=\"tl\"><a:srgbClr val=\"000000\"><a:alpha val=\"43137\"/></a:srgbClr></a:outerShdw></a:effectLst>",
        );
    }
    if let Some(latin) = font_name(ctx, cf.font) {
        kids.push_str(&el("a:latin", vec![("typeface", s(latin))], ""));
    }
    if let Some(ea) = font_name(ctx, cf.ea_font) {
        kids.push_str(&el("a:ea", vec![("typeface", s(ea))], ""));
    }
    if let Some(sym) = font_name(ctx, cf.symbol_font) {
        kids.push_str(&el("a:sym", vec![("typeface", s(sym))], ""));
    }
    if let Some(l) = link {
        kids.push_str(&el("a:hlinkClick", vec![("r:id", s(l.rid.clone())), ("action", l.action.as_deref().and_then(s))], ""));
    }
    el_a(tag, &a, &kids)
}

/// Links ordered by start, plus the boundaries (for binary search).
struct LinkIndex {
    links: Vec<super::text::TextLink>,
    starts: Vec<u64>,
    bounds: Vec<u64>,
}

fn link_index(block: &TextBlock) -> LinkIndex {
    let mut links: Vec<super::text::TextLink> = block.links.as_ref().clone();
    links.sort_by_key(|l| l.start);
    let mut bounds: Vec<u64> = links.iter().flat_map(|l| [u64::from(l.start), u64::from(l.end)]).collect();
    bounds.sort_unstable();
    bounds.dedup();
    let starts = links.iter().map(|l| u64::from(l.start)).collect();
    LinkIndex { links, starts, bounds }
}

/// First index `i` with `v < a[i]` (`a` non-decreasing); `a.len()` if none.
fn upper_bound(a: &[u64], v: u64) -> usize {
    a.partition_point(|&x| x <= v)
}

/// The hyperlink (or jump) of the range → [`RunLink`].
fn run_link_for(lk: &LinkIndex, pos: u64, ctx: &TextConvCtx<'_>, host: &mut Option<&mut dyn TextHost>) -> Option<RunLink> {
    // links ordered by start: candidates = the last ones with start ≤ pos (hyperlink ranges do
    // not overlap; it looks at a few so as not to scan everything)
    let hi = upper_bound(&lk.starts, pos);
    let mut l = None;
    let mut i = hi;
    while i > hi.saturating_sub(8) {
        i -= 1;
        let c = lk.links.get(i)?;
        if pos < u64::from(c.end) {
            l = Some(*c);
            break;
        }
    }
    let l = l?;
    if l.action == 4 {
        let target = ctx.doc.hyperlinks.get(&l.hyperlink_id).and_then(|h| h.target.as_deref()).filter(|t| !t.is_empty());
        if let (Some(t), Some(h)) = (target, host.as_mut()) {
            return Some(RunLink { rid: h.link_rel(t), action: None });
        }
        return None;
    }
    if l.action == 3
        && let Some(j) = JUMPS.get(usize::from(l.jump)).filter(|j| !j.is_empty())
    {
        return Some(RunLink { rid: String::new(), action: Some(format!("ppaction://hlinkshowjump?jump={j}")) });
    }
    None
}

/// The content a meta character stands for.
struct Field {
    fld: Option<&'static str>,
    text: String,
}

/// Meta character → field (`<a:fld type>`) or literal text: slide number and automatic date
/// become fields; fixed date, header and footer become the text of the HeadersFootersContainer.
fn meta_char_content(rec_type: u16, ctx: &TextConvCtx<'_>) -> Option<Field> {
    match rec_type {
        rt::SLIDE_NUMBER_META_CHAR_ATOM => {
            Some(Field { fld: Some("slidenum"), text: uo_jsnum::format::to_js_string(ctx.slide_number.unwrap_or(1.0)) })
        }
        rt::DATE_TIME_META_CHAR_ATOM | rt::GENERIC_DATE_META_CHAR_ATOM | rt::RTF_DATE_TIME_META_CHAR_ATOM => {
            if let Some(d) = ctx.hf.as_ref().and_then(|h| h.user_date.clone()) {
                return Some(Field { fld: None, text: d });
            }
            let (y, m, d) = ctx.today;
            Some(Field { fld: Some("datetime1"), text: format!("{d:02}/{m:02}/{y}") })
        }
        rt::FOOTER_META_CHAR_ATOM => Some(Field { fld: None, text: ctx.hf.as_ref().and_then(|h| h.footer.clone()).unwrap_or_default() }),
        rt::HEADER_META_CHAR_ATOM => Some(Field { fld: None, text: ctx.hf.as_ref().and_then(|h| h.header.clone()).unwrap_or_default() }),
        _ => None,
    }
}

/// `fieldId()`: the GUID of the next `a:fld`; the counter wraps at 0xFFFF and starts at 1 for
/// every conversion (the TS counter is module-global and never reset: the captures convert each
/// deck in a fresh module).
fn field_id(ctx: &TextConvCtx<'_>) -> String {
    let v = (ctx.field_seq.get() + 1) % 0xffff;
    ctx.field_seq.set(v);
    format!("{{B6F15528-21DE-4FAA-801E-634DDDAF{v:04X}}}")
}

/// Cumulative ends of the runs (binary search).
struct RunIndex<'r, T> {
    runs: &'r [T],
    ends: Vec<u64>,
}

fn run_index<'r, T>(runs: &'r [T], count: impl Fn(&T) -> u32) -> RunIndex<'r, T> {
    let mut ends = Vec::with_capacity(runs.len());
    let mut acc = 0u64;
    for r in runs {
        acc += u64::from(count(r));
        ends.push(acc);
    }
    RunIndex { runs, ends }
}

/// The run that covers `pos` (the last one, if `pos` passes the end).
fn run_at<'r, T>(ix: &RunIndex<'r, T>, pos: u64) -> Option<&'r T> {
    if ix.runs.is_empty() {
        return None;
    }
    let i = upper_bound(&ix.ends, pos).min(ix.runs.len() - 1);
    ix.runs.get(i)
}

/// Boundaries of the character runs (and of the hyperlinks) inside `[start, end)`.
fn cf_boundaries(cf_ends: &[u64], lk: &LinkIndex, start: u64, end: u64) -> Vec<u64> {
    let mut set: Vec<u64> = vec![start, end];
    for a in [cf_ends, lk.bounds.as_slice()] {
        let mut i = upper_bound(a, start);
        while let Some(&v) = a.get(i) {
            if v >= end {
                break;
            }
            set.push(v);
            i += 1;
        }
    }
    set.sort_unstable();
    set.dedup();
    set
}

/// A text block → the sequence of `<a:p>`.
pub fn paragraphs_xml(block: &TextBlock, ctx: &TextConvCtx<'_>, host: &mut Option<&mut dyn TextHost>) -> String {
    let text: &[u16] = &block.text;
    let mut paras: Vec<(u64, u64)> = Vec::new();
    let mut st = 0u64;
    for (i, &u) in text.iter().enumerate() {
        if u == 0x0d {
            let i = i as u64;
            paras.push((st, i));
            st = i + 1;
        }
    }
    paras.push((st, text.len() as u64));
    let mut fields: HashMap<u32, Field> = HashMap::new();
    for m in block.meta_chars.iter() {
        if let Some(c) = meta_char_content(m.rec_type, ctx) {
            fields.insert(m.pos, c);
        }
    }
    let pf_ix = run_index(&block.pf_runs, |r| r.count);
    let cf_ix = run_index(&block.cf_runs, |r| r.count);
    let lk = link_index(block);
    let mut out = String::new();
    for &(p_start, p_end) in &paras {
        let pf_run = run_at(&pf_ix, p_start);
        let level = pf_run.map_or(0, |r| i32::from(r.indent_level)).min(4);
        let base = resolve_level(ctx, block.text_type, level);
        let mut pf = base.pf.clone();
        // the block's ruler: margins of the level
        if let Some(ruler) = &block.ruler {
            let li = usize::try_from(level).unwrap_or(0);
            if let Some(lm) = ruler.left_margin.get(li).copied().flatten() {
                pf.left_margin = Some(lm);
            }
            if let Some(id) = ruler.indent.get(li).copied().flatten() {
                pf.indent = Some(id);
            }
            if let Some(d) = ruler.default_tab_size {
                pf.default_tab_size = Some(d);
            }
            if let Some(ts) = &ruler.tab_stops
                && !ts.is_empty()
            {
                pf.tab_stops = Some(ts.clone());
            }
        }
        if let Some(r) = pf_run {
            pf.assign(&r.pf);
        }
        // PP9: the pp9rt of the first run of the paragraph indexes the StyleTextProp9Atom
        let first_cf = run_at(&cf_ix, p_start);
        if let Some(pf9) = &block.pf9
            && !pf9.is_empty()
            && pf.has_bullet == Some(true)
        {
            let idx = first_cf.and_then(|c| c.cf.pp9rt).unwrap_or(0);
            let p9 = usize::try_from(idx).ok().and_then(|i| pf9.get(i));
            if let Some(p9) = p9 {
                if p9.has_auto_num == Some(true) {
                    pf.has_auto_num = Some(true);
                    if p9.auto_num_scheme.is_some() {
                        pf.auto_num_scheme = p9.auto_num_scheme;
                    }
                    if p9.auto_num_start.is_some() {
                        pf.auto_num_start = p9.auto_num_start;
                    }
                }
                if p9.bullet_blip_ref.is_some() {
                    pf.bullet_blip_ref = p9.bullet_blip_ref;
                }
            }
        }
        let mut body = p_pr_xml("a:pPr", &pf, &base.cf, ctx, level, host);
        let cf_at = |pos: u64| -> TextCF {
            let mut c = base.cf.clone();
            if let Some(r) = run_at(&cf_ix, pos) {
                c.assign(&r.cf);
            }
            c
        };
        if p_end > p_start {
            let bounds = cf_boundaries(&cf_ix.ends, &lk, p_start, p_end);
            for w in bounds.windows(2) {
                let (a, b) = (w[0], w[1]);
                let mut cf = cf_at(a);
                let link = run_link_for(&lk, a, ctx, host);
                // in the binary, text with a hyperlink is painted in the "accent and hyperlink"
                // colour of the scheme
                if let Some(l) = &link
                    && l.action.is_none()
                    && ctx.scheme.get(6).is_some_and(|c| !c.is_empty())
                {
                    cf.color = Some(ColorIndex { r: 0, g: 0, b: 0, index: 6 });
                }
                let r_pr = r_pr_xml("a:rPr", &cf, ctx, link.as_ref());
                // line breaks (\v) and fields (meta characters) split the run
                let mut seg: Vec<u16> = Vec::new();
                let flush = |seg: &mut Vec<u16>, body: &mut String| {
                    if !seg.is_empty() {
                        body.push_str(&format!("<a:r>{r_pr}<a:t>{}</a:t></a:r>", esc_text_units(seg)));
                    }
                    seg.clear();
                };
                let mut i = a;
                while i < b {
                    let ch = usize::try_from(i).ok().and_then(|k| text.get(k)).copied().unwrap_or(0);
                    let fld = u32::try_from(i).ok().and_then(|k| fields.get(&k));
                    if let Some(f) = fld {
                        flush(&mut seg, &mut body);
                        if let Some(ty) = f.fld {
                            let at = attrs(&Attrs::of(vec![("id", s(field_id(ctx))), ("type", s(ty))]));
                            body.push_str(&format!("<a:fld{at}>{r_pr}<a:t>{}</a:t></a:fld>", esc_text(&f.text)));
                        } else if !f.text.is_empty() {
                            body.push_str(&format!("<a:r>{r_pr}<a:t>{}</a:t></a:r>", esc_text(&f.text)));
                        }
                    } else if ch == 0x0b {
                        flush(&mut seg, &mut body);
                        body.push_str(&format!("<a:br>{r_pr}</a:br>"));
                    } else {
                        seg.push(ch);
                    }
                    i += 1;
                }
                flush(&mut seg, &mut body);
            }
        } else {
            // empty paragraph: an empty run carries the line size to the layout
            body.push_str(&format!("<a:r>{}<a:t></a:t></a:r>", r_pr_xml("a:rPr", &cf_at(p_start), ctx, None)));
        }
        body.push_str(&r_pr_xml("a:endParaRPr", &cf_at(p_end), ctx, None));
        out.push_str(&format!("<a:p>{body}</a:p>"));
    }
    out
}

/// `<p:titleStyle>`/`<p:bodyStyle>`/`<p:otherStyle>`/`<p:defaultTextStyle>`: 9 levels.
pub fn levels_xml(tag: &str, ctx: &TextConvCtx<'_>, text_type: u32, host: &mut Option<&mut dyn TextHost>) -> String {
    let mut kids = String::new();
    for i in 0..9i32 {
        let st = resolve_level(ctx, text_type, i.min(4));
        kids.push_str(&p_pr_xml(&format!("a:lvl{}pPr", i + 1), &st.pf, &st.cf, ctx, 0, host));
    }
    el(tag, vec![], &kids)
}
