//! Automatic hyphenation: Liang's patterns per language, the registry of the loaded ones and the
//! per-document cache. The pattern files are data (`data/hyphen/<id>.txt`, NFC, generated once
//! from the frozen TS modules by `tools/gen_hyphen_data.mjs`); with the feature `hyphen-data`
//! the native and .NET builds embed them, the wasm build never does (the shell fetches the file
//! of each language and passes it to [`HyphenationRegistry::register`]).
//!
//! The text format of a pattern file: `#` lines (the licence notice of the source), then
//! `id <id>`, `left <n>`, `right <n>`, a `[patterns]` line, the patterns separated by whitespace
//! over any number of lines, an `[exceptions]` line and the exceptions likewise. [`parse_patterns`]
//! reads it, for the shell's files as well as the embedded ones.

use uo_jsnum::text::trim;

mod cache;
mod liang;
mod registry;

pub use cache::HyphenSession;
pub use liang::{HyphenationPatterns, Hyphenator};
pub use registry::{
    HYPHENATION_PATTERN_IDS, HyphenationRegistry, TS_HYPHENATION_PATTERN_IDS, hyphenation_pattern_id, hyphenation_pattern_id_with,
    pattern_ids_to_load, pattern_ids_to_load_with,
};

/// The patterns of a pattern file (see the module documentation), borrowing the text. `None` when
/// the header or the two section lines are missing or a number does not parse.
pub fn parse_patterns(text: &str) -> Option<HyphenationPatterns<'_>> {
    let mut patterns_line: Option<(usize, usize)> = None;
    let mut exceptions_line: Option<(usize, usize)> = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let end = at + line.len();
        match line.trim_end_matches(['\n', '\r']) {
            "[patterns]" if patterns_line.is_none() => patterns_line = Some((at, end)),
            "[exceptions]" if patterns_line.is_some() && exceptions_line.is_none() => exceptions_line = Some((at, end)),
            _ => {}
        }
        at = end;
    }
    let ((patterns_from, patterns_start), (exceptions_from, exceptions_start)) = (patterns_line?, exceptions_line?);
    let header = text.get(..patterns_from)?;
    let patterns = text.get(patterns_start..exceptions_from)?;
    let exceptions = text.get(exceptions_start..)?;
    let (mut id, mut left, mut right) = (None, None, None);
    for line in header.lines().filter(|l| !l.starts_with('#')) {
        match line.split_once(' ') {
            Some(("id", v)) => id = Some(trim(v)),
            Some(("left", v)) => left = trim(v).parse::<u32>().ok(),
            Some(("right", v)) => right = trim(v).parse::<u32>().ok(),
            _ => {}
        }
    }
    Some(HyphenationPatterns { id: id?, left: left?, right: right?, patterns, exceptions })
}

#[cfg(feature = "hyphen-data")]
const EMBEDDED: [(&str, &str); 8] = [
    ("pt", include_str!("../../data/hyphen/pt.txt")),
    ("en-us", include_str!("../../data/hyphen/en-us.txt")),
    ("es", include_str!("../../data/hyphen/es.txt")),
    ("fr", include_str!("../../data/hyphen/fr.txt")),
    ("de-1996", include_str!("../../data/hyphen/de-1996.txt")),
    ("it", include_str!("../../data/hyphen/it.txt")),
    ("nl", include_str!("../../data/hyphen/nl.txt")),
    // WordCraft study (fields, D1): English under Word's rules (CC0 patterns + Moby exceptions; the notices in
    // data/hyphen/THIRD-PARTY-NOTICES.md)
    ("en-us-word", include_str!("../../data/hyphen/en-us-word.txt")),
];

/// The shipped patterns of an id (`pt`, `en-us`, `es`, `fr`, `de-1996`, `it`, `nl`), own keys only.
#[cfg(feature = "hyphen-data")]
pub fn embedded_patterns(id: &str) -> Option<HyphenationPatterns<'static>> {
    let (_, text) = EMBEDDED.iter().find(|(key, _)| *key == id)?;
    parse_patterns(text).filter(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# notice\n# more\n\nid xx\nleft 2\nright 3\n[patterns]\na1b b1c\nc1d\n[exceptions]\nta-ble\n";

    #[test]
    fn a_pattern_file_is_read_into_its_fields() {
        let p = parse_patterns(SAMPLE).unwrap();
        assert_eq!((p.id, p.left, p.right), ("xx", 2, 3));
        assert_eq!(p.patterns, "a1b b1c\nc1d\n");
        assert_eq!(p.exceptions, "ta-ble\n");
        let h = Hyphenator::new(&p);
        assert_eq!(h.points("abcd"), [1, 2, 3]);
        assert_eq!(h.points("table"), [2]);
    }

    #[test]
    fn a_malformed_file_is_none() {
        for bad in [
            "",
            "id xx\nleft 2\nright 3\n",
            "id xx\nleft 2\nright 3\n[patterns]\na\n",
            "id xx\nleft 2\nright 3\n[exceptions]\nb\n[patterns]\na\n",
            "left 2\nright 3\n[patterns]\na\n[exceptions]\n",
            "id xx\nleft x\nright 3\n[patterns]\na\n[exceptions]\n",
            "id xx\nleft 2\n[patterns]\na\n[exceptions]\n",
        ] {
            assert!(parse_patterns(bad).is_none(), "{bad:?}");
        }
        let empty = parse_patterns("id xx\nleft 0\nright 0\n[patterns]\n[exceptions]\n").unwrap();
        assert_eq!((empty.patterns, empty.exceptions), ("", ""));
    }
}
