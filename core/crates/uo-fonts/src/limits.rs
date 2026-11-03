//! Limits for untrusted fonts (gate 1, G5/G11, revised after the skeptic): the V8 limits the TS
//! hits by accident (Map and Array sizes), the work budget the TS does not have, and the output
//! caps. Counted on what is actually read or produced, never trusted from sizes declared in the
//! file: no vector is allocated from a declared count.
//!
//! The defaults depend on the target ([`FontLimits::DEFAULT`], G5): on wasm32 a `Vec` cannot
//! exceed 2 GiB - 1 and the memory of a phone is far below 4 GiB, so the pair map and the cmap
//! index have lower caps there and the limit error always comes before an allocation failure.

use crate::error::{FontError, LimitKind};

/// Limits applied by the readers, the subsetter and the converters of this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontLimits {
    /// entries of the kerning pair map: the V8 `Map` limit (2^24, measured on Node 24.15) on
    /// native; the insertion of the next absent pair is the TS `RangeError`
    pub max_kern_pairs: u32,
    /// units of work of one public call, counted at the points of the module record (R-BUDGET)
    pub max_table_work: u64,
    /// component visits of one `glyph_outline` call (the TS hangs on a self-referencing composite)
    pub max_outline_work: u32,
    /// points of one outline
    pub max_outline_points: u32,
    /// outlines kept per face; the cache is cleared when it holds this many and a new glyph is added
    pub outline_cache_entries: u32,
    /// ranges (format 4 segments, format 12 groups) of the chosen cmap subtable: the JS array
    /// length limit (2^27, measured) native; the TS rejects the font there
    pub max_cmap_ranges: u32,
    /// bytes of an output built from declared sizes (`standalone_face`, `subset_true_type`)
    pub max_output_bytes: u64,
}

impl FontLimits {
    /// The native defaults: 2^24 kerning pairs (the V8 `Map` limit, exact parity; the largest
    /// real font, Gabriola, has 742,163), 2^26 units of work (the largest real GPOS needs 6.9
    /// million), 4,096 component visits and 2^16 points per outline (D3, gate 2: the largest real outline of 472 faces has about 1,200 commands), 2,048 cached outlines, 2^27
    /// cmap ranges (the JS array limit; the largest real cmap, Meiryo, has 14,335) and 256 MiB of
    /// output.
    pub const NATIVE: FontLimits = FontLimits {
        max_kern_pairs: 1 << 24,
        max_table_work: 1 << 26,
        max_outline_work: 4_096,
        max_outline_points: 1 << 16,
        outline_cache_entries: 2_048,
        max_cmap_ranges: 1 << 27,
        max_output_bytes: 256 << 20,
    };

    /// The wasm32 defaults (G5, D2 of gate 2): 2^20 kerning pairs (about 18 MB kept and 27 MB at
    /// the peak, 1.4 times the largest real font), 2^22 cmap ranges (16 MiB of index) and 64 MiB of output; the rest as
    /// [`FontLimits::NATIVE`].
    pub const WASM32: FontLimits =
        FontLimits { max_kern_pairs: 1 << 20, max_cmap_ranges: 1 << 22, max_output_bytes: 64 << 20, ..FontLimits::NATIVE };

    /// The defaults of the target: [`FontLimits::WASM32`] with 32-bit pointers,
    /// [`FontLimits::NATIVE`] elsewhere.
    #[cfg(target_pointer_width = "32")]
    pub const DEFAULT: FontLimits = FontLimits::WASM32;

    /// The defaults of the target: [`FontLimits::WASM32`] with 32-bit pointers,
    /// [`FontLimits::NATIVE`] elsewhere.
    #[cfg(not(target_pointer_width = "32"))]
    pub const DEFAULT: FontLimits = FontLimits::NATIVE;

    /// A fresh work budget of one public call.
    pub const fn work(&self) -> WorkBudget {
        WorkBudget::new(self.max_table_work)
    }
}

