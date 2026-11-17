//! Metafiles (EMF, WMF, EMF+), QuickDraw PICT and the Slides TIFF as pictures: the producer side of `raster.ts`
//! (`rasterizeSpecialImage`), B5b of the Slides port. The TS plays a metafile on a canvas it creates at the display size;
//! the Rust side cannot paint, so it RECORDS the canvas calls ([`rec::Rec`]) as a [`Recipe`]: the shell replays them
//! (`uo-wasm/js/meta-player.ts`) on a canvas of the recipe's size, and that canvas is the picture, exactly as in the TS.
//! The calls, their order, the canvases they go to and every number are the TS's (checked line by line against the
//! recording context of the parity harness, `core/crates/uo-pptx/js/capture-meta.mjs`).
//!
//! Sources: `metafile.ts` ([`emf`], [`wmf`], the shared [`gdi`]), `emfplus.ts` ([`emfplus`]), `pict.ts` ([`pict`]),
//! `raster.ts` ([`rasterize_special`]). What the TS asks of the canvas that is not a drawing call is the [`Measure`]
//! trait: `measureText` (text metrics; the two-phase protocol of the emitter: unknown answers are collected as
//! [`Need`]s, the shell measures them, the pass runs again) and the decoded size of an EMF+ PNG/JPEG bitmap.

// The numbers of the players are JS numbers read from the file: the float-to-integer `as` of the offsets, counts and pixel sizes
// saturates, and every site is bounded by the length of the data (the TS indexes typed arrays with the same values). `!(a > b)` is
// the JS comparison with NaN kept (NaN passes where `a <= b` would not).
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::neg_cmp_op_on_partial_ord)]

pub mod emf;
pub mod emfplus;
pub mod gdi;
pub mod pict;
pub mod rec;
pub mod wmf;

use rustc_hash::FxHashMap;

use self::rec::{JsStr, Rec, js};

/// A JS exception inside the player (a `DataView` read past the end): the TS catches it where it rasterizes and keeps
/// what was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Throw;

pub type R<T> = Result<T, Throw>;

/// Concatenates code-unit strings: `cat!(&js("a"), name, &js("b"))`.
macro_rules! cat {
    ($($p:expr),* $(,)?) => {{
        let mut v: Vec<u16> = Vec::new();
        $( v.extend_from_slice(AsRef::<[u16]>::as_ref(&$p)); )*
        v
    }};
}
pub(crate) use cat;

/// `TextMetrics` of a `measureText`: the width and the font bounding box (what the players read).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub width: f64,
    pub font_ascent: f64,
    pub font_descent: f64,
}

/// The shell's answer about a compressed bitmap of an EMF+ (`decoded.get(bitmapKey(c))`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decoded {
    /// not asked yet (the TS's `prepareEmfPlus` has not run): the pass reports it as a need
    Unknown,
    /// the shell could not decode it (`null`)
    Failed,
    /// decoded: its natural size
    Ready { w: f64, h: f64 },
}

/// What the players ask of the host.
pub trait Measure {
    /// `ctx.measureText(text)` with `ctx.font = font`; `None` = not known yet.
    fn metrics(&mut self, font: &[u16], text: &[u16]) -> Option<Metrics>;
    /// The decoded state of the compressed bitmap with the TS key `key` (`bitmapKey`).
    fn decoded(&mut self, key: &str) -> Decoded;
}

/// An answer the pass needs before its output can be trusted.
#[derive(Clone, Debug, PartialEq)]
pub enum Need {
    /// `measureText(text)` under `font`
    Text { font: JsStr, text: JsStr },
    /// decode this compressed bitmap (PNG/JPEG/GIF…); `key` is `bitmapKey`
    Decode { key: String, data: Vec<u8> },
}

/// The state of one rasterization: the recording, the host, the unmet needs and the font-availability memo
/// (`fontAvail`, per rasterization: the probe is a state-only canvas sequence, so the shell's memo of it changes no pixel).
pub struct Gx<'m> {
    pub rec: Rec,
    meas: &'m mut dyn Measure,
    pub needs: Vec<Need>,
    pub font_avail: FxHashMap<String, bool>,
}

