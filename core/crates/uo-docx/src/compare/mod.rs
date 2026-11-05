//! Document comparison (the `compare` package of the TS, D5a): the diff of the bodies of two documents
//! read with the docx reader, in the final view of the track-changes state. Feature `compare`.
//! The `<universal-write-diff>` element of that package is the shell's (it queries the two documents
//! through the binding).
//!
//! | TS | module |
//! |---|---|
//! | `tokens.ts` | [`tokens`] |
//! | `myers.ts` | [`myers`] |
//! | `histogram.ts` | [`histogram`] |
//! | `diff.ts` | [`diff`] (+ [`json`] for the shape of the result) |
//!
//! The algorithms are unbounded in the TS (Myers keeps a snapshot of its vector per step, the
//! histogram diff compares every candidate of a bucket); here their work is capped ([`budget`]) and
//! a hostile pair finishes in bounded time with a valid, not minimal, script ([`DocDiff::limited`]).
//! The declarations of the port (the nine kinds) are in `core/docs/modules/uo-docx-d5/D5a.md`.

pub mod budget;
pub mod diff;
pub mod histogram;
pub mod json;
pub mod matchers;
pub mod myers;
pub mod tokens;

use std::borrow::Cow;

use crate::final_view::final_paragraphs;
use crate::model::{Block, Document, Paragraph, Story, Table};

pub use budget::{Budget, DIFF_WORK_LIMIT, INTRA_TRACE_CELLS, INTRA_WORK_LIMIT, MYERS_TRACE_CELLS};
pub use diff::{
    BlockRange, DEFAULT_DOC_DIFF_OPTIONS, DiffAlgorithm, DiffLimits, DocDiff, DocDiffOptions, DocDiffStats, DocEdit, DocEditKind,
    DocIntraEdit, Importance, IntraKind, Side, TableRowEdit, diff_blocks, diff_blocks_with_limits, intra_word_marks,
};
pub use histogram::histogram_diff;
pub use myers::{DiffOp, DiffOpKind, myers_diff};
pub use tokens::{BlockToken, Interner, KeyId, bytes_equal, fnv1a, tokenize_block, tokenize_blocks, tokens_equal};

/// A block of the diff: a paragraph (borrowed, or owned when the final view changed it), or a table.
/// `final_cells` says that the table is a body table in the final view: the top-level paragraphs of
/// its cells are the final ones (the TS `finalBlocks` rewrites those and leaves the tables nested in
/// the cells as they are).
#[derive(Debug, Clone)]
pub enum DiffBlock<'a> {
    Paragraph(Cow<'a, Paragraph>),
    Table { table: &'a Table, final_cells: bool },
}

/// The blocks of a story as they are (the TS `Block[]`).
#[must_use]
pub fn blocks_of(story: &Story) -> Vec<DiffBlock<'_>> {
    story
        .blocks()
        .map(|b| match b {
            Block::Paragraph(p) => DiffBlock::Paragraph(Cow::Borrowed(p)),
            Block::Table(t) => DiffBlock::Table { table: t, final_cells: false },
        })
        .collect()
}

/// `finalBlocks(blocks)`: the final view of the track-changes state: `finalParagraphs` over each run
/// of consecutive paragraphs, and the cells of the tables.
#[must_use]
pub fn final_blocks(story: &Story) -> Vec<DiffBlock<'_>> {
    fn flush<'a>(run: &mut Vec<&'a Paragraph>, out: &mut Vec<DiffBlock<'a>>) {
        if run.is_empty() {
            return;
        }
        for p in final_paragraphs(run.iter().copied()).paras {
            out.push(DiffBlock::Paragraph(p));
        }
        run.clear();
    }
    let mut out = Vec::new();
    let mut run: Vec<&Paragraph> = Vec::new();
    for b in story.blocks() {
        match b {
            Block::Paragraph(p) => run.push(p),
            Block::Table(t) => {
                flush(&mut run, &mut out);
                out.push(DiffBlock::Table { table: t, final_cells: true });
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

/// `diffDocuments(a, b, opts)`: the diff of the bodies of two documents in the final view of the
/// track-changes state (the text deleted in revision does not count).
#[must_use]
pub fn diff_documents(a: &Document, b: &Document, opts: &DocDiffOptions) -> DocDiff {
    diff_documents_with_limits(a, b, opts, &DiffLimits::default())
}

/// [`diff_documents`] with explicit limits.
#[must_use]
pub fn diff_documents_with_limits(a: &Document, b: &Document, opts: &DocDiffOptions, limits: &DiffLimits) -> DocDiff {
    let (fa, fb) = (final_blocks(&a.body), final_blocks(&b.body));
    diff_blocks_with_limits(&fa, &fb, &a.media, &b.media, opts, limits)
}
