//! The one cell store (plan §4.0). Record: `uo-calc.md` "S. The cell store".
//!
//! Column-major, chunked, struct-of-arrays: a sheet is a directory of columns, a column a sorted list of
//! chunks of [`CHUNK_ROWS`] rows, a chunk four lanes (presence, kinds, payload, styles), each lane in its
//! narrowest form. giant-1m (10,000 x 100 integers, no style) costs ~4.6 B/cell; the TS heap holds 239 B/cell
//! for the same sheet (model 132.5 + cell index 37 + engine 69.5, measured 2026-11-24).
//!
//! What is NOT per cell: rare file details (raw `<v>` text that is not the canonical print of the value,
//! raw `r`/`s`/`t` attributes, `cm`/`vm`, `<is>` shapes other than `{text}`, rich runs) live in the sheet's
//! sorted side table [`CellExtra`]; formulas in [`crate::formula::FormulaTable`]; out-of-order and duplicate
//! `<c>`/`<row>` elements in [`crate::load::FileOrder`]; spill/CSE ownership in the engine's area index (the
//! cell only carries the `OWNED` bit).
//!
//! String references: a cell whose kind holds a string ([`CellKind::holds_str`]) owns ONE reference of its
//! [`StrId`] in the pool; every write path takes the new reference and releases the old one.

use std::collections::BTreeMap;

use crate::error::{CalcError, Result};
use crate::formula::FormulaTable;
use crate::key::{Area, CellKey, MAX_COL, MAX_ROW, SheetIx};
use crate::load::FileOrder;
use crate::strings::{JsString, StrId, StrPool};

/// Rows per chunk. 256: a viewport (≈ 40-120 rows) touches at most two chunks per column, a column scan is
/// a run of contiguous lanes, row offsets fit a `u8`, and a chunk header is amortized under 0.6 B/cell when dense.
pub const CHUNK_ROWS: u32 = 256;

/// The value variant of a cell (low 4 bits of [`CellKind`]): the file type attribute `t` AND the calc value
/// family, so `cellValue` (`from-sheet-core.ts:73-101`) is a pure function of (kind, payload) except for
/// non-canonical file texts (then the raw text in [`CellExtra`] decides, as the TS reads it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ValueTag {
    /// No `t` (number when `HAS_VALUE`; a styled-only cell without it). Payload: number.
    None = 0,
    /// `t="n"` written explicitly (kept for the save). Payload: number.
    N = 1,
    /// `t="b"`. Payload: 0/1 (`'1'`/`'true'` → 1).
    Bool = 2,
    /// `t="e"`. Payload: the [`crate::value::ErrorCode`] (`errorOf` of the text).
    Err = 3,
    /// `t="s"`. Payload: the shared-string index (the save writes it back, never re-interns).
    Sst = 4,
    /// `t="inlineStr"`. Payload: a [`StrId`] of the calc text (`textOf(is)`; `""` without `<is>`).
    Inline = 5,
    /// `t="str"` (formula/spill text). Payload: a [`StrId`] of `<v>` when `HAS_VALUE`.
    Str = 6,
    /// `t="d"`. Payload: a [`StrId`] of the ISO text when `HAS_VALUE` (the serial is computed on read).
    Date = 7,
    /// Any other `t` (`""` included; the TS casts it, `read.ts:603`): the raw `t` in the extra. Payload: number.
    Other = 8,
}

impl ValueTag {
    /// From the low nibble (unknown nibbles read as `Other`; never produced by the store).
    #[must_use]
    pub const fn from_nibble(n: u8) -> Self {
        match n & 0x0F {
            0 => Self::None,
            1 => Self::N,
            2 => Self::Bool,
            3 => Self::Err,
            4 => Self::Sst,
            5 => Self::Inline,
            6 => Self::Str,
            7 => Self::Date,
            _ => Self::Other,
        }
    }
    /// The payload is a number (`f64`).
    #[must_use]
    pub const fn is_number(self) -> bool {
        matches!(self, Self::None | Self::N | Self::Other)
    }
}

/// One byte per cell: [`ValueTag`] in the low nibble + flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[repr(transparent)]
pub struct CellKind(pub u8);

impl CellKind {
    /// The cell has a formula (an entry of the sheet's [`FormulaTable`]).
    pub const HAS_FORMULA: u8 = 0x10;
    /// The cell is filled by another cell's array (CSE area or dynamic spill): `CellRec.owner` of the TS.
    pub const OWNED: u8 = 0x20;
    /// The sheet's [`CellExtra`] table has an entry for this cell.
    pub const HAS_EXTRA: u8 = 0x40;
    /// The cell has a `<v>` (the model's `value !== undefined`).
    pub const HAS_VALUE: u8 = 0x80;
    /// Marker of an absent cell in a [`Viewport`] gather (nibble 15 is never a stored tag).
    pub const ABSENT: CellKind = CellKind(0x0F);

    /// A kind from a tag and flags.
    #[must_use]
    pub const fn new(tag: ValueTag, flags: u8) -> Self {
        Self(tag as u8 | (flags & 0xF0))
    }
    /// The value tag.
    #[must_use]
    pub const fn tag(self) -> ValueTag {
        ValueTag::from_nibble(self.0)
    }
    /// Has every bit of `flag`.
    #[must_use]
    pub const fn has(self, flag: u8) -> bool {
        self.0 & flag == flag
    }
    /// With `flag` set or cleared.
    #[must_use]
    pub const fn with(self, flag: u8, on: bool) -> Self {
        if on { Self(self.0 | flag) } else { Self(self.0 & !flag) }
    }
    /// The payload is a pooled string the cell holds a reference of.
    #[must_use]
    pub const fn holds_str(self) -> bool {
        match self.tag() {
            ValueTag::Inline => true,
            ValueTag::Str | ValueTag::Date => self.has(Self::HAS_VALUE),
            _ => false,
        }
    }
    /// `true` for [`CellKind::ABSENT`].
    #[must_use]
    pub const fn is_absent(self) -> bool {
        self.0 == Self::ABSENT.0
    }
}

/// One cell as the lanes hold it, widened: number payloads as `f64` bits, the rest zero-extended ids/codes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct RawCell {
    /// Kind byte.
    pub kind: CellKind,
    /// Payload (see [`ValueTag`]).
    pub payload: u64,
    /// `s` + 1 (0 = no `s` attribute).
    pub style: u32,
}

impl RawCell {
    /// The number payload.
    #[must_use]
    pub fn number(self) -> f64 {
        f64::from_bits(self.payload)
    }
    /// The pooled string of a string-holding kind.
    #[must_use]
    pub fn str_id(self) -> Option<StrId> {
        if self.kind.holds_str() { StrId::from_raw(self.payload as u32) } else { None }
    }
}

/// The narrow (`u32`) form of a payload, if it has one: an `i32` integer number (not `-0`), or any id/code.
fn narrow_of(kind: CellKind, payload: u64) -> Option<u32> {
    if kind.tag().is_number() {
        let x = f64::from_bits(payload);
        let i = uo_jsnum::conv::to_int32(x);
        (f64::from(i).to_bits() == payload).then_some(i as u32)
    } else {
        u32::try_from(payload).ok()
    }
}

