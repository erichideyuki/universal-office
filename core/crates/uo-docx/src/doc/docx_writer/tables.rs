//! Blocks (paragraphs and tables) of a story: `renderBlocks`, `renderTable` and the table, row and
//! cell properties.

use std::collections::BTreeSet;
use std::rc::Rc;

use super::body::PInfo;
use super::stories::SectMap;
use super::{DocxWriter, PartRels, Story};
use crate::doc::ooxml_props::{brc_xml, shd_xml};
use crate::doc::props::{PapKey, Shd, TableBorders, Tap, Tc, apply_tap, brc_none, finish_tap};
use crate::doc::sprm::{prls, sgc_of};
use crate::doc::xmlw::{AttrVal, el_of};
use crate::doc::{DocError, DocLimitKind};

/// The keys of a paragraph frame (`FRAME_KEYS`): a positioned table's position, not its paragraphs'.
const FRAME_KEYS: [PapKey; 9] = [
    PapKey::DxaAbs,
    PapKey::DyaAbs,
    PapKey::DxaWidth,
    PapKey::Pc,
    PapKey::Wr,
    PapKey::WHeightAbs,
    PapKey::DyaFromText,
    PapKey::DxaFromText,
    PapKey::FrameLocked,
];

/// A row of a table: the cells as `[from, to)` paragraph ranges, its TAP, the paragraph of its end
/// mark and the cell boundaries.
struct Row {
    cells: Vec<(usize, usize)>,
    /// shared: the last row that has no end mark shares the TAP of the one before it (the TS keeps
    /// one object, and `tblPrEx` compares by identity)
    tap: Rc<Tap>,
    ttp: Option<usize>,
    centers: Vec<i64>,
}

fn int(v: i64) -> Option<AttrVal<'static>> {
    Some(AttrVal::Int(v))
}

/// `widthXml`: a width with its type, or nothing.
fn width_xml(name: &str, w: Option<(u8, i64)>) -> String {
    let Some((fts, w)) = w else { return String::new() };
    if fts == 0 {
        return String::new();
    }
    let ty = ["nil", "auto", "pct", "dxa"].get(usize::from(fts & 3)).copied().unwrap_or("dxa");
    el_of(name, &[("w:w", int(if fts == 1 { 0 } else { w })), ("w:type", Some(AttrVal::Str(ty)))], "")
}

/// `bordersXml`.
fn borders_xml(b: &TableBorders, inside: bool) -> String {
    let mut x = String::new();
    let sides = [
        ("w:top", &b.top),
        ("w:left", &b.left),
        ("w:bottom", &b.bottom),
        ("w:right", &b.right),
        ("w:insideH", &b.inside_h),
        ("w:insideV", &b.inside_v),
    ];
    for (name, brc) in sides.iter().take(if inside { 6 } else { 4 }) {
        x += &brc_xml(name, brc.as_ref());
    }
    x
}

fn is_positioned(tap: &Tap) -> bool {
    tap.dxa_abs.is_some() || tap.dya_abs.is_some() || tap.pc.is_some()
}

fn all_none(b: &TableBorders) -> bool {
    [&b.top, &b.left, &b.bottom, &b.right, &b.inside_h, &b.inside_v].iter().all(|s| brc_none(s.as_ref()))
}

/// `cellMargins`: the default cell margins of the row as `w:top`… elements.
fn cell_margins(tap: &Tap) -> String {
    let [top, mut left, bottom, mut right] = tap.padding;
    // Word 97: without sprmTCellPaddingDefault the margin is the dxaGapHalf
    if left.is_none()
        && right.is_none()
        && let Some(g) = tap.dxa_gap_half
    {
        left = Some(g);
        right = Some(g);
    }
    let mut m = String::new();
    for (name, v) in [("w:top", top), ("w:left", left), ("w:bottom", bottom), ("w:right", right)] {
        if let Some(v) = v {
            m += &el_of(name, &[("w:w", int(i64::from(v))), ("w:type", Some(AttrVal::Str("dxa")))], "");
        }
    }
    m
}

/// `tableIndent`: the indent of the table (compatibility mode 11: up to the text of the first cell).
fn table_indent(tap: &Tap) -> Option<i64> {
    let first = *tap.centers.first()?;
    let left = tap.tcs.first().and_then(|t| t.mar[1]).or(tap.padding[1]).or(tap.dxa_gap_half).unwrap_or(0);
    Some(first + i64::from(left))
}

