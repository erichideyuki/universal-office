//! The Slides PDF (R-S5, `core/docs/modules/uo-pdf.md` "Slides PDF"): the Slides display list of every slide
//! (`uo_render::slides`, emitted by `uo_dml_render::emit` at 1 px = 1 pt) interpreted into PDF pages through
//! `uo-pdf`, with the text laid out from the advance tables of the open fonts ([`measure`]).
//!
//! - [`pdf`]: a deck to PDF bytes (one page per slide, the page the slide's size) and a [`PdfReport`].
//! - [`pdf_from_lists`]: the interpreter alone over lists someone else emitted (tests, hostile lists).
//! - What needs pixels (canvas shadows, soft edges and reflections, the bevel) is rasterized ([`raster`], the canvas over
//!   pixels) and placed as images, the way Word writes its effects; WordArt warps are outlines ([`warp`]); a character the
//!   run's face lacks is drawn with a fallback face ([`FontSource::fallback`]).
//! - What the PDF still cannot draw is not drawn and is counted in [`PdfReport::missing`] (the metafile XOR, pictures
//!   no decoder reads, characters no face has...); nothing panics on a hostile list: a limit stops the page, a
//!   malformed item is skipped.
//!
//! Where the vendor converter (`slides-processing`) produced PDFs, the differences are listed in the record.

mod css;
mod effects;
mod geom;
mod images;
mod interp;
mod links;
pub mod measure;
mod paint;
mod raster;
mod recipe;
mod text;
mod warp;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustc_hash::FxHashMap;
use uo_dml_render::PartSource;
use uo_dml_render::emit::{EmitCaches, RunLink};
use uo_dml_render::text::{MAX_ROUNDS, MeasureTable};
use uo_pdf::{FontId, PdfDocument, PdfError, PdfImage, PdfLimits};
use uo_render::slides::{SlidesList, SlidesResources, validate};

use crate::deck::Deck;
use crate::emit::{EmitFail, View, emit_slide};
use crate::error::PptxError;
use crate::links::{LinkSource, LinkTarget, ShowJump};
pub use measure::{FaceSet, FontSource, fallback_chain, fallback_families, fill_measures, text_width, text_width_in};

/// 1 px of the display list is 1 pt of the page: the slide is emitted at `1 / 12700` px per EMU.
pub const PX_PER_EMU: f64 = 1.0 / 12700.0;

/// What [`pdf`] needs from the host.
pub struct PdfOptions<'a> {
    /// The faces of the document's families (the host's font policy; open fonts, or the host's own files).
    pub fonts: &'a dyn FontSource,
    /// Compress the streams (the writer's deflate rule).
    pub compress: bool,
    pub limits: PdfLimits,
    pub image_limits: uo_image::ImageLimits,
    /// Raised by the host to stop the export: checked before each slide and before each raster effect (shadow,
    /// raster layer, bevel), the export then fails with [`PdfFail::Pdf`]`(`[`PdfError::Cancelled`]`)` (the Write PDF's
    /// flag and error, `uo_pdf::PdfOptions::cancel`).
    pub cancel: Option<Arc<AtomicBool>>,
}

