//! JS-exact strings and the interned string pool.
//!
//! Calc text functions index by UTF-16 code unit and must carry lone surrogates (`MID`, `LEN`, `FIND`,
//! `fromCodePoint` of `&#xD800;` in the sheet XML): a Rust `String` cannot. Like V8's one-byte/two-byte
//! strings, a string whose units are all `<= 0xFF` is stored one byte per unit (Latin-1, which covers
//! Portuguese and Spanish text), any other as UTF-16. Both views compare and hash by code unit.
//!
//! The pool interns the strings that live in cells (shared-string entries, inline strings, formula
//! results): a cell holds a 4-byte [`StrId`]. Entries are reference counted by the cells that hold them,
//! freed slots are reused and the byte arena is compacted when more than half of it is garbage (M2: an edit
//! soak stays flat). Strings produced while evaluating live in the engine's per-recalc temporary arena
//! ([`crate::value::StrRef::Temp`]) and are interned only when written to a cell.

use std::collections::HashMap;
use std::num::NonZeroU32;

/// A borrowed JS string: Latin-1 bytes (every unit `<= 0xFF`) or UTF-16 code units.
#[derive(Clone, Copy, Debug)]
pub enum JsStr<'a> {
    /// One byte per UTF-16 unit.
    Latin1(&'a [u8]),
    /// UTF-16 code units (may hold lone surrogates).
    Utf16(&'a [u16]),
}

/// An owned JS string (same two representations; the narrow one is chosen whenever it fits).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum JsString {
    /// One byte per UTF-16 unit.
    Latin1(Box<[u8]>),
    /// UTF-16 code units.
    Utf16(Box<[u16]>),
}

/// Id of an interned string (`NonZeroU32`: `Option<StrId>` is 4 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StrId(pub NonZeroU32);

/// One pool entry (12 bytes).
#[derive(Clone, Copy, Debug)]
pub struct StrEntry {
    /// Byte offset in the arena.
    pub offset: u32,
    /// Length in UTF-16 units; the top bit marks the UTF-16 representation (2 bytes per unit in the arena).
    pub len: u32,
    /// Cells (and shared-string entries) holding the id; 0 = free slot (on the free list).
    pub refs: u32,
}

/// The interned string pool of one workbook.
#[derive(Debug, Default)]
pub struct StrPool {
    /// Latin-1 bytes and UTF-16 units (little endian, 2-byte aligned) of every live string.
    pub arena: Vec<u8>,
    /// Slot `i` is `StrId(i + 1)`.
    pub entries: Vec<StrEntry>,
    /// Freed slots, reused last-in first-out (deterministic ids for a given call sequence).
    pub free: Vec<u32>,
    /// Content hash → slot of the first entry with that hash; collisions chain through `next`.
    pub index: HashMap<u64, u32>,
    /// Hash-collision chain, parallel to `entries` (`u32::MAX` = end).
    pub next: Vec<u32>,
    /// Arena bytes no live entry uses (compaction trigger).
    pub garbage: usize,
}

/// Bounds of the pool (M5; untrusted workbooks can hold millions of distinct strings).
#[derive(Clone, Copy, Debug)]
pub struct StrPoolLimits {
    /// Most live entries.
    pub max_entries: u32,
    /// Most arena bytes.
    pub max_bytes: usize,
}
