//! EMF/WMF records → display items (`emf.ts`, `wmf.ts`, frontier F3; LQ7: here now, moving to a
//! shared crate with Slides' EMF path later). EMF bitmap records become BMP bytes and EMF+ images
//! are copied: derived images (K12), one `ImageId` per TS object (RQ3: per bitmap record, per
//! EMF+ image object). Bounded by `max_metafile_records`, `max_metafile_items` and a payload cap
//! (path commands + glyphs): past one of them the drawing stops with a warning (LQ14; the TS has
//! no bound and draws until memory runs out).
//!
//! Ported 1:1 in the TS operation order. JS semantics kept (each declared in the record, "Wave 1 —
//! F3"):
//! - every `DataView` read past the end of the file throws in the TS, so `layoutDocument` fails:
//!   here it is `Err(LayoutError::Internal)` (LQ9); a `Uint8Array` index past the end is
//!   `undefined` (`at`), with the TS comparison on it;
//! - `Uint8Array.subarray` clamps (negative indices count from the end), `set` past the end
//!   throws;
//! - the record guards of the TS (200,000 EMF records, 64 for the EMF+ probe) are kept; the
//!   argument spreads that throw `RangeError` in V8 above about 10^5 elements (`path.push(...)`,
//!   emf.ts:289, 314) succeed here (accepted, K11a).

use rustc_hash::FxHashMap;

use crate::drawing::ResWriter;
use crate::error::{LResult, LayoutError};
use crate::items::{ItemSide, PassItems};
use crate::options::LayoutLimits;
use uo_jsnum::math::{abs, hypot, max, min, round, sign};
use uo_render::types::{ImageItem, Ltrb, PathCmd, RunFlags, Stroke};
use uo_render::{FaceId, GlyphRunHead, ImageId, Item, OptF64, Resources, Sym};

/// The only thing the metafile players need from the engine: a face for a font record (LQ7) and a
/// place for warnings (the TS passes `DocContext` for those two only). The face's metrics, cmap and
/// advances are read from `Resources.faces` (`FaceRes::face`, the TS `face.font`).
pub trait FaceLookup {
    /// `ctx.faces.get(family, bold, italic)` (the `FaceResolver`): registers the answer in `res`
    /// when it is new; the TS throws when no face exists at all (`Err(NoFace)`).
    fn face(&mut self, res: &mut Resources, family: &[u16], bold: bool, italic: bool) -> LResult<FaceId>;
    /// `ctx.warn(msg)`.
    fn warn(&mut self, msg: &str);
}

/// Where a metafile is drawn: the destination rectangle in page twips (`dest`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetafileFrame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The bounds of one metafile (LQ14): records read, items emitted, and the payload of the items
/// (path commands plus glyphs). Past any of them the drawing stops with the warning
/// [`TRUNCATED`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetafileLimits {
    pub records: u32,
    pub items: u32,
    pub payload: u64,
}

/// Path commands plus glyphs one metafile may emit (≈ 56 MiB of `PathCmd`s; a record can declare
/// more points than it holds and read them from the records after it, so the TS work grows with
/// records × file size — the TS hangs or exhausts memory there).
pub const MAX_METAFILE_PAYLOAD: u64 = 1 << 20;

/// The warning of a truncated metafile.
pub const TRUNCATED: &str = "metafile truncated";

impl MetafileLimits {
    #[must_use]
    pub fn of(l: &LayoutLimits) -> MetafileLimits {
        MetafileLimits { records: l.max_metafile_records, items: l.max_metafile_items, payload: MAX_METAFILE_PAYLOAD }
    }
}

impl Default for MetafileLimits {
    fn default() -> MetafileLimits {
        MetafileLimits::of(&LayoutLimits::default())
    }
}

/// Which player a picture's bytes go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetafileKind {
    Emf,
    Wmf,
}

/// `isEmf(b) ? Emf : isWmf(b) ? Wmf : None`, the order of both TS call sites.
#[must_use]
pub fn metafile_kind(b: &[u8]) -> Option<MetafileKind> {
    if is_emf(b) {
        Some(MetafileKind::Emf)
    } else if is_wmf(b) {
        Some(MetafileKind::Wmf)
    } else {
        None
    }
}

/// The metafile branch of `paintObject` (engine.ts:3605–3608) and `paintInlineObject`
/// (paint.ts:202–210): `{ t: 'clip', op: 'push', x, y, w, h, src }`, the items of the player,
/// `{ t: 'clip', op: 'pop' }`. `Ok(false)` (nothing pushed) when the bytes are neither EMF nor WMF.
pub fn paint_metafile(
    bytes: &[u8],
    src: ImageId,
    dest: MetafileFrame,
    limits: MetafileLimits,
    faces: &mut dyn FaceLookup,
    w: &mut ResWriter<'_>,
    out: &mut PassItems,
) -> LResult<bool> {
    let Some(kind) = metafile_kind(bytes) else { return Ok(false) };
    out.list.push_plain(Item::ClipPush {
        x: OptF64::some(dest.x),
        y: OptF64::some(dest.y),
        w: OptF64::some(dest.w),
        h: OptF64::some(dest.h),
    });
    out.side.push(ItemSide { clip_src: Some(src), ..ItemSide::default() });
    match kind {
        MetafileKind::Emf => render_emf(bytes, dest, limits, faces, w, out)?,
        MetafileKind::Wmf => render_wmf(bytes, dest, limits, faces, w, out)?,
    }
    out.list.push_plain(Item::ClipPop);
    out.side.push(ItemSide::default());
    Ok(true)
}

// ───────────────────────── reading ─────────────────────────

const OOB: LayoutError = LayoutError::Internal("metafile: DataView read past the end");

/// The `DataView` of the TS over the whole file: a read past the end (or at a negative offset)
/// throws (`Err`).
#[derive(Clone, Copy)]
struct Rd<'b> {
    b: &'b [u8],
}

impl<'b> Rd<'b> {
    fn len(self) -> i64 {
        i64::try_from(self.b.len()).unwrap_or(i64::MAX)
    }

    fn get<const N: usize>(self, o: i64) -> LResult<[u8; N]> {
        let s = usize::try_from(o).map_err(|_| OOB)?;
        let e = s.checked_add(N).ok_or(OOB)?;
        self.b.get(s..e).and_then(|x| <[u8; N]>::try_from(x).ok()).ok_or(OOB)
    }

    fn u32(self, o: i64) -> LResult<u32> {
        self.get::<4>(o).map(u32::from_le_bytes)
    }

    fn i32(self, o: i64) -> LResult<i32> {
        self.get::<4>(o).map(i32::from_le_bytes)
    }

    fn u16(self, o: i64) -> LResult<u16> {
        self.get::<2>(o).map(u16::from_le_bytes)
    }

    fn i16(self, o: i64) -> LResult<i16> {
        self.get::<2>(o).map(i16::from_le_bytes)
    }

    fn f32(self, o: i64) -> LResult<f64> {
        self.get::<4>(o).map(|b| f64::from(f32::from_le_bytes(b)))
    }

    fn fi32(self, o: i64) -> LResult<f64> {
        self.i32(o).map(f64::from)
    }

    fn fi16(self, o: i64) -> LResult<f64> {
        self.i16(o).map(f64::from)
    }

    /// `bytes[o]` (`undefined` past the end).
    fn at(self, o: i64) -> Option<u8> {
        usize::try_from(o).ok().and_then(|i| self.b.get(i)).copied()
    }

    /// Every read of `n` items of `size` bytes from `o` stays inside the file (the TS throws at
    /// the first one that does not).
    fn check(self, o: i64, n: u64, size: u64) -> LResult<()> {
        if n == 0 {
            return Ok(());
        }
        let span = n.checked_mul(size).and_then(|s| i64::try_from(s).ok()).ok_or(OOB)?;
        if o < 0 || o.checked_add(span).is_none_or(|e| e > self.len()) { Err(OOB) } else { Ok(()) }
    }
}

/// `Uint8Array.subarray(start, end)` on integer arguments: relative indices (negative counts
/// from the end), clamped, empty when `end < start`.
fn subarray(b: &[u8], start: i64, end: i64) -> &[u8] {
    let len = i64::try_from(b.len()).unwrap_or(i64::MAX);
    let rel = |x: i64| if x < 0 { (len + x).max(0) } else { x.min(len) };
    let (s, e) = (rel(start), rel(end));
    if e <= s {
        return &[];
    }
    match (usize::try_from(s), usize::try_from(e)) {
        (Ok(s), Ok(e)) => b.get(s..e).unwrap_or(&[]),
        _ => &[],
    }
}

/// `target.set(src, offset)`: throws (`RangeError`) when it does not fit.
fn set_at(target: &mut [u8], src: &[u8], offset: i64) -> LResult<()> {
    const RANGE: LayoutError = LayoutError::Internal("metafile: typed array set past the end");
    let o = usize::try_from(offset).map_err(|_| RANGE)?;
    let e = o.checked_add(src.len()).ok_or(RANGE)?;
    target.get_mut(o..e).ok_or(RANGE)?.copy_from_slice(src);
    Ok(())
}

/// `setUint32(o, n)` of a length: `ToUint32` (modulo 2^32).
fn low32(n: usize) -> u32 {
    u32::try_from(u64::try_from(n).unwrap_or(u64::MAX) & 0xffff_ffff).unwrap_or(0)
}

