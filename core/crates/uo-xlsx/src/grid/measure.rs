//! Cell fonts as Excel measures them (`sheet/src/text-metrics.ts` `CellFonts`): ppem at an integer size
//! (11 pt → 15 px, not 14.67), the width of each glyph in whole px (the GDI table `gdiGlyphFixes`, or the shell's
//! canvas advance rounded), no kerning; scripts that need shaping (Arabic, Hebrew, Indic, Thai…) are measured as a
//! whole string.
//!
//! **Two phases** (as Slides' `NeedMeasures`): Rust owns the GDI tables; what only the shell can measure — the
//! advance of a character the GDI table does not list, the `'Hg'` font box, a whole complex-script string — is
//! asked for. A width that is missing answers `0` and the table records the request ([`MeasureTable::needs`]); the
//! shell answers ([`MeasureTable::provide_advance`] …, or [`MeasureTable::satisfy`] with a [`Measurer`] on hosts
//! that can measure natively) and the frame is built again. The table persists across frames, so steady
//! scrolling never round-trips. Caps (T12 M5): 64 fonts and 4,096 non-ASCII characters per font; the table is
//! cleared at [`MeasureTable::begin_frame`] when it passed a cap or the font set changed.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::metrics::{excel_row_px, gdi_glyph_fixes, ppem_of, round_i32, row_descent_px};
use super::text::FontMetrics;
use uo_text::utf16::code_points;

/// Fonts kept before the table is cleared (T12 M5).
pub const MAX_FONTS: usize = 64;
/// Non-ASCII advances kept per font.
pub const MAX_CHARS_PER_FONT: usize = 4096;
/// Whole strings (complex scripts) kept per font.
pub const MAX_STRINGS_PER_FONT: usize = 256;
/// Fonts the table refuses past (a hostile style sheet): the extra font measures with font 0.
const HARD_FONTS: usize = 4096;

/// Scripts that depend on shaping/bidi (`COMPLEX`): measured and drawn as a whole string.
#[must_use]
pub fn is_complex(s: &[u16]) -> bool {
    s.iter().any(|&u| {
        matches!(u, 0x0590..=0x08ff | 0x0900..=0x0dff | 0x0e00..=0x0eff | 0x0f00..=0x109f | 0x1780..=0x18af | 0xfb1d..=0xfdff | 0xfe70..=0xfeff)
    })
}

/// A cell font as the style sheet names it (`CellFonts.get`'s arguments).
#[derive(Clone, Debug, PartialEq)]
pub struct FontRequest<'a> {
    /// Family name (`font?.name ?? normal.name`).
    pub name: &'a str,
    /// Written name when the font follows the theme and differs (the CSS stack's first fallback).
    pub alt_name: Option<&'a str>,
    /// Size in pt (`font?.size ?? 11`).
    pub size_pt: f64,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Zoom.
    pub zoom: f64,
}

/// What the shell needs to build the CSS font of a [`FontId`]: the request plus the derived ppem and the family
/// whose GDI widths apply.
#[derive(Clone, Debug, PartialEq)]
pub struct FontDesc {
    /// Family name.
    pub name: Box<str>,
    /// Alternate (theme) name.
    pub alt_name: Option<Box<str>>,
    /// Size in pt.
    pub size_pt: f64,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Zoom.
    pub zoom: f64,
    /// `ppemOf(size × zoom)`: the CSS font is `[italic ]{400|700} {ppem}px <stack of name, alt_name, Arial>`.
    pub ppem: i32,
    /// Family whose GDI widths and metrics apply (the requested one, or `arial` when the browser fell back).
    pub metric: Box<str>,
}

/// The shell's answer to "which family's widths apply to this name" (`r.source === 'default' ? 'arial' : name`).
pub trait FontEnv {
    /// The metric family for the font name.
    fn metric_family(&self, name: &str) -> String;
}

/// The environment of a host that has the font itself (or the tests): the name is the family.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentityEnv;

impl FontEnv for IdentityEnv {
    fn metric_family(&self, name: &str) -> String {
        name.to_owned()
    }
}

/// A font of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId(pub u32);

