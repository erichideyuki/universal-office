//! Line ids (`lineBox.line`, the call's `lineSeq` counter) of a previous run in a new run's
//! numbering (LA7): when an edit adds or removes painted lines, every line painted after it gets
//! another id in a full layout. A [`LineMap`] moves the ids at or past a point; [`Relabel`]
//! applies it to everything that holds items (exhaustive destructuring: a new field that holds
//! items does not compile until it is classed here). The convergence test compares `Debug`
//! strings: [`relabel_debug`] applies the same maps to the ids written in one.

use crate::engine::Engine;
use crate::engine::notes::{NoteCarry, NoteCut};
use crate::engine::page::{PageNote, PageState, ZObj};
use crate::engine::tables::{FloatCarry, FloatFlow, FlowMerge, LastRowEmit};
use crate::items::PassItems;
use crate::keys::LineSeq;
use crate::output::LayoutPage;
use crate::table::{CellContent, MergeText, NestRow, RowLayout, RowSplit};

/// Ids at or past `from` move by `by`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineMap {
    pub from: u32,
    pub by: i64,
}

impl LineMap {
    #[must_use]
    pub(crate) fn apply(self, id: u32) -> u32 {
        if id < self.from {
            return id;
        }
        u32::try_from(i64::from(id) + self.by).unwrap_or(id)
    }

    #[must_use]
    pub(crate) fn is_identity(self) -> bool {
        self.by == 0
    }
}

/// Applies `maps` in order.
pub(crate) fn map_id(maps: &[LineMap], id: u32) -> u32 {
    maps.iter().fold(id, |x, m| m.apply(x))
}

/// The line ids in `self` through `maps`.
pub(crate) trait Relabel {
    fn relabel(&mut self, maps: &[LineMap]);
}

impl Relabel for PassItems {
    fn relabel(&mut self, maps: &[LineMap]) {
        for s in &mut self.side {
            if let Some(b) = s.glyph.as_mut().and_then(|g| g.line_box.as_mut()) {
                b.line = LineSeq(map_id(maps, b.line.0));
            }
        }
    }
}

impl<T: Relabel> Relabel for Vec<T> {
    fn relabel(&mut self, maps: &[LineMap]) {
        for x in self {
            x.relabel(maps);
        }
    }
}

impl<T: Relabel> Relabel for Option<T> {
    fn relabel(&mut self, maps: &[LineMap]) {
        if let Some(x) = self {
            x.relabel(maps);
        }
    }
}

impl<T: Relabel> Relabel for Box<T> {
    fn relabel(&mut self, maps: &[LineMap]) {
        (**self).relabel(maps);
    }
}

impl<T: Relabel + Clone> Relabel for std::sync::Arc<T> {
    fn relabel(&mut self, maps: &[LineMap]) {
        std::sync::Arc::make_mut(self).relabel(maps);
    }
}

impl<K, V: Relabel, S> Relabel for indexmap::IndexMap<K, V, S> {
    fn relabel(&mut self, maps: &[LineMap]) {
        for v in self.values_mut() {
            v.relabel(maps);
        }
    }
}

impl<A: Relabel, B> Relabel for (A, B) {
    fn relabel(&mut self, maps: &[LineMap]) {
        self.0.relabel(maps);
    }
}

impl Relabel for CellContent {
    fn relabel(&mut self, maps: &[LineMap]) {
        let CellContent { items, lines: _, height: _, keep_next: _, paras: _, nest, align_h: _, shape: _ } = self;
        items.relabel(maps);
        nest.relabel(maps);
    }
}

impl Relabel for NestRow {
    fn relabel(&mut self, maps: &[LineMap]) {
        let NestRow { top: _, row, items: _, lines: _ } = self;
        row.relabel(maps);
    }
}

impl Relabel for RowLayout {
    fn relabel(&mut self, maps: &[LineMap]) {
        let RowLayout {
            height: _,
            items,
            lines: _,
            cant_split: _,
            header: _,
            keep_next: _,
            natural: _,
            border_items,
            break_border_items,
            merge_at: _,
            merge_text,
            break_w: _,
            split,
            top_cut: _,
            top_borders: _,
            own_top_borders,
            cut: _,
            merge_fills: _,
            cells: _,
        } = self;
        items.relabel(maps);
        border_items.relabel(maps);
        break_border_items.relabel(maps);
        merge_text.relabel(maps);
        split.relabel(maps);
        own_top_borders.relabel(maps);
    }
}

impl Relabel for MergeText {
    fn relabel(&mut self, maps: &[LineMap]) {
        let MergeText { col: _, items, lines: _, dy: _, start: _, content, mar_t: _, band: _, v_align: _ } = self;
        items.relabel(maps);
        content.relabel(maps);
    }
}

impl Relabel for RowSplit {
    fn relabel(&mut self, maps: &[LineMap]) {
        // (`rc` holds the table's description and cell geometry: no items)
        self.contents.relabel(maps);
    }
}

impl Relabel for crate::table::TableLayout {
    fn relabel(&mut self, maps: &[LineMap]) {
        let crate::table::TableLayout { x: _, width: _, rows, border_left: _, eff_r: _, tblp_pr: _ } = self;
        rows.relabel(maps);
    }
}

