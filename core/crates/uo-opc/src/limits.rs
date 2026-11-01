//! Limits for untrusted packages (gate 1, G4): counted on the bytes actually produced, never
//! trusted from sizes declared in the file. XML parts and binary parts (media) have separate
//! caps: a deck with a 300 MB video opens, as it does in the TS.

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
    /// XML nesting (lenient presets); the strict mode has its own depth
    pub max_xml_depth: u32,
}

impl Limits {
    /// The defaults: 256 MiB per XML part (the C# `OpcPackageLimits`), 2 GiB per binary part
    /// (where the TS stops), 4 GiB in total, 65,535 entries, XML depth 1,000.
    pub const DEFAULT: Limits = Limits {
        max_xml_part_bytes: 256 << 20,
        max_binary_part_bytes: 2 << 30,
        max_total_bytes: 4 << 30,
        max_entries: 65_535,
        max_xml_depth: 1_000,
    };

    /// The cap for a part, by its name: XML-like extensions get the XML cap.
    pub fn part_cap(&self, name: &str) -> u64 {
        if is_xml_like(name) { self.max_xml_part_bytes } else { self.max_binary_part_bytes }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}

/// Parts whose content is parsed as XML (case-insensitive extension check).
pub fn is_xml_like(name: &str) -> bool {
    let lower = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    lower == "[content_types].xml" || [".xml", ".rels", ".vml", ".xaml", ".fdseq", ".fdoc", ".fpage"].iter().any(|e| lower.ends_with(e))
}
