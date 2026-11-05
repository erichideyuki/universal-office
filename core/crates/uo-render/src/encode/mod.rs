//! The canvas encoder (frontier E, uo-render §3.2-§3.4, §6): one [`Page`] and the [`Resources`]
//! it uses -> the per-page message of `core/schema/render.uos` the TS canvas painter decodes.
//!
//! - **resource deltas**: a message carries the entries of each table the view has not sent, up
//!   to the highest id this page needs (so pages may be pulled in any order: the entries between
//!   are sent with it) — [`DeltaMark`] is the per-view watermark, one per generation;
//! - **canvas hints** (§3.3, [`hints`]): shape parts in twips, gradient geometry and 64 samples,
//!   glyph batch joins, the F0xx remap, `gid` outlines, the run box and the fx gradient geometry,
//!   all in the canvas arithmetic;
//! - **app section** ([`AppSection`]): the producer's opaque, schema-typed bytes for the page
//!   (Write: `write.uos` `WritePage`), carried in `Page.app`;
//! - the reverse direction ([`decode`]) builds Rust values from a message, for tests, tools and
//!   the TS adapter of R-P2c (a TS layout encoded by the generated TS encoder, painted by the
//!   wasm exports).

pub mod decode;
pub mod hints;
mod to_wire;

use crate::codec as w;
use crate::paint::PaintLimits;
use crate::resources::Resources;
use crate::types::Page;
use to_wire::Conv;

