//! Slides wave 5 through the deck: the PowerPoint rules of the read model and the emission against PowerPoint's own output
//! of the synthetic decks in `tests/data/wave5/` (made by `core/target/w5-a/gen.py`, exported by PowerPoint 365 pt-BR),
//! and the TS behaviour under `SlidesRules::ts_strict` (record `docs/modules/uo-pptx.md` "PowerPoint rules (wave 5)").
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::Path;

use uo_dml_render::SlidesRules;
use uo_dml_render::datefield::{FieldClock, format_datetime_field};
use uo_pptx::deck::Deck;
use uo_pptx::limits::Limits;
use uo_pptx::pdf::PdfOptions;

#[path = "support/open_fonts.rs"]
mod open_fonts;
#[path = "support/pdf_view.rs"]
mod pdf_view;

fn data(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/wave5").join(name)).unwrap()
}

fn deck(name: &str, rules: SlidesRules) -> Deck {
    let mut d = Deck::open(&data(name), Limits::default()).unwrap();
    d.set_rules(rules);
    d
}

fn fills(d: &mut Deck) -> Vec<String> {
    let m = d.slide(0).unwrap();
    m.shapes.iter().filter_map(|s| s.fill.as_deref().and_then(|f| f.color.as_ref()).map(|c| c.as_str().to_ascii_uppercase())).collect()
}

#[test]
fn sys_clr_deck_reads_the_system_colours() {
    // deck sys-1: row 1 = sysClr with lastClr="123456" (scrollBar, background, activeCaption, …)
    let ppt = fills(&mut deck("sys-1.pptx", SlidesRules::default()));
    assert_eq!(&ppt[..4], ["#C8C8C8", "#000000", "#99B4D1", "#BFCDDB"]);
    assert_eq!(ppt[13], "#0078D7", "highlight");
    let ts = fills(&mut deck("sys-1.pptx", SlidesRules::ts_strict()));
    assert!(ts[..30].iter().all(|c| c == "#123456"), "the TS takes lastClr: {:?}", &ts[..4]);
}

fn field_texts(d: &mut Deck) -> Vec<(String, Option<String>, String)> {
    let m = d.slide(0).unwrap();
    let mut out = Vec::new();
    for sh in &m.shapes {
        for p in sh.tx_body.iter().flat_map(|t| t.paragraphs.iter()) {
            for r in &p.runs {
                if let Some(f) = r.x().field.as_deref() {
                    out.push((f.to_owned(), r.lang.as_deref().map(str::to_owned), r.text.to_string()));
                }
            }
        }
    }
    out
}

#[test]
fn date_fields_show_the_clock_in_the_field_language() {
    let clock = FieldClock { year: 2026, month: 11, day: 18, hour: 6, minute: 3, second: 55, default_lang: "pt-BR".into() };
    let mut d = deck("date-1.pptx", SlidesRules::default());
    // no clock: the cached text (deterministic by default)
    assert!(field_texts(&mut d).iter().all(|(_, _, t)| t.starts_with('Q')));
    d.set_field_clock(Some(clock.clone()));
    let got = field_texts(&mut d);
    assert_eq!(got.len(), 14 * 7);
    for (kind, lang, text) in &got {
        assert_eq!(text, &format_datetime_field(kind, lang.as_deref(), &clock).unwrap(), "{kind} {lang:?}");
    }
    // what PowerPoint wrote in that deck (date-1, the date-only types)
    let find = |k: &str, l: Option<&str>| got.iter().find(|(kk, ll, _)| kk == k && ll.as_deref() == l).unwrap().2.clone();
    assert_eq!(find("datetime1", Some("en-US")), "11/18/2026");
    assert_eq!(find("datetime1", Some("de-DE")), "18.11.2026");
    assert_eq!(find("datetime2", Some("fr-FR")), "mercredi 18 novembre 2026");
    assert_eq!(find("datetime3", Some("ja-JP")), "令和8年11月18日");
    assert_eq!(find("datetime4", None), "18 de novembro de 2026");
    // the TS: the cached text even with a clock
    let mut ts = deck("date-1.pptx", SlidesRules::ts_strict());
    ts.set_field_clock(Some(clock));
    assert!(field_texts(&mut ts).iter().all(|(_, _, t)| t.starts_with('Q')));
}

