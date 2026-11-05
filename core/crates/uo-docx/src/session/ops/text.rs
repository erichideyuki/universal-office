//! Typing, Enter, Backspace, Delete and the selection (D2a): `type`, `pressEnter`,
//! `pressBackspace`, `pressDelete`, `trackedKey`, `deleteSelection`, `trackedDelete`,
//! `setSelection` of `universal-write.ts` (9701–9930, 2978), over the story of the edit context,
//! every model op guarded (`guard::guarded`, the TS wrappers 899–931).
//!
//! The parts that belong to other families are hooks those families own (record "D2b frontier
//! briefs"): `clearSelectedCells` (tables), `listEnterOnEmpty`/`listBackspaceAtStart` (lists),
//! `applyNextStyle` (styles), the extra ranges of a multi-selection (find/selection). The
//! cancelable `uw-before-input` stays in the shell, before the command (U16).

use super::super::command::{CommandError, Event, Outcome};
use super::super::guard::guarded;
use super::super::state::Selection;
use super::super::text::{CarriedProps, DocPosition, DocRange, normalize_range, para_length, props_at, without_revisions};
use super::super::{ExecCx, MarkupMode, Session};
use super::context::EditContext;
use crate::model::para::Paragraph;
use crate::model::run::Run;
use crate::text::Utf16;
use crate::word::obj;

/// The text commands.
#[derive(Debug, Clone, PartialEq)]
pub enum TextCmd {
    /// `type(text)`: typed (or IME-committed) text at the caret, over the selection
    Type { text: Utf16 },
    /// `pressEnter()`
    Enter,
    /// `pressBackspace()`
    Backspace,
    /// `pressDelete()`
    Delete,
    /// `setSelection(anchor, focus)`: clamped to the story
    SetSelection { anchor: DocPosition, focus: DocPosition },
}

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn len_of(paras: Option<&crate::pvec::PVec<Paragraph>>, i: u32) -> u32 {
    paras.and_then(|p| p.get(i as usize)).map_or(0, |p| u32_of(para_length(p)))
}