/// Why a page did not encode. On an error the output buffer and the [`DeltaMark`] are as they
/// were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// an id of the page is past the end of its table in the [`Resources`]
    BadId(&'static str),
    /// a glyph span or run index does not fit the list's columns
    BadSpan,
    /// the codec refused (a count past 32 bits, columns of different lengths)
    Codec(uo_codec::EncodeError),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::BadId(t) => write!(f, "an id of table {t} is past its end"),
            EncodeError::BadSpan => f.write_str("a span does not fit the display list's columns"),
            EncodeError::Codec(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EncodeError {}

impl From<uo_codec::EncodeError> for EncodeError {
    fn from(e: uo_codec::EncodeError) -> EncodeError {
        EncodeError::Codec(e)
    }
}

/// The producer's section of one page (uo-render §3.4): the complete message of the producer's
/// own schema (own magic and hash), opaque here.
pub trait AppSection {
    /// Appends the section to `out`.
    fn write(&self, out: &mut Vec<u8>);
}

/// No section (Slides, Sheet before they have one; tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoApp;

impl AppSection for NoApp {
    fn write(&self, _out: &mut Vec<u8>) {}
}

impl AppSection for [u8] {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
}

impl AppSection for Vec<u8> {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
}

/// Options of the canvas encoder.
#[derive(Clone, Debug)]
pub struct CanvasOptions {
    /// send `GlyphRun.text` (RQ7: nothing in the painters reads it; debugging only)
    pub debug_text: bool,
    /// limits of the helpers the hints call (the 64 gradient samples)
    pub limits: PaintLimits,
}

impl Default for CanvasOptions {
    fn default() -> CanvasOptions {
        CanvasOptions { debug_text: false, limits: PaintLimits::DEFAULT }
    }
}

/// What a view has sent in one generation: the entries of each table (symbols, faces, images,
/// text effects, page fills, custom geometries), in the order of the schema's tables.
#[derive(Clone, Debug, Default)]
pub struct DeltaMark {
    generation: u32,
    sent: [usize; 6],
}

impl DeltaMark {
    /// The mark of a new layout (`generation` = its id in the view; the decoder drops its tables
    /// on a new one).
    #[must_use]
    pub fn new(generation: u32) -> DeltaMark {
        DeltaMark { generation, sent: [0; 6] }
    }

    #[must_use]
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Entries sent so far per table.
    #[must_use]
    pub fn sent(&self) -> [usize; 6] {
        self.sent
    }
}

/// [`encode_page_for_canvas_with`] with the default options.
///
/// # Errors
/// See [`EncodeError`].
pub fn encode_page_for_canvas(
    page: &Page,
    res: &Resources,
    app: &dyn AppSection,
    sent: &mut DeltaMark,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    encode_page_for_canvas_with(page, res, app, sent, &CanvasOptions::default(), out)
}

/// Appends the message of `page` to `out` and advances `sent`.
///
/// # Errors
/// [`EncodeError::BadId`] for an id past its table, [`EncodeError::BadSpan`] for a malformed list,
/// [`EncodeError::Codec`] for a size no 32-bit count expresses.
pub fn encode_page_for_canvas_with(
    page: &Page,
    res: &Resources,
    app: &dyn AppSection,
    sent: &mut DeltaMark,
    opts: &CanvasOptions,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    let mut c = Conv::new(res, opts);
    let items = page.list.items.iter().map(|it| c.item(&page.list, it)).collect::<Result<Vec<_>, _>>()?;
    let background = page.background.map(|b| c.sym(b));
    let background_fill = page.background_fill.map(|f| {
        c.refs.page_fills = c.refs.page_fills.max(f.index().saturating_add(1));
        f.raw().saturating_sub(1)
    });
    let mut app_bytes = Vec::new();
    app.write(&mut app_bytes);
    let msg = w::Page { w: page.w, h: page.h, background, background_fill, background_show: page.background_show, items, app: app_bytes };

    // the entries this page needs: those of the highest id it uses, then what those entries use
    let from = sent.sent;
    let lens = [res.symbols.len(), res.faces.len(), res.images.len(), res.text_fx.len(), res.page_fills.len(), res.cust.len()];
    let mut need = c.refs.as_array();
    let names = ["sym", "faces", "images", "text_fx", "page_fills", "cust"];
    for (k, name) in names.iter().enumerate() {
        if need.get(k).copied().unwrap_or(0) > lens.get(k).copied().unwrap_or(0) {
            return Err(EncodeError::BadId(name));
        }
    }
    // entries of page_fills / text_fx / faces / images refer to symbols and images
    let mut delta = w::Delta { generation: sent.generation, ..w::Delta::default() };
    let upto = |k: usize, need: &[usize; 6]| need.get(k).copied().unwrap_or(0).max(from.get(k).copied().unwrap_or(0));
    // page fills first (they may need images), then the tables that only need symbols
    for i in from[4]..upto(4, &need) {
        if let Some(f) = res.page_fills.get(i) {
            let e = c.page_fill(f);
            delta.page_fills.items.push(e);
        }
    }
    need = c.refs.as_array();
    for i in from[2]..upto(2, &need) {
        if let Some(im) = res.images.get(i) {
            let e = c.image(im);
            delta.images.items.push(e);
        }
    }
    for i in from[1]..upto(1, &need) {
        if let Some(f) = res.faces.get(i) {
            let e = c.face(f);
            delta.faces.items.push(e);
        }
    }
    for i in from[3]..upto(3, &need) {
        if let Some(fx) = res.text_fx.get(i) {
            let e = c.text_fx(fx);
            delta.text_fx.items.push(e);
        }
    }
    for i in from[5]..upto(5, &need) {
        if let Some(cu) = res.cust.get(i) {
            delta.cust.items.push(Conv::cust(cu));
        }
    }
    need = c.refs.as_array();
    for (k, name) in names.iter().enumerate() {
        if need.get(k).copied().unwrap_or(0) > lens.get(k).copied().unwrap_or(0) {
            return Err(EncodeError::BadId(name));
        }
    }
    let sym_to = upto(0, &need);
    for u in res.symbols.since(from[0]).take(sym_to.saturating_sub(from[0])) {
        delta.sym.items.push(u.to_vec());
    }
    let from_u32 = |i: usize| u32::try_from(from.get(i).copied().unwrap_or(0)).unwrap_or(u32::MAX);
    delta.sym.from = from_u32(0);
    delta.faces.from = from_u32(1);
    delta.images.from = from_u32(2);
    delta.text_fx.from = from_u32(3);
    delta.page_fills.from = from_u32(4);
    delta.cust.from = from_u32(5);

    let start = out.len();
    if let Err(e) = w::encode_page(out, &delta, &msg) {
        out.truncate(start);
        return Err(e.into());
    }
    sent.sent = [
        from[0] + delta.sym.items.len(),
        from[1] + delta.faces.items.len(),
        from[2] + delta.images.items.len(),
        from[3] + delta.text_fx.items.len(),
        from[4] + delta.page_fills.items.len(),
        from[5] + delta.cust.items.len(),
    ];
    Ok(())
}
