//! Memory gate M1 of frontier R4a: every public function of `read::props_map` returns the live
//! bytes exactly to their value before it, outputs dropped (`uo_alloc::assert_no_leak`), absent
//! input and the `change` chains included. The functions return values and have no error path.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_docx::js::JsNumber;
use uo_docx::model::Run;
use uo_docx::model::RunProps;
use uo_docx::model::props::{BorderSpec, ColorSpec, PBdr, PPr, PPrChange, RFonts, RPr, RPrChange, RevisionInfo, UnderlineSpec};
use uo_docx::model::run::{NoteRef, RunExt};
use uo_docx::read::DOCX;
use uo_docx::read::props::{read_ppr, read_rpr};
use uo_docx::read::props_map::{edit_base_props, para_props_from_ppr, read_border_edge, read_sect_pr, rev_info, run_props_from_rpr};
use uo_opc::xml::parse;

fn chain(depth: usize) -> (RPr, PPr) {
    let mut rpr = RPr {
        u: Some(Box::new(UnderlineSpec { val: "wave".into(), theme_color: Some("accent1".into()), ..UnderlineSpec::default() })),
        color: Some(Box::new(ColorSpec { val: Some("FF0000".into()), theme_color: Some("text1".into()), ..ColorSpec::default() })),
        r_fonts: Some(Box::new(RFonts::default())),
        ..RPr::default()
    };
    let mut ppr = PPr {
        p_bdr: Some(Box::new(PBdr {
            top: Some(Box::new(BorderSpec { val: "single".into(), ..BorderSpec::default() })),
            bar: Some(Box::new(BorderSpec { val: "double".into(), color: Some("auto".into()), ..BorderSpec::default() })),
            ..PBdr::default()
        })),
        mark_ins: Some(Box::new(RevisionInfo { author: "a".into(), ..RevisionInfo::default() })),
        ..PPr::default()
    };
    for i in 0..depth {
        rpr = RPr {
            change: Some(Box::new(RPrChange {
                author: i.to_string().into(),
                date: Some("d".into()),
                r_pr: Box::new(rpr),
                ..RPrChange::default()
            })),
            ..RPr::default()
        };
        ppr = PPr {
            change: Some(Box::new(PPrChange { author: i.to_string().into(), p_pr: Box::new(ppr), ..PPrChange::default() })),
            ..PPr::default()
        };
    }
    (rpr, ppr)
}

#[test]
fn m1_props_mapping() {
    let (rpr, ppr) = chain(50);
    let (flat_rpr, flat_ppr) = chain(0);
    assert_no_leak(|| black_box(run_props_from_rpr(None)));
    assert_no_leak(|| black_box(run_props_from_rpr(Some(&flat_rpr))));
    assert_no_leak(|| black_box(run_props_from_rpr(Some(&rpr))));
    assert_no_leak(|| black_box(para_props_from_ppr(None)));
    assert_no_leak(|| black_box(para_props_from_ppr(Some(&flat_ppr))));
    assert_no_leak(|| black_box(para_props_from_ppr(Some(&ppr))));
    let r = RevisionInfo { author: "a".into(), date: Some("d".into()), r#move: Some(true), ..RevisionInfo::default() };
    assert_no_leak(|| black_box(rev_info(&r)));
    let runs = [
        Run { props: Arc::new(RunProps { link: Some("https://x".into()), ..RunProps::default() }), ..Run::default() },
        Run {
            ext: Some(Box::new(RunExt {
                note_ref: Some(NoteRef { kind: "footnote".into(), id: JsNumber::Num(1.0), ..NoteRef::default() }),
                sym_font: Some("Wingdings".into()),
                ..RunExt::default()
            })),
            ..Run::default()
        },
        Run::default(),
    ];
    for run in &runs {
        assert_no_leak(|| black_box(edit_base_props(run, None)));
        assert_no_leak(|| black_box(edit_base_props(run, Some(&flat_rpr))));
        assert_no_leak(|| black_box(edit_base_props(run, Some(&rpr))));
    }
}

#[test]
fn m1_section_and_borders() {
    let part = parse(
        concat!(
            r#"<w:body><w:sectPr><w:pgSz w:w="1"/><w:type w:val="continuous"/><w:cols w:num="3" w:equalWidth="0"><w:col w:w="1"/><w:col/><w:col/></w:cols>"#,
            r#"<w:titlePg/><w:pgBorders w:offsetFrom="page"><w:top w:val="single"/></w:pgBorders><w:pgNumType w:start="2" w:fmt="x" w:chapStyle="1" w:chapSep="colon"/>"#,
            r#"<w:footnotePr><w:pos w:val="x"/></w:footnotePr><w:endnotePr><w:numFmt w:val="y"/></w:endnotePr><w15:footnoteColumns w:val="2"/>"#,
            r#"<w:lnNumType w:countBy="1"/><w:vAlign w:val="both"/></w:sectPr><w:sectPr/><w:top w:val="single" w:color="auto"/><w:left w:val="nil"/></w:body>"#
        )
        .to_owned(),
        &DOCX,
    )
    .unwrap();
    let kids: Vec<_> = part.root().child_elements().collect();
    for el in &kids {
        if el.name() == "w:sectPr" {
            assert_no_leak(|| black_box(read_sect_pr(Some(*el))));
        } else {
            assert_no_leak(|| black_box(read_border_edge(Some(*el))));
        }
    }
    assert_no_leak(|| black_box(read_sect_pr(None)));
    assert_no_leak(|| black_box(read_border_edge(None)));
}

#[test]
fn m1_mapping_over_a_parsed_part() {
    let part = parse(
        concat!(
            r#"<w:p><w:pPr><w:jc w:val="start"/><w:numPr><w:numId w:val="1002"/></w:numPr><w:pBdr><w:top w:val="single" w:sz="4"/></w:pBdr>"#,
            r#"<w:tabs><w:tab w:val="left" w:pos="720" w:leader="dot"/></w:tabs><w:pPrChange w:author="a"><w:pPr><w:keepNext/></w:pPr></w:pPrChange></w:pPr>"#,
            r#"<w:r><w:rPr><w:b/><w:u w:val="double"/><w:rFonts w:hAnsi="x"/><w:rPrChange w:author="a"><w:rPr><w:i/></w:rPr></w:rPrChange></w:rPr></w:r></w:p>"#
        )
        .to_owned(),
        &DOCX,
    )
    .unwrap();
    let ppr_el = part.root().descendants().find(|e| e.name() == "w:pPr").unwrap();
    let rpr_el = part.root().descendants().find(|e| e.name() == "w:rPr").unwrap();
    assert_no_leak(|| {
        let ppr = read_ppr(Some(ppr_el));
        let rpr = read_rpr(Some(rpr_el));
        black_box((para_props_from_ppr(ppr.as_ref()), run_props_from_rpr(rpr.as_ref())))
    });
}
