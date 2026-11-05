//! The identity keys of the port (LA3): every TS identity (`===`, `WeakMap`, `WeakSet`,
//! `Set<object>`, `Map<object, …>`) the layout relies on, as an explicit index. Each frontier
//! declares in the record which LA3 row each of its sites uses.
//!
//! | TS identity | key |
//! |---|---|
//! | `Run` objects (late floats, `prePlaced`, `GLUED_FIELD`, `FRAME_CARRY`, `Glyph.obj`, `pinnedFloats`) | [`RunKey`] (+ `RunRef` when a model run) |
//! | `Paragraph` objects (`CONTINUATION`, `FRAME_SKIP`, `INNER`, `anchorCache`, `listItemValues`, `hyphenationSkip`) | [`LParaId`] (+ `ParaId` when the model object itself) |
//! | `DrawingTextbox` (`FRAME_INFO`, `indexChains`, `members.has`) | [`TbKey`] |
//! | `TextboxChain` (`chainSlices`) | [`ChainId`] |
//! | `TableLayout` (`floatFlow` `WeakMap`) | [`TableLayoutId`] |
//! | items and lines in row splits (`LATE_ITEMS`, `moved`/`own`/`keep`, `lastOf.get(ref) === last`, `idx`, `seen`) | [`ItemMeta`] (carried by every pass-scope item and line) |
//! | `CellContent.nest` aliases, `mergeText`/`mergeFills`/`topBorders`, the `moved`/`own`/`keep` sets of row splits | [`ObjId`] on the items and lines ([`crate::table::NestRow`], F6) |
//! | the items array a border box went into (`boxPage`) | [`ContainerId`] |
//! | `PageState` objects (`lnPage`, pinned floats, `lastBox`) | [`PageIx`] |
//! | `FontFace` objects (kerning/ligatures/merging across the same face) | `uo_render::FaceId` = the provider answer's identity |
//! | `TableCell` (`cellNb`) | `CellKey` (uo-docx) |
//! | `CellContent` objects held by the pass (chain slices, note cache) | [`CellContentId`] |
//! | module counter `lineSeq` (`lineBox.line`) | [`LineSeq`], per call (relabelled by first appearance in the canonical stream) |

macro_rules! key {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
        pub struct $name(pub u32);

        impl $name {
            /// The key of entry `i` (`None` past `u32::MAX`).
            #[must_use]
            pub fn of(i: usize) -> Option<$name> {
                u32::try_from(i).ok().map($name)
            }

            #[must_use]
            pub fn ix(self) -> usize {
                self.0 as usize
            }
        }
    };
}

key!(
    /// A paragraph of the derived document (LDoc): a model paragraph or a TS copy.
    LParaId
);
key!(
    /// A run of the derived document: a model run (also inside copies, K1) or a synthetic one
    /// (slices, frame runs).
    RunKey
);
key!(
    /// A textbox of the derived document: a model textbox or a synthetic frame box.
    TbKey
);
key!(
    /// A chain of linked textboxes.
    ChainId
);
key!(
    /// A `TableLayout` of the pass (the TS `floatFlow` WeakMap key, K18: the layout itself stays
    /// owned by its stack frame).
    TableLayoutId
);
key!(
    /// The side-table entry of a pass-scope item or line (K2e): `LATE_ITEMS`, the `moved`/`own`/
    /// `keep` sets of row splits, the line identity of `lastOf`.
    ItemMeta
);
key!(
    /// The identity of a pass-scope item or line **object** (added by F6): the TS `===` of the
    /// `moved`/`own`/`keep`/`top` sets of the row splits (table.ts:130–189, 1355), of
    /// `mergeText`/`mergeFills`/`topBorders` and of `NestRow.items`/`lines`. Given where an object
    /// is referenced from a second place; a `shiftItem`/`shiftLine` copy has none (a new object),
    /// a move of the same object keeps it. Unique within the container family that compares it
    /// (a cell content and its parts; the rows of one table).
    ObjId
);
key!(
    /// A page of the pass.
    PageIx
);
key!(
    /// A laid-out cell content held by the pass (chain slices, notes).
    CellContentId
);
key!(
    /// `lineBox.line` (paint.ts:16 `lineSeq`): per call here; the TS value depends on every
    /// layout before it in the process, so the canonical stream relabels it.
    LineSeq
);

/// The items array a paragraph border box went into (`boxPage: items`, engine.ts:1398, K2f).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ContainerId {
    /// a page's body items
    PageBody(PageIx),
    /// a cell content or a static frame (header/footer, textbox) of the pass
    Static(CellContentId),
}
