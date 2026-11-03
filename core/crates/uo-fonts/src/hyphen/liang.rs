//! Liang's hyphenation: for each stretch of the word that matches a pattern the highest number
//! between two letters counts; an odd one allows a break. Patterns are NFC by contract (the TS
//! normalized them when it built the hyphenator; here the shipped data is checked NFC when it is
//! generated and the caller of [`Hyphenator::new`] with its own data owes the same). Words are
//! counted in code points, as `Array.from(word)` does.

use rustc_hash::FxHashMap;

use uo_jsnum::text::{lower_cp, split_js_space, to_lowercase};

/// The data of one language: the minimums before and after a hyphen, the patterns in TeX syntax
/// (`1b2l`) and the exceptions (words with `-` at the break points), each separated by JS
/// whitespace.
#[derive(Debug, Clone, Copy)]
pub struct HyphenationPatterns<'a> {
    pub id: &'a str,
    pub left: u32,
    pub right: u32,
    pub patterns: &'a str,
    pub exceptions: &'a str,
}

/// The pattern trie frozen into arrays: the children of node `n` are
/// `edges[first_edge[n]..first_edge[n + 1]]`, sorted by code point, and `values[n]` holds the
/// numbers between the letters of the pattern that ends there (letters + 1 of them).
#[derive(Debug, Default)]
struct Trie {
    first_edge: Vec<u32>,
    edges: Vec<(u32, u32)>,
    values: Vec<Option<Box<[u8]>>>,
}

impl Trie {
    /// The child of `node` for a code point.
    fn child(&self, node: usize, cp: u32) -> Option<usize> {
        let from = usize::try_from(*self.first_edge.get(node)?).ok()?;
        let to = usize::try_from(*self.first_edge.get(node + 1)?).ok()?;
        let edges = self.edges.get(from..to)?;
        let at = edges.binary_search_by_key(&cp, |&(c, _)| c).ok()?;
        usize::try_from(edges.get(at)?.1).ok()
    }
}

/// The trie while patterns are added: children by hash map, values per node.
#[derive(Default)]
struct TrieBuilder {
    children: Vec<FxHashMap<u32, u32>>,
    values: Vec<Option<Box<[u8]>>>,
}

impl TrieBuilder {
    fn new() -> TrieBuilder {
        TrieBuilder { children: vec![FxHashMap::default()], values: vec![None] }
    }

    /// The pattern `1b2l`: digits set the value after the last letter (the last one wins), every
    /// other character is a letter; a repeated pattern replaces the values of its node.
    fn add(&mut self, pattern: &str) {
        let mut letters: Vec<u32> = Vec::new();
        let mut vals: Vec<u8> = vec![0];
        for ch in pattern.chars() {
            if let Some(d) = ch.to_digit(10) {
                if let Some(last) = vals.last_mut() {
                    *last = u8::try_from(d).unwrap_or(0);
                }
            } else {
                letters.push(u32::from(ch));
                vals.push(0);
            }
        }
        let mut node = 0usize;
        for cp in letters {
            let existing = self.children.get(node).and_then(|m| m.get(&cp)).copied();
            node = match existing.and_then(|n| usize::try_from(n).ok()) {
                Some(n) => n,
                None => {
                    let fresh = self.children.len();
                    self.children.push(FxHashMap::default());
                    self.values.push(None);
                    if let (Some(map), Ok(id)) = (self.children.get_mut(node), u32::try_from(fresh)) {
                        map.insert(cp, id);
                    }
                    fresh
                }
            };
        }
        if let Some(slot) = self.values.get_mut(node) {
            *slot = Some(vals.into_boxed_slice());
        }
    }

    fn freeze(self) -> Trie {
        let mut first_edge: Vec<u32> = Vec::with_capacity(self.children.len() + 1);
        let mut edges: Vec<(u32, u32)> = Vec::new();
        for map in &self.children {
            first_edge.push(u32::try_from(edges.len()).unwrap_or(u32::MAX));
            let start = edges.len();
            edges.extend(map.iter().map(|(&cp, &node)| (cp, node)));
            if let Some(slice) = edges.get_mut(start..) {
                slice.sort_unstable_by_key(|&(cp, _)| cp);
            }
        }
        first_edge.push(u32::try_from(edges.len()).unwrap_or(u32::MAX));
        Trie { first_edge, edges, values: self.values }
    }
}

/// An immutable hyphenator, shared by `Arc`; the cache lives in a
/// [`HyphenSession`](crate::HyphenSession).
#[derive(Debug)]
pub struct Hyphenator {
    id: String,
    left: u32,
    right: u32,
    trie: Trie,
    exceptions: FxHashMap<String, Box<[u32]>>,
}

const DOT: u32 = 0x2E;

