//! `diagram-metrics.ts`: APPROXIMATE text metrics for the SmartArt engine's font fitting (the engine
//! picks the largest size that fits, as the application does when it redoes the layout): advance widths
//! (1/1000 em, base 36 with two digits; U+0020–U+007E and U+00A0–U+00FF) of the open metric-compatible
//! faces and the line height. No browser, no font file: the tables are the TS's own.
//!
//! The width function is the `DiagramMeasure` of `diagram-layout.ts:22`: the engine takes a
//! [`DiagramMeasure`] (the read path passes [`ApproxMeasure`], as the TS does — it never passes its
//! own measurer), so a host that has real advance tables can plug them in without touching the engine.

use std::sync::OnceLock;

use uo_jsnum::parse::parse_int;

use super::metrics_data::{self as data, Face};

/// `DiagramFont` (`diagram-layout.ts:23`).
#[derive(Clone, Copy, Debug, Default)]
pub struct DiagramFont<'a> {
    pub family: Option<&'a str>,
    pub bold: bool,
    pub italic: bool,
}

/// `DiagramMeasure`: the width (EMU) of `text` at `size_pt`.
pub trait DiagramMeasure {
    fn width(&self, text: &str, size_pt: f64, font: &DiagramFont<'_>) -> f64;
    /// Line height as a fraction of the size for the family (`lineFactor`).
    fn line_factor(&self, family: Option<&str>) -> f64;
}

/// The built-in tables (`measureApprox` and `lineFactor`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ApproxMeasure;

impl DiagramMeasure for ApproxMeasure {
    fn width(&self, text: &str, size_pt: f64, font: &DiagramFont<'_>) -> f64 {
        measure_approx(text, size_pt, font)
    }
    fn line_factor(&self, family: Option<&str>) -> f64 {
        line_factor(family)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FaceId {
    Carlito,
    Sans,
    Serif,
    Caladea,
    Aptos,
}

fn face_def(f: FaceId) -> &'static Face {
    match f {
        FaceId::Carlito => &data::CARLITO,
        FaceId::Sans | FaceId::Aptos => &data::SANS,
        FaceId::Serif => &data::SERIF,
        FaceId::Caladea => &data::CALADEA,
    }
}

fn decode(s: &str) -> Vec<f64> {
    s.as_bytes().chunks(2).map(|c| parse_int(std::str::from_utf8(c).unwrap_or(""), 36)).collect()
}

/// `widths(face, style)`: style 0 = r, 1 = b, 2 = i, 3 = z.
fn widths(face: FaceId, style: usize) -> &'static [f64] {
    static CACHE: OnceLock<Vec<Vec<f64>>> = OnceLock::new();
    let all = CACHE.get_or_init(|| {
        let mut v = Vec::with_capacity(16);
        for f in [FaceId::Carlito, FaceId::Sans, FaceId::Serif, FaceId::Caladea] {
            for s in 0..4 {
                v.push(decode(face_def(f).w.get(s).copied().unwrap_or("")));
            }
        }
        v
    });
    let fi = match face {
        FaceId::Carlito => 0,
        FaceId::Sans | FaceId::Aptos => 1,
        FaceId::Serif => 2,
        FaceId::Caladea => 3,
    };
    all.get(fi * 4 + style).map_or(&[], Vec::as_slice)
}

fn aptos_ratios(bold: bool) -> &'static [f64] {
    static CACHE: OnceLock<[Vec<f64>; 2]> = OnceLock::new();
    let both = CACHE.get_or_init(|| {
        let mk = |s: &str| decode(s).into_iter().map(|v| v / 100.0).collect::<Vec<f64>>();
        [mk(data::APTOS_R), mk(data::APTOS_B)]
    });
    both.get(usize::from(bold)).map_or(&[], Vec::as_slice)
}

/// `faceOf(family)`: the table and the width factor.
fn face_of(family: Option<&str>) -> (FaceId, f64) {
    let f = uo_jsnum::text::to_lowercase(family.unwrap_or(""));
    let has = |s: &str| f.contains(s);
    if has("aptos") {
        return (FaceId::Aptos, if has("narrow") { 0.85 } else { 1.0 });
    }
    if has("calibri") || has("carlito") || has("corbel") || has("candara") {
        return (FaceId::Carlito, 1.0);
    }
    if has("cambria") || has("caladea") || has("constantia") {
        return (FaceId::Caladea, 1.0);
    }
    if has("times") || has("liberation serif") || has("tinos") {
        return (FaceId::Serif, 1.0);
    }
    if has("georgia") {
        return (FaceId::Serif, 1.12);
    }
    if has("garamond") {
        return (FaceId::Serif, 0.94);
    }
    if has("palatino") || has("book antiqua") || has("bookman") {
        return (FaceId::Serif, 1.06);
    }
    if has("narrow") || has("condensed") {
        return (FaceId::Sans, 0.82);
    }
    if has("verdana") {
        return (FaceId::Sans, 1.2);
    }
    if has("century gothic") || has("avant") {
        return (FaceId::Sans, 1.12);
    }
    if has("trebuchet") || has("gill") || has("franklin") {
        return (FaceId::Sans, 0.95);
    }
    (FaceId::Sans, 1.0)
}

/// `measureApprox` (`diagram-metrics.ts:77`): the width in EMU of `text` at `size_pt`.
pub fn measure_approx(text: &str, size_pt: f64, font: &DiagramFont<'_>) -> f64 {
    let (face, k) = face_of(font.family);
    let ap = (face == FaceId::Aptos).then(|| aptos_ratios(font.bold));
    let st = if font.bold {
        if font.italic { 3 } else { 1 }
    } else if font.italic {
        2
    } else {
        0
    };
    let w = widths(face, st);
    let at = |i: usize| w.get(i).copied().unwrap_or(f64::NAN);
    let mut em = 0.0;
    for ch in text.chars() {
        let c = u32::from(ch);
        let i: Option<usize> = if (32..127).contains(&c) {
            usize::try_from(c - 32).ok()
        } else if (160..256).contains(&c) {
            usize::try_from(c - 160 + 95).ok()
        } else {
            None
        };
        let mut v;
        if let Some(i) = i {
            v = at(i) / 1000.0;
        } else if (0x2e80..0xffa0).contains(&c) {
            em += 1.0; // CJK / full width
            continue;
        } else if c == 9 {
            em += 2.0;
            continue;
        } else {
            v = (at(33) + at(65)) / 2000.0; // the rest: average of A and a
        }
        if let Some(ap) = ap {
            let r = if (32..127).contains(&c) { ap.get(usize::try_from(c - 32).unwrap_or(0)).copied().unwrap_or(f64::NAN) } else { 0.95 };
            v *= r;
        }
        em += v;
    }
    em * k * size_pt * 12700.0
}

/// `lineFactor` (`diagram-metrics.ts:92`): line height (fraction of the size) of the family.
pub fn line_factor(family: Option<&str>) -> f64 {
    let (f, _) = face_of(family);
    if f == FaceId::Aptos { 1.2207 } else { face_def(f).line }
}
