//! Floating objects (frontier F7b): the wrap frame of a line around the page's floats
//! (`wrapFrame`, `pickSeg`, `moreSegs`, `polyBandIntervals`), placing and unplacing floats
//! (`placeFloats`, `placeFloat`, `unplaceFloat`, `placeFloatsEarly`), drawing objects
//! (`paintObject`, `paintInlineDrawings`) and the module helpers of engine.ts (`sideOf`,
//! `floatRot`, `swapsBox`, `horizRef`, `vertRef`, `tbInsets`, `vertKind`, `tbFrame`,
//! `verticalMatrix`, `verticalBands`, `verticalHeight`, `verticalContent`, `textRectInsets`,
//! `alignPos`, `tbContentPlacement`, `rotation`, `floatAnchor`, `followsText`, `narrower`,
//! `markLineH`, `rotatedInkY`) — engine.ts:89–151, 3139–3732, 3748–4183.
//!
//! **Identity (LA3).** A float is its run's [`RunKey`] (`prePlaced`, `pinnedFloats`, `lateFound`);
//! a placed `ZObj` is named by its `seq` (unique in the pass); `unplaceFloat` removes the placed
//! object and the wrap box by value (removing any one of equal elements leaves the same array,
//! F7b-3).

use std::borrow::Cow;

use crate::context::FlowState;
use crate::error::{LResult, LayoutError, LimitKind};
use crate::items::{ItemSide, PassItems, PassLine};
use crate::keys::{PageIx, RunKey};
use crate::lines::{Line, LineBreaker, LineFrame};
use crate::metafile::MetafileFrame;
use crate::model::{DrawingAnchor, DrawingTextbox, JsNumber, Run};
use crate::output::{Layer, LineRef, LineStory, ObjectFrame, PlacedObject, RefPath};
use crate::para::{FieldSource, ParaEnv, PreparedPara};
use crate::table::CellContent;
use uo_jsnum::math as jm;
use uo_render::OptF64;
use uo_render::types::{Item, RunFlags};

use super::objects::{ImgSlot, ShapeLook, picture_id, push_image_item, push_metafile, push_shape_item};
use super::page::{FloatBox, SectGeom, WrapFrame, ZObj};
use super::static_layout::SBlock;
use super::{Cx, Engine, js_eq};

/// `pinnedFloats` value (engine.ts:2891, K2c): the page and y of a float placed before its anchor.
#[derive(Clone, Copy, Debug)]
pub struct Pinned {
    pub ps: PageIx,
    pub y: f64,
}

/// Which z layer of a page a float went into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZLayer {
    Behind,
    Front,
    HfBehind,
    HfFront,
}

/// `FloatReceipt` (engine.ts:4110): a placed float, to undo it.
#[derive(Clone, Debug)]
pub struct FloatReceipt {
    pub ps: PageIx,
    pub run: RunKey,
    /// `target` (`None` = `[]`: a hidden float, a float in a cell)
    pub target: Option<ZLayer>,
    /// the `seq` of the `ZObj` (unique in the pass)
    pub zseq: f64,
    pub obj: PlacedObject,
    /// the object went into `ps.page.objects`
    pub obj_placed: bool,
    pub float_box: Option<FloatBox>,
    /// the box went into `ps.floats`
    pub box_placed: bool,
    /// lists/SEQ/notes before the placement (a textbox): restored by the unplace
    pub flow: Option<FlowState>,
    /// the cell items of the float (`zobj.items` of a float in a cell)
    pub cell_items: Option<PassItems>,
}

/// `WRAPPING` (engine.ts:4105): the wraps that push lines away.
#[must_use]
pub fn wrapping(w: &str) -> bool {
    matches!(w, "square" | "tight" | "through" | "topAndBottom")
}

/// `ABSOLUTE_V` (engine.ts:4093): vertical anchors independent of the text flow.
#[must_use]
pub fn absolute_v(v: &str) -> bool {
    matches!(v, "page" | "margin" | "topMargin" | "bottomMargin" | "insideMargin" | "outsideMargin")
}

/// `MIN_GAP` (engine.ts:4107): 18 pt beside an object to receive text (twips; oracle wrapsides).
const MIN_GAP: f64 = 359.5;

/// `VERT_PITCH` (engine.ts:3849).
const VERT_PITCH: f64 = 1.0 / 36.8;

/// `n ?? 0` of a model number (`null` is nullish).
fn n0(v: Option<JsNumber>) -> f64 {
    v.and_then(JsNumber::as_f64).unwrap_or(0.0)
}

/// `floatAnchor(run)` (engine.ts:4123).
#[must_use]
pub fn float_anchor(run: &Run) -> Option<&DrawingAnchor> {
    let e = run.ext()?;
    e.image
        .as_ref()
        .and_then(|x| x.anchor.as_deref())
        .or_else(|| e.shape.as_ref().and_then(|x| x.anchor.as_deref()))
        .or_else(|| e.group.as_ref().and_then(|x| x.anchor.as_deref()))
        .or_else(|| e.textbox.as_ref().and_then(|x| x.anchor.as_deref()))
}

/// `followsText(a)` (engine.ts:4128).
#[must_use]
pub fn follows_text(a: Option<&DrawingAnchor>) -> bool {
    a.is_some_and(|a| a.simple_pos.is_none() && !absolute_v(&a.pos_v.relative_from))
}

/// `run.image?.widthTwips ?? run.shape?.widthTwips ?? run.group?.widthTwips ?? run.textbox?.widthTwips
/// ?? 0` and the height alike.
#[must_use]
pub fn float_size(run: &Run) -> (f64, f64) {
    let Some(e) = run.ext() else { return (0.0, 0.0) };
    let w = e
        .image
        .as_ref()
        .map(|x| x.width_twips)
        .or_else(|| e.shape.as_ref().map(|x| x.width_twips))
        .or_else(|| e.group.as_ref().map(|x| x.width_twips))
        .or_else(|| e.textbox.as_ref().map(|x| x.width_twips));
    let h = e
        .image
        .as_ref()
        .map(|x| x.height_twips)
        .or_else(|| e.shape.as_ref().map(|x| x.height_twips))
        .or_else(|| e.group.as_ref().map(|x| x.height_twips))
        .or_else(|| e.textbox.as_ref().map(|x| x.height_twips));
    (n0(w), n0(h))
}

/// `sideOf(v, even, axis)` (engine.ts:3748): on an even page "inside"/"outside" swap sides.
#[must_use]
pub fn side_of(v: Option<&str>, even: bool, h: bool) -> Option<&str> {
    let v = v?;
    if !even || v.is_empty() {
        return Some(v);
    }
    Some(match v {
        "inside" => {
            if h {
                "right"
            } else {
                "bottom"
            }
        }
        "outside" => {
            if h {
                "left"
            } else {
                "top"
            }
        }
        "insideMargin" => {
            if h {
                "rightMargin"
            } else {
                "bottomMargin"
            }
        }
        "outsideMargin" => {
            if h {
                "leftMargin"
            } else {
                "topMargin"
            }
        }
        _ => v,
    })
}

/// `floatRot(run)` (engine.ts:3760).
fn float_rot(run: &Run) -> f64 {
    let Some(e) = run.ext() else { return 0.0 };
    let tb = e.textbox.as_deref();
    tb.and_then(|t| t.rot.and_then(JsNumber::as_f64))
        .or_else(|| tb.and_then(|t| t.shape.as_ref()).and_then(|s| s.rot.and_then(JsNumber::as_f64)))
        .or_else(|| e.shape.as_ref().and_then(|s| s.style.rot.and_then(JsNumber::as_f64)))
        .or_else(|| e.image.as_ref().and_then(|i| i.style.as_ref()).and_then(|s| s.rot.and_then(JsNumber::as_f64)))
        .or_else(|| e.group.as_ref().and_then(|g| g.rot.and_then(JsNumber::as_f64)))
        .unwrap_or(0.0)
}

/// `swapsBox(rot)` (engine.ts:3766).
#[must_use]
pub fn swaps_box(rot: f64) -> bool {
    let r = jm::rem(jm::rem(rot, 360.0) + 360.0, 360.0);
    (45.0..135.0).contains(&r) || (225.0..315.0).contains(&r)
}

/// `horizRef(from, g, colX, colW)` (engine.ts:3771): `(x, w)`.
#[must_use]
pub fn horiz_ref(from: &str, g: &SectGeom, col_x: f64, col_w: f64) -> (f64, f64) {
    match from {
        "page" => (0.0, g.page_w),
        "margin" => (g.mar_l, g.page_w - g.mar_l - g.mar_r),
        "leftMargin" | "insideMargin" => (0.0, g.mar_l),
        "rightMargin" | "outsideMargin" => (g.page_w - g.mar_r, g.mar_r),
        _ => (col_x, col_w),
    }
}

/// `vertRef(from, g, ps, paraTop)` (engine.ts:3782): `(y, h)`.
fn vert_ref(from: &str, g: &SectGeom, body_top: f64, para_top: f64) -> (f64, f64) {
    match from {
        "page" => (0.0, g.page_h),
        "margin" => {
            let top = jm::max(g.mar_t, body_top);
            (top, g.page_h - top - g.mar_b)
        }
        "topMargin" | "insideMargin" => (0.0, g.mar_t),
        "bottomMargin" | "outsideMargin" => (g.page_h - g.mar_b, g.mar_b),
        _ => (para_top, 0.0),
    }
}

/// `alignPos(align, offset, base, span, size, pct, pageSpan, e0, e1)` (engine.ts:3917).
#[expect(clippy::too_many_arguments, reason = "the TS signature")]
#[must_use]
pub fn align_pos(
    align: Option<&str>,
    offset: Option<f64>,
    base: f64,
    span: f64,
    size: f64,
    pct: Option<f64>,
    page_span: f64,
    e0: f64,
    e1: f64,
) -> f64 {
    let align = align.filter(|a| !a.is_empty());
    if let Some(a) = align
        && (uo_jsnum::conv::truthy(e0) || uo_jsnum::conv::truthy(e1))
    {
        return align_pos(Some(a), offset, base, span, size + e0 + e1, pct, page_span, 0.0, 0.0) + e0;
    }
    if let Some(a) = align {
        return match a {
            "center" => base + (span - size) / 2.0,
            "right" | "bottom" | "outside" => base + span - size,
            _ => base,
        };
    }
    if let Some(p) = pct {
        return base + (p / 100_000.0) * page_span;
    }
    base + offset.unwrap_or(0.0)
}

