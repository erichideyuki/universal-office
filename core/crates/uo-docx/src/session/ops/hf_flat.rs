//! Wave 7 (B, HF-T1): the flat editing view of a story that holds a table: a header/footer part, a
//! note, a textbox.
//!
//! The TS editor edits a header/footer part through `hfParts.paras` = `flattenBlocks(blocks)`
//! (notes.ts:96): the paragraphs of the part WITH those of its table cells, depth first. A click in
//! a header cell puts the caret in that cell's paragraph (the edit lines of the part are flattened
//! the same way: `uo-docx-layout` `hit.rs` `scope_paras`), and the typing goes there. Before every
//! layout and the save the TS aligns the blocks to the edited list (`hfPartBlocks` →
//! `reconcileBlocks(blocks, paras, true)`, engine-doc.ts:229): the paragraphs that are gone leave
//! their place, a new one goes right after the previous paragraph of the list that was there
//! before (by OBJECT identity), a cell left without a paragraph gets an empty one.
//!
//! Here the context story of such a part is [`HfFlat::story`] (paragraphs only); after every
//! command (`Session::change_set`) [`Session::sync_flat_views`] reconciles it into the part's blocks
//! by paragraph identity ([`ParaId`], G22: an in-place edit keeps it, a new TS object takes a new
//! one) and rebuilds the view from the result, so `flatten(story) == flat` between commands: the
//! layout, the writer and the snapshots see the blocks. Differences from the TS, both on edges:
//! the TS reconciles from the blocks it READ every time (a new paragraph whose previous paragraph
//! is deleted later moves with the new previous one), and the empty paragraph of an emptied cell is
//! not in its list (here it is: the view is rebuilt from the blocks). A part without a table has no
//! view: its paragraphs are already the list.
//!
//! Notes and textboxes follow the same rule (wave 7, B, fix round 2). The TS edits a note through
//! `this.model = { paragraphs: flattenBlocks(blocks) }` (universal-write.ts `enterNote`) and puts the
//! edits back with `syncNoteEdit` (`reconcileBlocks(blocks, paragraphs, true)`); it edits a box
//! through `tb.paras` (= `flattenBlocks(tb.blocks)` for a box read from a .docx, the `Synced` view)
//! and reconciles `tb.blocks` the same way (`liveEngineDoc`, `exitTextbox`). The layout's hit test
//! flattens both scopes (`scope_paras`). The note's view lives in [`crate::model::Notes::flat`], the
//! box's in its `DrawingTextbox::flat`; both are [`crate::model::section::EditSlot`]s, outside the
//! model's equality and dropped by the JSON round trip of an undo step (`JsNormalize`). A box of the
//! ODF reader (`TopLevel`: the TS `paras` are its top-level paragraphs) keeps editing those.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::context::{EditContext, story_of, story_of_mut};
use super::drawing::loc;
use crate::ids::{HfPartId, IdAlloc, ParaId};
use crate::js::NumKey;
use crate::model::document::NoteFlat;
use crate::model::para::Paragraph;
use crate::model::run::TextboxView;
use crate::model::section::HfFlat;
use crate::model::story::{Block, Story};
use crate::model::table::Table;
use crate::pvec::PVec;

/// The paragraphs of `s` with those of its cells, depth first (`flattenBlocks`), as the shared nodes.
#[must_use]
pub fn flatten_arcs(s: &Story) -> Vec<Arc<Paragraph>> {
    fn walk(s: &Story, out: &mut Vec<Arc<Paragraph>>, depth: u32) {
        let (mut pi, mut ti) = (0usize, 0usize);
        // `Story::blocks` order: the anchors of paragraph N before it, the rest after the last
        loop {
            let table = s.tables.get(ti).filter(|a| a.before_para as usize <= pi || pi >= s.paras.len());
            if let Some(a) = table {
                ti += 1;
                if depth < 64 {
                    for row in &a.table.rows {
                        for c in &row.cells {
                            walk(&c.content, out, depth + 1);
                        }
                    }
                }
                continue;
            }
            match s.paras.get_arc(pi) {
                Some(p) => {
                    out.push(Arc::clone(p));
                    pi += 1;
                }
                None => break,
            }
        }
    }
    let mut out = Vec::new();
    walk(s, &mut out, 0);
    out
}

