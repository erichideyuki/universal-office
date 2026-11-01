//! Limits for untrusted packages (gate 1, G4): counted on the bytes actually produced, never
//! trusted from sizes declared in the file. XML parts and binary parts (media) have separate
//! caps: a deck with a 300 MB video opens, as it does in the TS.
//!
//! The defaults depend on the target (gate 2, D7): on wasm32 a `Vec` cannot exceed 2 GiB − 1
//! and the whole memory (the retained input included) is 4 GiB, so the native 2 GiB part and
//! 4 GiB total could never be reached there; [`Limits::DEFAULT`] is [`Limits::WASM32`] on that
//! target and [`Limits::NATIVE`] elsewhere, so the limit error always comes before an
//! allocation failure.

use crate::error::{Error, LimitKind, Result};
use crate::xml::{self, Document, ParseOptions};

/// Limits applied while reading a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// one XML-like part (inflated and parsed): `.xml`, `.rels`, `.vml` and the like
    pub max_xml_part_bytes: u64,
    /// one binary part (images, media, embeddings), inflated
    pub max_binary_part_bytes: u64,
    /// all parts read from one package, inflated
    pub max_total_bytes: u64,
    /// entries in the archive
    pub max_entries: u32,
    /// XML nesting of a part parsed through [`crate::Package::xml_part`] (lenient presets count
    /// open elements; the strict mode keeps the lower of this and its own depth)
    pub max_xml_depth: u32,
    /// elements of one part parsed through [`crate::Package::xml_part`] (gate 2, D2): bounds the
    /// tree, which is up to ~17 × its source for hostile shapes (module record, M5)
    pub max_xml_elements: u32,
}

/// The XML part cap of the defaults: 256 MiB (the C# `OpcPackageLimits`), never above the
/// binary part cap.
const fn xml_cap(binary: u64) -> u64 {
    if binary < 256 << 20 { binary } else { 256 << 20 }
}

impl Limits {
    /// The native defaults: 256 MiB per XML part (the C# `OpcPackageLimits`), 2 GiB per binary
    /// part (where the TS stops), 4 GiB in total, 65,535 entries, XML depth 1,000, and
    /// 20,000,000 elements per XML part (about 10 × the largest real part of the repository:
    /// `sheet/samples/giant-1m.xlsx` `xl/worksheets/sheet1.xml`, 2,010,005 elements).
    pub const NATIVE: Limits = Limits {
        max_xml_part_bytes: xml_cap(2 << 30),
        max_binary_part_bytes: 2 << 30,
        max_total_bytes: 4 << 30,
        max_entries: 65_535,
        max_xml_depth: 1_000,
        max_xml_elements: 20_000_000,
    };

    /// The wasm32 defaults (gate 2, D7): 512 MiB per binary part and 1 GiB in total (a phone
    /// kills the tab long before 4 GiB); the XML part cap is min(256 MiB, the binary cap); the
    /// rest as [`Limits::NATIVE`].
    pub const WASM32: Limits =
        Limits { max_xml_part_bytes: xml_cap(512 << 20), max_binary_part_bytes: 512 << 20, max_total_bytes: 1 << 30, ..Limits::NATIVE };

    /// The defaults of the target: [`Limits::WASM32`] on wasm32, [`Limits::NATIVE`] elsewhere.
    #[cfg(target_arch = "wasm32")]
    pub const DEFAULT: Limits = Limits::WASM32;

    /// The defaults of the target: [`Limits::WASM32`] on wasm32, [`Limits::NATIVE`] elsewhere.
    #[cfg(not(target_arch = "wasm32"))]
    pub const DEFAULT: Limits = Limits::NATIVE;

    /// The cap of a raw read of a part, by its name: XML-like extensions get the XML cap. A part
    /// parsed as XML gets the XML cap whatever its name ([`crate::Package::xml_part`], D3).
    pub fn part_cap(&self, name: &str) -> u64 {
        if is_xml_like(name) { self.max_xml_part_bytes } else { self.max_binary_part_bytes }
    }

    /// `options` with their depth and element limits lowered to [`Limits::max_xml_depth`] and
    /// [`Limits::max_xml_elements`] (gate 2, D2/D3): what [`crate::Package::xml_part`] parses
    /// with, and what a format crate passes to [`xml::parse`] for XML that does not come through
    /// it. The rest of the options is kept, so a part within the limits gives the same tree.
    pub fn bounded<'a>(&self, options: &ParseOptions<'a>) -> ParseOptions<'a> {
        let lower = |own: Option<u32>, limit: u32| Some(own.map_or(limit, |o| o.min(limit)));
        ParseOptions {
            max_depth: lower(options.max_depth, self.max_xml_depth),
            max_elements: lower(options.max_elements, self.max_xml_elements),
            ..*options
        }
    }

    /// Parses XML text given by the caller (the edit helpers of `rels`, `content_types` and
    /// `xml::raw`) as [`crate::Package::xml_part`] parses a part: text longer than
    /// [`Limits::max_xml_part_bytes`] is `Limit { PartBytes }`, and the options are
    /// [`Limits::bounded`] (stage-6 finding S1: unbounded, a text up to the XML cap could build a
    /// tree of up to ~17 times its size, where a failed growth aborts the process).
    pub(crate) fn parse(&self, text: &str, options: &ParseOptions<'_>) -> Result<Document> {
        if text.len() as u64 > self.max_xml_part_bytes {
            return Err(Error::Limit { kind: LimitKind::PartBytes, max: self.max_xml_part_bytes, part: None });
        }
        xml::parse(text, &self.bounded(options))
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}

/// Parts whose content is parsed as XML (case-insensitive extension check).
pub fn is_xml_like(name: &str) -> bool {
    // compared without lowercasing a copy: every read checks its cap through here
    let base = name.rsplit('/').next().unwrap_or(name).as_bytes();
    let ends = |e: &str| base.len() >= e.len() && base.get(base.len() - e.len()..).is_some_and(|t| t.eq_ignore_ascii_case(e.as_bytes()));
    base.eq_ignore_ascii_case(b"[content_types].xml")
        || [".xml", ".rels", ".vml", ".xaml", ".fdseq", ".fdoc", ".fpage"].iter().any(|e| ends(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_wasm32_defaults() {
        assert_eq!(Limits::NATIVE.max_binary_part_bytes, 2 << 30);
        assert_eq!(Limits::NATIVE.max_total_bytes, 4 << 30);
        assert_eq!(Limits::NATIVE.max_xml_part_bytes, 256 << 20);
        assert_eq!(Limits::NATIVE.max_xml_elements, 20_000_000);
        assert_eq!(Limits::WASM32.max_binary_part_bytes, 512 << 20);
        assert_eq!(Limits::WASM32.max_total_bytes, 1 << 30);
        assert_eq!(Limits::WASM32.max_xml_part_bytes, 256 << 20);
        assert_eq!(
            Limits { max_xml_part_bytes: 0, max_binary_part_bytes: 0, max_total_bytes: 0, ..Limits::WASM32 },
            Limits { max_xml_part_bytes: 0, max_binary_part_bytes: 0, max_total_bytes: 0, ..Limits::NATIVE }
        );
        assert_eq!(xml_cap(100 << 20), 100 << 20);
        // the native test build is not wasm32 (the wasm value is checked in the uo-wasm build,
        // `crates/uo-wasm/tests/opc_memory.test.mjs`)
        assert_eq!(Limits::DEFAULT, if cfg!(target_arch = "wasm32") { Limits::WASM32 } else { Limits::NATIVE });
        assert_eq!(Limits::default(), Limits::DEFAULT);
    }
}