impl<'a> PdfOptions<'a> {
    #[must_use]
    pub fn new(fonts: &'a dyn FontSource) -> PdfOptions<'a> {
        PdfOptions { fonts, compress: true, limits: PdfLimits::DEFAULT, image_limits: uo_image::ImageLimits::default(), cancel: None }
    }
}

/// Why no PDF came out (a slide that fails alone becomes a blank page and is listed in the report).
#[derive(Clone, PartialEq, Debug)]
pub enum PdfFail {
    Deck(PptxError),
    Pdf(PdfError),
}

impl std::fmt::Display for PdfFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfFail::Deck(e) => write!(f, "{e}"),
            PdfFail::Pdf(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PdfFail {}

impl From<PdfError> for PdfFail {
    fn from(e: PdfError) -> Self {
        PdfFail::Pdf(e)
    }
}

/// The PDF and what it could not draw.
#[derive(Debug)]
pub struct PdfOutput {
    pub bytes: Vec<u8>,
    pub report: PdfReport,
}

/// The counters of one document. `missing` keys (each counts the items skipped or approximated):
/// `effect:bevel` (a bevel whose page could not be rasterized), `effect:metafile-rop`, `blur` and `composite:<mode>`
/// (outside a layer: no raster pass takes them), `raster:fallback` (a layer that needed raster compositing but holds
/// something the raster cannot read, a JPEG or a metafile: drawn as vector, its blurs and modes counted too),
/// `bevel:underlay` (a bevel whose page holds such a picture: shaded over the rest), `warp:picture-fill`,
/// `gradient:conic`, `gradient:stroke`, `pattern:stroke`, `clear-rect`, `layer:dropped` (a layer a skipped
/// destination-in/out made wrong), `layer:depth`, `media-frame`, `image:recipe-truncated`, `image:unsupported`,
/// `image:jpeg-effect`, `font:missing` (no face), `font:not-embeddable`, `glyph:missing` (no face has the
/// character, not even the fallbacks), `meta:xor-rect`, `transform:singular`, `chart:frame` (a chart the shell
/// paints with `uo-charts`), `hyperlink:unplaced` (a model link run with no drawn piece), `hyperlink:scheme` (an address
/// that is not http, https or mailto), `hyperlink:action` (a jump the PDF cannot express: last viewed, end of show).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PdfReport {
    pub pages: u32,
    /// Slides that could not be emitted or interpreted (their page is blank).
    pub failed_slides: Vec<u32>,
    pub missing: BTreeMap<&'static str, u64>,
    /// Items skipped or approximated, per page.
    pub missing_per_page: Vec<u64>,
    pub text_runs: u64,
    pub glyphs: u64,
    pub fonts: u32,
    pub images: u32,
    pub layers: u64,
    /// link annotations placed (run pieces, shape clicks and jumps)
    pub links: u64,
    /// of `links`, the ones that go to a page of the document (slide and show jumps)
    pub internal_links: u64,
    /// of `links`, the run links placed over their shape's box because its text drew no run (WordArt warps)
    pub shape_wide_links: u64,
    /// linked pieces drawn entirely outside the page (text overflowing the slide): no annotation, not a missing effect
    pub links_off_page: u64,
    /// metafile, PICT and TIFF pictures played as vector content
    pub recipes: u64,
    /// canvas shadows drawn (an image each)
    pub shadows: u64,
    /// layers rasterized for their compositing (soft edges, reflections, blurs)
    pub raster_layers: u64,
    /// bevels shaded
    pub bevels: u64,
    /// WordArt warps drawn as outlines
    pub warps: u64,
    /// glyphs drawn with a fallback face (a character the run's face lacks)
    pub fallback_glyphs: u64,
}

impl PdfReport {
    /// Everything the PDF did not draw as the list says.
    #[must_use]
    pub fn missing_total(&self) -> u64 {
        self.missing.values().sum()
    }

    pub(crate) fn note(&mut self, key: &'static str) {
        *self.missing.entry(key).or_insert(0) += 1;
    }
}

/// What a picture is in the PDF: an image XObject, or a form (a metafile recipe played as vector content).
#[derive(Clone, Copy)]
pub(crate) enum PicSrc {
    Image(PdfImage),
    Form(u32),
}

/// A picture of the document, by what it was made of (shared by every slide for media parts).
#[derive(Clone)]
pub(crate) struct ImgHandle {
    pub src: PicSrc,
    /// the natural size in px
    pub w: f64,
    pub h: f64,
}

impl ImgHandle {
    /// The resource name `Do` draws.
    pub(crate) fn name(&self) -> String {
        match self.src {
            PicSrc::Image(i) => i.resource_name(),
            PicSrc::Form(f) => format!("Fm{f}"),
        }
    }

