//! Line breaking (`lines.ts`, frontier F5; seed: `experiments/rust-spike/src/linebreak.rs`, 0
//! mismatches on 13,353 lines): Word's rules for tabs (relative to the frame's left edge; default
//! grid from 0; the hanging indent as an implicit stop), justification and its space compression,
//! hanging punctuation, hyphenation, line height by the spacing rule.
//!
//! Ported 1:1 in the TS order of operations. What the TS reads through `this.ctx` (the settings,
//! `advanceTwips`, `faceMetrics`, the run's `w:ptab`) goes through [`LineEnv`], implemented by the
//! engine's [`Cx`] and by the parity replay of the `p3.lines` facet (record "Stage 3 — F5", F5-1).
//! The TS `includes`/`===` over `PlacedGlyph` objects become index masks; `PlacedGlyph.g` (a
//! paragraph glyph, a marker glyph or the automatic hyphen the breaker builds) is a [`GlyphSrc`].

use std::borrow::Cow;

use crate::engine::Cx;
use crate::error::{LResult, LayoutError};
use crate::fonts::FaceMetrics;
use crate::hyphenate::{HyphenUnit, choose_hyphenation};
use crate::output::{TabAlign, TabLeader, TabStop};
use crate::para::{Glyph, GlyphFlags, GlyphKind, LineRule, PreparedPara};
use uo_jsnum::math as jm;
use uo_render::FaceId;

/// Which glyph a placed glyph draws (`PlacedGlyph.g`, a TS object that is a paragraph glyph, a
/// marker glyph or the automatic hyphen the breaker builds, `{ ...kg, kind: 'shy', … }`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlyphSrc {
    Para(u32),
    Marker(u32),
    /// the automatic hyphen after paragraph glyph `k`
    AutoHyphen(u32),
}

/// `PlacedGlyph` (lines.ts:25).
#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    pub g: GlyphSrc,
    /// x relative to the frame's left edge
    pub x: f64,
    /// effective width (tabs, justification)
    pub w: f64,
    /// `leader` of a tab (the TabSpec leader name)
    pub leader: Option<TabLeader>,
}

/// `'page' | 'column'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakKind {
    Page,
    Column,
}

/// `'break' | 'start'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GapTab {
    Break,
    Start,
}

/// `Line` (lines.ts:35).
#[derive(Clone, Debug)]
pub struct Line {
    /// `[start, end)` in `para.glyphs`
    pub start: u32,
    pub end: u32,
    pub placed: Vec<PlacedGlyph>,
    pub marker: Option<Vec<PlacedGlyph>>,
    pub width: f64,
    pub x_start: f64,
    pub asc: f64,
    pub desc: f64,
    pub height: f64,
    pub baseline: f64,
    pub break_after: Option<BreakKind>,
    pub clear: bool,
    pub last: bool,
    pub gap_tab: Option<GapTab>,
    pub spanned: bool,
    /// `hrFrame`: `{ left, right }`
    pub hr_frame: Option<(f64, f64)>,
}

/// `LineFrame` (lines.ts:66): the space of one line, relative to the frame (indents applied).
#[derive(Clone, Copy, Debug)]
pub struct LineFrame {
    pub left: f64,
    pub right: f64,
    pub gap: bool,
    /// `next`: the next gap of the same line (after a float)
    pub next: Option<(f64, f64)>,
}

/// A breaker position to go back to (`mark()` / `reset(m)`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BreakMark {
    pub pos: u32,
    pub line_idx: u32,
    pub trailing_break: bool,
    pub hyphen_run: f64,
}

/// `ctx.opts.justifyShrink` (fixed to the TS default: the lab override is dropped, LQ12).
#[derive(Clone, Copy, Debug)]
pub struct JustifyShrink {
    pub ratio: f64,
    pub expand_ratio: f64,
}

/// `{ ratio: 0.25, expandRatio: 0.5 }`.
pub const JUSTIFY_SHRINK: JustifyShrink = JustifyShrink { ratio: 0.25, expand_ratio: 0.5 };

/// The settings the breaker and the hyphenation choice read (`ctx.fmt.settings`).
#[derive(Clone, Copy, Debug, Default)]
pub struct BreakSettings {
    /// `defaultTabStop` (`null` read as 0, as the TS comparisons do)
    pub default_tab_stop: f64,
    /// `compatibilityMode`
    pub compatibility_mode: f64,
    /// `!!compat['doNotUseIndentAsNumberingTabStop']`
    pub no_indent_as_numbering_tab_stop: bool,
    /// `!!compat['splitPgBreakAndParaMark']`
    pub split_pg_break_and_para_mark: bool,
    /// `consecutiveHyphenLimit ?? 0`
    pub consecutive_hyphen_limit: f64,
    /// `hyphenationZone` (`??` the default at the use site)
    pub hyphenation_zone: Option<f64>,
}

/// `w:ptab` of a tab glyph (`g.ptab`): alignment and leader as the document wrote them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtabSpec {
    pub alignment: PtabAlign,
    /// `leader` when present and not `'none'`
    pub leader: Option<TabLeader>,
}

/// `g.ptab.alignment`: `'center'`, `'right'`, anything else is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtabAlign {
    Left,
    Center,
    Right,
}

/// What the line breaker reads outside the prepared paragraph (the TS `this.ctx`): implemented by
/// the engine's [`Cx`] and by the facet replay.
pub trait LineEnv {
    /// `ctx.fmt.settings` (the fields the breaker reads)
    fn settings(&self) -> BreakSettings;
    /// `advanceTwips(ctx, face, cp, sizePt)` (para.ts:378, F1)
    fn advance_twips(&mut self, face: FaceId, cp: u32, size_pt: f64) -> LResult<f64>;
    /// `faceMetrics(face.font)` (fonts.ts:57, F1)
    fn face_metrics(&mut self, face: FaceId) -> LResult<FaceMetrics>;
    /// `g.ptab` of a tab glyph (`None` = no `w:ptab`)
    fn ptab(&self, p: &PreparedPara, g: &Glyph) -> Option<PtabSpec>;
    /// charges the work budget (LQ14: the TS has no limit; one unit per glyph visited)
    fn charge(&mut self, units: u64) -> LResult<()>;
}

/// `FIXED_SPACE(cp)` (lines.ts:9): fixed-width spaces (en, em, thin, ideographic…), outside
/// justification.
fn fixed_space(cp: u32) -> bool {
    (0x2000..=0x200a).contains(&cp) || cp == 0x3000
}

/// `FIT_EPS` (lines.ts:13): the slack of the "fits" test (twips).
const FIT_EPS: f64 = 0.001;

/// The code units of a glyph's `ch`.
fn ch_units(g: &Glyph) -> &[u16] {
    g.ch.units.get(..usize::from(g.ch.len)).unwrap_or(&[])
}

/// `g.ch === c` for a one-unit string.
pub(crate) fn ch_is(g: &Glyph, c: u16) -> bool {
    ch_units(g) == [c]
}

/// `g.hr !== undefined`.
fn has_hr(g: &Glyph) -> bool {
    g.hr.is_some()
}

