//! The ops of `js/capture-d2b-f3-ops.mjs` run by the Rust port: the same op codes, the same
//! `ret` texts.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use uo_docx::ids::IdAlloc;
use uo_docx::js::JsValue;
use uo_docx::model::MediaStore;
use uo_docx::model::para::Paragraph;
use uo_docx::model::table::Table;
use uo_docx::session::normalize::normalize;
use uo_docx::session::ops::tables::formula::{
    FormulaEnv, default_formula, evaluate_formula, formula_instr, formula_result_text, parse_cell_number,
};
use uo_docx::session::ops::tables::sort::{SortOptions, parse_sort_date, parse_sort_number, sort_table};
use uo_docx::session::ops::tables::table_ops as t_ops;
use uo_docx::session::ops::tables::table_ops::{
    AutoFit, AutoFitMode, CellRect, DeleteShift, InsertShift, NewTableOptions, NormalSpacing, SplitOptions, StyleOpt,
};
use uo_jsnum::format::to_js_string;
use uo_jsnum::parse::to_number;

pub fn hex(t: &str) -> String {
    let h: Vec<String> = t.encode_utf16().map(|u| format!("{u:x}")).collect();
    if h.is_empty() { "-".to_owned() } else { h.join(".") }
}

fn jnum(x: f64) -> String {
    to_js_string(x)
}

fn jopt(x: Option<f64>) -> String {
    x.map_or_else(|| "null".to_owned(), jnum)
}

fn rect_of(f: &[&str], i: usize) -> CellRect {
    let g = |k: usize| to_number(f[i + k]);
    CellRect::new(g(0) as i64, g(1) as i64, g(2) as i64, g(3) as i64)
}

fn rc(r: CellRect) -> String {
    format!("{}:{}:{}:{}", r.r1, r.c1, r.r2, r.c2)
}

pub fn canon_digest(t: &Table, media: &MediaStore) -> String {
    let mut s = crate::support::Sink(uo_testkit::Stream::new());
    let mut w = uo_docx::canon::CanonWriter::new(&mut s).with_media(media);
    t.canon(&mut w);
    s.0.finish().short()
}

pub fn json_digest(t: &Table, ids: &mut IdAlloc, media: &MediaStore) -> String {
    let mut c = t.clone();
    normalize(&mut c, ids);
    canon_digest(&c, media)
}

pub fn paras_digest(ps: &[Paragraph], media: &MediaStore) -> String {
    let mut s = crate::support::Sink(uo_testkit::Stream::new());
    for (i, p) in ps.iter().enumerate() {
        s.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = uo_docx::canon::CanonWriter::new(&mut s).with_media(media);
        p.canon(&mut w);
    }
    s.0.finish().short()
}

fn has_image(t: &Table) -> bool {
    t.rows.iter().any(|r| {
        r.cells.iter().any(|c| {
            c.content.paras.iter().any(|p| {
                p.runs
                    .iter()
                    .any(|x| x.ext().is_some_and(|e| e.image.is_some() || e.textbox.is_some() || e.shape.is_some() || e.group.is_some()))
            }) || c.content.tables.iter().any(|a| has_image(&a.table))
        })
    })
}

const NORMAL: NormalSpacing = NormalSpacing { after: Some(160.0), line: Some(259.0) };

