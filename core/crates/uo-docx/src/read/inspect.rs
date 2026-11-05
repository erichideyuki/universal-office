//! Frontier R5 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `inspect.ts`: queries over the
//! content of the body: is it empty ([`document_is_empty`]), which links ([`document_links`]) and
//! which plugin inlines ([`document_custom_inlines`]) it has, in document order. Used by the editor and
//! the viewer (`isEmpty`, `getLinks`, `getCustomInlines`).
//!
//! The TS generators (`paragraphsOf`, `tableParagraphs`) are one iterator over an explicit stack
//! ([`ParagraphsOf`]): paragraphs in document order, a paragraph's text boxes after it, cells (nested
//! tables included) in order.

use uo_jsnum::text::is_js_space_unit;

use crate::js::Str;
use crate::js_object;
use crate::model::run::{CustomInline, Run, TextboxView};
use crate::model::story::Blocks;
use crate::model::{Block, Paragraph, Story};
use crate::text::Utf16;

js_object! {
    /// A link of the document (`DocumentLink`): `address` (external: http, mailto…) or `anchor` (an
    /// internal bookmark), and the visible text (neighbouring runs with the same target joined).
    pub struct DocumentLink {
        opt address: Str = "address",
        opt anchor: Str = "anchor",
        req text: Utf16 = "text",
    }
}

enum Frame<'a> {
    Blocks(Blocks<'a>),
    Runs(std::slice::Iter<'a, Run>),
}

/// The paragraphs of a story in document order: the body's, those of the cells (nested ones included)
/// and those of the text boxes (`paragraphsOf`).
pub struct ParagraphsOf<'a> {
    stack: Vec<Frame<'a>>,
}

impl<'a> ParagraphsOf<'a> {
    #[must_use]
    pub fn new(story: &'a Story) -> ParagraphsOf<'a> {
        ParagraphsOf { stack: vec![Frame::Blocks(story.blocks())] }
    }
}

impl<'a> Iterator for ParagraphsOf<'a> {
    type Item = &'a Paragraph;

    fn next(&mut self) -> Option<&'a Paragraph> {
        loop {
            match self.stack.last_mut()? {
                Frame::Blocks(it) => match it.next() {
                    None => {
                        self.stack.pop();
                    }
                    Some(Block::Paragraph(p)) => {
                        self.stack.push(Frame::Runs(p.runs.iter()));
                        return Some(p);
                    }
                    Some(Block::Table(t)) => {
                        for row in t.rows.iter().rev() {
                            for cell in row.cells.iter().rev() {
                                self.stack.push(Frame::Blocks(cell.content.blocks()));
                            }
                        }
                    }
                },
                Frame::Runs(it) => match it.next() {
                    None => {
                        self.stack.pop();
                    }
                    Some(r) => {
                        // `r.textbox?.blocks`: a text box read without a block reader has none
                        if let Some(tb) = r.ext().and_then(|e| e.textbox.as_deref())
                            && tb.view != TextboxView::ParasOnly
                        {
                            self.stack.push(Frame::Blocks(tb.content.blocks()));
                        }
                    }
                },
            }
        }
    }
}

/// `runHasContent(r)`: the run shows content (text that is not only white space and object marks, an
/// image, a plugin inline, a note, a shape).
fn run_has_content(r: &Run) -> bool {
    let Some(ext) = r.ext() else { return has_text(r) };
    if ext.textbox.is_some() {
        return false;
    }
    if ext.image.is_some() || ext.custom.is_some() || ext.note_ref.is_some() || ext.shape.is_some() || ext.group.is_some() {
        return true;
    }
    has_text(r)
}

/// `r.text.replace(/￼/g, '').trim().length > 0`: a unit that is neither U+FFFC nor JS white space.
fn has_text(r: &Run) -> bool {
    units_have_text(r.text.units())
}

/// The test of [`has_text`] over code units: what remains of the text after removing every U+FFFC and
/// trimming is not empty (trimming only takes the ends, but whatever is left after it has a unit that is
/// not white space exactly when such a unit exists).
#[must_use]
pub fn units_have_text(units: &[u16]) -> bool {
    units.iter().any(|&u| u != 0xFFFC && !is_js_space_unit(u))
}

/// `documentIsEmpty(blocks)`: no table, and no visible text, image, shape, plugin inline or note in
/// the paragraphs (text boxes by their content).
#[must_use]
pub fn document_is_empty(body: &Story) -> bool {
    if !body.tables.is_empty() {
        return false;
    }
    !ParagraphsOf::new(body).any(|p| p.runs.iter().any(run_has_content))
}

/// `documentLinks(blocks)`: the links of the document in the order they appear.
#[must_use]
pub fn document_links(body: &Story) -> Vec<DocumentLink> {
    let mut out: Vec<(Option<Str>, Option<Str>, Vec<u16>)> = Vec::new();
    for para in ParagraphsOf::new(body) {
        // the target of the link being extended and its index in `out`
        let mut current: Option<(&str, usize)> = None;
        for r in &para.runs {
            let target = if r.ext().is_some_and(|e| e.custom.is_some()) { None } else { r.props.link.as_deref().filter(|s| !s.is_empty()) };
            let Some(target) = target else {
                current = None;
                continue;
            };
            if let Some((t, i)) = current
                && t == target
            {
                if let Some(link) = out.get_mut(i) {
                    link.2.extend_from_slice(r.text.units());
                }
                continue;
            }
            let (address, anchor) = match target.strip_prefix('#') {
                Some(a) => (None, Some(Str::from(a))),
                None => (Some(Str::from(target)), None),
            };
            out.push((address, anchor, r.text.units().to_vec()));
            current = Some((target, out.len() - 1));
        }
    }
    out.into_iter()
        .map(|(address, anchor, text)| DocumentLink { address, anchor, text: Utf16::from_units(text), ..DocumentLink::default() })
        .collect()
}

/// `documentCustomInlines(blocks, type)`: the plugin inlines of the document in the order they appear
/// (all of them, or those of one type).
#[must_use]
pub fn document_custom_inlines(body: &Story, type_: Option<&str>) -> Vec<CustomInline> {
    let mut out = Vec::new();
    for para in ParagraphsOf::new(body) {
        for r in &para.runs {
            if let Some(c) = r.ext().and_then(|e| e.custom.as_deref())
                && type_.is_none_or(|t| &*c.r#type == t)
            {
                out.push(CustomInline {
                    r#type: c.r#type.clone(),
                    data: c.data.clone(),
                    fallback_text: c.fallback_text.clone(),
                    ..CustomInline::default()
                });
            }
        }
    }
    out
}
