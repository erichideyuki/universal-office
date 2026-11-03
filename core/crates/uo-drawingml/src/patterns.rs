//! The preset patterns of DrawingML (`a:pattFill prst`): 8x8 bitmaps, port of
//! `shared/drawingml/src/patterns.ts`. The data is the generated table
//! (`tables/patterns.rs`, the TS key order); lookups find own names only (G7: the TS reads
//! `PRESET_PATTERNS[prst]` and would answer for `constructor` or throw).

use crate::tables::patterns::PRESET_PATTERNS;

/// Every preset pattern in the JS key order (the UI list): name and 8 rows (bit 7 = column 0).
pub fn preset_patterns() -> &'static [(&'static str, [u8; 8])] {
    PRESET_PATTERNS
}

/// The 8 rows of a preset pattern; `None` for an unknown name (own names only, G7).
pub fn pattern_rows(prst: &str) -> Option<[u8; 8]> {
    PRESET_PATTERNS.iter().find(|(name, _)| *name == prst).map(|&(_, rows)| rows)
}

/// The fraction of foreground pixels (the average colour); 0.5 for an unknown name.
pub fn pattern_density(prst: &str) -> f64 {
    let Some(rows) = pattern_rows(prst) else { return 0.5 };
    let n: u32 = rows.iter().map(|b| b.count_ones()).sum();
    f64::from(n) / 64.0
}
