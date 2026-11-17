//! The measurement protocol of decision T1 (`uo-dml-render.md` "Measurement protocol"): every measurement the TS
//! asks the browser for (sites A1–A12) is an **input** of the layout and the emission, answered by the shell
//! (canvas `measureText`, decoded image sizes), by `uo-fonts` on the server/PDF path, or by a capture log in the
//! parity tests. A pass that misses an answer reports [`MeasureReq`]s and runs again once they are filled
//! (two-phase, as Write's jobs). The table is an input, not a cache of results: the TS clears its `advCache` per
//! paint (render.ts:224) and every value it memoizes is a pure function of the same answers.
//!
//! The two layout measurers of `render.ts` are here: [`Measurer::Ppt`] is `pptMeasure` (render.ts:2102–2150, the
//! per-character advances at 1000 px rounded to 1/8 pt per use, the whole-string kerning delta, and whole-string
//! widths for text outside the `PER_CHAR` scripts) and [`Measurer::Plain`] is the WordArt layout measurer of
//! `paintWarp` (render.ts:1889).

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ids::MediaId;

/// The face part of a CSS font: within one paint the shell's `fontStack(family, bold, italic)` is a fixed function
/// (render.ts:92), so this key is equivalent to the TS font string and the shell builds the identical string from
/// it (decision T2: the stack, fonts installed by name included, is the shell's).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct FaceKey {
    pub family: Box<str>,
    pub bold: bool,
    pub italic: bool,
}

/// The f64 bits of a size in px or a letter spacing in px (the request carries exactly the value the Rust side
/// computed, so the shell measures with the same number).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Bits(pub u64);

impl Bits {
    #[must_use]
    pub fn of(v: f64) -> Bits {
        Bits(v.to_bits())
    }

    #[must_use]
    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// The canvas `fontKerning` state of a measurement.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Kerning {
    /// The context's current state (the default `auto` at these sites).
    Current,
    None,
    Normal,
}

/// The capabilities the TS tests and that change results.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capabilities {
    /// `'fontKerning' in ctx` (A2 is skipped and A3's kerning is not set without it).
    pub font_kerning: bool,
    /// `letterSpacing` support (painter fit, warp).
    pub letter_spacing: bool,
    /// `Intl.Segmenter` (grapheme vs code-point breaking of a word longer than the line, text-layout.ts:217).
    pub segmenter: bool,
    /// `fontBoundingBox*` in `TextMetrics` (charts, warp fallback to the actual box).
    pub font_bounding_box: bool,
}

impl Default for Capabilities {
    /// A current browser: everything present.
    fn default() -> Self {
        Capabilities { font_kerning: true, letter_spacing: true, segmenter: true, font_bounding_box: true }
    }
}

/// The `TextMetrics` fields the TS reads (A7, A9, A11).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct TextMetrics {
    pub width: f64,
    pub actual_left: f64,
    pub actual_right: f64,
    pub actual_ascent: f64,
    pub actual_descent: f64,
    pub font_ascent: Option<f64>,
    pub font_descent: Option<f64>,
}

/// A whole-string measurement key (A3, A4, A5, A7, A9, A10, A11).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct StringKey {
    pub face: FaceKey,
    pub size_px: Bits,
    pub kerning: Kerning,
    pub letter_spacing_px: Bits,
    /// The measured string. A lone low surrogate the TS measures (the kerning seam after an astral character,
    /// text-layout.ts:504) is written as U+FFFD, which a canvas draws for it (declared decision B3-3).
    pub text: Box<str>,
}

