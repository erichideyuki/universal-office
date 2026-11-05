//! C# `ParagraphText`: a paragraph's text assembled from the `w:t` of its runs (through
//! hyperlinks, content controls, simple fields, smart tags…), to find and replace text Word
//! split over several runs. Tabs, breaks, symbols, objects and equations are one sentinel unit
//! (U+FFFF) that never matches. Positions are UTF-16 units, as in .NET.

use super::dom::{Dom, NS_M, NS_MC, NodeId, n};
use super::error::{ApiError, Result};
use super::net;
use super::ooxml;

/// The unit that stands for content without searchable text.
pub const SENTINEL: u16 = 0xFFFF;

#[derive(Debug, Clone, Copy)]
struct Segment {
    t: NodeId,
    start: usize,
    len: usize,
}

/// The text of one paragraph and where each `w:t` sits in it.
#[derive(Debug, Clone)]
pub struct ParagraphText {
    pub paragraph: NodeId,
    pub text: Vec<u16>,
    segments: Vec<Segment>,
}

const WALK_INTO: &[super::dom::Name] = &[n::hyperlink, n::smartTag, n::customXml, n::fldSimple, n::ins, n::moveTo, n::dir, n::bdo];
const SENTINEL_IN_RUN: &[super::dom::Name] =
    &[n::tab, n::br, n::cr, n::sym, n::noBreakHyphen, n::ptab, n::drawing, n::pict, n::object, n::footnoteReference, n::endnoteReference];

impl ParagraphText {
    pub fn build(dom: &Dom, paragraph: NodeId) -> ParagraphText {
        let mut text = Vec::new();
        let mut segments = Vec::new();
        // iterative walk: (container, next child index)
        let mut stack: Vec<(NodeId, usize)> = vec![(paragraph, 0)];
        while let Some((c, k)) = stack.pop() {
            let Some(&child) = dom.children(c).get(k) else { continue };
            stack.push((c, k + 1));
            if !dom.is_element(child) {
                continue;
            }
            if dom.is(child, n::r) {
                for rc in dom.elements(child) {
                    if dom.is(rc, n::t) {
                        let v = net::units(&dom.value(rc));
                        segments.push(Segment { t: rc, start: text.len(), len: v.len() });
                        text.extend(v);
                    } else if dom.is_any(rc, SENTINEL_IN_RUN) || dom.ns_of(rc) == NS_MC {
                        text.push(SENTINEL);
                    }
                }
            } else if dom.is_any(child, WALK_INTO) {
                stack.push((child, 0));
            } else if dom.is(child, n::sdt) {
                if let Some(content) = dom.element(child, n::sdtContent) {
                    stack.push((content, 0));
                }
            } else if dom.ns_of(child) == NS_M {
                text.push(SENTINEL);
            }
        }
        ParagraphText { paragraph, text, segments }
    }

    /// Occurrences (start, length), left to right, without overlap.
    pub fn find(&self, needle: &str, case_sensitive: bool, whole_word: bool) -> Vec<(usize, usize)> {
        let needle = net::units(needle);
        let mut out = Vec::new();
        if needle.is_empty() || needle.len() > self.text.len() {
            return out;
        }
        let mut from = 0;
        while from <= self.text.len() - needle.len() {
            let Some(at) = net::index_of(&self.text, &needle, from, !case_sensitive) else { break };
            let window = self.text.get(at..at + needle.len()).unwrap_or(&[]);
            if window.contains(&SENTINEL) || (whole_word && !is_whole_word(&self.text, at, needle.len())) {
                from = at + 1;
                continue;
            }
            out.push((at, needle.len()));
            from = at + needle.len();
        }
        out
    }

