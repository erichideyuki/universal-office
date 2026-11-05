//! Memory gate (M1) of the D1b table property emitters (`write::emit::table`, frontier W3): every
//! public function returns the live bytes exactly, on its success paths and its error paths (a raw
//! element that does not parse, one over the limits).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_docx::js::{JsNumber, Str};
use uo_docx::model::para::BorderEdge;
use uo_docx::model::props::{BorderSpec, ShadingSpec, TblBorders, TblLook, TblPr, TcBorders, TcPr, TrHeight, TrPr, WidthSpec};
use uo_docx::model::section::PageBorders;
use uo_docx::model::table::{Cell, Row, Table, TableBorders};
use uo_docx::write::ctx::WriteCtx;
use uo_docx::write::emit::table as t;
use uo_docx::write::sig::SigMode;
use uo_opc::Limits;

fn cx() -> WriteCtx {
    WriteCtx::new(Limits::DEFAULT, SigMode::Canonical)
}

fn num(x: f64) -> Option<JsNumber> {
    Some(JsNumber::Num(x))
}

fn width(w: f64) -> WidthSpec {
    WidthSpec { w: JsNumber::Num(w), r#type: "dxa".into(), ..WidthSpec::default() }
}

fn border() -> Box<BorderSpec> {
    Box::new(BorderSpec { val: "single".into(), sz: num(4.0), color: Some(Str::from("FF0000")), ..BorderSpec::default() })
}

fn tbl_pr() -> TblPr {
    TblPr {
        tbl_style: Some("TableGrid".into()),
        tbl_w: Some(width(5000.0)),
        jc: Some("center".into()),
        tbl_borders: Some(Box::new(TblBorders { top: Some(border()), inside_v: Some(border()), ..TblBorders::default() })),
        shd: Some(Box::new(ShadingSpec { fill: Some("DDDDDD".into()), ..ShadingSpec::default() })),
        tbl_look: Some(Box::new(TblLook { first_row: Some(true), ..TblLook::default() })),
        ..TblPr::default()
    }
}

fn tr_pr() -> TrPr {
    TrPr {
        cant_split: Some(true),
        tr_height: Some(TrHeight { val: JsNumber::Num(300.0), h_rule: "exact".into(), ..TrHeight::default() }),
        ..TrPr::default()
    }
}

fn tc_pr() -> TcPr {
    TcPr {
        tc_w: Some(width(3000.0)),
        grid_span: num(2.0),
        v_merge: Some("restart".into()),
        tc_borders: Some(Box::new(TcBorders { top: Some(border()), ..TcBorders::default() })),
        ..TcPr::default()
    }
}

fn table() -> Table {
    let cell = |legacy: bool| Cell {
        grid_span: JsNumber::Num(2.0),
        width_twips: num(1500.0),
        shading_hex: Some("FFAA00".into()),
        borders: Some(Box::new(TableBorders {
            top: Some(BorderEdge { style: "single".into(), size_eighth_pt: JsNumber::Num(4.0), ..BorderEdge::default() }),
            ..TableBorders::default()
        })),
        tc_pr: (!legacy).then(|| Arc::new(tc_pr())),
        ..Cell::default()
    };
    Table {
        grid_cols_twips: vec![JsNumber::Num(1000.0), JsNumber::Num(2000.0)],
        borders: Some(Box::new(TableBorders {
            top: Some(BorderEdge { style: "double".into(), size_eighth_pt: JsNumber::Num(8.0), ..BorderEdge::default() }),
            ..TableBorders::default()
        })),
        rows: vec![Row {
            cells: vec![cell(true), cell(false)],
            tr_pr: Some(Arc::new(tr_pr())),
            tbl_pr_ex: Some(Arc::new(tbl_pr())),
            ..Row::default()
        }],
        ..Table::default()
    }
}

#[test]
fn m1_generators_return_the_memory() {
    let (p, r, c, tb) = (tbl_pr(), tr_pr(), tc_pr(), table());
    assert_no_leak(|| black_box(t::border_xml("top", &border())));
    assert_no_leak(|| {
        let pb = PageBorders { top: Some(*border()), ..PageBorders::default() };
        black_box(t::pg_borders_xml(Some(&pb)))
    });
    assert_no_leak(|| black_box(t::grid_xml(&tb)));
    assert_no_leak(|| black_box(t::shd_xml(&ShadingSpec::default())));
    assert_no_leak(|| black_box(t::look_xml(&TblLook::default())));
    assert_no_leak(|| black_box(t::tbl_pr_xml(&p)));
    assert_no_leak(|| black_box(t::tbl_pr_ex_xml(&mut cx(), &p).unwrap()));
    assert_no_leak(|| black_box(t::tr_pr_xml(&mut cx(), &r).unwrap()));
    assert_no_leak(|| black_box(t::tc_pr_xml(&mut cx(), &c).unwrap()));
    assert_no_leak(|| black_box(t::tc_pr_xml_legacy(&mut cx(), &tb.rows[0].cells[0]).unwrap()));
    assert_no_leak(|| black_box(t::legacy_or_typed_tbl_pr(&mut cx(), &tb).unwrap()));
    let typed = Table { tbl_pr: Some(Arc::new(tbl_pr())), ..table() };
    assert_no_leak(|| black_box(t::legacy_or_typed_tbl_pr(&mut cx(), &typed).unwrap()));
}

#[test]
fn m1_patches_return_the_memory() {
    let (p, r, c) = (tbl_pr(), tr_pr(), tc_pr());
    let raw_tbl = "<w:tblPr><w:tblStyle w:val=\"x\"/><w14:foo/><w:jc w:val=\"left\"/></w:tblPr>";
    let raw_tr = "<w:trPr><w:cantSplit/><w:foo/></w:trPr>";
    let raw_tc = "<w:tcPr><w:tcW w:w=\"1\" w:type=\"dxa\"/></w:tcPr>";
    for raw in [raw_tbl, "<w:tblPr/>"] {
        assert_no_leak(|| black_box(t::patch_tbl_pr(&mut cx(), raw, Some(&p)).unwrap()));
        assert_no_leak(|| black_box(t::patch_tbl_pr(&mut cx(), raw, None).unwrap()));
        assert_no_leak(|| black_box(t::patch_tbl_pr_ex(&mut cx(), raw, Some(&p)).unwrap()));
    }
    for raw in [raw_tr, "<w:trPr/>"] {
        assert_no_leak(|| black_box(t::patch_tr_pr(&mut cx(), raw, Some(&r)).unwrap()));
        assert_no_leak(|| black_box(t::patch_tr_pr(&mut cx(), raw, None).unwrap()));
    }
    for raw in [raw_tc, "<w:tcPr/>"] {
        assert_no_leak(|| black_box(t::patch_tc_pr(&mut cx(), raw, Some(&c)).unwrap()));
        assert_no_leak(|| black_box(t::patch_tc_pr(&mut cx(), raw, None).unwrap()));
    }
    // nothing changed: the raw text itself
    let same = TcPr { tc_w: Some(width(1.0)), ..TcPr::default() };
    assert_no_leak(|| black_box(t::patch_tc_pr(&mut cx(), raw_tc, Some(&same)).unwrap()));
}

#[test]
fn m1_error_paths_return_the_memory() {
    let p = tbl_pr();
    for bad in ["", "<w:tblPr", "<w:tblPr><w:a></w:tblPr>", "not xml"] {
        assert_no_leak(|| black_box(t::patch_tbl_pr(&mut cx(), bad, Some(&p)).is_err()));
        assert_no_leak(|| black_box(t::patch_tr_pr(&mut cx(), bad, None).is_err()));
    }
    let tiny = Limits { max_xml_depth: 2, ..Limits::DEFAULT };
    let deep = "<w:tblPr><w:a><w:b><w:c/></w:b></w:a></w:tblPr>";
    assert_no_leak(|| {
        let mut small = WriteCtx::new(tiny.clone(), SigMode::Canonical);
        black_box(t::patch_tbl_pr(&mut small, deep, Some(&p)).is_err())
    });
}
