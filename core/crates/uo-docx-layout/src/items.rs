//! Pass-scope item and line containers (K2e, RQ2): what the TS keeps as `PageItem[]` and
//! `LaidOutLine[]` arrays while laying out (cell contents, table rows, static frames, z-ordered
//! floats, page bodies). Items move between them by copy (`shiftItem`, `shiftLine`), and every
//! copy keeps or drops its side data exactly as the TS keeps or drops the `WeakMap` entry of the
//! object (declared per site by F5–F7). At page emission they become a uo-render `Page` and the
//! Write section (`output.rs`).

use std::sync::Arc;

use crate::keys::{ItemMeta, ObjId};
use crate::output::{GlyphMeta, LineParaFormat, LineRef};
use uo_render::types::Item;
use uo_render::{CapacityError, GlyphRunHead, ImageId, ItemList};

/// The side data of one pass-scope item, parallel to `PassItems::list.items`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ItemSide {
    /// the item's `LATE_ITEMS` entry (table.ts:1383: the z-order mark a `shiftItem` copy keeps)
    pub meta: Option<ItemMeta>,
    /// `lineBox`, `lineNum` of a glyph run
    pub glyph: Option<GlyphMeta>,
    /// `ClipItem.src`
    pub clip_src: Option<ImageId>,
    /// the object's identity (`moved`/`own`/`keep`/`top`, `NestRow.items`, merge texts and fills;
    /// added by F6): a `shiftItem` copy is a new object and has none
    pub obj: Option<ObjId>,
}

/// A pass-scope `PageItem[]`.
#[derive(Clone, Debug, Default)]
pub struct PassItems {
    pub list: ItemList,
    pub side: Vec<ItemSide>,
}

impl PassItems {
    #[must_use]
    pub fn new() -> PassItems {
        PassItems::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// `out.push(shiftItem(src[i], dy, dx))` with the side data kept (the TS `shiftItem` copies the
    /// `LATE_ITEMS` entry to the new object, table.ts:1385; `lineBox` and `src` are fields).
    pub fn push_shifted(&mut self, src: &PassItems, i: usize, dy: f64, dx: f64) -> Result<(), CapacityError> {
        let before = self.list.len();
        self.list.push_shifted(&src.list, i, dy, dx)?;
        if self.list.len() > before {
            // a new object: the `LATE_ITEMS` mark and the fields travel, the identity does not
            let side = src.side.get(i).copied().unwrap_or_default();
            self.side.push(ItemSide { obj: None, ..side });
        }
        Ok(())
    }

    /// `out.push(src[i])`: the same object moved to another array (side data and identity kept;
    /// no arithmetic, so `-0` stays `-0`; added by F6).
    pub fn push_same(&mut self, src: &PassItems, i: usize) -> Result<(), CapacityError> {
        let Some(it) = src.list.items.get(i) else { return Ok(()) };
        match it {
            Item::Glyphs { face, color, size_pt, y, glyphs, run, flags } => {
                let rs = src.list.run(*run).copied().unwrap_or_default();
                let head = GlyphRunHead {
                    face: *face,
                    color: *color,
                    size_pt: *size_pt,
                    y: *y,
                    text: src.list.text_of(rs.text),
                    scale_x: rs.scale_x,
                    alpha: rs.alpha,
                    fx: rs.fx,
                    flags: *flags,
                };
                self.list.push_glyphs(head, src.list.glyphs_of(*glyphs))?;
            }
            Item::Path { cmds, fill, stroke, even_odd } => self.list.push_path(src.list.cmds_of(*cmds), *fill, *stroke, *even_odd)?,
            other => self.list.push_plain(other.clone()),
        }
        self.side.push(src.side.get(i).copied().unwrap_or_default());
        Ok(())
    }
}

/// A pass-scope `LaidOutLine` (with its identity, K2e: `lastOf.get(ref) === last`).
#[derive(Clone, Debug)]
pub struct PassLine {
    pub at: LineRef,
    pub start: u32,
    pub end: u32,
    pub x: f64,
    pub y: f64,
    pub height: f64,
    pub baseline: f64,
    pub width: f64,
    /// `chars`: `(o, x, w)`
    pub chars: Option<Box<[(u32, f64, f64)]>>,
    pub hyph: Option<u32>,
    pub rev: Option<(f64, f64)>,
    pub frame: Option<(f64, f64)>,
    /// `pfmt`: one object per paragraph in the TS (`PFMT` WeakMap); shared here, compared by value
    pub pfmt: Option<Arc<LineParaFormat>>,
    pub meta: Option<ItemMeta>,
    /// the line object's identity (as [`ItemSide::obj`]; added by F6): `{ ...l }` copies have none
    pub obj: Option<ObjId>,
}

impl PassLine {
    /// `shiftLine(l, dy)` (engine.ts:4096): `{ ...l, y: l.y + dy, baseline: l.baseline + dy }`.
    #[must_use]
    pub fn shifted(&self, dy: f64) -> PassLine {
        PassLine { y: self.y + dy, baseline: self.baseline + dy, obj: None, ..self.clone() }
    }

    /// `shiftLineXY(l, dx, dy)` (engine.ts:4100): chars move only when `dx` is truthy.
    #[must_use]
    pub fn shifted_xy(&self, dx: f64, dy: f64) -> PassLine {
        let chars = match &self.chars {
            Some(cs) if uo_jsnum::conv::truthy(dx) => Some(cs.iter().map(|&(o, x, w)| (o, x + dx, w)).collect()),
            other => other.clone(),
        };
        PassLine { x: self.x + dx, y: self.y + dy, baseline: self.baseline + dy, chars, obj: None, ..self.clone() }
    }
}
