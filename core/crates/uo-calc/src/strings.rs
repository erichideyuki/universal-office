//! JS-exact strings and the interned string pool.
//!
//! Calc text functions index by UTF-16 code unit and must carry lone surrogates (`MID`, `LEN`, `FIND`,
//! `fromCodePoint` of `&#xD800;` in the sheet XML): a Rust `String` cannot. Like V8's one-byte/two-byte
//! strings, a string whose units are all `<= 0xFF` is stored one byte per unit (Latin-1, which covers
//! Portuguese and Spanish text), any other as UTF-16. Both views compare and hash by code unit; the narrow form
//! is chosen whenever it fits, so a JS string has exactly one representation.
//!
//! The pool interns the strings that live in cells (inline strings, formula/spill text, `t="d"` texts,
//! shared-string entries): a cell holds a 4-byte [`StrId`]. Entries are reference counted by the cells that
//! hold them, freed slots are reused last-in first-out (deterministic ids for a given call sequence) and the
//! arenas are compacted when more than half of them is garbage (M2: an edit soak stays flat). Strings
//! produced while evaluating live in the engine's per-recalc temporary arena ([`crate::value::StrRef::Temp`])
//! and are interned only when written to a cell.

use std::hash::{BuildHasher, Hasher};
use std::num::NonZeroU32;

use crate::error::{CalcError, Result};

/// A borrowed JS string: Latin-1 bytes (every unit `<= 0xFF`) or UTF-16 code units.
#[derive(Clone, Copy, Debug)]
pub enum JsStr<'a> {
    /// One byte per UTF-16 unit.
    Latin1(&'a [u8]),
    /// UTF-16 code units (may hold lone surrogates).
    Utf16(&'a [u16]),
}

/// An owned JS string (same two representations; the narrow one is chosen whenever it fits). Equality and
/// hashing are by code unit, whatever the representation.
#[derive(Clone, Debug, Eq)]
pub enum JsString {
    /// One byte per UTF-16 unit.
    Latin1(Box<[u8]>),
    /// UTF-16 code units.
    Utf16(Box<[u16]>),
}

impl Default for JsString {
    fn default() -> Self {
        Self::Latin1(Box::default())
    }
}

impl<'a> JsStr<'a> {
    /// The empty string.
    pub const EMPTY: JsStr<'static> = JsStr::Latin1(&[]);

    /// Length in UTF-16 code units (`.length`).
    #[must_use]
    pub fn len(self) -> usize {
        match self {
            Self::Latin1(b) => b.len(),
            Self::Utf16(u) => u.len(),
        }
    }

    /// `.length === 0`.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Code unit `i` (`charCodeAt`), if in range.
    #[must_use]
    pub fn unit(self, i: usize) -> Option<u16> {
        match self {
            Self::Latin1(b) => b.get(i).map(|&x| u16::from(x)),
            Self::Utf16(u) => u.get(i).copied(),
        }
    }

    /// The code units.
    pub fn units(self) -> impl Iterator<Item = u16> + 'a {
        let (l, w): (&'a [u8], &'a [u16]) = match self {
            Self::Latin1(b) => (b, &[]),
            Self::Utf16(u) => (&[], u),
        };
        l.iter().map(|&x| u16::from(x)).chain(w.iter().copied())
    }

    /// The units as a UTF-16 vector.
    #[must_use]
    pub fn to_utf16(self) -> Vec<u16> {
        self.units().collect()
    }

    /// Text for Rust APIs: lone surrogates become U+FFFD (as `Buffer.from(s, 'utf8')`).
    #[must_use]
    pub fn to_string_lossy(self) -> String {
        match self {
            Self::Latin1(b) => b.iter().map(|&x| char::from(x)).collect(),
            Self::Utf16(u) => char::decode_utf16(u.iter().copied()).map(|c| c.unwrap_or('\u{fffd}')).collect(),
        }
    }

    /// Equal code units (`===`).
    #[must_use]
    pub fn eq_units(self, other: JsStr<'_>) -> bool {
        match (self, other) {
            (Self::Latin1(a), JsStr::Latin1(b)) => a == b,
            (Self::Utf16(a), JsStr::Utf16(b)) => a == b,
            _ => self.len() == other.len() && self.units().eq(other.units()),
        }
    }

    /// Equal to an ASCII/Unicode Rust string by code units.
    #[must_use]
    pub fn eq_str(self, s: &str) -> bool {
        match self {
            Self::Latin1(b) if s.is_ascii() => b == s.as_bytes(),
            _ => self.units().eq(s.encode_utf16()),
        }
    }

    /// An owned copy (narrow when it fits).
    #[must_use]
    pub fn to_owned_js(self) -> JsString {
        match self {
            Self::Latin1(b) => JsString::Latin1(b.into()),
            Self::Utf16(u) => JsString::from_utf16(u),
        }
    }
}

