//! Replay of `tests/captures/dml_color.capture` (`tools/capture-dml-color.mjs`, the frozen TS
//! `readSpPr` → `srgbOf` and `resolveDrawingColor`): the Word colour path of uo-docx plan G12
//! (`dml::word_spec`, `WordSpec::to_ts_string`, `dml::resolve_word_spec`) gives the TS spec
//! string, alpha and resolved colour on every case.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_drawingml::dml::{NoScheme, resolve_word_spec, word_spec};
use uo_opc::xml::{ParseOptions, parse};

const THEME: &[(&str, &str)] = &[
    ("dk1", "000000"),
    ("lt1", "FFFFFF"),
    ("dk2", "44546A"),
    ("lt2", "E7E6E6"),
    ("accent1", "4472C4"),
    ("accent2", "ED7D31"),
    ("accent3", "A5A5A5"),
    ("accent4", "FFC000"),
    ("accent5", "5B9BD5"),
    ("accent6", "70AD47"),
    ("hlink", "0563C1"),
    ("folHlink", "954F72"),
    ("weird", "zz00zz"),
];

#[test]
fn word_colour_path_matches_the_ts() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/captures/dml_color.capture")).unwrap();
    let theme: Vec<(Box<str>, Box<str>)> = THEME.iter().map(|(k, v)| (Box::from(*k), Box::from(*v))).collect();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.starts_with('@')) {
        let f: Vec<&str> = line.split('\t').collect();
        let [xml, fill, alpha, resolved] = f[..] else { panic!("bad line {line}") };
        let doc = parse(xml, &ParseOptions::LENIENT).unwrap();
        let solid = doc.root().first_child("a:solidFill").unwrap();
        let spec = word_spec(solid, &NoScheme);
        let got_fill = spec.as_ref().map_or_else(|| "~".to_string(), |s| s.to_ts_string());
        assert_eq!(got_fill, fill, "fill of {xml}");
        let got_alpha = spec.as_ref().and_then(|s| s.alpha).map_or_else(|| "~".to_string(), uo_jsnum::format::to_js_string);
        assert_eq!(got_alpha, alpha, "alpha of {xml}");
        let got = spec.as_ref().and_then(|s| resolve_word_spec(&s.head, s.parsed_mods(), &theme[..])).unwrap_or_else(|| "~".into());
        assert_eq!(got, resolved, "resolved of {xml}");
        n += 1;
    }
    assert_eq!(n, 1500);
}