#[derive(Debug)]
struct TableFont {
    desc: FontDesc,
    /// The GDI corrections decoded: (code point, px), sorted.
    fixes: Box<[(u32, u8)]>,
    /// Measured advances of ASCII (−1 = not measured).
    ascii: [i16; 128],
    other: HashMap<u32, i32>,
    strings: HashMap<Vec<u16>, i32>,
    /// `'Hg'` box (ascent, descent) rounded, when the shell gave it.
    hg_box: Option<(i32, i32)>,
}

/// What the shell must still measure (two-phase). `texts` with one unit are character advances.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeedMeasures {
    /// `(font id, UTF-16 text)`: a one-unit text is a character advance; a code point outside the BMP is its two
    /// units.
    pub texts: Vec<(u32, Vec<u16>)>,
    /// Fonts whose `'Hg'` box is missing.
    pub font_boxes: Vec<u32>,
    /// The Normal font's MDW is not in the Excel table: measure the digits `0`-`9` of font 0 (the Normal font) at
    /// the Normal size and call [`super::style::StyleTable::set_canvas_mdw`].
    pub canvas_mdw: bool,
}

impl NeedMeasures {
    /// Nothing is missing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty() && self.font_boxes.is_empty() && !self.canvas_mdw
    }
}

#[derive(Debug, Default)]
struct Pending {
    texts: Vec<(u32, Vec<u16>)>,
    boxes: Vec<u32>,
    seen: HashSet<(u32, u32)>,
    seen_text: HashSet<(u32, Vec<u16>)>,
}

/// The persistent measurement table of the cell fonts.
#[derive(Debug, Default)]
pub struct MeasureTable {
    fonts: Vec<TableFont>,
    pending: RefCell<Pending>,
    /// A cap was passed: cleared at the next [`Self::begin_frame`].
    over_cap: bool,
}

/// A native measurer (hosts without a browser canvas; the tests).
pub trait Measurer {
    /// `Math.round(measureText(ch).width)` of a code point in the font.
    fn advance(&self, font: &FontDesc, cp: u32) -> i32;
    /// `Math.round(measureText(s).width)` of a whole string.
    fn text_width(&self, font: &FontDesc, s: &[u16]) -> i32;
    /// `fontBoundingBoxAscent` and `fontBoundingBoxDescent` of `'Hg'` (`None` = the browser lacks them).
    fn font_box(&self, font: &FontDesc) -> (Option<f64>, Option<f64>);
}

