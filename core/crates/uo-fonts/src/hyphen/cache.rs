//! The word cache bound to its hyphenator, owned by the caller per document and language. The TS
//! kept the cache inside the hyphenator, keyed by the word only; a cache handed to another
//! language's hyphenator would answer with the wrong points, so here the session owns both
//! (S12): its cache can only ever hold the points of its own hyphenator.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::liang::{Hyphenator, keep_inside};

/// The cache is cleared when it holds more than this many words, before the next insert (the TS
/// rule: `size > 50000`).
const CACHE_CAP: usize = 50_000;

#[derive(Debug)]
pub struct HyphenSession {
    hyphenator: Arc<Hyphenator>,
    cache: FxHashMap<String, Box<[u32]>>,
}

impl HyphenSession {
    pub fn new(hyphenator: Arc<Hyphenator>) -> HyphenSession {
        HyphenSession { hyphenator, cache: FxHashMap::default() }
    }

    pub fn hyphenator(&self) -> &Arc<Hyphenator> {
        &self.hyphenator
    }

    /// The points of a word, cached; the cache is cleared when it holds more than 50,000 entries.
    pub fn points(&mut self, word: &str) -> Vec<u32> {
        if let Some(hit) = self.cache.get(word) {
            return hit.to_vec();
        }
        let out = self.hyphenator.points(word);
        if self.cache.len() > CACHE_CAP {
            self.cache.clear();
        }
        self.cache.insert(word.to_owned(), out.clone().into_boxed_slice());
        out
    }

    /// The points with at least `left` letters before and `right` after the hyphen.
    pub fn hyphenate(&mut self, word: &str, left: u32, right: u32) -> Vec<u32> {
        let n = i64::try_from(word.chars().count()).unwrap_or(i64::MAX);
        keep_inside(self.points(word), n, left, right)
    }

    /// Words held in the cache.
    pub fn cached_words(&self) -> usize {
        self.cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::super::liang::HyphenationPatterns;
    use super::*;
    use uo_alloc::assert_no_leak;

    fn hyphenator() -> Arc<Hyphenator> {
        Arc::new(Hyphenator::new(&HyphenationPatterns { id: "xx", left: 2, right: 2, patterns: "a1b b1c", exceptions: "" }))
    }

    #[test]
    fn the_session_gives_the_points_of_its_own_hyphenator_cached_or_not() {
        let h = hyphenator();
        let mut s = HyphenSession::new(h.clone());
        assert_eq!(s.points("abc"), h.points("abc"));
        assert_eq!(s.cached_words(), 1);
        assert_eq!(s.points("abc"), [1, 2]);
        assert_eq!(s.cached_words(), 1);
        assert_eq!(s.hyphenate("abcabc", 2, 2), h.hyphenate("abcabc", 2, 2));
        assert_eq!(s.hyphenate("abc", 2, 1), [2]);
        assert!(Arc::ptr_eq(s.hyphenator(), &h));
    }

    #[test]
    fn two_sessions_of_two_languages_never_share_answers() {
        let other = Arc::new(Hyphenator::new(&HyphenationPatterns { id: "yy", left: 2, right: 2, patterns: "a2b b2c", exceptions: "" }));
        let (mut a, mut b) = (HyphenSession::new(hyphenator()), HyphenSession::new(other));
        assert_eq!(a.points("abc"), [1, 2]);
        assert_eq!(b.points("abc"), Vec::<u32>::new());
    }

    #[test]
    fn the_cache_is_cleared_past_fifty_thousand_words() {
        let mut s = HyphenSession::new(hyphenator());
        for i in 0..CACHE_CAP {
            s.points(&format!("w{i}"));
        }
        assert_eq!(s.cached_words(), CACHE_CAP);
        s.points("one-more");
        assert_eq!(s.cached_words(), CACHE_CAP + 1, "only a cache holding MORE than the cap is cleared");
        s.points("the-clearing-one");
        assert_eq!(s.cached_words(), 1, "cleared before the insert");
        assert_eq!(s.points("abc"), [1, 2]);
    }

    #[test]
    fn m1_no_leak_and_the_cache_is_dropped_with_its_session() {
        let h = hyphenator();
        assert_no_leak(|| {
            let mut s = HyphenSession::new(h.clone());
            (s.points("abc"), s.points("abc"), s.hyphenate("abcabc", 1, 1), s.cached_words())
        });
        assert_no_leak(|| {
            let mut s = HyphenSession::new(h.clone());
            for i in 0..(CACHE_CAP + 10) {
                s.points(&format!("{i}ab"));
            }
            s.cached_words()
        });
    }
}