impl Session {
    /// `hasSelection()`.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection.anchor != self.selection.focus
    }

    /// `range()`.
    #[must_use]
    pub fn range(&self) -> DocRange {
        DocRange { start: self.selection.anchor, end: self.selection.focus }
    }

    /// `hiddenChar(p, offset)`: the character is hidden in this markup view (deleted text in the
    /// final views, inserted text in the original). D2a takes the run rule of `revDisplay` for the
    /// views without an author filter (D2a-T3); with "Show Markup" options that turn insertions and
    /// deletions off or filter by author (D2b-F6), the layout's `RevisionDisplay.run` decides.
    fn hidden_char(&self, p: Option<&Paragraph>, offset: usize) -> bool {
        let Some(p) = p else { return false };
        let filtered = self.opts.markup_options.filters_text();
        if self.opts.markup == MarkupMode::All && !filtered {
            return false;
        }
        let display = filtered.then(|| self.revision_display());
        let mut acc = 0usize;
        for r in &p.runs {
            if offset < acc + r.text.len() {
                return offset >= acc
                    && match &display {
                        Some(d) => d.run(&r.props) == Some(crate::word::revisions::RunRevision::Hide),
                        None => self.run_hidden(r),
                    };
            }
            acc += r.text.len();
        }
        false
    }

    /// `revDisplay()`: the layout's display rules for the markup mode and options.
    pub(crate) fn revision_display(&self) -> crate::word::revisions::RevisionDisplay {
        use crate::word::revisions::{RevisionDisplay, RevisionMode};
        let mode = match self.opts.markup {
            MarkupMode::All => RevisionMode::All,
            MarkupMode::Simple => RevisionMode::Simple,
            MarkupMode::None => RevisionMode::Final,
            MarkupMode::Original => RevisionMode::Original,
        };
        let o = &self.opts.markup_options;
        RevisionDisplay::new(mode, Some(o.ins_del), Some(o.formatting), o.authors.as_deref(), None, None, &[])
    }

    fn run_hidden(&self, r: &Run) -> bool {
        match self.opts.markup {
            MarkupMode::All => false,
            MarkupMode::Simple | MarkupMode::None => r.props.del.is_some(),
            MarkupMode::Original => r.props.ins.is_some(),
        }
    }

    /// `canonPos(pos)`: the first of the visually equal positions.
    pub(crate) fn canon_pos(&self, pos: DocPosition) -> DocPosition {
        let p = self.paras().and_then(|ps| ps.get(pos.para as usize));
        let mut k = pos.offset as usize;
        while k > 0 && self.hidden_char(p, k - 1) {
            k -= 1;
        }
        DocPosition { para: pos.para, offset: u32_of(k) }
    }

    /// `runBefore(pos)`: the run that ends at `pos`.
    fn run_before(&self, pos: DocPosition) -> Option<&Run> {
        let p = self.paras()?.get(pos.para as usize)?;
        if pos.offset == 0 {
            return None;
        }
        let o = pos.offset as usize;
        let mut acc = 0usize;
        for r in &p.runs {
            let end = acc + r.text.len();
            if o > acc && o <= end {
                return Some(r);
            }
            acc = end;
        }
        None
    }

    /// `propsAfterNoteRef()`: text typed right after a note reference (or a note's own mark) does
    /// not take the reference style nor the superscript.
    fn props_after_note_ref(&self) -> Option<CarriedProps> {
        let r = self.run_before(self.selection.focus)?;
        let is_ref = r.ext().is_some_and(|e| e.note_ref.is_some());
        let is_mark = r.ext().and_then(|e| e.field.as_deref()).is_some_and(super::super::settle::is_note_mark_field);
        if !is_ref && !is_mark {
            return None;
        }
        let mut props = (*r.props).clone();
        obj::delete(&mut props, "charStyle");
        obj::delete(&mut props, "vertAlign");
        Some(CarriedProps { props, link: r.rpr_link.clone() })
    }

    /// `deleteSelection()`: the range goes, with the table anchors between its paragraphs.
    pub(crate) fn delete_selection(&mut self) -> Result<DocPosition, CommandError> {
        if self.opts.tracking {
            // `trackedDelete(this.range(), 'end')`: the caret after the deleted text
            return self.tracked_delete(self.range(), false);
        }
        let range = self.range();
        let DocRange { start, end } = normalize_range(range);
        let delta = end.para.saturating_sub(start.para);
        if delta > 0 && matches!(self.ctx, EditContext::Body) {
            let story = self.story_mut().ok_or(CommandError::Position)?;
            // `tableAnchors.filter((a) => a.beforePara <= start.para || a.beforePara > end.para)`
            let mut i = 0;
            while i < story.tables.len() {
                let gone = story.tables.get(i).is_some_and(|a| a.before_para > start.para && a.before_para <= end.para);
                if gone {
                    story.tables.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        let pos = guarded::delete_range(paras, range)?;
        if delta > 0 {
            self.shift_anchors(start.para, -i64::from(delta));
            self.touch_structure();
        }
        self.touch_para(pos.para as usize);
        Ok(pos)
    }

    /// `trackedDelete(range, caret)`: `start` = Backspace (the caret before), else after.
    fn tracked_delete(&mut self, range: DocRange, start: bool) -> Result<DocPosition, CommandError> {
        let rev = self.new_rev();
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        let r = guarded::track_delete(paras, range, &rev)?;
        for removed in &r.merged {
            self.shift_anchors(removed.saturating_sub(1), -1);
            self.touch_structure();
        }
        let at = if start { r.start } else { r.end };
        self.touch_para(at.para as usize);
        Ok(self.canon_pos(at))
    }

    /// `insertTyped(pos, text, props)`.
    pub(crate) fn insert_typed(&mut self, pos: DocPosition, text: &[u16], props: Option<&CarriedProps>) -> Result<DocPosition, CommandError> {
        let out = if self.opts.tracking {
            let rev = self.new_rev();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::track_insert_text(paras, pos, text, &rev, props)?
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::insert_text(paras, pos, text, props)?
        };
        self.touch_para(pos.para as usize);
        Ok(out)
    }

    /// `splitAt(pos)` (Enter).
    pub(crate) fn split_typed(&mut self, pos: DocPosition) -> Result<DocPosition, CommandError> {
        let rev = self.opts.tracking.then(|| self.new_rev());
        let ctx = self.ctx.clone();
        let doc = &mut self.doc;
        let mut ids = std::mem::take(&mut doc.ids);
        let out = match super::context::story_of_mut(doc, &ctx) {
            Some(story) => match &rev {
                Some(rev) => guarded::track_split_paragraph(&mut story.paras, &mut ids, pos, rev),
                None => guarded::split_paragraph(&mut story.paras, &mut ids, pos),
            },
            None => Err(super::super::text::OpError::Position),
        };
        self.doc.ids = ids;
        let out = out?;
        self.touch_para(pos.para as usize);
        self.touch_structure();
        Ok(out)
    }

    fn caret_at(&mut self, pos: DocPosition) {
        self.selection = Selection::caret(pos);
    }

    /// `type(text)`.
    fn type_text(&mut self, text: &[u16]) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        if self.table_sel.is_some() {
            // selected cells: Word clears their content and types in the first
            self.clear_selected_cells()?;
            self.table_sel = None;
        }
        let mut replaced: Option<CarriedProps> = None;
        if self.has_selection() {
            let st = normalize_range(self.range()).start;
            if let Some(paras) = self.paras() {
                replaced = Some(props_at(paras, DocPosition { para: st.para, offset: st.offset.saturating_add(1) }));
            }
            let pos = self.delete_selection()?;
            self.caret_at(pos);
        }
        let props = self.pending_props.clone().or_else(|| self.props_after_note_ref()).or(replaced);
        let pos = self.insert_typed(self.selection.focus, text, props.as_ref())?;
        self.pending_props = None;
        self.caret_at(pos);
        let mut cs = self.finish();
        cs.events.push(Event::Input { text: Utf16::from_units(text.to_vec()) });
        Ok(Outcome::Done(cs))
    }

    /// `pressEnter()`.
    fn press_enter(&mut self) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        if self.list_enter_on_empty()? {
            return Ok(Outcome::Done(self.finish()));
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            let pos = self.delete_selection()?;
            self.caret_at(pos);
        }
        let at = self.selection.focus.para;
        let at_end = self.selection.focus.offset >= len_of(self.paras(), at);
        let pos = self.split_typed(self.selection.focus)?;
        self.shift_anchors(at, 1);
        if at_end {
            self.apply_next_style(at as usize)?;
        }
        self.caret_at(pos);
        Ok(Outcome::Done(self.finish()))
    }

    /// `pressBackspace()`.
    fn press_backspace(&mut self) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        // right after a note reference the first Backspace only SELECTS it
        if !self.has_selection()
            && let Some(len) =
                self.run_before(self.selection.focus).filter(|r| r.ext().is_some_and(|e| e.note_ref.is_some())).map(|r| r.text.len())
        {
            let DocPosition { para, offset } = self.selection.focus;
            self.selection =
                Selection { anchor: DocPosition { para, offset: offset.saturating_sub(u32_of(len)) }, focus: DocPosition { para, offset } };
            return Ok(Outcome::Done(self.change_set()));
        }
        if self.list_backspace_at_start()? {
            return Ok(Outcome::Done(self.finish()));
        }
        if self.opts.tracking && !self.has_selection() {
            return self.tracked_key(false);
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            let pos = self.delete_selected_ranges()?; // `deleteSelectedRanges` (D2b-F9)
            self.caret_at(pos);
        } else {
            let DocPosition { para, offset } = self.selection.focus;
            let in_body = matches!(self.ctx, EditContext::Body);
            let prev_break =
                para > 0 && self.paras().and_then(|p| p.get(para as usize - 1)).is_some_and(|p| p.props.section_break.is_some());
            if offset > 0 {
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                let pos = guarded::delete_range(paras, DocRange::new(DocPosition::new(para, offset - 1), DocPosition::new(para, offset)))?;
                self.touch_para(para as usize);
                self.caret_at(pos);
            } else if prev_break && in_body {
                // right after a section break Word does NOT delete it: the character before it
                // goes (the end of the break's paragraph or, empty, the previous mark)
                let len = len_of(self.paras(), para - 1);
                let pos = if len > 0 {
                    let paras = self.paras_mut().ok_or(CommandError::Position)?;
                    let pos =
                        guarded::delete_range(paras, DocRange::new(DocPosition::new(para - 1, len - 1), DocPosition::new(para - 1, len)))?;
                    self.touch_para(para as usize - 1);
                    pos
                } else if para > 1 {
                    let paras = self.paras_mut().ok_or(CommandError::Position)?;
                    let pos = guarded::merge_with_previous(paras, para - 1)?;
                    self.shift_anchors(para - 2, -1);
                    self.touch_structure();
                    pos
                } else {
                    DocPosition::new(para - 1, 0)
                };
                self.caret_at(pos);
            } else if para > 0 {
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                let pos = guarded::merge_with_previous(paras, para)?;
                self.shift_anchors(para - 1, -1);
                self.touch_structure();
                self.caret_at(pos);
            } else {
                self.discard_step();
                return Ok(Outcome::Done(self.change_set()));
            }
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `pressDelete()`.
    fn press_delete(&mut self) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        if self.opts.tracking && !self.has_selection() {
            return self.tracked_key(true);
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            let pos = self.delete_selected_ranges()?; // `deleteSelectedRanges` (D2b-F9)
            self.caret_at(pos);
        } else {
            let DocPosition { para, offset } = self.selection.focus;
            let len = len_of(self.paras(), para);
            let n = self.paras().map_or(0, crate::pvec::PVec::len);
            if offset < len {
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                guarded::delete_range(paras, DocRange::new(DocPosition::new(para, offset), DocPosition::new(para, offset + 1)))?;
                self.touch_para(para as usize);
            } else if (para as usize) + 1 < n {
                let paras = self.paras_mut().ok_or(CommandError::Position)?;
                guarded::merge_with_previous(paras, para + 1)?;
                // (the TS does not shift the anchors here: `pressDelete` 9860)
                self.touch_structure();
            } else {
                self.discard_step();
                return Ok(Outcome::Done(self.change_set()));
            }
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `trackedKey(dir)`: Backspace (`forward` false) / Delete without a selection, tracking on.
    fn tracked_key(&mut self, forward: bool) -> Result<Outcome, CommandError> {
        let para = self.selection.focus.para;
        let mut k = self.selection.focus.offset as usize;
        let Some(p) = self.paras().and_then(|ps| ps.get(para as usize)) else { return Err(CommandError::Position) };
        let len = para_length(p);
        let deleted = |i: usize| -> bool {
            let mut acc = 0usize;
            for r in &p.runs {
                if i < acc + r.text.len() {
                    return i >= acc && r.props.del.is_some();
                }
                acc += r.text.len();
            }
            false
        };
        let n = self.paras().map_or(0, crate::pvec::PVec::len);
        // `!this.cellEdit && !this.hfEdit && !this.tbEdit` (a note does not count here)
        let break_rule = !matches!(self.ctx, EditContext::Cell { .. } | EditContext::HeaderFooter { .. } | EditContext::Textbox { .. });
        let mut go: Option<DocPosition> = None;
        let mut range: Option<DocRange> = None;
        if forward {
            if k < len && deleted(k) && !self.hidden_char(Some(p), k) {
                go = Some(DocPosition::new(para, u32_of(k + 1)));
            } else {
                while k < len && self.hidden_char(Some(p), k) {
                    k += 1;
                }
            }
        } else if k > 0 && deleted(k - 1) && !self.hidden_char(Some(p), k - 1) {
            go = Some(DocPosition::new(para, u32_of(k - 1)));
        } else {
            while k > 0 && self.hidden_char(Some(p), k - 1) {
                k -= 1;
            }
        }
        if go.is_none() {
            let ku = u32_of(k);
            if !forward && k > 0 {
                range = Some(DocRange::new(DocPosition::new(para, ku - 1), DocPosition::new(para, ku)));
            } else if forward && k < len {
                range = Some(DocRange::new(DocPosition::new(para, ku), DocPosition::new(para, ku + 1)));
            } else if !forward && para > 0 {
                let paras = self.paras();
                let prev = paras.and_then(|ps| ps.get(para as usize - 1));
                let pl = len_of(paras, para - 1);
                if prev.is_some_and(|x| x.props.section_break.is_some()) && break_rule {
                    if pl > 0 {
                        range = Some(DocRange::new(DocPosition::new(para - 1, pl - 1), DocPosition::new(para - 1, pl)));
                    } else if para > 1 && !paras.and_then(|ps| ps.get(para as usize - 2)).is_some_and(|x| x.props.mark_del.is_some()) {
                        range = Some(DocRange::new(DocPosition::new(para - 2, len_of(paras, para - 2)), DocPosition::new(para - 1, 0)));
                    } else {
                        return Ok(Outcome::Done(self.change_set()));
                    }
                } else if prev.is_some_and(|x| x.props.mark_del.is_some()) {
                    go = Some(DocPosition::new(para - 1, pl));
                } else {
                    range = Some(DocRange::new(DocPosition::new(para - 1, pl), DocPosition::new(para, 0)));
                }
            } else if forward && (para as usize) + 1 < n {
                if p.props.mark_del.is_some() {
                    go = Some(DocPosition::new(para + 1, 0));
                } else {
                    range = Some(DocRange::new(DocPosition::new(para, u32_of(len)), DocPosition::new(para + 1, 0)));
                }
            } else {
                return Ok(Outcome::Done(self.change_set()));
            }
        }
        if let Some(pos) = go {
            let c = self.canon_pos(pos);
            self.caret_at(c);
            return Ok(Outcome::Done(self.change_set()));
        }
        let Some(range) = range else { return Ok(Outcome::Done(self.change_set())) };
        self.begin_step(false, &[]);
        let pos = self.tracked_delete(range, !forward)?;
        self.caret_at(pos);
        Ok(Outcome::Done(self.finish()))
    }

    /// `setSelection(anchor, focus)`: clamped (`Math.floor` is the identity on integers).
    fn set_selection(&mut self, anchor: DocPosition, focus: DocPosition) -> Outcome {
        let paras = self.paras();
        self.selection = super::context::clamp_selection(paras, Selection { anchor, focus });
        self.pending_props = None;
        self.float_sel = None;
        // a new selection object: the extra ranges of a multi-selection go (D2b-F9)
        self.f9.drop_extras();
        Outcome::Done(self.change_set())
    }

    pub(crate) fn exec_text(&mut self, cmd: TextCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            TextCmd::Type { text } => self.type_text(text.units()),
            TextCmd::Enter => self.press_enter(),
            TextCmd::Backspace => self.press_backspace(),
            TextCmd::Delete => self.press_delete(),
            TextCmd::SetSelection { anchor, focus } => Ok(self.set_selection(anchor, focus)),
        }
    }
}

/// The props at the caret without the revision marks (`propsAt` for the toolbar).
#[must_use]
pub fn caret_props(s: &Session) -> Option<CarriedProps> {
    let paras = s.paras()?;
    let p = props_at(paras, s.selection().focus);
    Some(CarriedProps { props: without_revisions(&p.props), link: p.link })
}