/// The fill colours (`r g b rg`) of every stream of a PDF, as 8-bit triples.
fn rg_colours(pdf: &[u8]) -> Vec<[i32; 3]> {
    let v = pdf_view::view(pdf).unwrap();
    let mut out = Vec::new();
    for o in &v.objs {
        let (_, data) = pdf_view::split(o);
        let Some(data) = data else { continue };
        let text: String = data.iter().map(|&c| char::from(c)).collect();
        let toks: Vec<&str> = text.split_ascii_whitespace().collect();
        for w in toks.windows(4) {
            let [r, g, b] = [w[0], w[1], w[2]].map(uo_jsnum::parse::to_number);
            if w[3] == "rg" && r.is_finite() && g.is_finite() && b.is_finite() {
                let byte = |v: f64| uo_jsnum::conv::to_int32(uo_jsnum::math::round(v * 255.0));
                out.push([byte(r), byte(g), byte(b)]);
            }
        }
    }
    out
}

fn hex(s: &str) -> [i32; 3] {
    let n = i32::from_str_radix(s, 16).unwrap();
    [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

#[test]
fn colour_effects_over_a_metafile_match_powerpoint() {
    // deck fx-emf: an EMF of 8 colour stripes under grayscale, duotone, brightness/contrast and colour change; PowerPoint draws
    // each exactly as the PNG of the same stripes with the effect (fx-png), measured pixels below
    let fonts = open_fonts::open_fonts();
    let mut d = deck("fx-emf.pptx", SlidesRules::default());
    let out = uo_pptx::pdf::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap();
    let got = rg_colours(&out.bytes);
    let expect = [
        // grayscale (Rec. 709 on sRGB): red, green, blue, yellow
        "363636", "B6B6B6", "121212", "ECECEC", // duotone 1F3864 → FFE699
        "4E5D6F", "BFB48A", "2F4468", "EED995", "576471", "8F8F7F", // brightness +30 %
        "FF4C4C", "4CFF4C", "4C4CFF", "8C8C8C", "CCCCCC", // contrast +40 % (the 25 % grey)
        "151515", // brightness −20 %, contrast −30 %
        "AD0000", "00AD00", "0000AD", "ADAD00", "272727", "545454", "ADADAD", // colour change blue → FF8000
        "FF8000",
    ];
    for e in expect {
        let want = hex(e);
        assert!(
            got.iter().any(|c| (0..3).all(|i| (c[i] - want[i]).abs() <= 1)),
            "#{e} not among the PDF's fill colours (effects over the metafile not applied?)"
        );
    }
    assert_eq!(out.report.missing.get("image:recipe-truncated"), None);
}

#[path = "support/stub_measure.rs"]
mod stub;

fn emission(d: &mut Deck) -> uo_dml_render::emit::Emission {
    use uo_dml_render::text::{MAX_ROUNDS, MeasureTable};
    let w = d.presentation().unwrap().slide_w;
    let view = uo_pptx::emit::View { scale: 1280.0 / w, ox: 0.0, oy: 0.0 };
    let mut table = MeasureTable::default();
    let mut res = uo_render::slides::SlidesResources::new();
    let mut caches = uo_dml_render::emit::EmitCaches::new();
    for _ in 0..=MAX_ROUNDS {
        match uo_pptx::emit::emit_slide(d, 0, view, &table, &mut res, &mut caches) {
            Ok(e) => return e,
            Err(uo_pptx::emit::EmitFail::Need(n)) => stub::answer(d, &mut table, &n.0),
            Err(e) => panic!("{e:?}"),
        }
    }
    panic!("rounds")
}

#[test]
fn table_cell_links_and_run_jumps_have_run_marks() {
    // PowerPoint links a table cell's run and marks run-level slide and show jumps (agent C's l3 case and run jumps)
    let mut d = deck("links.pptx", SlidesRules::default());
    let urls: Vec<String> = emission(&mut d).links.iter().map(|l| l.url.to_string()).collect();
    assert!(urls.iter().any(|u| u == "https://example.com/"), "{urls:?}");
    assert_eq!(urls.iter().filter(|u| *u == "https://example.com/").count(), 2, "the cell's run and the shape's run: {urls:?}");
    assert!(urls.iter().any(|u| u == "ppaction://hlinksldjump#ppt/slides/slide1.xml"), "{urls:?}");
    assert!(urls.iter().any(|u| u == "ppaction://hlinkshowjump?jump=nextslide"), "{urls:?}");
    // the TS: only the shape's http run
    let mut ts = deck("links.pptx", SlidesRules::ts_strict());
    let urls: Vec<String> = emission(&mut ts).links.iter().map(|l| l.url.to_string()).collect();
    assert_eq!(urls, ["https://example.com/"]);
}

#[test]
fn runs_carry_their_fallback_context_only_under_powerpoint_rules() {
    let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks/b3a/0002.pptx")).unwrap();
    let contexts = |rules: SlidesRules| -> Vec<(Option<String>, String)> {
        let mut d = Deck::open(&bytes, Limits::default()).unwrap();
        d.set_rules(rules);
        let m = d.slide(0).unwrap();
        m.shapes
            .iter()
            .flat_map(|s| s.tx_body.iter().flat_map(|t| t.paragraphs.iter()))
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| r.x().font_fallback.as_deref().map(|f| (f.lang.as_deref().map(str::to_owned), f.ea.to_string())))
            .collect()
    };
    let ppt = contexts(SlidesRules::default());
    for lang in ["ja-JP", "ko-KR", "ar-SA", "he-IL", "hi-IN"] {
        assert!(ppt.iter().any(|(l, _)| l.as_deref() == Some(lang)), "{lang}: {ppt:?}");
    }
    assert!(!ppt.iter().any(|(l, _)| l.as_deref() == Some("en-US")), "a Latin run without ea/cs needs no context");
    assert!(contexts(SlidesRules::ts_strict()).is_empty());
}

/// The RGB pixels of every 200 x 100 image XObject of a PDF.
fn images_200x100(pdf: &[u8]) -> Vec<Vec<u8>> {
    let v = pdf_view::view(pdf).unwrap();
    let mut out = Vec::new();
    for o in &v.objs {
        let (dict, data) = pdf_view::split(o);
        if dict.contains("/Subtype /Image")
            && dict.contains("/Width 200")
            && dict.contains("/Height 100")
            && dict.contains("/DeviceRGB")
            && let Some(d) = data.filter(|d| d.len() == 200 * 100 * 3)
        {
            out.push(d);
        }
    }
    out
}

#[test]
fn colour_effects_on_a_jpeg_match_powerpoint() {
    // deck fx-jpg: the stripes as a JPEG under none, grayscale, bi-level, duotone, brightness, colour change; PowerPoint's
    // pixels (its PDF) for the 8 stripes of each. The JPEG is decoded (uo-image `jpeg`), the effects applied, embedded RGBA.
    // (brightness alone keeps the JPEG and rides on its `/Decode`: not a decoded picture, not listed)
    let ppt: [[&str; 8]; 4] = [
        ["353535", "B6B6B6", "121212", "ECECEC", "000000", "404040", "808080", "FFFFFF"],
        ["000000", "FFFFFF", "000000", "FFFFFF", "000000", "000000", "FFFFFF", "FFFFFF"],
        ["4E5C6F", "BFB48A", "2F4468", "EED995", "1F3864", "576471", "8F8F7F", "FFE699"],
        ["FE0000", "00FF01", "FF8000", "FFFF00", "000000", "404040", "808080", "FFFFFF"],
    ];
    let fonts = open_fonts::open_fonts();
    let mut d = deck("fx-jpg.pptx", SlidesRules::default());
    let out = uo_pptx::pdf::pdf(&mut d, &PdfOptions::new(&fonts)).unwrap();
    assert_eq!(out.report.missing.get("image:jpeg-effect"), None, "{:?}", out.report.missing);
    let imgs = images_200x100(&out.bytes);
    let stripes = |px: &[u8]| -> Vec<[i32; 3]> {
        (0..8)
            .map(|i| {
                let (x, y) = ((i % 4) * 50 + 25, (i / 4) * 50 + 25);
                let o = (y * 200 + x) * 3;
                [i32::from(px[o]), i32::from(px[o + 1]), i32::from(px[o + 2])]
            })
            .collect()
    };
    for (k, want) in ppt.iter().enumerate() {
        let found = imgs.iter().any(|img| stripes(img).iter().zip(want).all(|(c, w)| (0..3).all(|j| (c[j] - hex(w)[j]).abs() <= 2)));
        assert!(found, "effect {k}: no embedded picture with PowerPoint's colours {want:?}");
    }
}
