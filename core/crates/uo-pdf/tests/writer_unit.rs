//! Unit tests of the writer layer: the numbers and strings, the file frame, the streaming rule,
//! the content of every drawing call, the fonts, links, outline and the limits. The expected
//! texts are what the TS writes (they were checked against it by `writer_parity.rs`).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::sync::Arc;

use support::pdf_parse::{normalize, parse_pdf};
use uo_jsnum::format::to_radix_string;
use uo_pdf::{
    ChunkedSink, FontProgram, GlyphOptions, JpegImage, LimitKind, PathCmd, PdfDocument, PdfError, PdfLimits, PdfLink, PdfOutlineItem,
    PdfSink, PdfWriter, PlacedGlyph, ShadingGeom, SoftMask, Stroke, fmt, hex_color, safe_pdf_uri, utf16be_hex,
};

fn text(pdf: &[u8]) -> String {
    pdf.iter().map(|&b| char::from(b)).collect()
}

/// A program with no real font behind it: glyph = code point - 0x20 (0 below), advance 500 + gid.
struct Mock;

impl FontProgram for Mock {
    fn units_per_em(&self) -> f64 {
        1000.0
    }

    fn glyph_id(&self, cp: u32) -> u32 {
        if cp >= 0xf000 { cp - 0xf000 } else { cp.saturating_sub(0x20) }
    }

    fn advance_width(&self, gid: u32) -> f64 {
        f64::from(500 + gid)
    }
}

fn doc(compress: bool) -> PdfDocument<Vec<u8>> {
    PdfDocument::new(Vec::new(), compress)
}

const RED: [f64; 3] = [1.0, 0.0, 0.0];

// ---- numbers and strings ----------------------------------------------------------------------

#[test]
fn fmt_rounds_clamps_and_never_writes_an_exponent() {
    let cases: [(f64, &str); 18] = [
        (0.0, "0"),
        (-0.0, "0"),
        (f64::NAN, "0"),
        (f64::INFINITY, "1000000000"),
        (f64::NEG_INFINITY, "-1000000000"),
        (1e300, "1000000000"),
        (0.0005, "0.001"),
        (-0.0005, "0"),
        (0.0004, "0"),
        (1.0005, "1.001"),
        (2.5, "2.5"),
        (-2.5, "-2.5"),
        (1e-7, "0"),
        (123456.78912, "123456.789"),
        (0.1 + 0.2, "0.3"),
        (612.0, "612"),
        (595.276, "595.276"),
        (999_999_999.999_6, "1000000000"),
    ];
    for (n, want) in cases {
        assert_eq!(fmt(n), want, "fmt({n})");
    }
}

#[test]
fn hex_color_is_parse_int_over_the_pairs() {
    assert_eq!(hex_color("#FF8800"), [1.0, 136.0 / 255.0, 0.0]);
    assert_eq!(hex_color("ff8800"), [1.0, 136.0 / 255.0, 0.0]);
    let auto = hex_color("auto");
    assert_eq!(auto[0], 10.0 / 255.0);
    assert!(auto[1].is_nan() && auto[2].is_nan());
    let short = hex_color("#ff");
    assert_eq!(short[0], 1.0);
    assert!(short[1].is_nan());
    assert!(hex_color("").iter().all(|c| c.is_nan()));
    // the first # anywhere goes, whitespace and a sign are parseInt's, a non-digit ends it
    assert_eq!(hex_color("1#2030 4"), [18.0 / 255.0, 3.0 / 255.0, 0.0]);
}

#[test]
fn safe_pdf_uri_keeps_three_schemes_and_escapes() {
    assert_eq!(safe_pdf_uri(Some("https://a.b/c")).as_deref(), Some("https://a.b/c"));
    assert_eq!(safe_pdf_uri(Some("  MAILTO:x@y.z ")).as_deref(), Some("MAILTO:x@y.z"));
    assert_eq!(safe_pdf_uri(Some("HTTP://X")).as_deref(), Some("HTTP://X"));
    assert_eq!(safe_pdf_uri(Some("javascript:alert(1)")), None);
    assert_eq!(safe_pdf_uri(Some("file:///etc/passwd")), None);
    assert_eq!(safe_pdf_uri(Some("")), None);
    assert_eq!(safe_pdf_uri(None), None);
    assert_eq!(safe_pdf_uri(Some("http://a b\"<>\u{e9}(x)\\")).as_deref(), Some("http://a%20b%22%3C%3E%C3%A9\\(x\\)\\\\"));
    // JS white space: NBSP and BOM are trimmed, U+180E is not white space
    assert_eq!(safe_pdf_uri(Some("\u{a0}\u{feff}http://x")).as_deref(), Some("http://x"));
    assert_eq!(safe_pdf_uri(Some("\u{180e}http://x")), None);
    assert_eq!(safe_pdf_uri(Some("\u{17f}ttp://x")), None);
}

