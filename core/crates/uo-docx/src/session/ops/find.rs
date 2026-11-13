//! D2b family: Find and Replace (D2b-F9, Opus matcher; record `core/docs/modules/uo-docx-d2b/F9.md`):
//! `findNextMatch`, `replaceMatch`, `replaceAllMatches`, `selectAllMatches`, `countMatches`,
//! `highlightMatches`, `findQueryError`, `goTo`, `goToBookmark`, `goToComment` of
//! `universal-write.ts` over [`engine`] (`find-engine.ts`) and [`regex`] (its `RegExp` as a bounded
//! hand matcher). A command searches with one step [`regex::Budget`]; when it runs out, the answer
//! says so (`budget`) and nothing changes (the TS would keep searching). The scroll and the
//! highlight are the shell's.

pub mod engine;
#[rustfmt::skip]
mod fold;
pub mod regex;

use super::super::command::{CommandError, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::state::{CellHop, Selection};
use super::super::text::{
    CarriedProps, DocPosition, DocRange, OpError, compare_pos, normalize_range, para_length, props_at, without_revisions,
};
use super::super::{ExecCx, Session};
use super::context::EditContext;
use super::navigation::F9Result;
use super::revision_cmds::{CellStep, StoryLoc};
use crate::js::JsNumber;
use crate::model::formatting::StyleSheet;
use crate::model::para::{ParaProps, Paragraph};
use crate::model::run::{Run, RunProps};
use crate::model::story::{Block, Story};
use crate::pvec::PVec;
use crate::word::obj;
use engine::{
    FindFailure, FindFormat, FindHit, FindQuery, QueryError, StoryText, adapt_case, expand_replacement, find_all, found_text, pos_of,
    story_text,
};
use regex::{Budget, Vm};

/// The spell checker's language resolver (`LangResolver` of editor `spell.ts`, the `lang` half) and
/// `findEnv()`'s paragraph style: the environment of a search (`FindEnv`).
pub struct SpellLangs<'a> {
    pub sheet: Option<&'a StyleSheet>,
    /// `spellDefaultLang()` (`spellcheck-default-lang`; `DEFAULT_SPELL_LANG` = `pt-BR`)
    pub fallback: &'a str,
}

impl SpellLangs<'_> {
    /// `style(id)`: the language of a style and its `basedOn` chain (a style already seen on the
    /// chain counts as none). The TS memoises per resolver; with a cyclic chain the memo makes the
    /// answer depend on the first entry point (declared F9-D5); here every call walks the chain.
    fn style_lang(&self, id: Option<&str>) -> Option<String> {
        let sheet = self.sheet?;
        let mut seen: Vec<&str> = Vec::new();
        let mut id = id.filter(|s| !s.is_empty());
        while let Some(i) = id {
            if seen.contains(&i) {
                return None;
            }
            let def = sheet.styles.get(i)?;
            seen.push(i);
            if let Some(v) = def.r_pr.as_deref().and_then(|r| r.lang.as_deref()).and_then(|l| l.val.as_deref()) {
                return Some(v.to_owned());
            }
            id = def.based_on.as_deref().filter(|s| !s.is_empty());
        }
        None
    }
}

impl engine::FindEnv for SpellLangs<'_> {
    /// `langs.run(r, p).lang`.
    fn lang(&self, run: &Run, para: &Paragraph) -> Option<String> {
        let rpr = run.rpr.as_deref();
        if let Some(v) = rpr.and_then(|r| r.lang.as_deref()).and_then(|l| l.val.as_deref()) {
            return Some(v.to_owned());
        }
        if let Some(v) = self.style_lang(rpr.and_then(|r| r.r_style.as_deref())) {
            return Some(v);
        }
        let pid = para
            .props
            .style_id
            .as_deref()
            .or_else(|| para.ppr.as_deref().and_then(|p| p.p_style.as_deref()))
            .or_else(|| self.sheet.and_then(|s| s.defaults.paragraph.as_deref()));
        if let Some(v) = self.style_lang(pid) {
            return Some(v);
        }
        if let Some(v) =
            self.sheet.and_then(|s| s.doc_defaults.r_pr.as_deref()).and_then(|r| r.lang.as_deref()).and_then(|l| l.val.as_deref())
        {
            return Some(v.to_owned());
        }
        Some(self.fallback.to_owned())
    }

    /// `p.props.styleId ?? p.ppr?.pStyle ?? 'Normal'`.
    fn para_style(&self, p: &Paragraph) -> Option<String> {
        Some(p.props.style_id.as_deref().or_else(|| p.ppr.as_deref().and_then(|x| x.p_style.as_deref())).unwrap_or("Normal").to_owned())
    }
}

/// `FindRunOptions.direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    Down,
    Up,
    /// everywhere, wrapping (`'all'`, the default)
    #[default]
    All,
}

/// `FindRunOptions.within`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Within {
    #[default]
    Document,
    /// `'selection'`: the story of the cursor, inside the selection
    Selection,
}

/// `FindOutcome`: found, wrapped around, or the query's error; `budget` = the search ran out of its
/// step budget (the TS would keep searching).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FindOutcome {
    pub found: bool,
    pub wrapped: bool,
    pub error: Option<QueryError>,
    pub budget: bool,
}

/// `replaceMatch`'s and `replaceAllMatches`' answer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReplaceOutcome {
    pub find: FindOutcome,
    pub replaced: bool,
    /// `replaceAllMatches`: how many were replaced
    pub count: u32,
}

/// What `goTo` goes to (`GoToWhat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoToWhat {
    Page,
    Section,
    Line,
    Bookmark,
    Comment,
    Footnote,
    Endnote,
    Field,
    Table,
    Graphic,
    Equation,
    Object,
    Heading,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum FindCmd {
    /// `findNextMatch(q, { direction })`
    FindNext { query: FindQuery, direction: Direction },
    /// `replaceMatch(q, replacement, { direction, replaceFormat })`
    ReplaceMatch { query: FindQuery, replacement: Vec<u16>, direction: Direction, replace_format: Option<Box<FindFormat>> },
    /// `replaceAllMatches(q, replacement, { within, replaceFormat })`
    ReplaceAll { query: FindQuery, replacement: Vec<u16>, within: Within, replace_format: Option<Box<FindFormat>> },
    /// `selectAllMatches(q, { within })`
    SelectAllMatches { query: FindQuery, within: Within },
    /// `highlightMatches(q, { within })`: clears the Navigation search; the spans are
    /// [`Session::highlight_spans`] (the paint is the shell's)
    HighlightMatches { query: FindQuery, within: Within },
    /// `goTo(what, target)`; page and line are the layout's (the shell answers them)
    GoTo { what: GoToWhat, target: Vec<u16> },
    /// `goToBookmark(name)`
    GoToBookmark { name: String },
    /// `goToComment(id)`
    GoToComment { id: f64 },
    /// `replaceAll(needle, replacement)` (universal-write.ts 4764; wave 7, A1, native only): the plain, case-insensitive
    /// replacement over the edit story's paragraphs; the count is `F9Result::Count`
    ReplaceAllPlain { needle: Vec<u16>, replacement: Vec<u16> },
}