fn widen(kind: CellKind, n: u32) -> u64 {
    if kind.tag().is_number() { f64::from(n as i32).to_bits() } else { u64::from(n) }
}

/// Which rows of the chunk exist.
#[derive(Clone, Debug)]
pub enum Presence {
    /// All [`CHUNK_ROWS`] rows (lanes are indexed by `row - base`).
    Dense,
    /// The existing rows' offsets from `base`, ascending (lanes are parallel to it; `1..=255` entries).
    Rows(Vec<u8>),
}

/// The kinds of the chunk's cells.
#[derive(Clone, Debug)]
pub enum Kinds {
    /// Every cell has the same kind (the common case: a column of plain numbers).
    Uniform(CellKind),
    /// One byte per present cell.
    PerCell(Vec<CellKind>),
}

/// The value payloads of the chunk's cells.
#[derive(Clone, Debug)]
pub enum Payload {
    /// Every payload fits 32 bits: numbers are `i32` integers (not `-0`), the rest ids/codes/booleans.
    Narrow(Vec<u32>),
    /// Numbers are `f64` bits (a non-integer, `-0`, NaN or out-of-`i32` number widens the chunk once).
    Wide(Vec<u64>),
}

/// The style lane: the `s` attribute + 1 (0 = no `s`: the TS `cell.style === undefined`, written without
/// `s`, read as xf 0; `serialize.ts:64`).
#[derive(Clone, Debug)]
pub enum StyleLane {
    /// Every present cell has the same value.
    Uniform(u32),
    /// Every value `< 65,536` (Excel's own limit is 64,000 xfs).
    U16(Vec<u16>),
    /// Anything else.
    U32(Vec<u32>),
}

/// A chunk: rows `base .. base + CHUNK_ROWS` of one column. Never empty.
#[derive(Clone, Debug)]
pub struct Chunk {
    /// First row (a multiple of [`CHUNK_ROWS`]).
    pub base: u32,
    /// Existing rows.
    pub present: Presence,
    /// Kinds.
    pub kinds: Kinds,
    /// Payloads.
    pub payload: Payload,
    /// Styles.
    pub styles: StyleLane,
}

impl Chunk {
    fn single(base: u32, off: u8, raw: RawCell) -> Self {
        Self {
            base,
            present: Presence::Rows(vec![off]),
            kinds: Kinds::Uniform(raw.kind),
            payload: match narrow_of(raw.kind, raw.payload) {
                Some(n) => Payload::Narrow(vec![n]),
                None => Payload::Wide(vec![raw.payload]),
            },
            styles: StyleLane::Uniform(raw.style),
        }
    }

