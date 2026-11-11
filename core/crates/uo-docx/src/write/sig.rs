//! The baseline signatures of the lossless save (source.ts:141–166, 239–318; G23, U6).
//!
//! The TS takes, right after the editor installs the document (`SourceStore.baseline()`), a
//! 53-bit hash (`cyrb53`) of `JSON.stringify` of every source paragraph's props and runs and of
//! every source table; at save, a paragraph or table whose hashes are unchanged is **pristine**
//! and goes out byte for byte (its raw source). Here:
//!
//! - [`sig_hash`] is `sigHash` (cyrb53 over the UTF-16 code units of the text, exact);
//! - [`SigMode`] chooses the text: [`SigMode::Canonical`] (the default, G23) writes the keys of
//!   every declared object sorted, so two objects that differ only in key insertion order sign
//!   the same; [`SigMode::Ts`] writes them in insertion order, the TS text itself (for the U6
//!   harness, which counts the steps where the two verdicts differ). Media bytes are written as
//!   `"#bytes:<length>"` in both (`sigReplacer`);
//! - [`Baseline`] holds the signatures by [`SourceKey`] (the TS keeps them on the
//!   `ParaSource`/`TableSource`, which here are immutable after the read: A2);
//! - [`Pristine`] answers `pristine(p)` / `tablePristine(t)` with the per-save memo of the TS
//!   (`memo`, a `WeakMap` by object: here by the address of the borrowed node, which is stable for
//!   the whole save because the writer borrows the document).
//!
//! Signatures are compared only with signatures computed by the same code (baseline vs save), so
//! the Rust values never need to equal the TS values; [`SigMode::Ts`] does reproduce them for an
//! object whose recorded key order is the TS one (tested on literals).

use std::collections::HashMap;
use std::sync::Arc;

use crate::ids::SourceKey;
use crate::js::{JsValue, JsonWriter, SigEncoding};
use crate::model::Document;
use crate::model::media::MediaStore;
use crate::model::para::Paragraph;
use crate::model::run::Run;
use crate::model::sources::SourceStore;
use crate::model::story::{Block, Story};
use crate::model::table::Table;

/// `sigHash` (cyrb53) of a string, over its UTF-16 code units.
#[must_use]
pub fn sig_hash(s: &str) -> u64 {
    let mut st = Cyrb53::new();
    for u in s.encode_utf16() {
        st.push(u);
    }
    st.finish()
}

/// `sigHash` over UTF-16 code units (lone surrogates included).
#[must_use]
pub fn sig_hash_units(units: &[u16]) -> u64 {
    let mut st = Cyrb53::new();
    for &u in units {
        st.push(u);
    }
    st.finish()
}

/// The cyrb53 state: `Math.imul` is a wrapping `u32` multiplication of the bit patterns, `^` and
/// `>>>` work on the same patterns.
struct Cyrb53 {
    h1: u32,
    h2: u32,
}

impl Cyrb53 {
    fn new() -> Cyrb53 {
        Cyrb53 { h1: 0xdead_beef, h2: 0x41c6_ce57 }
    }

    fn push(&mut self, c: u16) {
        let c = u32::from(c);
        self.h1 = (self.h1 ^ c).wrapping_mul(2_654_435_761);
        self.h2 = (self.h2 ^ c).wrapping_mul(1_597_334_677);
    }

    fn finish(self) -> u64 {
        let Cyrb53 { mut h1, mut h2 } = self;
        h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507) ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
        h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507) ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
        // 4294967296 * (2097151 & h2) + (h1 >>> 0): exact in f64 (53 bits), so exact as u64
        (u64::from(h2 & 0x001f_ffff) << 32) | u64::from(h1)
    }
}

/// Which text the signatures hash (G23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SigMode {
    /// declared objects with their keys sorted: order-insensitive (G23, the default)
    #[default]
    Canonical,
    /// `JSON.stringify` in key insertion order, as the TS (the U6 harness)
    Ts,
}

impl SigMode {
    fn encoding(self) -> SigEncoding {
        SigEncoding { sorted_keys: self == SigMode::Canonical }
    }
}

/// The signature text of a value: `JSON.stringify(v, sigReplacer)` in `mode`.
#[must_use]
pub fn sig_text(v: &dyn JsValue, media: Option<&MediaStore>, mode: SigMode) -> String {
    let mut w = JsonWriter::for_signature(media, mode.encoding());
    if v.json(&mut w) { w.finish() } else { String::new() }
}

/// `runSig(run)`.
#[must_use]
pub fn run_sig(run: &Run, media: Option<&MediaStore>, mode: SigMode) -> u64 {
    sig_hash(&sig_text(run, media, mode))
}

/// `propsSig(p)`: the paragraph's props.
#[must_use]
pub fn props_sig(p: &Paragraph, media: Option<&MediaStore>, mode: SigMode) -> u64 {
    sig_hash(&sig_text(&*p.props, media, mode))
}

/// `tableSig(t)`: the whole table (rows, cells with their three views, as the TS object).
#[must_use]
pub fn table_sig(t: &Table, media: Option<&MediaStore>, mode: SigMode) -> u64 {
    sig_hash(&sig_text(t, media, mode))
}

