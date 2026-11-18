//! `layoutTextBody` (`text-layout.ts:277–686`), frontier B3, ported 1:1: word wrap on the usable width (the
//! 1/8-pt line grid), autofit `fontScale`/`lnSpcReduction`, columns, vertical anchors, alignment with justify and
//! distribute, bullets and auto-numbering with hanging indents, tab stops (left/right/centre/decimal), line spacing
//! in percent and points with the measured baseline factors, `spcBef`/`spcAft`, kinsoku and hanging punctuation,
//! the kerning seam between units, and the character break of a word longer than the line by grapheme clusters.
//!
//! Positions are EMU relative to the box's top-left, as the TS. The box of a slide shape is `(cx × scale) / scale`
//! (render.ts:1621), so the layout runs per scale (`uo-pptx.md` correction 2). Widths come from a
//! [`MeasureTable`] through one of the two measurers ([`Measurer`]); a pass with missing answers returns
//! [`LayoutError::NeedMeasures`].
//!
//! Identity: the TS compares display runs by reference (`last.run === t.run`, the kerning seam); here a display run
//! is an index into the pass's run list, which is the same relation.

use std::borrow::Cow;

use uo_jsnum::math::{floor, max, min, round};
use uo_jsnum::text::{is_js_space, trim_end};

use crate::color::CssColor;
use crate::model::{Fill, ImageRef, OuterShadow, Paragraph, PatternFill, Run, RunExtras, TableCell, TextBody};
use crate::symbol::{is_symbol_font, symbol_text};
use crate::text::measure::{MFont, MeasureReq, MeasureTable, Measurer, Meter, NeedMeasures};

/// EMU per point (`model.ts`).
const EMU_PER_POINT: f64 = 12700.0;
/// The family the CONVERTED text of symbol fonts is drawn with (`SYMBOL_FAMILY`, text-layout.ts:6).
pub const SYMBOL_FAMILY: &str = "Segoe UI Symbol";
// PowerPoint bodyPr defaults: 0.1in sides, 0.05in top and bottom.
const DEF_INS_L: f64 = 91440.0;
const DEF_INS_T: f64 = 45720.0;
const DEF_INS_R: f64 = 91440.0;
const DEF_INS_B: f64 = 45720.0;
/// The approximate single-line factor of PowerPoint.
const LINE_FACTOR: f64 = 1.2;
/// The share of the line height above the baseline.
const BASELINE_FACTOR: f64 = 0.8;
/// The same with expanded (> 100 %) or exact line spacing (measured in 8 fonts, syn-a-tbl8).
const EXP_BASELINE_FACTOR: f64 = 0.75;
/// 1/8 pt in EMU: the width grid of the reference application.
const EIGHTH_PT: f64 = EMU_PER_POINT / 8.0;
/// `displayRuns` recursion past this depth is the TS's endless recursion (a symbol-area character the Symbol table
/// does not map stays in the symbol area): the TS throws `RangeError` (decision B3-4).
const MAX_DISPLAY_DEPTH: u32 = 64;

/// The font of a measurement or a drawn run (`LayoutFont`, `text-layout.ts:23`).
#[derive(Clone, PartialEq, Debug)]
pub struct LayoutFont {
    pub family: Box<str>,
    pub size_pt: f64,
    pub bold: bool,
    pub italic: bool,
    /// The run asks for kerning and its size reaches the threshold (the TS sets `kern: true` only then).
    pub kern: bool,
}

/// A placed run (`PlacedRun`, `text-layout.ts:36`).
#[derive(Clone, PartialEq, Debug)]
pub struct PlacedRun {
    pub text: String,
    /// X of the run's start, EMU from the box.
    pub x: f64,
    /// Measured width, EMU.
    pub w: f64,
    pub font: LayoutFont,
    pub color: CssColor,
    pub underline: bool,
    pub hyperlink: Option<Box<str>>,
    pub shadow: Option<OuterShadow>,
    /// Vertical offset (EMU, + = up) of superscript/subscript.
    pub rise: Option<f64>,
    pub strike: bool,
    /// Extra space per character (EMU).
    pub spc: Option<f64>,
    /// Colour and width (EMU) of the letters' outline.
    pub outline: Option<(CssColor, f64)>,
    pub patt_fill: Option<PatternFill>,
    pub grad_fill: Option<Box<Fill>>,
    pub fill_image: Option<ImageRef>,
    pub fill_tile: bool,
    pub alpha: Option<f64>,
}

/// The bullet of a paragraph's first line (`PlacedLine.bullet`).
#[derive(Clone, PartialEq, Debug)]
pub struct PlacedBullet {
    pub text: String,
    pub x: f64,
    pub font: LayoutFont,
    pub color: CssColor,
    pub image: Option<ImageRef>,
}

/// A laid-out line (`PlacedLine`, `text-layout.ts:65`).
#[derive(Clone, PartialEq, Debug)]
pub struct PlacedLine {
    pub runs: Vec<PlacedRun>,
    /// Top of the line, EMU from the box top (always present in the TS output).
    pub top: f64,
    pub baseline: f64,
    pub height: f64,
    pub bullet: Option<PlacedBullet>,
    /// Index of the source paragraph (paragraph and unit animations).
    pub para: u32,
}

/// `TextLayoutResult` (`text-layout.ts:79`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TextLayoutResult {
    pub lines: Vec<PlacedLine>,
    /// Total height of the text block, EMU.
    pub total_h: f64,
}

/// `LayoutTextOptions` (`text-layout.ts:85`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LayoutTextOptions {
    pub ins_l: Option<f64>,
    pub ins_t: Option<f64>,
    pub ins_r: Option<f64>,
    pub ins_b: Option<f64>,
    pub default_sz_pt: Option<f64>,
    pub default_font: Option<Box<str>>,
    pub default_color: Option<CssColor>,
    pub default_anchor: Option<Box<str>>,
    /// The emphasis "font size" scale (animation), continuous, over the autofit's.
    pub size_scale: Option<f64>,
    /// The PowerPoint-over-TS switches (the layout reads `ts_first_baseline`); the emission passes the deck's.
    pub rules: crate::rules::SlidesRules,
}