    pub(crate) fn is_form(&self) -> bool {
        matches!(self.src, PicSrc::Form(_))
    }
}

pub(crate) struct FontEntry {
    pub id: FontId,
    pub face: Arc<uo_fonts::Face>,
}

/// The document being written and the resources every page shares.
pub(crate) struct Writer<'a> {
    pub doc: Option<PdfDocument<Vec<u8>>>,
    pub opts: &'a PdfOptions<'a>,
    pub report: PdfReport,
    /// `(family, bold, italic)` as the list spells it -> the face the host answered, embedded once per file
    pub fonts: FxHashMap<(String, bool, bool), Option<Arc<FontEntry>>>,
    pub font_files: FxHashMap<usize, Arc<FontEntry>>,
    /// media by part name (+ effect key) -> embedded picture
    pub images: FxHashMap<String, Option<ImgHandle>>,
    pub fatal: Option<PdfError>,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(opts: &'a PdfOptions<'a>) -> Writer<'a> {
        Writer {
            doc: Some(PdfDocument::with_limits(Vec::new(), opts.compress, opts.limits)),
            opts,
            report: PdfReport::default(),
            fonts: FxHashMap::default(),
            font_files: FxHashMap::default(),
            images: FxHashMap::default(),
            fatal: None,
        }
    }

    /// Whether the host raised the cancellation flag (the error is kept for the end).
    pub(crate) fn cancelled(&mut self) -> bool {
        if self.opts.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            self.fatal.get_or_insert(PdfError::Cancelled);
            return true;
        }
        false
    }

    /// Keeps a limit or sink error for the end; a refused resource (`Invalid`) is just absent.
    pub(crate) fn note<V>(&mut self, r: Result<V, PdfError>) -> Option<V> {
        match r {
            Ok(v) => Some(v),
            Err(PdfError::Invalid(_)) => None,
            Err(e) => {
                self.fatal.get_or_insert(e);
                None
            }
        }
    }

    /// One page: the list interpreted over the document's resources, then its links.
    pub(crate) fn page(
        &mut self,
        list: &SlidesList,
        res: &SlidesResources,
        src: &dyn PartSource,
        links: &PageLinks<'_>,
    ) -> Result<(), PdfFail> {
        validate(list, &res.lens()).map_err(|_| PdfFail::Pdf(PdfError::Invalid("the Slides list is inconsistent")))?;
        let marks = links::LinkMarks::new(links.runs);
        let page = interp::run(self, list, res, src, &marks);
        let (w, h) = (list.view.sw, list.view.sh);
        let (w, h) = (if w.is_finite() && w > 0.0 { w } else { 720.0 }, if h.is_finite() && h > 0.0 { h } else { 540.0 });
        let page_index = i64::from(self.report.pages);
        let Some(doc) = self.doc.as_mut() else { return Err(PdfFail::Pdf(PdfError::Invalid("the document is closed"))) };
        let mut pw = doc.add_page(w, h);
        pw.raw(page.ops, &page.content);
        // (a) the run marks: one annotation per drawn piece, at the box the run was drawn under
        let raw = marks.take();
        // what each drawn piece wrote (for the unplaced count below)
        let covered: Vec<(u32, &str, String)> = raw
            .iter()
            .filter_map(|(li, _)| links.runs.get(*li))
            .map(|l| {
                let text = usize::try_from(l.run)
                    .ok()
                    .and_then(|r| list.runs.get(r))
                    .and_then(|r| interp::span_of(&list.texts, &r.text))
                    .map(String::from_utf16_lossy)
                    .unwrap_or_default();
                (l.shape, &*l.url, text)
            })
            .collect();
        let drawn = merge_marks(raw, links.runs);
        let (mut placed, mut refused, mut jumps, mut actions, mut off_page) = (0u64, 0u64, 0u64, 0u64, 0u64);
        for (li, b) in &drawn {
            let Some(l) = links.runs.get(*li) else { continue };
            // cut to the page: a piece entirely off it (text overflowing the slide) gets no annotation
            let b = [
                uo_jsnum::math::max(b[0], 0.0),
                uo_jsnum::math::max(b[1], 0.0),
                uo_jsnum::math::min(b[2], w),
                uo_jsnum::math::min(b[3], h),
            ];
            if !(b[2] > b[0] && b[3] > b[1]) {
                off_page += 1;
                continue;
            }
            if uo_pdf::safe_pdf_uri(Some(&l.url)).is_none() {
                refused += 1;
                continue;
            }
            pw.add_link(&uo_pdf::PdfLink {
                x_pt: b[0],
                y_top_pt: b[1],
                w_pt: b[2] - b[0],
                h_pt: b[3] - b[1],
                uri: Some(l.url.to_string()),
                ..uo_pdf::PdfLink::default()
            });
            placed += 1;
        }
        // (b) the shape-level clicks and jumps over the shape's box as the page shows it
        let last = i64::from(links.page_count.max(1)) - 1;
        for (b, target) in &links.shapes {
            let mut link =
                uo_pdf::PdfLink { x_pt: b[0], y_top_pt: b[1], w_pt: b[2] - b[0], h_pt: b[3] - b[1], ..uo_pdf::PdfLink::default() };
            match target {
                LinkTarget::Url(u) => {
                    if uo_pdf::safe_pdf_uri(Some(u)).is_none() {
                        refused += 1;
                        continue;
                    }
                    link.uri = Some(u.to_string());
                }
                LinkTarget::Slide(i) => link.page_index = Some(i64::from(*i)),
                LinkTarget::Show(ShowJump::Next) => link.page_index = Some((page_index + 1).min(last)),
                LinkTarget::Show(ShowJump::Previous) => link.page_index = Some((page_index - 1).max(0)),
                LinkTarget::Show(ShowJump::First) => link.page_index = Some(0),
                LinkTarget::Show(ShowJump::Last) => link.page_index = Some(last),
                LinkTarget::Show(ShowJump::LastViewed | ShowJump::End) => {
                    actions += 1;
                    continue;
                }
            }
            if link.uri.is_none() {
                jumps += 1;
            }
            pw.add_link(&link);
            placed += 1;
        }
        // a link run of the model is placed when a piece of its shape with its address was drawn (by address, not by text: caps,
        // symbol fonts and tabs change the drawn text); a shape whose text drew no linked piece at all (WordArt warps draw
        // outlines, not runs) gets its link over the shape's box, the TS viewer's hit-test geometry
        let mut unplaced = 0u64;
        let mut shape_wide: Vec<u32> = Vec::new();
        for (shape, url) in &links.model {
            let hit = covered.iter().any(|(s, u, t)| s == shape && *u == url.as_str() && !uo_jsnum::text::trim(t).is_empty());
            if hit {
                continue;
            }
            let drew_none = !covered.iter().any(|(s, _, _)| s == shape);
            match links.fallback.iter().find(|(s, _, _)| s == shape) {
                Some((_, b, u)) if drew_none => {
                    if !shape_wide.contains(shape) {
                        shape_wide.push(*shape);
                        if uo_pdf::safe_pdf_uri(Some(u)).is_some() {
                            pw.add_link(&uo_pdf::PdfLink {
                                x_pt: b[0],
                                y_top_pt: b[1],
                                w_pt: b[2] - b[0],
                                h_pt: b[3] - b[1],
                                uri: Some(u.clone()),
                                ..uo_pdf::PdfLink::default()
                            });
                            placed += 1;
                            self.report.shape_wide_links += 1;
                        } else {
                            refused += 1;
                        }
                    }
                }
                _ => unplaced += 1,
            }
        }
        self.report.links += placed;
        self.report.internal_links += jumps;
        self.report.links_off_page += off_page;
        for (key, n) in [("hyperlink:unplaced", unplaced), ("hyperlink:scheme", refused), ("hyperlink:action", actions)] {
            for _ in 0..n {
                self.report.note(key);
            }
        }
        pw.finish()?;
        self.report.pages += 1;
        self.report.missing_per_page.push(page.missing);
        if let Some(e) = page.fatal {
            return Err(PdfFail::Pdf(e));
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<PdfOutput, PdfFail> {
        if let Some(e) = self.fatal {
            return Err(PdfFail::Pdf(e));
        }
        let doc = self.doc.take().ok_or(PdfFail::Pdf(PdfError::Invalid("the document is closed")))?;
        let bytes = doc.build()?;
        Ok(PdfOutput { bytes, report: self.report })
    }
}

/// A deck as a PDF: one page per slide at the slide's size (1 px = 1 pt, EMU / 12700), the text from `options.fonts`.
///
/// A slide that cannot be read or emitted becomes a blank page of the slide's size and is listed in
/// [`PdfReport::failed_slides`].
///
/// # Errors
/// [`PdfFail::Deck`] when the presentation itself cannot be read; [`PdfFail::Pdf`] when a limit or the sink stops
/// the document.
pub fn pdf(deck: &mut Deck, options: &PdfOptions<'_>) -> Result<PdfOutput, PdfFail> {
    let (slide_w, slide_h, count) = {
        let p = deck.presentation().map_err(PdfFail::Deck)?;
        (p.slide_w, p.slide_h, p.slides.len())
    };
    let mut w = Writer::new(options);
    let mut table = MeasureTable::default();
    for index in 0..u32::try_from(count).unwrap_or(u32::MAX) {
        if w.cancelled() {
            return Err(PdfFail::Pdf(PdfError::Cancelled));
        }
        let mut res = SlidesResources::new();
        let mut caches = EmitCaches::new();
        let list = emit_with_measures(deck, index, &mut table, &mut res, &mut caches, options);
        match list {
            Some(emission) => {
                w.report.note_charts(emission.chart_frames.len());
                let page_count = u32::try_from(count).unwrap_or(u32::MAX);
                let links = page_links(deck, index, &emission.links, page_count);
                let src = deck.source();
                if let Err(e) = w.page(&emission.list, &res, &src, &links) {
                    match e {
                        PdfFail::Pdf(e @ (PdfError::Limit(_) | PdfError::Sink | PdfError::Cancelled)) => return Err(PdfFail::Pdf(e)),
                        _ => w.blank_page(index, slide_w, slide_h)?,
                    }
                }
            }
            None => w.blank_page(index, slide_w, slide_h)?,
        }
    }
    w.finish()
}

impl PdfReport {
    pub(crate) fn note_charts(&mut self, n: usize) {
        *self.missing.entry("chart:frame").or_insert(0) += n as u64;
    }
}

impl Writer<'_> {
    fn blank_page(&mut self, index: u32, slide_w: f64, slide_h: f64) -> Result<(), PdfFail> {
        self.report.failed_slides.push(index);
        let (w, h) = (slide_w * PX_PER_EMU, slide_h * PX_PER_EMU);
        let (w, h) = (if w.is_finite() && w > 0.0 { w } else { 720.0 }, if h.is_finite() && h > 0.0 { h } else { 540.0 });
        let Some(doc) = self.doc.as_mut() else { return Err(PdfFail::Pdf(PdfError::Invalid("the document is closed"))) };
        doc.add_page(w, h).finish()?;
        self.report.pages += 1;
        self.report.missing_per_page.push(0);
        Ok(())
    }
}

