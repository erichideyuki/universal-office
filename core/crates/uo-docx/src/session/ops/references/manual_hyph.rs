//! Manual Hyphenation (universal-write.ts 17318–17440): the next word the automatic hyphenation
//! would break at the end of a line, in text order, within the selection (without one, the whole
//! document); Yes inserts an optional hyphen (`w:softHyphen`), No skips the word.
//!
//! The forced-hyphenation layout is the oracle's (`next_hyphenation`): it answers the FIRST body
//! candidate at or after a position, so when the word is too short (the TS `continue`s) the
//! session asks again from the next offset (F10-D4). The patterns come from the registry the
//! host gave ([`Session::set_hyphenation_registry`], F10-D3); without one the points are only the
//! optional hyphens the word already has (the TS `hyphenatorFor` answering null).

use std::collections::HashSet;

use super::super::super::command::{CommandError, Outcome};
use super::super::super::settle::{block_of_para, sync_paras};
use super::super::super::state::Selection;
use super::super::super::text::{CarriedProps, DocPosition, DocRange, compare_pos, normalize_range, para_text, without_revisions};
use super::super::super::{ExecCx, Session};
use super::super::context::EditContext;
use super::F10Result;
use super::fields::para_of_block;
use super::hyphen::{ManualHyphenation, manual_points, offset_after_letters, word_around};
use crate::ids::ParaId;
use crate::text::Utf16;
use crate::view::{BlockPath, DocRef, HyphenSkips, StoryRef};

/// The Manual Hyphenation in course (`manualHyph`).
#[derive(Debug, Clone)]
pub(crate) struct ManualHyph {
    pub from: DocPosition,
    pub to: Option<DocPosition>,
    pub skip: HashSet<(ParaId, u32)>,
    pub cur: Option<Cur>,
}

/// The word proposed (`manualHyph.cur`).
#[derive(Debug, Clone)]
pub(crate) struct Cur {
    pub para: u32,
    pub start: usize,
    pub end: usize,
    pub raw: Vec<u16>,
    pub points: Vec<u32>,
    pub limit: u32,
}

/// `manualHyph` as the host sees it ([`Session::manual_hyphenation_state`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualHyphState {
    pub from: DocPosition,
    /// `to` of a search inside a selection
    pub to: Option<DocPosition>,
    pub cur: Option<ManualHyphWord>,
    /// how many words were refused (`skip`)
    pub skipped: usize,
}

/// `manualHyph.cur` without the raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualHyphWord {
    pub para: u32,
    pub start: u32,
    pub end: u32,
    pub points: Vec<u32>,
    pub limit: u32,
}

/// The most layouts one `nextManualHyphenation` asks for (the words it rejects as too short each
/// cost one forced-hyphenation layout; F10-D4/D24: a hostile document is bounded by this, not by its length; at the
/// bound the search ends with no word, as at the end of the document).
const MAX_ASKS: usize = 1_000;

impl Session {
    /// `langAt(p, offset)`: the language (`w:lang`) of the text at a point (run, document
    /// default, `pt-BR`).
    fn lang_at(&self, para: u32, offset: usize) -> String {
        let mut acc = 0usize;
        if let Some(p) = self.doc.body.paras.get(para as usize) {
            for r in &p.runs {
                if offset < acc + r.text.len() {
                    if let Some(v) = r.rpr.as_ref().and_then(|x| x.lang.as_ref()).and_then(|l| l.val.as_ref()).filter(|v| !v.is_empty()) {
                        return v.to_string();
                    }
                    break;
                }
                acc += r.text.len();
            }
        }
        self.doc
            .formatting
            .styles
            .doc_defaults
            .r_pr
            .as_ref()
            .and_then(|r| r.lang.as_ref())
            .and_then(|l| l.val.as_ref())
            // `??`: an empty `w:val` of the defaults is a value (the run level above is truthy-tested)
            .map_or_else(|| "pt-BR".to_owned(), ToString::to_string)
    }

