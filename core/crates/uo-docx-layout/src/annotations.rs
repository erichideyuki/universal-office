//! The viewer's annotations (bindings gate item G8; record "Write page section (G8)"): a port of
//! `collectAnnotations(full, layout)` (viewer `annotations.ts`) — the comments with the rectangles
//! of their commented text on the pages (merged along a line) in reading order, and the headings
//! (level, text, page, top) of the outline pane. It reads the model and the lines of every page,
//! so it is a layout query, not part of the page message.

use rustc_hash::FxHashMap;

use crate::model::{DocView, NumKey};
use crate::output::Layout;
use crate::refs::Locator;
use crate::section::{outline_level, paragraph_text, text_layer_paragraph};
use crate::word::StyleResolver;
use uo_jsnum::math;

/// A rectangle of commented text (twips of page `page`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnnRect {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `CommentEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct CommentEntry {
    pub id: f64,
    /// only when the comment has a non-empty author (`...(c.author ? { author } : {})`)
    pub author: Option<Box<[u16]>>,
    pub text: Box<[u16]>,
    /// the commented text on the pages (empty: no visible anchor)
    pub rects: Vec<AnnRect>,
}

/// `HeadingEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct HeadingEntry {
    /// `levelResolver` (1–9 in practice)
    pub level: f64,
    /// the paragraph text with white space collapsed and trimmed
    pub text: Box<[u16]>,
    pub page: u32,
    pub y: f64,
}

/// `Annotations`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Annotations {
    pub comments: Vec<CommentEntry>,
    pub headings: Vec<HeadingEntry>,
}

/// `s.replace(/\s+/g, ' ').trim()`.
fn collapse(units: &[u16]) -> Box<[u16]> {
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut space = false;
    for &c in units {
        if uo_jsnum::text::is_js_space_unit(c) {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(u16::from(b' '));
        }
        space = false;
        out.push(c);
    }
    out.into_boxed_slice()
}

/// `collectAnnotations(full, layout)` (annotations.ts).
#[must_use]
pub fn annotations(view: DocView<'_>, layout: &Layout) -> Annotations {
    let loc = Locator::new(view);
    let mut styles = StyleResolver::new();
    let mut levels: FxHashMap<crate::model::ParaId, Option<f64>> = FxHashMap::default();
    let mut rects: FxHashMap<NumKey, Vec<AnnRect>> = FxHashMap::default();
    let mut headings = Vec::new();
    let mut seen: rustc_hash::FxHashSet<crate::model::ParaId> = rustc_hash::FxHashSet::default();
    for (pi, page) in (0u32..).zip(&layout.pages) {
        for line in &page.lines {
            let Some(p) = text_layer_paragraph(&loc, line) else { continue };
            // a heading: the first line of the paragraph
            if !seen.contains(&p.id) {
                let level = *levels.entry(p.id).or_insert_with(|| outline_level(view, &mut styles, p));
                if let Some(level) = level {
                    seen.insert(p.id);
                    let text = collapse(&paragraph_text(p));
                    if !text.is_empty() {
                        headings.push(HeadingEntry { level, text, page: pi, y: line.y });
                    }
                }
            }
            // commented text: the extent of each commented run's characters on the line
            let chars = line.chars.and_then(|s| s.range()).unwrap_or(0..0);
            let mut acc: f64 = 0.0;
            for run in &p.runs {
                let a = acc;
                acc += f64::from(u32::try_from(run.text.len()).unwrap_or(u32::MAX));
                let Some(ids) = run.props.comment_ids.as_deref().filter(|v| !v.is_empty()) else { continue };
                if acc <= f64::from(line.start) || a >= f64::from(line.end) {
                    continue;
                }
                let (mut x1, mut x2) = (f64::INFINITY, f64::NEG_INFINITY);
                for k in chars.clone() {
                    let (Some(&o), Some(&x), Some(&w)) = (page.chars.o.get(k), page.chars.x.get(k), page.chars.w.get(k)) else { continue };
                    let o = f64::from(o);
                    if o < a || o >= acc {
                        continue;
                    }
                    x1 = math::min(x1, x);
                    x2 = math::max(x2, x + w);
                }
                #[expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x2 > x1)` (NaN skips the box)")]
                if !(x2 > x1) {
                    continue;
                }
                for id in ids {
                    let list = rects.entry(NumKey::new(id.to_number())).or_default();
                    match list.last_mut() {
                        Some(last) if last.page == pi && last.y == line.y && math::abs(last.x + last.w - x1) < 1.0 => last.w = x2 - last.x,
                        _ => list.push(AnnRect { page: pi, x: x1, y: line.y, w: x2 - x1, h: line.height }),
                    }
                }
            }
        }
    }
    let mut comments: Vec<CommentEntry> = view
        .comments()
        .iter()
        .map(|c| {
            let id = c.id.to_number();
            CommentEntry {
                id,
                author: c.author.as_deref().filter(|a| !a.is_empty()).map(|a| a.encode_utf16().collect()),
                text: c.text.units().into(),
                rects: rects.get(&NumKey::new(id)).cloned().unwrap_or_default(),
            }
        })
        .collect();
    // reading order by the first anchor; none: at the end (`ka[0] - kb[0] || …`, a stable sort)
    let k = |c: &CommentEntry| c.rects.first().map_or([1e9, 0.0, 0.0], |r| [f64::from(r.page), r.y, r.x]);
    uo_jsnum::sort::js_sort_by(&mut comments, |a, b| {
        let ([a0, a1, a2], [b0, b1, b2]) = (k(a), k(b));
        // `||` goes on for 0 and NaN; the last difference is returned as is
        let d0 = a0 - b0;
        if d0 != 0.0 && !d0.is_nan() {
            return d0;
        }
        let d1 = a1 - b1;
        if d1 != 0.0 && !d1.is_nan() {
            return d1;
        }
        a2 - b2
    });
    Annotations { comments, headings }
}
