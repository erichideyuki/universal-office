//! Frontier R4a (`read::props_map`): hand-checked cases of the props mapping and the section
//! reader, and the no-panic loops over random attribute values. The exact comparison with the
//! frozen TS is `parity_props_map.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::js::{JsNumber, JsObject, JsonWriter, Slot};
use uo_docx::model::Run;
use uo_docx::model::RunProps;
use uo_docx::model::props::{
    BorderSpec, ColorSpec, IndSpec, NumPr, PBdr, PPr, PPrChange, RFonts, RPr, RPrChange, RevisionInfo, SpacingSpec, TabSpec, UnderlineSpec,
};
use uo_docx::model::run::{NoteRef, RunExt};
use uo_docx::read::DOCX;
use uo_docx::read::props::{read_ppr, read_rpr};
use uo_docx::read::props_map::{edit_base_props, para_props_from_ppr, read_border_edge, read_sect_pr, rev_info, run_props_from_rpr};
use uo_opc::xml::{Document, parse};
use uo_testkit::generate::Rng;

fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

fn json<T: uo_docx::js::JsValue>(v: &T) -> String {
    JsonWriter::stringify(v, None).unwrap()
}

fn xml(s: &str) -> Document {
    parse(s.to_owned(), &DOCX).unwrap()
}

#[test]
fn run_props_defaults_and_simple_fields() {
    assert_eq!(
        json(&run_props_from_rpr(None)),
        r#"{"bold":false,"italic":false,"underline":false,"strike":false,"allCaps":false,"smallCaps":false}"#
    );
    let rpr = RPr {
        b: Some(true),
        i: Some(false),
        vanish: Some(true),
        w: Some(n(100.0)),
        position: Some(n(0.0)),
        kern: Some(n(24.0)),
        spacing: Some(n(-0.0)),
        sz: Some(n(f64::NAN)),
        caps: Some(true),
        vert_align: Some("baseline".into()),
        highlight: Some("none".into()),
        r_style: Some("Strong".into()),
        ..RPr::default()
    };
    let p = run_props_from_rpr(Some(&rpr));
    assert_eq!(p.bold, Some(true));
    assert_eq!(p.italic, Some(false));
    assert_eq!(p.hidden, Some(true));
    assert_eq!(p.scale_pct, None, "w = 100 is the default");
    assert_eq!(p.position_half_pt, None, "0 is falsy");
    assert_eq!(p.kern_half_pt, Some(n(24.0)));
    assert_eq!(p.char_spacing_twips, None, "-0 is 0");
    assert_eq!(p.vert_align, None);
    assert_eq!(p.highlight, None);
    assert_eq!(p.char_style.as_deref(), Some("Strong"));
    assert!(matches!(p.sz_half_pt, Some(JsNumber::Num(x)) if x.is_nan()), "NaN is stored");
    assert_eq!(p.all_caps, Some(true));
    assert_eq!(
        json(&p),
        r#"{"bold":true,"italic":false,"underline":false,"strike":false,"allCaps":true,"smallCaps":false,"hidden":true,"kernHalfPt":24,"szHalfPt":null,"charStyle":"Strong"}"#
    );
}

