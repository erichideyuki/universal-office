//! The `uo-fonts`-backed filler of the Slides [`MeasureTable`] (the server/PDF path of decision T1, R-S5): the
//! measurements the layout and the emission ask of the shell (`MeasureReq`: A1, A2, A3 ... A14) answered from the
//! advance widths and the kerning pairs of the font files, `units * px / upem`, as the capture's measurer
//! (`js/capture-textlayout.mjs` `fakeWidth`) does over the open fonts: the logs of the B3 captures can therefore be
//! replayed against this filler (`tests/pdf_measure.rs`).
//!
//! The faces come from a [`FontSource`], the host's font policy (T2 on the server: the family asked for, else its
//! metric-compatible substitute, else Liberation Sans). Proprietary fonts are the host's decision: this module only
//! reads what the source hands over and never looks at the file system.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_dml_render::PartSource;
use uo_dml_render::text::{FaceKey, Kerning, MeasureReq, MeasureTable, StringKey, TextMetrics};
use uo_fonts::Face;
use uo_jsnum::text::{split_js_space, trim};

use crate::deck::Deck;

/// Where the faces come from: the family and variant the document asks for, answered with a face (the policy
/// of substitution is the source's).
pub trait FontSource {
    /// The face to draw and measure `family` (as the document spells it) in the given variant with; `None` = no
    /// font at all (the text is not drawn and measures as nothing).
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>>;
}

impl<F: Fn(&str, bool, bool) -> Option<Arc<Face>>> FontSource for F {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        self(family, bold, italic)
    }
}

/// A set of faces keyed by family and variant with the substitution rule of the catalog: the exact family
/// (case-insensitive), else its metric-compatible substitute (`uo_fonts::metric_substitute`), else the fallback
/// family. A variant missing in a family answers the family's regular face.
#[derive(Default)]
pub struct FaceSet {
    faces: FxHashMap<(String, bool, bool), Arc<Face>>,
    fallback: Option<String>,
}

impl FaceSet {
    /// An empty set whose fallback family is `fallback` (lower case, e.g. `liberation sans`).
    #[must_use]
    pub fn new(fallback: &str) -> FaceSet {
        FaceSet { faces: FxHashMap::default(), fallback: Some(lower(fallback)) }
    }

    /// Adds the face of `family` in the given variant.
    pub fn insert(&mut self, family: &str, bold: bool, italic: bool, face: Arc<Face>) {
        self.faces.insert((lower(family), bold, italic), face);
    }

    fn exact(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        let key = lower(family);
        self.faces
            .get(&(key.clone(), bold, italic))
            .or_else(|| self.faces.get(&(key.clone(), bold, false)))
            .or_else(|| self.faces.get(&(key, false, false)))
            .cloned()
    }
}

fn lower(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

impl FontSource for FaceSet {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        if let Some(f) = self.exact(family, bold, italic) {
            return Some(f);
        }
        if let Some(sub) = uo_fonts::metric_substitute(&lower(family))
            && let Some(f) = self.exact(sub, bold, italic)
        {
            return Some(f);
        }
        self.fallback.as_deref().and_then(|fb| self.exact(fb, bold, italic))
    }
}

/// `measureText(text).width` of the capture's measurer: the advances of the glyphs (a lone surrogate counts as
/// U+FFFD) plus, unless `kerning` is off, the pair kerning between consecutive glyphs, in px at `size_px`; the
/// letter spacing adds `spacing_px` per character, as the canvas does.
#[must_use]
pub fn text_width(face: &Face, size_px: f64, kerning: bool, spacing_px: f64, text: &str) -> f64 {
    let mut units = 0.0f64;
    let mut prev: Option<u32> = None;
    let mut n = 0u32;
    for ch in text.chars() {
        let g = face.glyph_id(u32::from(ch));
        units += f64::from(face.advance_width(g));
        if kerning && let Some(p) = prev {
            units += f64::from(face.kern_units(p, g));
        }
        prev = Some(g);
        n += 1;
    }
    let upem = f64::from(face.metrics().units_per_em);
    let base = units * size_px / upem;
    if spacing_px == 0.0 { base } else { base + spacing_px * f64::from(n) }
}

