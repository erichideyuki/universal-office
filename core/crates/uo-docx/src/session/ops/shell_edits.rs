//! Wave 7 (A2) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7 — A2: lists, paste,
//! tables, objects and the page"): the edits of the TS shell's gestures that no [`super::super::Command`]
//! names yet — the table tools and handles, the Quick Tables, the wrap points, the multi-selection's move and
//! delete, the drag commits of the objects (ED-202/204/215), the OLE replacement. A separate entry point, [`Session::exec_shell_edit`], so the
//! session's generated wire codec (`Command`) does not change during the wave (decision 1 of the record: fold
//! these into `Command` at the milestone gate). The same rules as [`Session::exec`]: a pending two-phase
//! command refuses it (`Busy`), the step is the TS's (one undo step each, `oneUndo` where the TS uses it).

use super::super::command::{CommandError, Outcome};
use super::super::{ExecCx, Session};
use super::drawing::ObjFrame;
use super::drawing::{ObjFacts, Path, Step};
use super::tables::completion::{FilledTable, TableGesture};
use crate::js::{JsNumber, Nullable};
use crate::model::picture::WrapPolygon;
use crate::session::DocPosition;
use crate::write::emit::fx::PictureChange;
use crate::write::emit::object::ShapeChange;

/// One shell edit (see the module documentation).
#[derive(Debug, Clone, PartialEq)]
pub enum ShellEdit {
    /// `insertQuickTable(id)` (universal-write.ts 20394) with the content the shell built
    FilledTable(Box<FilledTable>),
    /// the table tools, the edge drag and the handles (universal-write.ts 20646–21000)
    Table(Box<TableGesture>),
    /// `wrapPointsMouseDown`'s commit (universal-write.ts 14280): the picture's polygon, `edited` (the closing
    /// point is added here, as the TS `[...next, next[0]]`)
    SetWrapPolygon { pts: Vec<(f64, f64)> },
    /// `moveSelectedFloatsBy(dx, dy)` (universal-write.ts 9256): every selected floating object, one step; `facts`
    /// one entry per selected object in selection order (as `AlignObjects`; no `rect`: left out)
    MoveSelectedBy { dx: f64, dy: f64, facts: Vec<ObjFacts> },
    /// `deleteSelectedFloats()` (universal-write.ts 9266): every selected object, one step (undo selects the main
    /// object again, as the TS's kept first snapshot)
    DeleteSelectedObjects,
    /// `editEmbeddedObject(run)`'s commit (universal-write.ts 20345): the body run `(para, run)` (an inline picture
    /// with an OLE object) gets the edited package and its new preview (`png`, twips); the raw drawing, the VML and
    /// the run's source go (the object is written anew). One step.
    ReplaceOle { para: u32, run: u32, data: std::sync::Arc<[u8]>, png: std::sync::Arc<[u8]>, width: f64, height: f64 },
    /// `moveInlineRun(run, to)` (universal-write.ts 13972, ED-204): the inline picture `(para, run)` of the context moves
    /// to `to` (a drop on itself only puts the caret before it); one step, the picture selected at its new place
    MoveInlineRun { para: u32, run: u32, to: DocPosition },
    /// the end of an object drag (`commitPictureDrag` 13853, `commitDrawingDrag` 13881, `startCropDrag`'s mouseup 14096;
    /// ED-202, ED-215): one step on the selected object
    CommitDrag(Box<DragCommit>),
    /// `defineListLevel({ ..., picture })` (universal-write.ts 4010): a picture bullet (the format's other fields as the
    /// session's `DefineListLevel`)
    DefineListLevelPicture { format: Box<super::lists::ListLevelFormat>, pic: Box<super::lists::PicBulletIn> },
    /// wave 7c (V): the mousedown of a live object gesture (a wrap-point drag, a move/resize/rotate drag, a Crop drag): the
    /// TS `snapshot()` (universal-write.ts 13730, 14063, 14258) — one undo step with the state before the gesture, the
    /// document dirty, `uw-change`; the gesture's live forms and its commit stay in this step
    LiveBegin,
    /// wave 7c (V): one live form of the open gesture (the TS writes the object's model fields on every move and lays the
    /// document out once per animation frame: 13757–13788, 14090–14106, 14270–14273): applied in place, no new step, no
    /// event; the paragraph is touched so the shell's incremental relayout follows. Ignored without an open gesture.
    LiveSet(Box<LiveForm>),
    /// wave 7c (V): the mouseup of a gesture that moved: the commit (`SetWrapPolygon` or `CommitDrag`) inside the
    /// gesture's step (one undo step, the state of the mousedown), `uw-change` once (the TS `changed()`)
    LiveEnd(Box<ShellEdit>),
    /// wave 7c (V): the mouseup of a gesture that did not move: the TS `discardSnapshot()` (the step goes; a document
    /// the step made dirty is clean again)
    LiveCancel,
}

