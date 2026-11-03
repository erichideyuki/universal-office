//! Limits of the docx reader for untrusted input (plan "Security"): the package limits of
//! uo-opc plus the Word-level ones. A limit hit is a `DocxError::Limit` value, never a panic, a
//! stack overflow or an allocation from a size the file declares; content nested past
//! [`DocxLimits::max_nesting`] is read as plain text in place (G20), not rejected.

/// The limits of one read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocxLimits {
    /// zip, inflate and XML limits of the package (`docx-pos` XML depth 1,000)
    pub package: uo_opc::Limits,
    /// nested stories (table in cell, textbox, group…) below which content is read as text
    /// paragraphs in place (G20, U18)
    pub max_nesting: u32,
    /// complex fields open at once in a story (C# `MaxFieldNesting`)
    pub max_field_depth: u32,
    /// complex fields per story (C# `MaxFieldsPerStory`)
    pub max_fields_per_story: u32,
    /// paragraphs of one part (body, header, notes part…)
    pub max_paragraphs_per_part: u32,
    /// runs of one part
    pub max_runs_per_part: u32,
    /// numbering levels read per abstract numbering (`w:lvl`, Word has 9)
    pub max_numbering_levels: u32,
    /// `basedOn` links followed when resolving a style (iterative, with a seen set)
    pub max_based_on_chain: u32,
}

impl DocxLimits {
    /// The defaults of the target (package limits per target, uo-opc D7).
    pub const DEFAULT: DocxLimits = DocxLimits {
        package: uo_opc::Limits::DEFAULT,
        max_nesting: 64,
        max_field_depth: 64,
        max_fields_per_story: 100_000,
        max_paragraphs_per_part: 4_000_000,
        max_runs_per_part: 16_000_000,
        max_numbering_levels: 9,
        max_based_on_chain: 10_000,
    };
}

impl Default for DocxLimits {
    fn default() -> Self {
        DocxLimits::DEFAULT
    }
}