    /// Occurrences of `#\{[A-Za-z0-9]+\}` (C# `DocxText.AnyParameter` through `Find(Regex)`).
    pub fn find_any_parameter(&self) -> Vec<(usize, usize)> {
        let t = &self.text;
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < t.len() {
            if t.get(i) == Some(&u16::from(b'#')) && t.get(i + 1) == Some(&u16::from(b'{')) {
                let mut j = i + 2;
                while t.get(j).is_some_and(|&c| c < 0x80 && (c as u8).is_ascii_alphanumeric()) {
                    j += 1;
                }
                if j > i + 2 && t.get(j) == Some(&u16::from(b'}')) {
                    let m = (i, j + 1 - i);
                    if !t.get(i..=j).is_some_and(|w| w.contains(&SENTINEL)) {
                        out.push(m);
                    }
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// Replaces the span by the text: the new text goes into the run of the span's FIRST unit;
    /// runs left empty leave; tabs and line breaks become `w:tab`/`w:br`. Apply back to front.
    pub fn replace(&self, dom: &mut Dom, start: usize, length: usize, replacement: &str) -> Result<()> {
        let end = start + length;
        let mut touched: Vec<Segment> =
            self.segments.iter().copied().filter(|s| s.len > 0 && s.start < end && s.start + s.len > start).collect();
        if touched.is_empty() {
            let Some(host) = self.segments.iter().rposition(|s| s.start <= start && s.start + s.len >= start) else { return Ok(()) };
            if let Some(&s) = self.segments.get(host) {
                touched = vec![s];
            }
        }
        let Some(&first) = touched.first() else { return Ok(()) };
        let first_text = net::units(&dom.value(first.t));
        let local_start = start.saturating_sub(first.start).min(first_text.len());
        let local_start = if start < first.start { 0 } else { local_start };
        let suffix: Vec<u16> = if touched.len() == 1 {
            let cut = clamp_i(i64_of(end) - i64_of(first.start), local_start, first_text.len());
            first_text.get(cut..).unwrap_or(&[]).to_vec()
        } else {
            Vec::new()
        };
        let prefix = first_text.get(..local_start).unwrap_or(&[]).to_vec();
        replace_text_element(dom, first.t, &net::from_units(&prefix), replacement, &net::from_units(&suffix))?;
        let last = touched.len() - 1;
        for (k, seg) in touched.iter().enumerate().skip(1) {
            let text = net::units(&dom.value(seg.t));
            let rest = if k == last {
                let cut = clamp_i(i64_of(end) - i64_of(seg.start), 0, text.len());
                text.get(cut..).unwrap_or(&[]).to_vec()
            } else {
                Vec::new()
            };
            set_text(dom, seg.t, &net::from_units(&rest))?;
        }
        Ok(())
    }

    /// Replaces the span by an element of paragraph content created from a copy of the `w:rPr`
    /// of the span's first run (C# `ReplaceWithElement`).
    pub fn replace_with_element(
        &self,
        dom: &mut Dom,
        start: usize,
        length: usize,
        create: impl FnOnce(&mut Dom, Option<NodeId>) -> NodeId,
    ) -> Result<NodeId> {
        let placeholder = pick_placeholder(&self.text)?;
        self.replace(dom, start, length, &net::from_units(&[placeholder]))?;
        let rebuilt = ParagraphText::build(dom, self.paragraph);
        let at = rebuilt.text.iter().position(|&c| c == placeholder);
        let seg = at
            .and_then(|at| rebuilt.segments.iter().find(|s| at >= s.start && at < s.start + s.len).map(|s| (at, *s)))
            .ok_or_else(|| ApiError::invalid_operation("Sequence contains no matching element"))?;
        let (at, seg) = seg;
        let run = dom.parent(seg.t).ok_or_else(|| ApiError::invalid_operation("the text has no run"))?;
        let local = at - seg.start;
        let after = split_run(dom, run, seg.t, local + 1)?;
        let middle = split_run(dom, run, seg.t, local)?;
        remove_if_empty(dom, after);
        let rpr = dom.element(middle, n::rPr).map(|r| dom.deep_clone(r));
        let element = create(dom, rpr);
        dom.replace_with(middle, &[element])?;
        remove_if_empty(dom, run);
        Ok(element)
    }
}

fn i64_of(v: usize) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// `Math.Clamp(v, lo, hi)` to an index.
fn clamp_i(v: i64, lo: usize, hi: usize) -> usize {
    let v = v.max(i64_of(lo)).min(i64_of(hi));
    usize::try_from(v).unwrap_or(lo)
}

fn pick_placeholder(text: &[u16]) -> Result<u16> {
    (0xE000..=0xF8FFu16).rev().find(|c| !text.contains(c)).ok_or_else(|| ApiError::invalid_operation("no free character to mark the span"))
}

/// "Whole word": a search that starts (ends) with a letter or digit may not touch one before
/// (after) it; a search starting or ending with a symbol has no boundary on that side.
pub fn is_whole_word(text: &[u16], start: usize, length: usize) -> bool {
    let wc = |i: usize| text.get(i).is_some_and(|&c| net::is_letter_or_digit(c) || c == u16::from(b'_'));
    let starts_word = wc(start);
    let ends_word = wc(start + length - 1);
    if starts_word && start > 0 && wc(start - 1) {
        return false;
    }
    if ends_word && start + length < text.len() && wc(start + length) {
        return false;
    }
    true
}

/// Splits the run at `offset` inside its `w:t`: the run keeps what comes before; a new run (same
/// properties) with the rest goes right after it. Returns the new run.
pub(crate) fn split_run(dom: &mut Dom, run: NodeId, t: NodeId, offset: usize) -> Result<NodeId> {
    let name = dom.name_of(run).unwrap_or(n::r);
    let tail = dom.new_element(name);
    for a in dom.attrs(run).to_vec() {
        dom.set_attr(tail, a.name, Some(&a.value));
    }
    if let Some(rpr) = dom.element(run, n::rPr) {
        let c = dom.deep_clone(rpr);
        dom.append(tail, c);
    }
    let text = net::units(&dom.value(t));
    if offset < text.len() {
        let rest = net::from_units(text.get(offset..).unwrap_or(&[]));
        let te = ooxml::t(dom, &rest);
        dom.append(tail, te);
    }
    for f in dom.elements_after(t) {
        dom.remove(f)?;
        dom.append(tail, f);
    }
    if offset == 0 {
        dom.remove(t)?;
    } else {
        set_text(dom, t, &net::from_units(text.get(..offset).unwrap_or(&[])))?;
    }
    dom.add_after(run, &[tail])?;
    Ok(tail)
}

fn replace_text_element(dom: &mut Dom, t: NodeId, prefix: &str, replacement: &str, suffix: &str) -> Result<()> {
    let full = format!("{prefix}{replacement}{suffix}");
    if !full.contains(['\t', '\n', '\r', '\u{B}']) {
        return set_text(dom, t, &ooxml::remove_invalid_chars(&full));
    }
    let parts = ooxml::run_content(dom, &full);
    dom.replace_with(t, &parts)?;
    Ok(())
}

/// C# `SetText`: empty removes the `w:t` (and a run left empty); otherwise sets the text and
/// `xml:space="preserve"` when it has edge or double spaces.
pub(crate) fn set_text(dom: &mut Dom, t: NodeId, value: &str) -> Result<()> {
    if value.is_empty() {
        let run = dom.parent(t);
        dom.remove(t)?;
        if let Some(r) = run {
            remove_if_empty(dom, r);
        }
        return Ok(());
    }
    dom.set_value(t, value);
    if net::trim(value).len() != value.len() || value.contains("  ") {
        dom.set_attr(t, n::xml_space, Some("preserve"));
    }
    Ok(())
}

/// Removes the run when only its `w:rPr` is left.
pub(crate) fn remove_if_empty(dom: &mut Dom, run: NodeId) {
    if !dom.is(run, n::r) || dom.parent(run).is_none() {
        return;
    }
    if dom.elements(run).all(|e| dom.is(e, n::rPr)) {
        dom.remove_if_attached(run);
    }
}

/// Every paragraph (tables and text boxes included), in document order.
pub fn paragraphs(dom: &Dom, root: NodeId) -> Vec<NodeId> {
    dom.descendants_named(root, n::p)
}

/// The "visible" text of a paragraph (tab `\t`, break `\n`, non-breaking hyphen `-`).
pub fn plain_text(dom: &Dom, paragraph: NodeId) -> String {
    let mut out = String::new();
    let mut stack: Vec<(NodeId, usize)> = vec![(paragraph, 0)];
    while let Some((c, k)) = stack.pop() {
        let Some(&child) = dom.children(c).get(k) else { continue };
        stack.push((c, k + 1));
        if !dom.is_element(child) {
            continue;
        }
        if dom.is(child, n::r) {
            for rc in dom.elements(child) {
                if dom.is(rc, n::t) {
                    out.push_str(&dom.value(rc));
                } else if dom.is_any(rc, &[n::tab, n::ptab]) {
                    out.push('\t');
                } else if dom.is_any(rc, &[n::br, n::cr]) {
                    out.push('\n');
                } else if dom.is(rc, n::noBreakHyphen) {
                    out.push('-');
                }
            }
        } else if dom.is_any(child, WALK_INTO) {
            stack.push((child, 0));
        } else if dom.is(child, n::sdt)
            && let Some(content) = dom.element(child, n::sdtContent)
        {
            stack.push((content, 0));
        }
    }
    out
}
