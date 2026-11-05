//! D2b family: pictures: `insertImage` (two-phase), wrap/position/size/rotation/flip/crop/borders/format/adjust/transparency/recolour (`setImageAdjust` two-phase), `changeImage*` (two-phase), `compressImages` (two-phase), `resetImage`, contour points; editor `picture-ops.ts`, docx-core `picture-xml.ts`.
//!
//! Owned by frontier D2b-F7 (record `core/docs/modules/uo-docx-d2b/F7.md`). The `setImage*` methods
//! of the TS element all go through `editDrawing` (`drawing.rs`), so they work on shapes too (the
//! `setShape*` aliases are `DrawingCmd::OnShape`). Image decoding and encoding are the shell's jobs
//! ([`Job::DecodeImage`], [`Job::EncodeImage`]): a command that needs pixels returns
//! [`Outcome::NeedsJobs`] and takes its undo step in the completion only (a cancelled command
//! leaves no step).

use std::sync::Arc;

use uo_jsnum::math;

use super::super::command::{Command, CommandError, Job, JobId, JobResult, Outcome};
use super::super::policy::GuardArg;
use super::super::state::Selection;
use super::super::text::DocPosition;
use super::super::{ExecCx, Session};
use super::drawing::pure::{self, ZOp};
use super::drawing::{FormatArg, ObjFacts, ObjFrame, Path, Step, apply_format_arg, format_of, raw_of};
use crate::js::{JsNumber, Nullable, Str};
use crate::model::media::MediaRef;
use crate::model::picture::{A14Props, PicColor, PicLine, PicPos, Pic3D, PicBlipFx, SizeRel, WrapPolygon};
use crate::model::run::{ImageLayer, InlineImage, OleData, Run, RunExt};
use crate::text::Utf16;
use crate::word::obj;
use crate::write::emit::fx::{DistSide, GeomChange, PicWrapKind, PictureChange, Rect4, new_picture_xml, patch_picture};
use crate::write::emit::object::ShapeChange;

type Res = Result<Outcome, CommandError>;

/// `ImageLayoutInput` (the Layout dialog), points and percentages as the UI gives them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LayoutInput {
    pub wrap: Option<PicWrapKind>,
    pub wrap_text: Option<Str>,
    pub dist_pt: Option<[Option<f64>; 4]>,
    pub pos_h: Option<LayoutPos>,
    pub pos_v: Option<LayoutPos>,
    pub move_with_text: Option<bool>,
    pub lock_anchor: Option<bool>,
    pub allow_overlap: Option<bool>,
    pub layout_in_cell: Option<bool>,
    pub width_pt: Option<f64>,
    pub height_pt: Option<f64>,
    /// `relWidth`: `Some(None)` = `null` (absolute)
    pub rel_width: Option<Option<(Str, f64)>>,
    pub rel_height: Option<Option<(Str, f64)>>,
    pub lock_aspect: Option<bool>,
    pub rotation: Option<f64>,
}

/// `ImageLayoutInput.posH/posV`.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPos {
    pub relative_from: Str,
    pub align: Option<Str>,
    pub offset_pt: Option<f64>,
    pub pct: Option<f64>,
}

/// `arrangeImage` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrangeOp {
    Front,
    Back,
    Forward,
    Backward,
    FrontOfText,
    BehindText,
}

/// The kinds of `setImageEffect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    Shadow,
    Reflection,
    Glow,
    SoftEdge,
    Bevel,
    Rotation,
}

/// `setImageRecolor(preset | colour)`.
#[derive(Debug, Clone, PartialEq)]
pub enum RecolorArg {
    Preset(Str),
    Color(PicColor),
}

/// The ole arguments of `insertImage(data, ext, obj)`.
#[derive(Debug, Clone, PartialEq)]
pub struct OleArg {
    pub prog_id: Str,
    pub data: Arc<[u8]>,
    pub ext: Str,
    pub width_twips: f64,
    pub height_twips: f64,
}

/// `setImageAdjust` values (`undefined` = keep).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AdjustArg {
    pub sharpen: Option<f64>,
    pub bright: Option<f64>,
    pub contrast: Option<f64>,
    pub saturation: Option<f64>,
    pub temperature: Option<f64>,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum PictureCmd {
    /// `setImageWrap(kind)` (a picture wrapped tight/through asks the shell for its pixels)
    SetWrap { kind: PicWrapKind },
    /// `setImagePosition(preset)`
    SetPosition { preset: Str },
    /// `setImageMoveWithText(on)`
    SetMoveWithText { on: bool, facts: ObjFacts },
    /// `setImageLockAnchor(on)`
    SetLockAnchor { on: bool },
    /// `imageAlignTo = …`
    SetAlignTo { page: bool },
    /// `alignImage(kind, relativeTo)`; `rects`: the selected floating objects when several are
    Align { kind: super::drawing::AlignKind, relative_to: Option<Str>, rects: Vec<ObjFacts> },
    /// `setImageSize({ widthPt, heightPt, lockAspect })`
    SetSize { width_pt: Option<f64>, height_pt: Option<f64>, lock_aspect: Option<bool> },
    /// `setImageRotation(deg)`
    SetRotation { deg: f64 },
    /// `rotateImage(deg)`
    Rotate { deg: f64 },
    /// `flipImage(axis)`
    Flip { vertical: bool },
    /// `setImageAltText({ text, title, decorative })`
    SetAltText { text: Option<Str>, title: Option<Str>, decorative: Option<bool> },
    /// `arrangeImage(op)`
    Arrange { op: ArrangeOp },
    /// `setImageLayout(o)`
    SetLayout { o: LayoutInput, facts: ObjFacts },
    /// `setImageBorder({ color, weightPt, dash })`
    SetBorder { color: Option<Nullable<PicColor>>, weight_pt: Option<f64>, dash: Option<Str> },
    /// `setImageFormat(o)`
    SetFormat { fmt: FormatArg },
    /// `setImageEffect(kind, preset)`
    SetEffect { kind: EffectKind, preset: Option<Str> },
    /// `cropImageFillFit(mode)` with the natural size in points (`None`: not decoded, nothing happens)
    CropFillFit { fill: bool, natural_pt: Option<(f64, f64)> },
    /// `cropImageToAspect(w, h)`
    CropToAspect { w: f64, h: f64 },
    /// `cropImageToShape(prst)`
    CropToShape { prst: Str },
    /// `setImageTransparency(pct)`
    SetTransparency { pct: f64 },
    /// `setImageRecolor(preset | colour)`
    SetRecolor { arg: RecolorArg },
    /// `setImageTransparentColor(hex | null)`
    SetTransparentColor { hex: Option<Str> },
    /// `setImageCrop({ l, t, r, b })`
    SetCrop { l: f64, t: f64, r: f64, b: f64 },
    /// `resetImage(size)` with the natural size in points
    Reset { size: bool, natural_pt: Option<(f64, f64)> },
    /// `deleteSelectedImage()`
    DeleteSelectedImage,
    /// `insertImage(data, ext, obj)` (two-phase: the shell decodes the size)
    Insert { data: Arc<[u8]>, ext: Str, ole: Option<OleArg>, max_width_twips: Option<f64> },
    /// `setImageAdjust(o)` (two-phase: decode, then encode)
    SetAdjust { o: AdjustArg },
    /// `changeImage(data, ext)` (two-phase: the shell decodes the size)
    Change { data: Arc<[u8]>, ext: Str },
    /// `compressImages({ ppi, deleteCropped, all })` (two-phase: decode all, encode all)
    Compress { ppi: f64, delete_cropped: bool, all: bool },
}

/// What a command with more than one round of jobs carries between the rounds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stage {
    /// `setImageAdjust` after its decode: the encode of the processed pixels is asked
    Adjust { path: Path, next: AdjustNext, original: Option<(MediaRef, Str)>, cur_layer_rid: Option<Str> },
    /// `compressImages` after its decodes: the encodes are asked
    Compress { items: Vec<CompressItem> },
}

/// `setImageAdjust`: the values after the merge with the picture's own (`next`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct AdjustNext {
    pub saturation: Option<f64>,
    pub color_temp: Option<f64>,
    pub sharpen: Option<f64>,
    pub bright: Option<f64>,
    pub contrast: Option<f64>,
}

/// One image of `compressImages` between its decode and its encode.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompressItem {
    pub path: Path,
    pub crop: bool,
    pub jpeg: bool,
}

/// The size limit of a decoded picture the session accepts from the shell (M3): 64 Mpx.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

fn num(v: f64) -> JsNumber {
    JsNumber::Num(v)
}

fn pic(p: PictureChange) -> ShapeChange {
    ShapeChange { pic: p, ..ShapeChange::default() }
}

fn zero_crop() -> Rect4 {
    Rect4::default()
}

fn pos_of(p: &PicPos) -> (f64, Option<f64>) {
    (p.offset.map_or(0.0, JsNumber::to_number), p.pct.map(JsNumber::to_number))
}