#[test]
fn utf16be_hex_writes_code_units() {
    assert_eq!(utf16be_hex("A\u{e9}"), "004100e9");
    assert_eq!(utf16be_hex("\u{1F600}"), "d83dde00");
    assert_eq!(utf16be_hex(""), "");
}

#[test]
fn gid_hex_is_to_string_16_padded_to_4() {
    for gid in (0..=0xffff_u32).chain([0x1_0000, 0xdead_beef, u32::MAX]) {
        let js = to_radix_string(f64::from(gid), 16).unwrap();
        assert_eq!(format!("{gid:04x}"), format!("{js:0>4}"), "{gid}");
    }
}

// ---- the file frame ---------------------------------------------------------------------------

#[test]
fn a_minimal_file_is_exactly_the_ts_file() {
    let mut w = PdfWriter::new(Vec::new(), true);
    let catalog = w.add_dict("<< /Type /Catalog >>").unwrap();
    let out = w.build(catalog).unwrap();
    let want = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<< /Type /Catalog >>\nendobj\nxref\n0 2\n0000000000 65535 f \n0000000015 00000 n \ntrailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n51\n%%EOF\n";
    assert_eq!(text(&out), text(want));
    let parsed = parse_pdf(&out).unwrap();
    assert_eq!((parsed.count, parsed.root, parsed.objs.len()), (2, 1, 1));
}

#[test]
fn a_document_with_no_page_is_a_valid_file() {
    let out = doc(true).build().unwrap();
    let t = text(&out);
    assert!(t.contains("/Type /Pages /Count 0 /Kids []"), "{t}");
    parse_pdf(&out).unwrap();
}

/// A sink that records how much it had when each write came.
struct Spy {
    bytes: Vec<u8>,
    writes: usize,
}

impl PdfSink for Spy {
    fn write(&mut self, bytes: &[u8]) -> Result<(), PdfError> {
        self.bytes.extend_from_slice(bytes);
        self.writes += 1;
        Ok(())
    }
}

#[test]
fn objects_leave_when_every_lower_number_is_written() {
    let mut w = PdfWriter::new(Spy { bytes: Vec::new(), writes: 0 }, false);
    let (a, b, c) = (w.alloc().unwrap(), w.alloc().unwrap(), w.alloc().unwrap());
    w.define_text(c, "<< /C 1 >>").unwrap();
    w.define_text(b, "<< /B 1 >>").unwrap();
    assert_eq!(w.bytes_written(), 0, "nothing is written while object 1 is undefined");
    assert!(w.pending_bytes() > 0);
    w.define_text(a, "<< /A 1 >>").unwrap();
    assert_eq!(w.pending_bytes(), 0);
    let written = w.bytes_written();
    assert!(written > 0);
    let spy = w.build(a).unwrap();
    let t = text(&spy.bytes);
    let (ia, ib, ic) = (t.find("1 0 obj").unwrap(), t.find("2 0 obj").unwrap(), t.find("3 0 obj").unwrap());
    assert!(ia < ib && ib < ic, "file order is number order");
    parse_pdf(&spy.bytes).unwrap();
}

#[test]
fn a_redefinition_of_a_waiting_object_replaces_it_and_a_written_one_is_an_error() {
    let mut w = PdfWriter::new(Vec::new(), false);
    let (a, b) = (w.alloc().unwrap(), w.alloc().unwrap());
    w.define_text(b, "<< /First 1 >>").unwrap();
    w.define_text(b, "<< /Second 1 >>").unwrap();
    w.define_text(a, "<< /A 1 >>").unwrap();
    assert_eq!(w.define_text(a, "<< >>"), Err(PdfError::Invalid("object defined twice")));
    assert_eq!(w.define_text(9, "<< >>"), Err(PdfError::Invalid("object defined before it was allocated")));
    let out = w.build(a).unwrap();
    let t = text(&out);
    assert!(t.contains("/Second") && !t.contains("/First"));
}

