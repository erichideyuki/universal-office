//! The package session (`package.ts` `PptxPackage`): the parts as a `Map<string, Uint8Array>` with
//! JS `Map` semantics, plus a generation per part version (`uo-pptx.md` P2).
//!
//! - `set` on an existing name keeps its position; on a new name appends; `remove` shifts the rest.
//!   Insertion order is the ZIP order on save, and `history.ts restore` depends on it (a part
//!   restored by undo is appended in snapshot order).
//! - Bytes are `Arc<[u8]>`, shared with snapshots, media and caches; a part is replaced, never
//!   changed in place (as the TS: the editor swaps whole `Uint8Array`s).
//! - [`PartGen`] identifies a part version: caches and read sets compare generations where the TS
//!   compares `Uint8Array` identity (`changedSince`, `restore`).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_opc::package::{OpenOptions, Package};
use uo_opc::zip::{WriteOptions, ZipWriter};

use crate::error::PptxError;

/// A part version: unique per store, monotone.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct PartGen(pub u64);

/// One part of the store.
#[derive(Clone, Debug)]
pub struct PartEntry {
    pub name: Box<str>,
    pub bytes: Arc<[u8]>,
    pub generation: PartGen,
}

/// The parts of an open deck in insertion order.
#[derive(Clone, Debug, Default)]
pub struct PartStore {
    entries: Vec<PartEntry>,
    index: FxHashMap<Box<str>, u32>,
    next_gen: u64,
}

impl PartStore {
    /// An empty store (`new Map()`).
    pub fn new() -> PartStore {
        PartStore::default()
    }

    /// A store from parts in order; a repeated name keeps its first position and its last bytes
    /// (`new Map(entries)`).
    pub fn from_parts(parts: impl IntoIterator<Item = (Box<str>, Arc<[u8]>)>) -> PartStore {
        let mut s = PartStore::new();
        for (name, bytes) in parts {
            s.set(&name, bytes);
        }
        s
    }

    /// Number of parts.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No parts.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The parts in store (ZIP) order.
    pub fn entries(&self) -> &[PartEntry] {
        &self.entries
    }

    /// `partNames()`.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| &*e.name)
    }

    /// The entry of a part (`getPart`).
    pub fn get(&self, name: &str) -> Option<&PartEntry> {
        let slot = *self.index.get(name)?;
        self.entries.get(usize::try_from(slot).ok()?)
    }

    /// The bytes of a part (`getPart`).
    pub fn bytes(&self, name: &str) -> Option<&Arc<[u8]>> {
        self.get(name).map(|e| &e.bytes)
    }

    /// `getPartText`: `TextDecoder` (UTF-8, BOM stripped, U+FFFD for bad bytes).
    pub fn text(&self, name: &str) -> Option<String> {
        self.get(name).map(|e| uo_jsnum::text::decode_utf8(&e.bytes).into_owned())
    }

    /// The generation the next `set` will stamp.
    pub fn peek_next_gen(&self) -> PartGen {
        PartGen(self.next_gen)
    }

    /// `setPart` (`Map.set`): an existing name keeps its position, a new one is appended; the part gets a
    /// fresh generation.
    pub fn set(&mut self, name: &str, bytes: Arc<[u8]>) -> PartGen {
        let generation = PartGen(self.next_gen);
        self.next_gen += 1;
        match self.index.get(name).and_then(|&i| self.entries.get_mut(i as usize)) {
            Some(e) => {
                e.bytes = bytes;
                e.generation = generation;
            }
            None => {
                self.index.insert(name.into(), u32::try_from(self.entries.len()).unwrap_or(u32::MAX));
                self.entries.push(PartEntry { name: name.into(), bytes, generation });
            }
        }
        generation
    }

    /// `setPartText`: `TextEncoder` (UTF-8; a part edited by text loses its BOM — TS behaviour).
    pub fn set_text(&mut self, name: &str, text: &str) -> PartGen {
        self.set(name, Arc::from(text.as_bytes()))
    }

    /// `removePart` (`Map.delete`): the rest shifts.
    pub fn remove(&mut self, name: &str) -> bool {
        let Some(&slot) = self.index.get(name) else { return false };
        self.entries.remove(slot as usize);
        self.index.clear();
        for (i, e) in self.entries.iter().enumerate() {
            self.index.insert(e.name.clone(), u32::try_from(i).unwrap_or(u32::MAX));
        }
        true
    }

    /// `save()` (`package.ts:46`): deflate level 6 in store order.
    pub fn save(&self) -> Result<Vec<u8>, PptxError> {
        let mut w = ZipWriter::new(WriteOptions { level: 6, ..WriteOptions::default() });
        for e in &self.entries {
            w.add(&e.name, &e.bytes).map_err(|_| PptxError::Limit("zip entry"))?;
        }
        Ok(w.finish())
    }

    /// A shallow snapshot (one `Arc` per part, in store order).
    pub fn snapshot(&self) -> PartsSnapshot {
        PartsSnapshot(self.entries.clone())
    }
}

/// Parts in order (name, bytes), as the converters and `fromParts` produce them.
pub type RawParts = Vec<(Box<str>, Arc<[u8]>)>;

/// A shallow snapshot of the package (`PartsSnapshot` of `history.ts`): one `Arc` per part, in
/// store order.
#[derive(Clone, Debug, Default)]
pub struct PartsSnapshot(pub Vec<PartEntry>);

/// `PptxPackage.load` (`package.ts:20`) for a ZIP: the TS bomb filter (`originalSize <=
/// max(1 MiB, size × 1100)`), every entry inflated in file order, and the `ppt/presentation.xml` check.
/// A `.ppt` (CFB) goes through the converter of frontier B2.
pub fn load(bytes: &[u8], opc: &uo_opc::Limits) -> Result<PartStore, PptxError> {
    if uo_opc::cfb::is_cfb(bytes) {
        #[cfg(feature = "ppt")]
        return Ok(PartStore::from_parts(crate::ppt::convert(bytes)?));
        #[cfg(not(feature = "ppt"))]
        return Err(PptxError::PptFormat("the .ppt converter is not built (feature `ppt`, frontier B2)".into()));
    }
    let opts = OpenOptions { skip_impossible_ratio: true, ..OpenOptions::default() };
    let pkg = Package::open(bytes, opc, &opts).map_err(|_| PptxError::NotAPackage)?;
    let mut store = PartStore::new();
    for name in pkg.part_names() {
        let data = pkg.part(name).map_err(|_| PptxError::NotAPackage)?.ok_or(PptxError::NotAPackage)?;
        store.set(name, Arc::from(data));
    }
    if store.get("ppt/presentation.xml").is_none() {
        return Err(PptxError::NoPresentation);
    }
    Ok(store)
}
