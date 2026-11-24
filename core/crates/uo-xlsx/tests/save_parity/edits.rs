//! The scripted edit steps of the captures (`e<k>`), the ones this crate can replay with the APIs that exist:
//! style changes over a range (`font`, `fill`, `numfmt`: `setFormat` -> `applyStyle`, `universal-sheet.ts:2405-2541`,
//! through the XfPool) and undo/redo that did nothing. The scripts are cumulative (each step starts from the state
//! the previous ones left), so the replay of a workbook stops at the first step that needs the C5 session (merge,
//! fill, paste, find/replace, clear, text and formula entry, width, hide, freeze, undo/redo of a real edit,
//! recalculate): that step and the ones after it are counted as deferred.

use super::*;
use uo_xlsx::save::{FontChange, NumFmtChange, StyleChange, XfPool};

/// `A1` / `AB12` -> (col, row), 1-based.
fn cell_ref(s: &str) -> (u32, u32) {
    let letters: String = s.chars().take_while(char::is_ascii_alphabetic).collect();
    let col = letters.bytes().fold(0u32, |a, b| a * 26 + u32::from(b.to_ascii_uppercase() - b'A') + 1);
    (col, s[letters.len()..].parse().unwrap_or(1))
}

fn range_of(s: &str) -> (u32, u32, u32, u32) {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let ((c1, r1), (c2, r2)) = (cell_ref(a), cell_ref(b));
    (c1, r1, c2, r2)
}

