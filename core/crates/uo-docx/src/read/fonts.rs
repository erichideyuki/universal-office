//! Frontier R5 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core `fonts.ts`: the fonts EMBEDDED in
//! the .docx (`word/fontTable.xml` + `word/fonts/*.odttf`). Word obfuscates each TTF/OTF file
//! (ECMA-376 §17.8.1): the first 32 bytes are XORed with a 16-byte key derived from the `w:fontKey`
//! (a GUID), its bytes in reverse order. Here they are read, de-obfuscated (`uo_fonts::odttf`) and
//! returned ready for use (face registration, metrics).

use std::collections::HashMap;
use std::sync::Arc;

use uo_opc::Package;

use crate::error::DocxError;

use super::{DOCX, EmbeddedFont};

/// `EMBED_TAGS`, in the order `Object.entries` gives them.
const EMBED_TAGS: [(&str, &str); 4] =
    [("w:embedRegular", "regular"), ("w:embedBold", "bold"), ("w:embedItalic", "italic"), ("w:embedBoldItalic", "boldItalic")];

/// `readEmbeddedFonts(pkg)`: every embedded font of the package, de-obfuscated; empty without any.
/// The part of a relationship is the one [`Package::resolve_target`] gives (uo-opc D6, the one rule
/// of the library: the TS `word/${target}` rules are replaced, accepted by name).
pub fn read_embedded_fonts(pkg: &Package) -> Result<Vec<EmbeddedFont>, DocxError> {
    if pkg.part_text("word/fontTable.xml")?.is_none_or(|t| t.is_empty()) {
        return Ok(Vec::new());
    }
    let mut rels: HashMap<String, String> = HashMap::new();
    if pkg.part_text("word/_rels/fontTable.xml.rels")?.is_some_and(|t| !t.is_empty()) {
        let doc = pkg.xml_part("word/_rels/fontTable.xml.rels", &DOCX)?;
        for rel in doc.root().children_named("Relationship") {
            if let (Some(id), Some(target)) = (rel.attr("Id").filter(|s| !s.is_empty()), rel.attr("Target").filter(|s| !s.is_empty())) {
                rels.insert(id.to_owned(), target.to_owned());
            }
        }
    }
    let mut out = Vec::new();
    let table = pkg.xml_part("word/fontTable.xml", &DOCX)?;
    for font in table.root().children_named("w:font") {
        let Some(family) = font.attr("w:name").filter(|s| !s.is_empty()) else { continue };
        for (tag, variant) in EMBED_TAGS {
            let Some(embed) = font.first_child(tag) else { continue };
            let (Some(rid), Some(font_key)) =
                (embed.attr("r:id").filter(|s| !s.is_empty()), embed.attr("w:fontKey").filter(|s| !s.is_empty()))
            else {
                continue;
            };
            let Some(target) = rels.get(rid) else { continue };
            let Some(path) = pkg.resolve_target("word/fontTable.xml", target) else { continue };
            let Some(raw) = pkg.part(path)? else { continue };
            out.push(EmbeddedFont {
                family: family.into(),
                variant: variant.into(),
                data: Arc::from(uo_fonts::odttf::deobfuscate(raw, font_key).into_owned()),
            });
        }
    }
    Ok(out)
}