    /// Present cells.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.present {
            Presence::Dense => CHUNK_ROWS as usize,
            Presence::Rows(r) => r.len(),
        }
    }

    /// Never true for a stored chunk.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lane index of the row offset `off` (`Err` = insertion point).
    #[inline]
    pub fn index_of(&self, off: u8) -> core::result::Result<usize, usize> {
        match &self.present {
            Presence::Dense => Ok(usize::from(off)),
            Presence::Rows(r) => r.binary_search(&off),
        }
    }

    /// First lane index whose row is `>= row`.
    #[must_use]
    pub fn lower_bound(&self, row: u32) -> usize {
        if row <= self.base {
            return 0;
        }
        let off = row - self.base;
        if off >= CHUNK_ROWS {
            return self.len();
        }
        match self.index_of(off as u8) {
            Ok(i) | Err(i) => i,
        }
    }

    /// Row of lane `i`.
    #[inline]
    #[must_use]
    pub fn row_at(&self, i: usize) -> u32 {
        match &self.present {
            Presence::Dense => self.base + i as u32,
            Presence::Rows(r) => self.base + u32::from(r.get(i).copied().unwrap_or(0)),
        }
    }

    /// Kind of lane `i`.
    #[inline]
    #[must_use]
    pub fn kind_at(&self, i: usize) -> CellKind {
        match &self.kinds {
            Kinds::Uniform(k) => *k,
            Kinds::PerCell(v) => v.get(i).copied().unwrap_or_default(),
        }
    }

    /// Cell of lane `i`.
    #[inline]
    #[must_use]
    pub fn get(&self, i: usize) -> RawCell {
        let kind = self.kind_at(i);
        let payload = match &self.payload {
            Payload::Narrow(v) => widen(kind, v.get(i).copied().unwrap_or(0)),
            Payload::Wide(v) => v.get(i).copied().unwrap_or(0),
        };
        let style = match &self.styles {
            StyleLane::Uniform(s) => *s,
            StyleLane::U16(v) => u32::from(v.get(i).copied().unwrap_or(0)),
            StyleLane::U32(v) => v.get(i).copied().unwrap_or(0),
        };
        RawCell { kind, payload, style }
    }

    fn materialize_kinds(&mut self) -> Option<&mut Vec<CellKind>> {
        if let Kinds::Uniform(k) = self.kinds {
            self.kinds = Kinds::PerCell(vec![k; self.len()]);
        }
        match &mut self.kinds {
            Kinds::PerCell(v) => Some(v),
            Kinds::Uniform(_) => None,
        }
    }

    fn widen_payload(&mut self) {
        if let Payload::Narrow(v) = &self.payload {
            let wide = match &self.kinds {
                Kinds::Uniform(k) => v.iter().map(|&n| widen(*k, n)).collect(),
                Kinds::PerCell(ks) => v.iter().zip(ks).map(|(&n, &k)| widen(k, n)).collect(),
            };
            self.payload = Payload::Wide(wide);
        }
    }

    fn styles_for(&mut self, s: u32) {
        match &self.styles {
            StyleLane::Uniform(u) => {
                let (u, n) = (*u, self.len());
                self.styles = if u <= 0xFFFF && s <= 0xFFFF { StyleLane::U16(vec![u as u16; n]) } else { StyleLane::U32(vec![u; n]) };
            }
            StyleLane::U16(v) if s > 0xFFFF => self.styles = StyleLane::U32(v.iter().map(|&x| u32::from(x)).collect()),
            _ => {}
        }
    }

    /// Overwrites lane `i`.
    fn put(&mut self, i: usize, raw: RawCell) {
        // widen with the OLD kinds first: the widening reads each lane's kind
        let narrow = narrow_of(raw.kind, raw.payload);
        if narrow.is_none() {
            self.widen_payload();
        }
        if !matches!(self.kinds, Kinds::Uniform(k) if k == raw.kind)
            && let Some(slot) = self.materialize_kinds().and_then(|v| v.get_mut(i))
        {
            *slot = raw.kind;
        }
        match (&mut self.payload, narrow) {
            (Payload::Narrow(v), Some(n)) => set_at(v, i, n),
            (Payload::Wide(v), _) => set_at(v, i, raw.payload),
            (Payload::Narrow(_), None) => {}
        }
        if !matches!(self.styles, StyleLane::Uniform(s) if s == raw.style) {
            self.styles_for(raw.style);
            match &mut self.styles {
                StyleLane::U16(v) => set_at(v, i, raw.style as u16),
                StyleLane::U32(v) => set_at(v, i, raw.style),
                StyleLane::Uniform(_) => {}
            }
        }
    }

    /// Inserts a new cell at lane `i` with row offset `off` (`self` is a `Rows` chunk).
    fn insert(&mut self, i: usize, off: u8, raw: RawCell) {
        // widen with the OLD kinds first (the kinds lane is still parallel to the payloads)
        let narrow = narrow_of(raw.kind, raw.payload);
        if narrow.is_none() {
            self.widen_payload();
        }
        if !matches!(self.kinds, Kinds::Uniform(k) if k == raw.kind)
            && let Some(v) = self.materialize_kinds()
        {
            v.insert(i.min(v.len()), raw.kind);
        }
        match (&mut self.payload, narrow) {
            (Payload::Narrow(v), Some(n)) => v.insert(i.min(v.len()), n),
            (Payload::Wide(v), _) => v.insert(i.min(v.len()), raw.payload),
            (Payload::Narrow(_), None) => {}
        }
        if !matches!(self.styles, StyleLane::Uniform(s) if s == raw.style) {
            self.styles_for(raw.style);
            match &mut self.styles {
                StyleLane::U16(v) => v.insert(i.min(v.len()), raw.style as u16),
                StyleLane::U32(v) => v.insert(i.min(v.len()), raw.style),
                StyleLane::Uniform(_) => {}
            }
        }
        if let Presence::Rows(r) = &mut self.present {
            r.insert(i.min(r.len()), off);
            if r.len() == CHUNK_ROWS as usize {
                self.present = Presence::Dense;
            }
        }
    }

    /// Removes lane `i` (the caller drops the chunk when it becomes empty).
    fn remove(&mut self, i: usize) {
        if let Presence::Dense = self.present {
            self.present = Presence::Rows((0..=255u8).collect());
        }
        if let Presence::Rows(r) = &mut self.present {
            remove_at(r, i);
        }
        if let Kinds::PerCell(v) = &mut self.kinds {
            remove_at(v, i);
        }
        match &mut self.payload {
            Payload::Narrow(v) => remove_at(v, i),
            Payload::Wide(v) => remove_at(v, i),
        }
        match &mut self.styles {
            StyleLane::U16(v) => remove_at(v, i),
            StyleLane::U32(v) => remove_at(v, i),
            StyleLane::Uniform(_) => {}
        }
    }

    /// Re-chooses the narrowest lanes (load end, compaction): uniform kinds/styles, narrow payloads.
    fn renarrow(&mut self) {
        if let Kinds::PerCell(v) = &self.kinds
            && let Some(&k) = v.first()
            && v.iter().all(|&x| x == k)
        {
            self.kinds = Kinds::Uniform(k);
        }
        if let Payload::Wide(_) = &self.payload {
            let n = self.len();
            let narrow: Option<Vec<u32>> = (0..n)
                .map(|i| {
                    let c = self.get(i);
                    narrow_of(c.kind, c.payload)
                })
                .collect();
            if let Some(v) = narrow {
                self.payload = Payload::Narrow(v);
            }
        }
        let first = self.get(0).style;
        let uniform = match &self.styles {
            StyleLane::Uniform(_) => false,
            StyleLane::U16(v) => v.iter().all(|&x| u32::from(x) == first),
            StyleLane::U32(v) => v.iter().all(|&x| x == first),
        };
        if uniform {
            self.styles = StyleLane::Uniform(first);
        } else if let StyleLane::U32(v) = &self.styles
            && v.iter().all(|&x| x <= 0xFFFF)
        {
            self.styles = StyleLane::U16(v.iter().map(|&x| x as u16).collect());
        }
        self.shrink();
    }

    fn shrink(&mut self) {
        if let Presence::Rows(r) = &mut self.present {
            r.shrink_to_fit();
        }
        if let Kinds::PerCell(v) = &mut self.kinds {
            v.shrink_to_fit();
        }
        match &mut self.payload {
            Payload::Narrow(v) => v.shrink_to_fit(),
            Payload::Wide(v) => v.shrink_to_fit(),
        }
        match &mut self.styles {
            StyleLane::U16(v) => v.shrink_to_fit(),
            StyleLane::U32(v) => v.shrink_to_fit(),
            StyleLane::Uniform(_) => {}
        }
    }

    fn heap_bytes(&self) -> usize {
        let p = match &self.present {
            Presence::Dense => 0,
            Presence::Rows(r) => r.capacity(),
        };
        let k = match &self.kinds {
            Kinds::Uniform(_) => 0,
            Kinds::PerCell(v) => v.capacity(),
        };
        let v = match &self.payload {
            Payload::Narrow(v) => v.capacity() * 4,
            Payload::Wide(v) => v.capacity() * 8,
        };
        let s = match &self.styles {
            StyleLane::Uniform(_) => 0,
            StyleLane::U16(v) => v.capacity() * 2,
            StyleLane::U32(v) => v.capacity() * 4,
        };
        p + k + v + s
    }

    fn check(&self) -> core::result::Result<(), String> {
        let n = self.len();
        if n == 0 {
            return Err(format!("empty chunk at {}", self.base));
        }
        if !self.base.is_multiple_of(CHUNK_ROWS) {
            return Err(format!("unaligned base {}", self.base));
        }
        if let Presence::Rows(r) = &self.present
            && (r.len() >= CHUNK_ROWS as usize || r.windows(2).any(|w| w[0] >= w[1]))
        {
            return Err(format!("bad offsets at {}", self.base));
        }
        let lens = [
            match &self.kinds {
                Kinds::Uniform(_) => n,
                Kinds::PerCell(v) => v.len(),
            },
            match &self.payload {
                Payload::Narrow(v) => v.len(),
                Payload::Wide(v) => v.len(),
            },
            match &self.styles {
                StyleLane::Uniform(_) => n,
                StyleLane::U16(v) => v.len(),
                StyleLane::U32(v) => v.len(),
            },
        ];
        if lens.iter().any(|&l| l != n) {
            return Err(format!("lane lengths {lens:?} != {n} at {}", self.base));
        }
        Ok(())
    }
}

fn set_at<T>(v: &mut [T], i: usize, x: T) {
    if let Some(slot) = v.get_mut(i) {
        *slot = x;
    }
}

fn remove_at<T>(v: &mut Vec<T>, i: usize) {
    if i < v.len() {
        v.remove(i);
    }
}

/// The chunks of one column, sorted by `base` (no empty chunk is kept).
#[derive(Clone, Debug, Default)]
pub struct Column {
    /// Chunks.
    pub chunks: Vec<Chunk>,
}

impl Column {
    /// Index of the chunk holding `base` (`Err` = insertion point).
    #[inline]
    pub fn find(&self, base: u32) -> core::result::Result<usize, usize> {
        // a column filled from its top has chunk i at base i·256: one probe, no search
        let guess = (base / CHUNK_ROWS) as usize;
        if self.chunks.get(guess).is_some_and(|c| c.base == base) {
            return Ok(guess);
        }
        // appending (load, fill down) hits the last chunk
        match self.chunks.last() {
            Some(c) if c.base == base => Ok(self.chunks.len() - 1),
            Some(c) if c.base < base => Err(self.chunks.len()),
            _ => self.chunks.binary_search_by_key(&base, |c| c.base),
        }
    }

