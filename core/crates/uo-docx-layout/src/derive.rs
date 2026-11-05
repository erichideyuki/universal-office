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
//! `liftFrames` (frames.ts:87) runs here once per call (F7b): over the body (anchors end at the
//! section ends) and over every other story (headers/footers, notes, textbox contents — the TS
//! lifts them in every `layoutStatic` outside a cell, F7b-8). A frame's paragraphs get
//! `FRAME_SKIP`, its synthetic textbox (`frame_run`, an `LTextbox` with `FrameInfo` over the
//! `INNER` copies) is carried by a copy of the anchor paragraph or by `frame_carry`.
//! `indexChains` (chain.ts) fills `LDoc::chains` (F7b); laying a chain out (`chainSlice`) is not
//! ported yet.

use crate::error::{LResult, LayoutError, LimitKind};
use crate::keys::{ChainId, LParaId, RunKey, TbKey};
use crate::ldoc::{Chain, LBlock, LDoc, LPara, LStory, LTextbox, ParaFlags, ParaSrc, RunSrc};
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
    index_chains(&mut b.ldoc)?;
    // `liftFrames`: the body (anchors end at section ends) and every static flow laid out outside a
    // cell — headers/footers, notes, textbox contents (F7b)
    let stops: Vec<f64> = view.sections().iter().map(|s| s.block_end.to_number()).collect();
    lift_frames(&mut b.ldoc, BODY, &stops)?;
    let n_stories = u32_of(b.ldoc.stories.len())?;
    for st in 1..n_stories {
        lift_frames(&mut b.ldoc, st, &[])?;
    }
    if !notes.endnotes.is_empty() {
        b.ldoc.en_sep = crate::engine::notes::endnote_separator(&mut b.ldoc)?;
    }
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
            s.sref = Some(sref);
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

/// `indexChains(full)` (chain.ts:21): the chains of linked textboxes of the body and of each
/// header/footer part (story ids are per part): the first box with content (`story`) and the
/// boxes that link to it (`link.id`), in `seq` order (F7b).
fn index_chains(ldoc: &mut LDoc<'_>) -> LResult<()> {
    let view = ldoc.view;
    let mut scans: Vec<&Story> = vec![view.body()];
    let mut seen = Vec::new();
    for s in view.sections() {
        for rec in [&s.headers, &s.footers] {
            for k in ["default", "first", "even"] {
                let Some(&part) = rec.get(k) else { continue };
                if seen.contains(&part) {
                    continue;
                }
                seen.push(part);
                if let Some(st) = view.hf_story(part) {
                    scans.push(st);
                }
            }
        }
    }
    for st in scans {
        // `forEachTextbox(blocks, …)`: document order, a box's own content after it, group members
        let mut boxes: Vec<&uo_docx::model::run::DrawingTextbox> = Vec::new();
        collect_textboxes(st, &mut boxes, 0);
        let mut heads: Vec<(f64, &uo_docx::model::run::DrawingTextbox)> = Vec::new();
        let mut links: Vec<&uo_docx::model::run::DrawingTextbox> = Vec::new();
        for tb in boxes {
            if tb.link.is_some() {
                links.push(tb);
            } else if let Some(id) = tb.story
                && !heads.iter().any(|(h, _)| crate::engine::js_eq(*h, id.to_number()))
            {
                heads.push((id.to_number(), tb));
            }
        }
        if links.is_empty() {
            continue;
        }
        for (id, head) in heads {
            let mut ms: Vec<&uo_docx::model::run::DrawingTextbox> =
                links.iter().copied().filter(|l| l.link.as_ref().is_some_and(|k| crate::engine::js_eq(k.id.to_number(), id))).collect();
            if ms.is_empty() {
                continue;
            }
            let seq = |t: &uo_docx::model::run::DrawingTextbox| t.link.as_ref().map_or(f64::NAN, |l| l.seq.to_number());
            crate::word::obj::js_sort(&mut ms, |a, b| seq(a) - seq(b));
            let mut members = Vec::with_capacity(ms.len() + 1);
            for tb in std::iter::once(head).chain(ms) {
                if let Some(&k) = ldoc.tb_of_drawing.get(&tb.drawing) {
                    members.push(k);
                }
            }
            let cid = ChainId(u32_of(ldoc.chains.len())?);
            for k in &members {
                if let Some(t) = ldoc.textboxes.get_mut(k.ix()) {
                    t.chain = Some(cid);
                }
            }
            ldoc.chains.push(Chain { members });
        }
    }
    Ok(())
}

