//! Resolved styles: one [`ResolvedXf`] per `cellXfs` index, built once per workbook and extended when the XfPool
//! adds an xf. The TS resolves `fonts[xf.fontId]` … for every painted cell (`universal-sheet.ts` `resolveStyle`) and
//! recomputes the row facts of every font per cell (`fitRowHeights`); here the index is the key and the facts are
//! computed once. The Normal font (`fonts[0]`) gives the MDW, the default row height and the default descent
//! (`universal-sheet.ts:566-576`).
//!
//! The layers above a cell's xf — table/pivot style, conditional format — are the C3b/C3c frontiers; a cell is
//! `ResolvedXf` + those layers per viewport.

use super::metrics::{excel_mdw, excel_row_px, measured_family, ppem_of, round_i32, row_descent_px};
use crate::read::model::{CellXf, Font, Styles};
use uo_numfmt::{builtin_format_code, pt_br};

/// Horizontal alignment of the xf (`alignment horizontal`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HAlign {
    /// No alignment attribute.
    #[default]
    None,
    /// `general`.
    General,
    /// `left`.
    Left,
    /// `center`.
    Center,
    /// `right`.
    Right,
    /// `fill`.
    Fill,
    /// `justify`.
    Justify,
    /// `centerContinuous`.
    CenterContinuous,
    /// `distributed`.
    Distributed,
    /// Any other text.
    Other,
}

/// Vertical alignment of the xf.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VAlign {
    /// No attribute (bottom).
    #[default]
    None,
    /// `top`.
    Top,
    /// `center`.
    Center,
    /// `bottom`.
    Bottom,
    /// `justify`.
    Justify,
    /// `distributed`.
    Distributed,
    /// Any other text.
    Other,
}

fn halign(s: Option<&str>) -> HAlign {
    match s {
        None => HAlign::None,
        Some("general") => HAlign::General,
        Some("left") => HAlign::Left,
        Some("center") => HAlign::Center,
        Some("right") => HAlign::Right,
        Some("fill") => HAlign::Fill,
        Some("justify") => HAlign::Justify,
        Some("centerContinuous") => HAlign::CenterContinuous,
        Some("distributed") => HAlign::Distributed,
        Some(_) => HAlign::Other,
    }
}

fn valign(s: Option<&str>) -> VAlign {
    match s {
        None => VAlign::None,
        Some("top") => VAlign::Top,
        Some("center") => VAlign::Center,
        Some("bottom") => VAlign::Bottom,
        Some("justify") => VAlign::Justify,
        Some("distributed") => VAlign::Distributed,
        Some(_) => VAlign::Other,
    }
}

/// The row facts of a font for the automatic row height (`pxOf`, `descOf`, `row0Desc` of `fitRowHeights`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FitFacts {
    /// Height of a line of text in the font (`excelRowPx` + the super/subscript extra).
    pub px: i32,
    /// Descent for the shared baseline + the super/subscript extra (`descOf`); `None` = no known descent.
    pub desc: Option<i32>,
    /// Descent of the font alone, 0 when unknown (`row0Desc`: the PAINT base of the row).
    pub paint_desc: i32,
    /// Index into `styles.fonts` of the font the facts come from (`None`: `fonts[i]` was undefined).
    pub font_ix: Option<u32>,
}

/// A resolved `cellXfs` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedXf {
    /// The xf exists (a cell whose `s` is outside `cellXfs` resolves to an empty style).
    pub exists: bool,
    /// Index into `styles.fonts` (`None` = no font: `fonts[fontId]` undefined).
    pub font: Option<u32>,
    /// Index into `styles.fills`.
    pub fill: Option<u32>,
    /// Index into `styles.borders`.
    pub border: Option<u32>,
    /// `numFmtId` as the file gave it.
    pub num_fmt_id: f64,
    /// The number-format code: `numFmts.get(id) ?? builtinFormatCode(id) ?? 'General'`.
    pub code: Box<str>,
    /// The same code as UTF-16 (what `uo_numfmt` takes).
    pub code16: Box<[u16]>,
    /// Horizontal alignment.
    pub align: HAlign,
    /// Vertical alignment.
    pub valign: VAlign,
    /// `wrapText`.
    pub wrap: bool,
    /// Indent levels.
    pub indent: f64,
    /// Rotation (0 = horizontal; 1–90 counterclockwise, 91–180 clockwise, 255 stacked).
    pub rotation: f64,
    /// Row facts of the xf's font.
    pub fit: FitFacts,
}

