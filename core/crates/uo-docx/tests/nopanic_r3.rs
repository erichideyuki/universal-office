//! No-panic loops of frontier R3: every public function of `read::drawing`, `read::vml_shape`,
//! `read::page_background` and `read::watermark` over the quirk corpus, seeded mutations of it (a
//! character replaced, removed, duplicated, a junk string or a surrogate-pair half inserted, a
//! tag cut), every element of every parse (so each function also sees elements it was not
//! written for), and random strings for the text functions. A panic fails the test; the result is
//! not looked at (the parity tests look at it) except for one invariant: a drawing that
//! `vml_textbox_to_drawing` returns parses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_docx::read::DOCX_POS;
use uo_docx::read::drawing::{
    anchor_child, ends_with_ascii_lower, parse_vml_style, read_cust_geom, read_sp_pr, read_wp_anchor, split_vml_len, srgb_of,
    val_int_group, vml_anchor, vml_color, vml_len_twips, vml_shape_style,
};
use uo_docx::read::page_background::read_page_background;
use uo_docx::read::vml_shape::{VmlLang, vml_single_textbox, vml_textbox_to_drawing};
use uo_docx::read::watermark::{
    attr, find_open_tag, find_shape, has_word_prefixed, is_watermark_xml, read_watermark_xml, strip_quotes, style_prop,
};
use uo_opc::xml::{Document, parse};

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

fn read_all(d: &Document) -> usize {
    let root = d.root();
    let mut n = 0;
    for el in std::iter::once(root).chain(root.descendants()) {
        n += usize::from(read_wp_anchor(Some(el)).is_some_and(|a| a.wrap.is_empty()));
        n += read_sp_pr(Some(el), el.first_child("wps:style")).geom.len();
        n += read_sp_pr(el.first_child("wps:spPr"), el.first_child("wps:style")).geom.len();
        n += read_cust_geom(el, 1_905_000.0, 952_500.0).len();
        n += read_cust_geom(el, f64::NAN, f64::INFINITY).len();
        n += read_cust_geom(el, -5.0, 0.0).len();
        n += vml_shape_style(el, "rect").geom.len();
        n += usize::from(vml_anchor(&parse_vml_style(el.attr("style")), el.first_child("w10:wrap")).is_some());
        n += usize::from(anchor_child(el, "wp:positionV").is_some());
        n += usize::from(srgb_of(Some(el)).is_some());
        n += usize::from(vml_single_textbox(el).is_some());
        n += usize::from(read_page_background(el, &mut |rid| if rid.len() % 2 == 0 { Some(Default::default()) } else { None }).is_some());
        let xml = el.source_slice().unwrap_or("");
        n += usize::from(is_watermark_xml(Some(xml)));
        n += usize::from(read_watermark_xml(xml, &|v| vml_color(v)).is_some());
        for lang in [VmlLang::Pt, VmlLang::En, VmlLang::Es] {
            if let Ok(Some(drawing)) = vml_textbox_to_drawing(el, 3.0, lang) {
                assert!(parse(&drawing, &DOCX_POS).is_ok(), "a returned drawing parses: {drawing}");
                n += drawing.len();
            }
        }
    }
    n
}

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(4) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(7) {
            0 => chars[at] = ['"', '<', '>', '/', '=', ';', ':', ' ', '\u{a0}', '\u{0}', '😀', 'é', '&', '\''][rng.below(14)],
            1 => {
                chars.remove(at);
            }
            2 => chars.insert(at, chars[at]),
            3 => {
                let junk =
                    ["#😀", "1e999", "--", "99999999999999999999", "-0", "NaN", "&#xD800;", "\u{feff}", "px", "%", "f", ".", ",", "\\"]
                        [rng.below(14)];
                for (k, c) in junk.chars().enumerate() {
                    chars.insert(at + k, c);
                }
            }
            4 => chars.truncate(at),
            5 => {
                let to = (at + 1 + rng.below(40)).min(chars.len());
                chars.drain(at..to);
            }
            _ => {
                let other = rng.below(chars.len());
                chars.swap(at, other);
            }
        }
    }
    chars.into_iter().collect()
}

fn quirk_texts() -> Vec<String> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/parity/drawing.quirks.txt")).unwrap();
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t").replace("\\\\", "\\"))
        .collect()
}

#[test]
fn quirk_corpus_does_not_panic() {
    let mut total = 0;
    for t in quirk_texts() {
        if let Ok(d) = parse(&t, &DOCX_POS) {
            total += read_all(&d);
        }
    }
    assert!(total > 0);
}

#[test]
fn mutated_corpus_does_not_panic() {
    let texts = quirk_texts();
    let mut rng = Rng(0x2545_f491);
    let (mut parsed, mut total) = (0, 0);
    for round in 0..6000 {
        let base = &texts[rng.below(texts.len())];
        let t = mutate(&mut rng, base);
        if let Ok(d) = parse(&t, &DOCX_POS) {
            parsed += 1;
            total += read_all(&d);
        }
        // text functions over the raw (possibly malformed) string
        let _ = (
            find_shape(&t),
            is_watermark_xml(Some(&t)),
            read_watermark_xml(&t, &|v| vml_color(v)),
            attr(&t, "style"),
            style_prop(&t, "width"),
        );
        let _ = (
            find_open_tag(&t, "v:shape"),
            has_word_prefixed(&t, "gain="),
            strip_quotes(&t),
            vml_color(Some(&t)),
            parse_vml_style(Some(&t)),
        );
        let _ = (vml_len_twips(Some(&t)), split_vml_len(&t), val_int_group(&t), ends_with_ascii_lower(&t));
        assert!(round < 6000);
    }
    assert!(parsed > 1000 && total > 0, "{parsed}");
}