impl Gx<'_> {
    /// `ctx.measureText(text)` under the canvas's current font; a missing answer is recorded as a [`Need`] and a neutral
    /// value is returned (the pass is discarded when `needs` is not empty).
    pub fn measure(&mut self, cv: u32, text: &[u16]) -> Metrics {
        let font = self.rec.cur_font(cv).to_vec();
        match self.meas.metrics(&font, text) {
            Some(m) => m,
            None => {
                let n = Need::Text { font, text: text.to_vec() };
                if !self.needs.contains(&n) && self.needs.len() < MAX_NEEDS {
                    self.needs.push(n);
                }
                Metrics { width: 0.0, font_ascent: 0.0, font_descent: 0.0 }
            }
        }
    }

    /// The decoded state of a compressed bitmap; an unknown one is a [`Need`].
    pub fn decoded(&mut self, key: &str, data: &[u8]) -> Decoded {
        let d = self.meas.decoded(key);
        if d == Decoded::Unknown {
            let n = Need::Decode { key: key.to_string(), data: data.to_vec() };
            if !self.needs.contains(&n) && self.needs.len() < MAX_NEEDS {
                self.needs.push(n);
            }
        }
        d
    }
}

/// The longest array a JS spread (`push(...xs)`) accepts: V8 throws a `RangeError` (stack overflow) past about this many
/// elements, the exact limit depending on the stack depth at the call. The players throw at this size.
pub const SPREAD_LIMIT: usize = 120_000;

/// Cap of the needs of one pass (a hostile file cannot ask for an unbounded list).
pub const MAX_NEEDS: usize = 4096;

// ─────────────────────────── detection and size ───────────────────────────

