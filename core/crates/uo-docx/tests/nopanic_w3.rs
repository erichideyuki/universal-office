//! No panic in the D1b table property emitters (`write::emit::table`, frontier W3) on hostile input:
//! the corpus `w:tblPr`/`w:tblPrEx`/`w:trPr`/`w:tcPr` elements of `tests/parity/table-xml.inputs`
//! with bytes flipped, cut, duplicated or tags removed, patched with their own props and with
//! hostile ones; and the patch of one huge element finishes in linear time.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Instant;

use uo_docx::js::JsNumber;
use uo_docx::model::props::{TblPr, TcPr, TrPr, WidthSpec};
use uo_docx::read::props::{read_tbl_pr, read_tc_pr, read_tr_pr};
use uo_docx::write::ctx::WriteCtx;
use uo_docx::write::emit::table as t;
use uo_docx::write::lossless::split;
use uo_docx::write::sig::SigMode;
use uo_opc::Limits;
use uo_testkit::generate::Rng;

fn cx() -> WriteCtx {
    WriteCtx::new(Limits::DEFAULT, SigMode::Ts)
}

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

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut s: Vec<u8> = text.as_bytes().to_vec();
    for _ in 0..1 + rng.below(4) {
        if s.is_empty() {
            break;
        }
        let at = rng.below(s.len());
        match rng.below(6) {
            0 => s[at] = b"<>/\"='&w: x"[rng.below(11)],
            1 => s.truncate(at),
            2 => {
                let end = (at + 1 + rng.below(120)).min(s.len());
                let dup = s[at..end].to_vec();
                s.splice(at..at, dup);
            }
            3 => {
                if let Some(k) = s[at..].iter().position(|&b| b == b'>') {
                    s.remove(at + k);
                }
            }
            4 => {
                let end = (at + rng.below(40)).min(s.len());
                s.drain(at..end);
            }
            _ => s[at] = s[at].wrapping_add(1),
        }
    }
    String::from_utf8_lossy(&s).into_owned()
}

fn hostile_tbl() -> TblPr {
    TblPr {
        tbl_w: Some(WidthSpec { w: JsNumber::Num(f64::NAN), r#type: "<&>\"".into(), ..WidthSpec::default() }),
        tbl_style: Some("\u{0}<&\">".into()),
        tbl_style_row_band_size: Some(JsNumber::Num(f64::INFINITY)),
        ..TblPr::default()
    }
}

#[test]
fn hostile_elements_do_not_panic() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let inputs = std::fs::read_to_string(dir.join("table-xml.inputs")).unwrap();
    let rows: Vec<(&str, String)> =
        inputs.lines().filter(|l| !l.starts_with('#')).map(|l| l.split_once('\t').unwrap()).map(|(k, r)| (k, unescape(r))).collect();
    let mut rng = Rng::new(0x7ab1_e0f3);
    let (mut ok, mut refused) = (0, 0);
    for i in 0..20_000 {
        let (kind, raw) = &rows[rng.below(rows.len())];
        let raw = if i % 4 == 0 { raw.clone() } else { mutate(&mut rng, raw) };
        let mut cx = cx();
        let own = split(&raw, &cx.limits).ok();
        let r = match *kind {
            "tblPr" | "tblPrEx" => {
                let cur = own.as_ref().and_then(|s| read_tbl_pr(Some(s.doc.root()))).filter(|_| i % 3 != 0).unwrap_or_else(hostile_tbl);
                if *kind == "tblPr" { t::patch_tbl_pr(&mut cx, &raw, Some(&cur)) } else { t::patch_tbl_pr_ex(&mut cx, &raw, Some(&cur)) }
            }
            "trPr" => {
                let cur = own.as_ref().and_then(|s| read_tr_pr(Some(s.doc.root()))).filter(|_| i % 3 != 0).unwrap_or_default();
                t::patch_tr_pr(&mut cx, &raw, Some(&cur))
            }
            _ => {
                let cur = own.as_ref().and_then(|s| read_tc_pr(Some(s.doc.root()))).filter(|_| i % 3 != 0).unwrap_or_default();
                t::patch_tc_pr(&mut cx, &raw, Some(&cur))
            }
        };
        if r.is_ok() {
            ok += 1
        } else {
            refused += 1
        }
        let _ = t::tbl_pr_xml(&hostile_tbl());
        let _ = t::tr_pr_xml(&mut cx, &TrPr::default());
        let _ = t::tc_pr_xml(&mut cx, &TcPr::default());
    }
    println!("no panic: {ok} patches, {refused} refused");
    assert!(ok > 5_000);
}

#[test]
fn a_huge_element_is_patched_in_linear_time() {
    // 300,000 children, a quarter of them duplicates of the changed one
    let kids = "<w:jc w:val=\"left\"/><w:tblStyle w:val=\"a\"/><w:foo/><w:tblW w:w=\"1\" w:type=\"dxa\"/>".repeat(75_000);
    let raw = format!("<w:tblPr>{kids}</w:tblPr>");
    let cur = TblPr {
        jc: Some("center".into()),
        tbl_w: Some(WidthSpec { w: JsNumber::Num(2.0), r#type: "pct".into(), ..WidthSpec::default() }),
        ..TblPr::default()
    };
    let start = Instant::now();
    let mut cx = WriteCtx::new(Limits::DEFAULT, SigMode::Ts);
    let out = t::patch_tbl_pr(&mut cx, &raw, Some(&cur)).unwrap();
    assert!(out.len() < raw.len());
    let secs = start.elapsed().as_secs_f64();
    println!("huge patch: {} B in {secs:.2}s", raw.len());
    assert!(secs < 5.0, "{secs}s");
}
