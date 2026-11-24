//! Table styles (ListObjects) and pivot styles (`table-styles.ts`, `table-styles-data.ts`, `pivot-styles.ts`): the
//! definition of a style by element (the built-in ones measured in Excel — [`data`] — or the custom `tableStyles` of
//! `styles.xml` + dxfs) and the formatting each cell receives.
//!
//! Rules (measured in the oracle: workbooks `t-tblA/B/over`):
//! - the elements overlap in this order (the later wins per property): wholeTable, column stripes, row stripes,
//!   lastColumn, firstColumn, headerRow, totalRow, the corner cells (first/last header/total cell);
//! - the borders of an element hold on the OUTLINE of its region (left/right/top/bottom) and between its cells
//!   (vertical/horizontal);
//! - the cell's DIRECT formatting beats the table's: its own fill, its own border per side (a table border never mixes
//!   with the cell's: on a boundary any explicit border of either cell wins), its own font colour only when it
//!   DIFFERS from the Normal font's colour; bold/italic add.
//!
//! Nothing is computed per sheet: a sheet holds only the regions ([`TableRegion`], [`pivot::PivotRegion`]) and their
//! resolved specs; the format of a cell is evaluated on demand ([`table_cell_format`], [`pivot::pivot_cell_format`]) for
//! the cells of a viewport.

#[rustfmt::skip]
pub mod data;
pub mod pivot;

use super::paint_rules::{BStyle, Color, Edge, FillRef, PaintFill, Palette};
use crate::read::model::{ColorContext, Dxf, Styles, TableInfo};
use crate::read::theme::apply_tint;

/// The elements of a table/pivot style the code reads (`TableStyleElementType` names; any other name in a custom style
/// is kept by the TS and never read).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum El {
    /// `wholeTable`.
    WholeTable,
    /// `firstColumnStripe`.
    FirstColumnStripe,
    /// `secondColumnStripe`.
    SecondColumnStripe,
    /// `firstRowStripe`.
    FirstRowStripe,
    /// `secondRowStripe`.
    SecondRowStripe,
    /// `lastColumn`.
    LastColumn,
    /// `firstColumn`.
    FirstColumn,
    /// `headerRow`.
    HeaderRow,
    /// `totalRow`.
    TotalRow,
    /// `firstHeaderCell`.
    FirstHeaderCell,
    /// `lastHeaderCell`.
    LastHeaderCell,
    /// `firstTotalCell`.
    FirstTotalCell,
    /// `lastTotalCell`.
    LastTotalCell,
    /// `firstSubtotalColumn`.
    FirstSubtotalColumn,
    /// `secondSubtotalColumn`.
    SecondSubtotalColumn,
    /// `thirdSubtotalColumn`.
    ThirdSubtotalColumn,
    /// `firstSubtotalRow`.
    FirstSubtotalRow,
    /// `secondSubtotalRow`.
    SecondSubtotalRow,
    /// `thirdSubtotalRow`.
    ThirdSubtotalRow,
    /// `firstRowSubheading`.
    FirstRowSubheading,
    /// `secondRowSubheading`.
    SecondRowSubheading,
    /// `thirdRowSubheading`.
    ThirdRowSubheading,
    /// `blankRow`.
    BlankRow,
    /// `pageFieldLabels`.
    PageFieldLabels,
    /// `pageFieldValues`.
    PageFieldValues,
}

/// Number of [`El`] variants.
pub const N_ELEMENTS: usize = 25;

impl El {
    /// The element of a `type` name.
    #[must_use]
    pub fn parse(s: &str) -> Option<El> {
        Some(match s {
            "wholeTable" => El::WholeTable,
            "firstColumnStripe" => El::FirstColumnStripe,
            "secondColumnStripe" => El::SecondColumnStripe,
            "firstRowStripe" => El::FirstRowStripe,
            "secondRowStripe" => El::SecondRowStripe,
            "lastColumn" => El::LastColumn,
            "firstColumn" => El::FirstColumn,
            "headerRow" => El::HeaderRow,
            "totalRow" => El::TotalRow,
            "firstHeaderCell" => El::FirstHeaderCell,
            "lastHeaderCell" => El::LastHeaderCell,
            "firstTotalCell" => El::FirstTotalCell,
            "lastTotalCell" => El::LastTotalCell,
            "firstSubtotalColumn" => El::FirstSubtotalColumn,
            "secondSubtotalColumn" => El::SecondSubtotalColumn,
            "thirdSubtotalColumn" => El::ThirdSubtotalColumn,
            "firstSubtotalRow" => El::FirstSubtotalRow,
            "secondSubtotalRow" => El::SecondSubtotalRow,
            "thirdSubtotalRow" => El::ThirdSubtotalRow,
            "firstRowSubheading" => El::FirstRowSubheading,
            "secondRowSubheading" => El::SecondRowSubheading,
            "thirdRowSubheading" => El::ThirdRowSubheading,
            "blankRow" => El::BlankRow,
            "pageFieldLabels" => El::PageFieldLabels,
            "pageFieldValues" => El::PageFieldValues,
            _ => return None,
        })
    }
}