/// Why a layout pass gives no result.
#[derive(Clone, PartialEq, Debug)]
pub enum LayoutError {
    /// Measurements the table does not have: the shell answers them and the pass runs again.
    NeedMeasures(NeedMeasures),
    /// The TS throws here (`RangeError` of an endless `displayRuns` recursion): the painter skips the shape.
    Throws(&'static str),
}

/// `formatAutoNum` (`text-layout.ts:114`): the number of a `buAutoNum` in its scheme (common subset; the other
/// schemes print as `arabicPeriod`).
#[must_use]
pub fn format_auto_num(scheme: &str, n: i64) -> String {
    let roman = |v: i64| -> String {
        const TABLE: [(i64, &str); 13] = [
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ];
        let mut out = String::new();
        let mut x = v;
        for (val, sym) in TABLE {
            while x >= val {
                out.push_str(sym);
                x -= val;
            }
        }
        out
    };
    // String.fromCharCode(96 + ((v - 1) % 26) + 1): `%` keeps the dividend's sign (n = 0 gives '`')
    let alpha = |v: i64| -> String {
        let code = 96 + (v.wrapping_sub(1) % 26) + 1;
        let unit = u16::try_from(code.rem_euclid(65_536)).unwrap_or(0);
        char::from_u32(u32::from(unit)).map_or_else(|| '\u{FFFD}'.to_string(), |c| c.to_string())
    };
    let up = |s: String| uo_text::case::to_uppercase_str(&s);
    match scheme {
        "alphaLcPeriod" => format!("{}.", alpha(n)),
        "alphaUcPeriod" => format!("{}.", up(alpha(n))),
        "alphaLcParenR" => format!("{})", alpha(n)),
        "alphaUcParenR" => format!("{})", up(alpha(n))),
        "romanLcPeriod" => format!("{}.", roman(n)),
        "romanUcPeriod" => format!("{}.", up(roman(n))),
        "arabicParenR" => format!("{n})"),
        "arabicPlain" => format!("{n}"),
        _ => format!("{n}."),
    }
}

/// `cellBody` (render.ts:2160): a table cell's body as the text engine sees it: the cell's margins and anchor (the
/// application defaults 0.1" on the sides, 0.05" top and bottom, anchor at the TOP) and the table style's colour,
/// bold and italic where the run has none.
#[must_use]
pub fn cell_body(cell: &TableCell) -> Option<TextBody> {
    let src = cell.tx_body.as_deref()?;
    let mut tb = src.clone();
    tb.anchor = Some(cell.anchor.clone().unwrap_or_else(|| "t".into()));
    if let Some(v) = cell.vert.as_ref().filter(|v| !v.is_empty()) {
        tb.x_mut().vert = Some(v.clone());
    }
    tb.ins_l = Some(cell.mar_l.unwrap_or(91440.0));
    tb.ins_r = Some(cell.mar_r.unwrap_or(91440.0));
    tb.ins_t = Some(cell.mar_t.unwrap_or(45720.0));
    tb.ins_b = Some(cell.mar_b.unwrap_or(45720.0));
    if cell.tx_color.is_some() || cell.tx_bold.is_some() || cell.tx_italic.is_some() {
        for p in &mut tb.paragraphs {
            for r in &mut p.runs {
                if r.color.is_none() && cell.tx_color.is_some() {
                    r.color.clone_from(&cell.tx_color);
                }
                if r.bold.is_none() && cell.tx_bold.is_some() {
                    r.bold = cell.tx_bold;
                }
                if r.italic.is_none() && cell.tx_italic.is_some() {
                    r.italic = cell.tx_italic;
                }
            }
        }
    }
    Some(tb)
}

/// Lays out a body in the box `box_w × box_h` (EMU). Pure: depends only on the arguments.
pub fn layout_text_body(
    tb: &TextBody,
    box_w: f64,
    box_h: f64,
    measurer: Measurer,
    table: &MeasureTable,
    opts: &LayoutTextOptions,
) -> Result<TextLayoutResult, LayoutError> {
    let mut m = Meter::new(table, measurer, false);
    let out = layout(tb, box_w, box_h, &mut m, opts);
    let (missing, _) = m.into_missing();
    finish(out, missing)
}

/// The measurements this pass is missing (empty when [`layout_text_body`] can complete, or when it throws). While
/// any per-character advance is missing only those are asked; the kerning and whole-string widths of the true line
/// breaks follow in the next round.
#[must_use]
pub fn need_measures(
    tb: &TextBody,
    box_w: f64,
    box_h: f64,
    measurer: Measurer,
    table: &MeasureTable,
    opts: &LayoutTextOptions,
) -> Vec<MeasureReq> {
    let mut m = Meter::new(table, measurer, false);
    let _ = layout(tb, box_w, box_h, &mut m, opts);
    m.into_missing().0
}

/// [`layout_text_body`] that also returns every question the pass asked, answered or not (parity harnesses
/// compare it with the TS's `measureText` calls).
pub fn layout_text_body_traced(
    tb: &TextBody,
    box_w: f64,
    box_h: f64,
    measurer: Measurer,
    table: &MeasureTable,
    opts: &LayoutTextOptions,
) -> (Result<TextLayoutResult, LayoutError>, Vec<MeasureReq>) {
    let mut m = Meter::new(table, measurer, true);
    let out = layout(tb, box_w, box_h, &mut m, opts);
    let (missing, trace) = m.into_missing();
    let mut asked: Vec<MeasureReq> = trace.unwrap_or_default().into_iter().collect();
    asked.sort();
    (finish(out, missing), asked)
}

fn finish(out: Result<TextLayoutResult, LayoutError>, missing: Vec<MeasureReq>) -> Result<TextLayoutResult, LayoutError> {
    match out {
        Err(e) => Err(e),
        Ok(_) if !missing.is_empty() => Err(LayoutError::NeedMeasures(NeedMeasures(missing))),
        Ok(r) => Ok(r),
    }
}

// ------------------------------------------------------------------------------------------------ display runs

/// A display run (`DisplayRun`, text-layout.ts:139): the run as it will be DRAWN (caps, small caps, symbol fonts as
/// Unicode, superscript/subscript size), with the fields of its source run. `src` is `None` for the synthetic empty
/// run of a paragraph without characters (`{ text: '', sz }`).
struct DRun<'a> {
    src: Option<&'a Run>,
    text: String,
    sz: Option<f64>,
    font: Option<&'a str>,
    rise: Option<f64>,
}

impl<'a> DRun<'a> {
    fn ex(&self) -> Option<&'a RunExtras> {
        self.src.and_then(|r| r.extras.as_deref())
    }
    fn bold(&self) -> bool {
        self.src.and_then(|r| r.bold).unwrap_or(false)
    }
    fn italic(&self) -> bool {
        self.src.and_then(|r| r.italic).unwrap_or(false)
    }
    fn lang(&self) -> &'a str {
        self.src.and_then(|r| r.lang.as_deref()).unwrap_or("")
    }
    fn spc(&self) -> Option<f64> {
        self.ex().and_then(|e| e.spc)
    }
}

/// `/[\uF020-\uF0FF]/`: the symbol area of the private use (Symbol, Wingdings codes 0x20–0xFF).
fn is_pua(c: char) -> bool {
    ('\u{F020}'..='\u{F0FF}').contains(&c)
}

/// `/\p{Ll}/u` over one code point.
fn is_ll(c: char) -> bool {
    uo_text::props::is_lowercase_letter(u32::from(c))
}