    /// Index of the first chunk whose rows reach `row`.
    #[must_use]
    pub fn first_from(&self, row: u32) -> usize {
        let base = row & !(CHUNK_ROWS - 1);
        match self.find(base) {
            Ok(i) | Err(i) => i,
        }
    }

    /// Present cells.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.iter().map(Chunk::len).sum()
    }

    /// No cell.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

/// File details of a cell that are not the canonical form of its lanes (rare; sorted for deterministic
/// iteration). Everything the TS model keeps and the save writes back verbatim.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellExtra {
    /// Raw `<v>` text when it is not the canonical print of the value (`0.10000000000000001`, `" 5"`, `1E3`),
    /// and every `<v>` of an inline-string cell.
    pub raw_value: Option<JsString>,
    /// Raw `r` attribute when it is not the canonical A1 of the position (`$A$1`, another row: SH1).
    pub raw_ref: Option<JsString>,
    /// Raw `t` text for [`ValueTag::Other`].
    pub raw_type: Option<JsString>,
    /// `s` that the lane cannot hold (`parseInt` gave NaN, `-0`, a negative or a huge value): the JS number.
    pub raw_style: Option<f64>,
    /// `cm` (1-based cell metadata; dynamic arrays), as `parseInt` read it.
    pub cm: Option<f64>,
    /// `vm` (1-based value metadata).
    pub vm: Option<f64>,
    /// The `<is>` element when it is not what the kind implies (inline kind: `{text: payload}`; other
    /// kinds: absent).
    pub inline: Option<InlineExtra>,
}

impl CellExtra {
    /// Nothing recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// An `<is>` shape kept in the extra.
#[derive(Clone, Debug, PartialEq)]
pub enum InlineExtra {
    /// No `<is>` (an inline-string cell without it reads as `""`).
    Absent,
    /// `<is>` with an optional plain text and optional rich runs (index into [`Store::rich`]).
    Shape {
        /// `text`.
        text: Option<JsString>,
        /// `runs`.
        rich: Option<u32>,
    },
}

/// One run of a rich string (`SharedString.runs[i]`).
#[derive(Clone, Debug, PartialEq)]
pub struct RichRun {
    /// The run text.
    pub text: JsString,
    /// Raw attributes of `<rPr>` (`rPr`), when present.
    pub rpr: Option<Box<[(JsString, JsString)]>>,
    /// The parsed run properties (`props`): an id owned by the host (uo-xlsx's run-property table; the store
    /// never reads it).
    pub props: Option<u32>,
}

/// Row attributes (`model.ts:170-189`), only for rows that have any (or a `<row>` element with no cell).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowMeta {
    /// `ht` (points, as `parseFloat` read it).
    pub height: Option<f64>,
    /// `s` as `parseInt` read it.
    pub style: Option<f64>,
    /// `spans` (interned; the meta holds a reference).
    pub spans: Option<StrId>,
    /// [`RowMeta::CUSTOM_HEIGHT`] …
    pub flags: u8,
}

impl RowMeta {
    /// `customHeight`.
    pub const CUSTOM_HEIGHT: u8 = 1;
    /// `hidden`.
    pub const HIDDEN: u8 = 2;
    /// `customFormat`.
    pub const CUSTOM_FORMAT: u8 = 4;
    /// `thickTop`.
    pub const THICK_TOP: u8 = 8;
    /// `thickBot`.
    pub const THICK_BOT: u8 = 16;
    /// The `<row>` element exists without attributes and cells (kept for the save).
    pub const ELEMENT_ONLY: u8 = 32;

    /// Any attribute besides `r`.
    #[must_use]
    pub fn has_attrs(&self) -> bool {
        self.height.is_some() || self.style.is_some() || self.spans.is_some() || self.flags & 0x1F != 0
    }
}

/// A set of rows (hidden, filtered): sorted disjoint ranges.
#[derive(Clone, Debug, Default)]
pub struct RowSet {
    /// `(first, last)` inclusive, ascending, disjoint, not adjacent.
    pub ranges: Vec<(u32, u32)>,
}

impl RowSet {
    /// Whether `row` is in the set.
    #[must_use]
    pub fn contains(&self, row: u32) -> bool {
        let i = self.ranges.partition_point(|&(_, b)| b < row);
        self.ranges.get(i).is_some_and(|&(a, _)| a <= row)
    }
    /// Adds `row`.
    pub fn insert(&mut self, row: u32) {
        let i = self.ranges.partition_point(|&(_, b)| b.saturating_add(1) < row);
        match self.ranges.get_mut(i) {
            Some(r) if r.0 <= row.saturating_add(1) => {
                r.0 = r.0.min(row);
                r.1 = r.1.max(row);
                // merge with the next range when they now touch
                if let Some(&(na, nb)) = self.ranges.get(i + 1)
                    && na <= self.ranges[i].1.saturating_add(1)
                {
                    self.ranges[i].1 = self.ranges[i].1.max(nb);
                    self.ranges.remove(i + 1);
                }
            }
            _ => self.ranges.insert(i, (row, row)),
        }
    }
}

/// One sheet of the store.
#[derive(Clone, Debug, Default)]
pub struct SheetCells {
    /// Columns by index (`len` = last used column + 1).
    pub columns: Vec<Column>,
    /// Present cells.
    pub cells: u64,
    /// Largest row that ever held a cell (`SheetStore.maxRow`, `engine.ts:94`: not lowered by deletes).
    pub max_row: Option<u32>,
    /// Row attributes, sorted by row; the LAST `<row>` element of a row number wins.
    pub rows: Vec<(u32, RowMeta)>,
    /// Column default styles (`<col style>` ≠ 0, later definitions win; `cell-format.ts:16-20`) as sorted
    /// disjoint runs `(first, last, style)`, 0-based.
    pub col_styles: Vec<(u32, u32, u32)>,
    /// Hidden rows (all, filtered included; `workbook.ts:146-149`).
    pub hidden: RowSet,
    /// Rows hidden by a filter.
    pub filtered: RowSet,
    /// Merged areas (block spills; `engine.ts:980`).
    pub merges: Vec<Area>,
    /// Side table of [`CellExtra`] by `ts_key` (`row * 16384 + col`).
    pub extras: BTreeMap<u64, CellExtra>,
    /// Formulas (file side).
    pub formulas: FormulaTable,
    /// Non-canonical file order, if any.
    pub file_order: Option<Box<FileOrder>>,
}

impl SheetCells {
    fn heap_bytes(&self) -> usize {
        let cols = self.columns.capacity() * core::mem::size_of::<Column>();
        let chunks: usize = self
            .columns
            .iter()
            .map(|c| c.chunks.capacity() * core::mem::size_of::<Chunk>() + c.chunks.iter().map(Chunk::heap_bytes).sum::<usize>())
            .sum();
        cols + chunks + self.rows.capacity() * core::mem::size_of::<(u32, RowMeta)>() + self.col_styles.capacity() * 12
    }

