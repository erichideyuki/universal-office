//! The editor's caret, click and selection geometry over a layout (frontier F8): the exported
//! `caretXIn`, `offsetAtX`, `isSortedByX` of the editor (engine-doc.ts:371–400) and the
//! universal-write.ts methods that call them over the engine lines — `installEngineResult` (the
//! line index with `collapseCustomChars`), `rebuildEditLines`/`rebuildTextboxLines` (the lines of
//! the paragraphs being edited), `hitTestRaw` (engine branch) + `hitLineX`, `lineOf` + `caretX`
//! (`caretRectAt` in page twips), `paintEngineSelection`, `engLineAt`, `cellBoxAt` and the find
//! list's `pageOf`.
//!
//! The shell keeps choosing the page under the pointer (canvas rectangles, zoom); these take page
//! twips. [`EditLines`] and [`LineIndex`] are built once per layout (they copy what they need);
//! the queries on them do not allocate (M1/M3 of the record: a hit test allocates nothing).

use rustc_hash::FxHashMap;

use crate::model::{DocView, DrawingId, HfPartId, HfWhich, NoteKind, NumKey, Paragraph, StoryRef};
use crate::output::{CharsSoA, Layout, LayoutPage, LineGeom, LineStory};
use crate::refs::{LocKind, Located, Locator, ParaLoc, para_length, path_in};
use uo_jsnum::math as jm;

// ───────────────────────── characters of a line ─────────────────────────

/// The `chars` of one line (`{ o, x, w }` columns of equal length).
#[derive(Clone, Copy, Debug)]
pub struct CharsView<'a> {
    pub o: &'a [u32],
    pub x: &'a [f64],
    pub w: &'a [f64],
}

impl<'a> CharsView<'a> {
    pub const EMPTY: CharsView<'static> = CharsView { o: &[], x: &[], w: &[] };