/// Indices into [`ElementFormat::border`] / [`TableCellFormat::border`].
pub const LEFT: usize = 0;
/// Right.
pub const RIGHT: usize = 1;
/// Top.
pub const TOP: usize = 2;
/// Bottom.
pub const BOTTOM: usize = 3;
/// Internal vertical (elements only).
pub const VERTICAL: usize = 4;
/// Internal horizontal (elements only).
pub const HORIZONTAL: usize = 5;

/// The formatting of one element of a style (`ElementFormat`; also what `fromDxf` and a conditional format's dxf
/// carry): `fill` is `Some` when the TS has a fill OBJECT, even one that paints nothing (a gradient without stops).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct ElementFormat {
    /// The fill object (`FillRef::None` inside = present but invisible).
    pub fill: Option<FillRef>,
    /// Font colour.
    pub font_color: Option<Color>,
    /// Bold.
    pub bold: Option<bool>,
    /// Italic.
    pub italic: Option<bool>,
    /// Underline.
    pub underline: Option<bool>,
    /// Strike-through.
    pub strike: Option<bool>,
    /// Borders: left, right, top, bottom, vertical, horizontal.
    pub border: [Edge; 6],
    /// Stripe size (stripe elements; `None` = 1).
    pub size: Option<f64>,
}

impl ElementFormat {
    /// `fromDxf`: a fill that is not `none`, the font colour, the four flags, the border.
    pub fn from_dxf(d: &Dxf, pal: &mut Palette, arena: &mut Vec<PaintFill>) -> ElementFormat {
        let mut f = ElementFormat::default();
        if let Some(fill) = &d.fill
            && fill.pattern.as_deref() != Some("none")
        {
            f.fill = Some(FillRef::resolve(fill, pal, arena));
        }
        f.font_color = d.font_color.as_deref().map(|c| pal.intern(c));
        f.bold = d.bold;
        f.italic = d.italic;
        f.underline = d.underline;
        f.strike = d.strike;
        if let Some(b) = &d.border {
            let mut edge = |e: &Option<crate::read::model::BorderEdge>| {
                e.as_ref().map_or(Edge::NONE, |e| Edge::new(BStyle::parse(&e.style), pal.intern(&e.color)))
            };
            f.border = [edge(&b.left), edge(&b.right), edge(&b.top), edge(&b.bottom), edge(&b.vertical), edge(&b.horizontal)];
        }
        f
    }
}

/// A style by element.
#[derive(Clone, Debug)]
pub struct Spec {
    els: Box<[Option<ElementFormat>; N_ELEMENTS]>,
}

impl Spec {
    fn empty() -> Spec {
        Spec { els: Box::new([None; N_ELEMENTS]) }
    }

    /// The element `e` (`spec.get(name)`).
    #[must_use]
    pub fn el(&self, e: El) -> Option<&ElementFormat> {
        self.els.get(e as usize).and_then(Option::as_ref)
    }

    fn set(&mut self, e: El, f: ElementFormat) {
        if let Some(slot) = self.els.get_mut(e as usize) {
            *slot = Some(f);
        }
    }
}

/// The compact definition of a built-in style by name (`BUILTIN_TABLE_STYLES[name]`).
#[must_use]
pub fn builtin_definition(name: &str) -> Option<&'static str> {
    data::BUILTIN_TABLE_STYLES
        .binary_search_by(|(k, _)| (*k).cmp(name))
        .ok()
        .and_then(|i| data::BUILTIN_TABLE_STYLES.get(i))
        .map(|(_, d)| *d)
}

