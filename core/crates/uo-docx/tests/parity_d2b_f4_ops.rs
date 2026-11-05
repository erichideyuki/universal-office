//! Parity of the pure model files of D2b-F4 against the frozen TS (record "D2b frontier briefs",
//! `docs/modules/uo-docx-d2b/F4.md`): replays `tests/parity/d2b-f4-ops.capture`, written by
//! `js/capture-d2b-f4-ops.mjs` — `columnsFor`, `presetColumns`, `columnsPresetOf`, `columnWidths`,
//! `maxColumns`, `contentWidth` and `setOrientation` (section-ops.ts), `lineNumberingInfo`,
//! `withLineNumberMode` and `lineNumberingFromOptions` (text-layout.ts), `gradientGeometry` and
//! `pageFillFrom` for the gradients (page-design.ts). Every result is compared as the TS prints it
//! (JSON, so the key order of the objects is part of it).
//!
//! Recapture: `UO_TS_ROOT=<built checkout of main> node core/crates/uo-docx/js/capture-d2b-f4-ops.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::js::{JsNumber, JsonWriter};
use uo_docx::model::media::MediaStore;
use uo_docx::model::section::{ColWidth, Cols, LineNumbering, SectPr};
use uo_docx::session::ops::page::{GradientStyle, PageFillInput, PageLang, gradient_geometry, page_fill_from};
use uo_docx::session::ops::sections::section_ops::{
    ColumnPreset, ColumnSpec, Orientation, column_widths, columns_for, columns_preset_of, content_width, max_columns, preset_columns,
    set_orientation,
};
use uo_docx::session::ops::text_layout::{
    LineNumberMemory, LineNumberMode, LineNumberingOptions, LineRestart, line_numbering_from_options, line_numbering_info,
    with_line_number_mode,
};
use uo_jsnum::json::number;
use uo_jsnum::parse::to_number;
use uo_testkit::{Capture, Replay};

fn js(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn jn(x: f64) -> String {
    number(x)
}

fn json_of(v: &dyn uo_docx::js::JsValue) -> String {
    JsonWriter::stringify(v, None).unwrap_or_else(|| "undefined".to_owned())
}

fn json_opt<T: uo_docx::js::JsValue>(v: Option<&T>) -> String {
    v.map_or_else(|| "undefined".to_owned(), |v| json_of(v))
}

/// `enc(sp)`: `pw,ph,ml,mr,mt,mb[;count,space,sep[;w/s,w/s]]`.
fn sect(enc: &str) -> SectPr {
    let mut parts = enc.split(';');
    let g: Vec<f64> = parts.next().unwrap().split(',').map(to_number).collect();
    let mut sp = SectPr {
        page_width_twips: JsNumber::Num(g[0]),
        page_height_twips: JsNumber::Num(g[1]),
        margin_left_twips: JsNumber::Num(g[2]),
        margin_right_twips: JsNumber::Num(g[3]),
        margin_top_twips: JsNumber::Num(g[4]),
        margin_bottom_twips: JsNumber::Num(g[5]),
        ..SectPr::default()
    };
    if let Some(c) = parts.next() {
        let f: Vec<&str> = c.split(',').collect();
        let mut cols = Cols {
            count: JsNumber::Num(to_number(f[0])),
            space_twips: JsNumber::Num(to_number(f[1])),
            sep: f[2] == "1",
            ..Cols::default()
        };
        if let Some(w) = parts.next() {
            let ws: Vec<ColWidth> = w
                .split(',')
                .map(|x| {
                    let (a, b) = x.split_once('/').unwrap();
                    ColWidth { width_twips: JsNumber::Num(to_number(a)), space_twips: JsNumber::Num(to_number(b)), ..ColWidth::default() }
                })
                .collect();
            uo_docx::word::obj::set(&mut cols, "widths", |c| c.widths = Some(ws));
        }
        uo_docx::word::obj::set(&mut sp, "cols", |s| s.cols = Some(cols));
    }
    sp
}

fn spec_json(s: &ColumnSpec, kind: &str) -> String {
    // `presetColumns`'s literals: one { count, sep }; two/three { count, spaceTwips, sep };
    // left/right { count, spaceTwips, sep, widths }
    let mut o = format!("{{\"count\":{}", jn(s.count));
    if kind != "one" {
        o.push_str(&format!(",\"spaceTwips\":{}", jn(s.space_twips.unwrap_or(f64::NAN))));
    }
    o.push_str(&format!(",\"sep\":{}", s.sep));
    if let Some(w) = &s.widths {
        let v: Vec<String> = w.iter().map(|(a, b)| format!("{{\"widthTwips\":{},\"spaceTwips\":{}}}", jn(*a), jn(*b))).collect();
        o.push_str(&format!(",\"widths\":[{}]", v.join(",")));
    }
    o.push('}');
    o
}

fn preset_of(p: &str) -> ColumnPreset {
    match p {
        "one" => ColumnPreset::One,
        "two" => ColumnPreset::Two,
        "three" => ColumnPreset::Three,
        "left" => ColumnPreset::Left,
        _ => ColumnPreset::Right,
    }
}

fn preset_name(p: Option<ColumnPreset>) -> &'static str {
    match p {
        None => "null",
        Some(ColumnPreset::One) => "one",
        Some(ColumnPreset::Two) => "two",
        Some(ColumnPreset::Three) => "three",
        Some(ColumnPreset::Left) => "left",
        Some(ColumnPreset::Right) => "right",
    }
}

