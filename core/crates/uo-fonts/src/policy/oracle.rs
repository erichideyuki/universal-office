//! The `oracle` policy: `createNodeFontProvider` of `write/tools/corpus-lab/node-fonts.mjs`
//! (lines 162-181), the provider the parity corpus ran the frozen TS engine with. Test-only in
//! the product: it exists so the Rust layout can be driven by the same fonts the oracle used.
//!
//! The pick (lower-case family, name ID 1 index then typographic ID 16, lowest score, the first in
//! index order on a tie) is the [`FaceSource`](crate::registry::FaceSource)'s. What is the
//! policy's: the answers are cached per `family.toLowerCase()|bold|italic`, so `Arial` and `ARIAL`
//! share one answer; `missing` names families to treat as not installed (the corpus lab imitates
//! the machine that produced the reference PDFs, which lacked the Meiryo UI).

use std::collections::HashSet;
use std::sync::Arc;

use uo_jsnum::text::to_lowercase;

use crate::registry::answers::{AnswerCache, Resolved};
use crate::registry::{FaceLedger, FaceSource, FontAnswer, FontProvider};

type Key = (String, bool, bool);

pub struct OracleProvider {
    source: Arc<dyn FaceSource>,
    ledger: Option<Arc<FaceLedger>>,
    missing: HashSet<String>,
    cache: AnswerCache<Key>,
}

impl OracleProvider {
    /// `missing`: families (any case) to answer `None` for, as `opts.missing` of node-fonts.
    pub fn new(source: Arc<dyn FaceSource>, ledger: Option<Arc<FaceLedger>>, missing: &[&str]) -> OracleProvider {
        OracleProvider { source, ledger, missing: missing.iter().map(|f| to_lowercase(f)).collect(), cache: AnswerCache::new() }
    }
}

impl OracleProvider {
    /// Requests remembered (diagnostics, M5 tests): bounded by [`crate::registry::MAX_ANSWER_SLOTS`].
    pub fn remembered(&self) -> usize {
        self.cache.len()
    }
}

impl FontProvider for OracleProvider {
    fn budget_refusals(&self) -> u64 {
        self.ledger.as_ref().map_or(0, |l| l.refused())
    }

    fn full_name_face(&self, name: &str, ui_lcid: u16) -> Option<(String, bool, bool)> {
        self.source.full_name(name, ui_lcid)
    }

    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        let lower = to_lowercase(family);
        let key: Key = (lower.clone(), bold, italic);
        self.cache.get_or_resolve(&key, || {
            if self.missing.contains(&lower) {
                return Resolved::Missing;
            }
            match self.source.pick(family, bold, italic) {
                None => Resolved::Missing,
                Some(picked) => super::from_source(picked, bold, italic, self.ledger.as_ref()),
            }
        })
    }
}