/// One question for the shell.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum MeasureReq {
    /// A1: `measureText(ch).width` at 1000 px with kerning off (`fontKerning = 'none'`; without the `fontKerning`
    /// capability the context's current state); the table keeps the raw width (the `/ 1000` and the per-use
    /// `Math.round(em × size × 8) / 8` are done in Rust as in the TS).
    CharAt1000 { face: FaceKey, ch: char },
    /// A2: `(w0, w1)` of the whole string at 1000 px, kerning `none` then `normal`.
    KernAt1000 { face: FaceKey, text: Box<str> },
    /// A3, A4, A5, A10: `measureText(text).width`.
    Width(StringKey),
    /// A7, A9, A11: the full `TextMetrics`.
    Metrics(StringKey),
    /// A8: the metafile font probe (`fontAvail`, metafile.ts:694).
    FontAvailable { family: Box<str> },
    /// A12: the decoded natural size (EXIF-oriented; SVG as the browser sizes it).
    ImageSize { media: MediaId },
}

/// A pass needs answers it does not have: the shell measures them all and the pass runs again (rounds capped by
/// the caller, [`MAX_ROUNDS`]; past the cap the emit fails typed, never with a placeholder in the output).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct NeedMeasures(pub Vec<MeasureReq>);

/// The round cap of the two-phase protocol (round 1 asks every advance, round 2 the kerning and whole-string
/// widths of the true line breaks; later rounds only when kerning or complex-script widths move a break).
pub const MAX_ROUNDS: u32 = 8;

/// Default cap of a table (`uo-pptx.md` M5: 65,536 entries or 8 MiB, then cleared; the TS clears per paint, so no
/// value depends on it).
pub const DEFAULT_CAP_ENTRIES: usize = 65_536;
/// Default byte cap of a table.
pub const DEFAULT_CAP_BYTES: usize = 8 << 20;

/// The per-entry overhead counted against the byte cap (map slot, key head, value), on top of the key text.
const ENTRY_COST: usize = 48;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct WidthHead {
    face: u32,
    size_px: Bits,
    kerning: Kerning,
    letter_spacing_px: Bits,
}

/// The answers. Filled by the shell between the two phases, or by `uo-fonts` on the server, or from a capture log.
/// Lookups never allocate (faces are interned; strings are looked up by `&str`).
#[derive(Clone, Debug)]
pub struct MeasureTable {
    pub caps: Capabilities,
    generation: u32,
    /// family → face index per `bold * 2 + italic` (`u32::MAX` = not interned)
    faces: FxHashMap<Box<str>, [u32; 4]>,
    n_faces: u32,
    char_at_1000: FxHashMap<(u32, char), f64>,
    kern_at_1000: FxHashMap<u32, FxHashMap<Box<str>, (f64, f64)>>,
    width: FxHashMap<WidthHead, FxHashMap<Box<str>, f64>>,
    metrics: FxHashMap<WidthHead, FxHashMap<Box<str>, TextMetrics>>,
    font_available: FxHashMap<Box<str>, bool>,
    image_size: FxHashMap<MediaId, (f64, f64)>,
    entries: usize,
    bytes: usize,
    cap_entries: usize,
    cap_bytes: usize,
}

impl Default for MeasureTable {
    fn default() -> Self {
        MeasureTable::with_cap(DEFAULT_CAP_ENTRIES, DEFAULT_CAP_BYTES)
    }
}

const NO_FACE: u32 = u32::MAX;

fn style_ix(bold: bool, italic: bool) -> usize {
    usize::from(bold) * 2 + usize::from(italic)
}

impl MeasureTable {
    /// An empty table with the given caps (an insert past either cap clears the table first).
    #[must_use]
    pub fn with_cap(entries: usize, bytes: usize) -> MeasureTable {
        MeasureTable {
            caps: Capabilities::default(),
            generation: 0,
            faces: FxHashMap::default(),
            n_faces: 0,
            char_at_1000: FxHashMap::default(),
            kern_at_1000: FxHashMap::default(),
            width: FxHashMap::default(),
            metrics: FxHashMap::default(),
            font_available: FxHashMap::default(),
            image_size: FxHashMap::default(),
            entries: 0,
            bytes: 0,
            cap_entries: entries.max(1),
            cap_bytes: bytes,
        }
    }

