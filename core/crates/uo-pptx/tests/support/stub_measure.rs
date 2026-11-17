//! The deterministic measurement stub of the TS captures (`js/lib/record-ctx.mjs` `stubMeasure`) answered into a
//! `MeasureTable`, shared by the parity example (`examples/emit_uolf.rs`) and the emission tests: the same
//! advances the recorded TS streams were made with, so the Rust emission is compared exactly (no font files).
#![allow(dead_code, clippy::unwrap_used)]

use uo_dml_render::PartSource;
use uo_dml_render::text::{FaceKey, Kerning, MeasureReq, MeasureTable, TextMetrics};
use uo_jsnum::format::to_js_string;
use uo_pptx::Deck;

const KERN_PAIRS: [&str; 33] = [
    "AV", "AW", "AY", "Av", "Aw", "Ay", "FA", "LT", "LV", "LY", "P.", "P,", "Ta", "Te", "To", "Tr", "Tu", "Tw", "Ty", "VA", "Va", "Ve",
    "Vo", "Wa", "We", "Wo", "Ya", "Ye", "Yo", "r.", "r,", "y.", "y,",
];

/// `/(\d+(?:\.\d+)?)px/` on the CSS font string, `parseFloat` of the match (10 when absent).
fn font_px(font: &str) -> f64 {
    let b = font.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
                j += 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if font.get(j..).is_some_and(|r| r.starts_with("px")) {
                return uo_jsnum::parse::parse_float(&font[i..j]);
            }
        }
        i += 1;
    }
    10.0
}

/// `/(^|\s)(bold|[6-9]00)(\s|$)/.test(font)`.
fn font_bold(font: &str) -> bool {
    let tok = |t: &str| t == "bold" || (t.len() == 3 && t.ends_with("00") && matches!(t.as_bytes()[0], b'6'..=b'9'));
    font.split(uo_jsnum::text::is_js_space).any(tok)
}

/// `stubMeasure(font, text, kerning)`.
pub fn stub_measure(font: &str, text: &str, kerning_none: bool) -> f64 {
    let size = font_px(font);
    let bold = font_bold(font);
    let mut em = 0.0f64;
    let mut prev = String::new();
    for ch in text.chars() {
        if !kerning_none {
            let mut pair = prev.clone();
            pair.push(ch);
            if KERN_PAIRS.contains(&pair.as_str()) {
                em += -0.07;
            }
        }
        prev = ch.to_string();
        let c = u32::from(ch);
        let a = if ch == ' ' {
            0.278
        } else if ".,:;'|!ijlI".contains(ch) {
            0.26
        } else if "mwMW".contains(ch) {
            0.82
        } else if ch.is_ascii_uppercase() {
            0.66
        } else if ch.is_ascii_digit() {
            0.556
        } else if c >= 0x2e80 {
            1.0
        } else {
            0.5
        };
        em += a;
    }
    if bold {
        em *= 1.06;
    }
    em * size
}

/// `layoutFontStr` with the default stack of the TS (`DEFAULT_FONT_STACK`).
pub fn font_str(face: &FaceKey, size_px: f64) -> String {
    format!(
        "{}{}{}px '{}', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif",
        if face.italic { "italic " } else { "" },
        if face.bold { "700 " } else { "400 " },
        to_js_string(size_px),
        face.family
    )
}

/// `imageSize` of `ts-paint.mjs`: PNG, GIF and JPEG headers; 96 x 96 otherwise.
pub fn image_size(b: &[u8]) -> (f64, f64) {
    let g = |i: usize| u32::from(b.get(i).copied().unwrap_or(0));
    if b.len() > 24 && b[0] == 0x89 && b[1] == 0x50 {
        return (f64::from(g(16) << 24 | g(17) << 16 | g(18) << 8 | g(19)), f64::from(g(20) << 24 | g(21) << 16 | g(22) << 8 | g(23)));
    }
    if b.len() > 10 && b[0] == 0x47 && b[1] == 0x49 {
        return (f64::from(g(6) | g(7) << 8), f64::from(g(8) | g(9) << 8));
    }
    if b.len() > 4 && b[0] == 0xff && b[1] == 0xd8 {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xff {
                i += 1;
                continue;
            }
            let mk = b[i + 1];
            if (0xc0..=0xcf).contains(&mk) && mk != 0xc4 && mk != 0xc8 && mk != 0xcc {
                return (f64::from(g(i + 7) << 8 | g(i + 8)), f64::from(g(i + 5) << 8 | g(i + 6)));
            }
            i += 2 + usize::try_from(g(i + 2) << 8 | g(i + 3)).unwrap_or(0);
        }
    }
    (96.0, 96.0)
}

pub fn answer(deck: &Deck, table: &mut MeasureTable, reqs: &[MeasureReq]) {
    for r in reqs {
        match r {
            MeasureReq::CharAt1000 { face, ch } => {
                let w = stub_measure(&font_str(face, 1000.0), &ch.to_string(), true);
                table.set_char_at_1000(face, *ch, w);
            }
            MeasureReq::KernAt1000 { face, text } => {
                let f = font_str(face, 1000.0);
                table.set_kern_at_1000(face, text, stub_measure(&f, text, true), stub_measure(&f, text, false));
            }
            MeasureReq::Width(k) => {
                let w = stub_measure(&font_str(&k.face, k.size_px.value()), &k.text, k.kerning == Kerning::None);
                table.set_width(k, w);
            }
            MeasureReq::ImageSize { media } => {
                let (w, h) = deck.source().media_entry(*media).map_or((96.0, 96.0), |(_, b)| image_size(&b));
                table.set_image_size(*media, w, h);
            }
            MeasureReq::MetaText { font, text } => {
                // B5b: the metafile players' `measureText` under a raw CSS font: the recorder's width and bounding box
                let size = font_px(font);
                table.set_meta_text(font, text, stub_measure(font, text, false), 0.9 * size, 0.21 * size);
            }
            // the capture runs in Node, which has no `createImageBitmap`: no compressed EMF+ bitmap is ever decoded
            MeasureReq::MetaDecode { key, .. } => table.set_meta_decoded(key, None),
            MeasureReq::Metrics(k) => {
                // the recorder's `measureText`: the actual box 0.72 / 0.21 of the size, the font box 0.9 / 0.21 (the `fontK` of the charts)
                let size = k.size_px.value();
                let width = stub_measure(&font_str(&k.face, size), &k.text, k.kerning == Kerning::None);
                table.set_metrics(
                    k,
                    TextMetrics {
                        width,
                        actual_left: 0.0,
                        actual_right: width,
                        actual_ascent: 0.72 * size,
                        actual_descent: 0.21 * size,
                        font_ascent: Some(0.9 * size),
                        font_descent: Some(0.21 * size),
                    },
                );
            }
            MeasureReq::FontAvailable { .. } => {}
        }
    }
}