/// `ParaSource.sig`: the paragraph as installed.
#[derive(Debug, Clone, PartialEq)]
pub struct ParaSig {
    pub props: u64,
    /// `r.props.src ?? 0` of each run
    pub sids: Box<[u32]>,
    pub runs: Box<[u64]>,
    /// the node as installed: a node still behind the same `Arc` at save is unchanged (copy-on-
    /// write), so the signatures need not be computed again (A1.3: `Arc::ptr_eq` as a
    /// content-unchanged fast path, never identity). Shared, not copied; it keeps the installed
    /// version alive after an edit (at most one version per source node).
    pub node: Option<Arc<Paragraph>>,
}

/// The baseline of a document (`SourceStore.baseline()`): taken once after the install, kept by
/// the session for every later save.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Baseline {
    pub mode: SigMode,
    pub paras: HashMap<SourceKey, ParaSig>,
    /// the table signature and the table as installed (the same fast path)
    pub tables: HashMap<SourceKey, (u64, Arc<Table>)>,
}

impl Baseline {
    /// `baseline()`: the signatures of every paragraph and table of `doc` that carries a source
    /// key known to the store (the TS `paraRefs`/`tableRefs`: the nodes the reader registered),
    /// in every story — body, cells, headers and footers, notes, textboxes (group items
    /// included). Iterative (G20). A key met twice keeps the last node in this walk's order
    /// (the TS keeps the last in read order; the reader never gives two nodes one key).
    #[must_use]
    pub fn take(doc: &Document, mode: SigMode) -> Baseline {
        let mut b = Baseline { mode, ..Baseline::default() };
        let Some(store) = doc.sources.as_deref() else { return b };
        let media = Some(&doc.media);
        for_each_node(doc, &mut |n| match n {
            Node::Para(p) => {
                if let Some(k) = p.props.src
                    && store.paras.contains_key(&k)
                {
                    let mut sig = para_sig(p, media, mode);
                    sig.node = Some(Arc::clone(p));
                    b.paras.insert(k, sig);
                }
            }
            Node::Table(t) => {
                if let Some(k) = t.src
                    && store.tables.contains_key(&k)
                {
                    b.tables.insert(k, (table_sig(t, media, mode), Arc::clone(t)));
                }
            }
        });
        b
    }

    /// The baseline over a store's keys only (a document without sources has none).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paras.is_empty() && self.tables.is_empty()
    }
}

/// The signature of a paragraph as the baseline records it.
#[must_use]
pub fn para_sig(p: &Paragraph, media: Option<&MediaStore>, mode: SigMode) -> ParaSig {
    ParaSig {
        props: props_sig(p, media, mode),
        sids: p.runs.iter().map(|r| r.props.src.map_or(0, |k| k.0)).collect(),
        runs: p.runs.iter().map(|r| run_sig(r, media, mode)).collect(),
        node: None,
    }
}

/// A node met by [`for_each_node`].
pub enum Node<'a> {
    Para(&'a Arc<Paragraph>),
    Table(&'a Arc<Table>),
}

/// Every paragraph and table of the document, stories after stories (body, header/footer parts,
/// footnotes, endnotes), each story in block order with its cells and the textboxes of its
/// runs. Iterative: an explicit stack of stories.
pub fn for_each_node<'a>(doc: &'a Document, f: &mut dyn FnMut(Node<'a>)) {
    let mut stories: Vec<&'a Story> = Vec::new();
    stories.push(&doc.body);
    for part in doc.hf.iter() {
        stories.push(&part.story);
    }
    for s in doc.notes.footnotes.values() {
        stories.push(s);
    }
    for s in doc.notes.endnotes.values() {
        stories.push(s);
    }
    // keep the order above: process the first story first
    stories.reverse();
    while let Some(story) = stories.pop() {
        // the paragraphs, then the tables (a sum per key: the order only decides between two
        // nodes with one key, which the reader never gives)
        let mut nested: Vec<&'a Story> = Vec::new();
        for p in story.paras.iter_arcs() {
            f(Node::Para(p));
            textbox_stories(&p.runs, &mut nested);
        }
        for a in story.tables.iter() {
            let t = &a.table;
            f(Node::Table(t));
            for row in &t.rows {
                for cell in &row.cells {
                    nested.push(&cell.content);
                }
            }
        }
        nested.reverse();
        stories.extend(nested);
    }
}

/// The textbox stories of runs: a run's textbox, and the textboxes of a group's items (groups
/// nested in groups included), in order.
pub(crate) fn textbox_stories<'a>(runs: &'a [Run], out: &mut Vec<&'a Story>) {
    let mut stack: Vec<&'a Run> = runs.iter().rev().collect();
    while let Some(r) = stack.pop() {
        let Some(ext) = r.ext() else { continue };
        if let Some(tb) = ext.textbox.as_deref() {
            out.push(&tb.content);
        }
        if let Some(g) = ext.group.as_deref() {
            for it in g.items.iter().rev() {
                stack.push(&it.run);
            }
        }
    }
}

