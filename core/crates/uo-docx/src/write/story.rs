//! The lossless core of `serialize.ts`: what goes out raw and what is regenerated.
//!
//! - **stories** (`groupsXml`, `storyBlocksXml`, `storyPristine`, `countCopies`, `blocksXml`):
//!   each top-level child of a part whose blocks are all present, in order and pristine is
//!   written as its raw source; children without blocks (marks between paragraphs) keep their
//!   relative place; a changed `w:sdt`/`w:customXml` is rewrapped around its blocks;
//! - **paragraphs** (`paraXml`, `paraXmlRuns`, `runsLossless`, `paraSectPr`): a pristine
//!   paragraph (one copy) is its raw `w:p`; otherwise the original start tag (first copy), the
//!   patched `w:pPr` and the runs, each top-level segment of the source raw when its runs are
//!   intact, generated otherwise;
//! - **tables** (`tableXml`, `rowsXml`, `wrappedXml`): a pristine table is raw; otherwise each
//!   part (table, row, cell — by id, not by index) is its raw source with the typed props patched;
//! - **objects** (`objectXml`, `txbxInner`): a drawing object goes out as its source element with
//!   the current geometry and the text of its boxes; a rebuilt shape or group through the object
//!   emitters;
//! - **notes parts** (`writeNotesPart`, `noteXml`).
//!
//! The string emitters (generated runs, fields, objects, table props) are the D1b emitter
//! frontiers' ([`super::emit`]). Walks across stories recurse through tables, cells and
//! textboxes, bounded by the reader's nesting limit (G20, F17).

use std::collections::{HashMap, HashSet};

use super::ctx::{Lossless, Writer};
use super::emit;
use super::js::num_str;
use super::lossless::{HfRefXml, PPrOut, SectArg, WResult, patch_ppr, patch_rpr, patch_sect_pr, ppr_from_props, rpr_from_props, split};
use super::matchers::{count_txbx_contents, grid_of, replace_txbx_contents};
use super::sig::block_src;
use crate::error::DocxError;
use crate::ids::SourceKey;
use crate::model::para::{BlockSdt, Paragraph, SectionBreak};
use crate::model::run::{DrawingTextbox, Run, TextboxView};
use crate::model::sources::{ParaSource, SegSource, StoryGroup, StorySource, WrapSeg};
use crate::model::story::{Block, Story};
use crate::model::table::{Cell, Table};

/// `isObjectRun(r)`.
#[must_use]
pub fn is_object_run(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some())
}

/// `isAtomic(r)`: objects without a generator of their own.
#[must_use]
pub fn is_atomic(r: &Run) -> bool {
    let e = r.ext();
    e.is_some_and(|e| e.shape.is_some() || e.group.is_some() || e.math.is_some() || e.form_field.is_some())
        || r.props.is_math == Some(true)
        || e.and_then(|e| e.image.as_deref()).is_some_and(|i| i.raw_drawing_xml.is_none())
}

/// The block-content-control mark of a block (`sdtOf`).
fn sdt_of<'a>(b: Option<&Block<'a>>) -> Option<&'a BlockSdt> {
    match b {
        Some(Block::Paragraph(p)) => p.props.block_sdt.as_deref(),
        _ => None,
    }
}

