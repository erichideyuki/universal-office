//! Fonts EMBEDDED in the `.pptx` (`fonts.ts`) — the same logic as Universal Write's (`docx-core/fonts.ts`)
//! adapted to PresentationML: `ppt/presentation.xml <p:embeddedFontLst>` lists
//! `<p:embeddedFont><p:font typeface/><p:regular|bold|italic|boldItalic r:id/>` and the bytes live in
//! `ppt/fonts/fontN.fntdata`.
//!
//! PowerPoint writes the fntdata as EOT v2.2 with a subset + MicroType Express (flags 0x5); Google
//! Slides as EOT with MTX (0x4); we (shared/fonts) write EOT without compression; some producers
//! write a raw sfnt or the odttf obfuscation of ECMA-376 §17.8.1 (XOR of the first 32 bytes with the
//! reversed GUID). Detected by the sfnt magic and, without a valid magic, the fontKey and then the EOT
//! are tried. **The EOT reader belongs to frontier B5b** (`eot.ts` → `uo-fonts`): an EOT part is
//! skipped here (what the TS does for an unknown format).

use std::sync::Arc;

use uo_dml_render::source::{PartSource, R, read_rels};

use crate::package::PartStore;

/// `FontVariant`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontVariant {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// `EmbeddedFont`.
#[derive(Clone, PartialEq, Debug)]
pub struct EmbeddedFont {
    /// Family (`p:font/@typeface`), e.g. "Creepster".
    pub family: Box<str>,
    pub variant: FontVariant,
    /// sfnt bytes ready for a `FontFace`.
    pub data: Arc<[u8]>,
    /// The REAL weight and italic of the font (OS/2 usWeightClass / fsSelection), which may differ from
    /// the slot: Google Slides repeats the regular in the "bold" slot.
    pub weight: Option<u32>,
    pub italic: Option<bool>,
}

/// usWeightClass + italic bit of the OS/2 table (`None` without the table).
fn sfnt_style(d: &[u8]) -> Option<(u32, bool)> {
    if d.len() < 12 {
        return None;
    }
    let n = (usize::from(*d.get(4)?) << 8) | usize::from(*d.get(5)?);
    for i in 0..n {
        let r = 12 + 16 * i;
        if r + 16 > d.len() {
            return None;
        }
        if d.get(r..r + 4)? != b"OS/2" {
            continue;
        }
        let o = u32::from_be_bytes([*d.get(r + 8)?, *d.get(r + 9)?, *d.get(r + 10)?, *d.get(r + 11)?]) as usize;
        if o + 64 > d.len() {
            return None;
        }
        let weight = (u32::from(*d.get(o + 4)?) << 8) | u32::from(*d.get(o + 5)?);
        let fs_sel = (u32::from(*d.get(o + 62)?) << 8) | u32::from(*d.get(o + 63)?);
        return Some((if (1..=1000).contains(&weight) { weight } else { 400 }, fs_sel & 1 != 0));
    }
    None
}