impl Default for ResolvedXf {
    fn default() -> Self {
        ResolvedXf {
            exists: false,
            font: None,
            fill: None,
            border: None,
            num_fmt_id: 0.0,
            code: "General".into(),
            code16: "General".encode_utf16().collect(),
            align: HAlign::None,
            valign: VAlign::None,
            wrap: false,
            indent: 0.0,
            rotation: 0.0,
            fit: FitFacts::default(),
        }
    }
}

/// The Normal font (`styles.fonts[0]`, defaults Calibri 11).
#[derive(Clone, Debug, PartialEq)]
pub struct NormalFont {
    /// Family.
    pub name: String,
    /// Size (pt).
    pub size: f64,
    /// Bold.
    pub bold: bool,
}

/// The resolved styles of a workbook.
#[derive(Clone, Debug)]
pub struct StyleTable {
    /// One per `cellXfs` index.
    pub xfs: Vec<ResolvedXf>,
    /// An index outside `cellXfs` in the row fit: `fonts[0]` (the TS `xfs[i]?.fontId ?? 0`).
    pub fit_missing: FitFacts,
    /// Normal font.
    pub normal: NormalFont,
    /// MDW (px) of the Normal font.
    pub mdw: i32,
    /// The MDW came from the canvas (family outside the measured table): the shell measures it
    /// ([`Self::set_canvas_mdw`]).
    pub mdw_from_canvas: bool,
    /// Height (px) of a row with only the Normal font.
    pub normal_row_px: i32,
    /// `descOf(Normal)`.
    pub normal_desc: Option<i32>,
    /// `row0Desc(Normal)`: the paint base of a row with only the Normal font.
    pub normal_paint_desc: i32,
    /// Some xf wraps text (the row fit skips the text path when none does).
    pub any_wrap: bool,
    custom_codes: Vec<(f64, String)>,
}

/// `vaExtra`: super/subscript add to the auto-fit (measured: Arial 10 +2/+4, Calibri 11 +3/+4, Arial 20 +4/+6),
/// approximated by the ppem.
fn va_extra(size: f64, va: Option<&str>) -> i32 {
    match va {
        Some("superscript") => round_i32(f64::from(ppem_of(size)) * 0.17),
        Some("subscript") => round_i32(f64::from(ppem_of(size)) * 0.25),
        _ => 0,
    }
}

impl StyleTable {
    /// Builds the table for a workbook's styles.
    #[must_use]
    pub fn build(styles: &Styles) -> Self {
        let f0 = styles.fonts.first();
        let normal = NormalFont {
            name: f0.and_then(|f| f.name.clone()).unwrap_or_else(|| "Calibri".to_owned()),
            size: f0.and_then(|f| f.size).unwrap_or(11.0),
            bold: f0.is_some_and(|f| f.bold),
        };
        let table_mdw = excel_mdw(Some(&normal.name), normal.size);
        let mdw_from_canvas = table_mdw.is_none() || measured_family(Some(&normal.name)).is_none();
        // the canvas estimate is the shell's; until it answers the table value (the Arial one) stands, else 7
        let mdw = table_mdw.unwrap_or(7);
        let normal_row_px = excel_row_px(Some(&normal.name), normal.size, normal.bold);
        let mut t = StyleTable {
            xfs: Vec::with_capacity(styles.cell_xfs.len()),
            fit_missing: FitFacts::default(),
            normal_desc: row_descent_px(Some(&normal.name), normal.size, normal.bold),
            normal_paint_desc: row_descent_px(Some(&normal.name), normal.size, normal.bold).unwrap_or(0),
            normal,
            mdw,
            mdw_from_canvas,
            normal_row_px,
            any_wrap: false,
            custom_codes: Vec::new(),
        };
        t.fit_missing = FitFacts { font_ix: f0.map(|_| 0), ..t.facts(f0) };
        t.sync(styles);
        t
    }

    /// Resolves the xfs the XfPool added since the last call (and the number formats it added).
    pub fn sync(&mut self, styles: &Styles) {
        self.custom_codes.clone_from(&styles.num_fmts);
        // the facts of fonts[0] may have changed with an edited Normal font: not handled (the TS rebuilds at load)
        for i in self.xfs.len()..styles.cell_xfs.len() {
            let r = self.resolve(styles, &styles.cell_xfs[i]);
            self.any_wrap |= r.wrap;
            self.xfs.push(r);
        }
    }

