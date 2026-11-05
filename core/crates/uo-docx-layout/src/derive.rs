//! Building the derived document of a call (LA2, frontier F7a): every model paragraph the passes
//! touch as a `ParaSrc::Model` with its runs (and their field results, F4-12) keyed once; the
//! stories the engine walks — the body (`stories[0]`), each header/footer part, each note, each
//! textbox content — as block lists; the revision author order (`revisionAuthors(full)`).
//!
//! What the TS synthesizes **while** laying out (`joinDeleted`, `joinHidden`, the line-number
//! paragraph, `sliceParagraphFrom`) is not built here: the engine makes those copies when it meets
//! them (`engine::flow`, a relocated scratch `LDoc`, decision F7a-2), because they depend on the
//! pass (a hidden paragraph mark is known only after preparing the paragraph).
//!
//! `liftFrames` (frames.ts:87) and `indexChains` (chain.ts) are F7b's with the float placement
//! (record "Stage 3 — F7a", handed on): a framed paragraph (`w:framePr` without drop cap) is
//! reported by [`frame_of`] so the engine can stop with `Unported(F7b)` instead of laying it out
//! in the flow.

use crate::error::{LResult, LayoutError, LimitKind};
use crate::keys::{LParaId, RunKey, TbKey};
use crate::ldoc::{LBlock, LDoc, LPara, LStory, LTextbox, ParaFlags, ParaSrc, RunSrc};
use crate::model::{BlockPath, DocView, Paragraph, RunRef, RunSub, StoryRef};
use crate::options::LayoutOptions;
use uo_docx::model::story::{Block, Story};

/// Index of the body story in `LDoc::stories`.
pub const BODY: u32 = 0;

/// The derived document of `view` for one call.
pub fn build<'a>(view: DocView<'a>, opts: &LayoutOptions) -> LResult<LDoc<'a>> {
    let mut b = Builder { ldoc: LDoc::empty(view), max_depth: opts.limits.max_depth };
    let body = b.story(view.body(), StoryRef::Body, 0)?;
    debug_assert_eq!(body, BODY);
    let mut seen = Vec::new();
    for s in view.sections() {
        for rec in [&s.headers, &s.footers] {
            for (_, &part) in rec.iter() {
                if seen.contains(&part) {
                    continue;
                }
                seen.push(part);
                if let Some(st) = view.hf_story(part) {
                    let ix = b.story(st, StoryRef::HeaderFooter(part), 0)?;
                    b.ldoc.hf_stories.insert(part, ix);
                }
            }
        }
    }
    let notes = view.notes();
    for (kind, set) in [(crate::model::NoteKind::Footnote, &notes.footnotes), (crate::model::NoteKind::Endnote, &notes.endnotes)] {
        for (id, st) in set {
            let ix = b.story(st, StoryRef::Note(kind, *id), 0)?;
            b.ldoc.note_stories.insert((kind, *id), ix);
        }
    }
    b.ldoc.revision_order = view.revision_authors();
    Ok(b.ldoc)
}

/// `frameOf(p)` (frames.ts:33): the frame of a paragraph that `liftFrames` takes out of the flow
/// (`w:framePr` without a drop cap; `INNER` copies never).
#[must_use]
pub fn frame_of(ldoc: &LDoc<'_>, p: LParaId) -> bool {
    let Some(lp) = ldoc.para(p) else { return false };
    if lp.flags.has(ParaFlags::INNER) {
        return false;
    }
    let ppr = match (&lp.ppr, lp.src) {
        (Some(ppr), _) => Some(&**ppr),
        (None, ParaSrc::Model { para, .. }) => para.ppr.as_deref(),
        (None, ParaSrc::Copy { base }) => return frame_of(ldoc, base),
    };
    ppr.and_then(|p| p.frame_pr.as_ref()).is_some_and(|f| f.drop_cap.as_deref().is_none_or(|d| d == "none"))
}

struct Builder<'a> {
    ldoc: LDoc<'a>,
    max_depth: u32,
}

fn u32_of(n: usize) -> LResult<u32> {
    u32::try_from(n).map_err(|_| LayoutError::Limit(LimitKind::Capacity))
}