/// `displayRuns` (text-layout.ts:147) over one run view (`r` with `text`/`font`/`sz` possibly replaced by the
/// symbol-area split, which recurses as the TS does).
fn display_run<'a>(
    src: Option<&'a Run>,
    text: &str,
    font: Option<&'a str>,
    sz: Option<f64>,
    default_sz: f64,
    depth: u32,
    out: &mut Vec<DRun<'a>>,
) -> Result<(), LayoutError> {
    if depth > MAX_DISPLAY_DEPTH {
        return Err(LayoutError::Throws("RangeError"));
    }
    let ex = src.and_then(|r| r.extras.as_deref());
    if text == "\n" {
        out.push(DRun { src, text: text.to_string(), sz, font, rise: None });
        return Ok(());
    }
    let mut base = DRun { src, text: text.to_string(), sz, font, rise: None };
    if is_symbol_font(font) {
        base.text = symbol_text(font, text);
        base.font = Some(SYMBOL_FAMILY);
    } else if text.chars().any(is_pua) {
        // symbol-area characters (U+F0xx) use the <a:sym> symbol font (Symbol without one): γ written as U+F067
        let sym = ex.and_then(|e| e.sym_font.as_deref());
        let sf = if sym.is_some() && is_symbol_font(sym) { sym } else { Some("Symbol") };
        // text.match(/[\uF020-\uF0FF]+|[^\uF020-\uF0FF]+/g)
        let mut start = 0usize;
        let mut chars = text.char_indices().peekable();
        while let Some((_, c)) = chars.next() {
            let pua = is_pua(c);
            let end = match chars.peek() {
                Some(&(_, d)) if is_pua(d) == pua => continue,
                Some(&(j, _)) => j,
                None => text.len(),
            };
            let m = text.get(start..end).unwrap_or("");
            start = end;
            if pua {
                display_run(src, &symbol_text(sf, m), Some(SYMBOL_FAMILY), sz, default_sz, depth + 1, out)?;
            } else {
                display_run(src, m, font, sz, default_sz, depth + 1, out)?;
            }
        }
        return Ok(());
    }
    if let Some(b) = ex.and_then(|e| e.baseline).filter(|b| *b != 0.0 && !b.is_nan()) {
        let s = sz.unwrap_or(default_sz);
        base.sz = Some((s * 2.0) / 3.0);
        base.rise = Some(b * s);
    }
    match ex.and_then(|e| e.cap.as_deref()) {
        Some("all") => {
            base.text = uo_text::case::to_uppercase_str(&base.text);
            out.push(base);
        }
        Some("small") => {
            let s = base.sz.unwrap_or(default_sz);
            // base.text.match(/[^\p{Ll}]+|\p{Ll}+/gu): maximal runs of lower-case letters and of the rest
            let t = std::mem::take(&mut base.text);
            let mut start = 0usize;
            let mut it = t.char_indices().peekable();
            while let Some((_, c)) = it.next() {
                let lower = is_ll(c);
                if matches!(it.peek(), Some(&(_, d)) if is_ll(d) == lower) {
                    continue;
                }
                let end = it.peek().map_or(t.len(), |&(j, _)| j);
                let m = t.get(start..end).unwrap_or("");
                start = end;
                if lower {
                    out.push(DRun {
                        src: base.src,
                        text: uo_text::case::to_uppercase_str(m),
                        sz: Some(s * 0.8),
                        font: base.font,
                        rise: base.rise,
                    });
                } else {
                    out.push(DRun { src: base.src, text: m.to_string(), sz: base.sz, font: base.font, rise: base.rise });
                }
            }
        }
        _ => out.push(base),
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------ break units

/// CJK ideographs, kana, punctuation and full-width forms (Hangul excluded): a line breaks between any two of them
/// (measured, syn-a-tbl6 #6/#7, syn-a-tbl8 #6). One UTF-16 unit: an astral character never matches.
fn is_cjk(c: char) -> bool {
    matches!(u32::from(c), 0x2E80..=0x2FFF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F | 0x3190..=0x31FF | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF)
}

const KINSOKU_NO_START: &str = "!%),.:;?]}¢°’”‰′″℃、。々〉》」』】〕ぁぃぅぇぉっゃゅょゎゕゖ゛゜ゝゞァィゥェォッャュョヮヵヶ・ーヽヾ！％），．：；？］｝｡｣､･ｧｨｩｪｫｬｭｮｯｰﾞﾟ￠";
const KINSOKU_NO_END: &str = "$([\\{£¥‘“〈《「『【〔＄（［｛｢￡￥";
const KINSOKU_HANG: &str = "、。，．､｡";

/// `/^(ja|zh|ko)\b/i`.
fn ea_lang(lang: &str) -> bool {
    let b = lang.as_bytes();
    let (Some(a), Some(c)) = (b.first(), b.get(1)) else { return false };
    let two = [a.to_ascii_lowercase(), c.to_ascii_lowercase()];
    matches!(&two, b"ja" | b"zh" | b"ko") && !b.get(2).is_some_and(|d| d.is_ascii_alphanumeric() || *d == b'_')
}

/// After these dashes a line may break (measured, syn-b-e19); not after ‐ ‑ or the soft hyphen.
fn break_after(c: char) -> bool {
    matches!(c, '-' | '–' | '—')
}

/// A space that hosts a break (not TAB nor no-break space).
fn is_space(c: char) -> bool {
    c != '\t' && c != '\u{a0}' && is_js_space(c)
}

/// The class of a UTF-16 unit for the break test: an astral character is a surrogate pair, and a surrogate is
/// neither space, dash, no-break space nor CJK, which is what the TS sees at both of its units (U+FFFF stands for it).
fn unit_class(c: char) -> char {
    if u32::from(c) > 0xFFFF { '\u{FFFF}' } else { c }
}

/// A piece of a unit: text and its display run.
#[derive(Clone)]
struct Piece {
    text: String,
    run: usize,
}

/// `breakUnits` (text-layout.ts:236): the paragraph's break units.
fn break_units(runs: &[DRun<'_>]) -> Vec<Vec<Piece>> {
    let mut out: Vec<Vec<Piece>> = Vec::new();
    let mut cur: Vec<Piece> = Vec::new();
    // `prev`: the last unit of the previous run (None = '')
    let mut prev: Option<char> = None;
    // the current run of spaces has a no-break space: "ipsum.  dolor" breaks nowhere (measured, syn-b-e23)
    let mut nb = false;
    fn close(out: &mut Vec<Vec<Piece>>, cur: &mut Vec<Piece>, prev: &mut Option<char>, nb: &mut bool) {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
        *prev = None;
        *nb = false;
    }
    for (ri, r) in runs.iter().enumerate() {
        if r.text == "\n" {
            close(&mut out, &mut cur, &mut prev, &mut nb);
            out.push(vec![Piece { text: "\n".into(), run: ri }]);
            continue;
        }
        let t = r.text.as_str();
        let mut start = 0usize;
        let mut last: Option<char> = None; // the unit before i inside this run
        for (i, c0) in t.char_indices() {
            let c = unit_class(c0);
            if c0 == '\t' {
                if i > start {
                    cur.push(Piece { text: t.get(start..i).unwrap_or("").into(), run: ri });
                }
                close(&mut out, &mut cur, &mut prev, &mut nb);
                out.push(vec![Piece { text: "\t".into(), run: ri }]);
                start = i + 1;
                last = Some('\t');
                continue;
            }
            let p = if i > start { last } else { prev };
            if let Some(p) = p
                && !is_space(c)
                && c != '\u{a0}'
                && ((is_space(p) && !nb) || break_after(p) || (p != '\u{a0}' && !is_space(p) && (is_cjk(c) || is_cjk(p))))
            {
                if i > start {
                    cur.push(Piece { text: t.get(start..i).unwrap_or("").into(), run: ri });
                }
                close(&mut out, &mut cur, &mut prev, &mut nb);
                start = i;
            }
            if c == '\u{a0}' {
                nb = true;
            } else if !is_space(c) {
                nb = false;
            }
            last = Some(c);
        }
        if t.len() > start {
            cur.push(Piece { text: t.get(start..).unwrap_or("").into(), run: ri });
            prev = t.chars().next_back().map(unit_class);
        }
    }
    close(&mut out, &mut cur, &mut prev, &mut nb);
    out
}

// ------------------------------------------------------------------------------------------------ the layout

/// The size scales: the recorded autofit's (`fontScale`) and a continuous extra one (animated emphasis).
#[derive(Clone, Copy)]
struct SizeScale {
    auto: f64,
    extra: f64,
}

/// The effective size: the autofit rounds the scaled size to the whole POINT (measured, syn-b-e23: 46 × 90% → 41).
fn scaled_sz(sz: f64, sc: SizeScale) -> f64 {
    (if sc.auto != 1.0 { max(1.0, round(sz * sc.auto)) } else { sz }) * sc.extra
}

struct Env<'o> {
    sc: SizeScale,
    default_sz: f64,
    default_font: &'o str,
}

fn font_of(r: &DRun<'_>, env: &Env<'_>) -> LayoutFont {
    let size_pt = scaled_sz(r.sz.unwrap_or(env.default_sz), env.sc);
    let kern_at = r.src.and_then(|s| s.kern);
    // kern="1200": kerning pairs only from 12 pt on; without kern (or 0), never
    let kern = kern_at.is_some_and(|k| k > 0.0 && size_pt >= k);
    let family = r.font.unwrap_or(env.default_font);
    // the run's fallback context (only present under `!ts_fallback_fonts`) rides in the face name; a display run whose
    // face was converted (symbol fonts) keeps the plain family
    let own = r.src.is_some_and(|s| s.font.as_deref().is_none_or(|f| f == family));
    let fb = r.ex().and_then(|e| e.font_fallback.as_deref()).filter(|_| own);
    let family: Box<str> = match fb {
        Some(fb) => super::measure::encode_face(family, fb).into(),
        None => family.into(),
    };
    LayoutFont { family, size_pt, bold: r.bold(), italic: r.italic(), kern }
}

fn mfont(f: &LayoutFont) -> MFont<'_> {
    MFont { family: &f.family, size_pt: f.size_pt, bold: f.bold, italic: f.italic, kern: f.kern }
}

/// `[...text].length`.
fn cp_len(s: &str) -> f64 {
    s.chars().count() as f64
}

/// `measureEmu`.
fn measure_emu(m: &mut Meter<'_>, text: &str, f: &LayoutFont) -> f64 {
    m.measure(text, &mfont(f)) * EMU_PER_POINT
}

/// `measureRun`: with the character spacing (`spc`; the autofit does NOT scale it — measured, syn-b-e27).
fn measure_run(m: &mut Meter<'_>, text: &str, r: &DRun<'_>, f: &LayoutFont, sc: SizeScale) -> f64 {
    let spc = r.spc().filter(|s| *s != 0.0 && !s.is_nan());
    measure_emu(m, text, f) + spc.map_or(0.0, |s| s * EMU_PER_POINT * sc.extra * cp_len(text))
}

/// The tab stops of a paragraph (`tabTo`, text-layout.ts:384).
struct TabCtx<'a> {
    p: &'a Paragraph,
    def_tab: f64,
    indent: f64,
    mar_l: f64,
}