/// A story of the search (`FindStory`): a stretch of body paragraphs between tables, or a cell.
#[derive(Debug, Clone, PartialEq)]
enum StoryRef {
    Body { first: usize, count: usize },
    Cell { anchor: u32, hops: Vec<CellHop> },
}

/// Where a hit goes in the model (`selectFindHit`'s positions).
#[derive(Debug, Clone, PartialEq)]
struct Target {
    story: StoryRef,
    a: DocPosition,
    b: DocPosition,
}

/// A hit to replace.
struct Replace {
    a: DocPosition,
    b: DocPosition,
    found: Vec<u16>,
    groups: Option<Vec<Vec<u16>>>,
}

type Found<T> = Result<T, FindFailure>;

/// The hits of one story, as selection targets and replacements.
type StoryHits = (StoryRef, Vec<(Target, Replace)>);

fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `SameValueZero` (the keys of a JS `Set`/`Map`).
fn same_value_zero(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// The searcher of one command: the compiled query, the budget, the matcher's scratch state.
struct Search<'q> {
    q: &'q FindQuery,
    compiled: Option<engine::Compiled>,
    budget: Budget,
    vm: Vm,
}

impl Session {
    /// `findStories()`: the body stretches between tables and the cells (nested too), in document
    /// order; a ¶ searched for only crosses paragraphs of one story.
    fn find_stories(&self) -> Vec<(StoryRef, &PVec<Paragraph>, usize, usize)> {
        let body = &self.doc.body;
        let ps = &body.paras;
        let mut out = Vec::new();
        let mut seg = 0usize;
        for i in 0..=ps.len() {
            let here: Vec<(usize, &crate::model::story::TableAnchor)> =
                body.tables.iter().enumerate().filter(|(_, a)| a.before_para as usize == i).collect();
            if !here.is_empty() || i == ps.len() {
                if i > seg {
                    out.push((StoryRef::Body { first: seg, count: i - seg }, ps, seg, i - seg));
                }
                seg = i;
                for (ai, a) in here {
                    let mut cells = Vec::new();
                    super::navigation::table_cells(&a.table, &[], &mut cells);
                    for (hops, story) in cells {
                        if !story.paras.is_empty() {
                            out.push((StoryRef::Cell { anchor: u32_of(ai), hops }, &story.paras, 0, story.paras.len()));
                        }
                    }
                }
            }
        }
        out
    }