impl Relabel for ZObj {
    fn relabel(&mut self, maps: &[LineMap]) {
        self.items.relabel(maps);
    }
}

impl Relabel for PageNote {
    fn relabel(&mut self, maps: &[LineMap]) {
        self.content.relabel(maps);
    }
}

impl Relabel for PageState {
    fn relabel(&mut self, maps: &[LineMap]) {
        let PageState {
            page: _,
            geom: _,
            body_top: _,
            body_bottom: _,
            behind,
            hf,
            body,
            front,
            hf_behind,
            hf_front,
            floats: _,
            sect_page: _,
            notes,
            note_area: _,
            body_bottom_base: _,
            notes_full: _,
            cols: _,
        } = self;
        behind.relabel(maps);
        hf.relabel(maps);
        body.relabel(maps);
        front.relabel(maps);
        hf_behind.relabel(maps);
        hf_front.relabel(maps);
        notes.relabel(maps);
    }
}

impl Relabel for FloatCarry {
    fn relabel(&mut self, maps: &[LineMap]) {
        let FloatCarry { id: _, t, rows, fp: _, ref_x: _ } = self;
        t.relabel(maps);
        rows.relabel(maps);
    }
}

impl Relabel for FloatFlow {
    fn relabel(&mut self, maps: &[LineMap]) {
        let FloatFlow { moved: _, seen: _, emits: _, merges } = self;
        merges.relabel(maps);
    }
}

impl Relabel for FlowMerge {
    fn relabel(&mut self, maps: &[LineMap]) {
        self.content.relabel(maps);
    }
}

impl Relabel for NoteCarry {
    fn relabel(&mut self, maps: &[LineMap]) {
        self.content.relabel(maps);
    }
}

impl Relabel for NoteCut {
    fn relabel(&mut self, maps: &[LineMap]) {
        let NoteCut { id: _, first, rest } = self;
        first.relabel(maps);
        rest.relabel(maps);
    }
}

impl Relabel for LastRowEmit {
    fn relabel(&mut self, maps: &[LineMap]) {
        self.break_border_items.relabel(maps);
    }
}

impl Relabel for Engine {
    fn relabel(&mut self, maps: &[LineMap]) {
        let Engine {
            ctx: _,
            story: _,
            pages,
            cur: _,
            col: _,
            y: _,
            col_top: _,
            at_top: _,
            top_gap_prev_after: _,
            prev: _,
            last_box: _,
            first_in_doc: _,
            ln_count: _,
            ln_page: _,
            ln_sect: _,
            last_row_emit,
            tbl_x0: _,
            in_hf: _,
            float_carry,
            float_flow,
            next_table_layout: _,
            only_text_floats: _,
            pre_placed: _,
            z_seq: _,
            col_used: _,
            balance_saved: _,
            balance_last_free: _,
            placed_tables: _,
            late_split: _,
            col_bottoms: _,
            in_cell: _,
            hf_frame_after,
            hf_frame_tbl,
            static_dx: _,
            cell_mar: _,
            col_max_y: _,
            sect_gap_after: _,
            pending_join: _,
            pending_del: _,
            float_table_gap: _,
            carry_anchor_gap: _,
            note_carry,
            row_note_cut,
            flow_fills: _,
            en_flow: _,
            cur_endnote: _,
            endnotes_done: _,
            pinned_floats: _,
            chain_slices,
            field_at: _,
            cell_ps: _,
            cell_collect: _,
            late_marks: _,
            scratch,
            static_seq: _,
            field_page: _,
            en_blocks: _,
            sect_geoms: _,
            geoms: _,
            cover_page: _,
            first_page_number: _,
            list_texts: _,
            cell_nb: _,
            note_cache,
            sep_cache: _,
            found: _,
            fields: _,
            trace: _,
        } = self;
        pages.relabel(maps);
        last_row_emit.relabel(maps);
        float_carry.relabel(maps);
        float_flow.relabel(maps);
        hf_frame_after.relabel(maps);
        hf_frame_tbl.relabel(maps);
        note_carry.relabel(maps);
        row_note_cut.relabel(maps);
        chain_slices.relabel(maps);
        scratch.relabel(maps);
        note_cache.relabel(maps);
    }
}

impl Relabel for LayoutPage {
    fn relabel(&mut self, maps: &[LineMap]) {
        for (_, g) in &mut self.item_meta {
            if let Some(b) = g.line_box.as_mut() {
                b.line = LineSeq(map_id(maps, b.line.0));
            }
        }
    }
}

/// `s` (a `Debug` string) with every `LineSeq(n)` through `maps`.
pub(crate) fn relabel_debug(s: &str, maps: &[LineMap]) -> String {
    if maps.iter().all(|m| m.is_identity()) {
        return s.to_owned();
    }
    const TAG: &str = "LineSeq(";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(TAG) {
        let (head, tail) = rest.split_at(i + TAG.len());
        out.push_str(head);
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        let (num, after) = tail.split_at(digits);
        match num.parse::<u32>() {
            Ok(n) => out.push_str(&map_id(maps, n).to_string()),
            Err(_) => out.push_str(num),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}
