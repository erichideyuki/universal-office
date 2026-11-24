//! Grid geometry as Excel computes it (96 dpi, zoom 100%) from the tables MEASURED in Excel 365: column widths by
//! the maximum digit width (MDW) of the Normal font, row heights by the largest font of the row, glyph advances as
//! GDI hints them, descents of the Normal font. 1:1 port of `sheet-core/src/grid-metrics.ts`, `glyph-widths.ts` and
//! the descent half of `sheet/src/text-metrics.ts`; the tables are `&'static` (`data.rs`, generated).
//!
//! Decisions (record "C3a — as built"): family keys that are properties of a JS object (`constructor`,
//! `__proto__`…) are not families here (the TS `ROW_PX[a] !== undefined` takes them for measured ones and then
//! throws or reads garbage); the family name is `normalize('NFKC').trim().toLowerCase()` through `uo-text`.

use super::data::{
    DESC_PPEM_MAX, DESC_PPEM_MIN, GLYPH_FIX, GLYPH_PPEM_MAX, GLYPH_PPEM_MIN, MDW, MDW_PPEM_MAX, MDW_PPEM_MIN, ROW_DESC, ROW_DESC_BOLD,
    ROW_PPEM_MAX, ROW_PPEM_MIN, ROW_PX, ROW_PX_BOLD,
};
use uo_jsnum::conv::to_int32;
use uo_jsnum::math;
use uo_text::utf16::{from_str, to_string_lossy};

/// `B62.indexOf(ch)` (-1 for a character outside the alphabet, as JS).
#[inline]
fn b62(ch: u8) -> i32 {
    match ch {
        b'0'..=b'9' => i32::from(ch - b'0'),
        b'a'..=b'z' => i32::from(ch - b'a') + 10,
        b'A'..=b'Z' => i32::from(ch - b'A') + 36,
        _ => -1,
    }
}

/// `Math.round` to an `i32` (half toward +infinity; values are pixel counts).
#[inline]
pub(crate) fn round_i32(x: f64) -> i32 {
    to_int32(math::round(x))
}

/// Windows `FontSubstitutes` and localized names → measured family (`ALIASES`).
static ALIASES: &[(&str, &str)] = &[
    ("宋体", "simsun"),
    ("新宋体", "simsun"),
    ("ms pゴシック", "ms pgothic"),
    ("ms ゴシック", "ms gothic"),
    ("游ゴシック", "yu gothic"),
    ("新細明體", "pmingliu"),
    ("細明體", "mingliu"),
    ("돋움", "dotum"),
    ("굴림", "gulim"),
    ("바탕", "batang"),
    ("맑은 고딕", "malgun gothic"),
    ("微软雅黑", "microsoft yahei"),
    ("黑体", "simhei"),
    ("メイリオ", "meiryo"),
    ("微軟正黑體", "microsoft jhenghei"),
    ("helvetica", "arial"),
    ("helv", "ms sans serif"),
    ("tms rmn", "times new roman"),
    ("times", "times new roman"),
    ("arial cyr", "arial"),
    ("arial ce", "arial"),
    ("arial greek", "arial"),
    ("arial tur", "arial"),
    ("arial baltic", "arial"),
    ("times new roman cyr", "times new roman"),
    ("times new roman ce", "times new roman"),
    ("courier new cyr", "courier new"),
    ("courier new ce", "courier new"),
];

/// MDW of families without a synthetic measurement (`MDW_CORPUS`): PMingLiU/MingLiU have half-em digits.
static MDW_CORPUS: &[(&str, &str)] = &[("mingliu", "445566778899aabbccddeeffg"), ("pmingliu", "445566778899aabbccddeeffg")];

