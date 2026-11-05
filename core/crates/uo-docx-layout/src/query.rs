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
use crate::section::{SectionError, SectionModel};
use uo_render::{DeltaMark, Navigation};

/// Queries still to be ported (gate 2 needs the list empty). `encode_page` landed with G8
/// ([`crate::section`], record "Write page section (G8)").
pub const PENDING: &[&str] = &[];

impl Layout {
    /// Appends page `i` encoded for the canvas: uo-render's message (items, the resource deltas
    /// since `mark`, the painter hints) carrying the Write section of [`crate::section`] as its
    /// `app` — the page's lines, chars, formats, objects, cells and, from `m` (gathered once per
    /// layout by [`SectionModel::new`]), each body line's paragraph text and heading level for the
    /// viewer's text layer. `out` and `mark` are untouched on error.
    ///
    /// # Errors
    /// See [`SectionError`].
    pub fn encode_page(&self, i: usize, m: &SectionModel, mark: &mut DeltaMark, out: &mut Vec<u8>) -> Result<(), SectionError> {
        let page = self.pages.get(i).ok_or(SectionError::NoPage)?;
        let app = self.write_section(i, m, mark.generation())?;
        uo_render::encode_page_for_canvas(&page.page, &self.resources, &app, mark, out).map_err(SectionError::Render)
    }

    /// `collectAnnotations(full, layout)` of the viewer (annotations.ts): comments with the
    /// rectangles of their text, and the headings of the outline pane (G8).
    #[must_use]
    pub fn annotations(&self, view: DocView<'_>) -> crate::annotations::Annotations {
        crate::annotations::annotations(view, self)
    }

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