/// Runs one op on `t`; the `ret` text (`err` when the op refuses).
pub fn run(code: &str, t: &mut Table, ids: &mut IdAlloc, media: &MediaStore) -> String {
    let f: Vec<&str> = code.split(':').collect();
    let num = |i: usize| to_number(f[i]);
    let int = |i: usize| num(i) as i64;
    match f[0] {
        "ir" => t_ops::insert_rows(t, rect_of(&f, 1), f[5] == "a", None, ids).map_or("err".into(), |r| format!("r{}", rc(r))),
        "ic" => t_ops::insert_columns(t, rect_of(&f, 1), f[5] == "l", None, ids).map_or("err".into(), |r| format!("r{}", rc(r))),
        "dr" => t_ops::delete_rows(t, int(1), int(2)).map_or("err".into(), |b| if b { "t" } else { "f" }.into()),
        "dc" => t_ops::delete_columns(t, int(1), int(2)).map_or("err".into(), |b| if b { "t" } else { "f" }.into()),
        "dl" => t_ops::delete_cells(t, rect_of(&f, 1), if f[5] == "l" { DeleteShift::Left } else { DeleteShift::Up }, ids)
            .map_or("err".into(), |()| "u".into()),
        "il" => t_ops::insert_cells(t, rect_of(&f, 1), if f[5] == "r" { InsertShift::Right } else { InsertShift::Down }, ids)
            .map_or("err".into(), |r| format!("r{}", rc(r))),
        "mg" => {
            let r = rect_of(&f, 1);
            if !t_ops::can_merge(t, r) {
                return "n".into();
            }
            t_ops::merge_cells(t, r, Some(NORMAL), ids).map_or("err".into(), |r| format!("r{}", rc(r)))
        }
        "sp" => {
            let r = rect_of(&f, 1);
            let hs: Vec<f64> = heights(t.rows.len())
                .into_iter()
                .skip(usize::try_from(r.r1).unwrap())
                .take(usize::try_from(r.r2 - r.r1 + 1).unwrap())
                .collect();
            let o = SplitOptions { merge_first: Some(f[7] == "1"), row_heights: hs, normal_spacing: Some(NORMAL) };
            t_ops::split_cells(t, r, num(5), num(6), &o, ids).map_or("err".into(), |r| format!("r{}", rc(r)))
        }
        "st" => match t_ops::split_table(t, int(1), ids) {
            Some(t2) => format!("t2:{}", canon_digest(&t2, media)),
            None => "null".into(),
        },
        "rh" => {
            t_ops::set_row_height(t, int(1), int(2), num(3), f[4]);
            "u".into()
        }
        "dsr" => {
            let (a, b) = (int(1), int(2));
            let hs: Vec<f64> = heights(t.rows.len()).into_iter().skip(a as usize).take((b - a + 1) as usize).collect();
            t_ops::distribute_rows(t, a, b, &hs);
            "u".into()
        }
        "cw" => t_ops::set_column_width(t, int(1), int(2), num(3)).map_or("err".into(), |()| "u".into()),
        "cc" => t_ops::set_cell_width(t, rect_of(&f, 1), num(5)).map_or("err".into(), |()| "u".into()),
        "pw" => {
            t_ops::set_preferred_width(t, widths(int(1) as usize).as_ref(), 8000.0, ids);
            "u".into()
        }
        "cpct" => {
            t_ops::set_column_pct(t, int(1), int(2), num(3));
            "u".into()
        }
        "dsc" => t_ops::distribute_columns(t, int(1), int(2)).map_or("err".into(), |()| "u".into()),
        "af" => {
            let mode = match f[1] {
                "content" => AutoFitMode::Content,
                "window" => AutoFitMode::Window,
                _ => AutoFitMode::Fixed,
            };
            let a = num(2);
            t_ops::auto_fit(t, mode, if a != 0.0 { Some(a) } else { None });
            "u".into()
        }
        "scp" => {
            t_ops::set_cell_props(t, rect_of(&f, 1), &cell_patch(int(5) as usize), ids);
            "u".into()
        }
        "srp" => {
            t_ops::set_row_props(t, int(1), int(2), &row_patch(int(3) as usize), ids);
            "u".into()
        }
        "stp" => {
            t_ops::set_table_props(t, &tbl_patch(int(1) as usize), ids);
            "u".into()
        }
        "tal" => {
            t_ops::set_table_align(t, JCS[int(1) as usize], ids);
            "u".into()
        }
        "csp" => {
            t_ops::set_cell_spacing(t, Some(num(1)), ids);
            "u".into()
        }
        "tdr" => {
            t_ops::set_text_direction(t, rect_of(&f, 1), DIRS[int(5) as usize], ids);
            "u".into()
        }
        "bd" => {
            let sb = style_borders(int(8) as usize);
            t_ops::set_borders(t, rect_of(&f, 1), kind(f[5]), spec(int(6) as usize).as_ref(), f[7] == "1", sb.as_ref(), ids);
            "u".into()
        }
        "shd" => {
            t_ops::set_shading(t, rect_of(&f, 1), shading(int(5) as usize).as_ref(), f[6] == "1", ids);
            "u".into()
        }
        "lk" => {
            t_ops::set_look(t, &look(int(1) as usize), ids);
            "u".into()
        }
        "t2t" => format!("p{}", paras_digest(&t_ops::table_to_text(t, &sep(int(1) as usize), ids), media)),
        "x2t" => {
            let paras = t_ops::table_to_text(t, &t_ops::TextSeparator::Tab, ids);
            let mut o = NewTableOptions::new(9000.0);
            o.ind_twips = (f[3] == "1").then_some(360.0);
            o.style_id = if f[3] == "2" { StyleOpt::None } else { StyleOpt::Id("Tabelacomgrade".into()) };
            o.autofit = Some([AutoFit::Fixed, AutoFit::Content, AutoFit::Window][int(3) as usize]);
            match t_ops::text_to_table(paras, &sep(int(1) as usize), Some(num(2)), &mut o, ids) {
                Ok(nt) => format!("t{}", canon_digest(&nt, media)),
                Err(_) => "err".into(),
            }
        }
        "srt" => {
            let o = SortOptions { keys: sort_keys(int(3) as usize), case_sensitive: f[4] == "1", separator: Default::default() };
            let r1 = int(1) + i64::from(f[4] == "1");
            if sort_table(t, r1, int(2), &o) { "t".into() } else { "f".into() }
        }
        "fx" => {
            let formula = FORMULAS[int(3) as usize];
            let fu: Vec<u16> = formula.encode_utf16().collect();
            let pic: Option<Vec<u16>> = PICS[int(4) as usize].map(|p| p.encode_utf16().collect());
            let instr = formula_instr(&fu, pic.as_deref());
            let env = FormulaEnv { table: t, r: int(1), c: int(2), bookmarks: None };
            let txt = formula_result_text(&instr, &env).unwrap_or_else(|| "THROW".into());
            let ev = match evaluate_formula(&fu, &env) {
                Err(e) => hex(e),
                Ok(ev) => format!(
                    "{},{},{},{},{}",
                    jnum(ev.value),
                    jnum(ev.fmt.decimals),
                    u8::from(ev.fmt.thousands),
                    hex(ev.fmt.prefix.as_deref().unwrap_or("")),
                    hex(ev.fmt.suffix.as_deref().unwrap_or(""))
                ),
            };
            format!("x{}|{}", hex(&txt), ev)
        }
        "df" => format!("d{}", hex(default_formula(t, int(1), int(2)))),
        "pc" => {
            let u: Vec<u16> = NUMS[int(1) as usize].encode_utf16().collect();
            match parse_cell_number(&u) {
                Some(x) => format!(
                    "n{},{},{},{},{}",
                    jnum(x.n),
                    jnum(x.decimals),
                    u8::from(x.thousands),
                    hex(x.prefix.as_deref().unwrap_or("")),
                    hex(x.suffix.as_deref().unwrap_or(""))
                ),
                None => "null".into(),
            }
        }
        "ps" => {
            let i = int(1) as usize;
            let txt = if i < DATES.len() { DATES[i] } else { NUMS[i - DATES.len()] };
            let u: Vec<u16> = txt.encode_utf16().collect();
            format!("s{},{}", jopt(parse_sort_date(&u)), jopt(parse_sort_number(&u)))
        }
        "clt" => {
            if has_image(t) {
                return "c-img".into();
            }
            format!("c{}", canon_digest(&t_ops::clone_table(t, ids), media))
        }
        "rt" => {
            normalize(t, ids);
            "u".into()
        }
        _ => panic!("op {code}"),
    }
}

