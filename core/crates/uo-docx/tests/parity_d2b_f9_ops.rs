//! Parity of D2b-F9's pure model files (record `core/docs/modules/uo-docx-d2b/F9.md`): replays
//! `tests/parity/d2b-f9-ops.capture`, written by `js/capture-d2b-f9-ops.mjs` from the frozen
//! editor's `find-engine.ts` and `navigation.ts` over the body paragraphs of corpus documents.
//!
//! Facets: `st` (the story text), per query `cq` (the RegExp source and flags, or the error with
//! V8's message), `hits` (every hit with its groups; `timeout` where the TS did not finish: the Rust
//! side must answer its step budget there), `rep` (the replacement of the first hits); per
//! Navigation-pane search `fit`/`snip`; per word `mph`, `forms`, `adapt`; `pure#*` (headingSpan,
//! moveMap, drawingGridOf, gridLines, rulerSnap/snapTo).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node --experimental-transform-types --no-warnings
//! core/crates/uo-docx/js/capture-d2b-f9-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2b_f9_support;

use std::path::Path;

use d2b_f9_support::{CLIP, REPLS, format, hs, hx, query, sha, units};
use uo_docx::read::{ReadOptions, read_document};
use uo_docx::session::ops::find::SpellLangs;
use uo_docx::session::ops::find::engine::{
    self, FindFailure, QueryError, adapt_case, compile_query, expand_replacement, find_all, found_text, metaphone, story_text, word_forms,
};
use uo_docx::session::ops::find::regex::{Budget, Vm};
use uo_docx::session::ops::navigation::{self as nav, FindOptions, GridBox};
use uo_jsnum::format::to_js_string;
use uo_jsnum::json;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_testkit::{Capture, Replay, repo_root};

fn cq_text(q: &engine::FindQuery, r: &Result<Option<engine::Compiled>, QueryError>) -> String {
    match r {
        Ok(Some(c)) => format!("ok|{}|{}", c.flags, hx(&escape_source(&c.source))),
        Ok(None) => "null".to_owned(),
        Err(QueryError::Pattern) => format!("err|pattern|{}", hx(&q.text)),
        Err(QueryError::Special(m)) => format!("err|special|{}", hx(m)),
        Err(QueryError::Syntax(m)) => format!("err|syntax|{}", hx(m)),
        Err(e) => format!("err|rust|{e:?}"),
    }
}