#[test]
fn run_props_underline_and_theme_colours() {
    let rpr = RPr {
        u: Some(Box::new(UnderlineSpec {
            val: "wave".into(),
            color: Some("auto".into()),
            theme_color: Some("accent1".into()),
            theme_tint: Some("99".into()),
            theme_shade: Some("".into()),
            ..UnderlineSpec::default()
        })),
        color: Some(Box::new(ColorSpec { val: Some("auto".into()), theme_color: Some("text1".into()), ..ColorSpec::default() })),
        ..RPr::default()
    };
    let p = run_props_from_rpr(Some(&rpr));
    assert_eq!(p.underline, Some(true));
    assert_eq!(p.underline_style.as_deref(), Some("wave"));
    assert_eq!(p.underline_color, None, "auto");
    assert_eq!(json(&p.underline_theme), r#"{"themeColor":"accent1","tint":"99"}"#);
    assert_eq!(json(&p.color_theme), r#"{"themeColor":"text1"}"#);
    assert_eq!(p.color, None);
    for (val, underline, style) in
        [("none", false, None), ("", false, None), ("single", true, None), ("x", true, None), ("dottedHeavy", true, Some("dottedHeavy"))]
    {
        let rpr = RPr { u: Some(Box::new(UnderlineSpec { val: val.into(), ..UnderlineSpec::default() })), ..RPr::default() };
        let p = run_props_from_rpr(Some(&rpr));
        assert_eq!((p.underline, p.underline_style.as_deref()), (Some(underline), style), "{val}");
    }
}

#[test]
fn run_props_font_ascii_can_be_an_explicit_undefined() {
    let rpr = RPr { r_fonts: Some(Box::new(RFonts { h_ansi: Some("X".into()), ..RFonts::default() })), ..RPr::default() };
    let p = run_props_from_rpr(Some(&rpr));
    assert!(matches!(RunProps::slot(&p, key("fontAscii")), Slot::Undefined));
    assert!(!json(&p).contains("fontAscii"));
    let rpr = RPr { r_fonts: Some(Box::new(RFonts { ascii: Some("Aptos".into()), ..RFonts::default() })), ..RPr::default() };
    assert!(matches!(RunProps::slot(&run_props_from_rpr(Some(&rpr)), key("fontAscii")), Slot::Val(_)));
}

fn key(name: &str) -> usize {
    usize::from(RunProps::key_index(name).unwrap())
}

fn change(author: &str, mv: Option<bool>, inner: RPr) -> Box<RPrChange> {
    Box::new(RPrChange {
        author: author.into(),
        date: Some(String::new().into()),
        r#move: mv,
        r_pr: Box::new(inner),
        ..RPrChange::default()
    })
}

#[test]
fn run_props_format_change_chain() {
    let inner = RPr { change: Some(change("c", Some(false), RPr { b: Some(true), ..RPr::default() })), i: Some(true), ..RPr::default() };
    let rpr = RPr { change: Some(change("b", Some(true), inner)), ..RPr::default() };
    let p = run_props_from_rpr(Some(&rpr));
    let fmt = p.fmt.as_deref().unwrap();
    assert_eq!((&*fmt.author, fmt.date.as_deref(), fmt.r#move), ("b", Some(""), Some(true)));
    let mid = fmt.old.fmt.as_deref().unwrap();
    assert_eq!((&*mid.author, mid.r#move), ("c", None), "move false is not copied");
    assert_eq!(fmt.old.italic, Some(true));
    assert_eq!(mid.old.bold, Some(true));
    assert!(mid.old.fmt.is_none());
}

#[test]
fn run_props_chain_is_iterative() {
    let depth = 2000;
    let handle = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            let mut rpr = RPr { b: Some(true), ..RPr::default() };
            let mut ppr = PPr { keep_next: Some(true), ..PPr::default() };
            for i in 0..depth {
                rpr = RPr { i: Some(true), change: Some(change(&i.to_string(), None, rpr)), ..RPr::default() };
                ppr = PPr {
                    keep_lines: Some(true),
                    change: Some(Box::new(PPrChange { author: i.to_string().into(), p_pr: Box::new(ppr), ..PPrChange::default() })),
                    ..PPr::default()
                };
            }
            let p = run_props_from_rpr(Some(&rpr));
            let mut d = 0;
            let mut cur = &p;
            while let Some(f) = cur.fmt.as_deref() {
                d += 1;
                cur = &f.old;
            }
            let q = para_props_from_ppr(Some(&ppr));
            let mut e = 0;
            let mut cur = &q;
            while let Some(f) = cur.pfmt.as_deref() {
                e += 1;
                cur = &f.old;
            }
            assert_eq!((d, e), (depth, depth));
            assert_eq!(cur.keep_next, Some(true));
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn rev_info_keeps_only_the_authorship() {
    let r = RevisionInfo {
        author: "a".into(),
        date: Some("d".into()),
        date_utc: Some("".into()),
        r#move: Some(false),
        ..RevisionInfo::default()
    };
    assert_eq!(json(&rev_info(&r)), r#"{"author":"a","date":"d","dateUtc":""}"#);
    let r = RevisionInfo { author: "a".into(), r#move: Some(true), ..RevisionInfo::default() };
    assert_eq!(json(&rev_info(&r)), r#"{"author":"a","move":true}"#);
}

fn run_with(link: Option<&str>, note: bool, sym: Option<&str>) -> Run {
    let props = Arc::new(RunProps { link: link.map(Into::into), ..RunProps::default() });
    let ext = (note || sym.is_some()).then(|| {
        Box::new(RunExt {
            note_ref: note.then(|| NoteRef { kind: "footnote".into(), id: n(1.0), ..NoteRef::default() }),
            sym_font: sym.map(Into::into),
            ..RunExt::default()
        })
    });
    Run { props, ext, ..Run::default() }
}

#[test]
fn edit_base_props_adds_what_the_reader_forces_in_that_order() {
    let rpr = RPr { sz: Some(n(22.0)), r_fonts: Some(Box::new(RFonts::default())), ..RPr::default() };
    let all = run_with(Some("https://x"), true, Some("Wingdings"));
    let p = edit_base_props(&all, Some(&rpr));
    assert_eq!(p.underline, Some(true));
    assert_eq!(p.color.as_deref(), Some("0563C1"));
    assert_eq!(p.vert_align.as_deref(), Some("superscript"));
    assert_eq!(p.font_ascii.as_deref(), Some("Wingdings"));
    assert!(matches!(RunProps::slot(&p, key("fontAscii")), Slot::Val(_)), "the undefined key now holds a value");
    assert_eq!(
        json(&p),
        r#"{"bold":false,"italic":false,"underline":true,"strike":false,"allCaps":false,"smallCaps":false,"szHalfPt":22,"fontAscii":"Wingdings","color":"0563C1","vertAlign":"superscript"}"#
    );
    // an existing colour or superscript stays; a hyperlink with an underline already on is untouched
    let rpr = RPr {
        u: Some(Box::new(UnderlineSpec { val: "single".into(), ..UnderlineSpec::default() })),
        color: Some(Box::new(ColorSpec { val: Some("FF0000".into()), ..ColorSpec::default() })),
        vert_align: Some("subscript".into()),
        ..RPr::default()
    };
    let p = edit_base_props(&all, Some(&rpr));
    assert_eq!((p.color.as_deref(), p.vert_align.as_deref()), (Some("FF0000"), Some("subscript")));
    let p = edit_base_props(&run_with(Some("https://x"), false, None), None);
    assert_eq!(p.color.as_deref(), Some("0563C1"));
    let p = edit_base_props(&run_with(Some("https://x"), false, None), Some(&rpr));
    assert_eq!(p.color.as_deref(), Some("FF0000"));
    let p = edit_base_props(&run_with(Some(""), false, Some("")), None);
    assert_eq!(json(&p), json(&run_props_from_rpr(None)), "empty strings are falsy");
}

#[test]
fn para_props_alignment_numbering_indent_spacing() {
    for (jc, want) in [
        ("center", Some("center")),
        ("start", Some("left")),
        ("end", Some("right")),
        ("distribute", Some("both")),
        ("both", Some("both")),
        ("x", None),
        ("", None),
    ] {
        let ppr = PPr { jc: Some(jc.into()), ..PPr::default() };
        assert_eq!(para_props_from_ppr(Some(&ppr)).alignment.as_deref(), want, "{jc}");
    }
    let ppr = PPr {
        num_pr: Some(Box::new(NumPr { num_id: Some(n(1002.0)), ilvl: None, ..NumPr::default() })),
        ind: Some(Box::new(IndSpec { left: Some(n(10.0)), first_line: Some(n(5.0)), hanging: Some(n(0.0)), ..IndSpec::default() })),
        spacing: Some(Box::new(SpacingSpec {
            line: Some(n(240.0)),
            line_rule: Some("exact".into()),
            after: Some(n(1.0)),
            before: Some(n(2.0)),
            before_autospacing: Some(false),
            ..SpacingSpec::default()
        })),
        ..PPr::default()
    };
    let p = para_props_from_ppr(Some(&ppr));
    assert_eq!(json(&p.numbering), r#"{"kind":"decimal","ilvl":0,"numId":1002}"#);
    assert_eq!(p.indent_left_twips, Some(n(10.0)));
    assert!(matches!(p.indent_first_line_twips, Some(JsNumber::Num(x)) if x == 0.0 && x.is_sign_negative()), "hanging 0 is -0");
    assert_eq!(p.line_twentieths, None, "only the auto rule");
    assert_eq!(
        json(&p),
        r#"{"numbering":{"kind":"decimal","ilvl":0,"numId":1002},"indentLeftTwips":10,"indentFirstLineTwips":0,"spaceAfterTwips":1,"spaceBeforeTwips":2,"spaceBeforeAuto":false}"#
    );
    let ppr = PPr { num_pr: Some(Box::new(NumPr::default())), ..PPr::default() };
    assert_eq!(json(&para_props_from_ppr(Some(&ppr)).numbering), r#"{"kind":"bullet","ilvl":0,"numId":0}"#);
    let ppr = PPr {
        num_pr: Some(Box::new(NumPr { num_id: Some(JsNumber::Null), ilvl: Some(JsNumber::Null), ..NumPr::default() })),
        ..PPr::default()
    };
    assert_eq!(json(&para_props_from_ppr(Some(&ppr)).numbering), r#"{"kind":"bullet","ilvl":0,"numId":0}"#, "null is nullish");
}

fn border(val: &str, sz: Option<f64>, color: Option<&str>, space: Option<f64>) -> Option<Box<BorderSpec>> {
    Some(Box::new(BorderSpec {
        val: val.into(),
        sz: sz.map(n),
        color: color.map(Into::into),
        space: space.map(n),
        ..BorderSpec::default()
    }))
}

#[test]
fn para_props_borders_tabs_and_flags() {
    let ppr = PPr {
        p_bdr: Some(Box::new(PBdr {
            top: border("single", Some(8.0), Some("FF0000"), None),
            left: border("nil", None, None, None),
            bottom: border("double", None, Some("auto"), Some(4.0)),
            bar: border("none", None, None, None),
            ..PBdr::default()
        })),
        tabs: Some(vec![
            TabSpec { val: "left".into(), pos: n(720.0), leader: Some("dot".into()), ..TabSpec::default() },
            TabSpec { val: "clear".into(), pos: n(1.0), leader: Some("none".into()), ..TabSpec::default() },
        ]),
        shd: Some(Box::new(uo_docx::model::props::ShadingSpec { fill: Some("auto".into()), ..Default::default() })),
        textbox_tight_wrap: Some("allLines".into()),
        outline_lvl: Some(n(9.0)),
        frame_pr: Some(Box::new(uo_docx::model::props::FramePr { drop_cap: Some("drop".into()), ..Default::default() })),
        mark_ins: Some(Box::new(RevisionInfo { author: "a".into(), r#move: Some(true), ..RevisionInfo::default() })),
        keep_next: Some(false),
        ..PPr::default()
    };
    let p = para_props_from_ppr(Some(&ppr));
    assert_eq!(
        json(&p),
        concat!(
            r#"{"tabStops":[{"posTwips":720,"align":"left","leader":"dot"},{"posTwips":1,"align":"clear"}],"boxBorder":true,"#,
            r#""borders":{"top":{"style":"single","sizeEighthPt":8,"color":"FF0000","spacePt":0},"bottom":{"style":"double","sizeEighthPt":4,"spacePt":4}},"#,
            r#""keepNext":false,"textboxTightWrap":"allLines","outlineLvl":9,"dropCap":true,"markIns":{"author":"a","move":true}}"#
        )
    );
    let ppr = PPr {
        p_bdr: Some(Box::new(PBdr { left: border("nil", None, None, None), ..PBdr::default() })),
        tabs: Some(vec![]),
        ..PPr::default()
    };
    let p = para_props_from_ppr(Some(&ppr));
    assert_eq!(json(&p), r#"{"boxBorder":true}"#, "no edge left: no borders key; empty tabs: no tabStops");
    let ppr = PPr {
        textbox_tight_wrap: Some("x".into()),
        frame_pr: Some(Box::new(uo_docx::model::props::FramePr { drop_cap: Some("none".into()), ..Default::default() })),
        ..PPr::default()
    };
    assert_eq!(json(&para_props_from_ppr(Some(&ppr))), "{}");
}

#[test]
fn border_edge_of_an_element() {
    let doc = xml(r#"<w:top w:val="single" w:sz="12" w:color="auto"/>"#);
    let e = read_border_edge(Some(doc.root())).unwrap();
    assert_eq!(json(&e), r#"{"style":"single","sizeEighthPt":12,"spacePt":0}"#);
    assert!(matches!(RunPropsLess::color(&e), Slot::Undefined), "auto is an explicit undefined");
    for (x, want_none) in [
        (r#"<w:top w:val="none"/>"#, true),
        (r#"<w:top w:val="nil"/>"#, true),
        (r#"<w:top/>"#, true),
        (r#"<w:top w:val=""/>"#, true),
        (r#"<w:top w:val="x" w:sz="abc" w:space=" 3 "/>"#, false),
    ] {
        let d = xml(x);
        let e = read_border_edge(Some(d.root()));
        assert_eq!(e.is_none(), want_none, "{x}");
        if let Some(e) = e {
            assert!(matches!(e.size_eighth_pt, JsNumber::Num(v) if v.is_nan()));
            assert_eq!(e.space_pt, n(3.0));
        }
    }
    assert!(read_border_edge(None).is_none());
}

struct RunPropsLess;

impl RunPropsLess {
    fn color(e: &uo_docx::model::para::BorderEdge) -> Slot<'_> {
        e.slot(usize::from(<uo_docx::model::para::BorderEdge as JsObject>::key_index("color").unwrap()))
    }
}

#[test]
fn sect_pr_defaults_and_key_order() {
    let p = read_sect_pr(None);
    assert_eq!(
        json(&p),
        r#"{"pageWidthTwips":11906,"pageHeightTwips":16838,"marginLeftTwips":1701,"marginRightTwips":1701,"marginTopTwips":1417,"marginBottomTwips":1417,"headerDistanceTwips":720,"footerDistanceTwips":720}"#
    );
    let doc = xml(concat!(
        r#"<w:sectPr><w:vAlign w:val="center"/><w:titlePg/><w:pgSz w:w="12pt" w:h="x"/><w:type w:val="continuous"/>"#,
        r#"<w:cols w:num="3" w:space=" 100 " w:sep="1" w:equalWidth="false"><w:col w:w="10"/><w:col w:w="abc" w:space="5"/><w:col/><w:col w:w="9"/></w:cols>"#,
        r#"<w:pgNumType w:start="7" w:fmt="lowerRoman" w:chapStyle="2" w:chapSep="colon"/>"#,
        r#"<w:footnotePr><w:pos w:val="beneathText"/><w:numStart w:val="3"/></w:footnotePr><w:endnotePr/>"#,
        r#"<w15:footnoteColumns w:val="2"/><w:lnNumType w:countBy="1" w:distance="x" w:restart="newSection"/></w:sectPr>"#
    ));
    let p = read_sect_pr(Some(doc.root()));
    assert!(matches!(p.page_width_twips, JsNumber::Num(x) if x == 12.0));
    assert!(matches!(p.page_height_twips, JsNumber::Num(x) if x.is_nan()));
    assert_eq!(
        json(&p),
        concat!(
            r#"{"pageWidthTwips":12,"pageHeightTwips":null,"marginLeftTwips":1701,"marginRightTwips":1701,"marginTopTwips":1417,"marginBottomTwips":1417,"#,
            r#""headerDistanceTwips":720,"footerDistanceTwips":720,"type":"continuous","#,
            r#""cols":{"count":3,"spaceTwips":100,"sep":true,"widths":[{"widthTwips":10,"spaceTwips":0},{"widthTwips":0,"spaceTwips":5},{"widthTwips":0,"spaceTwips":0}]},"#,
            r#""titlePg":true,"pgNumStart":7,"pgNumFmt":"lowerRoman","pgNumChapStyle":2,"pgNumChapSep":"colon","#,
            r#""footnotePr":{"pos":"beneathText","numStart":3},"footnoteColumns":2,"lnNumType":{"countBy":1,"restart":"newSection"},"vAlign":"center"}"#
        )
    );
}

#[test]
fn sect_pr_columns_and_borders_edge_cases() {
    let big = "9".repeat(400);
    let cases = [
        (r#"<w:sectPr><w:cols w:num="1"/></w:sectPr>"#, false),
        (r#"<w:sectPr><w:cols w:num="2" w:equalWidth="0"><w:col/></w:cols></w:sectPr>"#, true),
        (r#"<w:sectPr><w:cols w:num="abc"/></w:sectPr>"#, false),
    ];
    for (x, has_cols) in cases {
        let d = xml(x);
        let p = read_sect_pr(Some(d.root()));
        assert_eq!(p.cols.is_some(), has_cols, "{x}");
        assert!(p.cols.is_none_or(|c| c.widths.is_none()), "{x}: too few w:col");
    }
    let d = xml(&format!(
        r#"<w:sectPr><w:cols w:num="{big}" w:equalWidth="0"><w:col/></w:cols><w:pgNumType w:start="{big}" w:chapStyle="{big}"/></w:sectPr>"#
    ));
    let p = read_sect_pr(Some(d.root()));
    let cols = p.cols.clone().unwrap();
    assert_eq!(cols.count, n(f64::INFINITY));
    assert!(cols.widths.is_none());
    assert_eq!(p.pg_num_start, Some(n(f64::INFINITY)));
    assert_eq!(p.pg_num_chap_style, Some(n(f64::INFINITY)));
    assert!(json(&p).contains(r#""count":null"#), "Infinity is null in JSON");
    let d = xml(r#"<w:sectPr><w:titlePg w:val="false"/><w:pgNumType w:start="x" w:chapStyle="0" w:chapSep="period"/></w:sectPr>"#);
    let p = read_sect_pr(Some(d.root()));
    assert_eq!((p.title_pg, p.pg_num_start, p.pg_num_chap_style, p.pg_num_chap_sep), (None, None, None, None));
    let d = xml(
        r#"<w:sectPr><w:pgBorders w:offsetFrom="page" w:display="x" w:zOrder="back"><w:left w:val="single" w:sz="4"/><w:top/></w:pgBorders></w:sectPr>"#,
    );
    let p = read_sect_pr(Some(d.root()));
    assert_eq!(
        json(&p).split_once(r#""pgBorders":"#).unwrap().1,
        r#"{"offsetFrom":"page","zOrder":"back","top":{"val":"single"},"left":{"val":"single","sz":4}}}"#
    );
}

const ATTRS: &[&str] = &[
    "0",
    "1",
    "-1",
    "-0",
    "12",
    "1002",
    "12pt",
    "1e3",
    " 12 ",
    "0x10",
    "NaN",
    "Infinity",
    "",
    "abc",
    "1.5",
    "+7",
    "true",
    "false",
    "on",
    "off",
    "auto",
    "none",
    "nil",
    "single",
    "x",
    "\u{1F600}",
    "é",
    "99999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999",
];

fn random_part(rng: &mut Rng) -> String {
    let v = |rng: &mut Rng| ATTRS[rng.below(ATTRS.len())].to_owned();
    let mut kids = String::new();
    let rpr_tags = [
        "w:rStyle",
        "w:b",
        "w:i",
        "w:u",
        "w:color",
        "w:vanish",
        "w:w",
        "w:position",
        "w:kern",
        "w:strike",
        "w:dstrike",
        "w:caps",
        "w:vertAlign",
        "w:sz",
        "w:highlight",
        "w:spacing",
        "w:rFonts",
    ];
    let ppr_tags = [
        "w:jc",
        "w:pStyle",
        "w:numPr",
        "w:ind",
        "w:spacing",
        "w:shd",
        "w:tabs",
        "w:pBdr",
        "w:keepNext",
        "w:textboxTightWrap",
        "w:outlineLvl",
        "w:framePr",
    ];
    let attrs = [
        "w:val",
        "w:color",
        "w:themeColor",
        "w:themeTint",
        "w:themeShade",
        "w:ascii",
        "w:left",
        "w:right",
        "w:hanging",
        "w:firstLine",
        "w:line",
        "w:lineRule",
        "w:before",
        "w:after",
        "w:fill",
        "w:pos",
        "w:leader",
        "w:sz",
        "w:space",
        "w:dropCap",
        "w:ilvl",
        "w:numId",
        "w:beforeAutospacing",
    ];
    let tag = |rng: &mut Rng, names: &[&str]| {
        let name = names[rng.below(names.len())];
        let mut s = format!("<{name}");
        for _ in 0..rng.below(5) {
            let a = attrs[rng.below(attrs.len())];
            if !s.contains(&format!(" {a}=")) {
                s.push_str(&format!(" {a}=\"{}\"", v(rng)));
            }
        }
        if name == "w:pBdr" || name == "w:tabs" || name == "w:numPr" {
            s.push_str(&format!("><w:top w:val=\"{}\"/><w:left w:sz=\"{}\"/><w:tab w:pos=\"{}\"/><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{}\"/><w:between/><w:bar w:val=\"single\"/></{name}>", v(rng), v(rng), v(rng), v(rng), v(rng)));
        } else {
            s.push_str("/>");
        }
        s
    };
    kids.push_str("<w:pPr>");
    for _ in 0..rng.below(8) {
        kids.push_str(&tag(rng, &ppr_tags));
    }
    kids.push_str("<w:pPrChange w:author=\"a\"><w:pPr>");
    for _ in 0..rng.below(3) {
        kids.push_str(&tag(rng, &ppr_tags));
    }
    kids.push_str("</w:pPr></w:pPrChange></w:pPr><w:r><w:rPr>");
    for _ in 0..rng.below(10) {
        kids.push_str(&tag(rng, &rpr_tags));
    }
    kids.push_str("<w:rPrChange w:author=\"a\"><w:rPr>");
    for _ in 0..rng.below(3) {
        kids.push_str(&tag(rng, &rpr_tags));
    }
    kids.push_str("</w:rPr></w:rPrChange></w:rPr></w:r>");
    let sect_tags = [
        "w:pgSz",
        "w:pgMar",
        "w:type",
        "w:cols",
        "w:titlePg",
        "w:pgBorders",
        "w:pgNumType",
        "w:footnotePr",
        "w15:footnoteColumns",
        "w:lnNumType",
        "w:vAlign",
    ];
    let sect_attrs = [
        "w:w",
        "w:h",
        "w:left",
        "w:top",
        "w:header",
        "w:num",
        "w:space",
        "w:sep",
        "w:equalWidth",
        "w:val",
        "w:start",
        "w:fmt",
        "w:chapStyle",
        "w:chapSep",
        "w:countBy",
        "w:distance",
        "w:restart",
        "w:offsetFrom",
        "w:display",
        "w:zOrder",
    ];
    kids.push_str("</w:p><w:sectPr>");
    for _ in 0..rng.below(8) {
        let name = sect_tags[rng.below(sect_tags.len())];
        kids.push_str(&format!("<{name}"));
        let mut seen = Vec::new();
        for _ in 0..rng.below(5) {
            let a = sect_attrs[rng.below(sect_attrs.len())];
            if !seen.contains(&a) {
                seen.push(a);
                kids.push_str(&format!(" {a}=\"{}\"", v(rng)));
            }
        }
        kids.push_str(&format!("><w:col w:w=\"{}\"/><w:col/><w:col w:space=\"{}\"/><w:top w:val=\"{}\"/><w:pos w:val=\"{}\"/><w:numStart w:val=\"{}\"/></{name}>", v(rng), v(rng), v(rng), v(rng), v(rng)));
    }
    kids.push_str("</w:sectPr>");
    format!("<w:body>{kids}</w:body>")
}

#[test]
fn no_panic_on_random_attribute_values() {
    let mut rng = Rng::new(0x4a4a_0002);
    let runs = [run_with(Some("l"), true, Some("s")), run_with(None, false, None)];
    let mut touched = 0usize;
    for _ in 0..3000 {
        let doc = xml(&random_part(&mut rng));
        for el in doc.root().descendants() {
            match el.name() {
                "w:rPr" => {
                    let rpr = read_rpr(Some(el));
                    let _ = json(&run_props_from_rpr(rpr.as_ref()));
                    for r in &runs {
                        let _ = json(&edit_base_props(r, rpr.as_ref()));
                    }
                    touched += 1;
                }
                "w:pPr" => {
                    let _ = json(&para_props_from_ppr(read_ppr(Some(el)).as_ref()));
                    touched += 1;
                }
                "w:sectPr" => {
                    let _ = json(&read_sect_pr(Some(el)));
                    touched += 1;
                }
                "w:top" | "w:left" | "w:bottom" | "w:right" | "w:between" | "w:bar" => {
                    let _ = read_border_edge(Some(el)).map(|e| json(&e));
                    touched += 1;
                }
                _ => {}
            }
        }
    }
    assert!(touched > 20_000, "the loop exercised {touched} elements");
}