impl<'a> Builder<'a> {
    /// A story: its blocks (tables by index) and every paragraph inside, cells and textboxes
    /// included; returns the story index.
    fn story(&mut self, st: &'a Story, sref: StoryRef, depth: u32) -> LResult<u32> {
        let ix = u32_of(self.ldoc.stories.len())?;
        self.ldoc.stories.push(LStory::default());
        let mut blocks = Vec::new();
        for (i, blk) in st.blocks().enumerate() {
            let i = u32_of(i)?;
            let path = BlockPath::root(i);
            match blk {
                Block::Paragraph(p) => blocks.push(LBlock::Paragraph(self.para(p, sref, path, depth)?)),
                Block::Table(t) => {
                    self.table(t, sref, &path, depth)?;
                    blocks.push(LBlock::Table { block: i });
                }
            }
        }
        if let Some(s) = self.ldoc.stories.get_mut(ix as usize) {
            s.blocks = blocks;
        }
        Ok(ix)
    }

    fn table(&mut self, t: &'a uo_docx::model::Table, sref: StoryRef, path: &BlockPath, depth: u32) -> LResult<()> {
        if depth >= self.max_depth {
            return Err(LayoutError::Limit(LimitKind::Depth));
        }
        for (ri, row) in t.rows.iter().enumerate() {
            for (ci, c) in row.cells.iter().enumerate() {
                let (ri, ci) = (u32_of(ri)?, u32_of(ci)?);
                for (bi, blk) in c.content.blocks().enumerate() {
                    let p2 = path.cell_block(ri, ci, u32_of(bi)?);
                    match blk {
                        Block::Paragraph(p) => {
                            self.para(p, sref, p2, depth + 1)?;
                        }
                        Block::Table(t2) => self.table(t2, sref, &p2, depth + 1)?,
                    }
                }
            }
        }
        Ok(())
    }

    fn para(&mut self, p: &'a Paragraph, sref: StoryRef, path: BlockPath, depth: u32) -> LResult<LParaId> {
        let mut runs = Vec::with_capacity(p.runs.len());
        for (i, r) in p.runs.iter().enumerate() {
            let i = u32_of(i)?;
            let key = RunKey(u32_of(self.ldoc.runs.len())?);
            self.ldoc.runs.push(RunSrc::Model { run: r, at: RunRef { story: sref, path: path.clone(), run: i, sub: RunSub::None } });
            runs.push(key);
            if let Some(fr) = r.ext().and_then(|e| e.field_result.as_deref()) {
                for (j, x) in fr.iter().enumerate() {
                    let j = u32_of(j)?;
                    let k2 = RunKey(u32_of(self.ldoc.runs.len())?);
                    self.ldoc.runs.push(RunSrc::Model {
                        run: x,
                        at: RunRef { story: sref, path: path.clone(), run: i, sub: RunSub::FieldResult(j) },
                    });
                    self.ldoc.field_results.insert((key, j), k2);
                }
            }
            if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref()) {
                self.textbox(tb, depth)?;
            }
            if let Some(g) = r.ext().and_then(|e| e.group.as_deref()) {
                for it in &g.items {
                    if let Some(tb) = it.run.ext().and_then(|e| e.textbox.as_deref()) {
                        self.textbox(tb, depth)?;
                    }
                }
            }
        }
        let id = LParaId(u32_of(self.ldoc.paras.len())?);
        self.ldoc.paras.push(LPara {
            src: ParaSrc::Model { para: p, id: p.id },
            ppr: None,
            props: None,
            runs,
            rev_joins: Vec::new(),
            flags: ParaFlags::default(),
            frame_carry: None,
            story: sref,
            path,
        });
        self.ldoc.by_para.insert(p.id, id);
        Ok(id)
    }

    fn textbox(&mut self, tb: &'a uo_docx::model::run::DrawingTextbox, depth: u32) -> LResult<()> {
        if depth >= self.max_depth {
            return Err(LayoutError::Limit(LimitKind::Depth));
        }
        let story = self.story(&tb.content, StoryRef::Textbox(tb.drawing), depth + 1)?;
        let k = TbKey(u32_of(self.ldoc.textboxes.len())?);
        self.ldoc.textboxes.push(LTextbox { drawing: Some(tb.drawing), story, frame: None, chain: None });
        self.ldoc.tb_of_drawing.insert(tb.drawing, k);
        Ok(())
    }
}
