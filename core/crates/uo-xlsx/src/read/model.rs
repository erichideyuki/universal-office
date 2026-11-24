//! The workbook model outside the cells: `sheet-core` `model.ts` and the `styles.ts`/`theme.ts` types as plain
//! structs (record "Track C architecture" T2). Every TS `?:` is an `Option`; a number the TS got from
//! `parseInt`/`parseFloat` is an `f64` (NaN survives, as in the TS); a boolean the TS sets only when true is a
//! `bool` (false = absent); lists the TS sets only when non-empty are `Vec`s (empty = absent). The cells of the
//! worksheet (`Worksheet.rows`) are NOT here: they are the `uo-calc` store.

/// An element of a parsed XML part kept as data (`XmlElement`): theme fill styles, drawing objects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XEl {
    /// Name as the TS has it (prefixes of the document, the main SpreadsheetML prefix stripped).
    pub name: String,
    /// Attributes in document order (a repeated name keeps its last value, as the TS object).
    pub attrs: Vec<(String, String)>,
    /// Children: elements and text.
    pub children: Vec<XChild>,
}

/// A child of [`XEl`].
#[derive(Clone, Debug, PartialEq)]
pub enum XChild {
    /// An element.
    El(XEl),
    /// Text (entities decoded).
    Text(String),
}

impl XEl {
    /// A new element.
    pub fn new(name: &str, attrs: &[(&str, &str)], children: Vec<XEl>) -> XEl {
        XEl {
            name: name.to_owned(),
            attrs: attrs.iter().map(|&(k, v)| (k.to_owned(), v.to_owned())).collect(),
            children: children.into_iter().map(XChild::El).collect(),
        }
    }
    /// The attribute `k`.
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    /// Child elements, optionally by name.
    pub fn elements<'a>(&'a self, name: Option<&'a str>) -> impl Iterator<Item = &'a XEl> + 'a {
        self.children.iter().filter_map(move |c| match c {
            XChild::El(e) if name.is_none_or(|n| e.name == n) => Some(e),
            _ => None,
        })
    }
    /// The first child element named `name`.
    pub fn first<'a>(&'a self, name: &'a str) -> Option<&'a XEl> {
        self.elements(Some(name)).next()
    }
}

// ---------------------------------------------------------------- workbook

/// `<sheet>` of `xl/workbook.xml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetRef {
    /// `name`.
    pub name: String,
    /// `sheetId`.
    pub sheet_id: String,
    /// `r:id`.
    pub r_id: String,
    /// `state`.
    pub state: Option<String>,
    /// Resolved part name.
    pub target: Option<String>,
}

/// `<definedName>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DefinedName {
    /// `name`.
    pub name: String,
    /// The formula text.
    pub formula: String,
    /// `localSheetId`.
    pub local_sheet_id: Option<f64>,
    /// `hidden`.
    pub hidden: bool,
}

/// `<calcPr>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalcPr {
    /// `calcId`.
    pub calc_id: Option<f64>,
    /// `calcMode`.
    pub calc_mode: Option<String>,
    /// `fullCalcOnLoad`.
    pub full_calc_on_load: bool,
    /// `forceFullCalc`.
    pub force_full_calc: bool,
    /// `iterate`.
    pub iterate: bool,
    /// `iterateCount`.
    pub iterate_count: Option<f64>,
    /// `iterateDelta`.
    pub iterate_delta: Option<f64>,
    /// `refMode`.
    pub ref_mode: Option<String>,
}

/// One cached external defined name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExternalName {
    /// `name`.
    pub name: String,
    /// `refersTo`.
    pub refers_to: Option<String>,
    /// `sheetId`.
    pub sheet_id: Option<f64>,
}

/// One cached external cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExternalCell {
    /// `r`.
    pub r#ref: String,
    /// `t`.
    pub t: Option<String>,
    /// `<v>`.
    pub v: Option<String>,
}

/// Cached values of one external sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExternalSheetData {
    /// `sheetId`.
    pub sheet_id: f64,
    /// The cells.
    pub cells: Vec<ExternalCell>,
}

