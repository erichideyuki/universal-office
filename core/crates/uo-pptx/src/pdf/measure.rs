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

    /// A face with a glyph for `cp` when the run's face (`primary`) has none: by default the chain of the Write PDF
    /// ([`fallback_chain`]: the families Word falls back to, each through this source's own substitution). `None` =
    /// the character keeps the `.notdef` advance and is counted as `glyph:missing`.
    fn fallback(&self, cp: u32, bold: bool, italic: bool, primary: &Face) -> Option<Arc<Face>> {
        fallback_chain(self, cp, bold, italic, primary)
    }
}

/// `FALLBACK_FAMILIES` of the Write layout (para.ts:186).
const FALLBACK_FAMILIES: [&str; 7] =
    ["Segoe UI Symbol", "Segoe UI Emoji", "Arial Unicode MS", "MS Gothic", "Cambria Math", "Segoe UI", "Arial"];

/// `cjk(cp)` of the Write layout.
fn cjk(cp: u32) -> bool {
    (0x2e80..=0x9fff).contains(&cp)
        || (0xac00..=0xd7af).contains(&cp)
        || (0xf900..=0xfaff).contains(&cp)
        || (0xff00..=0xffef).contains(&cp)
        || (0x3000..=0x303f).contains(&cp)
        || (0x20000..=0x2ffff).contains(&cp)
}

/// `classFallback(font)` of the Write layout: Courier New for a monospaced PANOSE, Arial for a sans serif one, else
/// Times New Roman.
fn class_fallback(font: &Face) -> &'static str {
    if let Some(pn) = font.metrics().panose {
        if pn.first() == Some(&2) && pn.get(3) == Some(&9) {
            return "Courier New";
        }
        if pn.first() == Some(&2) && pn.get(1).is_some_and(|&x| (11..=15).contains(&x)) {
            return "Arial";
        }
    }
    "Times New Roman"
}

/// The families the Write layout tries for a character its face lacks (`fallbackOrder`, with Word's measured rules:
/// Thai to Leelawadee UI, CJK to MS Mincho, Hangul to Malgun Gothic), in order.
#[must_use]
pub fn fallback_families(cp: u32, primary: &Face) -> Vec<&'static str> {
    let mut out = Vec::with_capacity(11);
    if (0x0e00..=0x0e7f).contains(&cp) {
        out.push("Leelawadee UI");
    }
    if cjk(cp) {
        let hangul = (0xac00..=0xd7af).contains(&cp) || (0x1100..=0x11ff).contains(&cp) || (0x3130..=0x318f).contains(&cp);
        out.push(if hangul { "Malgun Gothic" } else { "MS Mincho" });
    }
    if (0x2190..=0x21ff).contains(&cp) || (0x25a0..=0x25ff).contains(&cp) {
        out.push(class_fallback(primary));
        out.push("Cambria Math");
    } else if (0x2200..=0x22ff).contains(&cp) || (0x2980..=0x29ff).contains(&cp) {
        out.push("Cambria Math");
    }
    out.extend(FALLBACK_FAMILIES);
    out
}

/// The default [`FontSource::fallback`]: the first face of [`fallback_families`] (as `src` answers each family, its
/// metric substitutes included) that has a glyph for `cp`.
pub fn fallback_chain<S: FontSource + ?Sized>(src: &S, cp: u32, bold: bool, italic: bool, primary: &Face) -> Option<Arc<Face>> {
    if cp <= 0x20 {
        return None;
    }
    for fam in fallback_families(cp, primary) {
        if let Some(f) = src.face(fam, bold, italic)
            && f.glyph_id(cp) != 0
        {
            return Some(f);
        }
    }
    None
}

impl<F: Fn(&str, bool, bool) -> Option<Arc<Face>>> FontSource for F {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        self(family, bold, italic)
    }
}

/// A set of faces keyed by family and variant with the substitution rule of the catalog: the exact family
/// (case-insensitive), else its metric-compatible substitute (`uo_fonts::metric_substitute`), else the fallback
/// family. A variant missing in a family answers the family's regular face.
///
/// A character the run's face lacks falls back through [`fallback_chain`], then to any face of the set that has it
/// (the same variant first, in the order the faces were inserted): "the open fonts available".
#[derive(Default)]
pub struct FaceSet {
    faces: FxHashMap<(String, bool, bool), Arc<Face>>,
    /// insertion order, for a deterministic scan
    order: Vec<(String, bool, bool)>,
    fallback: Option<String>,
    /// no per-character fallback at all (the capture's measurer of the B3 logs has none)
    no_glyph_fallback: bool,
}

