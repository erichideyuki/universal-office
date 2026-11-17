//! The deterministic `measureText` of the parity captures (`uo-pptx/js/lib/record-ctx.mjs` `stubMeasure`, default kerning), for a
//! raw CSS font string and UTF-16 text: advance table in em fractions, bold 6 % wider, a few kerning pairs; the font bounding
//! box is `0.9 x size` / `0.21 x size`. Shared by `examples/meta_recipe.rs` and the metafile tests.
#![allow(dead_code)]

use uo_dml_render::metafile::Metrics;

/// The leftmost `(\d+(?:\.\d+)?)px` of the font, as a number (the TS `parseFloat`), else 10.
fn font_size(font: &[u16]) -> f64 {
    let s: Vec<char> = font.iter().map(|&u| char::from_u32(u32::from(u)).unwrap_or('\u{fffd}')).collect();
    let mut i = 0;
    while i < s.len() {
        if s[i].is_ascii_digit() {
            let mut j = i;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            let mut end = j;
            if j < s.len() && s[j] == '.' {
                let mut k = j + 1;
                while k < s.len() && s[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 {
                    end = k;
                }
            }
            if s.get(end) == Some(&'p') && s.get(end + 1) == Some(&'x') {
                let txt: String = s[i..end].iter().collect();
                return uo_jsnum::parse::parse_float(&txt);
            }
            // the digits did not end in `px`: the regex retries from the next position
        }
        i += 1;
    }
    10.0
}

/// `/(^|\s)(bold|[6-9]00)(\s|$)/`
fn is_bold(font: &[u16]) -> bool {
    let s: String = font.iter().map(|&u| char::from_u32(u32::from(u)).unwrap_or('\u{fffd}')).collect();
    let b: Vec<char> = s.chars().collect();
    for i in 0..b.len() {
        let start_ok = i == 0 || uo_jsnum::text::is_js_space(b[i - 1]);
        if !start_ok {
            continue;
        }
        for w in ["bold", "600", "700", "800", "900"] {
            let wc: Vec<char> = w.chars().collect();
            if b.get(i..i + wc.len()) == Some(&wc[..]) && (i + wc.len() == b.len() || uo_jsnum::text::is_js_space(b[i + wc.len()])) {
                return true;
            }
        }
    }
    false
}

const KERN: [&str; 33] = [
    "AV", "AW", "AY", "Av", "Aw", "Ay", "FA", "LT", "LV", "LY", "P.", "P,", "Ta", "Te", "To", "Tr", "Tu", "Tw", "Ty", "VA", "Va", "Ve",
    "Vo", "Wa", "We", "Wo", "Ya", "Ye", "Yo", "r.", "r,", "y.", "y,",
];

/// Code points of UTF-16 (a lone surrogate is one element: `for (const ch of text)`).
fn points(text: &[u16]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = u32::from(text[i]);
        if (0xd800..0xdc00).contains(&c) && i + 1 < text.len() && (0xdc00..0xe000).contains(&u32::from(text[i + 1])) {
            out.push(0x10000 + ((c - 0xd800) << 10) + (u32::from(text[i + 1]) - 0xdc00));
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

pub fn stub_metrics(font: &[u16], text: &[u16]) -> Metrics {
    let size = font_size(font);
    let bold = is_bold(font);
    let mut em = 0.0f64;
    let mut prev = String::new();
    for c in points(text) {
        let ch = char::from_u32(c).unwrap_or('\u{fffd}');
        let pair = format!("{prev}{ch}");
        em += if KERN.contains(&pair.as_str()) { -0.07 } else { 0.0 };
        prev = ch.to_string();
        let a = if ch == ' ' {
            0.278
        } else if matches!(ch, '.' | ',' | ':' | ';' | '\'' | '|' | '!' | 'i' | 'j' | 'l' | 'I') {
            0.26
        } else if matches!(ch, 'm' | 'w' | 'M' | 'W') {
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
    Metrics { width: em * size, font_ascent: 0.9 * size, font_descent: 0.21 * size }
}