/// `refRect(from, axis, f)`: origin and extent of a positioning reference.
fn ref_rect(from: &str, horizontal: bool, f: &ObjFrame) -> (f64, f64) {
    if horizontal {
        match from {
            "page" => (0.0, f.page_w),
            "margin" => (f.mar_l, f.page_w - f.mar_l - f.mar_r),
            "leftMargin" | "insideMargin" => (0.0, f.mar_l),
            "rightMargin" | "outsideMargin" => (f.page_w - f.mar_r, f.mar_r),
            _ => (f.col_x, f.col_w),
        }
    } else {
        match from {
            "page" => (0.0, f.page_h),
            "margin" => (f.body_top, f.page_h - f.body_top - f.mar_b),
            "topMargin" | "insideMargin" => (0.0, f.mar_t),
            "bottomMargin" | "outsideMargin" => (f.page_h - f.mar_b, f.mar_b),
            _ => (f.para_top, 0.0),
        }
    }
}

/// `posFor(cur, axis, xy, frame)`: the position `xy` (page twips) in the reference of `cur`.
pub(crate) fn pos_for(cur: Option<&PicPos>, horizontal: bool, xy: f64, frame: &ObjFrame) -> Option<PicPos> {
    let cur = cur?;
    let (o, _) = ref_rect(&cur.relative_from, horizontal, frame);
    if cur.pct.is_some() && cur.align.is_none() {
        let span = if horizontal { frame.page_w } else { frame.page_h };
        return Some(PicPos {
            relative_from: cur.relative_from.clone(),
            pct: Some(num(math::round(((xy - o) / span) * 100_000.0))),
            fallback: Some(num(math::round(xy * 635.0))),
            ..PicPos::default()
        });
    }
    Some(PicPos { relative_from: cur.relative_from.clone(), offset: Some(num(math::round((xy - o) * 635.0))), ..PicPos::default() })
}

/// `Math.round((((deg % 360) + 360) % 360) * 60000) % 21600000`.
fn rot_of(deg: f64) -> f64 {
    math::rem(math::round(math::rem(math::rem(deg, 360.0) + 360.0, 360.0) * 60000.0), 21_600_000.0)
}

impl Session {
    fn done8(&mut self) -> Res {
        Ok(Outcome::Done(self.change_set()))
    }

