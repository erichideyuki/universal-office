//! Table styles (`table-style.ts`): the `<a:tblPr>` of each `<a:tbl>` references a style by GUID
//! (`<a:tableStyleId>`) and turns its parts on (firstRow, lastRow, firstCol, lastCol, bandRow,
//! bandCol). Rules measured in the reference application (decks syn-a-tbl*): only the EMBEDDED
//! definitions of the application count, by GUID — `ppt/tableStyles.xml` is ignored; without a style
//! the table is "No Style, Table Grid" (1 pt tx1 grid); each part holds over a REGION and later parts
//! in the precedence override fill, text and borders they define.
//!
//! The read resolves everything to the model: fill/colour/bold per cell and the table's border grid
//! (`hLines`/`vLines`), with the cells' explicit borders already in.

use std::sync::Arc;

use uo_jsnum::conv::js_index;
use uo_jsnum::math;
use uo_jsnum::text::trim;
use uo_opc::xml::{Document, Element, ElementId};

use crate::color::CssColor;
use crate::model::{Fill, FillKind, LineGrid, Table, TableBgPaint, TableCell, TableLine};
use crate::source::{R, parse_xml, pint};
use crate::table_styles_data::{MAP, TPL};
use crate::theme::{Theme, color_from_dml};

/// A resolved fill (solid/gradient/pattern in `fill`, an image in `image`) — `CellPaint`.
pub type CellPaint = TableBgPaint;

/// The result of a [`PaintResolver`]: `undefined`, `null` (`<a:noFill/>`) or a paint.
#[derive(Clone, PartialEq, Debug)]
pub enum PaintOutcome {
    Undefined,
    Null,
    Paint(CellPaint),
}

/// Resolves a fill: `el` is the `<a:fill>` (or a container with EG_FillProperties) or the `<a:fillRef>`.
pub type PaintResolver<'a> = &'a dyn for<'d> Fn(Element<'d>) -> R<PaintOutcome>;

/// `null` or a paint, as a table part keeps it.
#[derive(Clone, PartialEq, Debug)]
pub enum PartPaint {
    Null,
    Paint(CellPaint),
}

/// `TablePartName` in application order (`PRECEDENCE`, measured against the 74 definitions).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TablePart {
    WholeTbl,
    Band1V,
    Band2V,
    Band1H,
    Band2H,
    FirstCol,
    LastCol,
    FirstRow,
    LastRow,
    NwCell,
    NeCell,
    SwCell,
    SeCell,
}

/// The parts in precedence order (the last one wins).
pub const PRECEDENCE: [TablePart; 13] = [
    TablePart::WholeTbl,
    TablePart::Band1V,
    TablePart::Band2V,
    TablePart::Band1H,
    TablePart::Band2H,
    TablePart::FirstCol,
    TablePart::LastCol,
    TablePart::FirstRow,
    TablePart::LastRow,
    TablePart::NwCell,
    TablePart::NeCell,
    TablePart::SwCell,
    TablePart::SeCell,
];

impl TablePart {
    fn index(self) -> usize {
        PRECEDENCE.iter().position(|p| *p == self).unwrap_or(0)
    }
    fn element_name(self) -> &'static str {
        match self {
            TablePart::WholeTbl => "a:wholeTbl",
            TablePart::Band1V => "a:band1V",
            TablePart::Band2V => "a:band2V",
            TablePart::Band1H => "a:band1H",
            TablePart::Band2H => "a:band2H",
            TablePart::FirstCol => "a:firstCol",
            TablePart::LastCol => "a:lastCol",
            TablePart::FirstRow => "a:firstRow",
            TablePart::LastRow => "a:lastRow",
            TablePart::NwCell => "a:nwCell",
            TablePart::NeCell => "a:neCell",
            TablePart::SwCell => "a:swCell",
            TablePart::SeCell => "a:seCell",
        }
    }
}

/// `BdrSide`: the six border positions of a part.
const BDR_SIDES: [&str; 6] = ["left", "right", "top", "bottom", "insideH", "insideV"];
const B_LEFT: usize = 0;
const B_RIGHT: usize = 1;
const B_TOP: usize = 2;
const B_BOTTOM: usize = 3;
const B_INSIDE_H: usize = 4;
const B_INSIDE_V: usize = 5;

/// A line edge that may be `null` (`<a:noFill/>`) — `TableLine | null`.
#[derive(Clone, PartialEq, Debug)]
pub enum LineOrNull {
    Null,
    Line(TableLine),
}

