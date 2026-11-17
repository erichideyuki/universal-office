//! The Slides PDF (R-S5, `core/docs/modules/uo-pdf.md` "Slides PDF"): the Slides display list of every slide
//! (`uo_render::slides`, emitted by `uo_dml_render::emit` at 1 px = 1 pt) interpreted into PDF pages through
//! `uo-pdf`, with the text laid out from the advance tables of the open fonts ([`measure`]).
//!
//! - [`pdf`]: a deck to PDF bytes (one page per slide, the page the slide's size) and a [`PdfReport`].
//! - [`pdf_from_lists`]: the interpreter alone over lists someone else emitted (tests, hostile lists).
//! - What the PDF cannot draw is not drawn and is counted in [`PdfReport::missing`] (the `Effect` items, canvas
//!   shadows and blurs, the compositing modes that are not PDF blend modes, conic gradients...); nothing panics
//!   on a hostile list: a limit stops the page, a malformed item is skipped.
//!
//! Where the vendor converter (`slides-processing`) produced PDFs, the differences are listed in the record.

mod css;
mod geom;
mod images;
mod interp;
pub mod measure;
mod paint;
mod recipe;
mod text;

use std::collections::BTreeMap;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_dml_render::PartSource;
use uo_dml_render::emit::EmitCaches;
use uo_dml_render::text::{MAX_ROUNDS, MeasureTable};
use uo_pdf::{FontId, PdfDocument, PdfError, PdfImage, PdfLimits};
use uo_render::slides::{SlidesList, SlidesResources, validate};

use crate::deck::Deck;
use crate::emit::{EmitFail, View, emit_slide};
use crate::error::PptxError;
pub use measure::{FaceSet, FontSource, fill_measures, text_width};

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
}

impl<'a> PdfOptions<'a> {
    #[must_use]
    pub fn new(fonts: &'a dyn FontSource) -> PdfOptions<'a> {
        PdfOptions { fonts, compress: true, limits: PdfLimits::DEFAULT, image_limits: uo_image::ImageLimits::default() }
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
/// `effect:bevel`, `effect:warp`, `effect:metafile-rop`, `shadow`, `blur`, `composite:<mode>` (the canvas
/// modes PDF has no blend mode for), `gradient:conic`, `gradient:stroke`, `pattern:stroke`, `clear-rect`,
/// `layer:dropped` (a layer a skipped destination-in/out made wrong), `layer:depth`, `media-frame`,
/// `image:recipe` (a metafile/PICT raster the shell replays), `image:unsupported`, `image:jpeg-effect`,
/// `font:missing` (no face), `font:not-embeddable`, `glyph:missing`, `text:baseline`, `transform:singular`,
/// `chart:frame` (a chart the shell paints with `uo-charts`), `hyperlink:unplaced`.
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
    pub links: u64,
    /// metafile, PICT and TIFF pictures played as vector content
    pub recipes: u64,
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

    /// One page: the list interpreted over the document's resources.
    pub(crate) fn page(
        &mut self,
        list: &SlidesList,
        res: &SlidesResources,
        src: &dyn PartSource,
        links: &[(String, String)],
    ) -> Result<(), PdfFail> {
        validate(list, &res.lens()).map_err(|_| PdfFail::Pdf(PdfError::Invalid("the Slides list is inconsistent")))?;
        let page = interp::run(self, list, res, src);
        let (w, h) = (list.view.sw, list.view.sh);
        let (w, h) = (if w.is_finite() && w > 0.0 { w } else { 720.0 }, if h.is_finite() && h > 0.0 { h } else { 540.0 });
        let Some(doc) = self.doc.as_mut() else { return Err(PdfFail::Pdf(PdfError::Invalid("the document is closed"))) };
        let mut pw = doc.add_page(w, h);
        pw.raw(page.ops, &page.content);
        // the hyperlinks of the model over the runs drawn: the next run with the link's text, in order
        let (mut placed_links, mut unplaced) = (0u64, 0u64);
        let mut j = 0usize;
        for p in &page.placed {
            let found =
                links.iter().enumerate().skip(j).take(4).find(|(_, (t, _))| uo_jsnum::text::trim(t) == uo_jsnum::text::trim(&p.text));
            if let Some((k, (_, url))) = found {
                unplaced += (k - j) as u64;
                j = k + 1;
                pw.add_link(&uo_pdf::PdfLink {
                    x_pt: p.x,
                    y_top_pt: p.y - p.size * 0.9,
                    w_pt: p.w,
                    h_pt: p.size * 1.2,
                    uri: Some(url.clone()),
                    ..uo_pdf::PdfLink::default()
                });
                placed_links += 1;
            }
        }
        unplaced += links.len().saturating_sub(j) as u64;
        self.report.links += placed_links;
        for _ in 0..unplaced {
            self.report.note("hyperlink:unplaced");
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
        let mut res = SlidesResources::new();
        let mut caches = EmitCaches::new();
        let list = emit_with_measures(deck, index, &mut table, &mut res, &mut caches, options);
        match list {
            Some(emission) => {
                w.report.note_charts(emission.chart_frames.len());
                let links = deck.slide(index).map(|s| slide_links(&s)).unwrap_or_default();
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
        w.page(list, res, src, &[])?;
    }
    w.finish()
}

/// The hyperlinks of a slide's text, in reading order: the text of each linked run and its address. The list
/// does not carry them (the display list is the canvas calls), so they are placed over the runs drawn by text.
fn slide_links(slide: &crate::model::SlideModel) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for sh in &slide.shapes {
        let Some(tb) = sh.tx_body.as_deref() else { continue };
        for p in &tb.paragraphs {
            for r in &p.runs {
                if let Some(url) = r.extras.as_deref().and_then(|e| e.hyperlink.as_deref())
                    && !r.text.is_empty()
                {
                    out.push((r.text.to_string(), url.to_owned()));
                }
            }
        }
    }
    out
}