fn json_field<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let i = obj.find(&pat)? + pat.len();
    let rest = &obj[i..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn style_change(book: &mut Book, pool: &mut XfPool, op: &str, arg: &str) -> Option<StyleChange> {
    Some(match op {
        "fill" => StyleChange { fill: Some(if arg == "null" { None } else { Some(arg.to_owned()) }), ..StyleChange::default() },
        "font" => {
            let flag = |k: &str| json_field(arg, k).map(|v| v == "true");
            let color = json_field(arg, "color").map(|v| if v == "null" { None } else { Some(v.trim_matches('"').to_owned()) });
            let size = json_field(arg, "size").and_then(|v| uo_jsnum::parse::parse_float(v).is_finite().then(|| uo_jsnum::parse::parse_float(v)));
            StyleChange { font: Some(FontChange { bold: flag("bold"), italic: flag("italic"), size, color, ..FontChange::default() }), ..StyleChange::default() }
        }
        "numfmt" => {
            let (id, code): (Option<u32>, Option<String>) = if let Some(k) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) {
                match k {
                    "percent" => (Some(10), None),
                    "shortDate" => (Some(14), None),
                    "scientific" => (Some(11), None),
                    "text" => (Some(49), None),
                    "currency" => (None, Some(r#""R$"\ #,##0.00;[Red]\-"R$"\ #,##0.00"#.to_owned())),
                    "comma" => (None, Some(r#"_-* #,##0.00_-;\-* #,##0.00_-;_-* "-"??_-;_-@_-"#.to_owned())),
                    _ => return None,
                }
            } else {
                let c = json_field(arg, "code")?;
                // JSON string: `\"` and `\\` unescaped
                (None, Some(c.trim_matches('"').replace("\\\"", "\"").replace("\\\\", "\\")))
            };
            let nf = match (id, code) {
                (Some(id), _) => NumFmtChange { id: f64::from(id), code: None },
                (None, Some(code)) => NumFmtChange { id: pool.format_id(&mut book.styles, &code), code: Some(code) },
                _ => return None,
            };
            StyleChange { num_fmt: Some(nf), ..StyleChange::default() }
        }
        _ => return None,
    })
}

/// `applyStyle` over the rectangle: each cell takes the derived xf; an absent cell is created, a cell that ends
/// without content or style is dropped.
fn apply_style_step(book: &mut Book, pool: &mut XfPool, sheet: usize, desc: &str) -> bool {
    use uo_calc::store::{CellKind, ValueTag};
    let mut it = desc.splitn(3, ' ');
    let (Some(op), Some(rng), Some(arg)) = (it.next(), it.next(), it.next()) else { return false };
    let Some(change) = style_change(book, pool, op, arg) else { return false };
    let (c1, r1, c2, r2) = range_of(rng);
    let ix = uo_calc::key::SheetIx(sheet as u32);
    for row in r1..=r2 {
        for col in c1..=c2 {
            let (r0, c0) = (row - 1, col - 1);
            let present = book.store.get(ix, r0, c0).is_some();
            let xf = book.store.style_at(ix, r0, c0) as usize;
            let nx = pool.apply(&mut book.styles, xf, &change);
            if nx == xf {
                continue;
            }
            if !present {
                let fc = uo_calc::FileCell { s: if xf == 0 { None } else { Some(xf as f64) }, ..uo_calc::FileCell::default() };
                book.store.set_file_cell(ix, r0, c0, &fc).unwrap();
            }
            if nx == 0 {
                book.store.set_style(ix, r0, c0, None).unwrap();
                let raw = book.store.get(ix, r0, c0).unwrap();
                if !raw.kind.has(CellKind::HAS_VALUE)
                    && !raw.kind.has(CellKind::HAS_FORMULA)
                    && !raw.kind.has(CellKind::HAS_EXTRA)
                    && raw.kind.tag() != ValueTag::Inline
                {
                    book.store.remove(ix, r0, c0);
                }
            } else {
                book.store.set_style(ix, r0, c0, Some(nx as u32)).unwrap();
            }
        }
    }
    true
}

/// Replays the leading steps of a workbook's script and compares the save after each.
pub(super) fn replay_edits(rec: &uo_testkit::capture::Record, key: &str, base: &BTreeMap<String, Vec<u8>>, book: &mut Book, mut dirty: Vec<usize>, t: &mut Totals) {
    let mut pool = XfPool::new(&book.styles);
    // the TS builds its engine lazily (the displays of the capture do): its presence changes what a save writes
    if rec.get("engine") == Some("built") && book.engine.is_none() {
        uo_xlsx::save::engine::build_engine(book, &engine_opts()).unwrap();
    }
    let active = book.active_sheet().unwrap_or(0);
    let mut steps = 0;
    while rec.get(&format!("e{steps}")).is_some() {
        steps += 1;
    }
    for k in 0..steps {
        let desc = serde_like_unquote(rec.get(&format!("e{k}")).unwrap_or(""));
        let result = rec.get(&format!("e{k}.r")).unwrap_or("-");
        let op = desc.split(' ').next().unwrap_or("");
        let nothing_to_undo = matches!(op, "undo" | "redo") && result == "false";
        if !nothing_to_undo {
            if !apply_style_step(book, &mut pool, active, &desc) {
                t.edits_deferred += steps - k;
                return;
            }
            if !dirty.contains(&active) {
                dirty.push(active);
            }
        }
        let Ok(saved) = save(book, &SaveRequest { dirty_sheets: &dirty, pool: Some(&pool), ..SaveRequest::default() }) else {
            t.mismatches.push(format!("{key} e{k}: save failed"));
            return;
        };
        let (lines, changed, _) = parts_of(&saved.bytes, base);
        t.edits_compared += 1;
        let ok1 = rec.get(&format!("e{k}.save")) == Some(list_dig(&lines).as_str());
        let want_chg = rec.get(&format!("e{k}.chg")).unwrap_or("-");
        let ours_chg = if changed.is_empty() { "-".to_owned() } else { changed.join(" ") };
        if ok1 && want_chg == ours_chg {
            t.edits_equal += 1;
        } else if has_sh1(book) {
            t.edits_sh1 += 1;
        } else {
            t.mismatches.push(format!("{key} e{k} ({desc}): save {} chg {}", if ok1 { "ok" } else { "DIFF" }, if want_chg == ours_chg { "ok" } else { "DIFF" }));
        }
    }
}