impl LineOrNull {
    fn into_edge(self) -> Option<TableLine> {
        match self {
            LineOrNull::Null => None,
            LineOrNull::Line(l) => Some(l),
        }
    }
}

/// A style part (`wholeTbl`/`band1H`/`firstRow`/…) with resolved values.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TableStylePart {
    /// `<a:fill>`/`<a:fillRef>`; `Null` = explicit `<a:noFill/>`.
    pub paint: Option<PartPaint>,
    pub tx_color: Option<CssColor>,
    pub tx_bold: Option<bool>,
    pub tx_italic: Option<bool>,
    /// `<a:tcBdr>` by side (`left right top bottom insideH insideV`; `Null` = `<a:noFill/>`).
    pub bdr: [Option<LineOrNull>; 6],
}

/// A style definition: the table background and the parts.
#[derive(Clone, Debug)]
pub struct TableStyleDef {
    /// `<a:tblBg>`: the fill, and the element carrying the `<a:effectRef>`.
    pub tbl_bg: Option<TblBg>,
    pub parts: [Option<TableStylePart>; 13],
    /// The parsed style body (`effect_el` points into it).
    doc: Arc<Document>,
}

/// `TableStyleDef.tblBg`.
#[derive(Clone, Debug, Default)]
pub struct TblBg {
    pub paint: Option<CellPaint>,
    pub effect_el: Option<ElementId>,
}

impl TableStyleDef {
    /// The `<a:tblBg>` element when it has an `<a:effectRef>` (`tblBg.effectEl`).
    pub fn effect_el(&self) -> Option<Element<'_>> {
        self.doc.element(self.tbl_bg.as_ref()?.effect_el?)
    }
}

/// Options on in the `<a:tblPr>` (`TblFlags`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TblFlags {
    pub first_row: bool,
    pub last_row: bool,
    pub first_col: bool,
    pub last_col: bool,
    pub band_row: bool,
    pub band_col: bool,
}

/// `<a:lnRef idx>` colour and width, or an `<a:ln>` → [`TableLine`] (`lineOf`, `table-style.ts:66`).
/// `None` = undefined; `Some(Null)` = no line.
fn line_of(el: Element<'_>, theme: &Theme) -> Option<LineOrNull> {
    if el.name() == "a:lnRef" {
        let col = color_from_dml(el.child_elements().next(), theme)?;
        let idx = el.attr("idx").map_or(1.0, pint) - 1.0;
        let width = js_index(idx, usize::MAX).and_then(|i| theme.line_widths.as_ref()?.get(i).copied()).unwrap_or(9525.0);
        return Some(LineOrNull::Line(TableLine { color: col, width, alpha: None, dash: None, cmpd: None }));
    }
    if el.first_child("a:noFill").is_some() {
        return Some(LineOrNull::Null);
    }
    let c = el.first_child("a:solidFill").and_then(|sf| sf.child_elements().next());
    let color = c.and_then(|c| color_from_dml(Some(c), theme))?;
    let mut out = TableLine { color, width: el.attr("w").map_or(12700.0, pint), alpha: None, dash: None, cmpd: None };
    if let Some(a) = c.and_then(|c| c.last_child("a:alpha")).and_then(|a| a.attr("val")) {
        out.alpha = Some(pint(a) / 100000.0);
    }
    if let Some(d) = el.first_child("a:prstDash").and_then(|d| d.attr("val")).filter(|d| !d.is_empty() && *d != "solid") {
        out.dash = Some(d.into());
    }
    if let Some(c) = el.attr("cmpd").filter(|c| !c.is_empty() && *c != "sng") {
        out.cmpd = Some(c.into());
    }
    Some(LineOrNull::Line(out))
}

/// `cellLine` (`table-style.ts:88`): a cell border (`<a:lnL>`…): a line, `Null` (noFill) or
/// `None` (no colour).
pub fn cell_line(el: Element<'_>, theme: &Theme) -> Option<LineOrNull> {
    line_of(el, theme)
}