/// `xl/externalLinks/externalLinkN.xml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExternalLink {
    /// Target of the external book.
    pub target: Option<String>,
    /// Sheet names.
    pub sheet_names: Vec<String>,
    /// Defined names.
    pub defined_names: Vec<ExternalName>,
    /// Cached values.
    pub sheet_data: Vec<ExternalSheetData>,
}

/// `xl/workbook.xml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Workbook {
    /// Sheets in order.
    pub sheets: Vec<SheetRef>,
    /// `activeTab`.
    pub active_tab: Option<f64>,
    /// `date1904`.
    pub date1904: bool,
    /// Defined names (empty = absent).
    pub defined_names: Vec<DefinedName>,
    /// `<calcPr>`.
    pub calc_pr: Option<CalcPr>,
    /// `<fileVersion appName>`.
    pub app_name: Option<String>,
    /// External links (empty = absent).
    pub external_links: Vec<ExternalLink>,
    /// `cm` indexes of dynamic arrays (empty = absent).
    pub dynamic_array_cm: Vec<f64>,
}

// ---------------------------------------------------------------- theme

/// `theme1.xml` and the indexed palette.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Theme {
    /// Palette by `theme="N"`.
    pub colors: Vec<String>,
    /// Colors by slot name, in insertion order (the TS object order).
    pub by_name: Vec<(String, String)>,
    /// `fontScheme` major latin face.
    pub major: Option<String>,
    /// `fontScheme` minor latin face.
    pub minor: Option<String>,
    /// `fmtScheme/fillStyleLst` children.
    pub fill_styles: Option<Vec<XEl>>,
    /// `lnStyleLst` widths in points.
    pub line_widths: Option<Vec<f64>>,
}

/// Theme and indexed palette (`ColorContext`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorContext {
    /// The theme.
    pub theme: Theme,
    /// Effective indexed palette (65 entries and any override).
    pub indexed: Vec<String>,
}

// ---------------------------------------------------------------- styles

/// `<font>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Font {
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Underline style when not `single`.
    pub underline_style: Option<&'static str>,
    /// Strike.
    pub strike: bool,
    /// `sz`.
    pub size: Option<f64>,
    /// Resolved color.
    pub color: Option<String>,
    /// `name` (or the theme face).
    pub name: Option<String>,
    /// The written name when the font follows the theme and differs.
    pub alt_name: Option<String>,
    /// `superscript` | `subscript`.
    pub vert_align: Option<&'static str>,
}

/// A gradient stop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientStop {
    /// `position`.
    pub position: f64,
    /// Resolved color.
    pub color: String,
}

/// `<gradientFill>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gradient {
    /// `degree`.
    pub degree: f64,
    /// Stops.
    pub stops: Vec<GradientStop>,
    /// `path` gradient: `left`, `right`, `top`, `bottom`.
    pub path: Option<[f64; 4]>,
}

/// `<fill>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fill {
    /// Main color.
    pub fg_color: Option<String>,
    /// Background color behind a pattern.
    pub bg_color: Option<String>,
    /// `patternType`.
    pub pattern: Option<String>,
    /// Gradient.
    pub gradient: Option<Gradient>,
}

/// One border edge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BorderEdge {
    /// `style`.
    pub style: String,
    /// Resolved color.
    pub color: String,
}

/// `<border>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Border {
    /// Left (or start).
    pub left: Option<BorderEdge>,
    /// Right (or end).
    pub right: Option<BorderEdge>,
    /// Top.
    pub top: Option<BorderEdge>,
    /// Bottom.
    pub bottom: Option<BorderEdge>,
    /// Diagonal.
    pub diagonal: Option<BorderEdge>,
    /// `diagonalUp`.
    pub diagonal_up: bool,
    /// `diagonalDown`.
    pub diagonal_down: bool,
    /// Inner vertical (dxf).
    pub vertical: Option<BorderEdge>,
    /// Inner horizontal (dxf).
    pub horizontal: Option<BorderEdge>,
}

impl Border {
    /// No key at all (`Object.keys(b).length === 0`).
    pub fn is_empty(&self) -> bool {
        *self == Border::default()
    }
}