impl MeasureTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fonts held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    /// No font held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// Bytes of heap held (estimate for the memory gates).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.fonts
            .iter()
            .map(|f| std::mem::size_of::<TableFont>() + f.other.len() * 16 + f.strings.keys().map(|k| k.len() * 2 + 32).sum::<usize>())
            .sum()
    }

    /// Forgets the measurements (fonts loaded or changed: `CellFonts.clear`).
    pub fn clear(&mut self) {
        self.fonts.clear();
        *self.pending.borrow_mut() = Pending::default();
        self.over_cap = false;
    }

    /// Start of a frame: clears the table when a cap was passed (T12 M5).
    pub fn begin_frame(&mut self) {
        if self.over_cap || self.fonts.len() > MAX_FONTS {
            self.clear();
        }
    }

    /// The font for a request (`CellFonts.get`): created on first use.
    pub fn font(&mut self, req: &FontRequest<'_>, env: &dyn FontEnv) -> FontId {
        if let Some(i) = self.fonts.iter().position(|f| {
            f.desc.name.as_ref() == req.name
                && f.desc.alt_name.as_deref() == req.alt_name
                && f.desc.size_pt.to_bits() == req.size_pt.to_bits()
                && f.desc.bold == req.bold
                && f.desc.italic == req.italic
                && f.desc.zoom.to_bits() == req.zoom.to_bits()
        }) {
            return FontId(u32::try_from(i).unwrap_or(0));
        }
        if self.fonts.len() >= HARD_FONTS {
            return FontId(0);
        }
        if self.fonts.len() >= MAX_FONTS {
            self.over_cap = true;
        }
        let ppem = ppem_of(req.size_pt * req.zoom);
        let metric = env.metric_family(req.name);
        let mut fix_map = std::collections::BTreeMap::new();
        if let Some(g) = gdi_glyph_fixes(&metric, req.bold, ppem) {
            for (c, w) in g.pairs() {
                fix_map.insert(u32::from(c), u8::try_from(w).unwrap_or(0));
            }
        }
        let fixes: Box<[(u32, u8)]> = fix_map.into_iter().collect();
        let desc = FontDesc {
            name: req.name.into(),
            alt_name: req.alt_name.map(Into::into),
            size_pt: req.size_pt,
            bold: req.bold,
            italic: req.italic,
            zoom: req.zoom,
            ppem,
            metric: metric.into(),
        };
        self.fonts.push(TableFont { desc, fixes, ascii: [-1; 128], other: HashMap::new(), strings: HashMap::new(), hg_box: None });
        FontId(u32::try_from(self.fonts.len() - 1).unwrap_or(0))
    }

    /// The description of a font (what the shell needs to build its CSS font).
    #[must_use]
    pub fn desc(&self, id: FontId) -> Option<&FontDesc> {
        self.fonts.get(id.0 as usize).map(|f| &f.desc)
    }

    /// The metrics of a font for the layout functions.
    #[must_use]
    pub fn metrics(&self, id: FontId) -> CellFont<'_> {
        CellFont { table: self, id }
    }

    /// The shell's advance (`Math.round(measureText(ch).width)`) of code point `cp`.
    pub fn provide_advance(&mut self, id: FontId, cp: u32, px: i32) {
        let Some(f) = self.fonts.get_mut(id.0 as usize) else { return };
        if cp < 128 {
            f.ascii[cp as usize] = i16::try_from(px).unwrap_or(i16::MAX);
        } else if f.other.len() < MAX_CHARS_PER_FONT {
            f.other.insert(cp, px);
        }
        self.pending.borrow_mut().seen.remove(&(id.0, cp));
    }

    /// The shell's width of a whole string (complex scripts).
    pub fn provide_text(&mut self, id: FontId, text: &[u16], px: i32) {
        let Some(f) = self.fonts.get_mut(id.0 as usize) else { return };
        if f.strings.len() < MAX_STRINGS_PER_FONT {
            f.strings.insert(text.to_vec(), px);
        }
        self.pending.borrow_mut().seen_text.remove(&(id.0, text.to_vec()));
    }

    /// The shell's `'Hg'` box (`fontBoundingBoxAscent/Descent`); `None` = absent (the TS defaults: 0.9 and 0.22 of
    /// the ppem).
    pub fn provide_box(&mut self, id: FontId, ascent: Option<f64>, descent: Option<f64>) {
        let Some(f) = self.fonts.get_mut(id.0 as usize) else { return };
        let p = f64::from(f.desc.ppem);
        f.hg_box = Some((round_i32(ascent.unwrap_or(p * 0.9)), round_i32(descent.unwrap_or(p * 0.22))));
        self.pending.borrow_mut().boxes.retain(|&b| b != id.0);
    }

    /// What is still missing since the last [`Self::take_needs`] (the frame built with zeros for these).
    pub fn take_needs(&mut self) -> NeedMeasures {
        let mut p = self.pending.borrow_mut();
        let texts = std::mem::take(&mut p.texts);
        let font_boxes = std::mem::take(&mut p.boxes);
        // requests answered later must be asked again if still missing: forget what was handed out
        p.seen.clear();
        p.seen_text.clear();
        NeedMeasures { texts, font_boxes, canvas_mdw: false }
    }

    /// Whether anything is missing.
    #[must_use]
    pub fn has_needs(&self) -> bool {
        let p = self.pending.borrow();
        !p.texts.is_empty() || !p.boxes.is_empty()
    }

    /// Answers every pending request with a native measurer.
    pub fn satisfy(&mut self, m: &dyn Measurer) {
        let needs = self.take_needs();
        for (fid, text) in needs.texts {
            let id = FontId(fid);
            let Some(desc) = self.desc(id).cloned() else { continue };
            let mut it = code_points(&text);
            let single = match (it.next(), it.next()) {
                (Some(cp), None) => Some(cp),
                _ => None,
            };
            match single {
                Some(cp) => {
                    // a character's advance and, for a complex-script character, also its width as a string
                    self.provide_advance(id, cp, m.advance(&desc, cp));
                    if is_complex(&text) {
                        self.provide_text(id, &text, m.text_width(&desc, &text));
                    }
                }
                None => self.provide_text(id, &text, m.text_width(&desc, &text)),
            }
        }
        for fid in needs.font_boxes {
            let id = FontId(fid);
            let Some(desc) = self.desc(id).cloned() else { continue };
            let (a, d) = m.font_box(&desc);
            self.provide_box(id, a, d);
        }
    }

    fn ask_char(&self, id: u32, cp: u32) {
        let mut p = self.pending.borrow_mut();
        if p.seen.insert((id, cp)) {
            let mut t = Vec::with_capacity(2);
            uo_text::utf16::push(&mut t, cp);
            p.texts.push((id, t));
        }
    }

    fn ask_text(&self, id: u32, s: &[u16]) {
        let mut p = self.pending.borrow_mut();
        if p.seen_text.insert((id, s.to_vec())) {
            p.texts.push((id, s.to_vec()));
        }
    }

    fn ask_box(&self, id: u32) {
        let mut p = self.pending.borrow_mut();
        if !p.boxes.contains(&id) {
            p.boxes.push(id);
        }
    }
}