/// `parsePart` (`table-style.ts:93`).
fn parse_part(el: Element<'_>, theme: &Theme, paint: PaintResolver<'_>) -> R<TableStylePart> {
    let mut out = TableStylePart::default();
    if let Some(tx) = el.first_child("a:tcTxStyle") {
        match tx.attr("b") {
            Some("on") => out.tx_bold = Some(true),
            Some("off") => out.tx_bold = Some(false),
            _ => {}
        }
        match tx.attr("i") {
            Some("on") => out.tx_italic = Some(true),
            Some("off") => out.tx_italic = Some(false),
            _ => {}
        }
        // text colour: the COLOUR child straight under tcTxStyle (outside fontRef/font)
        for c in tx.child_elements() {
            if c.name() == "a:fontRef" || c.name() == "a:font" {
                continue;
            }
            if let Some(col) = color_from_dml(Some(c), theme) {
                out.tx_color = Some(col);
                break;
            }
        }
    }
    if let Some(tc) = el.first_child("a:tcStyle") {
        // <a:fill> and <a:fillRef> are siblings inside tcStyle
        if let Some(f) = tc.first_child("a:fill").or_else(|| tc.first_child("a:fillRef")) {
            match paint(f)? {
                PaintOutcome::Undefined => {}
                PaintOutcome::Null => out.paint = Some(PartPaint::Null),
                PaintOutcome::Paint(p) => out.paint = Some(PartPaint::Paint(p)),
            }
        }
        if let Some(bdr) = tc.first_child("a:tcBdr") {
            for (i, side) in BDR_SIDES.iter().enumerate() {
                let Some(s) = bdr.first_child(&format!("a:{side}")) else { continue };
                let Some(ln) = s.first_child("a:ln").or_else(|| s.first_child("a:lnRef")) else { continue };
                if let (Some(l), Some(slot)) = (line_of(ln, theme), out.bdr.get_mut(i)) {
                    *slot = Some(l);
                }
            }
        }
    }
    Ok(out)
}

/// The embedded definition of a style GUID (`builtinTableStyleXml`): the parts' XML, or `None`.
fn builtin_table_style_xml(style_id: &str) -> Option<String> {
    let key: String = style_id.chars().filter(|c| *c != '{' && *c != '}').collect::<String>().to_ascii_uppercase();
    let &(_, t, accent) = MAP.iter().find(|(k, _, _)| *k == key)?;
    let tpl = *TPL.get(t)?;
    Some(if accent != 0 { tpl.replace("{A}", &format!("accent{accent}")) } else { tpl.to_string() })
}

/// "No Style, Table Grid": what the application draws without a style or with an unknown one.
pub const TABLE_GRID_STYLE_ID: &str = "{5940675A-B579-460E-94D1-54222C63F5DA}";

/// `readTableStyle` (`table-style.ts:128`): the style by GUID — ONLY the application's embedded
/// definitions; an unknown/absent one is the plain grid.
pub fn read_table_style(style_id: Option<&str>, theme: &Theme, paint: PaintResolver<'_>) -> R<Option<TableStyleDef>> {
    let body =
        style_id.filter(|s| !s.is_empty()).and_then(builtin_table_style_xml).or_else(|| builtin_table_style_xml(TABLE_GRID_STYLE_ID));
    let Some(body) = body else { return Ok(None) };
    let Ok(doc) = parse_xml(format!("<a:tblStyle xmlns:a=\"a\">{body}</a:tblStyle>")) else { return Ok(None) };
    let doc = Arc::new(doc);
    let st = doc.root();
    let mut def = TableStyleDef { tbl_bg: None, parts: Default::default(), doc: Arc::clone(&doc) };
    if let Some(bg) = st.first_child("a:tblBg") {
        let f = bg.first_child("a:fill").or_else(|| bg.first_child("a:fillRef"));
        let p = match f {
            Some(f) => match paint(f)? {
                PaintOutcome::Paint(p) => Some(p),
                _ => None,
            },
            None => None,
        };
        let mut tb = TblBg { paint: p, effect_el: None };
        if bg.first_child("a:effectRef").is_some() {
            tb.effect_el = Some(bg.id());
        }
        def.tbl_bg = Some(tb);
    }
    for name in PRECEDENCE {
        if let Some(el) = st.first_child(name.element_name()) {
            let part = parse_part(el, theme, paint)?;
            if let Some(slot) = def.parts.get_mut(name.index()) {
                *slot = Some(part);
            }
        }
    }
    Ok(Some(def))
}