impl JsString {
    /// From UTF-16 units; narrow when every unit is `<= 0xFF`.
    #[must_use]
    pub fn from_utf16(units: &[u16]) -> Self {
        if units.iter().all(|&u| u <= 0xFF) { Self::Latin1(units.iter().map(|&u| u as u8).collect()) } else { Self::Utf16(units.into()) }
    }

    /// From a Rust string (UTF-8 → UTF-16 units).
    #[must_use]
    pub fn from_str_js(s: &str) -> Self {
        if s.is_ascii() {
            return Self::Latin1(s.as_bytes().into());
        }
        let units: Vec<u16> = s.encode_utf16().collect();
        Self::from_utf16(&units)
    }

    /// The borrowed view.
    #[must_use]
    pub fn as_js(&self) -> JsStr<'_> {
        match self {
            Self::Latin1(b) => JsStr::Latin1(b),
            Self::Utf16(u) => JsStr::Utf16(u),
        }
    }

    /// Heap bytes held.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        match self {
            Self::Latin1(b) => b.len(),
            Self::Utf16(u) => u.len() * 2,
        }
    }
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        self.as_js().eq_units(other.as_js())
    }
}

impl std::hash::Hash for JsString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.as_js().len());
        for u in self.as_js().units() {
            state.write_u16(u);
        }
    }
}

/// Id of an interned string (`NonZeroU32`: `Option<StrId>` is 4 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct StrId(pub NonZeroU32);

impl StrId {
    /// The slot index.
    #[must_use]
    pub const fn slot(self) -> usize {
        (self.0.get() - 1) as usize
    }
    /// The raw id (`slot + 1`), as stored in a payload lane.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0.get()
    }
    /// From a raw payload (`None` for 0).
    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match NonZeroU32::new(raw) {
            Some(n) => Some(Self(n)),
            None => None,
        }
    }
}

/// One pool entry (12 bytes).
#[derive(Clone, Copy, Debug)]
pub struct StrEntry {
    /// Offset in the Latin-1 arena (bytes) or in the UTF-16 arena (units).
    pub offset: u32,
    /// Length in UTF-16 units; the top bit marks the UTF-16 representation.
    pub len: u32,
    /// Holders of the id (cells, shared-string entries); 0 = free slot (on the free list); `u32::MAX` = pinned.
    pub refs: u32,
}

const WIDE: u32 = 1 << 31;
const PINNED: u32 = u32::MAX;
const EMPTY_SLOT: u32 = 0;
const TOMB: u32 = u32::MAX;
/// Compaction starts only above this much garbage (bytes), so small pools never move.
const COMPACT_MIN: usize = 4096;

/// Bounds of the pool (M5; untrusted workbooks can hold millions of distinct strings).
#[derive(Clone, Copy, Debug)]
pub struct StrPoolLimits {
    /// Most live entries.
    pub max_entries: u32,
    /// Most arena bytes (Latin-1 bytes + 2 × UTF-16 units).
    pub max_bytes: usize,
}

impl Default for StrPoolLimits {
    fn default() -> Self {
        // record "Memory plan" M5: 2^24 entries, 512 MiB native / 256 MiB wasm32
        Self { max_entries: 1 << 24, max_bytes: if cfg!(target_pointer_width = "32") { 256 << 20 } else { 512 << 20 } }
    }
}

/// The interned string pool of one workbook. Slot 0 (`StrPool::EMPTY`) is the empty string, pinned.
#[derive(Debug)]
pub struct StrPool {
    bytes: Vec<u8>,
    wide: Vec<u16>,
    entries: Vec<StrEntry>,
    free: Vec<u32>,
    /// Open addressing by content hash: `slot + 1`, 0 = empty, `u32::MAX` = deleted.
    table: Vec<u32>,
    /// Non-empty buckets of `table` (live + deleted).
    table_used: usize,
    hasher: std::collections::hash_map::RandomState,
    /// Arena bytes no live entry uses.
    garbage: usize,
    live: u32,
    limits: StrPoolLimits,
}

impl Default for StrPool {
    fn default() -> Self {
        Self::new()
    }
}

impl StrPool {
    /// The empty string's id (always present).
    pub const EMPTY: StrId = StrId(NonZeroU32::MIN);

