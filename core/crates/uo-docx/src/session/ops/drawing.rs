//! D2b family: shapes and textboxes: every `setShape*`, `changeShape`, `setTextboxOptions`, `insertTextbox`, `drawTextbox`, textbox links, gallery, `deleteSelectedShape`, `alignObjects`, selection pane; editor `shape-ops.ts`, docx-core `shape-xml.ts` (the D1b W2 emitters).
//!
//! Owned by frontier D2b-F7 (record "D2a — session (architecture)", "D2b frontier briefs", and
//! `core/docs/modules/uo-docx-d2b/F7.md`). The pictures half (`picture-ops.ts`, the `setImage*`
//! methods that shapes share through `editDrawing`) is `pictures.rs`; the machinery both use is here:
//! where a drawing run lives ([`loc`]), the selection (`floatSel`, `floatMulti`, the inline
//! picture), `editDrawing`/`applyDrawingXml`, and the textbox chains.

mod loc;
pub mod pure;
pub mod rx;
pub(crate) mod textbox;

use std::sync::Arc;

use uo_jsnum::math;

use super::super::command::{CommandError, JobId, JobResult, Outcome};
use super::super::policy::GuardArg;
use super::super::state::Selection;
use super::super::text::DocPosition;
use super::super::{ExecCx, Session};
use super::context::EditContext;
use super::pictures::PictureCmd;
use crate::js::{JsNumber, Nullable, Str};
use crate::model::Document;
use crate::model::picture::{PicColor, PicLine, PictureFormat};
use crate::model::run::{DrawingTextbox, Run};
use crate::model::story::Story;
use crate::read::inline_objects::{sync_image_from_xml, sync_shape_from_xml};
use crate::read::picture_format::read_picture_format;
use crate::word::obj;
use crate::write::emit::fx::{PictureChange, patch_picture};
use crate::write::emit::object::{ShapeChange, ShapeFill, ShapeFormat, StoryChange, TextboxBodyPatch, patch_shape, read_shape_format};

pub(crate) use loc::{Path, Step};
#[doc(hidden)]
pub use textbox::template_xml;

type Res = Result<Outcome, CommandError>;

/// A rectangle in twips on a page (the layout's `FloatHandle.rect`): facts only the shell has.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ObjRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `ObjectFrame`: the positioning references the layout placed an object with (twips).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ObjFrame {
    pub page_w: f64,
    pub page_h: f64,
    pub mar_l: f64,
    pub mar_r: f64,
    pub mar_t: f64,
    pub mar_b: f64,
    pub col_x: f64,
    pub col_w: f64,
    pub body_top: f64,
    pub para_top: f64,
}

/// What the shell knows about an object that the model does not (a `FloatHandle`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ObjFacts {
    pub rect: Option<ObjRect>,
    pub frame: Option<ObjFrame>,
}

/// `Partial<TextboxBody>` of `setTextboxOptions` in the units the UI gives.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextboxOptions {
    pub margin_l_pt: Option<f64>,
    pub margin_t_pt: Option<f64>,
    pub margin_r_pt: Option<f64>,
    pub margin_b_pt: Option<f64>,
    /// `t` | `ctr` | `b`
    pub anchor: Option<Str>,
    pub direction: Option<Str>,
    pub wrap: Option<bool>,
    /// `none` | `shape` | `norm`
    pub autofit: Option<Str>,
    pub upright: Option<bool>,
}

/// The values of `setShapeFormat`/`setImageFormat` (`undefined` = not given, `null` removes).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FormatArg {
    pub line: Option<Nullable<PicLine>>,
    pub shadow: Option<Nullable<crate::model::picture::PicShadow>>,
    pub reflection: Option<Nullable<crate::model::picture::PicReflection>>,
    pub glow: Option<Nullable<crate::model::picture::PicGlow>>,
    pub soft_edge: Option<Nullable<f64>>,
    pub three_d: Option<Nullable<crate::model::picture::Pic3D>>,
}

/// The align kinds of `alignObjects`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignKind {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
    DistributeH,
    DistributeV,
}

/// A user gallery item (`saveSelectionToTextboxGallery`): the `w:drawing` text with its content.
#[derive(Debug, Clone, PartialEq)]
pub struct GalleryBox {
    pub xml: Str,
}

/// What the selection pane lists (`selectionPaneItems`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    Image,
    Textbox,
    Shape,
    Group,
}

/// One object of the selection pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneItem {
    pub key: String,
    pub name: String,
    pub hidden: bool,
    pub selected: bool,
    pub kind: PaneKind,
}

/// The text body values of a selected box (`ShapeState.text`, before the unit conversion).
#[derive(Debug, Clone, PartialEq)]
pub struct TextState {
    /// `lIns`, `tIns`, `rIns`, `bIns` in EMU
    pub margins_emu: [f64; 4],
    pub anchor: &'static str,
    pub direction: String,
    pub wrap: bool,
    pub autofit: &'static str,
    pub upright: bool,
}

/// `ShapeState` / `ImageState` with the model values (EMU, 60000ths of a degree).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectState {
    pub kind: PaneKind,
    pub inline: bool,
    pub wrap: String,
    pub cx: f64,
    pub cy: f64,
    pub rot: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub lock_aspect: bool,
    pub move_with_text: bool,
    pub lock_anchor: bool,
    pub name: String,
    pub alt_text: String,
    pub decorative: bool,
    pub geometry: String,
    pub style_index: u32,
    pub editing_text: bool,
    pub text: Option<TextState>,
    pub link_next: bool,
    pub link_previous: bool,
    pub format: PictureFormat,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawingCmd {
    /// `floatSel = contextDrawingRuns()[index]` (`None` clears the selection and the multi-selection)
    SelectObject { index: Option<u32> },
    /// `selectInlineRun(run)`: the inline picture at `para`/`run` of the story, the caret over it
    SelectInline { para: u32, run: u32 },
    /// `selectPaneItem(key, add)`
    SelectPaneItem { key: Str, add: bool },
    /// `selectPaneItems(keys)`
    SelectPaneItems { keys: Vec<Str> },
    /// `renamePaneItem(key, name)`
    RenamePaneItem { key: Str, name: Str },
    /// `setPaneItemHidden(key, hidden)` (`None`: every object of the pane)
    SetPaneItemHidden { key: Option<Str>, hidden: bool },
    /// `setShapeFill(fill)`: `None` = no fill; a colour is `ShapeFill::Solid`
    SetShapeFill { fill: Option<ShapeFill> },
    /// `setShapePictureFill(data, ext, tile)`
    SetShapePictureFill { data: Arc<[u8]>, ext: Str, tile: bool },
    /// `setShapeOutline({ color, weightPt, dash, compound })`
    SetShapeOutline { color: Option<Nullable<PicColor>>, weight_pt: Option<f64>, dash: Option<Str>, compound: Option<Str> },
    /// `setShapeFormat(o)`
    SetShapeFormat { fmt: Box<FormatArg> },
    /// `setShapeStyle(n)`
    SetShapeStyle { n: f64 },
    /// `changeShape(prst)`
    ChangeShape { prst: Str },
    /// `setTextboxOptions(o)`
    SetTextboxOptions { o: TextboxOptions },
    /// `setShapeWrap`…`setShapeLayout`: the picture command when the selected object is not a picture
    OnShape(Box<PictureCmd>),
    /// `setShapeEffect(kind, preset)`
    SetShapeEffect { kind: super::pictures::EffectKind, preset: Option<Str> },
    /// `deleteSelectedShape()`
    DeleteSelectedShape,
    /// `linkTextboxTo(key)`: the box of the pane list after the selected one
    LinkTextboxTo { key: Str },
    /// `breakTextboxLink()`
    BreakTextboxLink,
    /// `insertTextbox(id)`: a gallery template or a user item; with `page` the new box is entered
    /// (host/focus from the new run) and its first text selected
    InsertTextbox { id: Str, page: Option<u32>, theme_accent1: Option<Str>, lang: Str, user: Option<GalleryBox> },
    /// `createDrawnTextbox(page, rect, vertical, anchorPos)` after `drawTextbox` (`frame` is
    /// applied by a following [`DrawingCmd::PlaceAt`], the TS refreshes the layout in between)
    CreateDrawnTextbox { page: u32, rect: ObjRect, vertical: bool, anchor_para: Option<u32>, lang: Str },
    /// the second half of a command that floats an object (`setImageWrap` from inline,
    /// `createDrawnTextbox`): the offsets from the frame the layout gave the placed object
    PlaceAt { frame: ObjFrame, x: f64, y: f64 },
    /// `setAsDefaultTextbox()`
    SetAsDefaultTextbox,
    /// `alignObjects(kind)`: one entry per selected object in selection order (selected first); an
    /// entry without `rect` is an inline or not laid out object, which the TS leaves out
    AlignObjects { kind: AlignKind, rects: Vec<ObjFacts> },
    /// `editSelectedTextbox()`: the selected box is edited, the caret at the end of its text (`page`:
    /// the page of `selectedFloat()`, 0 without one)
    EditSelectedTextbox { page: u32 },
}