    /// `nextManualHyphenation()`.
    pub(crate) fn next_manual_hyphenation(&mut self, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        self.f10.result = Some(F10Result::Hyphenation(None));
        if self.paras().is_none() || self.locked() || !matches!(self.ctx, EditContext::Body) {
            return self.done_nothing();
        }
        if self.f10.manual.is_none() {
            let r = normalize_range(self.range());
            self.f10.manual = Some(if self.has_selection() {
                ManualHyph { from: r.start, to: Some(r.end), skip: HashSet::new(), cur: None }
            } else {
                ManualHyph { from: DocPosition::new(0, 0), to: None, skip: HashSet::new(), cur: None }
            });
        }
        let (from, to, skip) = match self.f10.manual.as_mut() {
            Some(mh) => {
                mh.cur = None;
                (mh.from, mh.to, HyphenSkips(mh.skip.clone()))
            }
            None => return self.done_nothing(),
        };
        sync_paras(&mut self.doc.body.paras);
        let mut ask = from;
        for _ in 0..MAX_ASKS {
            let Ok(block) = u32::try_from(block_of_para(&self.doc.body, ask.para as usize)) else { break };
            let at_ref = DocRef { story: StoryRef::Body, path: BlockPath::root(block), offset: ask.offset };
            let cand = cx.layout.next_hyphenation(self.view(), &at_ref, &skip).map_err(CommandError::Layout)?;
            let Some(c) = cand else { break };
            // a body paragraph at the top level (the oracle answers the body's lines)
            if c.at.story != StoryRef::Body || c.at.path.steps().len() != 1 {
                break;
            }
            let Some(pi) = para_of_block(&self.doc.body, c.at.path.top() as usize) else { break };
            let at = DocPosition::new(u32::try_from(pi).unwrap_or(u32::MAX), c.at.offset);
            if compare_pos(at, from).is_lt() {
                // (never: the oracle answers at or after `from`)
                ask = DocPosition::new(at.para, at.offset.saturating_add(1));
                continue;
            }
            if to.is_some_and(|t| compare_pos(at, t).is_gt()) {
                break;
            }
            let Some(para) = self.doc.body.paras.get(pi) else { break };
            let text = para_text(para);
            let (a, b) = word_around(&text, c.at.offset as usize);
            if b - a < 2 {
                ask = DocPosition::new(at.para, at.offset.saturating_add(1));
                continue;
            }
            let raw: Vec<u16> = text.get(a..b).unwrap_or(&[]).to_vec();
            let lang = self.lang_at(at.para, a);
            let letters_u: Vec<u16> = raw.iter().copied().filter(|&u| u != 0x00AD).collect();
            let letters = String::from_utf16_lossy(&letters_u);
            let (l0, r0) = (2u32, if self.doc.word_compat_mode() >= 15.0 { 3u32 } else { 2u32 });
            let patterns: Vec<u32> = self
                .f10
                .hyphenation
                .as_ref()
                .and_then(|r| r.for_lang(Some(&lang)))
                .map(|h| h.hyphenate(&letters, l0, r0))
                .unwrap_or_default();
            let (word, mut points) = manual_points(&raw, &patterns);
            let upto = (c.at.offset as usize).min(text.len());
            let suggested =
                u32::try_from(text.get(a.min(upto)..upto).unwrap_or(&[]).iter().filter(|&&u| u != 0x00AD).count()).unwrap_or(u32::MAX);
            if !points.contains(&suggested) {
                points.push(suggested);
                points.sort_unstable();
            }
            if let Some(mh) = self.f10.manual.as_mut() {
                mh.cur = Some(Cur { para: at.para, start: a, end: b, raw, points: points.clone(), limit: suggested });
            }
            self.selection = Selection {
                anchor: DocPosition::new(at.para, u32::try_from(a).unwrap_or(u32::MAX)),
                focus: DocPosition::new(at.para, u32::try_from(b).unwrap_or(u32::MAX)),
            };
            self.f10.result = Some(F10Result::Hyphenation(Some(ManualHyphenation {
                word: Utf16::from_units(word),
                lang,
                points,
                suggested,
                limit: suggested,
            })));
            return self.done_nothing();
        }
        self.f10.manual = None;
        self.done_nothing()
    }