    /// The characters of a page line (`line.chars`, absent = empty).
    #[must_use]
    pub fn of(page: &'a LayoutPage, line: &LineGeom) -> CharsView<'a> {
        line.chars.and_then(|s| s.range()).map_or(CharsView::EMPTY, |r| CharsView::slice(&page.chars, r))
    }

    fn slice(c: &'a CharsSoA, r: std::ops::Range<usize>) -> CharsView<'a> {
        match (c.o.get(r.clone()), c.x.get(r.clone()), c.w.get(r)) {
            (Some(o), Some(x), Some(w)) => CharsView { o, x, w },
            _ => CharsView::EMPTY,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.o.len().min(self.x.len()).min(self.w.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn at(&self, k: usize) -> (u32, f64, f64) {
        (self.o.get(k).copied().unwrap_or(0), self.x.get(k).copied().unwrap_or(f64::NAN), self.w.get(k).copied().unwrap_or(f64::NAN))
    }
}

/// `caretXIn(line, offset)` (engine-doc.ts:371): the x of the caret before the character at
/// `offset` — the character's x; else the end of the last character before it (the rightmost of
/// equal offsets); else the leftmost character; `line.x` without characters.
#[must_use]
pub fn caret_x_in(cs: CharsView<'_>, line_x: f64, offset: f64) -> f64 {
    let n = cs.len();
    if n == 0 {
        return line_x;
    }
    let mut prev: Option<(f64, f64, f64)> = None;
    for k in 0..n {
        let (o, x, w) = cs.at(k);
        let o = f64::from(o);
        #[expect(clippy::float_cmp, reason = "the TS compares with ===")]
        if o == offset {
            return x;
        }
        let better = match prev {
            None => true,
            #[expect(clippy::float_cmp, reason = "the TS compares with ===")]
            Some((po, px, _)) => o > po || (o == po && x > px),
        };
        if o < offset && better {
            prev = Some((o, x, w));
        }
    }
    if let Some((_, x, w)) = prev {
        return x + w;
    }
    let mut first = cs.at(0).1;
    for k in 0..n {
        let x = cs.at(k).1;
        if x < first {
            first = x;
        }
    }
    first
}

/// `isSortedByX(cs)` (engine-doc.ts:397).
#[must_use]
pub fn is_sorted_by_x(xs: &[f64]) -> bool {
    xs.windows(2).all(|p| match p {
        #[expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `cs[i].x < cs[i - 1].x` (NaN counts as in order)")]
        [a, b] => !(b < a),
        _ => true,
    })
}

/// `offsetAtX(line, x, end)` (engine-doc.ts:386): the offset of the character nearest to `x` —
/// the first character, in x order, whose middle is right of `x`; `end` past them all;
/// `line.start` without characters. The TS sorts a copy (stable, by `a.x - b.x`) when the
/// characters are not in x order; here the same element is found without the copy: the least
/// `(x, index)` among the candidates (F8-3).
#[must_use]
pub fn offset_at_x(cs: CharsView<'_>, line_start: u32, x: f64, end: u32) -> u32 {
    let n = cs.len();
    if n == 0 {
        return line_start;
    }
    if is_sorted_by_x(cs.x.get(..n).unwrap_or(&[])) {
        for k in 0..n {
            let (o, cx, cw) = cs.at(k);
            if x < cx + cw / 2.0 {
                return o;
            }
        }
        return end;
    }
    let mut best: Option<(f64, u32)> = None;
    for k in 0..n {
        let (o, cx, cw) = cs.at(k);
        if x < cx + cw / 2.0 && best.is_none_or(|(bx, _)| cx < bx) {
            best = Some((cx, o));
        }
    }
    best.map_or(end, |(_, o)| o)
}

/// `collapseCustomChars(line, para)` (universal-write.ts:953) = `collapseCustomInlineChars`
/// (run-boxes.ts:34): the characters of a plugin inline's fallback text become one character
/// (`{ o: a, x: first.x, w: last.x + last.w - first.x }`). In place over owned columns.
pub fn collapse_custom_chars(c: &mut CharsSoA, para: &Paragraph) {
    let mut acc: u32 = 0;
    for r in &para.runs {
        let a = acc;
        acc = acc.wrapping_add(u32::try_from(r.text.len()).unwrap_or(u32::MAX));
        let Some(custom) = r.ext().and_then(|e| e.custom.as_deref()) else { continue };
        // `Array.from(fallbackText || ' ').length`: code points
        let fb = custom.fallback_text.units();
        let n = if fb.is_empty() { 1 } else { code_points(fb) }.max(1);
        let Some(i) = c.o.iter().position(|&o| o == a) else { continue };
        let len = c.o.len().min(c.x.len()).min(c.w.len());
        let mut j = i;
        while j + 1 < len && j - i + 1 < n && c.o.get(j + 1).copied() == c.o.get(j).map(|o| o.wrapping_add(1)) {
            j += 1;
        }
        let (Some(&x0), Some(&xl), Some(&wl)) = (c.x.get(i), c.x.get(j), c.w.get(j)) else { continue };
        let w = xl + wl - x0;
        c.o.splice(i..=j, [a]);
        c.x.splice(i..=j, [x0]);
        c.w.splice(i..=j, [w]);
    }
}

fn code_points(u: &[u16]) -> usize {
    let mut n = 0;
    let mut k = 0;
    while k < u.len() {
        let hi = u.get(k).copied().unwrap_or(0);
        let lo = u.get(k + 1).copied().unwrap_or(0);
        k += if (0xd800..0xdc00).contains(&hi) && (0xdc00..0xe000).contains(&lo) { 2 } else { 1 };
        n += 1;
    }
    n
}

/// `xfInvert(m, x, y)` (universal-write.ts:555): a page point in the text frame of a rotated box.
#[must_use]
pub fn xf_invert(m: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    let [a, b, c, d, e, f] = *m;
    let det = a * d - b * c;
    let (dx, dy) = (x - e, y - f);
    ((d * dx - c * dy) / det, (-b * dx + a * dy) / det)
}

/// `dy * 4 + dx`: the distance of a point to a line box (vertical band first).
fn band_distance(px: f64, py: f64, x0: f64, x1: f64, y: f64, h: f64) -> f64 {
    let dy = if py < y {
        y - py
    } else if py > y + h {
        py - y - h
    } else {
        0.0
    };
    let dx = if px < x0 {
        x0 - px
    } else if px > x1 {
        px - x1
    } else {
        0.0
    };
    dy * 4.0 + dx
}

// ───────────────────────── the line index (`eng.lines`) ─────────────────────────

/// `eng.lines` of the editor (`installEngineResult`): every page line with its location and
/// paragraph, page by page.
#[derive(Debug, Default)]
pub struct LineIndex {
    /// per page, per line of `page.lines`
    pages: Vec<Vec<Located>>,
}

impl LineIndex {
    /// Locates every line of `layout` (`locateRef` + `lineParagraph`).
    #[must_use]
    pub fn new(loc: &Locator<'_>, layout: &Layout) -> LineIndex {
        let pages = layout.pages.iter().map(|p| p.lines.iter().map(|l| loc.locate(p.section, &l.at)).collect()).collect();
        LineIndex { pages }
    }

    /// The location of line `line` of page `page`.
    #[must_use]
    pub fn located(&self, page: u32, line: u32) -> Option<&Located> {
        self.pages.get(page as usize)?.get(line as usize)
    }

    /// `engLineAt` (universal-write.ts:18689) on page `page`: the line nearest to `(x, y)` (the
    /// vertical band first, then the horizontal distance), lines of kind `other` skipped; the
    /// index in `page.lines`.
    #[must_use]
    pub fn line_at(&self, layout: &Layout, page: u32, x: f64, y: f64) -> Option<u32> {
        let p = layout.pages.get(page as usize)?;
        let locs = self.pages.get(page as usize)?;
        let mut best = None;
        let mut best_d = f64::INFINITY;
        for (k, (l, at)) in p.lines.iter().zip(locs).enumerate() {
            if at.kind == LocKind::Other {
                continue;
            }
            let d = band_distance(x, y, l.x, l.x + l.width, l.y, l.height);
            if d < best_d {
                best_d = d;
                best = u32::try_from(k).ok();
            }
        }
        best
    }

    /// The find list's `pageOf(p, off)` (universal-write.ts:17874): the last page holding a line
    /// of the paragraph that starts at or before `off`.
    #[must_use]
    pub fn page_of(&self, layout: &Layout, para: &ParaLoc, off: f64) -> Option<u32> {
        let mut best = None;
        for (pg, (p, locs)) in layout.pages.iter().zip(&self.pages).enumerate() {
            for (l, at) in p.lines.iter().zip(locs) {
                if at.para.as_ref() == Some(para) && f64::from(l.start) <= off {
                    best = u32::try_from(pg).ok();
                }
            }
        }
        best
    }
}

/// `cellBoxAt` (universal-write.ts:20255) on page `page`: the innermost body table cell box
/// holding `(x, y)`; the index in `page.cells`.
#[must_use]
pub fn cell_at(loc: &Locator<'_>, layout: &Layout, page: u32, x: f64, y: f64) -> Option<u32> {
    let p = layout.pages.get(page as usize)?;
    let mut best: Option<(u32, usize)> = None;
    for (k, b) in p.cells.as_deref().unwrap_or(&[]).iter().enumerate() {
        if x < b.x || x > b.x + b.w || y < b.y || y > b.y + b.h {
            continue;
        }
        if b.at.story != LineStory::Body {
            continue;
        }
        let Some(hops) = loc.cell_hops(&b.at.path.0) else { continue };
        if best.is_none_or(|(_, n)| hops > n) {
            best = Some((u32::try_from(k).ok()?, hops));
        }
    }
    best.map(|(k, _)| k)
}

// ───────────────────────── the lines being edited ─────────────────────────

/// Which paragraphs the editor is editing (the `this.model.paragraphs` of `rebuildEditLines`).
#[derive(Clone, Debug)]
pub enum EditScope<'s> {
    /// the body's top-level paragraphs
    Body,
    /// a body table cell's top-level paragraphs: the table at `table`, row `row`, the cell's
    /// index `cell` in the row (`cellEdit`)
    Cell { table: crate::model::BlockPath, row: u32, cell: u32 },
    /// a header/footer part's paragraphs, cells included (the part's flat paragraph array is the
    /// model of `hfEdit`), on page `page` (`hfActivePage`)
    HeaderFooter { which: HfWhich, part: HfPartId, page: u32 },
    /// a note's paragraphs, cells included (`noteEdit`: `flattenBlocks`)
    Note { kind: NoteKind, id: NumKey },
    /// a textbox chain's text: `chain[0]` is the box whose content is edited (`tbEdit`: its flat
    /// `paras`, cells included); `page`
    /// = the header/footer page when the box is in a header or footer
    Textbox { chain: &'s [DrawingId], page: Option<u32> },
}

/// A position in the edited paragraphs (`DocPosition`: paragraph index in the scope, UTF-16
/// offset).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditPos {
    pub para: u32,
    pub offset: u32,
}

/// One line of an edited paragraph (`LaidLine` with its engine line).
#[derive(Clone, Debug)]
pub struct EditLine {
    pub page: u32,
    /// `rich.start`, `rich.end` (clamped to the paragraph; the first line from 0, the last to
    /// the paragraph's length)
    pub start: u32,
    pub end: u32,
    /// the engine line (`eng`): `x`, `y`, `height`, `baseline`, `width`, `start` (shifted onto the
    /// page for a textbox without rotation)
    pub x: f64,
    pub y: f64,
    pub height: f64,
    pub baseline: f64,
    pub width: f64,
    pub line_start: u32,
    /// the text → page matrix of a rotated or vertical textbox (`xf`)
    pub xf: Option<[f64; 6]>,
    /// the engine line's characters in [`EditLines`]' own columns
    chars: std::ops::Range<usize>,
}

/// One edited paragraph and its lines.
#[derive(Clone, Debug)]
pub struct EditPara {
    pub at: ParaLoc,
    /// `paraLength(p)`
    pub len: u32,
    pub lines: Vec<EditLine>,
}

/// A caret: page, x, top and height in page twips (in the text frame when `xf` is set).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretRect {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub xf: Option<[f64; 6]>,
}

/// A selection rectangle (`fillRect(x1, y, max(0, x2 - x1), h)`, in the text frame when `xf`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub xf: Option<[f64; 6]>,
}

/// `layoutCache` of the editor: the lines of the edited paragraphs (`rebuildEditLines`).
#[derive(Clone, Debug, Default)]
pub struct EditLines {
    pub paras: Vec<EditPara>,
    /// the header/footer page in use (`hfActivePage` after the fallback), or the textbox page
    pub page: Option<u32>,
    chars: CharsSoA,
}

impl EditLines {
    /// `rebuildEditLines()` / `rebuildTextboxLines()` for `scope` over `layout` (whose lines
    /// `index` located).
    #[must_use]
    pub fn new(loc: &Locator<'_>, layout: &Layout, index: &LineIndex, scope: &EditScope<'_>) -> EditLines {
        let view = loc.view;
        let mut out = EditLines::default();
        let paras = scope_paras(loc, scope);
        let mut at: FxHashMap<ParaLoc, usize> = FxHashMap::default();
        for (i, p) in paras.iter().enumerate() {
            at.entry(p.clone()).or_insert(i);
        }
        out.paras = paras
            .into_iter()
            .map(|p| {
                let len = crate::refs::paragraph(view, &p).map_or(0, para_length);
                EditPara { at: p, len, lines: Vec::new() }
            })
            .collect();
        if let EditScope::Textbox { chain, page } = scope {
            out.textbox_lines(view, layout, &at, chain, *page);
            out.finish();
            return out;
        }
        // the header/footer page: the asked one if it shows the part, else the first that does
        let hf = match scope {
            EditScope::HeaderFooter { which, page, .. } => {
                let shows = |pg: usize| -> bool {
                    let (Some(p), Some(locs)) = (layout.pages.get(pg), index.pages.get(pg)) else { return false };
                    p.lines.iter().zip(locs).any(|(l, a)| hf_line(l, *which) && a.para.as_ref().is_some_and(|q| at.contains_key(q)))
                };
                let mut pg = *page;
                if !shows(pg as usize) {
                    'find: for (k, (p, locs)) in layout.pages.iter().zip(&index.pages).enumerate() {
                        for (l, a) in p.lines.iter().zip(locs) {
                            if hf_line(l, *which) && a.para.as_ref().is_some_and(|q| at.contains_key(q)) {
                                pg = u32::try_from(k).unwrap_or(u32::MAX);
                                break 'find;
                            }
                        }
                    }
                }
                out.page = Some(pg);
                Some((*which, pg))
            }
            _ => None,
        };
        // F8-15: `installEngineResult` collapses plugin-inline characters IN PLACE, once per
        // line, and the lines the engine shifted from one source line (`shiftLine`: a repeated
        // table header) share one `chars` array — so that array is collapsed once per sharer.
        // The sharers: the page lines of such a paragraph with the same ref, offsets and
        // characters (identity is not kept by the layout).
        let mut shared: FxHashMap<ShareKey, usize> = FxHashMap::default();
        for (p, locs) in layout.pages.iter().zip(&index.pages) {
            for (l, a) in p.lines.iter().zip(locs) {
                if a.para.as_ref().and_then(|q| crate::refs::paragraph(view, q)).is_some_and(has_custom) {
                    *shared.entry(share_key(p, l)).or_insert(0) += 1;
                }
            }
        }
        for (pg, (p, locs)) in layout.pages.iter().zip(&index.pages).enumerate() {
            let pg32 = u32::try_from(pg).unwrap_or(u32::MAX);
            for (l, a) in p.lines.iter().zip(locs) {
                let pi = match (scope, hf) {
                    (EditScope::HeaderFooter { .. }, Some((which, hpg))) => {
                        if pg32 == hpg && hf_line(l, which) {
                            a.para.as_ref().and_then(|q| at.get(q).copied())
                        } else {
                            None
                        }
                    }
                    (EditScope::Note { .. }, _) => {
                        if a.kind == LocKind::Note {
                            a.para.as_ref().and_then(|q| at.get(q).copied())
                        } else {
                            None
                        }
                    }
                    (EditScope::Body, _) => {
                        if a.kind == LocKind::Body {
                            a.para.as_ref().and_then(|q| at.get(q).copied())
                        } else {
                            None
                        }
                    }
                    (EditScope::Cell { .. }, _) => {
                        if a.kind == LocKind::Cell {
                            a.para.as_ref().and_then(|q| at.get(q).copied())
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let Some(pi) = pi else { continue };
                // `installEngineResult` collapsed the plugin inlines of the line's paragraph
                let custom = a.para.as_ref().and_then(|q| crate::refs::paragraph(view, q)).filter(|q| has_custom(q));
                let times = if custom.is_some() { shared.get(&share_key(p, l)).copied().unwrap_or(1) } else { 0 };
                out.push_line(pi, pg32, p, l, LineCopy::Page(custom.map(|q| (q, times))));
            }
        }
        out.finish();
        out
    }

    /// `rebuildTextboxLines()`: the content lines of every box of the chain (on the header/footer
    /// page only, when given), shifted onto the page when the box is not rotated.
    fn textbox_lines(
        &mut self,
        view: DocView<'_>,
        layout: &Layout,
        at: &FxHashMap<ParaLoc, usize>,
        chain: &[DrawingId],
        hf_page: Option<u32>,
    ) {
        let Some(&head) = chain.first() else { return };
        let Some(head_story) = view.find_textbox(head).map(|t| &t.content) else { return };
        self.page = hf_page;
        for (pg, p) in layout.pages.iter().enumerate() {
            let pg32 = u32::try_from(pg).unwrap_or(u32::MAX);
            if hf_page.is_some_and(|h| h != pg32) {
                continue;
            }
            for o in p.objects.as_deref().unwrap_or(&[]) {
                let Some((range, m)) = &o.content else { continue };
                // `o.run.textbox` in the chain (a group's run has no `textbox`)
                let tb = o.run.as_ref().and_then(|r| view.run(r)).and_then(|r| r.ext()).and_then(|e| e.textbox.as_deref());
                if !tb.is_some_and(|t| chain.contains(&t.drawing)) {
                    continue;
                }
                let [a, b, c, d, e, f] = *m;
                let plain = jm::abs(a - 1.0) < 1e-9 && jm::abs(d - 1.0) < 1e-9 && jm::abs(b) < 1e-9 && jm::abs(c) < 1e-9;
                for li in range.clone() {
                    let Some(l) = p.content_lines.get(li as usize) else { continue };
                    let Some(path) = path_in(head_story, &l.at.path.0) else { continue };
                    let Some(&pi) = at.get(&ParaLoc { story: StoryRef::Textbox(head), path }) else { continue };
                    self.push_line(pi, pg32, p, l, if plain { LineCopy::Shift(e, f) } else { LineCopy::Xf(*m) });
                }
            }
        }
    }

    /// Pushes a line unless the paragraph already has one with the same `start`/`end` ("a
    /// repeated line — a table header on every page — keeps the first"; the textbox lines also
    /// compare the page).
    fn push_line(&mut self, pi: usize, page: u32, p: &LayoutPage, l: &LineGeom, how: LineCopy<'_>) {
        let textbox = !matches!(how, LineCopy::Page(_));
        let Some(para) = self.paras.get(pi) else { return };
        if para.lines.iter().any(|x| x.start == l.start && x.end == l.end && (!textbox || x.page == page)) {
            return;
        }
        let len = para.len;
        let clamp = |o: u32| o.min(len);
        // the engine line's characters, collapsed (plugin inlines) or shifted (textbox)
        let cs = CharsView::of(p, l);
        let s = self.chars.o.len();
        match how {
            LineCopy::Shift(dx, _) => self.chars.x.extend(cs.x.iter().map(|x| x + dx)),
            _ => self.chars.x.extend_from_slice(cs.x),
        }
        self.chars.o.extend_from_slice(cs.o);
        self.chars.w.extend_from_slice(cs.w);
        if let LineCopy::Page(Some((q, times))) = how {
            let mut own = CharsSoA { o: self.chars.o.split_off(s), x: self.chars.x.split_off(s), w: self.chars.w.split_off(s) };
            for _ in 0..times {
                collapse_custom_chars(&mut own, q);
            }
            self.chars.o.append(&mut own.o);
            self.chars.x.append(&mut own.x);
            self.chars.w.append(&mut own.w);
        }
        let e = self.chars.o.len();
        // `shiftEngLine(l, dx, dy)`: x, y, baseline and the characters' x, always added
        let (x, y, baseline, xf) = match how {
            LineCopy::Shift(dx, dy) => (l.x + dx, l.y + dy, l.baseline + dy, None),
            LineCopy::Xf(m) => (l.x, l.y, l.baseline, Some(m)),
            LineCopy::Page(_) => (l.x, l.y, l.baseline, None),
        };
        let Some(para) = self.paras.get_mut(pi) else { return };
        para.lines.push(EditLine {
            page,
            start: clamp(l.start),
            end: clamp(l.end),
            x,
            y,
            height: l.height,
            baseline,
            width: l.width,
            line_start: l.start,
            xf,
            chars: s..e,
        });
    }

    /// "The whole paragraph is covered by its lines (hidden text at the start or end)".
    fn finish(&mut self) {
        for p in &mut self.paras {
            let len = p.len;
            if let Some(f) = p.lines.first_mut() {
                f.start = 0;
            }
            if let Some(l) = p.lines.last_mut() {
                l.end = len;
            }
        }
    }

    /// The characters of an edited line.
    #[must_use]
    pub fn chars(&self, line: &EditLine) -> CharsView<'_> {
        CharsView::slice(&self.chars, line.chars.clone())
    }

    /// The paragraph of an edit position.
    #[must_use]
    pub fn para(&self, pos: EditPos) -> Option<&EditPara> {
        self.paras.get(pos.para as usize)
    }

    /// `lineOf(pos)` (universal-write.ts:13300): the line holding the offset (at a line end, the
    /// next line unless it is the last), else the paragraph's last line.
    #[must_use]
    pub fn line_of(&self, pos: EditPos) -> Option<&EditLine> {
        let pl = self.paras.get(pos.para as usize)?;
        let n = pl.lines.len();
        for (k, l) in pl.lines.iter().enumerate() {
            if pos.offset >= l.start && pos.offset <= l.end {
                if pos.offset == l.end && k + 1 != n {
                    continue;
                }
                return Some(l);
            }
        }
        pl.lines.last()
    }

    /// `caretX(pos, line)` over the engine line (`caretXIn`).
    #[must_use]
    pub fn caret_x(&self, offset: u32, line: &EditLine) -> f64 {
        caret_x_in(self.chars(line), line.x, f64::from(offset))
    }

    /// `hitLineX(line, x)`: `offsetAtX` within the line's `[start, end]`.
    #[must_use]
    pub fn hit_line_x(&self, line: &EditLine, x: f64) -> u32 {
        let o = offset_at_x(self.chars(line), line.line_start, x, line.end);
        line.start.max(line.end.min(o))
    }

    /// `caretRectAt(pos)` in page twips: the line's page, the caret x, the line's top and height.
    #[must_use]
    pub fn caret(&self, pos: EditPos) -> Option<CaretRect> {
        let l = self.line_of(pos)?;
        Some(CaretRect { page: l.page, x: self.caret_x(pos.offset, l), y: l.y, h: l.height, xf: l.xf })
    }

    /// `hitTestRaw` (universal-write.ts:15859, engine branch) on page `page`: the line nearest
    /// to `(x, y)` (in the text frame of a rotated box) and the offset under `x`; with no edited
    /// line on the page, `{ para: 0, offset: 0 }`.
    #[must_use]
    pub fn hit(&self, page: u32, x: f64, y: f64) -> EditPos {
        let mut best: Option<(usize, &EditLine)> = None;
        let mut best_d = f64::INFINITY;
        let mut best_x = x;
        for (pi, p) in self.paras.iter().enumerate() {
            for l in &p.lines {
                if l.page != page {
                    continue;
                }
                let (px, py) = match &l.xf {
                    Some(m) => xf_invert(m, x, y),
                    None => (x, y),
                };
                let d = band_distance(px, py, l.x, l.x + l.width, l.y, l.height);
                if d < best_d {
                    best_d = d;
                    best = Some((pi, l));
                    best_x = px;
                }
            }
        }
        match best {
            Some((pi, l)) => EditPos { para: u32::try_from(pi).unwrap_or(u32::MAX), offset: self.hit_line_x(l, best_x) },
            None => EditPos { para: 0, offset: 0 },
        }
    }

    /// `paintEngineSelection(ctx, page, { start, end })` (universal-write.ts:18602): the
    /// selection rectangles of page `page` (`start` ≤ `end`), appended to `out`.
    pub fn selection_rects(&self, page: u32, start: EditPos, end: EditPos, out: &mut Vec<SelRect>) {
        let mut pi = start.para;
        while pi <= end.para {
            let Some(pl) = self.paras.get(pi as usize) else { break };
            let n = pl.lines.len();
            for (k, l) in pl.lines.iter().enumerate() {
                if l.page != page {
                    continue;
                }
                let from = l.start.max(if pi == start.para { start.offset } else { 0 });
                let to = l.end.min(if pi == end.para { end.offset } else { l.end });
                // a whole line selected past its end: a little room, like Word's ¶ mark
                let past = pi < end.para && k + 1 == n;
                if to <= from && !past {
                    continue;
                }
                let x1 = self.caret_x(from, l);
                let x2 = self.caret_x(to, l) + if past { 80.0 } else { 0.0 };
                out.push(SelRect { x: x1, y: l.y, w: jm::max(0.0, x2 - x1), h: l.height, xf: l.xf });
            }
            if pi == u32::MAX {
                break;
            }
            pi += 1;
        }
    }

    /// The model position of an edit position.
    #[must_use]
    pub fn doc_ref(&self, pos: EditPos) -> Option<crate::model::DocRef> {
        let p = self.paras.get(pos.para as usize)?;
        Some(crate::model::DocRef { story: p.at.story, path: p.at.path.clone(), offset: pos.offset })
    }

    /// The edit position of a model position (`None` outside the scope).
    #[must_use]
    pub fn edit_pos(&self, at: &crate::model::DocRef) -> Option<EditPos> {
        let i = self.paras.iter().position(|p| p.at.story == at.story && p.at.path == at.path)?;
        Some(EditPos { para: u32::try_from(i).ok()?, offset: at.offset })
    }
}

/// How an engine line is copied into the edit lines.
#[derive(Clone, Copy)]
enum LineCopy<'p> {
    /// a page line; its paragraph when it holds plugin inlines (`collapseCustomChars`) and how
    /// many times the TS collapses its `chars` (F8-15)
    Page(Option<(&'p Paragraph, usize)>),
    /// a textbox line moved onto the page (`shiftEngLine`)
    Shift(f64, f64),
    /// a textbox line in a rotated or vertical box
    Xf([f64; 6]),
}

/// What makes two page lines one shared TS line (F8-15): ref, offsets, and the characters' bits.
type ShareKey = (crate::output::LineRef, u32, u32, Vec<u64>);

fn share_key(p: &LayoutPage, l: &LineGeom) -> ShareKey {
    let cs = CharsView::of(p, l);
    let mut bits = Vec::with_capacity(cs.len() * 3);
    for k in 0..cs.len() {
        let (o, x, w) = cs.at(k);
        bits.extend_from_slice(&[u64::from(o), x.to_bits(), w.to_bits()]);
    }
    (l.at.clone(), l.start, l.end, bits)
}

fn hf_line(l: &LineGeom, which: HfWhich) -> bool {
    matches!(l.at.story, LineStory::HeaderFooter { which: w, .. } if w == which)
}

fn has_custom(p: &Paragraph) -> bool {
    p.runs.iter().any(|r| r.ext().is_some_and(|e| e.custom.is_some()))
}

/// The paragraphs of a scope, in the editor's order.
fn scope_paras(loc: &Locator<'_>, scope: &EditScope<'_>) -> Vec<ParaLoc> {
    use crate::model::{Block, BlockPath, Story};
    let view = loc.view;
    let top = |story: StoryRef, s: &Story| -> Vec<ParaLoc> {
        s.blocks()
            .enumerate()
            .filter(|(_, b)| matches!(b, Block::Paragraph(_)))
            .map(|(i, _)| ParaLoc { story, path: BlockPath::root(u32::try_from(i).unwrap_or(u32::MAX)) })
            .collect()
    };
    match scope {
        EditScope::Body => top(StoryRef::Body, view.body()),
        EditScope::Cell { table, row, cell } => {
            let Some(crate::model::Block::Table(t)) = view.block(&StoryRef::Body, table) else { return Vec::new() };
            let Some(c) = t.rows.get(*row as usize).and_then(|r| r.cells.get(*cell as usize)) else { return Vec::new() };
            c.content
                .blocks()
                .enumerate()
                .filter(|(_, b)| matches!(b, Block::Paragraph(_)))
                .map(|(i, _)| ParaLoc { story: StoryRef::Body, path: table.cell_block(*row, *cell, u32::try_from(i).unwrap_or(u32::MAX)) })
                .collect()
        }
        EditScope::HeaderFooter { part, .. } => {
            let mut out = Vec::new();
            if let Some(s) = view.hf_story(*part) {
                flatten(s, None, StoryRef::HeaderFooter(*part), &mut out);
            }
            out
        }
        EditScope::Note { kind, id } => {
            let story = StoryRef::Note(*kind, *id);
            let mut out = Vec::new();
            if let Some(s) = view.note_story(*kind, *id) {
                flatten(s, None, story, &mut out);
            }
            out
        }
        EditScope::Textbox { chain, .. } => {
            let mut out = Vec::new();
            if let Some((h, t)) = chain.first().and_then(|&h| view.find_textbox(h).map(|t| (h, t))) {
                flatten(&t.content, None, StoryRef::Textbox(h), &mut out);
            }
            out
        }
    }
}

/// `flattenBlocks(blocks)` (notes.ts:96): the paragraphs with those of the cells, depth first.
fn flatten(s: &crate::model::Story, base: Option<(&crate::model::BlockPath, u32, u32)>, story: StoryRef, out: &mut Vec<ParaLoc>) {
    use crate::model::{Block, BlockPath};
    for (i, b) in s.blocks().enumerate() {
        let i = u32::try_from(i).unwrap_or(u32::MAX);
        let path = match base {
            None => BlockPath::root(i),
            Some((p, r, c)) => p.cell_block(r, c, i),
        };
        match b {
            Block::Paragraph(_) => out.push(ParaLoc { story, path }),
            Block::Table(t) => {
                for (r, row) in t.rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate() {
                        let (r, c) = (u32::try_from(r).unwrap_or(u32::MAX), u32::try_from(c).unwrap_or(u32::MAX));
                        flatten(&cell.content, Some((&path, r, c)), story, out);
                    }
                }
            }
        }
    }
}
