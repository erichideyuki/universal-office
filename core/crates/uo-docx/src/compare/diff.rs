//! The orchestrator of the document diff (compare `diff.ts`): tokenize → histogram/Myers →
//! [`DocEdit`]s.
//!
//! The range convention is OpenCompare's (validated against Beyond Compare 5): 1-based, inclusive at
//! both ends; an empty range uses `start` = the block AFTER which the edit is anchored and
//! `end = start − 1` (length 0). Adjacent delete + insert fold into one `change` edit.
//!
//! Beyond OpenCompare's model, this knows documents:
//!
//! - aligned pairs with equal content and different formatting become a `change` with `format_only`
//!   and importance `minor` (Beyond Compare's "unimportant difference"), without breaking the
//!   alignment; a resized image with equal bytes is that case (the size lives in the format key, the
//!   bytes in the content key);
//! - the `change` edits of paragraphs get a word-level refinement (Myers over word and space tokens);
//! - the `change` pairs of tables get a nested diff of their rows.
//!
//! Limits (declared in the record): the work of the alignment, of the nested row diff and of the word
//! refinement is capped ([`super::budget`]); past a limit the result is a valid but not minimal script
//! ([`DocDiff::limited`]), and a word refinement is left out.

use std::borrow::Cow;

use crate::model::{MediaStore, Paragraph};

use super::DiffBlock;
use super::budget::{Budget, DIFF_WORK_LIMIT, INTRA_TRACE_CELLS, INTRA_WORK_LIMIT, MYERS_TRACE_CELLS};
use super::histogram::histogram_diff;
use super::matchers::{has_non_space, word_tokens};
use super::myers::{DiffOp, DiffOpKind, myers_diff};
use super::tokens::{BlockToken, Interner, KeyId, row_content_key, row_format_key, tokenize_blocks, tokens_equal_cost};

/// `DocEditKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocEditKind {
    Equal,
    Insert,
    Delete,
    Change,
}

impl DocEditKind {
    /// The TS string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DocEditKind::Equal => "equal",
            DocEditKind::Insert => "insert",
            DocEditKind::Delete => "delete",
            DocEditKind::Change => "change",
        }
    }
}

/// `BlockRange`: 1-based, inclusive; `end = start − 1` is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRange {
    pub start: usize,
    pub end: usize,
}

/// The side of a mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

/// `DocIntraEdit`: a word inserted or deleted inside an aligned pair of a `change` edit; offsets in
/// UTF-16 code units of the paragraph's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocIntraEdit {
    pub kind: IntraKind,
    pub side: Side,
    /// 0-based, the i-th aligned pair of the edit
    pub pair_offset: usize,
    /// inclusive
    pub start_ch: usize,
    /// exclusive
    pub end_ch: usize,
}

/// `DocIntraEdit.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntraKind {
    Insert,
    Delete,
}

/// `TableRowEdit`: a row of the nested diff of a changed pair of tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRowEdit {
    pub kind: DocEditKind,
    /// 0-based rows of table A / B
    pub a_rows: Vec<usize>,
    pub b_rows: Vec<usize>,
    pub format_only: bool,
}

/// `DocEdit.importance`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Importance {
    Important,
    Minor,
}

/// `DocEdit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEdit {
    pub kind: DocEditKind,
    pub a: BlockRange,
    pub b: BlockRange,
    /// 0-based indices of the blocks of this edit in each document
    pub a_blocks: Vec<usize>,
    pub b_blocks: Vec<usize>,
    /// equal content, only the formatting changed (includes a resized image)
    pub format_only: bool,
    pub importance: Option<Importance>,
    pub intra: Option<Vec<DocIntraEdit>>,
    /// per aligned pair of tables: the diff of the rows (`None` when the pair is not table ↔ table)
    pub table_rows: Option<Vec<Option<Vec<TableRowEdit>>>>,
}

/// `DocDiffStats`: counts by block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DocDiffStats {
    pub inserted: usize,
    pub deleted: usize,
    pub changed: usize,
    pub format_only: usize,
    pub equal: usize,
}

/// `DocDiff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocDiff {
    pub edits: Vec<DocEdit>,
    pub stats: DocDiffStats,
    /// a work or memory limit was hit: some part of the script is the coarse one (not minimal), or a
    /// word refinement was left out (not in the TS shape, not in [`DocDiff::to_json`])
    pub limited: bool,
}