/// The F7 state of a session (the TS element's `floatMulti`, the inline selection, the answers of
/// the jobs of the pending command).
#[derive(Debug, Default)]
pub struct F7State {
    /// the selected inline picture (`floatSel` on a picture that does not float)
    pub(crate) inline_sel: Option<Path>,
    /// `floatMulti`
    pub(crate) multi: Vec<Path>,
    /// the answers of the jobs of the pending command, in arrival order
    pub(crate) results: Vec<(JobId, JobResult)>,
    /// the stage of a command with more than one round of jobs, and what it carries
    pub(crate) stage: Option<super::pictures::Stage>,
    /// the object just floated, waiting for its placement (`PlaceAt`)
    pub(crate) placed: Option<Path>,
    /// `imageAlignTo === 'page'`
    pub(crate) align_page: bool,
    /// a second round of jobs is out (the encodes of a command whose decodes are in)
    pub(crate) encoding: bool,
    /// the pictures the last `compressImages` changed
    pub(crate) compressed: usize,
    /// the box a step was taken in (host, focus, page, selection): entered again after the pop
    pub(crate) reenter: Option<(u32, u32, u32, Selection)>,
}

impl F7State {
    pub(crate) fn on_restore(&mut self) {
        // `popHistory`: `floatSel = null`, the model is new
        self.inline_sel = None;
        self.multi.clear();
        self.placed = None;
    }

    pub(crate) fn stash(&mut self, job: JobId, r: JobResult) {
        // M5: the answers of one command (a compress of 100 images asks 200 jobs)
        if self.results.len() < 4096 {
            self.results.push((job, r));
        }
    }

    /// Drops what a finished or cancelled command carried.
    pub(crate) fn clear_pending(&mut self) {
        self.results.clear();
        self.stage = None;
        self.encoding = false;
    }
}

/// The formatting of a drawing object: a picture's or a shape's (`PictureFormat | ShapeFormat`).
#[derive(Debug, Clone)]
pub(crate) enum Fmt {
    Pic(Box<PictureFormat>),
    Shape(Box<ShapeFormat>),
}

impl Fmt {
    pub(crate) fn base(&self) -> &PictureFormat {
        match self {
            Fmt::Pic(p) => p,
            Fmt::Shape(s) => &s.base,
        }
    }
}

/// The raw `w:drawing` of a run (`rawOf`): image, box, shape, group in that order.
pub(crate) fn raw_of(r: &Run) -> Option<&str> {
    let e = r.ext()?;
    if let Some(i) = e.image.as_deref() {
        return i.raw_drawing_xml.as_deref();
    }
    if let Some(t) = e.textbox.as_deref() {
        return t.raw_drawing_xml.as_deref();
    }
    if let Some(s) = e.shape.as_deref() {
        return s.raw_drawing_xml.as_deref();
    }
    e.group.as_deref().and_then(|g| g.raw_drawing_xml.as_deref())
}

/// `formatOf(r)`.
pub(crate) fn format_of(r: &Run) -> Option<Fmt> {
    let e = r.ext()?;
    if let Some(i) = e.image.as_deref() {
        return i.format.as_deref().map(|f| Fmt::Pic(Box::new(f.clone())));
    }
    let raw = raw_of(r)?;
    if e.group.is_some() && e.textbox.is_none() && e.shape.is_none() {
        return read_picture_format(raw, true).map(|f| Fmt::Pic(Box::new(f)));
    }
    read_shape_format(raw).map(|f| Fmt::Shape(Box::new(f)))
}

fn is_image(r: &Run) -> bool {
    r.ext().is_some_and(|e| e.image.is_some())
}

/// Whether the change carries a key of `EXTENT_KEYS` (`k in ch`).
fn has_extent_key(ch: &ShapeChange) -> bool {
    let p = &ch.pic;
    p.size.is_some()
        || p.rot.is_some()
        || p.line.is_some()
        || p.shadow.is_some()
        || p.reflection.is_some()
        || p.glow.is_some()
        || p.soft_edge.is_some()
        || p.three_d.is_some()
        || p.wrap.is_some()
        || ch.style_preset.is_some()
}

fn finite(x: f64) -> bool {
    x.is_finite()
}

/// A change that would write a non-finite number into the XML is refused (F7-N1: the TS writes
/// `NaN` into the attribute; the file would not open).
pub(crate) fn change_is_finite(ch: &ShapeChange) -> bool {
    let p = &ch.pic;
    p.size.is_none_or(|(a, b)| finite(a) && finite(b))
        && p.rot.is_none_or(finite)
        && p.at.is_none_or(|(a, b)| finite(a) && finite(b))
        && p.new_relative_height.is_none_or(finite)
        && p.relative_height.is_none_or(finite)
        && p.src_rect.is_none_or(|r| finite(r.l) && finite(r.t) && finite(r.r) && finite(r.b))
        && p.effect_extent.is_none_or(|r| finite(r.l) && finite(r.t) && finite(r.r) && finite(r.b))
        && p.dist.as_ref().is_none_or(|d| d.iter().all(|(_, v)| finite(*v)))
        && p.pos_h
            .as_ref()
            .is_none_or(|q| q.offset.is_none_or(|n| n.to_number().is_finite()) && q.pct.is_none_or(|n| n.to_number().is_finite()))
        && p.pos_v
            .as_ref()
            .is_none_or(|q| q.offset.is_none_or(|n| n.to_number().is_finite()) && q.pct.is_none_or(|n| n.to_number().is_finite()))
        && p.soft_edge.as_ref().is_none_or(|n| match n {
            Nullable::Val(v) => finite(*v),
            Nullable::Null => true,
        })
}

impl Session {
    fn done7(&mut self) -> Res {
        Ok(Outcome::Done(self.finish_if_stepped()))
    }

    // ── the stories drawing runs live in ───────────────────────────────────────────────────