/// A colour of the compact format: `<theme index>` or `<index>/<tint>` against the workbook's theme.
fn spec_color(s: &str, cx: &ColorContext, pal: &mut Palette) -> Color {
    let (k, t) = match s.split_once('/') {
        Some((k, t)) => (k, Some(t)),
        None => (s, None),
    };
    let base = uo_jsnum::conv::js_index(crate::read::pi(k), cx.theme.colors.len())
        .and_then(|i| cx.theme.colors.get(i))
        .map_or("#000000", String::as_str);
    match t.filter(|t| !t.is_empty()) {
        Some(t) => pal.intern(&apply_tint(base, crate::read::pf(t))),
        None => pal.intern(base),
    }
}

/// `parseBuiltin`: decodes a compact definition against the theme.
fn parse_builtin(def: &str, cx: &ColorContext, pal: &mut Palette) -> Spec {
    let mut spec = Spec::empty();
    for part in def.split(';') {
        if part.is_empty() {
            continue;
        }
        let Some((name, props)) = part.split_once('=') else { continue };
        let mut f = ElementFormat::default();
        for p in props.split(',') {
            if p == "B" {
                f.bold = Some(true);
                continue;
            }
            if p == "I" {
                f.italic = Some(true);
                continue;
            }
            let mut it = p.split(':');
            let (k, a, b) = (it.next().unwrap_or(""), it.next(), it.next());
            let side = match k {
                "l" => Some(LEFT),
                "r" => Some(RIGHT),
                "t" => Some(TOP),
                "b" => Some(BOTTOM),
                "v" => Some(VERTICAL),
                "h" => Some(HORIZONTAL),
                _ => None,
            };
            match (k, a) {
                ("f", Some(a)) => f.fill = Some(FillRef::Solid(spec_color(a, cx, pal))),
                ("c", Some(a)) => f.font_color = Some(spec_color(a, cx, pal)),
                ("s", Some(a)) => f.size = Some(crate::read::pi(a)),
                (_, Some(a)) => {
                    if let (Some(s), Some(b)) = (side, b)
                        && let Some(slot) = f.border.get_mut(s)
                    {
                        *slot = Edge::new(BStyle::parse(a), spec_color(b, cx, pal));
                    }
                }
                _ => {}
            }
        }
        if let Some(e) = El::parse(name) {
            spec.set(e, f);
        }
    }
    spec
}

/// `resolveTableStyle(name, styles, cx)`: the definition of style `name` — the custom one of the workbook, else the
/// built-in one — or `None` (an empty name or an unknown style: no formatting). A name that is a property of a JS object
/// (`constructor`…) is unknown here (the TS would throw reading it as a definition; decision C3c-2).
pub fn resolve_table_style(
    name: Option<&str>,
    styles: &Styles,
    cx: &ColorContext,
    pal: &mut Palette,
    arena: &mut Vec<PaintFill>,
) -> Option<Spec> {
    let name = name.filter(|n| !n.is_empty())?;
    if let Some(custom) = styles.table_styles.as_ref().and_then(|v| v.iter().find(|t| t.name == name)) {
        let mut spec = Spec::empty();
        for el in &custom.elements {
            let dxf = el.dxf_id.and_then(|i| uo_jsnum::conv::js_index(i, styles.dxfs.len())).and_then(|i| styles.dxfs.get(i));
            let mut f = dxf.map_or_else(ElementFormat::default, |d| ElementFormat::from_dxf(d, pal, arena));
            if let Some(s) = el.size
                && s > 1.0
            {
                f.size = Some(s);
            }
            if let Some(e) = El::parse(&el.r#type) {
                spec.set(e, f);
            }
        }
        return Some(spec);
    }
    builtin_definition(name).map(|d| parse_builtin(d, cx, pal))
}

/// The formatting a table gives ONE cell (`TableCellFormat`), the layer below the cell's direct formatting.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct TableCellFormat {
    /// Fill object.
    pub fill: Option<FillRef>,
    /// Font colour.
    pub font_color: Option<Color>,
    /// Bold.
    pub bold: Option<bool>,
    /// Italic.
    pub italic: Option<bool>,
    /// Underline.
    pub underline: Option<bool>,
    /// Strike.
    pub strike: Option<bool>,
    /// Borders: left, right, top, bottom.
    pub border: [Edge; 4],
}

impl TableCellFormat {
    /// `hasAny`/the TS `if (f.fill || f.fontColor !== undefined || … || f.border)`.
    #[must_use]
    pub fn has_any(&self) -> bool {
        self.fill.is_some()
            || self.font_color.is_some()
            || self.bold.is_some()
            || self.italic.is_some()
            || self.underline.is_some()
            || self.strike.is_some()
            || self.border.iter().any(|e| e.is_some())
    }

