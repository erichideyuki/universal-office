//! Parity of the lossless surgery (`write::lossless`, docx-core `lossless.ts`): replays
//! `tests/parity/lossless.capture`, written by `js/capture-lossless.mjs` from the frozen TS
//! (`patchRPr`, `patchPPr`, `patchSectPr`) over the distinct `w:rPr`/`w:pPr`/`w:sectPr` of the corpus
//! (`tests/parity/lossless.inputs`), each with every named mutation of the editing props — the same
//! mutation lists as the capture script (`R_MUTS`, `P_MUTS`, `S_MUTS`), implemented here.
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-lossless.mjs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use uo_docx::js::{JsNumber, Str};
use uo_docx::model::Paragraph;
use uo_docx::model::para::ParaProps;
use uo_docx::model::run::{Run, RunProps};
use uo_docx::model::section::{Cols, LineNumbering, SectPr};
use uo_docx::read::props::{read_ppr, read_rpr};
use uo_docx::read::props_map::{edit_base_props, para_props_from_ppr, read_sect_pr};
use uo_docx::write::ctx::{DEFAULT_NUM_IDS, WriteCtx};
use uo_docx::write::lossless::{HfRefXml, SectArg, patch_ppr, patch_rpr, patch_sect_pr, split};
use uo_docx::write::sig::SigMode;
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

fn not(b: Option<bool>) -> Option<bool> {
    Some(b != Some(true))
}

const R_MUTS: &[&str] =
    &["none", "bold", "italic", "sz", "color", "font", "underline", "super", "hidden", "style", "scale", "spacing", "unset"];
const P_MUTS: &[&str] = &["none", "center", "indent", "space", "style", "keep", "nonum", "tabs", "auto", "outline"];
const S_MUTS: &[&str] = &["none", "swap", "margins", "title", "cols", "valign", "pgnum", "refs", "lnnum"];

fn mutate_run(m: &str, p: &mut RunProps) {
    match m {
        "none" => {}
        "bold" => p.bold = not(p.bold),
        "italic" => p.italic = not(p.italic),
        "sz" => p.sz_half_pt = n(31.0),
        "color" => {
            p.color = Some(Str::from("FF0000"));
            p.color_theme = None;
        }
        "font" => p.font_ascii = Some(Str::from("Arial")),
        "underline" => p.underline = not(p.underline),
        "super" => p.vert_align = Some(Str::from("superscript")),
        "hidden" => p.hidden = not(p.hidden),
        "style" => p.char_style = Some(Str::from("Strong")),
        "scale" => {
            p.scale_pct = n(150.0);
            p.kern_half_pt = n(0.0);
            p.position_half_pt = n(6.0);
        }
        "spacing" => {
            p.char_spacing_twips = n(20.0);
            p.highlight = Some(Str::from("yellow"));
        }
        "unset" => {
            p.sz_half_pt = None;
            p.color = None;
            p.font_ascii = None;
        }
        other => panic!("{other}"),
    }
}

fn mutate_para(m: &str, p: &mut ParaProps) {
    match m {
        "none" => {}
        "center" => p.alignment = Some(Str::from("center")),
        "indent" => {
            p.indent_left_twips = n(720.0);
            p.indent_first_line_twips = n(-360.0);
        }
        "space" => {
            p.space_before_twips = n(120.0);
            p.line_twentieths = n(360.0);
        }
        "style" => p.style_id = Some(Str::from("Heading1")),
        "keep" => p.keep_next = not(p.keep_next),
        "nonum" => p.numbering = None,
        "tabs" => {
            p.tab_stops = Some(Vec::new());
            p.shading_hex = Some(Str::from("DDDDDD"));
        }
        "auto" => p.space_after_auto = Some(true),
        "outline" => p.outline_lvl = n(2.0),
        other => panic!("{other}"),
    }
}

fn add(x: JsNumber, d: f64) -> JsNumber {
    JsNumber::Num(x.to_number() + d)
}

fn mutate_sect(m: &str, s: &mut SectPr) -> Option<Vec<HfRefXml>> {
    match m {
        "none" => {}
        "swap" => std::mem::swap(&mut s.page_width_twips, &mut s.page_height_twips),
        "margins" => {
            s.margin_top_twips = add(s.margin_top_twips, 100.0);
            s.margin_left_twips = add(s.margin_left_twips, 100.0);
            s.header_distance_twips = n(500.0);
        }
        "title" => s.title_pg = not(s.title_pg),
        "cols" => s.cols = Some(Cols { count: JsNumber::Num(2.0), space_twips: JsNumber::Num(708.0), sep: true, ..Cols::default() }),
        "valign" => {
            s.v_align = Some(Str::from("center"));
            s.r#type = Some(Str::from("continuous"));
        }
        "pgnum" => {
            s.pg_num_start = n(3.0);
            s.pg_num_fmt = Some(Str::from("lowerRoman"));
        }
        "refs" => return Some(vec![HfRefXml { kind: "header".into(), r#type: "default".into(), rid: "rIdX".into() }]),
        "lnnum" => s.ln_num_type = Some(LineNumbering { count_by: n(5.0), ..LineNumbering::default() }),
        other => panic!("{other}"),
    }
    None
}

fn result(kind: &str, raw: &str, m: &str) -> Option<String> {
    let mut cx = WriteCtx::new(uo_opc::Limits::DEFAULT, SigMode::Ts);
    let sp = split(raw, &cx.limits).ok()?;
    let root = sp.doc.root();
    match kind {
        "rPr" => {
            let base_run = Run { text: "x".into(), ..Run::default() };
            let mut props = edit_base_props(&base_run, read_rpr(Some(root)).as_ref());
            mutate_run(m, &mut props);
            let run = Run { props: Arc::new(props), ..base_run };
            patch_rpr(&mut cx, Some(raw), &run).ok()
        }
        "pPr" => {
            let mut props = para_props_from_ppr(read_ppr(Some(root)).as_ref());
            mutate_para(m, &mut props);
            let para = Paragraph { props: Arc::new(props), ..Paragraph::default() };
            patch_ppr(&mut cx, Some(raw), &para, &SectArg::Keep, DEFAULT_NUM_IDS).ok().map(|o| o.xml)
        }
        _ => {
            let mut cur = read_sect_pr(Some(root));
            let refs = mutate_sect(m, &mut cur);
            patch_sect_pr(&mut cx, raw, &cur, refs.as_deref()).ok()
        }
    }
}

#[test]
fn lossless_surgery_matches_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("lossless.capture")).unwrap();
    let inputs = std::fs::read_to_string(dir.join("lossless.inputs")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut index = std::collections::HashMap::<String, usize>::new();
    for line in inputs.lines().filter(|l| !l.starts_with('#')) {
        let (kind, raw) = line.split_once('\t').unwrap();
        let raw = unescape(raw);
        let i = index.entry(kind.to_owned()).or_default();
        let muts = match kind {
            "rPr" => R_MUTS,
            "pPr" => P_MUTS,
            _ => S_MUTS,
        };
        for m in muts {
            let out = result(kind, &raw, m).map_or_else(|| "err".to_owned(), |s| Digest::of(s.as_bytes()).short());
            replay.check(&format!("{kind}#{i}#{m}"), "out", &out);
        }
        *i += 1;
    }
    let report = replay.finish();
    println!("lossless: {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(30));
    }
    report.assert_clean();
    assert!(report.equal > 20_000);
}