impl Hyphenator {
    /// Builds the trie of the patterns and the exception table. Patterns and exceptions are split
    /// on JS whitespace (the TS `/\s+/`); an exception's break positions count code points, its key
    /// is the word without the dashes in lower case, and a repeated key keeps the last entry.
    pub fn new(p: &HyphenationPatterns<'_>) -> Hyphenator {
        let mut builder = TrieBuilder::new();
        for pattern in split_js_space(p.patterns).filter(|t| !t.is_empty()) {
            builder.add(pattern);
        }
        let mut exceptions: FxHashMap<String, Box<[u32]>> = FxHashMap::default();
        for word in split_js_space(p.exceptions).filter(|t| !t.is_empty()) {
            let mut points: Vec<u32> = Vec::new();
            let mut n: u32 = 0;
            let mut bare = String::with_capacity(word.len());
            for ch in word.chars() {
                if ch == '-' {
                    points.push(n);
                } else {
                    n = n.saturating_add(1);
                    bare.push(ch);
                }
            }
            exceptions.insert(to_lowercase(&bare), points.into_boxed_slice());
        }
        Hyphenator { id: p.id.to_owned(), left: p.left, right: p.right, trie: builder.freeze(), exceptions }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn left(&self) -> u32 {
        self.left
    }

    pub fn right(&self) -> u32 {
        self.right
    }

    /// The break points by the patterns alone, without minimums: `k` in `1..n` means a hyphen after
    /// `k` letters (the word in NFC, counted in code points). Not cached: use a
    /// [`HyphenSession`](crate::HyphenSession) for that.
    pub fn points(&self, word: &str) -> Vec<u32> {
        let chars: Vec<char> = word.chars().collect();
        let n = chars.len();
        let low: Vec<char> = chars.iter().map(|&c| lower_cp(c)).collect();
        if !self.exceptions.is_empty() {
            let key: String = low.iter().collect();
            if let Some(points) = self.exceptions.get(&key) {
                let n = u32::try_from(n).unwrap_or(u32::MAX);
                return points.iter().copied().filter(|&k| k > 0 && k < n).collect();
            }
        }
        let mut w: Vec<u32> = Vec::with_capacity(n + 2);
        w.push(DOT);
        w.extend(low.iter().map(|&c| u32::from(c)));
        w.push(DOT);
        let mut vals: Vec<u8> = vec![0; w.len() + 1];
        for i in 0..w.len() {
            let mut node = 0usize;
            for &cp in w.get(i..).unwrap_or(&[]) {
                let Some(next) = self.trie.child(node, cp) else { break };
                node = next;
                if let Some(Some(values)) = self.trie.values.get(node) {
                    for (q, &v) in values.iter().enumerate() {
                        if let Some(slot) = vals.get_mut(i + q)
                            && v > *slot
                        {
                            *slot = v;
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        for k in 1..n {
            if vals.get(k + 1).is_some_and(|v| v & 1 == 1) {
                out.push(u32::try_from(k).unwrap_or(u32::MAX));
            }
        }
        out
    }

    /// The points with at least `left` letters before and `right` after the hyphen.
    pub fn hyphenate(&self, word: &str, left: u32, right: u32) -> Vec<u32> {
        let n = i64::try_from(word.chars().count()).unwrap_or(i64::MAX);
        keep_inside(self.points(word), n, left, right)
    }
}

/// `points.filter(k => k >= left && n - k >= right)`.
pub(super) fn keep_inside(points: Vec<u32>, n: i64, left: u32, right: u32) -> Vec<u32> {
    points.into_iter().filter(|&k| k >= left && n - i64::from(k) >= i64::from(right)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_alloc::assert_no_leak;

    fn patterns<'a>(patterns: &'a str, exceptions: &'a str) -> HyphenationPatterns<'a> {
        HyphenationPatterns { id: "xx", left: 2, right: 2, patterns, exceptions }
    }

    #[test]
    fn a_pattern_sets_the_numbers_between_its_letters() {
        let h = Hyphenator::new(&patterns("a1b", ""));
        assert_eq!(h.points("ab"), [1]);
        assert_eq!(h.points("xaby"), [2]);
        assert_eq!(h.points("ba"), Vec::<u32>::new());
        assert_eq!(h.points("a"), Vec::<u32>::new());
        assert_eq!(h.points(""), Vec::<u32>::new());
    }

    #[test]
    fn the_highest_number_wins_and_even_numbers_forbid() {
        let h = Hyphenator::new(&patterns("a1b a2b ab3c c2d", ""));
        assert_eq!(h.points("abc"), [2], "2 at a-b beats 1 and forbids it, 3 at b-c allows the break");
        let h = Hyphenator::new(&patterns("a1b ab3c", ""));
        assert_eq!(h.points("abc"), [1, 2]);
        let h = Hyphenator::new(&patterns("a1b a2b", ""));
        assert_eq!(h.points("ab"), Vec::<u32>::new(), "the later pattern replaces the numbers of the same letters");
    }

    #[test]
    fn dots_anchor_the_word_and_the_value_after_the_last_letter_counts() {
        let h = Hyphenator::new(&patterns(".a1b 1c. 3d", ""));
        assert_eq!(h.points("ab"), [1]);
        assert_eq!(h.points("xab"), Vec::<u32>::new());
        assert_eq!(h.points("xc"), [1]);
        assert_eq!(h.points("xcc"), [2], "`1c.` anchors at the end of the word");
        assert_eq!(h.points("xdx"), [1]);
        let h = Hyphenator::new(&patterns("a1b2", ""));
        assert_eq!(h.points("abx"), [1], "the 2 after the last letter forbids the break between b and x");
        let h = Hyphenator::new(&patterns("a1b3", ""));
        assert_eq!(h.points("abx"), [1, 2]);
    }

    #[test]
    fn the_last_digit_of_a_run_wins() {
        let h = Hyphenator::new(&patterns("a12b", ""));
        assert_eq!(h.points("ab"), Vec::<u32>::new());
        let h = Hyphenator::new(&patterns("a21b", ""));
        assert_eq!(h.points("ab"), [1]);
    }

    #[test]
    fn a_pattern_without_letters_is_never_applied() {
        let h = Hyphenator::new(&patterns("1 3", ""));
        assert_eq!(h.points("abcd"), Vec::<u32>::new());
    }

    #[test]
    fn exceptions_use_the_lower_cased_word_and_positions_in_code_points() {
        let h = Hyphenator::new(&patterns("a1b", "ta-ble Me-ta-l\u{e9}"));
        assert_eq!(h.points("table"), [2]);
        assert_eq!(h.points("TABLE"), [2]);
        assert_eq!(h.points("Table"), [2]);
        assert_eq!(h.points("Metal\u{e9}"), [2, 4]);
        assert_eq!(h.points("tab"), [2], "without an exception the patterns apply");
    }

    #[test]
    fn exception_points_outside_the_word_are_dropped_and_the_last_entry_wins() {
        let h = Hyphenator::new(&patterns("", "-ab-cd- ab-cd abc-d"));
        assert_eq!(h.points("abcd"), [3], "a repeated key keeps the last entry");
        let h = Hyphenator::new(&patterns("", "-ab-cd-"));
        assert_eq!(h.points("abcd"), [2]);
        let h = Hyphenator::new(&patterns("", "a-"));
        assert_eq!(h.points("a"), Vec::<u32>::new());
        let h = Hyphenator::new(&patterns("a1b", "ab"));
        assert_eq!(h.points("ab"), Vec::<u32>::new(), "an exception without dashes forbids every break");
    }

    #[test]
    fn the_exception_key_is_the_full_lowercase_and_the_lookup_is_per_code_point() {
        let h = Hyphenator::new(&patterns("", "\u{130}-a"));
        assert_eq!(h.points("\u{130}a"), Vec::<u32>::new(), "U+0130 keeps itself in the lookup, its key lowers to two code points");
        let h = Hyphenator::new(&patterns("", "A-B"));
        assert_eq!(h.points("ab"), [1]);
    }

    #[test]
    fn minimums_filter_the_points() {
        let h = Hyphenator::new(&patterns("a1b b1c c1d d1e", ""));
        assert_eq!(h.points("abcde"), [1, 2, 3, 4]);
        assert_eq!(h.hyphenate("abcde", 2, 2), [2, 3]);
        assert_eq!(h.hyphenate("abcde", 1, 1), [1, 2, 3, 4]);
        assert_eq!(h.hyphenate("abcde", 3, 3), Vec::<u32>::new());
        assert_eq!(h.hyphenate("abcde", 0, 0), [1, 2, 3, 4]);
        assert_eq!((h.id(), h.left(), h.right()), ("xx", 2, 2));
    }

    #[test]
    fn words_are_counted_in_code_points() {
        let h = Hyphenator::new(&patterns("\u{1F600}1b b1\u{e9}", ""));
        assert_eq!(h.points("\u{1F600}b\u{e9}"), [1, 2]);
        assert_eq!(h.hyphenate("\u{1F600}b\u{e9}", 1, 1), [1, 2]);
        assert_eq!(h.hyphenate("\u{1F600}b\u{e9}", 2, 1), [2]);
    }

    #[test]
    fn whitespace_splits_as_js_does() {
        let h = Hyphenator::new(&patterns("a1b\u{a0}c1d\u{feff}e1f\n\t g1h", "x-y\u{3000}z-w"));
        assert_eq!(h.points("ab"), [1]);
        assert_eq!(h.points("cd"), [1]);
        assert_eq!(h.points("ef"), [1]);
        assert_eq!(h.points("gh"), [1]);
        assert_eq!(h.points("xy"), [1]);
        assert_eq!(h.points("zw"), [1]);
    }

    #[test]
    fn m1_no_leak() {
        let data = patterns("a1b b1c .a2 3z. bc1d", "ta-ble ca-sa");
        assert_no_leak(|| Hyphenator::new(&data).id().len());
        let h = Hyphenator::new(&data);
        assert_no_leak(|| (h.points("abcd"), h.points("table"), h.points(""), h.hyphenate("abcdabcd", 2, 3), h.points("\u{1F600}x")));
        assert_no_leak(|| Hyphenator::new(&patterns("", "")).points("x"));
    }
}