/// `reconcileBlocks(blocks, flat, true)` by paragraph identity: `story`'s blocks aligned to `flat`
/// (a paragraph of both: `flat`'s node in its place; the rest as the module comment says).
#[must_use]
pub fn reconcile(story: &Story, flat: &[Arc<Paragraph>], ids: &mut IdAlloc) -> Story {
    let order = flatten_arcs(story);
    let present: HashSet<ParaId> = order.iter().map(|p| p.id).collect();
    let live: HashMap<ParaId, &Arc<Paragraph>> = flat.iter().map(|p| (p.id, p)).collect();
    // new paragraphs go right after the previous paragraph of the list that was there before
    let mut after: HashMap<ParaId, Vec<Arc<Paragraph>>> = HashMap::new();
    let mut at_start: Vec<Arc<Paragraph>> = Vec::new();
    let mut prev: Option<ParaId> = None;
    for p in flat {
        if present.contains(&p.id) {
            prev = Some(p.id);
            continue;
        }
        match prev {
            Some(q) => after.entry(q).or_default().push(Arc::clone(p)),
            None => at_start.push(Arc::clone(p)),
        }
    }
    let cx = Cx { live, after };
    let mut out = Story::default();
    for p in at_start {
        out.paras.push_arc(p);
    }
    cx.rebuild_into(story, &mut out, ids, 0);
    out
}

struct Cx<'a> {
    live: HashMap<ParaId, &'a Arc<Paragraph>>,
    after: HashMap<ParaId, Vec<Arc<Paragraph>>>,
}

impl Cx<'_> {
    /// The blocks of `s` aligned into `out` (a paragraph still in the list: its node of the list;
    /// the new ones after it; a table: its cells rebuilt).
    fn rebuild_into(&self, s: &Story, out: &mut Story, ids: &mut IdAlloc, depth: u32) {
        for b in s.blocks() {
            match b {
                Block::Paragraph(p) => {
                    if let Some(q) = self.live.get(&p.id) {
                        out.paras.push_arc(Arc::clone(q));
                    }
                    for q in self.after.get(&p.id).map_or(&[][..], Vec::as_slice) {
                        out.paras.push_arc(Arc::clone(q));
                    }
                }
                Block::Table(t) => {
                    let t = if depth < 64 { self.table(t, ids, depth + 1) } else { Arc::new(t.clone()) };
                    out.push_table(t);
                }
            }
        }
    }

    fn table(&self, t: &Table, ids: &mut IdAlloc, depth: u32) -> Arc<Table> {
        let mut nt = t.clone();
        for row in &mut nt.rows {
            for c in &mut row.cells {
                let mut inner = Story::default();
                self.rebuild_into(&c.content, &mut inner, ids, depth);
                if inner.paras.is_empty() {
                    // `inner.push({ type: 'paragraph', paragraph: { runs: [], props: {} } })`
                    inner.push_paragraph(Paragraph::empty(ids.para()));
                }
                c.content = inner;
            }
        }
        Arc::new(nt)
    }
}

fn same_nodes(a: &[Arc<Paragraph>], b: &PVec<Paragraph>) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter_arcs()).all(|(x, y)| Arc::ptr_eq(x, y))
}

fn same_list(a: &[Arc<Paragraph>], b: &[Arc<Paragraph>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Arc::ptr_eq(x, y))
}

