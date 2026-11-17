//! The animation side of the emission (frontier B4b, `uo-render.md` S6): the slots the TS painter's hooks open
//! (`shapeOpacity`/`shapeAnim`/`mediaFrame` of `paintShapeList`, `hooks.bg` of `paintShape`, `hooks.chartParts` of
//! `paintBody`, `hooks.para`/`hooks.unit` of `paintText`, render.ts) and the per-frame style overrides of a re-emitted
//! shape.
//!
//! The TS is lazy: the painter calls each hook with the geometry it computes while painting, and what it paints depends on
//! which hooks exist (a paragraph with a state draws its runs without the fit spacing, a text entering by unit is drawn
//! piece by piece). That STRUCTURE depends only on the placed effects of the slide, not on the time, so the caller
//! (`uo_pptx`) describes it per shape in an [`AnimPlan`] and the emitter writes the slots where the TS calls the hooks:
//! one `SlotBegin(i)`/`SlotEnd` bracket per call site, and an [`EmitSlot`] per slot with the base the TS passes the hook
//! (`boxBase`, render.ts:106). The per-frame values are `uo_pptx::evaluate_slots` over those bases.
//!
//! A state carrying a field `applyPart` does not apply (colours, font weight/style/family, underline, a font scale) is the
//! re-emit rule: the caller emits that shape again with a [`SlotStates`] that answers each slot's state, and the emitter
//! applies the style fields as the TS does (`fontOf`, `colorOf`, `underOf`, `fillOv`/`lineOv`, the font scale of the layout).

pub use uo_charts::emit::{ChartSel, DataSel};

use crate::color::CssColor;
use crate::model::TextBody;
use crate::text::PlacedLine;

/// A text unit (`UnitRef`, anim-engine.ts:113): global and per-paragraph index and count.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UnitRef {
    pub para: u32,
    pub k: u32,
    pub count: u32,
    pub k_para: u32,
    pub count_para: u32,
}

/// Where a slot sits (the TS hook it stands for).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmitSlotKind {
    /// the whole shape: `shapeOpacity` + `hooks.all` (every shape of a slide has one)
    Shape,
    /// the body: `hooks.bg`
    Background,
    /// paragraph `i`: `hooks.para(i, …)`
    Paragraph(u32),
    /// a letter or word: `hooks.unit.state(unit, …, word)`
    Unit(UnitRef),
    /// the bullet of a paragraph entering by unit: the state of its first unit only gates it
    UnitGate(UnitRef),
    /// chart part `j`: `hooks.chartParts[j].state(…)` (applied on the part's own `save`)
    ChartPart(u32),
    /// the `chartPartBounds` probe of chart part `j` on a bounds-cache miss: drawn in the first frame of a stage only
    BoundsPass(u32),
}

/// One slot of an emission: what the hook receives (`AnimBase`) and what `applyPart` needs (`bx`, `base_rot_rad`).
#[derive(Clone, PartialEq, Debug)]
pub struct EmitSlot {
    pub kind: EmitSlotKind,
    /// the shape's index in the slide
    pub shape: u32,
    /// `boxBase(...)`: centre and size as fractions of the slide (`x, y, w, h`)
    pub base: [f64; 4],
    /// the part's box in px (`x, y, w, h` of `applyPart`)
    pub bx: [f64; 4],
    /// `applyPart`'s `baseRotRad` (the shape slot only)
    pub base_rot_rad: f64,
    /// `AnimBase.rot` (degrees; the shape and background slots)
    pub rot_deg: Option<f64>,
    /// `AnimBase.fill`, `line`, `text`: the starting colours of `animClr` without `from`
    pub fill: Option<CssColor>,
    pub line: Option<CssColor>,
    pub text: Option<CssColor>,
    /// the letter's word (`unit.state(…, word)`)
    pub word: Option<UnitRef>,
}

/// The text-unit hook of a shape (`hooks.unit`): letters or words, a word effect too, and the paragraph range of each
/// unit effect (`None` = the whole body). A unit has a state when an effect's range holds its paragraph.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct UnitHook {
    pub letters: bool,
    pub words: bool,
    pub ranges: Vec<Option<(f64, f64)>>,
}

impl UnitHook {
    /// `unit.state(...)` is not null for a unit of paragraph `para` (anim-engine.ts:670).
    #[must_use]
    pub fn covers(&self, para: u32) -> bool {
        let p = f64::from(para);
        self.ranges.iter().any(|r| r.is_none_or(|(a, b)| !(p < a || p > b)))
    }
}