#[test]
fn build_refuses_an_object_that_was_never_defined() {
    let mut w = PdfWriter::new(Vec::new(), false);
    let a = w.alloc().unwrap();
    w.alloc().unwrap();
    w.define_text(a, "<< >>").unwrap();
    assert_eq!(w.build(a).err(), Some(PdfError::Invalid("an object was allocated but not defined")));
}

#[test]
fn streams_are_deflated_only_above_64_bytes_and_only_when_smaller() {
    let mut w = PdfWriter::new(Vec::new(), true);
    let short = w.add_stream("/A 1", &[b'x'; 64], true).unwrap();
    let long = w.add_stream("/B 1", &[b'x'; 65], true).unwrap();
    let noise = uo_testkit::generate::Rng::new(7).bytes(4000);
    let rand = w.add_stream("/C 1", &noise, true).unwrap();
    let flagged = w.add_stream("/D 1", &[b'x'; 500], false).unwrap();
    let out = w.build(short).unwrap();
    let t = text(&out);
    assert!(t.contains("<< /A 1 /Length 64 >>\nstream\n"), "64 bytes stay stored");
    assert!(t.contains("<< /B 1 /Filter /FlateDecode /Length "), "65 bytes of one letter deflate");
    assert!(t.contains("<< /C 1 /Length 4000 >>"), "noise stays stored: the deflated form is not smaller");
    assert!(t.contains("<< /D 1 /Length 500 >>"), "allow_compress false");
    let _ = (long, rand, flagged);
    normalize(&out).unwrap();
}

#[test]
fn the_stream_dictionary_is_trimmed_like_the_ts() {
    let mut w = PdfWriter::new(Vec::new(), false);
    let a = w.add_stream("", b"abc", true).unwrap();
    w.add_stream("  /X 1 \n", b"abc", true).unwrap();
    let t = text(&w.build(a).unwrap());
    assert!(t.contains("<<  /Length 3 >>\nstream\nabc\nendstream"), "an empty dictionary leaves both spaces");
    assert!(t.contains("<< /X 1 /Length 3 >>"));
}

#[test]
fn the_output_limit_is_an_error_not_an_allocation() {
    let limits = PdfLimits { max_output_bytes: 2000, ..PdfLimits::NATIVE };
    let mut w = PdfWriter::with_limits(Vec::new(), false, &limits);
    let a = w.alloc().unwrap();
    w.define(a, &[b'x'; 1000]).unwrap();
    let b = w.alloc().unwrap();
    assert_eq!(w.define(b, &[b'y'; 1500]), Err(PdfError::Limit(LimitKind::Output)));
    // a waiting object counts too
    let mut w = PdfWriter::with_limits(Vec::new(), false, &limits);
    let (_a, b) = (w.alloc().unwrap(), w.alloc().unwrap());
    assert_eq!(w.define(b, &[b'y'; 2500]), Err(PdfError::Limit(LimitKind::Output)));
}

#[test]
fn a_failing_sink_stops_the_document() {
    struct Broken;
    impl PdfSink for Broken {
        fn write(&mut self, _: &[u8]) -> Result<(), PdfError> {
            Err(PdfError::Sink)
        }
    }
    let mut w = PdfWriter::new(Broken, false);
    let a = w.alloc().unwrap();
    assert_eq!(w.define_text(a, "<< >>"), Err(PdfError::Sink));
}

#[test]
fn the_chunked_sink_hands_over_chunks_of_exactly_the_size() {
    let mut chunks: Vec<usize> = Vec::new();
    let mut d = PdfDocument::new(
        ChunkedSink::new(2048, |c: Vec<u8>| {
            chunks.push(c.len());
            Ok(())
        }),
        true,
    );
    for i in 0..40u32 {
        let px: Vec<u8> = (0..32 * 32 * 4u32).map(|k| (k.wrapping_mul(i + 7) >> 3) as u8).collect();
        d.embed_rgba(&px, 32, 32).unwrap();
    }
    let sink = d.build().unwrap();
    let _ = sink.finish().unwrap();
    assert!(chunks.len() > 10, "{chunks:?}");
    let (last, rest) = chunks.split_last().unwrap();
    assert!(rest.iter().all(|&n| n == 2048), "{chunks:?}");
    assert!(*last > 0);
}

