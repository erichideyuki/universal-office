//! D2b family: bookmarks and links (frontier D2b-F6): `insertBookmark`, `deleteBookmark`,
//! `insertHyperlink`, `setLink`, `removeLink` of `universal-write.ts` (6105–6211, 3486, 7711–7757)
//! with the pure bookmark rules of `editor/src/references.ts` they use (`validBookmarkName`,
//! `bookmarksIn`, `findBookmark`, `removeBookmarkMarks`, `addBookmarkMarks`, `textBetween`,
//! `maxBookmarkId`, `unlinkRuns`; the rest of that file is D2b-F10's).
//!
//! The bookmark lists (`markStories()`) are the document's stories stored once: the body, every
//! table cell, the notes, the header/footer parts. `nextBmId` is per session (`Session::next_bm_id`).
//! Hooks of other families: `builtin_style_id` (styles, D2b-F2) and `ref_bookmark` (references,
//! D2b-F10) are stubs in the owners' files.

use std::sync::Arc;

use super::super::command::{CommandError, Event, Outcome};
use super::super::guard::guarded;
use super::super::policy::GuardArg;
use super::super::state::Selection;
use super::super::text::{
    CarriedProps, DocPosition, DocRange, compare_pos, insert_index, normalize_paragraph, normalize_range, para_length, props_at, split_at,
};
use super::super::{ExecCx, Session};
use super::context::EditContext;
use super::revision_cmds::{CellStep, StoryLoc, story_at, story_at_mut};
use crate::js::{JsMap, JsNumber, NumKey, Str};
use crate::model::para::Paragraph;
use crate::model::run::{BookmarkMark, Run, RunProps};
use crate::model::sources::{SourceStore, StoryGroup};
use crate::model::story::Story;
use crate::pvec::PVec;
use crate::read::plain::default_run_props;
use crate::text::Utf16;
use crate::word::obj;

// ───────────────────────── pure rules (references.ts) ─────────────────────────

/// `validBookmarkName(name)`: `/^\p{L}[\p{L}\p{N}_]*$/u` and at most 40 UTF-16 units — the first
/// character a letter, then letters, digits or `_`.
#[must_use]
pub fn valid_bookmark_name(name: &str) -> bool {
    let units = name.encode_utf16().count();
    if units == 0 || units > 40 {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    uo_text::props::is_letter(first as u32)
        && chars.all(|c| c == '_' || uo_text::props::is_letter(c as u32) || uo_text::props::is_number(c as u32))
}

/// `isHiddenBookmark(name)`: `_Ref`, `_Toc`, `_GoBack`… (only Word creates them).
#[must_use]
pub fn is_hidden_bookmark(name: &str) -> bool {
    name.starts_with('_')
}

/// `name.toLowerCase() === other.toLowerCase()`.
fn lower_eq(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(b)
    } else {
        uo_text::case::to_lowercase_str(a) == uo_text::case::to_lowercase_str(b)
    }
}

/// `BookmarkLoc`: a bookmark in the model, the two marks and where they are.
#[derive(Debug, Clone, PartialEq)]
pub struct BookmarkLoc {
    pub name: Str,
    pub id: JsNumber,
    pub start: DocPosition,
    /// no end mark found (the end is raw outside the paragraph): a point
    pub end: DocPosition,
}

/// `bookmarksIn(paras)`: the bookmarks (by their marks) of a paragraph list, in the order of their
/// starts.
#[must_use]
pub fn bookmarks_in<'a>(paras: impl IntoIterator<Item = &'a Paragraph>) -> Vec<BookmarkLoc> {
    let mut open: JsMap<NumKey, usize> = JsMap::default();
    let mut out: Vec<BookmarkLoc> = Vec::new();
    for (para, p) in paras.into_iter().enumerate() {
        let para = u32::try_from(para).unwrap_or(u32::MAX);
        let mut offset = 0usize;
        for r in &p.runs {
            if let Some(b) = r.ext().and_then(|e| e.bookmark.as_ref()) {
                let at = DocPosition::new(para, u32::try_from(offset).unwrap_or(u32::MAX));
                if b.end != Some(true) {
                    open.insert(NumKey::new(b.id.to_number()), out.len());
                    out.push(BookmarkLoc { name: b.name.clone(), id: b.id, start: at, end: at });
                } else if let Some(i) = open.shift_remove(&NumKey::new(b.id.to_number()))
                    && let Some(loc) = out.get_mut(i)
                {
                    loc.end = at;
                }
            }
            offset += r.text.len();
        }
    }
    out
}

/// `findBookmark(paras, name)`.
#[must_use]
pub fn find_bookmark<'a>(paras: impl IntoIterator<Item = &'a Paragraph>, name: &str) -> Option<BookmarkLoc> {
    bookmarks_in(paras).into_iter().find(|b| lower_eq(&b.name, name))
}