/// `rotation(cx, cy, deg)` (engine.ts:4082).
#[must_use]
pub fn rotation(cx: f64, cy: f64, deg: f64) -> [f64; 6] {
    let r = (deg * std::f64::consts::PI) / 180.0;
    let (c, s) = (jm::cos(r), jm::sin(r));
    [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c]
}

/// `mulM(m, n)` (engine.ts:4043).
fn mul_m(m: &[f64; 6], n: &[f64; 6]) -> [f64; 6] {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

/// `{ l, t, r, b }`.
#[derive(Clone, Copy, Debug)]
pub struct Ins {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

/// A textbox's preset (`tb.geom`).
struct GeomRef<'g> {
    prst: &'g str,
    adj: Option<&'g crate::model::JsRecord<JsNumber>>,
}

/// `textRectInsets(g, w, h)` (engine.ts:3892).
fn text_rect_insets(g: &GeomRef<'_>, w: f64, h: f64) -> Option<Ins> {
    let names: Option<Vec<&str>> = g.adj.map(|a| a.iter().map(|(k, _)| k).collect());
    let vals: Option<Vec<f64>> = g.adj.map(|a| a.iter().map(|(_, v)| v.to_number()).collect());
    if let Some(tr) = uo_drawingml::preset_text_rect(Some(g.prst), w, h, vals.as_deref(), names.as_deref()) {
        return Some(Ins { l: tr.l, t: tr.t, r: w - tr.r, b: h - tr.b });
    }
    let adj = |k: &str, d: f64| g.adj.and_then(|a| a.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_number())).unwrap_or(d);
    match g.prst {
        "roundRect" => {
            let i = ((jm::min(w, h) * adj("adj", 16667.0)) / 100_000.0) * 0.29289;
            Some(Ins { l: i, t: i, r: i, b: i })
        }
        "ellipse" => {
            let k = 0.5 - std::f64::consts::FRAC_1_SQRT_2 / 2.0;
            Some(Ins { l: w * k, t: h * k, r: w * k, b: h * k })
        }
        "triangle" => {
            let x1 = (w * adj("adj", 50000.0)) / 200_000.0;
            Some(Ins { l: x1, t: h / 2.0, r: w / 2.0 - x1, b: 0.0 })
        }
        "diamond" => Some(Ins { l: w / 4.0, t: h / 4.0, r: w / 4.0, b: h / 4.0 }),
        _ => None,
    }
}

/// `tbInsets(tb, theme, w, h)` (engine.ts:3796).
fn tb_insets(tb: &DrawingTextbox, theme: &crate::model::Theme, w: f64, h: f64) -> Ins {
    let ins0 = match &tb.insets {
        Some(i) => Ins { l: i.l.to_number(), t: i.t.to_number(), r: i.r.to_number(), b: i.b.to_number() },
        None => Ins { l: 144.0, t: 72.0, r: 144.0, b: 72.0 },
    };
    let tr = tb.geom.as_ref().and_then(|g| text_rect_insets(&GeomRef { prst: &g.prst, adj: g.adj.as_ref() }, w, h));
    let ins = match tr {
        Some(t) => Ins { l: ins0.l + t.l, t: ins0.t + t.t, r: ins0.r + t.r, b: ins0.b + t.b },
        None => ins0,
    };
    // without a theme in the file Word's holds: lines of 1, 1.5, 2 pt (oracle tbinset)
    let lw = match &tb.line_inset {
        Some(li) => match li.w.and_then(JsNumber::as_f64) {
            Some(w) => w,
            None => match li.r#ref.filter(|r| r.truthy()) {
                Some(r) => {
                    let i = r.to_number() - 1.0;
                    let from_theme =
                        theme.line_widths_pt.as_ref().and_then(|v| super::index_of(i).and_then(|k| v.get(k))).and_then(|x| x.as_f64());
                    let dflt = super::index_of(i).and_then(|k| [1.0, 1.5, 2.0].get(k).copied());
                    from_theme.or(dflt).unwrap_or(0.75)
                }
                None => 0.0,
            },
        },
        None => tb.shape.as_ref().and_then(|s| s.line.as_ref()).map_or(0.0, |l| l.width_pt.to_number()),
    };
    let half = lw * 10.0;
    if uo_jsnum::conv::truthy(half) { Ins { l: ins.l + half, t: ins.t + half, r: ins.r + half, b: ins.b + half } } else { ins }
}

/// `TbRot` (engine.ts:3812).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TbRot {
    None,
    R90,
    R270,
    M90,
}

/// `vertKind(v)` (engine.ts:3814).
fn vert_kind(v: Option<&str>) -> TbRot {
    match v {
        Some("vert" | "eaVert") => TbRot::R90,
        Some("vert270") => TbRot::R270,
        Some("mongolianVert" | "wordArtVert" | "wordArtVertRtl") => TbRot::M90,
        _ => TbRot::None,
    }
}

/// `TbFrame` (engine.ts:3826).
#[derive(Clone, Copy, Debug)]
pub struct TbFrame {
    pub ins: Ins,
    pub iw: f64,
    pub ih: f64,
    pub rot: TbRot,
}

/// `tbFrame(tb, theme, w, h)` (engine.ts:3828).
#[must_use]
pub fn tb_frame(tb: &DrawingTextbox, theme: &crate::model::Theme, w: f64, h: f64) -> TbFrame {
    let p = tb_insets(tb, theme, w, h);
    let rot = vert_kind(tb.vert.as_deref());
    match rot {
        TbRot::R90 => TbFrame { ins: Ins { l: p.t, t: p.r, r: p.b, b: p.l }, iw: h - p.t - p.b, ih: w - p.l - p.r, rot },
        TbRot::R270 => TbFrame { ins: Ins { l: p.b, t: p.l, r: p.t, b: p.r }, iw: h - p.t - p.b, ih: w - p.l - p.r, rot },
        TbRot::M90 => TbFrame { ins: Ins { l: p.t, t: p.l, r: p.b, b: p.r }, iw: h - p.t - p.b, ih: w - p.l - p.r, rot },
        TbRot::None => TbFrame { ins: p, iw: w - p.l - p.r, ih: h - p.t - p.b, rot },
    }
}

/// `verticalMatrix(rot, x, y, w, h)` (engine.ts:3840).
fn vertical_matrix(rot: TbRot, x: f64, y: f64, w: f64, h: f64) -> [f64; 6] {
    if rot == TbRot::R270 { [0.0, -1.0, 1.0, 0.0, x, y + h] } else { [0.0, 1.0, -1.0, 0.0, x + w, y] }
}

