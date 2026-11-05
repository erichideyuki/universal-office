//! The PDF navigation of a layout (frontier F8): `pdf-navigation.ts` — body hyperlinks become
//! link rectangles (external: the URI; `#bookmark`: an internal destination at the first line of
//! the bookmark's block) and the Navigation pane's headings (`documentOutline`) become the outline,
//! in points. The value is app-neutral ([`uo_render::Navigation`]); uo-pdf writes it.

use crate::model::DocView;
use crate::output::{Layout, LineStory};
use crate::refs::Locator;
use crate::run_boxes::{RunFilter, run_boxes_with};
use uo_render::{LinkTarget, Navigation, OutlineEntry, PageLink, PdfLink};

/// Twips per point.
const T: f64 = 20.0;

/// `blockLine(layout, block)`: the first line, in page order, of body block `block` (`b<n>` or
/// below it): its page and top.
#[must_use]
pub fn block_line(layout: &Layout, block: f64) -> Option<(u32, f64)> {
    // `b${block}`: only a canonical non-negative integer can equal a line ref's root
    if !(block >= 0.0 && uo_jsnum::conv::is_integer(block) && block < 4_294_967_296.0) {
        return None;
    }
    let b = uo_jsnum::conv::to_uint32(block);
    for (pg, p) in layout.pages.iter().enumerate() {
        if let Some(l) = p.lines.iter().find(|l| l.at.story == LineStory::Body && l.at.path.0.first() == Some(&b)) {
            return Some((u32::try_from(pg).ok()?, l.y));
        }
    }
    None
}

/// `pdfNavigation(full, layout)` (pdf-navigation.ts:32).
#[must_use]
pub fn navigation(view: DocView<'_>, layout: &Layout) -> Navigation {
    let loc = Locator::new(view);
    let mut links = Vec::new();
    for b in run_boxes_with(&loc, layout, RunFilter::Links) {
        let (x_pt, y_top_pt, w_pt, h_pt) = (b.x / T, b.y / T, b.w / T, b.h / T);
        if b.value.first() == Some(&u16::from(b'#')) {
            let name = String::from_utf16(b.value.get(1..).unwrap_or(&[])).ok();
            let block = name.as_deref().and_then(|n| view.bookmarks().starts.get(n)).map(|s| s.block.to_number());
            let target = block.and_then(|blk| block_line(layout, blk));
            if let Some((page_index, y)) = target {
                links.push(PageLink {
                    page: b.page,
                    link: PdfLink { x_pt, y_top_pt, w_pt, h_pt, target: LinkTarget::Dest { page_index, dest_top_pt: y / T } },
                });
            }
        } else {
            links.push(PageLink { page: b.page, link: PdfLink { x_pt, y_top_pt, w_pt, h_pt, target: LinkTarget::Uri(b.value) } });
        }
    }
    let mut outline = Vec::new();
    for h in uo_docx::word::outline::document_outline(view, None) {
        if let Some((page_index, y)) = block_line(layout, f64::from(h.block)) {
            outline.push(OutlineEntry {
                title: h.text.units().into(),
                level: uo_jsnum::conv::to_uint32(h.level),
                page_index,
                top_pt: y / T,
            });
        }
    }
    Navigation { links, outline }
}
