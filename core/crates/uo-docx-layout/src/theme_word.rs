//! Wave 7 (B, gate-O3): the theme a layout resolves `w:themeColor`/`w:themeFill` and DrawingML scheme colours with.
//!
//! A package without a theme part reads as an empty theme (`readTheme(undefined)`), and every theme colour then falls
//! back to the written `w:color` — the frozen TS. Word paints the Office 2023 colour scheme instead (measured
//! 2026-11-12, hidden Word via COM, `core/target/m2-w7b/word/w7out/theme-none.pdf`: accent1..6, text1/2, background2,
//! dark2/light2, hyperlink/followedHyperlink and 13 tint/shade cases identical to the same document WITH the 2023
//! theme part; `w:color C00000` + `themeColor accent2` → E97132). Word is the authority (the maintainer's ruling): the
//! layout follows it. Only the colour scheme — the fonts and the format scheme stay empty (not measured).
//! `LayoutOptions::ts_empty_theme` keeps the frozen TS behaviour for the strict parity replays.

use std::sync::LazyLock;

use uo_docx::model::formatting::Theme;

use crate::model::DocView;

/// The theme of a document without a theme part as Word lays it out: the Office 2023 colour scheme
/// (`uo_docx::write::ctx::DEFAULT_SHAPE_THEME`, the one copy of the palette the writer already uses).
static WORD_NO_THEME_PART: LazyLock<Theme> = LazyLock::new(|| {
    let mut t = Theme::default();
    for (k, v) in uo_docx::write::ctx::DEFAULT_SHAPE_THEME {
        t.colors.insert(k, (*v).into());
    }
    t
});

/// The theme the layout of `view` uses: the document's, or — when the package has none (the model's theme is the empty
/// one) and `ts_empty` is off — Word's Office 2023 colours.
#[must_use]
pub fn theme_of<'a>(view: DocView<'a>, ts_empty: bool) -> &'a Theme {
    let t = &view.formatting().theme;
    if !ts_empty && is_empty_theme(t) { &WORD_NO_THEME_PART } else { t }
}

/// The model's theme is the one `readTheme(undefined)` gives (no theme part).
fn is_empty_theme(t: &Theme) -> bool {
    t.colors.is_empty()
        && t.major.scripts.is_empty()
        && t.major.latin.is_none()
        && t.minor.scripts.is_empty()
        && t.minor.latin.is_none()
        && t.line_widths_pt.is_none()
        && t.fill_styles.is_none()
        && t.bg_fill_styles.is_none()
        && t.effect_styles.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_is_office_2023() {
        let t = &*WORD_NO_THEME_PART;
        assert_eq!(t.colors.get("accent2").map(|s| &**s), Some("E97132"));
        assert_eq!(t.colors.get("accent1").map(|s| &**s), Some("156082"));
        assert_eq!(t.colors.len(), 12);
        assert!(is_empty_theme(&Theme::default()));
        assert!(!is_empty_theme(t));
    }
}
