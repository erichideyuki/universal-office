//! The Slides display list (plan §4.2, record `core/docs/modules/uo-render.md`, "Slides extensions"): the TS
//! canvas call sequence of one slide at one (scale, origin), its resources and its binary codec. A separate
//! schema from Write's page (`core/schema/slides.uos`, magic `UORS`; the animation values of a frame are
//! `core/schema/slots.uos`, magic `UORF`): nothing here touches `render.uos` or its bytes.
//!
//! - **Types** are the generated wire types ([`SlidesList`], [`SItem`], [`Paint`], [`TextRun`], [`Effect`], ...):
//!   a Slides item *is* a canvas call, so there is no second in-memory shape to convert to (Write needs one for its
//!   side columns and 40-byte items; here the producers fill the wire types directly). Ids of the persistent tables
//!   (`sym`, `faces`, `media`, `derived`, `bitmaps`) are plain positions (`u32`); [`SlidesResources`] hands them out.
//! - [`SlidesResources`]: the persistent tables of one slideshow generation with interning and de-duplication,
//!   plus the bytes behind the generated bitmaps (pulled by handle, never encoded).
//! - [`SlidesEncoder`] / [`SlidesDecoder`]: the stream of messages, with the table delta mark per generation.
//! - [`validate`]: what the generated decoder cannot check (arena spans, arena indices, layer and register
//!   bounds, and, on the encode side, the table ids); both ends call it, so a list that leaves the encoder
//!   decodes and a decoded list is safe to interpret without per-item bounds panics.
//! - [`SlotFrame`]: the per-frame animation values, positional over `SlidesList.slots` ([`check_frame`]).

use std::fmt;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_codec::{DecodeError, EncodeError, Limits};

use crate::codec::slides_generated as wire;
use crate::codec::slots_generated as slots;
use crate::sym::SymbolTable;

pub use crate::codec::slides_generated::{
    Align, ArcOp, ArcToOp, Baseline, BevelEffect, Bitmap, Cap, CellTextItem, ClrChange, CompositeOp, CtmAxis, CtmOperand, Cubic,
    Decoration, Derived, DrawImageItem, Duotone, Edges, Effect, EllipseOp, FaceKey, FillRule, FillTextItem, FontRef, GlyphRunItem,
    Gradient, ImageFx, ImageSrc, Join, LayerBeginItem, LayerEndItem, LayerThen, MAGIC, Mat6, Media, MediaFrameItem, MetafileRopEffect,
    Paint, Pair, PathOp, PatternRes, PatternSrc, PxSize, Quad, RawStop, Rect4, Repeat, RoundRectOp, SCHEMA_HASH, SItem, ShadowOff,
    SheetFontItem, SlidesList, SlotDef, SlotKind, Span, Stop, TextOutline, TextRun, TextShadow, TileDef, TileParams, ViewBox, WarpEffect,
    WarpFont, WarpLine, WarpPaint, WarpPaintKind, WarpPattern, WarpRun, WarpStroke, WarpTile, decode_slides_list, encode_slides_list,
};
pub use crate::codec::slots_generated::{
    AnimFilter, PartState, Replacement, ShapeState, SlotFrame, SlotValue, decode_slot_frame, encode_slot_frame,
};

/// The `K` of the off-screen shadow idiom (`shadowOffsetX = sx + m.a * K`, then `translate(-K, 0)`).
pub const SHADOW_OFFSET_K: f64 = 20000.0;
/// Captured-transform registers per list (`CaptureCtm(r)`, `r < 4`).
pub const CTM_REGISTERS: u32 = 4;
/// Layer ids per list (`LayerBegin { layer }`, `layer < 16`): an id indexes the painter's layer stack.
pub const MAX_LAYERS: u32 = 16;

/// Why a list or a message was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlidesError {
    /// the codec refused the message (decode)
    Decode(DecodeError),
    /// the codec could not write the list (a count past 32 bits)
    Encode(EncodeError),
    /// the list is inconsistent: the named field points outside its arena or table, or breaks a bound
    Invalid(&'static str),
}

impl fmt::Display for SlidesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SlidesError::Decode(e) => write!(f, "{e}"),
            SlidesError::Encode(e) => write!(f, "{e}"),
            SlidesError::Invalid(what) => write!(f, "invalid Slides list: {what}"),
        }
    }
}

impl std::error::Error for SlidesError {}

impl From<DecodeError> for SlidesError {
    fn from(e: DecodeError) -> SlidesError {
        SlidesError::Decode(e)
    }
}