/// The glyph a placed glyph draws (the automatic hyphen is built from its paragraph glyph as the
/// TS spread does: `{ ...kg, kind: 'shy', ch: '', cp: 0xad, adv: 0, hyph: false, autoHyphen: true,
/// offset: kg.offset + kg.ch.length }`).
pub fn glyph_of(p: &PreparedPara, src: GlyphSrc) -> Option<Cow<'_, Glyph>> {
    match src {
        GlyphSrc::Para(i) => p.glyphs.get(i as usize).map(Cow::Borrowed),
        GlyphSrc::Marker(i) => p.marker.as_ref().and_then(|m| m.glyphs.get(i as usize)).map(Cow::Borrowed),
        GlyphSrc::AutoHyphen(k) => p.glyphs.get(k as usize).map(|kg| {
            let mut g = kg.clone();
            g.kind = GlyphKind::Shy;
            g.offset = kg.offset.wrapping_add(u32::from(kg.ch.len));
            g.ch = crate::para::Ch::default();
            g.cp = 0xad;
            g.adv = 0.0;
            g.flags = GlyphFlags((kg.flags.0 & !GlyphFlags::HYPH) | GlyphFlags::AUTO_HYPHEN);
            Cow::Owned(g)
        }),
    }
}

/// The glyph of a placed glyph, or the TS `undefined` read as an error (a placed glyph always
/// names a glyph the breaker read).
fn view<'p>(p: &'p PreparedPara, q: &PlacedGlyph) -> LResult<Cow<'p, Glyph>> {
    glyph_of(p, q.g).ok_or(LayoutError::Internal("lines: placed glyph out of range"))
}

/// The result of `nextTab`: the stop position and the custom stop, if any.
#[derive(Clone, Copy, Debug)]
struct TabHit {
    pos: f64,
    tab: Option<TabStop>,
}

/// `nextTab(x, tabs, defaultTab, hangingStop, rightLimit)` (lines.ts:76); `rightLimit` is unused
/// in the TS (`void rightLimit`).
fn next_tab(x: f64, tabs: &[TabStop], default_tab: f64, hanging_stop: Option<f64>) -> TabHit {
    let mut best: Option<TabHit> = None;
    for t in tabs {
        if t.val == TabAlign::Bar {
            continue;
        }
        if t.pos > x + 0.5 {
            best = Some(TabHit { pos: t.pos, tab: Some(*t) });
            break;
        }
    }
    if let Some(h) = hanging_stop
        && h > x + 0.5
        && best.is_none_or(|b| h < b.pos)
    {
        best = Some(TabHit { pos: h, tab: None });
    }
    if let Some(b) = best {
        return b;
    }
    // `tabs.length ? Math.max(...tabs.map((t) => t.pos)) : -Infinity`
    let last_custom = if tabs.is_empty() { f64::NEG_INFINITY } else { tabs.iter().fold(f64::NEG_INFINITY, |m, t| jm::max(m, t.pos)) };
    let from = jm::max(x, last_custom);
    let dts = if default_tab > 0.0 { default_tab } else { 720.0 };
    let mut pos = (jm::floor(from / dts + 1e-6) + 1.0) * dts;
    if pos <= x + 0.5 {
        pos += dts;
    }
    TabHit { pos, tab: None }
}

/// `moreTabs(gs, i)` (lines.ts:98): another tab after glyph `i`, before the next line or page
/// break?
fn more_tabs(gs: &[Glyph], i: usize) -> bool {
    for q in gs.iter().skip(i + 1) {
        if q.kind == GlyphKind::Tab {
            return true;
        }
        if matches!(q.kind, GlyphKind::Lb | GlyphKind::Pb | GlyphKind::Cb) {
            return false;
        }
    }
    false
}

fn aligned(t: Option<&TabStop>) -> bool {
    t.is_some_and(|t| matches!(t.val, TabAlign::Right | TabAlign::Center | TabAlign::Decimal))
}

/// `lineBox(asc, desc, p, textNat?, grid?)` (lines.ts:108): height and baseline by the spacing
/// rule (measured in Word). Returns `(height, baseline)`.
#[must_use]
pub fn line_box(asc: f64, desc: f64, p: &PreparedPara, text_nat: Option<f64>, grid: Option<f64>) -> (f64, f64) {
    let nat = asc + desc;
    if p.line_rule == LineRule::Exact {
        let h = p.line;
        return (h, h * 0.8);
    }
    // `grid && grid > 0`
    if let Some(grid) = grid.filter(|&g| uo_jsnum::conv::truthy(g) && g > 0.0) {
        // the document grid (docGrid type=lines), measured in the docgrid oracle: the line takes the
        // fewest grid steps that hold its natural height, text CENTRED; multiple spacing applies
        // to ONE step; at-least: the grid box sits at the FOOT of the minimum height
        let g = jm::max(1.0, jm::ceil((nat - 1.0) / grid)) * grid;
        let g_base = (g - nat) / 2.0 + asc;
        if p.line_rule == LineRule::AtLeast {
            let h = jm::max(g, p.line);
            return (h, h - g + g_base);
        }
        let h = jm::max((grid * p.line) / 240.0, g);
        return (h, (h - nat) / 2.0 + asc);
    }
    if p.line_rule == LineRule::AtLeast {
        let h = jm::max(nat, p.line);
        return (h, h - desc);
    }
    // multiple: the extra is (factor − 1) × the TEXT height (inline objects are not multiplied —
    // measured in Word) and goes BELOW the line
    let extra = (p.line / 240.0 - 1.0) * text_nat.unwrap_or(nat);
    let h = nat + extra;
    (h, if extra >= 0.0 { asc } else { asc * (h / nat) })
}

/// `shadowExtent(style)` (para.ts:321, F4's file; here because the empty-line height reads it —
/// F4 uses this one): the extent of a `w14:shadow` above and below the text.
#[must_use]
pub fn shadow_extent(style: &crate::para::RunStyle) -> (f64, f64) {
    let Some(sh) = style.rpr.text_shadow.as_deref() else { return (0.0, 0.0) };
    if !sh.blur.truthy() && !sh.dist.truthy() {
        return (0.0, 0.0);
    }
    let (blur, dist, dir) = (sh.blur.to_number(), sh.dist.to_number(), sh.dir.to_number());
    let k = jm::pow(style.size_pt / 72.0, 0.7);
    let dy = dist * jm::sin((dir * std::f64::consts::PI) / 180.0);
    (k * jm::max(0.0, blur - dy) * 20.0, k * jm::max(0.0, blur + dy) * 20.0)
}

/// `pendTab` (lines.ts:272): an aligned tab whose advance follows the segment after it.
#[derive(Clone, Copy, Debug)]
struct PendTab {
    idx: usize,
    pos: f64,
    tab: TabStop,
    seg_start: f64,
    start_x: f64,
    decimal_seen: bool,
    decimal_x: f64,
}

/// `closePending()` (lines.ts:283).
fn close_pending(pend: &mut Option<PendTab>, placed: &mut [PlacedGlyph], x: &mut f64, content_end: &mut f64) {
    let Some(pt) = pend.take() else { return };
    let seg_w = *x - pt.seg_start;
    let mut shift = 0.0;
    let avail = pt.pos - pt.start_x;
    if pt.tab.val == TabAlign::Right {
        shift = jm::max(0.0, avail - seg_w);
    } else if pt.tab.val == TabAlign::Center {
        shift = jm::max(0.0, avail - seg_w / 2.0);
    } else if pt.tab.val == TabAlign::Decimal {
        let dx = if pt.decimal_seen { pt.decimal_x - pt.seg_start } else { seg_w };
        shift = jm::max(0.0, avail - dx);
    }
    if let Some(q) = placed.get_mut(pt.idx) {
        q.w = shift;
    }
    for q in placed.iter_mut().skip(pt.idx + 1) {
        q.x += shift;
    }
    *x += shift;
    if *content_end > pt.seg_start {
        *content_end += shift;
    }
}

