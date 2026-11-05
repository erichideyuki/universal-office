//! Parity of the table property emitters (`write::emit::table`, docx-core `table-xml.ts` and the
//! legacy generators of `serialize.ts`, D1b frontier W3): replays `tests/parity/table-xml.capture`,
//! written by `js/capture-table-xml.mjs` from the frozen TS over the distinct `w:tblPr`/`w:tblPrEx`/
//! `w:trPr`/`w:tcPr` of the corpus (`tests/parity/table-xml.inputs`) with every named change of the
//! typed props (the same lists as the capture script, implemented here), over seeded synthetic props
//! (`patch(gen(A), B)`) and over seeded synthetic tables (`tableXml`). The generator of the synthetic
//! values is the script's (`lcg`, the same draws in the same order).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-table-xml.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![expect(clippy::field_reassign_with_default, reason = "the named changes set one field over the TS defaults, as the capture script does")]

use std::path::Path;
use std::sync::Arc;

use uo_docx::js::{JsNumber, Str};
use uo_docx::model::para::BorderEdge;
use uo_docx::model::props::{
    BorderSpec, CellMargins, ShadingSpec, TblBorders, TblLook, TblPr, TblpPr, TcBorders, TcPr, TrHeight, TrPr, WidthSpec,
};
use uo_docx::model::table::{Cell, Row, Table, TableBorders};
use uo_docx::model::{Document, Story};
use uo_docx::read::props::{read_tbl_pr, read_tc_pr, read_tr_pr};
use uo_docx::write::ctx::{WriteCtx, Writer};
use uo_docx::write::emit::table as t;
use uo_docx::write::lossless::split;
use uo_docx::write::sig::SigMode;
use uo_opc::package::OpenOptions;
use uo_opc::{Limits, Package};
use uo_testkit::{Capture, Digest, Replay};

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

fn n(x: f64) -> Option<JsNumber> {
    Some(JsNumber::Num(x))
}

fn s(x: &str) -> Option<Str> {
    Some(Str::from(x))
}