    /// Some border side is set.
    #[must_use]
    pub fn has_border(&self) -> bool {
        self.border.iter().any(|e| e.is_some())
    }
}

/// `applyElement`: applies an element to a cell of its region `[ca..cb] x [ra..rb]` (corners: the region's outline
/// takes the element's left/right/top/bottom, the inside its vertical/horizontal).
#[allow(clippy::too_many_arguments)]
pub fn apply_element(out: &mut TableCellFormat, el: Option<&ElementFormat>, c: f64, r: f64, ca: f64, cb: f64, ra: f64, rb: f64) {
    let Some(el) = el else { return };
    if el.fill.is_some() {
        out.fill = el.fill;
    }
    if el.font_color.is_some() {
        out.font_color = el.font_color;
    }
    if el.bold.is_some() {
        out.bold = el.bold;
    }
    if el.italic.is_some() {
        out.italic = el.italic;
    }
    if el.underline.is_some() {
        out.underline = el.underline;
    }
    if el.strike.is_some() {
        out.strike = el.strike;
    }
    let b = &el.border;
    let pick = |own: usize, inner: usize, at_edge: bool| b.get(if at_edge { own } else { inner }).copied().unwrap_or(Edge::NONE);
    let sides = [
        (LEFT, pick(LEFT, VERTICAL, c == ca)),
        (RIGHT, pick(RIGHT, VERTICAL, c == cb)),
        (TOP, pick(TOP, HORIZONTAL, r == ra)),
        (BOTTOM, pick(BOTTOM, HORIZONTAL, r == rb)),
    ];
    for (side, e) in sides {
        if e.is_some()
            && let Some(slot) = out.border.get_mut(side)
        {
            *slot = e;
        }
    }
}

/// A rectangle with the numbers `parseRange` gives (1-based; any double: `parseInt` of the reference digits).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RectF {
    /// First column.
    pub c1: f64,
    /// First row.
    pub r1: f64,
    /// Last column.
    pub c2: f64,
    /// Last row.
    pub r2: f64,
}

impl RectF {
    /// The cell is inside.
    #[must_use]
    pub fn contains(&self, c: f64, r: f64) -> bool {
        c >= self.c1 && c <= self.c2 && r >= self.r1 && r <= self.r2
    }

    /// The rectangles intersect.
    #[must_use]
    pub fn intersects(&self, o: &RectF) -> bool {
        self.c1 <= o.c2 && self.c2 >= o.c1 && self.r1 <= o.r2 && self.r2 >= o.r1
    }
}

/// `parseRange`: `"A1:C5"` → the rectangle, `None` for a text the TS rejects (empty first part, no cell reference).
#[must_use]
pub fn parse_range(r: &str) -> Option<RectF> {
    let mut it = r.split(':');
    let a = it.next().filter(|a| !a.is_empty())?;
    let b = it.next().filter(|b| !b.is_empty());
    let (sc, sr) = crate::read::ref_to_row_col(a.as_bytes())?;
    let (ec, er) = match b {
        Some(b) => crate::read::ref_to_row_col(b.as_bytes())?,
        None => (sc, sr),
    };
    use uo_jsnum::math::{max, min};
    Some(RectF { c1: min(sc, ec), r1: min(sr, er), c2: max(sc, ec), r2: max(sr, er) })
}

/// A table of the sheet, ready to give the format of any cell.
#[derive(Clone, Debug)]
pub struct TableRegion {
    /// `parseRange(t.ref)`.
    pub rect: RectF,
    /// The resolved style (index into `SheetPaint::specs`), `None` without one.
    pub spec: Option<u32>,
    /// `headerRows`/`totalsRows` (0 or 1).
    pub header_rows: f64,
    /// Totals rows.
    pub totals_rows: f64,
    /// `showRowStripes`.
    pub row_stripes: bool,
    /// `showColumnStripes`.
    pub col_stripes: bool,
    /// `showFirstColumn`.
    pub first_column: bool,
    /// `showLastColumn`.
    pub last_column: bool,
    /// The outline border of `tableBorderDxfId` (left, right, top, bottom), laid over the style.
    pub outline: [Edge; 4],
    /// Stripe sizes `(first column, second column, first row, second row)` of the spec.
    sizes: [f64; 4],
}

