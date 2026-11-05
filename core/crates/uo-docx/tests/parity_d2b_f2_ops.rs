//! Parity of D2b-F2's pure model files (lists and styles; record uo-docx-d2b/F2.md): replays
//! `tests/parity/d2b-f2-ops.capture`, written by `js/capture-d2b-f2-ops.mjs` from the frozen TS —
//! `styles.ts` / `style-sheet.ts` (ensureDefined, create, modify, rename, delete, setQuick,
//! resolveStyleId over the `word/styles.xml` of corpus documents) and `numbering-store.ts`
//! (`DocNumbering`: addList, addNum, copyAbstract, setLevel, setLevels, withTentative over their
//! `word/numbering.xml`, with `Math.random` = the testkit xorshift32). Facets per op: `ret` (what
//! it returned, `err` for a throw) and `xml` (the text after it).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f2-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use uo_docx::session::env::FixedEnv;
use uo_docx::session::ops::lists::testing as num;
use uo_docx::session::ops::styles::testing as sty;
use uo_docx::session::ops::styles::{Ch, ModifyStyleSpec, StyleFormat};
use uo_docx::session::versions::VersionStore;
use uo_jsnum::format::to_js_string;
use uo_jsnum::json;
use uo_testkit::{Capture, Digest, Replay, repo_root};

const NAMES: [&str; 6] = ["Meu Estilo", "Destaque X", "Citação", "Nota 1", "Ttulo 1", "heading 1"];

fn format(i: usize) -> StyleFormat {
    let mut f = StyleFormat::default();
    match i {
        1 => {
            f.bold = Ch::Val(true);
            f.sz_half_pt = Ch::Val(28.0);
        }
        2 => {
            f.alignment = Ch::Val("center".to_owned());
            f.space_before_twips = Ch::Val(120.0);
            f.line_twentieths = Ch::Val(360.0);
        }
        3 => {
            f.font_ascii = Ch::Val("Arial".to_owned());
            f.color = Ch::Val("FF0000".to_owned());
            f.italic = Ch::Val(true);
            f.underline = Ch::Val(true);
        }
        4 => {
            f.indent_left_twips = Ch::Val(720.0);
            f.indent_first_line_twips = Ch::Val(-360.0);
            f.char_spacing_twips = Ch::Val(20.0);
        }
        5 => {
            f.bold = Ch::Null;
            f.font_ascii = Ch::Null;
            f.alignment = Ch::Null;
            f.indent_first_line_twips = Ch::Null;
        }
        _ => {}
    }
    f
}