impl From<EncodeError> for SlidesError {
    fn from(e: EncodeError) -> SlidesError {
        SlidesError::Encode(e)
    }
}

/// The persistent resources of one slideshow generation: symbols (CSS colours, names), faces, media parts, derived
/// images and generated bitmaps, each a table whose ids are positions, in first-use order. A new generation
/// (`SlidesEncoder::new(g + 1)` with a fresh `SlidesResources`) makes the decoder drop its tables.
#[derive(Debug, Default)]
pub struct SlidesResources {
    symbols: SymbolTable,
    faces: Vec<FaceKey>,
    face_index: FxHashMap<(u32, bool, bool), u32>,
    media: Vec<Media>,
    media_index: FxHashMap<u32, u32>,
    derived: Vec<Derived>,
    bitmaps: Vec<Bitmap>,
    bitmap_data: Vec<Arc<[u8]>>,
    /// per bitmap: the bytes are a metafile recipe (`uo_dml_render::metafile`), not RGBA
    bitmap_recipe: Vec<bool>,
}

fn id32(n: usize) -> Option<u32> {
    u32::try_from(n).ok()
}

impl SlidesResources {
    #[must_use]
    pub fn new() -> SlidesResources {
        SlidesResources::default()
    }

    /// The symbol id of `units` (a CSS string as the TS built it), interned on first use.
    pub fn sym(&mut self, units: &[u16]) -> Option<u32> {
        self.symbols.intern(units).and_then(|s| id32(s.index()))
    }

    /// [`SlidesResources::sym`] of a Rust string.
    pub fn sym_str(&mut self, s: &str) -> Option<u32> {
        self.symbols.intern_str(s).and_then(|s| id32(s.index()))
    }

    /// The face `(family, bold, italic)`, added on first use.
    pub fn face(&mut self, family: u32, bold: bool, italic: bool) -> Option<u32> {
        if let Some(&id) = self.face_index.get(&(family, bold, italic)) {
            return Some(id);
        }
        if usize::try_from(family).ok()? >= self.symbols.len() {
            return None;
        }
        let id = id32(self.faces.len())?;
        self.faces.push(FaceKey { family, bold, italic });
        self.face_index.insert((family, bold, italic), id);
        Some(id)
    }

    /// The media entry of package part `part` (a symbol), added on first use; a second call with the same part
    /// answers the first entry (the other fields are the first call's).
    pub fn media(&mut self, entry: Media) -> Option<u32> {
        if let Some(&id) = self.media_index.get(&entry.part) {
            return Some(id);
        }
        if usize::try_from(entry.part).ok()? >= self.symbols.len() || usize::try_from(entry.ext).ok()? >= self.symbols.len() {
            return None;
        }
        let id = id32(self.media.len())?;
        self.media_index.insert(entry.part, id);
        self.media.push(entry);
        Some(id)
    }

    /// A derived image (the colour effects of `fx` over media entry `image`); the shell caches by the entry.
    pub fn derived(&mut self, image: u32, fx: ImageFx) -> Option<u32> {
        if usize::try_from(image).ok()? >= self.media.len() {
            return None;
        }
        if let Some(i) = self.derived.iter().position(|d| d.image == image && d.fx == fx) {
            return id32(i);
        }
        let id = id32(self.derived.len())?;
        self.derived.push(Derived { image, fx });
        Some(id)
    }

    /// A generated bitmap: RGBA 8-bit, `w * h * 4` bytes (`None` otherwise). The bytes stay here (pulled by handle).
    pub fn bitmap(&mut self, w: u32, h: u32, rgba: Arc<[u8]>) -> Option<u32> {
        let want = usize::try_from(w).ok()?.checked_mul(usize::try_from(h).ok()?)?.checked_mul(4)?;
        if rgba.len() != want {
            return None;
        }
        let id = id32(self.bitmaps.len())?;
        self.bitmaps.push(Bitmap { w, h, data: id });
        self.bitmap_data.push(rgba);
        self.bitmap_recipe.push(false);
        Some(id)
    }

    /// A picture the shell paints by replaying a recipe (a metafile or PICT, B5b): the bitmap `w` x `h` is the canvas the recipe
    /// makes; the bytes are the recipe, not RGBA (see [`SlidesResources::is_recipe`]).
    pub fn recipe_bitmap(&mut self, w: u32, h: u32, recipe: Arc<[u8]>) -> Option<u32> {
        let id = id32(self.bitmaps.len())?;
        self.bitmaps.push(Bitmap { w, h, data: id });
        self.bitmap_data.push(recipe);
        self.bitmap_recipe.push(true);
        Some(id)
    }