/// `<tableStyleElement>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableStyleElementDef {
    /// `type`.
    pub r#type: String,
    /// `dxfId`.
    pub dxf_id: Option<f64>,
    /// `size`.
    pub size: Option<f64>,
}

/// A custom `<tableStyle>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableStyleDef {
    /// `name`.
    pub name: String,
    /// `pivot !== '0'`.
    pub pivot: bool,
    /// `table !== '0'`.
    pub table: bool,
    /// Elements.
    pub elements: Vec<TableStyleElementDef>,
}

/// `<xf>` of `cellXfs`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellXf {
    /// `numFmtId`.
    pub num_fmt_id: f64,
    /// `fontId`.
    pub font_id: f64,
    /// `fillId`.
    pub fill_id: f64,
    /// `borderId`.
    pub border_id: f64,
    /// `xfId`.
    pub xf_id: f64,
    /// `applyNumberFormat`.
    pub apply_number_format: bool,
    /// `applyFont`.
    pub apply_font: bool,
    /// `applyFill`.
    pub apply_fill: bool,
    /// `applyBorder`.
    pub apply_border: bool,
    /// `applyAlignment`.
    pub apply_alignment: bool,
    /// `pivotButton`.
    pub pivot_button: bool,
    /// `horizontal`.
    pub horizontal: Option<String>,
    /// `vertical`.
    pub vertical: Option<String>,
    /// `wrapText`.
    pub wrap_text: bool,
    /// `indent`.
    pub indent: Option<f64>,
    /// `textRotation` (kept only when 1..=180 or 255).
    pub text_rotation: Option<f64>,
    /// `shrinkToFit`.
    pub shrink_to_fit: bool,
    /// `readingOrder`.
    pub reading_order: Option<f64>,
    /// `justifyLastLine`.
    pub justify_last_line: bool,
}

/// `<dxf>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dxf {
    /// Solid color.
    pub fill_color: Option<String>,
    /// Fill.
    pub fill: Option<Fill>,
    /// Font color.
    pub font_color: Option<String>,
    /// `<b>`.
    pub bold: Option<bool>,
    /// `<i>`.
    pub italic: Option<bool>,
    /// `<u>`.
    pub underline: Option<bool>,
    /// `<strike>`.
    pub strike: Option<bool>,
    /// Border.
    pub border: Option<Border>,
    /// `numFmtId`.
    pub num_fmt_id: Option<f64>,
    /// `formatCode`.
    pub num_fmt_code: Option<String>,
}

/// `xl/styles.xml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styles {
    /// Custom number formats in file order (the XfPool's first match depends on it; a repeated id keeps its
    /// first position and its last code, as a JS `Map`).
    pub num_fmts: Vec<(f64, String)>,
    /// `cellXfs`.
    pub cell_xfs: Vec<CellXf>,
    /// Fonts.
    pub fonts: Vec<Font>,
    /// Fills.
    pub fills: Vec<Fill>,
    /// Borders.
    pub borders: Vec<Border>,
    /// Differential formats.
    pub dxfs: Vec<Dxf>,
    /// Custom table styles (`None` = no `<tableStyle>` with a name).
    pub table_styles: Option<Vec<TableStyleDef>>,
    /// `defaultTableStyle` (non-empty).
    pub default_table_style: Option<String>,
    /// `defaultPivotStyle` (non-empty).
    pub default_pivot_style: Option<String>,
}

// ---------------------------------------------------------------- shared strings

/// Parsed `<rPr>` of a rich run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunProps {
    /// `<b>`.
    pub bold: Option<bool>,
    /// `<i>`.
    pub italic: Option<bool>,
    /// `<u>`.
    pub underline: Option<bool>,
    /// `<strike>`.
    pub strike: Option<bool>,
    /// `<sz val>`.
    pub size: Option<f64>,
    /// Resolved `<color>`.
    pub color: Option<String>,
    /// `<rFont val>`.
    pub font: Option<String>,
    /// `superscript` | `subscript`.
    pub vert_align: Option<&'static str>,
    /// A `<color>` element was present, even when it resolved to nothing (the TS key exists with an
    /// `undefined` value and still counts for `Object.keys(props).length`).
    pub color_el: bool,
}