fn fnum(s: &str) -> f64 {
    uo_jsnum::parse::parse_int(s, 10)
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn renamed_json(r: &[(String, String)]) -> String {
    format!("{{{}}}", r.iter().map(|(k, v)| format!("{}:{}", json::string(k), json::string(v))).collect::<Vec<_>>().join(","))
}

/// One style op: the new text (in place) and `ret`.
fn style_op(xml: &mut String, code: &str) -> String {
    let f: Vec<&str> = code.split(':').collect();
    let n = |i: usize| f[i].parse::<usize>().unwrap();
    match f[0] {
        "ens" => {
            *xml = sty::ensure_defined(xml, f[1], f[2] == "1");
            "-".to_owned()
        }
        "cre" => {
            let (x, id) = sty::create(xml, NAMES[n(1)], f[2], format(n(3)), (f[4] != "-").then_some(f[4]));
            *xml = x;
            id
        }
        "mod" => {
            let mut spec = ModifyStyleSpec { format: format(n(2)), ..ModifyStyleSpec::default() };
            match n(3) {
                1 => {
                    spec.quick = Some(false);
                    spec.auto_update = Some(true);
                }
                2 => spec.next = Ch::Null,
                3 => {
                    spec.name = Some(NAMES[4].to_owned());
                    spec.quick = Some(true);
                }
                _ => {}
            }
            match sty::modify(xml, f[1], &spec) {
                Some((x, r)) => {
                    *xml = x;
                    renamed_json(&r)
                }
                None => "err".to_owned(),
            }
        }
        "ren" => match sty::rename(xml, f[1], NAMES[n(2)]) {
            Some((x, r)) => {
                *xml = x;
                renamed_json(&r)
            }
            None => "err".to_owned(),
        },
        "del" => match sty::delete(xml, f[1]) {
            Some((x, r)) => {
                *xml = x;
                format!("[{}]", r.iter().map(|s| json::string(s)).collect::<Vec<_>>().join(","))
            }
            None => "null".to_owned(),
        },
        "qk" => {
            *xml = sty::set_quick(xml, f[1], f[2] == "1");
            "-".to_owned()
        }
        "rsv" => sty::resolve(xml, f[1]),
        _ => panic!("style op {code}"),
    }
}

#[test]
fn d2b_f2_pure_model_files_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f2-ops.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let ops: usize = cap.meta("ops").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut docs = 0;
    let mut rels: Vec<&str> = cap.records().iter().map(|r| r.key.split('#').next().unwrap()).collect();
    rels.dedup();
    for rel in rels {
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
        // ── the style sheet ──
        if let Some(mut xml) = pkg.part_text("word/styles.xml").unwrap().map(|t| t.into_owned()) {
            for j in 0..ops {
                let key = format!("{rel}#s{j}");
                let Some(code) = cap.get(&key, "op") else { break };
                replay.check(&key, "op", code);
                let ret = style_op(&mut xml, code);
                replay.check(&key, "ret", &ret);
                replay.check(&key, "xml", &sha(&xml));
            }
        }
        // ── the numbering part ──
        let mut st = VersionStore::new(pkg.part_text("word/numbering.xml").unwrap().map(|t| Arc::from(&*t)));
        let mut env = FixedEnv::default();
        let mut touched: Vec<f64> = Vec::new();
        let touch = |t: &mut Vec<f64>, a: Option<f64>| {
            if let Some(a) = a
                && !t.contains(&a)
            {
                t.push(a);
            }
        };
        for j in 0..ops {
            let key = format!("{rel}#n{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            let f: Vec<&str> = code.split(':').collect();
            let n = |i: usize| f[i].parse::<usize>().unwrap();
            let abstracts = num::abstract_ids(&st);
            let mut ret = "-".to_owned();
            match f[0] {
                "al" => {
                    let (levels, mlt, tmpl) = num::library(n(1), n(2));
                    let id = num::add_list(&mut st, &mut env, &levels, mlt, tmpl.as_deref());
                    ret = to_js_string(id);
                    touch(&mut touched, num::abstract_of(&st, id));
                }
                "an" => {
                    let ov = if f[2] == "1" { "<w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"3\"/></w:lvlOverride>" } else { "" };
                    ret = to_js_string(num::add_num(&mut st, fnum(f[1]), ov));
                }
                "ca" => {
                    let r = num::copy_abstract(&mut st, &mut env, fnum(f[1]));
                    ret = r.map_or_else(|| "undefined".to_owned(), to_js_string);
                    touch(&mut touched, r);
                }
                "sl" => {
                    let (levels, _, _) = num::library(n(3), n(4));
                    let lv = levels.get(n(2)).cloned().unwrap_or_default();
                    num::set_level(&mut st, fnum(f[1]), n(2) as f64, &num::replace_ilvl(&lv, n(2) as f64));
                    touch(&mut touched, Some(fnum(f[1])));
                }
                "sm" => {
                    let (levels, _, _) = num::library(n(2), n(3));
                    num::set_levels(&mut st, fnum(f[1]), &levels, Some("multilevel"), Some("01234567"));
                    touch(&mut touched, Some(fnum(f[1])));
                }
                "wt" => {
                    let u: Vec<f64> = match n(2) {
                        0 => vec![],
                        1 => vec![0.0],
                        2 => vec![0.0, 1.0, 2.0],
                        _ => vec![8.0],
                    };
                    let used: Vec<(f64, Vec<f64>)> = abstracts.iter().map(|a| (*a, u.clone())).collect();
                    ret = sha(&num::with_tentative(&st, &touched, &used).unwrap_or_default());
                }
                _ => panic!("numbering op {code}"),
            }
            replay.check(&key, "ret", &ret);
            replay.check(&key, "xml", &st.xml().map_or_else(|| "-".to_owned(), |x| sha(x)));
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f2-ops: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(40));
    }
    report.assert_clean();
}
