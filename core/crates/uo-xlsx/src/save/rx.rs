//! JS regular expressions (ECMAScript syntax) for the text surgery of the save, on the vendored `regress`
//! that uo-calc uses. Patterns are built per call: the surgery runs once per rewritten part.

use super::SaveError;

/// A compiled pattern.
pub(crate) struct Rx(regress::Regex);

/// One match: byte ranges of the whole match and of the groups.
pub(crate) struct Hit(regress::Match);

impl Rx {
    pub(crate) fn new(pattern: &str) -> Result<Rx, SaveError> {
        regress::Regex::new(pattern).map(Rx).map_err(|_| SaveError::Frame("internal pattern"))
    }

    /// Case-insensitive (`/.../i`, ASCII).
    pub(crate) fn new_i(pattern: &str) -> Result<Rx, SaveError> {
        regress::Regex::with_flags(pattern, regress::Flags { icase: true, ..regress::Flags::default() })
            .map(Rx)
            .map_err(|_| SaveError::Frame("internal pattern"))
    }

    /// `pattern.exec(text.slice(from))` positioned (`find_from`: look-behind sees the text before `from`).
    pub(crate) fn find(&self, text: &str, from: usize) -> Option<Hit> {
        if from > text.len() || !text.is_char_boundary(from) {
            return None;
        }
        self.0.find_from(text, from).next().map(Hit)
    }

    /// Every non-overlapping match, as a global replace sees them.
    pub(crate) fn find_all(&self, text: &str) -> Vec<Hit> {
        self.0.find_iter(text).map(Hit).collect()
    }
}

impl Hit {
    pub(crate) fn start(&self) -> usize {
        self.0.start()
    }

    pub(crate) fn end(&self) -> usize {
        self.0.end()
    }

    /// Group `i` (1-based); `None` when it did not take part in the match.
    pub(crate) fn group(&self, i: usize) -> Option<std::ops::Range<usize>> {
        self.0.group(i)
    }
}