/// `readTblPr` (`table-style.ts:157`): the style id and options of `<a:tblPr>`.
pub fn read_tbl_pr(tbl: Element<'_>) -> (TblFlags, Option<String>) {
    let pr = tbl.first_child("a:tblPr");
    let on = |k: &str| pr.and_then(|p| p.attr(k)).is_some_and(|v| v == "1" || v == "true");
    let flags = TblFlags {
        first_row: on("firstRow"),
        last_row: on("lastRow"),
        first_col: on("firstCol"),
        last_col: on("lastCol"),
        band_row: on("bandRow"),
        band_col: on("bandCol"),
    };
    let style_id = pr.and_then(|p| p.first_child("a:tableStyleId")).map(|e| trim(&e.text_content()).to_string());
    (flags, style_id)
}

/// A region (inclusive grid rectangle) of each part that applies to a cell.
#[derive(Clone, Copy)]
struct Region {
    part: TablePart,
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
}

/// The parts that hold at grid cell (r, c), in precedence order, with the region of each.
fn parts_at(r: usize, c: usize, n_r: usize, n_c: usize, f: &TblFlags) -> Vec<Region> {
    let mut out = vec![Region { part: TablePart::WholeTbl, r0: 0, r1: n_r - 1, c0: 0, c1: n_c - 1 }];
    let (fr, lr, fc, lc) = (usize::from(f.first_row), usize::from(f.last_row), usize::from(f.first_col), usize::from(f.last_col));
    let is_fr = f.first_row && r == 0;
    let is_lr = f.last_row && r == n_r - 1 && n_r > fr;
    let is_fc = f.first_col && c == 0;
    let is_lc = f.last_col && c == n_c - 1 && n_c > fc;
    if f.band_col && !is_fc && !is_lc {
        let k = c.wrapping_sub(fc);
        // `(c - fc) % 2` in JS: c >= fc here because a first column is excluded above
        let part = if k % 2 == 0 { TablePart::Band1V } else { TablePart::Band2V };
        out.push(Region { part, r0: fr, r1: (n_r - 1).wrapping_sub(lr), c0: c, c1: c });
    }
    if f.band_row && !is_fr && !is_lr {
        let k = r.wrapping_sub(fr);
        let part = if k % 2 == 0 { TablePart::Band1H } else { TablePart::Band2H };
        out.push(Region { part, r0: r, r1: r, c0: fc, c1: (n_c - 1).wrapping_sub(lc) });
    }
    if is_fc {
        out.push(Region { part: TablePart::FirstCol, r0: 0, r1: n_r - 1, c0: 0, c1: 0 });
    }
    if is_lc {
        out.push(Region { part: TablePart::LastCol, r0: 0, r1: n_r - 1, c0: n_c - 1, c1: n_c - 1 });
    }
    if is_fr {
        out.push(Region { part: TablePart::FirstRow, r0: 0, r1: 0, c0: 0, c1: n_c - 1 });
    }
    if is_lr {
        out.push(Region { part: TablePart::LastRow, r0: n_r - 1, r1: n_r - 1, c0: 0, c1: n_c - 1 });
    }
    if is_fr && is_fc {
        out.push(Region { part: TablePart::NwCell, r0: 0, r1: 0, c0: 0, c1: 0 });
    }
    if is_fr && is_lc {
        out.push(Region { part: TablePart::NeCell, r0: 0, r1: 0, c0: n_c - 1, c1: n_c - 1 });
    }
    if is_lr && is_fc {
        out.push(Region { part: TablePart::SwCell, r0: n_r - 1, r1: n_r - 1, c0: 0, c1: 0 });
    }
    if is_lr && is_lc {
        out.push(Region { part: TablePart::SeCell, r0: n_r - 1, r1: n_r - 1, c0: n_c - 1, c1: n_c - 1 });
    }
    // stable by precedence
    out.sort_by_key(|reg| reg.part.index());
    out
}