/// `DocDiffOptions.algorithm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffAlgorithm {
    #[default]
    Histogram,
    Myers,
}

/// `DocDiffOptions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocDiffOptions {
    pub algorithm: DiffAlgorithm,
    /// word refinement of the `change` edits of paragraphs
    pub intra_word: bool,
}

/// `DEFAULT_DOC_DIFF_OPTIONS`.
pub const DEFAULT_DOC_DIFF_OPTIONS: DocDiffOptions = DocDiffOptions { algorithm: DiffAlgorithm::Histogram, intra_word: true };

impl Default for DocDiffOptions {
    fn default() -> Self {
        DEFAULT_DOC_DIFF_OPTIONS
    }
}

/// The limits of one diff (the defaults are [`super::budget`]'s; tests shrink them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffLimits {
    pub work: u64,
    pub myers_trace_cells: usize,
    pub intra_work: u64,
    pub intra_trace_cells: usize,
}

impl Default for DiffLimits {
    fn default() -> Self {
        DiffLimits {
            work: DIFF_WORK_LIMIT,
            myers_trace_cells: MYERS_TRACE_CELLS,
            intra_work: INTRA_WORK_LIMIT,
            intra_trace_cells: INTRA_TRACE_CELLS,
        }
    }
}

/// `OpKind` of the TS: `format` is an aligned pair with equal content and different formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpKind {
    Equal,
    Delete,
    Insert,
    Format,
}

struct OpRun {
    kind: OpKind,
    a_idxs: Vec<usize>,
    b_idxs: Vec<usize>,
    /// 1-based; for an empty run, the block after which it anchors
    a_start: usize,
    b_start: usize,
}

fn ops_to_runs(ops: &[DiffOp], a: &[BlockToken<'_>], b: &[BlockToken<'_>]) -> Vec<OpRun> {
    let tagged: Vec<(OpKind, Option<usize>, Option<usize>)> = ops
        .iter()
        .map(|o| {
            let kind = match o.op {
                DiffOpKind::Equal => {
                    let differs = match (o.a_index.and_then(|i| a.get(i)), o.b_index.and_then(|j| b.get(j))) {
                        (Some(x), Some(y)) => x.format_key != y.format_key,
                        _ => false,
                    };
                    if differs { OpKind::Format } else { OpKind::Equal }
                }
                DiffOpKind::Delete => OpKind::Delete,
                DiffOpKind::Insert => OpKind::Insert,
            };
            (kind, o.a_index, o.b_index)
        })
        .collect();
    let mut runs = Vec::new();
    let (mut i, mut a_pos, mut b_pos) = (0usize, 0usize, 0usize);
    while i < tagged.len() {
        let Some(&(kind, _, _)) = tagged.get(i) else { break };
        let mut j = i + 1;
        while tagged.get(j).is_some_and(|t| t.0 == kind) {
            j += 1;
        }
        let slice = tagged.get(i..j).unwrap_or(&[]);
        let a_idxs: Vec<usize> = slice.iter().filter_map(|t| t.1).collect();
        let b_idxs: Vec<usize> = slice.iter().filter_map(|t| t.2).collect();
        let (na, nb) = (a_idxs.len(), b_idxs.len());
        runs.push(OpRun { kind, a_idxs, b_idxs, a_start: a_pos + 1, b_start: b_pos + 1 });
        a_pos += na;
        b_pos += nb;
        i = j;
    }
    runs
}

fn range_of(start: usize, len: usize) -> BlockRange {
    BlockRange { start, end: (start + len).saturating_sub(1) }
}

fn plain_edit(kind: DocEditKind, a: BlockRange, b: BlockRange, a_blocks: Vec<usize>, b_blocks: Vec<usize>) -> DocEdit {
    DocEdit { kind, a, b, a_blocks, b_blocks, format_only: false, importance: None, intra: None, table_rows: None }
}