/// `emit_slide` with the measure rounds answered from the options' fonts (at most [`MAX_ROUNDS`]).
fn emit_with_measures(
    deck: &mut Deck,
    index: u32,
    table: &mut MeasureTable,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
    options: &PdfOptions<'_>,
) -> Option<uo_dml_render::emit::Emission> {
    let view = View { scale: PX_PER_EMU, ox: 0.0, oy: 0.0 };
    for _ in 0..=MAX_ROUNDS {
        match emit_slide(deck, index, view, table, res, caches) {
            Ok(e) => return Some(e),
            Err(EmitFail::Need(n)) => fill_measures(deck, options.fonts, table, &n.0),
            Err(EmitFail::Deck(_)) => return None,
        }
    }
    None
}

/// The interpreter over lists emitted elsewhere: one page per `(list, resources)`, with `src` giving the bytes
/// of the media parts the lists name.
///
/// # Errors
/// [`PdfFail::Pdf`] when a limit or the sink stops the document or a list is inconsistent.
pub fn pdf_from_lists(
    slides: &[(&SlidesList, &SlidesResources)],
    src: &dyn PartSource,
    options: &PdfOptions<'_>,
) -> Result<PdfOutput, PdfFail> {
    let mut w = Writer::new(options);
    for (list, res) in slides {
        if w.cancelled() {
            return Err(PdfFail::Pdf(PdfError::Cancelled));
        }
        w.page(list, res, src, &PageLinks::none())?;
    }
    w.finish()
}