fn same_view(a: Option<&Arc<HfFlat>>, b: Option<&Arc<HfFlat>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

/// One story's flat view after a command: the view's edits into `blocks` (`reconcileBlocks`, `Some`
/// when they changed them; a story whose blocks changed outside the view keeps them), then the view
/// rebuilt from the result while `keep` and the story holds a table (the same `Arc` when nothing
/// changed), else `None`.
#[must_use]
pub fn settle_flat(blocks: &Story, view: Option<&Arc<HfFlat>>, keep: bool, ids: &mut IdAlloc) -> (Option<Story>, Option<Arc<HfFlat>>) {
    let mut story: Option<Story> = None;
    if let Some(f) = view
        && same_list(&flatten_arcs(blocks), &f.base)
        && !same_nodes(&f.base, &f.story.paras)
    {
        let flat: Vec<Arc<Paragraph>> = f.story.paras.iter_arcs().cloned().collect();
        story = Some(reconcile(blocks, &flat, ids));
    }
    let b = story.as_ref().unwrap_or(blocks);
    if !keep || b.tables.is_empty() {
        return (story, None);
    }
    let base = flatten_arcs(b);
    let next = match view {
        Some(f) if same_nodes(&base, &f.story.paras) => {
            if same_list(&base, &f.base) {
                Arc::clone(f)
            } else {
                Arc::new(HfFlat { story: f.story.clone(), base })
            }
        }
        _ => Arc::new(HfFlat { story: Story { paras: PVec::from_arcs(base.iter().cloned()), tables: PVec::default() }, base }),
    };
    (story, Some(next))
}

/// The header/footer part and the note a context edits through (also around a box in them).
fn owners(c: &EditContext) -> (Option<HfPartId>, Option<(bool, NumKey)>) {
    match c {
        EditContext::HeaderFooter { part, .. } => (Some(*part), None),
        EditContext::Note { footnote, id, .. } => (None, Some((*footnote, NumKey::new(*id)))),
        EditContext::Textbox { outer, .. } => owners(outer),
        _ => (None, None),
    }
}

/// Whether two textbox contexts edit the same box (the outer story and the path in it).
fn same_box(a: &EditContext, b: &EditContext) -> bool {
    match (a, b) {
        (EditContext::Textbox { outer: o1, at: a1, .. }, EditContext::Textbox { outer: o2, at: a2, .. }) => {
            a1 == a2 && owners(o1) == owners(o2) && std::mem::discriminant(&**o1) == std::mem::discriminant(&**o2)
        }
        _ => false,
    }
}

impl super::super::Session {
    /// Wave 7 (B): the edits of every flat editing view into its story's blocks (`reconcileBlocks`),
    /// then the view of the story in edit rebuilt from them (or dropped: no table, or the story is not
    /// edited any more). The box first (its edits change a paragraph of the outer story), then the
    /// note and the header/footer parts. A story whose blocks changed outside the view (an undo, a
    /// command on the blocks) keeps them: the view is rebuilt from the blocks. Runs at the end of every
    /// command (`change_set`) and where a command indexes a story it just entered or restored.
    pub(crate) fn sync_flat_views(&mut self) {
        self.sync_box_view();
        self.sync_note_view();
        self.sync_hf_flat();
    }

    /// The box of a textbox context: its view settled (`keep`: kept/installed for a box whose TS
    /// `paras` are `flattenBlocks(blocks)`, the `Synced` view); whether it has a view afterwards.
    fn settle_box(&mut self, ctx: &EditContext, keep: bool) -> bool {
        let EditContext::Textbox { outer, at, .. } = ctx else { return false };
        let found = story_of(&self.doc, outer)
            .and_then(|s| loc::run_at(s, at.steps()))
            .and_then(|r| r.ext())
            .and_then(|e| e.textbox.as_deref())
            .map(|tb| (tb.content.clone(), tb.flat.0.clone(), tb.view == TextboxView::Synced));
        let Some((content, view0, synced)) = found else { return false };
        let keep = keep && synced;
        if view0.is_none() && !(keep && !content.tables.is_empty()) {
            return false;
        }
        let (blocks, view) = settle_flat(&content, view0.as_ref(), keep, &mut self.doc.ids);
        let has = view.is_some();
        if blocks.is_none() && same_view(view0.as_ref(), view.as_ref()) {
            return has;
        }
        let tb = story_of_mut(&mut self.doc, outer)
            .and_then(|s| loc::run_at_mut(s, at.steps()))
            .and_then(|r| r.ext.as_deref_mut())
            .and_then(|e| e.textbox.as_deref_mut());
        let Some(tb) = tb else { return false };
        if let Some(b) = blocks {
            tb.content = b;
        }
        tb.flat.0 = view;
        has
    }

    /// The box in edit (`tbEdit`): the view of a box left settled and dropped, the one of the box in
    /// edit kept or installed.
    fn sync_box_view(&mut self) {
        let now = matches!(self.ctx, EditContext::Textbox { .. }).then(|| self.ctx.clone());
        if let Some(old) = self.flat_box.take()
            && !now.as_ref().is_some_and(|c| same_box(c, &old))
        {
            self.settle_box(&old, false);
        }
        if let Some(c) = now
            && self.settle_box(&c, true)
        {
            self.flat_box = Some(c);
        }
    }

    /// The note in edit (`noteEdit`, `syncNoteEdit`): the TS edits `flattenBlocks(blocks)`.
    fn sync_note_view(&mut self) {
        let want = owners(&self.ctx).1;
        let cur = self.doc.notes.flat.0.as_ref().map(|f| ((f.footnote, f.id), Arc::clone(&f.view)));
        let mut keys: Vec<(bool, NumKey)> = cur.iter().map(|c| c.0).collect();
        if let Some(w) = want
            && !keys.contains(&w)
        {
            keys.push(w);
        }
        for key in keys {
            let keep = want == Some(key);
            let view0 = cur.as_ref().filter(|c| c.0 == key).map(|c| Arc::clone(&c.1));
            let map = if key.0 { &self.doc.notes.footnotes } else { &self.doc.notes.endnotes };
            let Some(blocks) = map.get(&key.1) else {
                if view0.is_some() {
                    Arc::make_mut(&mut self.doc.notes).flat.0 = None;
                }
                continue;
            };
            if view0.is_none() && !(keep && !blocks.tables.is_empty()) {
                continue;
            }
            let (nb, nv) = settle_flat(blocks, view0.as_ref(), keep, &mut self.doc.ids);
            if nb.is_none() && same_view(view0.as_ref(), nv.as_ref()) {
                continue;
            }
            let notes = Arc::make_mut(&mut self.doc.notes);
            if let Some(b) = nb {
                let map = if key.0 { &mut notes.footnotes } else { &mut notes.endnotes };
                if let Some(s) = map.get_mut(&key.1) {
                    *s = b;
                }
            }
            if keep {
                notes.flat.0 = nv.map(|view| NoteFlat { footnote: key.0, id: key.1, view });
            } else if notes.flat.0.as_ref().is_some_and(|f| (f.footnote, f.id) == key) {
                notes.flat.0 = None;
            }
        }
    }

    /// Wave 7 (B, HF-T1): the header/footer parts (`hfPartBlocks`): every part with a view, and the
    /// part in edit.
    fn sync_hf_flat(&mut self) {
        let want = owners(&self.ctx).0;
        for i in 0..self.doc.hf.len() {
            let id = HfPartId(u32::try_from(i).unwrap_or(u32::MAX));
            let Some(h) = self.doc.hf.get(i) else { continue };
            let keep = want == Some(id);
            let view0 = h.flat.0.clone();
            if view0.is_none() && !(keep && !h.story.tables.is_empty()) {
                continue;
            }
            let (story, view) = settle_flat(&h.story, view0.as_ref(), keep, &mut self.doc.ids);
            if story.is_none() && same_view(view0.as_ref(), view.as_ref()) {
                continue;
            }
            let Some(h) = self.doc.hf.make_mut(i) else { continue };
            if let Some(s) = story {
                h.story = s;
            }
            h.flat.0 = view;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::table::{Cell, Row};

    fn para(ids: &mut IdAlloc, t: &str) -> Paragraph {
        let mut p = Paragraph::empty(ids.para());
        p.runs.push(crate::model::run::Run { text: crate::text::Utf16::from(t), ..crate::model::run::Run::default() });
        p
    }

    fn cell(ps: Vec<Paragraph>) -> Cell {
        let mut content = Story::default();
        for p in ps {
            content.push_paragraph(p);
        }
        Cell { content, ..Cell::default() }
    }

    fn texts(s: &Story) -> Vec<String> {
        flatten_arcs(s).iter().map(|p| p.runs.iter().map(|r| String::from_utf16_lossy(r.text.units())).collect()).collect()
    }

    /// `reconcileBlocks` on the header of the fidelity harness's hf-cell: a table of two cells, then a paragraph.
    #[test]
    fn reconcile_follows_the_ts_rule() {
        let mut ids = IdAlloc::new();
        let (a, b, c) = (para(&mut ids, "a"), para(&mut ids, "b"), para(&mut ids, ""));
        let t =
            Table { rows: vec![Row { cells: vec![cell(vec![a.clone()]), cell(vec![b.clone()])], ..Row::default() }], ..Table::default() };
        let mut s = Story::default();
        s.push_table(Arc::new(t));
        s.push_paragraph(c.clone());
        let flat0 = flatten_arcs(&s);
        assert_eq!(texts(&s), ["a", "b", ""]);
        // typing in cell 1 (same id) and Enter there (a new paragraph after it): both in cell 1
        let mut a2 = (*flat0[0]).clone();
        a2.runs[0].text = crate::text::Utf16::from("xa");
        let n = para(&mut ids, "n");
        let flat = vec![Arc::new(a2), Arc::new(n), Arc::clone(&flat0[1]), Arc::clone(&flat0[2])];
        let r = reconcile(&s, &flat, &mut ids);
        assert_eq!(texts(&r), ["xa", "n", "b", ""]);
        assert_eq!(r.tables.len(), 1);
        assert_eq!(r.tables.get(0).unwrap().table.rows[0].cells[0].content.paras.len(), 2);
        assert_eq!(r.paras.len(), 1, "the paragraph after the table stays at the top level");
        // cell 2's paragraph merged away: the cell gets an empty paragraph
        let flat = vec![Arc::clone(&flat0[0]), Arc::clone(&flat0[2])];
        let r = reconcile(&s, &flat, &mut ids);
        assert_eq!(texts(&r), ["a", "", ""]);
        // a new paragraph before every old one: at the start of the part
        let z = para(&mut ids, "z");
        let mut flat = vec![Arc::new(z)];
        flat.extend(flat0.iter().cloned());
        let r = reconcile(&s, &flat, &mut ids);
        assert_eq!(texts(&r), ["z", "a", "b", ""]);
        assert_eq!(r.paras.len(), 2);
        assert_eq!(r.tables.get(0).unwrap().before_para, 1);
    }
}