fn wd(w: f64, ty: &str) -> WidthSpec {
    WidthSpec { w: JsNumber::Num(w), r#type: Str::from(ty), ..WidthSpec::default() }
}

fn side(color: &str) -> Box<BorderSpec> {
    Box::new(BorderSpec { val: "single".into(), sz: n(4.0), space: n(0.0), color: s(color), ..BorderSpec::default() })
}

fn nil() -> Box<BorderSpec> {
    Box::new(BorderSpec { val: "nil".into(), ..BorderSpec::default() })
}

fn not(b: Option<bool>) -> Option<bool> {
    Some(b != Some(true))
}

// ───────────────────────── the named changes ─────────────────────────

const TBL_MUTS: &[&str] = &[
    "none",
    "style",
    "width",
    "jc",
    "borders",
    "noborders",
    "emptyborders",
    "layout",
    "look",
    "shd",
    "mar",
    "ind",
    "pos",
    "misc",
    "unset",
    "undefined",
];
const TR_MUTS: &[&str] = &["none", "height", "heightLeast", "header", "split", "hidden", "grid", "jc", "cnf", "unset", "undefined"];
const TC_MUTS: &[&str] =
    &["none", "width", "span", "hmerge", "vmerge", "borders", "noborders", "shd", "wrap", "mar", "dir", "cnf", "unset", "undefined"];

fn mutate_tbl(m: &str, p: &mut TblPr) {
    match m {
        "none" => {}
        "style" => p.tbl_style = s("TableGrid"),
        "width" => p.tbl_w = Some(wd(5000.0, "pct")),
        "jc" => p.jc = if p.jc.as_deref() == Some("center") { s("right") } else { s("center") },
        "borders" => {
            p.tbl_borders = Some(Box::new(TblBorders {
                top: Some(side("FF0000")),
                bottom: Some(nil()),
                inside_h: Some(Box::new(BorderSpec {
                    val: "dashed".into(),
                    sz: n(8.0),
                    theme_color: s("accent1"),
                    shadow: Some(true),
                    ..BorderSpec::default()
                })),
                ..TblBorders::default()
            }))
        }
        "noborders" => p.tbl_borders = None,
        "emptyborders" => p.tbl_borders = Some(Box::default()),
        "layout" => p.tbl_layout = s("fixed"),
        "look" => p.tbl_look = Some(Box::new(TblLook { first_row: Some(true), no_h_band: Some(true), ..TblLook::default() })),
        "shd" => p.shd = Some(Box::new(ShadingSpec { val: s("clear"), fill: s("DDDDDD"), ..ShadingSpec::default() })),
        "mar" => {
            p.tbl_cell_mar =
                Some(Box::new(CellMargins { top: Some(wd(50.0, "dxa")), left: Some(wd(108.0, "dxa")), ..CellMargins::default() }))
        }
        "ind" => {
            p.tbl_ind = Some(wd(720.0, "dxa"));
            p.tbl_cell_spacing = Some(wd(10.0, "dxa"));
        }
        "pos" => {
            p.tblp_pr = Some(Box::new(TblpPr {
                left_from_text: n(180.0),
                vert_anchor: s("text"),
                tblp_x: n(100.0),
                tblp_y_spec: s("top"),
                ..TblpPr::default()
            }))
        }
        "misc" => {
            p.tbl_caption = s("a&b\"<c>");
            p.tbl_description = s("d");
            p.bidi_visual = not(p.bidi_visual);
            p.tbl_overlap = s("never");
            p.tbl_style_row_band_size = n(2.0);
            p.tbl_style_col_band_size = n(3.0);
        }
        "unset" => *p = TblPr::default(),
        other => panic!("{other}"),
    }
}

fn mutate_tr(m: &str, p: &mut TrPr) {
    match m {
        "none" => {}
        "height" => p.tr_height = Some(TrHeight { val: JsNumber::Num(300.0), h_rule: "exact".into(), ..TrHeight::default() }),
        "heightLeast" => p.tr_height = Some(TrHeight { val: JsNumber::Num(400.0), h_rule: "atLeast".into(), ..TrHeight::default() }),
        "header" => p.tbl_header = not(p.tbl_header),
        "split" => p.cant_split = not(p.cant_split),
        "hidden" => p.hidden = not(p.hidden),
        "grid" => {
            p.grid_before = n(1.0);
            p.grid_after = n(2.0);
            p.w_before = Some(wd(100.0, "dxa"));
            p.w_after = Some(wd(200.0, "pct"));
        }
        "jc" => {
            p.jc = s("center");
            p.tbl_cell_spacing = Some(wd(20.0, "dxa"));
        }
        "cnf" => p.cnf_style = s("100000000000"),
        "unset" => *p = TrPr::default(),
        other => panic!("{other}"),
    }
}

fn mutate_tc(m: &str, p: &mut TcPr) {
    match m {
        "none" => {}
        "width" => p.tc_w = Some(wd(3000.0, "dxa")),
        "span" => p.grid_span = n(2.0),
        "hmerge" => {
            p.h_merge = s("restart");
            p.v_merge = s("continue");
        }
        "vmerge" => {
            p.v_merge = s("restart");
            p.h_merge = s("continue");
        }
        "borders" => {
            p.tc_borders = Some(Box::new(TcBorders {
                top: Some(side("00FF00")),
                left: Some(nil()),
                tl2br: Some(Box::new(BorderSpec { val: "single".into(), sz: n(6.0), frame: Some(false), ..BorderSpec::default() })),
                ..TcBorders::default()
            }))
        }
        "noborders" => p.tc_borders = Some(Box::default()),
        "shd" => {
            p.shd = Some(Box::new(ShadingSpec {
                val: s("clear"),
                color: s("auto"),
                fill: s("FFAA00"),
                theme_fill: s("accent2"),
                ..ShadingSpec::default()
            }))
        }
        "wrap" => {
            p.no_wrap = not(p.no_wrap);
            p.tc_fit_text = Some(true);
            p.hide_mark = Some(false);
        }
        "mar" => {
            p.tc_mar = Some(Box::new(CellMargins { top: Some(wd(20.0, "dxa")), right: Some(wd(30.0, "dxa")), ..CellMargins::default() }))
        }
        "dir" => {
            p.text_direction = s("btLr");
            p.v_align = s("center");
        }
        "cnf" => p.cnf_style = s("001000000000"),
        "unset" => *p = TcPr::default(),
        other => panic!("{other}"),
    }
}

// ───────────────────────── the seeded synthetic values ─────────────────────────

struct R(u32);

impl R {
    fn new(seed: u32) -> R {
        R(seed.wrapping_mul(2_654_435_761))
    }
    fn n(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.0 >> 16) as usize) % n
    }
}

