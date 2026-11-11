//! Hostile arguments of the D2b-F10 commands (no-panic and M1 tests).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::session::ops::references::{
    CaptionFormat, CaptionNumbering, ChapterSeparator, CrossRefKind, CrossRefOptions, CrossRefType, TableOfContentsOptions, TocFormat,
    TocOptions,
};

pub const ITEMS: [&str; 14] = [
    "p:0",
    "p:1",
    "p:99999999999999999999",
    "p:",
    "p:-1",
    "p:1.5",
    "n:NaN",
    "n:0",
    "n:1",
    "n:99999999999",
    "x",
    "",
    "p:4294967296",
    "heading",
];

pub const LABELS: [&str; 9] = ["Figure", "Table", "Equation", "", " ", "a b", "q\"uote", "back\\slash", "\u{fffd}\u{1F600}"];

pub const TYPES: [CrossRefType; 6] = [
    CrossRefType::Numbered,
    CrossRefType::Heading,
    CrossRefType::Bookmark,
    CrossRefType::Footnote,
    CrossRefType::Endnote,
    CrossRefType::Caption,
];

pub fn kinds() -> Vec<CrossRefKind> {
    use CrossRefKind::*;
    vec![
        Text,
        Page,
        Number,
        NumberNoContext,
        NumberFullContext,
        Position,
        NoteNumber,
        NoteNumberFormatted,
        CaptionEntire,
        CaptionLabel,
        CaptionText,
    ]
}

pub fn cross_ref(i: usize) -> CrossRefOptions {
    CrossRefOptions {
        ty: TYPES[i % TYPES.len()],
        label: Some(LABELS[i % LABELS.len()].to_owned()),
        item: ITEMS[i % ITEMS.len()].to_owned(),
        kind: kinds()[i % kinds().len()],
        hyperlink: [None, Some(true), Some(false)][i % 3],
        position: [None, Some(true), Some(false)][(i / 3) % 3],
        separator: [None, Some(String::new()), Some("-".to_owned()), Some("\"".to_owned())][i % 4].clone(),
    }
}

pub fn numbering(i: usize) -> CaptionNumbering {
    use CaptionFormat::*;
    let f = [Arabic, AlphabeticUpper, AlphabeticLower, RomanUpper, RomanLower][i % 5];
    let chapter = (!i.is_multiple_of(3)).then(|| uo_docx::session::ops::references::pure::CaptionChapter {
        level: [1.0, 2.0, 9.0, 0.0, f64::NAN, 1e300, -1.0, f64::INFINITY][i % 8],
        separator: [
            ChapterSeparator::Hyphen,
            ChapterSeparator::Period,
            ChapterSeparator::Colon,
            ChapterSeparator::EmDash,
            ChapterSeparator::EnDash,
        ][i % 5],
    });
    CaptionNumbering { format: f, chapter }
}

pub fn toc_dialog(i: usize) -> TableOfContentsOptions {
    TableOfContentsOptions {
        page_numbers: [None, Some(true), Some(false)][i % 3],
        right_align: [None, Some(true), Some(false)][(i / 3) % 3],
        leader: [None, Some("dot".to_owned()), Some("none".to_owned()), Some("hyphen".to_owned()), Some("x".to_owned())][i % 5].clone(),
        hyperlinks: [None, Some(true), Some(false)][(i / 2) % 3],
        format: [
            None,
            Some(TocFormat::Template),
            Some(TocFormat::Classic),
            Some(TocFormat::Distinctive),
            Some(TocFormat::Fancy),
            Some(TocFormat::Modern),
            Some(TocFormat::Formal),
            Some(TocFormat::Simple),
        ][i % 8],
        levels: [None, Some(1.0), Some(3.0), Some(9.0), Some(f64::NAN), Some(f64::INFINITY), Some(-5.0), Some(1e300), Some(2.5)][i % 9],
        use_styles: [None, Some(true), Some(false)][(i / 5) % 3],
        styles: [None, Some(vec![]), Some(vec![uo_docx::session::ops::references::pure::TocStyle { style: "Quote".into(), level: 2.0 }])]
            [i % 3]
            .clone(),
        outline_levels: [None, Some(true), Some(false)][(i / 7) % 3],
        fields: [None, Some(true), Some(false)][(i / 11) % 3],
        field_id: [None, Some("x".to_owned()), Some("a b".to_owned()), Some(String::new())][i % 4].clone(),
    }
}

/// Instructions for the TOC option parser: the grammar and garbage.
pub const INSTRS: [&str; 16] = [
    " TOC \\o \"1-3\" \\h \\z \\u ",
    " TOC ",
    "TOC \\t \"a;1;b\"",
    "TOC \\c \"Figure\" \\h",
    "TOC \\a \"Table\" \\n \\p \" \"",
    "TOC \\f \\l \"1-2\" \\n \"2-3\"",
    "TOC \\o",
    "TOC \\o \"9-1\"",
    "TOC \\t \";;;\"",
    "TOC \"\\\"\"",
    "TOC \\o \"1-3",
    "\\\\\\\"\"\"",
    "TOC \\f \"",
    "TOC \\f \"a\\\"",
    "TOC \\l 99999999999",
    "",
];

pub fn toc_options(i: usize) -> TocOptions {
    uo_docx::session::ops::references::pure::parse_toc_instr(INSTRS[i % INSTRS.len()])
}