    fn spell_langs(&self) -> SpellLangs<'_> {
        SpellLangs { sheet: Some(&self.doc.formatting.styles), fallback: self.f9.spell_default_lang.as_deref().unwrap_or("pt-BR") }
    }

    /// `findCursor(stories, texts)`: the story and text indices of the selection.
    fn find_cursor(&self, stories: &[(StoryRef, &PVec<Paragraph>, usize, usize)], texts: &[StoryText<'_>]) -> (usize, u32, u32) {
        let DocRange { start, end } = normalize_range(self.range());
        let at = |si: usize, para: u32, s: DocPosition, e: DocPosition| {
            let Some(st) = texts.get(si) else { return (si, 0, 0) };
            let g = |p: DocPosition| {
                let base = p.para.checked_sub(para).and_then(|k| st.starts.get(k as usize)).copied().unwrap_or(u32_of(st.text.len()));
                base.wrapping_add(p.offset)
            };
            (si, g(s), g(e))
        };
        match &self.ctx {
            EditContext::Cell { anchor, hops, .. } => {
                if let Some(si) = stories.iter().position(|s| matches!(&s.0, StoryRef::Cell { anchor: a, hops: h } if a == anchor && h[..] == hops[..])) {
                    return at(si, 0, start, end);
                }
            }
            EditContext::Body => {
                if let Some(si) =
                    stories.iter().position(|s| matches!(s.0, StoryRef::Body { first, count } if (start.para as usize) >= first && (start.para as usize) < first + count))
                    && let Some((StoryRef::Body { first, count }, ps, ..)) = stories.get(si)
                {
                    let last = first + count - 1;
                    let e2 = if (end.para as usize) < first + count { end } else { DocPosition::new(u32_of(last), u32_of(ps.get(last).map_or(0, para_length))) };
                    return at(si, u32_of(*first), start, e2);
                }
            }
            _ => {}
        }
        (0, 0, 0)
    }

    /// The searcher of a command, or the query's error (`findQueryError`).
    fn searcher<'q>(&self, q: &'q FindQuery) -> Result<Search<'q>, QueryError> {
        let compiled = engine::compile_query(q)?;
        Ok(Search { q, compiled, budget: Budget::new(self.f9.find_budget.unwrap_or(regex::DEFAULT_STEP_BUDGET)), vm: Vm::default() })
    }

    fn texts<'s>(&'s self, stories: &[(StoryRef, &'s PVec<Paragraph>, usize, usize)], budget: &mut Budget) -> Found<Vec<StoryText<'s>>> {
        stories.iter().map(|(_, ps, first, count)| story_text(ps, *first, *count, budget)).collect()
    }

    fn hits(&self, s: &mut Search<'_>, st: &StoryText<'_>) -> Found<Vec<FindHit>> {
        let env = self.spell_langs();
        find_all(st, s.q, s.compiled.as_ref(), &env, &mut s.budget, &mut s.vm)
    }

    /// `selectFindHit`'s positions: `b` clamped to the last paragraph (the final ¶ mark).
    fn target(story: &StoryRef, st: &StoryText<'_>, h: &FindHit) -> Target {
        let (ap, ao) = pos_of(&st.starts, h.start);
        let (bp, mut bo) = pos_of(&st.starts, h.end);
        let last = st.para_count().saturating_sub(1);
        if bp as usize == last
            && let Some(len) = st.para(last).map(para_length)
            && bo as usize > len
        {
            bo = u32_of(len);
        }
        let base = match story {
            StoryRef::Body { first, .. } => u32_of(*first),
            StoryRef::Cell { .. } => 0,
        };
        Target { story: story.clone(), a: DocPosition::new(base + ap, ao), b: DocPosition::new(base + bp, bo) }
    }

    /// `findNextMatch`'s search: the hit to select and whether it wrapped.
    fn next_target(&self, s: &mut Search<'_>, dir: Direction) -> Found<Option<(Target, bool)>> {
        let stories = self.find_stories();
        if stories.is_empty() {
            return Ok(None);
        }
        let texts = self.texts(&stories, &mut s.budget)?;
        let (cs, from, to) = self.find_cursor(&stories, &texts);
        let n = stories.len();
        let pick = |si: usize, h: &FindHit| stories.get(si).zip(texts.get(si)).map(|(sr, st)| Session::target(&sr.0, st, h));
        if dir != Direction::Up {
            for k in 0..n {
                let si = cs + k;
                if si >= n {
                    break;
                }
                let Some(st) = texts.get(si) else { break };
                let hs = self.hits(s, st)?;
                if let Some(h) = hs.iter().find(|x| k > 0 || x.start >= to || (x.start >= from && from == to && x.end > from)) {
                    return Ok(pick(si, h).map(|t| (t, false)));
                }
            }
            if dir == Direction::All {
                for si in 0..=cs {
                    let Some(st) = texts.get(si) else { break };
                    let hs = self.hits(s, st)?;
                    if let Some(h) = hs.iter().find(|x| si < cs || x.start < to) {
                        return Ok(pick(si, h).map(|t| (t, true)));
                    }
                }
            }
            return Ok(None);
        }
        for si in (0..=cs).rev() {
            let Some(st) = texts.get(si) else { continue };
            let hs = self.hits(s, st)?;
            if let Some(h) = hs.iter().rev().find(|x| si < cs || x.end <= from) {
                return Ok(pick(si, h).map(|t| (t, false)));
            }
        }
        Ok(None)
    }

    /// `selectFindHit(story, st, h)`: selects a hit (entering its cell).
    fn select_target(&mut self, t: Target) {
        self.float_sel = None;
        match t.story {
            StoryRef::Body { .. } => {
                self.back_to_body();
                self.selection = Selection { anchor: t.a, focus: t.b };
            }
            StoryRef::Cell { anchor, hops } => {
                if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } | EditContext::Note { .. }) {
                    self.back_to_body();
                }
                if !self.enter_cell(anchor, hops, Some(t.a)) {
                    return;
                }
                self.selection = Selection { anchor: t.a, focus: t.b };
            }
        }
        self.pending_props = None;
    }

    /// `findNextMatch(q, o)` without the query check (the caller's).
    fn find_next_in(&mut self, s: &mut Search<'_>, dir: Direction) -> FindOutcome {
        match self.next_target(s, dir) {
            Ok(Some((t, wrapped))) => {
                self.select_target(t);
                FindOutcome { found: true, wrapped, ..FindOutcome::default() }
            }
            Ok(None) => FindOutcome::default(),
            Err(FindFailure::Budget) => FindOutcome { budget: true, ..FindOutcome::default() },
        }
    }

    /// `collectMatches(q, o)`: the hits per story (within the selection: the cursor's story only).
    fn collect(&self, s: &mut Search<'_>, within: Within) -> Found<Vec<StoryHits>> {
        let stories = self.find_stories();
        let texts = self.texts(&stories, &mut s.budget)?;
        let mut out = Vec::new();
        let one = |sr: &StoryRef, st: &StoryText<'_>, hs: Vec<FindHit>| -> Vec<(Target, Replace)> {
            hs.into_iter()
                .map(|h| {
                    let t = Session::target(sr, st, &h);
                    let r = Replace { a: t.a, b: t.b, found: found_text(st, &h), groups: h.groups.clone() };
                    (t, r)
                })
                .collect()
        };
        if within == Within::Selection {
            if !self.has_selection() {
                return Ok(out);
            }
            let (cs, from, to) = self.find_cursor(&stories, &texts);
            let (Some(st), Some(sr)) = (texts.get(cs), stories.get(cs)) else { return Ok(out) };
            let hs: Vec<FindHit> = self.hits(s, st)?.into_iter().filter(|h| h.start >= from && h.end <= to).collect();
            out.push((sr.0.clone(), one(&sr.0, st, hs)));
            return Ok(out);
        }
        for (sr, st) in stories.iter().zip(&texts) {
            let hs = self.hits(s, st)?;
            out.push((sr.0.clone(), one(&sr.0, st, hs)));
        }
        Ok(out)
    }

    /// `countMatches(q, o)`: how many hits (0 for a bad query); the budget is the error.
    pub fn count_matches(&self, q: &FindQuery, within: Within) -> Found<u32> {
        let Ok(mut s) = self.searcher(q) else { return Ok(0) };
        Ok(u32_of(self.collect(&mut s, within)?.iter().map(|g| g.1.len()).sum()))
    }

    /// `findQueryError(q)`: the error of the query, if any.
    #[must_use]
    pub fn find_query_error(&self, q: &FindQuery) -> Option<QueryError> {
        engine::compile_query(q).err()
    }

    /// The spans of the last `highlightMatches` (story, paragraph range per hit).
    #[must_use]
    pub fn highlight_spans(&self) -> &[(super::navigation::SearchLoc, u32, u32)] {
        &self.f9.highlight
    }

    /// `findFormatPatch(rf)`: the replacement's format as run props (key order of the TS).
    fn find_format_patch(f: Option<&FindFormat>) -> RunProps {
        let mut p = RunProps::default();
        let Some(f) = f else { return p };
        let flag = |p: &mut RunProps, key: &str, v: Option<bool>, w: fn(&mut RunProps, bool)| {
            if let Some(v) = v {
                obj::set(p, key, |x| w(x, v));
            }
        };
        flag(&mut p, "bold", f.bold, |x, v| x.bold = Some(v));
        flag(&mut p, "italic", f.italic, |x, v| x.italic = Some(v));
        flag(&mut p, "underline", f.underline, |x, v| x.underline = Some(v));
        flag(&mut p, "strike", f.strike, |x, v| x.strike = Some(v));
        flag(&mut p, "doubleStrike", f.double_strike, |x, v| x.double_strike = Some(v));
        flag(&mut p, "allCaps", f.all_caps, |x, v| x.all_caps = Some(v));
        flag(&mut p, "smallCaps", f.small_caps, |x, v| x.small_caps = Some(v));
        flag(&mut p, "hidden", f.hidden, |x, v| x.hidden = Some(v));
        if let Some(v) = &f.vert_align {
            match v {
                Some(v) => obj::set(&mut p, "vertAlign", |x| x.vert_align = Some(v.as_str().into())),
                None => obj::set_undefined(&mut p, "vertAlign", |x| x.vert_align = None),
            }
        }
        if let Some(font) = &f.font {
            obj::set(&mut p, "fontAscii", |x| x.font_ascii = Some(font.as_str().into()));
        }
        if let Some(sz) = f.size_pt {
            obj::set(&mut p, "szHalfPt", |x| x.sz_half_pt = Some(JsNumber::Num(uo_jsnum::math::round(sz * 2.0))));
        }
        if let Some(c) = &f.color {
            obj::set(&mut p, "color", |x| x.color = Some(uo_text::case::to_uppercase_str(c).into()));
        }
        // the highlight: Word's default colour (yellow; Options.DefaultHighlightColorIndex 7)
        if let Some(h) = f.highlight {
            if h {
                obj::set(&mut p, "highlight", |x| x.highlight = Some("yellow".into()));
            } else {
                obj::set_undefined(&mut p, "highlight", |x| x.highlight = None);
            }
        }
        if let Some(cs) = &f.char_style {
            if cs.is_empty() {
                obj::set_undefined(&mut p, "charStyle", |x| x.char_style = None);
            } else {
                obj::set(&mut p, "charStyle", |x| x.char_style = Some(cs.as_str().into()));
            }
        }
        p
    }

    /// `applyRunOne(range, patch)` (F1's is private): tracked as `w:rPrChange` with tracking on.
    fn apply_run_one_f9(&mut self, range: DocRange, patch: &RunProps) -> Result<(), CommandError> {
        let nr = normalize_range(range);
        if self.opts.tracking {
            let rev = self.new_rev();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::track_apply_run_props(paras, range, patch, &rev)?;
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::apply_run_props(paras, range, patch)?;
        }
        for pi in nr.start.para..=nr.end.para {
            self.touch_para(pi as usize);
        }
        Ok(())
    }

    /// `splitAt(pos)` (the text family's is private).
    fn split_at_f9(&mut self, pos: DocPosition) -> Result<DocPosition, CommandError> {
        let rev = self.opts.tracking.then(|| self.new_rev());
        let ctx = self.ctx.clone();
        let mut ids = std::mem::take(&mut self.doc.ids);
        let out = match super::context::story_of_mut(&mut self.doc, &ctx) {
            Some(story) => match &rev {
                Some(rev) => guarded::track_split_paragraph(&mut story.paras, &mut ids, pos, rev),
                None => guarded::split_paragraph(&mut story.paras, &mut ids, pos),
            },
            None => Err(OpError::Position),
        };
        self.doc.ids = ids;
        let out = out?;
        self.touch_para(pos.para as usize);
        self.touch_structure();
        Ok(out)
    }

    /// `setParaStyleAt(para, styleId)`.
    fn set_para_style_at(&mut self, para: u32, style: &str) -> Result<(), CommandError> {
        let len = self.paras().and_then(|p| p.get(para as usize)).map_or(0, para_length);
        let mut patch = ParaProps::default();
        obj::set(&mut patch, "styleId", |p| p.style_id = Some(style.into()));
        self.apply_para(DocRange::new(DocPosition::new(para, 0), DocPosition::new(para, u32_of(len))), &patch)
    }

    /// `replaceRange(range, found, groups, q, repl, rf)`: one hit replaced in the edited story;
    /// the end of the new text.
    #[allow(clippy::too_many_arguments)]
    fn replace_range(
        &mut self,
        range: DocRange,
        found: &[u16],
        groups: Option<&[Vec<u16>]>,
        q: &FindQuery,
        repl: &[u16],
        rf: Option<&FindFormat>,
        clipboard: &[u16],
    ) -> Result<DocPosition, CommandError> {
        let DocRange { start, end } = normalize_range(range);
        let patch = Session::find_format_patch(rf);
        let para_style = rf.and_then(|f| f.para_style.as_deref());
        if q.text.is_empty() && repl.is_empty() {
            if !obj::key_indices(&patch).is_empty() {
                self.apply_run_one_f9(DocRange::new(start, end), &patch)?;
            }
            if let Some(ps) = para_style {
                for p in start.para..=end.para {
                    self.set_para_style_at(p, ps)?;
                }
            }
            return Ok(end);
        }
        // the new text takes the format of the first character found (+ the replacement's)
        let first: CarriedProps = if start != end {
            self.paras().map_or_else(CarriedProps::default, |ps| props_at(ps, DocPosition::new(start.para, start.offset.saturating_add(1))))
        } else {
            self.paras()
                .map_or_else(|| CarriedProps::plain(crate::read::plain::default_run_props()), |ps| props_at(ps, self.selection.focus))
        };
        let mut text = expand_replacement(repl, found, groups, q.wildcards, clipboard);
        if !q.match_case && !q.wildcards {
            text = adapt_case(found, &text);
        }
        self.selection = Selection { anchor: start, focus: end };
        let mut pos = if start != end { self.delete_selection()? } else { start };
        let props = CarriedProps { props: obj::spread(&without_revisions(&first.props), &patch), link: first.link };
        let p0 = pos.para;
        for (i, part) in text.split(|&u| u == 0x0D).enumerate() {
            if i > 0 {
                self.shift_anchors(pos.para, 1);
                pos = self.split_at_f9(pos)?;
            }
            if !part.is_empty() {
                pos = self.insert_typed(pos, part, Some(&props))?;
            }
        }
        if let Some(ps) = para_style {
            for p in p0..=pos.para {
                self.set_para_style_at(p, ps)?;
            }
        }
        Ok(pos)
    }

    fn clipboard_text(cx: &ExecCx<'_>) -> Vec<u16> {
        cx.clipboard.current().map(|c| c.text.units().to_vec()).unwrap_or_default()
    }

    fn find_result(&mut self, r: F9Result) -> Result<Outcome, CommandError> {
        self.f9.result = Some(r);
        Ok(Outcome::Done(self.change_set()))
    }

    /// `replaceMatch(q, replacement, o)`: the selection, if it is a hit, is replaced and the next one
    /// found; otherwise only the next one is found.
    fn replace_match(
        &mut self,
        q: &FindQuery,
        repl: &[u16],
        dir: Direction,
        rf: Option<&FindFormat>,
        cx: &mut ExecCx<'_>,
    ) -> Result<Outcome, CommandError> {
        let mut s = match self.searcher(q) {
            Ok(s) => s,
            Err(e) => {
                return self.find_result(F9Result::Replace(ReplaceOutcome {
                    find: FindOutcome { error: Some(e), ..FindOutcome::default() },
                    ..ReplaceOutcome::default()
                }));
            }
        };
        if self.locked() {
            return self.find_result(F9Result::Replace(ReplaceOutcome::default()));
        }
        let hit = (|| -> Found<Option<Replace>> {
            let stories = self.find_stories();
            let texts = self.texts(&stories, &mut s.budget)?;
            let (cs, from, to) = self.find_cursor(&stories, &texts);
            let (Some(st), Some(sr)) = (texts.get(cs), stories.get(cs)) else { return Ok(None) };
            if !self.has_selection() {
                return Ok(None);
            }
            let hs = self.hits(&mut s, st)?;
            Ok(hs.into_iter().find(|h| h.start == from && h.end == to).map(|h| {
                let t = Session::target(&sr.0, st, &h);
                Replace { a: t.a, b: t.b, found: found_text(st, &h), groups: h.groups }
            }))
        })();
        let hit = match hit {
            Ok(h) => h,
            Err(FindFailure::Budget) => {
                let find = FindOutcome { budget: true, ..FindOutcome::default() };
                return self.find_result(F9Result::Replace(ReplaceOutcome { find, ..ReplaceOutcome::default() }));
            }
        };
        let Some(hit) = hit else {
            let find = self.find_next_in(&mut s, dir);
            return self.find_result(F9Result::Replace(ReplaceOutcome { find, ..ReplaceOutcome::default() }));
        };
        self.begin_step(false, &[]);
        let range = normalize_range(self.range());
        let clip = Session::clipboard_text(cx);
        let pos = self.replace_range(range, &hit.found, hit.groups.as_deref(), q, repl, rf, &clip)?;
        self.selection = Selection::caret(pos);
        let next = self.find_next_in(&mut s, dir);
        if !next.found {
            self.selection = Selection::caret(pos);
        }
        self.f9.result = Some(F9Result::Replace(ReplaceOutcome { find: next, replaced: true, count: 0 }));
        Ok(Outcome::Done(self.finish()))
    }

    /// `replaceAllMatches(q, replacement, o)`: one undo step; the new text is not searched again.
    fn replace_all(
        &mut self,
        q: &FindQuery,
        repl: &[u16],
        within: Within,
        rf: Option<&FindFormat>,
        cx: &mut ExecCx<'_>,
    ) -> Result<Outcome, CommandError> {
        let mut s = match self.searcher(q) {
            Ok(s) => s,
            Err(e) => {
                return self.find_result(F9Result::Replace(ReplaceOutcome {
                    find: FindOutcome { error: Some(e), ..FindOutcome::default() },
                    ..ReplaceOutcome::default()
                }));
            }
        };
        if self.locked() {
            return self.find_result(F9Result::Replace(ReplaceOutcome::default()));
        }
        let groups = match self.collect(&mut s, within) {
            Ok(g) => g.into_iter().filter(|g| !g.1.is_empty()).collect::<Vec<_>>(),
            Err(FindFailure::Budget) => {
                let find = FindOutcome { budget: true, ..FindOutcome::default() };
                return self.find_result(F9Result::Replace(ReplaceOutcome { find, ..ReplaceOutcome::default() }));
            }
        };
        if groups.is_empty() {
            return self.find_result(F9Result::Replace(ReplaceOutcome::default()));
        }
        let keep_cell = match &self.ctx {
            EditContext::Cell { anchor, hops, .. } => Some((*anchor, hops.to_vec())),
            _ => None,
        };
        let keep_sel = self.selection;
        let clip = Session::clipboard_text(cx);
        self.begin_step(false, &[]);
        let mut count = 0u32;
        // from the end back: earlier positions (and the stories' indices) do not move
        for (story, hits) in groups.into_iter().rev() {
            match &story {
                StoryRef::Body { .. } => {
                    self.back_to_body();
                }
                StoryRef::Cell { anchor, hops } => {
                    if matches!(self.ctx, EditContext::HeaderFooter { .. } | EditContext::Textbox { .. } | EditContext::Note { .. }) {
                        self.back_to_body();
                    }
                    if !self.enter_cell(*anchor, hops.clone(), Some(DocPosition::default())) {
                        continue;
                    }
                }
            }
            for (_, h) in hits.into_iter().rev() {
                self.replace_range(DocRange::new(h.a, h.b), &h.found, h.groups.as_deref(), q, repl, rf, &clip)?;
                count += 1;
            }
            if matches!(story, StoryRef::Cell { .. }) {
                self.exit_cell();
            }
        }
        // back where it was (the position may have shrunk: clamped)
        self.back_to_body();
        if let Some((anchor, hops)) = keep_cell {
            self.enter_cell(anchor, hops, Some(DocPosition::default()));
        }
        let c = super::context::clamp_selection(self.paras(), Selection::caret(keep_sel.focus));
        self.selection = c;
        self.f9.result = Some(F9Result::Replace(ReplaceOutcome { count, ..ReplaceOutcome::default() }));
        Ok(Outcome::Done(self.finish()))
    }

    /// `selectAllMatches(q, o)`: every hit of the body selected (a non-contiguous selection, as
    /// Word); the hits in tables only count — when there are only those, the first is selected.
    fn select_all_matches(&mut self, q: &FindQuery, within: Within) -> Result<Outcome, CommandError> {
        let Ok(mut s) = self.searcher(q) else { return self.find_result(F9Result::Count(0)) };
        let groups = match self.collect(&mut s, within) {
            Ok(g) => g,
            Err(FindFailure::Budget) => return self.find_result(F9Result::Find(FindOutcome { budget: true, ..FindOutcome::default() })),
        };
        let mut ranges: Vec<DocRange> = Vec::new();
        let mut cell_only: Option<Target> = None;
        for (story, hits) in groups {
            for (t, _) in hits {
                if !matches!(story, StoryRef::Body { .. }) {
                    if cell_only.is_none() {
                        cell_only = Some(t);
                    }
                    continue;
                }
                let len = self.doc.body.paras.get(t.b.para as usize).map_or(0, para_length);
                ranges.push(DocRange::new(t.a, DocPosition::new(t.b.para, t.b.offset.min(u32_of(len)))));
            }
        }
        let Some(main) = ranges.pop() else {
            if let Some(t) = cell_only {
                self.select_target(t);
                return self.find_result(F9Result::Count(1));
            }
            return self.find_result(F9Result::Count(0));
        };
        self.back_to_body();
        self.float_sel = None;
        self.selection = Selection { anchor: main.start, focus: main.end };
        let n = u32_of(ranges.len() + 1);
        self.set_extra_selection(ranges);
        self.find_result(F9Result::Count(n))
    }

    /// `highlightMatches(q, o)`: the spans of every hit (Reading Highlight › Highlight All).
    fn highlight_matches(&mut self, q: &FindQuery, within: Within) -> Result<Outcome, CommandError> {
        self.f9.clear_search();
        self.f9.highlight.clear();
        let Ok(mut s) = self.searcher(q) else { return self.find_result(F9Result::Count(0)) };
        let groups = match self.collect(&mut s, within) {
            Ok(g) => g,
            Err(FindFailure::Budget) => return self.find_result(F9Result::Find(FindOutcome { budget: true, ..FindOutcome::default() })),
        };
        let mut n = 0u32;
        let mut spans = Vec::new();
        for (story, hits) in groups {
            for (t, _) in hits {
                n += 1;
                for p in t.a.para..=t.b.para {
                    let len = match &story {
                        StoryRef::Body { .. } => self.doc.body.paras.get(p as usize).map_or(0, para_length),
                        StoryRef::Cell { anchor, hops } => {
                            let ctx = EditContext::Cell { anchor: *anchor, hops: hops.clone().into(), saved: Selection::default() };
                            super::context::story_of(&self.doc, &ctx).and_then(|st| st.paras.get(p as usize)).map_or(0, para_length)
                        }
                    };
                    let s0 = if p == t.a.para { t.a.offset } else { 0 };
                    let e0 = if p == t.b.para { t.b.offset } else { u32_of(len) };
                    if e0 <= s0 {
                        continue;
                    }
                    let loc = match &story {
                        StoryRef::Body { .. } => super::navigation::SearchLoc::Body { para: p },
                        StoryRef::Cell { anchor, hops } => {
                            super::navigation::SearchLoc::Cell { anchor: *anchor, hops: hops.clone(), para: p }
                        }
                    };
                    spans.push((loc, s0, e0));
                }
            }
        }
        self.f9.highlight = spans;
        self.find_result(F9Result::Count(n))
    }

    /// `goToBookmark(name)`: the bookmark's range selected (in its cell), or the paragraph of a
    /// bookmark known only by paragraph.
    fn go_to_bookmark(&mut self, name: &str) -> bool {
        for ms in self.mark_stories() {
            let Some(st) = super::revision_cmds::story_at(&self.doc, &ms.loc) else { continue };
            let Some(b) = super::bookmarks::find_bookmark(st.paras.iter(), name) else { continue };
            match &ms.loc {
                StoryLoc::Body => {
                    self.back_to_body();
                    self.selection = Selection { anchor: b.start, focus: b.end };
                }
                StoryLoc::Cell(path) => {
                    let Some((anchor, hops)) = cell_hops(&self.doc.body, path) else { return false };
                    self.back_to_body();
                    if !self.enter_cell(anchor, hops, Some(b.start)) {
                        return false;
                    }
                    self.selection = Selection { anchor: b.start, focus: b.end };
                }
                _ => return false,
            }
            self.pending_props = None;
            return true;
        }
        let lname = uo_text::case::to_lowercase_str(name);
        let para = self.bookmark_paras.iter().find(|(k, _)| uo_text::case::to_lowercase_str(k) == lname).map(|(_, v)| v);
        let Some(para) = para else { return false };
        self.back_to_body();
        let last = u32_of(self.doc.body.paras.len().saturating_sub(1));
        self.selection = Selection::caret(DocPosition::new(para.min(last), 0));
        self.pending_props = None;
        true
    }

    /// `goToComment(id)`: the first run of the comment selected (body).
    fn go_to_comment(&mut self, id: f64) {
        let Some(paras) = self.paras() else { return };
        let mut hit = None;
        'p: for (pi, p) in paras.iter().enumerate() {
            let mut acc = 0u32;
            for r in &p.runs {
                if r.props.comment_ids.as_deref().is_some_and(|ids| ids.iter().any(|x| x.to_number() == id)) {
                    let len = u32_of(r.text.len());
                    hit = Some(Selection { anchor: DocPosition::new(u32_of(pi), acc), focus: DocPosition::new(u32_of(pi), acc + len) });
                    break 'p;
                }
                acc += u32_of(r.text.len());
            }
        }
        if let Some(sel) = hit {
            self.selection = sel;
        }
    }

    /// `goTo(what, target)` (Find and Replace › Go To): the n-th (`"5"`), relative (`"+2"`, `"-1"`)
    /// or next (`""`) item; by name for a bookmark and a field, by reviewer for a comment.
    fn go_to(&mut self, what: GoToWhat, target: &[u16]) -> Result<bool, CommandError> {
        if matches!(what, GoToWhat::Page | GoToWhat::Line) {
            return Err(CommandError::NotPorted("goTo page/line: the layout's (the shell answers it)"));
        }
        let t: Vec<u16> = uo_text::space::trim(target).to_vec();
        let (rel, abs) = goto_target(target);
        let pick = |len: usize, cur: f64| -> Option<usize> {
            if len == 0 {
                return None;
            }
            let lenf = len as f64;
            if let Some(a) = abs {
                let k = uo_jsnum::math::min(lenf, uo_jsnum::math::max(1.0, a)) - 1.0;
                return (k >= 0.0 && k < lenf).then(|| uo_jsnum::conv::to_uint32(k) as usize);
            }
            let r = rel.unwrap_or(1.0);
            let i = if r > 0.0 { uo_jsnum::math::floor(cur) + r } else { uo_jsnum::math::ceil(cur) + r };
            let k = uo_jsnum::math::min(lenf - 1.0, uo_jsnum::math::max(0.0, i));
            (k >= 0.0 && k < lenf).then(|| uo_jsnum::conv::to_uint32(k) as usize)
        };
        let focus = match &self.ctx {
            EditContext::Cell { anchor, .. } => {
                DocPosition::new(self.doc.body.tables.get(*anchor as usize).map_or(0, |a| a.before_para), 0)
            }
            _ => self.selection.focus,
        };
        let cur_in = |ps: &[DocPosition]| -> f64 {
            let mut k = -0.5;
            for (i, p) in ps.iter().enumerate() {
                match compare_pos(*p, focus) {
                    std::cmp::Ordering::Equal => return i as f64,
                    std::cmp::Ordering::Less => k = i as f64 + 0.5,
                    std::cmp::Ordering::Greater => {}
                }
            }
            k
        };
        let caret_to = |s: &mut Session, p: DocPosition, sel: Option<DocPosition>| {
            s.back_to_body();
            s.float_sel = None;
            s.selection = Selection { anchor: p, focus: sel.unwrap_or(p) };
            s.pending_props = None;
            true
        };
        let body = &self.doc.body;
        match what {
            GoToWhat::Page | GoToWhat::Line => Ok(false),
            GoToWhat::Section => {
                let mut starts = vec![DocPosition::new(0, 0)];
                for (pi, p) in body.paras.iter().enumerate() {
                    if p.props.section_break.is_some() {
                        starts.push(DocPosition::new(u32_of(pi + 1), 0));
                    }
                }
                let Some(p) = pick(starts.len(), cur_in(&starts)).and_then(|k| starts.get(k).copied()) else { return Ok(false) };
                Ok(caret_to(self, p, None))
            }
            GoToWhat::Heading => {
                let ps: Vec<DocPosition> = self.headings().iter().map(|h| DocPosition::new(h.para, 0)).collect();
                let Some(p) = pick(ps.len(), cur_in(&ps)).and_then(|k| ps.get(k).copied()) else { return Ok(false) };
                Ok(caret_to(self, p, None))
            }
            GoToWhat::Bookmark => {
                if t.is_empty() {
                    return Ok(false);
                }
                Ok(self.go_to_bookmark(&String::from_utf16_lossy(&t)))
            }
            GoToWhat::Comment => {
                let by_author: Vec<(f64, String)> =
                    self.comments().iter().map(|c| (c.id.to_number(), c.author.as_deref().unwrap_or("").to_owned())).collect();
                let tname = String::from_utf16_lossy(&t);
                let mut out: Vec<(f64, DocPosition)> = Vec::new();
                let mut seen: Vec<f64> = Vec::new();
                for (pi, p) in body.paras.iter().enumerate() {
                    let mut off = 0u32;
                    for r in &p.runs {
                        for id in r.props.comment_ids.as_deref().unwrap_or(&[]) {
                            let id = id.to_number();
                            if seen.iter().any(|s| same_value_zero(*s, id)) {
                                continue;
                            }
                            seen.push(id);
                            if rel.is_none() && abs.is_none() && !t.is_empty() {
                                // `byAuthor.get(id) !== t` (a Map keyed by SameValueZero)
                                let author = by_author.iter().find(|(k, _)| same_value_zero(*k, id)).map(|(_, a)| a.as_str());
                                if author != Some(tname.as_str()) {
                                    continue;
                                }
                            }
                            out.push((id, DocPosition::new(u32_of(pi), off)));
                        }
                        off += u32_of(r.text.len());
                    }
                }
                let ps: Vec<DocPosition> = out.iter().map(|x| x.1).collect();
                let Some(c) = pick(out.len(), cur_in(&ps)).and_then(|k| out.get(k).copied()) else { return Ok(false) };
                self.back_to_body();
                self.go_to_comment(c.0);
                Ok(true)
            }
            GoToWhat::Footnote | GoToWhat::Endnote => {
                let kind = if what == GoToWhat::Footnote { "footnote" } else { "endnote" };
                let mut out: Vec<DocPosition> = Vec::new();
                for (pi, p) in body.paras.iter().enumerate() {
                    let mut off = 0u32;
                    for r in &p.runs {
                        if r.ext().and_then(|e| e.note_ref.as_ref()).is_some_and(|n| &*n.kind == kind) {
                            out.push(DocPosition::new(u32_of(pi), off));
                        }
                        off += u32_of(r.text.len());
                    }
                }
                let Some(p) = pick(out.len(), cur_in(&out)).and_then(|k| out.get(k).copied()) else { return Ok(false) };
                Ok(caret_to(self, p, None))
            }
            GoToWhat::Table => {
                let mut tabs: Vec<(usize, u32, f64)> = body
                    .tables
                    .iter()
                    .enumerate()
                    .map(|(ai, a)| {
                        (
                            ai,
                            a.before_para,
                            a.table
                                .rows
                                .first()
                                .and_then(|r| r.tr_pr.as_deref())
                                .and_then(|p| p.grid_before)
                                .map_or(0.0, |g| g.to_number()),
                        )
                    })
                    .collect();
                tabs.sort_by_key(|x| x.1);
                let ps: Vec<DocPosition> = tabs.iter().map(|x| DocPosition::new(x.1, 0)).collect();
                let Some(&(ai, _, gb)) = pick(tabs.len(), cur_in(&ps)).and_then(|k| tabs.get(k)) else { return Ok(false) };
                self.back_to_body();
                Ok(self.enter_cell(
                    u32_of(ai),
                    vec![CellHop { row: 0, col: uo_jsnum::conv::to_uint32(gb), block: None }],
                    Some(DocPosition::default()),
                ))
            }
            GoToWhat::Field | GoToWhat::Graphic | GoToWhat::Equation | GoToWhat::Object => {
                let name: Option<Vec<u16>> =
                    (rel.is_none() && abs.is_none()).then(|| uo_text::case::to_uppercase(&t).into_owned()).filter(|n| !n.is_empty());
                let mut out: Vec<DocPosition> = Vec::new();
                for (pi, p) in body.paras.iter().enumerate() {
                    let mut off = 0u32;
                    for r in &p.runs {
                        let e = r.ext();
                        let instr = e.and_then(|e| e.field.as_deref().or(e.field_begin.as_deref()));
                        let truthy = |s: Option<&str>| s.is_some_and(|s| !s.is_empty());
                        let is_field = e.is_some_and(|e| {
                            truthy(e.field.as_deref())
                                || truthy(e.field_begin.as_deref())
                                || e.field_result.is_some()
                                || e.form_field.is_some()
                        });
                        let ole = e.and_then(|e| e.image.as_deref()).is_some_and(|i| i.ole.is_some());
                        let ok = match what {
                            GoToWhat::Field => {
                                is_field
                                    && name
                                        .as_ref()
                                        .is_none_or(|n| field_name(&instr.unwrap_or("").encode_utf16().collect::<Vec<u16>>()) == *n)
                            }
                            GoToWhat::Graphic => e.is_some_and(|e| e.image.is_some() || e.shape.is_some() || e.group.is_some()) && !ole,
                            GoToWhat::Equation => e.is_some_and(|e| e.math.is_some()) || r.props.is_math == Some(true),
                            _ => ole,
                        };
                        let here = DocPosition::new(u32_of(pi), off);
                        if ok && out.last() != Some(&here) {
                            out.push(here);
                        }
                        off += u32_of(r.text.len());
                    }
                }
                let Some(p) = pick(out.len(), cur_in(&out)).and_then(|k| out.get(k).copied()) else { return Ok(false) };
                // a graphic, object or equation stays selected (its character)
                let len = {
                    let mut o = 0u32;
                    let mut l = 0u32;
                    if let Some(para) = body.paras.get(p.para as usize) {
                        for r in &para.runs {
                            if o == p.offset {
                                l = u32_of(r.text.len());
                                break;
                            }
                            o += u32_of(r.text.len());
                        }
                    }
                    l
                };
                let sel = (what != GoToWhat::Field).then(|| DocPosition::new(p.para, p.offset + len));
                Ok(caret_to(self, p, sel))
            }
        }
    }

    pub(crate) fn exec_find(&mut self, cmd: FindCmd, cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            FindCmd::FindNext { query, direction } => {
                let out = match self.searcher(&query) {
                    Err(e) => FindOutcome { error: Some(e), ..FindOutcome::default() },
                    Ok(mut s) => self.find_next_in(&mut s, direction),
                };
                self.find_result(F9Result::Find(out))
            }
            FindCmd::ReplaceMatch { query, replacement, direction, replace_format } => {
                self.replace_match(&query, &replacement, direction, replace_format.as_deref(), cx)
            }
            FindCmd::ReplaceAll { query, replacement, within, replace_format } => {
                self.replace_all(&query, &replacement, within, replace_format.as_deref(), cx)
            }
            FindCmd::SelectAllMatches { query, within } => self.select_all_matches(&query, within),
            FindCmd::HighlightMatches { query, within } => self.highlight_matches(&query, within),
            FindCmd::GoTo { what, target } => {
                if self.paras().is_none() {
                    return self.find_result(F9Result::Bool(false));
                }
                let v = self.go_to(what, &target)?;
                self.find_result(F9Result::Bool(v))
            }
            FindCmd::GoToBookmark { name } => {
                let v = self.go_to_bookmark(&name);
                self.find_result(F9Result::Bool(v))
            }
            FindCmd::GoToComment { id } => {
                self.go_to_comment(id);
                Ok(Outcome::Done(self.change_set()))
            }
            FindCmd::ReplaceAllPlain { needle, replacement } => self.replace_all_plain(&needle, &replacement),
        }
    }

    /// `replaceAll(needle, replacement)` (universal-write.ts 4764): every paragraph of the edit story (`this.model`), the
    /// text lowered by JS `toLowerCase` (indices of the lowered text, as the TS), past each replacement; a hit over deleted
    /// text (`rangeHasDeleted`) moves one unit on; tracked: `trackedDelete(range, 'end')` + `insertTyped`, else
    /// `deleteRange` + `insertText`. One undo step (the TS snapshots even when nothing matches; no lock check, as the TS).
    fn replace_all_plain(&mut self, needle: &[u16], repl: &[u16]) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || needle.is_empty() {
            return self.find_result(F9Result::Count(0));
        }
        self.begin_step(false, &[]);
        let lower = uo_text::case::to_lowercase(needle).into_owned();
        let mut count = 0u32;
        let mut pi = 0usize;
        while pi < self.paras().map_or(0, PVec::len) {
            let mut from = 0usize;
            while let Some(p) = self.paras().and_then(|ps| ps.get(pi)) {
                let text = uo_text::case::to_lowercase(&super::super::text::para_text(p)).into_owned();
                let Some(idx) = index_of_from(&text, &lower, from) else { break };
                let (a, b) = (u32::try_from(idx).unwrap_or(u32::MAX), u32::try_from(idx + needle.len()).unwrap_or(u32::MAX));
                let pu = u32::try_from(pi).unwrap_or(u32::MAX);
                let range = DocRange::new(DocPosition::new(pu, a), DocPosition::new(pu, b));
                if range_has_deleted(p, a as usize, b as usize) {
                    from = idx + 1;
                    continue;
                }
                let r = if self.opts.tracking {
                    self.tracked_delete(range, false).and_then(|pos| {
                        self.insert_typed(pos, repl, None)?;
                        Ok(pos.offset as usize + repl.len())
                    })
                } else {
                    match self.paras_mut().ok_or(CommandError::Position) {
                        Ok(paras) => guarded::delete_range(paras, range)
                            .and_then(|pos| guarded::insert_text(paras, pos, repl, None))
                            .map(|_| idx + repl.len())
                            .map_err(CommandError::from),
                        Err(e) => Err(e),
                    }
                };
                match r {
                    Ok(next) => from = next,
                    Err(e) => {
                        self.discard_step();
                        return Err(e);
                    }
                }
                self.touch_para(pi);
                count += 1;
            }
            pi += 1;
        }
        self.f9.result = Some(F9Result::Count(count));
        Ok(Outcome::Done(self.finish()))
    }
}

