//! The geometry of the grid as Excel computes it (96 dpi, zoom 100%), with the tables MEASURED in Excel 365 (`grid-metrics.ts`
//! over `excel-metrics-data.ts`): column widths by the maximum digit width (MDW) of the Normal font and row heights auto-fitted by
//! the largest font of the row. The converter needs them to turn the cell anchors of the shapes into EMU with the same widths and
//! heights in px as the grid (`drawing.ts`).
//!
//! The grid port (frontier C3a) has its own copy of these tables and functions; this one is the converter's (record C2, C2-6).

use uo_jsnum::math::{ceil, floor, max, round};

use super::data::{MDW, MDW_PPEM_MAX, MDW_PPEM_MIN, ROW_PPEM_MAX, ROW_PPEM_MIN, ROW_PX, ROW_PX_BOLD};

const B62: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// The MDW of families without a synthetic measurement (PMingLiU/MingLiU have half-width digits — ⌊ppem/2⌋).
const MDW_CORPUS: [(&str, &str); 2] = [("pmingliu", "445566778899aabbccddeeffg"), ("mingliu", "445566778899aabbccddeeffg")];

/// Localized names and Windows substitutions (FontSubstitutes) → measured family.
fn alias(k: &str) -> Option<&'static str> {
    Some(match k {
        "宋体" | "新宋体" => "simsun",
        "ms pゴシック" => "ms pgothic",
        "ms ゴシック" => "ms gothic",
        "游ゴシック" => "yu gothic",
        "新細明體" => "pmingliu",
        "細明體" => "mingliu",
        "돋움" => "dotum",
        "굴림" => "gulim",
        "바탕" => "batang",
        "맑은 고딕" => "malgun gothic",
        "微软雅黑" => "microsoft yahei",
        "黑体" => "simhei",
        "メイリオ" => "meiryo",
        "微軟正黑體" => "microsoft jhenghei",
        "helvetica" => "arial",
        "helv" => "ms sans serif",
        "tms rmn" | "times" => "times new roman",
        "arial cyr" | "arial ce" | "arial greek" | "arial tur" | "arial baltic" => "arial",
        "times new roman cyr" | "times new roman ce" => "times new roman",
        "courier new cyr" | "courier new ce" => "courier new",
        _ => return None,
    })
}

fn row_px_of(table: &'static [(&'static str, &'static str)], fam: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == fam).map(|(_, v)| *v)
}

/// The measured family for the name of the document (`None` = not measured). The TS normalizes with NFKC first; here only
/// the ASCII fast path is exact (the compatibility forms of non-ASCII names are not normalized: record C2, C2-6).
fn measured_family(name: &str) -> Option<&'static str> {
    let k = uo_jsnum::text::to_lowercase(uo_jsnum::text::trim(name));
    let a: String = alias(&k).map_or_else(|| k.clone(), str::to_owned);
    ROW_PX.iter().find(|(f, _)| *f == a).map(|(f, _)| *f)
}

/// The ppem of the GDI for a size in pt at 96 dpi (MulDiv rounds).
fn ppem_of(size_pt: f64) -> i32 {
    uo_jsnum::conv::to_int32(max(1.0, round((size_pt * 96.0) / 72.0)))
}

fn decode_rows(s: &str) -> Vec<i32> {
    let mut v = 0i32;
    let mut out = Vec::with_capacity(s.len());
    for (i, ch) in s.bytes().enumerate() {
        let d = B62.iter().position(|&b| b == ch).map_or(-1, |p| i32::try_from(p).unwrap_or(-1));
        v = if i == 0 { d } else { v + d - 16 };
        out.push(v);
    }
    out
}

/// The height (px) Excel gives a line of text in the font — the height of an auto-fitted row whose largest font is this one.
/// A font not measured → Arial's (Excel's mapping).
pub fn excel_row_px(name: &str, size_pt: f64, bold: bool) -> f64 {
    let fam = measured_family(name).unwrap_or("arial");
    let p = ppem_of(size_pt);
    let reg = row_px_of(&ROW_PX, fam).map(decode_rows).unwrap_or_default();
    // bold measured up to ppem 40 (only where it differs from regular)
    let bt = if bold { row_px_of(&ROW_PX_BOLD, fam).map(decode_rows) } else { None };
    if p <= ROW_PPEM_MIN {
        let first = bt.as_ref().unwrap_or(&reg).first().copied().unwrap_or(0);
        return max(1.0, round((f64::from(first) * f64::from(p)) / f64::from(ROW_PPEM_MIN)));
    }
    if p > ROW_PPEM_MAX {
        return round((f64::from(reg.last().copied().unwrap_or(0)) * f64::from(p)) / f64::from(ROW_PPEM_MAX));
    }
    let i = usize::try_from(p - ROW_PPEM_MIN).unwrap_or(0);
    match &bt {
        Some(b) if i < b.len() => f64::from(b[i]),
        _ => f64::from(reg.get(i).copied().unwrap_or(0)),
    }
}

/// The width of the maximum digit (px) of the Normal font in Excel; `None` when the family was not measured.
pub fn excel_mdw(name: &str, size_pt: f64) -> Option<f64> {
    let fam = measured_family(name).unwrap_or("arial");
    let s = row_px_of(&MDW, fam).or_else(|| MDW_CORPUS.iter().find(|(k, _)| *k == fam).map(|(_, v)| *v))?;
    let p = ppem_of(size_pt);
    let at = |q: i32| -> f64 {
        let idx = usize::try_from(q - MDW_PPEM_MIN).unwrap_or(0);
        s.as_bytes().get(idx).and_then(|&c| B62.iter().position(|&b| b == c)).map_or(-1.0, |v| f64::from(u32::try_from(v).unwrap_or(0)))
    };
    if p < MDW_PPEM_MIN {
        return Some(max(1.0, round((at(MDW_PPEM_MIN) * f64::from(p)) / f64::from(MDW_PPEM_MIN))));
    }
    if p > MDW_PPEM_MAX {
        return Some(round((at(MDW_PPEM_MAX) * f64::from(p)) / f64::from(MDW_PPEM_MAX)));
    }
    Some(at(p))
}

/// A column width (units of characters of the file) → px.
pub fn col_width_to_px(width: f64, mdw: f64) -> f64 {
    floor(((256.0 * width + floor(128.0 / mdw)) / 256.0) * mdw)
}

/// The default column width (px) of a sheet.
pub fn default_col_px(mdw: f64, default_col_width: Option<f64>, base_col_width: Option<f64>) -> f64 {
    if let Some(w) = default_col_width.filter(|&w| w > 0.0) {
        return col_width_to_px(w, mdw);
    }
    let base = base_col_width.unwrap_or(8.0);
    ceil((base * mdw + 2.0 * ceil(mdw / 4.0) + 1.0) / 8.0) * 8.0
}

/// A stored height (pt) → px as Excel shows it (truncated to the pixel).
pub fn row_pt_to_px(pt: f64) -> f64 {
    floor((pt * 96.0) / 72.0 + 1e-6)
}
