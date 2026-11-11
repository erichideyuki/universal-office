//! What must not change for an incremental relayout to reuse the previous run (LA7 level 2): the
//! options, the font provider's answers, every document part but the body (shared `Arc`s), the
//! shape of the derived document (its keys), and which body blocks the edit replaced.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::fonts::{FontProvider, ProviderAsk};
use crate::ldoc::{LBlock, LDoc, ParaSrc, RunSrc};
use crate::model::{Document, Story};
use crate::options::LayoutOptions;

/// The options as a key: every field but the cancellation (a flag and a poll), by `Debug`;
/// `None` when the hyphenation skips are not empty (a `HashSet`: its `Debug` order is not
/// stable — the editor's relayouts pass none; D-LA7-8).
pub(crate) fn options_key(o: &LayoutOptions) -> Option<String> {
    if !o.hyphenation_skips.0.is_empty() {
        return None;
    }
    let mut k = o.clone();
    k.cancel = None;
    k.cancel_poll = None;
    Some(format!("{k:?}"))
}

/// The provider still answers every request of the previous call with the same face (the same
/// answer identity) and still misses what it missed.
pub(crate) fn provider_same(p: &dyn FontProvider, log: &[ProviderAsk]) -> bool {
    log.iter().all(|a| p.face(&a.family, a.bold, a.italic).map(|x| x.id()) == a.answer)
}

/// The parts of the document besides the body, kept from the previous call (shallow clones:
/// `Arc`s and persistent vectors, so their pointers stay alive and are never reused).
pub(crate) struct DocParts {
    pub body: Story,
    pub doc: Document,
}

impl DocParts {
    pub(crate) fn of(d: &Document) -> DocParts {
        // the body is compared apart; the rest of the document is kept as is (cheap: Arcs)
        DocParts { body: d.body.clone(), doc: Document { body: Story::default(), ..d.clone() } }
    }

    /// Every part the layout reads besides the body is the same object (or equal).
    pub(crate) fn same_parts(&self, d: &Document) -> bool {
        let o = &self.doc;
        o.formatting_version == d.formatting_version
            && Arc::ptr_eq(&o.sections, &d.sections)
            && Arc::ptr_eq(&o.notes, &d.notes)
            && Arc::ptr_eq(&o.comments, &d.comments)
            && Arc::ptr_eq(&o.styles, &d.styles)
            && Arc::ptr_eq(&o.bookmarks, &d.bookmarks)
            && Arc::ptr_eq(&o.formatting, &d.formatting)
            && match (&o.background, &d.background) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && o.core_props == d.core_props
            && o.hf.len() == d.hf.len()
            && o.hf.iter_arcs().zip(d.hf.iter_arcs()).all(|(a, b)| Arc::ptr_eq(a, b))
    }

    /// The body blocks the edit replaced, `[first, last]` in `Story::blocks` order; `Some(None)`
    /// when none; `None` when the paragraph or table count changed (a structure edit).
    pub(crate) fn changed_blocks(&self, new: &Story) -> Option<Option<(usize, usize)>> {
        let old = &self.body;
        if old.paras.len() != new.paras.len() || old.tables.len() != new.tables.len() {
            return None;
        }
        let befores: Vec<u32> = new.tables.iter().map(|t| t.before_para).collect();
        let mut range: Option<(usize, usize)> = None;
        let mut note = |b: usize| {
            range = Some(match range {
                Some((lo, hi)) => (lo.min(b), hi.max(b)),
                None => (b, b),
            });
        };
        for (i, a) in new.paras.unshared_chunk_elements(&old.paras) {
            if old.paras.get_arc(i).is_none_or(|b| !Arc::ptr_eq(a, b)) {
                // the tables anchored before paragraph i (or before it, `before_para <= i`) come
                // first in the block order
                let i32_ = u32::try_from(i).unwrap_or(u32::MAX);
                note(i + befores.partition_point(|&p| p <= i32_));
            }
        }
        let n = new.paras.len();
        for (j, a) in new.tables.unshared_chunk_elements(&old.tables) {
            let same = old
                .tables
                .get_arc(j)
                .is_some_and(|b| Arc::ptr_eq(a, b) || (a.before_para == b.before_para && Arc::ptr_eq(&a.table, &b.table)));
            if !same {
                let bp = a.before_para as usize;
                note(j + bp.min(n));
            }
        }
        Some(range)
    }
}

fn h(x: &impl Hash) -> u64 {
    let mut s = rustc_hash::FxHasher::default();
    x.hash(&mut s);
    s.finish()
}

/// The shape of the derived document: everything its keys (`LParaId`, `RunKey`, `TbKey`, story
/// indices) depend on. Equal shapes = the previous run's keys name the same things.
pub(crate) fn shape(l: &LDoc<'_>) -> Vec<u64> {
    let n = |x: usize| x as u64;
    let mut v = vec![
        n(l.paras.len()),
        n(l.runs.len()),
        n(l.stories.len()),
        n(l.textboxes.len()),
        n(l.chains.len()),
        n(l.field_results.len()),
        n(l.by_para.len()),
        n(l.hf_stories.len()),
        n(l.note_stories.len()),
        n(l.tb_of_drawing.len()),
        l.en_sep.map_or(u64::MAX, |k| u64::from(k.0)),
        n(l.revision_order.len()),
    ];
    for a in &l.revision_order {
        v.push(h(a));
    }
    for p in &l.paras {
        match p.src {
            ParaSrc::Model { id, .. } => v.extend([1, u64::from(id.0)]),
            ParaSrc::Copy { base } => v.extend([2, u64::from(base.0)]),
        }
        v.extend([
            u64::from(p.flags.0),
            p.frame_carry.map_or(u64::MAX, |k| u64::from(k.0)),
            u64::from(p.ppr.is_some()),
            u64::from(p.props.is_some()),
            h(&format!("{:?}", p.story)),
            h(&p.path.steps()),
            n(p.runs.len()),
            n(p.rev_joins.len()),
        ]);
        v.extend(p.runs.iter().map(|k| u64::from(k.0)));
        v.extend(p.rev_joins.iter().map(|&j| u64::from(j)));
    }
    for r in &l.runs {
        match r {
            RunSrc::Model { at, .. } => v.extend([1, h(&format!("{:?}{:?}", at.story, at.sub)), h(&at.path.steps()), u64::from(at.run)]),
            RunSrc::Slice { base, from } => v.extend([2, u64::from(base.0), u64::from(*from)]),
            RunSrc::Synth(_) => v.push(3),
        }
    }
    for s in &l.stories {
        v.push(n(s.blocks.len()));
        v.push(h(&format!("{:?}", s.sref)));
        for b in &s.blocks {
            match b {
                LBlock::Paragraph(k) => v.extend([1, u64::from(k.0)]),
                LBlock::Table { block } => v.extend([2, u64::from(*block)]),
            }
        }
    }
    for t in &l.textboxes {
        v.extend([
            u64::from(t.story),
            t.drawing.map_or(u64::MAX, |d| u64::from(d.0)),
            u64::from(t.frame.is_some()),
            t.chain.map_or(u64::MAX, |c| u64::from(c.0)),
        ]);
    }
    v
}