/// `cellShd`: the shading of the cell `i`.
fn cell_shd(tap: &Tap, i: usize) -> Option<Shd> {
    if let Some(shd) = tap.tcs.get(i).and_then(|tc| tc.shd) {
        return Some(shd);
    }
    let pick = |arr: &Option<Vec<Shd>>| -> Option<Shd> {
        let s = arr.as_ref()?.get(i).copied()?;
        // ShdAuto (all automatic, pattern clear) = no shading
        if s.pat == 0 && s.back == -1 && s.fore == -1 {
            return None;
        }
        if s.pat == 0xffff {
            return None;
        }
        Some(s)
    };
    pick(&tap.shd_raw).or_else(|| pick(&tap.shd_def)).or_else(|| pick(&tap.shd80))
}

fn text_direction(v: u16) -> &'static str {
    ["lrTb", "tbRl", "btLr", "lrTbV", "tbRlV", "tbLrV"].get(usize::from(v)).copied().unwrap_or("lrTb")
}

impl<'a> DocxWriter<'a> {
    /// `renderBlocks`: the paragraphs and tables of `paras[from..to]` at table depth `depth`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_blocks(
        &mut self,
        paras: &mut [PInfo<'a>],
        from: usize,
        to: usize,
        depth: i64,
        ctx: Story,
        rels: &mut PartRels,
        sect: Option<&SectMap<'_>>,
    ) -> Result<String, DocError> {
        let mut x = String::new();
        let mut i = from;
        while i < to {
            if paras[i].depth > depth {
                let mut j = i;
                while j < to && paras[j].depth > depth {
                    j += 1;
                }
                x += &self.render_table(paras, i, j, depth + 1, ctx, rels)?;
                i = j;
                continue;
            }
            let sect_pr = sect.and_then(|s| s.of(&paras[i])).map(str::to_owned);
            x += &self.render_paragraph(&paras[i], ctx, rels, sect_pr.as_deref())?;
            i += 1;
        }
        Ok(x)
    }

    /// `tapOf`: the TAP of the row whose end mark is `row`.
    fn tap_of(&self, row: &PInfo<'a>) -> Tap {
        let mut tap = Tap::new();
        // the table style: the TAPX of the style first (without the structure of the cells)
        for p in &row.tap_prls {
            if p.sprm == 0x563a
                && let Some(tapx) = self.r.styles.style(p.u16()).and_then(|st| st.tapx)
            {
                for q in prls(tapx) {
                    if sgc_of(q.sprm) == 5 && q.sprm != 0x563a {
                        apply_tap(&mut tap, &q);
                    }
                }
            }
        }
        for p in &row.tap_prls {
            apply_tap(&mut tap, p);
        }
        finish_tap(&mut tap);
        tap
    }