    /// Whether the bytes of bitmap handle `handle` are a recipe.
    #[must_use]
    pub fn is_recipe(&self, handle: u32) -> bool {
        usize::try_from(handle).ok().and_then(|i| self.bitmap_recipe.get(i)).copied().unwrap_or(false)
    }

    /// The bytes of a bitmap handle (`Bitmap.data`).
    #[must_use]
    pub fn bitmap_bytes(&self, handle: u32) -> Option<&Arc<[u8]>> {
        self.bitmap_data.get(usize::try_from(handle).ok()?)
    }

    /// The face table (read by the PDF interpreter, R-S5; the shell gets it by the delta).
    #[must_use]
    pub fn face_entries(&self) -> &[FaceKey] {
        &self.faces
    }

    /// The media table (R-S5: the PDF interpreter loads the part bytes by `Media.part`).
    #[must_use]
    pub fn media_entries(&self) -> &[Media] {
        &self.media
    }

    /// The derived-image table (R-S5).
    #[must_use]
    pub fn derived_entries(&self) -> &[Derived] {
        &self.derived
    }

    /// The generated-bitmap table (R-S5; the bytes are [`SlidesResources::bitmap_bytes`]).
    #[must_use]
    pub fn bitmap_entries(&self) -> &[Bitmap] {
        &self.bitmaps
    }

    /// The entries of every table: `sym, faces, media, derived, bitmaps` (the bounds of the ids of a list).
    #[must_use]
    pub fn lens(&self) -> [usize; 5] {
        [self.symbols.len(), self.faces.len(), self.media.len(), self.derived.len(), self.bitmaps.len()]
    }

    /// The code units of symbol `id`.
    #[must_use]
    pub fn sym_units(&self, id: u32) -> &[u16] {
        usize::try_from(id).ok().and_then(crate::ids::Sym::from_index).map_or(&[], |s| self.symbols.get(s))
    }

    /// The entries past `sent` as the delta of generation `generation`.
    fn delta(&self, generation: u32, sent: &[usize; 5]) -> wire::Delta {
        let [s0, s1, s2, s3, s4] = *sent;
        let sym = (s0..self.symbols.len())
            .map(|i| crate::ids::Sym::from_index(i).map_or_else(Vec::new, |s| self.symbols.get(s).to_vec()))
            .collect();
        wire::Delta {
            generation,
            sym: uo_codec::TableDelta { from: id32(s0).unwrap_or(0), items: sym },
            faces: uo_codec::TableDelta { from: id32(s1).unwrap_or(0), items: self.faces.get(s1..).unwrap_or(&[]).to_vec() },
            media: uo_codec::TableDelta { from: id32(s2).unwrap_or(0), items: self.media.get(s2..).unwrap_or(&[]).to_vec() },
            derived: uo_codec::TableDelta { from: id32(s3).unwrap_or(0), items: self.derived.get(s3..).unwrap_or(&[]).to_vec() },
            bitmaps: uo_codec::TableDelta { from: id32(s4).unwrap_or(0), items: self.bitmaps.get(s4..).unwrap_or(&[]).to_vec() },
        }
    }
}

/// The producer's side of a stream of `SlidesList` messages: the generation and how many entries of each table the
/// decoder already has. One per (view, generation); a resize that re-emits the list keeps the mark, a new slideshow
/// starts a new generation.
#[derive(Debug, Clone)]
pub struct SlidesEncoder {
    generation: u32,
    sent: [usize; 5],
}

impl SlidesEncoder {
    #[must_use]
    pub fn new(generation: u32) -> SlidesEncoder {
        SlidesEncoder { generation, sent: [0; 5] }
    }

    #[must_use]
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Entries of each table sent so far.
    #[must_use]
    pub fn sent(&self) -> [usize; 5] {
        self.sent
    }

    /// Appends the message of `list` (with the table entries the decoder has not seen) to `out` and advances the
    /// mark. On an error `out` and the mark are as they were.
    ///
    /// # Errors
    /// [`SlidesError::Invalid`] when the list points outside its arenas or the tables of `res`;
    /// [`SlidesError::Encode`] for a count past 32 bits.
    pub fn encode(&mut self, list: &SlidesList, res: &SlidesResources, out: &mut Vec<u8>) -> Result<(), SlidesError> {
        let lens = res.lens();
        validate(list, &lens)?;
        let delta = res.delta(self.generation, &self.sent);
        encode_slides_list(out, &delta, list)?;
        self.sent = lens;
        Ok(())
    }
}

