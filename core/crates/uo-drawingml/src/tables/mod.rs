//! The tables generated from the frozen TS by `tools/gen-tables.mjs` (G11), checked in and
//! included as they are (no formatting): `cargo test` re-parses the TS while it exists and
//! compares (`tests/tables.rs`).

pub(crate) mod presets {
    use crate::preset::def::{Op, PresetDef, c, g, p};
    include!("presets.rs");
}

pub(crate) mod light {
    include!("light.rs");
}

pub(crate) mod word_bevel {
    include!("word_bevel.rs");
}

pub(crate) mod patterns {
    include!("patterns.rs");
}

pub(crate) mod adjust {
    include!("adjust.rs");
}

pub(crate) mod srgb {
    include!("srgb.rs");
}

#[cfg(test)]
mod tests;