fn find<'a, T>(table: &'a [(&'static str, T)], key: &str) -> Option<&'a T> {
    table.binary_search_by(|(k, _)| (*k).cmp(key)).ok().and_then(|i| table.get(i)).map(|(_, v)| v)
}

/// `name.normalize('NFKC').trim().toLowerCase()`.
#[must_use]
pub fn normalize_family(name: &str) -> String {
    let t = uo_jsnum::text::trim(name);
    if t.is_ascii() {
        return t.to_ascii_lowercase();
    }
    let units = from_str(t);
    let d = uo_text::nfc::nfkd(&units);
    let c = uo_text::nfc::nfc(&d);
    let s = to_string_lossy(&c);
    uo_jsnum::text::to_lowercase(uo_jsnum::text::trim(&s))
}

/// `measuredFamily(name)`: the measured family for the document's name (`None` = not measured).
#[must_use]
pub fn measured_family(name: Option<&str>) -> Option<&'static str> {
    let n = name.filter(|n| !n.is_empty())?;
    with_normalized(n, |k| {
        let a = ALIASES.iter().find(|(from, _)| *from == k).map_or(k, |(_, to)| *to);
        ROW_PX.iter().find(|(key, _)| *key == a).map(|(key, _)| *key)
    })
}

/// [`normalize_family`] handed to `f`: an ASCII name is normalised on the stack (the viewport asks for every cell: no
/// allocation, C4b), any other through the Unicode path.
fn with_normalized<R>(name: &str, f: impl FnOnce(&str) -> R) -> R {
    let t = uo_jsnum::text::trim(name);
    let mut buf = [0u8; 64];
    match buf.get_mut(..t.len()) {
        Some(b) if t.is_ascii() => {
            for (d, c) in b.iter_mut().zip(t.bytes()) {
                *d = c.to_ascii_lowercase();
            }
            f(std::str::from_utf8(b).unwrap_or(""))
        }
        _ => f(&normalize_family(name)),
    }
}

/// `ppemOf`: the GDI ppem for a size in pt at 96 dpi (`MulDiv` rounds).
#[must_use]
pub fn ppem_of(size_pt: f64) -> i32 {
    round_i32((size_pt * 96.0) / 72.0).max(1)
}

/// Value `i` of a row string (first char absolute, the others the delta to the previous + 16).
fn row_at(s: &str, i: usize) -> Option<i32> {
    let mut v = 0;
    for (k, ch) in s.bytes().enumerate() {
        let d = b62(ch);
        v = if k == 0 { d } else { v + d - 16 };
        if k == i {
            return Some(v);
        }
    }
    None
}

/// Height (px) Excel gives a text line in the font: the height of an auto-fitted row whose largest font is this
/// one, and of each line of a wrapped text (`excelRowPx`). An unmeasured family is Arial.
#[must_use]
pub fn excel_row_px(name: Option<&str>, size_pt: f64, bold: bool) -> i32 {
    let fam = measured_family(name).unwrap_or("arial");
    let p = ppem_of(size_pt);
    let reg = find(ROW_PX, fam).copied().unwrap_or("");
    let bt = if bold { find(ROW_PX_BOLD, fam).copied() } else { None };
    if p <= ROW_PPEM_MIN {
        let first = row_at(bt.unwrap_or(reg), 0).unwrap_or(0);
        return round_i32(f64::from(first * p) / f64::from(ROW_PPEM_MIN)).max(1);
    }
    if p > ROW_PPEM_MAX {
        let last = row_at(reg, reg.len().saturating_sub(1)).unwrap_or(0);
        return round_i32(f64::from(last * p) / f64::from(ROW_PPEM_MAX));
    }
    let i = usize::try_from(p - ROW_PPEM_MIN).unwrap_or(0);
    if let Some(b) = bt
        && i < b.len()
    {
        return row_at(b, i).unwrap_or(0);
    }
    row_at(reg, i).unwrap_or(0)
}

/// Width (px) of the maximum digit of the Normal font in Excel (`excelMdw`); `None` when the family was not
/// measured (the caller estimates it with the canvas).
#[must_use]
pub fn excel_mdw(name: Option<&str>, size_pt: f64) -> Option<i32> {
    let fam = measured_family(name).unwrap_or("arial");
    let s = find(MDW, fam).or_else(|| find(MDW_CORPUS, fam)).copied()?;
    let p = ppem_of(size_pt);
    let at = |q: i32| -> i32 { usize::try_from(q - MDW_PPEM_MIN).ok().and_then(|i| s.as_bytes().get(i)).map_or(-1, |c| b62(*c)) };
    if p < MDW_PPEM_MIN {
        return Some(round_i32(f64::from(at(MDW_PPEM_MIN) * p) / f64::from(MDW_PPEM_MIN)).max(1));
    }
    if p > MDW_PPEM_MAX {
        return Some(round_i32(f64::from(at(MDW_PPEM_MAX) * p) / f64::from(MDW_PPEM_MAX)));
    }
    Some(at(p))
}

/// File column width (character units) → px (`colWidthToPx`).
#[must_use]
pub fn col_width_to_px(width: f64, mdw: i32) -> f64 {
    let m = f64::from(mdw);
    math::floor(((256.0 * width + math::floor(128.0 / m)) / 256.0) * m)
}

/// Default column width (px) of a sheet (`defaultColPx`).
#[must_use]
pub fn default_col_px(mdw: i32, default_col_width: Option<f64>, base_col_width: Option<f64>) -> f64 {
    if let Some(w) = default_col_width
        && w > 0.0
    {
        return col_width_to_px(w, mdw);
    }
    let base = base_col_width.unwrap_or(8.0);
    let m = f64::from(mdw);
    math::ceil((base * m + 2.0 * math::ceil(m / 4.0) + 1.0) / 8.0) * 8.0
}

/// Saved height (pt) → px as Excel shows it, truncating to the pixel (`rowPtToPx`).
#[must_use]
pub fn row_pt_to_px(pt: f64) -> f64 {
    math::floor((pt * 96.0) / 72.0 + 1e-6)
}

/// The GDI advance corrections of one (family, bold, ppem): pairs (character, width) of the glyphs whose GDI
/// advance differs from the rounded canvas advance (`gdiGlyphFixes`). The row is the `&'static` text of the
/// table; a lookup scans it (≤ 200 pairs; callers memoize per font).
#[derive(Clone, Copy, Debug)]
pub struct GlyphFixes(&'static str);

impl GlyphFixes {
    /// The corrected advance of `ch` (the LAST pair of a repeated character wins, as the TS `Map.set`).
    #[must_use]
    pub fn get(&self, ch: char) -> Option<i32> {
        let mut found = None;
        let mut it = self.0.chars();
        while let (Some(c), Some(d)) = (it.next(), it.next()) {
            if c == ch {
                found = u8::try_from(u32::from(d)).ok().map(b62);
            }
        }
        found
    }

    /// The pairs `(character, width)`.
    pub fn pairs(&self) -> impl Iterator<Item = (char, i32)> + '_ {
        let mut it = self.0.chars();
        std::iter::from_fn(move || {
            let c = it.next()?;
            let d = it.next()?;
            Some((c, u8::try_from(u32::from(d)).ok().map_or(-1, b62)))
        })
    }
}

/// `gdiGlyphFixes(family, bold, ppem)`; `None` when the family/ppem was not measured.
#[must_use]
pub fn gdi_glyph_fixes(family: &str, bold: bool, ppem: i32) -> Option<GlyphFixes> {
    if !(GLYPH_PPEM_MIN..=GLYPH_PPEM_MAX).contains(&ppem) {
        return None;
    }
    let key = format!("{}|{}", uo_jsnum::text::to_lowercase(uo_jsnum::text::trim(family)), i32::from(bold));
    let rows = find_rows(&key)?;
    let s = rows.get(usize::try_from(ppem - GLYPH_PPEM_MIN).ok()?)?;
    Some(GlyphFixes(s))
}

fn find_rows(key: &str) -> Option<&'static [&'static str]> {
    GLYPH_FIX.binary_search_by(|(k, _)| (*k).cmp(key)).ok().and_then(|i| GLYPH_FIX.get(i)).map(|(_, v)| *v)
}