    /// The story `contextDrawingRuns`/`contextTextboxRuns` read: the header/footer part in edit
    /// (also around a box), else the body.
    fn base_story(&self) -> &Story {
        fn go<'a>(doc: &'a Document, ctx: &EditContext) -> &'a Story {
            match ctx {
                EditContext::HeaderFooter { part, .. } => doc.hf.get(part.index()).map_or(&doc.body, |h| &h.story),
                EditContext::Textbox { outer, .. } => go(doc, outer),
                _ => &doc.body,
            }
        }
        go(&self.doc, &self.ctx)
    }

    pub(crate) fn base_story_mut(&mut self) -> &mut Story {
        fn go<'a>(doc: &'a mut Document, ctx: &EditContext) -> &'a mut Story {
            match ctx {
                EditContext::HeaderFooter { part, .. } => match doc.hf.make_mut(part.index()) {
                    Some(h) => &mut h.story,
                    None => &mut doc.body,
                },
                EditContext::Textbox { outer, .. } => go(doc, outer),
                _ => &mut doc.body,
            }
        }
        go(&mut self.doc, &self.ctx)
    }

    /// `contextDrawingRuns`.
    pub(crate) fn drawing_paths(&self) -> Vec<Path> {
        loc::drawing_paths(self.base_story())
    }

    /// `contextTextboxRuns`.
    pub(crate) fn textbox_paths(&self) -> Vec<Path> {
        loc::textbox_paths(self.base_story())
    }

    pub(crate) fn run_ref(&self, path: &[Step]) -> Option<&Run> {
        loc::run_at(self.base_story(), path)
    }

    /// The run at `path` for a change (marks what the layout must redo).
    pub(crate) fn run_mut(&mut self, path: &[Step]) -> Option<&mut Run> {
        match loc::top_level(path) {
            Some((para, _)) if matches!(self.ctx, EditContext::Body | EditContext::HeaderFooter { .. }) => self.touch_para(para as usize),
            _ => self.touch_all(),
        }
        loc::run_at_mut(self.base_story_mut(), path)
    }

    // ── the selection (`floatSel`, `tbEdit.focusRun`, `floatMulti`) ─────────────────────────

    /// `floatSel` as a path.
    pub(crate) fn sel_path(&self) -> Option<Path> {
        if let Some(i) = self.float_sel {
            return self.drawing_paths().get(i as usize).cloned();
        }
        let p = self.f7.inline_sel.as_ref()?;
        self.run_ref(p).is_some().then(|| p.clone())
    }

    /// `tbEdit.focusRun` as a path (the box of the focus in `contextTextboxRuns`).
    pub(crate) fn focus_path(&self) -> Option<Path> {
        match &self.ctx {
            EditContext::Textbox { focus, host, .. } => {
                let all = self.textbox_paths();
                all.get(*focus as usize).or_else(|| all.get(*host as usize)).cloned()
            }
            _ => None,
        }
    }

    /// `drawRun()`: the selected object, or the box in edit; with XML the editor can change.
    pub(crate) fn draw_path(&self) -> Option<Path> {
        let p = self.sel_path().or_else(|| self.focus_path())?;
        let r = self.run_ref(&p)?;
        if let Some(i) = r.ext().and_then(|e| e.image.as_deref()) {
            return (i.raw_drawing_xml.is_some() && i.format.is_some()).then_some(p);
        }
        format_of(r).is_some().then_some(p)
    }

    /// `picRun()`: the selected picture with XML the editor can change.
    pub(crate) fn pic_path(&self) -> Option<Path> {
        let p = self.sel_path()?;
        let i = self.run_ref(&p)?.ext()?.image.as_deref()?;
        (i.raw_drawing_xml.is_some() && i.format.is_some()).then_some(p)
    }

    pub(crate) fn set_float_sel(&mut self, path: Option<Path>) {
        self.f7.inline_sel = None;
        match path {
            None => self.float_sel = None,
            Some(p) => {
                let idx = self.drawing_paths().iter().position(|q| *q == p);
                match idx {
                    Some(i) => self.float_sel = u32::try_from(i).ok(),
                    None => {
                        // an inline picture
                        self.float_sel = None;
                        self.f7.inline_sel = Some(p);
                    }
                }
            }
        }
    }

    /// What the paint of the handles does through `selectedFloat()`: an inline picture stays
    /// selected only while the text selection is exactly over its run.
    pub(crate) fn settle_float_sel(&mut self) {
        let Some(p) = self.f7.inline_sel.clone() else { return };
        let over = match p.as_slice() {
            [Step::Para { para, run }] => self.paras().and_then(|ps| ps.get(*para as usize)).is_some_and(|pp| {
                let at: usize = pp.runs.iter().take(*run as usize).map(|r| r.text.len()).sum();
                let len = pp.runs.get(*run as usize).map_or(0, |r| r.text.len());
                let (s, e) = {
                    let r = super::super::text::normalize_range(self.range());
                    (r.start, r.end)
                };
                s.para == *para && e.para == *para && s.offset as usize == at && e.offset as usize == at + len
            }),
            // an inline picture of a table cell or of a box is not under the selection of the story
            _ => false,
        };
        if !over {
            self.f7.inline_sel = None;
        }
    }

    // ── the edit pipeline (`editDrawing`, `applyDrawingXml`) ───────────────────────────────

    /// `patchDrawing(r, ch)`: `patchPicture` for a picture, `patchShape` for the rest.
    fn patch_drawing(&self, path: &[Step], ch: &ShapeChange) -> Option<String> {
        let r = self.run_ref(path)?;
        let raw = raw_of(r)?;
        if is_image(r) { patch_picture(raw, &ch.pic).ok() } else { patch_shape(raw, ch).ok() }
    }

    /// `applyDrawingXml(run, xml, extent)` on the run at `path` (the step is taken).
    pub(crate) fn apply_drawing_xml(&mut self, path: &[Step], xml: String, extent: bool) {
        // `floatSel` is the run itself: when the object stops floating (or starts to) the list of
        // floating objects changes under the index, the selection follows the run
        let keep = if self.float_sel.is_some() || self.f7.inline_sel.is_some() { self.sel_path() } else { None };
        self.apply_drawing_xml_inner(path, xml, extent);
        if keep.is_some() {
            self.set_float_sel(keep);
        }
    }

    fn apply_drawing_xml_inner(&mut self, path: &[Step], xml: String, extent: bool) {
        let Some(run0) = self.run_ref(path) else { return };
        let image = is_image(run0);
        let mut x = xml;
        if extent {
            let f = read_picture_format(&x, !image);
            if let Some(f) = f {
                let half_line = if image {
                    None
                } else {
                    match &f.line {
                        Some(Nullable::Val(l)) if l.w.is_some() => {
                            let mut l2 = (**l).clone();
                            l2.w = l.w.map(|w| JsNumber::Num(w.to_number() / 2.0));
                            Some(l2)
                        }
                        _ => None,
                    }
                };
                let ee = pure::effect_extent_of(&f, half_line.as_ref());
                let cur = &f.effect_extent;
                if ee.l != cur.l.to_number() || ee.t != cur.t.to_number() || ee.r != cur.r.to_number() || ee.b != cur.b.to_number() {
                    let ch = PictureChange { effect_extent: Some(ee), ..PictureChange::default() };
                    if let Ok(y) = patch_picture(&x, &ch) {
                        x = y;
                    }
                }
            }
        }
        let Some(run) = self.run_mut(path) else { return };
        if image {
            if let Some(img) = run.ext_mut().image.as_deref_mut() {
                obj::set(img, "rawDrawingXml", |i| i.raw_drawing_xml = Some(x.clone().into()));
                obj::set(img, "edited", |i| i.edited = Some(true));
                sync_image_from_xml(img);
            }
            return;
        }
        let ext = run.ext_mut();
        if let Some(t) = ext.textbox.as_deref_mut() {
            obj::set(t, "rawDrawingXml", |i| i.raw_drawing_xml = Some(x.clone().into()));
            obj::set(t, "edited", |i| i.edited = Some(true));
        } else if let Some(s) = ext.shape.as_deref_mut() {
            obj::set(s, "rawDrawingXml", |i| i.raw_drawing_xml = Some(x.clone().into()));
            obj::set(s, "edited", |i| i.edited = Some(true));
        } else if let Some(g) = ext.group.as_deref_mut() {
            obj::set(g, "rawDrawingXml", |i| i.raw_drawing_xml = Some(x.clone().into()));
            obj::set(g, "edited", |i| i.edited = Some(true));
        }
        sync_shape_from_xml(ext);
    }

    /// `editDrawing(fn, imageOnly)`: one change of the selected object, one undo step; `f` answers
    /// `None` when there is nothing to change.
    pub(crate) fn edit_drawing(&mut self, image_only: bool, f: &mut dyn FnMut(&Fmt, &Run) -> Option<ShapeChange>) -> bool {
        let path = if image_only { self.pic_path() } else { self.draw_path() };
        let Some(path) = path else { return false };
        if self.locked() {
            return false;
        }
        let Some(run) = self.run_ref(&path) else { return false };
        let Some(fmt) = format_of(run) else { return false };
        let Some(ch) = f(&fmt, run) else { return false };
        if !change_is_finite(&ch) {
            return false;
        }
        let Some(xml) = self.patch_drawing(&path, &ch) else { return false };
        self.begin_step(false, &[]);
        let extent = has_extent_key(&ch);
        self.apply_drawing_xml(&path, xml, extent);
        self.after_picture_edit();
        true
    }

    /// `afterPictureEdit()` (the layout refresh is the settle step).
    pub(crate) fn after_picture_edit(&mut self) {
        self.changed();
    }

    /// `zValues(except)`: the `relativeHeight`s of the floating objects of the paragraphs of the
    /// current story.
    pub(crate) fn z_values(&self, except: Option<&[Step]>) -> Vec<f64> {
        let mut out = Vec::new();
        let Some(paras) = self.paras() else { return out };
        for (pi, p) in paras.iter().enumerate() {
            for (ri, r) in p.runs.iter().enumerate() {
                if let (Some(ex), true) = (except, matches!(self.ctx, EditContext::Body | EditContext::HeaderFooter { .. })) {
                    let me = [Step::Para { para: u32::try_from(pi).unwrap_or(u32::MAX), run: u32::try_from(ri).unwrap_or(u32::MAX) }];
                    if ex == me {
                        continue;
                    }
                }
                let Some(e) = r.ext() else { continue };
                let a = e
                    .image
                    .as_deref()
                    .and_then(|i| i.anchor.as_deref())
                    .or_else(|| e.textbox.as_deref().and_then(|t| t.anchor.as_deref()))
                    .or_else(|| e.shape.as_deref().and_then(|s| s.anchor.as_deref()))
                    .or_else(|| e.group.as_deref().and_then(|g| g.anchor.as_deref()));
                if let Some(h) = a.and_then(|a| a.relative_height)
                    && let Some(v) = h.as_f64()
                {
                    out.push(v);
                }
            }
        }
        out
    }

    /// `nextDrawingId()`: the largest `wp:docPr` id of the story, the boxes inside them and the
    /// header/footer parts, plus one.
    pub(crate) fn next_drawing_id(&mut self) -> f64 {
        fn scan_story(s: &Story, max: &mut f64, depth: usize) {
            if depth > loc::MAX_DEPTH {
                return;
            }
            for p in s.paras.iter() {
                for r in &p.runs {
                    scan_run(r, max, depth);
                }
            }
            for a in s.tables.iter() {
                for row in &a.table.rows {
                    for c in &row.cells {
                        scan_story(&c.content, max, depth + 1);
                    }
                }
            }
        }
        fn scan_run(r: &Run, max: &mut f64, depth: usize) {
            if let Some(raw) = raw_of(r) {
                for d in rx::doc_pr_ids(raw) {
                    *max = math::max(*max, uo_jsnum::parse::to_number(d));
                }
            }
            if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref()) {
                scan_story(&tb.content, max, depth + 1);
            }
        }
        let mut max = 0.0f64;
        scan_story(&self.doc.body, &mut max, 0);
        for h in self.doc.hf.iter() {
            scan_story(&h.story, &mut max, 0);
        }
        if max >= 2_147_483_646.0 {
            // ids this big (Word draws 9-digit ones): the next must fit the schema's int
            let r = f64::from(self.env_mut().random_u32()) / 4_294_967_296.0;
            return 1.0 + math::floor(r * 2_147_483_646.0);
        }
        max + 1.0
    }

    /// `newRunProps(base)`: the props without revisions, with the new revision when tracking.
    pub(crate) fn new_run_props_from(&mut self, base: &crate::model::run::RunProps) -> crate::model::run::RunProps {
        let mut p = super::super::text::without_revisions(base);
        if self.opts.tracking {
            let rev = self.new_rev();
            obj::set(&mut p, "ins", |x| x.ins = Some(Box::new(rev)));
        }
        p
    }

    // ── textbox chains ─────────────────────────────────────────────────────────────────────

    fn tb_at(&self, path: &[Step]) -> Option<&DrawingTextbox> {
        self.run_ref(path)?.ext()?.textbox.as_deref()
    }

    /// `textboxChain(tb)`: the paths of the boxes of the chain (the first first), or `None`.
    pub(crate) fn textbox_chain(&self, path: &[Step]) -> Option<Vec<Path>> {
        let tb = self.tb_at(path)?;
        if tb.story.is_none() && tb.link.is_none() {
            return None;
        }
        let all = self.textbox_paths();
        let mut head: Option<(f64, Path)> = None;
        let mut heads: Vec<(f64, Path)> = Vec::new();
        let mut links: Vec<(f64, f64, Path)> = Vec::new();
        for p in &all {
            let Some(t) = self.tb_at(p) else { continue };
            if let Some(l) = &t.link {
                links.push((l.id.to_number(), l.seq.to_number(), p.clone()));
            } else if let Some(s) = t.story
                && !heads.iter().any(|(id, _)| same_value_zero(*id, s.to_number()))
            {
                heads.push((s.to_number(), p.clone()));
            }
        }
        let id = match &tb.link {
            Some(l) => l.id.to_number(),
            None => tb.story.map_or(f64::NAN, JsNumber::to_number),
        };
        if let Some(h) = heads.iter().find(|(i, _)| same_value_zero(*i, id)) {
            head = Some(h.clone());
        }
        let mut ms: Vec<(f64, Path)> = links.into_iter().filter(|(i, _, _)| same_value_zero(*i, id)).map(|(_, s, p)| (s, p)).collect();
        ms.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let head = head?;
        if ms.is_empty() {
            return None;
        }
        let mut out = vec![head.1];
        out.extend(ms.into_iter().map(|m| m.1));
        Some(out)
    }

    /// `set(t, story)` of `unchainTextbox`: the box's XML takes the story change.
    fn set_story(&mut self, path: &[Step], story: StoryChange) {
        let Some(r) = self.run_ref(path) else { return };
        let Some(raw) = r.ext().and_then(|e| e.textbox.as_deref()).and_then(|t| t.raw_drawing_xml.as_deref()) else { return };
        let ch = ShapeChange { story: Some(story), ..ShapeChange::default() };
        if let Ok(x) = patch_shape(raw, &ch) {
            self.apply_drawing_xml(path, x, false);
        }
    }

    /// `unchainTextbox(tb)`: the text stays in the boxes that remain.
    fn unchain_textbox(&mut self, path: &[Step]) {
        let Some(chain) = self.textbox_chain(path) else { return };
        let Some(first) = chain.first() else { return };
        let Some(id) = self.tb_at(first).and_then(|t| t.story).map(JsNumber::to_number) else { return };
        let rest: Vec<Path> = chain.iter().filter(|p| p.as_slice() != path).cloned().collect();
        if chain.first().map(Vec::as_slice) == Some(path) {
            let Some(next) = rest.first().cloned() else { return };
            let moved = self.tb_at(path).map(|t| (t.content.clone(), t.view));
            if let (Some((content, view)), Some(run)) = (moved, self.run_mut(&next))
                && let Some(t) = run.ext_mut().textbox.as_deref_mut()
            {
                t.content = content;
                t.view = view;
            }
            let story = if rest.len() > 1 {
                StoryChange::Txbx { id: Some(id), content: Some("<w:p/>".into()) }
            } else {
                StoryChange::Txbx { id: None, content: Some("<w:p/>".into()) }
            };
            self.set_story(&next, story);
        } else if rest.len() == 1 {
            self.set_story(&rest[0], StoryChange::Txbx { id: None, content: None });
        }
        for (k, t) in rest.iter().enumerate().skip(1) {
            self.set_story(t, StoryChange::Linked { id, seq: k as f64 });
        }
    }

    // ── deleting ───────────────────────────────────────────────────────────────────────────

    /// `deleteSelectedFloat()`: removes the selected object (one step); a chained box leaves its
    /// chain first. `false` when nothing is selected.
    pub(crate) fn delete_selected_float(&mut self) -> bool {
        let Some(path) = self.sel_path() else { return false };
        self.begin_step(false, &[]);
        if self.tb_at(&path).is_some() {
            self.unchain_textbox(&path);
        }
        let mut done = false;
        let inline_pic =
            self.run_ref(&path).is_some_and(|r| r.ext().and_then(|e| e.image.as_deref()).is_some_and(|i| i.floating != Some(true)));
        if let Some((para, run)) = loc::top_level(&path)
            && matches!(self.ctx, EditContext::Body | EditContext::HeaderFooter { .. })
            && self.paras().is_some()
        {
            // the paragraph list the TS searches is the model's (body, or the part in edit)
            let off: usize = self
                .paras()
                .and_then(|ps| ps.get(para as usize))
                .map_or(0, |p| p.runs.iter().take(run as usize).map(|r| r.text.len()).sum());
            if let Some(ps) = self.base_story_mut().paras.make_mut(para as usize)
                && (run as usize) < ps.runs.len()
            {
                ps.runs.remove(run as usize);
                done = true;
            }
            if done {
                self.touch_para(para as usize);
                if inline_pic {
                    let at = DocPosition::new(para, u32::try_from(off).unwrap_or(u32::MAX));
                    self.selection = Selection { anchor: at, focus: at };
                }
            }
        }
        if !done && !matches!(self.ctx, EditContext::HeaderFooter { .. }) {
            // an object anchored in a table cell of the body, or inside another object
            let (idx, last) = path.split_at(path.len().saturating_sub(1));
            if let Some(Step::Para { para, run }) = last.first()
                && !idx.is_empty()
                && let Some(parent) = self.parent_story_mut(idx)
                && let Some(p) = parent.paras.make_mut(*para as usize)
                && (*run as usize) < p.runs.len()
            {
                p.runs.remove(*run as usize);
                done = true;
                self.touch_all();
            }
        }
        let _ = done;
        self.f7.inline_sel = None;
        self.float_sel = None;
        self.f7.multi.clear();
        self.changed();
        true
    }

    /// The story a path whose last step is a paragraph run lives in (`prefix` = the path without
    /// its last step: cell steps only).
    fn parent_story_mut(&mut self, prefix: &[Step]) -> Option<&mut Story> {
        let mut story = self.base_story_mut();
        for s in prefix {
            match s {
                Step::Cell { anchor, row, col } => {
                    let a = story.tables.make_mut(*anchor as usize)?;
                    let t = Arc::make_mut(&mut a.table);
                    story = &mut t.rows.get_mut(*row as usize)?.cells.get_mut(*col as usize)?.content;
                }
                _ => return None,
            }
        }
        Some(story)
    }

    // ── the pane ───────────────────────────────────────────────────────────────────────────

    /// `paneRuns()`: the objects of the pane with their key (`docPr` id or `#n`) and formatting.
    pub(crate) fn pane_runs(&self) -> Vec<(Path, String, Fmt)> {
        let mut out = Vec::new();
        let mut n = 0usize;
        for p in self.drawing_paths() {
            let Some(r) = self.run_ref(&p) else { continue };
            if raw_of(r).is_none() {
                continue;
            }
            let Some(f) = format_of(r) else { continue };
            if is_image(r) && f.base().inline {
                continue;
            }
            n += 1;
            let key = f.base().doc_pr.id.as_deref().map_or_else(|| format!("#{n}"), str::to_owned);
            out.push((p, key, f));
        }
        out
    }

    /// The keys of the selection pane's objects (`selectionPaneItems`), in document order.
    #[must_use]
    pub fn pane_keys(&self) -> Vec<String> {
        self.pane_runs().into_iter().map(|x| x.1).collect()
    }

    fn pane_path(&self, key: &str) -> Option<Path> {
        self.pane_runs().into_iter().find(|x| x.1 == key).map(|x| x.0)
    }

    // ── the queries of the shell ───────────────────────────────────────────────────────────

    /// `stringifyState().fs`: the index of the selected object in the drawing runs of the context
    /// (`None`: nothing selected, or the selection is an inline picture).
    #[must_use]
    pub fn float_selection(&self) -> Option<u32> {
        self.float_sel
    }

    /// `selectedObjectCount()`: the selected object and the others of the multi-selection. The TS
    /// counts the handles `enumFloats()` finds on the laid-out pages (`selectedFloats().length ||
    /// 1`); the model has no layout, so every selected object counts (F7-D6).
    #[must_use]
    pub fn selected_object_count(&self) -> usize {
        let Some(sel) = self.sel_path() else { return 0 };
        let mut n = 1usize;
        let mut seen: Vec<&Path> = vec![&sel];
        for p in &self.f7.multi {
            if !seen.contains(&p) && self.run_ref(p).is_some() {
                seen.push(p);
                n += 1;
            }
        }
        n
    }

    /// `selectionPaneItems()`: the objects of the pane, the front-most first.
    #[must_use]
    pub fn selection_pane_items(&self) -> Vec<PaneItem> {
        let cur = self.sel_path().or_else(|| self.focus_path());
        let multi_on = self.sel_path().is_some();
        let mut out: Vec<(f64, PaneItem)> = Vec::new();
        for (path, key, f) in self.pane_runs() {
            let Some(r) = self.run_ref(&path) else { continue };
            let b = f.base();
            let kind = match r.ext() {
                Some(e) if e.image.is_some() => PaneKind::Image,
                Some(e) if e.textbox.is_some() => PaneKind::Textbox,
                Some(e) if e.group.is_some() => PaneKind::Group,
                _ => PaneKind::Shape,
            };
            let selected = cur.as_ref() == Some(&path) || (multi_on && self.f7.multi.contains(&path));
            let z = b.relative_height.map_or(0.0, JsNumber::to_number);
            out.push((
                z,
                PaneItem {
                    key,
                    name: b.doc_pr.name.as_deref().unwrap_or("").to_owned(),
                    hidden: b.doc_pr.hidden.unwrap_or(false),
                    selected,
                    kind,
                },
            ));
        }
        // `sort((x, y) => y.z - x.z)`: stable, front-most first
        out.sort_by(|x, y| {
            let d = y.0 - x.0;
            if d > 0.0 {
                std::cmp::Ordering::Greater
            } else if d < 0.0 {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        });
        out.into_iter().map(|x| x.1).collect()
    }

    /// `selectedShape()` / `selectedImage()`: the model values of the object `drawRun()` /
    /// `picRun()` finds (`want_image`), with the unit conversions (`/ 12700`, `/ 60000`) and the
    /// labels left to the shell (F7-D5).
    #[must_use]
    pub fn selected_object_state(&self, want_image: bool) -> Option<ObjectState> {
        let path = if want_image { self.pic_path()? } else { self.draw_path()? };
        let run = self.run_ref(&path)?;
        if !want_image && is_image(run) {
            return None;
        }
        let f = format_of(run)?;
        let b = f.base();
        let rel = b.pos_v.as_ref().map(|p| &*p.relative_from);
        let tb = run.ext().and_then(|e| e.textbox.as_deref());
        let chain = tb.and_then(|_| self.textbox_chain(&path));
        let (kind, style_index, text) = if want_image {
            (PaneKind::Image, 0, None)
        } else {
            let kind = if run.ext().is_some_and(|e| e.group.is_some()) {
                PaneKind::Group
            } else if tb.is_some() {
                PaneKind::Textbox
            } else {
                PaneKind::Shape
            };
            let (si, text) = match &f {
                Fmt::Shape(sf) => (
                    raw_of(run).map_or(0, rx::shape_style_index),
                    sf.body.as_ref().map(|bd| TextState {
                        margins_emu: [bd.l_ins, bd.t_ins, bd.r_ins, bd.b_ins],
                        anchor: match bd.anchor {
                            crate::write::emit::object::BodyAnchor::T => "t",
                            crate::write::emit::object::BodyAnchor::Ctr => "ctr",
                            crate::write::emit::object::BodyAnchor::B => "b",
                        },
                        direction: bd.vert.clone(),
                        wrap: bd.wrap != crate::write::emit::object::BodyWrap::None,
                        autofit: match bd.autofit {
                            crate::write::emit::object::BodyAutofit::None => "none",
                            crate::write::emit::object::BodyAutofit::Shape => "shape",
                            crate::write::emit::object::BodyAutofit::Norm => "norm",
                        },
                        upright: bd.upright,
                    }),
                ),
                Fmt::Pic(_) => (0, None),
            };
            (kind, si, text)
        };
        let link = chain.as_ref().map(|c| {
            let i = c.iter().position(|p| *p == path);
            (i.is_some_and(|i| i + 1 < c.len()), i.is_some_and(|i| i > 0))
        });
        Some(ObjectState {
            kind,
            inline: b.inline,
            wrap: b.wrap.to_string(),
            cx: b.cx.to_number(),
            cy: b.cy.to_number(),
            rot: b.rot.to_number(),
            flip_h: b.flip_h,
            flip_v: b.flip_v,
            lock_aspect: b.no_change_aspect,
            move_with_text: !b.inline && matches!(rel, Some("paragraph" | "line")),
            lock_anchor: b.locked,
            name: b.doc_pr.name.as_deref().unwrap_or("").to_owned(),
            alt_text: b.doc_pr.descr.as_deref().unwrap_or("").to_owned(),
            decorative: b.doc_pr.decorative.unwrap_or(false),
            geometry: b.geom.to_string(),
            style_index,
            editing_text: !want_image && self.focus_path().as_ref() == Some(&path),
            text,
            link_next: link.is_some_and(|l| l.0),
            link_previous: link.is_some_and(|l| l.1),
            format: b.clone(),
        })
    }

    // ── the commands ───────────────────────────────────────────────────────────────────────

    pub(crate) fn exec_drawing(&mut self, cmd: DrawingCmd, cx: &mut ExecCx<'_>) -> Res {
        match cmd {
            DrawingCmd::SelectObject { index } => {
                self.f7.multi.clear();
                match index {
                    None => self.set_float_sel(None),
                    Some(i) => {
                        let p = self.drawing_paths().get(i as usize).cloned();
                        self.set_float_sel(p);
                    }
                }
                self.done7()
            }
            DrawingCmd::SelectInline { para, run } => {
                let path = vec![Step::Para { para, run }];
                let Some(r) = self.run_ref(&path) else { return self.done7() };
                if !r.ext().is_some_and(|e| e.image.is_some()) {
                    return self.done7();
                }
                let len = u32::try_from(r.text.len()).unwrap_or(u32::MAX);
                let off: u32 = self.paras().and_then(|ps| ps.get(para as usize)).map_or(0, |p| {
                    u32::try_from(p.runs.iter().take(run as usize).map(|r| r.text.len()).sum::<usize>()).unwrap_or(u32::MAX)
                });
                self.set_float_sel(Some(path));
                self.selection = Selection { anchor: DocPosition::new(para, off), focus: DocPosition::new(para, off.saturating_add(len)) };
                self.done7()
            }
            DrawingCmd::SelectPaneItem { key, add } => {
                let Some(p) = self.pane_path(&key) else { return self.done7() };
                if matches!(self.ctx, EditContext::Textbox { .. }) {
                    self.exit_textbox();
                }
                if add {
                    self.toggle_float_in_selection(p);
                } else {
                    self.set_float_sel(Some(p));
                    self.f7.multi.clear();
                }
                self.done7()
            }
            DrawingCmd::SelectPaneItems { keys } => {
                let paths: Vec<Path> = keys.iter().filter_map(|k| self.pane_path(k)).collect();
                if matches!(self.ctx, EditContext::Textbox { .. }) {
                    self.exit_textbox();
                }
                self.set_float_sel(paths.first().cloned());
                self.f7.multi = paths.into_iter().skip(1).collect();
                self.done7()
            }
            DrawingCmd::RenamePaneItem { key, name } => {
                let Some(p) = self.pane_path(&key) else { return self.done7() };
                let Some(f) = self.run_ref(&p).and_then(format_of) else { return self.done7() };
                let nm = name.trim_matches(|c: char| uo_jsnum::text::is_js_space(c) || c == '\u{feff}');
                if self.locked() || nm.is_empty() || f.base().doc_pr.name.as_deref() == Some(&*name) {
                    return self.done7();
                }
                let ch =
                    ShapeChange { pic: PictureChange { name: Some(nm.to_owned()), ..PictureChange::default() }, ..ShapeChange::default() };
                if let Some(x) = self.patch_drawing(&p, &ch) {
                    self.begin_step(false, &[]);
                    self.apply_drawing_xml(&p, x, false);
                    self.after_picture_edit();
                }
                self.done7()
            }
            DrawingCmd::SetPaneItemHidden { key, hidden } => {
                if self.locked() {
                    return self.done7();
                }
                let runs: Vec<(Path, String, Fmt)> = match &key {
                    None => self.pane_runs(),
                    Some(k) => self.pane_runs().into_iter().filter(|x| x.1 == **k).collect(),
                };
                let todo: Vec<Path> =
                    runs.into_iter().filter(|(_, _, f)| f.base().doc_pr.hidden.unwrap_or(false) != hidden).map(|x| x.0).collect();
                if todo.is_empty() {
                    return self.done7();
                }
                let mut patched = Vec::new();
                for p in &todo {
                    let ch =
                        ShapeChange { pic: PictureChange { hidden: Some(hidden), ..PictureChange::default() }, ..ShapeChange::default() };
                    if let Some(x) = self.patch_drawing(p, &ch) {
                        patched.push((p.clone(), x));
                    }
                }
                self.begin_step(false, &[]);
                for (p, x) in patched {
                    self.apply_drawing_xml(&p, x, false);
                }
                self.after_picture_edit();
                self.done7()
            }
            DrawingCmd::SetShapeFill { fill } => {
                let f = fill.unwrap_or(ShapeFill::None);
                self.edit_drawing(false, &mut |_, run| {
                    if run.ext().is_some_and(|e| e.group.is_some()) {
                        None
                    } else {
                        Some(ShapeChange { fill: Some(f.clone()), ..ShapeChange::default() })
                    }
                });
                self.done7()
            }
            DrawingCmd::SetShapePictureFill { data, ext, tile } => self.set_shape_picture_fill(&data, &ext, tile),
            DrawingCmd::SetShapeOutline { color, weight_pt, dash, compound } => {
                self.edit_drawing(false, &mut |f, run| {
                    if run.ext().is_some_and(|e| e.group.is_some()) {
                        return None;
                    }
                    let line = f.base().line.as_ref().and_then(Nullable::as_val);
                    if matches!(color, Some(Nullable::Null)) {
                        return if matches!(line.and_then(|l| l.fill.as_ref()), Some(Nullable::Null)) {
                            None
                        } else {
                            Some(ShapeChange { outline: Some(None), ..ShapeChange::default() })
                        };
                    }
                    let mut base = match line {
                        Some(l) if matches!(l.fill, Some(Nullable::Val(_))) => (**l).clone(),
                        other => PicLine {
                            w: Some(other.and_then(|l| l.w).unwrap_or(JsNumber::Num(9525.0))),
                            fill: Some(Nullable::Val(PicColor { kind: "schemeClr".into(), val: "tx1".into(), ..PicColor::default() })),
                            ..PicLine::default()
                        },
                    };
                    if let Some(Nullable::Val(c)) = &color {
                        obj::set(&mut base, "fill", |l| l.fill = Some(Nullable::Val(c.clone())));
                    }
                    if let Some(w) = weight_pt {
                        let v = math::max(0.0, math::round(w * 12700.0));
                        obj::set(&mut base, "w", |l| l.w = Some(JsNumber::Num(v)));
                    }
                    if let Some(d) = &dash {
                        if &**d == "solid" {
                            obj::delete(&mut base, "dash");
                        } else {
                            obj::set(&mut base, "dash", |l| l.dash = Some(d.clone()));
                        }
                    }
                    if let Some(c) = &compound {
                        if &**c == "sng" {
                            obj::delete(&mut base, "cmpd");
                        } else {
                            obj::set(&mut base, "cmpd", |l| l.cmpd = Some(c.clone()));
                        }
                    }
                    Some(ShapeChange { outline: Some(Some(base)), ..ShapeChange::default() })
                });
                self.done7()
            }
            DrawingCmd::SetShapeFormat { fmt } => {
                if self.draw_is_image() {
                    return self.done7();
                }
                self.edit_drawing(false, &mut |_, run| {
                    if run.ext().is_some_and(|e| e.group.is_some()) {
                        return None;
                    }
                    let mut ch = ShapeChange::default();
                    apply_format_arg(&mut ch.pic, &fmt);
                    if let Some(l) = &fmt.line {
                        ch.outline = Some(l.as_val().cloned());
                    }
                    Some(ch)
                });
                self.done7()
            }
            DrawingCmd::SetShapeStyle { n } => {
                let Some(p) = crate::drawing::shape_styles::shape_style_preset(n) else { return self.done7() };
                self.edit_drawing(false, &mut |_, run| {
                    if run.ext().is_some_and(|e| e.group.is_some()) {
                        None
                    } else {
                        Some(ShapeChange {
                            style_preset: Some(crate::write::emit::object::StylePreset {
                                sp_pr: p.sp_pr.to_owned(),
                                style: p.style.to_owned(),
                            }),
                            ..ShapeChange::default()
                        })
                    }
                });
                self.done7()
            }
            DrawingCmd::ChangeShape { prst } => {
                self.edit_drawing(false, &mut |f, run| {
                    if run.ext().is_some_and(|e| e.group.is_some()) || *f.base().geom == *prst {
                        None
                    } else {
                        Some(ShapeChange {
                            pic: PictureChange {
                                geom: Some(crate::write::emit::fx::GeomChange { prst: prst.to_string(), adj: None }),
                                ..PictureChange::default()
                            },
                            ..ShapeChange::default()
                        })
                    }
                });
                self.done7()
            }
            DrawingCmd::SetTextboxOptions { o } => {
                self.edit_drawing(false, &mut |f, _| {
                    let Fmt::Shape(sf) = f else { return None };
                    let b = sf.body.as_ref()?;
                    let patch = textbox_patch(&o);
                    let changed = patch.l_ins.is_some_and(|v| v != b.l_ins)
                        || patch.t_ins.is_some_and(|v| v != b.t_ins)
                        || patch.r_ins.is_some_and(|v| v != b.r_ins)
                        || patch.b_ins.is_some_and(|v| v != b.b_ins)
                        || patch.anchor.as_ref().is_some_and(|v| *v != b.anchor)
                        || patch.vert.as_ref().is_some_and(|v| *v != b.vert)
                        || patch.wrap.is_some_and(|v| v != b.wrap)
                        || patch.autofit.is_some_and(|v| v != b.autofit)
                        || patch.upright.is_some_and(|v| v != b.upright);
                    changed.then(|| ShapeChange { body: Some(patch), ..ShapeChange::default() })
                });
                self.done7()
            }
            DrawingCmd::OnShape(c) => {
                if self.draw_is_image() {
                    return self.done7();
                }
                self.exec_pictures(*c, cx)
            }
            DrawingCmd::SetShapeEffect { kind, preset } => {
                if self.draw_is_image() {
                    return self.done7();
                }
                self.exec_pictures(PictureCmd::SetEffect { kind, preset }, cx)
            }
            DrawingCmd::DeleteSelectedShape => {
                let Some(p) = self.draw_path() else { return self.done7() };
                if self.draw_is_image() || self.locked() {
                    return self.done7();
                }
                if matches!(self.ctx, EditContext::Textbox { .. }) {
                    self.exit_textbox();
                    self.set_float_sel(Some(p));
                }
                self.delete_selected_float();
                self.done7()
            }
            DrawingCmd::LinkTextboxTo { key } => {
                let from = self.draw_path();
                let to = self.pane_path(&key);
                if let (Some(from), Some(to)) = (from, to) {
                    self.link_textboxes(&from, &to);
                }
                self.done7()
            }
            DrawingCmd::BreakTextboxLink => {
                self.break_textbox_link();
                self.done7()
            }
            DrawingCmd::InsertTextbox { id, page, theme_accent1, lang, user } => {
                self.insert_textbox(&id, page, theme_accent1.as_deref(), &lang, user.as_ref())
            }
            DrawingCmd::CreateDrawnTextbox { page, rect, vertical, anchor_para, lang } => {
                self.create_drawn_textbox(page, rect, vertical, anchor_para, &lang)
            }
            DrawingCmd::PlaceAt { frame, x, y } => {
                self.place_at(&frame, x, y);
                self.done7()
            }
            DrawingCmd::SetAsDefaultTextbox => {
                self.set_as_default_textbox();
                self.done7()
            }
            DrawingCmd::AlignObjects { kind, rects } => self.align_objects(kind, &rects, cx),
            DrawingCmd::EditSelectedTextbox { page } => self.edit_selected_textbox(page),
        }
    }

    /// `drawRun()?.image`.
    pub(crate) fn draw_is_image(&self) -> bool {
        self.draw_path().and_then(|p| self.run_ref(&p).map(is_image)).unwrap_or(false)
    }

    /// `toggleFloatInSelection(run)`.
    fn toggle_float_in_selection(&mut self, p: Path) {
        let Some(cur) = self.sel_path() else {
            self.set_float_sel(Some(p));
            self.f7.multi.clear();
            return;
        };
        if cur == p {
            let mut rest: Vec<Path> = self.f7.multi.iter().filter(|r| **r != p).cloned().collect();
            let first = if rest.is_empty() { None } else { Some(rest.remove(0)) };
            self.set_float_sel(first);
            self.f7.multi = rest;
            return;
        }
        if self.f7.multi.contains(&p) {
            self.f7.multi.retain(|r| *r != p);
        } else {
            self.f7.multi.push(p);
        }
    }

    pub(crate) fn complete_drawing(&mut self, _cmd: DrawingCmd, _job: JobId, _result: JobResult, _cx: &mut ExecCx<'_>) -> Res {
        self.f7.clear_pending();
        Err(CommandError::UnknownJob)
    }

    /// `setShapePictureFill(data, ext, tile)`.
    fn set_shape_picture_fill(&mut self, data: &[u8], ext: &str, tile: bool) -> Res {
        let Some(path) = self.draw_path() else { return self.done7() };
        let is_plain = self.run_ref(&path).is_some_and(|r| !is_image(r) && !r.ext().is_some_and(|e| e.group.is_some()));
        if !is_plain || self.locked() {
            return self.done7();
        }
        let tile_fill = tile.then(|| crate::write::emit::object::FillTile {
            tx: 0.0,
            ty: 0.0,
            sx: 100_000.0,
            sy: 100_000.0,
            flip: "none".into(),
            algn: "tl".into(),
        });
        let fill = ShapeFill::Picture { embed: "uw:fill".into(), tile: tile_fill, alpha: None };
        let ok = self.edit_drawing(false, &mut |_, _| Some(ShapeChange { fill: Some(fill.clone()), ..ShapeChange::default() }));
        if ok {
            // the new media: the drawing and the save find it in the style (`blip`)
            let id = self.doc.media.add(data);
            let ext_s: Str = ext.into();
            let tile_style = tile.then(|| crate::model::drawing::BlipTile {
                tx: JsNumber::Num(0.0),
                ty: JsNumber::Num(0.0),
                sx: JsNumber::Num(100_000.0),
                sy: JsNumber::Num(100_000.0),
                algn: "tl".into(),
                flip: "none".into(),
                ..Default::default()
            });
            let blip = crate::model::drawing::ShapeBlip {
                image: crate::model::media::ImageData { data: crate::model::media::MediaRef(id), ext: ext_s, ..Default::default() },
                tile: tile_style,
                alpha: None,
                ..Default::default()
            };
            if let Some(run) = self.run_mut(&path) {
                let e = run.ext_mut();
                if let Some(t) = e.textbox.as_deref_mut() {
                    if t.shape.is_none() {
                        obj::set(t, "shape", |x| {
                            x.shape = Some(Box::new(crate::model::drawing::ShapeStyle { geom: "rect".into(), ..Default::default() }));
                        });
                    }
                    if let Some(s) = t.shape.as_deref_mut() {
                        obj::set(s, "blip", |x| x.blip = Some(Box::new(blip)));
                    }
                } else if let Some(sh) = e.shape.as_deref_mut() {
                    obj::set(&mut sh.style, "blip", |x| x.blip = Some(Box::new(blip)));
                }
            }
        }
        self.done7()
    }
}

