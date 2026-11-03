//! Memory gate M1 of uo-docx-layout frontier F2 (`uo_docx::word`, record "Wave 1 — F2"): every
//! public operation returns the live bytes exactly to their value before it, outputs dropped
//! (`uo_alloc::assert_no_leak`). The per-call caches (`StyleResolver`) and the counters
//! (`NumberingState`) are plain values owned by their caller: created, used and dropped inside the
//! measured closure, and `StyleResolver::clear` gives the capacity back.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::sync::Arc;

use uo_alloc::assert_no_leak;

/// `assert_no_leak` with the M3 numbers printed (`--nocapture`).
fn nl<R>(f: impl FnOnce() -> R) {
    let u = assert_no_leak(f);
    eprintln!("M1 {} B allocated, {} B peak, 0 B left", u.allocated, u.peak_above_start);
}
use uo_docx::js::JsNumber;
use uo_docx::model::Document;
use uo_docx::model::Paragraph;
use uo_docx::model::formatting::DocFormatting;
use uo_docx::model::props::{PPr, RPr};
use uo_docx::model::run::{Run, RunProps};
use uo_docx::read::DOCX;
use uo_docx::read::parts::{read_numbering_text, read_settings, read_style_sheet_text, read_theme_text};
use uo_docx::read::props::{read_ppr, read_rpr};
use uo_docx::read::props_map::{para_props_from_ppr, run_props_from_rpr};
use uo_docx::text::Utf16;
use uo_docx::view::DocView;
use uo_docx::word::{self, LocalDateTime, NumberingState, RevisionDisplay, RevisionMode, StyleResolver, TextCap};
use uo_docx::{DocxLimits, js::JsRecord};
use uo_opc::xml::parse;

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#;

fn rpr(inner: &str) -> RPr {
    let d = parse(format!("<w:rPr {W}>{inner}</w:rPr>"), &DOCX).unwrap();
    read_rpr(Some(d.root())).unwrap()
}

fn ppr(inner: &str) -> PPr {
    let d = parse(format!("<w:pPr {W}>{inner}</w:pPr>"), &DOCX).unwrap();
    read_ppr(Some(d.root())).unwrap()
}

fn formatting() -> DocFormatting {
    let styles = format!(
        r#"<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/><w:lang w:val="pt-BR"/></w:rPr></w:rPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:styleId="Normal" w:default="1"><w:name w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr><w:outlineLvl w:val="0"/><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr><w:rPr><w:b/><w:caps/></w:rPr></w:style>
        <w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/><w:rPr><w:b/></w:rPr></w:style>
        <w:style w:type="table" w:styleId="Grid"><w:name w:val="Grid"/><w:tblPr><w:tblBorders><w:top w:val="single"/></w:tblBorders></w:tblPr><w:tblStylePr w:type="firstRow"><w:rPr><w:b/></w:rPr></w:tblStylePr></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<w:numbering {W}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="upperRoman"/><w:lvlText w:val="%1."/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
        <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%1.%2)"/></w:lvl></w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="1"><w:startOverride w:val="3"/></w:lvlOverride></w:num></w:numbering>"#
    );
    let theme = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="x"><a:accent1><a:srgbClr val="156082"/></a:accent1></a:clrScheme><a:fontScheme name="f"><a:majorFont><a:latin typeface="Aptos Display"/></a:majorFont><a:minorFont><a:latin typeface="Aptos"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;
    DocFormatting {
        styles: read_style_sheet_text(&styles).unwrap(),
        numbering: read_numbering_text(&numbering, &DocxLimits::default()).unwrap(),
        theme: read_theme_text(theme).unwrap(),
        settings: read_settings(None),
        font_table: JsRecord::new(),
        ..DocFormatting::default()
    }
}

fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[test]
fn merges_cascade_and_colours() {
    let a = rpr(
        r#"<w:rFonts w:ascii="A" w:hAnsiTheme="minorHAnsi"/><w:b/><w:lang w:val="pt-BR"/><w:color w:val="FF0000" w:themeColor="accent1" w:themeTint="99"/><w:shd w:val="pct25" w:color="000000" w:fill="FFFF00"/>"#,
    );
    let b = rpr(r#"<w:rFonts w:hAnsi="B"/><w:b w:val="0"/><w:lang w:eastAsia="ja-JP"/><w14:ligatures w14:val="standard"/>"#);
    let p = ppr(
        r#"<w:ind w:left="720" w:hanging="360"/><w:tabs><w:tab w:val="clear" w:pos="720"/><w:tab w:val="right" w:pos="9000"/></w:tabs><w:pBdr><w:top w:val="single"/></w:pBdr><w:rPr><w:i/></w:rPr>"#,
    );
    let q = ppr(r#"<w:ind w:firstLine="200"/><w:tabs><w:tab w:val="left" w:pos="100"/></w:tabs><w:framePr w:w="100"/>"#);
    let fmt = formatting();
    nl(|| black_box(word::merge_rpr(Some(&a), Some(&b))));
    nl(|| black_box(word::merge_rfonts(a.r_fonts.as_deref(), b.r_fonts.as_deref())));
    nl(|| black_box(word::merge_ppr(Some(&p), Some(&q))));
    nl(|| black_box(word::merge_ppr(None, Some(&p))));
    nl(|| black_box(word::cascade_rpr(Some(&a), &[Some(&b), None, Some(&a)], Some(&b))));
    nl(|| black_box(word::resolve_color(a.color.as_deref().map(word::ColorRef::of_color), &fmt.theme)));
    nl(|| black_box(word::shading_fill(a.shd.as_deref(), &fmt.theme)));
    nl(|| black_box(word::apply_tint_shade("156082", Some("33"), Some("BF"))));
    nl(|| black_box(word::font_of_slot(a.r_fonts.as_deref(), word::FontSlot::HAnsi, &fmt.theme)));
}

#[test]
fn style_resolver_and_direct_formatting() {
    let fmt = formatting();
    nl(|| {
        let mut sr = StyleResolver::new();
        for id in [Some("Heading1"), Some("Normal"), None, Some("Missing")] {
            black_box(sr.para_style(&fmt, id));
            black_box(sr.char_style(&fmt, id.or(Some("Strong"))));
            black_box(sr.table_style(&fmt, Some("Grid")));
            black_box(StyleResolver::para_chain(&fmt, id));
        }
        sr.clear();
        black_box(sr);
    });
    let r = rpr(r#"<w:b/><w:color w:val="FF0000"/><w:rFonts w:ascii="X"/>"#);
    let mut props = run_props_from_rpr(Some(&r));
    props.bold = Some(false);
    props.font_ascii = Some("Y".into());
    let run = Run { text: Utf16::from("x"), props: Arc::new(props), rpr: Some(Arc::new(r)), ..Run::default() };
    let bare = Run {
        text: Utf16::from("y"),
        props: Arc::new(RunProps { bold: Some(true), color: Some("00FF00".into()), ..RunProps::default() }),
        ..Run::default()
    };
    nl(|| black_box(word::direct_rpr(&run)));
    nl(|| black_box(word::direct_rpr(&bare)));
    let p = ppr(r#"<w:pStyle w:val="Heading1"/><w:jc w:val="center"/><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr>"#);
    let mut pp = para_props_from_ppr(Some(&p));
    pp.indent_first_line_twips = Some(JsNumber::Num(-200.0));
    nl(|| black_box(word::direct_ppr(Some(&p), &pp)));
    nl(|| {
        let mut sr = StyleResolver::new();
        black_box(word::resolve_ppr(&fmt, &mut sr, Some(&p), &pp, Some(&p)));
        black_box(word::inherited_ppr(&fmt, &mut sr, Some(&p), &pp));
        black_box(word::effective_ppr(&fmt, &mut sr, Some(&p), &pp));
    });
}

#[test]
fn numbering_dates_formulas() {
    let fmt = formatting();
    nl(|| {
        let mut ns = NumberingState::new("pt-BR", TextCap::default());
        for (n, l) in [(1.0, 0.0), (1.0, 1.0), (1.0, 1.0), (1.0, 0.0), (2.0, 0.0), (1.0, f64::NAN)] {
            black_box(ns.next(&fmt, JsNumber::Num(n), JsNumber::Num(l)));
        }
        let c = ns.clone();
        black_box(c.list_key(&fmt, JsNumber::Num(1.0)));
        black_box(ns);
    });
    let mut cap = TextCap::default();
    for f in [Some("upperRoman"), Some("lowerLetter"), Some("chicago"), Some("cardinalText"), Some("hex"), Some("ordinal"), None] {
        nl(|| black_box(word::format_number(1234.0, f, "en-US", &mut cap)));
    }
    let d = LocalDateTime::at(1_768_478_400_000.0, -10_800_000.0);
    nl(|| black_box(word::format_word_date(&d, &u("dddd, d 'de' MMMM 'de' yyyy HH:mm am/pm"), "pt-BR")));
    nl(|| black_box(word::date_field_value(&u(r#" DATE \@ "MMMM d" \* Caps "#), &d, "pt-BR", &mut cap)));
    let mut value = |_: &str, _: &[u16]| -> Option<Vec<u16>> { Some(u("12")) };
    nl(|| black_box(word::evaluate_field_formula(&u(r#"= { NUMPAGES } * (2 - 1,5) \# "0,00""#), &mut value, Some("pt-BR"), 256, &mut cap)));
    let deep = u(&format!("={}1{}", "(".repeat(1000), ")".repeat(1000)));
    let mut none = |_: &str, _: &[u16]| -> Option<Vec<u16>> { None };
    nl(|| black_box(word::evaluate_field_formula(&deep, &mut none, None, 256, &mut cap)));
    nl(|| black_box(word::format_picture(-1234.5, "#.##0,00;(#.##0,00)")));
}

#[test]
fn revisions_outline_hyphenation() {
    let order: Vec<Box<str>> = vec!["A".into(), "B".into()];
    let ins = RunProps {
        ins: Some(Box::new(uo_docx::model::props::RevisionInfo { author: "B".into(), ..Default::default() })),
        ..RunProps::default()
    };
    nl(|| {
        let rd = RevisionDisplay::new(RevisionMode::All, None, None, None, None, None, &order);
        black_box(rd.run(&ins));
        black_box(word::revision_ranges(&rd, [(&ins, 3usize)], &uo_docx::model::ParaProps::default(), &[2]));
        black_box(rd);
    });
    let fmt = formatting();
    let mut doc = Document { formatting: Arc::new(fmt), ..Document::default() };
    let p = Paragraph {
        runs: vec![Run { text: Utf16::from("Título\t um"), props: Arc::new(RunProps::default()), ..Run::default() }],
        props: Arc::new(uo_docx::model::ParaProps { style_id: Some("Heading1".into()), ..Default::default() }),
        ppr: Some(Arc::new(ppr(r#"<w:pStyle w:val="Heading1"/>"#))),
        ..Paragraph::default()
    };
    doc.body.push_paragraph(p);
    let view = DocView::new(&doc);
    nl(|| black_box(word::document_outline(view, None)));
    nl(|| black_box(word::list_item_values(view, Some("en-US"))));
    nl(|| black_box(word::numbered_items(view, None)));
    nl(|| black_box(word::outline_paragraphs(view, None)));
    nl(|| black_box(word::document_hyphenation_languages(view, "pt-BR", true)));
}
