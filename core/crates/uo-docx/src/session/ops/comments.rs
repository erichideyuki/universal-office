//! D2b family: comments (frontier D2b-F6): `addComment`, `removeComment` of `universal-write.ts`
//! (7869–7898). The comment list is the document's (`fullDoc.comments`); the undo state of the TS
//! does not carry it (`stringifyState` has the runs' `commentIds` only), so neither does the
//! session: an undo takes the marks off the text and leaves the entry in the list, as the TS.
//!
//! `goToComment` is a selection move and stays with the navigation family; `addComment` returns
//! its id through [`Session::comments`] (the last entry).

use std::sync::Arc;

use super::super::command::{CommandError, Event, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::text::{DocRange, normalize_range};
use super::super::{ExecCx, Session};
use crate::js::{JsNumber, Str};
use crate::model::Comment;
use crate::model::run::RunProps;
use crate::text::Utf16;
use crate::word::obj;

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum CommentCmd {
    /// `addComment(text, author)`: comments the selection (no selection, blank text or a locked
    /// editor: nothing happens). `author` `None` = the session's author.
    Add { text: Utf16, author: Option<Str> },
    /// `removeComment(id)`: the entry and the marks of every run of the context's paragraphs
    /// (no lock check, as the TS)
    Remove { id: f64 },
}

/// `String.prototype.trim` over code units (the JS white space set).
fn trim_units(u: &[u16]) -> &[u16] {
    let is_space = uo_jsnum::text::is_js_space_unit;
    let start = u.iter().position(|&c| !is_space(c)).unwrap_or(u.len());
    let end = u.iter().rposition(|&c| !is_space(c)).map_or(start, |i| i + 1);
    u.get(start..end).unwrap_or(&[])
}

impl Session {
    /// The comments of the document (`fullDoc.comments`).
    #[must_use]
    pub fn comments(&self) -> &[Comment] {
        &self.doc.comments
    }

    /// `addComment`.
    fn add_comment(&mut self, text: &Utf16, author: Option<Str>) -> Result<Outcome, CommandError> {
        let trimmed = trim_units(text.units());
        if self.paras().is_none() || !self.has_selection() || trimmed.is_empty() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let author = author.unwrap_or_else(|| self.opts.author.clone());
        self.begin_step(false, &[]);
        // `comments.reduce((m, c) => Math.max(m, c.id), -1) + 1`
        let id = self.doc.comments.iter().fold(-1.0_f64, |m, c| uo_jsnum::math::max(m, c.id.to_number())) + 1.0;
        let range = self.range();
        let mut patch = RunProps::default();
        obj::set(&mut patch, "commentIds", |p| p.comment_ids = Some(vec![JsNumber::Num(id)]));
        let paras = self.paras_mut().ok_or(CommandError::Position)?;
        guarded::apply_run_props(paras, range, &patch)?;
        let DocRange { start, end } = normalize_range(range);
        for p in start.para..=end.para {
            self.touch_para(p as usize);
        }
        let comment =
            Comment { id: JsNumber::Num(id), author: Some(author), text: Utf16::from_units(trimmed.to_vec()), ..Comment::default() };
        let list: Arc<[Comment]> = self.doc.comments.iter().cloned().chain(std::iter::once(comment)).collect();
        self.doc.comments = list;
        let mut cs = self.finish();
        cs.events.push(Event::CommentsChanged);
        Ok(Outcome::Done(cs))
    }

    /// `removeComment(id)`.
    fn remove_comment(&mut self, id: f64) -> Result<Outcome, CommandError> {
        if self.paras().is_none() {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        let kept: Arc<[Comment]> = self.doc.comments.iter().filter(|c| c.id.to_number() != id).cloned().collect();
        self.doc.comments = kept;
        let marked: Vec<usize> = self
            .paras()
            .map(|ps| ps.iter().enumerate().filter(|(_, p)| p.runs.iter().any(|r| r.props.comment_ids.is_some())).map(|(i, _)| i).collect())
            .unwrap_or_default();
        for i in marked {
            let Some(p) = self.paras_mut().and_then(|ps| ps.make_mut(i)) else { continue };
            for run in &mut p.runs {
                let Some(ids) = run.props.comment_ids.as_ref() else { continue };
                let rest: Vec<JsNumber> = ids.iter().copied().filter(|x| x.to_number() != id).collect();
                let props = Arc::make_mut(&mut run.props);
                if rest.is_empty() {
                    obj::delete(props, "commentIds");
                } else {
                    obj::set(props, "commentIds", |p| p.comment_ids = Some(rest));
                }
            }
            self.touch_para(i);
        }
        let mut cs = self.finish();
        cs.events.push(Event::CommentsChanged);
        Ok(Outcome::Done(cs))
    }

    pub(crate) fn exec_comments(&mut self, cmd: CommentCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            CommentCmd::Add { text, author } => self.add_comment(&text, author),
            CommentCmd::Remove { id } => self.remove_comment(id),
        }
    }
}

impl CommentCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads:
    /// the comment methods are not guarded.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        None
    }
}