    /// The row's attributes (the last `<row>` element with that number).
    #[must_use]
    pub fn row_meta(&self, row: u32) -> Option<&RowMeta> {
        self.rows.binary_search_by_key(&row, |&(r, _)| r).ok().and_then(|i| self.rows.get(i)).map(|(_, m)| m)
    }
}

/// A shared-string entry: the interned plain text (`textOf(entry)`) and, when rich, its runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SstEntry {
    /// The calc text (the pool reference is held by the entry).
    pub text: StrId,
    /// Rich runs (index into [`Store::rich`]).
    pub rich: Option<u32>,
}

/// A decoded shared string handed to the store by an [`SstSource`].
#[derive(Clone, Debug, Default)]
pub struct SstDecoded {
    /// `textOf(entry)`.
    pub text: JsString,
    /// Runs, when rich.
    pub runs: Option<Vec<RichRun>>,
}

/// The reader's lazy shared-string decoder (S3: the SST is indexed at open, decoded on demand).
pub trait SstSource {
    /// Decodes entry `index` (`None`: no such entry).
    fn decode(&mut self, index: u32) -> Option<SstDecoded>;
}

/// The calc needs a shared string that is not decoded yet (decode it with [`Store::resolve_sst`], then retry).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NeedSst(pub u32);

/// Bounds for untrusted workbooks (on top of the zip limits of `uo-opc`).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Most cells in the workbook.
    pub max_cells: u64,
    /// Most sheets.
    pub max_sheets: u32,
    /// Most formulas per sheet.
    pub max_formulas: u32,
}

impl Default for Limits {
    fn default() -> Self {
        // 64M cells is ~300 MB of lanes at the dense-decimal rate: past it an open is hostile on any device
        Self { max_cells: 1 << 26, max_sheets: 4096, max_formulas: 1 << 24 }
    }
}

/// A row-major block of cells gathered for a viewport (buffers reused across frames).
#[derive(Clone, Debug, Default)]
pub struct Viewport {
    /// First row.
    pub r1: u32,
    /// First column.
    pub c1: u32,
    /// Rows.
    pub rows: u32,
    /// Columns.
    pub cols: u32,
    /// Kinds, row-major ([`CellKind::ABSENT`] for no cell).
    pub kinds: Vec<CellKind>,
    /// Payloads (widened, as [`RawCell::payload`]).
    pub payload: Vec<u64>,
    /// Styles (`s` + 1).
    pub styles: Vec<u32>,
}

impl Viewport {
    /// The cell at `(row, col)` relative to the block's origin, if present.
    #[must_use]
    pub fn get(&self, dr: u32, dc: u32) -> Option<RawCell> {
        let i = (dr * self.cols + dc) as usize;
        let kind = *self.kinds.get(i)?;
        (!kind.is_absent()).then(|| RawCell {
            kind,
            payload: self.payload.get(i).copied().unwrap_or(0),
            style: self.styles.get(i).copied().unwrap_or(0),
        })
    }
}

/// The present cells of one chunk inside a column range: lanes `start..end`.
#[derive(Clone, Copy, Debug)]
pub struct ColumnRun<'a> {
    /// The chunk.
    pub chunk: &'a Chunk,
    /// First lane.
    pub start: usize,
    /// End lane (exclusive).
    pub end: usize,
}

impl ColumnRun<'_> {
    /// Cells in the run.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    /// No cell.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.end == self.start
    }
    /// Row of the run's `k`-th cell.
    #[must_use]
    pub fn row(&self, k: usize) -> u32 {
        self.chunk.row_at(self.start + k)
    }
    /// The run's `k`-th cell.
    #[must_use]
    pub fn get(&self, k: usize) -> RawCell {
        self.chunk.get(self.start + k)
    }
    /// Folds the numbers of the run in row order (cells with a number tag and a value whose payload is
    /// finite: the `cellValue` numbers), with a fast path for a uniform narrow run.
    pub fn fold_numbers<A>(&self, mut acc: A, mut f: impl FnMut(A, f64) -> A) -> A {
        if let (Kinds::Uniform(k), Payload::Narrow(v)) = (&self.chunk.kinds, &self.chunk.payload) {
            if !k.tag().is_number() || !k.has(CellKind::HAS_VALUE) {
                return acc;
            }
            for &n in v.get(self.start..self.end).unwrap_or(&[]) {
                acc = f(acc, f64::from(n as i32));
            }
            return acc;
        }
        for i in self.start..self.end {
            let c = self.chunk.get(i);
            if c.kind.tag().is_number() && c.kind.has(CellKind::HAS_VALUE) {
                let x = c.number();
                if x.is_finite() {
                    acc = f(acc, x);
                }
            }
        }
        acc
    }
}

/// The cell store of one workbook.
#[derive(Debug, Default)]
pub struct Store {
    /// Sheets in workbook order (external-link caches are in the engine, read-only).
    pub sheets: Vec<SheetCells>,
    /// Interned strings.
    pub strings: StrPool,
    /// Shared strings (`xl/sharedStrings.xml`), decoded lazily by index; `None` = not decoded yet.
    pub sst: Vec<Option<SstEntry>>,
    /// Rich-text runs of inline strings and shared strings (free slots `None`).
    pub rich: Vec<Option<Box<[RichRun]>>>,
    rich_free: Vec<u32>,
    /// 1904 date system (`t="d"` serials).
    pub date1904: bool,
    /// Limits.
    pub limits: Limits,
    cells: u64,
}

pub(crate) fn ts_key(row: u32, col: u32) -> u64 {
    u64::from(row) * u64::from(MAX_COL + 1) + u64::from(col)
}

impl Store {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty store with explicit limits.
    #[must_use]
    pub fn with_limits(limits: Limits) -> Self {
        Self { limits, ..Self::default() }
    }

    /// Appends an empty sheet.
    pub fn add_sheet(&mut self) -> Result<SheetIx> {
        if self.sheets.len() >= self.limits.max_sheets as usize {
            return Err(CalcError::Limit("sheets"));
        }
        self.sheets.push(SheetCells::default());
        Ok(SheetIx((self.sheets.len() - 1) as u32))
    }

    /// The sheet.
    #[must_use]
    pub fn sheet(&self, sheet: SheetIx) -> Option<&SheetCells> {
        self.sheets.get(sheet.0 as usize)
    }

    pub(crate) fn sheet_mut(&mut self, sheet: SheetIx) -> Result<&mut SheetCells> {
        self.sheets.get_mut(sheet.0 as usize).ok_or(CalcError::NoSheet(sheet.0))
    }

    /// Present cells of the workbook.
    #[must_use]
    pub fn cell_count(&self) -> u64 {
        self.cells
    }

    /// The cell at (0-based) `row`, `col`.
    #[inline]
    #[must_use]
    pub fn get(&self, sheet: SheetIx, row: u32, col: u32) -> Option<RawCell> {
        let column = self.sheets.get(sheet.0 as usize)?.columns.get(col as usize)?;
        let chunk = column.chunks.get(column.find(row & !(CHUNK_ROWS - 1)).ok()?)?;
        let i = chunk.index_of((row & (CHUNK_ROWS - 1)) as u8).ok()?;
        Some(chunk.get(i))
    }