    /// The shell's font generation.
    #[must_use]
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// A new font generation (a `FontFace` loaded): every answer may change, so the table empties.
    pub fn set_generation(&mut self, generation: u32) {
        if generation != self.generation {
            self.clear();
            self.generation = generation;
        }
    }

    /// Empties the answers (capabilities, caps and generation stay).
    pub fn clear(&mut self) {
        self.faces = FxHashMap::default();
        self.n_faces = 0;
        self.char_at_1000 = FxHashMap::default();
        self.kern_at_1000 = FxHashMap::default();
        self.width = FxHashMap::default();
        self.metrics = FxHashMap::default();
        self.font_available = FxHashMap::default();
        self.image_size = FxHashMap::default();
        self.entries = 0;
        self.bytes = 0;
    }

    /// Answers held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// The bytes counted against the byte cap.
    #[must_use]
    pub fn counted_bytes(&self) -> usize {
        self.bytes
    }

    fn face_of(&self, family: &str, bold: bool, italic: bool) -> Option<u32> {
        let ix = self.faces.get(family)?.get(style_ix(bold, italic)).copied()?;
        (ix != NO_FACE).then_some(ix)
    }

    /// Room for one more entry of `text_len` key bytes: past a cap the table is cleared first.
    fn reserve(&mut self, text_len: usize) {
        let cost = ENTRY_COST.saturating_add(text_len);
        if self.entries.saturating_add(1) > self.cap_entries || self.bytes.saturating_add(cost) > self.cap_bytes {
            self.clear();
        }
        self.entries = self.entries.saturating_add(1);
        self.bytes = self.bytes.saturating_add(cost);
    }

    fn intern(&mut self, face: &FaceKey) -> u32 {
        if let Some(ix) = self.face_of(&face.family, face.bold, face.italic) {
            return ix;
        }
        let ix = self.n_faces;
        self.n_faces = self.n_faces.saturating_add(1);
        let slot = self.faces.entry(face.family.clone()).or_insert([NO_FACE; 4]);
        if let Some(s) = slot.get_mut(style_ix(face.bold, face.italic)) {
            *s = ix;
        }
        ix
    }

    fn head(&mut self, k: &StringKey) -> WidthHead {
        WidthHead { face: self.intern(&k.face), size_px: k.size_px, kerning: k.kerning, letter_spacing_px: k.letter_spacing_px }
    }

    /// A1 answer.
    pub fn set_char_at_1000(&mut self, face: &FaceKey, ch: char, width: f64) {
        self.reserve(face.family.len());
        let f = self.intern(face);
        self.char_at_1000.insert((f, ch), width);
    }

    /// A2 answer.
    pub fn set_kern_at_1000(&mut self, face: &FaceKey, text: &str, w0: f64, w1: f64) {
        self.reserve(face.family.len().saturating_add(text.len()));
        let f = self.intern(face);
        self.kern_at_1000.entry(f).or_default().insert(text.into(), (w0, w1));
    }

    /// A3/A4/A5/A10 answer.
    pub fn set_width(&mut self, key: &StringKey, width: f64) {
        self.reserve(key.face.family.len().saturating_add(key.text.len()));
        let h = self.head(key);
        self.width.entry(h).or_default().insert(key.text.clone(), width);
    }

    /// A7/A9/A11 answer.
    pub fn set_metrics(&mut self, key: &StringKey, m: TextMetrics) {
        self.reserve(key.face.family.len().saturating_add(key.text.len()));
        let h = self.head(key);
        self.metrics.entry(h).or_default().insert(key.text.clone(), m);
    }

    /// A8 answer.
    pub fn set_font_available(&mut self, family: &str, available: bool) {
        self.reserve(family.len());
        self.font_available.insert(family.into(), available);
    }

    /// A12 answer.
    pub fn set_image_size(&mut self, media: MediaId, w: f64, h: f64) {
        self.reserve(0);
        self.image_size.insert(media, (w, h));
    }

    /// A1 lookup.
    #[must_use]
    pub fn char_at_1000(&self, family: &str, bold: bool, italic: bool, ch: char) -> Option<f64> {
        let f = self.face_of(family, bold, italic)?;
        self.char_at_1000.get(&(f, ch)).copied()
    }

