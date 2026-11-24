//! C5b: the scripted edit steps of `sheet-x1` that C1b deferred (`save_parity`'s `replay_edits` stops at the first step
//! that needs the session). Every workbook of the capture is opened by the [`Session`] (pt-BR, the capture's clock and
//! `Math.random` stream) and its 8 seeded steps run through the session; after each step the save (every part
//! decompressed: name, length, SHA-256 prefix, archive order), the parts changed from the input, the result of the call
//! and the display text of the active sheet are compared with the TS.
//!
//! The kind of a `merge` step is drawn by the script AFTER the script is generated (`el.merge(R.pick([...]))`), so the
//! description does not carry it: the replay tries `center`, `across`, `cells` and keeps the kind whose save equals the
//! TS's (counted separately).
//!
//! Run: `cargo test -p uo-xlsx --features edit,facet --test edit_x1 -- --nocapture`
#![cfg(all(feature = "edit", feature = "facet"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::cast_possible_truncation, clippy::too_many_lines)]

#[path = "support/edit_run.rs"]
mod edit_run;

use edit_run::*;
use uo_testkit::Capture;
use uo_xlsx::save::{FontChange, StyleChange};
use uo_xlsx::session::{BorderKind, ClearKind, FindOptions, MergeKind, NumFmtArg, Session};