    /// The cell by key.
    #[must_use]
    pub fn get_key(&self, key: CellKey) -> Option<RawCell> {
        self.get(key.sheet(), key.row(), key.col())
    }

    /// The extra of a cell.
    #[must_use]
    pub fn extra(&self, sheet: SheetIx, row: u32, col: u32) -> Option<&CellExtra> {
        self.sheet(sheet)?.extras.get(&ts_key(row, col))
    }

    /// Writes the lanes of a cell, creating it if absent. A string-holding `raw` transfers one reference of its
    /// id to the store; the old cell's reference is released. Flags `HAS_EXTRA`/`HAS_FORMULA` are the caller's:
    /// they must match the side tables.
    pub(crate) fn put_raw(&mut self, sheet: SheetIx, row: u32, col: u32, raw: RawCell) -> Result<Option<RawCell>> {
        if row > MAX_ROW || col > MAX_COL {
            return Err(CalcError::Limit("cell outside the sheet"));
        }
        let si = sheet.0 as usize;
        let Some(sh) = self.sheets.get_mut(si) else {
            if let Some(id) = raw.str_id() {
                self.strings.release(id);
            }
            return Err(CalcError::NoSheet(sheet.0));
        };
        if sh.columns.len() <= col as usize {
            sh.columns.resize_with(col as usize + 1, Column::default);
        }
        let column = &mut sh.columns[col as usize];
        let base = row & !(CHUNK_ROWS - 1);
        let off = (row & (CHUNK_ROWS - 1)) as u8;
        let old = match column.find(base) {
            Ok(ci) => {
                let chunk = &mut column.chunks[ci];
                match chunk.index_of(off) {
                    Ok(i) => {
                        let old = chunk.get(i);
                        chunk.put(i, raw);
                        Some(old)
                    }
                    Err(i) => {
                        if self.cells >= self.limits.max_cells {
                            if let Some(id) = raw.str_id() {
                                self.strings.release(id);
                            }
                            return Err(CalcError::Limit("cells"));
                        }
                        chunk.insert(i, off, raw);
                        None
                    }
                }
            }
            Err(ci) => {
                if self.cells >= self.limits.max_cells {
                    if let Some(id) = raw.str_id() {
                        self.strings.release(id);
                    }
                    return Err(CalcError::Limit("cells"));
                }
                column.chunks.insert(ci, Chunk::single(base, off, raw));
                None
            }
        };
        if old.is_none() {
            self.cells += 1;
            sh.cells += 1;
            sh.max_row = Some(sh.max_row.map_or(row, |m| m.max(row)));
        }
        if let Some(id) = old.and_then(RawCell::str_id) {
            self.strings.release(id);
        }
        Ok(old)
    }

    /// Changes only the kind byte's flags of an existing cell (no reference traffic).
    pub(crate) fn set_flags(&mut self, sheet: SheetIx, row: u32, col: u32, flag: u8, on: bool) {
        let Some(old) = self.get(sheet, row, col) else { return };
        let kind = old.kind.with(flag, on);
        if kind == old.kind {
            return;
        }
        if let Some(chunk) = self.chunk_mut(sheet, row, col)
            && let Ok(i) = chunk.index_of((row & (CHUNK_ROWS - 1)) as u8)
        {
            chunk.put(i, RawCell { kind, ..old });
        }
    }

    fn chunk_mut(&mut self, sheet: SheetIx, row: u32, col: u32) -> Option<&mut Chunk> {
        let column = self.sheets.get_mut(sheet.0 as usize)?.columns.get_mut(col as usize)?;
        let ci = column.find(row & !(CHUNK_ROWS - 1)).ok()?;
        column.chunks.get_mut(ci)
    }

    /// Deletes a cell with its extra and formula (`SheetStore.delete` + the model's `removeCell`). `false` if
    /// there was none.
    pub fn remove(&mut self, sheet: SheetIx, row: u32, col: u32) -> bool {
        let Some(sh) = self.sheets.get_mut(sheet.0 as usize) else { return false };
        let Some(column) = sh.columns.get_mut(col as usize) else { return false };
        let Ok(ci) = column.find(row & !(CHUNK_ROWS - 1)) else { return false };
        let chunk = &mut column.chunks[ci];
        let Ok(i) = chunk.index_of((row & (CHUNK_ROWS - 1)) as u8) else { return false };
        let old = chunk.get(i);
        chunk.remove(i);
        if chunk.is_empty() {
            column.chunks.remove(ci);
        }
        // trailing empty columns go away (the directory is `last used column + 1`)
        while sh.columns.last().is_some_and(Column::is_empty) {
            sh.columns.pop();
        }
        sh.cells -= 1;
        let extra = if old.kind.has(CellKind::HAS_EXTRA) { sh.extras.remove(&ts_key(row, col)) } else { None };
        if sh.extras.is_empty() {
            // an emptied BTreeMap keeps its root leaf: drop it (M1)
            sh.extras = BTreeMap::new();
        }
        if old.kind.has(CellKind::HAS_FORMULA) {
            sh.formulas.remove(row, col);
        }
        self.cells -= 1;
        if let Some(extra) = extra {
            self.drop_extra(extra);
        }
        if let Some(id) = old.str_id() {
            self.strings.release(id);
        }
        true
    }

    pub(crate) fn drop_extra(&mut self, extra: CellExtra) {
        if let Some(InlineExtra::Shape { rich: Some(r), .. }) = extra.inline {
            self.free_rich(r);
        }
    }

    /// Stores rich runs; returns their index.
    pub fn add_rich(&mut self, runs: Vec<RichRun>) -> u32 {
        let runs = runs.into_boxed_slice();
        if let Some(i) = self.rich_free.pop()
            && let Some(slot) = self.rich.get_mut(i as usize)
        {
            *slot = Some(runs);
            return i;
        }
        self.rich.push(Some(runs));
        (self.rich.len() - 1) as u32
    }

    /// Frees rich runs.
    pub fn free_rich(&mut self, i: u32) {
        if let Some(slot) = self.rich.get_mut(i as usize)
            && slot.take().is_some()
        {
            self.rich_free.push(i);
        }
        while self.rich.last().is_some_and(Option::is_none) {
            self.rich.pop();
            let n = self.rich.len() as u32;
            self.rich_free.retain(|&x| x < n);
        }
    }

    /// The rich runs at `i`.
    #[must_use]
    pub fn rich_runs(&self, i: u32) -> Option<&[RichRun]> {
        self.rich.get(i as usize)?.as_deref()
    }

    /// Sets the `OWNED` bit (a spill/CSE member, kept by the engine).
    pub fn set_owned(&mut self, sheet: SheetIx, row: u32, col: u32, owned: bool) {
        self.set_flags(sheet, row, col, CellKind::OWNED, owned);
    }