/// [`pdf_from_lists`] with each list's hyperlinked runs (`Emission::links`): one annotation per drawn piece.
///
/// # Errors
/// As [`pdf_from_lists`].
pub fn pdf_from_lists_with_links(
    slides: &[(&SlidesList, &SlidesResources, &[RunLink])],
    src: &dyn PartSource,
    options: &PdfOptions<'_>,
) -> Result<PdfOutput, PdfFail> {
    let mut w = Writer::new(options);
    let page_count = u32::try_from(slides.len()).unwrap_or(u32::MAX);
    for (list, res, runs) in slides {
        if w.cancelled() {
            return Err(PdfFail::Pdf(PdfError::Cancelled));
        }
        w.page(list, res, src, &PageLinks { runs, model: Vec::new(), shapes: Vec::new(), fallback: Vec::new(), page_count })?;
    }
    w.finish()
}

/// Joins the drawn pieces of one link that touch (the layout places a run word by word, a vertical or right-to-left line
/// piece by piece): the same shape and address, the boxes overlapping by half their extent across the line and at most
/// 0.6 of the line's thickness apart along it — in a row or in a column.
fn merge_marks(marks: Vec<(usize, [f64; 4])>, runs: &[RunLink]) -> Vec<(usize, [f64; 4])> {
    use uo_jsnum::math::{max, min};
    let touches = |p: &[f64; 4], b: &[f64; 4]| {
        let (hp, hb, wp, wb) = (p[3] - p[1], b[3] - b[1], p[2] - p[0], b[2] - b[0]);
        let ov_y = min(p[3], b[3]) - max(p[1], b[1]);
        let ov_x = min(p[2], b[2]) - max(p[0], b[0]);
        let gap_x = max(b[0] - p[2], p[0] - b[2]);
        let gap_y = max(b[1] - p[3], p[1] - b[3]);
        (ov_y >= 0.5 * min(hp, hb) && gap_x <= 0.6 * max(hp, hb)) || (ov_x >= 0.5 * min(wp, wb) && gap_y <= 0.6 * max(wp, wb))
    };
    let mut out: Vec<(usize, [f64; 4])> = Vec::with_capacity(marks.len());
    for (li, b) in marks {
        if let Some((pl, pb)) = out.last_mut()
            && let (Some(p), Some(l)) = (runs.get(*pl), runs.get(li))
            && p.shape == l.shape
            && p.url == l.url
            && touches(pb, &b)
        {
            *pb = [min(pb[0], b[0]), min(pb[1], b[1]), max(pb[2], b[2]), max(pb[3], b[3])];
            continue;
        }
        out.push((li, b));
    }
    out
}