fn has_start_mark(paras: &PVec<Paragraph>, name: &str) -> bool {
    paras.iter().any(|p| {
        p.runs.iter().any(|r| r.ext().and_then(|e| e.bookmark.as_ref()).is_some_and(|b| b.end != Some(true) && lower_eq(&b.name, name)))
    })
}

/// `removeBookmarkMarks(paras, name)`: takes the marks of the bookmark off (the text stays).
pub fn remove_bookmark_marks(paras: &mut PVec<Paragraph>, name: &str) -> bool {
    let mut ids: Vec<NumKey> = Vec::new();
    for p in paras.iter() {
        for r in &p.runs {
            if let Some(b) = r.ext().and_then(|e| e.bookmark.as_ref())
                && b.end != Some(true)
                && lower_eq(&b.name, name)
            {
                let k = NumKey::new(b.id.to_number());
                if !ids.contains(&k) {
                    ids.push(k);
                }
            }
        }
    }
    if ids.is_empty() {
        return false;
    }
    let is_mark = |r: &Run| r.ext().and_then(|e| e.bookmark.as_ref()).is_some_and(|b| ids.contains(&NumKey::new(b.id.to_number())));
    for i in 0..paras.len() {
        if paras.get(i).is_some_and(|p| p.runs.iter().any(&is_mark))
            && let Some(p) = paras.make_mut(i)
        {
            p.runs.retain(|r| !is_mark(r));
        }
    }
    true
}

/// `mark(id, name, end, props)`: a zero-width bookmark run (inside a hyperlink it carries the
/// link's target).
fn mark_run(id: f64, name: &str, end: bool, props: Option<&RunProps>) -> Run {
    let mut p = default_run_props();
    if let Some(src) = props
        && let Some(link) = src.link.as_ref().filter(|l| !l.is_empty())
    {
        obj::set(&mut p, "link", |x| x.link = Some(link.clone()));
        if let Some(tip) = src.link_tip.as_ref().filter(|t| !t.is_empty()) {
            obj::set(&mut p, "linkTip", |x| x.link_tip = Some(tip.clone()));
        }
    }
    let mut run = Run { props: Arc::new(p), ..Run::default() };
    let ext = run.ext_mut();
    ext.bookmark = Some(BookmarkMark { id: JsNumber::Num(id), name: name.into(), end: end.then_some(true), ..BookmarkMark::default() });
    crate::js::JsObject::mark(ext, "bookmark");
    run
}

/// `addBookmarkMarks(paras, name, id, start, end)`: the marks of a new bookmark at `[start, end]`:
/// the start after the marks already at the point, the end before them (the new bookmark is
/// inside); a point bookmark has both together. Positions are checked first (D2a-T1).
pub fn add_bookmark_marks(
    paras: &mut PVec<Paragraph>,
    name: &str,
    id: f64,
    start: DocPosition,
    end: DocPosition,
) -> Result<(), super::super::text::OpError> {
    let (a, b) = if compare_pos(start, end).is_le() { (start, end) } else { (end, start) };
    if paras.get(a.para as usize).is_none() || paras.get(b.para as usize).is_none() {
        return Err(super::super::text::OpError::Position);
    }
    if compare_pos(a, b).is_eq() {
        let pa = paras.make_mut(a.para as usize).ok_or(super::super::text::OpError::Position)?;
        let i = insert_index(pa, a.offset as usize);
        let near = pa.runs.get(i).or_else(|| i.checked_sub(1).and_then(|k| pa.runs.get(k))).map(|r| Arc::clone(&r.props));
        let at = i.min(pa.runs.len());
        pa.runs.splice(at..at, [mark_run(id, name, false, near.as_deref()), mark_run(id, name, true, near.as_deref())]);
        return Ok(());
    }
    // the end first (the start's index does not move when it is another paragraph or comes before)
    let pb = paras.make_mut(b.para as usize).ok_or(super::super::text::OpError::Position)?;
    let ie = split_at(pb, b.offset as usize);
    let prev = ie.checked_sub(1).and_then(|k| pb.runs.get(k)).map(|r| Arc::clone(&r.props));
    let at = ie.min(pb.runs.len());
    pb.runs.insert(at, mark_run(id, name, true, prev.as_deref()));
    let pa = paras.make_mut(a.para as usize).ok_or(super::super::text::OpError::Position)?;
    let is = insert_index(pa, a.offset as usize);
    let next = pa.runs.get(is).map(|r| Arc::clone(&r.props));
    let at = is.min(pa.runs.len());
    pa.runs.insert(at, mark_run(id, name, false, next.as_deref()));
    Ok(())
}

