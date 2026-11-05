//! The viewer-, PDF- and editor-facing queries on a `Layout` (frontier F8), gathered: run boxes
//! and the plugin inlines' faces ([`crate::run_boxes`]), the PDF navigation
//! ([`crate::navigation`]), the line refs back to the model ([`crate::refs`]), and the editor's
//! caret, click and selection geometry ([`crate::hit`]: [`LineIndex`] = the editor's `eng.lines`,
//! [`EditLines`] = its `layoutCache`). The `LayoutOracle` of uo-docx is [`crate::oracle`].

use crate::hit::{EditLines, EditScope, LineIndex};
use crate::model::DocView;
use crate::output::{Layout, LineGeom, LineRef};
use crate::refs::Locator;
use crate::run_boxes::{RunBox, RunFilter};
use uo_render::Navigation;

/// Queries still to be ported (gate 2 needs the list empty). `encode_page` is the Write section
/// encoder of frontier E (uo-render `AppSection` for the Write geometry), not F8's.
pub const PENDING: &[&str] = &["encode_page"];

impl Layout {
    /// Appends page `i` encoded for the canvas (uo-render codec + the Write section) — frontier
    /// E's encoder; nothing until it lands.
    pub fn encode_page(&self, _i: usize, _out: &mut Vec<u8>) {}

    /// `runBoxes(full, layout, pick)` (run-boxes.ts).
    #[must_use]
    pub fn run_boxes(&self, view: DocView<'_>, pick: RunFilter) -> Vec<RunBox> {
        crate::run_boxes::run_boxes(view, self, pick)
    }

    /// `pdfNavigation(full, layout)` in points (pdf-navigation.ts).
    #[must_use]
    pub fn navigation(&self, view: DocView<'_>) -> Navigation {
        crate::navigation::navigation(view, self)
    }

    /// The lines of a paragraph, `(page, line)` in page order.
    pub fn lines_of<'s>(&'s self, at: &'s LineRef) -> impl Iterator<Item = (u32, &'s LineGeom)> + 's {
        self.pages.iter().zip(0u32..).flat_map(move |(p, i)| p.lines.iter().filter(move |l| &l.at == at).map(move |l| (i, l)))
    }

    /// The editor's `eng.lines`: every page line located in the model (`engLineAt`, `pageOf`).
    #[must_use]
    pub fn line_index(&self, view: DocView<'_>) -> LineIndex {
        LineIndex::new(&Locator::new(view), self)
    }

    /// The editor's `layoutCache` for the paragraphs of `scope` (hit test, caret, selection).
    #[must_use]
    pub fn edit_lines(&self, view: DocView<'_>, index: &LineIndex, scope: &EditScope<'_>) -> EditLines {
        EditLines::new(&Locator::new(view), self, index, scope)
    }
}
