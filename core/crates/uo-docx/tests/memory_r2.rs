//! Memory gate M1 of frontier R2: every public function of `read::parts`, `read::styles`,
//! `read::table_styles` and `read::style_names_es` returns the live bytes exactly to their value
//! before the call, outputs dropped, error paths included, on the synthetic inputs of
//! `js/quirk-parts.mjs` (`tests/parity/parts.inputs`) and on hand-made error inputs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::path::Path;

use uo_alloc::assert_no_leak;
use uo_docx::js::{JsRecord, Str};
use uo_docx::limits::DocxLimits;
use uo_docx::read::DOCX;
use uo_docx::read::ReadOptions;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::parts::{
    numbering_images, parse_text, part_by_rel_type, read_doc_formatting, read_font_table, read_font_table_text, read_note_props,
    read_numbering, read_numbering_text, read_settings, read_settings_text, read_style_sheet, read_style_sheet_text, read_theme,
    read_theme_text, target_ext, target_ext_group, vml_len_match, vml_len_pt,
};
use uo_docx::read::style_names_es::{builtin_style_name_es, family_replace, split_accent, table_style_name_es};
use uo_docx::read::styles::{read_style_para_props, read_style_run_props, read_styles, read_styles_text, read_styles_xml};
use uo_docx::read::table_styles::{
    attr_value, builtin_table_styles, local_style_id, materialize_style_xml, replace_marker, style_def_of, style_id_by_name, theme_hex,
};
use uo_opc::Package;
use uo_opc::xml::parse;

type Parts = Vec<(String, Vec<u8>)>;

fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn inputs() -> Vec<Parts> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/parts.inputs")).unwrap();
    let mut out: Vec<Parts> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if line.starts_with("@input ") {
            out.push(Vec::new());
        } else if let Some((path, data)) = line.split_once('\t') {
            let bytes = if data.starts_with("hex:") { vec![0x89, 0x50, 0x4e, 0x47] } else { unescape(data).into_bytes() };
            out.last_mut().unwrap().push((path.to_owned(), bytes));
        }
    }
    out
}

fn texts(parts: &Parts, name: &str) -> Option<String> {
    parts.iter().find(|(p, _)| p == name).and_then(|(_, b)| String::from_utf8(b.clone()).ok())
}

#[test]
fn m1_read_doc_formatting_and_styles_over_the_inputs() {
    let all = inputs();
    assert_eq!(all.len(), 300);
    let limits = uo_opc::Limits::DEFAULT;
    let options = uo_opc::package::OpenOptions::default();
    for parts in &all {
        assert_no_leak(|| {
            let pkg = Package::from_parts(parts.clone(), &limits, &options);
            let opts = ReadOptions::default();
            let mut ctx = ReadCtx::new(&pkg, &opts);
            let f = read_doc_formatting(&mut ctx);
            let s = read_styles(&ctx);
            let rels = pkg.part_text("word/_rels/document.xml.rels").ok().flatten().and_then(|t| parse(&*t, &DOCX).ok());
            let list = rels.map(|d| uo_opc::rels::from_document(&d)).unwrap_or_default();
            let images = numbering_images(&ctx, "word/numbering.xml");
            let p = part_by_rel_type(&ctx, &list, "styles", "word/styles.xml");
            black_box((f, s, images, p, ctx.media.len()))
        });
    }
}

#[test]
fn m1_readers_over_the_part_texts() {
    let limits = DocxLimits::DEFAULT;
    for parts in &inputs() {
        if let Some(t) = texts(parts, "word/styles.xml") {
            assert_no_leak(|| {
                let a = read_style_sheet_text(&t);
                let b = read_styles_text(&t, &limits);
                if let Ok(Some(d)) = parse_text(&t) {
                    drop(black_box((read_style_sheet(Some(&d)), read_styles_xml(Some(&d), &limits))));
                    for el in d.root().descendants() {
                        match el.name() {
                            "w:rPr" => drop(black_box(read_style_run_props(Some(el)))),
                            "w:pPr" => drop(black_box(read_style_para_props(Some(el)))),
                            _ => {}
                        }
                    }
                }
                black_box((a, b))
            });
        }
        if let Some(t) = texts(parts, "word/numbering.xml") {
            assert_no_leak(|| black_box(read_numbering_text(&t, &limits)));
        }
        if let Some(t) = texts(parts, "word/theme/theme1.xml") {
            assert_no_leak(|| black_box(read_theme_text(&t)));
        }
        if let Some(t) = texts(parts, "word/settings.xml") {
            assert_no_leak(|| {
                if let Ok(Some(d)) = parse_text(&t) {
                    for el in d.root().child_elements() {
                        black_box(read_note_props(Some(el)));
                    }
                }
                black_box(read_settings_text(&t))
            });
        }
        if let Some(t) = texts(parts, "word/fontTable.xml") {
            assert_no_leak(|| black_box(read_font_table_text(&t)));
        }
    }
}