    /// A2 lookup.
    #[must_use]
    pub fn kern_at_1000(&self, family: &str, bold: bool, italic: bool, text: &str) -> Option<(f64, f64)> {
        let f = self.face_of(family, bold, italic)?;
        self.kern_at_1000.get(&f)?.get(text).copied()
    }

    /// A3/A4/A5/A10 lookup.
    #[must_use]
    pub fn width(&self, face: (&str, bool, bool), size_px: f64, kerning: Kerning, letter_spacing_px: f64, text: &str) -> Option<f64> {
        let f = self.face_of(face.0, face.1, face.2)?;
        let h = WidthHead { face: f, size_px: Bits::of(size_px), kerning, letter_spacing_px: Bits::of(letter_spacing_px) };
        self.width.get(&h)?.get(text).copied()
    }

    /// A7/A9/A11 lookup.
    #[must_use]
    pub fn metrics(
        &self,
        face: (&str, bool, bool),
        size_px: f64,
        kerning: Kerning,
        letter_spacing_px: f64,
        text: &str,
    ) -> Option<TextMetrics> {
        let f = self.face_of(face.0, face.1, face.2)?;
        let h = WidthHead { face: f, size_px: Bits::of(size_px), kerning, letter_spacing_px: Bits::of(letter_spacing_px) };
        self.metrics.get(&h)?.get(text).copied()
    }

    /// A8 lookup.
    #[must_use]
    pub fn font_available(&self, family: &str) -> Option<bool> {
        self.font_available.get(family).copied()
    }

    /// A12 lookup.
    #[must_use]
    pub fn image_size(&self, media: MediaId) -> Option<(f64, f64)> {
        self.image_size.get(&media).copied()
    }
}

/// Which measurer of `render.ts` the layout uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Measurer {
    /// `pptMeasure` (render.ts:2102): text boxes, table cells.
    Ppt,
    /// The plain `measureText` of `paintWarp` (render.ts:1889): the WordArt layout.
    Plain,
}

/// The layout font of a measurement (`LayoutFont` fields the measurers read).
pub(crate) struct MFont<'a> {
    pub family: &'a str,
    pub size_pt: f64,
    pub bold: bool,
    pub italic: bool,
    pub kern: bool,
}

/// A1 and A2 at this size: `layoutFontStr({…f, sizePt: 1000}, 1)`.
const ADV_REF: f64 = 1000.0;