/// `sdtRunEnd(blocks, from, key)`.
fn sdt_run_end(blocks: &[Block<'_>], from: usize, key: &str) -> usize {
    let mut j = from;
    while j < blocks.len() && sdt_of(blocks.get(j)).is_some_and(|s| s.key.0.as_ref() == key) {
        j += 1;
    }
    j
}

impl<'a> Writer<'a> {
    fn lossless_mut(&mut self) -> WResult<&mut Lossless<'a>> {
        self.lossless.as_mut().ok_or(DocxError::Write("lossless state missing"))
    }

    // ───────────────────────────── blocks ─────────────────────────────

    /// `blockXml(b)`.
    pub fn block_xml(&mut self, b: Block<'a>) -> WResult<String> {
        match b {
            Block::Table(t) => self.table_xml(t),
            Block::Paragraph(p) => self.para_xml(p),
        }
    }

    /// `newSdtXml(pr, inner)`.
    fn new_sdt_xml(pr: &str, inner: &str) -> String {
        format!("<w:sdt>{pr}<w:sdtContent>{inner}</w:sdtContent></w:sdt>")
    }

    /// `blocksXml(blocks)`: blocks without sources, the new block controls wrapped.
    pub fn blocks_xml(&mut self, blocks: &[Block<'a>]) -> WResult<String> {
        let mut out = String::new();
        let mut i = 0;
        while i < blocks.len() {
            let s = sdt_of(blocks.get(i));
            if let Some(s) = s
                && let Some(pr) = s.pr.as_deref().filter(|p| !p.is_empty())
            {
                let j = sdt_run_end(blocks, i, &s.key.0);
                let inner = self.blocks_inner(&blocks[i..j])?;
                out.push_str(&Self::new_sdt_xml(pr, &inner));
                i = j;
            } else {
                out.push_str(&self.block_xml(blocks[i])?);
                i += 1;
            }
        }
        Ok(out)
    }

    /// `blocks.map(blockXml).join('')`.
    pub fn blocks_inner(&mut self, blocks: &[Block<'a>]) -> WResult<String> {
        let mut out = String::new();
        for &b in blocks {
            out.push_str(&self.block_xml(b)?);
        }
        Ok(out)
    }

    /// `storyBlocksXml(story, blocks, store)`: the copies counted, then the groups.
    pub fn story_blocks_xml(&mut self, story: &'a StorySource, blocks: &[Block<'a>]) -> WResult<String> {
        let mut remaining = std::mem::take(&mut self.lossless_mut()?.remaining);
        count_copies(blocks, &mut remaining);
        self.lossless_mut()?.remaining = remaining;
        self.groups_xml(&story.groups, blocks)
    }

    /// `storyPristine(story, blocks, store)`: exactly the blocks read, all as in the baseline.
    pub fn story_pristine(&mut self, story: &'a StorySource, blocks: &[Block<'a>]) -> WResult<bool> {
        let ids: Vec<f64> = story.groups.iter().flat_map(|g| g.blocks.iter().map(|b| b.to_number())).collect();
        if ids.len() != blocks.len() {
            return Ok(false);
        }
        for (i, &b) in blocks.iter().enumerate() {
            if block_src(b).map(|k| f64::from(k.0)) != ids.get(i).copied() {
                return Ok(false);
            }
            if !self.block_pristine(b)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn block_pristine(&mut self, b: Block<'a>) -> WResult<bool> {
        let l = self.lossless_mut()?;
        Ok(match b {
            Block::Table(t) => l.pristine.table(t),
            Block::Paragraph(p) => l.pristine.para(p),
        })
    }

    /// `groupsXml(groups, blocks, store)`.
    pub fn groups_xml(&mut self, groups: &'a [StoryGroup], blocks: &[Block<'a>]) -> WResult<String> {
        // a group's block ids (0 = a block without source, never looked up)
        let key = |x: f64| -> Option<SourceKey> {
            (x > 0.0 && x <= f64::from(u32::MAX) && x.fract() == 0.0).then(|| SourceKey(uo_jsnum::conv::to_uint32(x)))
        };
        let mut group_of: HashMap<SourceKey, usize> = HashMap::new();
        for (gi, g) in groups.iter().enumerate() {
            for id in &g.blocks {
                if let Some(k) = key(id.to_number()) {
                    group_of.insert(k, gi);
                }
            }
        }
        let mut seen: HashSet<SourceKey> = HashSet::new();
        let mut dup: HashSet<SourceKey> = HashSet::new();
        for &b in blocks {
            if let Some(id) = block_src(b)
                && !seen.insert(id)
            {
                dup.insert(id);
            }
        }
        let gkey = |g: &StoryGroup, k: usize| g.blocks.get(k).and_then(|x| key(x.to_number()));
        // the size of the raw part (what an unchanged story writes) and room for the part's root
        // and section, which the caller adds in place (no regrowth, no second buffer)
        let mut out = String::with_capacity(groups.iter().map(|g| self.src_text(&g.raw).len()).sum::<usize>() + 4096);
        let mut zp = 0usize;
        let mut i = 0usize;
        let flush_zero = |w: &mut Self, out: &mut String, zp: &mut usize, up_to: usize| {
            while *zp < up_to {
                if let Some(g) = groups.get(*zp)
                    && g.blocks.is_empty()
                {
                    out.push_str(w.src_text(&g.raw));
                }
                *zp += 1;
            }
        };
        while i < blocks.len() {
            let b = blocks[i];
            let id = block_src(b);
            let gi = id.and_then(|k| group_of.get(&k).copied());
            if let Some(gi) = gi
                && gi >= zp
            {
                let g = &groups[gi];
                let n = g.blocks.len();
                let mut ok = gkey(g, 0) == id && i + n <= blocks.len();
                let mut k = 0;
                while ok && k < n {
                    let bk = blocks[i + k];
                    let bid = block_src(bk);
                    ok = bid == gkey(g, k) && bid.is_some_and(|x| !dup.contains(&x)) && self.block_pristine(bk)?;
                    k += 1;
                }
                // a new block of the same control right after (Enter in the last paragraph)
                let trailing = |k: usize| -> bool {
                    g.sdt_key.as_ref().is_some_and(|sk| {
                        blocks.get(k).is_some_and(|&bk| block_src(bk).is_none()) && sdt_of(blocks.get(k)).is_some_and(|s| s.key.0 == sk.0)
                    })
                };
                if ok && trailing(i + n) {
                    ok = false;
                }
                flush_zero(self, &mut out, &mut zp, gi);
                if ok {
                    let l = self.lossless_mut()?;
                    for k in 0..n {
                        if let Some(bid) = gkey(g, k) {
                            l.claimed.insert(bid);
                        }
                    }
                    out.push_str(self.src_text(&g.raw));
                    zp = gi + 1;
                    i += n;
                    continue;
                }
                if let Some(wrap_open) = g.wrap_open.as_ref()
                    && gkey(g, 0) == id
                {
                    let in_group = |k: usize| blocks.get(k).and_then(|&b| block_src(b)).is_some_and(|x| group_of.get(&x) == Some(&gi));
                    let mut j = i;
                    while j < blocks.len() {
                        if in_group(j) {
                            j += 1;
                            continue;
                        }
                        // new blocks between blocks of the group stay inside the wrapper
                        let mut k = j;
                        while k < blocks.len() && block_src(blocks[k]).is_none() {
                            k += 1;
                        }
                        if k > j && k < blocks.len() && in_group(k) {
                            j = k;
                            continue;
                        }
                        if trailing(j) {
                            j += 1;
                            continue;
                        }
                        break;
                    }
                    let inner_blocks: Vec<Block<'a>> = blocks[i..j].to_vec();
                    let inner = match g.inner.as_deref() {
                        Some(inner) => self.groups_xml(inner, &inner_blocks)?,
                        None => self.blocks_inner(&inner_blocks)?,
                    };
                    let close = self.src_text(g.wrap_close.as_ref().unwrap_or(&crate::model::sources::SrcText::default())).to_owned();
                    let open = self.src_text(wrap_open).to_owned();
                    out.push_str(&open);
                    out.push_str(&inner);
                    out.push_str(&close);
                    zp = gi + 1;
                    i = j;
                    continue;
                }
            }
            if let Some(s) = sdt_of(Some(&b))
                && let Some(pr) = s.pr.as_deref().filter(|p| !p.is_empty())
            {
                let j = sdt_run_end(blocks, i + 1, &s.key.0);
                let inner = self.blocks_inner(&blocks[i..j])?;
                out.push_str(&Self::new_sdt_xml(pr, &inner));
                i = j;
                continue;
            }
            out.push_str(&self.block_xml(b)?);
            i += 1;
        }
        flush_zero(self, &mut out, &mut zp, groups.len());
        Ok(out)
    }

    // ───────────────────────────── paragraphs ─────────────────────────────

    /// `paraXml(para)`: with `paraRuns` set to the paragraph's runs for its duration.
    pub fn para_xml(&mut self, para: &'a Paragraph) -> WResult<String> {
        if !self.active() {
            return self.para_xml_runs(para);
        }
        let prev = self.lossless.as_mut().and_then(|l| l.para_runs.replace(&para.runs));
        let r = self.para_xml_runs(para);
        if let Some(l) = self.lossless.as_mut() {
            l.para_runs = prev;
        }
        r
    }

    fn para_xml_runs(&mut self, para: &'a Paragraph) -> WResult<String> {
        let src: Option<(SourceKey, &'a ParaSource)> =
            self.store().and_then(|s| para.props.src.and_then(|k| s.paras.get(&k).map(|ps| (k, ps))));
        let Some((id, src)) = src else { return self.para_xml_generated(para) };
        let l = self.lossless_mut()?;
        let first = l.claimed.insert(id);
        let left = l.remaining.get(&id).copied().unwrap_or(1) - 1;
        l.remaining.insert(id, left);
        let last = left <= 0;
        if first && last && l.pristine.para(para) {
            return Ok(self.src_text(&src.raw).to_owned());
        }
        let ppr_raw = src.p_pr.as_ref().map(|t| self.src_text(t));
        let sect = self.para_sect_pr(para, ppr_raw)?;
        let ids = self.lossless_mut()?.num_ids;
        let ppr = patch_ppr(&mut self.cx, ppr_raw, para, &sect, ids)?;
        if ppr.gen_numbering {
            self.lossless_mut()?.gen_numbering = true;
        }
        let open = if first { self.src_text(&src.open).to_owned() } else { "<w:p>".to_owned() };
        let runs = self.runs_lossless(para, src, id, last)?;
        Ok(format!("{open}{}{runs}</w:p>", ppr.xml))
    }

    /// The paragraph without a source: generated (`paraXmlRuns`, first branch).
    fn para_xml_generated(&mut self, para: &'a Paragraph) -> WResult<String> {
        let skip = self.store().map(|s| &s.comment_ids);
        let (before, after) = emit::run::comment_marks(&para.runs, skip);
        let items: Vec<emit::run::GenItem<'a>> =
            para.runs.iter().zip(before).zip(after).map(|((run, before), after)| emit::run::GenItem { run, before, after }).collect();
        if !self.active() {
            let runs = emit::run::revision_runs(self, &items)?;
            return emit::run::para_xml_inner(self, &para.props, &runs);
        }
        // formatting copied from a read paragraph: its raw `w:pPr` with the differences
        let fmt = self.store().and_then(|s| para.props.fmt_src.and_then(|k| s.paras.get(&k)));
        let ids = self.lossless_mut()?.num_ids;
        let ppr: PPrOut = if let Some(fmt) = fmt {
            let raw = fmt.p_pr.as_ref().map(|t| self.src_text(t));
            let sect = self.para_sect_pr(para, raw)?;
            patch_ppr(&mut self.cx, raw, para, &sect, ids)?
        } else {
            let sect = match para.props.section_break.as_deref() {
                Some(sb) => self.section_break_xml(sb, None)?,
                None => String::new(),
            };
            ppr_from_props(&mut self.cx, &para.props, &sect, ids)?
        };
        if ppr.gen_numbering {
            self.lossless_mut()?.gen_numbering = true;
        }
        let runs = emit::run::gen_runs(self, &items, false)?;
        Ok(format!("<w:p>{}{runs}</w:p>", ppr.xml))
    }

    /// `paraSectPr(para, rawPPr)`: the `w:sectPr` the paragraph carries.
    fn para_sect_pr(&mut self, para: &Paragraph, raw_ppr: Option<&str>) -> WResult<SectArg> {
        let raw_sect = match raw_ppr {
            Some(r) if !r.is_empty() => split(r, &self.cx.limits)?.kids.into_iter().find(|k| k.name == "w:sectPr").map(|k| k.raw),
            _ => None,
        };
        let Some(sb) = para.props.section_break.as_deref() else {
            return Ok(if raw_sect.is_some() { SectArg::Remove } else { SectArg::Keep });
        };
        Ok(SectArg::Set(self.section_break_xml(sb, raw_sect.as_deref())?))
    }

    /// `sectionBreakXml(sb, rawSect)`.
    pub fn section_break_xml(&mut self, sb: &SectionBreak, raw_sect: Option<&str>) -> WResult<String> {
        let refs: Vec<HfRefXml> = sb
            .header_refs
            .iter()
            .chain(sb.footer_refs.iter())
            .map(|r| HfRefXml { kind: r.kind.clone(), r#type: r.r#type.clone(), rid: r.rid.clone() })
            .collect();
        let base = raw_sect.or(sb.raw.as_deref()).filter(|b| !b.is_empty());
        match base {
            Some(b) => patch_sect_pr(&mut self.cx, b, &sb.sect_pr, Some(&refs)),
            None => Ok(format!("<w:sectPr>{}</w:sectPr>", emit::run::sect_pr_inner(&self.cx, &sb.sect_pr, &refs)?)),
        }
    }

    /// `rPrXml(run)`: the raw source `w:rPr` patched, or generated from the props.
    pub fn rpr_xml(&mut self, run: &Run) -> WResult<String> {
        let src = self.store().and_then(|s| run.props.src.and_then(|k| s.runs.get(&k)));
        match src {
            Some(rs) => {
                let raw = rs.r_pr.as_ref().map(|t| self.src_text(t));
                patch_rpr(&mut self.cx, raw, run)
            }
            None => {
                let active = self.active();
                rpr_from_props(&mut self.cx, &run.props, active)
            }
        }
    }

    /// `runsLossless(para, src, pid, last)`.
    fn runs_lossless(&mut self, para: &'a Paragraph, src: &'a ParaSource, pid: SourceKey, last: bool) -> WResult<String> {
        let store = self.store().ok_or(DocxError::Write("lossless state missing"))?;
        let segs: &'a [SegSource] = &src.segs;
        let cur: &'a [Run] = &para.runs;
        let sid_of =
            |x: f64| -> Option<u32> { (x > 0.0 && x <= f64::from(u32::MAX) && x.fract() == 0.0).then(|| uo_jsnum::conv::to_uint32(x)) };
        let mut where_: HashMap<u32, (usize, usize)> = HashMap::new();
        for (gi, g) in segs.iter().enumerate() {
            for (k, sid) in g.sids.iter().enumerate() {
                if let Some(s) = sid_of(sid.to_number()) {
                    where_.insert(s, (gi, k));
                }
            }
        }
        // the ORIGINAL run of each id: the first equal to the baseline
        let mode = self.cx.mode;
        let media = Some(&self.doc.media);
        let base: HashMap<u32, u64> = match self.lossless.as_ref().and_then(|l| l.pristine.baseline().paras.get(&pid)) {
            Some(sig) => sig.sids.iter().copied().zip(sig.runs.iter().copied()).collect(),
            None => HashMap::new(),
        };
        let mut orig: HashMap<u32, usize> = HashMap::new();
        for (i, r) in cur.iter().enumerate() {
            if let Some(k) = r.props.src
                && !orig.contains_key(&k.0)
                && base.get(&k.0) == Some(&super::sig::run_sig(r, media, mode))
            {
                orig.insert(k.0, i);
            }
        }
        let intact = |i: usize| cur.get(i).and_then(|r| r.props.src).is_some_and(|k| orig.get(&k.0) == Some(&i));
        let (before, after) = emit::run::comment_marks(cur, Some(&store.comment_ids));
        let mut first_atomic: HashMap<u32, usize> = HashMap::new();
        for (i, r) in cur.iter().enumerate() {
            if let Some(k) = r.props.src
                && is_atomic(r)
            {
                first_atomic.entry(k.0).or_insert(i);
            }
        }
        let mut out = String::new();
        let mut pending: Vec<emit::run::GenItem<'a>> = Vec::new();
        let mut zp = 0usize;
        let taken = |w: &Self, gi: usize| w.lossless.as_ref().is_some_and(|l| l.claimed_segs.contains(&(pid, gi)));
        let take = |w: &mut Self, gi: usize| {
            if let Some(l) = w.lossless.as_mut() {
                l.claimed_segs.insert((pid, gi));
            }
        };
        macro_rules! flush_gen {
            () => {
                if !pending.is_empty() {
                    let items = std::mem::take(&mut pending);
                    out.push_str(&emit::run::gen_runs(self, &items, false)?);
                }
            };
        }
        macro_rules! flush_zero {
            ($up_to:expr) => {
                let up_to = $up_to;
                while zp < up_to {
                    if let Some(g) = segs.get(zp)
                        && g.sids.is_empty()
                        && !taken(self, zp)
                    {
                        take(self, zp);
                        flush_gen!();
                        out.push_str(self.src_text(&g.raw));
                    }
                    zp += 1;
                }
            };
        }
        let mut i = 0usize;
        while i < cur.len() {
            let r = &cur[i];
            let loc = r.props.src.and_then(|k| where_.get(&k.0).copied());
            if let Some((g0, _)) = loc
                && segs[g0].open == Some(true)
            {
                // a field that closes in a later paragraph (a TOC): its start goes out raw, once
                if !taken(self, g0) {
                    flush_zero!(g0);
                    flush_gen!();
                    out.push_str(&before[i]);
                    out.push_str(self.src_text(&segs[g0].raw));
                    take(self, g0);
                    zp = zp.max(g0 + 1);
                }
                i += 1;
                continue;
            }
            if let Some((g0, k0)) = loc
                && g0 >= zp
                && !taken(self, g0)
            {
                let g = &segs[g0];
                let n = g.sids.len();
                let mut ok = k0 == 0 && i + n <= cur.len();
                let mut k = 0;
                while ok && k < n {
                    ok = cur[i + k].props.src.map(|s| f64::from(s.0)) == g.sids.get(k).map(|x| x.to_number()) && intact(i + k);
                    k += 1;
                }
                // an object without a generator: the raw, even changed (an object with its stored
                // source XML is rebuilt by objectXml, with the current text)
                let src_k = r.props.src.map(|s| s.0);
                let fallback = !ok
                    && n == 1
                    && is_atomic(r)
                    && src_k.and_then(|s| first_atomic.get(&s)) == Some(&i)
                    && !(is_object_run(r) && src_k.and_then(|s| store.runs.get(&SourceKey(s))).is_some_and(|rs| rs.obj.is_some()));
                if ok || fallback {
                    flush_zero!(g0);
                    flush_gen!();
                    out.push_str(&before[i]);
                    out.push_str(self.src_text(&g.raw));
                    out.push_str(&after[i + n - 1]);
                    // markObjects(g)
                    if let Some(l) = self.lossless.as_mut() {
                        for sid in &g.sids {
                            if let Some(s) = sid_of(sid.to_number())
                                && store.runs.get(&SourceKey(s)).is_some_and(|rs| rs.obj.is_some())
                            {
                                l.emitted_obj.insert(SourceKey(s));
                            }
                        }
                    }
                    take(self, g0);
                    zp = g0 + 1;
                    i += n;
                    continue;
                }
                flush_zero!(g0);
                let same_wrap = |x: &Run| -> bool {
                    match g.link.as_deref() {
                        None => true,
                        Some(link) => {
                            x.props.link.as_deref() == Some(link)
                                && x.props.link_tip.as_deref().unwrap_or("") == g.tip.as_deref().unwrap_or("")
                        }
                    }
                };
                if let Some(wrap_open) = g.wrap_open.as_ref()
                    && same_wrap(r)
                {
                    let own: HashSet<u32> = g.sids.iter().filter_map(|x| sid_of(x.to_number())).collect();
                    let mut j = i;
                    while j < cur.len() && cur[j].props.src.is_some_and(|s| own.contains(&s.0)) && same_wrap(&cur[j]) {
                        j += 1;
                    }
                    flush_gen!();
                    let items: Vec<emit::run::GenItem<'a>> =
                        (i..j).map(|k| emit::run::GenItem { run: &cur[k], before: before[k].clone(), after: after[k].clone() }).collect();
                    let open = self.src_text(wrap_open).to_owned();
                    let inner = emit::run::gen_runs(self, &items, g.link.is_some())?;
                    let close = g.wrap_close.as_ref().map_or("", |t| self.src_text(t)).to_owned();
                    out.push_str(&open);
                    out.push_str(&inner);
                    out.push_str(&close);
                    take(self, g0);
                    zp = g0 + 1;
                    i = j;
                    continue;
                }
            }
            pending.push(emit::run::GenItem { run: r, before: before[i].clone(), after: after[i].clone() });
            i += 1;
        }
        flush_gen!();
        if last {
            flush_zero!(segs.len());
        }
        Ok(out)
    }

    // ───────────────────────────── tables ─────────────────────────────

    /// `tableXml(t)`.
    pub fn table_xml(&mut self, t: &'a Table) -> WResult<String> {
        let src = self.store().and_then(|s| t.src.and_then(|k| s.tables.get(&k).map(|ts| (k, ts))));
        if let Some((k, src)) = src {
            let l = self.lossless_mut()?;
            let owner = l.claimed.insert(k);
            if owner && l.pristine.table(t) {
                return Ok(self.src_text(&src.raw).to_owned());
            }
            let grid_join =
                t.grid_cols_twips.iter().map(|x| if x.is_null() { String::new() } else { num_str(*x) }).collect::<Vec<_>>().join(",");
            let same_grid = src.tbl_grid.as_ref().is_some_and(|g| grid_of(self.src_text(g)) == grid_join);
            let tbl_pr = match src.tbl_pr.as_ref() {
                Some(raw) => {
                    let raw = self.src_text(raw);
                    emit::table::patch_tbl_pr(&mut self.cx, raw, t.tbl_pr.as_deref())?
                }
                None => emit::table::legacy_or_typed_tbl_pr(&mut self.cx, t)?,
            };
            let grid = match (same_grid, src.tbl_grid.as_ref()) {
                (true, Some(g)) => self.src_text(g).to_owned(),
                _ => emit::table::grid_xml(t),
            };
            let open = if owner { self.src_text(&src.open).to_owned() } else { "<w:tbl>".to_owned() };
            let rows = self.rows_xml(t)?;
            return Ok(format!("{open}{tbl_pr}{grid}{rows}</w:tbl>"));
        }
        let tbl_pr = emit::table::legacy_or_typed_tbl_pr(&mut self.cx, t)?;
        let grid = emit::table::grid_xml(t);
        let rows = self.rows_xml(t)?;
        Ok(format!("<w:tbl>{tbl_pr}{grid}{rows}</w:tbl>"))
    }

    /// `rowsXml(t)`.
    fn rows_xml(&mut self, t: &'a Table) -> WResult<String> {
        let mut rows: Vec<Wrapped<'a>> = Vec::new();
        for row in &t.rows {
            let rs = self.store().and_then(|s| row.src.and_then(|k| s.trs.get(&k).map(|r| (k, r))));
            let row_owner = match rs {
                Some((k, _)) => self.lossless_mut()?.claimed.insert(k),
                None => false,
            };
            let ex = match rs.and_then(|(_, r)| r.tbl_pr_ex.as_ref()) {
                Some(raw) => {
                    let raw = self.src_text(raw);
                    emit::table::patch_tbl_pr_ex(&mut self.cx, raw, row.tbl_pr_ex.as_deref())?
                }
                None => match row.tbl_pr_ex.as_deref() {
                    Some(p) => emit::table::tbl_pr_ex_xml(&mut self.cx, p)?,
                    None => String::new(),
                },
            };
            let tr_pr = match rs.and_then(|(_, r)| r.tr_pr.as_ref()) {
                Some(raw) => {
                    let raw = self.src_text(raw);
                    emit::table::patch_tr_pr(&mut self.cx, raw, row.tr_pr.as_deref())?
                }
                None => match row.tr_pr.as_deref() {
                    Some(p) => emit::table::tr_pr_xml(&mut self.cx, p)?,
                    None => String::new(),
                },
            };
            let mut cells: Vec<Wrapped<'a>> = Vec::new();
            for cell in &row.cells {
                cells.push(self.cell_xml(cell)?);
            }
            let cells = wrapped_xml(self, &cells);
            let open = match (row_owner, rs) {
                (true, Some((_, r))) => self.src_text(&r.open).to_owned(),
                _ => "<w:tr>".to_owned(),
            };
            let xml = format!("{open}{ex}{tr_pr}{cells}</w:tr>");
            rows.push(match (row_owner, rs) {
                (true, Some((_, r))) => Wrapped { pre: r.pre.as_deref(), post: r.post.as_deref(), xml },
                _ => Wrapped { pre: None, post: None, xml },
            });
        }
        Ok(wrapped_xml(self, &rows))
    }

    fn cell_xml(&mut self, cell: &'a Cell) -> WResult<Wrapped<'a>> {
        let cs = self.store().and_then(|s| cell.src.and_then(|k| s.tcs.get(&k).map(|c| (k, c))));
        let cell_owner = match cs {
            Some((k, _)) => self.lossless_mut()?.claimed.insert(k),
            None => false,
        };
        let tc_pr = match cs.and_then(|(_, c)| c.tc_pr.as_ref()) {
            Some(raw) => {
                let raw = self.src_text(raw);
                emit::table::patch_tc_pr(&mut self.cx, raw, cell.tc_pr.as_deref())?
            }
            None => match (cell.tc_pr.as_deref(), cs) {
                (Some(p), _) => emit::table::tc_pr_xml(&mut self.cx, p)?,
                (None, Some(_)) => String::new(),
                (None, None) => emit::table::tc_pr_xml_legacy(&mut self.cx, cell)?,
            },
        };
        let blocks: Vec<Block<'a>> = cell.content.blocks().collect();
        let groups = match (cell_owner, cs) {
            (true, Some((_, c))) => c.groups.as_deref(),
            _ => None,
        };
        let body = match groups {
            Some(g) if !blocks.is_empty() => self.groups_xml(g, &blocks)?,
            _ => self.cell_body_xml(&blocks)?,
        };
        let open = match (cell_owner, cs) {
            (true, Some((_, c))) => self.src_text(&c.open).to_owned(),
            _ => "<w:tc>".to_owned(),
        };
        let xml = format!("{open}{tc_pr}{body}</w:tc>");
        Ok(match (cell_owner, cs) {
            (true, Some((_, c))) => Wrapped { pre: c.pre.as_deref(), post: c.post.as_deref(), xml },
            _ => Wrapped { pre: None, post: None, xml },
        })
    }

    /// `cellBodyXml(cell)`: the blocks, or `<w:p/>` for an empty cell.
    fn cell_body_xml(&mut self, blocks: &[Block<'a>]) -> WResult<String> {
        if blocks.is_empty() {
            return Ok("<w:p/>".to_owned());
        }
        self.blocks_inner(blocks)
    }

    // ───────────────────────────── objects ─────────────────────────────

    /// `txbxInner(tb)`: a textbox's content, its block wrappers back (the groups found by the
    /// textbox's `TxbxSrc`, U4c).
    pub fn txbx_inner(&mut self, tb: &'a DrawingTextbox) -> WResult<String> {
        let blocks: Vec<Block<'a>> = if tb.view == TextboxView::ParasOnly { Vec::new() } else { tb.content.blocks().collect() };
        let groups = self.store().and_then(|s| tb.txbx_src.and_then(|t| s.txbx_groups.get(t.index())));
        if let Some(g) = groups
            && !blocks.is_empty()
        {
            return self.groups_xml(g, &blocks);
        }
        if !blocks.is_empty() {
            return self.blocks_inner(&blocks);
        }
        self.textbox_paras_xml(tb)
    }

    /// `tb.paras.map(paraXml).join('')` (the phantom empty paragraph of a box without one).
    pub fn textbox_paras_xml(&mut self, tb: &'a DrawingTextbox) -> WResult<String> {
        let mut out = String::new();
        if tb.view == TextboxView::Phantom {
            let p = Paragraph::default();
            // a fresh paragraph without source: generated
            out.push_str(&self.para_xml_owned(&p)?);
            return Ok(out);
        }
        let paras = if tb.view == TextboxView::TopLevel { tb.content.paras.iter().collect() } else { tb.content.flat_paragraphs() };
        for p in paras {
            out.push_str(&self.para_xml(p)?);
        }
        Ok(out)
    }

    /// `paraXml` of a paragraph that is not in the document (the phantom of a textbox): no
    /// source, so the generated path, without the `paraRuns` bookkeeping a borrowed run needs.
    fn para_xml_owned(&mut self, p: &Paragraph) -> WResult<String> {
        if !p.runs.is_empty() {
            return Err(DocxError::Write("a phantom paragraph with runs"));
        }
        let ids = self.lossless.as_ref().map_or(super::ctx::DEFAULT_NUM_IDS, |l| l.num_ids);
        if !self.active() {
            return emit::run::para_xml_inner(self, &p.props, "");
        }
        let ppr = ppr_from_props(&mut self.cx, &p.props, "", ids)?;
        Ok(format!("<w:p>{}</w:p>", ppr.xml))
    }

    /// `drawingWithParas(raw, paras, blocks, tb)`: every `w:txbxContent` of the raw drawing with
    /// the box's current content.
    pub fn drawing_with_paras(&mut self, raw: &str, tb: &'a DrawingTextbox) -> WResult<String> {
        let inner = self.txbx_inner(tb)?;
        Ok(replace_txbx_contents(raw, |_| inner.clone()))
    }

    /// `objectXml(run)`: a read object (`None` = no source object: an old generator's run).
    pub fn object_xml(&mut self, run: &'a Run) -> WResult<Option<String>> {
        let store = self.store().ok_or(DocxError::Write("lossless state missing"))?;
        let ext = run.ext();
        let sid = run.props.src;
        let src = sid.and_then(|k| store.runs.get(&k));
        let obj_head = |rs: &crate::model::sources::RunSource| rs.obj_head.and_then(|x| sid_key(x.to_number()));
        let head_sid: Option<SourceKey> = match src {
            Some(rs) if rs.obj.is_some() => sid,
            Some(rs) => obj_head(rs),
            None => None,
        };
        let head = head_sid.and_then(|k| store.runs.get(&k));
        let rpr = self.rpr_xml(run)?;
        // a textbox/shape edited in Format Shape (or new): rebuilt from the wps
        let tb_edited = ext.and_then(|e| e.textbox.as_deref()).filter(|t| t.edited == Some(true) && t.raw_drawing_xml.is_some());
        let sh_edited = ext.and_then(|e| e.shape.as_deref()).filter(|s| s.edited == Some(true) && s.raw_drawing_xml.is_some());
        if tb_edited.is_some() || sh_edited.is_some() {
            if let (Some(h), Some(hs)) = (head, head_sid)
                && h.obj.is_some()
            {
                self.lossless_mut()?.emitted_obj.insert(hs);
            }
            let drawing = tb_edited.map(|t| t.drawing).or(sh_edited.map(|s| s.drawing)).unwrap_or_default();
            let xml = emit::object::edited_shape_xml(self, run)?;
            let xml = emit::object::once_shape(self, drawing, xml)?;
            return Ok(Some(format!("<w:r>{rpr}{xml}</w:r>")));
        }
        // a group edited in the Selection Pane: the original with the new name/hidden/order
        if let Some(g) = ext.and_then(|e| e.group.as_deref())
            && g.edited == Some(true)
            && let Some(raw) = g.raw_drawing_xml.as_deref()
            && let (Some(h), Some(hs)) = (head, head_sid)
            && let Some(obj) = h.obj.as_ref()
        {
            let obj = self.src_text(obj).to_owned();
            let xml = emit::object::edited_group_xml(self, &obj, raw)?;
            let emitted = self.lossless_mut()?.emitted_obj.contains(&hs);
            let out = if emitted { emit::object::fresh_drawing_ids(self, &xml)? } else { xml };
            self.lossless_mut()?.emitted_obj.insert(hs);
            return Ok(Some(format!("<w:r>{rpr}{out}</w:r>")));
        }
        let Some((head, head_sid)) = head.zip(head_sid).filter(|(h, _)| h.obj.is_some()) else {
            let img = ext.and_then(|e| e.image.as_deref());
            if let Some(img) = img {
                if img.ole.is_some() && img.raw_drawing_xml.is_none() {
                    return Ok(Some(format!("<w:r>{rpr}{}</w:r>", emit::object::ole_object_xml(self, run)?)));
                }
                if img.raw_drawing_xml.is_some() {
                    return Ok(Some(format!("<w:r>{rpr}{}</w:r>", emit::object::picture_xml(self, img, None)?)));
                }
                return Ok(Some(format!("<w:r>{}</w:r>", emit::object::drawing_xml(self, run)?)));
            }
            if let Some(tb) = ext.and_then(|e| e.textbox.as_deref()) {
                if let Some(raw) = tb.raw_drawing_xml.as_deref() {
                    if tb.vml == Some(true) {
                        let xml = emit::object::edited_shape_xml(self, run)?;
                        let xml = emit::object::once_shape(self, tb.drawing, xml)?;
                        return Ok(Some(format!("<w:r>{rpr}{xml}</w:r>")));
                    }
                    let geo = emit::object::patch_drawing_geometry(raw, &emit::object::Geometry::of_textbox(tb))?;
                    let xml = self.drawing_with_paras(&geo, tb)?;
                    return Ok(Some(format!("<w:r>{rpr}{xml}</w:r>")));
                }
                return Ok(Some(String::new()));
            }
            return Ok(None);
        };
        // members of the object: the run that carries it and the next runs of the same object
        let runs: &'a [Run] = self.lossless.as_ref().and_then(|l| l.para_runs).unwrap_or(&[]);
        let idx = runs.iter().position(|r| std::ptr::eq(r, run));
        let is_member =
            |r: &Run| -> bool { is_object_run(r) && r.props.src.and_then(|k| store.runs.get(&k)).and_then(obj_head) == Some(head_sid) };
        if let Some(idx) = idx
            && src.and_then(obj_head).is_some()
        {
            // runs[j..idx] are members of the object (TS: k = j - 1)
            let mut j = idx;
            while j > 0 && is_member(&runs[j - 1]) {
                j -= 1;
            }
            if j > 0 && runs[j - 1].props.src == Some(head_sid) && is_object_run(&runs[j - 1]) {
                return Ok(Some(String::new()));
            }
            if j != idx {
                return Ok(Some(String::new()));
            }
        }
        let mut members: Vec<&'a Run> = vec![run];
        if let Some(idx) = idx {
            let mut k = idx + 1;
            while k < runs.len() && is_member(&runs[k]) {
                members.push(&runs[k]);
                k += 1;
            }
        }
        let mut xml = head.obj.as_ref().map_or("", |t| self.src_text(t)).to_owned();
        // the boxes of the object in XML order: textbox runs and group items
        let mut boxes: Vec<&'a DrawingTextbox> = Vec::new();
        for r in &members {
            let Some(e) = r.ext() else { continue };
            if let Some(tb) = e.textbox.as_deref() {
                boxes.push(tb);
            } else if let Some(g) = e.group.as_deref() {
                for it in &g.items {
                    if let Some(tb) = it.run.ext().and_then(|e| e.textbox.as_deref()) {
                        boxes.push(tb);
                    }
                }
            }
        }
        if !boxes.is_empty() {
            let mut inners = Vec::with_capacity(boxes.len());
            for b in &boxes {
                inners.push(self.txbx_inner(b)?);
            }
            let total = count_txbx_contents(&xml);
            let nb = boxes.len();
            // one box: every occurrence (mc:Choice + the VML fallback); n boxes: in order (and
            // again in the fallback); a count that does not fit: the text stays as it was
            let pick: Option<Box<dyn Fn(usize) -> usize>> = if nb == 1 {
                Some(Box::new(|_| 0))
            } else if total == nb || total == 2 * nb {
                Some(Box::new(move |k| k % nb))
            } else {
                None
            };
            if let Some(pick) = pick {
                xml = replace_txbx_contents(&xml, |k| inners.get(pick(k)).cloned().unwrap_or_default());
            }
        }
        if members.len() <= 1 {
            if let Some(img) = ext.and_then(|e| e.image.as_deref()) {
                if img.raw_drawing_xml.is_some() {
                    xml = emit::object::picture_xml(self, img, Some(&xml))?;
                } else {
                    xml = emit::object::patch_drawing_geometry(&xml, &emit::object::Geometry::of_image(img))?;
                }
            } else if let Some(tb) = ext.and_then(|e| e.textbox.as_deref()) {
                xml = emit::object::patch_drawing_geometry(&xml, &emit::object::Geometry::of_textbox(tb))?;
            }
        }
        if self.lossless_mut()?.emitted_obj.contains(&head_sid) {
            xml = emit::object::fresh_drawing_ids(self, &xml)?;
        }
        self.lossless_mut()?.emitted_obj.insert(head_sid);
        Ok(Some(format!("<w:r>{rpr}{xml}</w:r>")))
    }

    // ───────────────────────────── notes ─────────────────────────────

    /// `noteXml(kind, id, blocks)`: a new note (or of a part without source), generated.
    pub fn note_xml(&mut self, kind: &str, id: f64, story: &'a Story) -> WResult<String> {
        let blocks: Vec<Block<'a>> = story.blocks().collect();
        let inner = if blocks.is_empty() { "<w:p/>".to_owned() } else { self.blocks_inner(&blocks)? };
        Ok(format!("<w:{kind} w:id=\"{}\">{inner}</w:{kind}>", uo_jsnum::format::to_js_string(id)))
    }
}

/// A positive integer `src` value as a key (`0`, NaN and fractions name no source).
fn sid_key(x: f64) -> Option<SourceKey> {
    (x > 0.0 && x <= f64::from(u32::MAX) && x.fract() == 0.0).then(|| SourceKey(uo_jsnum::conv::to_uint32(x)))
}

/// A row or cell with the raw pieces around it (`{ pre?, post?, xml }`).
struct Wrapped<'a> {
    pre: Option<&'a [WrapSeg]>,
    post: Option<&'a [WrapSeg]>,
    xml: String,
}

/// `wrappedXml(items)`: the wrappers only when the nesting still holds after the edit; the loose
/// marks always.
fn wrapped_xml(w: &Writer<'_>, items: &[Wrapped<'_>]) -> String {
    let mut stack: Vec<u32> = Vec::new();
    let mut ok = true;
    for it in items {
        for s in it.pre.unwrap_or(&[]).iter().chain(it.post.unwrap_or(&[]).iter()) {
            if let Some(o) = s.open {
                stack.push(o.0);
            } else if let Some(c) = s.close
                && stack.pop() != Some(c.0)
            {
                ok = false;
            }
        }
    }
    if !stack.is_empty() {
        ok = false;
    }
    let segs = |ss: Option<&[WrapSeg]>, out: &mut String| {
        for s in ss.unwrap_or(&[]) {
            if ok || (s.open.is_none() && s.close.is_none()) {
                out.push_str(w.src_text(&s.raw));
            }
        }
    };
    let mut out = String::new();
    for it in items {
        segs(it.pre, &mut out);
        out.push_str(&it.xml);
        segs(it.post, &mut out);
    }
    out
}

/// `countCopies(blocks, out)`: the copies of each source paragraph in the part, cells and
/// textboxes included (a run's textbox, and the textboxes of a group's items — one level, as the
/// TS). A sum, so the walk order is free; iterative.
pub fn count_copies(blocks: &[Block<'_>], out: &mut HashMap<SourceKey, i64>) {
    let mut stack: Vec<Block<'_>> = blocks.to_vec();
    while let Some(b) = stack.pop() {
        match b {
            Block::Paragraph(p) => {
                if let Some(k) = p.props.src {
                    *out.entry(k).or_insert(0) += 1;
                }
                for r in &p.runs {
                    let Some(e) = r.ext() else { continue };
                    if let Some(tb) = e.textbox.as_deref() {
                        stack.extend(tb.content.blocks());
                    } else if let Some(g) = e.group.as_deref() {
                        for it in &g.items {
                            if let Some(tb) = it.run.ext().and_then(|e| e.textbox.as_deref()) {
                                stack.extend(tb.content.blocks());
                            }
                        }
                    }
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for c in &row.cells {
                        stack.extend(c.content.blocks());
                    }
                }
            }
        }
    }
}