/// `LineBreaker` (lines.ts:140): breaks one prepared paragraph line by line, each line in the
/// frame the engine gives it (floats change the frame per line).
#[derive(Clone, Debug, Default)]
pub struct LineBreaker {
    pub pos: u32,
    pub line_idx: u32,
    /// a line break (`w:br`) at the END of the paragraph: the paragraph mark goes to its own empty
    /// line (Word)
    trailing_break: bool,
    /// consecutive lines ending in a hyphen (`w:consecutiveHyphenLimit`)
    hyphen_run: f64,
    /// `grid`: the line-grid pitch of the section (twips) — body only, when the paragraph snaps
    pub grid: Option<f64>,
}

impl LineBreaker {
    /// `new LineBreaker(ctx, p, grid?)`; the paragraph is passed to every call.
    #[must_use]
    pub fn new(_p: &PreparedPara, grid: Option<f64>) -> LineBreaker {
        LineBreaker { grid, ..LineBreaker::default() }
    }

    /// `done()`.
    #[must_use]
    pub fn done(&self, p: &PreparedPara) -> bool {
        self.pos as usize >= p.glyphs.len() && self.line_idx > 0 && !self.trailing_break
    }

    /// `peekWordWidth()` (lines.ts:162): the width of the next word (up to the first break point);
    /// the gap beside an object takes text only if it fits (oracle wrapsides).
    #[must_use]
    pub fn peek_word_width(&self, p: &PreparedPara) -> f64 {
        let mut w = 0.0;
        for q in p.glyphs.iter().skip(self.pos as usize) {
            if matches!(q.kind, GlyphKind::Space | GlyphKind::Tab | GlyphKind::Lb | GlyphKind::Pb | GlyphKind::Cb) {
                break;
            }
            if q.kind == GlyphKind::Obj && q.adv == 0.0 {
                continue;
            }
            w += if q.kind == GlyphKind::Shy { 0.0 } else { q.adv };
            if q.flags.has(GlyphFlags::BREAK_AFTER) {
                break;
            }
        }
        w + if self.indent_first(p) { jm::max(0.0, p.first_line) } else { 0.0 }
    }

    /// `indentFirst()` (lines.ts:178): the line with the first-line indent and the list number —
    /// the first, or, in a paragraph that STARTS with a column break, the one after it.
    fn indent_first(&self, p: &PreparedPara) -> bool {
        let first_cb = p.glyphs.first().is_some_and(|g| g.kind == GlyphKind::Cb) && p.glyphs.len() > 1;
        self.line_idx == u32::from(first_cb)
    }

    /// `mark()`.
    #[must_use]
    pub fn mark(&self) -> BreakMark {
        BreakMark { pos: self.pos, line_idx: self.line_idx, trailing_break: self.trailing_break, hyphen_run: self.hyphen_run }
    }

    /// `reset(m)`.
    pub fn reset(&mut self, m: BreakMark) {
        self.pos = m.pos;
        self.line_idx = m.line_idx;
        self.trailing_break = m.trailing_break;
        self.hyphen_run = m.hyphen_run;
    }

    /// `fitRight(frame, xStart, first)` (lines.ts:196): the right edge of the "fits" test, the
    /// column width and each indent converted to 1/4096 pt separately (oracle emwidth). The TS
    /// returns `frame.right` for the lab options `advanceRound !== 'word'` / `advanceEm: 'device'`,
    /// which are dropped (LQ12).
    fn fit_right(p: &PreparedPara, frame_left: f64, frame_right: f64, x_start: f64, first: bool) -> f64 {
        let q = crate::para::to_word_unit;
        x_start + q(frame_right + p.ind_right)
            - q(p.ind_right)
            - q(frame_left - p.ind_left)
            - q(p.ind_left)
            - if first { q(p.first_line) } else { 0.0 }
    }

    /// `next(frame)` through the engine's context.
    pub fn next(&mut self, cx: &mut Cx<'_, '_>, p: &PreparedPara, frame: LineFrame) -> LResult<Line> {
        self.next_in(cx, p, frame)
    }