impl RunProps {
    /// `Object.keys(props).length === 0`.
    pub fn is_empty(&self) -> bool {
        *self == RunProps::default()
    }
}

// ---------------------------------------------------------------- worksheet

/// `<col>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColDef {
    /// `min`.
    pub min: f64,
    /// `max`.
    pub max: f64,
    /// `width`.
    pub width: Option<f64>,
    /// `customWidth`.
    pub custom_width: bool,
    /// `hidden`.
    pub hidden: bool,
    /// `style`.
    pub style: Option<f64>,
}

/// `<pane>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaneInfo {
    /// `xSplit`.
    pub x_split: f64,
    /// `ySplit`.
    pub y_split: f64,
    /// `topLeftCell`.
    pub top_left_cell: Option<String>,
    /// `state` (default `split`).
    pub state: String,
}

/// `<hyperlink>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hyperlink {
    /// `ref`.
    pub r#ref: String,
    /// External target from the rels.
    pub target: Option<String>,
    /// `location`.
    pub location: Option<String>,
    /// `display`.
    pub display: Option<String>,
    /// `tooltip`.
    pub tooltip: Option<String>,
}

/// `<sheetFormatPr>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetFormatPr {
    /// `defaultColWidth`.
    pub default_col_width: Option<f64>,
    /// `baseColWidth`.
    pub base_col_width: Option<f64>,
    /// `defaultRowHeight`.
    pub default_row_height: Option<f64>,
    /// `customHeight`.
    pub custom_height: bool,
    /// `zeroHeight`.
    pub zero_height: bool,
}

/// A `cfvo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cfvo {
    /// `type`.
    pub r#type: String,
    /// Raw value.
    pub val: Option<String>,
    /// Numeric value.
    pub value: Option<f64>,
    /// `gte="0"` → `Some(false)`.
    pub gte: Option<bool>,
}

/// A color-scale stop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorStop {
    /// `type`.
    pub r#type: String,
    /// Numeric value.
    pub value: Option<f64>,
    /// Raw value.
    pub val: Option<String>,
    /// Resolved color.
    pub color: String,
}

/// A custom icon (x14).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CfIcon {
    /// Icon set.
    pub set: String,
    /// `iconId`.
    pub id: f64,
}

/// A conditional-format rule (`CondRule`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CondRule {
    /// `type`.
    pub r#type: String,
    /// `priority`.
    pub priority: f64,
    /// `stopIfTrue`.
    pub stop_if_true: bool,
    /// Color scale stops.
    pub stops: Option<Vec<ColorStop>>,
    /// Data bar color.
    pub bar_color: Option<String>,
    /// `minLength`.
    pub bar_min_len: Option<f64>,
    /// `maxLength`.
    pub bar_max_len: Option<f64>,
    /// x14 `border`.
    pub bar_border: Option<bool>,
    /// x14 `gradient`.
    pub bar_gradient: Option<bool>,
    /// Border color.
    pub bar_border_color: Option<String>,
    /// Negative color.
    pub bar_neg_color: Option<String>,
    /// Negative border color.
    pub bar_neg_border_color: Option<String>,
    /// `negativeBarColorSameAsPositive`.
    pub bar_neg_same: bool,
    /// `negativeBarBorderColorSameAsPositive`.
    pub bar_neg_border_same: bool,
    /// `axisPosition`.
    pub bar_axis: Option<String>,
    /// Axis color.
    pub bar_axis_color: Option<String>,
    /// `direction`.
    pub bar_direction: Option<String>,
    /// Control points.
    pub cfvos: Option<Vec<Cfvo>>,
    /// `showValue="0"` → `Some(false)`.
    pub show_value: Option<bool>,
    /// x14 definition present.
    pub x14: bool,
    /// x14 id.
    pub x14_id: Option<String>,
    /// Icon set.
    pub icon_set: Option<String>,
    /// `reverse`.
    pub icon_reverse: bool,
    /// Custom icons.
    pub icons: Option<Vec<CfIcon>>,
    /// `operator`.
    pub operator: Option<String>,
    /// Formulas.
    pub formula: Option<Vec<String>>,
    /// `dxfId`.
    pub dxf_id: Option<f64>,
    /// Embedded dxf (x14).
    pub dxf: Option<Dxf>,
    /// `text`.
    pub text: Option<String>,
    /// `timePeriod`.
    pub time_period: Option<String>,
    /// `rank`.
    pub rank: Option<f64>,
    /// `percent`.
    pub percent: bool,
    /// `bottom`.
    pub bottom: bool,
    /// `aboveAverage="0"` → `Some(false)`.
    pub above_average: Option<bool>,
    /// `equalAverage`.
    pub equal_average: bool,
    /// `stdDev`.
    pub std_dev: Option<f64>,
}