    /// A pool holding only the empty string.
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(StrPoolLimits::default())
    }

    /// A pool with explicit limits.
    #[must_use]
    pub fn with_limits(limits: StrPoolLimits) -> Self {
        let mut p = Self {
            bytes: Vec::new(),
            wide: Vec::new(),
            entries: vec![StrEntry { offset: 0, len: 0, refs: PINNED }],
            free: Vec::new(),
            table: Vec::new(),
            table_used: 0,
            hasher: std::collections::hash_map::RandomState::new(),
            garbage: 0,
            live: 1,
            limits,
        };
        p.rehash(8);
        p
    }

    /// Live entries (the empty string included).
    #[must_use]
    pub fn len(&self) -> usize {
        self.live as usize
    }

    /// Only the empty string is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live <= 1
    }

    /// The text of an id. An id that is not live reads as the empty string (never a panic).
    #[must_use]
    pub fn get(&self, id: StrId) -> JsStr<'_> {
        self.entries.get(id.slot()).filter(|e| e.refs != 0).map_or(JsStr::EMPTY, |e| self.text_of(*e))
    }

    /// Reference count of an id (`u32::MAX` for the pinned empty string, 0 for a free slot).
    #[must_use]
    pub fn refs(&self, id: StrId) -> u32 {
        self.entries.get(id.slot()).map_or(0, |e| e.refs)
    }

    fn text_of(&self, e: StrEntry) -> JsStr<'_> {
        let (o, n) = (e.offset as usize, (e.len & !WIDE) as usize);
        if e.len & WIDE != 0 {
            JsStr::Utf16(self.wide.get(o..o + n).unwrap_or(&[]))
        } else {
            JsStr::Latin1(self.bytes.get(o..o + n).unwrap_or(&[]))
        }
    }

    fn hash(&self, s: JsStr<'_>) -> u64 {
        let mut h = self.hasher.build_hasher();
        match s {
            JsStr::Latin1(b) => {
                h.write_u8(0);
                h.write(b);
            }
            JsStr::Utf16(u) => {
                // the canonical form: a UTF-16 string that fits Latin-1 hashes as Latin-1
                if u.iter().all(|&x| x <= 0xFF) {
                    h.write_u8(0);
                    for &x in u {
                        h.write_u8(x as u8);
                    }
                } else {
                    h.write_u8(1);
                    for &x in u {
                        h.write_u16(x);
                    }
                }
            }
        }
        h.finish()
    }

    /// The id of a string already in the pool.
    #[must_use]
    pub fn find(&self, s: JsStr<'_>) -> Option<StrId> {
        if s.is_empty() {
            return Some(Self::EMPTY);
        }
        let mask = self.table.len() - 1;
        let mut i = (self.hash(s) as usize) & mask;
        loop {
            match self.table[i] {
                EMPTY_SLOT => return None,
                TOMB => {}
                v => {
                    let e = self.entries[(v - 1) as usize];
                    if self.text_of(e).eq_units(s) {
                        return StrId::from_raw(v);
                    }
                }
            }
            i = (i + 1) & mask;
        }
    }

    /// Interns `s` and takes one reference (the caller owns it: a cell, an SST entry).
    pub fn intern(&mut self, s: JsStr<'_>) -> Result<StrId> {
        if let Some(id) = self.find(s) {
            self.retain(id);
            return Ok(id);
        }
        let wide = matches!(s, JsStr::Utf16(u) if u.iter().any(|&x| x > 0xFF));
        let n = s.len();
        let bytes_needed = if wide { n * 2 } else { n };
        if self.live >= self.limits.max_entries || n >= WIDE as usize {
            return Err(CalcError::Limit("string pool entries"));
        }
        if self.arena_bytes() - self.garbage + bytes_needed > self.limits.max_bytes {
            return Err(CalcError::Limit("string pool bytes"));
        }
        let offset = if wide {
            let o = self.wide.len();
            self.wide.extend(s.units());
            o
        } else {
            let o = self.bytes.len();
            match s {
                JsStr::Latin1(b) => self.bytes.extend_from_slice(b),
                JsStr::Utf16(u) => self.bytes.extend(u.iter().map(|&x| x as u8)),
            }
            o
        };
        let entry = StrEntry {
            offset: u32::try_from(offset).map_err(|_| CalcError::Limit("string pool bytes"))?,
            len: n as u32 | if wide { WIDE } else { 0 },
            refs: 1,
        };
        let slot = if let Some(slot) = self.free.pop() {
            self.entries[slot as usize] = entry;
            slot
        } else {
            self.entries.push(entry);
            (self.entries.len() - 1) as u32
        };
        self.live += 1;
        if (self.table_used + 1) * 4 > self.table.len() * 3 {
            self.rehash(((self.live as usize) * 2).next_power_of_two().max(8));
        }
        let h = self.hash(s) as usize;
        self.insert_bucket(h, slot + 1);
        Ok(StrId(NonZeroU32::MIN.saturating_add(slot)))
    }

    fn insert_bucket(&mut self, h: usize, v: u32) {
        let mask = self.table.len() - 1;
        let mut i = h & mask;
        while self.table[i] != EMPTY_SLOT && self.table[i] != TOMB {
            i = (i + 1) & mask;
        }
        if self.table[i] == EMPTY_SLOT {
            self.table_used += 1;
        }
        self.table[i] = v;
    }

    fn rehash(&mut self, cap: usize) {
        self.table = vec![EMPTY_SLOT; cap];
        self.table_used = 0;
        for slot in 0..self.entries.len() {
            let e = self.entries[slot];
            if slot == 0 || e.refs == 0 {
                continue;
            }
            let h = self.hash(self.text_of(e)) as usize;
            self.insert_bucket(h, slot as u32 + 1);
        }
    }

    /// One more holder of `id`.
    pub fn retain(&mut self, id: StrId) {
        if let Some(e) = self.entries.get_mut(id.slot())
            && e.refs != 0
            && e.refs != PINNED
        {
            // saturating: a count that reaches the pin value stays (never freed); unreachable under Limits
            e.refs = e.refs.saturating_add(1);
        }
    }

    /// One holder fewer; the entry is freed at zero (its slot reused, its bytes garbage until compaction).
    pub fn release(&mut self, id: StrId) {
        let slot = id.slot();
        let Some(e) = self.entries.get_mut(slot) else { return };
        if e.refs == 0 || e.refs == PINNED {
            return;
        }
        e.refs -= 1;
        if e.refs != 0 {
            return;
        }
        let e = *e;
        let h = self.hash(self.text_of(e)) as usize;
        let mask = self.table.len() - 1;
        let mut i = h & mask;
        while self.table[i] != EMPTY_SLOT {
            if self.table[i] == slot as u32 + 1 {
                self.table[i] = TOMB;
                break;
            }
            i = (i + 1) & mask;
        }
        let n = (e.len & !WIDE) as usize;
        self.garbage += if e.len & WIDE != 0 { n * 2 } else { n };
        self.entries[slot] = StrEntry { offset: 0, len: 0, refs: 0 };
        self.free.push(slot as u32);
        self.live -= 1;
        if self.garbage >= COMPACT_MIN && self.garbage * 2 > self.arena_bytes() {
            self.compact();
        }
    }

    fn arena_bytes(&self) -> usize {
        self.bytes.len() + self.wide.len() * 2
    }

    /// Bytes of arena no live entry uses.
    #[must_use]
    pub fn garbage(&self) -> usize {
        self.garbage
    }

    /// Rewrites the arenas with only the live strings (ids unchanged) and drops the deleted buckets. Runs by
    /// itself when more than half of the arenas is garbage.
    pub fn compact(&mut self) {
        let mut bytes = Vec::with_capacity(self.bytes.len().saturating_sub(self.garbage));
        let mut wide = Vec::new();
        for e in &mut self.entries {
            if e.refs == 0 || e.len & !WIDE == 0 {
                continue;
            }
            let (o, n) = (e.offset as usize, (e.len & !WIDE) as usize);
            if e.len & WIDE != 0 {
                let new = wide.len();
                wide.extend_from_slice(self.wide.get(o..o + n).unwrap_or(&[]));
                e.offset = new as u32;
            } else {
                let new = bytes.len();
                bytes.extend_from_slice(self.bytes.get(o..o + n).unwrap_or(&[]));
                e.offset = new as u32;
            }
        }
        self.bytes = bytes;
        self.wide = wide;
        self.garbage = 0;
        // trailing free slots go away; the rest of the free list stays in LIFO order
        while self.entries.len() > 1 && self.entries.last().is_some_and(|e| e.refs == 0) {
            self.entries.pop();
        }
        let n = self.entries.len() as u32;
        self.free.retain(|&s| s < n);
        self.entries.shrink_to_fit();
        self.free.shrink_to_fit();
        self.rehash(((self.live as usize) * 2).next_power_of_two().max(8));
    }

    /// Heap bytes held by the pool (capacities).
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        self.bytes.capacity()
            + self.wide.capacity() * 2
            + self.entries.capacity() * core::mem::size_of::<StrEntry>()
            + self.free.capacity() * 4
            + self.table.capacity() * 4
    }

    /// Checks the pool's invariants (tests and fuzzing): every live entry is findable, counts agree.
    #[doc(hidden)]
    pub fn check_invariants(&self) -> core::result::Result<(), String> {
        let live = self.entries.iter().filter(|e| e.refs != 0).count();
        if live != self.live as usize {
            return Err(format!("live {} != counted {live}", self.live));
        }
        for (slot, e) in self.entries.iter().enumerate().skip(1) {
            if e.refs == 0 {
                if !self.free.contains(&(slot as u32)) {
                    return Err(format!("free slot {slot} not on the free list"));
                }
                continue;
            }
            let id = StrId(NonZeroU32::MIN.saturating_add(slot as u32));
            if self.find(self.text_of(*e)) != Some(id) {
                return Err(format!("slot {slot} not findable"));
            }
        }
        Ok(())
    }
}