/// JS `SameValueZero` for the chain ids.
fn same_value_zero(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

fn textbox_patch(o: &TextboxOptions) -> TextboxBodyPatch {
    use crate::write::emit::object::{BodyAnchor, BodyAutofit, BodyWrap};
    let pt = |v: Option<f64>| v.map(|p| math::round(p * 12700.0));
    TextboxBodyPatch {
        l_ins: pt(o.margin_l_pt),
        t_ins: pt(o.margin_t_pt),
        r_ins: pt(o.margin_r_pt),
        b_ins: pt(o.margin_b_pt),
        anchor: o.anchor.as_deref().and_then(|a| match a {
            "t" => Some(BodyAnchor::T),
            "ctr" => Some(BodyAnchor::Ctr),
            "b" => Some(BodyAnchor::B),
            _ => None,
        }),
        vert: o.direction.as_deref().map(str::to_owned),
        wrap: o.wrap.map(|w| if w { BodyWrap::Square } else { BodyWrap::None }),
        autofit: o.autofit.as_deref().and_then(|a| match a {
            "none" => Some(BodyAutofit::None),
            "shape" => Some(BodyAutofit::Shape),
            "norm" => Some(BodyAutofit::Norm),
            _ => None,
        }),
        upright: o.upright,
        ..TextboxBodyPatch::default()
    }
}

/// The given keys of a [`FormatArg`] into a `PictureChange`.
pub(crate) fn apply_format_arg(p: &mut PictureChange, fmt: &FormatArg) {
    if let Some(l) = &fmt.line {
        p.line = Some(l.clone());
    }
    if let Some(v) = &fmt.shadow {
        p.shadow = Some(v.clone());
    }
    if let Some(v) = &fmt.reflection {
        p.reflection = Some(v.clone());
    }
    if let Some(v) = &fmt.glow {
        p.glow = Some(v.clone());
    }
    if let Some(v) = &fmt.soft_edge {
        p.soft_edge = Some(v.clone());
    }
    if let Some(v) = &fmt.three_d {
        p.three_d = Some(v.clone());
    }
}

impl DrawingCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        let m = match self {
            DrawingCmd::InsertTextbox { .. } => "insertTextbox",
            DrawingCmd::CreateDrawnTextbox { .. } => "drawTextbox",
            DrawingCmd::SetShapeFill { .. } | DrawingCmd::SetShapePictureFill { .. } => "setShapeFill",
            DrawingCmd::SetShapeOutline { .. } => "setShapeOutline",
            DrawingCmd::SetShapeEffect { .. } => "setShapeEffect",
            DrawingCmd::SetShapeStyle { .. } => "setShapeStyle",
            DrawingCmd::SetShapeFormat { .. } => "setShapeFormat",
            // the alias calls the guarded `setImage*` method
            DrawingCmd::OnShape(c) => return c.guard(),
            _ => return None,
        };
        Some((m, GuardArg::None))
    }
}