fn runs_to_edits(runs: Vec<OpRun>) -> Vec<DocEdit> {
    let mut edits = Vec::new();
    let mut iter = runs.into_iter().peekable();
    while let Some(r) = iter.next() {
        // folds delete + insert (either order) into `change`
        let fold = matches!(
            (r.kind, iter.peek().map(|n| n.kind)),
            (OpKind::Delete, Some(OpKind::Insert)) | (OpKind::Insert, Some(OpKind::Delete))
        );
        if fold && let Some(next) = iter.next() {
            let (del, ins) = if r.kind == OpKind::Delete { (r, next) } else { (next, r) };
            edits.push(plain_edit(
                DocEditKind::Change,
                range_of(del.a_start, del.a_idxs.len()),
                range_of(ins.b_start, ins.b_idxs.len()),
                del.a_idxs,
                ins.b_idxs,
            ));
            continue;
        }
        match r.kind {
            OpKind::Equal | OpKind::Format => {
                let mut e = plain_edit(
                    if r.kind == OpKind::Format { DocEditKind::Change } else { DocEditKind::Equal },
                    range_of(r.a_start, r.a_idxs.len()),
                    range_of(r.b_start, r.b_idxs.len()),
                    r.a_idxs,
                    r.b_idxs,
                );
                if r.kind == OpKind::Format {
                    e.format_only = true;
                    e.importance = Some(Importance::Minor);
                }
                edits.push(e);
            }
            OpKind::Delete => {
                let a = range_of(r.a_start, r.a_idxs.len());
                edits.push(plain_edit(DocEditKind::Delete, a, BlockRange { start: r.b_start, end: r.b_start - 1 }, r.a_idxs, Vec::new()));
            }
            OpKind::Insert => {
                let b = range_of(r.b_start, r.b_idxs.len());
                edits.push(plain_edit(DocEditKind::Insert, BlockRange { start: r.a_start, end: r.a_start - 1 }, b, Vec::new(), r.b_idxs));
            }
        }
    }
    edits
}

// ─────────────── word refinement inside a paragraph ───────────────

/// The marks of a pair of texts; `[]` when there is no useful refinement (equal texts, or no word in
/// common) or the word diff is over the limit (`budget`, `trace_cells`).
#[must_use]
pub fn intra_word_marks(a_text: &[u16], b_text: &[u16], pair_offset: usize, budget: &Budget, trace_cells: usize) -> Vec<DocIntraEdit> {
    if a_text == b_text {
        return Vec::new();
    }
    let a_toks = word_tokens(a_text);
    let b_toks = word_tokens(b_text);
    let eq = |i: usize, j: usize| -> bool {
        let (Some(&(s1, e1)), Some(&(s2, e2))) = (a_toks.get(i), b_toks.get(j)) else { return false };
        a_text.get(s1..e1) == b_text.get(s2..e2)
    };
    let Some(ops) = myers_diff(a_toks.len(), b_toks.len(), &eq, budget, trace_cells) else {
        budget.mark_fallback();
        return Vec::new();
    };
    let mut marks: Vec<DocIntraEdit> = Vec::new();
    let mut has_equal = false;
    for op in &ops {
        match op.op {
            DiffOpKind::Equal => {
                // only a real word anchors the refinement: space in common between texts that are
                // entirely different would mark "everything changed", word by word
                if let Some(&(s, e)) = op.a_index.and_then(|i| a_toks.get(i))
                    && has_non_space(a_text.get(s..e).unwrap_or(&[]))
                {
                    has_equal = true;
                }
            }
            DiffOpKind::Delete => {
                if let Some(&(s, e)) = op.a_index.and_then(|i| a_toks.get(i)) {
                    marks.push(DocIntraEdit { kind: IntraKind::Delete, side: Side::A, pair_offset, start_ch: s, end_ch: e });
                }
            }
            DiffOpKind::Insert => {
                if let Some(&(s, e)) = op.b_index.and_then(|j| b_toks.get(j)) {
                    marks.push(DocIntraEdit { kind: IntraKind::Insert, side: Side::B, pair_offset, start_ch: s, end_ch: e });
                }
            }
        }
    }
    // without a word in common the highlight would be "all marked": noise
    if !has_equal {
        return Vec::new();
    }
    // coalesces adjacent marks of the same side (word + space + word)
    let mut merged: Vec<DocIntraEdit> = Vec::new();
    for mk in marks {
        if let Some(last) = merged.last_mut()
            && last.side == mk.side
            && last.kind == mk.kind
            && last.end_ch == mk.start_ch
        {
            last.end_ch = mk.end_ch;
        } else {
            merged.push(mk);
        }
    }
    merged
}

// ─────────────── nested diff of the rows of a table ───────────────

struct Env<'a> {
    media_a: &'a MediaStore,
    media_b: &'a MediaStore,
    budget: &'a Budget,
    limits: DiffLimits,
}