    /// `next(frame)` (lines.ts:203): the next line in the given frame (indents already applied).
    pub fn next_in<E: LineEnv + ?Sized>(&mut self, env: &mut E, p: &PreparedPara, frame: LineFrame) -> LResult<Line> {
        let s = env.settings();
        let gs = &p.glyphs[..];
        let n = gs.len();
        let ind_first = self.indent_first(p);
        let start = self.pos as usize;
        let x_start = frame.left + if ind_first { p.first_line } else { 0.0 };
        let mut right = frame.right;
        let mut right_q = Self::fit_right(p, frame.left, frame.right, x_start, ind_first);
        let mut placed: Vec<PlacedGlyph> = Vec::new();
        let mut x = x_start;
        let mut marker_placed: Option<Vec<PlacedGlyph>> = None;
        let default_tab = s.default_tab_stop;
        // the hanging indent = an implicit stop (first line)
        let hanging_stop = if ind_first && p.first_line < 0.0 { Some(frame.left) } else { None };
        let mut work: u64 = 1;

        // the numbering marker (first line only)
        if let (true, Some(mk)) = (ind_first, p.marker.as_ref()) {
            let mut mx = x_start;
            if &*mk.jc == "right" {
                mx -= mk.width;
            } else if &*mk.jc == "center" {
                mx -= mk.width / 2.0;
            }
            let mut mp = Vec::with_capacity(mk.glyphs.len());
            for (k, g) in mk.glyphs.iter().enumerate() {
                let k = u32::try_from(k).map_err(|_| LayoutError::Internal("lines: marker glyph index"))?;
                mp.push(PlacedGlyph { g: GlyphSrc::Marker(k), x: mx, w: g.adv, leader: None });
                mx += g.adv;
            }
            marker_placed = Some(mp);
            x = jm::max(mx, x_start);
            match mk.suff {
                crate::para::MarkerSuffix::Tab => {
                    // tab after the number: the next stop (the left indent counts as a stop); the
                    // number may END ON the stop (end ≤ stop) — oracle numrpr (nrs-*)
                    let xe = x - 0.5 - 1e-3;
                    let mut ns = next_tab(xe, &p.tabs, default_tab, if frame.left > xe + 0.5 { Some(frame.left) } else { None });
                    // doNotUseIndentAsNumberingTabStop (modes ≤ 14): the hanging indent has no
                    // priority — the nearest stop wins, default or the indent (oracle numtabind)
                    if s.no_indent_as_numbering_tab_stop && s.compatibility_mode < 15.0 {
                        let plain = next_tab(xe, &p.tabs, default_tab, None);
                        ns = if frame.left > xe + 0.5 && frame.left < plain.pos { TabHit { pos: frame.left, tab: None } } else { plain };
                    }
                    x = ns.pos;
                }
                crate::para::MarkerSuffix::Space => {
                    let (face, size) = mk.glyphs.first().map_or_else(
                        || (p.mark_face, p.styles.get(p.mark_style.0 as usize).map_or(f64::NAN, |st| st.size_pt)),
                        |g| (g.face, g.size_pt),
                    );
                    x += env.advance_twips(face, 0x20, size)?;
                }
                crate::para::MarkerSuffix::Nothing => {}
            }
        }

        let mut i = start;
        // index after which a break is possible (`-1`: none)
        let mut last_break: Option<usize> = None;
        // justified: Word 2013+ compresses the spaces to fit the word — measured (rule in jsweep,
        // below). In compatibility modes ≤ 14 there is no compression: the word must fit (justify6)
        let justify = (&*p.jc == "both" || &*p.jc == "distribute") && s.compatibility_mode >= 15.0;
        let shrink = JUSTIFY_SHRINK;
        let mut spaces_w = 0.0; // sum of the line's spaces (after the last tab)
        let mut n_spaces = 0.0;
        // spaces BEFORE the segment's first word (line start / after a tab) neither compress nor
        // expand (oracle jlead)
        let mut seg_word = false;
        let mut last_gap_w = 0.0; // width of the spaces right before the word
        let mut last_gap_sp = 0.0; // … only those that compress/expand (counted in spacesW)
        // the unit that goes down whole when it does not fit (word + words glued by NBSP): its start
        // x, the line's spaces before it and the gap before it
        let (mut unit_x, mut unit_spaces_w, mut unit_gap_w, mut unit_gap_sp) = (x_start, 0.0, 0.0, 0.0);
        // East Asian layout of modes ≤ 14: how much the line's spaces may compress (oracle fecompress)
        let mut fe_shrink_w = 0.0;
        let mut word_ok = false; // the current word was accepted whole
        let mut squeezed = false; // a word of the line only fit by compressing/hanging
        let mut prev_was_gap = true;
        let mut width_at_break = 0.0; // content width up to the break point (no trailing gaps)
        let mut content_end = x_start; // x of the end of the last non-blank glyph
        let mut pend_tab: Option<PendTab> = None;
        let mut break_after: Option<BreakKind> = None;
        let mut clear_after = false;
        let mut gap_tab: Option<GapTab> = None;
        let mut spanned = false;
        let mut past_margin = false;
        let mut forced = false;
        // hyphenation of the word that did not fit: the line ends after glyph hyphAt, with the
        // hyphen (width hyphW) — automatic or at the optional hyphen (hyphenate.ts)
        let (mut hyph_at, mut hyph_w, mut ended_hyphen): (Option<usize>, f64, bool) = (None, 0.0, false);
        let lb = |k: Option<usize>| k.is_some_and(|k| k >= start);

        while i < n {
            work += 1;
            let Some(g) = gs.get(i) else { break };
            if let Some(ha) = hyph_at.filter(|&h| i > h) {
                let kg = gs.get(ha).ok_or(LayoutError::Internal("lines: gs[hyphAt]"))?;
                if kg.kind == GlyphKind::Shy {
                    if let Some(q) = placed.last_mut() {
                        q.w = hyph_w;
                    }
                } else {
                    let k = u32::try_from(ha).map_err(|_| LayoutError::Internal("lines: glyph index"))?;
                    placed.push(PlacedGlyph { g: GlyphSrc::AutoHyphen(k), x, w: hyph_w, leader: None });
                }
                content_end = x + hyph_w;
                // only the automatic hyphen counts for w:consecutiveHyphenLimit (oracle hyphshy)
                ended_hyphen = kg.kind != GlyphKind::Shy;
                pend_tab = None;
                break;
            }
            let gi = u32::try_from(i).map_err(|_| LayoutError::Internal("lines: glyph index"))?;
            if matches!(g.kind, GlyphKind::Lb | GlyphKind::Pb | GlyphKind::Cb) {
                close_pending(&mut pend_tab, &mut placed, &mut x, &mut content_end);
                placed.push(PlacedGlyph { g: GlyphSrc::Para(gi), x, w: 0.0, leader: None });
                i += 1;
                forced = true;
                if g.kind == GlyphKind::Pb {
                    break_after = Some(BreakKind::Page);
                    // after a page break, only float anchors to the end of the paragraph: they stay
                    // on this line and the mark opens no line on the next page (oracle pbanc)
                    let mut j = i;
                    while gs.get(j).is_some_and(|q| q.kind == GlyphKind::Obj && q.adv == 0.0 && !has_hr(q)) {
                        j += 1;
                    }
                    if j > i && j >= n {
                        while i < j {
                            let k = u32::try_from(i).map_err(|_| LayoutError::Internal("lines: glyph index"))?;
                            placed.push(PlacedGlyph { g: GlyphSrc::Para(k), x, w: 0.0, leader: None });
                            i += 1;
                        }
                    }
                } else if g.kind == GlyphKind::Cb {
                    break_after = Some(BreakKind::Column);
                } else if g.flags.has(GlyphFlags::CLEAR) {
                    clear_after = true;
                }
                break;
            }
            // VML horizontal line (o:hr): alone on its line — break before and after (oracle vmlhr)
            if has_hr(g) {
                let mut other = false;
                for q in &placed {
                    let v = view(p, q)?;
                    if v.kind != GlyphKind::Obj || v.adv > 0.0 || has_hr(&v) {
                        other = true;
                        break;
                    }
                }
                if other {
                    break;
                }
                placed.push(PlacedGlyph { g: GlyphSrc::Para(gi), x, w: 0.0, leader: None });
                i += 1;
                break;
            }
            if g.kind == GlyphKind::Tab {
                close_pending(&mut pend_tab, &mut placed, &mut x, &mut content_end);
                let ptab = env.ptab(p, g);
                let mut pos: f64;
                let mut tab: Option<TabStop>;
                if let Some(pt) = ptab {
                    // absolute tab: alignment relative to the margin/indent
                    let width = right - frame.left;
                    pos = match pt.alignment {
                        PtabAlign::Center => frame.left + width / 2.0,
                        PtabAlign::Right => right,
                        PtabAlign::Left => frame.left,
                    };
                    let val = match pt.alignment {
                        PtabAlign::Center => TabAlign::Center,
                        PtabAlign::Right => TabAlign::Right,
                        PtabAlign::Left => TabAlign::Left,
                    };
                    tab = Some(TabStop { val, pos, leader: pt.leader });
                } else {
                    let h = next_tab(x, &p.tabs, default_tab, hanging_stop);
                    pos = h.pos;
                    tab = h.tab;
                }
                // the gap beside a float: a stop beyond it with ANOTHER tab ahead on the line ends
                // the line before the tab and the rest goes below the float; a single tab stays at
                // the end of the gap (oracle tabgap)
                if frame.gap && ptab.is_none() && pos > right + 0.5 && more_tabs(gs, i) {
                    // with a stop in the next gap of the same line (before its end), the tab
                    // crosses the float and the line goes on there — centred as a whole line
                    // (oracle ftrtabs; corpus t20); without one, the line ends here (tabgap)
                    let nx = match frame.next {
                        Some((nl, _)) if !spanned => Some(next_tab(nl, &p.tabs, default_tab, hanging_stop)),
                        _ => None,
                    };
                    match (nx, frame.next) {
                        (Some(nx), Some((nl, nr))) if nx.pos <= nr + 0.5 => {
                            spanned = true;
                            x = nl;
                            right = nr;
                            right_q = Self::fit_right(p, frame.left, right, x_start, ind_first);
                            pos = nx.pos;
                            tab = nx.tab;
                        }
                        _ => {
                            if i > start {
                                gap_tab = Some(GapTab::Break);
                                break;
                            }
                            gap_tab = Some(GapTab::Start);
                        }
                    }
                }
                // Word 2013+ (oracle tocdots-beyond-c15): a custom right/center/decimal stop beyond
                // max(margin, line end) aligns the text RIGHT at the line end (margin − right
                // indent); in mode ≤ 14 the position holds
                if ptab.is_none()
                    && aligned(tab.as_ref())
                    && s.compatibility_mode >= 15.0
                    && pos > jm::max(right, right + p.ind_right) + 0.5
                {
                    pos = right;
                    tab = tab.map(|t| TabStop { val: TabAlign::Right, ..t });
                }
                // Word 2013+: a custom LEFT stop beyond the right margin — the line breaks before
                // the tab even with nothing after it (one more line at the paragraph end) and,
                // opening the line, the tab only goes to the margin (oracle tabbeyond)
                if ptab.is_none() && tab.is_some() && !aligned(tab.as_ref()) && s.compatibility_mode >= 15.0 && pos > right + 0.5 {
                    if i > start {
                        break;
                    }
                    pos = jm::max(x, right);
                }
                // no stop up to the right margin: the line breaks BEFORE the tab, which starts the
                // next line again (at its first stop) — oracle tabs (mode ≤ 14 with custom stops:
                // the tab goes on beyond the margin by the default grid)
                let legacy_no_wrap =
                    s.compatibility_mode < 15.0 && p.tabs.iter().any(|t| t.val != TabAlign::Bar && t.val != TabAlign::Clear);
                // a DEFAULT tab whose stop passes the right margin (outside the line start): break
                // before it, content after it or not — also at the paragraph end (oracle tabwrap)
                // (a CUSTOM stop beyond the margin holds — tables of contents with a right tab at
                // the margin)
                if ptab.is_none() && tab.is_none() && !legacy_no_wrap && pos > right + 0.5 && i > start {
                    break;
                }
                // old mode: after a tab beyond the margin the rest of the line does not break
                // (oracle tabs c14)
                if legacy_no_wrap && pos > right + 0.5 {
                    past_margin = true;
                }
                let leader = tab.and_then(|t| t.leader).filter(|&l| l != TabLeader::None);
                placed.push(PlacedGlyph { g: GlyphSrc::Para(gi), x, w: 0.0, leader });
                let idx = placed.len() - 1;
                match tab {
                    Some(t) if aligned(Some(&t)) => {
                        pend_tab = Some(PendTab { idx, pos, tab: t, seg_start: x, start_x: x, decimal_seen: false, decimal_x: 0.0 });
                    }
                    _ => {
                        if let Some(q) = placed.get_mut(idx) {
                            q.w = jm::max(0.0, pos - x);
                        }
                        x = jm::max(x, pos);
                    }
                }
                // the break sits BEFORE the tab: if the text after it does not fit, the tab goes
                // down with it and starts the next line — the leader runs from its start (oracle
                // tabnextline)
                last_break = Some(if i > start { i - 1 } else { i });
                width_at_break = content_end - x_start;
                spaces_w = 0.0;
                n_spaces = 0.0;
                fe_shrink_w = 0.0;
                seg_word = false;
                last_gap_w = 0.0;
                last_gap_sp = 0.0;
                prev_was_gap = true;
                i += 1;
                continue;
            }
            if g.kind == GlyphKind::Obj && g.adv == 0.0 {
                // anchor
                placed.push(PlacedGlyph { g: GlyphSrc::Para(gi), x, w: 0.0, leader: None });
                i += 1;
                continue;
            }
            let is_gap = g.kind == GlyphKind::Space;
            if !is_gap && g.kind != GlyphKind::Shy && prev_was_gap {
                // word start: does it fit whole (with compression, when justified)?
                let mut e = i;
                let mut word_w = 0.0;
                while e < n {
                    work += 1;
                    let Some(q) = gs.get(e) else { break };
                    if matches!(q.kind, GlyphKind::Space | GlyphKind::Tab | GlyphKind::Lb | GlyphKind::Pb | GlyphKind::Cb) {
                        break;
                    }
                    if q.kind == GlyphKind::Obj && q.adv == 0.0 {
                        e += 1;
                        continue;
                    }
                    // optional hyphen: a hyphenation point INSIDE the word (oracle hyphshy)
                    if q.kind == GlyphKind::Shy {
                        e += 1;
                        continue;
                    }
                    word_w += q.adv;
                    if q.flags.has(GlyphFlags::BREAK_AFTER) && q.kind != GlyphKind::Char {
                        break;
                    }
                    if q.flags.has(GlyphFlags::BREAK_AFTER) {
                        e += 1;
                        break;
                    }
                    e += 1;
                }
                let over = x + word_w - right_q;
                // the word after a space that does not break (glued to an NBSP) continues the
                // previous unit: if it goes down, the whole unit goes down (oracle nbspglue)
                let prev = if i > 0 { gs.get(i - 1) } else { None };
                if i == start || !prev.is_some_and(|q| q.kind == GlyphKind::Space && !q.flags.has(GlyphFlags::BREAK_AFTER)) {
                    // after a break WITHOUT a space (hyphen, dash, object): the alternative is
                    // breaking right there — no gap leaves the line (oracle hyphcomp)
                    let gap = i > start && prev.is_some_and(|q| q.kind == GlyphKind::Space);
                    unit_x = x;
                    unit_spaces_w = spaces_w;
                    unit_gap_w = if gap { last_gap_w } else { 0.0 };
                    unit_gap_sp = if gap { last_gap_sp } else { 0.0 };
                }
                // fits iff W ≤ A, widths and limit in Word's internal unit (oracles fitwidth, emwidth)
                word_ok = over <= FIT_EPS;
                // East Asian layout: the final punctuation hangs past the margin — in modes ≤ 14
                // always (oracle fecompress), in 15 only when justified (oracle hang15)
                let last_of_word = if e > i { gs.get(e - 1) } else { None };
                let hang_w = match last_of_word {
                    Some(q) if q.flags.has(GlyphFlags::HANG) && (justify || s.compatibility_mode < 15.0) => q.adv,
                    _ => 0.0,
                };
                if !word_ok && (hang_w > 0.0 || fe_shrink_w > 0.0) && i > start {
                    // modes ≤ 14: the spaces still compress to each one's minimum — but the last
                    // word of the paragraph that hangs its punctuation adds no compression (oracle
                    // fehangcomp)
                    let para_end = gs
                        .iter()
                        .skip(e)
                        .all(|q| q.kind == GlyphKind::Pb || q.kind == GlyphKind::Cb || (q.kind == GlyphKind::Obj && q.adv == 0.0));
                    work += (n - e.min(n)) as u64;
                    word_ok = over - hang_w <= (if hang_w > 0.0 && para_end { 0.0 } else { fe_shrink_w }) + FIT_EPS;
                }
                if !word_ok && justify && lb(last_break) && i > start {
                    // Word compares the two outcomes: compressing the n spaces so the word fits ×
                    // moving it to the next line and expanding the spaces left. It compresses if
                    // each space loses ≤ 25% AND ≤ half of what it would gain (oracle jsweep;
                    // slacks = sub-twip quantization measured in Word). The fractions are relative
                    // to the WIDTH of the spaces (spaces of different sizes on the line, double
                    // gap: each one compresses/expands in proportion — oracle jmixsp)
                    let slack = x + word_w - unit_x + unit_gap_w - over;
                    let rest = unit_spaces_w - unit_gap_sp;
                    let ov = over - hang_w; // the hanging punctuation need not fit (oracle hang15)
                    word_ok = n_spaces > 0.0
                        && ov <= shrink.ratio * spaces_w + 0.0175
                        && (n_spaces <= 1.0
                            || rest <= 0.0
                            || ov / spaces_w <= (shrink.expand_ratio * slack) / rest + 0.0075 * n_spaces / spaces_w);
                }
                // a word that only fit by compressing the spaces (or hanging the punctuation): the
                // line ends with it — the next goes down, even if it would fit compressing more
                // (oracle fejust)
                if squeezed && i > start {
                    word_ok = false;
                } else if word_ok && over > FIT_EPS {
                    squeezed = true;
                }
                // not whole: hyphenate (automatic points and optional hyphens), if Word would —
                // otherwise it goes down (or, opening the line, breaks by character)
                else if !word_ok && pend_tab.is_none() && !past_margin {
                    let u = HyphenUnit {
                        i: gi,
                        e: u32::try_from(e).map_err(|_| LayoutError::Internal("lines: glyph index"))?,
                        x,
                        right: right_q,
                        line_start: !lb(last_break),
                        content_end: x_start + width_at_break,
                        justify: &*p.jc == "both" || &*p.jc == "distribute",
                        spaces_w,
                        rest_w: unit_spaces_w - unit_gap_sp,
                        n_spaces,
                        hyphen_run: self.hyphen_run,
                    };
                    if let Some(ch) = choose_hyphenation(env, gs, &u)? {
                        hyph_at = Some(ch.k as usize);
                        hyph_w = ch.w;
                        word_ok = true;
                    }
                }
            }
            // overflow: break at the last possible point
            let projected = if is_gap { x } else { x + g.adv };
            // float anchors (an object without width on the line) do not count as content: what
            // comes after them and does not fit stays on the line anyway (oracle floatwide)
            // mode ≤ 14: the text of a RIGHT tab that does not fit stays on the line, past the
            // margin, and the line breaks no more (oracle tabnextline)
            if !is_gap
                && g.kind != GlyphKind::Shy
                && !word_ok
                && !past_margin
                && projected > right_q + FIT_EPS
                && i > start
                && pend_tab.is_some_and(|t| t.tab.val == TabAlign::Right)
                && s.compatibility_mode < 15.0
            {
                past_margin = true;
            }
            if !is_gap && g.kind != GlyphKind::Shy && !word_ok && !past_margin && projected > right_q + FIT_EPS && i > start {
                let mut content = false;
                for q in &placed {
                    let v = view(p, q)?;
                    if v.kind != GlyphKind::Obj || v.adv > 0.0 {
                        content = true;
                        break;
                    }
                }
                work += placed.len() as u64;
                if content {
                    if let Some(lbk) = last_break.filter(|&k| k >= start) {
                        // back to the break point
                        let keep = lbk + 1;
                        let at = GlyphSrc::Para(u32::try_from(lbk).map_err(|_| LayoutError::Internal("lines: glyph index"))?);
                        while placed.last().is_some_and(|q| q.g != at) {
                            placed.pop();
                        }
                        i = keep;
                        content_end = x_start + width_at_break;
                        pend_tab = None;
                        break;
                    }
                    // a word longer than the line: break by character (the current glyph goes
                    // down) — except a glyph glued to the previous one, which stays and overflows
                    // (hfgroup)
                    if !g.flags.has(GlyphFlags::GLUE) {
                        break;
                    }
                }
            }
            // optional hyphen: only a break (with a hyphen) through the hyphenation above
            if g.kind == GlyphKind::Shy {
                placed.push(PlacedGlyph { g: GlyphSrc::Para(gi), x, w: 0.0, leader: None });
                i += 1;
                continue;
            }
            placed.push(PlacedGlyph { g: GlyphSrc::Para(gi), x, w: g.adv, leader: None });
            if is_gap {
                if !prev_was_gap {
                    last_gap_w = 0.0;
                    last_gap_sp = 0.0;
                }
                last_gap_w += g.adv;
                // fixed-width spaces (en/em/thin… U+2000–U+200A) neither compress nor expand
                if seg_word && !fixed_space(g.cp) && !g.flags.has(GlyphFlags::FIXED) {
                    spaces_w += g.adv;
                    n_spaces += 1.0;
                    last_gap_sp += g.adv;
                    fe_shrink_w += g.fe_shrink.or(0.0);
                }
            } else {
                seg_word = true;
            }
            prev_was_gap = is_gap || g.flags.has(GlyphFlags::BREAK_AFTER);
            if let Some(pt) = pend_tab.as_mut()
                && pt.tab.val == TabAlign::Decimal
                && !pt.decimal_seen
                && (ch_is(g, u16::from(b',')) || ch_is(g, u16::from(b'.')))
            {
                pt.decimal_seen = true;
                pt.decimal_x = x;
            }
            x += g.adv;
            if !is_gap {
                content_end = x;
            }
            if g.flags.has(GlyphFlags::BREAK_AFTER) {
                last_break = Some(i);
                width_at_break = (if is_gap { content_end } else { x }) - x_start;
            }
            i += 1;
        }
        close_pending(&mut pend_tab, &mut placed, &mut x, &mut content_end);
        // trailing spaces/tabs hang: width = up to the last non-blank (a float anchor after the
        // spaces does not hold them — oracle anctrail)
        let mut width = 0.0;
        for q in &placed {
            let v = view(p, q)?;
            if v.kind == GlyphKind::Obj && v.adv == 0.0 {
                continue;
            }
            if !matches!(v.kind, GlyphKind::Space | GlyphKind::Lb | GlyphKind::Pb | GlyphKind::Cb) {
                width = jm::max(width, q.x + q.w - x_start);
            }
        }
        self.pos = u32::try_from(n.min(i)).map_err(|_| LayoutError::Internal("lines: glyph index"))?;
        if self.pos as usize == start && start < n {
            // guarantees progress
            self.pos = u32::try_from(start + 1).map_err(|_| LayoutError::Internal("lines: glyph index"))?;
        }
        self.hyphen_run = if ended_hyphen { self.hyphen_run + 1.0 } else { 0.0 };
        // a line or COLUMN break at the paragraph end: the mark goes to its own line (on the next
        // column/page — oracle colbreak); a page break's does not (pbfit) (a page break's too,
        // with splitPgBreakAndParaMark in modes ≤ 14 — oracle pbmark) (anchors after the break at
        // the paragraph end do not count — oracle pbanc)
        let mut lk_i = placed.len().checked_sub(1);
        while let Some(k) = lk_i.filter(|&k| k > 0) {
            let v = view(p, placed.get(k).ok_or(LayoutError::Internal("lines: placed[lkI]"))?)?;
            if v.kind == GlyphKind::Obj && v.adv == 0.0 && !has_hr(&v) {
                lk_i = Some(k - 1);
            } else {
                break;
            }
        }
        let kind_at = |k: Option<usize>| -> LResult<Option<GlyphKind>> {
            match k.and_then(|k| placed.get(k)) {
                Some(q) => Ok(Some(view(p, q)?.kind)),
                None => Ok(None),
            }
        };
        let lk = if kind_at(lk_i)? == Some(GlyphKind::Pb) { Some(GlyphKind::Pb) } else { kind_at(placed.len().checked_sub(1))? };
        let split_pg = s.compatibility_mode < 15.0 && s.split_pg_break_and_para_mark;
        self.trailing_break = forced
            && self.pos as usize >= n
            && (lk == Some(GlyphKind::Lb) || lk == Some(GlyphKind::Cb) || (lk == Some(GlyphKind::Pb) && split_pg));
        let last = self.pos as usize >= n && !self.trailing_break;
        // (saturating: 2^32 lines of one paragraph are far past the work budget)
        self.line_idx = self.line_idx.saturating_add(1);

        // vertical metrics
        let (mut asc, mut desc, mut t_asc, mut t_desc) = (0.0, 0.0, 0.0, 0.0);
        // the face of the object runs (text height of a line with objects only)
        let (mut o_asc, mut o_desc) = (0.0, 0.0);
        // `content = placed.filter((pg) => pg.g.kind !== 'obj' || pg.g.adv > 0)`
        let mut content: Vec<Cow<'_, Glyph>> = Vec::with_capacity(placed.len());
        for q in &placed {
            let v = view(p, q)?;
            if v.kind != GlyphKind::Obj || v.adv > 0.0 {
                content.push(v);
            }
        }
        // a line break (w:br) on a line with an inline object: like the paragraph mark, it gives no
        // height — the image's line keeps its height (oracle imgbr)
        let with_obj = content.iter().any(|v| v.kind == GlyphKind::Obj);
        for v in &content {
            if v.kind == GlyphKind::Pb || v.kind == GlyphKind::Cb {
                continue;
            }
            if v.kind == GlyphKind::Lb && with_obj {
                continue;
            }
            // the plain space (U+0020) and the tab give the line no height — at the end, in the
            // middle or alone (then the paragraph mark counts); the NBSP does — oracles trailsp/hfimg
            if ch_is(v, 0x20) || v.kind == GlyphKind::Tab {
                continue;
            }
            asc = jm::max(asc, v.asc);
            desc = jm::max(desc, v.desc);
            // (the check box counts as text — oracle cbline)
            if v.kind != GlyphKind::Obj {
                t_asc = jm::max(t_asc, v.n_asc.or(v.asc));
                t_desc = jm::max(t_desc, v.n_desc.or(v.desc));
            } else {
                let om = env.face_metrics(v.face)?;
                o_asc = jm::max(o_asc, om.ascent * v.size_pt * 20.0);
                o_desc = jm::max(o_desc, om.descent * v.size_pt * 20.0);
                // UNDERLINED object: Word draws the underline below it and the run face's descent
                // enters the line (oracle imgul)
                let st = p.styles.get(v.style.0 as usize);
                if st.and_then(|st| st.underline.as_ref()).is_some_and(|u| &*u.0 != "none") {
                    desc = jm::max(desc, om.descent * v.size_pt * 20.0);
                }
            }
        }
        // a line with inline objects only: at least the height of the object RUN's face (the low
        // object sits at its foot) — oracle imgsmall; the tall object sets the line (imgbr)
        if t_asc + t_desc == 0.0 && o_asc > 0.0 {
            asc = jm::max(asc, o_asc + o_desc - desc);
        }
        // horizontal rule (o:hr), alone on the line: the height is the rule RUN's face (the
        // paragraph mark does not count) and the rule is an object of height h + 5 pt at its foot —
        // multiple spacing adds (factor − 1) × that face's height (oracle hrfont)
        for q in &placed {
            let v = view(p, q)?;
            let Some(hr) = v.hr.get() else { continue };
            let om = env.face_metrics(v.face)?;
            let fa = om.ascent * v.size_pt * 20.0;
            let fd = om.descent * v.size_pt * 20.0;
            t_asc = jm::max(t_asc, fa);
            t_desc = jm::max(t_desc, fd);
            desc = jm::max(desc, fd);
            asc = jm::max_n(&[asc, fa, hr + 100.0 - desc]);
        }
        // a line with ONLY tabs that is not the paragraph's last (the text went down to the next):
        // the height is the tabs', not the mark's (oracle ftrtabs; corpus t20)
        if !last
            && asc == 0.0
            && desc == 0.0
            && content.iter().any(|v| v.kind == GlyphKind::Tab)
            && content.iter().all(|v| v.kind == GlyphKind::Tab || ch_is(v, 0x20))
        {
            for v in &content {
                if v.kind != GlyphKind::Tab {
                    continue;
                }
                asc = jm::max(asc, v.asc);
                desc = jm::max(desc, v.desc);
                t_asc = jm::max(t_asc, v.n_asc.or(v.asc));
                t_desc = jm::max(t_desc, v.n_desc.or(v.desc));
            }
        }
        let mark_style = p.styles.get(p.mark_style.0 as usize).ok_or(LayoutError::Internal("lines: p.markStyle"))?;
        let empty_line = asc == 0.0 && desc == 0.0;
        if empty_line {
            // empty line: height by the paragraph mark (shifted by w:position — oracle position)
            let mm = env.face_metrics(p.mark_face)?;
            // a superscript/subscript mark: the line has the NORMAL body height, as the text
            // (oracle supmark)
            let va = mark_style.rpr.vert_align.as_deref();
            let sz = (if va == Some("superscript") || va == Some("subscript") {
                mark_style.rpr.sz.map_or(20.0, |v| if v.is_null() { 20.0 } else { v.to_number() }) / 2.0
            } else {
                mark_style.size_pt
            }) * 20.0;
            // `va ? 0 : shift` (a non-empty string is truthy)
            let sh = if va.is_some_and(|v| !v.is_empty()) { 0.0 } else { mark_style.shift };
            // (the mark's w14:shadow counts too — oracle shadowh)
            let (ea, ed) = shadow_extent(mark_style);
            asc = jm::max(0.0, mm.ascent * sz + ea + sh);
            desc = jm::max(0.0, mm.descent * sz + ed - sh);
        }
        if t_asc + t_desc == 0.0 {
            // objects only: the text height (for multiple spacing) comes from the object RUN's face
            // (oracle imgline); empty line: the mark's box ALREADY shifted (pos-empty15); else the
            // paragraph mark
            let mm = env.face_metrics(p.mark_face)?;
            t_asc = if uo_jsnum::conv::truthy(o_asc) {
                o_asc
            } else if empty_line {
                asc
            } else {
                mm.ascent * mark_style.size_pt * 20.0
            };
            t_desc = if uo_jsnum::conv::truthy(o_asc) {
                o_desc
            } else if empty_line {
                desc
            } else {
                mm.descent * mark_style.size_pt * 20.0
            };
        }
        // numbering marker: only the ASCENT enters the line (the descent and multiple spacing follow
        // the text) — oracle markers
        if let Some(mp) = &marker_placed {
            for q in mp {
                asc = jm::max(asc, view(p, q)?.asc);
            }
        }
        let (height, baseline) = line_box(asc, desc, p, if t_asc + t_desc > 0.0 { Some(t_asc + t_desc) } else { None }, self.grid);

        // alignment/justification
        let avail = right - x_start;
        let mut dx = 0.0;
        let jc = &*p.jc;
        // East Asian layout of modes ≤ 14 (oracle fecompress): the hanging punctuation stays past
        // the margin and does not count in the alignment; unjustified, the spaces compress what
        // passes the margin
        let mut last_c: Option<usize> = None;
        let mut views: Vec<Cow<'_, Glyph>> = Vec::with_capacity(placed.len());
        for (k, q) in placed.iter().enumerate() {
            let v = view(p, q)?;
            if !matches!(v.kind, GlyphKind::Space | GlyphKind::Lb | GlyphKind::Pb | GlyphKind::Cb) {
                last_c = Some(k);
            }
            views.push(v);
        }
        let align_w = match last_c {
            Some(k) if views.get(k).is_some_and(|v| v.flags.has(GlyphFlags::HANG)) && width > avail + FIT_EPS => {
                width - placed.get(k).map_or(f64::NAN, |q| q.w)
            }
            _ => width,
        };
        let justified = (jc == "both" || jc == "distribute") && (!last || jc == "distribute") && break_after.is_none();
        if !justified && align_w > avail + FIT_EPS {
            // `(lastC?.x ?? 0)`
            let lc_x = last_c.and_then(|k| placed.get(k)).map_or(0.0, |q| q.x);
            // `pg.g.feShrink && pg.x < …`: feShrink truthy
            let sh: Vec<bool> =
                placed.iter().zip(&views).map(|(q, v)| v.fe_shrink.get().is_some_and(uo_jsnum::conv::truthy) && q.x < lc_x).collect();
            let mut total = 0.0;
            for (k, v) in views.iter().enumerate() {
                if sh.get(k).copied().unwrap_or(false) {
                    total += v.fe_shrink.or(f64::NAN);
                }
            }
            if total > 0.0 {
                let f = jm::min(1.0, (align_w - avail) / total);
                let mut acc = 0.0;
                for (k, q) in placed.iter_mut().enumerate() {
                    q.x -= acc;
                    let fe = views.get(k).and_then(|v| v.fe_shrink.get());
                    if let (Some(fe), true) = (fe, sh.get(k).copied().unwrap_or(false))
                        && uo_jsnum::conv::truthy(fe)
                    {
                        q.w -= fe * f;
                        acc += fe * f;
                    }
                }
            }
        }
        if jc == "center" {
            dx = (avail - align_w) / 2.0;
        } else if jc == "right" {
            dx = avail - align_w;
        } else if justified {
            // inner spaces (not trailing, after the last tab)
            let mut last_tab: Option<usize> = None;
            let mut last_content: Option<usize> = None;
            for (k, v) in views.iter().enumerate() {
                if v.kind == GlyphKind::Tab {
                    last_tab = Some(k);
                }
                if v.kind != GlyphKind::Space && v.kind != GlyphKind::Lb {
                    last_content = Some(k);
                }
            }
            let mut first_content = placed.len();
            for (k, v) in views.iter().enumerate().skip(last_tab.map_or(0, |t| t + 1)) {
                if v.kind != GlyphKind::Space {
                    first_content = k;
                    break;
                }
            }
            let spaces: Vec<bool> = views
                .iter()
                .enumerate()
                .map(|(k, v)| {
                    k > first_content
                        && last_content.is_some_and(|lc| k < lc)
                        && v.kind == GlyphKind::Space
                        && !fixed_space(v.cp)
                        && !v.flags.has(GlyphFlags::FIXED)
                })
                .collect();
            let n_sp = spaces.iter().filter(|b| **b).count();
            let extra = avail - align_w;
            // `extra > 0 || extra < 0` (false for NaN)
            if n_sp > 0 && uo_jsnum::conv::truthy(extra) {
                let each = extra / n_sp as f64;
                let mut acc = 0.0;
                for (k, q) in placed.iter_mut().enumerate() {
                    q.x += acc;
                    if spaces.get(k).copied().unwrap_or(false) {
                        q.w += each;
                        acc += each;
                    }
                }
            }
        }
        // `dx > 0 || dx < 0`
        if uo_jsnum::conv::truthy(dx) {
            for q in &mut placed {
                q.x += dx;
            }
            if jc != "left"
                && let Some(mp) = marker_placed.as_mut()
            {
                for q in mp {
                    q.x += dx;
                }
            }
        }
        let hr = views.iter().any(|v| has_hr(v));
        drop(views);
        drop(content);
        env.charge(work)?;
        Ok(Line {
            start: u32::try_from(start).map_err(|_| LayoutError::Internal("lines: glyph index"))?,
            end: self.pos,
            placed,
            marker: marker_placed,
            width,
            x_start: x_start + jm::max(0.0, dx),
            asc,
            desc,
            height,
            baseline,
            break_after,
            clear: clear_after,
            last,
            gap_tab,
            spanned,
            hr_frame: if hr { Some((frame.left, frame.right)) } else { None },
        })
    }
}

