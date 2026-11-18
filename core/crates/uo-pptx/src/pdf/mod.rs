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
mod svg;
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
use measure::PdfFonts;
pub use measure::{FaceSet, FontSource, fallback_chain, fallback_families, fill_measures, powerpoint_chain, text_width, text_width_in};

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
    /// Keep the slides marked hidden (`p:sld show="0"`) as pages, as the frozen TS does (and PowerPoint's "Print hidden
    /// slides"). Default `false`, PowerPoint's Save as PDF (measured, wave 5): hidden slides are left out, the slide
    /// number fields keep the slide's own number, a jump to a hidden slide writes no link, and a deck whose slides are
    /// all hidden gives no PDF ([`PdfFail::NoVisibleSlides`]: PowerPoint refuses to save it).
    pub ts_hidden_slides: bool,
    /// Link rectangles as the frozen TS-era painter placed them: the emitter's run boxes joined per line fragment, cut to
    /// the page, pieces off the page dropped; show jumps by page arithmetic. Default `false`, PowerPoint's rule
    /// (measured, wave 5, record `uo-pdf.md` "Link rectangles"): one rectangle per line and formatting run (per word in a
    /// justified or distributed paragraph, split at tabs), the ink bounds of its glyphs, kept even off the page; no
    /// rectangle for a link whose range holds the text's last paragraph mark; show jumps resolved by slide.
    pub ts_link_rects: bool,
}

impl<'a> PdfOptions<'a> {
    /// PowerPoint's rules (the product default).
    #[must_use]
    pub fn new(fonts: &'a dyn FontSource) -> PdfOptions<'a> {
        PdfOptions {
            fonts,
            compress: true,
            limits: PdfLimits::DEFAULT,
            image_limits: uo_image::ImageLimits::default(),
            cancel: None,
            ts_hidden_slides: false,
            ts_link_rects: false,
        }
    }

    /// Every PDF switch on: the frozen TS behaviour (parity replays). The deck's own switches go with it:
    /// [`pdf_ts_strict`] sets both.
    #[must_use]
    pub fn ts_strict(fonts: &'a dyn FontSource) -> PdfOptions<'a> {
        PdfOptions { ts_hidden_slides: true, ts_link_rects: true, ..PdfOptions::new(fonts) }
    }
}

/// [`pdf`] with the frozen TS behaviour everywhere: `deck.set_rules(SlidesRules::ts_strict())` (the read model and the
/// emission, fallback fonts included) and [`PdfOptions::ts_strict`]'s PDF switches over `options`' other settings. The
/// deck keeps the TS rules afterwards.
///
/// # Errors
/// As [`pdf`].
pub fn pdf_ts_strict(deck: &mut Deck, options: &PdfOptions<'_>) -> Result<PdfOutput, PdfFail> {
    deck.set_rules(uo_dml_render::SlidesRules::ts_strict());
    let opts = PdfOptions { ts_hidden_slides: true, ts_link_rects: true, ..options.clone_settings(options.fonts) };
    pdf(deck, &opts)
}

impl<'a> PdfOptions<'a> {
    /// These options over another font source.
    fn clone_settings<'b>(&self, fonts: &'b dyn FontSource) -> PdfOptions<'b> {
        PdfOptions {
            fonts,
            compress: self.compress,
            limits: self.limits,
            image_limits: self.image_limits,
            cancel: self.cancel.clone(),
            ts_hidden_slides: self.ts_hidden_slides,
            ts_link_rects: self.ts_link_rects,
        }
    }
}

/// Why no PDF came out (a slide that fails alone becomes a blank page and is listed in the report).
#[derive(Clone, PartialEq, Debug)]
pub enum PdfFail {
    Deck(PptxError),
    Pdf(PdfError),
    /// Every slide is hidden and hidden slides are left out (PowerPoint's Save as PDF refuses such a deck).
    NoVisibleSlides,
}

