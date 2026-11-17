//! Behaviour tests of the deck half the parity corpora reach only indirectly: the stale-slide re-read after
//! an edit (read set), the slide number of `slidenum` fields with `firstSlideNum`, the part store's JS `Map`
//! semantics, embedded fonts (sfnt, odttf, the EOT gap), `hasBackground` / `countTextUnits`, typefaces.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_dml_render::model::{Fill, FillKind, Paragraph, Run, RunExtras, Shape, TextBody};
use uo_pptx::fonts::{FontVariant, deobfuscate_font};
use uo_pptx::read::{count_text_units, has_background, ph_key};
use uo_pptx::timing::AnimTarget;
use uo_pptx::{Deck, Limits, PartStore};

fn deck_bytes(n: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks/b1a").join(format!("{n}.pptx"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn part_store_has_js_map_semantics() {
    let mut s = PartStore::new();
    let g1 = s.set("a", Arc::from(&b"1"[..]));
    s.set("b", Arc::from(&b"2"[..]));
    let g3 = s.set("a", Arc::from(&b"3"[..]));
    assert!(g3 > g1);
    assert_eq!(s.names().collect::<Vec<_>>(), ["a", "b"], "an existing name keeps its position");
    assert_eq!(&**s.bytes("a").unwrap(), b"3");
    s.set("c", Arc::from(&b"4"[..]));
    assert!(s.remove("a") && !s.remove("a"));
    s.set("a", Arc::from(&b"5"[..]));
    assert_eq!(s.names().collect::<Vec<_>>(), ["b", "c", "a"], "a removed part comes back at the end");
    // TextEncoder drops a BOM on the way back; TextDecoder strips it
    s.set("t", Arc::from(&b"\xEF\xBB\xBFhi"[..]));
    assert_eq!(s.text("t").as_deref(), Some("hi"));
    s.set_text("t", "h\u{e9}");
    assert_eq!(&**s.bytes("t").unwrap(), "h\u{e9}".as_bytes());
    let again =
        PartStore::from_parts([("x".into(), Arc::from(&b"1"[..])), ("y".into(), Arc::from(&b"2"[..])), ("x".into(), Arc::from(&b"3"[..]))]);
    assert_eq!(again.names().collect::<Vec<_>>(), ["x", "y"]);
    assert_eq!(&**again.bytes("x").unwrap(), b"3");
}

#[test]
fn save_and_reopen_keeps_every_part_in_order() {
    let deck = Deck::open(&deck_bytes("0008"), Limits::default()).unwrap();
    let saved = deck.save().unwrap();
    let again = Deck::open(&saved, Limits::default()).unwrap();
    assert_eq!(again.parts().names().collect::<Vec<_>>(), deck.parts().names().collect::<Vec<_>>());
    for e in deck.parts().entries() {
        assert_eq!(again.parts().bytes(&e.name), Some(&e.bytes), "{}", e.name);
    }
}

#[test]
fn an_edit_makes_the_slide_stale_and_untouched_slides_stay_cached() {
    let mut deck = Deck::open(&deck_bytes("0008"), Limits::default()).unwrap();
    let a0 = deck.slide(0).unwrap();
    let a1 = deck.slide(1).unwrap();
    assert!(Arc::ptr_eq(&a0, &deck.slide(0).unwrap()), "an unchanged slide is served from the cache");
    // replacing a part the slide read (its layout) makes THAT slide stale (read set), whatever the bytes
    let layout = a0
        .read_set
        .0
        .iter()
        .map(|(n, _)| n.to_string())
        .find(|n| n.starts_with("ppt/slideLayouts/slideLayout") && n.ends_with(".xml"))
        .unwrap();
    let bytes = deck.parts().bytes(&layout).unwrap().clone();
    deck.set_part(&layout, bytes);
    let b0 = deck.slide(0).unwrap();
    assert!(!Arc::ptr_eq(&a0, &b0), "the slide read the edited layout: re-read");
    assert!(a0.shapes == b0.shapes && a0.bg == b0.bg, "the same bytes read the same slide");
    assert_ne!(a0.read_set, b0.read_set, "the read set carries the new generation");
    let _ = a1;
    // a part the slide LOOKED FOR and did not find also invalidates (a .rels added later)
    let slide = deck.presentation().unwrap().slides[0].target.clone().unwrap();
    let rels = format!("{}_rels/{}.rels", &slide[..=slide.rfind('/').unwrap()], &slide[slide.rfind('/').unwrap() + 1..]);
    assert!(a0.read_set.0.iter().any(|(n, _)| **n == *rels));
}