    pub(crate) fn exec_pictures(&mut self, cmd: PictureCmd, cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            PictureCmd::SetWrap { kind } => return self.set_image_wrap(kind, None),
            PictureCmd::SetPosition { preset } => {
                if &*preset == "inline" {
                    return self.set_image_wrap(PicWrapKind::Inline, None);
                }
                let mut it = preset.splitn(2, '-');
                let (v, h) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
                let Some(path) = self.draw_path() else { return self.done8() };
                if self.locked() {
                    return self.done8();
                }
                let square = self.run_ref(&path).and_then(format_of).is_some_and(|f| &*f.base().wrap == "square");
                if !square {
                    let out = self.set_image_wrap(PicWrapKind::Square, None)?;
                    if matches!(out, Outcome::NeedsJobs(_)) {
                        return Ok(out);
                    }
                }
                let (v, h): (Str, Str) = (v.into(), h.into());
                self.edit_drawing(false, &mut |_, _| {
                    Some(pic(PictureChange {
                        pos_h: Some(PicPos { relative_from: "margin".into(), align: Some(h.clone()), ..PicPos::default() }),
                        pos_v: Some(PicPos { relative_from: "margin".into(), align: Some(v.clone()), ..PicPos::default() }),
                        ..PictureChange::default()
                    }))
                });
            }
            PictureCmd::SetMoveWithText { on, facts } => self.set_move_with_text(on, &facts),
            PictureCmd::SetLockAnchor { on } => {
                self.edit_drawing(false, &mut |f, _| {
                    if f.base().inline { None } else { Some(pic(PictureChange { locked: Some(on), ..PictureChange::default() })) }
                });
            }
            PictureCmd::SetAlignTo { page } => self.f7.align_page = page,
            PictureCmd::Align { kind, relative_to, rects } => return self.align_image(kind, relative_to.as_deref(), &rects, cx),
            PictureCmd::SetSize { width_pt, height_pt, lock_aspect } => {
                self.edit_drawing(false, &mut |f, _| {
                    let b = f.base();
                    let lock = lock_aspect.unwrap_or(b.no_change_aspect);
                    let (mut cx0, mut cy0) = (b.cx.to_number(), b.cy.to_number());
                    let (ocx, ocy) = (cx0, cy0);
                    if let Some(w) = width_pt {
                        let n = math::max(1.0, math::round(w * 12700.0));
                        if lock && truthy(cx0) {
                            cy0 = math::round((cy0 * n) / cx0);
                        }
                        cx0 = n;
                    }
                    if let Some(h) = height_pt {
                        let n = math::max(1.0, math::round(h * 12700.0));
                        if lock && truthy(cy0) && width_pt.is_none() {
                            cx0 = math::round((cx0 * n) / cy0);
                        }
                        cy0 = n;
                    }
                    let mut ch = PictureChange { size: Some((cx0, cy0)), ..PictureChange::default() };
                    let aspect_key = lock_aspect.is_some_and(|l| l != b.no_change_aspect);
                    if aspect_key {
                        ch.no_change_aspect = lock_aspect;
                    }
                    if b.size_rel_h.as_ref().and_then(|s| s.pct.as_f64()).is_some_and(truthy) {
                        ch.size_rel_h = b.size_rel_h.as_ref().map(|s| Nullable::Val(SizeRel { relative_from: s.relative_from.clone(), pct: num(0.0), ..Default::default() }));
                    }
                    if b.size_rel_v.as_ref().and_then(|s| s.pct.as_f64()).is_some_and(truthy) {
                        ch.size_rel_v = b.size_rel_v.as_ref().map(|s| Nullable::Val(SizeRel { relative_from: s.relative_from.clone(), pct: num(0.0), ..Default::default() }));
                    }
                    if cx0 == ocx && cy0 == ocy && !aspect_key { None } else { Some(pic(ch)) }
                });
            }
            PictureCmd::SetRotation { deg } => self.set_rotation(deg),
            PictureCmd::Rotate { deg } => {
                let cur = self.draw_path().and_then(|p| self.run_ref(&p).and_then(format_of)).map(|f| f.base().rot.to_number());
                if let Some(r) = cur {
                    self.set_rotation(r / 60000.0 + deg);
                }
            }
            PictureCmd::Flip { vertical } => {
                self.edit_drawing(false, &mut |f, _| {
                    let b = f.base();
                    Some(pic(if vertical {
                        PictureChange { flip_v: Some(!b.flip_v), ..PictureChange::default() }
                    } else {
                        PictureChange { flip_h: Some(!b.flip_h), ..PictureChange::default() }
                    }))
                });
            }
            PictureCmd::SetAltText { text, title, decorative } => {
                self.edit_drawing(false, &mut |f, _| {
                    let d = &f.base().doc_pr;
                    let mut ch = PictureChange::default();
                    if let Some(t) = &text
                        && **t != *d.descr.as_deref().unwrap_or("")
                    {
                        ch.descr = Some(t.to_string());
                    }
                    if let Some(t) = &title
                        && **t != *d.title.as_deref().unwrap_or("")
                    {
                        ch.title = Some(t.to_string());
                    }
                    if let Some(x) = decorative
                        && x != d.decorative.unwrap_or(false)
                    {
                        ch.decorative = Some(x);
                    }
                    (ch != PictureChange::default()).then(|| pic(ch))
                });
            }
            PictureCmd::Arrange { op } => return self.arrange_image(op),
            PictureCmd::SetLayout { o, facts } => return self.set_image_layout(&o, &facts),
            PictureCmd::SetBorder { color, weight_pt, dash } => {
                self.edit_drawing(false, &mut |f, _| {
                    let line = f.base().line.as_ref().and_then(Nullable::as_val);
                    if matches!(color, Some(Nullable::Null)) {
                        if matches!(line.and_then(|l| l.fill.as_ref()), Some(Nullable::Null)) {
                            return None;
                        }
                        let mut l = line.map(|l| (**l).clone()).unwrap_or_default();
                        obj::set(&mut l, "fill", |x| x.fill = Some(Nullable::Null));
                        return Some(pic(PictureChange { line: Some(Nullable::Val(l)), ..PictureChange::default() }));
                    }
                    let mut base = match line {
                        Some(l) if matches!(l.fill, Some(Nullable::Val(_))) => (**l).clone(),
                        _ => PicLine {
                            w: Some(num(9525.0)),
                            fill: Some(Nullable::Val(PicColor { kind: "schemeClr".into(), val: "tx1".into(), ..PicColor::default() })),
                            ..PicLine::default()
                        },
                    };
                    if let Some(Nullable::Val(c)) = &color {
                        obj::set(&mut base, "fill", |x| x.fill = Some(Nullable::Val(c.clone())));
                    }
                    if let Some(w) = weight_pt {
                        let v = math::max(0.0, math::round(w * 12700.0));
                        obj::set(&mut base, "w", |x| x.w = Some(num(v)));
                    }
                    if let Some(d) = &dash {
                        if &**d == "solid" {
                            obj::delete(&mut base, "dash");
                        } else {
                            obj::set(&mut base, "dash", |x| x.dash = Some(d.clone()));
                        }
                    }
                    Some(pic(PictureChange { line: Some(Nullable::Val(base)), ..PictureChange::default() }))
                });
            }
            PictureCmd::SetFormat { fmt } => {
                let mut ch = PictureChange::default();
                apply_format_arg(&mut ch, &fmt);
                self.edit_drawing(false, &mut |_, _| (ch != PictureChange::default()).then(|| pic(ch.clone())));
            }
            PictureCmd::SetEffect { kind, preset } => self.set_image_effect(kind, preset.as_deref()),
            PictureCmd::CropFillFit { fill, natural_pt } => {
                let Some((nw, nh)) = natural_pt else { return self.done8() };
                self.edit_drawing(true, &mut |f, _| {
                    let b = f.base();
                    let r = pure::fill_fit_crop(fill, b.cx.to_number(), b.cy.to_number(), nw, nh);
                    Some(pic(PictureChange { src_rect: Some(r), ..PictureChange::default() }))
                });
            }
            PictureCmd::CropToAspect { w, h } => {
                self.edit_drawing(true, &mut |f, _| {
                    let b = f.base();
                    let s = Rect4 { l: b.src_rect.l.to_number(), t: b.src_rect.t.to_number(), r: b.src_rect.r.to_number(), b: b.src_rect.b.to_number() };
                    let (crop, cx0, cy0) = pure::aspect_crop(b.cx.to_number(), b.cy.to_number(), s, w / h);
                    Some(pic(PictureChange { src_rect: Some(crop), size: Some((cx0, cy0)), ..PictureChange::default() }))
                });
            }
            PictureCmd::CropToShape { prst } => {
                self.edit_drawing(true, &mut |f, _| {
                    if *f.base().geom == *prst {
                        None
                    } else {
                        Some(pic(PictureChange { geom: Some(GeomChange { prst: prst.to_string(), adj: None }), ..PictureChange::default() }))
                    }
                });
            }
            PictureCmd::SetTransparency { pct } => {
                self.edit_drawing(true, &mut |f, _| {
                    let amt = math::round((100.0 - math::max(0.0, math::min(100.0, pct))) * 1000.0);
                    let b = f.base();
                    let cur = b.blip.alpha_mod_fix.and_then(JsNumber::as_f64).unwrap_or(100_000.0);
                    if cur == amt {
                        return None;
                    }
                    let mut fx = b.blip.clone();
                    fx.a14 = None;
                    obj::set(&mut fx, "alphaModFix", |x| x.alpha_mod_fix = Some(num(amt)));
                    Some(pic(PictureChange { blip_fx: Some(fx), ..PictureChange::default() }))
                });
            }
            PictureCmd::SetRecolor { arg } => {
                self.edit_drawing(true, &mut |f, _| {
                    let mut keep = f.base().blip.clone();
                    for k in ["a14", "grayscl", "duotone", "lum", "biLevel"] {
                        obj::delete(&mut keep, k);
                    }
                    match &arg {
                        RecolorArg::Color(pc) => {
                            let mut mods = pc.mods.clone().unwrap_or_default();
                            mods.push((Str::from("shade"), num(45000.0)));
                            mods.push((Str::from("satMod"), num(135000.0)));
                            let c = PicColor { mods: Some(mods), ..pc.clone() };
                            let black = PicColor { kind: "prstClr".into(), val: "black".into(), ..PicColor::default() };
                            obj::set(&mut keep, "duotone", |x| x.duotone = Some(Box::new((black.clone(), c.clone()))));
                            Some(pic(PictureChange { blip_fx: Some(keep), ..PictureChange::default() }))
                        }
                        RecolorArg::Preset(id) => {
                            let p = pure::recolor_preset(id)?;
                            merge_blip(&mut keep, &p);
                            Some(pic(PictureChange { blip_fx: Some(keep), ..PictureChange::default() }))
                        }
                    }
                });
            }
            PictureCmd::SetTransparentColor { hex } => {
                self.edit_drawing(true, &mut |f, _| {
                    let mut fx = f.base().blip.clone();
                    fx.a14 = None;
                    match hex.as_deref().filter(|h| !h.is_empty()) {
                        None => {
                            fx.clr_change.as_ref()?;
                            obj::delete(&mut fx, "clrChange");
                        }
                        Some(h) => {
                            let c = PicColor { kind: "srgbClr".into(), val: uo_text::case::to_uppercase_str(h).into(), ..PicColor::default() };
                            let to = PicColor { mods: Some(vec![(Str::from("alpha"), num(0.0))]), ..c.clone() };
                            obj::set(&mut fx, "clrChange", |x| {
                                x.clr_change = Some(Box::new(crate::model::picture::ClrChange { from: c.clone(), to: to.clone(), use_a: None, ..Default::default() }));
                            });
                        }
                    }
                    Some(pic(PictureChange { blip_fx: Some(fx), ..PictureChange::default() }))
                });
            }
            PictureCmd::SetCrop { l, t, r, b } => {
                self.edit_drawing(true, &mut |f, _| {
                    let fb = f.base();
                    let s = &fb.src_rect;
                    let vis_w0 = 1.0 - (s.l.to_number() + s.r.to_number()) / 100_000.0;
                    let vis_h0 = 1.0 - (s.t.to_number() + s.b.to_number()) / 100_000.0;
                    let vis_w = 1.0 - l - r;
                    let vis_h = 1.0 - t - b;
                    if vis_w <= 0.001 || vis_h <= 0.001 || vis_w.is_nan() || vis_h.is_nan() {
                        return None;
                    }
                    let cx0 = math::round((fb.cx.to_number() / vis_w0) * vis_w);
                    let cy0 = math::round((fb.cy.to_number() / vis_h0) * vis_h);
                    let src = Rect4 {
                        l: math::round(l * 100_000.0),
                        t: math::round(t * 100_000.0),
                        r: math::round(r * 100_000.0),
                        b: math::round(b * 100_000.0),
                    };
                    Some(pic(PictureChange { src_rect: Some(src), size: Some((cx0, cy0)), ..PictureChange::default() }))
                });
            }
            PictureCmd::Reset { size, natural_pt } => self.reset_image(size, natural_pt),
            PictureCmd::DeleteSelectedImage => {
                if self.sel_path().is_some() && !self.locked() {
                    self.delete_selected_float();
                }
            }
            PictureCmd::Insert { ref data, ref ext, ref ole, .. } => {
                if self.locked() || self.paras().is_none() {
                    return self.done8();
                }
                if let Some(o) = ole {
                    let (w, h) = (o.width_twips, o.height_twips);
                    return self.insert_image_apply(cmd.clone(), w, h);
                }
                let id = self.job_id();
                let job = Job::DecodeImage { id, bytes: Arc::clone(data), ext: ext.clone() };
                return Ok(self.defer(Command::Picture(cmd), vec![job]));
            }
            PictureCmd::SetAdjust { o } => return self.set_image_adjust_start(&o),
            PictureCmd::Change { ref data, ref ext } => {
                if self.pic_path().is_none() || self.locked() {
                    return self.done8();
                }
                let id = self.job_id();
                let job = Job::DecodeImage { id, bytes: Arc::clone(data), ext: ext.clone() };
                return Ok(self.defer(Command::Picture(cmd), vec![job]));
            }
            PictureCmd::Compress { ppi, delete_cropped, all } => return self.compress_start(ppi, delete_cropped, all),
        }
        self.done8()
    }

    // ── small commands ─────────────────────────────────────────────────────────────────────

    fn set_rotation(&mut self, deg: f64) {
        let r = rot_of(deg);
        self.edit_drawing(false, &mut |f, _| {
            if f.base().rot.to_number() == r { None } else { Some(pic(PictureChange { rot: Some(r), ..PictureChange::default() })) }
        });
    }

    fn set_move_with_text(&mut self, on: bool, facts: &ObjFacts) {
        let Some(path) = self.draw_path() else { return };
        let Some(f) = self.run_ref(&path).and_then(format_of) else { return };
        if f.base().inline {
            return;
        }
        let to: Str = if on { "paragraph" } else { "page" }.into();
        let (frame, rect) = (facts.frame, facts.rect);
        self.edit_drawing(false, &mut |f, _| {
            let cur = f.base().pos_v.as_ref();
            let pv = match (frame, rect) {
                (Some(fr), Some(r)) => {
                    let probe = PicPos { relative_from: to.clone(), offset: Some(num(0.0)), ..PicPos::default() };
                    pos_for(Some(&probe), false, r.y, &fr)?
                }
                _ => PicPos { relative_from: to.clone(), offset: Some(num(cur.map_or(0.0, |p| pos_of(p).0))), ..PicPos::default() },
            };
            Some(pic(PictureChange { pos_v: Some(pv), ..PictureChange::default() }))
        });
    }

    fn arrange_image(&mut self, op: ArrangeOp) -> Res {
        let Some(path) = self.draw_path() else { return self.done8() };
        let Some(f) = self.run_ref(&path).and_then(format_of) else { return self.done8() };
        let b = f.base();
        if b.inline {
            return self.done8();
        }
        match op {
            ArrangeOp::FrontOfText => {
                if &*b.wrap == "behind" {
                    return self.set_image_wrap(PicWrapKind::Front, None);
                }
                return self.done8();
            }
            ArrangeOp::BehindText => {
                if &*b.wrap != "behind" {
                    return self.set_image_wrap(PicWrapKind::Behind, None);
                }
                return self.done8();
            }
            _ => {}
        }
        let mine = b.relative_height.and_then(JsNumber::as_f64).unwrap_or(pure::Z_FIRST);
        let zop = match op {
            ArrangeOp::Front => ZOp::Front,
            ArrangeOp::Back => ZOp::Back,
            ArrangeOp::Forward => ZOp::Forward,
            _ => ZOp::Backward,
        };
        let z = pure::z_for(zop, mine, &self.z_values(Some(&path)));
        self.edit_drawing(false, &mut |_, _| {
            if z == mine { None } else { Some(pic(PictureChange { relative_height: Some(z), ..PictureChange::default() })) }
        });
        self.done8()
    }

    fn align_image(&mut self, kind: super::drawing::AlignKind, relative_to: Option<&str>, rects: &[ObjFacts], cx: &mut ExecCx<'_>) -> Res {
        use super::drawing::AlignKind as K;
        if relative_to.is_none() && rects.len() > 1 {
            return self.align_objects(kind, rects, cx);
        }
        let Some(path) = self.draw_path() else { return self.done8() };
        if self.run_ref(&path).and_then(format_of).is_none_or(|f| f.base().inline) {
            return self.done8();
        }
        let rel: Str = relative_to.map_or_else(|| if self.f7.align_page { "page" } else { "margin" }, |r| r).into();
        self.edit_drawing(false, &mut |f, _| {
            let b = f.base();
            match kind {
                K::Left | K::Center | K::Right => {
                    let al = match kind {
                        K::Left => "left",
                        K::Center => "center",
                        _ => "right",
                    };
                    if b.pos_h.as_ref().is_some_and(|p| *p.relative_from == *rel && p.align.as_deref() == Some(al)) {
                        return None;
                    }
                    Some(pic(PictureChange { pos_h: Some(PicPos { relative_from: rel.clone(), align: Some(al.into()), ..PicPos::default() }), ..PictureChange::default() }))
                }
                _ => {
                    let v = match kind {
                        K::Middle => "center",
                        K::Top => "top",
                        _ => "bottom",
                    };
                    if b.pos_v.as_ref().is_some_and(|p| *p.relative_from == *rel && p.align.as_deref() == Some(v)) {
                        return None;
                    }
                    Some(pic(PictureChange { pos_v: Some(PicPos { relative_from: rel.clone(), align: Some(v.into()), ..PicPos::default() }), ..PictureChange::default() }))
                }
            }
        });
        self.done8()
    }

    fn set_image_effect(&mut self, kind: EffectKind, preset: Option<&str>) {
        self.edit_drawing(false, &mut |f, _| {
            let mut ch = PictureChange::default();
            match kind {
                EffectKind::Shadow => match preset {
                    Some(id) => ch.shadow = Some(Nullable::Val(pure::shadow_preset(id)?)),
                    None => ch.shadow = Some(Nullable::Null),
                },
                EffectKind::Reflection => match preset {
                    Some(id) => ch.reflection = Some(Nullable::Val(pure::reflection_preset(id)?)),
                    None => ch.reflection = Some(Nullable::Null),
                },
                EffectKind::Glow => match preset {
                    None | Some("") => ch.glow = Some(Nullable::Null),
                    Some(p) => {
                        let (size, acc) = parse_glow(p)?;
                        let rad = (size >= 0.0 && size < 4.0).then(|| uo_jsnum::conv::to_uint32(size) as usize).and_then(|i| pure::GLOW_SIZES.get(i)).copied().unwrap_or(pure::GLOW_SIZES[0]);
                        ch.glow = Some(Nullable::Val(pure::glow_preset(rad, acc)));
                    }
                },
                EffectKind::SoftEdge => {
                    let v = preset.filter(|p| !p.is_empty()).map(uo_jsnum::parse::to_number).filter(|n| truthy(*n));
                    ch.soft_edge = Some(v.map_or(Nullable::Null, Nullable::Val));
                }
                EffectKind::Bevel => {
                    let mut t: Pic3D = f.base().three_d.as_deref().cloned().unwrap_or_default();
                    match preset.filter(|p| !p.is_empty()) {
                        Some(p) => {
                            let b = pure::bevel_3d(p);
                            obj::set(&mut t, "bevelT", |x| x.bevel_t = b.bevel_t.clone());
                            if t.camera.is_none() {
                                obj::set(&mut t, "camera", |x| x.camera = b.camera.clone());
                            }
                            if t.light_rig.is_none() {
                                obj::set(&mut t, "lightRig", |x| x.light_rig = b.light_rig.clone());
                            }
                        }
                        None => obj::delete(&mut t, "bevelT"),
                    }
                    let keep = t.bevel_t.is_some() || t.camera.as_ref().is_some_and(|c| *c.prst != *"orthographicFront");
                    ch.three_d = Some(if keep { Nullable::Val(t) } else { Nullable::Null });
                }
                EffectKind::Rotation => {
                    let mut t: Pic3D = f.base().three_d.as_deref().cloned().unwrap_or_default();
                    let prst: Str = preset.filter(|p| !p.is_empty()).unwrap_or("orthographicFront").into();
                    obj::set(&mut t, "camera", |x| x.camera = Some(crate::model::picture::Camera { prst: prst.clone(), ..Default::default() }));
                    if t.light_rig.is_none() {
                        obj::set(&mut t, "lightRig", |x| {
                            x.light_rig = Some(crate::model::picture::LightRig { rig: "threePt".into(), dir: "t".into(), ..Default::default() });
                        });
                    }
                    let keep = t.bevel_t.is_some() || preset.is_some_and(|p| !p.is_empty());
                    ch.three_d = Some(if keep { Nullable::Val(t) } else { Nullable::Null });
                }
            }
            Some(pic(ch))
        });
    }

    // ── wrap (two-phase for a contour), placement ──────────────────────────────────────────

    /// `setImageWrap(kind)`; `poly`: the contour the shell's pixels gave (the second phase).
    pub(crate) fn set_image_wrap(&mut self, kind: PicWrapKind, poly: Option<Option<Vec<[f64; 2]>>>) -> Res {
        let Some(path) = self.draw_path() else { return self.done8() };
        if self.locked() {
            return self.done8();
        }
        let Some(run) = self.run_ref(&path) else { return self.done8() };
        let Some(f) = format_of(run) else { return self.done8() };
        if *f.base().wrap == *kind.as_str() {
            return self.done8();
        }
        let is_img = run.ext().is_some_and(|e| e.image.is_some());
        let wants_contour = is_img && matches!(kind, PicWrapKind::Tight | PicWrapKind::Through);
        if wants_contour && poly.is_none() {
            let probe = run.ext().and_then(|e| e.image.as_deref()).and_then(|i| {
                let ok = i.svg.is_none() && matches!(uo_text::case::to_lowercase_str(&i.ext).as_str(), "png" | "gif" | "webp" | "tif" | "tiff" | "bmp");
                ok.then(|| (i.data.0, i.ext.clone()))
            });
            if let Some((mid, ext)) = probe
                && let Some(bytes) = self.doc.media.get_arc(mid).cloned()
            {
                let id = self.job_id();
                let job = Job::DecodeImage { id, bytes, ext };
                return Ok(self.defer(Command::Picture(PictureCmd::SetWrap { kind }), vec![job]));
            }
        }
        let polygon = poly.flatten().map(|pts| {
            Nullable::Val(WrapPolygon { edited: false, pts: pts.into_iter().map(|[x, y]| (num(x), num(y))).collect(), ..Default::default() })
        });
        let inline_f = f.base().inline;
        if !inline_f || kind == PicWrapKind::Inline {
            self.edit_drawing(false, &mut |_, _| {
                Some(pic(PictureChange { wrap: Some(kind), polygon: polygon.clone(), ..PictureChange::default() }))
            });
            return self.done8();
        }
        // in line → floating: the object floats at the origin; the shell places it with `PlaceAt`
        // once the layout gave it a frame
        let zs = self.z_values(Some(&path));
        let z = zs.iter().copied().fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| math::max(m, v)))).map_or(pure::Z_FIRST, |m| m + 1024.0);
        let ch = pic(PictureChange { wrap: Some(kind), at: Some((0.0, 0.0)), new_relative_height: Some(z), polygon, ..PictureChange::default() });
        let Some(xml) = self.patch_drawing_pub(&path, &ch) else { return self.done8() };
        self.begin_step(false, &[]);
        self.apply_drawing_xml(&path, xml, true);
        self.f7.placed = Some(path.clone());
        let p = if self.run_ref(&path).is_some() { Some(path) } else { None };
        self.set_float_sel(p);
        self.after_picture_edit();
        self.done8()
    }

    /// The second half of a command that floats an object: the offsets from the frame the layout
    /// gave the placed object (`posH` from the column, `posV` from the paragraph), in the step the
    /// first half took.
    pub(crate) fn place_at(&mut self, frame: &ObjFrame, x: f64, y: f64) {
        let Some(path) = self.f7.placed.take() else { return };
        let ch = pic(PictureChange {
            pos_h: Some(PicPos { relative_from: "column".into(), offset: Some(num(math::round((x - frame.col_x) * 635.0))), ..PicPos::default() }),
            pos_v: Some(PicPos { relative_from: "paragraph".into(), offset: Some(num(math::round((y - frame.para_top) * 635.0))), ..PicPos::default() }),
            ..PictureChange::default()
        });
        if !super::drawing::change_is_finite(&ch) {
            return;
        }
        if let Some(xml) = self.patch_drawing_pub(&path, &ch) {
            self.apply_drawing_xml(&path, xml, false);
        }
    }

    // ── the Layout dialog ──────────────────────────────────────────────────────────────────

    fn set_image_layout(&mut self, o: &LayoutInput, facts: &ObjFacts) -> Res {
        let Some(path) = self.draw_path() else { return self.done8() };
        if self.locked() {
            return self.done8();
        }
        let mark = self.history.undo_len();
        let cur_wrap = self.run_ref(&path).and_then(format_of).map(|f| f.base().wrap.clone());
        if let (Some(w), Some(cur)) = (o.wrap, cur_wrap)
            && *cur != *w.as_str()
        {
            let out = self.set_image_wrap(w, None)?;
            if matches!(out, Outcome::NeedsJobs(_)) {
                // a contour asked for (only Tight/Through on a picture): the layout dialog has no
                // second phase; the wrap goes without the contour (F7-D3)
                self.cancel_pending();
                self.f7.clear_pending();
                let _ = self.set_image_wrap(w, Some(None))?;
            }
        }
        let frame = facts.frame;
        self.edit_drawing(false, &mut |f, _| {
            let b = f.base();
            let mut ch = PictureChange::default();
            if !b.inline {
                if let Some(wt) = &o.wrap_text
                    && **wt != *b.wrap_text
                {
                    ch.wrap_text = Some(wt.to_string());
                }
                if let Some(d) = &o.dist_pt {
                    let mut v = Vec::new();
                    for (side, val) in [DistSide::T, DistSide::B, DistSide::L, DistSide::R].into_iter().zip(d.iter()) {
                        if let Some(p) = val {
                            v.push((side, math::round(p * 12700.0)));
                        }
                    }
                    ch.dist = Some(v);
                }
                let pos = |p: &LayoutPos, horizontal: bool| -> PicPos {
                    if let Some(a) = &p.align {
                        return PicPos { relative_from: p.relative_from.clone(), align: Some(a.clone()), ..PicPos::default() };
                    }
                    if let Some(pct) = p.pct {
                        let (ro, span) = match frame {
                            Some(fr) => (ref_rect(&p.relative_from, horizontal, &fr).0, if horizontal { fr.page_w } else { fr.page_h }),
                            None => (0.0, 0.0),
                        };
                        return PicPos {
                            relative_from: p.relative_from.clone(),
                            pct: Some(num(math::round(pct * 1000.0))),
                            fallback: Some(num(math::round((ro + (pct / 100.0) * span) * 635.0))),
                            ..PicPos::default()
                        };
                    }
                    PicPos { relative_from: p.relative_from.clone(), offset: Some(num(math::round(p.offset_pt.unwrap_or(0.0) * 12700.0))), ..PicPos::default() }
                };
                if let Some(p) = &o.pos_h {
                    ch.pos_h = Some(pos(p, true));
                }
                if let Some(p) = &o.pos_v {
                    ch.pos_v = Some(pos(p, false));
                }
                if let Some(l) = o.lock_anchor
                    && l != b.locked
                {
                    ch.locked = Some(l);
                }
                if let Some(l) = o.allow_overlap
                    && l != b.allow_overlap
                {
                    ch.allow_overlap = Some(l);
                }
                if let Some(l) = o.layout_in_cell
                    && l != b.layout_in_cell
                {
                    ch.layout_in_cell = Some(l);
                }
                if let Some(rw) = &o.rel_width {
                    ch.size_rel_h = Some(Nullable::Val(match rw {
                        Some((from, pct)) => SizeRel { relative_from: from.clone(), pct: num(math::round(pct * 1000.0)), ..Default::default() },
                        None => SizeRel { relative_from: b.size_rel_h.as_ref().map_or_else(|| "page".into(), |s| s.relative_from.clone()), pct: num(0.0), ..Default::default() },
                    }));
                }
                if let Some(rh) = &o.rel_height {
                    ch.size_rel_v = Some(Nullable::Val(match rh {
                        Some((from, pct)) => SizeRel { relative_from: from.clone(), pct: num(math::round(pct * 1000.0)), ..Default::default() },
                        None => SizeRel { relative_from: b.size_rel_v.as_ref().map_or_else(|| "page".into(), |s| s.relative_from.clone()), pct: num(0.0), ..Default::default() },
                    }));
                }
            }
            if o.width_pt.is_some() || o.height_pt.is_some() {
                let cx0 = o.width_pt.map_or(b.cx.to_number(), |w| math::max(1.0, math::round(w * 12700.0)));
                let cy0 = o.height_pt.map_or(b.cy.to_number(), |h| math::max(1.0, math::round(h * 12700.0)));
                if !(cx0 == b.cx.to_number() && cy0 == b.cy.to_number()) {
                    ch.size = Some((cx0, cy0));
                }
            }
            if let Some(l) = o.lock_aspect
                && l != b.no_change_aspect
            {
                ch.no_change_aspect = Some(l);
            }
            if let Some(r) = o.rotation {
                let r = rot_of(r);
                if r != b.rot.to_number() {
                    ch.rot = Some(r);
                }
            }
            (ch != PictureChange::default()).then(|| pic(ch))
        });
        if let Some(m) = o.move_with_text {
            let cur = self.run_ref(&path).and_then(format_of).map(|f| {
                let b = f.base();
                (b.inline, b.pos_v.as_ref().is_some_and(|p| &*p.relative_from == "paragraph" || &*p.relative_from == "line"))
            });
            if let Some((inline, now)) = cur
                && !inline
                && now != m
            {
                self.set_move_with_text(m, facts);
            }
        }
        // `oneUndo`: the first of the steps stays
        if self.history.undo_len() > mark + 1 {
            self.history.collapse_to(mark + 1);
        }
        self.done8()
    }

    // ── reset ──────────────────────────────────────────────────────────────────────────────

    fn reset_image(&mut self, size: bool, natural_pt: Option<(f64, f64)>) {
        let Some(path) = self.pic_path() else { return };
        if self.locked() {
            return;
        }
        let Some(f) = self.run_ref(&path).and_then(format_of) else { return };
        let b = f.base().clone();
        let mut ch = PictureChange {
            line: Some(Nullable::Null),
            shadow: Some(Nullable::Null),
            reflection: Some(Nullable::Null),
            glow: Some(Nullable::Null),
            soft_edge: Some(Nullable::Null),
            three_d: Some(Nullable::Null),
            blip_fx: Some(PicBlipFx::default()),
            ..PictureChange::default()
        };
        if b.blip.a14.is_some() {
            ch.a14 = Some(Nullable::Null);
            ch.embed = Some("uw:media".into());
        }
        if size {
            if let Some((nw, nh)) = natural_pt {
                ch.size = Some((math::round(nw * 12700.0), math::round(nh * 12700.0)));
            }
            ch.src_rect = Some(zero_crop());
            if &*b.geom != "rect" {
                ch.geom = Some(GeomChange { prst: "rect".into(), adj: None });
            }
        }
        if !super::drawing::change_is_finite(&pic(ch.clone())) {
            return;
        }
        // the layer is the original: it comes back as the picture
        let layer = self.run_ref(&path).and_then(|r| r.ext()).and_then(|e| e.image.as_deref()).and_then(|i| i.layer.clone());
        let raw = self.run_ref(&path).and_then(raw_of).map(str::to_owned);
        let Some(raw) = raw else { return };
        let Ok(xml) = patch_picture(&raw, &ch) else { return };
        self.begin_step(false, &[]);
        if let Some(l) = layer
            && let Some(img) = self.run_mut(&path).and_then(|r| r.ext_mut().image.as_deref_mut())
        {
            obj::set(img, "data", |i| i.data = l.data);
            obj::set(img, "ext", |i| i.ext = l.ext.clone());
            obj::delete(img, "layer");
        }
        self.apply_drawing_xml(&path, xml, true);
        self.after_picture_edit();
    }

    // ── two-phase: insert ──────────────────────────────────────────────────────────────────

    fn content_width_twips(&self, max: Option<f64>) -> f64 {
        if let Some(m) = max {
            return m;
        }
        let sp = &self.sections.body_sect_pr;
        sp.page_width_twips.to_number() - sp.margin_left_twips.to_number() - sp.margin_right_twips.to_number()
    }

    /// The step of `insertImage` once the size is known.
    fn insert_image_apply(&mut self, cmd: PictureCmd, w0: f64, h0: f64) -> Res {
        let PictureCmd::Insert { data, ext, ole, max_width_twips } = cmd else { return self.done8() };
        let (mut w, mut h) = (w0, h0);
        let max_w = self.content_width_twips(max_width_twips);
        if w > max_w {
            h = (h * max_w) / w;
            w = max_w;
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            match self.delete_selection() {
                Ok(pos) => self.selection = Selection { anchor: pos, focus: pos },
                Err(e) => {
                    self.discard_step();
                    return Err(e);
                }
            }
        }
        let DocPosition { para, offset } = self.selection.focus;
        let Some(paras) = self.paras() else {
            self.discard_step();
            return self.done8();
        };
        let Some(p) = paras.get(para as usize) else {
            self.discard_step();
            return Err(CommandError::Position);
        };
        // the run at the offset (the first that ends at or after it)
        let mut acc = 0usize;
        let mut hit: Option<(usize, usize)> = None;
        for ri in 0..=p.runs.len() {
            let len = p.runs.get(ri).map_or(0, |r| r.text.len());
            let (rs, re) = (acc, acc + len);
            if (offset as usize) <= re || ri == p.runs.len() {
                hit = Some((ri, rs));
                break;
            }
            acc = re;
        }
        let Some((ri, rs)) = hit else {
            self.discard_step();
            return self.done8();
        };
        let base_props: Option<Arc<crate::model::run::RunProps>> = p.runs.get(ri).map(|r| Arc::clone(&r.props));
        let base_link = p.runs.get(ri).and_then(|r| r.rpr_link.clone());
        let props = match &base_props {
            Some(b) => self.new_run_props_from(b),
            None => self.new_run_props_from(&crate::read::plain::default_run_props()),
        };
        let image = {
            let id = self.doc.ids.drawing();
            let media = self.doc.media.add(&data);
            let mut img = InlineImage {
                data: MediaRef(media),
                ext: ext.clone(),
                width_twips: num(w),
                height_twips: num(h),
                drawing: id,
                ..InlineImage::default()
            };
            if let Some(o) = &ole {
                let om = self.doc.media.add(&o.data);
                img.ole = Some(Box::new(OleData { prog_id: o.prog_id.clone(), data: MediaRef(om), ext: o.ext.clone(), ..Default::default() }));
            } else {
                // the XML Word writes on insert; the media joins at the save (`r:embed="uw:media"`)
                let r = f64::from(self.env_mut().random_u32()) / 4_294_967_296.0;
                let pid = 1.0 + math::floor(r * 2_000_000_000.0);
                let count = self.paras().map_or(0, |ps| ps.iter().map(|q| q.runs.iter().filter(|r| r.ext().is_some_and(|e| e.image.is_some())).count()).sum::<usize>());
                let name = format!("Imagem {}", count + 1);
                let xml = new_picture_xml(pid, &name, "uw:media", math::round(w * 635.0), math::round(h * 635.0));
                obj::set(&mut img, "rawDrawingXml", |i| i.raw_drawing_xml = Some(xml.clone().into()));
                obj::set(&mut img, "edited", |i| i.edited = Some(true));
                crate::read::inline_objects::sync_image_from_xml(&mut img);
            }
            img
        };
        let mut image_run = Run { text: Utf16::from_units(vec![0xFFFC]), props: Arc::new(props), ..Run::default() };
        image_run.ext = Some(Box::new(RunExt { image: Some(Box::new(image)), ..RunExt::default() }));
        let Some(paras) = self.paras_mut() else { return self.done8() };
        let Some(pm) = paras.make_mut(para as usize) else { return Err(CommandError::Position) };
        match pm.runs.get(ri).cloned() {
            None => pm.runs.push(image_run),
            Some(run) => {
                let cut = (offset as usize).saturating_sub(rs);
                let units = run.text.units();
                let mut parts: Vec<Run> = Vec::new();
                let piece = |s: &[u16]| Run {
                    text: Utf16::from_units(s.to_vec()),
                    props: Arc::new((*run.props).clone()),
                    rpr_link: base_link.clone(),
                    ..Run::default()
                };
                if cut > 0 && cut <= units.len() {
                    parts.push(piece(&units[..cut]));
                }
                parts.push(image_run);
                if cut < units.len() {
                    parts.push(piece(&units[cut.min(units.len())..]));
                }
                pm.runs.splice(ri..=ri, parts);
            }
        }
        self.touch_para(para as usize);
        let at = DocPosition::new(para, offset.saturating_add(1));
        self.selection = Selection { anchor: at, focus: at };
        self.done8()
    }

    // ── two-phase: adjust ──────────────────────────────────────────────────────────────────

    fn set_image_adjust_start(&mut self, o: &AdjustArg) -> Res {
        let Some(path) = self.pic_path() else { return self.done8() };
        if self.locked() {
            return self.done8();
        }
        let Some(run) = self.run_ref(&path) else { return self.done8() };
        let Some(img) = run.ext().and_then(|e| e.image.as_deref()) else { return self.done8() };
        let Some(f) = img.format.as_deref() else { return self.done8() };
        let cur = f.blip.a14.as_deref().cloned().unwrap_or_default();
        let pct = |v: Option<f64>, d: Option<JsNumber>| -> Option<f64> {
            match v {
                None => d.and_then(JsNumber::as_f64),
                Some(v) => Some(math::round(v * 100_000.0)),
            }
        };
        let mut next = AdjustNext {
            saturation: o.saturation.map(|v| math::round(v * 100_000.0)).or_else(|| cur.saturation.and_then(JsNumber::as_f64)),
            color_temp: o.temperature.map(math::round).or_else(|| cur.color_temp.and_then(JsNumber::as_f64)),
            sharpen: pct(o.sharpen, cur.sharpen),
            bright: pct(o.bright, cur.bright),
            contrast: pct(o.contrast, cur.contrast),
        };
        if next.saturation == Some(100_000.0) {
            next.saturation = None;
        }
        if next.color_temp == Some(6500.0) {
            next.color_temp = None;
        }
        for v in [&mut next.sharpen, &mut next.bright, &mut next.contrast] {
            if !v.is_some_and(truthy) {
                *v = None;
            }
        }
        let neutral = next == AdjustNext::default();
        let cur_layer_rid = cur.layer_rid.clone();
        let original: Option<(MediaRef, Str)> = match &img.layer {
            Some(l) => Some((l.data, l.ext.clone())),
            None if cur_layer_rid.is_some() => None,
            None => Some((img.data, img.ext.clone())),
        };
        let base = original.clone().unwrap_or((img.data, img.ext.clone()));
        if neutral {
            self.adjust_apply(&path, next, original, cur_layer_rid, None);
            return self.done8();
        }
        let bytes = self.doc.media.get_arc(base.0.0).cloned();
        let Some(bytes) = bytes else { return self.done8() };
        let id = self.job_id();
        self.f7.stage = Some(Stage::Adjust { path, next, original, cur_layer_rid });
        let job = Job::DecodeImage { id, bytes, ext: base.1 };
        Ok(self.defer(Command::Picture(PictureCmd::SetAdjust { o: *o }), vec![job]))
    }

    /// The step of `setImageAdjust` with the encoded picture (`None` = neutral).
    fn adjust_apply(&mut self, path: &Path, next: AdjustNext, original: Option<(MediaRef, Str)>, cur_layer_rid: Option<Str>, baked: Option<&[u8]>) {
        let Some(raw) = self.run_ref(path).and_then(raw_of).map(str::to_owned) else { return };
        let cur = self
            .run_ref(path)
            .and_then(|r| r.ext())
            .and_then(|e| e.image.as_deref())
            .and_then(|i| i.format.as_deref())
            .and_then(|f| f.blip.a14.as_deref().cloned())
            .unwrap_or_default();
        let other_raw = cur.other_raw.clone();
        let a14 = if baked.is_none() {
            let keep = other_raw.as_ref().is_some_and(|v| !v.is_empty());
            if keep {
                Nullable::Val(A14Props { layer_rid: cur_layer_rid.clone(), other_raw: other_raw.clone(), ..A14Props::default() })
            } else {
                Nullable::Null
            }
        } else {
            let layer_rid: Option<Str> = if original.is_some() { Some("uw:layer".into()) } else { cur_layer_rid };
            Nullable::Val(A14Props {
                saturation: next.saturation.map(num),
                color_temp: next.color_temp.map(num),
                sharpen: next.sharpen.map(num),
                bright: next.bright.map(num),
                contrast: next.contrast.map(num),
                layer_rid,
                other_raw,
                ..A14Props::default()
            })
        };
        let ch = PictureChange { a14: Some(a14), embed: Some("uw:media".into()), ..PictureChange::default() };
        let Ok(xml) = patch_picture(&raw, &ch) else { return };
        self.begin_step(false, &[]);
        let baked_id = baked.map(|b| self.doc.media.add(b));
        if let Some(img) = self.run_mut(path).and_then(|r| r.ext_mut().image.as_deref_mut()) {
            match baked_id {
                None => {
                    if let Some((d, e)) = &original {
                        obj::set(img, "data", |i| i.data = *d);
                        obj::set(img, "ext", |i| i.ext = e.clone());
                    }
                    obj::delete(img, "layer");
                }
                Some(id) => {
                    let (old_d, old_e) = (img.data, img.ext.clone());
                    obj::set(img, "data", |i| i.data = MediaRef(id));
                    obj::set(img, "ext", |i| i.ext = "png".into());
                    if original.is_some() {
                        let (d, e) = original.clone().unwrap_or((old_d, old_e));
                        obj::set(img, "layer", |i| i.layer = Some(ImageLayer { data: d, ext: e, ..Default::default() }));
                    }
                }
            }
            obj::set_undefined(img, "svg", |i| i.svg = None);
        }
        self.apply_drawing_xml(path, xml, false);
        self.after_picture_edit();
    }

    // ── two-phase: compress ────────────────────────────────────────────────────────────────

    fn compress_start(&mut self, ppi: f64, delete_cropped: bool, all: bool) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.done8();
        }
        let mut paths: Vec<Path> = Vec::new();
        if all {
            if let Some(ps) = self.paras() {
                for (pi, p) in ps.iter().enumerate() {
                    for (ri, r) in p.runs.iter().enumerate() {
                        let ok = r.ext().and_then(|e| e.image.as_deref()).is_some_and(|i| i.raw_drawing_xml.is_some() && i.format.is_some() && i.ole.is_none());
                        if ok {
                            paths.push(vec![Step::Para { para: u32::try_from(pi).unwrap_or(u32::MAX), run: u32::try_from(ri).unwrap_or(u32::MAX) }]);
                        }
                    }
                }
            }
        } else if let Some(p) = self.pic_path() {
            paths.push(p);
        }
        // M3/M5: one decode per image, at most 1,024 images per command
        paths.truncate(1024);
        let mut jobs = Vec::new();
        let mut items = Vec::new();
        for p in paths {
            let Some(img) = self.run_ref(&p).and_then(|r| r.ext()).and_then(|e| e.image.as_deref()) else { continue };
            let ext = uo_text::case::to_lowercase_str(&img.ext);
            if img.svg.is_some() || ext == "emf" || ext == "wmf" {
                continue;
            }
            let Some(bytes) = self.doc.media.get_arc(img.data.0).cloned() else { continue };
            let ext_s = img.ext.clone();
            let id = self.job_id();
            jobs.push(Job::DecodeImage { id, bytes, ext: ext_s });
            items.push(CompressItem { path: p, crop: false, jpeg: false });
        }
        if jobs.is_empty() {
            return self.done8();
        }
        self.f7.stage = Some(Stage::Compress { items });
        Ok(self.defer(Command::Picture(PictureCmd::Compress { ppi, delete_cropped, all }), jobs))
    }

    /// The number of pictures the last `compressImages` changed.
    #[must_use]
    pub fn compress_result(&self) -> usize {
        self.f7.compressed
    }

    // ── completion ─────────────────────────────────────────────────────────────────────────

    pub(crate) fn complete_pictures(&mut self, cmd: PictureCmd, _job: JobId, _result: JobResult, _cx: &mut ExecCx<'_>) -> Res {
        let results = std::mem::take(&mut self.f7.results);
        let stage = self.f7.stage.take();
        let r = self.complete_pictures_inner(cmd, results, stage);
        if self.pending.is_none() {
            self.f7.clear_pending();
        }
        r
    }

    fn complete_pictures_inner(&mut self, cmd: PictureCmd, results: Vec<(JobId, JobResult)>, stage: Option<Stage>) -> Res {
        match cmd {
            PictureCmd::Insert { .. } => {
                let Some((_, JobResult::Image { width, height, .. })) = results.last() else { return self.done8() };
                let (w, h) = (f64::from(*width) * 15.0, f64::from(*height) * 15.0);
                self.insert_image_apply(cmd, w, h)
            }
            PictureCmd::Change { data, ext } => {
                let Some((_, JobResult::Image { width, height, .. })) = results.last() else { return self.done8() };
                let (bw, bh) = (f64::from(*width), f64::from(*height));
                let Some(path) = self.pic_path() else { return self.done8() };
                self.change_image_apply(&path, &data, &ext, bw, bh);
                self.done8()
            }
            PictureCmd::SetWrap { kind } => {
                let poly = match results.last() {
                    Some((_, JobResult::Image { width, height, rgba: Some(px) })) if u64::from(*width) * u64::from(*height) <= MAX_PIXELS => {
                        crate::drawing::contour::wrap_contour(px, *width, *height).ok().flatten()
                    }
                    _ => None,
                };
                self.set_image_wrap(kind, Some(poly))
            }
            PictureCmd::SetAdjust { .. } => match stage {
                Some(Stage::Adjust { path, next, original, cur_layer_rid }) => {
                    // round 1: decoded pixels in; round 2: the encoded bytes
                    match results.last() {
                        Some((_, JobResult::Image { width, height, rgba: Some(px) })) if u64::from(*width) * u64::from(*height) <= MAX_PIXELS => {
                            let mut buf: Vec<u8> = px.to_vec();
                            if !self.adjust_pixels(&mut buf, *width, *height, &next) {
                                return self.done8();
                            }
                            let id = self.job_id();
                            let job = Job::EncodeImage { id, width: *width, height: *height, rgba: Arc::from(buf), mime: "image/png".into(), quality: None };
                            self.f7.stage = Some(Stage::Adjust { path, next, original, cur_layer_rid });
                            self.f7.encoding = true;
                            Ok(self.defer(Command::Picture(cmd), vec![job]))
                        }
                        Some((_, JobResult::Bytes(b))) if self.f7.encoding => {
                            self.f7.encoding = false;
                            if self.pic_path().as_ref() != Some(&path) {
                                return self.done8();
                            }
                            let b = Arc::clone(b);
                            self.adjust_apply(&path, next, original, cur_layer_rid, Some(&b));
                            self.done8()
                        }
                        _ => {
                            self.f7.encoding = false;
                            self.done8()
                        }
                    }
                }
                _ => self.done8(),
            },
            PictureCmd::Compress { ppi, delete_cropped, all } => self.compress_continue(cmd, ppi, delete_cropped, all, results, stage),
            _ => self.done8(),
        }
    }

    /// The pixel processing of `setImageAdjust`, in the TS order: saturation, temperature, sharpen,
    /// brightness/contrast.
    fn adjust_pixels(&self, d: &mut [u8], w: u32, h: u32, next: &AdjustNext) -> bool {
        use uo_drawingml as a;
        if let Some(s) = next.saturation {
            a::adjust_saturation(d, s / 100_000.0);
        }
        if let Some(t) = next.color_temp {
            a::adjust_temperature(d, t);
        }
        if let Some(s) = next.sharpen
            && a::adjust_sharpen(d, w, h, s / 100_000.0, &uo_drawingml::Limits::default()).is_err()
        {
            return false;
        }
        if next.bright.is_some() || next.contrast.is_some() {
            a::adjust_brightness_contrast(d, next.bright.unwrap_or(0.0) / 100_000.0, next.contrast.unwrap_or(0.0) / 100_000.0);
        }
        true
    }

    fn change_image_apply(&mut self, path: &Path, data: &[u8], ext: &str, bw: f64, bh: f64) {
        let Some(f) = self.run_ref(path).and_then(format_of) else { return };
        let fb = f.base().clone();
        let Some(raw) = self.run_ref(path).and_then(raw_of).map(str::to_owned) else { return };
        let cy = math::round((fb.cx.to_number() * bh) / math::max(1.0, bw));
        let stripped = strip_svg_blip_ext(&raw);
        let ch = PictureChange {
            embed: Some("uw:media".into()),
            a14: Some(Nullable::Null),
            src_rect: Some(zero_crop()),
            size: Some((fb.cx.to_number(), cy)),
            ..PictureChange::default()
        };
        if !super::drawing::change_is_finite(&pic(ch.clone())) {
            return;
        }
        let Ok(xml) = patch_picture(&stripped, &ch) else { return };
        self.begin_step(false, &[]);
        let id = self.doc.media.add(data);
        if let Some(img) = self.run_mut(path).and_then(|r| r.ext_mut().image.as_deref_mut()) {
            obj::set(img, "data", |i| i.data = MediaRef(id));
            obj::set(img, "ext", |i| i.ext = ext.into());
            obj::set_undefined(img, "svg", |i| i.svg = None);
            obj::delete(img, "layer");
        }
        self.apply_drawing_xml(path, xml, true);
        self.after_picture_edit();
    }

    fn compress_continue(
        &mut self,
        cmd: PictureCmd,
        ppi: f64,
        delete_cropped: bool,
        _all: bool,
        results: Vec<(JobId, JobResult)>,
        stage: Option<Stage>,
    ) -> Res {
        let Some(Stage::Compress { items }) = stage else { return self.done8() };
        if !self.f7.encoding {
            // round 1: the decodes are in (in job order); ask the encodes of the pictures that change
            let mut jobs = Vec::new();
            let mut todo = Vec::new();
            for (item, (_, r)) in items.iter().zip(results.iter()) {
                let JobResult::Image { width, height, rgba: Some(px) } = r else { continue };
                if u64::from(*width) * u64::from(*height) > MAX_PIXELS {
                    continue;
                }
                let Some(img) = self.run_ref(&item.path).and_then(|r| r.ext()).and_then(|e| e.image.as_deref()) else { continue };
                let Some(f) = img.format.as_deref() else { continue };
                let (iw, ih) = (f64::from(*width), f64::from(*height));
                let s = &f.src_rect;
                let (sl, st, sr, sb) = (s.l.to_number(), s.t.to_number(), s.r.to_number(), s.b.to_number());
                let crop = delete_cropped && (sl > 0.0 || st > 0.0 || sr > 0.0 || sb > 0.0);
                let sx = if crop { math::round((math::max(0.0, sl) / 100_000.0) * iw) } else { 0.0 };
                let sy = if crop { math::round((math::max(0.0, st) / 100_000.0) * ih) } else { 0.0 };
                let sw = if crop { math::max(1.0, math::round((1.0 - (math::max(0.0, sl) + math::max(0.0, sr)) / 100_000.0) * iw)) } else { iw };
                let sh = if crop { math::max(1.0, math::round((1.0 - (math::max(0.0, st) + math::max(0.0, sb)) / 100_000.0) * ih)) } else { ih };
                let tw = math::max(1.0, math::round((f.cx.to_number() / 914_400.0) * ppi));
                let th = math::max(1.0, math::round((f.cy.to_number() / 914_400.0) * ppi));
                let (w, h) = (math::min(sw, tw), math::min(sh, th));
                if w == sw && h == sh && !crop {
                    continue;
                }
                if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0) {
                    continue;
                }
                let (wu, hu) = (w as u32, h as u32);
                let buf = resample(px, *width, *height, [sx, sy, sw, sh], wu, hu);
                let jpeg = {
                    let e = uo_text::case::to_lowercase_str(&img.ext);
                    e == "jpg" || e == "jpeg"
                };
                let id = self.job_id();
                let (mime, quality): (Str, Option<f64>) = if jpeg { ("image/jpeg".into(), Some(0.85)) } else { ("image/png".into(), None) };
                jobs.push(Job::EncodeImage { id, width: wu, height: hu, rgba: Arc::from(buf), mime, quality });
                todo.push(CompressItem { path: item.path.clone(), crop, jpeg });
            }
            if jobs.is_empty() {
                self.f7.compressed = 0;
                return self.done8();
            }
            self.f7.stage = Some(Stage::Compress { items: todo });
            self.f7.encoding = true;
            return Ok(self.defer(Command::Picture(cmd), jobs));
        }
        self.f7.encoding = false;
        // round 2: the encoded bytes are in; one step for all
        let mut applied: Vec<(CompressItem, Arc<[u8]>)> = Vec::new();
        for (item, (_, r)) in items.iter().zip(results.iter()) {
            if let JobResult::Bytes(b) = r {
                applied.push((item.clone(), Arc::clone(b)));
            }
        }
        if applied.is_empty() {
            self.f7.compressed = 0;
            return self.done8();
        }
        let mut plans: Vec<(Path, String, Arc<[u8]>, bool)> = Vec::new();
        for (item, bytes) in applied {
            let Some(raw) = self.run_ref(&item.path).and_then(raw_of).map(str::to_owned) else { continue };
            let has_a14 = self
                .run_ref(&item.path)
                .and_then(|r| r.ext())
                .and_then(|e| e.image.as_deref())
                .and_then(|i| i.format.as_deref())
                .is_some_and(|f| f.blip.a14.is_some());
            let ch = PictureChange {
                embed: Some("uw:media".into()),
                src_rect: item.crop.then(zero_crop),
                a14: has_a14.then_some(Nullable::Null),
                ..PictureChange::default()
            };
            if let Ok(xml) = patch_picture(&raw, &ch) {
                plans.push((item.path, xml, bytes, item.jpeg));
            }
        }
        if plans.is_empty() {
            self.f7.compressed = 0;
            return self.done8();
        }
        self.begin_step(false, &[]);
        let n = plans.len();
        for (path, xml, bytes, jpeg) in plans {
            let id = self.doc.media.add(&bytes);
            if let Some(img) = self.run_mut(&path).and_then(|r| r.ext_mut().image.as_deref_mut()) {
                obj::set(img, "data", |i| i.data = MediaRef(id));
                obj::set(img, "ext", |i| i.ext = if jpeg { "jpg" } else { "png" }.into());
                obj::delete(img, "layer");
            }
            self.apply_drawing_xml(&path, xml, false);
        }
        self.f7.compressed = n;
        self.after_picture_edit();
        self.done8()
    }

    pub(crate) fn patch_drawing_pub(&self, path: &[Step], ch: &ShapeChange) -> Option<String> {
        let r = self.run_ref(path)?;
        let raw = raw_of(r)?;
        if r.ext().is_some_and(|e| e.image.is_some()) { patch_picture(raw, &ch.pic).ok() } else { crate::write::emit::object::patch_shape(raw, ch).ok() }
    }
}

