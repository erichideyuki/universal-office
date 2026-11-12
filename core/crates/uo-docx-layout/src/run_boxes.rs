//! Where runs ended up on the pages (frontier F8): `run-boxes.ts` (`runBoxes`,
//! `collapseCustomInlineChars`) — the clickable links of the viewer and the PDF, the plugin
//! inlines the shell draws — and, for the plugin renderers (K17e), what `paintCustomInlines`
//! (inline-renderer.ts:50) reads of the page: the glyph run over the box (`run0`: its face, size
//! and colour) and its `measureText`.
//!
//! Only body lines (refs `b…`); headers, footers and notes are left out, as in the TS.

use std::sync::Arc;

use crate::hit::{CharsView, collapse_custom_chars};
use crate::model::{DocView, RunProps, RunRef, StoryRef};
use crate::output::{CharsSoA, Layout, LineStory};
use crate::refs::Locator;
use uo_jsnum::math as jm;
use uo_render::FaceId;

/// Which runs [`run_boxes`] reports (the TS `pick(run)` callbacks of its two callers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunFilter {
    /// `r.custom ? undefined : r.props.link` (`pdfNavigation`, the viewer's link layer)
    Links,
    /// `r.custom` (the plugin inlines the shell draws over their fallback text)
    CustomInlines,
}

/// `RunBox` (run-boxes.ts:13): the box of a picked run on a line, page twips.
#[derive(Clone, Debug)]
pub struct RunBox {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub baseline: f64,
    /// the model run (`run`)
    pub run: RunRef,
    /// its props (K17e: the plugin contract hands `props` to the renderer)
    pub props: Arc<RunProps>,
    /// the picked value: the link (UTF-16) or the plugin inline's `type`
    pub value: Box<[u16]>,
}

/// `runBoxes(full, layout, pick)` (run-boxes.ts:57).
#[must_use]
pub fn run_boxes(view: DocView<'_>, layout: &Layout, pick: RunFilter) -> Vec<RunBox> {
    let loc = Locator::new(view);
    run_boxes_with(&loc, layout, pick)
}

/// [`run_boxes`] over a [`Locator`] the caller already built.
#[must_use]
pub fn run_boxes_with(loc: &Locator<'_>, layout: &Layout, pick: RunFilter) -> Vec<RunBox> {
    let mut out = Vec::new();
    let mut own = CharsSoA::default();
    for (pi, page) in layout.pages.iter().enumerate() {
        let pi = u32::try_from(pi).unwrap_or(u32::MAX);
        for line in &page.lines {
            // `/^b\d/.test(ref) ? paragraphAtPath(full.blocks, ref.split('/')) : null`
            if line.at.story != LineStory::Body {
                continue;
            }
            let Some(path) = loc.body_path(&line.at.path.0) else { continue };
            let Some(p) = loc.story_paragraph(&StoryRef::Body, &path) else { continue };
            let raw = CharsView::of(page, line);
            if raw.is_empty() {
                continue;
            }
            let chars = if p.runs.iter().any(|r| r.ext().is_some_and(|e| e.custom.is_some())) {
                own.o.clear();
                own.x.clear();
                own.w.clear();
                own.o.extend_from_slice(raw.o);
                own.x.extend_from_slice(raw.x);
                own.w.extend_from_slice(raw.w);
                collapse_custom_chars(&mut own, p);
                CharsView { o: &own.o, x: &own.x, w: &own.w }
            } else {
                raw
            };
            let mut acc: u32 = 0;
            for (ri, run) in p.runs.iter().enumerate() {
                let a = acc;
                acc = acc.wrapping_add(u32::try_from(run.text.len()).unwrap_or(u32::MAX));
                if acc <= line.start || a >= line.end {
                    continue;
                }
                let custom = run.ext().and_then(|e| e.custom.as_deref());
                let value: Box<[u16]> = match pick {
                    RunFilter::Links => match (custom, run.props.link.as_deref()) {
                        (None, Some(l)) if !l.is_empty() => l.encode_utf16().collect(),
                        _ => continue,
                    },
                    RunFilter::CustomInlines => match custom {
                        Some(c) => c.r#type.encode_utf16().collect(),
                        None => continue,
                    },
                };
                let (mut x1, mut x2) = (f64::INFINITY, f64::NEG_INFINITY);
                for k in 0..chars.len() {
                    let (Some(&o), Some(&x), Some(&w)) = (chars.o.get(k), chars.x.get(k), chars.w.get(k)) else { continue };
                    if o < a || o >= acc {
                        continue;
                    }
                    x1 = jm::min(x1, x);
                    x2 = jm::max(x2, x + w);
                }
                #[expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x2 > x1)` (NaN skips the box)")]
                if !(x2 > x1) {
                    continue;
                }
                out.push(RunBox {
                    page: pi,
                    x: x1,
                    y: line.y,
                    w: x2 - x1,
                    h: line.height,
                    baseline: line.baseline,
                    run: RunRef {
                        story: StoryRef::Body,
                        path: path.clone(),
                        run: u32::try_from(ri).unwrap_or(u32::MAX),
                        sub: Default::default(),
                    },
                    props: Arc::clone(&run.props),
                    value,
                });
            }
        }
    }
    out
}

