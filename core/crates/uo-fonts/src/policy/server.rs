//! The `server-substitute` policy: the C# `SubstituteFontProvider` over `SystemFontProvider`
//! (`dotnet/src/UniversalOffice.Docx/DocxLayout/`), which clickCompliance passes to
//! `DocxLayoutPipeline.ToPdf`. The C# is the reference (it exists nowhere in the TS).
//!
//! - [`SystemFontProvider`]: the family asked for, from the machine's fonts. It remembers its
//!   answers (and its `null`s) per exact `(family, bold, italic)`, case-sensitive: asking for
//!   `Arial` and `arial` gives two answers over the same file, as the C# gives two `FontFace`
//!   objects. The C# cache lives for the process; here an answer lives while someone holds it
//!   (see `answers.rs`), which nobody can tell apart.
//! - [`SubstituteFontProvider`]: the inner provider's answer when it has one; else the family's
//!   metric-compatible substitute from the catalog (Calibri to Carlito, Cambria to Caladea, Arial
//!   and Helvetica to Liberation Sans...) asked of the inner provider, returned as a **fresh copy
//!   marked `stand_in`** (a new identity at every call: the C# builds a new `FontFace`); no
//!   substitute, or the inner provider has none: `None`, and the layout follows Word's chain.

use std::sync::Arc;

use crate::catalog::metric_substitute;
use crate::registry::answers::{AnswerCache, Resolved};
use crate::registry::{FaceLedger, FaceSource, FontAnswer, FontProvider};

type Key = (String, bool, bool);

/// The C# `SystemFontProvider`.
pub struct SystemFontProvider {
    source: Arc<dyn FaceSource>,
    ledger: Option<Arc<FaceLedger>>,
    cache: AnswerCache<Key>,
}

impl SystemFontProvider {
    /// Over `source`, charging what it answers to `ledger` (the document budget) when given.
    pub fn new(source: Arc<dyn FaceSource>, ledger: Option<Arc<FaceLedger>>) -> SystemFontProvider {
        SystemFontProvider { source, ledger, cache: AnswerCache::new() }
    }
}

impl SystemFontProvider {
    /// Requests remembered (diagnostics, M5 tests): bounded by [`crate::registry::MAX_ANSWER_SLOTS`].
    pub fn remembered(&self) -> usize {
        self.cache.len()
    }
}

impl FontProvider for SystemFontProvider {
    fn budget_refusals(&self) -> u64 {
        self.ledger.as_ref().map_or(0, |l| l.refused())
    }

    fn full_name_face(&self, name: &str, ui_lcid: u16) -> Option<(String, bool, bool)> {
        self.source.full_name(name, ui_lcid)
    }

    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let key: Key = (family.to_owned(), bold, italic);
        self.cache.get_or_resolve(&key, || match self.source.pick(family, bold, italic) {
            None => Resolved::Missing,
            Some(picked) => super::from_source(picked, bold, italic, self.ledger.as_ref()),
        })
    }
}

/// The C# `SubstituteFontProvider`.
pub struct SubstituteFontProvider {
    inner: Arc<dyn FontProvider>,
}

#[cfg(all(feature = "system", not(target_family = "wasm")))]
impl SubstituteFontProvider {
    /// The process-wide provider of the server PDF (the C# `SubstituteFontProvider.Shared`): the
    /// substitute policy over the shared system index ([`crate::system::SystemFontIndex::shared`],
    /// scanned once, thread-safe), built at the first call. It stays over the index of that moment:
    /// after `rebuild_shared` build a new provider with [`SubstituteFontProvider::new`].
    pub fn shared() -> Arc<SubstituteFontProvider> {
        use std::sync::OnceLock;
        static SHARED: OnceLock<Arc<SubstituteFontProvider>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| {
            let source: Arc<dyn FaceSource> = crate::system::SystemFontIndex::shared();
            Arc::new(SubstituteFontProvider::new(Arc::new(SystemFontProvider::new(source, None))))
        }))
    }
}

impl SubstituteFontProvider {
    pub fn new(inner: Arc<dyn FontProvider>) -> SubstituteFontProvider {
        SubstituteFontProvider { inner }
    }
}

impl FontProvider for SubstituteFontProvider {
    fn budget_refusals(&self) -> u64 {
        self.inner.budget_refusals()
    }

    fn full_name_face(&self, name: &str, ui_lcid: u16) -> Option<(String, bool, bool)> {
        self.inner.full_name_face(name, ui_lcid)
    }

    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        if let Some(face) = self.inner.face(family, bold, italic) {
            return Some(face);
        }
        let sub = csharp_metric_substitute(family)?;
        let stand = self.inner.face(sub, bold, italic)?;
        Some(stand.marked_stand_in())
    }
}

/// C# `FontCatalog.MetricSubstitute(family)`: `family.Trim().ToLowerInvariant()` looked up in the
/// catalog read from `shared/fonts/src/catalog.ts` (the TS looks up `family.toLowerCase()` with no
/// trim).
pub fn csharp_metric_substitute(family: &str) -> Option<&'static str> {
    metric_substitute(&to_lower_invariant(trim_csharp(family)))
}

/// `string.Trim()`: leading and trailing `char.IsWhiteSpace` (the Unicode `White_Space` set).
fn trim_csharp(s: &str) -> &str {
    #[expect(
        clippy::disallowed_methods,
        reason = "C# char.IsWhiteSpace is the Unicode White_Space set, which is what str::trim uses; the JS set (uo_jsnum) is not the reference here"
    )]
    s.trim()
}

/// `string.ToLowerInvariant()`: the simple (one code point to one) Unicode lowercase mapping; a
/// code point whose lowercase is more than one stays as it is.
fn to_lower_invariant(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_lookup_trims_and_lowers_like_csharp() {
        assert_eq!(csharp_metric_substitute("Calibri"), Some("Carlito"));
        assert_eq!(csharp_metric_substitute("  CAMBRIA\u{a0}"), Some("Caladea"));
        assert_eq!(csharp_metric_substitute("Arial MT"), Some("Liberation Sans"));
        assert_eq!(csharp_metric_substitute("Aptos"), None);
        assert_eq!(csharp_metric_substitute(""), None);
        assert_eq!(csharp_metric_substitute("\u{feff}calibri"), None, "U+FEFF is not C# white space");
        assert_eq!(csharp_metric_substitute("\u{85}calibri\u{2028}"), Some("Carlito"), "NEL and the line separator are");
    }
}