/// `textBetween(paras, start, end)`: the text of `[start, end]` (objects left out, `\r` between
/// paragraphs).
#[must_use]
pub fn text_between(paras: &PVec<Paragraph>, start: DocPosition, end: DocPosition) -> Utf16 {
    let mut out: Vec<u16> = Vec::new();
    for pi in start.para..=end.para {
        let Some(p) = paras.get(pi as usize) else { break };
        let mut acc = 0usize;
        for r in &p.runs {
            let rs = acc;
            acc += r.text.len();
            let object = r.ext().is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some());
            if object {
                continue;
            }
            let from = if pi == start.para { start.offset as usize } else { 0 };
            let to = if pi == end.para { end.offset as usize } else { usize::MAX };
            let a = from.saturating_sub(rs).min(r.text.len());
            let b = to.saturating_sub(rs).min(r.text.len());
            if a < b {
                out.extend_from_slice(r.text.units().get(a..b).unwrap_or(&[]));
            }
        }
        if pi < end.para {
            out.push(13);
        }
    }
    Utf16::from_units(out)
}

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The ids (digit strings) of `/<w:bookmarkStart[^>]*w:id="(\d+)"/g` over a part's XML: the last
/// `w:id="digits"` of each tag wins (the greedy `[^>]*` backtracks) and the next search starts
/// after the match.
#[must_use]
pub fn bookmark_start_ids(xml: &str) -> Vec<&str> {
    const TAG: &[u8] = b"<w:bookmarkStart";
    const ATTR: &[u8] = b"w:id=\"";
    let b = xml.as_bytes();
    let mut out: Vec<&str> = Vec::new();
    let mut from = 0usize;
    while let Some(off) = b.get(from..).and_then(|s| s.windows(TAG.len()).position(|w| w == TAG)).map(|p| p + from) {
        let after = off + TAG.len();
        if b.get(after).is_some_and(|c| is_word_byte(*c)) {
            from = off + 1;
            continue;
        }
        let end = b.get(after..).and_then(|s| s.iter().position(|&c| c == b'>')).map_or(b.len(), |p| after + p);
        let region = b.get(after..end).unwrap_or(&[]);
        let mut best: Option<(usize, usize)> = None;
        for j in 1..region.len() {
            if region.get(j - 1).is_some_and(|c| !is_word_byte(*c)) && region.get(j..).is_some_and(|r| r.starts_with(ATTR)) {
                let ds = j + ATTR.len();
                let mut de = ds;
                while region.get(de).is_some_and(u8::is_ascii_digit) {
                    de += 1;
                }
                if de > ds && region.get(de) == Some(&b'"') {
                    best = Some((ds, de));
                }
            }
        }
        match best {
            Some((ds, de)) => {
                if let Some(d) = xml.get(after + ds..after + de) {
                    out.push(d);
                }
                from = after + de + 1;
            }
            None => from = off + 1,
        }
    }
    out
}

/// The largest id of [`bookmark_start_ids`], folded into `max` (`maxBookmarkId`'s scan of the raw XML).
pub fn max_bookmark_start_id(xml: &str, max: &mut f64) {
    for d in bookmark_start_ids(xml) {
        *max = uo_jsnum::math::max(*max, uo_jsnum::parse::to_number(d));
    }
}

/// `/^word\/(document|header\d*|footer\d*|footnotes|endnotes|comments)\.xml$/`.
#[must_use]
pub fn is_bookmark_part(name: &str) -> bool {
    let Some(stem) = name.strip_prefix("word/").and_then(|r| r.strip_suffix(".xml")) else { return false };
    let digits = |rest: &str| rest.bytes().all(|c| c.is_ascii_digit());
    matches!(stem, "document" | "footnotes" | "endnotes" | "comments")
        || stem.strip_prefix("header").is_some_and(digits)
        || stem.strip_prefix("footer").is_some_and(digits)
}

/// `unlinkRuns(p, from, to, hyperlinkStyle)`: takes the hyperlink off `[from, to]` of the
/// paragraph: the link, its tip and the Hyperlink character style.
pub fn unlink_runs(p: &mut Paragraph, from: usize, to: usize, hyperlink_style: &str) {
    split_at(p, to);
    let i1 = split_at(p, from);
    let mut acc = 0usize;
    for (i, r) in p.runs.iter_mut().enumerate() {
        let rs = acc;
        acc += r.text.len();
        if i < i1 || rs >= to {
            continue;
        }
        let mut props = (*r.props).clone();
        obj::delete(&mut props, "link");
        obj::delete(&mut props, "linkTip");
        if props.char_style.as_deref() == Some(hyperlink_style) {
            obj::delete(&mut props, "charStyle");
        }
        r.props = Arc::new(props);
    }
    normalize_paragraph(p);
}

// ───────────────────────── the document's stories ─────────────────────────

/// A story of `markStories()` with its place in the document (`key`: the order of the dialog's
/// "location" sort).
#[derive(Debug, Clone)]
pub struct MarkStory {
    pub loc: StoryLoc,
    pub key: f64,
    pub body: bool,
}

