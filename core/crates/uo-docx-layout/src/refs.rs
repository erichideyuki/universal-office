//! From the lines of a layout back to the model (frontier F8): `refs.ts` (`cellAtGrid`,
//! `cellBlocks`, `paragraphAtPath` — ported once, in `uo_docx::word::refs`) and the editor's
//! `locateRef` + `lineParagraph` (universal-write.ts:16540, engine-doc.ts:305) with `hfBlocksFor`
//! (engine-doc.ts:358) and `noteBlocks` (notes.ts:89).
//!
//! A line ref names a story and a path as the TS writes it (a table cell by its GRID column,
//! F0-5); the editor turns it into a location kind (`body`, `cell`, `hf`, `note`, `other`) and the
//! paragraph object the line belongs to. Here the paragraph is a [`ParaLoc`] (story + model
//! [`BlockPath`], a cell by its index in the row), and the kind is kept because the editor filters
//! on it (`engLineAt` skips `other`, not "no paragraph").

use crate::model::{Block, BlockPath, DocView, HfWhich, NumKey, Paragraph, Story, StoryRef};
use crate::output::{HfSlot, LineRef, LineStory};

/// A paragraph of the model: the TS `Paragraph` object a line resolves to (identity = equality
/// of story and path).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ParaLoc {
    pub story: StoryRef,
    pub path: BlockPath,
}

/// `LineLoc.kind` of `locateRef`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocKind {
    /// a top-level body paragraph (`b<n>` over a paragraph origin)
    Body,
    /// a paragraph in a body table (`b<n>/r/c/b…` over a table origin, every hop well formed)
    Cell,
    /// `hdr:<slot>` / `ftr:<slot>`
    HeaderFooter,
    /// `fn<id>` / `en<id>` (the id printed as digits)
    Note,
    /// anything else (`txbx`, `preview`, `itb`, `grp`, `ensep`, a malformed body path)
    Other,
}

/// Where a line is: its kind and its paragraph (`null` when the path does not resolve).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub kind: LocKind,
    pub para: Option<ParaLoc>,
}

impl Located {
    const OTHER: Located = Located { kind: LocKind::Other, para: None };
}

/// The blocks of the body, indexed once (`Story::block_at` is O(i); a layout locates every line).
pub struct Locator<'a> {
    pub view: DocView<'a>,
    body: Vec<Block<'a>>,
}

impl<'a> Locator<'a> {
    #[must_use]
    pub fn new(view: DocView<'a>) -> Locator<'a> {
        Locator { view, body: view.body().blocks().collect() }
    }

    /// The body block `i` (`full.blocks[i]`).
    #[must_use]
    pub fn body_block(&self, i: u32) -> Option<Block<'a>> {
        self.body.get(i as usize).copied()
    }

    /// Body blocks.
    #[must_use]
    pub fn body_len(&self) -> usize {
        self.body.len()
    }

    /// `paragraphAtPath(full.blocks, path)` over the indexed body blocks.
    #[must_use]
    pub fn body_path(&self, segs: &[u32]) -> Option<BlockPath> {
        walk_path(|b| self.body_block(b), segs)
    }

    /// `locateRef(ref, origins)` + `lineParagraph(loc, doc, section)` (`section` = the page's
    /// section, for `hfBlocksFor`).
    #[must_use]
    pub fn locate(&self, section: u32, at: &LineRef) -> Located {
        let segs: &[u32] = &at.path.0;
        match at.story {
            LineStory::Body => self.locate_body(segs),
            LineStory::HeaderFooter { which, slot, .. } => {
                let para = hf_story_for(self.view, section, which, slot)
                    .and_then(|(part, story)| path_in(story, segs).map(|path| ParaLoc { story: StoryRef::HeaderFooter(part), path }));
                Located { kind: LocKind::HeaderFooter, para }
            }
            LineStory::Note(kind, id) => {
                // `/^(fn|en)(\d+)$/`: the id as `String(id)` must be digits; `+digits` is the id
                if !digits_only(id.get()) {
                    return Located::OTHER;
                }
                let para = self
                    .view
                    .note_story(kind, NumKey::new(id.get()))
                    .and_then(|s| path_in(s, segs))
                    .map(|path| ParaLoc { story: StoryRef::Note(kind, NumKey::new(id.get())), path });
                Located { kind: LocKind::Note, para }
            }
            LineStory::Textbox(_) | LineStory::Preview | LineStory::EndnoteSep | LineStory::InlineTextbox | LineStory::GroupTextbox => {
                Located::OTHER
            }
        }
    }

    /// `locateRef` of a body ref (`origins` = the body blocks: a paragraph or a table anchor).
    fn locate_body(&self, segs: &[u32]) -> Located {
        let Some(&b) = segs.first() else { return Located::OTHER };
        let Some(origin) = self.body_block(b) else { return Located::OTHER };
        match origin {
            Block::Paragraph(_) => {
                if segs.len() == 1 {
                    Located { kind: LocKind::Body, para: Some(ParaLoc { story: StoryRef::Body, path: BlockPath::root(b) }) }
                } else {
                    Located::OTHER
                }
            }
            Block::Table(t) => {
                // complete (r, c, b) triples after the root; none = `other`
                let hops = (segs.len() - 1) / 3;
                if hops == 0 {
                    return Located::OTHER;
                }
                let para = cell_paragraph(t, b, segs.get(1..1 + hops * 3).unwrap_or(&[]));
                Located { kind: LocKind::Cell, para }
            }
        }
    }