/// `mergeOrigins` (`table-style.ts:218`): the origin cell covering each grid position (gridSpan/rowSpan merges).
pub fn merge_origins(t: &Table) -> Vec<Vec<(usize, usize)>> {
    let n_r = t.rows.len();
    let n_c = t.col_widths_emu.len();
    let mut own: Vec<Vec<(usize, usize)>> = (0..n_r).map(|r| (0..n_c).map(|c| (r, c)).collect()).collect();
    for (r, row) in t.rows.iter().enumerate() {
        for c in 0..n_c.min(row.cells.len()) {
            let Some(cell) = row.cells.get(c) else { continue };
            if cell.h_merge == Some(true) || cell.v_merge == Some(true) {
                continue;
            }
            let gs = math::max(1.0, cell.grid_span.unwrap_or(1.0));
            let rs = math::max(1.0, cell.row_span.unwrap_or(1.0));
            if gs == 1.0 && rs == 1.0 {
                continue;
            }
            let r_end = math::min(n_r as f64, r as f64 + rs);
            let c_end = math::min(n_c as f64, c as f64 + gs);
            let mut rr = r;
            while (rr as f64) < r_end {
                let mut cc = c;
                while (cc as f64) < c_end {
                    if (rr != r || cc != c)
                        && let Some(slot) = own.get_mut(rr).and_then(|row| row.get_mut(cc))
                    {
                        *slot = (r, c);
                    }
                    cc += 1;
                }
                rr += 1;
            }
        }
    }
    own
}

#[derive(Default)]
struct Sty {
    paint: Option<PartPaint>,
    tx: Option<CssColor>,
    b: Option<bool>,
    i: Option<bool>,
}