/// A live form of an object gesture (see [`ShellEdit::LiveSet`]): what the TS writes on the model on every move.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveForm {
    /// the wrap polygon being edited (21600ths, without the closing point): the anchor's `wrapPoly`/`wrapBox` and the
    /// format's `polygon` (universal-write.ts 14270–14273)
    pub wrap: Option<Vec<(f64, f64)>>,
    /// the object's size (twips, rounded as `applyResize`/the crop drag round it)
    pub size: Option<(f64, f64)>,
    /// the object's `offsetXTwips`/`offsetYTwips` (rounded)
    pub offset: Option<(f64, f64)>,
    /// `moveAnchorTo(obj, x, y)` (21389): the anchor relative to the PAGE at the shown position (a `simplePos` takes it)
    pub page_at: Option<(f64, f64)>,
    /// the rotation (degrees): the picture's style, the box's, the shape's style or the group's `rot`
    pub rot: Option<f64>,
    /// the picture's crop (fractions, `pic.crop`)
    pub crop: Option<(f64, f64, f64, f64)>,
}

impl ShellEdit {
    /// The guarded TS method the edit reaches (`POLICY_GUARDS`; the TS wrappers of `installPolicyGuards` refuse the
    /// call): `insertQuickTable` and `finishDraw`'s new table call `this.insertTable`, a picture bullet is
    /// `defineListLevel`. The other shell paths call no guarded method in the TS (the table gestures go through
    /// `editTableAt`, the drag commits through `applyPictureXml`/`applyDrawingXml`, the multi-selection through
    /// `moveFloatBy`/`deleteSelectedFloat`, the OLE edit and the inline move write the model directly).
    pub(crate) fn guard(&self) -> Option<(&'static str, super::super::policy::GuardArg<'static>)> {
        use super::super::policy::GuardArg;
        match self {
            ShellEdit::FilledTable(_) => Some(("insertTable", GuardArg::None)),
            ShellEdit::Table(g) if matches!(**g, TableGesture::DrawNew { .. }) => Some(("insertTable", GuardArg::None)),
            ShellEdit::DefineListLevelPicture { .. } => Some(("defineListLevel", GuardArg::None)),
            _ => None,
        }
    }
}

/// What an object drag writes (see [`ShellEdit::CommitDrag`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DragCommit {
    /// a rotation drag: the angle in degrees (`rot = round(deg × 60000) % 21600000`)
    pub rot: Option<f64>,
    /// the object's size after the drag (twips; `size` in EMU), with a relative size made absolute (`reset_rel`)
    pub size: Option<(f64, f64)>,
    /// whether a relative size becomes absolute (the move and resize drags; not the Crop drag)
    pub reset_rel: bool,
    /// the crop of a Crop drag (fractions of the picture: `a:srcRect` in thousandths of a percent)
    pub src_rect: Option<(f64, f64, f64, f64)>,
    /// a floating object's new top-left (page twips) and its positioning frame: `posH`/`posV` in the same references
    pub at: Option<(f64, f64, ObjFrame)>,
    /// a group's size at the drag start: the children follow by the ratio of the new size to it
    pub group_start: Option<(f64, f64)>,
}

/// The sort key of a drawing path: later paths first, so that removing one does not move the others.
fn path_key(p: &Path) -> Vec<(u8, u32, u32, u32)> {
    p.iter()
        .map(|s| match *s {
            Step::Cell { anchor, row, col } => (0, anchor, row, col),
            Step::Para { para, run } => (1, para, run, 0),
            Step::Item(i) => (2, i, 0, 0),
            Step::Content => (3, 0, 0, 0),
        })
        .collect()
}