    /// The font facts of a (possibly missing) font (`pxOf`, `descOf`, `row0Desc`).
    fn facts(&self, f: Option<&Font>) -> FitFacts {
        self.facts_raw(f.and_then(|f| f.name.as_deref()), f.and_then(|f| f.size), f.is_some_and(|f| f.bold), f.and_then(|f| f.vert_align))
    }

    /// The facts of a font given by its parts, each defaulting to the Normal font (a rich run's properties over
    /// its cell's font).
    #[must_use]
    pub fn facts_raw(&self, name: Option<&str>, size: Option<f64>, bold: bool, va: Option<&str>) -> FitFacts {
        let nf = &self.normal;
        let name = name.unwrap_or(&nf.name);
        let size = size.unwrap_or(nf.size);
        let extra = va_extra(size, va);
        let d = row_descent_px(Some(name), size, bold);
        FitFacts { px: excel_row_px(Some(name), size, bold) + extra, desc: d.map(|d| d + extra), paint_desc: d.unwrap_or(0), font_ix: None }
    }

    fn resolve(&self, styles: &Styles, xf: &CellXf) -> ResolvedXf {
        let idx = |v: f64, len: usize| uo_jsnum::conv::js_index(v, len).and_then(|i| u32::try_from(i).ok());
        let font = idx(xf.font_id, styles.fonts.len());
        let fit = FitFacts { font_ix: font, ..self.facts(font.and_then(|i| styles.fonts.get(i as usize))) };
        let code = self.code_of(xf.num_fmt_id);
        ResolvedXf {
            exists: true,
            font,
            fill: idx(xf.fill_id, styles.fills.len()),
            border: idx(xf.border_id, styles.borders.len()),
            num_fmt_id: xf.num_fmt_id,
            code16: code.encode_utf16().collect(),
            code: code.into(),
            align: halign(xf.horizontal.as_deref()),
            valign: valign(xf.vertical.as_deref()),
            wrap: xf.wrap_text,
            indent: xf.indent.unwrap_or(0.0),
            rotation: xf.text_rotation.unwrap_or(0.0),
            fit,
        }
    }

    /// `styles.numFmts.get(id) ?? builtinFormatCode(id) ?? 'General'` (the last definition of an id wins).
    #[must_use]
    pub fn code_of(&self, id: f64) -> String {
        if let Some((_, c)) = self.custom_codes.iter().rev().find(|(k, _)| *k == id) {
            return c.clone();
        }
        let builtin = if id >= 0.0 && id.fract() == 0.0 && id < 4_294_967_296.0 {
            builtin_format_code(uo_jsnum::conv::to_uint32(id), pt_br())
        } else {
            None
        };
        builtin.unwrap_or("General").to_owned()
    }

    /// The resolved xf of a cell's style index (`xfIndexAt` result; any double). An index outside `cellXfs` is
    /// the empty style.
    #[must_use]
    pub fn xf(&self, index: f64) -> &ResolvedXf {
        static EMPTY: std::sync::OnceLock<ResolvedXf> = std::sync::OnceLock::new();
        match uo_jsnum::conv::js_index(index, self.xfs.len()).and_then(|i| self.xfs.get(i)) {
            Some(x) => x,
            None => EMPTY.get_or_init(ResolvedXf::default),
        }
    }

    /// The row facts for a style index in the AUTO-FIT: a missing xf reads `fonts[0]`.
    #[must_use]
    pub fn fit_of(&self, index: f64) -> FitFacts {
        match uo_jsnum::conv::js_index(index, self.xfs.len()).and_then(|i| self.xfs.get(i)) {
            Some(x) => x.fit,
            None => self.fit_missing,
        }
    }

    /// The canvas MDW of a Normal font outside the measured tables (`canvasMdw`): the widest of the digits
    /// `0`-`9` as the shell measured it (`measureText().width`, not rounded per digit), rounded, at least 1.
    pub fn set_canvas_mdw(&mut self, widest_digit_width: f64) {
        self.mdw = round_i32(widest_digit_width).max(1);
    }

    /// Bytes of heap held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.xfs.capacity() * std::mem::size_of::<ResolvedXf>() + self.xfs.iter().map(|x| x.code.len()).sum::<usize>()
    }
}