/// `applyTableStyle` (`table-style.ts:236`): applies the style and resolves the border grid. Cells get
/// the style's fill/colour/bold/italic where their XML defines none. Borders (measured): the edge
/// between two cells is the TOP/LEFT cell's (`lnB`/`lnR`); `lnT`/`lnL` only on the table's outline;
/// without an explicit border the style's holds. A merge-covered cell inherits the origin's borders
/// only on the origin's row/column.
pub fn apply_table_style(t: &mut Table, def: Option<&TableStyleDef>, f: &TblFlags) {
    let n_r = t.rows.len();
    let n_c = t.col_widths_emu.len();
    if n_r == 0 || n_c == 0 {
        return;
    }
    if let Some(p) = def.and_then(|d| d.tbl_bg.as_ref()).and_then(|b| b.paint.as_ref())
        && t.bg_paint.is_none()
    {
        t.bg_paint = Some(Box::new(p.clone()));
    }
    let mut h_l = LineGrid { rows: u32_of(n_r + 1), cols: u32_of(n_c), cells: vec![None; (n_r + 1) * n_c] };
    let mut v_l = LineGrid { rows: u32_of(n_r), cols: u32_of(n_c + 1), cells: vec![None; n_r * (n_c + 1)] };
    // style values per origin cell (the part of highest precedence that defines wins)
    let mut sty: std::collections::BTreeMap<(usize, usize), Sty> = std::collections::BTreeMap::new();
    // part by part (the edges are SHARED: the later part wins on both cells)
    let regs_at: Vec<Vec<Vec<Region>>> = (0..n_r).map(|r| (0..n_c).map(|c| parts_at(r, c, n_r, n_c, f)).collect()).collect();
    for name in PRECEDENCE {
        let Some(p) = def.and_then(|d| d.parts.get(name.index())).and_then(Option::as_ref) else { continue };
        for r in 0..n_r {
            for c in 0..n_c {
                let cell = t.rows.get(r).and_then(|row| row.cells.get(c));
                let origin = cell.is_some_and(|ce| ce.h_merge != Some(true) && ce.v_merge != Some(true));
                let Some(regs) = regs_at.get(r).and_then(|x| x.get(c)) else { continue };
                for reg in regs {
                    if reg.part != name {
                        continue;
                    }
                    if origin {
                        let s = sty.entry((r, c)).or_default();
                        if p.paint.is_some() {
                            s.paint = p.paint.clone();
                        }
                        if p.tx_color.is_some() {
                            s.tx = p.tx_color.clone();
                        }
                        if p.tx_bold.is_some() {
                            s.b = p.tx_bold;
                        }
                        if p.tx_italic.is_some() {
                            s.i = p.tx_italic;
                        }
                    }
                    let b = &p.bdr;
                    let put = |g: &mut LineGrid, gr: usize, gc: usize, side: usize| {
                        if let Some(v) = b.get(side).and_then(Option::as_ref) {
                            g.set(gr, gc, v.clone().into_edge());
                        }
                    };
                    put(&mut h_l, r, c, if r == reg.r0 { B_TOP } else { B_INSIDE_H });
                    put(&mut h_l, r + 1, c, if r == reg.r1 { B_BOTTOM } else { B_INSIDE_H });
                    put(&mut v_l, r, c, if c == reg.c0 { B_LEFT } else { B_INSIDE_V });
                    put(&mut v_l, r, c + 1, if c == reg.c1 { B_RIGHT } else { B_INSIDE_V });
                }
            }
        }
    }
    // style → cell (the cell's XML wins)
    for ((r, c), s) in sty {
        let Some(cell) = t.rows.get_mut(r).and_then(|row| row.cells.get_mut(c)) else { continue };
        if let Some(PartPaint::Paint(sp)) = &s.paint
            && cell.fill.is_none()
            && cell.no_fill != Some(true)
            && cell.fill_paint.is_none()
            && cell.fill_image.is_none()
        {
            match &sp.fill {
                Some(fl) if fl.kind == FillKind::Solid && fl.color.is_some() => {
                    cell.fill = fl.color.clone();
                    if sp.alpha.is_some() && cell.fill_alpha.is_none() {
                        cell.fill_alpha = sp.alpha;
                    }
                }
                Some(fl) if fl.kind != FillKind::None => cell.fill_paint = Some(Box::new(fl.clone())),
                _ => {
                    if let Some(img) = &sp.image {
                        cell.fill_image = Some(img.clone());
                        if sp.tile == Some(true) {
                            cell.fill_tile = Some(true);
                        }
                    }
                }
            }
        }
        if cell.tx_color.is_none() && s.tx.is_some() {
            cell.tx_color = s.tx;
        }
        if cell.tx_bold.is_none() && s.b == Some(true) {
            cell.tx_bold = Some(true);
        }
        if cell.tx_italic.is_none() && s.i == Some(true) {
            cell.tx_italic = Some(true);
        }
    }
    // EXPLICIT borders: the top/left cell owns the edge; lnL/lnT only on the outline
    let own = merge_origins(t);
    for r in 0..n_r {
        for c in 0..n_c {
            let Some(&(or, oc)) = own.get(r).and_then(|x| x.get(c)) else { continue };
            let src: Option<&TableCell> = if or == r && oc == c {
                t.rows.get(r).and_then(|row| row.cells.get(c))
            } else if or == r || oc == c {
                t.rows.get(or).and_then(|row| row.cells.get(oc))
            } else {
                None
            };
            let Some(ex) = src.and_then(|s| s.lines.as_deref()) else { continue };
            if let Some(l) = &ex.r {
                v_l.set(r, c + 1, cell_edge(l));
            }
            if let Some(l) = &ex.b {
                h_l.set(r + 1, c, cell_edge(l));
            }
            if c == 0
                && let Some(l) = &ex.l
            {
                v_l.set(r, 0, cell_edge(l));
            }
            if r == 0
                && let Some(l) = &ex.t
            {
                h_l.set(0, c, cell_edge(l));
            }
        }
    }
    // INTERNAL edges of a merge do not show
    for r in 0..n_r {
        for c in 0..n_c {
            let Some(&o) = own.get(r).and_then(|x| x.get(c)) else { continue };
            if c + 1 < n_c && own.get(r).and_then(|x| x.get(c + 1)) == Some(&o) {
                v_l.set(r, c + 1, None);
            }
            if r + 1 < n_r && own.get(r + 1).and_then(|x| x.get(c)) == Some(&o) {
                h_l.set(r + 1, c, None);
            }
        }
    }
    t.h_lines = Some(h_l);
    t.v_lines = Some(v_l);
}

fn cell_edge(l: &crate::model::CellLine) -> Option<TableLine> {
    match l {
        crate::model::CellLine::NoFill => None,
        crate::model::CellLine::Line(x) => Some(x.clone()),
    }
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl LineGrid {
    /// `grid[r][c] = v`.
    pub fn set(&mut self, r: usize, c: usize, v: Option<TableLine>) {
        let cols = self.cols as usize;
        if r < self.rows as usize
            && c < cols
            && let Some(slot) = self.cells.get_mut(r * cols + c)
        {
            *slot = v;
        }
    }

    /// `grid[r]?.[c] ?? null`.
    pub fn get(&self, r: usize, c: usize) -> Option<&TableLine> {
        let cols = self.cols as usize;
        if r < self.rows as usize && c < cols { self.cells.get(r * cols + c)?.as_ref() } else { None }
    }
}

/// A solid fill of a colour (`{ type: 'solid', color }`).
pub fn solid(color: CssColor) -> Fill {
    let mut f = Fill::of_kind(FillKind::Solid);
    f.color = Some(color);
    f
}