/// The textboxes of a story in `forEachTextbox` order (depth-bounded like the derive walk).
fn collect_textboxes<'s>(st: &'s Story, out: &mut Vec<&'s uo_docx::model::run::DrawingTextbox>, depth: u32) {
    if depth > 64 {
        return;
    }
    for b in st.blocks() {
        match b {
            Block::Paragraph(p) => {
                for r in &p.runs {
                    collect_run_textboxes(r, out, depth);
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for c in &row.cells {
                        collect_textboxes(&c.content, out, depth + 1);
                    }
                }
            }
        }
    }
}

fn collect_run_textboxes<'s>(r: &'s crate::model::Run, out: &mut Vec<&'s uo_docx::model::run::DrawingTextbox>, depth: u32) {
    if depth > 64 {
        return;
    }
    let Some(e) = r.ext() else { return };
    if let Some(tb) = e.textbox.as_deref() {
        out.push(tb);
        collect_textboxes(&tb.content, out, depth + 1);
    }
    if let Some(g) = e.group.as_deref() {
        for it in &g.items {
            collect_run_textboxes(&it.run, out, depth + 1);
        }
    }
}

/// The frame of a paragraph object (`frameOf(p)`, frames.ts:33): its own `ppr.framePr` without a
/// drop cap, never for an `INNER` copy.
fn frame_pr_of<'l>(ldoc: &'l LDoc<'_>, p: LParaId) -> Option<&'l crate::model::FramePr> {
    let lp = ldoc.para(p)?;
    if lp.flags.has(ParaFlags::INNER) {
        return None;
    }
    let ppr = match (&lp.ppr, lp.src) {
        (Some(ppr), _) => Some(&**ppr),
        (None, ParaSrc::Model { para, .. }) => para.ppr.as_deref(),
        (None, ParaSrc::Copy { base }) => return frame_pr_of(ldoc, base),
    };
    ppr.and_then(|p| p.frame_pr.as_deref()).filter(|f| f.drop_cap.as_deref().is_none_or(|d| d.is_empty() || d == "none"))
}

