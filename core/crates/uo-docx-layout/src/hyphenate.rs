//! Automatic hyphenation (`hyphenate.ts`, frontier F4): Word's choice over
//! `uo_fonts::Hyphenator`. Which languages hyphenate is an **input** (`hyphenation_loaded`, K5,
//! LQ16), not what is compiled in. The `documentHyphenationLanguages` walk goes to
//! `uo_docx::word` (F2, explicit `lang`-site list).

use indexmap::IndexMap;

use crate::options::PatternId;

/// `HyphenUnit` (hyphenate.ts:137): the word that did not fit.
#[derive(Clone, Copy, Debug)]
pub struct HyphenUnit {
    pub i: u32,
    pub e: u32,
    pub x: f64,
    pub right: f64,
    pub line_start: bool,
    pub content_end: f64,
    pub justify: bool,
    pub spaces_w: f64,
    pub rest_w: f64,
    pub n_spaces: f64,
    pub hyphen_run: f64,
}

/// `HyphenChoice` (hyphenate.ts:158).
#[derive(Clone, Copy, Debug)]
pub struct HyphenChoice {
    /// last glyph of the line
    pub k: u32,
    /// hyphen width
    pub w: f64,
}

/// The hyphenation sessions of one engine: one per loaded pattern set, each with a word cache
/// capped at 100,000 words (M5). Filled by F4 over the uo-fonts hyphenation registry.
#[derive(Debug, Default)]
pub struct HyphenSessions {
    /// the pattern sets this call may use (`LayoutOptions::hyphenation_loaded`)
    pub loaded: Vec<PatternId>,
    /// words already split, per pattern set: offsets after which a hyphen may go
    pub cache: IndexMap<PatternId, WordCache>,
}

/// Split words of one pattern set: word → offsets after which a hyphen may go.
pub type WordCache = IndexMap<Box<[u16]>, Box<[u32]>>;

/// The word cache cap per language (M5).
pub const WORD_CACHE_CAP: usize = 100_000;
