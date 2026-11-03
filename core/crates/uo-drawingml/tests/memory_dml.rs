//! Memory gate M1 of the colour and theme readers (uo-docx plan G12): every call returns the live
//! bytes to their value before it, outputs dropped.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::hint::black_box;

use uo_alloc::assert_no_leak;
use uo_drawingml::dml::{NoScheme, SpecHead, read_color, resolve_word_spec, to_hex_upper, word_spec};
use uo_drawingml::theme;
use uo_opc::xml::{ParseOptions, parse};

#[test]
fn m1_colour_and_theme() {
    let xml = r#"<x><a:schemeClr val="accent1"><a:alpha val="50000"/><a:lumMod val="75000"/><a:lumOff/></a:schemeClr></x>"#;
    let doc = parse(xml, &ParseOptions::LENIENT).unwrap();
    let theme_colors: Vec<(Box<str>, Box<str>)> = vec![("accent1".into(), "4472C4".into())];
    assert_no_leak(|| black_box(word_spec(doc.root(), &NoScheme)));
    assert_no_leak(|| black_box(word_spec(doc.root(), &theme_colors[..])));
    assert_no_leak(|| black_box(read_color(doc.root().first_child("a:schemeClr").unwrap(), "")));
    let spec = word_spec(doc.root(), &NoScheme).unwrap();
    assert_no_leak(|| black_box(spec.to_ts_string()));
    assert_no_leak(|| black_box(resolve_word_spec(&spec.head, spec.parsed_mods(), &theme_colors[..])));
    assert_no_leak(|| black_box(resolve_word_spec(&SpecHead::Hex("bad".into()), [], &theme_colors[..])));
    assert_no_leak(|| black_box(to_hex_upper(1.0, f64::NAN, 300.0)));
    let t = r#"<a:theme><a:themeElements><a:clrScheme><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1></a:clrScheme><a:fontScheme><a:majorFont><a:latin typeface="A"/><a:font script="Jpan" typeface="B"/></a:majorFont></a:fontScheme><a:fmtScheme><a:fillStyleLst><a:solidFill/></a:fillStyleLst><a:lnStyleLst><a:ln w="1"/></a:lnStyleLst><a:effectStyleLst><a:effectStyle/></a:effectStyleLst></a:fmtScheme></a:themeElements></a:theme>"#;
    let tdoc = parse(t, &ParseOptions::LENIENT).unwrap();
    assert_no_leak(|| black_box(theme::read(&tdoc)));
}