const SYN_TEXTS: [&str; 20] = [
    "10",
    "20",
    "5,5",
    "R$ 3,00",
    "abc",
    "",
    "1.234,5",
    "(2)",
    "b",
    "A",
    "á",
    "10%",
    "7/1/2021",
    "2020-01-03",
    "x y",
    "-4",
    "é",
    "Z",
    "1,5",
    "0",
];

/// `makeSynthetic(build)` of the capture: `rows:cols:autofit:ind:style:colw:avail:seed`.
pub fn make_synthetic(build: &str, ids: &mut IdAlloc) -> Table {
    let f: Vec<f64> = build.split(':').map(to_number).collect();
    let (rows, cols, af, ind, style, colw, avail, seed) = (f[0], f[1], f[2] as usize, f[3], f[4] as usize, f[5], f[6], f[7] as usize);
    let mut o = NewTableOptions::new(avail);
    o.ind_twips = (ind != 0.0).then_some(ind);
    o.autofit = Some([AutoFit::Fixed, AutoFit::Content, AutoFit::Window][af]);
    o.style_id = match style {
        1 => StyleOpt::None,
        2 => StyleOpt::Id("Outro1".into()),
        _ => StyleOpt::Default,
    };
    o.col_twips = (colw != 0.0).then_some(colw);
    let mut t = t_ops::new_table(rows, cols, &mut o, ids);
    for (r, row) in t.rows.iter_mut().enumerate() {
        for (c, cell) in row.cells.iter_mut().enumerate() {
            let text = SYN_TEXTS[(r * 7 + c * 3 + seed) % SYN_TEXTS.len()];
            if !text.is_empty() {
                let mut p = Paragraph::empty(ids.para());
                p.runs.push(uo_docx::model::run::Run {
                    text: uo_docx::text::Utf16::from(text),
                    props: std::sync::Arc::new(uo_docx::read::plain::default_run_props()),
                    ..Default::default()
                });
                t_ops::set_cell_content(cell, vec![uo_docx::model::story::OwnedBlock::Paragraph(p)], ids);
            }
        }
    }
    t_ops::sync_legacy(&mut t);
    t
}