impl TableRegion {
    /// Builds the region of a `TableInfo` (`None` when the reference is not a range).
    pub fn new(
        t: &TableInfo,
        spec: Option<u32>,
        spec_ref: Option<&Spec>,
        styles: &Styles,
        pal: &mut Palette,
        arena: &mut Vec<PaintFill>,
    ) -> Option<TableRegion> {
        let rect = parse_range(&t.r#ref)?;
        let size = |e: El| spec_ref.and_then(|s| s.el(e)).and_then(|f| f.size).unwrap_or(1.0);
        let mut outline = [Edge::NONE; 4];
        if let Some(d) = t
            .border_dxf_id
            .and_then(|i| uo_jsnum::conv::js_index(i, styles.dxfs.len()))
            .and_then(|i| styles.dxfs.get(i))
            .filter(|d| d.border.is_some())
        {
            let f = ElementFormat::from_dxf(d, pal, arena);
            for (k, slot) in outline.iter_mut().enumerate() {
                *slot = f.border.get(k).copied().unwrap_or(Edge::NONE);
            }
        }
        Some(TableRegion {
            rect,
            spec,
            header_rows: if t.header_row { 1.0 } else { 0.0 },
            totals_rows: if t.totals_row { 1.0 } else { 0.0 },
            row_stripes: t.stripes,
            col_stripes: t.col_stripes,
            first_column: t.first_column,
            last_column: t.last_column,
            outline,
            sizes: [size(El::FirstColumnStripe), size(El::SecondColumnStripe), size(El::FirstRowStripe), size(El::SecondRowStripe)],
        })
    }
}

/// `tableCellFormats` for one cell: the style's elements in the TS order. `None` when the cell gets nothing.
#[must_use]
pub fn table_cell_format(t: &TableRegion, spec: &Spec, c: f64, r: f64) -> Option<TableCellFormat> {
    let rc = &t.rect;
    let hr = if t.header_rows > 0.0 { rc.r1 } else { -1.0 };
    let tr = if t.totals_rows > 0.0 { rc.r2 } else { -1.0 };
    let d1 = rc.r1 + uo_jsnum::math::max(0.0, t.header_rows);
    let d2 = rc.r2 - uo_jsnum::math::max(0.0, t.totals_rows);
    let el = |e: El| spec.el(e);
    let [cw1, cw2, rw1, rw2] = t.sizes;
    let is_data = r >= d1 && r <= d2;
    let mut f = TableCellFormat::default();
    apply_element(&mut f, el(El::WholeTable), c, r, rc.c1, rc.c2, rc.r1, rc.r2);
    if t.col_stripes && is_data {
        let i = c - rc.c1;
        let per = cw1 + cw2;
        let k = uo_jsnum::math::rem(i, per);
        let first = k < cw1;
        let a = rc.c1 + i - k + if first { 0.0 } else { cw1 };
        let b = uo_jsnum::math::min(rc.c2, if first { a + cw1 - 1.0 } else { a + cw2 - 1.0 });
        apply_element(&mut f, el(if first { El::FirstColumnStripe } else { El::SecondColumnStripe }), c, r, a, b, d1, d2);
    }
    if t.row_stripes && is_data {
        let i = r - d1;
        let per = rw1 + rw2;
        let k = uo_jsnum::math::rem(i, per);
        let first = k < rw1;
        let a = d1 + i - k + if first { 0.0 } else { rw1 };
        let b = uo_jsnum::math::min(d2, if first { a + rw1 - 1.0 } else { a + rw2 - 1.0 });
        apply_element(&mut f, el(if first { El::FirstRowStripe } else { El::SecondRowStripe }), c, r, rc.c1, rc.c2, a, b);
    }
    if t.last_column && c == rc.c2 {
        apply_element(&mut f, el(El::LastColumn), c, r, c, c, rc.r1, rc.r2);
    }
    if t.first_column && c == rc.c1 {
        apply_element(&mut f, el(El::FirstColumn), c, r, c, c, rc.r1, rc.r2);
    }
    if r == hr {
        apply_element(&mut f, el(El::HeaderRow), c, r, rc.c1, rc.c2, r, r);
    }
    if r == tr {
        apply_element(&mut f, el(El::TotalRow), c, r, rc.c1, rc.c2, r, r);
    }
    if r == hr && c == rc.c1 && t.first_column {
        apply_element(&mut f, el(El::FirstHeaderCell), c, r, c, c, r, r);
    }
    if r == hr && c == rc.c2 && t.last_column {
        apply_element(&mut f, el(El::LastHeaderCell), c, r, c, c, r, r);
    }
    if r == tr && c == rc.c1 && t.first_column {
        apply_element(&mut f, el(El::FirstTotalCell), c, r, c, c, r, r);
    }
    if r == tr && c == rc.c2 && t.last_column {
        apply_element(&mut f, el(El::LastTotalCell), c, r, c, c, r, r);
    }
    f.has_any().then_some(f)
}

/// The table's outline border (`tableBorderDxfId`) laid over the cell's format, per side the cell is on.
pub fn apply_outline(t: &TableRegion, c: f64, r: f64, cur: &mut Option<TableCellFormat>) {
    let rc = &t.rect;
    if !rc.contains(c, r) {
        return;
    }
    let sides = [(LEFT, c == rc.c1), (RIGHT, c == rc.c2), (TOP, r == rc.r1), (BOTTOM, r == rc.r2)];
    for (side, on) in sides {
        let e = t.outline.get(side).copied().unwrap_or(Edge::NONE);
        if on && e.is_some() {
            let f = cur.get_or_insert_with(TableCellFormat::default);
            if let Some(slot) = f.border.get_mut(side) {
                *slot = e;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::model::Theme;

    fn cx() -> ColorContext {
        let colors = ["#ffffff", "#000000", "#e7e6e6", "#44546a", "#4472c4", "#ed7d31", "#a5a5a5", "#ffc000", "#5b9bd5", "#70ad47"];
        ColorContext { theme: Theme { colors: colors.iter().map(|s| (*s).to_owned()).collect(), ..Theme::default() }, indexed: vec![] }
    }

    #[test]
    fn data_sorted_and_found() {
        assert!(data::BUILTIN_TABLE_STYLES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(data::BUILTIN_TABLE_STYLES.len(), 144);
        assert!(builtin_definition("TableStyleMedium2").is_some());
        assert!(builtin_definition("constructor").is_none());
    }

    #[test]
    fn medium2_decodes_against_the_theme() {
        let (mut pal, mut arena) = (Palette::default(), Vec::new());
        let styles = Styles::default();
        let spec = resolve_table_style(Some("TableStyleMedium2"), &styles, &cx(), &mut pal, &mut arena).unwrap();
        let h = spec.el(El::HeaderRow).unwrap();
        assert_eq!(h.bold, Some(true));
        assert!(matches!(h.fill, Some(FillRef::Solid(_))));
        assert!(resolve_table_style(Some("Nope"), &styles, &cx(), &mut pal, &mut arena).is_none());
        assert!(resolve_table_style(Some(""), &styles, &cx(), &mut pal, &mut arena).is_none());
    }

    #[test]
    fn range_parsing_and_stripes() {
        assert_eq!(parse_range("B2:D5"), Some(RectF { c1: 2.0, r1: 2.0, c2: 4.0, r2: 5.0 }));
        assert_eq!(parse_range("D5:B2"), Some(RectF { c1: 2.0, r1: 2.0, c2: 4.0, r2: 5.0 }));
        assert_eq!(parse_range("C3"), Some(RectF { c1: 3.0, r1: 3.0, c2: 3.0, r2: 3.0 }));
        assert_eq!(parse_range(""), None);
        assert_eq!(parse_range("zzz"), None);
        let (mut pal, mut arena) = (Palette::default(), Vec::new());
        let styles = Styles::default();
        let spec = resolve_table_style(Some("TableStyleMedium2"), &styles, &cx(), &mut pal, &mut arena).unwrap();
        let t = TableInfo { r#ref: "A1:C6".into(), header_row: true, stripes: true, ..TableInfo::default() };
        let reg = TableRegion::new(&t, Some(0), Some(&spec), &styles, &mut pal, &mut arena).unwrap();
        let head = table_cell_format(&reg, &spec, 2.0, 1.0).unwrap();
        assert_eq!(head.bold, Some(true));
        // row stripes: data row 2 (first stripe) is filled, row 3 is not
        let r2 = table_cell_format(&reg, &spec, 2.0, 2.0).unwrap();
        let r3 = table_cell_format(&reg, &spec, 2.0, 3.0).unwrap();
        assert!(r2.fill.is_some() && r3.fill.is_none());
        assert!(table_cell_format(&reg, &spec, 9.0, 9.0).is_none_or(|f| f.fill.is_none()));
    }
}