impl TabCtx<'_> {
    fn tab_to(&self, x_rel: f64, seg: (f64, f64), first: bool) -> f64 {
        let mut next = (floor(x_rel / self.def_tab) + 1.0) * self.def_tab;
        let mut algn = "l";
        for t in self.p.x().tabs.as_deref().unwrap_or(&[]) {
            if t.pos > x_rel + 1.0 || (first && t.pos >= x_rel - 1.0) {
                next = t.pos;
                algn = &t.algn;
                break;
            }
        }
        if self.indent < 0.0 && x_rel < self.mar_l - 1.0 && self.mar_l < next {
            return self.mar_l;
        }
        match algn {
            "r" => max(x_rel, next - seg.0),
            "ctr" => max(x_rel, next - seg.0 / 2.0),
            "dec" => max(x_rel, next - seg.1),
            _ => next,
        }
    }
}

/// `segOf`: the width of the stretch after a tab and its part before the decimal separator.
fn seg_of(m: &mut Meter<'_>, items: &[Piece], from: usize, runs: &[DRun<'_>], env: &Env<'_>) -> (f64, f64) {
    let mut w_all = 0.0;
    let mut w_dec = -1.0;
    for it in items.iter().skip(from) {
        if it.text == "\t" || it.text == "\n" {
            break;
        }
        let Some(r) = runs.get(it.run) else { continue };
        let fo = font_of(r, env);
        if w_dec < 0.0
            && let Some(ix) = it.text.find(['.', ','])
        {
            w_dec = w_all + measure_run(m, it.text.get(..ix).unwrap_or(""), r, &fo, env.sc);
        }
        w_all += measure_run(m, &it.text, r, &fo, env.sc);
    }
    (w_all, if w_dec < 0.0 { w_all } else { w_dec })
}

/// A line before materialization (`ProtoLine`).
struct ProtoLine {
    runs: Vec<Piece>,
    mar_l: f64,
    align: Box<str>,
    bullet: Option<PlacedBullet>,
    para: usize,
}

/// `unitW`: the width of a unit and the part of it WITHOUT the trailing spaces (which hang past the line).
fn unit_w(m: &mut Meter<'_>, u: &[Piece], runs: &[DRun<'_>], env: &Env<'_>) -> (f64, f64) {
    let mut w = 0.0;
    let mut trail = 0.0;
    let mut trailing = true;
    for pc in u.iter().rev() {
        let Some(r) = runs.get(pc.run) else { continue };
        let f = font_of(r, env);
        let full = measure_run(m, &pc.text, r, &f, env.sc);
        w += full;
        if trailing {
            let core = trim_end(&pc.text);
            trail += full - if core.is_empty() { 0.0 } else { measure_run(m, core, r, &f, env.sc) };
            if !core.is_empty() {
                trailing = false;
            }
        }
    }
    (w, w - trail)
}

/// The last UTF-16 unit of a string as `String.fromCodePoint(s.codePointAt(s.length - 1))` gives it: a BMP
/// character, or the lone low surrogate of an astral one (written U+FFFD, decision B3-3); always one unit.
fn last_unit(s: &str) -> char {
    match s.chars().next_back() {
        Some(c) if u32::from(c) > 0xFFFF => '\u{FFFD}',
        Some(c) => c,
        None => ' ',
    }
}