/// One part of a chart animated by parts (`hooks.chartParts[j]`): its drawing selection and whether it has a state.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartPart {
    pub sel: ChartSel,
    /// the TS `sel.key` (the bounds cache key; the static rest has none)
    pub key: Option<Box<str>>,
    pub animated: bool,
    /// its effects may produce a soft-edged reveal (`wipe`): the part is drawn in a soft layer
    pub soft: bool,
}

/// What the TS hooks of one animated shape are (`ShapeAnimHooks` without the time).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ShapeAnim {
    /// `hooks.bg` exists
    pub bg: bool,
    /// the ranges of `hooks.para` (empty = no paragraph hook); paragraph `i` has a state when a range holds it
    pub paras: Vec<(f64, f64)>,
    pub unit: Option<UnitHook>,
    pub chart: Option<Vec<ChartPart>>,
    /// the shape's effects may produce a soft-edged reveal (`wipe`): the shape is drawn in a soft layer
    pub soft: bool,
}

impl ShapeAnim {
    /// `hooks.para(i, …)` is not null.
    #[must_use]
    pub fn para_covers(&self, i: u32) -> bool {
        let p = f64::from(i);
        self.paras.iter().any(|&(a, b)| p >= a && p <= b)
    }
}

/// The style fields of a state (`PartState` minus what `applyPart` applies): what a re-emission draws with.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PartStyle {
    pub fill_color: Option<CssColor>,
    pub line_color: Option<CssColor>,
    pub text_color: Option<CssColor>,
    pub font_weight: Option<Box<str>>,
    pub font_style: Option<Box<str>>,
    pub underline: Option<bool>,
    pub font_scale: Option<f64>,
    pub font_family: Option<Box<str>>,
}

/// The states of a re-emitted shape, slot by slot (the caller evaluates the hooks over the slot's base).
pub trait SlotStates {
    /// The style of the state the TS hook would return for `slot` (only asked for slots that have a state).
    fn style(&mut self, slot: &EmitSlot) -> PartStyle;
}

/// The animation input of an emission: the hooks of each shape (by index; `None` = static) and whether the media frames
/// are wired (`hasMedia` of the viewer: pictures of media and GIF shapes then draw through `MediaFrame`).
#[derive(Default)]
pub struct AnimPlan<'a> {
    pub shapes: &'a [Option<ShapeAnim>],
    pub media: bool,
    /// a re-emission: the states of the slots (one shape, `EmitParams::only`)
    pub states: Option<&'a mut (dyn SlotStates + 'a)>,
}

impl std::fmt::Debug for AnimPlan<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimPlan")
            .field("shapes", &self.shapes)
            .field("media", &self.media)
            .field("states", &self.states.is_some())
            .finish()
    }
}

// ------------------------------------------------------------------------------------------------ planUnits

/// A piece of a run drawn as one unit (`UnitPiece`, render.ts:1960): its text, the UTF-16 offset in the run's text, its
/// unit and the letter's word.
#[derive(Clone, PartialEq, Debug)]
pub struct UnitPiece {
    /// UTF-16 code units of the piece (a letter may be half of a surrogate pair, as the TS `/\S/g` cuts it)
    pub text: Vec<u16>,
    pub start: usize,
    pub unit: UnitRef,
    pub word: Option<UnitRef>,
}

/// `planUnits` (render.ts:1985): `pieces[line][run]`, the first unit (and word) of each paragraph.
pub struct UnitPlan {
    pub pieces: Vec<Vec<Vec<UnitPiece>>>,
    letters: bool,
    with_words: bool,
    /// per paragraph: (first letter index, letters in the paragraph or None, word base, words or None)
    first: Vec<(u32, u32, Option<u32>, u32, Option<u32>)>,
    words_total: u32,
    letters_total: u32,
}

fn is_space(u: u16) -> bool {
    uo_jsnum::text::is_js_space_unit(u)
}

/// `/\S+/g` (or `/\S/g` with `single`) over UTF-16 code units: `(start, end)` of each match.
fn non_space_runs(t: &[u16], single: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t.get(i).copied().is_some_and(is_space) {
            i += 1;
            continue;
        }
        if single {
            out.push((i, i + 1));
            i += 1;
            continue;
        }
        let s = i;
        while i < t.len() && !t.get(i).copied().is_some_and(is_space) {
            i += 1;
        }
        out.push((s, i));
    }
    out
}

/// `String.prototype.indexOf(needle, from)` over UTF-16 code units (-1 = none).
fn index_of(hay: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(hay.len()));
    }
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay.get(i..i + needle.len()) == Some(needle))
}