/// A TS leader name as `TabLeader` (`TabSpec['leader']`). A name the enum does not know paints as
/// `'dot'` (paint.ts:105 falls through to `'.'`) and is mapped to it (F5-6).
#[must_use]
pub fn leader_from_ts(s: &str) -> TabLeader {
    match s {
        "none" => TabLeader::None,
        "hyphen" => TabLeader::Hyphen,
        "underscore" => TabLeader::Underscore,
        "heavy" => TabLeader::Heavy,
        "middleDot" => TabLeader::MiddleDot,
        _ => TabLeader::Dot,
    }
}

/// `run.ptab` of a glyph's run (the TS `g.ptab`, the run's object; a slice keeps its base run's).
fn ptab_of_cx(cx: &Cx<'_, '_>, p: &PreparedPara, g: &Glyph) -> Option<PtabSpec> {
    let _ = p;
    let run = crate::paint::model_run(cx.ldoc, g.ptab?)?;
    let pt = run.ext()?.ptab.as_ref()?;
    let alignment = match &*pt.alignment {
        "center" => PtabAlign::Center,
        "right" => PtabAlign::Right,
        _ => PtabAlign::Left,
    };
    // `g.ptab.leader && g.ptab.leader !== 'none'`
    let leader = pt.leader.as_deref().filter(|l| !l.is_empty() && *l != "none").map(leader_from_ts);
    Some(PtabSpec { alignment, leader })
}