impl Default for FontLimits {
    fn default() -> Self {
        FontLimits::DEFAULT
    }
}

/// The work counter of one public call (R-BUDGET): one unit at each named point; when a unit
/// would pass the limit the call stops with its own outcome (the TS catch of that function).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkBudget {
    used: u64,
    max: u64,
}

impl WorkBudget {
    pub const fn new(max: u64) -> WorkBudget {
        WorkBudget { used: 0, max }
    }

    /// One unit of work: `Err(Limit(TableWork))` when it would pass the limit (the counter stays
    /// at the limit, so every later unit fails too).
    #[inline]
    pub fn tick(&mut self) -> Result<(), FontError> {
        self.tick_n(1)
    }

    /// `n` units at once (a bulk step such as a coverage range).
    #[inline]
    pub fn tick_n(&mut self, n: u64) -> Result<(), FontError> {
        match self.used.checked_add(n) {
            Some(used) if used <= self.max => {
                self.used = used;
                Ok(())
            }
            _ => Err(FontError::Limit(LimitKind::TableWork)),
        }
    }

    /// Units spent so far.
    pub fn used(&self) -> u64 {
        self.used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_wasm32_defaults() {
        assert_eq!(FontLimits::NATIVE.max_kern_pairs, 1 << 24);
        assert_eq!(FontLimits::NATIVE.max_table_work, 1 << 26);
        assert_eq!(FontLimits::NATIVE.max_outline_work, 4_096);
        assert_eq!(FontLimits::NATIVE.max_outline_points, 1 << 16);
        assert_eq!(FontLimits::NATIVE.outline_cache_entries, 2_048);
        assert_eq!(FontLimits::NATIVE.max_cmap_ranges, 1 << 27);
        assert_eq!(FontLimits::NATIVE.max_output_bytes, 256 << 20);
        assert_eq!(FontLimits::WASM32.max_kern_pairs, 1 << 20);
        assert_eq!(FontLimits::WASM32.max_cmap_ranges, 1 << 22);
        assert_eq!(FontLimits::WASM32.max_output_bytes, 64 << 20);
        assert_eq!(FontLimits::WASM32.max_table_work, FontLimits::NATIVE.max_table_work);
        assert_eq!(FontLimits::WASM32.max_outline_work, FontLimits::NATIVE.max_outline_work);
        assert_eq!(FontLimits::WASM32.max_outline_points, FontLimits::NATIVE.max_outline_points);
        assert_eq!(FontLimits::WASM32.outline_cache_entries, FontLimits::NATIVE.outline_cache_entries);
        assert_eq!(FontLimits::DEFAULT, if cfg!(target_pointer_width = "32") { FontLimits::WASM32 } else { FontLimits::NATIVE });
        assert_eq!(FontLimits::default(), FontLimits::DEFAULT);
    }

    #[test]
    fn work_budget_stops_at_the_limit_and_stays_stopped() {
        let mut w = WorkBudget::new(3);
        assert_eq!(w.tick(), Ok(()));
        assert_eq!(w.tick_n(2), Ok(()));
        assert_eq!(w.used(), 3);
        assert_eq!(w.tick(), Err(FontError::Limit(LimitKind::TableWork)));
        assert_eq!(w.tick(), Err(FontError::Limit(LimitKind::TableWork)));
        assert_eq!(w.used(), 3);
        let mut z = WorkBudget::new(0);
        assert_eq!(z.tick(), Err(FontError::Limit(LimitKind::TableWork)));
        assert_eq!(z.tick_n(0), Ok(()));
        let mut big = WorkBudget::new(u64::MAX);
        assert_eq!(big.tick_n(u64::MAX), Ok(()));
        assert_eq!(big.tick(), Err(FontError::Limit(LimitKind::TableWork)));
        assert_eq!(FontLimits::DEFAULT.work(), WorkBudget::new(1 << 26));
    }
}