impl std::fmt::Display for PdfFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfFail::Deck(e) => write!(f, "{e}"),
            PdfFail::Pdf(e) => write!(f, "{e}"),
            PdfFail::NoVisibleSlides => f.write_str("every slide is hidden"),
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
/// `svg:text`, `svg:image`, `svg:pattern`, `svg:marker`, `svg:filter`, `svg:mask`, `svg:spread`, `svg:effect`,
/// `svg:limit`, `svg:shadow` (the parts of an SVG picture not drawn or drawn without the effect, `pdf/svg.rs`),
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
    /// linked pieces drawn entirely outside the page (text overflowing the slide): no annotation under
    /// `ts_link_rects`; PowerPoint's rule keeps the rectangle beyond the media box (counted in `links` too)
    pub links_off_page: u64,
    /// link rectangles PowerPoint's rule writes no annotation for (`!ts_link_rects`): a link whose range holds its text's
    /// last paragraph mark, a jump to a hidden slide that was left out
    pub links_not_written: u64,
    /// hidden slides left out of the PDF (`!ts_hidden_slides`)
    pub hidden_slides: u32,
    /// metafile, PICT and TIFF pictures played as vector content
    pub recipes: u64,
    /// SVG pictures (no raster beside them) played as vector content
    pub svgs: u64,
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
            .filter_map(|m| links.runs.get(m.link))
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
        let (mut placed, mut refused, mut jumps, mut actions, mut off_page, mut not_written) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
        let drawn: Vec<(usize, [f64; 4])> = if self.opts.ts_link_rects {
            merge_marks(raw.iter().map(|m| (m.link, m.layout)).collect(), links.runs)
        } else {
            let mut out = Vec::new();
            for r in link_rects(&raw, links.runs, list, &links.ranges) {
                if r.dropped {
                    not_written += 1;
                } else {
                    out.push((r.link, r.rect));
                }
            }
            out
        };
        for (li, b) in &drawn {
            let Some(l) = links.runs.get(*li) else { continue };
            let off = !(b[2] > 0.0 && b[0] < w && b[3] > 0.0 && b[1] < h);
            let b = if self.opts.ts_link_rects {
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
                b
            } else {
                // PowerPoint keeps the rectangle where the text is, beyond the media box too
                if off {
                    off_page += 1;
                }
                *b
            };
            let mut link =
                uo_pdf::PdfLink { x_pt: b[0], y_top_pt: b[1], w_pt: b[2] - b[0], h_pt: b[3] - b[1], ..uo_pdf::PdfLink::default() };
            // a run-level jump (the model's `ppaction://` address): a page of this PDF
            if let Some(dest) = links.run_dests.get(&*l.url) {
                match dest {
                    ShapeDest::Page(p) => link.page_index = Some(i64::from(*p)),
                    ShapeDest::NotWritten => {
                        not_written += 1;
                        continue;
                    }
                    ShapeDest::Show(j) => match j {
                        ShowJump::Next => link.page_index = Some((page_index + 1).min(i64::from(links.page_count.max(1)) - 1)),
                        ShowJump::Previous => link.page_index = Some((page_index - 1).max(0)),
                        ShowJump::First => link.page_index = Some(0),
                        ShowJump::Last => link.page_index = Some(i64::from(links.page_count.max(1)) - 1),
                        ShowJump::LastViewed | ShowJump::End => {
                            actions += 1;
                            continue;
                        }
                    },
                    ShapeDest::Url(_) => {}
                }
                jumps += 1;
                pw.add_link(&link);
                placed += 1;
                continue;
            }
            if uo_pdf::safe_pdf_uri(Some(&l.url)).is_none() {
                refused += 1;
                continue;
            }
            link.uri = Some(l.url.to_string());
            pw.add_link(&link);
            placed += 1;
        }
        // (b) the shape-level clicks and jumps over the shape's box as the page shows it
        let last = i64::from(links.page_count.max(1)) - 1;
        for (b, target) in &links.shapes {
            let mut link =
                uo_pdf::PdfLink { x_pt: b[0], y_top_pt: b[1], w_pt: b[2] - b[0], h_pt: b[3] - b[1], ..uo_pdf::PdfLink::default() };
            match target {
                ShapeDest::Url(u) => {
                    if uo_pdf::safe_pdf_uri(Some(u)).is_none() {
                        refused += 1;
                        continue;
                    }
                    link.uri = Some(u.to_string());
                }
                ShapeDest::Page(p) => link.page_index = Some(i64::from(*p)),
                ShapeDest::NotWritten => {
                    not_written += 1;
                    continue;
                }
                ShapeDest::Show(ShowJump::Next) => link.page_index = Some((page_index + 1).min(last)),
                ShapeDest::Show(ShowJump::Previous) => link.page_index = Some((page_index - 1).max(0)),
                ShapeDest::Show(ShowJump::First) => link.page_index = Some(0),
                ShapeDest::Show(ShowJump::Last) => link.page_index = Some(last),
                ShapeDest::Show(ShowJump::LastViewed | ShowJump::End) => {
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
        self.report.links_not_written += not_written;
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

/// A deck as a PDF: one page per slide at the slide's size (1 px = 1 pt, EMU / 12700), the text from `options.fonts`;
/// the slides marked hidden are left out unless [`PdfOptions::ts_hidden_slides`].
///
/// A slide that cannot be read or emitted becomes a blank page of the slide's size and is listed in
/// [`PdfReport::failed_slides`].
///
/// # Errors
/// [`PdfFail::Deck`] when the presentation itself cannot be read; [`PdfFail::Pdf`] when a limit or the sink stops
/// the document; [`PdfFail::NoVisibleSlides`] when every slide is hidden and hidden slides are left out.
pub fn pdf(deck: &mut Deck, options: &PdfOptions<'_>) -> Result<PdfOutput, PdfFail> {
    let (slide_w, slide_h, count) = {
        let p = deck.presentation().map_err(PdfFail::Deck)?;
        (p.slide_w, p.slide_h, p.slides.len())
    };
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    // the page of each slide (`None`: a hidden slide left out)
    let hidden = if options.ts_hidden_slides { vec![false; count as usize] } else { hidden_slides(deck, count) };
    let mut page_of: Vec<Option<u32>> = Vec::with_capacity(hidden.len());
    let mut pages = 0u32;
    for h in &hidden {
        page_of.push(if *h { None } else { Some(pages) });
        pages += u32::from(!*h);
    }
    if count > 0 && pages == 0 {
        return Err(PdfFail::NoVisibleSlides);
    }
    // the host's fonts under the deck's fallback rule (`SlidesRules::ts_fallback_fonts`)
    let policy = PdfFonts { inner: options.fonts, ts: deck.rules().ts_fallback_fonts };
    let opts = options.clone_settings(&policy);
    let mut w = Writer::new(&opts);
    let mut table = MeasureTable::default();
    for index in 0..count {
        if w.cancelled() {
            return Err(PdfFail::Pdf(PdfError::Cancelled));
        }
        if hidden.get(index as usize).copied().unwrap_or(false) {
            w.report.hidden_slides += 1;
            continue;
        }
        let mut res = SlidesResources::new();
        let mut caches = EmitCaches::new();
        let list = emit_with_measures(deck, index, &mut table, &mut res, &mut caches, &opts);
        match list {
            Some(emission) => {
                w.report.note_charts(emission.chart_frames.len());
                let pl = PageGeo { slide: index, page_of: &page_of, pages, pp: !opts.ts_link_rects };
                let links = page_links(deck, &emission.links, &pl);
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

/// Whether each slide is marked hidden (`p:sld show="0"` or `"false"`, the timing reader's rule).
fn hidden_slides(deck: &mut Deck, count: u32) -> Vec<bool> {
    let targets: Vec<Option<Box<str>>> =
        deck.presentation().map(|p| p.slides.iter().map(|r| r.target.clone()).collect()).unwrap_or_default();
    let src = deck.source();
    (0..count as usize)
        .map(|i| {
            let Some(Some(path)) = targets.get(i) else { return false };
            let Ok(Some(doc)) = src.xml(path) else { return false };
            matches!(doc.root().attr("show"), Some("0" | "false"))
        })
        .collect()
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
/// of the media parts the lists name. No deck: the fallback faces follow PowerPoint's rule.
///
/// # Errors
/// [`PdfFail::Pdf`] when a limit or the sink stops the document or a list is inconsistent.
pub fn pdf_from_lists(
    slides: &[(&SlidesList, &SlidesResources)],
    src: &dyn PartSource,
    options: &PdfOptions<'_>,
) -> Result<PdfOutput, PdfFail> {
    let policy = PdfFonts { inner: options.fonts, ts: false };
    let opts = options.clone_settings(&policy);
    let mut w = Writer::new(&opts);
    for (list, res) in slides {
        if w.cancelled() {
            return Err(PdfFail::Pdf(PdfError::Cancelled));
        }
        w.page(list, res, src, &PageLinks::none())?;
    }
    w.finish()
}

/// [`pdf_from_lists`] with each list's hyperlinked runs (`Emission::links`): one annotation per drawn piece (the TS
/// join) or per line and formatting run (PowerPoint's rule, without the text-range facts a deck gives: no link is
/// left out and no paragraph is taken as justified).
///
/// # Errors
/// As [`pdf_from_lists`].
pub fn pdf_from_lists_with_links(
    slides: &[(&SlidesList, &SlidesResources, &[RunLink])],
    src: &dyn PartSource,
    options: &PdfOptions<'_>,
) -> Result<PdfOutput, PdfFail> {
    let policy = PdfFonts { inner: options.fonts, ts: false };
    let opts = options.clone_settings(&policy);
    let mut w = Writer::new(&opts);
    let page_count = u32::try_from(slides.len()).unwrap_or(u32::MAX);
    for (list, res, runs) in slides {
        if w.cancelled() {
            return Err(PdfFail::Pdf(PdfError::Cancelled));
        }
        w.page(list, res, src, &PageLinks { runs, page_count, ..PageLinks::none() })?;
    }
    w.finish()
}

/// Joins the drawn pieces of one link that touch (the layout places a run word by word, a vertical or right-to-left line
/// piece by piece): the same shape and address, the boxes overlapping by half their extent across the line and at most
/// 0.6 of the line's thickness apart along it — in a row or in a column. The `ts_link_rects` rule.
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

/// One link rectangle of PowerPoint's rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LinkRect {
    /// the [`RunLink`] of its first piece
    pub link: usize,
    /// `x0, y0, x1, y1`, canvas space (y down), not cut to the page
    pub rect: [f64; 4],
    /// PowerPoint writes no annotation: the link's range holds its text's last paragraph mark
    pub dropped: bool,
}

/// What [`crate::links::run_ranges`] says about a drawn piece.
#[derive(Clone, Copy)]
struct PieceRange {
    range: Option<u32>,
    dropped: bool,
    justified: bool,
}

/// PowerPoint's link rectangles of a page (measured, wave 5, record `uo-pdf.md` "Link rectangles"): the drawn pieces of a
/// link joined while they are the same text range and the same formatting (face, size, fill) on one line (a column for
/// vertical text: touching as in [`merge_marks`]), never inside a justified or distributed paragraph (one rectangle per
/// word there); each rectangle is the union of the pieces' ink (their layout box when the ink was not measured, a piece
/// without ink adds nothing). The pieces are matched to the deck's runs by shape, address and character count.
pub(crate) fn link_rects(marks: &[links::Mark], runs: &[RunLink], list: &SlidesList, ranges: &[crate::links::RunRange]) -> Vec<LinkRect> {
    use uo_jsnum::math::{max, min};
    let union = |a: [f64; 4], b: [f64; 4]| [min(a[0], b[0]), min(a[1], b[1]), max(a[2], b[2]), max(a[3], b[3])];
    // the deck's linked runs of each (shape, address), in text order, with a cursor (entry, characters taken)
    let mut cursors: FxHashMap<(u32, &str), (Vec<&crate::links::RunRange>, usize, usize)> = FxHashMap::default();
    for r in ranges {
        if let Some(u) = r.url.as_deref() {
            cursors.entry((r.shape, u)).or_insert_with(|| (Vec::new(), 0, 0)).0.push(r);
        }
    }
    let piece_chars = |run: u32| -> usize {
        usize::try_from(run)
            .ok()
            .and_then(|i| list.runs.get(i))
            .and_then(|r| interp::span_of(&list.texts, &r.text))
            .map_or(0, |u| String::from_utf16_lossy(u).chars().filter(|c| !uo_jsnum::text::is_js_space(*c)).count())
    };
    struct Open {
        rect: LinkRect,
        layout: [f64; 4],
        ink: Option<[f64; 4]>,
        pr: PieceRange,
        vertical: bool,
        fmt: Option<(uo_render::slides::FontRef, uo_render::slides::Paint)>,
    }
    let mut out: Vec<LinkRect> = Vec::new();
    let mut open: Option<Open> = None;
    let close = |o: Open, out: &mut Vec<LinkRect>| {
        if let Some(ink) = o.ink {
            out.push(LinkRect { rect: ink, ..o.rect });
        }
    };
    for m in marks {
        let Some(l) = runs.get(m.link) else { continue };
        let n = piece_chars(m.run);
        let pr = match cursors.get_mut(&(l.shape, &*l.url)) {
            Some((entries, idx, taken)) if !entries.is_empty() => {
                while *idx + 1 < entries.len() && entries.get(*idx).is_some_and(|e| *taken >= e.chars) {
                    *idx += 1;
                    *taken = 0;
                }
                *taken += n;
                let e = entries.get(*idx).copied();
                PieceRange { range: e.map(|e| e.range), dropped: e.is_some_and(|e| e.dropped), justified: e.is_some_and(|e| e.justified) }
            }
            _ => PieceRange { range: None, dropped: false, justified: false },
        };
        let ink = if m.measured { m.ink } else { Some(m.layout) };
        if pr.justified && m.measured {
            // one rectangle per word (measured: also on the paragraph's last line, which the layout draws as one piece)
            if let Some(o) = open.take() {
                close(o, &mut out);
            }
            out.extend(m.words.iter().map(|w| LinkRect { link: m.link, rect: *w, dropped: pr.dropped }));
            continue;
        }
        let fmt = usize::try_from(m.run).ok().and_then(|i| list.runs.get(i)).map(|r| (r.font.clone(), r.fill.clone()));
        if let Some(o) = open.as_mut() {
            let p = &o.layout;
            let b = &m.layout;
            let same = runs.get(o.rect.link).is_some_and(|pl| pl.shape == l.shape && pl.url == l.url)
                && o.pr.range == pr.range
                && !pr.justified
                && o.vertical == m.vertical
                && o.fmt == fmt;
            let touches = if m.vertical {
                let (wp, wb) = (p[2] - p[0], b[2] - b[0]);
                min(p[2], b[2]) - max(p[0], b[0]) >= 0.5 * min(wp, wb) && max(b[1] - p[3], p[1] - b[3]) <= 0.6 * max(wp, wb)
            } else {
                let (hp, hb) = (p[3] - p[1], b[3] - b[1]);
                min(p[3], b[3]) - max(p[1], b[1]) >= 0.5 * min(hp, hb) && max(b[0] - p[2], p[0] - b[2]) <= 0.6 * max(hp, hb)
            };
            if same && touches {
                o.layout = union(o.layout, m.layout);
                o.ink = match (o.ink, ink) {
                    (Some(a), Some(b)) => Some(union(a, b)),
                    (a, b) => a.or(b),
                };
                continue;
            }
        }
        if let Some(o) = open.take() {
            close(o, &mut out);
        }
        open = Some(Open {
            rect: LinkRect { link: m.link, rect: m.layout, dropped: pr.dropped },
            layout: m.layout,
            ink,
            pr,
            vertical: m.vertical,
            fmt,
        });
    }
    if let Some(o) = open.take() {
        close(o, &mut out);
    }
    out
}

/// How far PowerPoint's rectangle of a shape click reaches left of and above the shape's box, in pt (measured, wave 5,
/// `sc` deck: 10, 20, 40, 60 pt text, three faces, default, zero and large insets, top and centred anchors): PowerPoint
/// writes an invisible text object at 0.6 em left of the text's left edge and about 1.24 em above its first baseline (the
/// line top minus 0.34 em), 2.1 em tall, and the link rectangle is the union of the shape's box and that object, so the
/// box grows by `max(0, 0.6 em − left inset)` to the left and, for top-anchored text, `max(0, 0.34 em − top inset)` upwards
/// (Aptos 0.30 em: the face is not known here, 0.34 is taken) and down to the object's bottom (60 pt text in an 80 pt box).
/// The em is the size of the shape's first run (18 pt without one); a shape without text, or a turned shape, keeps its
/// box. Returns `(left, up, the object's bottom below the grown top)`.
fn click_reach(sh: &uo_dml_render::model::Shape) -> (f64, f64, Option<f64>) {
    let Some(tb) = sh.tx_body.as_deref() else { return (0.0, 0.0, None) };
    let Some(run) = tb.paragraphs.iter().flat_map(|p| p.runs.iter()).find(|r| !r.text.is_empty()) else { return (0.0, 0.0, None) };
    let em = run.sz.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(18.0);
    let ins = |v: Option<f64>, d: f64| v.filter(|v| v.is_finite()).unwrap_or(d) * PX_PER_EMU;
    let dx = uo_jsnum::math::max(0.0, 0.6 * em - ins(tb.ins_l, 91_440.0));
    let top = tb.anchor.as_deref().is_none_or(|a| a == "t");
    if !top {
        return (dx, 0.0, None);
    }
    let reach = 0.34 * em - ins(tb.ins_t, 45_720.0);
    // the object's top relative to the box top is `-reach`; its bottom 2.1 em lower, measured from the grown top
    let dy = uo_jsnum::math::max(0.0, reach);
    (dx, dy, Some(2.1 * em - reach + dy))
}

/// Where a shape-level click or jump goes on the page.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum ShapeDest {
    Url(Box<str>),
    /// a page of this PDF
    Page(u32),
    /// a show jump resolved by page arithmetic at the painter (the `ts_link_rects` rule)
    Show(ShowJump),
    /// PowerPoint writes no link (a jump to a hidden slide left out, a link ending its text)
    NotWritten,
}

/// The links of one page: the emission's run marks, the model's link runs (for the unplaced count) and the shape-level
/// clicks and jumps in page pt (`x0, y0, x1, y1`).
pub(crate) struct PageLinks<'a> {
    pub runs: &'a [RunLink],
    /// `(shape, address)` of every model run with a `hyperlink` and some text
    pub model: Vec<(u32, String)>,
    pub shapes: Vec<([f64; 4], ShapeDest)>,
    /// the `RunUrl` boxes (turned with their shape): `(shape, box, address)`, placed for a shape that drew no linked piece
    pub fallback: Vec<(u32, [f64; 4], String)>,
    pub page_count: u32,
    /// the deck's linked runs with PowerPoint's ranges ([`crate::links::run_ranges`]; empty under `ts_link_rects`)
    pub ranges: Vec<crate::links::RunRange>,
    /// the run-level jumps of the emission (`RunLink::url` `ppaction://...`) resolved to their page
    pub run_dests: FxHashMap<Box<str>, ShapeDest>,
}

impl PageLinks<'_> {
    fn none() -> PageLinks<'static> {
        PageLinks {
            runs: &[],
            model: Vec::new(),
            shapes: Vec::new(),
            fallback: Vec::new(),
            page_count: 0,
            ranges: Vec::new(),
            run_dests: FxHashMap::default(),
        }
    }
}

/// Where a slide's page sits in the PDF.
struct PageGeo<'a> {
    slide: u32,
    /// the page of each slide (`None`: a hidden slide left out)
    page_of: &'a [Option<u32>],
    pages: u32,
    /// PowerPoint's link rule (`!ts_link_rects`)
    pp: bool,
}

impl PageGeo<'_> {
    /// The page a jump to slide `i` lands on, `NotWritten` when that slide was left out.
    fn slide(&self, i: u32) -> ShapeDest {
        match self.page_of.get(i as usize).copied().flatten() {
            Some(p) => ShapeDest::Page(p),
            None if self.page_of.is_empty() => ShapeDest::Page(i),
            None => ShapeDest::NotWritten,
        }
    }

    /// A show jump: by slide under PowerPoint's rule (next/previous of this slide, clamped to the deck, not skipping hidden
    /// slides: a hidden target writes no link, measured), by page arithmetic at the painter otherwise.
    fn show(&self, j: ShowJump) -> ShapeDest {
        if !self.pp {
            return ShapeDest::Show(j);
        }
        let last = u32::try_from(self.page_of.len()).unwrap_or(u32::MAX).max(1) - 1;
        match j {
            ShowJump::Next => self.slide((self.slide + 1).min(last)),
            ShowJump::Previous => self.slide(self.slide.saturating_sub(1)),
            ShowJump::First => self.slide(0),
            ShowJump::Last => self.slide(last),
            ShowJump::LastViewed | ShowJump::End => ShapeDest::Show(j),
        }
    }
}

/// The links of presentation slide `g.slide` for its page: the model's link runs, the `ShapeClick`/`RunJump` boxes of
/// [`crate::links::slide_links`] at the PDF view, turned with their shape (the `RunUrl` boxes are not placed: the run
/// marks are exact), their targets resolved to pages, and under PowerPoint's rule the deck's link ranges.
fn page_links<'a>(deck: &mut Deck, runs: &'a [RunLink], g: &PageGeo<'_>) -> PageLinks<'a> {
    let index = g.slide;
    let mut out = PageLinks { runs, page_count: g.pages, ..PageLinks::none() };
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
    if g.pp {
        out.ranges = crate::links::run_ranges(deck, index).unwrap_or_default();
    }
    // the run-level jumps the emission marked (the model keeps them as `ppaction://` addresses under PowerPoint's rules)
    let targets: Vec<Option<Box<str>>> =
        deck.presentation().map(|p| p.slides.iter().map(|r| r.target.clone()).collect()).unwrap_or_default();
    let mut jump_shapes: Vec<u32> = Vec::new();
    for l in runs {
        let Some(rest) = l.url.strip_prefix("ppaction://") else { continue };
        jump_shapes.push(l.shape);
        if out.run_dests.contains_key(&*l.url) {
            continue;
        }
        let dest = if let Some(part) = rest.strip_prefix("hlinksldjump#") {
            match targets.iter().position(|t| t.as_deref() == Some(part)).and_then(|i| u32::try_from(i).ok()) {
                Some(i) => g.slide(i),
                None => ShapeDest::NotWritten,
            }
        } else {
            let kind = rest.strip_prefix("hlinkshowjump?").and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("jump=")));
            match kind.map(str::to_ascii_lowercase).as_deref() {
                Some("nextslide") => g.show(ShowJump::Next),
                Some("previousslide") => g.show(ShowJump::Previous),
                Some("firstslide") => g.show(ShowJump::First),
                Some("lastslide") => g.show(ShowJump::Last),
                Some("lastslideviewed") => ShapeDest::Show(ShowJump::LastViewed),
                Some("endshow") => ShapeDest::Show(ShowJump::End),
                _ => ShapeDest::NotWritten,
            }
        };
        out.run_dests.insert(l.url.clone(), dest);
    }
    let view = View { scale: PX_PER_EMU, ox: 0.0, oy: 0.0 };
    for b in crate::links::slide_links(deck, index, view).unwrap_or_default() {
        let rot = usize::try_from(b.shape).ok().and_then(|i| slide.shapes.get(i)).and_then(|s| s.xfrm.as_ref()).and_then(|x| x.rot);
        let Some(mut r) = links::rotated_box(b.rect, rot) else { continue };
        if g.pp && b.source == LinkSource::ShapeClick && rot.is_none_or(|v| v == 0.0) {
            let sh = usize::try_from(b.shape).ok().and_then(|i| slide.shapes.get(i));
            let (dx, dy, bottom) = sh.map_or((0.0, 0.0, None), click_reach);
            r = [r[0] - dx, r[1] - dy, r[2], bottom.map_or(r[3], |b| uo_jsnum::math::max(r[3], r[1] - dy + b))];
        }
        // a run-level jump the emission marked is placed per run (above), not over the shape's box
        if b.source == LinkSource::RunJump && jump_shapes.contains(&b.shape) {
            continue;
        }
        // a run-level jump whose range ends its text writes no link (PowerPoint's rule)
        if g.pp
            && b.source == LinkSource::RunJump
            && out.ranges.iter().find(|x| x.shape == b.shape && x.jump.is_some()).is_some_and(|x| x.dropped)
        {
            out.shapes.push((r, ShapeDest::NotWritten));
            continue;
        }
        let dest = match (b.source, b.target) {
            (LinkSource::RunUrl, LinkTarget::Url(u)) => {
                out.fallback.push((b.shape, r, u.to_string()));
                continue;
            }
            (LinkSource::RunUrl, _) => continue,
            (_, LinkTarget::Url(u)) => ShapeDest::Url(u),
            (_, LinkTarget::Slide(i)) => g.slide(i),
            (_, LinkTarget::Show(j)) => g.show(j),
        };
        out.shapes.push((r, dest));
    }
    out
}