#[test]
fn slidenum_fields_show_the_current_number() {
    let mut seen = 0;
    for n in ["0000", "0001", "0002", "0003", "0004", "0005", "0006", "0007", "0008", "0009", "0010", "0011"] {
        let mut deck = Deck::open(&deck_bytes(n), Limits::default()).unwrap();
        let pres = deck.presentation().unwrap().clone();
        let pxml = String::from_utf8_lossy(deck.parts().bytes("ppt/presentation.xml").unwrap()).into_owned();
        let patched = pxml.replacen("<p:presentation ", "<p:presentation firstSlideNum=\"7\" ", 1);
        assert_ne!(pxml, patched);
        deck.set_part("ppt/presentation.xml", Arc::from(patched.as_bytes()));
        for i in 0..pres.slides.len() {
            let s = deck.slide(u32::try_from(i).unwrap()).unwrap();
            for sh in &s.shapes {
                for p in sh.tx_body.iter().flat_map(|t| &t.paragraphs) {
                    for r in &p.runs {
                        if r.extras.as_ref().and_then(|e| e.field.as_deref()) == Some("slidenum") {
                            assert_eq!(&*r.text, (7 + i).to_string(), "deck {n} slide {i}");
                            seen += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(seen > 0, "no slidenum field in the decks tried");
}

fn sfnt_with_os2(weight: u16, italic: bool) -> Vec<u8> {
    // header (12) + one table record (16) + an OS/2 table of 96 bytes at offset 28
    let mut d = vec![0, 1, 0, 0, 0, 1, 0, 16, 0, 0, 0, 0];
    d.extend_from_slice(b"OS/2");
    d.extend_from_slice(&[0, 0, 0, 0]);
    d.extend_from_slice(&28u32.to_be_bytes());
    d.extend_from_slice(&96u32.to_be_bytes());
    let mut os2 = vec![0u8; 96];
    os2[4..6].copy_from_slice(&weight.to_be_bytes());
    os2[62..64].copy_from_slice(&u16::from(italic).to_be_bytes());
    d.extend_from_slice(&os2);
    d
}

#[test]
fn embedded_fonts_sfnt_odttf_and_unknown_formats() {
    let plain = sfnt_with_os2(700, true);
    let key = "{01234567-89AB-CDEF-0123-456789ABCDEF}";
    // the odttf obfuscation XORs the first 32 bytes with the reversed GUID: applying it twice is the identity
    let obf = deobfuscate_font(&plain, key);
    assert_ne!(obf[..32], plain[..32]);
    assert_eq!(deobfuscate_font(&obf, key), plain);
    let pres = "<p:presentation xmlns:p=\"p\" xmlns:r=\"r\"><p:sldSz cx=\"9\" cy=\"9\"/><p:embeddedFontLst>\
<p:embeddedFont><p:font typeface=\"Plain\"/><p:regular r:id=\"rId1\"/><p:bold r:id=\"rId2\"/></p:embeddedFont>\
<p:embeddedFont><p:font typeface=\"Obf\"/><p:italic r:id=\"rId3\" fontKey=\"{01234567-89AB-CDEF-0123-456789ABCDEF}\"/></p:embeddedFont>\
<p:embeddedFont><p:font typeface=\"Eot\"/><p:regular r:id=\"rId4\"/></p:embeddedFont></p:embeddedFontLst></p:presentation>";
    let rels = "<Relationships><Relationship Id=\"rId1\" Target=\"fonts/a.fntdata\"/><Relationship Id=\"rId2\" Target=\"fonts/missing.fntdata\"/>\
<Relationship Id=\"rId3\" Target=\"fonts/b.fntdata\"/><Relationship Id=\"rId4\" Target=\"fonts/c.fntdata\"/></Relationships>";
    let parts: Vec<(Box<str>, Arc<[u8]>)> = vec![
        ("ppt/presentation.xml".into(), Arc::from(pres.as_bytes())),
        ("ppt/_rels/presentation.xml.rels".into(), Arc::from(rels.as_bytes())),
        ("ppt/fonts/a.fntdata".into(), Arc::from(plain.as_slice())),
        ("ppt/fonts/b.fntdata".into(), Arc::from(obf.as_slice())),
        ("ppt/fonts/c.fntdata".into(), Arc::from(&b"\x4c\x50\x00\x00 an EOT file"[..])),
    ];
    let deck = Deck::from_parts(parts, Limits::default()).unwrap();
    let fonts = deck.embedded_fonts();
    // the EOT part is skipped until the reader of frontier B5b; the missing part too
    assert_eq!(
        fonts.iter().map(|f| (&*f.family, f.variant)).collect::<Vec<_>>(),
        [("Plain", FontVariant::Regular), ("Obf", FontVariant::Italic)]
    );
    assert_eq!((fonts[0].weight, fonts[0].italic), (Some(700), Some(true)));
    assert_eq!(&*fonts[1].data, plain.as_slice());
}

#[test]
fn typefaces_left_literal_in_the_deck() {
    let parts: Vec<(Box<str>, Arc<[u8]>)> = vec![
        ("ppt/presentation.xml".into(), Arc::from(&b"<p:presentation/>"[..])),
        (
            "ppt/slides/slide1.xml".into(),
            Arc::from(
                &b"<a typeface=\"Zeta\"/><a typeface=\"+mn-lt\"/><a typeface=\"Alpha Beta\"/><a typeface=\"\"/><a typeface=\"Zeta\"/>"[..],
            ),
        ),
        ("ppt/slideMasters/slideMaster1.xml".into(), Arc::from("<a typeface=\"\u{e9}t\u{e9}\"/><a typeface=\"Beta\" x/>".as_bytes())),
        ("ppt/slides/sub/slide2.xml".into(), Arc::from(&b"<a typeface=\"Nested\"/>"[..])),
    ];
    let deck = Deck::from_parts(parts, Limits::default()).unwrap();
    assert_eq!(deck.deck_typefaces().iter().map(|s| &**s).collect::<Vec<_>>(), ["Alpha Beta", "Beta", "Zeta", "\u{e9}t\u{e9}"]);
}

fn shape(id: &str, f: impl FnOnce(&mut Shape)) -> Shape {
    let mut s = Shape { id: id.into(), ..Shape::default() };
    f(&mut s);
    s
}

fn body(paras: &[&str]) -> Box<TextBody> {
    Box::new(TextBody {
        paragraphs: paras
            .iter()
            .map(|t| Paragraph {
                runs: vec![Run { text: (*t).into(), extras: None::<Box<RunExtras>>, ..Run::default() }],
                ..Paragraph::default()
            })
            .collect(),
        ..TextBody::default()
    })
}

#[test]
fn background_and_text_unit_counts_for_the_time_model() {
    let shapes = vec![
        shape("1", |s| s.fill = Some(Box::new(Fill::of_kind(FillKind::Solid)))),
        shape("2", |s| s.fill = Some(Box::new(Fill::of_kind(FillKind::None)))),
        shape("3", |s| s.line_color = Some(uo_dml_render::CssColor::new("#000000"))),
        shape("4", |s| {
            s.line_color = Some(uo_dml_render::CssColor::new("#000000"));
            s.line_none = Some(true);
        }),
        shape("5", |s| {
            s.inherited = Some(true);
            s.fill = Some(Box::new(Fill::of_kind(FillKind::Solid)));
        }),
        shape("6", |s| s.tx_body = Some(body(&["Hello brave world", "Second  line", ""]))),
    ];
    assert_eq!([1, 2, 3, 4, 5, 9].map(|i| has_background(&shapes, &i.to_string())), [true, false, true, false, false, false]);
    let t = |spid: &str, p: Option<(f64, f64)>, c: Option<(f64, f64)>| AnimTarget {
        spid: spid.into(),
        p_range: p,
        char_range: c,
        ..AnimTarget::default()
    };
    assert_eq!(count_text_units(&shapes, None, "wd"), 1.0);
    assert_eq!(count_text_units(&shapes, Some(&t("6", None, None)), "el"), 1.0);
    assert_eq!(count_text_units(&shapes, Some(&t("6", None, None)), "wd"), 5.0);
    assert_eq!(count_text_units(&shapes, Some(&t("6", None, None)), "lt"), 25.0, "letters: spaces and breaks are not units");
    assert_eq!(count_text_units(&shapes, Some(&t("6", Some((1.0, 1.0)), None)), "wd"), 2.0);
    assert_eq!(count_text_units(&shapes, Some(&t("6", None, Some((0.0, 5.0)))), "lt"), 5.0);
    assert_eq!(count_text_units(&shapes, Some(&t("5", None, None)), "lt"), 1.0, "an inherited shape is not a target: at least 1");
    assert_eq!(ph_key(Some("ctrTitle"), Some("3")), "title#3");
    assert_eq!(ph_key(None, None), "body");
}