// ---- page content -----------------------------------------------------------------------------

/// The page content of a one-page document (compress off): the text of the only content stream.
fn content_of(draw: impl FnOnce(&mut uo_pdf::PageWriter<'_, Vec<u8>>)) -> String {
    let mut d = doc(false);
    {
        let mut p = d.add_page(600.0, 800.0);
        draw(&mut p);
        p.finish().unwrap();
    }
    let out = d.build().unwrap();
    let t = text(&out);
    let at = t.find("<<  /Length ").unwrap();
    let start = t[at..].find("stream\n").unwrap() + at + 7;
    let end = t[start..].find("\nendstream").unwrap() + start;
    t[start..end].to_owned()
}

#[test]
fn drawing_calls_write_the_ts_operators() {
    let got = content_of(|p| {
        p.fill_rect(10.0, 20.0, 100.0, 30.0, &RED);
        p.line(0.0, 0.0, 50.0, 100.0, &[0.0, 0.5, 1.0], 2.0, &[]);
        p.line(0.0, 0.0, 50.0, 100.0, &[0.0, 0.0, 0.0], 1.0, &[3.0, 1.5]);
        p.push_clip_rect(5.0, 6.0, 7.0, 8.0);
        p.pop_clip();
        p.push_transform(1.0, 0.0, 0.0, 1.0, 10.0, 20.0);
        p.pop_clip();
    });
    let want = "1 0 0 rg\n10 750 100 30 re f\n\
                0 0.5 1 RG\n2 w\n[] 0 d\n0 800 m 50 700 l S\n\
                0 0 0 RG\n1 w\n[3 1.5] 0 d\n0 800 m 50 700 l S\n\
                q\n5 786 7 8 re W n\nQ\n\
                q\n1 0 0 1 10 20 cm\nQ\n";
    assert_eq!(got, want);
}

#[test]
fn paths_write_the_paint_operator_of_each_combination() {
    let cmds = [
        PathCmd::M { x: 1.0, y: 2.0 },
        PathCmd::L { x: 3.0, y: 4.0 },
        PathCmd::C { x1: 1.0, y1: 1.0, x2: 2.0, y2: 2.0, x: 3.0, y: 3.0 },
        PathCmd::Z,
    ];
    let stroke = Stroke { color: [0.0, 0.0, 1.0], width_pt: 1.5, dash: &[] };
    let got = content_of(|p| {
        p.path(&cmds, Some(&RED), None, false);
        p.path(&cmds, Some(&RED), None, true);
        p.path(&cmds, None, Some(&stroke), false);
        p.path(&cmds, Some(&RED), Some(&stroke), false);
        p.path(&cmds, Some(&RED), Some(&stroke), true);
        p.path(&[], Some(&RED), None, false);
        p.path(&cmds, None, None, false);
        p.push_clip_path(&cmds[..2], true);
    });
    let body = "1 798 m\n3 796 l\n1 799 2 798 3 797 c\nh\n";
    let want = format!(
        "1 0 0 rg\n{body}f\n1 0 0 rg\n{body}f*\n0 0 1 RG 1.5 w [] 0 d\n{body}S\n\
         1 0 0 rg\n0 0 1 RG 1.5 w [] 0 d\n{body}B\n1 0 0 rg\n0 0 1 RG 1.5 w [] 0 d\n{body}B*\nq\n1 798 m\n3 796 l\nW* n\n"
    );
    assert_eq!(got, want);
}

#[test]
fn glyphs_one_matrix_per_glyph_or_one_per_run() {
    let mut d = doc(false);
    let font = d.embed_font("Mock", Arc::from(b"not a font".as_slice()), Mock);
    {
        let mut p = d.add_page(600.0, 800.0);
        let place = |p: &mut uo_pdf::PageWriter<'_, Vec<u8>>, items: &[(u32, f64, f64)]| -> Vec<PlacedGlyph> {
            items
                .iter()
                .map(|&(cp, x, y)| PlacedGlyph { gid: p.encode(font, cp).unwrap().gid, code_point: cp, x_pt: x, baseline_from_top_pt: y })
                .collect()
        };
        let g = place(&mut p, &[(0x41, 10.0, 100.0), (0x42, 17.0, 100.0), (0x43, 30.0, 100.0), (0x44, 5.0, 100.0)]);
        p.glyphs(font, 12.0, &RED, &g, &GlyphOptions::default());
        p.glyphs(font, 12.0, &RED, &g, &GlyphOptions { runs: true, synth_bold: true, synth_italic: true, scale_x: Some(0.5) });
        p.glyphs(font, 12.0, &RED, &[], &GlyphOptions::default());
        p.finish().unwrap();
    }
    let t = text(&d.build().unwrap());
    let per_glyph = "BT\n/F0 12 Tf\n1 0 0 rg\n1 0 0 1 10 700 Tm <0021> Tj\n1 0 0 1 17 700 Tm <0022> Tj\n1 0 0 1 30 700 Tm <0023> Tj\n1 0 0 1 5 700 Tm <0024> Tj\nET\n";
    assert!(t.contains(per_glyph), "{t}");
    // widths 533, 534, 535 thousandths at 12 pt: the pen is at 6.396 pt after the first glyph, the second glyph
    // is wanted 7 pt (14 pt text space at scale 0.5) from the first: adjustment round((6.396 - 14) * 1000 / 12) = -634
    let runs = "BT\n/F0 12 Tf\n1 0 0 rg\n1 0 0 RG 0.343 w 2 Tr\n0.5 0 0.21 1 10 700 Tm [<0021>-634<0022>";
    assert!(t.contains(runs), "{t}");
    assert!(t.contains("] TJ\n0.5 0 0.21 1 5 700 Tm [<0024>] TJ\n0 Tr\nET\n"), "a smaller x starts a new run: {t}");
}