impl PictureCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        let m = match self {
            PictureCmd::Insert { .. } => "insertImage",
            PictureCmd::SetFormat { .. } => "setImageFormat",
            PictureCmd::SetBorder { .. } => "setImageBorder",
            PictureCmd::SetCrop { .. } => "setImageCrop",
            PictureCmd::SetRecolor { .. } => "setImageRecolor",
            PictureCmd::SetTransparency { .. } => "setImageTransparency",
            PictureCmd::SetTransparentColor { .. } => "setImageTransparentColor",
            PictureCmd::SetLayout { .. } => "setImageLayout",
            PictureCmd::SetWrap { .. } => "setImageWrap",
            PictureCmd::SetPosition { .. } => "setImagePosition",
            PictureCmd::SetRotation { .. } => "setImageRotation",
            _ => return None,
        };
        Some((m, GuardArg::None))
    }
}

fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// `{ ...keep, ...p.fx }`.
fn merge_blip(keep: &mut PicBlipFx, p: &PicBlipFx) {
    if let Some(v) = p.alpha_mod_fix {
        obj::set(keep, "alphaModFix", |x| x.alpha_mod_fix = Some(v));
    }
    if let Some(v) = &p.lum {
        obj::set(keep, "lum", |x| x.lum = Some(v.clone()));
    }
    if let Some(v) = p.grayscl {
        obj::set(keep, "grayscl", |x| x.grayscl = Some(v));
    }
    if let Some(v) = p.bi_level {
        obj::set(keep, "biLevel", |x| x.bi_level = Some(v));
    }
    if let Some(v) = &p.duotone {
        obj::set(keep, "duotone", |x| x.duotone = Some(v.clone()));
    }
}