    /// `renderTable`.
    fn render_table(
        &mut self,
        paras: &mut [PInfo<'a>],
        from: usize,
        to: usize,
        depth: i64,
        ctx: Story,
        rels: &mut PartRels,
    ) -> Result<String, DocError> {
        if self.nest >= self.limits.max_nesting {
            return Err(DocError::Limit { kind: DocLimitKind::Nesting, max: u64::from(self.limits.max_nesting) });
        }
        self.nest += 1;
        let res = self.render_table_inner(paras, from, to, depth, ctx, rels);
        self.nest -= 1;
        res
    }

    fn render_table_inner(
        &mut self,
        paras: &mut [PInfo<'a>],
        from: usize,
        to: usize,
        depth: i64,
        ctx: Story,
        rels: &mut PartRels,
    ) -> Result<String, DocError> {
        let mut rows: Vec<Row> = Vec::new();
        let mut cells: Vec<(usize, usize)> = Vec::new();
        let mut cell_start = from;
        #[expect(clippy::needless_range_loop, reason = "`k` is also the cell and row boundary index, as in the TS loop")]
        for k in from..to {
            if paras[k].depth != depth {
                continue;
            }
            if paras[k].row_end {
                if k > cell_start && cells.is_empty() {
                    cells.push((cell_start, k));
                }
                let tap = Rc::new(self.tap_of(&paras[k]));
                rows.push(Row { cells: std::mem::take(&mut cells), tap, ttp: Some(k), centers: Vec::new() });
                cell_start = k + 1;
            } else if paras[k].cell_end {
                cells.push((cell_start, k + 1));
                cell_start = k + 1;
            }
        }
        if !cells.is_empty() || cell_start < to {
            if cell_start < to {
                cells.push((cell_start, to));
            }
            let tap = match rows.last() {
                Some(r) => Rc::clone(&r.tap),
                None => Rc::new(Tap::new()),
            };
            rows.push(Row { cells, tap, ttp: None, centers: Vec::new() });
        }
        if rows.is_empty() {
            return Ok(String::new());
        }
        self.charge(u64::try_from(rows.len()).unwrap_or(u64::MAX))?;
        // the grid: every cell boundary (the Word builds it from the rgdxaCenter)
        for row in &mut rows {
            row.centers = row.tap.centers.clone();
        }
        // centred rows: the Word centres EACH row; the rgdxaCenter of rows of almost equal widths can
        // be shifted from one to another (corpus t119): the grid is built with each row centred in its middle
        let centered = rows.iter().all(|r| r.tap.jc == Some(1)) && !is_positioned(&rows[0].tap);
        if centered {
            for row in &mut rows {
                let c = &row.centers;
                let n = i64::try_from(row.cells.len()).unwrap_or(i64::MAX).min(i64::try_from(c.len()).unwrap_or(i64::MAX) - 1);
                if n > 0
                    && let (Some(&a), Some(&b)) = (c.first(), c.get(usize::try_from(n).unwrap_or(usize::MAX)))
                {
                    // Math.round((a + b) / 2): half up
                    let sum = a + b;
                    let mid = sum.div_euclid(2) + sum.rem_euclid(2);
                    row.centers = c.iter().map(|v| v - mid).collect();
                }
            }
        }
        let mut bounds: BTreeSet<i64> = BTreeSet::new();
        for row in &rows {
            for (i, &v) in row.centers.iter().enumerate() {
                if i > row.cells.len() {
                    break;
                }
                bounds.insert(v);
            }
        }
        let grid: Vec<i64> = bounds.into_iter().collect();
        let first = Rc::clone(&rows[0].tap);
        let mut x = String::from("<w:tbl>");
        let centered_w = if centered && grid.len() > 1 { Some(grid[grid.len() - 1] - grid[0]) } else { None };
        x += &format!("<w:tblPr>{}</w:tblPr>", self.tbl_pr_xml(&first, centered_w));
        x += "<w:tblGrid>";
        for i in 0..grid.len().saturating_sub(1) {
            x += &el_of("w:gridCol", &[("w:w", int(grid[i + 1] - grid[i]))], "");
        }
        if grid.len() < 2 {
            x += &el_of("w:gridCol", &[("w:w", int(0))], "");
        }
        x += "</w:tblGrid>";
        // the index of the grid boundary nearest to a position (the first one at a tie)
        let grid_idx = |pos: i64| -> i64 {
            let i = grid.partition_point(|&g| g < pos);
            let best = if i == 0 {
                0
            } else if i >= grid.len() || pos - grid[i - 1] <= grid[i] - pos {
                i - 1
            } else {
                i
            };
            i64::try_from(best).unwrap_or(0)
        };
        let grid_len = i64::try_from(grid.len()).unwrap_or(i64::MAX);
        for row in &rows {
            let tap = &row.tap;
            // a positioned table (tblpPr): the frames of the paragraphs of the cells are the position of
            // the TABLE; the Word does not repeat them in the paragraphs (measured in the Word)
            if is_positioned(tap) {
                for &(cs, ce) in &row.cells {
                    for p in &mut paras[cs..ce] {
                        if p.depth == depth {
                            for key in FRAME_KEYS {
                                p.touched.delete(key);
                            }
                        }
                    }
                }
            }
            let c = &row.centers;
            let c_len = i64::try_from(c.len()).unwrap_or(i64::MAX);
            let mut tr_pr = String::new();
            let start_idx = c.first().map_or(0, |&v| grid_idx(v));
            let end_pos = usize::try_from(i64::try_from(row.cells.len()).unwrap_or(i64::MAX).min(c_len - 1)).ok().and_then(|i| c.get(i));
            let end_idx = if c.is_empty() { 0 } else { end_pos.map_or(0, |&v| grid_idx(v)) };
            if start_idx > 0 {
                tr_pr += &el_of("w:gridBefore", &[("w:val", int(start_idx))], "");
            }
            if end_idx < grid_len - 1 && !c.is_empty() {
                tr_pr += &el_of("w:gridAfter", &[("w:val", int(grid_len - 1 - end_idx))], "");
            }
            if start_idx > 0 {
                let w = grid[usize::try_from(start_idx).unwrap_or(0)] - grid[0];
                tr_pr += &el_of("w:wBefore", &[("w:w", int(w)), ("w:type", Some(AttrVal::Str("dxa")))], "");
            }
            if end_idx < grid_len - 1 && !c.is_empty() {
                let w = grid[grid.len() - 1] - grid[usize::try_from(end_idx).unwrap_or(0)];
                tr_pr += &el_of("w:wAfter", &[("w:w", int(w)), ("w:type", Some(AttrVal::Str("dxa")))], "");
            }
            if tap.cant_split == Some(true) {
                tr_pr += "<w:cantSplit/>";
            }
            if let Some(h) = tap.row_height
                && h != 0
            {
                tr_pr +=
                    &el_of("w:trHeight", &[("w:val", int(i64::from(h).abs())), ("w:hRule", (h < 0).then_some(AttrVal::Str("exact")))], "");
            }
            if tap.header == Some(true) {
                tr_pr += "<w:tblHeader/>";
            }
            if let Some(jc) = tap.jc
                && jc != 0
            {
                let v = ["left", "center", "right"].get(usize::from(jc)).copied().unwrap_or("left");
                tr_pr += &el_of("w:jc", &[("w:val", Some(AttrVal::Str(v)))], "");
            }
            // the hidden end-of-row mark (vanish) is a hidden row
            if let Some(t) = row.ttp {
                let end = paras[t].cp_end - 1;
                let entry = self.chp_for(&paras[t], end);
                if entry.chp.vanish == Some(true) {
                    tr_pr += "<w:hidden/>";
                }
            }
            x += "<w:tr>";
            let ex = self.tbl_pr_ex_xml(tap, &first);
            if !ex.is_empty() {
                x += &format!("<w:tblPrEx>{ex}</w:tblPrEx>");
            }
            if !tr_pr.is_empty() {
                x += &format!("<w:trPr>{tr_pr}</w:trPr>");
            }
            let mut ci = 0usize;
            while ci < row.cells.len() {
                let tc = tap.tcs.get(ci);
                let (cs, ce) = row.cells[ci];
                let mut span = 1usize;
                // cells merged horizontally (fMerged) vanish into the first
                if tc.is_some_and(|tc| tc.horz_merge >= 2) {
                    while ci + span < row.cells.len() && tap.tcs.get(ci + span).is_some_and(|t| t.horz_merge == 1) {
                        span += 1;
                    }
                }
                let left = c.get(ci).copied().unwrap_or(0);
                let right_idx = i64::try_from(ci + span).unwrap_or(i64::MAX).min(c_len - 1);
                let right = usize::try_from(right_idx).ok().and_then(|i| c.get(i)).copied().unwrap_or(left);
                let gs = (grid_idx(right) - grid_idx(left)).max(1);
                x += &format!("<w:tc><w:tcPr>{}</w:tcPr>", self.tc_pr_xml(tap, ci, gs, right - left));
                let inner = self.render_blocks(paras, cs, ce, depth, ctx, rels, None)?;
                x += if inner.is_empty() { "<w:p/>" } else { &inner };
                // the content of the merged cells (normally empty) goes into the first
                for k in 1..span {
                    let (ms, me) = row.cells[ci + k];
                    if paras[ms..me].iter().any(|pp| pp.cp_end - pp.cp_start > 1) {
                        x += &self.render_blocks(paras, ms, me, depth, ctx, rels, None)?;
                    }
                }
                x += "</w:tc>";
                ci += span;
            }
            x += "</w:tr>";
        }
        x += "</w:tbl>";
        Ok(x)
    }

    /// `tblPrXml`.
    fn tbl_pr_xml(&self, tap: &Tap, centered_w: Option<i64>) -> String {
        let mut x = String::new();
        if let Some(istd) = tap.istd
            && let Some(id) = self.style_ids.get(istd)
            && !id.is_empty()
            && istd != 11
        {
            x += &el_of("w:tblStyle", &[("w:val", Some(AttrVal::Str(id)))], "");
        }
        if is_positioned(tap) {
            let pc = tap.pc.unwrap_or(0);
            let pc_vert = usize::from((pc >> 4) & 3);
            let pc_horz = usize::from((pc >> 6) & 3);
            let xa = tap.dxa_abs.unwrap_or(0);
            let ya = tap.dya_abs.unwrap_or(0);
            let x_al = match xa {
                -4 => Some("center"),
                -8 => Some("right"),
                -12 => Some("inside"),
                -16 => Some("outside"),
                _ => None,
            };
            let y_al = match ya {
                -4 => Some("top"),
                -8 => Some("center"),
                -12 => Some("bottom"),
                -16 => Some("inside"),
                -20 => Some("outside"),
                _ => None,
            };
            let v = |o: Option<i16>| o.map(|v| AttrVal::Int(i64::from(v)));
            let xa_s = i64::from(xa);
            let ya_s = i64::from(ya);
            x += &el_of(
                "w:tblpPr",
                &[
                    ("w:leftFromText", v(tap.dxa_from_text)),
                    ("w:rightFromText", v(tap.dxa_from_text_right)),
                    ("w:topFromText", v(tap.dya_from_text)),
                    ("w:bottomFromText", v(tap.dya_from_text_bottom)),
                    ("w:vertAnchor", Some(AttrVal::Str(["margin", "page", "text"].get(pc_vert).copied().unwrap_or("text")))),
                    ("w:horzAnchor", Some(AttrVal::Str(["text", "margin", "page"].get(pc_horz).copied().unwrap_or("text")))),
                    ("w:tblpXSpec", x_al.map(AttrVal::Str)),
                    ("w:tblpX", if x_al.is_none() && xa != 0 { int(xa_s) } else { None }),
                    ("w:tblpYSpec", y_al.map(AttrVal::Str)),
                    ("w:tblpY", if y_al.is_none() && ya != 0 { int(ya_s) } else { None }),
                ],
                "",
            );
            if tap.no_allow_overlap == Some(true) {
                x += "<w:tblOverlap w:val=\"never\"/>";
            }
        }
        if tap.bidi == Some(true) {
            x += "<w:bidiVisual/>";
        }
        // (centred rows: the dxa width does not pass the grid that was built: it added the shift)
        let tw = tap.table_width.map(|w| (w.fts, i64::from(w.w)));
        let tw = match (centered_w, tw) {
            (Some(cw), Some((3, w))) if w > cw => Some((3u8, cw)),
            _ => tw,
        };
        let tbl_w = width_xml("w:tblW", tw);
        x += &if tbl_w.is_empty() { el_of("w:tblW", &[("w:w", int(0)), ("w:type", Some(AttrVal::Str("auto")))], "") } else { tbl_w };
        if let Some(jc) = tap.jc
            && jc != 0
        {
            let v = ["left", "center", "right"].get(usize::from(jc)).copied().unwrap_or("left");
            x += &el_of("w:jc", &[("w:val", Some(AttrVal::Str(v)))], "");
        }
        if let Some(sp) = tap.spacing
            && sp != 0
        {
            x += &el_of("w:tblCellSpacing", &[("w:w", int(i64::from(sp))), ("w:type", Some(AttrVal::Str("dxa")))], "");
        }
        // sprmTWidthIndent is the w:tblInd the Word writes; without it, computed from the first cell. In a
        // centred table it does not position, but in the modes <= 14 it still changes the maximum width of the
        // autofit (oracle tblindctr of docx-layout): it goes too
        let ind = match tap.width_indent {
            Some(w) if w.fts == 3 => Some(i64::from(w.w)),
            _ => table_indent(tap),
        };
        if let Some(ind) = ind
            && ind != 0
            && (tap.jc.is_none_or(|j| j == 0) || tap.jc == Some(1))
        {
            x += &el_of("w:tblInd", &[("w:w", int(ind)), ("w:type", Some(AttrVal::Str("dxa")))], "");
        }
        if tap.borders_set && !all_none(&tap.borders) {
            x += &format!("<w:tblBorders>{}</w:tblBorders>", borders_xml(&tap.borders, true));
        }
        if let Some(shd) = &tap.shd_table {
            x += &shd_xml(Some(shd));
        }
        if tap.autofit != Some(true) {
            x += "<w:tblLayout w:type=\"fixed\"/>";
        }
        let mar = cell_margins(tap);
        if !mar.is_empty() {
            x += &format!("<w:tblCellMar>{mar}</w:tblCellMar>");
        }
        // TLP.grfatl: bits 0x20..0x400 = first/last row/column, no banding (the same bits of w:tblLook)
        let look = tap.tlp.map_or(0, |t| t.grfatl) & 0x07e0;
        let val = format!("{look:04X}");
        let bit = |m: u16| Some(AttrVal::Int(i64::from(look & m != 0)));
        x += &el_of(
            "w:tblLook",
            &[
                ("w:val", Some(AttrVal::Str(&val))),
                ("w:firstRow", bit(0x20)),
                ("w:lastRow", bit(0x40)),
                ("w:firstColumn", bit(0x80)),
                ("w:lastColumn", bit(0x100)),
                ("w:noHBand", bit(0x200)),
                ("w:noVBand", bit(0x400)),
            ],
            "",
        );
        x
    }

    /// `tblPrExXml`: the properties of the row that differ from the first row.
    fn tbl_pr_ex_xml(&self, tap: &Rc<Tap>, first: &Rc<Tap>) -> String {
        if Rc::ptr_eq(tap, first) {
            return String::new();
        }
        let mut x = String::new();
        // JSON.stringify of the borders: the same values in the same key order
        if tap.borders_set && tap.borders != first.borders {
            x += &format!("<w:tblBorders>{}</w:tblBorders>", borders_xml(&tap.borders, true));
        }
        let mar = cell_margins(tap);
        if !mar.is_empty() && mar != cell_margins(first) {
            x += &format!("<w:tblCellMar>{mar}</w:tblCellMar>");
        }
        x
    }

    /// `tcPrXml`.
    fn tc_pr_xml(&self, tap: &Tap, i: usize, grid_span: i64, width: i64) -> String {
        let tc: Option<&Tc> = tap.tcs.get(i);
        let mut x = String::new();
        match tc {
            Some(tc) if tc.fts_width == 1 => x += &el_of("w:tcW", &[("w:w", int(0)), ("w:type", Some(AttrVal::Str("auto")))], ""),
            Some(tc) if tc.fts_width != 0 => x += &width_xml("w:tcW", Some((tc.fts_width, i64::from(tc.w_width)))),
            _ => x += &el_of("w:tcW", &[("w:w", int(width.max(0))), ("w:type", Some(AttrVal::Str("dxa")))], ""),
        }
        if grid_span > 1 {
            x += &el_of("w:gridSpan", &[("w:val", int(grid_span))], "");
        }
        if let Some(tc) = tc
            && tc.vert_merge != 0
        {
            x += if tc.vert_merge == 3 || tc.vert_merge == 2 { "<w:vMerge w:val=\"restart\"/>" } else { "<w:vMerge/>" };
        }
        if let Some(tc) = tc {
            let sides = [
                ("w:top", &tc.brc_top, tc.brc_set[0]),
                ("w:left", &tc.brc_left, tc.brc_set[1]),
                ("w:bottom", &tc.brc_bottom, tc.brc_set[2]),
                ("w:right", &tc.brc_right, tc.brc_set[3]),
            ];
            let b: String = sides.iter().filter(|(_, _, set)| *set).map(|(n, brc, _)| brc_xml(n, brc.as_ref())).collect();
            if !b.is_empty() {
                x += &format!("<w:tcBorders>{b}</w:tcBorders>");
            }
        }
        if let Some(shd) = cell_shd(tap, i) {
            x += &shd_xml(Some(&shd));
        }
        if tc.is_some_and(|t| t.no_wrap) {
            x += "<w:noWrap/>";
        }
        if let Some(tc) = tc
            && tc.mar.iter().any(Option::is_some)
        {
            let mut m = String::new();
            for (name, v) in [("w:top", tc.mar[0]), ("w:left", tc.mar[1]), ("w:bottom", tc.mar[2]), ("w:right", tc.mar[3])] {
                if let Some(v) = v {
                    m += &el_of(name, &[("w:w", int(i64::from(v))), ("w:type", Some(AttrVal::Str("dxa")))], "");
                }
            }
            x += &format!("<w:tcMar>{m}</w:tcMar>");
        }
        if let Some(tc) = tc
            && tc.text_flow != 0
        {
            x += &el_of("w:textDirection", &[("w:val", Some(AttrVal::Str(text_direction(u16::from(tc.text_flow)))))], "");
        }
        if tc.is_some_and(|t| t.fit_text) {
            x += "<w:tcFitText/>";
        }
        if let Some(tc) = tc
            && tc.vert_align != 0
        {
            let v = ["top", "center", "bottom"].get(usize::from(tc.vert_align)).copied().unwrap_or("top");
            x += &el_of("w:vAlign", &[("w:val", Some(AttrVal::Str(v)))], "");
        }
        if tc.is_some_and(|t| t.hide_mark) {
            x += "<w:hideMark/>";
        }
        x
    }
}