/// `JSON.stringify(text)` undone (the step descriptions).
fn unquote(j: &str) -> String {
    let b = uo_jsnum::text::trim(j);
    let b = b.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(b);
    let mut out = String::new();
    let mut it = b.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('u') => {
                let hex: String = it.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'));
            }
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

fn json_field<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let i = obj.find(&pat)? + pat.len();
    let rest = &obj[i..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Runs one step (`desc` = the unquoted description); `merge` takes the kind to try.
fn run(s: &mut Session, desc: &str, merge: MergeKind) -> Option<String> {
    let (op, rest) = desc.split_once(' ').unwrap_or((desc, ""));
    let dash = "-".to_owned();
    let sel = |s: &mut Session, r: &str| s.select_range(r).unwrap();
    Some(match op {
        "text" | "formula" => {
            let (cell, t) = rest.split_once(' ')?;
            sel(s, cell);
            s.set_active_cell_text(&u(&unquote(t))).to_json()
        }
        "font" => {
            let (g, j) = rest.split_once(' ')?;
            sel(s, g);
            let flag = |k: &str| json_field(j, k).map(|v| v == "true");
            let color = json_field(j, "color").map(|v| if v == "null" { None } else { Some(v.trim_matches('"').to_owned()) });
            let size = json_field(j, "size").map(uo_jsnum::parse::parse_float).filter(|x| x.is_finite());
            s.set_font(FontChange { bold: flag("bold"), italic: flag("italic"), size, color, ..FontChange::default() });
            dash
        }
        "numfmt" => {
            let (g, j) = rest.split_once(' ')?;
            sel(s, g);
            let arg = if let Some(i) = j.find("\"code\":\"") {
                // the JSON string value (a code holds commas)
                let rest = &j[i + 8..];
                let b = rest.as_bytes();
                let mut end = 0;
                while end < b.len() && !(b[end] == b'"' && (end == 0 || b[end - 1] != b'\\')) {
                    end += 1;
                }
                NumFmtArg::Code(unquote(&format!("\"{}\"", &rest[..end])))
            } else {
                use uo_xlsx::grid::NumberFormatKind as K;
                NumFmtArg::Kind(match unquote(j).as_str() {
                    "percent" => K::Percent,
                    "currency" => K::Currency,
                    "shortDate" => K::ShortDate,
                    "scientific" => K::Scientific,
                    "text" => K::Text,
                    "comma" => K::Comma,
                    _ => return None,
                })
            };
            s.set_number_format(&arg);
            dash
        }
        "borders" => {
            let (g, k) = rest.split_once(' ')?;
            sel(s, g);
            let kind = match k {
                "all" => BorderKind::All,
                "outside" => BorderKind::Outside,
                "thickOutside" => BorderKind::ThickOutside,
                "none" => BorderKind::None,
                "bottom" => BorderKind::Bottom,
                _ => return None,
            };
            s.set_borders(kind, "#000000");
            dash
        }
        "fill" => {
            let (g, c) = rest.split_once(' ')?;
            sel(s, g);
            s.set_format(&StyleChange { fill: Some(if c == "null" { None } else { Some(c.to_owned()) }), ..StyleChange::default() });
            dash
        }
        "merge" => {
            sel(s, rest);
            s.merge(merge);
            dash
        }
        "fill-down" | "fill-right" => {
            sel(s, rest);
            s.fill(op == "fill-down");
            dash
        }
        "clear-contents" | "clear-formats" | "clear-all" => {
            sel(s, rest);
            s.clear(match op {
                "clear-contents" => ClearKind::Contents,
                "clear-formats" => ClearKind::Formats,
                _ => ClearKind::All,
            });
            dash
        }
        "freeze" => {
            let (x, y) = rest.split_once(',')?;
            s.freeze_panes(Some(x.parse().ok()?), Some(y.parse().ok()?));
            dash
        }
        "width" => {
            let (g, w) = rest.split_once(' ')?;
            sel(s, g);
            s.set_column_width(w.parse().ok()?, None);
            dash
        }
        "hide-rows" => {
            sel(s, rest);
            s.set_hidden(false, true);
            dash
        }
        "replaceAll" => {
            // `"q" "r"`
            let q_end = rest[1..].find('"')? + 2;
            let (q, r) = (unquote(&rest[..q_end]), unquote(uo_jsnum::text::trim(&rest[q_end..])));
            s.replace_all(&u(&q), &u(&r), FindOptions::default()).to_string()
        }
        "autosum" => {
            sel(s, rest);
            s.auto_sum("SUM");
            dash
        }
        "copy" => {
            let mut it = rest.split(' ');
            let (g, _, c) = (it.next()?, it.next()?, it.next()?);
            sel(s, g);
            s.copy(false);
            sel(s, c);
            s.paste(None, false);
            dash
        }
        "undo" => s.undo().to_string(),
        "redo" => s.redo().to_string(),
        "recalculate" => {
            s.recalculate();
            dash
        }
        _ => return None,
    })
}

fn sidecar_x1() -> Side {
    sidecar(&dir().join("sheet-x1.side.gz"))
}

#[derive(Default)]
struct T {
    workbooks: usize,
    steps: usize,
    save_equal: usize,
    chg_equal: usize,
    d_equal: usize,
    r_equal: usize,
    merge_tried: usize,
    sh1: usize,
    unknown: Vec<String>,
    sh1_keys: std::collections::BTreeSet<String>,
    d_order: usize,
    mismatches: Vec<String>,
    by_op: std::collections::BTreeMap<String, (usize, usize)>,
}

#[test]
fn x1_edit_steps() {
    let cap = Capture::read(dir().join("sheet-x1.capture")).unwrap();
    let mut t = T::default();
    let side_x1 = sidecar_x1();
    for rec in cap.records() {
        let key = rec.key.as_str();
        let Some(n) = key.strip_prefix("gen/x1/") else { continue };
        if rec.get("e0").is_none() || rec.get("read") != Some("ok") {
            continue;
        }
        let wb = format!("x1/{n}");
        let Some(bytes) = wb_bytes(&wb) else { continue };
        t.workbooks += 1;
        let base = parts_of(&bytes);
        let steps: Vec<String> = (0..).map_while(|k| rec.get(&format!("e{k}")).map(unquote)).collect();
        let mut kinds: Vec<MergeKind> = vec![MergeKind::Center; steps.len()];
        let open = |kinds: &[MergeKind], upto: usize| -> (Session, Vec<Option<String>>) {
            let mut s = open(bytes.clone(), key, "pt-BR");
            let mut res = Vec::new();
            for (k, d) in steps.iter().enumerate().take(upto) {
                res.push(run(&mut s, d, kinds[k]));
            }
            (s, res)
        };
        let (s0, _) = open(&kinds, 0);
        let sh1 = has_sh1(&s0) && {
            let mut differs = false;
            for vi in 0..s0.views.len() {
                if let Some(want) = rec.get(&format!("pv{vi}")) {
                    differs |= list_dig(&store_lines(&s0, vi)) != want;
                }
            }
            differs
        };
        drop(s0);
        let mut prev = base.clone();
        for k in 0..steps.len() {
            let op = steps[k].split(' ').next().unwrap_or("").to_owned();
            let want_save = rec.get(&format!("e{k}.save")).unwrap_or("");
            let attempt = |kinds: &[MergeKind]| {
                let (mut s, res) = open(kinds, k + 1);
                let saved = s.save().map(|b| parts_of(&b)).unwrap_or_default();
                let lines: Vec<String> = saved.iter().map(|(n, d)| format!("{n} {} {}", d.len(), &sha_hex(d)[..32])).collect();
                let displays = display_lines(&mut s);
                if std::env::var_os("UO_EDIT_DUMP").is_some() {
                    for l in &displays {
                        eprintln!("D {key} e{k} {l:?}");
                    }
                }
                (list_dig(&lines), saved, res.last().cloned().flatten(), list_dig(&displays), displays)
            };
            let mut got = attempt(&kinds);
            if op == "merge" && got.0 != want_save {
                for kind in [MergeKind::Across, MergeKind::Cells] {
                    let mut kk = kinds.clone();
                    kk[k] = kind;
                    let g = attempt(&kk);
                    if g.0 == want_save {
                        kinds = kk;
                        got = g;
                        t.merge_tried += 1;
                        break;
                    }
                }
            }
            let (save_dig, saved, res, d_dig, displays) = got;
            t.steps += 1;
            let Some(res) = res else {
                t.unknown.push(format!("{key} e{k}: {}", steps[k]));
                break;
            };
            let e = t.by_op.entry(op.clone()).or_default();
            e.0 += 1;
            let changed: Vec<String> =
                saved.iter().filter(|(n, d)| base.iter().find(|x| &x.0 == n).is_none_or(|x| &x.1 != d)).map(|(n, _)| n.clone()).collect();
            let chg = if changed.is_empty() { "-".to_owned() } else { changed.join(" ") };
            let ok_save = save_dig == want_save;
            let ok_chg = rec.get(&format!("e{k}.chg")) == Some(chg.as_str());
            let mut ok_d = rec.get(&format!("e{k}.d")) == Some(d_dig.as_str());
            // the TS lists the cells in its model's row order (`createCell` binary-searches an unsorted row array when
            // `<row>` elements are out of order); the same lines in another order are the order class C5b-6
            if !ok_d {
                let mut ts: Vec<String> = side_x1
                    .get(key)
                    .and_then(|m| m.get(&format!("e{k}.d")))
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|l| unquote(l))
                    .collect();
                let mut ours = displays.clone();
                ts.sort();
                ours.sort();
                if !ts.is_empty() && ts == ours {
                    ok_d = true;
                    t.d_order += 1;
                }
            }
            // the `views` of an EditResult: a Set in the TS insertion order (the calc's change order; capture-sheet does not
            // sort it, capture-edit does): compared as a set
            let norm = |r: &str| -> String {
                match (r.find("\"views\":["), r.find("],\"grew\"")) {
                    (Some(a), Some(b)) => {
                        let mut v: Vec<u32> = r[a + 9..b].split(',').filter_map(|x| x.parse().ok()).collect();
                        v.sort_unstable();
                        format!("{}{}{}", &r[..a + 9], v.iter().map(ToString::to_string).collect::<Vec<_>>().join(","), &r[b..])
                    }
                    _ => r.to_owned(),
                }
            };
            let ok_r = rec.get(&format!("e{k}.r")).map(norm) == Some(norm(&one_line(&res).chars().take(200).collect::<String>()));
            t.save_equal += usize::from(ok_save);
            t.chg_equal += usize::from(ok_chg);
            t.d_equal += usize::from(ok_d);
            t.r_equal += usize::from(ok_r);
            if ok_save && ok_chg && ok_d && ok_r {
                e.1 += 1;
            } else if sh1 {
                t.sh1 += 1;
                t.sh1_keys.insert(format!("{key} (save {ok_save} chg {ok_chg} d {ok_d} r {ok_r})"));
            } else {
                if std::env::var_os("UO_EDIT_VERBOSE").is_some() && !ok_save {
                    let side = sidecar_x1();
                    let ts_lines = side.get(key).and_then(|m| m.get(&format!("e{k}.save"))).cloned().unwrap_or_default();
                    for (n, d) in &saved {
                        let ours = format!("{n} {} {}", d.len(), &sha_hex(d)[..32]);
                        if !ts_lines.iter().any(|l| unquote(l) == ours || *l == ours) {
                            eprintln!("    {key} e{k} part differs: {ours}");
                            if let Some(x) = side.get(key).and_then(|m| m.get(&format!("e{k}.save:{n}"))).and_then(|v| v.last()) {
                                let ts = unquote(x);
                                let me = String::from_utf8_lossy(d).into_owned();
                                let at = ts.bytes().zip(me.bytes()).position(|(a, b)| a != b).unwrap_or(0);
                                eprintln!(
                                    "      ts:   ...{}
      rust: ...{}",
                                    cut(&ts, at.saturating_sub(100), 260),
                                    cut(&me, at.saturating_sub(100), 260)
                                );
                            }
                        }
                    }
                }
                t.mismatches.push(format!(
                    "{key} e{k} ({}): save {} chg {} d {} r {}{}",
                    steps[k],
                    ok_save,
                    ok_chg,
                    ok_d,
                    ok_r,
                    if ok_r { String::new() } else { format!(" (ts {:?} rust {res:?})", rec.get(&format!("e{k}.r"))) }
                ));
            }
            prev = saved;
        }
        let _ = prev;
    }
    eprintln!(
        "sheet-x1 edit steps: {} workbooks, {} steps; save {} equal, chg {} equal, displays {} equal, results {} equal; merge kind found by trial {}; {} steps on SH1 workbooks; {} unknown; displays equal as sets only (row order, C5b-6) {}",
        t.workbooks,
        t.steps,
        t.save_equal,
        t.chg_equal,
        t.d_equal,
        t.r_equal,
        t.merge_tried,
        t.sh1,
        t.unknown.len(),
        t.d_order
    );
    for (k, (c, e)) in &t.by_op {
        eprintln!("  op {k:16} {c:4} steps {e:4} all equal");
    }
    for m in t.mismatches.iter().take(40) {
        eprintln!("  MISMATCH {m}");
    }
    for m in &t.sh1_keys {
        eprintln!("  SH1 {m}");
    }
    for m in &t.unknown {
        eprintln!("  UNKNOWN {m}");
    }
    assert!(t.workbooks >= 27);
    assert!(t.mismatches.is_empty() && t.unknown.is_empty(), "{} mismatches", t.mismatches.len());
}