fn table_row_diff(a: &DiffBlock<'_>, b: &DiffBlock<'_>, env: &Env<'_>, interner: &mut Interner) -> Option<Vec<TableRowEdit>> {
    let (DiffBlock::Table { table: ta, final_cells: fa }, DiffBlock::Table { table: tb, final_cells: fb }) = (a, b) else { return None };
    let a_keys: Vec<KeyId> = ta.rows.iter().map(|r| interner.intern(&row_content_key(r, *fa, env.media_a))).collect();
    let b_keys: Vec<KeyId> = tb.rows.iter().map(|r| interner.intern(&row_content_key(r, *fb, env.media_b))).collect();
    env.budget.spend(u64::try_from(a_keys.len() + b_keys.len()).unwrap_or(u64::MAX));
    let ops = {
        let eq = |i: usize, j: usize| a_keys.get(i) == b_keys.get(j);
        histogram_diff(
            a_keys.len(),
            b_keys.len(),
            &eq,
            &|i| a_keys.get(i).map_or(u32::MAX, |k| k.0),
            &|j| b_keys.get(j).map_or(u32::MAX - 1, |k| k.0),
            env.budget,
            env.limits.myers_trace_cells,
        )
    };
    let tagged: Vec<(OpKind, Option<usize>, Option<usize>)> = ops
        .iter()
        .map(|o| {
            let kind = match o.op {
                DiffOpKind::Equal => {
                    let differs = match (o.a_index.and_then(|i| ta.rows.get(i)), o.b_index.and_then(|j| tb.rows.get(j))) {
                        (Some(x), Some(y)) => row_format_key(x, *fa, env.media_a) != row_format_key(y, *fb, env.media_b),
                        _ => false,
                    };
                    if differs { OpKind::Format } else { OpKind::Equal }
                }
                DiffOpKind::Delete => OpKind::Delete,
                DiffOpKind::Insert => OpKind::Insert,
            };
            (kind, o.a_index, o.b_index)
        })
        .collect();
    // runs + fold delete/insert -> change, mirroring the block level
    let mut out: Vec<TableRowEdit> = Vec::new();
    let mut i = 0;
    while i < tagged.len() {
        let Some(&(op, _, _)) = tagged.get(i) else { break };
        let mut j = i + 1;
        while tagged.get(j).is_some_and(|t| t.0 == op) {
            j += 1;
        }
        let slice = tagged.get(i..j).unwrap_or(&[]);
        let a_rows: Vec<usize> = slice.iter().filter_map(|t| t.1).collect();
        let b_rows: Vec<usize> = slice.iter().filter_map(|t| t.2).collect();
        let fold_ins = op == OpKind::Insert && out.last().is_some_and(|l| l.kind == DocEditKind::Delete && l.b_rows.is_empty());
        let fold_del = op == OpKind::Delete && out.last().is_some_and(|l| l.kind == DocEditKind::Insert && l.a_rows.is_empty());
        if fold_ins && let Some(last) = out.last_mut() {
            last.kind = DocEditKind::Change;
            last.b_rows = b_rows;
        } else if fold_del && let Some(last) = out.last_mut() {
            last.kind = DocEditKind::Change;
            last.a_rows = a_rows;
        } else {
            out.push(TableRowEdit {
                kind: match op {
                    OpKind::Format => DocEditKind::Change,
                    OpKind::Equal => DocEditKind::Equal,
                    OpKind::Delete => DocEditKind::Delete,
                    OpKind::Insert => DocEditKind::Insert,
                },
                a_rows,
                b_rows,
                format_only: op == OpKind::Format,
            });
        }
        i = j;
    }
    Some(out)
}

/// `paraText(p)`: the runs' text, in code units.
fn para_text(p: &Paragraph) -> Vec<u16> {
    let mut out = Vec::with_capacity(p.text_len());
    for r in &p.runs {
        out.extend_from_slice(r.text.units());
    }
    out
}

fn paragraph_of<'x>(b: &'x DiffBlock<'_>) -> Option<&'x Paragraph> {
    match b {
        DiffBlock::Paragraph(p) => Some(match p {
            Cow::Borrowed(p) => p,
            Cow::Owned(p) => p,
        }),
        DiffBlock::Table { .. } => None,
    }
}

// ─────────────── public entry ───────────────