/// `liftFrames(blocks, stops)` (frames.ts:87) over a story of the derived document (F7b): the
/// paragraphs of each frame leave the flow (`FRAME_SKIP`) and become a synthetic textbox anchored
/// in a copy of the next non-frame paragraph of the same section (the copy carries the frame run
/// at its end), or carried by the frame's first paragraph (`FRAME_CARRY`) when there is none.
/// `stops`: the block indices after which no anchor is searched (section ends).
fn lift_frames(ldoc: &mut LDoc<'_>, story: u32, stops: &[f64]) -> LResult<()> {
    let Some(st) = ldoc.stories.get(story as usize) else { return Ok(()) };
    let blocks = st.blocks.clone();
    let n = blocks.len();
    let stop = |i: usize| stops.iter().any(|&s| crate::engine::js_eq(s, i as f64));
    let mut out: Option<Vec<LBlock>> = None;
    // `anchorCache`: the anchor copy of each paragraph and the frame runs it carries
    let mut anchors: Vec<(LParaId, Vec<RunKey>, LParaId)> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let Some(LBlock::Paragraph(k)) = blocks.get(i).copied() else {
            i += 1;
            continue;
        };
        let Some(f) = frame_pr_of(ldoc, k).cloned() else {
            i += 1;
            continue;
        };
        let mut group = vec![k];
        let mut j = i + 1;
        while j < n && !stop(j - 1) {
            let nf = match blocks.get(j) {
                Some(LBlock::Paragraph(q)) => frame_pr_of(ldoc, *q),
                _ => None,
            };
            if nf.is_none_or(|nf| *nf != f) {
                break;
            }
            if let Some(LBlock::Paragraph(q)) = blocks.get(j) {
                group.push(*q);
            }
            j += 1;
        }
        for p in &group {
            if let Some(lp) = ldoc.paras.get_mut(p.ix()) {
                lp.flags = lp.flags.with(ParaFlags::FRAME_SKIP, true);
            }
        }
        let run = frame_run(ldoc, &group, &f)?;
        // the anchor: the next paragraph that is not a frame, in the same section
        let mut k2 = j;
        while k2 < n && !stop(k2.wrapping_sub(1)) {
            match blocks.get(k2) {
                Some(LBlock::Paragraph(q)) if frame_pr_of(ldoc, *q).is_some() => k2 += 1,
                _ => break,
            }
        }
        let anchor = if k2 < n && !stop(k2.wrapping_sub(1)) { blocks.get(k2).copied() } else { None };
        if let Some(LBlock::Paragraph(src)) = anchor {
            let o = out.get_or_insert_with(|| blocks.clone());
            let pos = anchors.iter().position(|a| a.0 == src);
            let has = pos.and_then(|p| anchors.get(p)).is_some_and(|a| a.1.contains(&run));
            if !has {
                let (prev_runs, base) = match pos.and_then(|p| anchors.get(p)) {
                    Some(a) => (a.1.clone(), a.2),
                    None => (Vec::new(), src),
                };
                let blp = ldoc.para(base).ok_or(LayoutError::Internal("liftFrames: anchor"))?;
                let mut runs = blp.runs.clone();
                runs.push(run);
                let copy = LPara {
                    src: ParaSrc::Copy { base },
                    ppr: None,
                    props: None,
                    runs,
                    rev_joins: blp.rev_joins.clone(),
                    flags: ParaFlags::default(),
                    frame_carry: None,
                    story: blp.story,
                    path: blp.path.clone(),
                };
                let id = LParaId(u32_of(ldoc.paras.len())?);
                ldoc.paras.push(copy);
                let mut fr = prev_runs;
                fr.push(run);
                match pos {
                    Some(p) => {
                        if let Some(a) = anchors.get_mut(p) {
                            *a = (src, fr, id);
                        }
                    }
                    None => anchors.push((src, fr, id)),
                }
            }
            let para = anchors.iter().find(|a| a.0 == src).map_or(src, |a| a.2);
            if let Some(slot) = o.get_mut(k2) {
                *slot = LBlock::Paragraph(para);
            }
        } else if let Some(lp) = group.first().and_then(|g| ldoc.paras.get_mut(g.ix())) {
            lp.frame_carry = Some(run);
        }
        i = j;
    }
    if let (Some(o), Some(st)) = (out, ldoc.stories.get_mut(story as usize)) {
        st.blocks = o;
    }
    Ok(())
}

