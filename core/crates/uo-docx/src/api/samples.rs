//! C# `DocxParagraphSamples`: sample paragraphs of a template (a `#{Name}` marker whose
//! paragraph's formatting is the mold for content written with the body writer).

use super::body_writer::ParagraphProperties;
use super::document::DocxDocument;
use super::dom::{NodeId, n};
use super::error::Result;
use super::text::{self, TextScope};

/// The document has the marker `#{name}` (whole word, case-sensitive)?
pub fn contains(doc: &mut DocxDocument, name: &str) -> Result<bool> {
    text::contains(doc, &text::parameter(name), true, true, TextScope::Document)
}

/// Reads and REMOVES the samples: for each name, the properties of the FIRST paragraph with
/// `#{name}`, and every paragraph with it is removed. Names without a sample are left out; the
/// result is in the order of the names.
pub fn extract(doc: &mut DocxDocument, names: &[&str]) -> Result<Vec<(String, ParagraphProperties)>> {
    let mut samples: Vec<(String, ParagraphProperties)> = Vec::new();
    let mut removed = false;
    for &name in names {
        if samples.iter().any(|(k, _)| k == name) {
            continue;
        }
        let paragraphs = text::find_paragraphs(doc, &text::parameter(name), true, true, TextScope::Document)?;
        let Some(&first) = paragraphs.first() else { continue };
        let dom = doc.dom_mut();
        let ppr = match dom.element(first, n::pPr) {
            Some(found) => dom.deep_clone(found),
            None => dom.new_element(n::pPr),
        };
        let doomed: Vec<NodeId> = dom.elements(ppr).filter(|&e| dom.is_any(e, &[n::sectPr, n::pPrChange])).collect();
        for e in doomed {
            dom.remove_if_attached(e);
        }
        if let Some(props) = ParagraphProperties::from_copy(dom, ppr) {
            samples.push((name.to_owned(), props));
        }
        for p in paragraphs {
            text::remove_paragraph(doc.dom_mut(), p)?;
        }
        removed = true;
    }
    if removed {
        doc.mark_changed();
    }
    Ok(samples)
}