impl UnitPlan {
    /// `planUnits(tb, layout, type, withWords)`.
    #[must_use]
    pub fn new(tb: &TextBody, lines: &[PlacedLine], letters: bool, with_words: bool) -> UnitPlan {
        // runs of each paragraph in order (line index, run index), first appearance order of the paragraphs
        let mut by_para: Vec<(u32, Vec<(usize, usize)>)> = Vec::new();
        for (li, line) in lines.iter().enumerate() {
            let slot = match by_para.iter().position(|(p, _)| *p == line.para) {
                Some(i) => i,
                None => {
                    by_para.push((line.para, Vec::new()));
                    by_para.len() - 1
                }
            };
            if let Some((_, l)) = by_para.get_mut(slot) {
                l.extend((0..line.runs.len()).map(|ri| (li, ri)));
            }
        }
        let run_text = |li: usize, ri: usize| -> Vec<u16> {
            lines.get(li).and_then(|l| l.runs.get(ri)).map(|r| r.text.encode_utf16().collect()).unwrap_or_default()
        };
        // per paragraph: offset of each run, word index per code unit, word count
        let mut run_off: Vec<Vec<usize>> = lines.iter().map(|l| vec![0; l.runs.len()]).collect();
        let mut word_at: Vec<(u32, Vec<i32>)> = Vec::new();
        let mut word_count: Vec<(u32, u32)> = Vec::new();
        for (pi, runs) in &by_para {
            let orig: Vec<u16> = tb
                .paragraphs
                .get(usize::try_from(*pi).unwrap_or(usize::MAX))
                .map(|p| p.runs.iter().flat_map(|r| r.text.encode_utf16()).collect())
                .unwrap_or_default();
            let mut text = orig.clone();
            let mut offs: Vec<usize> = Vec::new();
            let mut cursor = 0usize;
            let mut ok = true;
            for &(li, ri) in runs {
                let rt = run_text(li, ri);
                match index_of(&orig, &rt, cursor) {
                    Some(idx) if !orig.get(cursor..idx).unwrap_or(&[]).iter().any(|&u| !is_space(u)) => {
                        offs.push(idx);
                        cursor = idx + rt.len();
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                // fallback: the text of the placed runs, lines separated
                text.clear();
                offs.clear();
                let mut prev = false;
                for line in lines {
                    if line.para != *pi {
                        continue;
                    }
                    if prev {
                        text.push(u16::from(b'\n'));
                    }
                    prev = true;
                    for r in &line.runs {
                        offs.push(text.len());
                        text.extend(r.text.encode_utf16());
                    }
                }
            }
            for (i, &(li, ri)) in runs.iter().enumerate() {
                if let Some(o) = run_off.get_mut(li).and_then(|l| l.get_mut(ri)) {
                    *o = offs.get(i).copied().unwrap_or(0);
                }
            }
            let mut at = vec![-1i32; text.len()];
            let mut k = 0i32;
            for (s, e) in non_space_runs(&text, false) {
                for v in at.get_mut(s..e).into_iter().flatten() {
                    *v = k;
                }
                k += 1;
            }
            word_at.push((*pi, at));
            word_count.push((*pi, u32::try_from(k).unwrap_or(0)));
        }
        let mut paras: Vec<u32> = by_para.iter().map(|(p, _)| *p).collect();
        paras.sort_unstable();
        let count_of = |pi: u32| word_count.iter().find(|(p, _)| *p == pi).map(|(_, c)| *c);
        let mut word_base: Vec<(u32, u32)> = Vec::new();
        let mut w_total = 0u32;
        for &pi in &paras {
            word_base.push((pi, w_total));
            w_total = w_total.saturating_add(count_of(pi).unwrap_or(0));
        }
        let base_of = |pi: u32| word_base.iter().find(|(p, _)| *p == pi).map_or(0, |(_, b)| *b);
        let word_ref = |pi: u32, kp: u32| UnitRef {
            para: pi,
            k: base_of(pi).saturating_add(kp),
            count: w_total,
            k_para: kp,
            count_para: count_of(pi).unwrap_or(1),
        };
        let at_of = |pi: u32| word_at.iter().find(|(p, _)| *p == pi).map(|(_, a)| a.as_slice()).unwrap_or(&[]);
        let word_index = |pi: u32, i: usize| -> u32 { u32::try_from(at_of(pi).get(i).copied().unwrap_or(0).max(0)).unwrap_or(0) };
        let mut pieces: Vec<Vec<Vec<UnitPiece>>> = lines.iter().map(|l| vec![Vec::new(); l.runs.len()]).collect();
        let mut first = Vec::new();
        let mut letters_total = 0u32;
        if !letters {
            for &pi in &paras {
                let Some((_, runs)) = by_para.iter().find(|(p, _)| *p == pi) else { continue };
                for &(li, ri) in runs {
                    let rt = run_text(li, ri);
                    let off = run_off.get(li).and_then(|l| l.get(ri)).copied().unwrap_or(0);
                    let list: Vec<UnitPiece> = non_space_runs(&rt, false)
                        .into_iter()
                        .map(|(s, e)| UnitPiece {
                            text: rt.get(s..e).unwrap_or(&[]).to_vec(),
                            start: s,
                            unit: word_ref(pi, word_index(pi, off + s)),
                            word: None,
                        })
                        .collect();
                    if let Some(slot) = pieces.get_mut(li).and_then(|l| l.get_mut(ri)) {
                        *slot = list;
                    }
                }
                first.push((pi, 0, None, base_of(pi), count_of(pi)));
            }
        } else {
            let mut per_para: Vec<(u32, u32)> = Vec::new();
            let mut first_l: Vec<(u32, u32)> = Vec::new();
            let mut total = 0u32;
            for &pi in &paras {
                first_l.push((pi, total));
                let Some((_, runs)) = by_para.iter().find(|(p, _)| *p == pi) else { continue };
                for &(li, ri) in runs {
                    let rt = run_text(li, ri);
                    let n = u32::try_from(rt.iter().filter(|&&u| !is_space(u)).count()).unwrap_or(u32::MAX);
                    total = total.saturating_add(n);
                    match per_para.iter_mut().find(|(p, _)| *p == pi) {
                        Some((_, c)) => *c = c.saturating_add(n),
                        None => per_para.push((pi, n)),
                    }
                }
            }
            let per_of = |pi: u32| per_para.iter().find(|(p, _)| *p == pi).map(|(_, c)| *c);
            let mut k = 0u32;
            for &pi in &paras {
                let mut kp = 0u32;
                let Some((_, runs)) = by_para.iter().find(|(p, _)| *p == pi) else { continue };
                for &(li, ri) in runs {
                    let rt = run_text(li, ri);
                    let off = run_off.get(li).and_then(|l| l.get(ri)).copied().unwrap_or(0);
                    let mut list = Vec::new();
                    for (s, e) in non_space_runs(&rt, true) {
                        let unit = UnitRef { para: pi, k, count: total, k_para: kp, count_para: per_of(pi).unwrap_or(1) };
                        k = k.saturating_add(1);
                        kp = kp.saturating_add(1);
                        let word = if with_words { Some(word_ref(pi, word_index(pi, off + s))) } else { None };
                        list.push(UnitPiece { text: rt.get(s..e).unwrap_or(&[]).to_vec(), start: s, unit, word });
                    }
                    if let Some(slot) = pieces.get_mut(li).and_then(|l| l.get_mut(ri)) {
                        *slot = list;
                    }
                }
                let fl = first_l.iter().find(|(p, _)| *p == pi).map_or(0, |(_, f)| *f);
                first.push((pi, fl, per_of(pi), base_of(pi), count_of(pi)));
            }
            letters_total = total;
        }
        UnitPlan { pieces, letters, with_words, first, words_total: w_total, letters_total }
    }

    /// `firstRef(pi)`: the paragraph's first unit (the bullet follows it).
    #[must_use]
    pub fn first_ref(&self, pi: u32) -> UnitRef {
        let f = self.first.iter().find(|f| f.0 == pi);
        if self.letters {
            UnitRef {
                para: pi,
                k: f.map_or(0, |f| f.1),
                count: self.letters_total,
                k_para: 0,
                count_para: f.and_then(|f| f.2).unwrap_or(1),
            }
        } else {
            self.word0(pi)
        }
    }

    fn word0(&self, pi: u32) -> UnitRef {
        let f = self.first.iter().find(|f| f.0 == pi);
        UnitRef { para: pi, k: f.map_or(0, |f| f.3), count: self.words_total, k_para: 0, count_para: f.and_then(|f| f.4).unwrap_or(1) }
    }

    /// `firstWordRef(pi)`: the first word when letters carry their word too.
    #[must_use]
    pub fn first_word_ref(&self, pi: u32) -> Option<UnitRef> {
        if self.letters && self.with_words { Some(self.word0(pi)) } else { None }
    }
}
