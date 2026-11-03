//! The corpus of the G16 capture: the generated legacy-kern cases and their mutants, the quirk
//! fonts and the open fonts. Native only (the wasm self-test includes `legacy.rs` alone).
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use super::corpus::OPEN_FONTS;

pub type Maker = Arc<dyn Fn() -> Vec<u8> + Send + Sync>;

/// The corpus: the generated legacy-kern cases, the quirk fonts, the open fonts and 50 mutants of
/// the tail of each legacy case.
pub fn legacy_entries(root: &Path) -> Vec<(String, Maker)> {
    let mut out: Vec<(String, Maker)> = Vec::new();
    for (i, case) in crate::gpos_support::legacy_cases().into_iter().enumerate() {
        let bytes = case.bytes.clone();
        out.push((format!("legacy/{}", case.name), Arc::new(move || bytes.clone())));
        for (k, m) in crate::gpos_support::mutants(&case.bytes, i).into_iter().take(50).enumerate() {
            out.push((format!("legacy-mut/{}/{k}", case.name), Arc::new(move || m.clone())));
        }
    }
    for (name, bytes) in crate::b_support::quirk_fonts() {
        out.push((format!("quirk/{name}"), Arc::new(move || bytes.clone())));
    }
    for rel in OPEN_FONTS {
        let path = root.join(rel);
        out.push((format!("file/{rel}"), Arc::new(move || std::fs::read(&path).unwrap())));
    }
    out
}