/// `/^glow-(\d+)-(\d)$/`: the size index and the accent number.
fn parse_glow(p: &str) -> Option<(f64, f64)> {
    let rest = p.strip_prefix("glow-")?;
    let (a, b) = rest.split_once('-')?;
    if a.is_empty() || !a.bytes().all(|c| c.is_ascii_digit()) || b.len() != 1 || !b.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((uo_jsnum::parse::to_number(a), uo_jsnum::parse::to_number(b)))
}

/// `xml.replace(/<a:ext uri="\{96DAC541-7B7A-43D3-8B79-37D633B846F1\}">[\s\S]*?<\/a:ext>/g, '')`.
fn strip_svg_blip_ext(raw: &str) -> String {
    const OPEN: &str = "<a:ext uri=\"{96DAC541-7B7A-43D3-8B79-37D633B846F1}\">";
    const CLOSE: &str = "</a:ext>";
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find(OPEN) {
        let after = &rest[i + OPEN.len()..];
        let Some(j) = after.find(CLOSE) else { break };
        out.push_str(&rest[..i]);
        rest = &after[j + CLOSE.len()..];
    }
    out.push_str(rest);
    out
}

/// Area-average resampling of the `[sx, sy, sw, sh]` region of `px` (RGBA, `w` × `h`) to
/// `tw` × `th` (the canvas `drawImage` with high smoothing is the shell's; the bytes of a
/// re-encoded picture are not compared).
fn resample(px: &[u8], w: u32, h: u32, region: [f64; 4], tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw as usize).saturating_mul(th as usize).saturating_mul(4)];
    let [sx, sy, sw, sh] = region;
    for ty in 0..th {
        let y0 = sy + f64::from(ty) * sh / f64::from(th);
        let y1 = sy + f64::from(ty + 1) * sh / f64::from(th);
        for tx in 0..tw {
            let x0 = sx + f64::from(tx) * sw / f64::from(tw);
            let x1 = sx + f64::from(tx + 1) * sw / f64::from(tw);
            let (mut acc, mut wsum) = ([0.0f64; 4], 0.0f64);
            let (iy0, iy1) = (math::floor(y0) as i64, math::ceil(y1) as i64);
            let (ix0, ix1) = (math::floor(x0) as i64, math::ceil(x1) as i64);
            for iy in iy0..iy1.max(iy0 + 1) {
                for ix in ix0..ix1.max(ix0 + 1) {
                    let (cx, cy) = (ix.clamp(0, i64::from(w) - 1) as usize, iy.clamp(0, i64::from(h) - 1) as usize);
                    let wy = (f64::min(y1, (iy + 1) as f64) - f64::max(y0, iy as f64)).max(1e-6);
                    let wx = (f64::min(x1, (ix + 1) as f64) - f64::max(x0, ix as f64)).max(1e-6);
                    let k = (cy * w as usize + cx) * 4;
                    let wgt = wx * wy;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += f64::from(px.get(k + c).copied().unwrap_or(0)) * wgt;
                    }
                    wsum += wgt;
                }
            }
            let o = (ty as usize * tw as usize + tx as usize) * 4;
            for c in 0..4 {
                if let Some(slot) = out.get_mut(o + c) {
                    *slot = (acc[c] / wsum.max(1e-9)).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

