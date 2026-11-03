//! The four RIBBI variants of a font family.

use uo_jsnum::text::{split_js_space, to_lowercase};

/// Regular, Bold, Italic or Bold Italic: the variants a family can have in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Variant {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// The variants in the order of the TS `VARIANTS` (regular, bold, italic, boldItalic).
pub const VARIANTS: [Variant; 4] = [Variant::Regular, Variant::Bold, Variant::Italic, Variant::BoldItalic];

/// The variant of a bold and an italic flag.
pub fn variant_of(bold: bool, italic: bool) -> Variant {
    match (bold, italic) {
        (true, true) => Variant::BoldItalic,
        (true, false) => Variant::Bold,
        (false, true) => Variant::Italic,
        (false, false) => Variant::Regular,
    }
}

pub fn is_bold_variant(v: Variant) -> bool {
    matches!(v, Variant::Bold | Variant::BoldItalic)
}

pub fn is_italic_variant(v: Variant) -> bool {
    matches!(v, Variant::Italic | Variant::BoldItalic)
}

/// The RIBBI variant of a face's style (subfamily) name: only "Regular", "Bold", "Italic",
/// "Bold Italic" and their synonyms; Light, Semibold, Black and the like give `None`; no style
/// is regular. The name is lower-cased (Unicode), every run of JS whitespace becomes one space
/// and the ends are trimmed, as the TS does.
pub fn variant_of_style(style: Option<&str>) -> Option<Variant> {
    let lowered = to_lowercase(style.unwrap_or("Regular"));
    let mut name = String::with_capacity(lowered.len());
    for (i, token) in split_js_space(&lowered).filter(|t| !t.is_empty()).enumerate() {
        if i > 0 {
            name.push(' ');
        }
        name.push_str(token);
    }
    match name.as_str() {
        "regular" | "normal" | "roman" | "" => Some(Variant::Regular),
        "bold" => Some(Variant::Bold),
        "italic" | "oblique" => Some(Variant::Italic),
        "bold italic" | "bold oblique" => Some(Variant::BoldItalic),
        _ => None,
    }
}

impl Variant {
    /// The TS name of the variant (`regular`, `bold`, `italic`, `boldItalic`).
    pub const fn name(self) -> &'static str {
        match self {
            Variant::Regular => "regular",
            Variant::Bold => "bold",
            Variant::Italic => "italic",
            Variant::BoldItalic => "boldItalic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_alloc::assert_no_leak;

    #[test]
    fn variants_in_the_ts_order() {
        assert_eq!(VARIANTS.map(Variant::name), ["regular", "bold", "italic", "boldItalic"]);
        assert_eq!(variant_of(true, true), Variant::BoldItalic);
        assert_eq!(variant_of(true, false), Variant::Bold);
        assert_eq!(variant_of(false, true), Variant::Italic);
        assert_eq!(variant_of(false, false), Variant::Regular);
        assert!(is_bold_variant(Variant::Bold) && is_bold_variant(Variant::BoldItalic));
        assert!(!is_bold_variant(Variant::Italic) && !is_bold_variant(Variant::Regular));
        assert!(is_italic_variant(Variant::Italic) && is_italic_variant(Variant::BoldItalic));
        assert!(!is_italic_variant(Variant::Bold) && !is_italic_variant(Variant::Regular));
    }

    #[test]
    fn styles_map_to_the_four_ribbi_variants() {
        for s in ["Regular", "regular", "NORMAL", "Roman", "", "   ", "\u{a0}\u{feff}"] {
            assert_eq!(variant_of_style(Some(s)), Some(Variant::Regular), "{s:?}");
        }
        assert_eq!(variant_of_style(None), Some(Variant::Regular));
        for s in ["Bold", "  BOLD  ", "bold\t"] {
            assert_eq!(variant_of_style(Some(s)), Some(Variant::Bold), "{s:?}");
        }
        for s in ["Italic", "Oblique", "OBLIQUE"] {
            assert_eq!(variant_of_style(Some(s)), Some(Variant::Italic), "{s:?}");
        }
        for s in ["Bold Italic", "bold  italic", "Bold\u{a0}\u{2003}Oblique", "\nbold \r\n oblique "] {
            assert_eq!(variant_of_style(Some(s)), Some(Variant::BoldItalic), "{s:?}");
        }
        for s in ["Light", "Semibold", "Black", "Bold Condensed", "Italic Bold", "bolditalic", "Bold-Italic", "\u{85}bold", "\u{200b}bold"]
        {
            assert_eq!(variant_of_style(Some(s)), None, "{s:?}");
        }
    }

    #[test]
    fn m1_no_leak() {
        assert_no_leak(|| variant_of_style(Some("Bold   Italic")));
        assert_no_leak(|| variant_of_style(None));
        assert_no_leak(|| variant_of_style(Some("Semibold")));
        assert_no_leak(|| (variant_of(true, false), VARIANTS.map(Variant::name)));
    }
}
