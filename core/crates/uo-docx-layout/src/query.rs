//! The viewer-, PDF- and editor-facing queries on a `Layout` (frontier F8; `refs.ts`,
//! `run-boxes.ts`, `pdf-navigation.ts`, the editor's `caretXIn`/`offsetAtX`/`locateRef`/
//! `isSortedByX`, `pageOf`). F0 fixes the signatures (plan §3); until F8 they answer "nothing"
//! and are listed in [`PENDING`].

use crate::model::{DocRef, DocView};
use crate::output::{Layout, LineGeom, LineRef};
use uo_render::{FaceId, Navigation};

/// Queries still to be ported (F8 empties the list; a gate-2 test asserts it is empty).
pub const PENDING: &[&str] =
    &["encode_page", "run_boxes", "measure_text", "navigation", "hit_test", "caret", "offset_at_x", "selection_rects", "page_of"];

/// A rectangle in page twips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Which runs `run_boxes` reports (the TS `pick(run)` callback: links of the PDF navigation,
/// custom inlines of the shell).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunFilter {
    /// runs with a hyperlink that are not custom inlines (`pdfNavigation`)
    Links,
    /// custom inlines (plugins draw over their fallback text, `paintCustomInlines`)
    CustomInlines,
}

/// `RunBox` (run-boxes.ts:13): the box of a picked run on a page, with what the plugin contract
/// exposes (K17e: the run's face and the model `RunProps` the renderer reads).
#[derive(Clone, Debug)]
pub struct RunBox {
    pub page: u32,
    pub rect: Rect,
    /// the picked value (`link` or the custom inline's id), UTF-16
    pub value: Box<[u16]>,
    pub face: Option<FaceId>,
    pub size_pt: f64,
}

/// Caret side at a line boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
    Before,
    After,
}

/// A caret rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretRect {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub h: f64,
}

/// A line of a page (`(page, index)`, in `lines` or in `content_lines`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineHandle {
    pub page: u32,
    pub line: u32,
    pub content: bool,
}

/// What a point hits.
#[derive(Clone, Debug)]
pub struct Hit {
    pub at: DocRef,
    pub line: Option<LineHandle>,
    /// an object (index into the page's `objects`)
    pub object: Option<u32>,
    /// a table cell (index into the page's `cells`)
    pub cell: Option<u32>,
}

impl Layout {
    /// Appends page `i` encoded for the canvas (uo-render codec + the Write section) — the
    /// encoder of frontier E, wired by F8.
    pub fn encode_page(&self, _i: usize, _out: &mut Vec<u8>) {}

    /// `runBoxes(full, layout, pick)` (run-boxes.ts).
    #[must_use]
    pub fn run_boxes(&self, _view: DocView<'_>, _pick: RunFilter) -> Vec<RunBox> {
        Vec::new()
    }

    /// The advance of `text` in `face` at `size_pt` (K17e: `paintCustomInlines` measured with
    /// `run0.face.font`).
    #[must_use]
    pub fn measure_text(&self, _face: FaceId, _size_pt: f64, _text: &[u16]) -> f64 {
        0.0
    }

    /// `pdfNavigation(full, layout)` in points (pdf-navigation.ts).
    #[must_use]
    pub fn navigation(&self, _view: DocView<'_>) -> Navigation {
        Navigation::default()
    }

    /// The lines of a paragraph, `(page, line)` in page order.
    pub fn lines_of<'s>(&'s self, at: &'s LineRef) -> impl Iterator<Item = (u32, &'s LineGeom)> + 's {
        self.pages.iter().zip(0u32..).flat_map(move |(p, i)| p.lines.iter().filter(move |l| &l.at == at).map(move |l| (i, l)))
    }

    /// What `(x, y)` on `page` hits.
    #[must_use]
    pub fn hit_test(&self, _page: u32, _x: f64, _y: f64) -> Option<Hit> {
        None
    }

    /// `caretXIn` and the line box of a model position.
    #[must_use]
    pub fn caret(&self, _at: &DocRef, _affinity: Affinity) -> Option<CaretRect> {
        None
    }

    /// `offsetAtX(line, x)`.
    #[must_use]
    pub fn offset_at_x(&self, _line: LineHandle, _x: f64) -> u32 {
        0
    }

    /// Selection rectangles between two model positions.
    #[must_use]
    pub fn selection_rects(&self, _from: &DocRef, _to: &DocRef) -> Vec<(u32, Rect)> {
        Vec::new()
    }

    /// The page of a model position (K17c: the editor's `pageOf` in the find list).
    #[must_use]
    pub fn page_of(&self, _at: &DocRef) -> Option<u32> {
        None
    }
}
