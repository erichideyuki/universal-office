//! Cell keys and areas (plan §4.0 "the `u64` cell key").
//!
//! The TS engine keys a cell by `row * 16384 + col` inside one sheet (`engine.ts:32`, the component's
//! `Map<row, Map<col, Cell>>` and `to-sheet-core.ts` use the same numbering); external-link sheets are sheet
//! indices from `EXTERNAL_BASE` (`engine.ts:29-30`). [`CellKey`] packs the sheet above that number, so the
//! natural `u64` order is (sheet, row, col) — the row-major order of `SheetStore.each` (`engine.ts:122-160`).

/// Engine sheet index: workbook sheets `0..`, external-link sheets `EXTERNAL_BASE + book * EXT_STRIDE + i`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct SheetIx(pub u32);

/// First sheet index of the external-link caches (`engine.ts:29`).
pub const EXTERNAL_BASE: u32 = 1 << 20;
/// Sheets per external book (`engine.ts:30`).
pub const EXT_STRIDE: u32 = 4096;
/// Last row, 0-based (`values.ts:104`).
pub const MAX_ROW: u32 = 1_048_575;
/// Last column, 0-based (`values.ts:105`).
pub const MAX_COL: u32 = 16_383;

const COL_BITS: u32 = 14;
const ROW_BITS: u32 = 20;

/// `sheet << 34 | row << 14 | col` (0-based row and column). Ordered as (sheet, row, col).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CellKey(u64);

impl CellKey {
    /// The key of a cell; `row <= MAX_ROW`, `col <= MAX_COL` are the caller's invariant (debug-checked).
    #[must_use]
    pub const fn new(sheet: SheetIx, row: u32, col: u32) -> Self {
        debug_assert!(row <= MAX_ROW && col <= MAX_COL);
        Self(((sheet.0 as u64) << (ROW_BITS + COL_BITS)) | ((row as u64) << COL_BITS) | col as u64)
    }
    /// The sheet.
    #[must_use]
    pub const fn sheet(self) -> SheetIx {
        SheetIx((self.0 >> (ROW_BITS + COL_BITS)) as u32)
    }
    /// The 0-based row.
    #[must_use]
    pub const fn row(self) -> u32 {
        ((self.0 >> COL_BITS) & ((1 << ROW_BITS) - 1)) as u32
    }
    /// The 0-based column.
    #[must_use]
    pub const fn col(self) -> u32 {
        (self.0 & ((1 << COL_BITS) - 1)) as u32
    }
    /// The TS in-sheet key `row * 16384 + col` (captures, the `Map` keys of the TS).
    #[must_use]
    pub const fn ts_key(self) -> u64 {
        self.0 & ((1 << (ROW_BITS + COL_BITS)) - 1)
    }
    /// The raw packed value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A rectangle of cells, 0-based and inclusive (`values.ts:58-62`). Whole-column/row references reach
/// `MAX_ROW`/`MAX_COL`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Area {
    /// The sheet.
    pub sheet: SheetIx,
    /// First row.
    pub r1: u32,
    /// First column.
    pub c1: u32,
    /// Last row.
    pub r2: u32,
    /// Last column.
    pub c2: u32,
}