/// The answers to `reqs`, written into `table`. Unknown faces answer 0-width text (nothing to measure with);
/// metafile probes (A8, A13, A14) are answered from the same faces (A14: never decodable here).
pub fn fill_measures(deck: &Deck, fonts: &dyn FontSource, table: &mut MeasureTable, reqs: &[MeasureReq]) {
    for r in reqs {
        match r {
            MeasureReq::CharAt1000 { face, ch } => {
                let w = face_of(fonts, face).map_or(0.0, |f| text_width(&f, 1000.0, false, 0.0, ch.encode_utf8(&mut [0; 4])));
                table.set_char_at_1000(face, *ch, w);
            }
            MeasureReq::KernAt1000 { face, text } => {
                let (w0, w1) = face_of(fonts, face)
                    .map_or((0.0, 0.0), |f| (text_width(&f, 1000.0, false, 0.0, text), text_width(&f, 1000.0, true, 0.0, text)));
                table.set_kern_at_1000(face, text, w0, w1);
            }
            MeasureReq::Width(k) => {
                let w = face_of(fonts, &k.face).map_or(0.0, |f| string_width(&f, k));
                table.set_width(k, w);
            }
            MeasureReq::Metrics(k) => {
                let m = face_of(fonts, &k.face).map_or_else(TextMetrics::default, |f| string_metrics(&f, k));
                table.set_metrics(k, m);
            }
            MeasureReq::FontAvailable { family } => table.set_font_available(family, fonts.face(family, false, false).is_some()),
            MeasureReq::ImageSize { media } => {
                let size = deck.source().media_entry(*media).and_then(|(_, b)| picture_size(&b)).unwrap_or((96.0, 96.0));
                table.set_image_size(*media, size.0, size.1);
            }
            MeasureReq::MetaText { font, text } => {
                let (family, bold, italic, px) = raw_css_font(font);
                let w = fonts.face(&family, bold, italic).map_or(0.0, |f| text_width(&f, px, true, 0.0, text));
                table.set_meta_text(font, text, w, 0.9 * px, 0.21 * px);
            }
            MeasureReq::MetaDecode { key, .. } => table.set_meta_decoded(key, None),
        }
    }
}

fn face_of(fonts: &dyn FontSource, key: &FaceKey) -> Option<Arc<Face>> {
    fonts.face(&key.family, key.bold, key.italic)
}

fn string_width(f: &Face, k: &StringKey) -> f64 {
    text_width(f, k.size_px.value(), k.kerning != Kerning::None, k.letter_spacing_px.value(), &k.text)
}

fn string_metrics(f: &Face, k: &StringKey) -> TextMetrics {
    let px = k.size_px.value();
    let upem = f64::from(f.metrics().units_per_em);
    let w = string_width(f, k);
    let asc = f64::from(f.metrics().ascender) * px / upem;
    let desc = -f64::from(f.metrics().descender) * px / upem;
    TextMetrics {
        width: w,
        actual_left: 0.0,
        actual_right: w,
        actual_ascent: asc,
        actual_descent: desc,
        font_ascent: Some(asc),
        font_descent: Some(desc),
    }
}

/// The natural size of a picture from its header (PNG, GIF, BMP, TIFF, JPEG); `None` when it is none of them.
fn picture_size(b: &[u8]) -> Option<(f64, f64)> {
    if let Some(j) = uo_image::jpeg_info(b) {
        return Some((f64::from(j.width), f64::from(j.height)));
    }
    let s = uo_image::image_pixel_size(b)?;
    Some((f64::from(s.width), f64::from(s.height)))
}

/// `[italic ][bold ]<n>px "<family>"...` of a raw CSS font as the metafile players write it: the first family,
/// the weight and the size (12 px when none).
pub(super) fn raw_css_font(font: &str) -> (String, bool, bool, f64) {
    let italic = split_js_space(font).any(|t| t == "italic" || t == "oblique");
    let bold = split_js_space(font).any(|t| t == "bold" || matches!(t, "600" | "700" | "800" | "900"));
    let mut px = 12.0;
    let mut family = String::new();
    if let Some(i) = font.find("px") {
        let head = font.get(..i).unwrap_or("");
        let start = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map_or(0, |p| p + 1);
        let n = uo_jsnum::parse::parse_float(head.get(start..).unwrap_or(""));
        px = if n.is_nan() { 12.0 } else { n };
        let rest = trim(font.get(i + 2..).unwrap_or(""));
        let first = trim(rest.split(',').next().unwrap_or(""));
        family = first.trim_matches(|c| c == '"' || c == '\'').to_owned();
    }
    (family, bold, italic, px)
}