fn random_string(rng: &mut Rng) -> String {
    let pieces = [
        "",
        " ",
        "\t",
        "\n",
        "\u{a0}",
        "\u{feff}",
        "-",
        "+",
        ".",
        "1",
        "12",
        "0",
        "e",
        "pt",
        "in",
        "px",
        "cm",
        "mm",
        "pc",
        "em",
        "#",
        "f",
        "%",
        "val",
        ",",
        ";",
        ":",
        "=",
        "\"",
        "'",
        "<",
        ">",
        "/",
        "&",
        "&quot;",
        "&amp;",
        "😀",
        "é",
        "ß",
        "id=\"",
        "PowerPlusWaterMarkObject",
        "WordPictureWatermark",
        "<v:shape",
        "</v:shape>",
        " style=\"",
        "width",
        "font-size",
        "rotation",
        "abc",
        "red",
        "silver",
        "\u{2028}",
        "\u{0}",
        "99999999999999999999",
        "1e999",
    ];
    let mut s = String::new();
    for _ in 0..rng.below(24) {
        s.push_str(pieces[rng.below(pieces.len())]);
    }
    s
}

#[test]
fn random_strings_do_not_panic() {
    let mut rng = Rng(0x9e37_79b9);
    for _ in 0..30_000 {
        let s = random_string(&mut rng);
        let _ = (vml_len_twips(Some(&s)), split_vml_len(&s), vml_color(Some(&s)), parse_vml_style(Some(&s)), val_int_group(&s));
        let _ =
            (find_shape(&s), is_watermark_xml(Some(&s)), read_watermark_xml(&s, &|v| vml_color(v)), attr(&s, "style"), attr(&s, "r:id"));
        let _ =
            (style_prop(&s, "width"), style_prop(&s, "font-family"), find_open_tag(&s, "v:textpath"), has_word_prefixed(&s, "blacklevel="));
        let _ = (strip_quotes(&s), ends_with_ascii_lower(&s));
    }
}

#[test]
fn hostile_numbers_in_attributes_do_not_panic() {
    let vals = [
        "0",
        "-0",
        "1e999",
        "-1e999",
        "NaN",
        "99999999999999999999",
        "-99999999999999999999",
        "4294967296",
        "-4294967297",
        "0x10",
        "1e-999",
        ".5",
        "5.",
        "",
    ];
    let mut rng = Rng(7);
    for _ in 0..3000 {
        let mut attrs = String::new();
        for name in [
            "distT",
            "relativeHeight",
            "behindDoc",
            "simplePos",
            "x",
            "y",
            "cx",
            "cy",
            "rot",
            "w",
            "ang",
            "pos",
            "idx",
            "dist",
            "dir",
            "blurRad",
            "rad",
            "sx",
            "kx",
            "val",
        ] {
            if rng.below(3) == 0 {
                attrs.push_str(&format!(" {name}=\"{}\"", vals[rng.below(vals.len())]));
            }
        }
        let xml = format!(
            "<r><wp:anchor{attrs}><wp:simplePos{attrs}/><wp:wrapTight><wp:wrapPolygon><wp:start{attrs}/><wp:lineTo{attrs}/><wp:lineTo{attrs}/><wp:lineTo{attrs}/></wp:wrapPolygon></wp:wrapTight><wp:effectExtent{attrs}/></wp:anchor>\
             <wps:spPr><a:xfrm{attrs}><a:ext{attrs}/></a:xfrm><a:custGeom><a:gdLst><a:gd name=\"g\" fmla=\"*/ w h 0\"/><a:gd name=\"h2\" fmla=\"at2 1e999 -1e999\"/></a:gdLst><a:pathLst><a:path{attrs}><a:moveTo><a:pt{attrs}/></a:moveTo><a:arcTo{attrs}/><a:lnTo/></a:path></a:pathLst></a:custGeom>\
             <a:gradFill{attrs}><a:gsLst><a:gs{attrs}><a:srgbClr val=\"ff\"><a:alpha{attrs}/></a:srgbClr></a:gs><a:gs{attrs}><a:srgbClr val=\"00\"/></a:gs></a:gsLst><a:lin{attrs}/><a:path{attrs}><a:fillToRect{attrs}/></a:path></a:gradFill>\
             <a:ln{attrs}><a:solidFill><a:srgbClr val=\"00\"/></a:solidFill></a:ln><a:effectLst><a:outerShdw{attrs}><a:srgbClr val=\"00\"/></a:outerShdw><a:reflection{attrs}/><a:glow{attrs}><a:srgbClr val=\"00\"/></a:glow></a:effectLst></wps:spPr></r>"
        );
        let d = parse(&xml, &DOCX_POS).unwrap();
        assert!(read_all(&d) > 0);
    }
}