/// `frameRun(paras, f)` (frames.ts:46): the synthetic textbox of a frame — the inner copies of its
/// paragraphs (`{ ...p, ppr }` without `framePr`, `INNER`) as its content, the frame's size,
/// anchor and wrap, and its `FRAME_INFO`.
fn frame_run(ldoc: &mut LDoc<'_>, paras: &[LParaId], f: &crate::model::FramePr) -> LResult<RunKey> {
    use crate::model::JsNumber;
    let num = |v: Option<JsNumber>| v.and_then(JsNumber::as_f64);
    let mut inner = Vec::with_capacity(paras.len());
    for &p in paras {
        let lp = ldoc.para(p).ok_or(LayoutError::Internal("frameRun: paragraph"))?;
        let ppr0 = match (&lp.ppr, lp.src) {
            (Some(ppr), _) => Some((**ppr).clone()),
            (None, ParaSrc::Model { para, .. }) => para.ppr.as_deref().cloned(),
            (None, ParaSrc::Copy { .. }) => None,
        };
        let mut ppr = ppr0.unwrap_or_default();
        crate::word::obj::delete(&mut ppr, "framePr");
        let q = LPara {
            src: ParaSrc::Copy { base: p },
            ppr: Some(std::sync::Arc::new(ppr)),
            props: None,
            runs: lp.runs.clone(),
            rev_joins: lp.rev_joins.clone(),
            flags: ParaFlags::INNER,
            frame_carry: None,
            story: lp.story,
            path: lp.path.clone(),
        };
        let id = LParaId(u32_of(ldoc.paras.len())?);
        ldoc.paras.push(q);
        inner.push(LBlock::Paragraph(id));
    }
    let story = u32_of(ldoc.stories.len())?;
    ldoc.stories.push(LStory { blocks: inner, sref: None });
    let truthy = |v: Option<f64>| v.is_some_and(uo_jsnum::conv::truthy);
    let h_rule: &str = match f.h_rule.as_deref() {
        Some(r) => r,
        None if truthy(num(f.h)) => "atLeast",
        None => "auto",
    };
    let wrap = match f.wrap.as_deref() {
        Some("none") => "none",
        Some("notBeside") => "topAndBottom",
        _ => "square",
    };
    let h_from = match f.h_anchor.as_deref() {
        Some("page") => "page",
        Some("margin") => "margin",
        _ => "column",
    };
    let v_from = match f.v_anchor.as_deref() {
        Some("page") => "page",
        Some("text") => "paragraph",
        _ => "margin",
    };
    let hs = num(f.h_space).unwrap_or(0.0);
    let vs = num(f.v_space).unwrap_or(0.0);
    let mut anchor = crate::model::DrawingAnchor {
        wrap: wrap.into(),
        wrap_text: Some("bothSides".into()),
        // (the gap left of the frame measures 1 twip less — ws-a-fr-l18, ws-fr-l30)
        dist_l: Some(JsNumber::Num(hs + 1.0)),
        dist_r: Some(JsNumber::Num(hs)),
        dist_t: Some(JsNumber::Num(vs)),
        dist_b: Some(JsNumber::Num(vs)),
        layout_in_cell: Some(true),
        allow_overlap: Some(true),
        ..crate::model::DrawingAnchor::default()
    };
    anchor.pos_h.relative_from = h_from.into();
    match f.x_align.as_deref().filter(|a| !a.is_empty()) {
        Some(a) => anchor.pos_h.align = Some(a.into()),
        None => anchor.pos_h.offset = Some(JsNumber::Num(num(f.x).unwrap_or(0.0))),
    }
    anchor.pos_v.relative_from = v_from.into();
    match f.y_align.as_deref().filter(|a| !a.is_empty() && *a != "inline") {
        Some(a) => anchor.pos_v.align = Some(a.into()),
        None => anchor.pos_v.offset = Some(JsNumber::Num(num(f.y).unwrap_or(0.0))),
    }
    let n_tb = u32_of(ldoc.textboxes.len())?;
    let drawing = crate::model::DrawingId(u32::MAX - n_tb);
    let tb = crate::model::DrawingTextbox {
        width_twips: JsNumber::Num(num(f.w).unwrap_or(0.0)),
        height_twips: JsNumber::Num(num(f.h).unwrap_or(0.0)),
        floating: Some(true),
        insets: Some(crate::model::Ltrb {
            l: JsNumber::Num(0.0),
            t: JsNumber::Num(0.0),
            r: JsNumber::Num(0.0),
            b: JsNumber::Num(0.0),
            ..crate::model::Ltrb::default()
        }),
        anchor: Some(Box::new(anchor)),
        drawing,
        ..crate::model::DrawingTextbox::default()
    };
    let fh = num(f.h);
    let info = crate::ldoc::FrameInfo {
        auto_width: !truthy(num(f.w)),
        h_rule: if h_rule == "exact" && truthy(fh) {
            crate::ldoc::FrameHRule::Exact
        } else if h_rule == "auto" || !truthy(fh) {
            crate::ldoc::FrameHRule::Auto
        } else {
            crate::ldoc::FrameHRule::AtLeast
        },
        h: fh.unwrap_or(0.0),
    };
    let mut run = crate::model::Run {
        text: crate::model::Utf16::from_units(vec![0xFFFC]),
        props: std::sync::Arc::new(uo_docx::read::plain::default_run_props()),
        ..crate::model::Run::default()
    };
    run.ext_mut().textbox = Some(Box::new(tb));
    let key = RunKey(u32_of(ldoc.runs.len())?);
    ldoc.runs.push(RunSrc::Synth(Box::new(run)));
    let tk = TbKey(n_tb);
    ldoc.textboxes.push(LTextbox { drawing: None, story, frame: Some(info), chain: None });
    ldoc.tb_of_drawing.insert(drawing, tk);
    Ok(key)
}