/// `text.indexOf(needle, from)` over UTF-16 units (`from` past the end: none, unless the needle is empty).
fn index_of_from(text: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.len() > text.len() {
        return None;
    }
    (from..=text.len() - needle.len()).find(|&i| text.get(i..i + needle.len()) == Some(needle))
}

/// `rangeHasDeleted` for a range inside one paragraph: a deleted run (`props.del`) overlapping `[from, to)`.
fn range_has_deleted(p: &Paragraph, from: usize, to: usize) -> bool {
    let mut acc = 0usize;
    for r in &p.runs {
        let rs = acc;
        acc += r.text.len();
        if r.props.del.is_some() && rs < to && acc > from {
            return true;
        }
    }
    false
}

/// `goTo`'s target: `rel` (`+n`, `-n`, `+`, `-`, empty = the next) and `abs` (`n`), from the
/// trimmed text.
fn goto_target(target: &[u16]) -> (Option<f64>, Option<f64>) {
    let t = uo_text::space::trim(target);
    let digits = |s: &[u16]| s.iter().all(|&u| (0x30..=0x39).contains(&u));
    let first = t.first().copied();
    let rel = if matches!(first, Some(0x2B | 0x2D)) && digits(t.get(1..).unwrap_or(&[])) {
        Some(if t.len() > 1 {
            uo_jsnum::parse::parse_int_utf16(t, 10)
        } else if first == Some(0x2D) {
            -1.0
        } else {
            1.0
        })
    } else if t.is_empty() {
        Some(1.0)
    } else {
        None
    };
    let abs = (rel.is_none() && !t.is_empty() && digits(t)).then(|| uo_jsnum::parse::parse_int_utf16(t, 10));
    (rel, abs)
}