/// The metrics of one font of a [`MeasureTable`] (`CellFont`).
#[derive(Clone, Copy)]
pub struct CellFont<'a> {
    table: &'a MeasureTable,
    id: FontId,
}

impl CellFont<'_> {
    fn font(&self) -> Option<&TableFont> {
        self.table.fonts.get(self.id.0 as usize)
    }

    /// The font id.
    #[must_use]
    pub fn id(&self) -> FontId {
        self.id
    }

    /// Bold.
    #[must_use]
    pub fn bold(&self) -> bool {
        self.font().is_some_and(|f| f.desc.bold)
    }

    /// Italic.
    #[must_use]
    pub fn italic(&self) -> bool {
        self.font().is_some_and(|f| f.desc.italic)
    }

    /// Ascent and descent of `'Hg'` (px, from the canvas); asks the shell when it is not known.
    #[must_use]
    pub fn hg_box(&self) -> (i32, i32) {
        match self.font().and_then(|f| f.hg_box) {
            Some(b) => b,
            None => {
                self.table.ask_box(self.id.0);
                let p = self.ppem();
                (round_i32(f64::from(p) * 0.9), round_i32(f64::from(p) * 0.22))
            }
        }
    }

    /// Excel's baseline: top + `xl_ascent` (top aligned); bottom − `xl_descent` (bottom aligned): the line height
    /// of the font minus the GDI descent above the base, the descent below it; a family without a descent falls
    /// back to the canvas box (`ascent + 1`, `descent + 2`). Returns `(xl_ascent, xl_descent)`.
    #[must_use]
    pub fn xl_ascent_descent(&self) -> (i32, i32) {
        let Some(f) = self.font() else { return (0, 0) };
        let size = f.desc.size_pt * f.desc.zoom;
        match row_descent_px(Some(&f.desc.metric), size, f.desc.bold) {
            Some(d) => (excel_row_px(Some(&f.desc.metric), size, f.desc.bold) - d, d),
            None => {
                let (a, d) = self.hg_box();
                (a + 1, d + 2)
            }
        }
    }
}

impl FontMetrics for CellFont<'_> {
    fn ppem(&self) -> i32 {
        self.font().map_or(1, |f| f.desc.ppem)
    }

    fn adv(&self, cp: u32) -> i32 {
        let Some(f) = self.font() else { return 0 };
        if let Ok(i) = f.fixes.binary_search_by_key(&cp, |&(c, _)| c) {
            return f.fixes.get(i).map_or(0, |&(_, w)| i32::from(w));
        }
        if cp < 128 {
            let w = f.ascii.get(cp as usize).copied().unwrap_or(-1);
            if w >= 0 {
                return i32::from(w);
            }
        } else if let Some(&w) = f.other.get(&cp) {
            return w;
        }
        self.table.ask_char(self.id.0, cp);
        0
    }

    fn width(&self, s: &[u16], limit: f64) -> f64 {
        if is_complex(s) {
            if let Some(w) = self.font().and_then(|f| f.strings.get(s)) {
                return f64::from(*w);
            }
            self.table.ask_text(self.id.0, s);
            return 0.0;
        }
        let mut w = 0.0;
        for cp in code_points(s) {
            w += f64::from(self.adv(cp));
            if w > limit {
                return w;
            }
        }
        w
    }
}