fn at(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

/// `isEmf`.
#[must_use]
pub fn is_emf(b: &[u8]) -> bool {
    b.len() > 44
        && at(b, 0) == 1
        && at(b, 1) == 0
        && at(b, 2) == 0
        && at(b, 3) == 0
        && at(b, 40) == 0x20
        && at(b, 41) == 0x45
        && at(b, 42) == 0x4d
        && at(b, 43) == 0x46
}

/// `isWmf`.
#[must_use]
pub fn is_wmf(b: &[u8]) -> bool {
    if b.len() < 40 {
        return false;
    }
    if at(b, 0) == 0xd7 && at(b, 1) == 0xcd && at(b, 2) == 0xc6 && at(b, 3) == 0x9a {
        return true;
    }
    (at(b, 0) == 1 || at(b, 0) == 2) && at(b, 1) == 0 && at(b, 2) == 9 && at(b, 3) == 0
}

/// `isMetafile`.
#[must_use]
pub fn is_metafile(b: &[u8]) -> bool {
    is_emf(b) || is_wmf(b)
}

fn i32le(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([at(b, o), at(b, o + 1), at(b, o + 2), at(b, o + 3)])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([at(b, o), at(b, o + 1), at(b, o + 2), at(b, o + 3)])
}
fn i16le(b: &[u8], o: usize) -> i32 {
    i32::from(i16::from_le_bytes([at(b, o), at(b, o + 1)]))
}
fn u16le(b: &[u8], o: usize) -> u32 {
    u32::from(u16::from_le_bytes([at(b, o), at(b, o + 1)]))
}

/// `metafileSize`: the natural size in px at 96 dpi; `None` = unknown.
#[must_use]
pub fn metafile_size(b: &[u8]) -> Option<(f64, f64)> {
    use uo_jsnum::math::abs;
    if is_emf(b) {
        let w = f64::from(i32le(b, 32).wrapping_sub(i32le(b, 24))) / 100.0;
        let h = f64::from(i32le(b, 36).wrapping_sub(i32le(b, 28))) / 100.0;
        // the TS subtracts in doubles: the i32 subtraction cannot wrap there
        let w = (f64::from(i32le(b, 32)) - f64::from(i32le(b, 24))) / 100.0 + 0.0 * w;
        let h = (f64::from(i32le(b, 36)) - f64::from(i32le(b, 28))) / 100.0 + 0.0 * h;
        if w > 0.0 && h > 0.0 {
            return Some(((w / 25.4) * 96.0, (h / 25.4) * 96.0));
        }
        let bw = f64::from(i32le(b, 16)) - f64::from(i32le(b, 8));
        let bh = f64::from(i32le(b, 20)) - f64::from(i32le(b, 12));
        return if bw > 0.0 && bh > 0.0 { Some((bw, bh)) } else { None };
    }
    if is_wmf(b) && at(b, 0) == 0xd7 {
        let mut inch = f64::from(u16le(b, 14));
        if inch == 0.0 {
            inch = 1440.0;
        }
        let w = abs(f64::from(i16le(b, 10)) - f64::from(i16le(b, 6)));
        let h = abs(f64::from(i16le(b, 12)) - f64::from(i16le(b, 8)));
        if w > 0.0 && h > 0.0 {
            return Some(((w / inch) * 96.0, (h / inch) * 96.0));
        }
    }
    if is_wmf(b) && at(b, 0) != 0xd7 {
        let mut q = u16le(b, 2) as usize * 2;
        let mut guard = 0;
        while q + 10 <= b.len() && guard < 20000 {
            let size = u32le(b, q) as usize * 2;
            let f = u16le(b, q + 4);
            if f == 0 || size < 6 {
                break;
            }
            if f == 0x020c {
                let w = abs(f64::from(i16le(b, q + 8)));
                let h = abs(f64::from(i16le(b, q + 6)));
                if w > 0.0 && h > 0.0 {
                    return Some((w / 15.0, h / 15.0));
                }
                break;
            }
            q += size;
            guard += 1;
        }
    }
    None
}

/// `rop3MixesDest`.
fn rop3_mixes_dest(rop: u32) -> bool {
    let r3 = (rop >> 16) & 0xff;
    let uses_d = ((r3 & 0xaa) >> 1) != (r3 & 0x55);
    let uses_s = ((r3 & 0xcc) >> 2) != (r3 & 0x33);
    let uses_p = (r3 >> 4) != (r3 & 0x0f);
    uses_d && (uses_s || uses_p)
}

/// `metafileReadsDest`: the metafile copies bits mixing with the destination.
#[must_use]
pub fn metafile_reads_dest(b: &[u8]) -> bool {
    let len = b.len();
    if is_emf(b) {
        let mut pos = 0usize;
        let mut guard = 0;
        while pos + 8 <= len && guard < 500_000 {
            let ty = u32le(b, pos);
            let size = u32le(b, pos + 4) as usize;
            if size < 8 || pos + size > len || ty == 14 {
                break;
            }
            let at_ = match ty {
                76 | 77 => 40,
                81 => 68,
                _ => usize::MAX,
            };
            if at_ != usize::MAX && pos + at_ + 4 <= len && rop3_mixes_dest(u32le(b, pos + at_)) {
                return true;
            }
            pos += size;
            guard += 1;
        }
        return false;
    }
    if !is_wmf(b) {
        return false;
    }
    let mut pos = if at(b, 0) == 0xd7 { 22 } else { 0 };
    if pos + 4 > len {
        return false;
    }
    pos += u16le(b, pos + 2) as usize * 2;
    let mut guard = 0;
    while pos + 10 <= len && guard < 500_000 {
        let size = u32le(b, pos) as usize * 2;
        let f = u16le(b, pos + 4);
        if f == 0 || size < 6 || pos + size > len {
            break;
        }
        if matches!(f, 0x0b41 | 0x0f43 | 0x0940 | 0x0922 | 0x0b23 | 0x061d) && size >= 10 && rop3_mixes_dest(u32le(b, pos + 6)) {
            return true;
        }
        pos += size;
        guard += 1;
    }
    false
}

// ─────────────────────────── the entry points ───────────────────────────

/// A picture the shell realizes by replaying the recipe on a canvas `w` x `h`.
#[derive(Clone, Debug)]
pub struct Recipe {
    pub w: u32,
    pub h: u32,
    /// the encoded ops (`rec::Rec::encode`)
    pub bytes: Vec<u8>,
    /// the recording hit a cap (the TS has none): the picture is what was recorded before
    pub truncated: bool,
}

/// What `rasterizeSpecialImage` made: a picture the shell paints by replaying a recipe (metafile, PICT and TIFF: a TIFF is one
/// `putImageData` of the decoded pixels, as the canvas the TS makes for it).
#[derive(Clone, Debug)]
pub enum Raster {
    Recipe(Recipe),
}

/// The answer of a rasterization: the picture (`None` = not a special image) or the needs of another pass.
#[derive(Clone, Debug)]
pub enum Outcome {
    Done(Option<Raster>),
    Need(Vec<Need>),
}

/// The canvas of `rasterizeMetafile`: width, height and the display it is rasterized for.
type RasterSize = (u32, u32, Option<(f64, f64)>);

/// `rasterizeMetafile`'s canvas size: `(w, h)` or `None` for a size a canvas cannot take (a non-finite display).
fn raster_size(bytes: &[u8], display: Option<(f64, f64)>) -> Option<RasterSize> {
    use uo_jsnum::math::{max, round};
    let nat = metafile_size(bytes).unwrap_or((512.0, 512.0));
    let mut k = 2.0;
    let (max_side, min_side) = (2400.0, 600.0);
    let disp = display.filter(|d| d.0 > 0.0 && d.1 > 0.0);
    if let Some(d) = disp {
        k *= max(1.0, max(d.0, d.1) / max(nat.0, nat.1));
    }
    let big = max(nat.0, nat.1) * k;
    if big > max_side {
        k *= max_side / big;
    } else if big < min_side {
        k *= min_side / max(1.0, big);
    }
    let w = max(1.0, round(nat.0 * k));
    let h = max(1.0, round(nat.1 * k));
    if !w.is_finite() || !h.is_finite() {
        return None;
    }
    Some((uo_jsnum::conv::to_uint32(w), uo_jsnum::conv::to_uint32(h), disp))
}

/// `rasterizeSpecialImage(bytes, makeCanvas, display)`: a metafile or PICT as a [`Recipe`], a TIFF as a bitmap.
/// `display` is the size (px at 96 dpi) the WHOLE image has on the slide (metafiles rasterize at that scale).
pub fn rasterize_special(bytes: &[u8], display: Option<(f64, f64)>, meas: &mut dyn Measure) -> Outcome {
    emfplus::reset_work();
    if is_metafile(bytes) {
        let Some((w, h, disp)) = raster_size(bytes, display) else { return Outcome::Done(None) };
        let mut g = Gx { rec: Rec::new(w, h), meas, needs: Vec::new(), font_avail: FxHashMap::default() };
        if metafile_reads_dest(bytes) {
            g.rec.fill_style(0, "#ffffff");
            g.rec.fill_rect(0, 0.0, 0.0, f64::from(w), f64::from(h));
        }
        let dev_px = disp.map(|d| uo_jsnum::math::sqrt((f64::from(w) / d.0) * (f64::from(h) / d.1)));
        // `try { drawMetafile(...) } catch { /* truncated: what was drawn stays */ }`
        let _ = draw_metafile(&mut g, 0, bytes, 0.0, 0.0, f64::from(w), f64::from(h), dev_px, 0);
        return finish(g, w, h);
    }
    if pict::is_pict(bytes) {
        let Some((w, h)) = pict::raster_size(bytes) else { return Outcome::Done(None) };
        let mut g = Gx { rec: Rec::new(w, h), meas, needs: Vec::new(), font_avail: FxHashMap::default() };
        pict::draw_pict(&mut g, 0, bytes, 0.0, 0.0, f64::from(w), f64::from(h));
        return finish(g, w, h);
    }
    if uo_image::is_tiff_slides(bytes) {
        // `try { img = decodeTiff(bytes) } catch { img = undefined }`, then the pixels on a canvas of their own (`putImageData`)
        let Some(img) = uo_image::decode_tiff_slides(bytes).ok().flatten() else { return Outcome::Done(None) };
        let mut g = Gx { rec: Rec::new(img.width, img.height), meas, needs: Vec::new(), font_avail: FxHashMap::default() };
        g.rec.put_image(0, img.data, img.width, img.height, 0.0, 0.0);
        return finish(g, img.width, img.height);
    }
    Outcome::Done(None)
}

/// `rasterizeMetafile` alone (no PICT/TIFF dispatch): `None` when the bytes are not an EMF or WMF.
pub fn rasterize_metafile(bytes: &[u8], display: Option<(f64, f64)>, meas: &mut dyn Measure) -> Outcome {
    if !is_metafile(bytes) {
        return Outcome::Done(None);
    }
    rasterize_special(bytes, display, meas)
}

/// `rasterizePict` alone: `None` when the bytes are not a PICT.
pub fn rasterize_pict(bytes: &[u8], meas: &mut dyn Measure) -> Outcome {
    if is_metafile(bytes) || !pict::is_pict(bytes) {
        return Outcome::Done(None);
    }
    rasterize_special(bytes, None, meas)
}

fn finish(g: Gx<'_>, w: u32, h: u32) -> Outcome {
    if !g.needs.is_empty() {
        return Outcome::Need(g.needs);
    }
    let truncated = g.rec.overflow;
    let bytes = g.rec.encode(w, h);
    Outcome::Done(Some(Raster::Recipe(Recipe { w, h, bytes, truncated })))
}

/// `drawMetafile(ctx, bytes, x, y, w, h, devPx, depth)`: plays an EMF/WMF in the rectangle of canvas `cv`.
#[allow(clippy::too_many_arguments)]
pub fn draw_metafile(g: &mut Gx<'_>, cv: u32, bytes: &[u8], x: f64, y: f64, w: f64, h: f64, dev_px: Option<f64>, depth: u32) -> R<()> {
    g.rec.save(cv);
    let r = play_in(g, cv, bytes, (x, y, w, h), dev_px, depth);
    g.rec.restore(cv);
    r
}

/// The `try` body of `drawMetafile`: the pixel scale of the destination and the player of the format.
fn play_in(g: &mut Gx<'_>, cv: u32, bytes: &[u8], (x, y, w, h): (f64, f64, f64, f64), dev_px: Option<f64>, depth: u32) -> R<()> {
    let mut px = match dev_px {
        Some(p) if p > 0.0 => p,
        _ => 0.0,
    };
    if px == 0.0 && is_emf(bytes) {
        px = match metafile_size(bytes) {
            Some(nat) => {
                let v = uo_jsnum::math::sqrt(uo_jsnum::math::abs((w / nat.0) * (h / nat.1)));
                if v == 0.0 || v.is_nan() { 0.0 } else { v }
            }
            None => 0.0,
        };
        if px > 4.0 {
            px = 0.0;
        }
    }
    let dest = Dest { x, y, w, h, px };
    if is_emf(bytes) {
        emf::play_emf(g, cv, bytes, &dest, depth)
    } else if is_wmf(bytes) {
        wmf::play_wmf(g, cv, bytes, &dest)
    } else {
        Ok(())
    }
}

/// The destination rectangle of a metafile in the canvas and `px` (output px per device px; 0 = unknown).
#[derive(Clone, Copy, Debug)]
pub struct Dest {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub px: f64,
}

/// A string of the TS that is plain ASCII.
#[must_use]
pub fn ascii(s: &str) -> JsStr {
    js(s)
}

/// Nothing is cached across calls (the memo of the font probe lives in [`Gx`]); kept for the memory-gate convention.
pub fn clear_caches() {}