#[test]
fn text_runs_and_the_to_unicode_map_of_a_font() {
    let mut d = doc(false);
    let font = d.embed_font("My Font! (x)", Arc::from(b"not a font".as_slice()), Mock);
    let enc = d.font_mut(font).unwrap().encode(0x41);
    assert_eq!(enc.gid, 0x21);
    // above 0xFFFF the glyph is .notdef (Identity-H has two bytes per glyph); the first code point of a glyph stays
    assert_eq!(d.font_mut(font).unwrap().encode(0x1F600).gid, 0);
    d.font_mut(font).unwrap().encode(0x41);
    assert_eq!(d.font(font).unwrap().used_glyphs(), 2);
    {
        let p = d.add_page(100.0, 100.0);
        p.finish().unwrap();
    }
    let t = text(&d.build().unwrap());
    assert!(t.contains("/FontName /MyFontx "), "psName keeps [A-Za-z0-9+-]: {t}");
    assert!(t.contains("/DW 1000 /W [0 [500] 33 [533]] >>"), "{t}");
    assert!(t.contains("2 beginbfchar\n<0000> <d83dde00>\n<0021> <0041>\nendbfchar\n"), "{t}");
    assert!(t.contains("/Length1 10 "), "the bytes of a font that cannot be subset go whole: {t}");
}

#[test]
fn symbol_fonts_get_the_word_unicode_equivalents() {
    for (name, cp, want) in [
        ("Symbol", 0xf0b7, "2022"),
        ("ABCDEF+Symbol", 0xf0ae, "2192"),
        ("Symbol MT, Bold", 0xf02d, "f02d"),
        ("Symbol,MT", 0xf02d, "2212"),
        ("Symbol, MT-Italic", 0xf02d, "2212"),
        ("SymbolMT-Bold", 0xf0d3, "00a9"),
        ("Wingdings", 0xf0fc, "2713"),
        ("XYZABC+Wingdings-Bold", 0xf0a7, "25aa"),
        ("Symbol", 0xf0a0, "f0a0"),
        ("Wingdings 2", 0xf0fc, "f0fc"),
        ("Symbolic", 0xf0b7, "f0b7"),
        ("abcdef+Symbol", 0xf0b7, "f0b7"),
        ("SymbolMTx", 0xf0b7, "f0b7"),
        ("Carlito", 0xf0b7, "f0b7"),
        ("Symbol", 0x41, "0041"),
    ] {
        let mut d = doc(false);
        let font = d.embed_font(name, Arc::from(b"x".as_slice()), Mock);
        d.font_mut(font).unwrap().encode(cp);
        d.add_page(10.0, 10.0).finish().unwrap();
        let t = text(&d.build().unwrap());
        let gid = format!("{:04x}", Mock.glyph_id(cp));
        assert!(t.contains(&format!("<{gid}> <{want}>")), "{name} {cp:x}: want {want} in\n{t}");
    }
}

