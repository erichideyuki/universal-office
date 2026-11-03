// Phase-2 frontier A (colour, patterns, adjust, blip) ports this module from patterns.ts.
//! The preset patterns of DrawingML (`a:pattFill prst`): 8x8 bitmaps, port of
//! `shared/drawingml/src/patterns.ts`. The data is the generated table
//! (`tables/patterns.rs`); phase 1 declares the final signatures and the bodies return the
//! stage-3 placeholder.

/// Every preset pattern in the JS key order (the UI list): name and 8 rows (bit 7 = column 0).
pub fn preset_patterns() -> &'static [(&'static str, [u8; 8])] {
    &[]
}

/// The 8 rows of a preset pattern; `None` for an unknown name (own names only, G7).
pub fn pattern_rows(_prst: &str) -> Option<[u8; 8]> {
    None
}

/// The fraction of foreground pixels (the average colour); 0.5 for an unknown name.
pub fn pattern_density(_prst: &str) -> f64 {
    f64::NAN
}