fn opt<T>(r: &mut R, f: impl FnOnce(&mut R) -> T) -> Option<T> {
    if r.n(3) == 0 { None } else { Some(f(r)) }
}

const S: &[&str] = &["TableGrid", "center", "a&b", "\"q\"<x>", "", "fixed"];

fn str_(r: &mut R) -> Str {
    Str::from(S[r.n(S.len())])
}

fn num(r: &mut R) -> JsNumber {
    match r.n(9) {
        0 => JsNumber::Num(0.0),
        1 => JsNumber::Num(1.0),
        2 => JsNumber::Num(20.0),
        3 => JsNumber::Num(108.0),
        4 => JsNumber::Num(-5.0),
        5 => JsNumber::Num(5000.0),
        6 => JsNumber::Num(1.5),
        7 => JsNumber::Num(1e21),
        _ => JsNumber::Null,
    }
}

fn bool_(r: &mut R) -> bool {
    r.n(2) == 1
}

fn width(r: &mut R) -> WidthSpec {
    let w = num(r);
    let ty = ["dxa", "pct", "auto", "nil"][r.n(4)];
    WidthSpec { w, r#type: Str::from(ty), ..WidthSpec::default() }
}

fn border(r: &mut R) -> BorderSpec {
    let val = ["single", "nil", "dashed", "a\"b"][r.n(4)];
    let mut b = BorderSpec { val: Str::from(val), ..BorderSpec::default() };
    b.sz = opt(r, num);
    b.space = opt(r, num);
    b.color = opt(r, |r| Str::from(["FF0000", "auto", "a&b"][r.n(3)]));
    b.theme_color = opt(r, str_);
    b.theme_tint = opt(r, str_);
    b.theme_shade = opt(r, str_);
    b.shadow = opt(r, bool_);
    b.frame = opt(r, bool_);
    b
}

fn borders8(r: &mut R) -> [Option<Box<BorderSpec>>; 8] {
    let mut out: [Option<Box<BorderSpec>>; 8] = Default::default();
    for slot in &mut out {
        *slot = opt(r, |r| Box::new(border(r)));
    }
    out
}

fn shd(r: &mut R) -> ShadingSpec {
    let mut o = ShadingSpec::default();
    o.val = opt(r, str_);
    o.color = opt(r, str_);
    o.fill = opt(r, str_);
    o.theme_fill = opt(r, str_);
    o.theme_fill_tint = opt(r, str_);
    o.theme_fill_shade = opt(r, str_);
    o.theme_color = opt(r, str_);
    o
}

fn mar(r: &mut R) -> CellMargins {
    let mut o = CellMargins::default();
    o.top = opt(r, width);
    o.left = opt(r, width);
    o.bottom = opt(r, width);
    o.right = opt(r, width);
    o
}

fn look(r: &mut R) -> TblLook {
    let mut o = TblLook::default();
    o.first_row = opt(r, bool_);
    o.last_row = opt(r, bool_);
    o.first_column = opt(r, bool_);
    o.last_column = opt(r, bool_);
    o.no_h_band = opt(r, bool_);
    o.no_v_band = opt(r, bool_);
    o
}

fn tblp(r: &mut R) -> TblpPr {
    let mut o = TblpPr::default();
    o.left_from_text = opt(r, num);
    o.right_from_text = opt(r, num);
    o.top_from_text = opt(r, num);
    o.bottom_from_text = opt(r, num);
    o.vert_anchor = opt(r, str_);
    o.horz_anchor = opt(r, str_);
    o.tblp_x_spec = opt(r, str_);
    o.tblp_x = opt(r, num);
    o.tblp_y_spec = opt(r, str_);
    o.tblp_y = opt(r, num);
    o
}

fn synth_tbl(r: &mut R) -> TblPr {
    let mut p = TblPr::default();
    p.tbl_style = opt(r, str_);
    p.tblp_pr = opt(r, |r| Box::new(tblp(r)));
    p.tbl_overlap = opt(r, str_);
    p.bidi_visual = opt(r, bool_);
    p.tbl_style_row_band_size = opt(r, num);
    p.tbl_style_col_band_size = opt(r, num);
    p.tbl_w = opt(r, width);
    p.jc = opt(r, str_);
    p.tbl_cell_spacing = opt(r, width);
    p.tbl_ind = opt(r, width);
    p.tbl_borders = opt(r, |r| {
        let [top, left, bottom, right, inside_h, inside_v, tl2br, tr2bl] = borders8(r);
        Box::new(TblBorders { top, left, bottom, right, inside_h, inside_v, tl2br, tr2bl, ..TblBorders::default() })
    });
    p.shd = opt(r, |r| Box::new(shd(r)));
    p.tbl_layout = opt(r, str_);
    p.tbl_cell_mar = opt(r, |r| Box::new(mar(r)));
    p.tbl_look = opt(r, |r| Box::new(look(r)));
    p.tbl_caption = opt(r, str_);
    p.tbl_description = opt(r, str_);
    p
}

fn synth_tr(r: &mut R) -> TrPr {
    let mut p = TrPr::default();
    p.cnf_style = opt(r, str_);
    p.grid_before = opt(r, num);
    p.grid_after = opt(r, num);
    p.w_before = opt(r, width);
    p.w_after = opt(r, width);
    p.cant_split = opt(r, bool_);
    p.tr_height = opt(r, |r| {
        let val = num(r);
        let h_rule = ["atLeast", "exact", "auto", "x\"y"][r.n(4)];
        TrHeight { val, h_rule: Str::from(h_rule), ..TrHeight::default() }
    });
    p.tbl_header = opt(r, bool_);
    p.tbl_cell_spacing = opt(r, width);
    p.jc = opt(r, str_);
    p.hidden = opt(r, bool_);
    p
}

fn merge(r: &mut R) -> Str {
    Str::from(["restart", "continue", "x"][r.n(3)])
}

fn synth_tc(r: &mut R) -> TcPr {
    let mut p = TcPr::default();
    p.cnf_style = opt(r, str_);
    p.tc_w = opt(r, width);
    p.grid_span = opt(r, num);
    p.h_merge = opt(r, merge);
    p.v_merge = opt(r, merge);
    p.tc_borders = opt(r, |r| {
        let [top, left, bottom, right, inside_h, inside_v, tl2br, tr2bl] = borders8(r);
        Box::new(TcBorders { top, left, bottom, right, inside_h, inside_v, tl2br, tr2bl, ..TcBorders::default() })
    });
    p.shd = opt(r, |r| Box::new(shd(r)));
    p.no_wrap = opt(r, bool_);
    p.tc_mar = opt(r, |r| Box::new(mar(r)));
    p.text_direction = opt(r, str_);
    p.tc_fit_text = opt(r, bool_);
    p.v_align = opt(r, str_);
    p.hide_mark = opt(r, bool_);
    p
}

fn edge(r: &mut R) -> BorderEdge {
    let style = ["single", "a\"b", "double"][r.n(3)];
    let size = num(r);
    let space = num(r);
    let color = opt(r, |r| Str::from(["FF0000", "", "a&b"][r.n(3)]));
    BorderEdge { style: Str::from(style), size_eighth_pt: size, space_pt: space, color, ..BorderEdge::default() }
}

fn table_borders(r: &mut R) -> TableBorders {
    let mut o = TableBorders::default();
    o.top = opt(r, edge);
    o.left = opt(r, edge);
    o.bottom = opt(r, edge);
    o.right = opt(r, edge);
    o.inside_h = opt(r, edge);
    o.inside_v = opt(r, edge);
    o
}

fn synth_table(r: &mut R) -> Table {
    let mut tb = Table::default();
    for _ in 0..r.n(4) {
        tb.grid_cols_twips.push(num(r));
    }
    tb.borders = opt(r, |r| Box::new(table_borders(r)));
    tb.tbl_pr = opt(r, |r| Arc::new(synth_tbl(r)));
    let rows = 1 + r.n(3);
    for _ in 0..rows {
        let mut row = Row::default();
        row.tr_pr = opt(r, |r| Arc::new(synth_tr(r)));
        row.tbl_pr_ex = opt(r, |r| Arc::new(synth_tbl(r)));
        let cells = 1 + r.n(3);
        for _ in 0..cells {
            let grid_span = JsNumber::Num([1.0, 2.0, 3.0, 0.0][r.n(4)]);
            let mut c = Cell { grid_span, content: Story::default(), ..Cell::default() };
            c.width_twips = opt(r, num);
            c.borders = opt(r, |r| Box::new(table_borders(r)));
            c.shading_hex = opt(r, |r| Str::from(["FFAA00", "", "a&b"][r.n(3)]));
            c.tc_pr = opt(r, |r| Arc::new(synth_tc(r)));
            row.cells.push(c);
        }
        tb.rows.push(row);
    }
    tb
}

// ───────────────────────── the replay ─────────────────────────

fn short(x: &str) -> String {
    Digest::of(x.as_bytes()).short()
}

fn res(r: Result<String, uo_docx::DocxError>) -> String {
    r.map_or_else(|_| "err".to_owned(), |s| short(&s))
}

fn cx() -> WriteCtx {
    WriteCtx::new(Limits::DEFAULT, SigMode::Ts)
}

/// Both facets of one corpus element and change.
fn corpus_result(kind: &str, raw: &str, m: &str) -> (String, String) {
    let mut cx = cx();
    let Ok(sp) = split(raw, &cx.limits) else { return ("err".into(), "na".into()) };
    let root = sp.doc.root();
    match kind {
        "tblPr" | "tblPrEx" => {
            let mut cur = read_tbl_pr(Some(root)).unwrap_or_default();
            let undefined = m == "undefined";
            mutate_tbl(if undefined { "none" } else { m }, &mut cur);
            let cur = (!undefined).then_some(cur);
            if kind == "tblPr" {
                (res(t::patch_tbl_pr(&mut cx, raw, cur.as_ref())), cur.as_ref().map_or("na".into(), |c| short(&t::tbl_pr_xml(c))))
            } else {
                (
                    res(t::patch_tbl_pr_ex(&mut cx, raw, cur.as_ref())),
                    cur.as_ref().map_or("na".into(), |c| res(t::tbl_pr_ex_xml(&mut self_cx(), c))),
                )
            }
        }
        "trPr" => {
            let mut cur = read_tr_pr(Some(root)).unwrap_or_default();
            let undefined = m == "undefined";
            mutate_tr(if undefined { "none" } else { m }, &mut cur);
            let cur = (!undefined).then_some(cur);
            (res(t::patch_tr_pr(&mut cx, raw, cur.as_ref())), cur.as_ref().map_or("na".into(), |c| res(t::tr_pr_xml(&mut self_cx(), c))))
        }
        _ => {
            let mut cur = read_tc_pr(Some(root)).unwrap_or_default();
            let undefined = m == "undefined";
            mutate_tc(if undefined { "none" } else { m }, &mut cur);
            let cur = (!undefined).then_some(cur);
            (res(t::patch_tc_pr(&mut cx, raw, cur.as_ref())), cur.as_ref().map_or("na".into(), |c| res(t::tc_pr_xml(&mut self_cx(), c))))
        }
    }
}

fn self_cx() -> WriteCtx {
    cx()
}

fn synth_result(kind: &str, i: u32) -> (String, String) {
    let mut r = R::new(i + 1);
    let mut cx = cx();
    match kind {
        "tblPr" => {
            let (a, b) = (synth_tbl(&mut r), synth_tbl(&mut r));
            let raw = t::tbl_pr_xml(&a);
            (short(&raw), res(t::patch_tbl_pr(&mut cx, &raw, Some(&b))))
        }
        "tblPrEx" => {
            let (a, b) = (synth_tbl(&mut r), synth_tbl(&mut r));
            let g = t::tbl_pr_ex_xml(&mut cx, &a).unwrap();
            let raw = if g.is_empty() { "<w:tblPrEx/>".to_owned() } else { g.clone() };
            (short(&g), res(t::patch_tbl_pr_ex(&mut cx, &raw, Some(&b))))
        }
        "trPr" => {
            let (a, b) = (synth_tr(&mut r), synth_tr(&mut r));
            let g = t::tr_pr_xml(&mut cx, &a).unwrap();
            let raw = if g.is_empty() { "<w:trPr/>".to_owned() } else { g.clone() };
            (short(&g), res(t::patch_tr_pr(&mut cx, &raw, Some(&b))))
        }
        _ => {
            let (a, b) = (synth_tc(&mut r), synth_tc(&mut r));
            let g = t::tc_pr_xml(&mut cx, &a).unwrap();
            (short(&g), res(t::patch_tc_pr(&mut cx, &g, Some(&b))))
        }
    }
}

#[test]
fn table_props_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("table-xml.capture")).unwrap();
    let inputs = std::fs::read_to_string(dir.join("table-xml.inputs")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut index = std::collections::HashMap::<String, usize>::new();
    for line in inputs.lines().filter(|l| !l.starts_with('#')) {
        let (kind, raw) = line.split_once('\t').unwrap();
        let raw = unescape(raw);
        let i = index.entry(kind.to_owned()).or_default();
        let muts = match kind {
            "tblPr" | "tblPrEx" => TBL_MUTS,
            "trPr" => TR_MUTS,
            _ => TC_MUTS,
        };
        for m in muts {
            let (patch, gen_) = corpus_result(kind, &raw, m);
            let key = format!("{kind}#{i}#{m}");
            replay.check(&key, "patch", &patch);
            replay.check(&key, "gen", &gen_);
        }
        *i += 1;
    }
    for kind in ["tblPr", "tblPrEx", "trPr", "tcPr"] {
        for i in 0..1500u32 {
            let (g, p) = synth_result(kind, i);
            let key = format!("synth{kind}#{i}");
            replay.check(&key, "gen", &g);
            replay.check(&key, "patch", &p);
        }
    }
    let pkg_limits = Limits::DEFAULT;
    let mut pkg = Package::new_empty(&pkg_limits, &OpenOptions::default());
    let doc = Document::default();
    for i in 0..800u32 {
        let table = synth_table(&mut R::new(100_000 + i));
        let mut fresh = 0.0;
        let mut w = Writer { doc: &doc, pkg: &mut pkg, cx: cx(), fresh_id: &mut fresh, lossless: None };
        let xml = w.table_xml(&table);
        replay.check(&format!("table#{i}"), "xml", &res(xml));
    }
    let report = replay.finish();
    println!("table-xml: {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(30));
    }
    report.assert_clean();
    assert!(report.equal > 25_000);
}