/// `PER_CHAR` (render.ts:2092) over one UTF-16 unit: the scripts whose characters have their own width. An astral
/// character is two surrogates, outside every range, so text with one is measured whole.
fn per_char_unit(u: u32) -> bool {
    matches!(u, 0x0000..=0x02FF | 0x0370..=0x03FF | 0x0400..=0x04FF | 0x1E00..=0x1FFF | 0x2000..=0x2BFF | 0x3000..=0x30FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// `PER_CHAR.test(text)`.
fn per_char(text: &str) -> bool {
    text.chars().all(|c| u32::from(c) <= 0xFFFF && per_char_unit(u32::from(c)))
}

/// The measurer of one layout pass: answers from the table; a missing answer is recorded and counts as 0 so the
/// pass can go on collecting the other misses (its result is discarded).
pub(crate) struct Meter<'t> {
    table: &'t MeasureTable,
    kind: Measurer,
    missing: FxHashSet<MeasureReq>,
    missing_order: Vec<MeasureReq>,
    has_char_miss: bool,
    /// every question asked, answered or not (parity harnesses)
    trace: Option<FxHashSet<MeasureReq>>,
}

impl<'t> Meter<'t> {
    pub(crate) fn new(table: &'t MeasureTable, kind: Measurer, trace: bool) -> Meter<'t> {
        Meter {
            table,
            kind,
            missing: FxHashSet::default(),
            missing_order: Vec::new(),
            has_char_miss: false,
            trace: trace.then(FxHashSet::default),
        }
    }

    pub(crate) fn caps(&self) -> Capabilities {
        self.table.caps
    }

    fn face(f: &MFont<'_>) -> FaceKey {
        FaceKey { family: f.family.into(), bold: f.bold, italic: f.italic }
    }

    fn ask(&mut self, req: impl FnOnce() -> MeasureReq, hit: bool) {
        if hit && self.trace.is_none() {
            return;
        }
        let r = req();
        if let Some(t) = &mut self.trace {
            t.insert(r.clone());
        }
        if !hit && self.missing.insert(r.clone()) {
            if matches!(r, MeasureReq::CharAt1000 { .. }) {
                self.has_char_miss = true;
            }
            self.missing_order.push(r);
        }
    }

    fn width_req(&mut self, f: &MFont<'_>, size_px: f64, kerning: Kerning, text: &str) -> f64 {
        let got = self.table.width((f.family, f.bold, f.italic), size_px, kerning, 0.0, text);
        self.ask(
            || {
                MeasureReq::Width(StringKey {
                    face: Self::face(f),
                    size_px: Bits::of(size_px),
                    kerning,
                    letter_spacing_px: Bits::of(0.0),
                    text: text.into(),
                })
            },
            got.is_some(),
        );
        got.unwrap_or(0.0)
    }

    /// The layout's `measure(text, font)` in points.
    pub(crate) fn measure(&mut self, text: &str, f: &MFont<'_>) -> f64 {
        match self.kind {
            Measurer::Plain => {
                // `ctx.font = layoutFontStr(f, 1); ctx.measureText(text).width` with the current kerning
                self.width_req(f, f.size_pt * 1.0, Kerning::Current, text)
            }
            Measurer::Ppt => self.ppt(text, f),
        }
    }

    /// `pptMeasure` (render.ts:2102–2150). The TS memo of whole strings returns what the loop computes again
    /// (same answers), so it is not reproduced.
    fn ppt(&mut self, text: &str, f: &MFont<'_>) -> f64 {
        let fk = self.table.caps.font_kerning;
        if !per_char(text) {
            let kerning = if !fk {
                Kerning::Current
            } else if f.kern {
                Kerning::Normal
            } else {
                Kerning::None
            };
            return self.width_req(f, f.size_pt * 1.0, kerning, text);
        }
        let mut sum = 0.0;
        for ch in text.chars() {
            let got = self.table.char_at_1000(f.family, f.bold, f.italic, ch);
            self.ask(|| MeasureReq::CharAt1000 { face: Self::face(f), ch }, got.is_some());
            let em = got.unwrap_or(0.0) / ADV_REF;
            sum += uo_jsnum::math::round(em * f.size_pt * 8.0) / 8.0;
        }
        // kerning: what the pairs take or add over the whole string (not rounded, measured)
        // `text.length > 1` in UTF-16 units: PER_CHAR text is BMP only, so units = chars
        if f.kern && text.chars().nth(1).is_some() && fk {
            let got = self.table.kern_at_1000(f.family, f.bold, f.italic, text);
            self.ask(|| MeasureReq::KernAt1000 { face: Self::face(f), text: text.into() }, got.is_some());
            let (w0, w1) = got.unwrap_or((0.0, 0.0));
            sum += ((w1 - w0) / ADV_REF) * f.size_pt;
        }
        sum
    }

    /// The questions left unanswered: only the advances while any is missing (they decide the line breaks that
    /// decide the kerning and whole-string questions), else everything missing, in the order first asked.
    pub(crate) fn into_missing(self) -> (Vec<MeasureReq>, Option<FxHashSet<MeasureReq>>) {
        let Meter { missing_order, has_char_miss, trace, .. } = self;
        let missing = if has_char_miss {
            missing_order.into_iter().filter(|r| matches!(r, MeasureReq::CharAt1000 { .. })).collect()
        } else {
            missing_order
        };
        (missing, trace)
    }
}