/// `<conditionalFormatting>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CondFormat {
    /// `sqref`.
    pub sqref: String,
    /// Rules.
    pub rules: Vec<CondRule>,
}

/// A cell comment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Comment {
    /// `ref`.
    pub r#ref: String,
    /// Plain text.
    pub text: String,
    /// Author.
    pub author: Option<String>,
}

/// A point of a drawing anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnchorPos {
    /// Column (0-based).
    pub col: f64,
    /// Row (0-based).
    pub row: f64,
    /// Column offset (EMU).
    pub col_off: f64,
    /// Row offset (EMU).
    pub row_off: f64,
}

/// Anchor kind other than two-cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    /// `oneCellAnchor`.
    OneCell,
    /// `absoluteAnchor`.
    Absolute,
}

/// `DrawingAnchor`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingAnchor {
    /// From.
    pub from: AnchorPos,
    /// To.
    pub to: AnchorPos,
    /// Kind (`None` = two cells).
    pub kind: Option<AnchorKind>,
    /// `editAs`.
    pub edit_as: Option<String>,
    /// Size in EMU.
    pub ext: Option<(f64, f64)>,
    /// Position in EMU.
    pub pos: Option<(f64, f64)>,
}

/// An embedded chart: anchor and the raw chart XML (parsed when first painted).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingChart {
    /// Anchor.
    pub anchor: DrawingAnchor,
    /// `chartN.xml` text.
    pub chart_xml: String,
    /// Position in the drawing order.
    pub order: Option<f64>,
    /// `cNvPr name`.
    pub name: Option<String>,
    /// Part name of the chart.
    pub part: Option<String>,
    /// `chartex`.
    pub chartex: bool,
}

/// A form control marker.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingControl {
    /// `objectType`.
    pub r#type: String,
    /// `checked`.
    pub checked: Option<String>,
}

/// A non-chart drawing object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingObject {
    /// Anchor.
    pub anchor: DrawingAnchor,
    /// Drawing order (a half step for OLE/controls).
    pub order: f64,
    /// The `xdr:*` element.
    pub el: XEl,
    /// Part the images resolve against.
    pub part: String,
    /// Form control.
    pub control: Option<DrawingControl>,
}

/// A table column.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableColumnInfo {
    /// `name` (`_xHHHH_` decoded).
    pub name: String,
    /// `calculatedColumnFormula`.
    pub calculated_column_formula: Option<String>,
    /// `totalsRowFunction`.
    pub totals_row_function: Option<String>,
    /// `totalsRowLabel`.
    pub totals_row_label: Option<String>,
    /// `totalsRowFormula`.
    pub totals_row_formula: Option<String>,
}

/// An autofilter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutoFilterInfo {
    /// `ref`.
    pub r#ref: String,
    /// Hidden buttons (empty = absent).
    pub hidden: Vec<f64>,
    /// Active columns (empty = absent).
    pub active: Vec<f64>,
}

/// A table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableInfo {
    /// `ref`.
    pub r#ref: String,
    /// `name`.
    pub name: Option<String>,
    /// `displayName`.
    pub display_name: Option<String>,
    /// `headerRowCount`.
    pub header_row_count: Option<f64>,
    /// `totalsRowCount`.
    pub totals_row_count: Option<f64>,
    /// Columns.
    pub columns: Option<Vec<TableColumnInfo>>,
    /// Header row.
    pub header_row: bool,
    /// Totals row.
    pub totals_row: bool,
    /// Row stripes.
    pub stripes: bool,
    /// Style name.
    pub style_name: Option<String>,
    /// Has an `<autoFilter>`.
    pub auto_filter: bool,
    /// Column stripes.
    pub col_stripes: bool,
    /// First column.
    pub first_column: bool,
    /// Last column.
    pub last_column: bool,
    /// Filter details.
    pub filter: Option<AutoFilterInfo>,
    /// `tableBorderDxfId`.
    pub border_dxf_id: Option<f64>,
}

