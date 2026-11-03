//! Content nested past `DocxLimits::max_nesting` (tables in cells, textboxes, groups) is read as
//! plain text paragraphs in place, not rejected (G20, U18; accepted by name): every `w:p`
//! below the element, in document order, becomes a paragraph with one run of its text and the
//! default run props (`w:t` text, `w:tab` → TAB, `w:br`/`w:cr` → LF, `w:noBreakHyphen` → U+2011,
//! `w:softHyphen` → U+00AD). Iterative.

use std::sync::Arc;

use uo_opc::xml::Element;

use crate::ids::IdAlloc;
use crate::model::run::RunProps;
use crate::model::{Paragraph, Run};
use crate::text::Utf16;

/// `DEFAULT_RUN_PROPS` of `document.ts`.
#[must_use]
pub fn default_run_props() -> RunProps {
    RunProps {
        bold: Some(false),
        italic: Some(false),
        underline: Some(false),
        strike: Some(false),
        all_caps: Some(false),
        small_caps: Some(false),
        ..RunProps::default()
    }
}

/// The text paragraphs of everything below `el` (see the module comment).
pub fn plain_paragraphs(el: Element<'_>, ids: &mut IdAlloc) -> Vec<Paragraph> {
    let mut out = Vec::new();
    for p in std::iter::once(el).chain(el.descendants()).filter(|e| e.name() == "w:p") {
        // a w:p inside another w:p (a textbox in a run) is its own paragraph: take only the
        // text that is not inside a nested w:p
        let mut text: Vec<u16> = Vec::new();
        let mut stack = vec![p.children()];
        while let Some(top) = stack.last_mut() {
            let Some(n) = top.next() else {
                stack.pop();
                continue;
            };
            let Some(c) = n.as_element() else { continue };
            match c.name() {
                "w:p" => {}
                "w:t" => text.extend(c.text_content().encode_utf16()),
                "w:tab" => text.push(9),
                "w:br" | "w:cr" => text.push(10),
                "w:noBreakHyphen" => text.push(0x2011),
                "w:softHyphen" => text.push(0xad),
                _ => stack.push(c.children()),
            }
        }
        let mut para = Paragraph::empty(ids.para());
        if !text.is_empty() {
            para.runs.push(Run { text: Utf16::from_units(text), props: Arc::new(default_run_props()), ..Run::default() });
        }
        out.push(para);
    }
    out
}