fn put_u32(b: &mut [u8], o: usize, v: u32) {
    if let Some(d) = b.get_mut(o..o + 4) {
        d.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_i32(b: &mut [u8], o: usize, v: i32) {
    if let Some(d) = b.get_mut(o..o + 4) {
        d.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_u16(b: &mut [u8], o: usize, v: u16) {
    if let Some(d) = b.get_mut(o..o + 2) {
        d.copy_from_slice(&v.to_le_bytes());
    }
}

/// `colorOf(v)` of both files: the COLORREF's red, green and blue as `RRGGBB` in upper case.
fn color_of(v: u32) -> String {
    let [r, g, b, _] = v.to_le_bytes();
    format!("{r:02X}{g:02X}{b:02X}")
}

/// `(bmp[k] * 255) / a` rounded and capped (the un-premultiply of both bitmap paths).
fn unpremul(bmp: &mut [u8], k: usize) {
    let Some(&a) = bmp.get(k + 3) else { return };
    if a > 0 && a < 255 {
        for c in 0..3 {
            if let Some(v) = bmp.get_mut(k + c) {
                *v = uo_jsnum::conv::to_uint8(min(255.0, round((f64::from(*v) * 255.0) / f64::from(a))));
            }
        }
    }
}

// ───────────────────────── output ─────────────────────────

/// The items one player emits, with its budget.
struct Emit<'o> {
    out: &'o mut PassItems,
    limits: MetafileLimits,
    items: u32,
    payload: u64,
    truncated: bool,
}

impl Emit<'_> {
    /// Room for one more item carrying `payload` units; `false` = the drawing stops here.
    fn room(&mut self, payload: u64) -> bool {
        if self.truncated || self.items >= self.limits.items || self.payload.saturating_add(payload) > self.limits.payload {
            self.truncated = true;
            return false;
        }
        self.items += 1;
        self.payload = self.payload.saturating_add(payload);
        true
    }

    /// Room for `payload` units collected in a path under construction (`BEGINPATH`).
    fn reserve(&mut self, payload: u64) -> bool {
        if self.truncated || self.payload.saturating_add(payload) > self.limits.payload {
            self.truncated = true;
            return false;
        }
        true
    }

    fn plain(&mut self, item: Item) {
        self.out.list.push_plain(item);
        self.out.side.push(ItemSide::default());
    }

    fn path(&mut self, cmds: &[PathCmd], fill: Option<Sym>, stroke: Option<Stroke>, even_odd: bool) -> LResult<()> {
        self.out.list.push_path(cmds, fill, stroke, even_odd)?;
        self.out.side.push(ItemSide::default());
        Ok(())
    }

    fn glyphs(&mut self, head: GlyphRunHead<'_>, glyphs: impl IntoIterator<Item = (u32, f64, Option<u32>)>) -> LResult<()> {
        self.out.list.push_glyphs(head, glyphs)?;
        self.out.side.push(ItemSide::default());
        Ok(())
    }
}

fn len64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

#[allow(clippy::too_many_arguments)]
fn image_item(x: f64, y: f64, w: f64, h: f64, image: ImageId, crop: Option<Ltrb>, flip_h: bool, flip_v: bool) -> Item {
    Item::Image(Box::new(ImageItem { x, y, w, h, image, crop, rot: OptF64::NONE, flip_h, flip_v, pic: None }))
}

// ───────────────────────── EMF ─────────────────────────

#[derive(Clone, Copy, Debug)]
struct Xform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

const IDENT: Xform = Xform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

fn mul(m: Xform, n: Xform) -> Xform {
    Xform {
        a: m.a * n.a + m.b * n.c,
        b: m.a * n.b + m.b * n.d,
        c: m.c * n.a + m.d * n.c,
        d: m.c * n.b + m.d * n.d,
        e: m.e * n.a + m.f * n.c + n.e,
        f: m.e * n.b + m.f * n.d + n.f,
    }
}

/// `Pen` (colour as the COLORREF; `colorOf` at emission).
#[derive(Clone, Copy, Debug)]
struct Pen {
    style: u32,
    width: f64,
    color: u32,
}

#[derive(Clone, Copy, Debug)]
struct Brush {
    style: u32,
    color: u32,
}

/// `Font`; `face` indexes the player's face names (0 = `Arial`, 1 = `Segoe UI`).
#[derive(Clone, Copy, Debug)]
struct EFont {
    height: f64,
    weight: f64,
    italic: bool,
    face: usize,
}

#[derive(Clone, Copy, Debug)]
enum Obj {
    Pen(Pen),
    Brush(Brush),
    Font(EFont),
}

/// `STOCK[h & 0x7fffffff]`.
fn stock(i: u32) -> Option<Obj> {
    let brush = |style, color| Some(Obj::Brush(Brush { style, color }));
    let pen = |style, color| Some(Obj::Pen(Pen { style, width: 0.0, color }));
    match i {
        0 => brush(0, 0xFF_FFFF),
        1 => brush(0, 0xC0_C0C0),
        2 => brush(0, 0x80_8080),
        3 => brush(0, 0x40_4040),
        4 => brush(0, 0),
        5 => brush(1, 0),
        6 => pen(0, 0xFF_FFFF),
        7 => pen(0, 0),
        8 => pen(5, 0),
        13 => Some(Obj::Font(EFont { height: -12.0, weight: 400.0, italic: false, face: 0 })),
        17 => Some(Obj::Font(EFont { height: -11.0, weight: 400.0, italic: false, face: 1 })),
        _ => None,
    }
}

/// `DcState` (plain data: `SAVEDC` copies it).
#[derive(Clone, Copy, Debug)]
struct DcState {
    map_mode: u32,
    win_org: [f64; 2],
    win_ext: [f64; 2],
    vp_org: [f64; 2],
    vp_ext: [f64; 2],
    world: Xform,
    pen: Pen,
    brush: Brush,
    font: EFont,
    text_color: u32,
    text_align: u32,
    poly_fill: u32,
    cur: [f64; 2],
}

/// `isEmf(b)`.
#[must_use]
pub fn is_emf(b: &[u8]) -> bool {
    b.len() > 44 && b.get(0..4) == Some(&[1, 0, 0, 0]) && b.get(40..44) == Some(&[0x20, 0x45, 0x4d, 0x46])
}

/// What the EMF player keeps between records.
struct EmfPlayer<'p, 'b> {
    rd: Rd<'b>,
    dest: MetafileFrame,
    px_per_hmm_x: f64,
    px_per_hmm_y: f64,
    fx0: f64,
    fy0: f64,
    sx: f64,
    sy: f64,
    dc: DcState,
    faces_names: Vec<Vec<u16>>,
    path: Option<Vec<PathCmd>>,
    emit: Emit<'p>,
}

impl EmfPlayer<'_, '_> {
    /// `mapScale()`: logical → device scale (no translation).
    fn map_scale(&self) -> [f64; 2] {
        let (px, py) = (self.px_per_hmm_x, self.px_per_hmm_y);
        let dc = &self.dc;
        match dc.map_mode {
            2 => [0.1 * px * 10.0, -0.1 * py * 10.0],
            3 => [0.01 * px * 100.0 / 1.0, -0.01 * py * 100.0],
            4 => [0.254 * px * 100.0, -0.254 * py * 100.0],
            5 => [0.0254 * px * 100.0, -0.0254 * py * 100.0],
            6 => [(25.4 / 1440.0) * px * 100.0, -(25.4 / 1440.0) * py * 100.0],
            7 | 8 => [dc.vp_ext[0] / or1(dc.win_ext[0]), dc.vp_ext[1] / or1(dc.win_ext[1])],
            _ => [1.0, 1.0],
        }
    }

    fn to_dev(&self, x: f64, y: f64) -> [f64; 2] {
        let w = self.dc.world;
        let wx = x * w.a + y * w.c + w.e;
        let wy = x * w.b + y * w.d + w.f;
        let [kx, ky] = self.map_scale();
        let mut s = [kx, ky];
        if self.dc.map_mode == 7 {
            let k = min(abs(kx), abs(ky));
            s = [sign(kx) * k, sign(ky) * k];
        }
        [(wx - self.dc.win_org[0]) * s[0] + self.dc.vp_org[0], (wy - self.dc.win_org[1]) * s[1] + self.dc.vp_org[1]]
    }

    fn to_page(&self, x: f64, y: f64) -> [f64; 2] {
        let [dx, dy] = self.to_dev(x, y);
        [self.dest.x + (dx - self.fx0) * self.sx, self.dest.y + (dy - self.fy0) * self.sy]
    }

    fn len_tw(&self, logical: f64) -> f64 {
        let [kx, _] = self.map_scale();
        let w = self.dc.world;
        abs(logical * hypot(w.a, w.b) * kx) * self.sx
    }

    /// `emitPath(cmds, fill, stroke)`.
    fn emit_path(&mut self, cmds: &[PathCmd], fill: bool, stroke: bool, w: &mut ResWriter<'_>) -> LResult<()> {
        if cmds.is_empty() {
            return Ok(());
        }
        let dc = self.dc;
        let f = fill && dc.brush.style != 1;
        let pen = dc.pen;
        let st = stroke && pen.style != 5;
        if !f && !st {
            return Ok(());
        }
        if !self.emit.room(len64(cmds.len())) {
            return Ok(());
        }
        let fill = if f { Some(w.sym(&color_of(dc.brush.color))?) } else { None };
        let stroke = if st {
            let width = if uo_jsnum::conv::truthy(pen.width) { pen.width } else { 1.0 };
            let dash = if pen.style >= 1 && pen.style <= 4 { Some(w.sym("dashed")?) } else { None };
            Some(Stroke { color: w.sym(&color_of(pen.color))?, width_pt: max(0.25, self.len_tw(width) / 20.0), dash })
        } else {
            None
        };
        self.emit.path(cmds, fill, stroke, dc.poly_fill == 1)
    }

    /// `draw(cmds, closed)`: into the open path, or emitted.
    fn draw(&mut self, cmds: Vec<PathCmd>, closed: bool, w: &mut ResWriter<'_>) -> LResult<()> {
        if let Some(p) = &mut self.path {
            p.extend(cmds);
            return Ok(());
        }
        self.emit_path(&cmds, closed, true, w)
    }

    /// The payload a record is about to produce fits (`false` = the drawing stops here).
    fn fits(&mut self, n: u64) -> bool {
        let pending = self.path.as_ref().map_or(0, |p| len64(p.len()));
        self.emit.reserve(n.saturating_add(pending))
    }

    fn pts16(&self, o: i64, n: u32) -> LResult<Vec<[f64; 2]>> {
        self.rd.check(o, u64::from(n), 4)?;
        let mut v = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
        let mut q = o;
        for _ in 0..n {
            v.push([self.rd.fi16(q)?, self.rd.fi16(q + 2)?]);
            q += 4;
        }
        Ok(v)
    }

    fn pts32(&self, o: i64, n: u32) -> LResult<Vec<[f64; 2]>> {
        self.rd.check(o, u64::from(n), 8)?;
        let mut v = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
        let mut q = o;
        for _ in 0..n {
            v.push([self.rd.fi32(q)?, self.rd.fi32(q + 4)?]);
            q += 8;
        }
        Ok(v)
    }

    fn pts(&self, short: bool, o: i64, n: u32) -> LResult<Vec<[f64; 2]>> {
        if short { self.pts16(o, n) } else { self.pts32(o, n) }
    }

    /// `poly(pts, closed, cont)`.
    fn poly(&self, pts: &[[f64; 2]], closed: bool, cont: bool) -> Vec<PathCmd> {
        let mut c = Vec::with_capacity(pts.len() + usize::from(closed));
        for (k, &[x, y]) in pts.iter().enumerate() {
            let [px, py] = self.to_page(x, y);
            c.push(if k == 0 && !cont { PathCmd::M { x: px, y: py } } else { PathCmd::L { x: px, y: py } });
        }
        if closed {
            c.push(PathCmd::Z);
        }
        c
    }

    /// `bezier(pts, startWithMove)`; `pts[0]![0]` of an empty list throws a `TypeError` in the TS
    /// (POLYBEZIER with no point).
    fn bezier(&self, pts: &[[f64; 2]], start_with_move: bool) -> LResult<Vec<PathCmd>> {
        let mut c = Vec::new();
        let mut k = 0usize;
        if start_with_move {
            let &[x0, y0] = pts.first().ok_or(LayoutError::Internal("metafile: POLYBEZIER without points"))?;
            let [x, y] = self.to_page(x0, y0);
            c.push(PathCmd::M { x, y });
            k = 1;
        }
        while k + 2 < pts.len() {
            let (Some(&a), Some(&b), Some(&d)) = (pts.get(k), pts.get(k + 1), pts.get(k + 2)) else { break };
            let a = self.to_page(a[0], a[1]);
            let b = self.to_page(b[0], b[1]);
            let d = self.to_page(d[0], d[1]);
            c.push(PathCmd::C { x1: a[0], y1: a[1], x2: b[0], y2: b[1], x: d[0], y: d[1] });
            k += 3;
        }
        Ok(c)
    }

    fn rect(&self, l: f64, t: f64, r: f64, b: f64) -> Vec<PathCmd> {
        self.poly(&[[l, t], [r, t], [r, b], [l, b]], true, false)
    }

    fn ellipse(&self, l: f64, t: f64, r: f64, b: f64) -> Vec<PathCmd> {
        const K: f64 = 0.5523;
        let cx = (l + r) / 2.0;
        let cy = (t + b) / 2.0;
        let rx = (r - l) / 2.0;
        let ry = (b - t) / 2.0;
        let p0 = self.to_page(cx + rx, cy);
        let seg = |x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64| {
            let a = self.to_page(x1, y1);
            let b2 = self.to_page(x2, y2);
            let d = self.to_page(x, y);
            PathCmd::C { x1: a[0], y1: a[1], x2: b2[0], y2: b2[1], x: d[0], y: d[1] }
        };
        vec![
            PathCmd::M { x: p0[0], y: p0[1] },
            seg(cx + rx, cy + ry * K, cx + rx * K, cy + ry, cx, cy + ry),
            seg(cx - rx * K, cy + ry, cx - rx, cy + ry * K, cx - rx, cy),
            seg(cx - rx, cy - ry * K, cx - rx * K, cy - ry, cx, cy - ry),
            seg(cx + rx * K, cy - ry, cx + rx, cy - ry * K, cx + rx, cy),
            PathCmd::Z,
        ]
    }

    /// `dib(off, cbBmi, offBits, cbBits, premul)`: the BMP file of a DIB record (`None` = `null`).
    fn dib(&self, off: i64, cb_bmi: u32, off_bits: i64, cb_bits: u32, premul: bool) -> LResult<Option<Vec<u8>>> {
        let len = self.rd.len();
        if cb_bmi == 0 || cb_bits == 0 || off + i64::from(cb_bmi) > len || off_bits + i64::from(cb_bits) > len {
            return Ok(None);
        }
        let bytes = self.rd.b;
        // BMP = file header (14) + BITMAPINFO (hdr bytes) + bits
        let build = |hdr: u32| -> LResult<Vec<u8>> {
            let total = 14usize + usize::try_from(hdr).map_err(|_| OOB)? + usize::try_from(cb_bits).map_err(|_| OOB)?;
            let mut b = vec![0u8; total];
            if let Some(m) = b.get_mut(0..2) {
                m.copy_from_slice(&[0x42, 0x4d]);
            }
            put_u32(&mut b, 2, low32(total));
            put_u32(&mut b, 10, 14u32.saturating_add(hdr));
            set_at(&mut b, subarray(bytes, off, off + i64::from(cb_bmi)), 14)?;
            set_at(&mut b, subarray(bytes, off_bits, off_bits + i64::from(cb_bits)), 14 + i64::from(hdr))?;
            Ok(b)
        };
        let mut bmp = build(cb_bmi)?;
        // AlphaBlend with AC_SRC_ALPHA: 32 bpp premultiplied → straight alpha: a V3 header (56
        // bytes) with BITFIELDS and explicit BGRA masks, which decodeBmp follows
        if premul {
            let bpp = Rd { b: &bmp }.u16(14 + 14)?;
            if bpp == 32 {
                let hdr = cb_bmi.max(56);
                bmp = build(hdr)?;
                put_u32(&mut bmp, 14, hdr);
                put_u32(&mut bmp, 14 + 16, 3);
                put_u32(&mut bmp, 14 + 40, 0x00ff_0000);
                put_u32(&mut bmp, 14 + 44, 0x0000_ff00);
                put_u32(&mut bmp, 14 + 48, 0x0000_00ff);
                put_u32(&mut bmp, 14 + 52, 0xff00_0000);
                let mut k = 14 + usize::try_from(hdr).unwrap_or(usize::MAX);
                while k.checked_add(3).is_some_and(|e| e < bmp.len()) {
                    unpremul(&mut bmp, k);
                    k += 4;
                }
            }
        }
        Ok(Some(bmp))
    }

    /// A blit's image item (`{ t: 'image', x: min, y: min, w: abs, h: abs, image }`).
    #[allow(clippy::too_many_arguments)]
    fn blit(&mut self, img: Vec<u8>, xd: f64, yd: f64, cx: f64, cy: f64, flip_v_on_y: bool, w: &mut ResWriter<'_>) -> LResult<()> {
        let [x0, y0] = self.to_page(xd, yd);
        let [x1, y1] = self.to_page(xd + cx, yd + cy);
        if !self.emit.room(0) {
            return Ok(());
        }
        let id = w.derived_image(img, "bmp", 0.0, 0.0)?;
        self.emit.plain(image_item(min(x0, x1), min(y0, y1), abs(x1 - x0), abs(y1 - y0), id, None, false, flip_v_on_y && y1 < y0));
        Ok(())
    }
}

/// `x || 1` for a window extent.
fn or1(x: f64) -> f64 {
    if uo_jsnum::conv::truthy(x) { x } else { 1.0 }
}

/// `renderEmf(ctx, bytes, dest)`: the EMF drawn into `dest` (twips), appended to `out`.
pub fn render_emf(
    bytes: &[u8],
    dest: MetafileFrame,
    limits: MetafileLimits,
    faces: &mut dyn FaceLookup,
    w: &mut ResWriter<'_>,
    out: &mut PassItems,
) -> LResult<()> {
    let rd = Rd { b: bytes };
    if !is_emf(bytes) {
        return Ok(());
    }
    // frame (0.01 mm) → pixels of the reference device
    let frame = [rd.fi32(24)?, rd.fi32(28)?, rd.fi32(32)?, rd.fi32(36)?];
    let dev_px = [rd.fi32(72)?, rd.fi32(76)?];
    let dev_mm = [rd.fi32(80)?, rd.fi32(84)?];
    let px_per_hmm_x = dev_px[0] / max(1.0, dev_mm[0] * 100.0);
    let px_per_hmm_y = dev_px[1] / max(1.0, dev_mm[1] * 100.0);
    let fx0 = frame[0] * px_per_hmm_x;
    let fy0 = frame[1] * px_per_hmm_y;
    let fw = max(1e-6, (frame[2] - frame[0]) * px_per_hmm_x);
    let fh = max(1e-6, (frame[3] - frame[1]) * px_per_hmm_y);
    let sx = dest.w / fw;
    let sy = dest.h / fh;
    let pen = Pen { style: 0, width: 0.0, color: 0 };
    let dc = DcState {
        map_mode: 1,
        win_org: [0.0, 0.0],
        win_ext: [1.0, 1.0],
        vp_org: [0.0, 0.0],
        vp_ext: [1.0, 1.0],
        world: IDENT,
        pen,
        brush: Brush { style: 0, color: 0xFF_FFFF },
        font: EFont { height: -12.0, weight: 400.0, italic: false, face: 0 },
        text_color: 0,
        text_align: 0,
        poly_fill: 1,
        cur: [0.0, 0.0],
    };
    let mut p = EmfPlayer {
        rd,
        dest,
        px_per_hmm_x,
        px_per_hmm_y,
        fx0,
        fy0,
        sx,
        sy,
        dc,
        faces_names: vec![units("Arial"), units("Segoe UI")],
        path: None,
        emit: Emit { out, limits, items: 0, payload: 0, truncated: false },
    };
    let r = play_emf(&mut p, faces, w);
    if p.emit.truncated {
        faces.warn(TRUNCATED);
    }
    r
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn play_emf(p: &mut EmfPlayer<'_, '_>, faces: &mut dyn FaceLookup, w: &mut ResWriter<'_>) -> LResult<()> {
    let rd = p.rd;
    let bytes = rd.b;
    let len = rd.len();
    let mut stack: Vec<DcState> = Vec::new();
    let mut objs: FxHashMap<u32, Obj> = FxHashMap::default();
    // EMF+ "only" (no equivalent GDI records): Word draws through GDI+ — the image subset is played
    let mut plus = if emf_plus_only(bytes) { Some(EmfPlusPlayer::new(bytes)) } else { None };
    // Word rasterizes a figure that copies bits with a destination-dependent raster operation
    // (masked icon: SRCAND + SRCINVERT): its text becomes outlines in the PDF (oracle emfrop)
    let raster_text = plus.is_none() && emf_rasters_dest(bytes);
    let mut pos: i64 = 0;
    let mut guard: u32 = 0;
    let mut records: u32 = 0;
    while pos + 8 <= len && guard < 200_000 {
        guard += 1;
        let ty = rd.u32(pos)?;
        let size = i64::from(rd.u32(pos + 4)?);
        if size < 8 || pos + size > len {
            break;
        }
        if records >= p.emit.limits.records {
            p.emit.truncated = true;
            break;
        }
        records += 1;
        let r = pos + 8;
        if let Some(pl) = &mut plus {
            if ty == 70 && size >= 16 && rd.u32(r + 4)? == EMF_PLUS {
                let end = (pos + size).min(r + 4 + i64::from(rd.u32(r)?));
                pl.play(r + 8, end, p, w)?;
                if p.emit.truncated {
                    break;
                }
            }
            if ty == 14 {
                break;
            }
            pos += size;
            continue;
        }
        if p.emit.truncated {
            break;
        }
        match ty {
            // EOF
            14 => {
                pos = len;
                continue;
            }
            17 => p.dc.map_mode = rd.u32(r)?,
            9 => p.dc.win_ext = [rd.fi32(r)?, rd.fi32(r + 4)?],
            10 => p.dc.win_org = [rd.fi32(r)?, rd.fi32(r + 4)?],
            11 => p.dc.vp_ext = [rd.fi32(r)?, rd.fi32(r + 4)?],
            12 => p.dc.vp_org = [rd.fi32(r)?, rd.fi32(r + 4)?],
            // SETWORLDTRANSFORM
            35 => {
                p.dc.world = Xform {
                    a: rd.f32(r)?,
                    b: rd.f32(r + 4)?,
                    c: rd.f32(r + 8)?,
                    d: rd.f32(r + 12)?,
                    e: rd.f32(r + 16)?,
                    f: rd.f32(r + 20)?,
                };
            }
            // MODIFYWORLDTRANSFORM
            36 => {
                let m = Xform {
                    a: rd.f32(r)?,
                    b: rd.f32(r + 4)?,
                    c: rd.f32(r + 8)?,
                    d: rd.f32(r + 12)?,
                    e: rd.f32(r + 16)?,
                    f: rd.f32(r + 20)?,
                };
                let mode = rd.u32(r + 24)?;
                p.dc.world = match mode {
                    1 => IDENT,
                    2 => mul(m, p.dc.world),
                    3 => mul(p.dc.world, m),
                    _ => m,
                };
            }
            // SAVEDC
            33 => stack.push(p.dc),
            // RESTOREDC (relative)
            34 => {
                let mut n = rd.i32(r)?;
                let mut s: Option<DcState> = None;
                while n < 0 && !stack.is_empty() {
                    s = stack.pop();
                    n += 1;
                }
                if let Some(s) = s {
                    p.dc = s;
                }
            }
            // SETBKMODE (kept by the TS, never read)
            18 => {
                rd.u32(r)?;
            }
            19 => p.dc.poly_fill = rd.u32(r)?,
            22 => p.dc.text_align = rd.u32(r)?,
            24 => p.dc.text_color = rd.u32(r)?,
            // SETBKCOLOR (kept by the TS, never read)
            25 => {
                rd.u32(r)?;
            }
            38 => {
                let h = rd.u32(r)?;
                let pen = Pen { style: rd.u32(r + 4)? & 0xf, width: rd.fi32(r + 8)?, color: rd.u32(r + 16)? };
                objs.insert(h, Obj::Pen(pen));
            }
            95 => {
                let h = rd.u32(r)?;
                let pen = Pen { style: rd.u32(r + 20)? & 0xf, width: f64::from(rd.u32(r + 24)?), color: rd.u32(r + 32)? };
                objs.insert(h, Obj::Pen(pen));
            }
            39 => {
                let h = rd.u32(r)?;
                let brush = Brush { style: rd.u32(r + 4)?, color: rd.u32(r + 8)? };
                objs.insert(h, Obj::Brush(brush));
            }
            // EXTCREATEFONTINDIRECTW
            82 => {
                let mut face = Vec::new();
                for k in 0..32 {
                    let c = rd.u16(r + 32 + k * 2)?;
                    if c == 0 {
                        break;
                    }
                    face.push(c);
                }
                let h = rd.u32(r)?;
                let height = rd.fi32(r + 4)?;
                rd.i32(r + 12)?;
                let weight = rd.fi32(r + 20)?;
                let italic = rd.at(r + 24) != Some(0);
                p.faces_names.push(face);
                let font = EFont { height, weight, italic, face: p.faces_names.len() - 1 };
                objs.insert(h, Obj::Font(font));
            }
            // SELECTOBJECT
            37 => {
                let h = rd.u32(r)?;
                let o = if h & 0x8000_0000 != 0 { stock(h & 0x7fff_ffff) } else { objs.get(&h).copied() };
                match o {
                    Some(Obj::Pen(pen)) => p.dc.pen = pen,
                    Some(Obj::Brush(b)) => p.dc.brush = b,
                    Some(Obj::Font(f)) => p.dc.font = f,
                    None => {}
                }
            }
            40 => {
                objs.remove(&rd.u32(r)?);
            }
            // MOVETOEX
            27 => {
                p.dc.cur = [rd.fi32(r)?, rd.fi32(r + 4)?];
                if p.path.is_some() {
                    if !p.fits(1) {
                        break;
                    }
                    let [x, y] = p.to_page(p.dc.cur[0], p.dc.cur[1]);
                    if let Some(path) = &mut p.path {
                        path.push(PathCmd::M { x, y });
                    }
                }
            }
            // LINETO
            54 => {
                let to = [rd.fi32(r)?, rd.fi32(r + 4)?];
                if !p.fits(2) {
                    break;
                }
                if p.path.is_some() {
                    let [x, y] = p.to_page(to[0], to[1]);
                    if let Some(path) = &mut p.path {
                        path.push(PathCmd::L { x, y });
                    }
                } else {
                    let c = p.poly(&[p.dc.cur, to], false, false);
                    p.draw(c, false, w)?;
                }
                p.dc.cur = to;
            }
            // RECTANGLE, ROUNDRECT (≈ rectangle)
            43 | 44 => {
                let (l, t, rr, b) = (rd.fi32(r)?, rd.fi32(r + 4)?, rd.fi32(r + 8)?, rd.fi32(r + 12)?);
                if !p.fits(5) {
                    break;
                }
                let c = p.rect(l, t, rr, b);
                p.draw(c, true, w)?;
            }
            // ELLIPSE
            42 => {
                let (l, t, rr, b) = (rd.fi32(r)?, rd.fi32(r + 4)?, rd.fi32(r + 8)?, rd.fi32(r + 12)?);
                if !p.fits(6) {
                    break;
                }
                let c = p.ellipse(l, t, rr, b);
                p.draw(c, true, w)?;
            }
            // POLYGON16, POLYLINE16, POLYGON, POLYLINE
            86 | 87 | 3 | 4 => {
                let n = rd.u32(r + 16)?;
                let short = ty == 86 || ty == 87;
                p.rd.check(r + 20, u64::from(n), if short { 4 } else { 8 })?;
                if !p.fits(u64::from(n) + 1) {
                    break;
                }
                let pts = p.pts(short, r + 20, n)?;
                let closed = ty == 86 || ty == 3;
                let c = p.poly(&pts, closed, false);
                p.draw(c, closed, w)?;
            }
            // POLYLINETO16, POLYLINETO
            89 | 6 => {
                let n = rd.u32(r + 16)?;
                let short = ty == 89;
                p.rd.check(r + 20, u64::from(n), if short { 4 } else { 8 })?;
                if !p.fits(u64::from(n) + 1) {
                    break;
                }
                let pts = p.pts(short, r + 20, n)?;
                if p.path.is_some() {
                    let c = p.poly(&pts, false, true);
                    if let Some(path) = &mut p.path {
                        path.extend(c);
                    }
                } else {
                    let mut all = Vec::with_capacity(pts.len() + 1);
                    all.push(p.dc.cur);
                    all.extend_from_slice(&pts);
                    let c = p.poly(&all, false, false);
                    p.draw(c, false, w)?;
                }
                if let Some(&last) = pts.last() {
                    p.dc.cur = last;
                }
            }
            // POLYBEZIER16, POLYBEZIER
            85 | 2 => {
                let n = rd.u32(r + 16)?;
                let short = ty == 85;
                p.rd.check(r + 20, u64::from(n), if short { 4 } else { 8 })?;
                if !p.fits(u64::from(n) + 1) {
                    break;
                }
                let pts = p.pts(short, r + 20, n)?;
                let c = p.bezier(&pts, true)?;
                p.draw(c, false, w)?;
            }
            // POLYBEZIERTO16, POLYBEZIERTO
            88 | 5 => {
                let n = rd.u32(r + 16)?;
                let short = ty == 88;
                p.rd.check(r + 20, u64::from(n), if short { 4 } else { 8 })?;
                if !p.fits(u64::from(n) + 1) {
                    break;
                }
                let pts = p.pts(short, r + 20, n)?;
                let mut all = Vec::with_capacity(pts.len() + 1);
                all.push(p.dc.cur);
                all.extend_from_slice(&pts);
                let c = p.bezier(&all, p.path.is_none())?;
                if let Some(path) = &mut p.path {
                    path.extend(c);
                } else {
                    p.draw(c, false, w)?;
                }
                if let Some(&last) = pts.last() {
                    p.dc.cur = last;
                }
            }
            // POLYPOLYGON16, POLYPOLYGON
            91 | 8 => {
                let n = rd.u32(r + 16)?;
                let short = ty == 91;
                let step: u64 = if short { 4 } else { 8 };
                p.rd.check(r + 24, u64::from(n), 4)?;
                let mut counts = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
                for k in 0..i64::from(n) {
                    counts.push(rd.u32(r + 24 + k * 4)?);
                }
                let mut o = r + 24 + i64::from(n) * 4;
                let mut cmds: Vec<PathCmd> = Vec::new();
                let mut stopped = false;
                for c in counts {
                    p.rd.check(o, u64::from(c), step)?;
                    if !p.fits(len64(cmds.len()).saturating_add(u64::from(c) + 1)) {
                        stopped = true;
                        break;
                    }
                    let pts = p.pts(short, o, c)?;
                    cmds.extend(p.poly(&pts, true, false));
                    o += i64::from(c) * if short { 4 } else { 8 };
                }
                if stopped {
                    break;
                }
                p.draw(cmds, true, w)?;
            }
            // POLYPOLYLINE16, POLYPOLYLINE
            90 | 7 => {
                let n = rd.u32(r + 16)?;
                let short = ty == 90;
                let step: u64 = if short { 4 } else { 8 };
                p.rd.check(r + 24, u64::from(n), 4)?;
                let mut counts = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
                for k in 0..i64::from(n) {
                    counts.push(rd.u32(r + 24 + k * 4)?);
                }
                let mut o = r + 24 + i64::from(n) * 4;
                let mut stopped = false;
                for c in counts {
                    p.rd.check(o, u64::from(c), step)?;
                    if !p.fits(u64::from(c)) {
                        stopped = true;
                        break;
                    }
                    let pts = p.pts(short, o, c)?;
                    let cmds = p.poly(&pts, false, false);
                    p.draw(cmds, false, w)?;
                    if p.emit.truncated {
                        stopped = true;
                        break;
                    }
                    o += i64::from(c) * if short { 4 } else { 8 };
                }
                if stopped {
                    break;
                }
            }
            // BEGINPATH
            59 => p.path = Some(Vec::new()),
            // ENDPATH
            60 => {}
            // CLOSEFIGURE
            61 => {
                if p.path.is_some() && !p.fits(1) {
                    break;
                }
                if let Some(path) = &mut p.path {
                    path.push(PathCmd::Z);
                }
            }
            // FILLPATH, STROKEANDFILLPATH, STROKEPATH
            62..=64 => {
                if let Some(path) = p.path.take() {
                    p.emit_path(&path, ty != 64, ty != 62, w)?;
                }
            }
            // SELECTCLIPPATH / ABORTPATH
            67 | 68 => p.path = None,
            // EXTTEXTOUTW
            84 => {
                if !ext_text_out_w(p, pos, r, raster_text, faces, w)? {
                    break;
                }
            }
            // BITBLT / STRETCHBLT
            76 | 77 => {
                let (xd, yd, cx, cy) = (rd.fi32(r + 16)?, rd.fi32(r + 20)?, rd.fi32(r + 24)?, rd.fi32(r + 28)?);
                let off_bmi = i64::from(rd.u32(r + 76)?);
                let cb_bmi = rd.u32(r + 80)?;
                let off_bits = i64::from(rd.u32(r + 84)?);
                let cb_bits = rd.u32(r + 88)?;
                let img = if cb_bmi != 0 { p.dib(pos + off_bmi, cb_bmi, pos + off_bits, cb_bits, false)? } else { None };
                if let Some(img) = img {
                    p.blit(img, xd, yd, cx, cy, false, w)?;
                } else {
                    let [x0, y0] = p.to_page(xd, yd);
                    let [x1, y1] = p.to_page(xd + cx, yd + cy);
                    // without a source bitmap Word paints only PATCOPY, BLACKNESS and WHITENESS —
                    // all three with the current brush; plain D, PATINVERT, DSTINVERT and
                    // NOTPATCOPY change nothing (oracle emfpat)
                    let rop3 = (rd.u32(r + 32)? >> 16) & 0xff;
                    if (rop3 == 0xf0 || rop3 == 0x00 || rop3 == 0xff) && p.dc.brush.style != 1 && p.emit.room(0) {
                        let fill = w.sym(&color_of(p.dc.brush.color))?;
                        p.emit.plain(Item::Rect {
                            x: min(x0, x1),
                            y: min(y0, y1),
                            w: abs(x1 - x0),
                            h: abs(y1 - y0),
                            fill,
                            alpha: OptF64::NONE,
                        });
                    }
                }
            }
            // ALPHABLEND
            114 => {
                let (xd, yd, cx, cy) = (rd.fi32(r + 16)?, rd.fi32(r + 20)?, rd.fi32(r + 24)?, rd.fi32(r + 28)?);
                let alpha_fmt = rd.at(r + 35);
                let off_bmi = i64::from(rd.u32(r + 76)?);
                let cb_bmi = rd.u32(r + 80)?;
                let off_bits = i64::from(rd.u32(r + 84)?);
                let cb_bits = rd.u32(r + 88)?;
                let img = p.dib(pos + off_bmi, cb_bmi, pos + off_bits, cb_bits, alpha_fmt == Some(1))?;
                if let Some(img) = img {
                    p.blit(img, xd, yd, cx, cy, false, w)?;
                }
            }
            // STRETCHDIBITS
            81 => {
                let (xd, yd) = (rd.fi32(r + 16)?, rd.fi32(r + 20)?);
                let off_bmi = i64::from(rd.u32(r + 40)?);
                let cb_bmi = rd.u32(r + 44)?;
                let off_bits = i64::from(rd.u32(r + 48)?);
                let cb_bits = rd.u32(r + 52)?;
                let (cx, cy) = (rd.fi32(r + 64)?, rd.fi32(r + 68)?);
                let img = p.dib(pos + off_bmi, cb_bmi, pos + off_bits, cb_bits, false)?;
                if let Some(img) = img {
                    p.blit(img, xd, yd, cx, cy, true, w)?;
                }
            }
            _ => {}
        }
        if p.emit.truncated {
            break;
        }
        pos += size;
    }
    Ok(())
}

/// `EXTTEXTOUTW` (emf.ts:337–372). `Ok(false)`: the drawing stops (a limit).
fn ext_text_out_w(
    p: &mut EmfPlayer<'_, '_>,
    pos: i64,
    r: i64,
    raster_text: bool,
    faces: &mut dyn FaceLookup,
    w: &mut ResWriter<'_>,
) -> LResult<bool> {
    let rd = p.rd;
    let ref_x = rd.fi32(r + 28)?;
    let ref_y = rd.fi32(r + 32)?;
    let n_chars = rd.u32(r + 36)?;
    let off_str = i64::from(rd.u32(r + 40)?);
    let off_dx = rd.u32(r + 64)?;
    // ETO_GLYPH_INDEX: the "string" is glyph indices, not characters — Word draws the glyphs
    // without extractable text (outlines here)
    let by_gid = (rd.u32(r + 44)? & 0x10) != 0;
    rd.check(pos + off_str, u64::from(n_chars), 2)?;
    let unit = |k: i64| rd.u16(pos + off_str + k * 2);
    let n = i64::from(n_chars);
    if !by_gid {
        // `!text.trim()` without building the string
        let mut blank = true;
        for k in 0..n {
            if !uo_jsnum::text::is_js_space_unit(unit(k)?) {
                blank = false;
                break;
            }
        }
        if blank {
            return Ok(true);
        }
    }
    if by_gid && n_chars == 0 {
        return Ok(true);
    }
    let f = p.dc.font;
    let family: &[u16] = match p.faces_names.get(f.face) {
        Some(n) if !n.is_empty() => n,
        _ => &ARIAL,
    };
    let family = family.to_vec();
    let face = faces.face(w.res, &family, f.weight >= 600.0, f.italic)?;
    let font = w.res.face(face).and_then(|fr| fr.face.clone()).ok_or(LayoutError::Internal("metafile: face without font"))?;
    let fm = *font.metrics();
    let upem = f64::from(fm.units_per_em);
    // negative height = em (body); positive = cell height
    let cell = {
        let c = abs(f.height);
        if uo_jsnum::conv::truthy(c) { c } else { 12.0 }
    };
    let em_log = if f.height < 0.0 { cell } else { (cell * upem) / max(1.0, f64::from(fm.us_win_ascent) + f64::from(fm.us_win_descent)) };
    let em_tw = p.len_tw(em_log);
    if em_tw < 1.0 {
        return Ok(true);
    }
    let size_pt = em_tw / 20.0;
    let ta = p.dc.text_align;
    let [px, py] = if ta & 1 != 0 { p.to_page(p.dc.cur[0], p.dc.cur[1]) } else { p.to_page(ref_x, ref_y) };
    let win_asc = if fm.us_win_ascent != 0 { f64::from(fm.us_win_ascent) } else { f64::from(fm.ascender) };
    let win_desc = if fm.us_win_descent != 0 { f64::from(fm.us_win_descent) } else { -f64::from(fm.descender) };
    let asc_tw = (win_asc / upem) * em_tw;
    let desc_tw = (win_desc / upem) * em_tw;
    let va = ta & 24;
    let base_y = if va == 24 {
        py
    } else if va == 8 {
        py - desc_tw
    } else {
        py + asc_tw
    };
    if off_dx != 0 {
        rd.check(pos + i64::from(off_dx), u64::from(n_chars), 4)?;
    }
    if !p.emit.room(u64::from(n_chars)) {
        return Ok(false);
    }
    // advances: the record's Dx vector (logical) or the font's metrics
    let mut text: Vec<u16> = Vec::with_capacity(usize::try_from(n_chars).unwrap_or(0));
    let mut dxs: Vec<f64> = Vec::with_capacity(text.capacity());
    for k in 0..n {
        let u = unit(k)?;
        text.push(u);
        let d = if off_dx != 0 {
            p.len_tw(rd.fi32(pos + i64::from(off_dx) + k * 4)?)
        } else {
            let gid = if by_gid { u32::from(u) } else { font.glyph_id(u32::from(u)) };
            (f64::from(font.advance_width(gid)) * em_tw) / upem
        };
        dxs.push(d);
    }
    let total = dxs.iter().fold(0.0, |a, b| a + b);
    let ha = ta & 6;
    let mut x = if ha == 2 {
        px - total
    } else if ha == 6 {
        px - total / 2.0
    } else {
        px
    };
    let mut glyphs = Vec::with_capacity(text.len());
    for (k, &u) in text.iter().enumerate() {
        glyphs.push((u32::from(u), x, by_gid.then_some(u32::from(u))));
        x += dxs.get(k).copied().unwrap_or(f64::NAN);
    }
    let color = w.sym(&color_of(p.dc.text_color))?;
    let head = GlyphRunHead {
        face,
        color,
        size_pt,
        y: base_y,
        text: &text,
        scale_x: OptF64::NONE,
        alpha: OptF64::NONE,
        fx: None,
        flags: RunFlags::default().with(RunFlags::AS_PATH, by_gid || raster_text),
    };
    p.emit.glyphs(head, glyphs)?;
    Ok(true)
}

const ARIAL: [u16; 5] = [0x41, 0x72, 0x69, 0x61, 0x6c];

// ───────────────────────── EMF+ (image subset) ─────────────────────────

const EMF_PLUS: u32 = 0x2b46_4d45; // "EMF+"

/// `emfRastersDest(b)`: the figure copies bits (BITBLT/STRETCHBLT with a bitmap, STRETCHDIBITS)
/// mixing the SOURCE with the DESTINATION.
fn emf_rasters_dest(b: &[u8]) -> bool {
    let rd = Rd { b };
    let len = rd.len();
    let mut pos: i64 = 0;
    let mut guard = 0;
    while pos + 8 <= len && guard < 200_000 {
        let (Ok(ty), Ok(size)) = (rd.u32(pos), rd.u32(pos + 4)) else { break };
        let size = i64::from(size);
        if size < 8 || pos + size > len || ty == 14 {
            break;
        }
        let at: i64 = if ty == 76 || ty == 77 {
            40
        } else if ty == 81 {
            68
        } else {
            -1
        };
        let with_bits = ty == 81 || (pos + 92 <= len && rd.u32(pos + 88).is_ok_and(|v| v > 0));
        if at >= 0
            && with_bits
            && pos + at + 4 <= len
            && let Ok(v) = rd.u32(pos + at)
        {
            let rop3 = (v >> 16) & 0xff;
            let uses_d = ((rop3 & 0xaa) >> 1) != (rop3 & 0x55);
            let uses_s = ((rop3 & 0xcc) >> 2) != (rop3 & 0x33);
            if uses_d && uses_s {
                return true;
            }
        }
        pos += size;
        guard += 1;
    }
    false
}

/// `emfPlusOnly(b)`: there are EMF+ records and the EMF+ header is NOT "dual".
fn emf_plus_only(b: &[u8]) -> bool {
    let rd = Rd { b };
    let len = rd.len();
    let mut pos: i64 = 0;
    let mut guard = 0;
    while pos + 16 <= len && guard < 64 {
        let (Ok(ty), Ok(size)) = (rd.u32(pos), rd.u32(pos + 4)) else { return false };
        if size < 8 {
            return false;
        }
        if ty == 70 && rd.u32(pos + 12).is_ok_and(|v| v == EMF_PLUS) && pos + 28 <= len {
            // 1st EMF+ record = header (0x4001); flag bit 0 = dual
            return rd.u16(pos + 16).is_ok_and(|v| v == 0x4001) && rd.u16(pos + 18).is_ok_and(|v| v & 1 == 0);
        }
        // the EMF+ header comes right after the EMF one
        if ty != 1 {
            return false;
        }
        pos += i64::from(size);
        guard += 1;
    }
    false
}

type Mat = [f64; 6];

fn mat_mul(m: Mat, n: Mat) -> Mat {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

/// An `InlineImage` the EMF+ player built (`emfPlusBitmap`): bytes and extension.
struct PlusBitmap {
    data: Vec<u8>,
    ext: &'static str,
}

/// `emfPlusBitmap(b, o, end)`: the pixels of an `EmfPlusBitmap` (BMP top-down; PNG/JPEG as is).
/// `Ok(None)` = `null`; a bitmap above the uo-image pixel limit is skipped with a warning (the TS
/// allocates it, or throws a `RangeError`).
fn emf_plus_bitmap(b: &[u8], o: i64, end: i64, warn: &mut bool) -> LResult<Option<PlusBitmap>> {
    let rd = Rd { b };
    if o + 20 > end {
        return Ok(None);
    }
    let w = rd.i32(o)?;
    let h = rd.i32(o + 4)?;
    let stride = rd.i32(o + 8)?;
    let fmt = rd.u32(o + 12)?;
    let kind = rd.u32(o + 16)?;
    let data = subarray(b, o + 20, end);
    if kind == 1 {
        // compressed: the file itself (PNG, JPEG, GIF, BMP, TIFF)
        let ext = match data.first() {
            Some(0x89) => "png",
            Some(0xff) => "jpeg",
            Some(0x47) => "gif",
            Some(0x42) => "bmp",
            _ => "tiff",
        };
        return Ok(Some(PlusBitmap { data: data.to_vec(), ext }));
    }
    let bpp = (fmt >> 8) & 0xff;
    let dlen = i64::try_from(data.len()).unwrap_or(i64::MAX);
    if w <= 0 || h <= 0 || (bpp != 32 && bpp != 24) || (fmt & 0x10000) != 0 || i64::from(stride) * i64::from(h) > dlen {
        return Ok(None);
    }
    if u64::from(w.unsigned_abs()) * u64::from(h.unsigned_abs()) > uo_image::ImageLimits::DEFAULT.max_px {
        *warn = true;
        return Ok(None);
    }
    let alpha = (fmt & 0x40000) != 0;
    let premul = (fmt & 0x80000) != 0;
    // BMP V3 (56 bytes, BITFIELDS) with a negative height = rows top-down
    let hdr: i64 = if bpp == 32 { 56 } else { 40 };
    let row_out = ((i64::from(w) * i64::from(bpp) + 31) / 32) * 4;
    let total = usize::try_from(14 + hdr + row_out * i64::from(h)).map_err(|_| OOB)?;
    let mut bmp = vec![0u8; total];
    if let Some(m) = bmp.get_mut(0..2) {
        m.copy_from_slice(&[0x42, 0x4d]);
    }
    put_u32(&mut bmp, 2, low32(total));
    put_u32(&mut bmp, 10, u32::try_from(14 + hdr).unwrap_or(0));
    put_u32(&mut bmp, 14, u32::try_from(hdr).unwrap_or(0));
    put_i32(&mut bmp, 18, w);
    put_i32(&mut bmp, 22, h.wrapping_neg());
    put_u16(&mut bmp, 26, 1);
    put_u16(&mut bmp, 28, u16::try_from(bpp).unwrap_or(0));
    if bpp == 32 {
        put_u32(&mut bmp, 30, 3);
        put_u32(&mut bmp, 54, 0x00ff_0000);
        put_u32(&mut bmp, 58, 0x0000_ff00);
        put_u32(&mut bmp, 62, 0x0000_00ff);
        put_u32(&mut bmp, 66, if alpha { 0xff00_0000 } else { 0 });
    }
    for y in 0..i64::from(h) {
        let src = y * i64::from(stride);
        let dst = 14 + hdr + y * row_out;
        set_at(&mut bmp, subarray(data, src, src + i64::from(stride).min(row_out)), dst)?;
        if premul {
            let mut k = dst;
            while k < dst + i64::from(w) * 4 {
                if let Ok(ku) = usize::try_from(k) {
                    unpremul(&mut bmp, ku);
                }
                k += 4;
            }
        }
    }
    Ok(Some(PlusBitmap { data: bmp, ext: "bmp" }))
}

/// An EMF+ image object: the bitmap (`null` when not drawable) and its declared size; the
/// `ImageId` is given at the first draw (one per object, RQ3).
struct PlusImage {
    img: Option<PlusBitmap>,
    id: Option<ImageId>,
    w: f64,
    h: f64,
}

/// `{ id, total, parts, got }` of a continued object.
struct Pending {
    id: u16,
    total: u32,
    data: Vec<u8>,
}

/// `EmfPlusPlayer`: image records of [MS-EMFPLUS] (Image objects, raw or compressed, also in
/// continued records; DrawImage and DrawImagePoints) with the world/page transforms and
/// Save/Restore. Device coordinates = pixels of the EMF's reference device.
struct EmfPlusPlayer<'b> {
    b: &'b [u8],
    world: Mat,
    page_scale: f64,
    unit: u16,
    dpi: f64,
    saved: FxHashMap<u32, (Mat, f64, u16)>,
    images: FxHashMap<u16, PlusImage>,
    pending: Option<Pending>,
    too_big: bool,
}

impl<'b> EmfPlusPlayer<'b> {
    fn new(b: &'b [u8]) -> EmfPlusPlayer<'b> {
        EmfPlusPlayer {
            b,
            world: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            page_scale: 1.0,
            unit: 2,
            dpi: 96.0,
            saved: FxHashMap::default(),
            images: FxHashMap::default(),
            pending: None,
            too_big: false,
        }
    }

    /// page unit → pixels
    fn unit_px(&self) -> f64 {
        match self.unit {
            3 => self.dpi / 72.0,
            4 => self.dpi,
            5 => self.dpi / 300.0,
            6 => self.dpi / 25.4,
            // world, display, pixel
            _ => 1.0,
        }
    }

    fn dev(&self, x: f64, y: f64) -> [f64; 2] {
        let m = self.world;
        let k = self.page_scale * self.unit_px();
        [(x * m[0] + y * m[2] + m[4]) * k, (x * m[1] + y * m[3] + m[5]) * k]
    }

    /// `image(id, b, o, end)` (bitmap only).
    fn image(&mut self, id: u16, b: &[u8], o: i64, end: i64) -> LResult<()> {
        let rd = Rd { b };
        if o + 16 > end || rd.u32(o + 4)? != 1 {
            self.images.insert(id, PlusImage { img: None, id: None, w: 0.0, h: 0.0 });
            return Ok(());
        }
        let img = emf_plus_bitmap(b, o + 8, end, &mut self.too_big)?;
        let (w, h) = (rd.fi32(o + 8)?, rd.fi32(o + 12)?);
        self.images.insert(id, PlusImage { img, id: None, w, h });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        id: u16,
        src_unit: i32,
        src: [f64; 4],
        p0: [f64; 2],
        p1: [f64; 2],
        p2: [f64; 2],
        e: &mut EmfPlayer<'_, '_>,
        w: &mut ResWriter<'_>,
    ) -> LResult<()> {
        let [d0x, d0y] = self.dev(p0[0], p0[1]);
        let [d1x, _] = self.dev(p1[0], p1[1]);
        let [_, d2y] = self.dev(p2[0], p2[1]);
        let Some(im) = self.images.get_mut(&id) else { return Ok(()) };
        let Some(bm) = &im.img else { return Ok(()) };
        let mut crop = None;
        if src_unit == 2 && im.w > 0.0 && im.h > 0.0 {
            let c = Ltrb { l: src[0] / im.w, t: src[1] / im.h, r: 1.0 - (src[0] + src[2]) / im.w, b: 1.0 - (src[1] + src[3]) / im.h };
            if abs(c.l) > 1e-3 || abs(c.t) > 1e-3 || abs(c.r) > 1e-3 || abs(c.b) > 1e-3 {
                crop = Some(c);
            }
        }
        // the `emit` callback of renderEmf
        let (dest, fx0, fy0, sx, sy) = (e.dest, e.fx0, e.fy0, e.sx, e.sy);
        let (px0, py0) = (dest.x + (d0x - fx0) * sx, dest.y + (d0y - fy0) * sy);
        let (px1, py1) = (dest.x + (d1x - fx0) * sx, dest.y + (d2y - fy0) * sy);
        if !e.emit.room(0) {
            return Ok(());
        }
        let image = match im.id {
            Some(i) => i,
            None => {
                let i = w.derived_image(bm.data.clone(), bm.ext, 0.0, 0.0)?;
                im.id = Some(i);
                i
            }
        };
        e.emit.plain(image_item(min(px0, px1), min(py0, py1), abs(px1 - px0), abs(py1 - py0), image, crop, px1 < px0, py1 < py0));
        Ok(())
    }

    fn play(&mut self, start: i64, end: i64, e: &mut EmfPlayer<'_, '_>, w: &mut ResWriter<'_>) -> LResult<()> {
        let rd = Rd { b: self.b };
        let mut q = start;
        while q + 12 <= end {
            let ty = rd.u16(q)?;
            let flags = rd.u16(q + 2)?;
            let size = i64::from(rd.u32(q + 4)?);
            let dsz = i64::from(rd.u32(q + 8)?);
            if size < 12 || q + size > end {
                break;
            }
            let d = q + 12;
            let d_end = (q + size).min(d + dsz);
            match ty {
                // header
                0x4001 => {
                    let v = f64::from(rd.u32(d + 8)?);
                    self.dpi = if uo_jsnum::conv::truthy(v) { v } else { 96.0 };
                }
                // Object (Image only)
                0x4008 => {
                    let id = flags & 0xff;
                    if ((flags >> 8) & 0x7f) == 5 {
                        if flags & 0x8000 != 0 {
                            // continued: TotalObjectSize + pieces in the following records
                            let total = rd.u32(d)?;
                            if self.pending.as_ref().is_none_or(|p| p.id != id) {
                                self.pending = Some(Pending { id, total, data: Vec::new() });
                            }
                            let part = subarray(self.b, d + 4, d_end);
                            let done = match &mut self.pending {
                                Some(pd) => {
                                    pd.data.extend_from_slice(part);
                                    u64::try_from(pd.data.len()).unwrap_or(u64::MAX) >= u64::from(pd.total)
                                }
                                None => false,
                            };
                            if done && let Some(pd) = self.pending.take() {
                                let all = pd.data;
                                let n = i64::try_from(all.len()).unwrap_or(i64::MAX);
                                self.image(id, &all, 0, n)?;
                            }
                        } else {
                            let b = self.b;
                            self.image(id, b, d, d_end)?;
                        }
                    }
                }
                // DrawImage
                0x401a => {
                    let id = flags & 0xff;
                    let src_unit = rd.i32(d + 4)?;
                    let src = [rd.f32(d + 8)?, rd.f32(d + 12)?, rd.f32(d + 16)?, rd.f32(d + 20)?];
                    let r = if flags & 0x4000 != 0 {
                        [rd.fi16(d + 24)?, rd.fi16(d + 26)?, rd.fi16(d + 28)?, rd.fi16(d + 30)?]
                    } else {
                        [rd.f32(d + 24)?, rd.f32(d + 28)?, rd.f32(d + 32)?, rd.f32(d + 36)?]
                    };
                    self.draw(id, src_unit, src, [r[0], r[1]], [r[0] + r[2], r[1]], [r[0], r[1] + r[3]], e, w)?;
                }
                // DrawImagePoints (upper left, upper right, lower left)
                0x401b => {
                    let id = flags & 0xff;
                    let src_unit = rd.i32(d + 4)?;
                    let src = [rd.f32(d + 8)?, rd.f32(d + 12)?, rd.f32(d + 16)?, rd.f32(d + 20)?];
                    // relative points
                    if flags & 0x0800 == 0 {
                        let pt = |k: i64| -> LResult<[f64; 2]> {
                            if flags & 0x4000 != 0 {
                                Ok([rd.fi16(d + 28 + k * 4)?, rd.fi16(d + 30 + k * 4)?])
                            } else {
                                Ok([rd.f32(d + 28 + k * 8)?, rd.f32(d + 32 + k * 8)?])
                            }
                        };
                        let (a, b, c) = (pt(0)?, pt(1)?, pt(2)?);
                        self.draw(id, src_unit, src, a, b, c, e, w)?;
                    }
                }
                0x402a => self.world = [rd.f32(d)?, rd.f32(d + 4)?, rd.f32(d + 8)?, rd.f32(d + 12)?, rd.f32(d + 16)?, rd.f32(d + 20)?],
                0x402b => self.world = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                0x402c..=0x402f => {
                    let m: Mat = match ty {
                        0x402c => [rd.f32(d)?, rd.f32(d + 4)?, rd.f32(d + 8)?, rd.f32(d + 12)?, rd.f32(d + 16)?, rd.f32(d + 20)?],
                        0x402d => [1.0, 0.0, 0.0, 1.0, rd.f32(d)?, rd.f32(d + 4)?],
                        0x402e => [rd.f32(d)?, 0.0, 0.0, rd.f32(d + 4)?, 0.0, 0.0],
                        _ => {
                            let a = (rd.f32(d)? * std::f64::consts::PI) / 180.0;
                            let (c, s) = (uo_jsnum::math::cos(a), uo_jsnum::math::sin(a));
                            [c, s, -uo_jsnum::math::sin(a), c, 0.0, 0.0]
                        }
                    };
                    // flag A (0x2000): after the current one; else before
                    self.world = if flags & 0x2000 != 0 { mat_mul(self.world, m) } else { mat_mul(m, self.world) };
                }
                0x4030 => {
                    self.unit = flags & 0xff;
                    let s = rd.f32(d)?;
                    self.page_scale = if uo_jsnum::conv::truthy(s) { s } else { 1.0 };
                }
                0x4025 => {
                    let k = rd.u32(d)?;
                    self.saved.insert(k, (self.world, self.page_scale, self.unit));
                }
                0x4026 => {
                    if let Some(&(wo, ps, un)) = self.saved.get(&rd.u32(d)?) {
                        self.world = wo;
                        self.page_scale = ps;
                        self.unit = un;
                    }
                }
                _ => {}
            }
            if self.too_big {
                self.too_big = false;
                e.emit.truncated = true;
            }
            if e.emit.truncated {
                break;
            }
            q += size;
        }
        Ok(())
    }
}

// ───────────────────────── WMF ─────────────────────────

/// Windows-1252 0x80–0x9F (the rest matches Latin-1).
const CP1252: [u16; 32] = [
    0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018,
    0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
];

/// `isWmf(b)`: the "placeable" (Aldus) header or the memory/disk one (type 1/2, 9 words).
#[must_use]
pub fn is_wmf(b: &[u8]) -> bool {
    if b.len() < 40 {
        return false;
    }
    if b.get(0..4) == Some(&[0xd7, 0xcd, 0xc6, 0x9a]) {
        return true;
    }
    matches!(b.get(0..4), Some([1 | 2, 0, 9, 0]))
}

#[derive(Clone, Copy, Debug)]
struct WFont {
    height: f64,
    weight: f64,
    italic: bool,
    face: usize,
    /// `bytes[p + 13]` (`undefined` past the end)
    charset: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
enum WObj {
    Pen(Pen),
    Brush(Brush),
    Font(WFont),
    Other,
}

#[derive(Clone, Copy, Debug)]
struct Dc {
    org: [f64; 2],
    ext: [f64; 2],
    pen: Pen,
    brush: Brush,
    font: WFont,
    text_color: u32,
    text_align: u16,
    cur: [f64; 2],
}

struct WmfPlayer<'p> {
    dest: MetafileFrame,
    dc: Dc,
    faces_names: Vec<Vec<u16>>,
    emit: Emit<'p>,
}

impl WmfPlayer<'_> {
    fn sx(&self) -> f64 {
        self.dest.w / self.dc.ext[0]
    }

    fn sy(&self) -> f64 {
        self.dest.h / self.dc.ext[1]
    }

    fn to_page(&self, x: f64, y: f64) -> [f64; 2] {
        [self.dest.x + (x - self.dc.org[0]) * self.sx(), self.dest.y + (y - self.dc.org[1]) * self.sy()]
    }

    /// `stroke(cmds, fill)`.
    fn stroke(&mut self, cmds: &[PathCmd], fill: bool, w: &mut ResWriter<'_>) -> LResult<()> {
        let dc = self.dc;
        let f = fill && dc.brush.style != 1;
        let st = dc.pen.style != 5;
        if cmds.is_empty() || !(f || st) {
            return Ok(());
        }
        if !self.emit.room(len64(cmds.len())) {
            return Ok(());
        }
        let fill = if f { Some(w.sym(&color_of(dc.brush.color))?) } else { None };
        let stroke = if st {
            let a = abs(dc.pen.width * self.sx());
            let a = if uo_jsnum::conv::truthy(a) { a } else { 0.0 };
            Some(Stroke { color: w.sym(&color_of(dc.pen.color))?, width_pt: max(0.25, a / 20.0), dash: None })
        } else {
            None
        };
        self.emit.path(cmds, fill, stroke, false)
    }

    /// `poly(o, n, closed)`.
    fn poly(&self, rd: Rd<'_>, o: i64, n: i16, closed: bool) -> LResult<Vec<PathCmd>> {
        let n = i64::from(n);
        let mut c = Vec::with_capacity(usize::try_from(n.max(0)).unwrap_or(0) + 1);
        for k in 0..n {
            let [x, y] = self.to_page(rd.fi16(o + 4 * k)?, rd.fi16(o + 4 * k + 2)?);
            c.push(if k != 0 { PathCmd::L { x, y } } else { PathCmd::M { x, y } });
        }
        if closed {
            c.push(PathCmd::Z);
        }
        Ok(c)
    }

    /// `text(x, y, raw, dx)`: `dx` is the offset of the Dx array.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        rd: Rd<'_>,
        x: f64,
        y: f64,
        raw: &[u8],
        dx: Option<i64>,
        faces: &mut dyn FaceLookup,
        w: &mut ResWriter<'_>,
    ) -> LResult<bool> {
        let f = self.dc.font;
        let sym = f.charset == Some(2);
        let s: Vec<u16> = raw
            .iter()
            .map(|&b| {
                if sym {
                    0xf000 + u16::from(b)
                } else if (0x80..0xa0).contains(&b) {
                    CP1252.get(usize::from(b - 0x80)).copied().unwrap_or(u16::from(b))
                } else {
                    u16::from(b)
                }
            })
            .collect();
        if s.iter().all(|&u| uo_jsnum::text::is_js_space_unit(u)) {
            return Ok(true);
        }
        let family: Vec<u16> = match self.faces_names.get(f.face) {
            Some(n) if !n.is_empty() => n.clone(),
            _ => ARIAL.to_vec(),
        };
        let face = faces.face(w.res, &family, f.weight >= 600.0, f.italic)?;
        let font = w.res.face(face).and_then(|fr| fr.face.clone()).ok_or(LayoutError::Internal("metafile: face without font"))?;
        let fm = *font.metrics();
        let upem = f64::from(fm.units_per_em);
        // negative height = em (body); positive = cell height
        let cell = {
            let c = abs(f.height);
            if uo_jsnum::conv::truthy(c) { c } else { 12.0 }
        };
        let em_log =
            if f.height < 0.0 { cell } else { (cell * upem) / max(1.0, f64::from(fm.us_win_ascent) + f64::from(fm.us_win_descent)) };
        let em_tw = abs(em_log * self.sy());
        if em_tw < 1.0 {
            return Ok(true);
        }
        let ta = self.dc.text_align;
        let [px, py] = if ta & 1 != 0 { self.to_page(self.dc.cur[0], self.dc.cur[1]) } else { self.to_page(x, y) };
        let win_asc = if fm.us_win_ascent != 0 { f64::from(fm.us_win_ascent) } else { f64::from(fm.ascender) };
        let win_desc = if fm.us_win_descent != 0 { f64::from(fm.us_win_descent) } else { -f64::from(fm.descender) };
        let asc = (win_asc / upem) * em_tw;
        let desc = (win_desc / upem) * em_tw;
        let va = ta & 24;
        let base_y = if va == 24 {
            py
        } else if va == 8 {
            py - desc
        } else {
            py + asc
        };
        let mut adv = Vec::with_capacity(s.len());
        for (k, &u) in s.iter().enumerate() {
            let a = match dx {
                Some(o) => abs(rd.fi16(o + 2 * i64::try_from(k).unwrap_or(i64::MAX))? * self.sx()),
                None => (f64::from(font.advance_width(font.glyph_id(u32::from(u)))) * em_tw) / upem,
            };
            adv.push(a);
        }
        let total = adv.iter().fold(0.0, |a, b| a + b);
        let ha = ta & 6;
        let mut gx = if ha == 2 {
            px - total
        } else if ha == 6 {
            px - total / 2.0
        } else {
            px
        };
        if !self.emit.room(len64(s.len())) {
            return Ok(false);
        }
        let mut glyphs = Vec::with_capacity(s.len());
        for (k, &u) in s.iter().enumerate() {
            glyphs.push((u32::from(u), gx, None));
            gx += adv.get(k).copied().unwrap_or(f64::NAN);
        }
        let color = w.sym(&color_of(self.dc.text_color))?;
        let head = GlyphRunHead {
            face,
            color,
            size_pt: em_tw / 20.0,
            y: base_y,
            text: &s,
            scale_x: OptF64::NONE,
            alpha: OptF64::NONE,
            fx: None,
            flags: RunFlags::default(),
        };
        self.emit.glyphs(head, glyphs)?;
        if ta & 1 != 0 {
            self.dc.cur = [self.dc.cur[0] + total / self.sx(), self.dc.cur[1]];
        }
        Ok(true)
    }
}

/// `renderWmf(ctx, bytes, dest)`: the WMF drawn into `dest` (twips), appended to `out`.
pub fn render_wmf(
    bytes: &[u8],
    dest: MetafileFrame,
    limits: MetafileLimits,
    faces: &mut dyn FaceLookup,
    w: &mut ResWriter<'_>,
    out: &mut PassItems,
) -> LResult<()> {
    if !is_wmf(bytes) {
        return Ok(());
    }
    let rd = Rd { b: bytes };
    let placeable = bytes.first() == Some(&0xd7);
    let mut pos: i64 = if placeable { 22 } else { 0 };
    // default window: the box of the "placeable" header
    let bx = if placeable { [rd.fi16(6)?, rd.fi16(8)?, rd.fi16(10)?, rd.fi16(12)?] } else { [0.0, 0.0, 1.0, 1.0] };
    // header (9 words)
    pos += i64::from(rd.u16(pos + 2)?) * 2;
    let dc = Dc {
        org: [bx[0], bx[1]],
        ext: [or1(bx[2] - bx[0]), or1(bx[3] - bx[1])],
        pen: Pen { style: 0, width: 0.0, color: 0 },
        brush: Brush { style: 0, color: 0xFF_FFFF },
        font: WFont { height: -12.0, weight: 400.0, italic: false, face: 0, charset: Some(0) },
        text_color: 0,
        text_align: 0,
        cur: [0.0, 0.0],
    };
    let mut p =
        WmfPlayer { dest, dc, faces_names: vec![units("Arial")], emit: Emit { out, limits, items: 0, payload: 0, truncated: false } };
    let r = play_wmf(&mut p, rd, pos, faces, w);
    if p.emit.truncated {
        faces.warn(TRUNCATED);
    }
    r
}

fn play_wmf(p: &mut WmfPlayer<'_>, rd: Rd<'_>, mut pos: i64, faces: &mut dyn FaceLookup, w: &mut ResWriter<'_>) -> LResult<()> {
    let bytes = rd.b;
    let len = rd.len();
    let mut stack: Vec<Dc> = Vec::new();
    let mut objs: Vec<Option<WObj>> = Vec::new();
    let add = |objs: &mut Vec<Option<WObj>>, o: WObj| match objs.iter().position(Option::is_none) {
        Some(k) => {
            if let Some(slot) = objs.get_mut(k) {
                *slot = Some(o);
            }
        }
        None => objs.push(Some(o)),
    };
    let mut records: u32 = 0;
    while pos + 6 <= len {
        let size = i64::from(rd.u32(pos)?) * 2;
        let func = rd.u16(pos + 4)?;
        let q = pos + 6;
        if func == 0 || size < 6 || pos + size > len {
            break;
        }
        if records >= p.emit.limits.records {
            p.emit.truncated = true;
            break;
        }
        records += 1;
        match func {
            // SETWINDOWORG (y, x)
            0x020b => p.dc.org = [rd.fi16(q + 2)?, rd.fi16(q)?],
            // SETWINDOWEXT
            0x020c => p.dc.ext = [or1(rd.fi16(q + 2)?), or1(rd.fi16(q)?)],
            0x012e => p.dc.text_align = rd.u16(q)?,
            0x0209 => p.dc.text_color = rd.u32(q)?,
            // SAVEDC
            0x001e => stack.push(p.dc),
            // RESTOREDC
            0x0127 => {
                let k = i64::from(rd.i16(q)?);
                if k < 0 {
                    // `stack.splice(stack.length + k)[0]`: a negative start counts from the end again
                    let n = i64::try_from(stack.len()).unwrap_or(i64::MAX);
                    let start = n + k;
                    let at = usize::try_from(if start < 0 { (n + start).max(0) } else { start.min(n) }).unwrap_or(0);
                    let s = stack.get(at).copied();
                    stack.truncate(at);
                    if let Some(s) = s {
                        p.dc = s;
                    }
                }
            }
            0x02fa => {
                let pen = Pen { style: u32::from(rd.u16(q)?), width: rd.fi16(q + 2)?, color: rd.u32(q + 6)? };
                add(&mut objs, WObj::Pen(pen));
            }
            0x02fc => {
                let brush = Brush { style: u32::from(rd.u16(q)?), color: rd.u32(q + 2)? };
                add(&mut objs, WObj::Brush(brush));
            }
            // CREATEFONTINDIRECT
            0x02fb => {
                let mut face = Vec::new();
                let mut k = q + 18;
                while k < (q + 50).min(pos + size) {
                    match rd.at(k) {
                        Some(c) if c != 0 => face.push(u16::from(c)),
                        _ => break,
                    }
                    k += 1;
                }
                let height = rd.fi16(q)?;
                let weight = rd.fi16(q + 8)?;
                let italic = rd.at(q + 10) != Some(0);
                let charset = rd.at(q + 13);
                p.faces_names.push(face);
                add(&mut objs, WObj::Font(WFont { height, weight, italic, face: p.faces_names.len() - 1, charset }));
            }
            // palette, DIB brush, region
            0x00f7 | 0x0142 | 0x01f9 | 0x06ff => add(&mut objs, WObj::Other),
            // SELECTOBJECT
            0x012d => match objs.get(usize::from(rd.u16(q)?)).copied().flatten() {
                Some(WObj::Pen(pen)) => p.dc.pen = pen,
                Some(WObj::Brush(b)) => p.dc.brush = b,
                Some(WObj::Font(f)) => p.dc.font = f,
                _ => {}
            },
            // DELETEOBJECT
            0x01f0 => {
                let k = usize::from(rd.u16(q)?);
                if let Some(slot) = objs.get_mut(k) {
                    *slot = None;
                }
            }
            // MOVETO (y, x)
            0x0214 => p.dc.cur = [rd.fi16(q + 2)?, rd.fi16(q)?],
            // LINETO
            0x0213 => {
                let [x0, y0] = p.to_page(p.dc.cur[0], p.dc.cur[1]);
                p.dc.cur = [rd.fi16(q + 2)?, rd.fi16(q)?];
                let [x1, y1] = p.to_page(p.dc.cur[0], p.dc.cur[1]);
                p.stroke(&[PathCmd::M { x: x0, y: y0 }, PathCmd::L { x: x1, y: y1 }], false, w)?;
            }
            // POLYLINE, POLYGON
            0x0325 | 0x0324 => {
                let n = rd.i16(q)?;
                let closed = func == 0x0324;
                if n > 0 {
                    rd.check(q + 2, u64::from(n.unsigned_abs()), 4)?;
                }
                if !p.emit.reserve(u64::from(n.unsigned_abs()) + 1) {
                    break;
                }
                let c = p.poly(rd, q + 2, n, closed)?;
                p.stroke(&c, closed, w)?;
            }
            // RECTANGLE (bottom, right, top, left)
            0x041b => {
                let [x0, y0] = p.to_page(rd.fi16(q + 6)?, rd.fi16(q + 4)?);
                let [x1, y1] = p.to_page(rd.fi16(q + 2)?, rd.fi16(q)?);
                let c = [
                    PathCmd::M { x: x0, y: y0 },
                    PathCmd::L { x: x1, y: y0 },
                    PathCmd::L { x: x1, y: y1 },
                    PathCmd::L { x: x0, y: y1 },
                    PathCmd::Z,
                ];
                p.stroke(&c, true, w)?;
            }
            // TEXTOUT: n, string (even), y, x
            0x0521 => {
                let n = i64::from(rd.i16(q)?);
                let o = q + 2 + ((n + 1) & !1);
                let (x, y) = (rd.fi16(o + 2)?, rd.fi16(o)?);
                let raw = subarray(bytes, q + 2, q + 2 + n);
                if !p.text(rd, x, y, raw, None, faces, w)? {
                    break;
                }
            }
            // EXTTEXTOUT: y, x, n, options, [rectangle], string (even), [dx]
            0x0a32 => {
                let n = i64::from(rd.i16(q + 4)?);
                let opt = rd.u16(q + 6)?;
                let so = q + 8 + if opt & 6 != 0 { 8 } else { 0 };
                let dxo = so + ((n + 1) & !1);
                let has_dx = dxo + 2 * n <= pos + size;
                let (x, y) = (rd.fi16(q + 2)?, rd.fi16(q)?);
                let raw = subarray(bytes, so, so + n);
                if !p.text(rd, x, y, raw, has_dx.then_some(dxo), faces, w)? {
                    break;
                }
            }
            _ => {}
        }
        if p.emit.truncated {
            break;
        }
        pos += size;
    }
    Ok(())
}