/// A pivot row/column item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PivotItem {
    /// `t`.
    pub t: Option<String>,
    /// `r`.
    pub r: f64,
    /// Count of `<x>`.
    pub x: f64,
}

/// A pivot table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PivotInfo {
    /// `location ref`.
    pub r#ref: String,
    /// `firstHeaderRow`.
    pub first_header_row: f64,
    /// `firstDataRow`.
    pub first_data_row: f64,
    /// `firstDataCol`.
    pub first_data_col: f64,
    /// `rowPageCount`.
    pub row_page_count: f64,
    /// `colPageCount`.
    pub col_page_count: f64,
    /// Page fields.
    pub page_field_count: f64,
    /// Page-field filter state.
    pub page_field_active: Option<Vec<bool>>,
    /// Row fields.
    pub row_fields: Vec<f64>,
    /// Column fields.
    pub col_fields: Vec<f64>,
    /// Row items.
    pub row_items: Vec<PivotItem>,
    /// Column items.
    pub col_items: Vec<PivotItem>,
    /// `compact`.
    pub compact: bool,
    /// `outline`.
    pub outline: bool,
    /// `showHeaders`.
    pub show_headers: bool,
    /// `rowGrandTotals`.
    pub row_grand_totals: bool,
    /// `colGrandTotals`.
    pub col_grand_totals: bool,
    /// Style name.
    pub style_name: Option<String>,
    /// `showRowHeaders`.
    pub show_row_headers: bool,
    /// `showColHeaders`.
    pub show_col_headers: bool,
    /// `showRowStripes`.
    pub show_row_stripes: bool,
    /// `showColStripes`.
    pub show_col_stripes: bool,
    /// `showLastColumn`.
    pub show_last_column: bool,
    /// Fields with an active filter.
    pub filtered_fields: Vec<f64>,
}

/// `<chartsheet>` view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartsheetView {
    /// Zoom in percent.
    pub zoom: f64,
    /// `zoomToFit`.
    pub zoom_to_fit: bool,
}

/// A worksheet without its cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Worksheet {
    /// Merged areas (A1 texts, empty = absent).
    pub merges: Vec<String>,
    /// `mergedAreas`: top-left ref → area, in file order.
    pub merged_areas: Vec<(String, String)>,
    /// `<cols>` (empty = absent).
    pub cols: Vec<ColDef>,
    /// Pane.
    pub pane: Option<PaneInfo>,
    /// `showGridLines="0"`.
    pub show_grid_lines_off: bool,
    /// `showZeros="0"`.
    pub show_zeros_off: bool,
    /// `rightToLeft`.
    pub right_to_left: bool,
    /// Ranges with "number stored as text" ignored.
    pub ignore_number_as_text: Vec<String>,
    /// Conditional formats.
    pub cond_formats: Vec<CondFormat>,
    /// Comments.
    pub comments: Vec<Comment>,
    /// Charts.
    pub charts: Vec<DrawingChart>,
    /// Other drawing objects.
    pub drawings: Vec<DrawingObject>,
    /// Chart-sheet view.
    pub chartsheet: Option<ChartsheetView>,
    /// Hyperlinks.
    pub hyperlinks: Vec<Hyperlink>,
    /// `<sheetFormatPr>`.
    pub sheet_format_pr: Option<SheetFormatPr>,
    /// Tables.
    pub tables: Vec<TableInfo>,
    /// Sheet autofilter.
    pub auto_filter: Option<AutoFilterInfo>,
    /// `filterMode`.
    pub filter_mode: bool,
    /// Pivot tables.
    pub pivots: Vec<PivotInfo>,
    /// List validations.
    pub list_validations: Vec<String>,
}