/// The links of one page: the emission's run marks, the model's link runs (for the unplaced count) and the shape-level
/// clicks and jumps in page pt (`x0, y0, x1, y1`).
pub(crate) struct PageLinks<'a> {
    pub runs: &'a [RunLink],
    /// `(shape, address)` of every model run with a `hyperlink` and some text
    pub model: Vec<(u32, String)>,
    pub shapes: Vec<([f64; 4], LinkTarget)>,
    /// the `RunUrl` boxes (turned with their shape): `(shape, box, address)`, placed for a shape that drew no linked piece
    pub fallback: Vec<(u32, [f64; 4], String)>,
    pub page_count: u32,
}

impl PageLinks<'_> {
    fn none() -> PageLinks<'static> {
        PageLinks { runs: &[], model: Vec::new(), shapes: Vec::new(), fallback: Vec::new(), page_count: 0 }
    }
}

/// The links of presentation slide `index` for its page: the model's link runs and the `ShapeClick`/`RunJump` boxes of
/// [`crate::links::slide_links`] at the PDF view, turned with their shape (the `RunUrl` boxes are not placed: the run
/// marks are exact).
fn page_links<'a>(deck: &mut Deck, index: u32, runs: &'a [RunLink], page_count: u32) -> PageLinks<'a> {
    let mut out = PageLinks { runs, model: Vec::new(), shapes: Vec::new(), fallback: Vec::new(), page_count };
    let Ok(slide) = deck.slide(index) else { return out };
    for (si, sh) in slide.shapes.iter().enumerate() {
        let Some(tb) = sh.tx_body.as_deref() else { continue };
        let Ok(si) = u32::try_from(si) else { break };
        for r in tb.paragraphs.iter().flat_map(|p| p.runs.iter()) {
            if let Some(url) = r.extras.as_deref().and_then(|e| e.hyperlink.as_deref())
                && !uo_jsnum::text::trim(&r.text).is_empty()
            {
                out.model.push((si, url.to_owned()));
            }
        }
    }
    let view = View { scale: PX_PER_EMU, ox: 0.0, oy: 0.0 };
    for b in crate::links::slide_links(deck, index, view).unwrap_or_default() {
        let rot = usize::try_from(b.shape).ok().and_then(|i| slide.shapes.get(i)).and_then(|s| s.xfrm.as_ref()).and_then(|x| x.rot);
        let Some(r) = links::rotated_box(b.rect, rot) else { continue };
        match (b.source, b.target) {
            (LinkSource::RunUrl, LinkTarget::Url(u)) => out.fallback.push((b.shape, r, u.to_string())),
            (LinkSource::RunUrl, _) => {}
            (_, t) => out.shapes.push((r, t)),
        }
    }
    out
}
