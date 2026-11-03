//! A story in the editor's flat shape (architecture A2, G3): paragraphs, plus tables anchored
//! "before paragraph N". The body, every table cell, every header/footer part, every note and
//! every textbox is a `Story`. The TS block order is derived ([`Story::blocks`]: the anchors of
//! paragraph N before it, in anchor order; anchors at or past the end after the last paragraph),
//! so block indices (section ranges, bookmark blocks, layout refs) are the TS ones.

use std::sync::Arc;

use crate::js::{JsValue, JsonWriter};
use crate::pvec::PVec;

use super::para::Paragraph;
use super::table::Table;

/// A table and the paragraph it comes before (`before_para == paras.len()`: after the last).
#[derive(Debug, Clone, PartialEq)]
pub struct TableAnchor {
    pub before_para: u32,
    pub table: Arc<Table>,
}

/// One story (see the module comment). `tables` is in TS anchor order (non-decreasing
/// `before_para`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Story {
    pub paras: PVec<Paragraph>,
    pub tables: PVec<TableAnchor>,
}

/// A block of a story in TS order.
#[derive(Debug, Clone, Copy)]
pub enum Block<'a> {
    Paragraph(&'a Paragraph),
    Table(&'a Table),
}

/// The blocks of a story in TS order.
pub struct Blocks<'a> {
    story: &'a Story,
    para: usize,
    table: usize,
}

impl<'a> Iterator for Blocks<'a> {
    type Item = Block<'a>;

    fn next(&mut self) -> Option<Block<'a>> {
        if let Some(a) = self.story.tables.get(self.table)
            && (a.before_para as usize <= self.para || self.para >= self.story.paras.len())
        {
            self.table += 1;
            return Some(Block::Table(&a.table));
        }
        let p = self.story.paras.get(self.para)?;
        self.para += 1;
        Some(Block::Paragraph(p))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.story.paras.len() + self.story.tables.len() - self.para - self.table;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Blocks<'_> {}

impl Story {
    #[must_use]
    pub fn new() -> Story {
        Story::default()
    }

    /// The blocks in TS order.
    #[must_use]
    pub fn blocks(&self) -> Blocks<'_> {
        Blocks { story: self, para: 0, table: 0 }
    }

    /// The number of blocks.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.paras.len() + self.tables.len()
    }

    /// Block `i` in TS order (O(i)).
    #[must_use]
    pub fn block_at(&self, i: usize) -> Option<Block<'_>> {
        self.blocks().nth(i)
    }

    /// Appends a paragraph after every block so far (building in document order).
    pub fn push_paragraph(&mut self, p: Paragraph) {
        self.paras.push(p);
    }

    /// Appends a table after every block so far (anchored before the next paragraph).
    pub fn push_table(&mut self, t: Arc<Table>) {
        let before_para = u32::try_from(self.paras.len()).unwrap_or(u32::MAX);
        self.tables.push(TableAnchor { before_para, table: t });
    }

    /// Appends a block (building in document order).
    pub fn push_block(&mut self, b: OwnedBlock) {
        match b {
            OwnedBlock::Paragraph(p) => self.push_paragraph(p),
            OwnedBlock::Table(t) => self.push_table(t),
        }
    }

    /// Whether the story has at least one paragraph (`blocks.some(b => b.type === 'paragraph')`).
    #[must_use]
    pub fn has_paragraph(&self) -> bool {
        !self.paras.is_empty()
    }

    /// The paragraphs of the blocks with those of the table cells (nested tables included, not
    /// textboxes), in document order: the TS `walk` of `textboxBlocks` and
    /// `readHeaderFooterPart` (the flat editing view of a textbox or header). Iterative.
    #[must_use]
    pub fn flat_paragraphs(&self) -> Vec<&Paragraph> {
        let mut out = Vec::new();
        let mut stack: Vec<Blocks<'_>> = vec![self.blocks()];
        while let Some(top) = stack.last_mut() {
            match top.next() {
                None => {
                    stack.pop();
                }
                Some(Block::Paragraph(p)) => out.push(p),
                Some(Block::Table(t)) => {
                    // cells in reverse so that the first cell's blocks come out first
                    for row in t.rows.iter().rev() {
                        for cell in row.cells.iter().rev() {
                            stack.push(cell.content.blocks());
                        }
                    }
                }
            }
        }
        out
    }

    /// The top-level paragraphs as a TS array (`Paragraph[]`).
    #[cfg(feature = "canon")]
    pub fn canon_paragraphs(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.array(self.paras.len());
        for p in self.paras.iter() {
            p.canon(w);
        }
    }
}

/// An owned block (reader output before it is placed in a story).
#[derive(Debug, Clone)]
pub enum OwnedBlock {
    Paragraph(Paragraph),
    Table(Arc<Table>),
}

/// A story's blocks as the TS `Block[]`: `{ type: 'paragraph', paragraph }` or the table.
impl JsValue for Story {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.array(self.block_count());
        for b in self.blocks() {
            match b {
                Block::Paragraph(p) => {
                    crate::canon::canon_entries(
                        w,
                        &mut [("type", &|w: &mut crate::canon::CanonWriter<'_>| w.str("paragraph")), ("paragraph", &|w| p.canon(w))],
                    );
                }
                Block::Table(t) => t.canon(w),
            }
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        BlocksJson(self).json(w)
    }
}

/// `JSON.stringify` of a story's `Block[]`.
pub struct BlocksJson<'a>(pub &'a Story);

struct ParaBlockJson<'a>(&'a Paragraph);

impl JsValue for ParaBlockJson<'_> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        self.0.canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.begin_object();
        w.member("type", &"paragraph");
        w.member("paragraph", self.0);
        w.end_object();
        true
    }
}

impl JsValue for BlocksJson<'_> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        self.0.canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        let items: Vec<Box<dyn JsValue + '_>> = self
            .0
            .blocks()
            .map(|b| -> Box<dyn JsValue + '_> {
                match b {
                    Block::Paragraph(p) => Box::new(ParaBlockJson(p)),
                    Block::Table(t) => Box::new(TableRef(t)),
                }
            })
            .collect();
        w.array(items.iter().map(|b| &**b as &dyn JsValue));
        true
    }
}

struct TableRef<'a>(&'a Table);

impl JsValue for TableRef<'_> {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        self.0.canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        self.0.json(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ParaId, TableId};

    fn table(id: u32) -> Arc<Table> {
        Arc::new(Table { id: TableId(id), ..Table::default() })
    }

    #[test]
    fn block_order_is_the_ts_order() {
        let mut s = Story::new();
        s.push_table(table(1));
        s.push_paragraph(Paragraph::empty(ParaId(10)));
        s.push_table(table(2));
        s.push_table(table(3));
        s.push_paragraph(Paragraph::empty(ParaId(11)));
        s.push_table(table(4));
        let order: Vec<String> = s
            .blocks()
            .map(|b| match b {
                Block::Paragraph(p) => format!("p{}", p.id.0),
                Block::Table(t) => format!("t{}", t.id.0),
            })
            .collect();
        assert_eq!(order, ["t1", "p10", "t2", "t3", "p11", "t4"]);
        assert_eq!(s.blocks().len(), 6);
        assert!(matches!(s.block_at(3), Some(Block::Table(t)) if t.id.0 == 3));
    }
}
