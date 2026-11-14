//! The patterns of a language tag and the hyphenators loaded in an engine. The TS kept the loaded
//! hyphenators in a module global and loaded each language with a dynamic `import()`; here the
//! registry belongs to one engine instance and loading is the caller's (the shell fetches the
//! pattern file, or the native build embeds it: `embedded_patterns`).

use std::sync::Arc;

use super::liang::{HyphenationPatterns, Hyphenator};
use uo_jsnum::text::to_lowercase;

/// The ids with patterns, in the TS order, then `en-us-word` (WordCraft study, fields D1: English under Word's rules —
/// DesignCraft's CC0 patterns + the Moby Hyphenator II exceptions —, which no language tag maps to by itself: the
/// layout takes it for English when its strict TS switch is off and the data is loaded; see
/// [`hyphenation_pattern_id_with`]).
pub const HYPHENATION_PATTERN_IDS: [&str; 8] = ["pt", "en-us", "es", "fr", "de-1996", "it", "nl", "en-us-word"];

/// The frozen TS's ids (`HYPHENATION_PATTERN_IDS` of hyphen/index.ts): what [`pattern_ids_to_load`] knows as ids and
/// what the parity replays compare.
pub const TS_HYPHENATION_PATTERN_IDS: [&str; 7] = ["pt", "en-us", "es", "fr", "de-1996", "it", "nl"];

/// [`hyphenation_pattern_id`] with Word's data (WordCraft study, fields D1): every English gives `en-us-word` when
/// `word_data`.
pub fn hyphenation_pattern_id_with(lang: Option<&str>, word_data: bool) -> Option<&'static str> {
    match hyphenation_pattern_id(lang)? {
        "en-us" if word_data => Some("en-us-word"),
        id => Some(id),
    }
}

/// [`pattern_ids_to_load`] with Word's data (WordCraft study, fields D1): English loads `en-us-word` when `word_data`
/// (the layout under Word's rules), `en-us` otherwise (under the frozen TS's switch `ts_hyphenation_patterns`, bit 16
/// of the bindings' layout flags): the bindings pass the bit, so the product never fetches the English data it does not
/// use.
pub fn pattern_ids_to_load_with<'a>(
    langs: impl IntoIterator<Item = Option<&'a str>>,
    loaded: &HyphenationRegistry,
    word_data: bool,
) -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    for lang in langs {
        let id = match lang.filter(|l| !l.is_empty()) {
            Some(l) => HYPHENATION_PATTERN_IDS
                .iter()
                .copied()
                .find(|id| *id == l)
                .map(|id| if word_data && id == "en-us" { "en-us-word" } else { id })
                .or_else(|| hyphenation_pattern_id_with(Some(l), word_data)),
            None => None,
        };
        if let Some(id) = id
            && !loaded.contains(id)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    ids
}

/// The patterns for a language tag (BCP 47, as in `w:lang`): `pt-BR` gives `pt`, every English
/// gives `en-us` (Word hyphenates en-US, en-GB and en-AU with the same points, and the American
/// patterns come closest), a language without patterns `None`. The tag is lower-cased (Unicode)
/// and cut at the first `-` or `_`.
pub fn hyphenation_pattern_id(lang: Option<&str>) -> Option<&'static str> {
    let lang = lang.filter(|l| !l.is_empty())?;
    let lowered = to_lowercase(lang);
    let base = lowered.split(['-', '_']).next().unwrap_or("");
    match base {
        "pt" => Some("pt"),
        "es" => Some("es"),
        "fr" => Some("fr"),
        "de" => Some("de-1996"),
        "it" => Some("it"),
        "nl" => Some("nl"),
        "en" => Some("en-us"),
        _ => None,
    }
}

/// The ids to load for the languages of a document: a language that is itself an id is taken as
/// it is (own keys only: `constructor` is not an id), any other goes through
/// [`hyphenation_pattern_id`]; those already in the registry are left out and an id appears once,
/// in the order of first use.
pub fn pattern_ids_to_load<'a>(langs: impl IntoIterator<Item = Option<&'a str>>, loaded: &HyphenationRegistry) -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    for lang in langs {
        let id = match lang.filter(|l| !l.is_empty()) {
            Some(l) => TS_HYPHENATION_PATTERN_IDS.iter().copied().find(|id| *id == l).or_else(|| hyphenation_pattern_id(Some(l))),
            None => None,
        };
        if let Some(id) = id
            && !loaded.contains(id)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    ids
}

/// The hyphenators loaded in one engine instance. A clone shares the hyphenators (the bindings'
/// copy-on-write registration while a layout or a session still holds the old set).
#[derive(Debug, Default, Clone)]
pub struct HyphenationRegistry {
    loaded: Vec<Arc<Hyphenator>>,
}

impl HyphenationRegistry {
    pub fn new() -> HyphenationRegistry {
        HyphenationRegistry::default()
    }

    /// Builds and registers the patterns; an id already registered is replaced in place (the TS
    /// `Map.set`). The returned hyphenator is shared with the registry.
    pub fn register(&mut self, p: &HyphenationPatterns<'_>) -> Arc<Hyphenator> {
        let hyphenator = Arc::new(Hyphenator::new(p));
        match self.loaded.iter_mut().find(|h| h.id() == p.id) {
            Some(slot) => *slot = hyphenator.clone(),
            None => self.loaded.push(hyphenator.clone()),
        }
        hyphenator
    }

    /// The loaded hyphenator of a language tag.
    pub fn for_lang(&self, lang: Option<&str>) -> Option<Arc<Hyphenator>> {
        let id = hyphenation_pattern_id(lang)?;
        self.get(id)
    }

