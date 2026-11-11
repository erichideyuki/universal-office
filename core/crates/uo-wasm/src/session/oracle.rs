//! The session's `LayoutOracle` in the binding: uo-docx-layout's [`EngineOracle`] over the host's fonts
//! (the `viewer` policy with the document's embedded fonts) and hyphenation patterns, built on the
//! first call of a command (typing, Enter and the other hot paths never ask the layout, so they never
//! build an engine). Without a font environment (`uo_session_set_env` never called) every method
//! answers a layout error, which the command reports as `CommandError::Layout`.

use std::cell::OnceCell;
use std::sync::Arc;

use uo_docx::read::EmbeddedFont;
use uo_docx::text::Utf16;
use uo_docx::view::{DocRef, DocView, FieldScope, HyphenCandidate, HyphenSkips, LayoutError, LayoutErrorKind, LayoutOracle, RunRef};
use uo_docx_layout::LayoutOptions;
use uo_docx_layout::fonts::{EmbeddedFontProvider, FontProvider};
use uo_docx_layout::oracle::EngineOracle;
use uo_fonts::policy::{PolicyName, provider};

use super::handle::HostFonts;

/// The provider of a session's layouts and oracle: the `viewer` policy over the host's registry, the
/// document's embedded fonts in front (`withEmbeddedFonts`).
///
/// # Errors
/// The policy does not build (a message for the last error).
pub fn font_provider(fonts: &HostFonts, embedded: &[EmbeddedFont]) -> Result<Arc<dyn FontProvider>, &'static str> {
    let p: Arc<dyn FontProvider> = match provider(&fonts.registry, PolicyName::Viewer) {
        Ok(p) => Arc::from(p),
        Err(_) => return Err("font policy"),
    };
    Ok(EmbeddedFontProvider::wrap(p, embedded, fonts.registry.cache(), Some(fonts.registry.ledger())))
}

fn no_fonts(detail: &str) -> LayoutError {
    LayoutError { kind: LayoutErrorKind::Internal, detail: detail.into() }
}

/// The oracle of one command (see the module comment). `provider` is the caller's cell, so that the
/// engine can borrow the provider it builds.
pub struct LazyOracle<'a> {
    pub fonts: Option<&'a HostFonts>,
    pub embedded: &'a [EmbeddedFont],
    pub options: &'a LayoutOptions,
    pub provider: &'a OnceCell<Option<Arc<dyn FontProvider>>>,
    inner: Option<EngineOracle<'a>>,
}

impl<'a> LazyOracle<'a> {
    #[must_use]
    pub fn new(
        fonts: Option<&'a HostFonts>,
        embedded: &'a [EmbeddedFont],
        options: &'a LayoutOptions,
        provider: &'a OnceCell<Option<Arc<dyn FontProvider>>>,
    ) -> LazyOracle<'a> {
        LazyOracle { fonts, embedded, options, provider, inner: None }
    }

    /// Whether an engine was built (the tests check typing never builds one).
    #[must_use]
    pub fn built(&self) -> bool {
        self.inner.is_some()
    }

    fn engine(&mut self) -> Result<&mut EngineOracle<'a>, LayoutError> {
        if self.inner.is_none() {
            let Some(fonts) = self.fonts else { return Err(no_fonts("no fonts: the session has no font environment")) };
            let embedded = self.embedded;
            let p = self.provider.get_or_init(|| font_provider(fonts, embedded).ok());
            let Some(p) = p.as_deref() else { return Err(no_fonts("no fonts: the font policy does not build")) };
            self.inner = Some(EngineOracle::new(p, Arc::clone(&fonts.hyphenation), self.options.clone()));
        }
        self.inner.as_mut().ok_or_else(|| no_fonts("no engine"))
    }
}

impl LayoutOracle for LazyOracle<'_> {
    fn field_values(&mut self, doc: DocView<'_>, scope: &FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError> {
        self.engine()?.field_values(doc, scope)
    }

    fn next_hyphenation(&mut self, doc: DocView<'_>, from: &DocRef, skip: &HyphenSkips) -> Result<Option<HyphenCandidate>, LayoutError> {
        self.engine()?.next_hyphenation(doc, from, skip)
    }

    fn shown_page_numbers(&mut self, doc: DocView<'_>, paras: &[DocRef]) -> Result<Vec<Option<f64>>, LayoutError> {
        self.engine()?.shown_page_numbers(doc, paras)
    }
}
