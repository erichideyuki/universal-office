//! NUMPAGES as Word shows it (`docx-layout/src/numpages.ts`): one page less when the body has a
//! cover page from the gallery and the first section starts numbering at 0.

use crate::model::para::Paragraph;

/// `COVER_PAGES_GALLERY`: the `w:docPartGallery` of Word's cover pages.
pub const COVER_PAGES_GALLERY: &str = "Cover Pages";

/// `hasCoverPage(paragraphs)` (numpages.ts:8): a block content control of the cover-page gallery.
pub fn has_cover_page<'a>(paragraphs: impl IntoIterator<Item = &'a Paragraph>) -> bool {
    paragraphs.into_iter().any(|p| p.props.block_sdt.as_deref().and_then(|s| s.gallery.as_deref()) == Some(COVER_PAGES_GALLERY))
}

/// `numPagesShown(pages, firstPageNumber, coverPage)` (numpages.ts:28).
#[must_use]
pub fn num_pages_shown(pages: f64, first_page_number: f64, cover_page: bool) -> f64 {
    if cover_page && first_page_number == 0.0 { pages - 1.0 } else { pages }
}
