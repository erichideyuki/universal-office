//! The session-free read → write round trip (D1b parity without D2a): the install and the save of
//! docx-core's own lossless harness (`write/packages/docx-core/test/lossless.test.ts`, `open` and
//! `save`), which are the docx-core-level contract of the lossless writer:
//!
//! - **install** = `markSectionBreaks(full)` then `sources.baseline()` ([`install`]);
//! - **save** = every section's header/footer parts (unique by path, the dirty ones chosen by the
//!   caller), the last section's references on the body `w:sectPr`, the comments, the body as
//!   blocks and the sources → `writeDocument` ([`save`]); then `pkg.save()`.
//!
//! The editor's own install (`installFullDoc`: `syncRunRPr`, the DATE field text, note-run marks,
//! `syncSections`, the settle step) and save (`saveDocxBody`: notes, settings, styles,
//! numbering, background…) are D2a's; their parity is the D2 harness's. This entry is what the
//! D1b capture (`js/capture-write-roundtrip.mjs`) runs on both sides.

use std::sync::Arc;

use uo_opc::Package;

use super::ctx::SaveState;
use super::lossless::HfRefXml;
use super::parts::{HfPartOut, WriteOptions, write_document};
use super::sig::SigMode;
use crate::error::DocxError;
use crate::model::Document;
use crate::model::para::{HfRefEdit, SectionBreak};
use crate::model::story::Block;

/// `markSectionBreaks(full)`: the last paragraph of each section but the final one carries
/// `props.sectionBreak` (a section ending in a table: the last paragraph before it).
pub fn mark_section_breaks(doc: &mut Document) {
    let sections = doc.sections.clone();
    let n = sections.len();
    let blocks_len = doc.body.block_count();
    for s in sections.iter().take(n.saturating_sub(1)) {
        let last = blocks_len as f64 - 1.0;
        let b = uo_jsnum::math::min(s.block_end.to_number(), last);
        // `while (b >= 0 && blocks[b].type !== 'paragraph') b--` over integer indices; a NaN or
        // a fraction indexes nothing (`blocks[1.5]` is undefined)
        if b.is_nan() || b < 0.0 || b.fract() != 0.0 {
            continue;
        }
        let mut bi = uo_jsnum::conv::to_uint32(b) as usize;
        let para_index = loop {
            match doc.body.block_at(bi) {
                Some(Block::Paragraph(_)) => break Some(paragraph_index(&doc.body, bi)),
                Some(Block::Table(_)) if bi > 0 => bi -= 1,
                _ => break None,
            }
        };
        let Some(Some(pi)) = para_index else { continue };
        let sb = SectionBreak {
            sect_pr: Arc::clone(&s.sect_pr),
            header_refs: s
                .header_refs
                .iter()
                .map(|r| HfRefEdit { kind: "header".into(), r#type: r.r#type.clone(), rid: r.rid.clone(), ..HfRefEdit::default() })
                .collect(),
            footer_refs: s
                .footer_refs
                .iter()
                .map(|r| HfRefEdit { kind: "footer".into(), r#type: r.r#type.clone(), rid: r.rid.clone(), ..HfRefEdit::default() })
                .collect(),
            ..SectionBreak::default()
        };
        if let Some(p) = doc.body.paras.make_mut(pi) {
            let props = Arc::make_mut(&mut p.props);
            crate::word::obj::set(props, "sectionBreak", |o| o.section_break = Some(Box::new(sb)));
        }
    }
}

/// The paragraph index of block `bi` of a story (the number of paragraphs before it).
fn paragraph_index(story: &crate::model::Story, bi: usize) -> Option<usize> {
    let mut pi = 0;
    for (i, b) in story.blocks().enumerate() {
        if i == bi {
            return matches!(b, Block::Paragraph(_)).then_some(pi);
        }
        if matches!(b, Block::Paragraph(_)) {
            pi += 1;
        }
    }
    None
}

/// The harness install: section breaks marked, then the baseline taken.
pub fn install(doc: &mut Document, mode: SigMode) -> SaveState {
    mark_section_breaks(doc);
    SaveState::install(doc, mode)
}

/// Which header/footer parts the save rewrites (the harness's `dirtyHf`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirtyHf {
    /// none: their bytes stay (only missing relationships and content types are added)
    None,
    /// all: every part goes through the lossless story path
    All,
}

/// The harness save: `writeDocument(pkg, { paragraphs, sectPr: last.sectPr }, hfParts,
/// { comments, bodySectRefs, blocks, sources })`.
pub fn save(pkg: &mut Package, doc: &Document, state: &mut SaveState, dirty: DirtyHf) -> Result<(), DocxError> {
    let Some(last) = doc.sections.last() else { return Err(DocxError::Write("a document without sections")) };
    let mut hf: Vec<HfPartOut<'_>> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for s in doc.sections.iter() {
        for (kind, refs, slots) in [("header", &s.header_refs, &s.headers), ("footer", &s.footer_refs, &s.footers)] {
            for r in refs {
                if seen.contains(&r.path.as_ref()) {
                    continue;
                }
                // `!set[r.type]`: the slot must hold a part
                let Some(part) = slots.get(&r.r#type).and_then(|id| doc.hf.get(id.index())) else { continue };
                seen.push(&r.path);
                hf.push(HfPartOut {
                    kind: kind.into(),
                    r#type: r.r#type.clone(),
                    rid: r.rid.clone(),
                    path: r.path.clone(),
                    story: &part.story,
                    dirty: dirty == DirtyHf::All,
                });
            }
        }
    }
    let last_paths: Vec<&str> = last.header_refs.iter().chain(last.footer_refs.iter()).map(|r| r.path.as_ref()).collect();
    let body_refs: Vec<HfRefXml> = hf
        .iter()
        .filter(|h| last_paths.contains(&h.path.as_ref()))
        .map(|h| HfRefXml { kind: h.kind.clone(), r#type: h.r#type.clone(), rid: h.rid.clone() })
        .collect();
    let opts = WriteOptions {
        comments: (!doc.comments.is_empty()).then_some(&*doc.comments),
        body_sect_refs: Some(body_refs),
        blocks: true,
        ..WriteOptions::default()
    };
    write_document(pkg, doc, &doc.body, &last.sect_pr, &hf, &opts, state)
}