impl Session {
    /// Runs one shell edit (see [`ShellEdit`]).
    ///
    /// # Errors
    /// [`CommandError::Busy`] while a two-phase command waits for its jobs; the errors of the operations.
    pub fn exec_shell_edit(&mut self, e: ShellEdit, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        if self.pending.is_some() {
            return Err(CommandError::Busy);
        }
        self.tx = super::super::Tx::default();
        if let Some((_, rp)) = &self.policy
            && let Some((method, arg)) = e.guard()
            && !super::super::policy::guard_allows(rp, method, arg, self)
        {
            // refused as `exec` refuses a guarded command: no effect, the shell learns it
            self.tx.events.push(super::super::Event::Blocked { command: method.into() });
            return Ok(Outcome::Done(self.change_set()));
        }
        let out = match e {
            ShellEdit::FilledTable(q) => self.shell_filled_table(&q, cx),
            ShellEdit::Table(g) => self.shell_table_gesture(*g, cx),
            ShellEdit::SetWrapPolygon { pts } => self.shell_wrap_polygon(&pts),
            ShellEdit::LiveBegin => {
                // the TS `snapshot()` runs for any object drag (a VML object too: its drag commits nothing, 13800)
                if self.locked() {
                    return Ok(Outcome::Done(self.finish()));
                }
                self.begin_step(false, &[]);
                self.f7.live = Some(self.history.undo_len());
                Ok(Outcome::Done(self.finish()))
            }
            ShellEdit::LiveSet(f) => self.shell_live_set(&f),
            ShellEdit::LiveEnd(commit) => self.shell_live_end(*commit, cx),
            ShellEdit::LiveCancel => {
                if let Some(depth) = self.f7.live.take()
                    && self.history.undo_len() == depth
                {
                    self.discard_step();
                }
                Ok(Outcome::Done(self.finish_if_stepped()))
            }
            ShellEdit::MoveSelectedBy { dx, dy, facts } => {
                let mut paths: Vec<Path> = Vec::new();
                if let Some(p) = self.sel_path() {
                    paths.push(p);
                }
                for p in self.f7.multi.clone() {
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
                let moves: Vec<(Path, ObjFacts)> = if facts.len() == paths.len() {
                    paths.into_iter().zip(facts).filter(|(_, o)| o.rect.is_some()).collect()
                } else {
                    Vec::new()
                };
                if moves.is_empty() || (dx == 0.0 && dy == 0.0) || !dx.is_finite() || !dy.is_finite() || self.locked() {
                    return Ok(Outcome::Done(self.finish()));
                }
                self.begin_step(false, &[]);
                for (p, o) in &moves {
                    self.move_float_by(p, o, dx, dy);
                }
                self.changed();
                Ok(Outcome::Done(self.finish_if_stepped()))
            }
            ShellEdit::DeleteSelectedObjects => {
                let mut paths: Vec<Path> = Vec::new();
                if let Some(p) = self.sel_path() {
                    paths.push(p);
                }
                for p in self.f7.multi.clone() {
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
                self.f7.multi.clear();
                if paths.len() <= 1 {
                    self.delete_selected_float();
                    return Ok(Outcome::Done(self.finish_if_stepped()));
                }
                // the TS `oneUndo` keeps the FIRST snapshot, taken while `floatSel` is still the main object (the
                // runs are deleted in selection order there): undo gives that selection back. Here the paths go
                // last-first (the indices stay valid), so the kept step is taken first, quietly (the deletions'
                // own steps raise the `Change` events, one per object as the TS)
                let depth = self.history.undo_len();
                let quiet = self.quiet;
                self.quiet = true;
                self.begin_step(false, &[]);
                self.quiet = quiet;
                paths.sort_by_key(|p| std::cmp::Reverse(path_key(p)));
                for p in paths {
                    self.set_float_sel(Some(p));
                    if self.sel_path().is_some() {
                        self.delete_selected_float();
                    }
                }
                self.history.collapse_to(depth + 1);
                self.set_float_sel(None);
                Ok(Outcome::Done(self.finish_if_stepped()))
            }
            ShellEdit::ReplaceOle { para, run, data, png, width, height } => {
                let path: Path = vec![Step::Para { para, run }];
                let ok = matches!(self.ctx, super::context::EditContext::Body)
                    && self
                        .run_ref(&path)
                        .and_then(|r| r.ext())
                        .and_then(|e| e.image.as_deref())
                        .and_then(|i| i.ole.as_deref())
                        .is_some_and(|o| o.prog_id.to_ascii_lowercase().starts_with("excel.sheet") && &*o.ext == "xlsx");
                if !ok || self.locked() || !width.is_finite() || !height.is_finite() {
                    return Ok(Outcome::Done(self.finish()));
                }
                self.begin_step(false, &[]);
                let xid = self.doc.media.add(&data);
                let pid = self.doc.media.add(&png);
                if let Some(r) = self.run_mut(&path) {
                    let props = std::sync::Arc::make_mut(&mut r.props);
                    crate::word::obj::delete(props, "src");
                    if let Some(img) = r.ext_mut().image.as_deref_mut() {
                        if let Some(o) = img.ole.as_deref_mut() {
                            crate::word::obj::set(o, "data", |x| x.data = crate::model::media::MediaRef(xid));
                        }
                        crate::word::obj::set(img, "data", |x| x.data = crate::model::media::MediaRef(pid));
                        crate::word::obj::set(img, "ext", |x| x.ext = "png".into());
                        crate::word::obj::set(img, "widthTwips", |x| x.width_twips = JsNumber::Num(width));
                        crate::word::obj::set(img, "heightTwips", |x| x.height_twips = JsNumber::Num(height));
                        crate::word::obj::delete(img, "vml");
                        crate::word::obj::delete(img, "rawDrawingXml");
                    }
                }
                self.touch_para(para as usize);
                Ok(Outcome::Done(self.finish()))
            }
            ShellEdit::MoveInlineRun { para, run, to } => self.shell_move_inline_run(para, run, to),
            ShellEdit::CommitDrag(d) => self.shell_commit_drag(&d),
            ShellEdit::DefineListLevelPicture { format, pic } => {
                self.define_list_level_with(&format, Some(&pic))?;
                Ok(Outcome::Done(self.finish()))
            }
        };
        if out.is_err() {
            self.tx = super::super::Tx::default();
        }
        out
    }
}

impl Session {
    /// `wrapPointsMouseDown`'s commit (universal-write.ts 14280): the picture's polygon, `edited`, closed here.
    fn shell_wrap_polygon(&mut self, pts: &[(f64, f64)]) -> Result<Outcome, CommandError> {
        if pts.len() < 3 {
            return Ok(Outcome::Done(self.finish()));
        }
        let mut closed: Vec<(JsNumber, JsNumber)> = pts.iter().map(|&(x, y)| (JsNumber::Num(x), JsNumber::Num(y))).collect();
        if let Some(&first) = closed.first() {
            closed.push(first);
        }
        let poly = WrapPolygon { edited: true, pts: closed, ..Default::default() };
        self.edit_drawing(true, &mut |_, _| {
            Some(ShapeChange {
                pic: PictureChange { polygon: Some(Nullable::Val(poly.clone())), ..PictureChange::default() },
                ..ShapeChange::default()
            })
        });
        Ok(Outcome::Done(self.finish_if_stepped()))
    }

    /// [`ShellEdit::LiveSet`]: the live form written on the object's model fields in place (no step, no event).
    fn shell_live_set(&mut self, f: &LiveForm) -> Result<Outcome, CommandError> {
        let open = self.f7.live.is_some_and(|d| self.history.undo_len() == d);
        let Some(path) = self.draw_path().filter(|_| open) else { return Ok(Outcome::Done(self.change_set())) };
        if let Some(r) = self.run_mut(&path) {
            live_form_into(r, f);
        }
        Ok(Outcome::Done(self.change_set()))
    }

    /// [`ShellEdit::LiveEnd`]: the commit inside the gesture's step (the writer's own step folded into it, as `oneUndo`
    /// keeps the first snapshot), `uw-change` once.
    fn shell_live_end(&mut self, commit: ShellEdit, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        let Some(depth) = self.f7.live.take().filter(|d| self.history.undo_len() == *d) else {
            // no open gesture: the commit as a plain edit
            return self.exec_shell_edit(commit, cx);
        };
        let quiet = self.quiet;
        self.quiet = true;
        let out = match commit {
            ShellEdit::SetWrapPolygon { pts } => self.shell_wrap_polygon(&pts),
            ShellEdit::CommitDrag(d) => self.shell_commit_drag(&d),
            _ => Ok(Outcome::Done(self.finish())),
        };
        self.quiet = quiet;
        self.history.collapse_to(depth);
        let mut out = out?;
        if let Outcome::Done(cs) = &mut out {
            // the step and the dirty flag came with `LiveBegin`; the commit raises the TS `changed()` once
            cs.events.retain(|e| !matches!(e, super::super::Event::Change | super::super::Event::DirtyChange { .. }));
            cs.events.push(super::super::Event::Change);
            cs.can_undo = self.history.can_undo();
            cs.can_redo = self.history.can_redo();
        }
        Ok(out)
    }
}

/// [`LiveForm`] on a drawing run's model fields (the TS writes on `h.obj`: the picture, the box, the shape or the group).
fn live_form_into(r: &mut crate::model::run::Run, f: &LiveForm) {
    use crate::model::drawing::{AnchorPos, DrawingAnchor, ShapeStyle, WrapBox, Xy};
    use crate::word::obj;
    let round = uo_jsnum::math::round;
    let n = JsNumber::Num;
    // `moveAnchorTo(obj, x, y)` (21389)
    let anchor_to = |a: &mut DrawingAnchor, x: f64, y: f64| {
        if a.simple_pos.is_some() {
            obj::set(a, "simplePos", |a| a.simple_pos = Some(Xy { x: n(round(x)), y: n(round(y)), ..Default::default() }));
            return;
        }
        obj::set(a, "posH", |a| a.pos_h = AnchorPos { relative_from: "page".into(), offset: Some(n(round(x))), ..Default::default() });
        obj::set(a, "posV", |a| a.pos_v = AnchorPos { relative_from: "page".into(), offset: Some(n(round(y))), ..Default::default() });
    };
    let e = r.ext_mut();
    if let Some(img) = e.image.as_deref_mut() {
        if let Some(pts) = &f.wrap {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for &(x, y) in pts {
                x0 = uo_jsnum::math::min(x0, x);
                y0 = uo_jsnum::math::min(y0, y);
                x1 = uo_jsnum::math::max(x1, x);
                y1 = uo_jsnum::math::max(y1, y);
            }
            if let Some(a) = img.anchor.as_deref_mut() {
                obj::set(a, "wrapPoly", |a| a.wrap_poly = Some(pts.iter().map(|&(x, y)| (n(x / 21600.0), n(y / 21600.0))).collect()));
                obj::set(a, "wrapBox", |a| {
                    a.wrap_box = Some(WrapBox {
                        x0: n(x0 / 21600.0),
                        y0: n(y0 / 21600.0),
                        x1: n(x1 / 21600.0),
                        y1: n(y1 / 21600.0),
                        ..Default::default()
                    });
                });
            }
            if let Some(fmt) = img.format.as_deref_mut() {
                let mut closed: Vec<(JsNumber, JsNumber)> = pts.iter().map(|&(x, y)| (n(x), n(y))).collect();
                if let Some(&first) = closed.first() {
                    closed.push(first);
                }
                obj::set(fmt, "polygon", |m| m.polygon = Some(Box::new(WrapPolygon { edited: true, pts: closed, ..Default::default() })));
            }
        }
        if let Some((l, t, rr, b)) = f.crop {
            obj::set(img, "crop", |i| {
                i.crop = Some(crate::model::drawing::Ltrb { l: n(l), t: n(t), r: n(rr), b: n(b), ..Default::default() })
            });
        }
        if let Some(rot) = f.rot {
            let mut st = img.style.as_deref().cloned().unwrap_or_else(|| ShapeStyle { geom: "rect".into(), ..Default::default() });
            obj::set(&mut st, "rot", |s| s.rot = Some(n(rot)));
            obj::set(img, "style", |i| i.style = Some(Box::new(st)));
        }
        if let Some((w, h)) = f.size {
            obj::set(img, "widthTwips", |i| i.width_twips = n(w));
            obj::set(img, "heightTwips", |i| i.height_twips = n(h));
        }
        if let Some((x, y)) = f.offset {
            obj::set(img, "offsetXTwips", |i| i.offset_x_twips = Some(n(x)));
            obj::set(img, "offsetYTwips", |i| i.offset_y_twips = Some(n(y)));
        }
        if let (Some((x, y)), Some(a)) = (f.page_at, img.anchor.as_deref_mut()) {
            anchor_to(a, x, y);
        }
    } else if let Some(tb) = e.textbox.as_deref_mut() {
        if let Some(rot) = f.rot {
            obj::set(tb, "rot", |t| t.rot = Some(n(rot)));
        }
        if let Some((w, h)) = f.size {
            obj::set(tb, "widthTwips", |t| t.width_twips = n(w));
            obj::set(tb, "heightTwips", |t| t.height_twips = n(h));
        }
        if let Some((x, y)) = f.offset {
            obj::set(tb, "offsetXTwips", |t| t.offset_x_twips = Some(n(x)));
            obj::set(tb, "offsetYTwips", |t| t.offset_y_twips = Some(n(y)));
        }
        if let (Some((x, y)), Some(a)) = (f.page_at, tb.anchor.as_deref_mut()) {
            anchor_to(a, x, y);
        }
    } else if let Some(sh) = e.shape.as_deref_mut() {
        if let Some(rot) = f.rot {
            obj::set(&mut sh.style, "rot", |s| s.rot = Some(n(rot)));
        }
        if let Some((w, h)) = f.size {
            obj::set(sh, "widthTwips", |t| t.width_twips = n(w));
            obj::set(sh, "heightTwips", |t| t.height_twips = n(h));
        }
        if let (Some((x, y)), Some(a)) = (f.page_at, sh.anchor.as_deref_mut()) {
            anchor_to(a, x, y);
        }
    } else if let Some(g) = e.group.as_deref_mut() {
        if let Some(rot) = f.rot {
            obj::set(g, "rot", |t| t.rot = Some(n(rot)));
        }
        if let Some((w, h)) = f.size {
            obj::set(g, "widthTwips", |t| t.width_twips = n(w));
            obj::set(g, "heightTwips", |t| t.height_twips = n(h));
        }
        if let (Some((x, y)), Some(a)) = (f.page_at, g.anchor.as_deref_mut()) {
            anchor_to(a, x, y);
        }
    }
}

impl Session {
    /// `moveInlineRun(run, to)` (universal-write.ts 13972).
    fn shell_move_inline_run(&mut self, para: u32, run: u32, to: DocPosition) -> Result<Outcome, CommandError> {
        let pi = para as usize;
        let ri = run as usize;
        let Some(p) = self.paras().and_then(|ps| ps.get(pi)) else { return Ok(Outcome::Done(self.finish())) };
        let Some(r) = p.runs.get(ri) else { return Ok(Outcome::Done(self.finish())) };
        if !r.ext().is_some_and(|e| e.image.is_some()) || self.paras().is_none_or(|ps| to.para as usize >= ps.len()) {
            return Ok(Outcome::Done(self.finish()));
        }
        let rlen = r.text.len();
        let from: usize = p.runs.iter().take(ri).map(|x| x.text.len()).sum();
        let from32 = u32::try_from(from).unwrap_or(u32::MAX);
        // a drop on itself: nothing changes (the caret before the picture)
        if to.para == para && (to.offset as usize == from || to.offset as usize == from + 1) {
            self.set_float_sel(None);
            self.selection = super::super::Selection { anchor: DocPosition::new(para, from32), focus: DocPosition::new(para, from32) };
            return Ok(Outcome::Done(self.finish()));
        }
        if self.locked() {
            return Ok(Outcome::Done(self.finish()));
        }
        // the TS caret followed the pointer during the drag (`this.selection = target`): the step's snapshot holds it
        self.set_float_sel(None);
        self.selection = super::super::Selection { anchor: to, focus: to };
        self.begin_step(false, &[]);
        let Some(ps) = self.paras_mut() else { return Ok(Outcome::Done(self.finish())) };
        let Some(p) = ps.make_mut(pi) else { return Ok(Outcome::Done(self.finish())) };
        let moved = p.runs.remove(ri);
        let mut off = to.offset as usize;
        if to.para == para && off > from {
            off -= rlen;
        }
        let Some(q) = ps.make_mut(to.para as usize) else { return Ok(Outcome::Done(self.finish())) };
        // (insertIndex splits the run and keeps after the bookmark marks and field ends, as Word)
        let at = crate::session::text::insert_index(q, off);
        q.runs.insert(at, moved);
        self.touch_para(pi);
        self.touch_para(to.para as usize);
        // the picture stays selected at its new place (`selectInlineRun`, as Word)
        let path: Path = vec![Step::Para { para: to.para, run: u32::try_from(at).unwrap_or(u32::MAX) }];
        let start: usize =
            self.paras().and_then(|ps| ps.get(to.para as usize)).map_or(0, |q| q.runs.iter().take(at).map(|x| x.text.len()).sum());
        let s32 = u32::try_from(start).unwrap_or(u32::MAX);
        self.set_float_sel(Some(path));
        self.selection = super::super::Selection {
            anchor: DocPosition::new(to.para, s32),
            focus: DocPosition::new(to.para, s32.saturating_add(u32::try_from(rlen).unwrap_or(u32::MAX))),
        };
        self.changed();
        Ok(Outcome::Done(self.finish()))
    }

    /// The end of an object drag: `commitPictureDrag` / `commitDrawingDrag` (the size and the position in the same
    /// references, a relative size made absolute, a group's children scaled from its start size) and the Crop drag's
    /// commit (the source rectangle too), written with the effect extent recomputed (`applyDrawingXml(..., true)`).
    fn shell_commit_drag(&mut self, d: &DragCommit) -> Result<Outcome, CommandError> {
        use crate::model::picture::SizeRel;
        use crate::write::emit::fx::Rect4;
        let Some(path) = self.draw_path() else { return Ok(Outcome::Done(self.finish())) };
        if self.locked() {
            return Ok(Outcome::Done(self.finish()));
        }
        let Some(fmt) = self.run_ref(&path).and_then(super::drawing::format_of) else { return Ok(Outcome::Done(self.finish())) };
        let b = fmt.base();
        let round = uo_jsnum::math::round;
        let mut ch = PictureChange::default();
        if let Some(deg) = d.rot {
            ch.rot = Some(round(deg * 60000.0) % 21_600_000.0);
        }
        if let Some((w, h)) = d.size {
            ch.size = Some((round(w * 635.0), round(h * 635.0)));
            if d.reset_rel {
                let zero = |cur: Option<&SizeRel>| -> Option<Nullable<SizeRel>> {
                    let c = cur?;
                    let pct = c.pct.to_number();
                    if pct == 0.0 || pct.is_nan() {
                        return None;
                    }
                    Some(Nullable::Val(SizeRel { relative_from: c.relative_from.clone(), pct: JsNumber::Num(0.0), ..Default::default() }))
                };
                ch.size_rel_h = zero(b.size_rel_h.as_ref());
                ch.size_rel_v = zero(b.size_rel_v.as_ref());
            }
        }
        if let Some((l, t, r, bt)) = d.src_rect {
            ch.src_rect =
                Some(Rect4 { l: round(l * 100_000.0), t: round(t * 100_000.0), r: round(r * 100_000.0), b: round(bt * 100_000.0) });
        }
        if let Some((x, y, fr)) = &d.at
            && !b.inline
        {
            ch.pos_h = super::pictures::pos_for(b.pos_h.as_ref(), true, *x, fr);
            ch.pos_v = super::pictures::pos_for(b.pos_v.as_ref(), false, *y, fr);
        }
        let sc = ShapeChange { pic: ch, ..ShapeChange::default() };
        let Some(xml) = self.patch_drawing_pub(&path, &sc) else { return Ok(Outcome::Done(self.finish())) };
        self.begin_step(false, &[]);
        if let Some((w, h)) = d.group_start
            && let Some(g) = self.run_mut(&path).and_then(|r| r.ext_mut().group.as_deref_mut())
        {
            crate::word::obj::set(g, "widthTwips", |x| x.width_twips = JsNumber::Num(w));
            crate::word::obj::set(g, "heightTwips", |x| x.height_twips = JsNumber::Num(h));
        }
        self.apply_drawing_xml(&path, xml, true);
        self.after_picture_edit();
        Ok(Outcome::Done(self.finish_if_stepped()))
    }
}