/// The family has a measured glyph table (`hasGlyphTable`).
#[must_use]
pub fn has_glyph_table(family: &str, bold: bool) -> bool {
    let key = format!("{}|{}", uo_jsnum::text::to_lowercase(uo_jsnum::text::trim(family)), i32::from(bold));
    find_rows(&key).is_some()
}

/// Effective descent measured in Excel per ppem for Aptos and Segoe UI (`EXCEL_DESC`, lab e28).
const APTOS_DESC: &[(i32, i32)] =
    &[(8, 2), (9, 2), (11, 3), (12, 3), (13, 4), (15, 4), (16, 4), (17, 5), (19, 5), (20, 5), (21, 6), (24, 7), (27, 7)];
const SEGOE_DESC: &[(i32, i32)] =
    &[(8, 2), (9, 2), (11, 2), (12, 3), (13, 4), (15, 5), (16, 5), (17, 6), (19, 6), (20, 8), (21, 9), (24, 8), (27, 9)];

/// Descent (px) the automatic row height uses for the font (`rowDescentPx`): a cell with n lines measures
/// `n·R(font) + max(0, row descent − font descent)`. `None` = a measured family without a known descent.
#[must_use]
pub fn row_descent_px(name: Option<&str>, size_pt: f64, bold: bool) -> Option<i32> {
    with_normalized(name.unwrap_or(""), |fam| row_descent_of(fam, name, size_pt, bold))
}

