//! Identity ids of the model (architecture A1.3, A2, G22): what the TS decides by object identity
//! (`===`, `WeakMap`, `Set<object>`) is an explicit id here. Ids are per document, allocated by
//! [`IdAlloc`] in document order by the reader; they are not observable in the TS and never enter
//! the canonical digest. `SourceKey` is different: it is the TS `src` number, a value.
//!
//! All ids are `u32` (the architecture sketched `HfPartId(u16)`: a `u32` removes a truncation
//! decision at no cost, record "Milestone 1 — D1a foundation").

use crate::js::{JsValue, JsonWriter};

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
        pub struct $name(pub u32);

        impl $name {
            /// The index of the id (dense, from 0, per document).
            #[must_use]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(
    /// A paragraph object of the TS (`p === q`).
    ParaId
);
id_type!(
    /// A table object of the TS (`TableBlock`).
    TableId
);
id_type!(
    /// A drawing object of the TS: image, textbox, shape or group (`floatSel`, `emittedShapes`,
    /// the PDF image identity of uo-render RQ3).
    DrawingId
);
id_type!(
    /// A header/footer part (the `hfCache` entry of `readFullDocument`, one per part path).
    HfPartId
);
id_type!(
    /// A media payload in the document's [`crate::model::MediaStore`] (deduplicated by full
    /// bytes, G24).
    MediaId
);
id_type!(
    /// A section break of the session (TS `sectionBreak.id`, `nextSectionId`; milestone 2).
    SectionBreakId
);
id_type!(
    /// A textbox content's source groups (`SourceStore.txbx`, a `WeakMap` keyed by the textbox in
    /// the TS): an index into `SourceStore::txbx_groups`, carried by the textbox (U4c: the lookup
    /// survives undo).
    TxbxSrc
);

/// The TS `src` number (`SourceStore.id()`): the key of a paragraph's, run's, table's, row's or
/// cell's source. A value (it survives undo and copies), so it enters the digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SourceKey(pub u32);

impl JsValue for SourceKey {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.num(f64::from(self.0));
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.num(f64::from(self.0));
        true
    }
}

impl JsValue for u32 {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.num(f64::from(*self));
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.num(f64::from(*self));
        true
    }
}

/// The id allocator of one document. Counters saturate at `u32::MAX` (unreachable within the
/// memory and package limits).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdAlloc {
    para: u32,
    table: u32,
    drawing: u32,
    hf: u32,
}

impl IdAlloc {
    #[must_use]
    pub fn new() -> IdAlloc {
        IdAlloc::default()
    }

    fn bump(n: &mut u32) -> u32 {
        let v = *n;
        *n = n.saturating_add(1);
        v
    }

    pub fn para(&mut self) -> ParaId {
        ParaId(Self::bump(&mut self.para))
    }

    pub fn table(&mut self) -> TableId {
        TableId(Self::bump(&mut self.table))
    }

    pub fn drawing(&mut self) -> DrawingId {
        DrawingId(Self::bump(&mut self.drawing))
    }

    pub fn hf_part(&mut self) -> HfPartId {
        HfPartId(Self::bump(&mut self.hf))
    }

    /// How many ids of each kind were given (paragraphs, tables, drawings, header/footer parts).
    #[must_use]
    pub fn counts(&self) -> [u32; 4] {
        [self.para, self.table, self.drawing, self.hf]
    }
}