    /// `cellBoxAt`'s `locateRef(box.ref + '/b0')` of a placed cell ref (`b3/r0/c1`): the number
    /// of hops when the kind is `cell`.
    #[must_use]
    pub fn cell_hops(&self, segs: &[u32]) -> Option<usize> {
        let b = *segs.first()?;
        match self.body_block(b)? {
            Block::Table(_) => {
                // the parts are the segments plus the appended `b0`
                let hops = segs.len() / 3;
                (hops > 0).then_some(hops)
            }
            Block::Paragraph(_) => None,
        }
    }
}

/// `String(id)` is a run of ASCII digits (`/^\d+$/`).
fn digits_only(id: f64) -> bool {
    id >= 0.0 && uo_jsnum::conv::is_integer(id) && id < 1e21
}

/// `resolveCell(table, hops)` then `cellBlocks(cell)[block]` as a paragraph: `segs` are complete
/// `(r, c_grid, b)` triples; every hop but the last enters a nested table at its `b`.
fn cell_paragraph(t: &crate::model::Table, root: u32, segs: &[u32]) -> Option<ParaLoc> {
    let mut table = t;
    let mut path = BlockPath::root(root);
    let mut i = 0;
    loop {
        let (&r, &c, &b) = (segs.get(i)?, segs.get(i + 1)?, segs.get(i + 2)?);
        let cell = uo_docx::word::refs::cell_at_grid(table, f64::from(r), f64::from(c))?;
        let k = table.rows.get(r as usize)?.cells.iter().position(|x| std::ptr::eq(x, cell))?;
        path = path.cell_block(r, u32::try_from(k).ok()?, b);
        let blk = cell.content.block_at(b as usize)?;
        if i + 3 >= segs.len() {
            return match blk {
                Block::Paragraph(_) => Some(ParaLoc { story: StoryRef::Body, path }),
                Block::Table(_) => None,
            };
        }
        let Block::Table(nt) = blk else { return None };
        table = nt;
        i += 3;
    }
}

/// `paragraphAtPath(blocks, path)` (refs.ts:22) over a story, returning the model path (cells by
/// their index in the row).
#[must_use]
pub fn path_in(story: &Story, segs: &[u32]) -> Option<BlockPath> {
    walk_path(|b| story.block_at(b as usize), segs)
}

/// `paragraphAtPath` with the first level's blocks given by `first` (the body's indexed blocks).
fn walk_path<'a>(first: impl Fn(u32) -> Option<Block<'a>>, segs: &[u32]) -> Option<BlockPath> {
    let mut list: Option<&'a Story> = None;
    let mut path: Option<BlockPath> = None;
    // the (row, cell index) of the step being entered
    let mut pending: Option<(u32, u32)> = None;
    let mut i = 0;
    while i < segs.len() {
        let b = *segs.get(i)?;
        let blk = match list {
            None => first(b)?,
            Some(s) => s.block_at(b as usize)?,
        };
        let here = match (&path, pending) {
            (Some(p), Some((r, k))) => p.cell_block(r, k, b),
            _ => BlockPath::root(b),
        };
        if i + 1 >= segs.len() {
            return match blk {
                Block::Paragraph(_) => Some(here),
                Block::Table(_) => None,
            };
        }
        let (Block::Table(t), Some(&r), Some(&c)) = (blk, segs.get(i + 1), segs.get(i + 2)) else { return None };
        let cell = uo_docx::word::refs::cell_at_grid(t, f64::from(r), f64::from(c))?;
        let k = u32::try_from(t.rows.get(r as usize)?.cells.iter().position(|x| std::ptr::eq(x, cell))?).ok()?;
        pending = Some((r, k));
        path = Some(here);
        list = Some(&cell.content);
        i += 3;
    }
    None
}

/// `hfBlocksFor(full, section, which, part)` (engine-doc.ts:358): the part the slot shows, from
/// the section or, when it does not define it, the nearest section before it.
#[must_use]
pub fn hf_story_for<'a>(view: DocView<'a>, section: u32, which: HfWhich, slot: HfSlot) -> Option<(crate::model::HfPartId, &'a Story)> {
    let secs = view.sections();
    let key = match slot {
        HfSlot::Default => "default",
        HfSlot::First => "first",
        HfSlot::Even => "even",
    };
    // `Math.min(section, full.sections.length - 1)` down to 0
    let top = i64::from(section).min(i64::try_from(secs.len()).unwrap_or(i64::MAX) - 1);
    let mut i = top;
    while i >= 0 {
        let s = secs.get(usize::try_from(i).ok()?)?;
        let set = match which {
            HfWhich::Header => &s.headers,
            HfWhich::Footer => &s.footers,
        };
        if let Some(&part) = set.get(key) {
            return view.hf_story(part).map(|st| (part, st));
        }
        i -= 1;
    }
    None
}

/// The paragraph a [`ParaLoc`] names.
#[must_use]
pub fn paragraph<'a>(view: DocView<'a>, at: &ParaLoc) -> Option<&'a Paragraph> {
    view.paragraph(&at.story, &at.path)
}

/// `paraLength(p)`: the UTF-16 length of the paragraph's runs.
#[must_use]
pub fn para_length(p: &Paragraph) -> u32 {
    p.runs.iter().fold(0u32, |n, r| n.wrapping_add(u32::try_from(r.text.len()).unwrap_or(u32::MAX)))
}