/// The shell's side: the tables of the stream and the decode limits.
#[derive(Debug, Clone, Default)]
pub struct SlidesDecoder {
    tables: wire::Tables,
    limits: Limits,
}

impl SlidesDecoder {
    #[must_use]
    pub fn new() -> SlidesDecoder {
        SlidesDecoder::default()
    }

    #[must_use]
    pub fn with_limits(limits: Limits) -> SlidesDecoder {
        SlidesDecoder { tables: wire::Tables::new(), limits }
    }

    /// The tables received so far (faces, media, derived, bitmaps, symbols).
    #[must_use]
    pub fn tables(&self) -> &wire::Tables {
        &self.tables
    }

    /// Reads one message: the new table entries are appended (a new generation replaces them), the list is
    /// [`validate`]d. On an error the tables are as they were.
    ///
    /// # Errors
    /// [`SlidesError::Decode`] for a malformed message, [`SlidesError::Invalid`] for an inconsistent list.
    pub fn decode(&mut self, buf: &[u8]) -> Result<SlidesList, SlidesError> {
        let head = wire::peek_header(buf)?;
        let saved = if self.tables.generation == Some(head.generation) { None } else { Some(std::mem::take(&mut self.tables)) };
        let lens = self.tables.lens();
        let result = decode_slides_list(buf, &mut self.tables, &self.limits)
            .map_err(SlidesError::from)
            .and_then(|list| validate(&list, &self.tables.lens()).map(|()| list));
        if result.is_err() {
            match saved {
                Some(old) => self.tables = old,
                None => self.tables.truncate(&lens),
            }
        }
        result
    }
}

struct Checker<'a> {
    list: &'a SlidesList,
    lens: &'a [usize; 5],
}

type Check = Result<(), SlidesError>;

fn bad(what: &'static str) -> Check {
    Err(SlidesError::Invalid(what))
}

fn below(i: u32, len: usize, what: &'static str) -> Check {
    if usize::try_from(i).is_ok_and(|i| i < len) { Ok(()) } else { bad(what) }
}

fn span_in(s: &Span, len: usize, what: &'static str) -> Check {
    let end = u64::from(s.start) + u64::from(s.len);
    if usize::try_from(end).is_ok_and(|e| e <= len) { Ok(()) } else { bad(what) }
}