fn ln_decode(s: &str) -> Option<LineNumbering> {
    if s == "-" {
        return None;
    }
    let f: Vec<&str> = s.split(',').collect();
    let mut l = LineNumbering::default();
    let n = |x: &str| JsNumber::Num(to_number(x));
    if f[0] != "-" {
        uo_docx::word::obj::set(&mut l, "countBy", |o| o.count_by = Some(n(f[0])));
    }
    if f[1] != "-" {
        uo_docx::word::obj::set(&mut l, "start", |o| o.start = Some(n(f[1])));
    }
    if f[2] != "-" {
        uo_docx::word::obj::set(&mut l, "distance", |o| o.distance = Some(n(f[2])));
    }
    if f[3] != "-" {
        uo_docx::word::obj::set(&mut l, "restart", |o| o.restart = Some(f[3].into()));
    }
    Some(l)
}

fn gs(i: usize) -> GradientStyle {
    [
        GradientStyle::Horizontal,
        GradientStyle::Vertical,
        GradientStyle::DiagonalUp,
        GradientStyle::DiagonalDown,
        GradientStyle::FromCorner,
        GradientStyle::FromCenter,
    ][i]
}

#[test]
fn d2b_f4_pure_files_match_the_ts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    let cap = Capture::read(dir.join("d2b-f4-ops.capture")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut n = 0;
    for rec in cap.records() {
        let key = rec.key.as_str();
        let Some(input) = cap.get(key, "in") else { continue };
        let out = if key.starts_with("cols#") {
            let (spc, specs) = input.split_once('|').unwrap();
            let sp = sect(spc);
            let f: Vec<&str> = specs.split(',').collect();
            let widths = (f.len() > 3)
                .then(|| f[3].split('/').collect::<Vec<_>>().chunks(2).map(|c| (to_number(c[0]), to_number(c[1]))).collect::<Vec<_>>());
            let spec = ColumnSpec { count: to_number(f[0]), space_twips: (f[1] != "-").then(|| to_number(f[1])), widths, sep: f[2] == "1" };
            format!("{}|{}|{}", json_opt(columns_for(&sp, &spec).as_ref()), js(max_columns(&sp)), js(content_width(&sp)))
        } else if let Some(rest) = key.strip_prefix("preset#") {
            let kind = rest.split_once(':').unwrap().1;
            let sp = sect(input);
            let spec = preset_columns(&sp, preset_of(kind));
            format!("{}|{}", spec_json(&spec, kind), json_opt(columns_for(&sp, &spec).as_ref()))
        } else if key.starts_with("presetof#") {
            let sp = sect(input);
            let w: Vec<String> =
                column_widths(&sp).iter().map(|(a, b)| format!("{{\"widthTwips\":{},\"spaceTwips\":{}}}", jn(*a), jn(*b))).collect();
            format!("{}|[{}]", preset_name(columns_preset_of(&sp)), w.join(","))
        } else if let Some(rest) = key.strip_prefix("orient#") {
            let kind = rest.split_once(':').unwrap().1;
            let mut sp = sect(input);
            set_orientation(&mut sp, if kind == "landscape" { Orientation::Landscape } else { Orientation::Portrait });
            [
                sp.page_width_twips,
                sp.page_height_twips,
                sp.margin_top_twips,
                sp.margin_right_twips,
                sp.margin_bottom_twips,
                sp.margin_left_twips,
            ]
            .iter()
            .map(|x| js(x.to_number()))
            .collect::<Vec<_>>()
            .join(",")
        } else if key.starts_with("lninfo#") {
            let sp = SectPr { ln_num_type: ln_decode(input), ..SectPr::default() };
            let i = line_numbering_info(&sp);
            let mode = match i.mode {
                LineNumberMode::None => "none",
                LineNumberMode::Continuous => "continuous",
                LineNumberMode::Page => "page",
                LineNumberMode::Section => "section",
            };
            format!(
                "{{\"mode\":\"{mode}\",\"startAt\":{},\"countBy\":{},\"distanceTwips\":{}}}",
                jn(i.start_at),
                jn(i.count_by),
                jn(i.distance_twips)
            )
        } else if let Some(rest) = key.strip_prefix("lnmode#") {
            let mode = match rest.split_once(':').unwrap().1 {
                "none" => LineNumberMode::None,
                "continuous" => LineNumberMode::Continuous,
                "page" => LineNumberMode::Page,
                _ => LineNumberMode::Section,
            };
            let (l, m) = input.split_once('|').unwrap();
            let sp = SectPr { ln_num_type: ln_decode(l), ..SectPr::default() };
            let memory = (m != "x").then(|| {
                let (a, b) = m.split_once(',').unwrap();
                LineNumberMemory { start: (a != "-").then(|| to_number(a)), distance: (b != "-").then(|| to_number(b)) }
            });
            json_opt(with_line_number_mode(&sp, mode, memory).as_ref())
        } else if key.starts_with("lnopt#") {
            let f: Vec<&str> = input.split(',').collect();
            let o = LineNumberingOptions {
                on: f[0] == "1",
                start_at: to_number(f[1]),
                count_by: to_number(f[2]),
                distance_twips: to_number(f[3]),
                restart: match f[4] {
                    "page" => LineRestart::Page,
                    "section" => LineRestart::Section,
                    _ => LineRestart::Continuous,
                },
            };
            json_opt(line_numbering_from_options(&o).as_ref())
        } else if key.starts_with("geo#") {
            let (si, v) = input.split_once(',').unwrap();
            json_of(&gradient_geometry(gs(si.parse().unwrap()), to_number(v)))
        } else if key.starts_with("fill#") {
            let f: Vec<&str> = input.split(',').collect();
            let style = gs(f[3].parse().unwrap());
            let variant = to_number(f[4]);
            let fill = match f[0] {
                "0" => PageFillInput::GradientOne { color: f[1].into(), degree: to_number(f[2]), style, variant },
                "1" => PageFillInput::GradientTwo { color: f[1].into(), color2: f[2].into(), style, variant },
                _ => PageFillInput::GradientPreset { preset: f[1].into(), style, variant },
            };
            let mut media = MediaStore::new();
            page_fill_from(&fill, PageLang::En, &mut media).map_or_else(|| "null".to_owned(), |b| json_of(&b))
        } else {
            panic!("case {key}");
        };
        replay.check(key, "out", &out);
        replay.skip(key, "in");
        n += 1;
    }
    let report = replay.finish();
    println!("d2b-f4-ops: {n} cases, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(30));
    }
    report.assert_clean();
}
