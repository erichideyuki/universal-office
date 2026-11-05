//! The `LayoutOracle` of uo-docx (G21b, the two methods D2a's `ExecCx` carries) over this engine
//! (frontier F8). The plan puts it in the bindings; until `uo-wasm`/`uo-ffi` carry a layout it
//! lives here behind the `oracle` feature (on by default), and a binding wraps it.
//!
//! - `field_values` = the editor's `refreshFieldCache` (universal-write.ts:6457): a full layout
//!   with `onField`, the last value of each field run in first-set order over every pass.
//! - `next_hyphenation` = the layout half of `nextManualHyphenation` (universal-write.ts:17337): a
//!   layout with `hyphenation: 'force'` and the refused points, then the first body line, in page
//!   order, whose automatic hyphen sits at or after `from`. The word around it, the `b - a < 2`
//!   skip, the `to` bound and the points are the session's (D2b-F10).

use std::sync::Arc;

use crate::LayoutEngine;
use crate::fonts::FontProvider;
use crate::model::{BlockPath, DocRef, DocView, FieldScope, HyphenCandidate, HyphenSkips, RunRef, StoryRef, Utf16};
use crate::options::{Hyphenation, LayoutOptions};
use crate::output::Layout;
use crate::refs::{LocKind, Locator};
use uo_docx::view::{LayoutError as OracleError, LayoutErrorKind, LayoutOracle};

/// The engine as the session's oracle: the host's provider, the engine (its hyphenation
/// sessions) and the options of the host's layouts (`layoutOpts()`: language, clock, loaded
/// pattern sets, revision view).
pub struct EngineOracle<'p> {
    provider: &'p dyn FontProvider,
    engine: LayoutEngine,
    pub options: LayoutOptions,
}

impl<'p> EngineOracle<'p> {
    #[must_use]
    pub fn new(
        provider: &'p dyn FontProvider,
        hyphenation: Arc<uo_fonts::hyphen::HyphenationRegistry>,
        options: LayoutOptions,
    ) -> EngineOracle<'p> {
        EngineOracle { provider, engine: LayoutEngine::with_hyphenation(hyphenation), options }
    }

    fn run(&mut self, doc: DocView<'_>, o: &LayoutOptions) -> Result<&Layout, OracleError> {
        self.engine.layout(doc, self.provider, o).map_err(to_oracle_error)
    }

    /// Drops the engine's caches (M1).
    pub fn clear_caches(&mut self) {
        self.engine.clear_caches();
    }
}

/// A layout failure as the session sees it.
#[must_use]
pub fn to_oracle_error(e: crate::LayoutError) -> OracleError {
    let kind = match e {
        crate::LayoutError::Limit(crate::LimitKind::Work) => LayoutErrorKind::Budget,
        crate::LayoutError::Limit(_) => LayoutErrorKind::Limit,
        _ => LayoutErrorKind::Internal,
    };
    OracleError { kind, detail: e.to_string().into() }
}

/// The first body hyphenation point of `layout` at or after `from` (`comparePos(at, from) >= 0`,
/// positions compared by body block, then offset), in page and line order.
#[must_use]
pub fn first_hyphenation(view: DocView<'_>, layout: &Layout, from: &DocRef) -> Option<HyphenCandidate> {
    let loc = Locator::new(view);
    let from_block = from.path.top();
    for p in &layout.pages {
        for l in &p.lines {
            let Some(h) = l.hyph else { continue };
            let at = loc.locate(p.section, &l.at);
            if at.kind != LocKind::Body {
                continue;
            }
            let Some(para) = at.para else { continue };
            let b = para.path.top();
            if b < from_block || (b == from_block && h < from.offset) {
                continue;
            }
            return Some(HyphenCandidate { at: DocRef { story: StoryRef::Body, path: BlockPath::root(b), offset: h } });
        }
    }
    None
}

impl LayoutOracle for EngineOracle<'_> {
    fn field_values(&mut self, doc: DocView<'_>, scope: &FieldScope) -> Result<Vec<(RunRef, Utf16)>, OracleError> {
        let mut o = self.options.clone();
        o.collect_fields = Some(scope.clone());
        let l = self.run(doc, &o)?;
        Ok(l.fields.clone().unwrap_or_default())
    }

    fn next_hyphenation(&mut self, doc: DocView<'_>, from: &DocRef, skip: &HyphenSkips) -> Result<Option<HyphenCandidate>, OracleError> {
        let mut o = self.options.clone();
        o.hyphenation = Hyphenation::Force;
        o.hyphenation_skips = skip.clone();
        o.collect_fields = None;
        let l = self.run(doc, &o)?;
        Ok(first_hyphenation(doc, l, from))
    }
}
