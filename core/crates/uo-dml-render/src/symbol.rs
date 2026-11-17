//! Symbol fonts (`symbol-fonts.ts`): Symbol, Wingdings 1/2/3 and Webdings text → Unicode, codes
//! 0x20–0xFF (the same in U+F020–F0FF, the "symbol area" of the cmap 3,0). List bullets and text in
//! Wingdings/Symbol are the Office default; those fonts only exist on Windows, elsewhere the glyph
//! would be the raw letter, so the equivalent Unicode symbol is drawn instead.

use uo_jsnum::text::{is_js_space, to_lowercase};

use crate::symbol_data::{SYMBOL, WEBDINGS, WINGDINGS, WINGDINGS2, WINGDINGS3};

/// `norm` (`symbol-fonts.ts:417`): lower case without white space, `_` and `-`.
fn norm(face: &str) -> String {
    to_lowercase(face).chars().filter(|c| !is_js_space(*c) && *c != '_' && *c != '-').collect()
}

/// Symbol fonts that do NOT ship with Windows/Office (Monotype Sorts, Zapf Dingbats): the
/// application has no glyph and draws the "missing glyph" box (measured in gd-676970).
fn is_missing(n: &str) -> bool {
    matches!(n, "monotypesorts" | "monotypesorts2" | "zapfdingbats" | "itczapfdingbats")
}

fn table(n: &str) -> Option<&'static [u32; 224]> {
    Some(match n {
        "symbol" => &SYMBOL,
        "webdings" => &WEBDINGS,
        "wingdings" => &WINGDINGS,
        "wingdings2" => &WINGDINGS2,
        "wingdings3" => &WINGDINGS3,
        _ => return None,
    })
}

/// `isSymbolFont`: a symbol font with a known table (or one missing in the application)?
pub fn is_symbol_font(face: Option<&str>) -> bool {
    face.is_some_and(|f| {
        let n = norm(f);
        // `norm(face) in T` also finds the prototype of the table object: `constructor` (the only Object.prototype
        // key that survives `norm`) is a "symbol font" whose text `symbolText` leaves unchanged (`t[i]` undefined)
        !f.is_empty() && (table(&n).is_some() || is_missing(&n) || n == "constructor")
    })
}

/// `symbolText`: the text of a symbol font as Unicode (codes 0x20–0xFF or U+F020–F0FF); no match
/// keeps the original code; a font that is not a symbol font leaves the text alone.
pub fn symbol_text(face: Option<&str>, text: &str) -> String {
    let n = face.filter(|f| !f.is_empty()).map(norm);
    if let Some(n) = &n
        && is_missing(n)
    {
        return text.chars().map(|c| if is_js_space(c) { c } else { '\u{25af}' }).collect();
    }
    let Some(t) = n.as_deref().and_then(table) else { return text.to_string() };
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let mut c = u32::from(ch);
        if (0xf020..=0xf0ff).contains(&c) {
            c -= 0xf000;
        }
        let u = if (0x20..=0xff).contains(&c) { t.get((c - 0x20) as usize).copied().unwrap_or(0) } else { 0 };
        let code = if u != 0 { u } else { u32::from(ch) };
        out.push(char::from_u32(code).unwrap_or(ch));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wingdings_check_mark_and_missing_fonts() {
        assert_eq!(symbol_text(Some("Wingdings"), "\u{f0fc}"), "\u{2713}");
        assert_eq!(symbol_text(Some("Wing dings-3"), "\u{f021}"), "\u{2b60}");
        assert_eq!(symbol_text(Some("Symbol"), "a"), "\u{3b1}");
        assert_eq!(symbol_text(Some("Arial"), "abc"), "abc");
        assert_eq!(symbol_text(Some("ZapfDingbats"), "a b"), "\u{25af} \u{25af}");
        assert!(is_symbol_font(Some("Webdings")) && is_symbol_font(Some("Monotype Sorts")) && !is_symbol_font(Some("Arial")));
        assert!(!is_symbol_font(None));
    }
}