fn layout(tb: &TextBody, box_w: f64, box_h: f64, m: &mut Meter<'_>, opts: &LayoutTextOptions) -> Result<TextLayoutResult, LayoutError> {
    let ins_l = tb.ins_l.or(opts.ins_l).unwrap_or(DEF_INS_L);
    let ins_t = tb.ins_t.or(opts.ins_t).unwrap_or(DEF_INS_T);
    let ins_r = tb.ins_r.or(opts.ins_r).unwrap_or(DEF_INS_R);
    let ins_b = tb.ins_b.or(opts.ins_b).unwrap_or(DEF_INS_B);
    let default_sz = opts.default_sz_pt.unwrap_or(18.0);
    let default_font: &str = opts.default_font.as_deref().unwrap_or("Calibri");
    let default_color = opts.default_color.clone().unwrap_or_else(|| CssColor::new("#000000"));
    let sc = SizeScale { auto: tb.x().font_scale.unwrap_or(1.0), extra: opts.size_scale.unwrap_or(1.0) };
    // the continuous factor (character spacing, superscript)
    let font_scale = sc.auto * sc.extra;
    let line_factor = LINE_FACTOR * (1.0 - tb.x().ln_spc_reduction.unwrap_or(0.0));
    let usable_w = max(0.0, box_w - ins_l - ins_r);
    // columns: break at the COLUMN width and flow to the next when the usable height runs out
    let num_col = max(1.0, tb.x().num_col.unwrap_or(1.0));
    let spc_col = tb.x().spc_col.unwrap_or(0.0);
    let col_w = if num_col > 1.0 { max(0.0, (usable_w - (num_col - 1.0) * spc_col) / num_col) } else { usable_w };
    let wrap = tb.wrap.as_deref() != Some("none");
    let env = Env { sc, default_sz, default_font };
    let caps = m.caps();

    let mut proto: Vec<ProtoLine> = Vec::new();
    let mut proto_runs: Vec<usize> = Vec::new(); // the paragraph (display-run list) of each proto line
    let mut heights: Vec<f64> = Vec::new();
    // EXTRA space (EMU) before each line (spcBef/spcAft of the paragraphs)
    let mut spacers: Vec<f64> = Vec::new();
    // per line: share of the base with expanded/exact spacing (0 = normal) and the largest font's size (EMU)
    let mut line_exp: Vec<f64> = Vec::new();
    let mut line_sz: Vec<f64> = Vec::new();
    let mut pending_spacer = 0.0;
    // auto-number counter per level (restarts when the level "goes up")
    let mut auto_num = Counters::default();
    // the display runs of every paragraph and the tab contexts (the materialization needs them)
    let mut para_runs: Vec<Vec<DRun<'_>>> = Vec::with_capacity(tb.paragraphs.len());
    let mut tab_ctx: Vec<TabCtx<'_>> = Vec::with_capacity(tb.paragraphs.len());
    let synthetic: Vec<Run> =
        tb.paragraphs.iter().map(|p| Run { sz: p.end_sz.or_else(|| p.runs.first().and_then(|r| r.sz)), ..Run::default() }).collect();

    for (pi, p) in tb.paragraphs.iter().enumerate() {
        let level = min(p.level.unwrap_or(0.0), 8.0);
        let mar_l = p.mar_l.unwrap_or(0.0);
        let indent = p.indent.unwrap_or(0.0);
        let align: Box<str> = p.align.clone().unwrap_or_else(|| "l".into());
        // a paragraph WITHOUT characters: the line height comes from endParaRPr, as in the application
        let has_chars = p.runs.iter().any(|r| !r.text.is_empty());
        let mut runs: Vec<DRun<'_>> = Vec::new();
        if has_chars {
            for r in &p.runs {
                display_run(Some(r), &r.text, r.font.as_deref(), r.sz, default_sz, 0, &mut runs)?;
            }
        } else {
            let s = synthetic.get(pi).and_then(|r| r.sz);
            runs.push(DRun { src: None, text: String::new(), sz: s, font: None, rise: None });
        }
        if runs.is_empty() {
            // unreachable with the TS rules (a run with characters always displays): the TS would throw on runs[0]
            return Err(LayoutError::Throws("TypeError"));
        }

        // the paragraph's bullet (first line only). An EMPTY paragraph shows no bullet and takes no number
        let mut bullet: Option<PlacedBullet> = None;
        let mut bullet_text_x: Option<f64> = None;
        if p.bullet == Some(true) && runs.iter().any(|r| !r.text.is_empty()) {
            // restart the counters of deeper levels (`for (l = level + 1; l < 9; l++) counters[l] = 0`)
            auto_num.reset_from(level + 1.0);
            let mut btext: String;
            if let Some(scheme) = &p.bu().bullet_auto_num {
                let n = auto_num.get(level).saturating_add(1);
                auto_num.set(level, n);
                btext = format_auto_num(scheme, n);
            } else {
                btext = p.bu().bullet_char.as_deref().unwrap_or("•").to_string();
            }
            // own font/colour/size (buFont/buClr/buSz) or those of the first run; symbol font → Unicode
            let first = runs.iter().position(|r| !r.text.is_empty() && r.text != "\n").unwrap_or(0);
            let first_run = runs.get(first).ok_or(LayoutError::Throws("TypeError"))?;
            let tf = font_of(first_run, &env);
            // numbering with a SYMBOL buFont (Wingdings): the digits come out in the text font (oracle, gd-183528 #11)
            let num_in_text = p.bu().bullet_auto_num.is_some() && is_symbol_font(p.bu().bullet_font.as_deref());
            let mut family: Box<str> =
                if num_in_text { tf.family.clone() } else { p.bu().bullet_font.clone().unwrap_or_else(|| tf.family.clone()) };
            if p.bu().bullet_auto_num.is_none() && is_symbol_font(p.bu().bullet_font.as_deref()) {
                btext = symbol_text(p.bu().bullet_font.as_deref(), &btext);
                family = SYMBOL_FAMILY.into();
            }
            let size_pt = match p.bu().bullet_size_pt {
                Some(pt) => pt * font_scale,
                None => tf.size_pt * p.bu().bullet_size_pct.unwrap_or(1.0),
            };
            // the bullet at the SMALLER of marL and marL+indent, the text at the larger (indent > 0 swaps); left of
            // the box the whole first line slides in. The text never starts before the END of the bullet — the
            // character's width at the BULLET size (buSzPct/buSzPts; measured, syn-b-e20/e25/e26)
            let lo = min(mar_l, mar_l + indent);
            let hi = max(mar_l, mar_l + indent);
            let shift = if lo < 0.0 { -lo } else { 0.0 };
            let raw_bullet: Cow<'_, str> = if p.bu().bullet_auto_num.is_none() {
                Cow::Borrowed(p.bu().bullet_char.as_deref().unwrap_or("•"))
            } else {
                Cow::Borrowed(btext.as_str())
            };
            let bw = if p.bu().bullet_image.is_some() {
                0.0
            } else {
                let bf = LayoutFont {
                    family: if num_in_text { tf.family.clone() } else { p.bu().bullet_font.clone().unwrap_or_else(|| tf.family.clone()) },
                    size_pt,
                    bold: tf.bold,
                    italic: tf.italic,
                    kern: false,
                };
                measure_emu(m, &raw_bullet, &bf)
            };
            bullet_text_x = Some(ins_l + max(hi + shift, lo + shift + bw));
            let color = p
                .bu()
                .bullet_color
                .clone()
                .or_else(|| first_run.src.and_then(|r| r.color.clone()))
                .unwrap_or_else(|| default_color.clone());
            bullet = Some(PlacedBullet {
                text: btext,
                x: ins_l + lo + shift,
                font: LayoutFont { family, size_pt, ..tf },
                color,
                image: p.bu().bullet_image.as_deref().cloned(),
            });
        }

        // the first-line indent only applies to the paragraph's FIRST line; continuations go back to marL
        let cont_x = ins_l + mar_l;
        let first_x = bullet_text_x.unwrap_or(if p.bullet == Some(true) { cont_x } else { ins_l + max(0.0, mar_l + indent) });
        let units = break_units(&runs);
        let words: Vec<Piece> = units.iter().flatten().cloned().collect();
        let def_tab = match p.def_tab_sz {
            Some(d) if d > 0.0 => d,
            _ => 914400.0,
        };
        let tabs = TabCtx { p, def_tab, indent, mar_l };

        // grouping into lines by width
        let mut cur = ProtoLine { runs: Vec::new(), mar_l: first_x, align: align.clone(), bullet, para: pi };
        let mut cur_w = 0.0;
        // where each unit of the current line starts in `cur.runs` (kinsoku moves the last one down)
        let mut u_starts: Vec<usize> = Vec::new();
        let mut first_line_of_para = true;
        let empty_pt = scaled_sz(runs.first().and_then(|r| r.sz).unwrap_or(default_sz), sc);
        let mut last_max_pt = empty_pt;
        // the paragraph's spacing: spcPct multiplies the single line; spcPts is absolute
        let para_factor = p.line_spacing.unwrap_or(1.0);

        let mut flush = |cur: &mut ProtoLine,
                         cur_w: &mut f64,
                         u_starts: &mut Vec<usize>,
                         first_line_of_para: &mut bool,
                         pending_spacer: &mut f64,
                         last_max_pt: &mut f64| {
            let line = std::mem::replace(cur, ProtoLine { runs: Vec::new(), mar_l: cont_x, align: align.clone(), bullet: None, para: pi });
            // line height = the largest size present (an empty line uses the paragraph's size)
            let mut max_pt = if line.runs.is_empty() { empty_pt } else { 0.0 };
            for t in &line.runs {
                let s = runs.get(t.run).and_then(|r| r.sz).unwrap_or(default_sz);
                max_pt = max(max_pt, scaled_sz(s, sc));
            }
            heights.push(match p.line_spacing_pt {
                Some(pt) => pt * EMU_PER_POINT,
                None => max_pt * EMU_PER_POINT * line_factor * para_factor,
            });
            // EXPANDED (> 100 %) or exact spacing: the base at 0.75 of the line (fontAlgn t/ctr/b shift it —
            // measured, syn-a-tbl10) and the foot of the last line not scaled (see below)
            let exp = p.line_spacing_pt.is_some() || line_factor * para_factor > LINE_FACTOR + 1e-9;
            line_exp.push(if exp {
                match p.x().font_algn.as_deref() {
                    Some("t") => 0.84,
                    Some("ctr") => 0.86,
                    Some("b") => 0.68,
                    _ => EXP_BASELINE_FACTOR,
                }
            } else {
                0.0
            });
            line_sz.push(max_pt * EMU_PER_POINT);
            // spcBef goes before the paragraph's 1st line (+ the pending spcAft of the previous one); spcPct is a
            // share of ONE single line (size × line factor) of the 1st line
            let bef = match p.space_before_pt {
                Some(pt) => pt * EMU_PER_POINT,
                None => p.space_before_pct.unwrap_or(0.0) * max_pt * EMU_PER_POINT * line_factor,
            };
            // the application IGNORES the spcBef of the box's 1st paragraph (measured: 0, 40 pt and 100% → same
            // position) — unless `spcFirstLastPara="1"` in the bodyPr (Google Slides exports)
            let first_para_bef = pi > 0 || tb.x().spc_first_last_para == Some(true);
            spacers.push(if *first_line_of_para { *pending_spacer + if first_para_bef { bef } else { 0.0 } } else { 0.0 });
            *last_max_pt = max_pt;
            if *first_line_of_para {
                *pending_spacer = 0.0;
            }
            *first_line_of_para = false;
            proto.push(line);
            proto_runs.push(pi);
            *cur_w = 0.0;
            u_starts.clear();
        };
        // the usable line width on the 1/8-pt grid ROUNDED DOWN: the application measures the line on it
        // (measured, syn-b-e18: the box W+0.02 pt does not fit what measures W+0.005)
        let avail_of = |x: f64| -> f64 { floor(max(0.0, col_w - (x - ins_l)) / EIGHTH_PT + 1e-6) * EIGHTH_PT };
        let mut wi = 0usize; // index of the unit's 1st piece in `words`
        for (ui, u) in units.iter().enumerate() {
            let Some(wd) = u.first() else { continue };
            let at = wi;
            wi += u.len();
            if wd.text == "\n" {
                flush(&mut cur, &mut cur_w, &mut u_starts, &mut first_line_of_para, &mut pending_spacer, &mut last_max_pt);
                continue;
            }
            if wd.text == "\t" {
                let mut x_rel = cur.mar_l - ins_l + cur_w;
                let mut first = cur.runs.is_empty();
                let seg = seg_of(m, &words, at + 1, &runs, &env);
                let mut target = tabs.tab_to(x_rel, seg, first);
                // the TAB is not a break opportunity: it sticks to the next unit — if the pair does not fit, both go
                // down and the TAB applies on the new line (gd-082972)
                let next_w = match units.get(ui + 1) {
                    Some(nu) if nu.first().is_some_and(|x| x.text != "\t" && x.text != "\n") => unit_w(m, nu, &runs, &env).1,
                    _ => 0.0,
                };
                if wrap && !first && target + next_w - (cur.mar_l - ins_l) > avail_of(cur.mar_l) {
                    flush(&mut cur, &mut cur_w, &mut u_starts, &mut first_line_of_para, &mut pending_spacer, &mut last_max_pt);
                    x_rel = cur.mar_l - ins_l + cur_w;
                    first = true;
                    let seg = seg_of(m, &words, at + 1, &runs, &env);
                    target = tabs.tab_to(x_rel, seg, first);
                }
                u_starts.push(cur.runs.len());
                cur.runs.push(Piece { text: "\t".into(), run: wd.run });
                cur_w += target - x_rel;
                continue;
            }
            let (w_emu, w_core) = unit_w(m, u, &runs, &env);
            let mut avail_w = avail_of(cur.mar_l);
            // the application measures the WHOLE line: the kerning pair at the seam with the previous unit counts
            let mut kp = 0.0;
            if let Some(prev_pc) = cur.runs.last().filter(|pc| pc.text != "\t" && !pc.text.is_empty())
                && let (Some(rb), Some(ra)) = (runs.get(wd.run), runs.get(prev_pc.run))
            {
                let fb = font_of(rb, &env);
                if fb.kern {
                    let fa = if prev_pc.run == wd.run { fb.clone() } else { font_of(ra, &env) };
                    if fa.family == fb.family && fa.size_pt == fb.size_pt && fa.bold == fb.bold && fa.italic == fb.italic {
                        let a = last_unit(&prev_pc.text);
                        let b = wd.text.chars().next().unwrap_or(' ');
                        if u32::from(b) <= 0xFFFF {
                            let mut ab = String::new();
                            ab.push(a);
                            ab.push(b);
                            let wab = measure_emu(m, &ab, &fb);
                            let wa = measure_emu(m, a.encode_utf8(&mut [0; 4]), &fb);
                            let wb = measure_emu(m, b.encode_utf8(&mut [0; 4]), &fb);
                            kp = wab - wa - wb;
                        }
                    }
                }
            }
            // it fits when the width up to the last visible character does not pass the line (no slack, syn-b-e18)
            if wrap && !cur.runs.is_empty() && cur_w + kp + w_core > avail_w {
                // kinsoku (East Asian language and eaLnBrk != 0 — measured, syn-a-tbl9)
                let joined: String = u.iter().map(|pc| pc.text.as_str()).collect();
                let chars: Vec<char> = trim_end(&joined).chars().collect();
                let ea = p.x().ea_ln_brk != Some(false) && ea_lang(runs.get(wd.run).map_or("", |r| r.lang()));
                // 、。 hang past the margin
                if ea && p.x().hanging_punct != Some(false) && chars.len() == 1 && chars.first().is_some_and(|c| KINSOKU_HANG.contains(*c))
                {
                    u_starts.push(cur.runs.len());
                    cur.runs.extend(u.iter().cloned());
                    cur_w += kp + w_emu;
                    continue;
                }
                let last_start = u_starts.last().copied().unwrap_or(0);
                let prev_u: Vec<Piece> = cur.runs.get(last_start..).unwrap_or(&[]).to_vec();
                let prev_joined: String = prev_u.iter().map(|pc| pc.text.as_str()).collect();
                let prev_ch = trim_end(&prev_joined).chars().next_back();
                let prev_ea =
                    p.x().ea_ln_brk != Some(false) && ea_lang(prev_u.last().and_then(|pc| runs.get(pc.run)).map_or("", |r| r.lang()));
                kp = 0.0;
                let no_start = ea && chars.first().is_some_and(|c| KINSOKU_NO_START.contains(*c));
                let no_end = prev_ea && prev_ch.is_some_and(|c| KINSOKU_NO_END.contains(c));
                if u_starts.len() > 1 && prev_u.first().is_none_or(|pc| pc.text != "\t") && (no_start || no_end) {
                    // the previous unit goes down too (it cannot end the line / the next cannot start it)
                    cur.runs.truncate(last_start);
                    u_starts.pop();
                    flush(&mut cur, &mut cur_w, &mut u_starts, &mut first_line_of_para, &mut pending_spacer, &mut last_max_pt);
                    u_starts.push(cur.runs.len());
                    cur.runs.extend(prev_u.iter().cloned());
                    cur_w = unit_w(m, &prev_u, &runs, &env).0;
                } else {
                    flush(&mut cur, &mut cur_w, &mut u_starts, &mut first_line_of_para, &mut pending_spacer, &mut last_max_pt);
                }
                avail_w = avail_of(cur.mar_l);
            }
            if wrap && cur.runs.is_empty() && w_core > avail_w {
                // a word longer than the whole line: break by CHARACTER at the longest prefix that fits, at least one
                // per line (measured, syn-b-e19: "Supercal|ifragilisti|c")
                for pc in u {
                    let Some(r) = runs.get(pc.run) else { continue };
                    let f = font_of(r, &env);
                    let mut buf = String::new();
                    let clusters: Vec<&str> = if caps.segmenter {
                        uo_text::grapheme::graphemes(&pc.text).collect()
                    } else {
                        pc.text.char_indices().map(|(i, c)| pc.text.get(i..i + c.len_utf8()).unwrap_or("")).collect()
                    };
                    for ch in clusters {
                        let cw = measure_run(m, ch, r, &f, sc);
                        if !ch.chars().any(is_js_space) && (!cur.runs.is_empty() || !buf.is_empty()) && cur_w + cw > avail_w {
                            if !buf.is_empty() {
                                u_starts.push(cur.runs.len());
                                cur.runs.push(Piece { text: std::mem::take(&mut buf), run: pc.run });
                            }
                            flush(&mut cur, &mut cur_w, &mut u_starts, &mut first_line_of_para, &mut pending_spacer, &mut last_max_pt);
                            avail_w = avail_of(cur.mar_l);
                        }
                        buf.push_str(ch);
                        cur_w += cw;
                    }
                    if !buf.is_empty() {
                        u_starts.push(cur.runs.len());
                        cur.runs.push(Piece { text: buf, run: pc.run });
                    }
                }
                continue;
            }
            u_starts.push(cur.runs.len());
            cur.runs.extend(u.iter().cloned());
            cur_w += kp + w_emu;
        }
        flush(&mut cur, &mut cur_w, &mut u_starts, &mut first_line_of_para, &mut pending_spacer, &mut last_max_pt);
        drop(flush);
        if let Some(pt) = p.space_after_pt {
            pending_spacer += pt * EMU_PER_POINT;
        } else if let Some(pct) = p.space_after_pct {
            pending_spacer += pct * last_max_pt * EMU_PER_POINT * line_factor;
        }
        para_runs.push(runs);
        tab_ctx.push(tabs);
    }

    // expanded (> 100 %) or exact spacing (measured, syn-a-tbl6 #4/#5 and syn-a-tbl8, every font): the base sits
    // at 0.75 of the spacing from the line top (not 0.8) and the LAST line ends 0.2 × (1.2 × size) below the base,
    // not scaled
    let n_l = heights.len();
    // PowerPoint's baseline in each line and the line's descent (Slides wave 5, `!ts_first_baseline`): known before the
    // anchor, because PowerPoint's block ends one descent below the LAST baseline, whatever the spacing (measured,
    // b3-foot-b/ctr)
    let mut ppt_lines: Vec<Option<(f64, f64)>> = Vec::new();
    if !opts.rules.ts_first_baseline {
        for (i, pl) in proto.iter().enumerate() {
            let runs = proto_runs.get(i).and_then(|&p| para_runs.get(p));
            let mut faces: Vec<(f64, f64)> = Vec::with_capacity(pl.runs.len());
            for pc in &pl.runs {
                if let Some(r) = runs.and_then(|rs| rs.get(pc.run)) {
                    let f = font_of(r, &env);
                    let known = super::measure::known_line_box(&f.family);
                    let asked = if known.is_some() { None } else { m.font_box(&mfont(&f)) };
                    let ratio = match known.or(asked) {
                        Some((a, d)) if a.is_finite() && d.is_finite() && a > 0.0 && d >= 0.0 => a / (a + d),
                        _ => ABSENT_FACE_RATIO,
                    };
                    faces.push((f.size_pt, ratio));
                }
            }
            let (h, sz, le) =
                (heights.get(i).copied().unwrap_or(0.0), line_sz.get(i).copied().unwrap_or(0.0), line_exp.get(i).copied().unwrap_or(0.0));
            ppt_lines.push(if faces.is_empty() { None } else { ppt_baseline(h, sz, le, &faces) });
        }
    }
    let ppt_last = n_l.checked_sub(1).and_then(|i| ppt_lines.get(i).copied().flatten().map(|v| (i, v)));
    let last_adj = match (n_l.checked_sub(1).and_then(|i| line_exp.get(i)), n_l.checked_sub(1)) {
        _ if ppt_last.is_some() => ppt_last.map_or(0.0, |(i, (off, d))| off + d - heights.get(i).copied().unwrap_or(0.0)),
        (Some(&e), Some(i)) if e != 0.0 => {
            (1.0 - BASELINE_FACTOR) * line_sz.get(i).copied().unwrap_or(0.0) * line_factor
                - (1.0 - e) * heights.get(i).copied().unwrap_or(0.0)
        }
        _ => 0.0,
    };
    // vertical anchor (paragraph spacers included); spcFirstLastPara: the spcAft of the LAST paragraph takes height
    let mut total_h = 0.0;
    for h in &heights {
        total_h += *h;
    }
    let mut sp_sum = 0.0;
    for s in &spacers {
        sp_sum += *s;
    }
    let total_h = total_h + sp_sum + if tb.x().spc_first_last_para == Some(true) { pending_spacer } else { 0.0 } + last_adj;
    let usable_h = max(0.0, box_h - ins_t - ins_b);
    let anchor = tb.anchor.as_deref().or(opts.default_anchor.as_deref()).unwrap_or("t");
    let mut y = ins_t;
    // several columns flow from the top (the reference application anchors columns at the top)
    if num_col == 1.0 {
        if anchor == "ctr" {
            y = ins_t + (usable_h - total_h) / 2.0;
        } else if anchor == "b" {
            y = ins_t + usable_h - total_h;
        }
    }
    let mut col_idx = 0.0;

    // materialize the lines: join contiguous pieces of the SAME run, measure and align
    let mut lines: Vec<PlacedLine> = Vec::with_capacity(proto.len());
    let mut proto_it = proto.into_iter().enumerate().peekable();
    while let Some((i, mut pl)) = proto_it.next() {
        let next_para = proto_it.peek().map(|(_, n)| n.para);
        let h = heights.get(i).copied().unwrap_or(0.0);
        y += spacers.get(i).copied().unwrap_or(0.0);
        // overflow to the next column when the usable height runs out
        if num_col > 1.0 && col_idx < num_col - 1.0 && y + h > ins_t + usable_h {
            col_idx += 1.0;
            y = ins_t;
        }
        let col_shift = col_idx * (col_w + spc_col);
        let pr_ix = proto_runs.get(i).copied().unwrap_or(0);
        let (Some(runs), Some(tabs)) = (para_runs.get(pr_ix), tab_ctx.get(pr_ix)) else { continue };
        let mut merged: Vec<Piece> = Vec::new();
        for t in pl.runs.drain(..) {
            match merged.last_mut() {
                // a TAB stays alone (its position is computed on the spot)
                Some(last) if last.run == t.run && t.text != "\t" && last.text != "\t" => last.text.push_str(&t.text),
                _ => merged.push(t),
            }
        }
        // trailing white space of the last piece does not count for the alignment
        if let Some(last) = merged.last_mut() {
            let keep = trim_end(&last.text).len();
            last.text.truncate(keep);
        }
        let mut placed: Vec<PlacedRun> = Vec::with_capacity(merged.len());
        let mut w = 0.0;
        for mi in 0..merged.len() {
            let Some(mm) = merged.get(mi) else { break };
            if mm.text == "\t" {
                let x_rel = pl.mar_l - ins_l + w;
                let seg = seg_of(m, &merged, mi + 1, runs, &env);
                w += tabs.tab_to(x_rel, seg, mi == 0) - x_rel;
                continue;
            }
            let Some(r) = runs.get(mm.run) else { continue };
            let f = font_of(r, &env);
            let mw = measure_run(m, &mm.text, r, &f, sc);
            let ex = r.ex();
            let src_color = r.src.and_then(|s| s.color.clone());
            let link = ex.and_then(|e| e.link_color.clone());
            let underline = r.src.and_then(|s| s.underline).unwrap_or(false) || link.is_some();
            let pr = PlacedRun {
                text: mm.text.clone(),
                x: pl.mar_l + w,
                w: mw,
                font: f,
                color: link.or(src_color).unwrap_or_else(|| default_color.clone()),
                underline,
                hyperlink: ex.and_then(|e| e.hyperlink.clone()),
                shadow: ex.and_then(|e| e.fx().shadow.clone()),
                rise: r.rise.filter(|v| *v != 0.0 && !v.is_nan()).map(|v| v * EMU_PER_POINT * font_scale),
                strike: ex.and_then(|e| e.strike).unwrap_or(false),
                spc: r.spc().filter(|v| *v != 0.0 && !v.is_nan()).map(|v| v * EMU_PER_POINT * sc.extra),
                outline: ex.and_then(|e| e.fx().outline.clone()),
                patt_fill: ex.and_then(|e| e.fx().patt_fill.clone()),
                grad_fill: ex.and_then(|e| e.fx().grad_fill.clone()),
                fill_image: ex.and_then(|e| e.fx().fill_image.clone()),
                fill_tile: ex.and_then(|e| e.fx().fill_image.as_ref().and(e.fx().fill_tile)).unwrap_or(false),
                alpha: ex.and_then(|e| e.alpha),
            };
            placed.push(pr);
            w += mw;
        }
        // horizontal alignment shifts the runs (the bullet stays at marL+indent); with columns relative to the
        // COLUMN width
        let mut dx = 0.0;
        let last_of_para = next_para != Some(pl.para);
        let just = &*pl.align == "just";
        let dist = &*pl.align == "dist";
        if (just || dist) && (!last_of_para || dist) && !placed.is_empty() {
            // justified: the space left goes to the SPACES between words (dist: the last line too)
            let extra = col_w - (pl.mar_l - ins_l) - w;
            // merged.find((m) => m.text === pr.text)?.run ?? merged[0].run
            let run_by_text = |text: &str| -> Option<usize> { merged.iter().find(|x| x.text == text).or(merged.first()).map(|x| x.run) };
            let mut pieces: Vec<PlacedRun> = Vec::new();
            for pr in &placed {
                let toks = space_tokens(&pr.text);
                if toks.len() <= 1 {
                    pieces.push(pr.clone());
                    continue;
                }
                let mut x0 = pr.x;
                let rr = run_by_text(&pr.text).and_then(|ix| runs.get(ix));
                for t in toks {
                    let tw = match rr {
                        Some(r) => measure_run(m, t, r, &pr.font, sc),
                        None => 0.0,
                    };
                    pieces.push(PlacedRun { text: t.to_string(), x: x0, w: tw, ..pr.clone() });
                    x0 += tw;
                }
            }
            // dist with a single word: the space goes between the CHARACTERS (measured, syn-just)
            if dist && pieces.len() == 1 && pieces.first().is_some_and(|p0| p0.text.chars().nth(1).is_some()) {
                let p0 = pieces.remove(0);
                let chars: Vec<char> = p0.text.chars().collect();
                let rr = run_by_text(&p0.text).and_then(|ix| runs.get(ix));
                let mut x0 = p0.x;
                let g = if extra > 0.0 { extra / (chars.len() as f64 - 1.0) } else { 0.0 };
                for ch in chars {
                    let s = ch.to_string();
                    let cw = match rr {
                        Some(r) => measure_run(m, &s, r, &p0.font, sc),
                        None => 0.0,
                    };
                    pieces.push(PlacedRun { text: s, x: x0, w: cw, ..p0.clone() });
                    x0 += cw + g;
                }
                placed = pieces.clone();
            }
            let all_space = |s: &str| !s.is_empty() && s.chars().all(is_js_space);
            let n = pieces.len();
            let gaps = pieces.iter().enumerate().filter(|(k, p)| all_space(&p.text) && k + 1 < n).count();
            if extra > 0.0 && gaps > 0 {
                let g = extra / gaps as f64;
                let mut add = 0.0;
                for (k, p) in pieces.iter_mut().enumerate() {
                    p.x += add;
                    if all_space(&p.text) && k + 1 < n {
                        add += g;
                    }
                }
                placed = pieces.into_iter().filter(|p| !all_space(&p.text)).collect();
            }
        } else if &*pl.align == "ctr" {
            dx = (col_w - (pl.mar_l - ins_l) - w) / 2.0;
        } else if &*pl.align == "r" {
            dx = col_w - (pl.mar_l - ins_l) - w;
        }
        if dx > 0.0 {
            for pr in &mut placed {
                pr.x += dx;
            }
        }
        if col_shift > 0.0 {
            for pr in &mut placed {
                pr.x += col_shift;
            }
            if let Some(b) = &mut pl.bullet {
                b.x += col_shift;
            }
        }
        let le = line_exp.get(i).copied().unwrap_or(0.0);
        let ts_base = h * if le != 0.0 { le } else { BASELINE_FACTOR };
        let base = ppt_lines.get(i).copied().flatten().map_or(ts_base, |(off, _)| off);
        lines.push(PlacedLine {
            runs: placed,
            top: y,
            baseline: y + base,
            height: h,
            bullet: pl.bullet.take(),
            para: u32::try_from(pl.para).unwrap_or(u32::MAX),
        });
        y += h;
    }
    Ok(TextLayoutResult { lines, total_h })
}