/// `verticalBands(c)` (engine.ts:3852): `(y0, y1, add)` per line in y order.
fn vertical_bands(c: &CellContent) -> Vec<(f64, f64, f64)> {
    let mut ls: Vec<&PassLine> = c.lines.iter().collect();
    ls.sort_by(|a, b| {
        let d = a.y - b.y;
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut acc = 0.0;
    ls.iter()
        .map(|l| {
            let b = (l.y, l.y + l.height, acc);
            acc += l.height * VERT_PITCH;
            b
        })
        .collect()
}

/// `verticalHeight(c)` (engine.ts:3859).
#[must_use]
pub fn vertical_height(c: &CellContent) -> f64 {
    c.height + c.lines.iter().fold(0.0, |a, l| a + l.height * VERT_PITCH)
}

/// The y span an item is shifted by in a vertical box (`shiftOf` arguments of `verticalContent`).
fn item_y_span(it: &Item) -> Option<(f64, f64)> {
    match it {
        Item::Glyphs { y, .. } => Some((*y, *y)),
        Item::Line { y1, y2, .. } => Some((jm::min(*y1, *y2), jm::max(*y1, *y2))),
        Item::Rect { y, h, .. } => Some((*y, *y + *h)),
        Item::Image(im) => Some((im.y, im.y + im.h)),
        Item::Shape(s) => Some((s.y, s.y + s.h)),
        Item::ClipPush { y, h, .. } => y.get().map(|y| (y, y + h.or(0.0))),
        _ => None,
    }
}

/// `verticalContent(c, fr, vAnchor, x, y, w, h)` (engine.ts:3863).
#[expect(clippy::too_many_arguments, reason = "the TS signature")]
fn vertical_content(
    c: &CellContent,
    fr: &TbFrame,
    v_anchor: Option<&str>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    out: &mut PassItems,
) -> LResult<()> {
    let lh = w;
    let mut dy = fr.ins.t;
    let free = lh - fr.ins.t - fr.ins.b - vertical_height(c);
    if v_anchor == Some("ctr") && free > 0.0 {
        dy += free / 2.0;
    } else if v_anchor == Some("b") && free > 0.0 {
        dy += free;
    }
    push(out, Item::XformPush { m: Some(Box::new(vertical_matrix(fr.rot, x, y, w, h))) });
    let bands = vertical_bands(c);
    let shift_of = |y0: f64, y1: f64| -> f64 {
        let mid = (y0 + y1) / 2.0;
        let b = bands.iter().find(|q| mid >= q.0 - 0.5 && mid <= q.1 + 0.5).or_else(|| bands.iter().rev().find(|q| q.0 <= mid));
        let add = b.map_or(0.0, |q| q.2);
        if fr.rot != TbRot::M90 {
            return dy + add;
        }
        let a0 = b.map_or(y0, |q| q.0) + add;
        let a1 = b.map_or(y1, |q| q.1) + add;
        lh - (dy + a1) - a0 + add
    };
    for i in 0..c.items.len() {
        let d = match c.items.list.items.get(i).and_then(item_y_span) {
            Some((a, b)) => shift_of(a, b),
            None => dy,
        };
        out.push_shifted(&c.items, i, d, fr.ins.l).map_err(|_| cap())?;
    }
    push(out, Item::XformPop);
    Ok(())
}

fn cap() -> LayoutError {
    LayoutError::Limit(LimitKind::Capacity)
}

fn push(out: &mut PassItems, it: Item) {
    out.list.push_plain(it);
    out.side.push(ItemSide::default());
}

/// Sets `asPath` on the glyph run `out[i]` (`{ ...it, asPath: true }`).
fn set_as_path(out: &mut PassItems, i: usize) {
    if let Some(Item::Glyphs { flags, .. }) = out.list.items.get_mut(i) {
        *flags = flags.with(RunFlags::AS_PATH, true);
    }
}

/// `tbContentPlacement(tb, theme, x, y, w, h, c)` (engine.ts:4051): the lines of a textbox's content
/// in the text's frame and the matrix to the page.
fn tb_content_placement(
    tb: &DrawingTextbox,
    theme: &crate::model::Theme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    c: &CellContent,
) -> (Vec<PassLine>, [f64; 6]) {
    let fr = tb_frame(tb, theme, w, h);
    let ins = fr.ins;
    let rot = if tb.upright == Some(true) {
        0.0
    } else {
        tb.rot.and_then(JsNumber::as_f64).or_else(|| tb.shape.as_ref().and_then(|s| s.rot.and_then(JsNumber::as_f64))).unwrap_or(0.0)
    };
    let around = if uo_jsnum::conv::truthy(rot) { rotation(x + w / 2.0, y + h / 2.0, rot) } else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };
    if tb.word_art.as_deref().is_some_and(|s| !s.is_empty()) {
        return (Vec::new(), around);
    }
    if fr.rot == TbRot::None {
        let mut dy = ins.t;
        let free = h - ins.t - ins.b - c.height;
        match tb.v_anchor.as_deref() {
            Some("ctr") if free > 0.0 => dy += free / 2.0,
            Some("b") if free > 0.0 => dy += free,
            _ => {}
        }
        return (c.lines.clone(), mul_m(&around, &[1.0, 0.0, 0.0, 1.0, x + ins.l, y + dy]));
    }
    let lh = w;
    let mut dy = ins.t;
    let free = lh - ins.t - ins.b - vertical_height(c);
    match tb.v_anchor.as_deref() {
        Some("ctr") if free > 0.0 => dy += free / 2.0,
        Some("b") if free > 0.0 => dy += free,
        _ => {}
    }
    let bands = vertical_bands(c);
    let lines = c
        .lines
        .iter()
        .map(|l| {
            let add = bands.iter().find(|q| jm::abs(q.0 - l.y) < 0.01).map_or(0.0, |q| q.2);
            let sh = if fr.rot == TbRot::M90 { lh - (dy + l.y + add + l.height) - l.y } else { dy + add };
            let chars = l.chars.as_ref().map(|cs| cs.iter().map(|&(o, cx, cw)| (o, cx + ins.l, cw)).collect());
            PassLine { x: l.x + ins.l, y: l.y + sh, baseline: l.baseline + sh, chars, obj: None, ..l.clone() }
        })
        .collect();
    (lines, mul_m(&around, &vertical_matrix(fr.rot, x, y, w, h)))
}

/// `narrower(a, b)` (engine.ts:4133).
#[must_use]
pub fn narrower(a: &WrapFrame, b: &WrapFrame) -> bool {
    a.left > b.left + 0.5 || a.right < b.right - 0.5 || a.segs.as_ref().map_or(0, Vec::len) != b.segs.as_ref().map_or(0, Vec::len)
}

/// `polyBandIntervals(poly, ya, yb)` (engine.ts:89): the x projection of the polygon cut by the
/// band [ya, yb] (merged intervals).
#[must_use]
pub fn poly_band_intervals(poly: &[(f64, f64)], ya: f64, yb: f64) -> Vec<(f64, f64)> {
    let mut iv: Vec<(f64, f64)> = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let (Some(&(x0, y0)), Some(&(x1, y1))) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        let lo = jm::max(ya, jm::min(y0, y1));
        let hi = jm::min(yb, jm::max(y0, y1));
        if lo > hi {
            continue;
        }
        #[expect(clippy::float_cmp, reason = "the TS `y0 === y1`")]
        if y0 == y1 {
            iv.push((jm::min(x0, x1), jm::max(x0, x1)));
            continue;
        }
        let xa = x0 + ((lo - y0) / (y1 - y0)) * (x1 - x0);
        let xb = x0 + ((hi - y0) / (y1 - y0)) * (x1 - x0);
        iv.push((jm::min(xa, xb), jm::max(xa, xb)));
    }
    // the interior at the band's edges (even-odd rule)
    for yy in [ya, yb] {
        let mut xs: Vec<f64> = Vec::new();
        for i in 0..n {
            let (Some(&(x0, y0)), Some(&(x1, y1))) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            if (y0 <= yy) != (y1 <= yy) {
                xs.push(x0 + ((yy - y0) / (y1 - y0)) * (x1 - x0));
            }
        }
        crate::word::obj::js_sort(&mut xs, |a, b| a - b);
        let mut k = 0;
        while k + 1 < xs.len() {
            if let (Some(&a), Some(&b)) = (xs.get(k), xs.get(k + 1)) {
                iv.push((a, b));
            }
            k += 2;
        }
    }
    crate::word::obj::js_sort(&mut iv, |a, b| a.0 - b.0);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in iv {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1e-6 => last.1 = jm::max(last.1, b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// `markLineH(p)` (engine.ts:4139): the natural height of a line in the mark's font (twips).
pub(crate) fn mark_line_h(cx: &Cx<'_, '_>, p: &PreparedPara) -> LResult<f64> {
    let mm = crate::paint::face_metrics_of(cx, p.mark_face)?;
    let size = p.styles.get(p.mark_style.0 as usize).ok_or(LayoutError::Internal("markLineH: style"))?.size_pt;
    Ok(jm::round((mm.ascent + mm.descent) * size * 20.0))
}

/// `l.y`, `l.y + l.height` overlap of a box with a line (the late-float hit test).
fn box_eq(a: &FloatBox, b: &FloatBox) -> bool {
    let bits = |v: f64| v.to_bits();
    [a.x, a.y, a.w, a.h, a.dist_l, a.dist_r, a.dist_t, a.dist_b].map(bits)
        == [b.x, b.y, b.w, b.h, b.dist_l, b.dist_r, b.dist_t, b.dist_b].map(bits)
        && a.wrap == b.wrap
        && a.side == b.side
        && a.body_only == b.body_only
        && a.poly.as_ref().map(|p| p.iter().map(|q| (q.0.to_bits(), q.1.to_bits())).collect::<Vec<_>>())
            == b.poly.as_ref().map(|p| p.iter().map(|q| (q.0.to_bits(), q.1.to_bits())).collect::<Vec<_>>())
}

fn obj_eq(a: &PlacedObject, b: &PlacedObject) -> bool {
    let bits = |v: f64| v.to_bits();
    a.run == b.run
        && a.drawing == b.drawing
        && [a.x, a.y, a.w, a.h].map(bits) == [b.x, b.y, b.w, b.h].map(bits)
        && a.layer == b.layer
        && a.hf == b.hf
        && a.frame.map(|f| [f.col_x, f.col_w, f.para_top, f.page_w, f.page_h, f.mar_l, f.mar_r, f.mar_t, f.mar_b, f.body_top].map(bits))
            == b.frame.map(|f| [f.col_x, f.col_w, f.para_top, f.page_w, f.page_h, f.mar_l, f.mar_r, f.mar_t, f.mar_b, f.body_top].map(bits))
}

/// The drawing of a run (its picture, shape, group or textbox).
fn drawing_of(run: &Run) -> Option<crate::model::DrawingId> {
    let e = run.ext()?;
    e.image
        .as_ref()
        .map(|x| x.drawing)
        .or_else(|| e.shape.as_ref().map(|x| x.drawing))
        .or_else(|| e.group.as_ref().map(|x| x.drawing))
        .or_else(|| e.textbox.as_ref().map(|x| x.drawing))
}

/// The textbox content ref (`'txbx'`, `'itb'`, `'grp'`).
fn tb_ref(story: LineStory) -> LineRef {
    LineRef { story, path: RefPath(Vec::new().into()) }
}

/// A textbox's content: what `placeFloat` laid out and how the final layout goes in place.
struct TbFinal<'a> {
    blocks: Vec<SBlock<'a>>,
    at: LineRef,
    iw: f64,
    env: ParaEnv,
    ins: Ins,
}

impl Engine {
    // ───────────────────────── the wrap frame ─────────────────────────

    /// `wrapFrame(frame, frameX, y, lineH, ps)` (engine.ts:3139): the line's frame minus the
    /// wrapping floats of page `ps` that cross it.
    pub(crate) fn wrap_frame(&self, frame: &WrapFrame, frame_x: f64, y: f64, line_h: f64, ps: PageIx) -> LResult<WrapFrame> {
        let floats = &self.ps(ps)?.floats;
        self.wrap_frame_in(floats, frame, frame_x, y, line_h)
    }

    /// `wrapFrame` over a float list (a page's, or the floats of a cell, `cellPs`).
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    pub(crate) fn wrap_frame_in(&self, floats: &[FloatBox], frame: &WrapFrame, frame_x: f64, y: f64, line_h: f64) -> LResult<WrapFrame> {
        if floats.is_empty() {
            return Ok(frame.clone());
        }
        let (left, right) = (frame.left, frame.right);
        // crossing indents (right before left: Word puts one character per line): the line has no
        // width of its own and the gap is measured in the whole column (oracle hfgroup)
        let crossed = right < left;
        let (mut tl, mut tr) = (left, right);
        if crossed {
            let col = self.col_frame()?;
            if jm::abs(frame_x - col.x) < 0.5 {
                tl = jm::min(left, 0.0);
                tr = jm::max(left, col.w);
            } else {
                tr = left;
            }
        }
        let mut skip_to: Option<f64> = None;
        let mut free: Vec<(f64, f64)> = vec![(tl, tr)];
        let mut hit = false;
        let mut min_bottom = f64::INFINITY;
        for f in floats {
            if !matches!(&*f.wrap, "square" | "tight" | "through" | "topAndBottom") {
                continue;
            }
            if f.body_only && self.in_hf {
                continue;
            }
            let top = f.y - f.dist_t;
            let bottom = f.y + f.h + f.dist_b;
            // (Word compares in whole twips — 0.02 pt of overlap does not push; 1 twip does)
            if y + line_h <= top + 0.5 || y >= bottom - 0.5 {
                continue;
            }
            // a non-rectangular wrap: the polygon's projection in the line's band (tight: from the
            // leftmost to the rightmost; through: the pieces, with the inner gaps)
            let mut parts: Option<Vec<(f64, f64)>> = None;
            if let Some(poly) = &f.poly {
                let band = poly_band_intervals(poly, y, y + line_h);
                let (Some(first), Some(last)) = (band.first(), band.last()) else { continue };
                let hull = vec![(first.0, last.1)];
                let src = if &*f.wrap == "through" { band.clone() } else { hull };
                parts = Some(src.iter().map(|&(a, b)| (a - f.dist_l - frame_x, b + f.dist_r - frame_x)).collect());
            }
            let fl = parts.as_ref().and_then(|p| p.first()).map_or(f.x - f.dist_l - frame_x, |p| p.0);
            let fr = parts.as_ref().and_then(|p| p.last()).map_or(f.x + f.w + f.dist_r - frame_x, |p| p.1);
            if fr <= tl || fl >= tr {
                continue;
            }
            if &*f.wrap == "topAndBottom" {
                skip_to = Some(jm::max(skip_to.unwrap_or(0.0), bottom));
                continue;
            }
            hit = true;
            min_bottom = jm::min(min_bottom, bottom);
            // wrapText: the text goes on one side only — the other is blocked too (largest: the
            // larger gap; oracle wrapsides)
            let side = f.side.as_deref().unwrap_or("bothSides");
            let blocks: Vec<(f64, f64)> = match side {
                "left" => vec![(fl, f64::INFINITY)],
                "right" => vec![(f64::NEG_INFINITY, fr)],
                "largest" => vec![if fl - tl >= tr - fr { (fl, f64::INFINITY) } else { (f64::NEG_INFINITY, fr) }],
                _ => match (&parts, side) {
                    (Some(p), "bothSides") => p.clone(),
                    _ => vec![(fl, fr)],
                },
            };
            for (bl, bh) in blocks {
                let mut next: Vec<(f64, f64)> = Vec::new();
                for &(a, b) in &free {
                    if bh <= a || bl >= b {
                        next.push((a, b));
                        continue;
                    }
                    if bl > a {
                        next.push((a, bl));
                    }
                    if bh < b {
                        next.push((bh, b));
                    }
                }
                free = next;
            }
        }
        if hit {
            // the widest interval (ties: the left one) — several floats on the line: the line goes
            // into the free gap, not below them (oracle hfmix); a gap under 18 pt gets no text
            // (oracle wrapsides); the gap counts only INSIDE the column (oracle ftnegind)
            let col = self.col_frame()?;
            let c_r = if jm::abs(frame_x - col.x) < 0.5 { col.w } else { f64::INFINITY };
            let c_l = if c_r == f64::INFINITY { f64::NEG_INFINITY } else { 0.0 };
            // (the gap counts up to the column margin: the paragraph indent does not narrow it —
            // oracle gapind)
            let ext = |a: f64, b: f64| -> (f64, f64) {
                if c_r == f64::INFINITY {
                    (a, b)
                } else {
                    (if a <= tl + 0.5 { jm::min(a, c_l) } else { a }, if b >= tr - 0.5 { jm::max(b, c_r) } else { b })
                }
            };
            free.retain(|&(a, b)| {
                let (ea, eb) = ext(a, b);
                jm::min(eb, c_r) - jm::max(ea, c_l) >= MIN_GAP
            });
            let mut best: Option<(f64, f64)> = None;
            for &iv in &free {
                if best.is_none_or(|b| iv.1 - iv.0 > b.1 - b.0) {
                    best = Some(iv);
                }
            }
            match best {
                // no useful room beside: the line goes down to the end of the first object that
                // blocks it
                None => skip_to = Some(jm::max(skip_to.unwrap_or(0.0), min_bottom)),
                // an object in the middle: the text takes the gaps from left to right (wrapsides)
                Some(b) if skip_to.is_none() => {
                    return Ok(if crossed {
                        WrapFrame { left, right, skip_to: OptF64::NONE, segs: None, gap: false }
                    } else {
                        let segs = (free.len() > 1).then(|| free.clone());
                        WrapFrame { left: b.0, right: b.1, skip_to: OptF64::NONE, segs, gap: true }
                    });
                }
                Some(_) => {}
            }
        }
        if let Some(s) = skip_to {
            return Ok(WrapFrame { skip_to: OptF64::some(s), ..frame.clone() });
        }
        Ok(WrapFrame { left, right, skip_to: OptF64::NONE, segs: None, gap: false })
    }

    /// `pickSeg(br, frame)` (engine.ts:3222): the gap the line starts in.
    pub(crate) fn pick_seg(br: &LineBreaker, p: &PreparedPara, frame: &WrapFrame) -> (i64, LineFrame) {
        let Some(segs) = &frame.segs else { return (-1, LineFrame { left: frame.left, right: frame.right, gap: frame.gap, next: None }) };
        let need = br.peek_word_width(p);
        for (k, &(a, b)) in segs.iter().enumerate() {
            let nx = segs.get(k + 1).copied();
            if b - a + 0.01 >= need {
                return (i64::try_from(k).unwrap_or(i64::MAX), LineFrame { left: a, right: b, gap: true, next: nx });
            }
        }
        (i64::try_from(segs.len()).unwrap_or(i64::MAX), LineFrame { left: frame.left, right: frame.right, gap: true, next: None })
    }

    /// `moreSegs(br, frame, k, last, put)` (engine.ts:3234): the lines of the next gaps of the
    /// same line (after gap `k`); `put` gets each.
    pub(crate) fn more_segs(
        cx: &mut Cx<'_, '_>,
        br: &mut LineBreaker,
        p: &PreparedPara,
        frame: &WrapFrame,
        k: i64,
        mut last: Line,
        put: &mut dyn FnMut(&mut Cx<'_, '_>, &Line) -> LResult<()>,
    ) -> LResult<Line> {
        let Some(segs) = &frame.segs else { return Ok(last) };
        // (a line that ended in a tab past the gap: the rest does not go to the next gap — tabgap)
        // (a line whose tab crossed the float: the next gap was used already — ftrtabs)
        let mut j = k + 1 + i64::from(last.spanned);
        while let Some(&(a, b)) = usize::try_from(j).ok().and_then(|j| segs.get(j)) {
            if br.done(p) || last.break_after.is_some() || last.clear || last.gap_tab == Some(crate::lines::GapTab::Break) {
                break;
            }
            if b - a + 0.01 < br.peek_word_width(p) {
                j += 1;
                continue;
            }
            let nx = usize::try_from(j + 1).ok().and_then(|j| segs.get(j)).copied();
            last = br.next(cx, p, LineFrame { left: a, right: b, gap: true, next: nx })?;
            put(cx, &last)?;
            if last.spanned {
                j += 1;
            }
            j += 1;
        }
        Ok(last)
    }

    // ───────────────────────── textboxes ─────────────────────────

    /// `tbEnv(tb)` (engine.ts:3251): a dark SOLID fill lightens the automatic text colour; the
    /// `fontRef` colour for text without one.
    pub(crate) fn tb_env(cx: &Cx<'_, '_>, tb: &DrawingTextbox) -> (bool, Option<Box<str>>) {
        let theme = &cx.ldoc.view.formatting().theme;
        let fc = tb.font_color.as_ref().and_then(|c| crate::drawing::resolve_color(c, theme));
        let s = tb.shape.as_deref();
        let solid = s
            .filter(|s| s.gradient.is_none() && s.pattern.is_none() && s.blip.is_none() && s.fill_ref.is_none())
            .and_then(|s| s.fill.as_ref());
        let dark = crate::para::is_dark(solid.and_then(|c| crate::drawing::resolve_color(c, theme)).as_deref());
        (dark, fc.filter(|c| !c.is_empty()).map(Into::into))
    }

    /// The content blocks of a textbox (its derived story).
    fn tb_blocks<'a>(&self, cx: &Cx<'_, 'a>, tb: &DrawingTextbox) -> LResult<Vec<SBlock<'a>>> {
        let k = *cx.ldoc.tb_of_drawing.get(&tb.drawing).ok_or(LayoutError::Internal("textbox not derived"))?;
        let story = cx.ldoc.textboxes.get(k.ix()).ok_or(LayoutError::Internal("textbox"))?.story;
        self.story_blocks(cx, story)
    }

    /// The textbox env (`{ field: this.fieldValue(ps), ...this.tbEnv(tb), hf? }`).
    fn tb_para_env(&self, cx: &Cx<'_, '_>, tb: &DrawingTextbox, with_field: bool) -> ParaEnv {
        let (dark, dc) = Self::tb_env(cx, tb);
        ParaEnv {
            field: with_field.then_some(FieldSource::HeaderFooter),
            dark_background: dark,
            default_color: dc,
            hf: self.in_hf && with_field,
            ..ParaEnv::default()
        }
    }

    /// A static flow with `fieldValue(ps)` for its fields (`field_page`, F7c).
    #[expect(clippy::too_many_arguments, reason = "layoutStatic's signature")]
    fn static_at<'a>(
        &mut self,
        cx: &mut Cx<'_, 'a>,
        blocks: &[SBlock<'a>],
        width: f64,
        env: &ParaEnv,
        at: &LineRef,
        ps: Option<PageIx>,
        field_ps: Option<PageIx>,
        y_origin: f64,
    ) -> LResult<CellContent> {
        let saved = self.field_page;
        if let Some(f) = field_ps {
            self.field_page = Some(f);
        }
        let r = self.layout_static(cx, blocks, 0.0, width, env, at, ps, y_origin, None);
        self.field_page = saved;
        r
    }

    // ───────────────────────── placing floats ─────────────────────────

    /// `placeFloats(p, ps, colX, colW, paraTop, flowItems, yOrigin, paraRef)` (engine.ts:3348).
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    pub(crate) fn place_floats(
        &mut self,
        cx: &mut Cx<'_, '_>,
        p: &PreparedPara,
        ps: PageIx,
        col_x: f64,
        col_w: f64,
        para_top: f64,
        mut flow: Option<&mut PassItems>,
        y_origin: f64,
        para_ref: Option<f64>,
    ) -> LResult<()> {
        let objs: Vec<RunKey> = p.glyphs.iter().filter_map(|g| g.obj).collect();
        for k in objs {
            let Some(run) = crate::paint::model_run(cx.ldoc, k) else { continue };
            if !super::static_layout::run_floats(run) || self.pre_placed.contains(&k) {
                continue;
            }
            if self.only_text_floats && !follows_text(float_anchor(run)) {
                continue;
            }
            self.place_float(cx, k, ps, col_x, col_w, para_top, flow.as_deref_mut(), y_origin, None, para_ref)?;
        }
        Ok(())
    }

    /// `placeFloat(run, ps, colX, colW, paraTop, flowItems, yOrigin, clampBottom, paraRef)`
    /// (engine.ts:3359).
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    pub(crate) fn place_float(
        &mut self,
        cx: &mut Cx<'_, '_>,
        rk: RunKey,
        ps: PageIx,
        col_x: f64,
        col_w: f64,
        para_top: f64,
        flow: Option<&mut PassItems>,
        y_origin: f64,
        clamp_bottom: Option<f64>,
        para_ref: Option<f64>,
    ) -> LResult<FloatReceipt> {
        cx.budget.charge(1)?;
        let run: &Run = crate::paint::model_run(cx.ldoc, rk).ok_or(LayoutError::Internal("placeFloat: run"))?;
        let gi = self.ps(ps)?.geom;
        let g = self.geom(gi)?.clone();
        let fmt = cx.ldoc.view.formatting();
        let compat = fmt.settings.compatibility_mode.to_number();
        let theme = &fmt.theme;
        let e = run.ext();
        let image = e.and_then(|e| e.image.as_deref());
        let shape = e.and_then(|e| e.shape.as_deref());
        let group = e.and_then(|e| e.group.as_deref());
        let textbox = e.and_then(|e| e.textbox.as_deref());
        let run_ref = cx.ldoc.run_ref(rk).cloned();
        let drawing = drawing_of(run);
        // a hidden floating picture (Selection Pane, docPr hidden): neither drawn nor pushing the
        // text; a textbox, shape or group hidden likewise (oracles pichidden, txbxhide)
        let hidden_pic = image.is_some_and(|i| i.floating == Some(true) && i.format.as_ref().and_then(|f| f.doc_pr.hidden) == Some(true));
        if hidden_pic
            || textbox.and_then(|t| t.hidden) == Some(true)
            || shape.and_then(|s| s.hidden) == Some(true)
            || group.and_then(|g| g.hidden) == Some(true)
        {
            let zseq = self.z_seq;
            self.z_seq += 1.0;
            return Ok(FloatReceipt {
                ps,
                run: rk,
                target: None,
                zseq,
                obj: PlacedObject {
                    run: run_ref,
                    drawing,
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                    layer: Layer::Front,
                    hf: self.in_hf,
                    frame: None,
                    content: None,
                },
                obj_placed: false,
                float_box: None,
                box_placed: false,
                flow: None,
                cell_items: None,
            });
        }
        let anchor = float_anchor(run);
        let (mut w, mut h) = float_size(run);
        // mode 15: "behind the text" only without wrapping; with wrapping the object goes into the
        // front layer, in relativeHeight order — body and header. Modes ≤ 14: behindDoc always holds
        let wraps_text = anchor.is_some_and(|a| &*a.wrap != "none");
        let behind = !(wraps_text && compat >= 15.0)
            && (anchor.and_then(|a| a.behind_doc) == Some(true)
                || image.and_then(|i| i.behind_doc) == Some(true)
                || textbox.and_then(|t| t.behind_doc) == Some(true));
        // the textbox content (automatic height)
        let mut tb_content: Option<CellContent> = None;
        let mut tb_final: Option<TbFinal<'_>> = None;
        let chained = match textbox {
            // a box of a linked chain: the piece of the content that fits in it (oracle txbxlink, F7d)
            Some(tb) => self.chain_content(cx, tb, super::chain::ChainEnv { field_ps: Some(ps), hf: self.in_hf })?,
            None => None,
        };
        if chained.is_some() {
            tb_content = chained;
        } else if let Some(tb) = textbox {
            // margins and width in the TEXT's frame (vertical text turns the margins)
            let fr = tb_frame(tb, theme, w, h);
            let ins = fr.ins;
            let blocks = self.tb_blocks(cx, tb)?;
            let env = self.tb_para_env(cx, tb, true);
            let mut inner = fr.iw;
            let fi = cx.ldoc.tb_of_drawing.get(&tb.drawing).and_then(|k| cx.ldoc.textboxes.get(k.ix())).and_then(|t| t.frame);
            let txbx = tb_ref(LineStory::Textbox(cx.ldoc.tb_of_drawing.get(&tb.drawing).copied().unwrap_or(crate::keys::TbKey(u32::MAX))));
            if fi.is_some_and(|f| f.auto_width) {
                // a frame without a width: its longest line's, breaking at the column width
                let probe = self.pure_static(cx, |e, cx| e.static_at(cx, &blocks, col_w, &env, &txbx, None, Some(ps), 0.0))?;
                let m = probe.lines.iter().fold(0.0, |m, l| {
                    let side = jm::min(l.x, col_w - l.x - l.width);
                    jm::max(m, l.width + if side < 2880.0 { side } else { 0.0 })
                });
                w = jm::max(1.0, m);
                inner = w;
            }
            if tb.no_wrap == Some(true) {
                // text without wrapping: the longest line; with auto fit the box takes it (Word)
                let probe = self.pure_static(cx, |e, cx| e.static_at(cx, &blocks, 31680.0, &env, &txbx, None, Some(ps), 0.0))?;
                let natural = probe.lines.iter().fold(0.0, |m, l| {
                    let side = jm::min(l.x, 31680.0 - l.x - l.width);
                    jm::max(m, l.width + if side < 2880.0 { side } else { 0.0 })
                });
                if tb.auto_fit == Some(true) {
                    w = natural + ins.l + ins.r;
                    inner = natural;
                } else {
                    inner = jm::max(inner, natural);
                }
            }
            let iw = jm::max(1.0, inner);
            let c = self.pure_static(cx, |e, cx| e.static_at(cx, &blocks, iw, &env, &txbx, None, Some(ps), 0.0))?;
            // auto fit: the text's height becomes the box's (the stored cy may be stale); vertical
            // text: the WIDTH (oracle txbxvert)
            if tb.auto_fit == Some(true) && fr.rot != TbRot::None {
                w = vertical_height(&c) + ins.t + ins.b;
            } else if tb.auto_fit == Some(true) {
                h = c.height + ins.t + ins.b;
            }
            if let Some(fi) = fi {
                h = match fi.h_rule {
                    crate::ldoc::FrameHRule::Exact => fi.h,
                    crate::ldoc::FrameHRule::AtLeast => jm::max(fi.h, c.height),
                    crate::ldoc::FrameHRule::Auto => c.height,
                };
            }
            // (vertical text: turned at paint time; floats inside it stay at the origin)
            if fr.rot == TbRot::None {
                tb_final = Some(TbFinal { blocks, at: txbx, iw, env, ins });
            }
            tb_content = Some(c);
        }
        // an even page: "inside"/"outside" and the inner/outer margins swap sides (oracle inside)
        let even = js_eq(jm::rem(self.ps(ps)?.page.number, 2.0), 0.0);
        let h_align = side_of(anchor.and_then(|a| a.pos_h.align.as_deref()), even, true);
        let v_align = side_of(anchor.and_then(|a| a.pos_v.align.as_deref()), even, false);
        let ee = anchor.and_then(|a| a.effect_extent.as_ref());
        let (ee_l, ee_r, ee_t, ee_b) =
            ee.map_or((0.0, 0.0, 0.0, 0.0), |e| (e.l.to_number(), e.r.to_number(), e.t.to_number(), e.b.to_number()));
        let off_h = anchor.and_then(|a| a.pos_h.offset).and_then(JsNumber::as_f64);
        let off_v = anchor.and_then(|a| a.pos_v.offset).and_then(JsNumber::as_f64);
        let pct_h = anchor.and_then(|a| a.pos_h.pct).and_then(JsNumber::as_f64);
        let pct_v = anchor.and_then(|a| a.pos_v.pct).and_then(JsNumber::as_f64);
        let (mut x, mut y);
        if let Some(sp) = anchor.and_then(|a| a.simple_pos.as_ref()) {
            x = sp.x.to_number();
            y = sp.y.to_number();
        } else {
            let hf = side_of(anchor.map(|a| &*a.pos_h.relative_from), even, true).unwrap_or("column");
            let (hx, hw) = horiz_ref(hf, &g, col_x, col_w);
            x = align_pos(h_align, off_h, hx, hw, w, pct_h, g.page_w, ee_l, ee_r);
            let pin = self.pinned_floats.get(&rk).copied();
            if let Some(pin) = pin.filter(|p| p.ps == ps) {
                y = pin.y;
            } else {
                let v_from0 = side_of(anchor.map(|a| &*a.pos_v.relative_from), even, false).unwrap_or("paragraph");
                let base = if v_from0 == "paragraph" && para_ref.is_some() { para_ref.unwrap_or(para_top) } else { para_top };
                let (vy, vh) = vert_ref(v_from0, &g, self.ps(ps)?.body_top, base);
                y = align_pos(v_align, off_v, vy, vh, h, pct_v, g.page_h, ee_t, ee_b);
                if let Some(cb) = clamp_bottom
                    && y + h > cb
                {
                    y = cb - h;
                }
                // wrapping and relative to the text: not above the body top; in the old modes (≤
                // 14) only not above the PAGE top (oracles toptbl/toptbl2)
                if !self.in_hf && anchor.is_some_and(|a| wrapping(&a.wrap)) && follows_text(anchor) {
                    y = jm::max(y, self.float_min_y(cx, ps)?);
                } else if self.in_hf && compat < 15.0 && anchor.is_some_and(|a| wrapping(&a.wrap)) && follows_text(anchor) {
                    // in the header/footer of modes ≤ 14: not out through the PAGE's top or left edge
                    // (oracle hff14wrap)
                    y = jm::max(y, 0.0);
                    x = jm::max(x, 0.0);
                }
            }
        }
        // anchored in a TABLE CELL: references relative to the cell (the cell's text area across;
        // the paragraph, or the cell top, down) and the items in the cell's flow, which moves with
        // the row — oracle cellfloat
        if self.in_cell && anchor.is_none_or(|a| a.simple_pos.is_none()) {
            let v_from = anchor.map_or("paragraph", |a| &*a.pos_v.relative_from);
            // left/right margin = the CELL's margin (right aligned: flush with its border)
            let (cm_l, cm_r, cm_t) = self.cell_mar.unwrap_or((0.0, 0.0, None));
            let h_from = side_of(anchor.map(|a| &*a.pos_h.relative_from), even, true).unwrap_or("column");
            let (hx, hw) = match h_from {
                "rightMargin" | "outsideMargin" => (col_x + col_w, cm_r),
                "leftMargin" | "insideMargin" => (col_x - cm_l, cm_l),
                _ => (col_x, col_w),
            };
            // layoutInCell="0" in modes ≤ 14 (15 ignores it): out of the cell (oracle cellout)
            let out_cell = anchor.and_then(|a| a.layout_in_cell) == Some(false) && compat < 15.0;
            if out_cell {
                let fr = self.col_frame()?;
                let (rx, rw) = horiz_ref(h_from, &g, fr.x, fr.w);
                x = align_pos(h_align, off_h, rx, rw, w, pct_h, g.page_w, ee_l, ee_r);
            } else {
                x = align_pos(h_align, off_h, hx, hw, w, None, g.page_w, ee_l, ee_r);
            }
            let v_ref = if out_cell && (v_from == "paragraph" || v_from == "line") {
                y_origin - cm_t.unwrap_or(0.0)
            } else if v_from == "paragraph" {
                para_ref.unwrap_or(para_top)
            } else if v_from == "line" {
                para_top
            } else {
                y_origin
            };
            y = align_pos(v_align, off_v, v_ref, 0.0, h, None, g.page_h, ee_t, ee_b);
            // wrapping and relative to the text: not above the cell's text area (oracle cellpic)
            if anchor.is_some_and(|a| wrapping(&a.wrap)) && (v_from == "paragraph" || v_from == "line") {
                y = jm::max(y, y_origin);
            }
            if let Some(tf) = &tb_final {
                let (bx, by) = (x, y);
                tb_content = Some(self.pure_static(cx, |e, cx| e.tb_final_layout(cx, tf, bx, by, ps))?);
            }
            let mut cell_items = PassItems::new();
            self.paint_object(cx, run, false, x, y, w, h, tb_content.as_ref(), ps, false, &mut cell_items)?;
            if let Some(fl) = flow {
                if behind {
                    let old = std::mem::take(fl);
                    for i in 0..cell_items.len() {
                        super::push_copy(fl, &cell_items, i)?;
                    }
                    for i in 0..old.len() {
                        super::push_copy(fl, &old, i)?;
                    }
                } else {
                    for i in 0..cell_items.len() {
                        super::push_copy(fl, &cell_items, i)?;
                    }
                }
            }
            // wrapping: the cell's text wraps around it (layoutStatic records the box — oracle
            // cellwrap)
            let wb = anchor.filter(|a| matches!(&*a.wrap, "tight" | "through")).and_then(|a| a.wrap_box.as_ref());
            let csw = wb.is_none() && swaps_box(float_rot(run));
            let cbox = match anchor {
                Some(a) if wrapping(&a.wrap) && a.layout_in_cell != Some(false) => Some(self.float_box(a, wb, csw, x, y, w, h)),
                _ => None,
            };
            let zseq = self.z_seq;
            self.z_seq += 1.0;
            return Ok(FloatReceipt {
                ps,
                run: rk,
                target: None,
                zseq,
                obj: PlacedObject {
                    run: run_ref,
                    drawing,
                    x,
                    y,
                    w,
                    h,
                    layer: if behind { Layer::Behind } else { Layer::Front },
                    hf: self.in_hf,
                    frame: None,
                    content: None,
                },
                obj_placed: false,
                float_box: cbox,
                box_placed: false,
                flow: None,
                cell_items: Some(cell_items),
            });
        }
        let target = match (self.in_hf, behind) {
            (true, true) => ZLayer::HfBehind,
            (true, false) => ZLayer::HfFront,
            (false, true) => ZLayer::Behind,
            (false, false) => ZLayer::Front,
        };
        if let Some(tf) = &tb_final {
            tb_content = Some(self.tb_final_layout(cx, tf, x, y, ps)?);
        }
        let mut items = PassItems::new();
        self.paint_object(cx, run, false, x, y, w, h, tb_content.as_ref(), ps, false, &mut items)?;
        let body_top = self.ps(ps)?.body_top;
        let mut obj = PlacedObject {
            run: run_ref,
            drawing,
            x,
            y,
            w,
            h,
            layer: if behind { Layer::Behind } else { Layer::Front },
            hf: self.in_hf,
            frame: Some(ObjectFrame {
                col_x,
                col_w,
                para_top: para_ref.unwrap_or(para_top),
                page_w: g.page_w,
                page_h: g.page_h,
                mar_l: g.mar_l,
                mar_r: g.mar_r,
                mar_t: g.mar_t,
                mar_b: g.mar_b,
                body_top: jm::max(g.mar_t, body_top),
            }),
            content: None,
        };
        if let (Some(tb), Some(c)) = (textbox, &tb_content) {
            let (lines, xf) = tb_content_placement(tb, theme, x, y, w, h, c);
            let page = &mut self.ps_mut(ps)?.page;
            let start = u32::try_from(page.content_lines.len()).map_err(|_| cap())?;
            page.content_lines.extend(lines);
            let end = u32::try_from(page.content_lines.len()).map_err(|_| cap())?;
            obj.content = Some((start..end, xf));
        }
        self.ps_mut(ps)?.page.objects.get_or_insert_with(Vec::new).push(obj.clone());
        // a frame in the header/footer: in the flow — in Word's PDF its text comes in the line's
        // order: before the anchor paragraph if at its start (fr-hdr-left), after it if further
        // right (fr-hdr-pgnum); in the body, after all the page's text (the front layer)
        let frame_flow = self.in_hf
            && textbox.is_some_and(|tb| {
                cx.ldoc.tb_of_drawing.get(&tb.drawing).and_then(|k| cx.ldoc.textboxes.get(k.ix())).is_some_and(|t| t.frame.is_some())
            });
        if frame_flow {
            let mut rel = PassItems::new();
            for i in 0..items.len() {
                if uo_jsnum::conv::truthy(y_origin) {
                    rel.push_shifted(&items, i, -y_origin, 0.0).map_err(|_| cap())?;
                } else {
                    super::push_copy(&mut rel, &items, i)?;
                }
            }
            if x > col_x + 20.0 {
                for i in 0..rel.len() {
                    super::push_copy(&mut self.hf_frame_after, &rel, i)?;
                }
            } else if let Some(fl) = flow {
                for i in 0..rel.len() {
                    super::push_copy(fl, &rel, i)?;
                }
            }
            items = PassItems::new();
        }
        let zseq = self.z_seq;
        self.z_seq += 1.0;
        let z = anchor.and_then(|a| a.relative_height).map_or(0.0, |r| r.as_f64().unwrap_or(0.0));
        {
            let p = self.ps_mut(ps)?;
            let layer = match target {
                ZLayer::Behind => &mut p.behind,
                ZLayer::Front => &mut p.front,
                ZLayer::HfBehind => &mut p.hf_behind,
                ZLayer::HfFront => &mut p.hf_front,
            };
            if !frame_flow {
                layer.push(ZObj { z, seq: zseq, items });
            }
        }
        // compatibility mode ≤ 14: header/footer floats do not push the header/footer's own text
        // (oracle hffloat, hff14-*), but the body's yes (oracle hff14wrap)
        let hf_legacy = self.in_hf && compat < 15.0;
        let mut fbox: Option<FloatBox> = None;
        if let Some(a) = anchor.filter(|a| wrapping(&a.wrap)) {
            // tight/through: the wrap polygon's box (oracle wrappoly)
            let wb = if matches!(&*a.wrap, "tight" | "through") { a.wrap_box.as_ref() } else { None };
            // turned between 45° and 135° (or 225° and 315°): the box swaps width and height about
            // its centre (oracle tb-rot90)
            let sw = wb.is_none() && swaps_box(float_rot(run));
            let mut fb = self.float_box(a, wb, sw, x, y, w, h);
            if let (Some(poly), Some(_)) = (&a.wrap_poly, wb) {
                fb.poly = Some(poly.iter().map(|(u, v)| (x + w * u.to_number(), y + h * v.to_number())).collect());
            }
            // (the body's tables go below them too — oracle tblfloat)
            if hf_legacy {
                fb.body_only = true;
            }
            fbox = Some(fb);
        }
        let mut box_placed = false;
        if let Some(b) = &fbox {
            self.ps_mut(ps)?.floats.push(b.clone());
            box_placed = true;
            // an absolute position covering body lines already placed on this page: in the next
            // pass it goes at the page start (oracle latefloat)
            if !self.in_hf && !follows_text(anchor) && !self.pre_placed.contains(&rk) && ps != super::SCRATCH {
                let p = self.ps(ps)?;
                let pg = self.geom(p.geom)?;
                let hit = p.page.lines.iter().any(|l| {
                    !matches!(l.at.story, LineStory::HeaderFooter { .. } | LineStory::Textbox(_))
                        && l.y + l.height > b.y - b.dist_t
                        && l.y < b.y + b.h + b.dist_b
                        && b.x + b.w + b.dist_r > pg.mar_l
                        && b.x - b.dist_l < pg.page_w - pg.mar_r
                });
                if hit {
                    let pi = u32::try_from(ps.ix()).map_err(|_| cap())?;
                    self.found.late_found.entry(pi).or_default().push(rk);
                }
            }
        }
        Ok(FloatReceipt {
            ps,
            run: rk,
            target: Some(target),
            zseq,
            obj,
            obj_placed: true,
            float_box: fbox,
            box_placed,
            flow: None,
            cell_items: None,
        })
    }

    /// The wrap box of a float (`FloatBox` of `placeFloat`).
    #[expect(clippy::too_many_arguments, reason = "the TS expression's inputs")]
    fn float_box(&self, a: &DrawingAnchor, wb: Option<&crate::model::WrapBox>, sw: bool, x: f64, y: f64, w: f64, h: f64) -> FloatBox {
        let (bx, by, bw, bh) = match wb {
            Some(b) => {
                let (x0, y0, x1, y1) = (b.x0.to_number(), b.y0.to_number(), b.x1.to_number(), b.y1.to_number());
                (x + w * x0, y + h * y0, w * (x1 - x0), h * (y1 - y0))
            }
            None if sw => (x + (w - h) / 2.0, y + (h - w) / 2.0, h, w),
            None => (x, y, w, h),
        };
        FloatBox {
            x: bx,
            y: by,
            w: bw,
            h: bh,
            wrap: a.wrap.to_string().into(),
            side: Some(a.wrap_text.as_deref().map_or_else(|| "bothSides".into(), Into::into)),
            dist_l: n0(a.dist_l),
            dist_r: n0(a.dist_r),
            dist_t: n0(a.dist_t),
            dist_b: n0(a.dist_b),
            body_only: false,
            poly: None,
        }
    }

    /// `tbFinal(bx, by)`: the box's content laid out in place (`staticDx = bx + ins.l`, the floats
    /// inside it relative to the box).
    fn tb_final_layout(&mut self, cx: &mut Cx<'_, '_>, tf: &TbFinal<'_>, bx: f64, by: f64, ps: PageIx) -> LResult<CellContent> {
        let prev_dx = self.static_dx;
        self.static_dx = bx + tf.ins.l;
        let mut cx2 = super::flow::with_ldoc!(cx, cx.ldoc);
        let r = self.static_at(&mut cx2, &tf.blocks, tf.iw, &tf.env, &tf.at, Some(ps), Some(ps), by + tf.ins.t);
        self.static_dx = prev_dx;
        r
    }

    /// `unplaceFloat(r)` (engine.ts:3548).
    pub(crate) fn unplace_float(&mut self, r: &FloatReceipt) -> LResult<()> {
        let p = self.ps_mut(r.ps)?;
        if let Some(t) = r.target {
            let layer = match t {
                ZLayer::Behind => &mut p.behind,
                ZLayer::Front => &mut p.front,
                ZLayer::HfBehind => &mut p.hf_behind,
                ZLayer::HfFront => &mut p.hf_front,
            };
            if let Some(i) = layer.iter().position(|z| js_eq(z.seq, r.zseq)) {
                layer.remove(i);
            }
        }
        if r.obj_placed
            && let Some(objs) = p.page.objects.as_mut()
            && let Some(i) = objs.iter().position(|o| obj_eq(o, &r.obj))
        {
            objs.remove(i);
        }
        if r.box_placed
            && let Some(b) = &r.float_box
            && let Some(i) = p.floats.iter().position(|f| box_eq(f, b))
        {
            p.floats.remove(i);
        }
        // the content of a textbox laid out already does not count: redone in the new position
        if let Some(f) = &r.flow {
            self.ctx.restore_flow(f);
        }
        Ok(())
    }

    /// `placeFloatsEarly(p, ps, colX, colW, paraTop, clampBottom, paraRef)` (engine.ts:3563): the
    /// body paragraph's wrapping floats placed BEFORE its lines.
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    pub(crate) fn place_floats_early(
        &mut self,
        cx: &mut Cx<'_, '_>,
        p: &PreparedPara,
        ps: PageIx,
        col_x: f64,
        col_w: f64,
        para_top: f64,
        clamp_bottom: Option<f64>,
        para_ref: Option<f64>,
    ) -> LResult<Vec<FloatReceipt>> {
        let mut out = Vec::new();
        let mut flow: Option<FlowState> = None;
        let objs: Vec<RunKey> = p.glyphs.iter().filter_map(|g| g.obj).collect();
        for k in objs {
            let Some(run) = crate::paint::model_run(cx.ldoc, k) else { continue };
            if !super::static_layout::run_floats(run) {
                continue;
            }
            let Some(anc) = float_anchor(run) else { continue };
            // (already placed at the page start as a late float: not duplicated)
            if !wrapping(&anc.wrap) || self.pre_placed.contains(&k) {
                continue;
            }
            let ft = follows_text(Some(anc));
            // the state before ALL placements (undoing in any order returns to it)
            if flow.is_none() {
                flow = Some(self.ctx.save_flow());
            }
            let cb = if ft { clamp_bottom } else { None };
            let mut r = self.place_float(cx, k, ps, col_x, col_w, para_top, None, 0.0, cb, para_ref)?;
            r.flow.clone_from(&flow);
            out.push(r);
        }
        Ok(out)
    }

    /// `paintInlineDrawings(out, line, frameX, top, ps)` (engine.ts:3583): inline drawing groups and
    /// textboxes (`wp:inline`), drawn as the floats in the glyph's box. `to_body`: `out ===
    /// this.cur.body` (they go to the front layer, after the page's text).
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    pub(crate) fn paint_inline_drawings(
        &mut self,
        cx: &mut Cx<'_, '_>,
        p: &PreparedPara,
        line: &Line,
        frame_x: f64,
        top: f64,
        ps: Option<PageIx>,
        to_body: bool,
        out: &mut PassItems,
    ) -> LResult<()> {
        for pg in &line.placed {
            let Some(gl) = crate::lines::glyph_of(p, pg.g) else { continue };
            if gl.kind != crate::para::GlyphKind::Obj || gl.adv <= 0.0 {
                continue;
            }
            let Some(rk) = gl.obj else { continue };
            let Some(r) = crate::paint::model_run(cx.ldoc, rk) else { continue };
            let Some(e) = r.ext() else { continue };
            if (e.group.is_none() && e.textbox.is_none()) || e.image.is_some() || e.shape.is_some() {
                continue;
            }
            let (w, h) = match (e.group.as_deref(), e.textbox.as_deref()) {
                (Some(g), _) => (g.width_twips.to_number(), g.height_twips.to_number()),
                (None, Some(t)) => (t.width_twips.to_number(), t.height_twips.to_number()),
                _ => (0.0, 0.0),
            };
            let shift = p.styles.get(gl.style.0 as usize).map_or(0.0, |s| s.shift);
            let x = frame_x + pg.x;
            let y = top + line.baseline - shift - h;
            let mut tc: Option<CellContent> = None;
            if let Some(tb) = e.textbox.as_deref()
                && let Some(c) = self.chain_content(cx, tb, super::chain::ChainEnv { field_ps: None, hf: false })?
            {
                tc = Some(c);
            } else if let Some(tb) = e.textbox.as_deref() {
                let blocks = self.tb_blocks(cx, tb)?;
                let theme = &cx.ldoc.view.formatting().theme;
                let iw = jm::max(1.0, tb_frame(tb, theme, w, h).iw);
                let (dark, dc) = Self::tb_env(cx, tb);
                let env = ParaEnv { dark_background: dark, default_color: dc, ..ParaEnv::default() };
                tc = Some(self.static_at(cx, &blocks, iw, &env, &tb_ref(LineStory::InlineTextbox), ps, None, 0.0)?);
            }
            let pix = match ps {
                Some(p) => p,
                None => self.cur_ix()?,
            };
            let mut drawn = PassItems::new();
            self.paint_object(cx, r, false, x, y, w, h, tc.as_ref(), pix, false, &mut drawn)?;
            // in the body, as the floats: after the page's text (Word's PDF order — oracle inlinegrp)
            if to_body {
                let zseq = self.z_seq;
                self.z_seq += 1.0;
                self.cur_ps_mut()?.front.push(ZObj { z: 0.0, seq: zseq, items: drawn });
            } else {
                for i in 0..drawn.len() {
                    super::push_copy(out, &drawn, i)?;
                }
            }
        }
        Ok(())
    }

    /// The cell branch of `layoutStatic` (engine.ts:1340–1356): the paragraph's floats placed
    /// BEFORE its lines, with their `LATE_ITEMS` mark; the cell's text wraps around the wrapping
    /// ones (oracle cellwrap); they are drawn after the text (`cell_items`).
    #[expect(clippy::too_many_arguments, reason = "layoutStatic's locals")]
    pub(crate) fn static_cell_floats(
        &mut self,
        cx: &mut Cx<'_, '_>,
        p: &PreparedPara,
        ps: PageIx,
        x: f64,
        width: f64,
        y_origin: f64,
        top: f64,
        para_ref_s: f64,
        sf: &mut super::tables::StaticFloats,
        cell_items: &mut PassItems,
    ) -> LResult<()> {
        let objs: Vec<RunKey> = p.glyphs.iter().filter_map(|g| g.obj).collect();
        for k in objs {
            let Some(run) = crate::paint::model_run(cx.ldoc, k) else { continue };
            if self.pre_placed.contains(&k) || !super::static_layout::run_floats(run) {
                continue;
            }
            let anchor = float_anchor(run);
            if self.only_text_floats && !follows_text(anchor) {
                continue;
            }
            let mut its = PassItems::new();
            let dx = self.static_dx;
            let rc =
                self.place_float(cx, k, ps, x + dx, width, y_origin + top, Some(&mut its), y_origin, None, Some(y_origin + para_ref_s))?;
            let z = anchor.and_then(|a| a.relative_height).and_then(JsNumber::as_f64).unwrap_or(0.0);
            let seq = self.z_seq;
            self.z_seq += 1.0;
            self.late_marks.push(super::LateMark { z, seq, behind: rc.obj.layer == Layer::Behind });
            let mk = crate::keys::ItemMeta::of(self.late_marks.len() - 1).ok_or(LayoutError::Limit(LimitKind::Capacity))?;
            for s in &mut its.side {
                s.meta = Some(mk);
            }
            for i in 0..its.len() {
                super::push_copy(cell_items, &its, i)?;
            }
            if let Some(b) = &rc.float_box
                && anchor.is_none_or(|a| a.simple_pos.is_none())
            {
                sf.cell_floats.push(FloatBox { x: b.x - dx, y: b.y - y_origin, ..b.clone() });
                // wrapping: the cell grows to its end; without, not (oracle cellwrap)
                sf.float_bottom = jm::max(sf.float_bottom, rc.obj.y - y_origin + rc.obj.h);
            }
            // all count in the height of the aligned content (centre/bottom — oracle cellwrap)
            sf.align_bottom = jm::max(sf.align_bottom, rc.obj.y - y_origin + rc.obj.h);
        }
        Ok(())
    }

    // ───────────────────────── drawing objects ─────────────────────────

    /// `paintObject(run, x, y, w, h, tbContent, ps, inGroup)` (engine.ts:3602). `copy`: `run` is an
    /// object the layout built (a group child under the group's rotation/flip), not the model's.
    #[expect(clippy::too_many_arguments, reason = "the TS signature")]
    #[expect(clippy::too_many_lines, reason = "one TS method, ported as one")]
    pub(crate) fn paint_object(
        &mut self,
        cx: &mut Cx<'_, '_>,
        run: &Run,
        copy: bool,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        tb_content: Option<&CellContent>,
        ps: PageIx,
        in_group: bool,
        out: &mut PassItems,
    ) -> LResult<()> {
        cx.budget.charge(1)?;
        let Some(e) = run.ext() else { return Ok(()) };
        let theme = &cx.ldoc.view.formatting().theme;
        let r = MetafileFrame { x, y, w, h };
        if let Some(im) = e.image.as_deref() {
            let data = cx.ldoc.view.media(im.data.0).unwrap_or(&[]);
            let id = picture_id(cx, im, copy)?;
            if !push_metafile(cx, data, id, r, out)? {
                push_image_item(cx, Some(im), id, r, out)?;
            }
        } else if let Some(sh) = e.shape.as_deref() {
            let line = sh.line.as_ref().map(|l| [l.x1.to_number(), l.y1.to_number(), l.x2.to_number(), l.y2.to_number()]);
            let look = ShapeLook { owner: sh.drawing, slot: ImgSlot::ShapeBlip, line };
            push_shape_item(cx, &sh.style, [x, y, w, h], Some(&look), out)?;
        } else if let Some(tb) = e.textbox.as_deref() {
            let s = tb.shape.as_deref();
            if let Some(s) = s
                && (s.fill.as_ref().is_some_and(crate::drawing::color_truthy)
                    || s.line.is_some()
                    || s.gradient.is_some()
                    || s.pattern.is_some()
                    || s.blip.is_some())
            {
                let look = ShapeLook { owner: tb.drawing, slot: ImgSlot::TextboxBlip, line: None };
                push_shape_item(cx, s, [x, y, w, h], Some(&look), out)?;
            }
            if let Some(c) = tb_content {
                // margins in the text's frame (vertical text: turned with the shape's sides)
                let fr = tb_frame(tb, theme, w, h);
                let ins = fr.ins;
                // a turned box: the content turns about the centre (the shape turns in the painter)
                let rot = tb.rot.and_then(JsNumber::as_f64).or_else(|| s.and_then(|s| s.rot.and_then(JsNumber::as_f64))).unwrap_or(0.0);
                // WordArt (textPlain): the text stretches to fill the inner box
                let mut art: Option<([f64; 6], f64)> = None;
                if tb.word_art.as_deref().is_some_and(|s| !s.is_empty()) && !c.lines.is_empty() {
                    let x0 = jm::min_n(&c.lines.iter().map(|l| l.x).collect::<Vec<_>>());
                    let x1 = jm::max_n(&c.lines.iter().map(|l| l.x + l.width).collect::<Vec<_>>());
                    let iw = jm::max(1.0, w - ins.l - ins.r);
                    let ih = jm::max(1.0, h - ins.t - ins.b);
                    let sx = iw / jm::max(1.0, x1 - x0);
                    let sy = ih / jm::max(1.0, c.height);
                    let mut m = [sx, 0.0, 0.0, sy, x + ins.l - x0 * sx, y + ins.t];
                    let mut dy = 0.0;
                    // v:textpath (watermark): the em takes the box HEIGHT and the baseline sits
                    // height·winDescent/upm above the bottom (oracle page-lab wmf-*)
                    let g0 = if tb.vml == Some(true) {
                        (0..c.items.len()).find(|&i| matches!(c.items.list.items.get(i), Some(Item::Glyphs { .. })))
                    } else {
                        None
                    };
                    if let Some(gi) = g0
                        && c.lines.len() == 1
                        && let Some(Item::Glyphs { face, size_pt, y: gy, .. }) = c.items.list.items.get(gi)
                    {
                        let font = cx.fonts.face(*face).ok_or(LayoutError::Internal("paintObject: face"))?;
                        let fm = font.metrics();
                        let u = if fm.units_per_em == 0 { 1000.0 } else { f64::from(fm.units_per_em) };
                        let wd = if fm.us_win_descent == 0 { -f64::from(fm.descender) } else { f64::from(fm.us_win_descent) };
                        let desc = wd / u;
                        let sy2 = ih / jm::max(1.0, size_pt * 20.0);
                        m = [sx, 0.0, 0.0, sy2, x + ins.l - x0 * sx, y + ins.t + ih - ih * desc - gy * sy2];
                        // a TURNED shape centred vertically: Word centres the box of the INK of the
                        // turned text (oracle page-lab wmd)
                        let centred = tb.anchor.as_ref().and_then(|a| a.pos_v.align.as_deref()) == Some("center");
                        if !js_eq(jm::rem(rot, 180.0), 0.0)
                            && centred
                            && let Some((lo, hi)) = rotated_ink_y(cx, &c.items, &m, x + w / 2.0, y + h / 2.0, rot)
                        {
                            dy = y + h / 2.0 - (lo + hi) / 2.0;
                        }
                    }
                    art = Some((m, dy));
                }
                let art_dy = art.map_or(0.0, |a| a.1);
                if uo_jsnum::conv::truthy(art_dy) {
                    push(out, Item::XformPush { m: Some(Box::new([1.0, 0.0, 0.0, 1.0, 0.0, art_dy])) });
                }
                if uo_jsnum::conv::truthy(rot) {
                    push(out, Item::XformPush { m: Some(Box::new(rotation(x + w / 2.0, y + h / 2.0, rot))) });
                }
                if let Some((m, _)) = art {
                    push(out, Item::XformPush { m: Some(Box::new(m)) });
                    // Word writes WordArt text as CURVES in the PDF
                    for i in 0..c.items.len() {
                        let at = out.len();
                        super::push_copy(out, &c.items, i)?;
                        set_as_path(out, at);
                    }
                    push(out, Item::XformPop);
                } else if fr.rot != TbRot::None {
                    // vertical text: the content turned in the frame
                    push(out, Item::ClipPush { x: OptF64::some(x), y: OptF64::some(y), w: OptF64::some(w), h: OptF64::some(h) });
                    vertical_content(c, &fr, tb.v_anchor.as_deref(), x, y, w, h, out)?;
                    push(out, Item::ClipPop);
                } else {
                    let mut dy = y + ins.t;
                    let free = h - ins.t - ins.b - c.height;
                    match tb.v_anchor.as_deref() {
                        Some("ctr") if free > 0.0 => dy += free / 2.0,
                        Some("b") if free > 0.0 => dy += free,
                        _ => {}
                    }
                    push(out, Item::ClipPush { x: OptF64::some(x), y: OptF64::some(y), w: OptF64::some(w), h: OptF64::some(h) });
                    // a box turned off multiples of 90°: Word rasterizes the content — curves here
                    // (oracle hidetext/rot-tb); a GROUP's box: text with horizontal scale as curves
                    // (oracle txbxscale)
                    let raster = !js_eq(jm::rem(rot, 90.0), 0.0);
                    // a figure ENTIRELY outside the box: Word does not draw it (oracle tbover)
                    let outside = |rx: f64, ry: f64, rw: f64, rh: f64| ry >= y + h || ry + rh <= y || rx >= x + w || rx + rw <= x;
                    let mut skip = 0i32;
                    for i in 0..c.items.len() {
                        let Some(it) = c.items.list.items.get(i) else { continue };
                        if skip > 0 {
                            match it {
                                Item::ClipPush { .. } => skip += 1,
                                Item::ClipPop => skip -= 1,
                                _ => {}
                            }
                            continue;
                        }
                        if let Item::ClipPush { x: cx0, y: cy, w: cw, h: ch } = it
                            && let (Some(cy0), Some(cw), Some(ch)) = (cy.get(), cw.get(), ch.get())
                            && outside(cx0.or(0.0) + x + ins.l, cy0 + dy, cw, ch)
                        {
                            skip = 1;
                            continue;
                        }
                        if let Item::Image(im) = it
                            && outside(im.x + x + ins.l, im.y + dy, im.w, im.h)
                        {
                            continue;
                        }
                        let at = out.len();
                        out.push_shifted(&c.items, i, dy, x + ins.l).map_err(|_| cap())?;
                        if let Some(Item::Glyphs { run: rr, .. }) = out.list.items.get(at) {
                            let sx = out.list.run(*rr).map(|s| s.scale_x).unwrap_or(OptF64::NONE);
                            let scaled = sx.get().is_some_and(|v| !js_eq(v, 1.0));
                            if raster || (in_group && scaled) {
                                set_as_path(out, at);
                            }
                        }
                    }
                    push(out, Item::ClipPop);
                }
                if uo_jsnum::conv::truthy(rot) {
                    push(out, Item::XformPop);
                }
                if uo_jsnum::conv::truthy(art_dy) {
                    push(out, Item::XformPop);
                }
            }
        } else if let Some(gr) = e.group.as_deref() {
            for item in &gr.items {
                // the group's rotation/flip applied to the child (shapes oracle)
                let ch = crate::drawing::group_child(gr, item, x, y, w, h);
                let child_copy = copy || matches!(ch.run, Cow::Owned(_));
                let r2: &Run = &ch.run;
                let mut tc: Option<CellContent> = None;
                if let Some(tb) = r2.ext().and_then(|e| e.textbox.as_deref()) {
                    // fields (PAGE…) of the group's box are per page, as in a loose box (oracle
                    // hfgroup); (with the group's rotation/flip the child is a copy: the chain is
                    // the original box's)
                    let orig = item.run.ext().and_then(|e| e.textbox.as_deref());
                    let chained = match orig {
                        Some(orig) => self.chain_content(cx, orig, super::chain::ChainEnv { field_ps: Some(ps), hf: self.in_hf })?,
                        None => None,
                    };
                    if chained.is_some() {
                        tc = chained;
                    } else {
                        let blocks = self.tb_blocks(cx, tb)?;
                        let iw = jm::max(1.0, tb_frame(tb, theme, ch.w, ch.h).iw);
                        let env = self.tb_para_env(cx, tb, true);
                        tc = Some(self.static_at(cx, &blocks, iw, &env, &tb_ref(LineStory::GroupTextbox), Some(ps), Some(ps), 0.0)?);
                    }
                }
                let (ix, iy, iw, ih) = (ch.x, ch.y, ch.w, ch.h);
                self.paint_object(cx, r2, child_copy, ix, iy, iw, ih, tc.as_ref(), ps, true, out)?;
            }
        }
        Ok(())
    }
}

/// `rotatedInkY(items, m, cx, cy, rotDeg)` (engine.ts:4157): the vertical span (page twips) of the
/// glyphs' INK after the WordArt scale `m` and the rotation about (cx, cy).
fn rotated_ink_y(cx: &Cx<'_, '_>, items: &PassItems, m: &[f64; 6], ccx: f64, ccy: f64, rot_deg: f64) -> Option<(f64, f64)> {
    let r = (rot_deg * std::f64::consts::PI) / 180.0;
    let (cs, sn) = (jm::cos(r), jm::sin(r));
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for it in &items.list.items {
        let Item::Glyphs { face, size_pt, y, glyphs, run, .. } = it else { continue };
        let Some(font) = cx.fonts.face(*face) else { continue };
        let upm = if font.metrics().units_per_em == 0 { 1000.0 } else { f64::from(font.metrics().units_per_em) };
        let k = (size_pt * 20.0) / upm;
        let sxg = items.list.run(*run).and_then(|s| s.scale_x.get()).unwrap_or(1.0);
        for (cp, gx, gid) in items.list.glyphs_of(*glyphs) {
            let Some(ol) = font.glyph_outline(gid.unwrap_or_else(|| font.glyph_id(cp))) else { continue };
            for c in ol.iter() {
                let pts: Vec<(f64, f64)> = match *c {
                    uo_fonts::outline::OutlineCmd::Z => continue,
                    uo_fonts::outline::OutlineCmd::Q { cx, cy, x, y } => vec![(cx, cy), (x, y)],
                    uo_fonts::outline::OutlineCmd::M { x, y } | uo_fonts::outline::OutlineCmd::L { x, y } => vec![(x, y)],
                };
                for (px, py) in pts {
                    let lx = gx + px * k * sxg;
                    let ly = y - py * k;
                    let xx = m[0] * lx + m[2] * ly + m[4];
                    let yy = m[1] * lx + m[3] * ly + m[5];
                    let yr = ccy + (xx - ccx) * sn + (yy - ccy) * cs;
                    if yr < lo {
                        lo = yr;
                    }
                    if yr > hi {
                        hi = yr;
                    }
                }
            }
        }
    }
    (lo <= hi).then_some((lo, hi))
}