    /// `applyManualHyphenation(letters)`: Yes: an optional hyphen after `letters` letters of the
    /// proposed word (one of the points, up to the line limit). One undo step.
    pub(crate) fn apply_manual_hyphenation(&mut self, letters: f64) -> Result<Outcome, CommandError> {
        let Some(c) = self.f10.manual.as_ref().and_then(|m| m.cur.clone()) else { return self.done_nothing() };
        if self.paras().is_none() || self.locked() || !c.points.iter().any(|&p| f64::from(p) == letters) || letters > f64::from(c.limit) {
            return self.done_nothing();
        }
        let off = offset_after_letters(&c.raw, c.start, letters);
        let text = self.doc.body.paras.get(c.para as usize).map(para_text).unwrap_or_default();
        let mut grow = 0usize;
        // the point is already an optional hyphen of the word: nothing to insert
        let before = off.checked_sub(1).and_then(|i| text.get(i)).copied();
        if before != Some(0x00AD) && text.get(off).copied() != Some(0x00AD) {
            self.begin_step(false, &[]);
            let props = self.run_props_at(c.para, c.start.max(off.saturating_sub(1)));
            match self.insert_typed(DocPosition::new(c.para, u32::try_from(off).unwrap_or(u32::MAX)), &[0x00AD], props.as_ref()) {
                Ok(pos) => self.selection = Selection::caret(pos),
                Err(e) => {
                    self.discard_step();
                    return Err(e);
                }
            }
            grow = 1;
        }
        if let Some(mh) = self.f10.manual.as_mut() {
            mh.from = DocPosition::new(c.para, u32::try_from(c.end + grow).unwrap_or(u32::MAX));
            mh.cur = None;
        }
        if grow == 1 { self.done_step() } else { self.done_nothing() }
    }

    /// `runPropsAt(para, offset)`: the props of the run at a point (the inserted character
    /// inherits them).
    fn run_props_at(&self, para: u32, offset: usize) -> Option<CarriedProps> {
        let p = self.paras()?.get(para as usize)?;
        let mut acc = 0usize;
        for r in &p.runs {
            if offset < acc + r.text.len() {
                return Some(CarriedProps { props: without_revisions(&r.props), link: r.rpr_link.clone() });
            }
            acc += r.text.len();
        }
        None
    }

    /// `skipManualHyphenation()`: No: the word stays without a hyphen and the search goes on after it.
    pub(crate) fn skip_manual_hyphenation(&mut self) -> Result<Outcome, CommandError> {
        let Some(c) = self.f10.manual.as_ref().and_then(|m| m.cur.clone()) else { return self.done_nothing() };
        let Some(id) = self.doc.body.paras.get(c.para as usize).map(|p| p.id) else { return self.done_nothing() };
        if let Some(mh) = self.f10.manual.as_mut() {
            mh.skip.insert((id, u32::try_from(c.start).unwrap_or(u32::MAX)));
            mh.from = DocPosition::new(c.para, u32::try_from(c.end).unwrap_or(u32::MAX));
            mh.cur = None;
        }
        self.done_nothing()
    }

    /// `endManualHyphenation()`: Cancel, or the end.
    pub(crate) fn end_manual_hyphenation(&mut self) -> Result<Outcome, CommandError> {
        self.f10.manual = None;
        self.done_nothing()
    }

    /// The whole state of the Manual Hyphenation in course (`manualHyph`): where it searches from
    /// and up to, the word proposed, the refusals (the parity harness and the shell).
    #[must_use]
    pub fn manual_hyphenation_state(&self) -> Option<ManualHyphState> {
        self.f10.manual.as_ref().map(|m| ManualHyphState {
            from: m.from,
            to: m.to,
            cur: m.cur.as_ref().map(|c| ManualHyphWord {
                para: c.para,
                start: u32::try_from(c.start).unwrap_or(u32::MAX),
                end: u32::try_from(c.end).unwrap_or(u32::MAX),
                points: c.points.clone(),
                limit: c.limit,
            }),
            skipped: m.skip.len(),
        })
    }

    /// The Manual Hyphenation in course: where it searches from and up to (the tests and the shell).
    #[must_use]
    pub fn manual_hyphenation_from(&self) -> Option<DocRange> {
        self.f10.manual.as_ref().map(|m| DocRange::new(m.from, m.to.unwrap_or(m.from)))
    }
}