    /// Sets the `s` attribute of an existing cell (`None` deletes it: the TS `setStyle(cell, 0)` writes `None`).
    pub fn set_style(&mut self, sheet: SheetIx, row: u32, col: u32, xf: Option<u32>) -> Result<()> {
        let Some(old) = self.get(sheet, row, col) else { return Ok(()) };
        let style = match xf {
            Some(x) if x < u32::MAX => x + 1,
            Some(_) => return Err(CalcError::Limit("style index")),
            None => 0,
        };
        let has_raw = self.extra(sheet, row, col).is_some_and(|e| e.raw_style.is_some());
        if has_raw {
            self.edit_extra(sheet, row, col, |e| e.raw_style = None);
        }
        if style != old.style {
            let cur = self.get(sheet, row, col).unwrap_or(old);
            if let Some(chunk) = self.chunk_mut(sheet, row, col)
                && let Ok(i) = chunk.index_of((row & (CHUNK_ROWS - 1)) as u8)
            {
                chunk.put(i, RawCell { style, ..cur });
            }
        }
        Ok(())
    }

    /// Edits the extra of an existing cell (created if missing; dropped when it becomes empty), keeping the
    /// `HAS_EXTRA` bit in sync.
    pub(crate) fn edit_extra(&mut self, sheet: SheetIx, row: u32, col: u32, f: impl FnOnce(&mut CellExtra)) {
        if self.get(sheet, row, col).is_none() {
            return;
        }
        let Some(sh) = self.sheets.get_mut(sheet.0 as usize) else { return };
        let k = ts_key(row, col);
        let e = sh.extras.entry(k).or_default();
        f(e);
        let empty = e.is_empty();
        if empty {
            sh.extras.remove(&k);
            if sh.extras.is_empty() {
                sh.extras = BTreeMap::new();
            }
        }
        self.set_flags(sheet, row, col, CellKind::HAS_EXTRA, !empty);
    }

    /// The xf index of a cell, present or not (`xfIndexAt`, `cell-format.ts:24-31`): its own `s` (0 without);
    /// for an absent cell the `<row s customFormat>` style, else the `<col style>`, else 0.
    #[must_use]
    pub fn style_at(&self, sheet: SheetIx, row: u32, col: u32) -> f64 {
        if let Some(c) = self.get(sheet, row, col) {
            if c.kind.has(CellKind::HAS_EXTRA)
                && let Some(s) = self.extra(sheet, row, col).and_then(|e| e.raw_style)
            {
                return s;
            }
            return if c.style == 0 { 0.0 } else { f64::from(c.style - 1) };
        }
        f64::from(self.default_style(sheet, row, col))
    }

    /// The default xf of an absent cell (`createCell(…, xfIndexAt(v, undefined, col, row))`).
    #[must_use]
    pub fn default_style(&self, sheet: SheetIx, row: u32, col: u32) -> u32 {
        let Some(sh) = self.sheet(sheet) else { return 0 };
        if let Some(m) = sh.row_meta(row)
            && m.flags & RowMeta::CUSTOM_FORMAT != 0
        {
            // `r.style ?? 0`; a NaN/negative style (hostile) reads as 0
            return m
                .style
                .filter(|s| uo_jsnum::conv::is_integer(*s) && *s >= 0.0 && *s < 4_294_967_295.0)
                .map_or(0, uo_jsnum::conv::to_uint32);
        }
        let i = sh.col_styles.partition_point(|&(_, b, _)| b < col);
        match sh.col_styles.get(i) {
            Some(&(a, _, s)) if a <= col => s,
            _ => 0,
        }
    }

    /// Column default styles from the `<col>` definitions in file order (`(min, max, style)`, 1-based as the
    /// file): later definitions win, `style` 0 is skipped (`defaultXfMaps`).
    pub fn set_col_styles(&mut self, sheet: SheetIx, defs: &[(u32, u32, u32)]) -> Result<()> {
        let sh = self.sheet_mut(sheet)?;
        let mut runs: Vec<(u32, u32, u32)> = Vec::new();
        for &(min, max, style) in defs {
            if style == 0 {
                continue;
            }
            let (a, b) = (min.max(1) - 1, max.min(MAX_COL + 1));
            if b == 0 || a >= b {
                continue;
            }
            let b = b - 1;
            // carve [a, b] out of the existing runs, then insert it
            let mut next = Vec::with_capacity(runs.len() + 2);
            for &(x, y, s) in &runs {
                if y < a || x > b {
                    next.push((x, y, s));
                    continue;
                }
                if x < a {
                    next.push((x, a - 1, s));
                }
                if y > b {
                    next.push((b + 1, y, s));
                }
            }
            next.push((a, b, style));
            next.sort_unstable_by_key(|r| r.0);
            runs = next;
        }
        sh.col_styles = runs;
        Ok(())
    }

    /// Sets the attributes of row `row` (0-based; the last definition wins). Takes the `spans` reference.
    pub fn set_row_meta(&mut self, sheet: SheetIx, row: u32, meta: RowMeta) -> Result<()> {
        let sh = self.sheets.get_mut(sheet.0 as usize).ok_or(CalcError::NoSheet(sheet.0))?;
        if meta.flags & RowMeta::HIDDEN != 0 {
            sh.hidden.insert(row);
        }
        let old = match sh.rows.binary_search_by_key(&row, |&(r, _)| r) {
            Ok(i) => Some(core::mem::replace(&mut sh.rows[i].1, meta)),
            Err(i) => {
                sh.rows.insert(i, (row, meta));
                None
            }
        };
        if let Some(id) = old.and_then(|m| m.spans) {
            self.strings.release(id);
        }
        Ok(())
    }

    /// Present cells of `area`, row by row (the order of `SheetStore.each`, `engine.ts:122-160`).
    pub fn each(&self, area: Area, mut f: impl FnMut(u32, u32, RawCell)) {
        let Some(sh) = self.sheet(area.sheet) else { return };
        if area.r1 > area.r2 || area.c1 > area.c2 {
            return;
        }
        if sh.columns.is_empty() {
            return;
        }
        let c2 = area.c2.min(sh.columns.len().saturating_sub(1) as u32);
        // one cursor per non-empty column: (col, chunk index, lane)
        let mut cur: Vec<(u32, usize, usize)> = Vec::new();
        for c in area.c1..=c2 {
            let Some(col) = sh.columns.get(c as usize) else { break };
            if col.is_empty() {
                continue;
            }
            let ci = col.first_from(area.r1);
            if let Some(ch) = col.chunks.get(ci) {
                cur.push((c, ci, ch.lower_bound(area.r1)));
            }
        }
        // the row each cursor points at (u32::MAX = done)
        let row_of = |sh: &SheetCells, (c, ci, i): (u32, usize, usize)| -> u32 {
            let col = &sh.columns[c as usize];
            match col.chunks.get(ci) {
                Some(ch) if i < ch.len() => ch.row_at(i),
                _ => u32::MAX,
            }
        };
        let advance = |sh: &SheetCells, x: &mut (u32, usize, usize)| {
            let col = &sh.columns[x.0 as usize];
            x.2 += 1;
            if col.chunks.get(x.1).is_some_and(|ch| x.2 >= ch.len()) {
                x.1 += 1;
                x.2 = 0;
            }
        };
        loop {
            let mut next = u32::MAX;
            for &x in &cur {
                next = next.min(row_of(sh, x));
            }
            if next == u32::MAX || next > area.r2 {
                return;
            }
            for x in &mut cur {
                if row_of(sh, *x) == next {
                    let col = &sh.columns[x.0 as usize];
                    if let Some(ch) = col.chunks.get(x.1) {
                        f(next, x.0, ch.get(x.2));
                    }
                    advance(sh, x);
                }
            }
        }
    }