/// `diffBlocks(aBlocks, bBlocks, opts)` with the default limits: `media_a` and `media_b` hold the
/// bytes of the images of each side.
#[must_use]
pub fn diff_blocks(
    a_blocks: &[DiffBlock<'_>],
    b_blocks: &[DiffBlock<'_>],
    media_a: &MediaStore,
    media_b: &MediaStore,
    opts: &DocDiffOptions,
) -> DocDiff {
    diff_blocks_with_limits(a_blocks, b_blocks, media_a, media_b, opts, &DiffLimits::default())
}

/// [`diff_blocks`] with explicit limits.
#[must_use]
pub fn diff_blocks_with_limits(
    a_blocks: &[DiffBlock<'_>],
    b_blocks: &[DiffBlock<'_>],
    media_a: &MediaStore,
    media_b: &MediaStore,
    opts: &DocDiffOptions,
    limits: &DiffLimits,
) -> DocDiff {
    let mut interner = Interner::default();
    let a_tokens = tokenize_blocks(a_blocks, media_a, &mut interner);
    let b_tokens = tokenize_blocks(b_blocks, media_b, &mut interner);
    let budget = Budget::new(limits.work);
    let intra_budget = Budget::new(limits.intra_work);

    let eq = |i: usize, j: usize| -> bool {
        let (Some(x), Some(y)) = (a_tokens.get(i), b_tokens.get(j)) else { return false };
        let (equal, cost) = tokens_equal_cost(x, y);
        if cost > 0 {
            budget.spend(cost / 256);
        }
        equal
    };
    let ops = match opts.algorithm {
        DiffAlgorithm::Myers => match myers_diff(a_tokens.len(), b_tokens.len(), &eq, &budget, limits.myers_trace_cells) {
            Some(ops) => ops,
            None => {
                budget.mark_fallback();
                let mut ops = Vec::new();
                super::myers::coarse_script(0, a_tokens.len(), 0, b_tokens.len(), &eq, &mut ops);
                ops
            }
        },
        DiffAlgorithm::Histogram => histogram_diff(
            a_tokens.len(),
            b_tokens.len(),
            &eq,
            &|i| a_tokens.get(i).map_or(u32::MAX, |t| t.content_key.0),
            &|j| b_tokens.get(j).map_or(u32::MAX - 1, |t| t.content_key.0),
            &budget,
            limits.myers_trace_cells,
        ),
    };

    let mut edits = runs_to_edits(ops_to_runs(&ops, &a_tokens, &b_tokens));

    // enrichment of the `change` edits: word level and the rows of tables, pair by pair (the i-th
    // block of A with the i-th of B, as Beyond Compare does with the lines of a hunk)
    let env = Env { media_a, media_b, budget: &budget, limits: *limits };
    for e in &mut edits {
        if e.kind != DocEditKind::Change || e.format_only {
            continue;
        }
        let pairs = e.a_blocks.len().min(e.b_blocks.len());
        let mut intra: Vec<DocIntraEdit> = Vec::new();
        let mut table_rows: Vec<Option<Vec<TableRowEdit>>> = Vec::new();
        let mut any_table = false;
        for p in 0..pairs {
            let (Some(a_b), Some(b_b)) =
                (e.a_blocks.get(p).and_then(|&i| a_blocks.get(i)), e.b_blocks.get(p).and_then(|&j| b_blocks.get(j)))
            else {
                continue;
            };
            if let (Some(pa), Some(pb)) = (paragraph_of(a_b), paragraph_of(b_b)) {
                if opts.intra_word {
                    intra.extend(intra_word_marks(&para_text(pa), &para_text(pb), p, &intra_budget, limits.intra_trace_cells));
                }
                table_rows.push(None);
            } else {
                let rd = table_row_diff(a_b, b_b, &env, &mut interner);
                if rd.is_some() {
                    any_table = true;
                }
                table_rows.push(rd);
            }
        }
        if !intra.is_empty() {
            e.intra = Some(intra);
        }
        if any_table {
            e.table_rows = Some(table_rows);
        }
    }

    // statistics by block
    let mut stats = DocDiffStats::default();
    for e in &edits {
        match e.kind {
            DocEditKind::Equal => stats.equal += e.a_blocks.len(),
            DocEditKind::Insert => stats.inserted += e.b_blocks.len(),
            DocEditKind::Delete => stats.deleted += e.a_blocks.len(),
            DocEditKind::Change if e.format_only => stats.format_only += e.a_blocks.len(),
            DocEditKind::Change => stats.changed += e.a_blocks.len().max(e.b_blocks.len()),
        }
    }
    DocDiff { edits, stats, limited: budget.fell_back() || intra_budget.fell_back() }
}