fn cells_stories(story: &Story, ti: usize, path: &mut Vec<CellStep>, key: f64, out: &mut Vec<MarkStory>, depth: usize) {
    let Some(a) = story.tables.get(ti) else { return };
    if depth >= 64 {
        return;
    }
    for (ri, row) in a.table.rows.iter().enumerate() {
        for (ci, cell) in row.cells.iter().enumerate() {
            path.push(CellStep {
                table: u32::try_from(ti).unwrap_or(u32::MAX),
                row: u32::try_from(ri).unwrap_or(u32::MAX),
                cell: u32::try_from(ci).unwrap_or(u32::MAX),
            });
            out.push(MarkStory { loc: StoryLoc::Cell(path.clone()), key, body: false });
            for nti in 0..cell.content.tables.len() {
                cells_stories(&cell.content, nti, path, key, out, depth + 1);
            }
            path.pop();
        }
    }
}

/// The nested textboxes of a paragraph (`r.textbox.paras`) for `allParagraphs()`.
fn max_id_in_paragraph(p: &Paragraph, max: &mut f64, depth: usize) {
    for r in &p.runs {
        let Some(e) = r.ext() else { continue };
        if let Some(b) = &e.bookmark
            && b.id.to_number() > *max
        {
            *max = b.id.to_number();
        }
        if depth < 32
            && let Some(tb) = &e.textbox
        {
            for q in tb.content.flat_paragraphs() {
                max_id_in_paragraph(q, max, depth + 1);
            }
        }
    }
}

fn max_id_in_story(s: &Story, max: &mut f64, depth: usize) {
    for p in s.paras.iter() {
        max_id_in_paragraph(p, max, depth);
    }
    if depth < 64 {
        for a in s.tables.iter() {
            for row in &a.table.rows {
                for cell in &row.cells {
                    max_id_in_story(&cell.content, max, depth + 1);
                }
            }
        }
    }
}

impl Session {
    /// `markStories()`: the lists of paragraphs that carry bookmark marks, in the document order of
    /// the dialog (the body; the tables' cells at their anchors; the notes; the header/footer parts).
    #[must_use]
    pub fn mark_stories(&self) -> Vec<MarkStory> {
        let mut out = vec![MarkStory { loc: StoryLoc::Body, key: -1.0, body: true }];
        let body = &self.doc.body;
        let mut path: Vec<CellStep> = Vec::new();
        for (ai, a) in body.tables.iter().enumerate() {
            cells_stories(body, ai, &mut path, f64::from(a.before_para) - 0.5, &mut out, 0);
        }
        let mut k = 1e9_f64;
        for (footnote, map) in [(true, &self.doc.notes.footnotes), (false, &self.doc.notes.endnotes)] {
            for id in map.keys() {
                out.push(MarkStory { loc: StoryLoc::Note { footnote, id: *id }, key: k, body: false });
                k += 1.0;
            }
        }
        let mut seen: Vec<crate::ids::HfPartId> = Vec::new();
        for s in self.doc.sections.iter() {
            for set in [&s.headers, &s.footers] {
                for slot in ["default", "first", "even"] {
                    if let Some(part) = set.get(slot).copied()
                        && !seen.contains(&part)
                    {
                        seen.push(part);
                        out.push(MarkStory { loc: StoryLoc::Hf(part), key: k, body: false });
                        k += 1.0;
                    }
                }
            }
        }
        out
    }

    /// `hasBookmark(name)`: a bookmark of that name (case-insensitive) anywhere.
    #[must_use]
    pub fn has_bookmark(&self, name: &str) -> bool {
        if self.bookmark_paras.iter().any(|(k, _)| lower_eq(k, name)) {
            return true;
        }
        self.mark_stories().iter().any(|s| story_at(&self.doc, &s.loc).is_some_and(|st| has_start_mark(&st.paras, name)))
    }