/// The ascent share of a face PowerPoint has no metrics for: it draws an absent family with Calibri's metrics
/// (`usWinAscent` 1950 / `usWinDescent` 550; measured, `browser-font-facts`).
const ABSENT_FACE_RATIO: f64 = 1950.0 / 2500.0;

/// PowerPoint's baseline in a line (Slides wave 5, measured: `uo-dml-render.md` "First-line baseline"), from the line
/// top, EMU. `h` = the line height, `line_sz` = the largest size of the line (EMU), `le` = the TS factor of expanded or
/// exact spacing (0 = single or reduced), `faces` = `(size pt, ascent share)` of each run of the line. Each face's
/// single line (1.2 × size) splits at its ascent share `r` = ascent / (ascent + descent) (win metrics, or typo + gap
/// with `USE_TYPO_METRICS`); the line takes the largest ascent `A` and the largest descent `D` of its faces, normalized
/// to 1.2 × the largest size: `A' = 1.2·M·A/(A+D)`, `D' = 1.2·M − A'`. Then, with the descent overshoot
/// `x = max(0, D' − 0.3·M)` (0.3·M = the TS descent: 0.25 of a single line):
/// - single or reduced spacing (`h ≤ 1.2·M`): `max(0.75·h − x, A' − (1.2·M − h))` (= `A'` at 100 %);
/// - expanded or exact: `le·h − x` (`le` = 0.75, the TS factor; a face whose descent overshoots sits higher).
///
/// Returns the baseline offset and `D'`, the descent the block keeps below the last baseline (every spacing, measured).
/// Mixed faces under reduced and expanded spacing measured too (b3-mix-80/150/x50, 12 cases within the drift).
fn ppt_baseline(h: f64, line_sz: f64, le: f64, faces: &[(f64, f64)]) -> Option<(f64, f64)> {
    let (mut a, mut d) = (0.0_f64, 0.0_f64);
    for &(sz, r) in faces {
        if !(sz.is_finite() && sz > 0.0 && r.is_finite()) {
            continue;
        }
        a = max(a, 1.2 * sz * r);
        d = max(d, 1.2 * sz * (1.0 - r));
    }
    if a + d <= 0.0 || line_sz.is_nan() || line_sz <= 0.0 {
        return None;
    }
    let single = 1.2 * line_sz;
    let a1 = single * a / (a + d);
    let x = max(0.0, (single - a1) - 0.3 * line_sz);
    let off = if le != 0.0 { le * h - x } else { max(0.75 * h - x, a1 - (single - h)) };
    Some((off, single - a1))
}