#[test]
fn m1_error_and_empty_paths() {
    let limits = DocxLimits { max_numbering_levels: 1, max_based_on_chain: 1, ..DocxLimits::DEFAULT };
    let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let two_levels = format!("<w:numbering {w}><w:abstractNum w:abstractNumId=\"1\"><w:lvl/><w:lvl/></w:abstractNum></w:numbering>");
    let chain = format!(
        "<w:styles {w}><w:style w:type=\"paragraph\" w:styleId=\"A\"><w:basedOn w:val=\"B\"/></w:style><w:style w:type=\"paragraph\" w:styleId=\"B\"><w:basedOn w:val=\"C\"/></w:style><w:style w:type=\"paragraph\" w:styleId=\"C\"/></w:styles>"
    );
    assert_no_leak(|| black_box(read_numbering_text(&two_levels, &limits)));
    assert_no_leak(|| black_box(read_styles_text(&chain, &limits)));
    for bad in ["", "<w:styles", "<a/><b/>", "\u{0}", "<w:styles><w:style w:styleId=\"A\">"] {
        assert_no_leak(|| {
            black_box((
                read_style_sheet_text(bad),
                read_numbering_text(bad, &limits),
                read_theme_text(bad),
                read_settings_text(bad),
                read_font_table_text(bad),
            ))
        });
        assert_no_leak(|| black_box((read_styles_text(bad, &limits), style_def_of(bad))));
    }
    assert_no_leak(|| {
        black_box((
            read_style_sheet(None),
            read_numbering(None, &limits),
            read_theme(None),
            read_settings(None),
            read_font_table(None),
            read_styles_xml(None, &limits),
        ))
    });
    assert_no_leak(|| black_box((read_note_props(None), read_style_run_props(None), read_style_para_props(None))));
    let empty = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    assert_no_leak(|| {
        let opts = ReadOptions::default();
        let mut ctx = ReadCtx::new(&empty, &opts);
        black_box((read_doc_formatting(&mut ctx), read_styles(&ctx), numbering_images(&ctx, "word/numbering.xml")))
    });
}

#[test]
fn m1_strings_and_tables() {
    let colors: JsRecord<Str> = {
        let mut r = JsRecord::new();
        r.insert("accent1", "4472C4".into());
        r
    };
    assert_no_leak(|| {
        let mut out = Vec::new();
        for s in ["width:10pt;height:5in", " ; width : 3", "", "width:1.2.3mm", "x;width:5 pi"] {
            out.push((vml_len_pt(s, "width"), vml_len_match(s, "height").map(|m| m.0.len())));
        }
        for s in ["a.PNG", "a.", "", "a.tar.gz"] {
            black_box((target_ext(s), target_ext_group(s)));
        }
        black_box(out)
    });
    assert_no_leak(|| black_box(builtin_table_styles()));
    assert_no_leak(|| {
        let mut out = Vec::new();
        for (tint, shade) in [(None, None), (Some("80"), None), (None, Some("BF")), (Some("zz"), Some("1"))] {
            out.push(theme_hex(&colors, "accent1", tint, shade));
            out.push(theme_hex(&colors, "constructor", tint, shade));
        }
        black_box(out)
    });
    assert_no_leak(|| {
        let xml = r#"<w:top w:color="~" w:themeColor="accent1" w:themeShade="BF"/><w:shd w:fill="~" w:themeFill="accent1"/>"#;
        black_box((
            materialize_style_xml(xml, &colors),
            replace_marker(xml, "w:color=\"~\"", |r| r.to_string()),
            attr_value(xml, "w:themeColor"),
            local_style_id("Tabela com grade - Ênfase 1"),
        ))
    });
    assert_no_leak(|| {
        let d = parse(
            "<w:styles xmlns:w=\"x\"><w:style w:type=\"table\" w:styleId=\"T\"><w:name w:val=\"Table Grid\"/></w:style></w:styles>",
            &DOCX,
        )
        .unwrap();
        let f = uo_docx::model::formatting::DocFormatting { styles: read_style_sheet(Some(&d)), ..Default::default() };
        black_box((style_id_by_name(Some(&f), "table grid").map(String::from), style_id_by_name(None, "x").is_none()))
    });
    assert_no_leak(|| {
        let mut out = Vec::new();
        for n in ["Grid Table 4 Accent 1", "Normal Table", "x", "Table\nGrid", "constructor", ""] {
            out.push((table_style_name_es(n), split_accent(n).map(|m| m.0.len())));
        }
        for r in 0..30 {
            out.push((Some(family_replace(r, "Grid Table 4")), None));
        }
        black_box((out, builtin_style_name_es("heading 1")))
    });
}
