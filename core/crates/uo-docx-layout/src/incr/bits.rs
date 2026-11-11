//! State equality for the convergence of an incremental relayout (LA7): the engine's carried,
//! finding, output-counter and side-effecting cache fields, compared by their `Debug` strings
//! (exact for the values they hold: integers, strings and `f64`s whose `Debug` is the shortest
//! round-trip form, `-0.0` included; two NaNs of different payloads print the same — D-LA7-9).
//! The destructuring is exhaustive: a new `Engine` field does not compile until it is classed here.

use std::fmt::Write;

use crate::context::DocContext;
use crate::engine::Engine;

/// The `Debug` string of what an `Engine` carries to the rest of its pass: every field except
/// its pages (compared apart, the open ones), the trace, the style-resolver memo and the cell
/// neighbour memo (pure functions of the document, no side effect on a miss).
pub(crate) fn state_string(e: &Engine) -> String {
    let Engine {
        ctx,
        story,
        pages: _,
        cur,
        col,
        y,
        col_top,
        at_top,
        top_gap_prev_after,
        prev,
        last_box,
        first_in_doc,
        ln_count,
        ln_page,
        ln_sect,
        last_row_emit,
        tbl_x0,
        in_hf,
        float_carry,
        float_flow,
        next_table_layout,
        only_text_floats,
        pre_placed,
        z_seq,
        col_used,
        balance_saved,
        balance_last_free,
        placed_tables,
        late_split,
        col_bottoms,
        in_cell,
        hf_frame_after,
        hf_frame_tbl,
        static_dx,
        cell_mar,
        col_max_y,
        sect_gap_after,
        pending_join,
        pending_del,
        float_table_gap,
        carry_anchor_gap,
        note_carry,
        row_note_cut,
        flow_fills,
        en_flow,
        cur_endnote,
        endnotes_done,
        pinned_floats,
        chain_slices,
        field_at,
        cell_ps,
        cell_collect,
        late_marks,
        scratch,
        static_seq,
        field_page,
        en_blocks,
        sect_geoms,
        geoms,
        cover_page,
        first_page_number,
        list_texts,
        cell_nb: _,
        note_cache,
        sep_cache,
        found,
        fields,
        trace: _,
    } = e;
    let DocContext { flow, resolver: _, rev, warnings, note_numbering, note_mark_override } = ctx;
    let mut s = String::with_capacity(4096);
    macro_rules! put {
        ($($x:ident),* $(,)?) => { $( let _ = write!(s, concat!(stringify!($x), "={:?};"), $x); )* };
    }
    put!(flow, rev, warnings, note_numbering, note_mark_override);
    put!(story, cur, col, y, col_top, at_top, top_gap_prev_after, prev, last_box, first_in_doc, ln_count, ln_page, ln_sect);
    put!(last_row_emit, tbl_x0, in_hf, float_carry, float_flow, next_table_layout, only_text_floats, pre_placed, z_seq);
    put!(col_used, balance_saved, balance_last_free, placed_tables, late_split, col_bottoms, in_cell, hf_frame_after);
    put!(hf_frame_tbl, static_dx, cell_mar, col_max_y, sect_gap_after, pending_join, pending_del, float_table_gap);
    put!(carry_anchor_gap, note_carry, row_note_cut, flow_fills, en_flow, cur_endnote, endnotes_done, pinned_floats);
    put!(chain_slices, field_at, cell_ps, cell_collect, late_marks, scratch, static_seq, field_page, en_blocks, sect_geoms);
    put!(geoms, cover_page, first_page_number, list_texts, note_cache, sep_cache, found, fields);
    s
}

/// The `Debug` string of what `run()`'s prologue computed (a pure function of the view and the
/// options): equal before a resume, or the relayout is a full one.
pub(crate) fn prologue_string(e: &Engine) -> String {
    let geoms = e.geoms.get(..e.sect_geoms as usize).unwrap_or(&[]);
    format!("{:?};{:?};{:?};{:?}", e.cover_page, e.first_page_number, e.sect_geoms, geoms)
}