/// `text.match(/\s+|\S+/g) ?? [text]`: alternating runs of JS white space and of the rest.
fn space_tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut it = text.char_indices().peekable();
    while let Some((_, c)) = it.next() {
        let sp = is_js_space(c);
        if matches!(it.peek(), Some(&(_, d)) if is_js_space(d) == sp) {
            continue;
        }
        let end = it.peek().map_or(text.len(), |&(j, _)| j);
        out.push(text.get(start..end).unwrap_or(""));
        start = end;
    }
    if out.is_empty() {
        out.push(text);
    }
    out
}

/// `autoNumCounters` (text-layout.ts:315): a 9-element array indexed by the paragraph level, which the reader gives
/// as `parseInt(lvl)` clamped by `Math.min(…, 8)` — so a negative or `NaN` level indexes a PROPERTY of the array
/// that persists like an element. Keyed by the number as `String(level)` names it (`-0` is `0`, every `NaN` is
/// one key); absent reads as `?? 0`.
#[derive(Default)]
struct Counters(Vec<(u64, i64)>);

impl Counters {
    fn key(level: f64) -> u64 {
        if level.is_nan() {
            f64::NAN.to_bits()
        } else if level == 0.0 {
            0
        } else {
            level.to_bits()
        }
    }

    fn get(&self, level: f64) -> i64 {
        let k = Self::key(level);
        self.0.iter().find(|e| e.0 == k).map_or(0, |e| e.1)
    }

    fn set(&mut self, level: f64, v: i64) {
        let k = Self::key(level);
        match self.0.iter_mut().find(|e| e.0 == k) {
            Some(e) => e.1 = v,
            None => self.0.push((k, v)),
        }
    }

    /// `for (let l = from; l < 9; l++) counters[l] = 0`: every key the loop reaches (`from`, `from + 1`, … below 9).
    /// Absent keys already read as 0, so only present ones change; a `NaN` start runs no iteration.
    fn reset_from(&mut self, from: f64) {
        for e in &mut self.0 {
            let k = f64::from_bits(e.0);
            if !k.is_nan() && k >= from && k < 9.0 && (k - from).fract() == 0.0 {
                e.1 = 0;
            }
        }
    }
}