impl FaceSet {
    /// An empty set whose fallback family is `fallback` (lower case, e.g. `liberation sans`).
    #[must_use]
    pub fn new(fallback: &str) -> FaceSet {
        FaceSet { fallback: Some(lower(fallback)), ..FaceSet::default() }
    }

    /// Adds the face of `family` in the given variant.
    pub fn insert(&mut self, family: &str, bold: bool, italic: bool, face: Arc<Face>) {
        let key = (lower(family), bold, italic);
        if !self.faces.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.faces.insert(key, face);
    }

    /// Turns the per-character fallback off (`false`) or on (the default).
    pub fn set_glyph_fallback(&mut self, on: bool) {
        self.no_glyph_fallback = !on;
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

    fn fallback(&self, cp: u32, bold: bool, italic: bool, primary: &Face) -> Option<Arc<Face>> {
        if self.no_glyph_fallback {
            return None;
        }
        if let Some(f) = fallback_chain(self, cp, bold, italic, primary) {
            return Some(f);
        }
        if cp <= 0x20 {
            return None;
        }
        let pick = |same: bool| {
            self.order
                .iter()
                .filter(|k| !same || (k.1 == bold && k.2 == italic))
                .filter_map(|k| self.faces.get(k))
                .find(|f| f.glyph_id(cp) != 0)
                .cloned()
        };
        pick(true).or_else(|| pick(false))
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

/// A per-character face chooser: the run's face where it has the glyph, else [`FontSource::fallback`] (memoized
/// per character). Shared by the measure filler, the PDF text and the raster text so the three agree.
pub(crate) struct FaceChooser<'a> {
    pub primary: Arc<Face>,
    fonts: &'a dyn FontSource,
    bold: bool,
    italic: bool,
    memo: FxHashMap<u32, Option<Arc<Face>>>,
}

impl<'a> FaceChooser<'a> {
    pub(crate) fn new(fonts: &'a dyn FontSource, primary: Arc<Face>, bold: bool, italic: bool) -> FaceChooser<'a> {
        FaceChooser { primary, fonts, bold, italic, memo: FxHashMap::default() }
    }

    /// The face that draws `cp`: `None` when no face has it (the primary's `.notdef` stands in).
    pub(crate) fn face_for(&mut self, cp: u32) -> Option<Arc<Face>> {
        if self.primary.glyph_id(cp) != 0 {
            return Some(Arc::clone(&self.primary));
        }
        if let Some(f) = self.memo.get(&cp) {
            return f.clone();
        }
        let f = self.fonts.fallback(cp, self.bold, self.italic, &self.primary);
        self.memo.insert(cp, f.clone());
        f
    }
}

/// One glyph placed along a run: the face that draws it (`None` = no face has the character: the primary's
/// `.notdef` advance, nothing drawn), its glyph id, the character, and the pen position (px from the run's start).
pub(crate) struct Placed {
    pub face: Option<Arc<Face>>,
    pub gid: u32,
    pub cp: u32,
    pub x: f64,
}

/// The glyphs of `text` placed with the faces of `ch` (advances, the pair kerning between consecutive glyphs of the
/// same face, the letter spacing after every character) and the total width. A run its primary face fully covers
/// gives exactly [`text_width`].
pub(crate) fn place_glyphs(ch: &mut FaceChooser<'_>, size_px: f64, kerning: bool, spacing_px: f64, text: &str) -> (Vec<Placed>, f64) {
    let mut out = Vec::with_capacity(text.len());
    let mut x = 0.0;
    let mut prev: Option<(u32, usize)> = None;
    for c in text.chars() {
        let cp = u32::from(c);
        let face = ch.face_for(cp);
        let (gid, adv_face) = match &face {
            Some(f) => (f.glyph_id(cp), Arc::clone(f)),
            None => (0, Arc::clone(&ch.primary)),
        };
        let id = Arc::as_ptr(&adv_face) as usize;
        let upem = f64::from(adv_face.metrics().units_per_em);
        if kerning
            && let Some((pg, pid)) = prev
            && pid == id
            && face.is_some()
        {
            x += f64::from(adv_face.kern_units(pg, gid)) * size_px / upem;
        }
        out.push(Placed { face: face.clone(), gid, cp, x });
        x += f64::from(adv_face.advance_width(gid)) * size_px / upem + spacing_px;
        prev = face.is_some().then_some((gid, id));
    }
    (out, x)
}

/// [`text_width`] with the per-character fallback of `fonts`: the same number when `face` has every character.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn text_width_in(
    fonts: &dyn FontSource,
    face: &Arc<Face>,
    bold: bool,
    italic: bool,
    size_px: f64,
    kerning: bool,
    spacing_px: f64,
    text: &str,
) -> f64 {
    if text.chars().all(|c| face.glyph_id(u32::from(c)) != 0) {
        return text_width(face, size_px, kerning, spacing_px, text);
    }
    let mut ch = FaceChooser::new(fonts, Arc::clone(face), bold, italic);
    if text.chars().all(|c| ch.face_for(u32::from(c)).is_none_or(|f| Arc::ptr_eq(&f, face))) {
        return text_width(face, size_px, kerning, spacing_px, text);
    }
    place_glyphs(&mut ch, size_px, kerning, spacing_px, text).1
}

/// The answers to `reqs`, written into `table`. Unknown faces answer 0-width text (nothing to measure with);
/// metafile probes (A8, A13, A14) are answered from the same faces (A14: never decodable here).
pub fn fill_measures(deck: &Deck, fonts: &dyn FontSource, table: &mut MeasureTable, reqs: &[MeasureReq]) {
    for r in reqs {
        match r {
            MeasureReq::CharAt1000 { face, ch } => {
                let w = face_of(fonts, face)
                    .map_or(0.0, |f| text_width_in(fonts, &f, face.bold, face.italic, 1000.0, false, 0.0, ch.encode_utf8(&mut [0; 4])));
                table.set_char_at_1000(face, *ch, w);
            }
            MeasureReq::KernAt1000 { face, text } => {
                let (w0, w1) = face_of(fonts, face).map_or((0.0, 0.0), |f| {
                    (
                        text_width_in(fonts, &f, face.bold, face.italic, 1000.0, false, 0.0, text),
                        text_width_in(fonts, &f, face.bold, face.italic, 1000.0, true, 0.0, text),
                    )
                });
                table.set_kern_at_1000(face, text, w0, w1);
            }
            MeasureReq::Width(k) => {
                let w = face_of(fonts, &k.face).map_or(0.0, |f| string_width(fonts, &f, k));
                table.set_width(k, w);
            }
            MeasureReq::Metrics(k) => {
                let m = face_of(fonts, &k.face).map_or_else(TextMetrics::default, |f| string_metrics(fonts, &f, k));
                table.set_metrics(k, m);
            }
            MeasureReq::FontAvailable { family } => table.set_font_available(family, fonts.face(family, false, false).is_some()),
            MeasureReq::ImageSize { media } => {
                let size = deck.source().media_entry(*media).and_then(|(_, b)| picture_size(&b)).unwrap_or((96.0, 96.0));
                table.set_image_size(*media, size.0, size.1);
            }
            MeasureReq::MetaText { font, text } => {
                let (family, bold, italic, px) = raw_css_font(font);
                let w = fonts.face(&family, bold, italic).map_or(0.0, |f| text_width_in(fonts, &f, bold, italic, px, true, 0.0, text));
                table.set_meta_text(font, text, w, 0.9 * px, 0.21 * px);
            }
            MeasureReq::MetaDecode { key, .. } => table.set_meta_decoded(key, None),
        }
    }
}

fn face_of(fonts: &dyn FontSource, key: &FaceKey) -> Option<Arc<Face>> {
    fonts.face(&key.family, key.bold, key.italic)
}

fn string_width(fonts: &dyn FontSource, f: &Arc<Face>, k: &StringKey) -> f64 {
    text_width_in(fonts, f, k.face.bold, k.face.italic, k.size_px.value(), k.kerning != Kerning::None, k.letter_spacing_px.value(), &k.text)
}

fn string_metrics(fonts: &dyn FontSource, f: &Arc<Face>, k: &StringKey) -> TextMetrics {
    let px = k.size_px.value();
    let upem = f64::from(f.metrics().units_per_em);
    let w = string_width(fonts, f, k);
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