impl Checker<'_> {
    fn sym(&self, id: u32) -> Check {
        below(id, self.lens[0], "symbol id")
    }

    fn opt_sym(&self, id: Option<u32>) -> Check {
        id.map_or(Ok(()), |id| self.sym(id))
    }

    fn face(&self, id: u32) -> Check {
        below(id, self.lens[1], "face id")
    }

    fn image(&self, src: &ImageSrc) -> Check {
        match src {
            ImageSrc::Media(i) => below(*i, self.lens[2], "media id"),
            ImageSrc::Derived(i) => below(*i, self.lens[3], "derived id"),
            ImageSrc::Bitmap(i) => below(*i, self.lens[4], "bitmap id"),
        }
    }

    fn font(&self, f: &FontRef) -> Check {
        self.face(f.face)
    }

    fn paint(&self, p: &Paint) -> Check {
        match p {
            Paint::Css(s) => self.sym(*s),
            Paint::Linear(i) | Paint::Radial(i) | Paint::Conic(i) => below(*i, self.list.gradients.len(), "gradient index"),
            Paint::Pattern(i) => below(*i, self.list.patterns.len(), "pattern index"),
        }
    }

    fn stops(&self, stops: &[Stop]) -> Check {
        stops.iter().try_for_each(|s| self.sym(s.color))
    }

    fn item(&self, at: usize, it: &SItem) -> Check {
        let l = self.list;
        match it {
            SItem::CaptureCtm(r) | SItem::TransformByInverseOf(r) | SItem::TransformUnshape(r) | SItem::SetTransformReg(r) => {
                below(*r, CTM_REGISTERS as usize, "ctm register")
            }
            SItem::FilterSet(s) | SItem::ShadowColor(s) => self.sym(*s),
            SItem::FillStyle(p) | SItem::StrokeStyle(p) => self.paint(p),
            SItem::LineDash(s) => span_in(s, l.dashes.len(), "dash span"),
            SItem::Font(f) | SItem::ChartFont(f) => self.font(f),
            SItem::FillText(t) => span_in(&t.text, l.texts.len(), "text span"),
            SItem::AppendPath(s) => span_in(s, l.paths.len(), "path span"),
            SItem::DrawImage(d) => self.image(&d.src),
            SItem::Prepare(s) => self.image(s),
            SItem::Text(i) => below(*i, l.runs.len(), "run index"),
            SItem::LayerBegin(b) => {
                below(b.layer, MAX_LAYERS as usize, "layer id")?;
                // the fallback target is an item after this one (the TS else-branch), at most one past the end
                let end = usize::try_from(b.fallback_end).unwrap_or(usize::MAX);
                if end > at && end <= l.items.len() { Ok(()) } else { bad("layer fallback") }
            }
            SItem::LayerEnd(e) => below(e.layer, MAX_LAYERS as usize, "layer id"),
            SItem::DrawLayer(i) | SItem::ReleaseLayer(i) => below(*i, MAX_LAYERS as usize, "layer id"),
            SItem::Effect(i) => below(*i, l.effects.len(), "effect index"),
            SItem::SlotBegin(i) => below(*i, l.slots.len(), "slot index"),
            SItem::MediaFrame(m) => m.poster.as_ref().map_or(Ok(()), |p| self.image(&p.src)),
            SItem::SheetFont(f) => {
                if let Some(font) = &f.font {
                    self.font(font)?;
                }
                f.css.map_or(Ok(()), |c| self.sym(c))
            }
            SItem::GlyphRun(g) => {
                span_in(&g.text, l.texts.len(), "text span")?;
                span_in(&g.advances, l.advances.len(), "advance span")
            }
            SItem::CellText(c) => {
                if c.units > c.len {
                    return Err(SlidesError::Invalid("cell text units past its span"));
                }
                c.face.map_or(Ok(()), |f| self.face(f))
            }
            _ => Ok(()),
        }
    }

    fn run(&self, r: &TextRun) -> Check {
        span_in(&r.text, self.list.texts.len(), "text span")?;
        self.font(&r.font)?;
        self.paint(&r.fill)?;
        if let Some(s) = &r.shadow {
            self.sym(s.color)?;
        }
        if let Some(o) = &r.outline {
            self.sym(o.color)?;
        }
        for d in [&r.underline, &r.strike].into_iter().flatten() {
            self.sym(d.color)?;
        }
        Ok(())
    }

    fn tile_params(&self, t: &TileParams) -> Check {
        self.opt_sym(t.algn)?;
        self.opt_sym(t.flip)
    }

    fn effect(&self, e: &Effect) -> Check {
        match e {
            Effect::Bevel(b) => {
                span_in(&b.path, self.list.paths.len(), "bevel path span")?;
                self.opt_sym(b.prst)?;
                self.opt_sym(b.rig)?;
                self.opt_sym(b.mat)?;
                self.opt_sym(b.dir)
            }
            Effect::MetafileRop(m) => below(m.operand, self.lens[4], "bitmap id"),
            Effect::Warp(w) => {
                self.sym(w.prst)?;
                if let Some(s) = &w.shadow {
                    self.sym(s.color)?;
                }
                if let Some(t) = &w.tile {
                    self.image(&t.image)?;
                    self.tile_params(&t.tile.params)?;
                }
                for line in &w.lines {
                    self.sym(line.align)?;
                    for r in &line.runs {
                        self.face(r.font.face)?;
                        self.sym(r.color)?;
                        if let Some(s) = &r.stroke {
                            self.sym(s.color)?;
                        }
                        if let Some(p) = &r.pattern {
                            self.sym(p.prst)?;
                            self.sym(p.fg)?;
                            self.sym(p.bg)?;
                        }
                        if let Some(p) = &r.paint {
                            self.stops(&p.stops)?;
                            self.opt_sym(p.path)?;
                            for s in &p.raw {
                                self.sym(s.color)?;
                            }
                            if let Some(i) = &p.image {
                                self.image(i)?;
                            }
                            if let Some(t) = &p.tile {
                                self.tile_params(t)?;
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

/// Checks `list` against the table sizes `lens` (`sym, faces, media, derived, bitmaps`): every span lies in its arena,
/// every arena index and table id exists, registers and layer ids are in bounds, a layer's fallback target lies
/// after its `LayerBegin`. It does not check structure that a painter repairs on its own (save/restore balance,
/// layer pairing) nor any number (NaN, infinities and huge values are legal canvas arguments).
///
/// # Errors
/// [`SlidesError::Invalid`] naming the first field that fails.
pub fn validate(list: &SlidesList, lens: &[usize; 5]) -> Result<(), SlidesError> {
    let c = Checker { list, lens };
    // the cursor of the `advances` arena: a `CellText` reads the values after the previous `GlyphRun`/`CellText` (C4b)
    let mut cursor: u64 = 0;
    let mut font_seen = false;
    let mut cell_seen = false;
    for (i, it) in list.items.iter().enumerate() {
        c.item(i, it)?;
        match it {
            SItem::GlyphRun(g) => cursor = u64::from(g.advances.start) + u64::from(g.advances.len),
            SItem::CellText(t) => {
                cursor += u64::from(t.len);
                if cursor > list.advances.len() as u64 {
                    return Err(SlidesError::Invalid("cell text past the advances"));
                }
                if t.same_font && !font_seen {
                    return Err(SlidesError::Invalid("cell text: the same font as no previous cell text"));
                }
                if t.rel && !cell_seen {
                    return Err(SlidesError::Invalid("cell text: relative to no previous cell text"));
                }
                font_seen |= t.face.is_some() || t.same_font;
                cell_seen = true;
            }
            _ => {}
        }
    }
    c.stops(&list.stops)?;
    for g in &list.gradients {
        span_in(&g.stops, list.stops.len(), "gradient stop span")?;
    }
    for p in &list.patterns {
        match &p.src {
            PatternSrc::Bitmap(b) => below(*b, lens[4], "bitmap id")?,
            PatternSrc::Image(s) => c.image(s)?,
        }
        if let Some(t) = &p.tile {
            c.tile_params(&t.params)?;
        }
    }
    for r in &list.runs {
        c.run(r)?;
    }
    for e in &list.effects {
        c.effect(e)?;
    }
    Ok(())
}

/// Checks that a frame fits the list it animates: one value per slot, replacements for shapes the slots name.
///
/// # Errors
/// [`SlidesError::Invalid`] naming the first problem.
pub fn check_frame(frame: &SlotFrame, list: &SlidesList) -> Result<(), SlidesError> {
    if frame.values.len() != list.slots.len() {
        return bad("slot values do not match the slots");
    }
    for r in &frame.replacements {
        if !list.slots.iter().any(|s| s.shape == r.shape) {
            return bad("replacement for a shape without a slot");
        }
    }
    Ok(())
}

/// The generated codec of the frame message (re-exported for tests and tools).
pub use slots::{Delta as SlotDelta, Tables as SlotTables};

/// The producer's side of a stream of `SlotFrame` messages (B4b): the filter-name symbols sent so far (the frame schema's
/// only table). One per list stream generation; [`FrameEncoder::clear`] starts it over (a new generation).
#[derive(Debug, Default)]
pub struct FrameEncoder {
    generation: u32,
    syms: SymbolTable,
    sent: usize,
}

impl FrameEncoder {
    #[must_use]
    pub fn new(generation: u32) -> FrameEncoder {
        FrameEncoder { generation, syms: SymbolTable::new(), sent: 0 }
    }

    /// The symbol of a filter name (interned on first use).
    pub fn sym(&mut self, s: &str) -> u32 {
        self.syms.intern_str(s).and_then(|s| u32::try_from(s.index()).ok()).unwrap_or(0)
    }

    /// Symbols interned so far.
    #[must_use]
    pub fn symbols(&self) -> usize {
        self.syms.len()
    }

    /// Appends the message of `frame` (with the symbols the decoder has not seen) to `out`.
    ///
    /// # Errors
    /// [`SlidesError::Encode`] for a count past 32 bits.
    pub fn encode(&mut self, frame: &SlotFrame, out: &mut Vec<u8>) -> Result<(), SlidesError> {
        let items: Vec<Vec<u16>> = self.syms.since(self.sent).map(<[u16]>::to_vec).collect();
        let delta =
            slots::Delta { generation: self.generation, sym: uo_codec::TableDelta { from: u32::try_from(self.sent).unwrap_or(0), items } };
        encode_slot_frame(out, &delta, frame)?;
        self.sent = self.syms.len();
        Ok(())
    }

    /// Forgets every symbol and moves to `generation` (the decoder replaces its table).
    pub fn reset(&mut self, generation: u32) {
        *self = FrameEncoder::new(generation);
    }
}