    /// The runs of present cells of `col` between rows `r1..=r2`, chunk by chunk (column scans).
    pub fn column_runs(&self, sheet: SheetIx, col: u32, r1: u32, r2: u32) -> impl Iterator<Item = ColumnRun<'_>> + '_ {
        let chunks: &[Chunk] = self.sheet(sheet).and_then(|s| s.columns.get(col as usize)).map_or(&[], |c| c.chunks.as_slice());
        let first = if chunks.is_empty() { 0 } else { chunks.partition_point(|c| c.base + CHUNK_ROWS <= r1) };
        chunks.get(first..).unwrap_or(&[]).iter().take_while(move |c| c.base <= r2).filter_map(move |c| {
            let start = c.lower_bound(r1);
            let end = if r2 >= c.base + CHUNK_ROWS - 1 { c.len() } else { c.lower_bound(r2 + 1) };
            (start < end).then_some(ColumnRun { chunk: c, start, end })
        })
    }

    /// Gathers the cells of rows `r1..=r2` × columns `c1..=c2` into `out` (row-major), reusing its buffers.
    pub fn gather(&self, sheet: SheetIx, r1: u32, r2: u32, c1: u32, c2: u32, out: &mut Viewport) {
        let rows = r2.saturating_sub(r1) + 1;
        let cols = c2.saturating_sub(c1) + 1;
        let n = (rows * cols) as usize;
        out.r1 = r1;
        out.c1 = c1;
        out.rows = rows;
        out.cols = cols;
        out.kinds.clear();
        out.kinds.resize(n, CellKind::ABSENT);
        out.payload.clear();
        out.payload.resize(n, 0);
        out.styles.clear();
        out.styles.resize(n, 0);
        let Some(sh) = self.sheet(sheet) else { return };
        for c in c1..=c2.min(sh.columns.len().saturating_sub(1) as u32) {
            let dc = (c - c1) as usize;
            for run in self.column_runs(sheet, c, r1, r2) {
                for k in 0..run.len() {
                    let i = (run.row(k) - r1) as usize * cols as usize + dc;
                    let cell = run.get(k);
                    if let (Some(a), Some(b), Some(s)) = (out.kinds.get_mut(i), out.payload.get_mut(i), out.styles.get_mut(i)) {
                        *a = cell.kind;
                        *b = cell.payload;
                        *s = cell.style;
                    }
                }
            }
        }
    }

    /// Re-chooses the narrowest lanes everywhere and drops spare capacity (end of a load).
    pub fn shrink_to_fit(&mut self) {
        for sh in &mut self.sheets {
            for col in &mut sh.columns {
                for ch in &mut col.chunks {
                    ch.renarrow();
                }
                col.chunks.shrink_to_fit();
            }
            sh.columns.shrink_to_fit();
            sh.rows.shrink_to_fit();
            sh.formulas.shrink_to_fit();
        }
        self.rich.shrink_to_fit();
        self.rich_free.shrink_to_fit();
        self.sst.shrink_to_fit();
    }

    /// Heap bytes of the lanes, side tables excluded (the M3 bytes/cell numerator; the counting allocator
    /// measures everything).
    #[must_use]
    pub fn lane_bytes(&self) -> usize {
        self.sheets.iter().map(SheetCells::heap_bytes).sum::<usize>() + self.sheets.capacity() * core::mem::size_of::<SheetCells>()
    }

    /// The store holds no cache; kept for the memory-gate convention (M1).
    pub fn clear_caches(&mut self) {}

    /// Checks every invariant (tests, fuzzing): sorted non-empty chunks, lane lengths, cell counts, the
    /// `HAS_EXTRA`/`HAS_FORMULA` bits against the side tables, pool reference counts = holders.
    #[doc(hidden)]
    pub fn check_invariants(&self) -> core::result::Result<(), String> {
        let mut holders: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut total = 0u64;
        for (si, sh) in self.sheets.iter().enumerate() {
            let mut n = 0u64;
            if sh.columns.last().is_some_and(Column::is_empty) {
                return Err(format!("sheet {si}: trailing empty column"));
            }
            for (c, col) in sh.columns.iter().enumerate() {
                if col.chunks.windows(2).any(|w| w[0].base >= w[1].base) {
                    return Err(format!("sheet {si} col {c}: chunks not sorted"));
                }
                for ch in &col.chunks {
                    ch.check().map_err(|e| format!("sheet {si} col {c}: {e}"))?;
                    for i in 0..ch.len() {
                        let cell = ch.get(i);
                        let row = ch.row_at(i);
                        let k = ts_key(row, c as u32);
                        if cell.kind.has(CellKind::HAS_EXTRA) != sh.extras.contains_key(&k) {
                            return Err(format!("sheet {si} {row},{c}: HAS_EXTRA out of sync"));
                        }
                        if cell.kind.has(CellKind::HAS_FORMULA) != sh.formulas.get(row, c as u32).is_some() {
                            return Err(format!("sheet {si} {row},{c}: HAS_FORMULA out of sync"));
                        }
                        if let Some(id) = cell.str_id() {
                            *holders.entry(id.raw()).or_default() += 1;
                        }
                        n += 1;
                    }
                }
            }
            if n != sh.cells {
                return Err(format!("sheet {si}: {n} cells, counted {}", sh.cells));
            }
            if sh.extras.len() as u64 > n || sh.formulas.len() as u64 > n {
                return Err(format!("sheet {si}: side tables larger than the cells"));
            }
            for (_, m) in &sh.rows {
                if let Some(id) = m.spans {
                    *holders.entry(id.raw()).or_default() += 1;
                }
            }
            if let Some(fo) = &sh.file_order {
                for row in &fo.rows {
                    if let Some(id) = row.meta.and_then(|m| m.spans) {
                        *holders.entry(id.raw()).or_default() += 1;
                    }
                }
            }
            total += n;
        }
        if total != self.cells {
            return Err(format!("{total} cells, counted {}", self.cells));
        }
        for e in self.sst.iter().flatten() {
            *holders.entry(e.text.raw()).or_default() += 1;
        }
        for (&raw, &n) in &holders {
            let Some(id) = StrId::from_raw(raw) else { continue };
            let refs = self.strings.refs(id);
            if refs != u32::MAX && refs != n {
                return Err(format!("string {raw}: refs {refs} != holders {n}"));
            }
        }
        let live_holders = holders.keys().filter(|&&r| r != StrPool::EMPTY.raw()).count();
        if live_holders + 1 != self.strings.len() {
            return Err(format!("pool has {} live strings, {} held", self.strings.len(), live_holders + 1));
        }
        self.strings.check_invariants()
    }
}