/// `re.source` (V8's `EscapeRegExpSource`): `/` outside a class and the line terminators escaped
/// (a `\` before a line terminator dropped). The capture records `source`, the matcher the raw
/// pattern.
fn escape_source(src: &[u16]) -> Vec<u16> {
    let lt = |u: u16| matches!(u, 0x0A | 0x0D | 0x2028 | 0x2029);
    let mut out = Vec::with_capacity(src.len());
    let mut in_class = false;
    let mut i = 0;
    while let Some(&c) = src.get(i) {
        match c {
            0x5C => {
                if src.get(i + 1).is_some_and(|&n| lt(n)) {
                    // the next one is escaped below
                } else {
                    out.push(c);
                    if let Some(&n) = src.get(i + 1) {
                        out.push(n);
                    }
                    i += 1;
                }
            }
            0x2F if !in_class => out.extend_from_slice(&[0x5C, 0x2F]),
            0x5B => {
                in_class = true;
                out.push(c);
            }
            0x5D => {
                in_class = false;
                out.push(c);
            }
            0x0A => out.extend(r"\n".encode_utf16()),
            0x0D => out.extend(r"\r".encode_utf16()),
            0x2028 => out.extend(r"\u2028".encode_utf16()),
            0x2029 => out.extend(r"\u2029".encode_utf16()),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

fn nums(v: &[f64]) -> String {
    v.iter().map(|x| json::number(*x)).collect::<Vec<_>>().join(",")
}

#[test]
fn d2b_f9_ops_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f9-ops.capture")).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let queries: usize = cap.meta("queries").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut vm = Vm::default();
    let mut docs = 0;
    let (mut max_steps, mut budget_hits, mut accepted_a1) = (0u64, 0usize, 0usize);
    for rec in cap.records().iter().filter(|r| r.key.ends_with("#st")) {
        let rel = rec.key.trim_end_matches("#st");
        let bytes = std::fs::read(root.join(rel)).unwrap();
        let pkg = Package::open(&bytes, &uo_opc::Limits::DEFAULT, &OpenOptions::default()).unwrap();
        let doc = read_document(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() }).unwrap();
        let paras = &doc.body.paras;
        let mut b0 = Budget::default();
        let st = story_text(paras, 0, paras.len(), &mut b0).unwrap();
        replay.check(&rec.key, "st", &format!("{}|{}|{}", st.text.len(), st.starts.len(), sha(&hx(&st.text))));
        let env = SpellLangs { sheet: Some(&doc.formatting.styles), fallback: "pt-BR" };
        for j in 0..queries {
            let key = format!("{rel}#q{j}");
            let Some(op) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", op);
            let f: Vec<&str> = op.split(':').collect();
            let q = query(f[0], &units(f[1]), f[2].parse().unwrap());
            let repl = REPLS[f[3].parse::<usize>().unwrap()];
            let c = compile_query(&q);
            replay.check(&key, "cq", &cq_text(&q, &c));
            let Ok(c) = c else { continue };
            let mut budget = Budget::default();
            let hits = match find_all(&st, &q, c.as_ref(), &env, &mut budget, &mut vm) {
                Ok(h) => h,
                Err(FindFailure::Budget) => {
                    budget_hits += 1;
                    replay.check(&key, "hits", "timeout");
                    continue;
                }
            };
            max_steps = max_steps.max(budget.used());
            if cap.get(&key, "hits") == Some("timeout") {
                // V8 backtracked past the capture's timeout; the Rust answer comes from a scan prune
                // that only skips start positions that cannot match: without the prunes the Rust
                // search runs out of its budget too
                let bare = c.as_ref().map(|c| engine::Compiled { re: c.re.without_prunes(), ..c.clone() });
                let mut b = Budget::default();
                let again = find_all(&st, &q, bare.as_ref(), &env, &mut b, &mut vm);
                if again == Err(FindFailure::Budget) {
                    replay.accept(&key, "hits", &format!("{} hits", hits.len()), "F9-A1: V8 backtracks past the timeout; the Rust scan prune answers (without it the Rust search runs out of its budget too)");
                    accepted_a1 += 1;
                } else {
                    replay.check(&key, "hits", &format!("{} hits, unpruned {:?}", hits.len(), again.map(|h| h.len())));
                }
                continue;
            }
            let list: Vec<String> = hits
                .iter()
                .map(|h| {
                    let g = h
                        .groups
                        .as_ref()
                        .map(|g| format!(",{}", g.iter().map(|x| hx(x)).collect::<Vec<_>>().join(";")))
                        .unwrap_or_default();
                    format!("{},{}{g}", h.start, h.end)
                })
                .collect();
            let list = list.join("\n");
            replay.check(&key, "hits", &format!("{}|{}", hits.len(), sha(&hs(&list))));
            if cap.get(&key, "hits.text").is_some() {
                replay.check(&key, "hits.text", &if list.is_empty() { "-".to_owned() } else { list.replace('\n', " ") });
            }
            let reps: Vec<String> = hits
                .iter()
                .take(3)
                .map(|h| {
                    let found = found_text(&st, h);
                    let mut t = expand_replacement(&units_of(repl), &found, h.groups.as_deref(), q.wildcards, &units_of(CLIP));
                    if !q.match_case && !q.wildcards {
                        t = adapt_case(&found, &t);
                    }
                    hx(&t)
                })
                .collect();
            let reps = reps.join(" ");
            replay.check(&key, "rep", if reps.is_empty() { "-" } else { &reps });
        }
        for j in 0..6 {
            let key = format!("{rel}#n{j}");
            let Some(op) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", op);
            let f: Vec<&str> = op.split(':').collect();
            let pi: usize = f[0].parse().unwrap();
            let text = uo_docx::session::text::para_text(paras.get(pi).unwrap());
            let needle = units(f[1]);
            let o = FindOptions { match_case: f[2].as_bytes()[0] == b'1', whole_word: f[2].as_bytes()[1] == b'1' };
            let found = nav::find_in_text(&text, &needle, o);
            let fit: Vec<String> = found.iter().map(usize::to_string).collect();
            replay.check(&key, "fit", &if fit.is_empty() { "-".to_owned() } else { fit.join(",") });
            let snip: Vec<String> = found
                .iter()
                .take(4)
                .map(|&s| {
                    let m = nav::match_snippet(&text, s, s + needle.len());
                    format!("{}|{}|{}", hx(&m.before), hx(&m.r#match), hx(&m.after))
                })
                .collect();
            replay.check(&key, "snip", &if snip.is_empty() { "-".to_owned() } else { snip.join(" ") });
        }
        for j in 0..8 {
            let key = format!("{rel}#w{j}");
            let Some(op) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", op);
            let w = units(op);
            let m = metaphone(&w);
            replay.check(&key, "mph", if m.is_empty() { "-" } else { &m });
            replay.check(&key, "forms", &word_forms(&w).iter().map(|f| hx(f)).collect::<Vec<_>>().join(" "));
            let rh = cap.get(&key, "repl").unwrap();
            replay.check(&key, "repl", rh);
            let r = units(rh);
            replay.check(&key, "adapt", &hx(&adapt_case(&w, &r)));
        }
        docs += 1;
    }
    for j in 0.. {
        let key = format!("pure#hs{j}");
        let Some(op) = cap.get(&key, "op") else { break };
        replay.check(&key, "op", op);
        let f: Vec<&str> = op.split(':').collect();
        let levels: Vec<f64> = f[0].split('.').map(|x| f64::from(x.parse::<u32>().unwrap())).collect();
        let i: usize = f[1].parse().unwrap();
        let t: usize = f[2].parse().unwrap();
        let (s, e) = nav::heading_span(&levels, i);
        replay.check(&key, "span", &format!("{s},{e}"));
        let map = if t >= s && t <= e {
            "-".to_owned()
        } else {
            nav::move_map(levels.len(), s, e, t).iter().map(usize::to_string).collect::<Vec<_>>().join(".")
        };
        replay.check(&key, "map", &map);
    }
    for j in 0.. {
        let key = format!("pure#grid{j}");
        let Some(op) = cap.get(&key, "op") else { break };
        replay.check(&key, "op", op);
        let x = String::from_utf16(&units(op)).unwrap();
        let g = nav::drawing_grid_of(if x.is_empty() { None } else { Some(&x) });
        let gj = format!(
            "{{\"hSpacing\":{},\"vSpacing\":{},\"everyH\":{},\"everyV\":{},\"fromMargins\":{},\"originX\":{},\"originY\":{}}}",
            json::number(g.h_spacing),
            json::number(g.v_spacing),
            json::number(g.every_h),
            json::number(g.every_v),
            g.from_margins,
            json::number(g.origin_x),
            json::number(g.origin_y)
        );
        replay.check(&key, "grid", &gj);
        replay.check(&key, "box", cap.get(&key, "box").unwrap());
        let b: Vec<f64> = cap.get(&key, "box").unwrap().split(',').map(uo_jsnum::parse::to_number).collect();
        let (xs, ys) = nav::grid_lines(&g, GridBox { x0: b[0], y0: b[1], x1: b[2], y1: b[3] }, (b[4], b[5]));
        let lj = format!("{{\"xs\":[{}],\"ys\":[{}]}}", nums(&xs), nums(&ys));
        replay.check(&key, "lines", &format!("{},{}|{}", xs.len(), ys.len(), sha(&hs(&lj))));
    }
    for j in 0.. {
        let key = format!("pure#snap{j}");
        let Some(op) = cap.get(&key, "op") else { break };
        replay.check(&key, "op", op);
        let (v, unit) = op.rsplit_once(':').unwrap();
        let step = nav::ruler_snap(unit == "cm");
        replay.check(&key, "snap", &format!("{}|{}", to_js_string(step), to_js_string(nav::snap_to(uo_jsnum::parse::to_number(v), step))));
    }
    let report = replay.finish();
    println!(
        "d2b-f9-ops: {docs} documents, {}; most steps of a finished query {max_steps}, {budget_hits} over the budget, {accepted_a1} F9-A1",
        report.summary()
    );
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_D2A_ALL").map_or(40, |_| 100_000)));
    }
    let _ = format(0);
    report.assert_clean();
}

fn units_of(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