/// What `paintCustomInlines` reads for a plugin inline's box: the glyph run drawn over it
/// (`run0`), its face, its size (`run0.sizePt ?? (props.szHalfPt ?? 22) / 2`) and its colour
/// (`run0.color ?? props.color ?? '000000'`).
#[derive(Clone, Debug, PartialEq)]
pub struct InlineFace {
    pub face: Option<FaceId>,
    pub size_pt: f64,
    pub color: Box<[u16]>,
}

impl Layout {
    /// `paintCustomInlines`' `run0` lookup for a box: the first glyph run within 400 twips of the
    /// baseline with a glyph inside the box (`g.x >= box.x - 0.5 && g.x < box.x + box.w`).
    #[must_use]
    pub fn custom_inline_face(&self, b: &RunBox) -> InlineFace {
        let run0 = self.pages.get(b.page as usize).and_then(|p| {
            let list = &p.page.list;
            list.items.iter().find_map(|it| match *it {
                uo_render::types::Item::Glyphs { face, color, size_pt, y, glyphs, .. }
                    if jm::abs(y - b.baseline) < 400.0 && list.glyphs_of(glyphs).any(|(_, gx, _)| gx >= b.x - 0.5 && gx < b.x + b.w) =>
                {
                    Some((face, color, size_pt))
                }
                _ => None,
            })
        });
        match run0 {
            Some((face, color, size_pt)) => InlineFace { face: Some(face), size_pt, color: self.resources.sym(color).into() },
            None => {
                let half = match b.props.sz_half_pt {
                    Some(crate::model::JsNumber::Num(v)) => v,
                    _ => 22.0,
                };
                let color: Box<[u16]> = match b.props.color.as_deref() {
                    Some(c) => c.encode_utf16().collect(),
                    None => "000000".encode_utf16().collect(),
                };
                InlineFace { face: None, size_pt: half / 2.0, color }
            }
        }
    }

    /// `measureText(text)` of `paintCustomInlines`: the advances of `text`'s code points in
    /// `face` at `size_pt` (`advanceWidth(glyphId(cp)) * sizePt * 20 / (unitsPerEm || 1000)`),
    /// or `text.length * sizePt * 10` without a face.
    #[must_use]
    pub fn measure_text(&self, face: Option<FaceId>, size_pt: f64, text: &[u16]) -> f64 {
        let Some(font) = face.and_then(|f| self.resources.face(f)).and_then(|f| f.face.as_ref()) else {
            return f64::from(u32::try_from(text.len()).unwrap_or(u32::MAX)) * size_pt * 10.0;
        };
        let upm = font.metrics().units_per_em;
        let upm = if upm == 0 { 1000.0 } else { f64::from(upm) };
        let mut w = 0.0;
        let mut k = 0;
        while k < text.len() {
            let hi = text.get(k).copied().unwrap_or(0);
            let lo = text.get(k + 1).copied().unwrap_or(0);
            let (cp, n) = if (0xd800..0xdc00).contains(&hi) && (0xdc00..0xe000).contains(&lo) {
                (0x10000 + ((u32::from(hi) - 0xd800) << 10) + (u32::from(lo) - 0xdc00), 2)
            } else {
                (u32::from(hi), 1)
            };
            w += (f64::from(font.advance_width(font.glyph_id(cp))) * size_pt * 20.0) / upm;
            k += n;
        }
        w
    }
}