    /// `getBookmarks({ hidden })` in the dialog's "location" order (`sort: 'location'`) or, with
    /// `by_location` false, in discovery order — the by-name sort is a locale collation
    /// (`localeCompare('pt-BR', { sensitivity: 'accent' })`) the shell keeps (F6-D3).
    #[must_use]
    pub fn bookmarks(&self, hidden: bool, by_location: bool) -> Vec<BookmarkInfo> {
        let mut rows: Vec<(BookmarkInfo, Vec<f64>)> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for s in self.mark_stories() {
            let Some(st) = story_at(&self.doc, &s.loc) else { continue };
            for b in bookmarks_in(st.paras.iter()) {
                let low = uo_text::case::to_lowercase_str(&b.name);
                if seen.contains(&low) {
                    continue;
                }
                seen.push(low);
                let k = if s.body {
                    vec![f64::from(b.start.para), f64::from(b.start.offset)]
                } else {
                    vec![s.key, f64::from(b.start.para), f64::from(b.start.offset)]
                };
                let hidden_row = is_hidden_bookmark(&b.name);
                let text = text_between(&st.paras, b.start, b.end);
                rows.push((BookmarkInfo { name: b.name, hidden: hidden_row, text }, k));
            }
        }
        for (name, para) in self.bookmark_paras.iter() {
            let low = uo_text::case::to_lowercase_str(name);
            if seen.contains(&low) {
                continue;
            }
            seen.push(low);
            rows.push((
                BookmarkInfo { name: name.clone(), hidden: is_hidden_bookmark(name), text: Utf16::default() },
                vec![f64::from(para) - 0.25, 0.0],
            ));
        }
        let mut vis: Vec<(BookmarkInfo, Vec<f64>)> = rows.into_iter().filter(|(r, _)| hidden || !r.hidden).collect();
        if by_location {
            let cmp_k = |a: &[f64], b: &[f64]| {
                for i in 0..a.len().max(b.len()) {
                    let d = a.get(i).copied().unwrap_or(-1.0) - b.get(i).copied().unwrap_or(-1.0);
                    if d != 0.0 {
                        return d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal);
                    }
                }
                std::cmp::Ordering::Equal
            };
            vis.sort_by(|a, b| cmp_k(&a.1, &b.1));
        }
        vis.into_iter().map(|(r, _)| r).collect()
    }

    /// `newBookmarkId()`: a free `w:id` (above every bookmark of the document, the raw ones in the
    /// parts included), computed once per session.
    pub(crate) fn new_bookmark_id(&mut self) -> f64 {
        if self.next_bm_id < 0.0 {
            let mut max = -1.0_f64;
            // `allParagraphs()`: the body, the cells, the header/footer parts, the notes (textboxes inside)
            max_id_in_story(&self.doc.body, &mut max, 0);
            for h in self.doc.hf.iter() {
                max_id_in_story(&h.story, &mut max, 0);
            }
            for s in self.doc.notes.footnotes.values().chain(self.doc.notes.endnotes.values()) {
                max_id_in_story(s, &mut max, 0);
            }
            let names: Vec<String> = self.pkg.part_names().filter(|n| is_bookmark_part(n)).map(str::to_owned).collect();
            for n in names {
                if let Ok(Some(text)) = self.pkg.part_text(&n) {
                    max_bookmark_start_id(&text, &mut max);
                }
            }
            self.next_bm_id = max + 1.0;
        }
        let id = self.next_bm_id;
        self.next_bm_id += 1.0;
        id
    }

    /// `dropBookmark(name)`: takes the marks of the bookmark off every story (and the raw entry);
    /// whether anything was found, and whether it was outside the active story.
    pub(crate) fn drop_bookmark(&mut self, name: &str) -> (bool, bool) {
        let mut hit = false;
        let mut elsewhere = false;
        for s in self.mark_stories() {
            if !story_at(&self.doc, &s.loc).is_some_and(|st| has_start_mark(&st.paras, name)) {
                continue;
            }
            let active = match (story_at(&self.doc, &s.loc), self.story()) {
                (Some(a), Some(b)) => std::ptr::eq(a, b),
                _ => false,
            };
            if story_at_mut(&mut self.doc, &s.loc).is_some_and(|st| remove_bookmark_marks(&mut st.paras, name)) {
                hit = true;
                elsewhere |= !active;
            }
        }
        let keys: Vec<Str> = self.bookmark_paras.iter().filter(|(k, _)| lower_eq(k, name)).map(|(k, _)| k.clone()).collect();
        for k in keys {
            self.bookmark_paras.map_mut().shift_remove(&k);
            hit = true;
        }
        (hit, elsewhere)
    }

    fn insert_bookmark(&mut self, name: &str) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() || !valid_bookmark_name(name) {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        let (_, elsewhere) = self.drop_bookmark(name);
        let DocRange { start, end } = normalize_range(self.range());
        let id = self.new_bookmark_id();
        let Some(paras) = self.paras_mut() else {
            self.discard_step();
            return Err(CommandError::Position);
        };
        if let Err(e) = add_bookmark_marks(paras, name, id, start, end) {
            self.discard_step();
            return Err(e.into());
        }
        for p in start.para..=end.para {
            self.touch_para(p as usize);
        }
        if elsewhere {
            self.touch_all();
        }
        let mut cs = self.finish();
        cs.events.push(Event::BookmarksChange);
        Ok(Outcome::Done(cs))
    }

    fn delete_bookmark(&mut self, name: &str) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() || !self.has_bookmark(name) {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        let (_, elsewhere) = self.drop_bookmark(name);
        if elsewhere {
            self.touch_all();
        } else {
            self.touch_structure();
        }
        let mut cs = self.finish();
        cs.events.push(Event::BookmarksChange);
        Ok(Outcome::Done(cs))
    }

    // ── links ───────────────────────────────────────────────────────────────────────────────

    /// `canInsertField()`: not locked, not in the read view, not in a plain-text content control.
    #[must_use]
    pub fn can_insert_field(&self) -> bool {
        if self.paras().is_none() || self.locked() {
            return false;
        }
        let DocRange { start, end } = normalize_range(self.range());
        let at = if compare_pos(start, end).is_eq() { vec![end] } else { vec![start, end] };
        !self.link_in_plain_text_control_at(&at)
    }

    /// `inPlainTextControlAt(at)` (also `canInsertNote`, D2b-F5).
    pub(crate) fn link_in_plain_text_control_at(&self, at: &[DocPosition]) -> bool {
        let Some(store) = self.doc.sources.as_deref() else { return false };
        let Some(paras) = self.paras() else { return false };
        at.iter().any(|pos| {
            let Some(p) = paras.get(pos.para as usize) else { return false };
            let run_at = |o: i64| -> Option<&Run> {
                let mut acc = 0i64;
                for r in &p.runs {
                    let len = i64::try_from(r.text.len()).unwrap_or(i64::MAX);
                    if o >= acc && o < acc + len {
                        return Some(r);
                    }
                    acc += len;
                }
                None
            };
            let o = i64::from(pos.offset);
            in_plain_text_control(store, p, &[run_at(o - 1), run_at(o)])
        })
    }

    /// `linkGroupAt()`: the stretch of the hyperlink that holds the caret (or the first character
    /// of the selection): `(para, from, to)`.
    fn link_group_at(&self) -> Option<(u32, usize, usize)> {
        let at = normalize_range(self.range()).start;
        let p = self.paras()?.get(at.para as usize)?;
        let mut acc = 0usize;
        let mut spans: Vec<(usize, usize, Option<&str>)> = Vec::new();
        for r in &p.runs {
            spans.push((acc, acc + r.text.len(), r.props.link.as_deref()));
            acc += r.text.len();
        }
        let off = at.offset as usize;
        let hit = spans.iter().find(|s| s.2.is_some_and(|l| !l.is_empty()) && s.0 <= off && off <= s.1 && s.1 > s.0)?;
        let link = hit.2;
        let (mut from, mut to) = (hit.0, hit.1);
        for s in &spans {
            if s.2 == link && (s.1 == from || (s.0 < from && s.1 >= from)) {
                from = from.min(s.0);
            }
        }
        for s in &spans {
            if s.2 == link && s.0 <= to && s.1 > to {
                to = s.1;
            }
        }
        Some((at.para, from, to))
    }

    /// `applyRun(range, patch)` (no extra ranges in the session): tracked when tracking.
    fn apply_run_patch(&mut self, range: DocRange, patch: &RunProps) -> Result<(), CommandError> {
        if self.opts.tracking {
            let rev = self.new_rev();
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::track_apply_run_props(paras, range, patch, &rev)?;
        } else {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::apply_run_props(paras, range, patch)?;
        }
        let DocRange { start, end } = normalize_range(range);
        for p in start.para..=end.para {
            self.touch_para(p as usize);
        }
        Ok(())
    }

    /// `setLink(url)`.
    fn set_link(&mut self, url: Option<&str>) -> Result<Outcome, CommandError> {
        let url = url.filter(|u| !u.is_empty());
        if self.paras().is_none() || self.locked() || (url.is_some() && !self.can_insert_field()) {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        if self.has_selection() {
            // `applyRunProps(model, range, { link: url || undefined })`
            let mut patch = RunProps::default();
            match url {
                Some(u) => obj::set(&mut patch, "link", |p| p.link = Some(u.into())),
                None => obj::set_undefined(&mut patch, "link", |p| p.link = None),
            }
            let range = self.range();
            let DocRange { start, end } = normalize_range(range);
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::apply_run_props(paras, range, &patch)?;
            for p in start.para..=end.para {
                self.touch_para(p as usize);
            }
        } else if let Some(u) = url {
            let start = self.selection.focus;
            let units: Vec<u16> = u.encode_utf16().collect();
            let pos = self.insert_typed(start, &units, None)?;
            let mut patch = RunProps::default();
            obj::set(&mut patch, "link", |p| p.link = Some(u.into()));
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            guarded::apply_run_props(paras, DocRange::new(start, pos), &patch)?;
            self.selection = Selection::caret(pos);
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `insertHyperlink(opts)`.
    fn insert_hyperlink(&mut self, opts: &HyperlinkOptions) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() || !self.can_insert_field() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let mut link: String = opts.address.as_deref().map_or("", uo_jsnum::text::trim).to_owned();
        self.begin_step(false, &[]);
        if link.is_empty()
            && let Some(anchor) = opts.anchor.as_deref().filter(|a| !a.is_empty())
        {
            let para = para_of_item(anchor);
            if let Some(pi) = para.filter(|_| matches!(self.ctx, EditContext::Body)) {
                // (a missing paragraph returns with the step taken, as the TS)
                let Some(len) = self.paras().and_then(|ps| ps.get(pi as usize)).map(para_length) else {
                    return Ok(Outcome::Done(self.change_set()));
                };
                let len = u32::try_from(len).unwrap_or(u32::MAX);
                match self.ref_bookmark(pi, 0, len, "_Toc") {
                    Ok(name) => link = format!("#{name}"),
                    Err(e) => {
                        self.discard_step();
                        return Err(e);
                    }
                }
            } else {
                link = format!("#{anchor}");
            }
        }
        if link.is_empty() {
            self.discard_step();
            return Ok(Outcome::Done(self.change_set()));
        }
        let style = match self.builtin_style_id("Hyperlink") {
            Ok(s) => s,
            Err(e) => {
                self.discard_step();
                return Err(e);
            }
        };
        // `{ link, linkTip: opts.tip || undefined, charStyle: style }`
        let mut patch = RunProps::default();
        obj::set(&mut patch, "link", |p| p.link = Some(link.as_str().into()));
        match opts.tip.as_deref().filter(|t| !t.is_empty()) {
            Some(t) => obj::set(&mut patch, "linkTip", |p| p.link_tip = Some(t.into())),
            None => obj::set_undefined(&mut patch, "linkTip", |p| p.link_tip = None),
        }
        obj::set(&mut patch, "charStyle", |p| p.char_style = Some(style.clone()));
        let g = if self.has_selection() { None } else { self.link_group_at() };
        let mut range = match g {
            Some((para, from, to)) => DocRange::new(
                DocPosition::new(para, u32::try_from(from).unwrap_or(u32::MAX)),
                DocPosition::new(para, u32::try_from(to).unwrap_or(u32::MAX)),
            ),
            None => normalize_range(self.range()),
        };
        let units: Vec<u16> = {
            let p = self.paras().and_then(|ps| ps.get(range.start.para as usize)).ok_or(CommandError::Position)?;
            let all = super::super::text::para_text(p);
            let a = (range.start.offset as usize).min(all.len());
            let b = if range.start.para == range.end.para { (range.end.offset as usize).min(all.len()) } else { all.len() };
            all.get(a..b.max(a)).unwrap_or(&[]).to_vec()
        };
        let point = compare_pos(range.start, range.end).is_eq();
        let text: Vec<u16> = match &opts.text {
            Some(t) => t.units().to_vec(),
            None if point => opts.address.as_deref().or(opts.anchor.as_deref()).unwrap_or("").encode_utf16().collect(),
            None => units.clone(),
        };
        if point || (opts.text.is_some() && text != units) {
            // new (or changed) text goes in, already linked
            let mut at = range.start;
            if !point {
                self.selection = Selection { anchor: range.start, focus: range.end };
                at = self.delete_selection()?;
            }
            let carried = self.paras().map(|ps| props_at(ps, at)).unwrap_or_default();
            let props = CarriedProps { props: obj::spread(&carried.props, &patch), link: carried.link };
            let end = self.insert_typed(at, &text, Some(&props))?;
            range = DocRange::new(at, end);
        } else {
            self.apply_run_patch(range, &patch)?;
        }
        self.selection = Selection::caret(range.end);
        Ok(Outcome::Done(self.finish()))
    }

    /// `removeLink()`.
    fn remove_link(&mut self) -> Result<Outcome, CommandError> {
        if self.paras().is_none() || self.locked() {
            return Ok(Outcome::Done(self.change_set()));
        }
        let g = if self.has_selection() { None } else { self.link_group_at() };
        let range = match g {
            Some((para, from, to)) => DocRange::new(
                DocPosition::new(para, u32::try_from(from).unwrap_or(u32::MAX)),
                DocPosition::new(para, u32::try_from(to).unwrap_or(u32::MAX)),
            ),
            None => normalize_range(self.range()),
        };
        if compare_pos(range.start, range.end).is_eq() {
            return Ok(Outcome::Done(self.change_set()));
        }
        self.begin_step(false, &[]);
        let style = match self.builtin_style_id("Hyperlink") {
            Ok(s) => s,
            Err(e) => {
                self.discard_step();
                return Err(e);
            }
        };
        for pi in range.start.para..=range.end.para {
            let paras = self.paras_mut().ok_or(CommandError::Position)?;
            let Some(p) = paras.make_mut(pi as usize) else {
                self.discard_step();
                return Err(CommandError::Position);
            };
            let from = if pi == range.start.para { range.start.offset as usize } else { 0 };
            let to = if pi == range.end.para { range.end.offset as usize } else { para_length(p) };
            unlink_runs(p, from, to, &style);
            self.touch_para(pi as usize);
        }
        Ok(Outcome::Done(self.finish()))
    }

    pub(crate) fn exec_bookmarks(&mut self, cmd: BookmarkCmd, _cx: &mut ExecCx<'_>) -> Result<Outcome, CommandError> {
        match cmd {
            BookmarkCmd::Insert { name } => self.insert_bookmark(&name),
            BookmarkCmd::Delete { name } => self.delete_bookmark(&name),
            BookmarkCmd::InsertHyperlink(o) => self.insert_hyperlink(&o),
            BookmarkCmd::SetLink { url } => self.set_link(url.as_deref()),
            BookmarkCmd::RemoveLink => self.remove_link(),
        }
    }
}

/// `paraOfItem(id)` as the TS number: `/^p:(\d+)$/` then `+m[1]` (`None` is −1).
#[must_use]
pub fn para_of_item_number(id: &str) -> Option<f64> {
    let digits = id.strip_prefix("p:")?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(uo_jsnum::parse::to_number(digits))
}

/// The paragraph index `paraOfItem` names, when it is one (a number past `u32` names no paragraph).
fn para_of_item(id: &str) -> Option<u32> {
    let v = para_of_item_number(id)?;
    if v <= f64::from(u32::MAX) { Some(uo_jsnum::conv::to_uint32(v)) } else { None }
}

// ───────────────────────── plain-text content controls ─────────────────────────

/// `plainTextSdt(open)`: the opening of a `w:sdt` Word treats as unformatted text (text, date,
/// combo box, drop-down list).
pub fn plain_text_sdt(open: &str) -> bool {
    let Some(rest) = open.strip_prefix("<w:sdt") else { return false };
    if !rest.chars().next().is_some_and(|c| c == '>' || uo_jsnum::text::is_js_space(c)) {
        return false;
    }
    let hay = open.find("</w:sdtPr>").map_or(open, |e| open.get(..e).unwrap_or(open));
    let b = hay.as_bytes();
    let mut from = 0usize;
    while let Some(i) = hay.get(from..).and_then(|s| s.find("<w:")).map(|p| p + from) {
        let rest = b.get(i + 3..).unwrap_or(&[]);
        for name in ["text", "date", "comboBox", "dropDownList"] {
            if rest.starts_with(name.as_bytes()) && !rest.get(name.len()).is_some_and(|c| is_word_byte(*c)) {
                return true;
            }
        }
        from = i + 1;
    }
    false
}

fn in_groups(store: &SourceStore, groups: &[StoryGroup], id: f64, depth: usize) -> bool {
    for g in groups {
        if !g.blocks.iter().any(|b| b.to_number() == id) {
            continue;
        }
        if g.wrap_open.as_ref().is_some_and(|w| plain_text_sdt(w.resolve(&store.parts)))
            || (depth < 64 && g.inner.as_deref().is_some_and(|inner| in_groups(store, inner, id, depth + 1)))
        {
            return true;
        }
    }
    false
}

/// `SourceStore.inPlainTextControl(p, around)`.
fn in_plain_text_control(store: &SourceStore, p: &Paragraph, around: &[Option<&Run>]) -> bool {
    let Some(id) = p.props.src else { return false };
    let id_n = f64::from(id.0);
    if store.stories.values().any(|s| in_groups(store, &s.groups, id_n, 0))
        || store.tcs.values().any(|c| c.groups.as_deref().is_some_and(|g| in_groups(store, g, id_n, 0)))
        || store.txbx_groups.iter().any(|g| in_groups(store, g, id_n, 0))
    {
        return true;
    }
    let sids: Vec<u32> = around.iter().filter_map(|r| r.and_then(|r| r.props.src)).map(|k| k.0).collect();
    !sids.is_empty()
        && store.paras.get(&id).is_some_and(|ps| {
            ps.segs.iter().any(|s| {
                s.wrap_open.as_ref().is_some_and(|w| plain_text_sdt(w.resolve(&store.parts)))
                    && s.sids.iter().any(|x| sids.iter().any(|sid| f64::from(*sid) == x.to_number()))
            })
        })
}

// ───────────────────────── the commands ─────────────────────────

/// A bookmark of the dialog (`BookmarkInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct BookmarkInfo {
    pub name: Str,
    pub hidden: bool,
    /// the text of the stretch (`\r` between paragraphs)
    pub text: Utf16,
}