/// A valid sfnt magic? (TrueType 00 01 00 00, OTTO, true, ttcf).
fn looks_like_sfnt(d: &[u8]) -> bool {
    matches!(d.get(..4), Some([0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf"))
}

/// Un-obfuscates odttf: XOR of the first 32 bytes with the key of the GUID (reversed).
pub fn deobfuscate_font(data: &[u8], font_key: &str) -> Vec<u8> {
    let hex: String = font_key.chars().filter(|c| !matches!(c, '{' | '}' | '-')).collect();
    let units: Vec<u16> = hex.encode_utf16().collect();
    if units.len() < 32 {
        return data.to_vec();
    }
    let mut key = [0u8; 16];
    for (i, k) in key.iter_mut().enumerate() {
        let piece = String::from_utf16_lossy(units.get(i * 2..i * 2 + 2).unwrap_or(&[]));
        let v = uo_jsnum::parse::parse_int(&piece, 16);
        *k = uo_jsnum::conv::to_uint8(v);
    }
    key.reverse();
    let mut out = data.to_vec();
    for (i, b) in out.iter_mut().take(32).enumerate() {
        *b ^= key.get(i % 16).copied().unwrap_or(0);
    }
    out
}

/// `readEmbeddedFonts` (`fonts.ts:94`): every embedded font of the package; empty if the deck embeds none.
pub fn read_embedded_fonts(src: &dyn PartSource) -> R<Vec<EmbeddedFont>> {
    let Some(doc) = src.xml("ppt/presentation.xml")? else { return Ok(Vec::new()) };
    let Some(lst) = doc.root().first_child("p:embeddedFontLst") else { return Ok(Vec::new()) };
    let rels = read_rels(src, "ppt/_rels/presentation.xml.rels", "ppt/presentation.xml")?;
    let mut out = Vec::new();
    for ef in lst.children_named("p:embeddedFont") {
        let Some(family) = ef.first_child("p:font").and_then(|f| f.attr("typeface")).filter(|f| !f.is_empty()) else { continue };
        for (tag, variant) in [
            ("p:regular", FontVariant::Regular),
            ("p:bold", FontVariant::Bold),
            ("p:italic", FontVariant::Italic),
            ("p:boldItalic", FontVariant::BoldItalic),
        ] {
            let v = ef.first_child(tag);
            let Some(rid) = v.and_then(|v| v.attr("r:id")).filter(|r| !r.is_empty()) else { continue };
            let Some(target) = rels.get(rid) else { continue };
            let Some(raw) = src.part(target) else { continue };
            let mut data: Vec<u8> = raw.to_vec();
            if !looks_like_sfnt(&data) {
                let key = v.and_then(|v| v.attr("fontKey").or_else(|| v.attr("p:fontKey")));
                if let Some(key) = key.filter(|k| !k.is_empty()) {
                    data = deobfuscate_font(&raw, key);
                }
                // EOT (Google Slides/Slidesgo): MicroType Express and/or XOR → TrueType: frontier B5b
                if !looks_like_sfnt(&data) {
                    continue; // unknown format: skipped
                }
            }
            let st = sfnt_style(&data);
            out.push(EmbeddedFont {
                family: family.into(),
                variant,
                data: Arc::from(data),
                weight: st.map(|s| s.0),
                italic: st.map(|s| s.1),
            });
        }
    }
    Ok(out)
}

/// `collectDeckTypefaces` (`fonts.ts:127`): the typefaces the deck uses (runs + theme): entry of the
/// on-demand loading of OS fonts. Theme aliases (+mj-lt/+mn-lt) must already be resolved in the runs by
/// the cascade; here what stayed literal in the XML is collected, sorted by UTF-16 code unit order.
pub fn collect_deck_typefaces(parts: &PartStore) -> Vec<Box<str>> {
    let mut out: Vec<Box<str>> = Vec::new();
    for name in parts.names() {
        if !is_deck_part(name) {
            continue;
        }
        let Some(text) = parts.text(name).filter(|t| !t.is_empty()) else { continue };
        let mut rest = text.as_str();
        while let Some(i) = rest.find("typeface=\"") {
            let after = rest.get(i + 10..).unwrap_or("");
            // `([^"+][^"]*)"`
            let first = after.chars().next();
            match (first, after.find('"')) {
                (Some(c), Some(end)) if c != '"' && c != '+' => {
                    let face = after.get(..end).unwrap_or("");
                    if !out.iter().any(|o| &**o == face) {
                        out.push(face.into());
                    }
                    rest = after.get(end + 1..).unwrap_or("");
                }
                _ => rest = after,
            }
        }
    }
    out.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    out
}

/// `/^ppt\/(slides|slideLayouts|slideMasters)\/[^/]+\.xml$/`.
fn is_deck_part(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ppt/") else { return false };
    for dir in ["slides/", "slideLayouts/", "slideMasters/"] {
        if let Some(file) = rest.strip_prefix(dir) {
            return file.len() > 4 && file.ends_with(".xml") && !file.contains('/');
        }
    }
    false
}