impl LineEnv for Cx<'_, '_> {
    fn settings(&self) -> BreakSettings {
        let st = &self.ldoc.view.formatting().settings;
        let nullish = |v: Option<crate::model::JsNumber>| v.and_then(crate::model::JsNumber::as_f64);
        BreakSettings {
            default_tab_stop: st.default_tab_stop.to_number(),
            compatibility_mode: st.compatibility_mode.to_number(),
            no_indent_as_numbering_tab_stop: st.compat.get("doNotUseIndentAsNumberingTabStop").copied().unwrap_or(false),
            split_pg_break_and_para_mark: st.compat.get("splitPgBreakAndParaMark").copied().unwrap_or(false),
            consecutive_hyphen_limit: nullish(st.consecutive_hyphen_limit).unwrap_or(0.0),
            hyphenation_zone: nullish(st.hyphenation_zone),
        }
    }

    fn advance_twips(&mut self, face: FaceId, cp: u32, size_pt: f64) -> LResult<f64> {
        crate::para::advance_twips(self, face, cp, size_pt)
    }

    fn face_metrics(&mut self, face: FaceId) -> LResult<FaceMetrics> {
        crate::paint::face_metrics_of(self, face)
    }

    fn ptab(&self, p: &PreparedPara, g: &Glyph) -> Option<PtabSpec> {
        ptab_of_cx(self, p, g)
    }

    fn charge(&mut self, units: u64) -> LResult<()> {
        self.budget.charge(units)
    }
}