fn row_descent_of(fam: &str, name: Option<&str>, size_pt: f64, bold: bool) -> Option<i32> {
    let p = ppem_of(size_pt);
    let xl = if fam.starts_with("aptos") {
        Some(APTOS_DESC)
    } else if fam.starts_with("segoe ui") {
        Some(SEGOE_DESC)
    } else {
        None
    };
    if let Some(xl) = xl {
        let at = |q: i32| xl.iter().find(|(k, _)| *k == q).map(|(_, v)| *v);
        if p > 27 {
            return Some(round_i32(f64::from(at(27).unwrap_or(0) * p) / 27.0));
        }
        let mut q = p.max(8);
        while q >= 8 {
            if let Some(v) = at(q) {
                return Some(v);
            }
            q -= 1;
        }
        return Some(2);
    }
    let measured = measured_family(name);
    let key: &str = if find(ROW_DESC, fam).is_some() { fam } else { measured.unwrap_or("arial") };
    let reg = find(ROW_DESC, key).copied()?;
    let s = if bold { find(ROW_DESC_BOLD, key).copied().filter(|b| !b.is_empty()).unwrap_or(reg) } else { reg };
    if p < DESC_PPEM_MIN {
        return Some(1);
    }
    if p > DESC_PPEM_MAX {
        let last = s.bytes().last().map_or(-1, b62);
        return Some(round_i32(f64::from(last * p) / f64::from(DESC_PPEM_MAX)));
    }
    let i = usize::try_from(p - DESC_PPEM_MIN).ok()?;
    s.as_bytes().get(i).map(|c| b62(*c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibri_11() {
        // Calibri 11 pt = 15 ppem: MDW 7, row 20 px, margin tables of the TS
        assert_eq!(ppem_of(11.0), 15);
        assert_eq!(excel_mdw(Some("Calibri"), 11.0), Some(7));
        assert_eq!(excel_row_px(Some("Calibri"), 11.0, false), 20);
        assert_eq!(col_width_to_px(9.140_625, 7), 64.0);
        assert_eq!(default_col_px(7, None, None), 64.0);
        assert_eq!(row_pt_to_px(15.0), 20.0);
        assert_eq!(measured_family(Some("Helvetica")), Some("arial"));
        assert_eq!(measured_family(Some("No Such Font")), None);
    }
}