    /// The loaded hyphenator with this exact pattern id.
    pub fn get(&self, id: &str) -> Option<Arc<Hyphenator>> {
        self.loaded.iter().find(|h| h.id() == id).cloned()
    }

    /// Whether patterns with this exact id are loaded.
    pub fn contains(&self, id: &str) -> bool {
        self.loaded.iter().any(|h| h.id() == id)
    }

    /// The ids loaded, in registration order.
    pub fn ids(&self) -> Vec<String> {
        self.loaded.iter().map(|h| h.id().to_owned()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_alloc::assert_no_leak;

    fn data<'a>(id: &'a str, patterns: &'static str) -> HyphenationPatterns<'a> {
        HyphenationPatterns { id, left: 2, right: 2, patterns, exceptions: "" }
    }

    #[test]
    fn language_tags_map_to_pattern_ids() {
        for (tag, id) in [
            ("pt", "pt"),
            ("pt-BR", "pt"),
            ("PT_pt", "pt"),
            ("es-MX", "es"),
            ("fr", "fr"),
            ("fr-CA", "fr"),
            ("de", "de-1996"),
            ("de-AT", "de-1996"),
            ("it", "it"),
            ("nl-BE", "nl"),
            ("en", "en-us"),
            ("en-GB", "en-us"),
            ("EN-au", "en-us"),
            ("en_US", "en-us"),
        ] {
            assert_eq!(hyphenation_pattern_id(Some(tag)), Some(id), "{tag}");
        }
        for tag in ["", "ja", "zh-CN", "-pt", "_pt", "ptx", "p", "constructor", "toString", "__proto__", "en-us-extra-x", "\u{130}t"] {
            let expected = if tag == "en-us-extra-x" { Some("en-us") } else { None };
            assert_eq!(hyphenation_pattern_id(Some(tag)), expected, "{tag:?}");
        }
        assert_eq!(hyphenation_pattern_id(None), None);
    }

    #[test]
    fn ids_to_load_skip_the_loaded_ones_and_repeats() {
        let mut registry = HyphenationRegistry::new();
        let langs = [
            Some("pt-BR"),
            None,
            Some("en-US"),
            Some("pt"),
            Some("en-us"),
            Some("ja"),
            Some("de-1996"),
            Some("constructor"),
            Some("DE"),
            Some(""),
        ];
        assert_eq!(pattern_ids_to_load(langs, &registry), ["pt", "en-us", "de-1996"]);
        registry.register(&data("pt", "a1b"));
        assert_eq!(pattern_ids_to_load(langs, &registry), ["en-us", "de-1996"]);
        assert_eq!(pattern_ids_to_load([Some("pt-PT")], &registry), Vec::<&str>::new());
        assert_eq!(pattern_ids_to_load([], &registry), Vec::<&str>::new());
    }

    /// WordCraft study (fields, D1): Word's English data only under Word's rules, the TS's `en-us` only under its switch.
    #[test]
    fn word_data_loads_en_us_word_or_en_us() {
        let mut registry = HyphenationRegistry::new();
        assert_eq!(pattern_ids_to_load_with([Some("en-GB"), Some("pt-BR")], &registry, true), ["en-us-word", "pt"]);
        assert_eq!(pattern_ids_to_load_with([Some("en-us")], &registry, true), ["en-us-word"]);
        assert_eq!(pattern_ids_to_load_with([Some("en-US"), Some("en-GB")], &registry, false), ["en-us"]);
        assert_eq!(pattern_ids_to_load_with([Some("en-us-word")], &registry, false), ["en-us-word"], "an id is taken as it is");
        registry.register(&data("en-us-word", "a1b"));
        assert_eq!(pattern_ids_to_load_with([Some("en-US")], &registry, true), Vec::<&str>::new());
        assert_eq!(pattern_ids_to_load_with([Some("en-US")], &registry, false), ["en-us"]);
        assert_eq!(hyphenation_pattern_id_with(Some("en-AU"), true), Some("en-us-word"));
        assert_eq!(hyphenation_pattern_id_with(Some("es"), true), Some("es"));
    }

    #[test]
    fn register_replaces_an_id_in_place() {
        let mut registry = HyphenationRegistry::new();
        let first = registry.register(&data("pt", "a1b"));
        registry.register(&data("en-us", "c1d"));
        let second = registry.register(&data("pt", "a2b"));
        assert_eq!(registry.ids(), ["pt", "en-us"]);
        assert_eq!(first.points("ab"), [1], "a hyphenator already handed out keeps its own patterns");
        assert_eq!(second.points("ab"), Vec::<u32>::new());
        let found = registry.for_lang(Some("pt-BR")).unwrap();
        assert!(Arc::ptr_eq(&found, &second));
        assert!(registry.for_lang(Some("ja")).is_none());
        assert!(registry.for_lang(None).is_none());
        assert!(registry.for_lang(Some("es")).is_none());
        assert!(registry.contains("en-us") && !registry.contains("es"));
        assert!(registry.get("pt").is_some() && registry.get("PT").is_none());
    }

    #[test]
    fn m1_no_leak() {
        assert_no_leak(|| {
            let mut registry = HyphenationRegistry::new();
            let a = registry.register(&data("pt", "a1b"));
            registry.register(&data("pt", "a2b"));
            (
                a.points("ab"),
                registry.for_lang(Some("pt")).map(|h| h.points("ab")),
                registry.ids(),
                pattern_ids_to_load([Some("fr")], &registry),
            )
        });
        assert_no_leak(|| (hyphenation_pattern_id(Some("pt-BR")), hyphenation_pattern_id(Some("\u{130}"))));
    }
}
