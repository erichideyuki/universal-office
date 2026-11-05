//! The standalone path (docx-core `export/from-docx.ts`): the document the reader gives, as the
//! input of the exporters. `docxToMarkdown` and `docxToHtml` of that file delegate to the converter
//! package's own copy of the exporters (class prefix `wn-`, no final view): they belong to the
//! converter port, not here (record, D5a "not ported").

use crate::model::Document;

use super::model::ExportDoc;

/// `exportDocFromFullDocument(full)`: the body's paragraphs, the tables as anchors "before paragraph
/// N" (the block order is kept), the comments and the notes; `title` is not set (Title/Heading
/// paragraphs are rendered as headings already, a title would duplicate the text). The first
/// section's `sectPr` of the TS shape is not carried: no exporter reads it.
#[must_use]
pub fn export_doc_from_full_document(doc: &Document) -> ExportDoc<'_> {
    ExportDoc {
        title: None,
        body: &doc.body,
        comments: &doc.comments,
        footnotes: Some(&doc.notes.footnotes),
        endnotes: Some(&doc.notes.endnotes),
        media: &doc.media,
        image_data_uris: None,
    }
}