/// `(instr ?? '').trim().split(/\s+/)[0]?.toUpperCase()`: a field's name.
fn field_name(instr: &[u16]) -> Vec<u16> {
    let ins = uo_text::space::trim(instr);
    let end = ins.iter().position(|&u| uo_text::space::is_space(u)).unwrap_or(ins.len());
    uo_text::case::to_uppercase(ins.get(..end).unwrap_or(&[])).into_owned()
}

/// The hand matchers of F9's regex sites (`js/regex-sites/d2b-f9.mjs`, rows of V8 results in
/// `tests/regex/d2b-f9.*`): the answer of the site `id` for `input`, as the row writes it.
#[doc(hidden)]
#[must_use]
pub fn regex_site(id: &str, input: &[u16]) -> Option<String> {
    let hx = |t: &[u16]| t.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".");
    let num = |x: Option<f64>| x.map_or("null".to_owned(), uo_jsnum::format::to_js_string);
    match id {
        "d2b-f9.goto_target" => {
            let (rel, abs) = goto_target(input);
            Some(format!("{}|{}", num(rel), num(abs)))
        }
        "d2b-f9.field_name" => Some(hx(&field_name(input))),
        _ => engine::site(id, input).or_else(|| super::navigation::site(id, input)),
    }
}

/// The `(anchor, hops)` of a cell path of `markStories` (`StoryLoc::Cell`): the grid column of each
/// cell (`gridBefore` + the spans before it) and, for a nested table, its block index.
fn cell_hops(body: &Story, path: &[CellStep]) -> Option<(u32, Vec<CellHop>)> {
    let first = path.first()?;
    let anchor = first.table;
    let mut story = body;
    let mut hops = Vec::new();
    for (i, step) in path.iter().enumerate() {
        let t = &story.tables.get(step.table as usize)?.table;
        if i > 0 {
            // the block index of this table in the containing cell
            let bi = story.blocks().enumerate().filter(|(_, b)| matches!(b, Block::Table(_))).nth(step.table as usize)?.0;
            if let Some(h) = hops.last_mut() {
                let h: &mut CellHop = h;
                h.block = Some(u32_of(bi));
            }
        }
        let row = t.rows.get(step.row as usize)?;
        let mut c = row.tr_pr.as_deref().and_then(|p| p.grid_before).map_or(0.0, |g| g.to_number());
        for cell in row.cells.iter().take(step.cell as usize) {
            c += uo_jsnum::math::max(1.0, cell.tc_pr.as_deref().and_then(|p| p.grid_span).unwrap_or(cell.grid_span).to_number());
        }
        hops.push(CellHop { row: step.row, col: uo_jsnum::conv::to_uint32(c), block: None });
        story = &row.cells.get(step.cell as usize)?.content;
    }
    Some((anchor, hops))
}

impl FindCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`): none of this family's.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        None
    }
}
