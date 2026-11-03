//! The PDF navigation of a layout: link rectangles and outline entries, in points, as
//! `pdf-navigation.ts` computes them. Produced by the app (Write: `Layout::navigation`, layout
//! F8), written by uo-pdf, which never sees the model.

/// `PdfNavigation` (pdf-navigation.ts:9).
#[derive(Clone, Debug, Default)]
pub struct Navigation {
    pub links: Vec<PageLink>,
    pub outline: Vec<OutlineEntry>,
}

/// `{ page, link }`: a link rectangle on page `page` (index).
#[derive(Clone, Debug)]
pub struct PageLink {
    pub page: u32,
    pub link: PdfLink,
}

/// `PdfLink` (pdf-writer/document.ts:13), rectangle in points from the page top.
#[derive(Clone, Debug)]
pub struct PdfLink {
    pub x_pt: f64,
    pub y_top_pt: f64,
    pub w_pt: f64,
    pub h_pt: f64,
    pub target: LinkTarget,
}

/// Where a link goes: `uri`, or `pageIndex` + `destTopPt`.
#[derive(Clone, Debug)]
pub enum LinkTarget {
    /// the run's `link` as the document wrote it (UTF-16; `safePdfUri` filters at write time)
    Uri(Box<[u16]>),
    /// an internal destination (bookmark)
    Dest { page_index: u32, dest_top_pt: f64 },
}

/// `PdfOutlineItem` (pdf-writer/document.ts:24).
#[derive(Clone, Debug)]
pub struct OutlineEntry {
    /// UTF-16 title (lone surrogates kept)
    pub title: Box<[u16]>,
    pub level: u32,
    pub page_index: u32,
    pub top_pt: f64,
}
