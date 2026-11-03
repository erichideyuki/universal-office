//! Interned UTF-16 strings of one layout.

use rustc_hash::FxHashMap;

use crate::ids::Sym;

/// The strings of a layout (colours as the document wrote them, preset, dash and effect names,
/// image extensions): dense ids in first-use order, so the codec sends "entries from index n"
/// per page (uo-render §3.2). Keys are UTF-16 code units, lone surrogates kept.
#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    strings: Vec<Box<[u16]>>,
    index: FxHashMap<Box<[u16]>, Sym>,
    units: usize,
}

impl SymbolTable {
    #[must_use]
    pub fn new() -> SymbolTable {
        SymbolTable::default()
    }

    /// The id of `s`, interning it on first use; `None` only past `u32::MAX - 1` strings.
    pub fn intern(&mut self, s: &[u16]) -> Option<Sym> {
        if let Some(&id) = self.index.get(s) {
            return Some(id);
        }
        let id = Sym::from_index(self.strings.len())?;
        let owned: Box<[u16]> = s.into();
        self.units += owned.len();
        self.strings.push(owned.clone());
        self.index.insert(owned, id);
        Some(id)
    }

    /// [`SymbolTable::intern`] of a Rust string (constants such as `"auto"` or preset names).
    pub fn intern_str(&mut self, s: &str) -> Option<Sym> {
        let units: Vec<u16> = s.encode_utf16().collect();
        self.intern(&units)
    }

    /// The code units of `id`; empty for an id of another table.
    #[must_use]
    pub fn get(&self, id: Sym) -> &[u16] {
        self.strings.get(id.index()).map_or(&[], |s| s)
    }

    /// Whether `id` names a string of this table.
    #[must_use]
    pub fn contains(&self, id: Sym) -> bool {
        id.index() < self.strings.len()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// The strings from index `from` (the codec's delta), in id order.
    pub fn since(&self, from: usize) -> impl Iterator<Item = &[u16]> {
        self.strings.iter().skip(from).map(|s| &**s)
    }

    /// Code units held (M3 accounting).
    #[must_use]
    pub fn units(&self) -> usize {
        self.units
    }
}