#[test]
fn a_code_point_past_the_unicode_range_is_the_ts_range_error() {
    let mut d = doc(false);
    let font = d.embed_font("F", Arc::from(b"x".as_slice()), Mock);
    d.font_mut(font).unwrap().encode(0x11_0000);
    d.add_page(10.0, 10.0).finish().unwrap();
    assert!(matches!(d.build(), Err(PdfError::Invalid(_))));
}

#[test]
fn opacity_states_are_named_in_first_use_order_and_shared_by_rounded_value() {
    let mut d = doc(false);
    {
        let mut p = d.add_page(100.0, 100.0);
        p.with_alpha(Some(0.5), None, |p| p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED));
        p.with_alpha(Some(0.5004), None, |p| p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED));
        p.with_alpha(None, Some(0.25), |p| p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED));
        p.with_alpha(Some(1.0), Some(2.0), |p| p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED));
        p.with_alpha(Some(-1.0), Some(f64::NAN), |p| p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED));
        p.finish().unwrap();
    }
    let t = text(&d.build().unwrap());
    assert!(t.contains("q\n/GS0 gs\n1 0 0 rg\n0 99 1 1 re f\nQ\nq\n/GS0 gs\n"), "{t}");
    assert!(t.contains("q\n/GS1 gs\n"), "{t}");
    assert!(t.contains("/GS2 gs\n1 0 0 rg\n0 99 1 1 re f\nQ\n"), "{t}");
    assert!(
        t.contains("/ExtGState << /GS0 << /Type /ExtGState /ca 0.5 /CA 1 >> /GS1 << /Type /ExtGState /ca 1 /CA 0.25 >> /GS2 << /Type /ExtGState /ca 0 /CA 0 >> >>"),
        "{t}"
    );
    assert_eq!(t.matches("/GS").count(), 4 + 3, "four uses in the content and three states in the resources: {t}");
}