/// `pristine(p)` / `tablePristine(t)` during one save, with the TS per-save memo.
pub struct Pristine<'a> {
    baseline: &'a Baseline,
    media: Option<&'a MediaStore>,
    paras: HashMap<usize, bool>,
    tables: HashMap<usize, bool>,
}

impl<'a> Pristine<'a> {
    /// `beginSave()`: an empty memo.
    #[must_use]
    pub fn new(baseline: &'a Baseline, media: Option<&'a MediaStore>) -> Pristine<'a> {
        Pristine { baseline, media, paras: HashMap::new(), tables: HashMap::new() }
    }

    /// The baseline the answers come from.
    #[must_use]
    pub fn baseline(&self) -> &'a Baseline {
        self.baseline
    }

    /// `pristine(p)`: same runs (same source keys, same signatures) and same props as the
    /// baseline. A run without a source key never matches (`undefined !== 0` in the TS, even
    /// where the baseline recorded `0`).
    pub fn para(&mut self, p: &Paragraph) -> bool {
        let key = std::ptr::from_ref(p).addr();
        if let Some(&v) = self.paras.get(&key) {
            return v;
        }
        let v = self.compute_para(p);
        self.paras.insert(key, v);
        v
    }

    fn compute_para(&self, p: &Paragraph) -> bool {
        let mode = self.baseline.mode;
        let Some(s) = p.props.src.and_then(|k| self.baseline.paras.get(&k)) else { return false };
        if s.node.as_deref().is_some_and(|n| std::ptr::eq(n, p)) {
            return true;
        }
        if p.runs.len() != s.sids.len() {
            return false;
        }
        for (i, r) in p.runs.iter().enumerate() {
            let same_src = matches!((r.props.src, s.sids.get(i)), (Some(k), Some(&sid)) if k.0 == sid);
            if !same_src || Some(&run_sig(r, self.media, mode)) != s.runs.get(i) {
                return false;
            }
        }
        props_sig(p, self.media, mode) == s.props
    }

    /// `tablePristine(t)`.
    pub fn table(&mut self, t: &Table) -> bool {
        let key = std::ptr::from_ref(t).addr();
        if let Some(&v) = self.tables.get(&key) {
            return v;
        }
        let v = t
            .src
            .and_then(|k| self.baseline.tables.get(&k))
            .is_some_and(|(s, node)| std::ptr::eq(&**node, t) || table_sig(t, self.media, self.baseline.mode) == *s);
        self.tables.insert(key, v);
        v
    }
}

/// The source key of a block (`blockSrc` / `blockId`).
#[must_use]
pub fn block_src(b: Block<'_>) -> Option<SourceKey> {
    match b {
        Block::Paragraph(p) => p.props.src,
        Block::Table(t) => t.src,
    }
}

/// Whether a store knows a paragraph key (helper for the writers).
#[must_use]
pub fn has_para(store: &SourceStore, k: SourceKey) -> bool {
    store.paras.contains_key(&k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_signatures_ignore_key_order_and_ts_ones_do_not() {
        use crate::js::JsObject;
        use crate::model::run::{Run, RunProps};
        let mut a = RunProps { bold: Some(true), color: Some("FF0000".into()), ..RunProps::default() };
        a.mark("bold");
        a.mark("color");
        let mut b = RunProps { bold: Some(true), color: Some("FF0000".into()), ..RunProps::default() };
        b.mark("color");
        b.mark("bold");
        let ra = Run { text: "x".into(), props: std::sync::Arc::new(a), ..Run::default() };
        let rb = Run { text: "x".into(), props: std::sync::Arc::new(b), ..Run::default() };
        assert_eq!(run_sig(&ra, None, SigMode::Canonical), run_sig(&rb, None, SigMode::Canonical));
        assert_ne!(run_sig(&ra, None, SigMode::Ts), run_sig(&rb, None, SigMode::Ts));
        // the TS text of the first one
        assert_eq!(sig_text(&ra, None, SigMode::Ts), "{\"text\":\"x\",\"props\":{\"bold\":true,\"color\":\"FF0000\"}}");
        // and its value is the TS `runSig` of the same object (Node 24.15)
        assert_eq!(run_sig(&ra, None, SigMode::Ts), 6_353_159_028_267_010);
    }

    #[test]
    fn cyrb53_matches_v8() {
        // values computed by the TS `sigHash` in Node 24.15
        assert_eq!(sig_hash(""), 3_338_908_027_751_811);
        assert_eq!(sig_hash("a"), 7_929_297_801_672_961);
        assert_eq!(sig_hash("abc"), 5_059_922_895_146_125);
        assert_eq!(sig_hash("{\"text\":\"x\",\"props\":{\"bold\":true}}"), 2_719_737_140_849_267);
        let units: Vec<u16> = vec![0xe9, 0xd83d, 0xde00, 0xd800, u16::from(b'x')];
        assert_eq!(sig_hash_units(&units), 3_068_794_784_086_284);
    }
}
