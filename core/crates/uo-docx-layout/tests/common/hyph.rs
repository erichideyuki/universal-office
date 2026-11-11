//! The hyphenation registry of every embedded pattern set and the base options that say they are
//! loaded (shared by `la7.rs`, `g4_cancel.rs` and `examples/bench_relayout.rs`; the copies in the
//! older parity tests are a declared follow-up).

use std::sync::Arc;

use uo_docx_layout::options::LayoutOptions;

/// Every embedded hyphenation pattern set, registered.
pub fn hyph_registry() -> Arc<uo_fonts::hyphen::HyphenationRegistry> {
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    Arc::new(reg)
}

/// The default options with every embedded pattern set loaded.
pub fn hyph_opts() -> LayoutOptions {
    LayoutOptions {
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    }
}