#[test]
fn images_and_shadings_are_numbered_in_paint_order_and_listed_in_the_resources() {
    let mut d = doc(false);
    let px = [10u8, 20, 30, 255, 40, 50, 60, 128];
    let a = d.embed_rgba(&px, 2, 1).unwrap();
    let opaque = d.embed_rgba(&[1, 2, 3, 255], 1, 1).unwrap();
    let jpeg = d.embed_jpeg(&JpegImage { components: 4, adobe_inverted: true, ..JpegImage::new(&[0xff, 0xd8], 8, 8) }).unwrap();
    let masked = d
        .embed_jpeg(&JpegImage {
            decode: &[1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            smask: Some(SoftMask { alpha: &[7, 8], width: 2, height: 1 }),
            ..JpegImage::new(&[0xff], 1, 1)
        })
        .unwrap();
    let sh = d.add_shading(&ShadingGeom::Radial { x0: 5.0, y0: 6.0, r: 7.0 }, &[1, 2, 3, 4, 5, 6], 100.0).unwrap();
    let ax = d.add_shading(&ShadingGeom::Axial { x0: 0.0, y0: 0.0, x1: 10.0, y1: 20.0 }, &[1], 100.0).unwrap();
    assert_eq!((a.obj_num(), opaque.obj_num(), jpeg.obj_num(), masked.obj_num()), (2, 3, 4, 6));
    assert_eq!((sh.obj_num(), ax.obj_num()), (8, 10));
    {
        let mut p = d.add_page(100.0, 100.0);
        p.draw_image(a, 1.0, 2.0, 30.0, 40.0);
        p.draw_image_matrix(opaque, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
        p.draw_image_transformed(jpeg, 10.0, 10.0, 20.0, 10.0, 90.0, true, false);
        p.fill_shaded(&[PathCmd::M { x: 0.0, y: 0.0 }, PathCmd::Z], sh, false);
        p.finish().unwrap();
    }
    let t = text(&d.build().unwrap());
    assert!(
        t.contains("<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 2 >>"),
        "{t}"
    );
    assert!(t.contains("/ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 1 0 R /Length 6 >>"), "{t}");
    assert!(t.contains("/ColorSpace /DeviceCMYK /BitsPerComponent 8 /Filter /DCTDecode /Decode [1 0 1 0 1 0 1 0] /Length 2 >>"), "{t}");
    assert!(t.contains("/Filter /DCTDecode /Decode [1 0 1 0 1 0] /SMask 5 0 R /Length 1 >>"), "{t}");
    assert!(
        t.contains("<< /FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [2] /BitsPerSample 8 /Order 1 /Encode [0 1] /Length 6 >>"),
        "{t}"
    );
    assert!(t.contains("<< /ColorSpace /DeviceRGB /ShadingType 3 /Coords [5 94 0 5 94 7] /Extend [true true] /Function 7 0 R >>"), "{t}");
    assert!(t.contains("/ShadingType 2 /Coords [0 100 10 80] /Extend [true true] /Function 9 0 R >>"), "{t}");
    assert!(t.contains("/XObject << /Im0 2 0 R /Im1 3 0 R /Im2 4 0 R /Im3 6 0 R >>"), "{t}");
    assert!(t.contains("/Shading << /Sh0 8 0 R /Sh1 10 0 R >>"), "{t}");
    assert!(t.contains("q\n30 0 0 40 1 58 cm\n/Im0 Do\nQ\n"), "{t}");
    assert!(t.contains("q\n1 -2 -3 4 8 90 cm\n/Im1 Do\nQ\n"), "{t}");
}

#[test]
fn embed_rgba_refuses_dimensions_the_data_cannot_fill() {
    let mut d = doc(false);
    assert_eq!(d.embed_rgba(&[0; 8], 3, 1).err(), Some(PdfError::Invalid("image dimensions exceed the pixel data")));
    assert_eq!(d.embed_rgba(&[], u32::MAX, u32::MAX).err(), Some(PdfError::Invalid("image dimensions exceed the pixel data")));
    assert!(d.embed_rgba(&[], 0, 5).is_ok());
    assert!(d.embed_rgba(&[0; 12], 1, 1).is_ok(), "more data than pixels is fine: the rest is ignored");
}

#[test]
fn links_and_the_outline_are_written_with_their_destinations() {
    let mut d = doc(false);
    for _ in 0..2 {
        d.add_page(200.0, 300.0).finish().unwrap();
    }
    let mut p = d.add_page(200.0, 300.0);
    p.add_link(&PdfLink { x_pt: 10.0, y_top_pt: 20.0, w_pt: 30.0, h_pt: 5.0, uri: Some("https://x.y/(a)".into()), ..PdfLink::default() });
    p.add_link(&PdfLink {
        x_pt: 1.0,
        y_top_pt: 2.0,
        w_pt: 3.0,
        h_pt: 4.0,
        page_index: Some(1),
        dest_top_pt: Some(50.0),
        ..PdfLink::default()
    });
    p.add_link(&PdfLink { page_index: Some(99), ..PdfLink::default() });
    p.add_link(&PdfLink { uri: Some("javascript:alert(1)".into()), page_index: Some(0), ..PdfLink::default() });
    p.add_link(&PdfLink::default());
    drop(p);
    for (i, (title, level)) in [("A", 1u32), ("B", 2), ("C", 3), ("D", 2), ("   ", 1), ("E", 1)].into_iter().enumerate() {
        d.add_outline(PdfOutlineItem { title: title.into(), level, page_index: i as i64 - 1, top_pt: 12.5 });
    }
    let out = d.build().unwrap();
    let t = text(&out);
    parse_pdf(&out).unwrap();
    assert_eq!(t.matches("/Type /Annot").count(), 3, "javascript: and the empty link are dropped: {t}");
    assert!(t.contains("/Rect [10 275 40 280] /Border [0 0 0] /A << /S /URI /URI (https://x.y/\\(a\\)) >> >>"), "{t}");
    assert!(t.contains("/Rect [1 294 4 298] /Border [0 0 0] /Dest [4 0 R /XYZ 0 250 null] >>"), "{t}");
    assert!(t.contains("/Dest [5 0 R /XYZ 0 300 null] >>"), "a page past the end clamps to the last, a missing top is 0: {t}");
    assert!(t.contains("/Annots ["), "{t}");
    // the tree: A (B (C), D), E
    assert!(t.contains("/Type /Outlines /First "), "{t}");
    assert!(t.contains("/Title <FEFF0041> /Parent "), "{t}");
    assert!(t.contains("/Count -3 /Dest"), "A has three descendants: {t}");
    assert!(t.contains("/Count -1 /Dest"), "B has one: {t}");
    assert!(t.contains("/Count 5 >>"), "five entries: {t}");
    assert_eq!(t.matches("/Prev ").count(), 2, "D after B, E after A: {t}");
    assert_eq!(t.matches("/Next ").count(), 2, "{t}");
}

#[test]
fn a_page_that_goes_over_the_ops_limit_stops_and_the_build_fails() {
    let limits = PdfLimits { max_ops_per_page: 10, ..PdfLimits::NATIVE };
    let mut d = PdfDocument::with_limits(Vec::new(), false, limits);
    let mut p = d.add_page(100.0, 100.0);
    for _ in 0..100 {
        p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED);
    }
    assert_eq!(p.failure(), Some(PdfError::Limit(LimitKind::Ops)));
    assert_eq!(p.operations(), 11);
    assert!(p.content_len() < 11 * 40, "nothing is written after the limit");
    assert_eq!(p.finish(), Err(PdfError::Limit(LimitKind::Ops)));
    assert_eq!(d.build().err(), Some(PdfError::Limit(LimitKind::Ops)));
}

#[test]
fn glyphs_count_one_operation_each_and_the_content_is_capped() {
    let limits = PdfLimits { max_ops_per_page: 5, max_content_bytes: 100, ..PdfLimits::NATIVE };
    let mut d = PdfDocument::with_limits(Vec::new(), false, limits);
    let font = d.embed_font("F", Arc::from(b"x".as_slice()), Mock);
    let g = PlacedGlyph { gid: 1, code_point: 0x21, x_pt: 0.0, baseline_from_top_pt: 0.0 };
    let mut p = d.add_page(10.0, 10.0);
    p.glyphs(font, 12.0, &RED, &[g; 6], &GlyphOptions::default());
    assert_eq!(p.failure(), Some(PdfError::Limit(LimitKind::Ops)));
    drop(p);
    let limits = PdfLimits { max_content_bytes: 100, ..PdfLimits::NATIVE };
    let mut d = PdfDocument::with_limits(Vec::new(), false, limits);
    let mut p = d.add_page(10.0, 10.0);
    p.path(&[PathCmd::M { x: 1.0, y: 1.0 }; 30], Some(&RED), None, false);
    assert_eq!(p.failure(), Some(PdfError::Limit(LimitKind::Content)));
}

#[test]
fn a_page_dropped_without_finish_is_still_in_the_file() {
    let mut d = doc(false);
    {
        let mut p = d.add_page(10.0, 10.0);
        p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED);
    }
    let t = text(&d.build().unwrap());
    assert!(t.contains("/Count 1 "), "{t}");
}

#[test]
fn resources_made_while_a_page_is_open_take_numbers_in_paint_order() {
    let mut d = doc(false);
    let mut p = d.add_page(10.0, 10.0);
    let img = p.document().embed_rgba(&[1, 2, 3, 4], 1, 1).unwrap();
    p.draw_image(img, 0.0, 0.0, 1.0, 1.0);
    let img2 = p.document().embed_rgba(&[1, 2, 3, 255], 1, 1).unwrap();
    p.finish().unwrap();
    assert_eq!((img.obj_num(), img2.obj_num()), (2, 3), "the soft mask is object 1 and the first image 2");
    let out = d.build().unwrap();
    parse_pdf(&out).unwrap();
}

#[test]
fn compressed_page_contents_are_held_deflated_until_build() {
    let mut d = doc(true);
    let mut p = d.add_page(100.0, 100.0);
    for _ in 0..200 {
        p.fill_rect(1.0, 2.0, 3.0, 4.0, &RED);
    }
    p.finish().unwrap();
    let out = d.build().unwrap();
    let t = text(&out);
    assert!(t.contains("<<  /Filter /FlateDecode /Length "), "{t}");
    let n = normalize(&out).unwrap();
    assert!(n.deflated >= 1);
}