/// `HyperlinkOptions`: an address (web, file, `mailto:`) or an in-document destination (a
/// bookmark name, `_top`, or `p:<paragraph>` for a heading), the screen tip and the display text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HyperlinkOptions {
    pub address: Option<Str>,
    pub anchor: Option<Str>,
    pub tip: Option<Str>,
    pub text: Option<Utf16>,
}

/// The commands of this family.
#[derive(Debug, Clone, PartialEq)]
pub enum BookmarkCmd {
    /// `insertBookmark(name)` at the selection (a repeated name moves the bookmark)
    Insert { name: Str },
    /// `deleteBookmark(name)`
    Delete { name: Str },
    /// `insertHyperlink(opts)`
    InsertHyperlink(HyperlinkOptions),
    /// `setLink(url)`: `None` removes the link of the selection
    SetLink { url: Option<Str> },
    /// `removeLink()`
    RemoveLink,
}

impl BookmarkCmd {
    /// The TS method the formatting policy guards (`POLICY_GUARDS`) and the argument it reads.
    pub(crate) fn guard(&self) -> Option<(&'static str, GuardArg<'_>)> {
        match self {
            BookmarkCmd::InsertHyperlink(_) => Some(("insertHyperlink", GuardArg::None)),
            BookmarkCmd::SetLink { .. } => Some(("setLink", GuardArg::None)),
            _ => None,
        }
    }
}
