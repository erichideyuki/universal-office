//! Slides wave 5: the PowerPoint rules measured in PowerPoint 365 (pt-BR) where the frozen TS differs, each against the
//! measured values (record `docs/modules/uo-dml-render.md` "PowerPoint rules (wave 5)"), and the TS behaviour behind its
//! `ts_*` switch. Measurement decks and scripts: `core/target/w5-a/` (outside git; `gen.py`, `gen2.py`, `an_*.py`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use uo_dml_render::SlidesRules;
use uo_dml_render::datefield::{FieldClock, format_datetime_field};
use uo_dml_render::ids::MediaId;
use uo_dml_render::metafile::rec::{RecipeFx, decode_fx_recipe, encode_fx_recipe};
use uo_dml_render::model::{Paragraph, Run, TextBody};
use uo_dml_render::source::{PartSource, R, parse_xml};
use uo_dml_render::text::{
    LayoutTextOptions, MAX_ROUNDS, MeasureReq, MeasureTable, Measurer, TextMetrics, layout_text_body, need_measures, ppt_line_box,
    split_face,
};
use uo_dml_render::theme::{color_from_dml, read_theme, system_color};
use uo_opc::xml::Document;

// ---------------------------------------------------------------------------------------------- a:sysClr

struct Mem {
    parts: HashMap<String, Arc<[u8]>>,
    rules: SlidesRules,
}

impl PartSource for Mem {
    fn part(&self, name: &str) -> Option<Arc<[u8]>> {
        self.parts.get(name).cloned()
    }
    fn xml(&self, name: &str) -> R<Option<Arc<Document>>> {
        match self.parts.get(name) {
            Some(b) => Ok(Some(Arc::new(parse_xml(String::from_utf8_lossy(b).into_owned())?))),
            None => Ok(None),
        }
    }
    fn media(&self, _part: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _id: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
    fn rules(&self) -> SlidesRules {
        self.rules
    }
}

/// The theme of deck `sys-theme`: dk1/lt1/accent1 are `sysClr` with a bogus `lastClr`.
fn sys_theme(rules: SlidesRules) -> uo_dml_render::Theme {
    let xml = "<a:theme xmlns:a=\"a\"><a:themeElements><a:clrScheme name=\"t\">\
<a:dk1><a:sysClr val=\"windowText\" lastClr=\"FF0000\"/></a:dk1><a:lt1><a:sysClr val=\"window\" lastClr=\"00FF00\"/></a:lt1>\
<a:dk2><a:srgbClr val=\"44546A\"/></a:dk2><a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
<a:accent1><a:sysClr val=\"highlight\" lastClr=\"0000FF\"/></a:accent1></a:clrScheme></a:themeElements></a:theme>";
    let mem = Mem { parts: [("t.xml".to_owned(), Arc::from(xml.as_bytes()))].into_iter().collect(), rules };
    read_theme(&mem, "t.xml").unwrap()
}

fn color(xml: &str, th: &uo_dml_render::Theme) -> String {
    let doc = parse_xml(format!("<x xmlns:a=\"a\">{xml}</x>")).unwrap();
    let el = doc.root().child_elements().next();
    color_from_dml(el, th).map(|c| c.as_str().to_ascii_uppercase()).unwrap_or_default()
}

#[test]
fn sys_clr_draws_the_system_colour_as_powerpoint() {
    // measured (deck sys-1): the 30 values with lastClr="123456", without lastClr, and with lumMod 50 % (PowerPoint's pixels)
    let measured = [
        ("scrollBar", "C8C8C8", "646464"),
        ("background", "000000", "000000"),
        ("activeCaption", "99B4D1", "38597D"),
        ("inactiveCaption", "BFCDDB", "4A6783"),
        ("menu", "F0F0F0", "787878"),
        ("windowFrame", "646464", "323232"),
        ("activeBorder", "B4B4B4", "5A5A5A"),
        ("inactiveBorder", "F4F7FC", "356AC3"),
        ("appWorkspace", "ABABAB", "565656"),
        ("highlight", "0078D7", "003C6C"),
        ("btnFace", "F0F0F0", "787878"),
        ("btnShadow", "A0A0A0", "505050"),
        ("grayText", "6D6D6D", "363636"),
        ("3dDkShadow", "696969", "353535"),
        ("3dLight", "E3E3E3", "727272"),
        ("infoBk", "FFFFE1", "F0F000"),
        ("hotLight", "0066CC", "003366"),
        ("gradientActiveCaption", "B9D1EA", "3068A1"),
        ("gradientInactiveCaption", "D7E4F2", "3870AC"),
        ("menuHighlight", "3399FF", "004D99"),
        ("menuBar", "F0F0F0", "787878"),
        ("windowText", "000000", "000000"),
        ("menuText", "000000", "000000"),
    ];
    let th = sys_theme(SlidesRules::default());
    for (val, plain, dark) in measured {
        assert_eq!(system_color(val), Some(plain), "{val}");
        assert_eq!(color(&format!("<a:sysClr val=\"{val}\" lastClr=\"123456\"/>"), &th), format!("#{plain}"), "{val}");
        assert_eq!(color(&format!("<a:sysClr val=\"{val}\"/>"), &th), format!("#{plain}"), "{val} without lastClr");
        // lumMod over it: within one level (PowerPoint rounds an exact .5 down — window 127.5 → 7F, grayText 54.5 → 36 —
        // where the HSL of `color_from_dml` rounds it up: a lumMod question, not a sysClr one)
        let got = color(&format!("<a:sysClr val=\"{val}\" lastClr=\"123456\"><a:lumMod val=\"50000\"/></a:sysClr>"), &th);
        let ch = |s: &str, i: usize| i32::from_str_radix(&s[1 + 2 * i..3 + 2 * i], 16).unwrap();
        let want = format!("#{dark}");
        assert!((0..3).all(|i| (ch(&got, i) - ch(&want, i)).abs() <= 1), "{val} lumMod: {got} vs {want}");
    }
    // the theme colours that are sysClr (deck sys-theme): tx1 = dk1 = windowText, bg1 = lt1 = window, accent1 = highlight
    assert_eq!(th.colors.get("dk1").unwrap().as_str().to_ascii_uppercase(), "#000000");
    assert_eq!(th.colors.get("tx1").unwrap().as_str().to_ascii_uppercase(), "#000000");
    assert_eq!(th.colors.get("lt1").unwrap().as_str().to_ascii_uppercase(), "#FFFFFF");
    assert_eq!(th.colors.get("accent1").unwrap().as_str().to_ascii_uppercase(), "#0078D7");
    // a value outside the list: lastClr, then black
    assert_eq!(color("<a:sysClr val=\"nope\" lastClr=\"123456\"/>", &th), "#123456");
    assert_eq!(color("<a:sysClr val=\"nope\"/>", &th), "#000000");
}

#[test]
fn sys_clr_takes_last_clr_under_ts_sys_clr() {
    let th = sys_theme(SlidesRules::ts_strict());
    assert_eq!(th.colors.get("dk1").unwrap().as_str().to_ascii_uppercase(), "#FF0000");
    assert_eq!(th.colors.get("accent1").unwrap().as_str().to_ascii_uppercase(), "#0000FF");
    assert_eq!(color("<a:sysClr val=\"highlight\" lastClr=\"123456\"/>", &th), "#123456");
    let only = SlidesRules { ts_sys_clr: true, ..SlidesRules::default() };
    assert_eq!(color("<a:sysClr val=\"window\" lastClr=\"00FF00\"/>", &sys_theme(only)), "#00FF00");
}

// ---------------------------------------------------------------------------------------------- first-line baseline

/// PowerPoint's line box of the measured faces (`ppt_line_box` over each font file's OS/2: win metrics, or typo + gap with
/// USE_TYPO_METRICS), per 1000 px.
fn face_box(family: &str) -> Option<(f64, f64)> {
    let (upem, b) = match family {
        "Liberation Sans" | "Arial" => (2048.0, ppt_line_box(1854, 434, 1491, -431, 307, false)),
        "Liberation Serif" | "Times New Roman" => (2048.0, ppt_line_box(1825, 443, 1420, -442, 307, false)),
        "Liberation Mono" | "Courier New" => (2048.0, ppt_line_box(1705, 615, 1255, -386, 0, false)),
        "Carlito" | "Calibri" => (2048.0, ppt_line_box(1950, 550, 1536, -512, 452, false)),
        "Caladea" => (1000.0, ppt_line_box(950, 222, 778, -222, 172, false)),
        "Liberation Sans Narrow" => (2048.0, ppt_line_box(1888, 431, 1491, -431, 269, true)),
        "Aptos" => (2048.0, ppt_line_box(2068, 563, 1923, -577, 0, true)),
        "Segoe UI" => (2048.0, ppt_line_box(2210, 514, 1491, -431, 269, false)),
        "Poppins" => (1000.0, ppt_line_box(1135, 627, 1050, -350, 100, true)),
        "Gabriola" => (4096.0, ppt_line_box(4880, 2660, 2800, -1296, 2867, true)),
        _ => return None,
    };
    Some((b.0 * 1000.0 / upem, b.1 * 1000.0 / upem))
}

fn answer(t: &mut MeasureTable, need: &[MeasureReq]) {
    for r in need {
        match r {
            MeasureReq::CharAt1000 { face, ch } => t.set_char_at_1000(face, *ch, if *ch == ' ' { 250.0 } else { 500.0 }),
            MeasureReq::KernAt1000 { face, text } => t.set_kern_at_1000(face, text, 1000.0, 1000.0),
            MeasureReq::Width(k) => t.set_width(k, k.size_px.value() * 0.5 * k.text.chars().count() as f64),
            MeasureReq::Metrics(k) => {
                let (fam, _) = split_face(&k.face.family);
                let px = k.size_px.value() / 1000.0;
                let (a, d) = face_box(fam).map_or((None, None), |(a, d)| (Some(a * px), Some(d * px)));
                t.set_metrics(k, TextMetrics { width: 0.0, font_ascent: a, font_descent: d, ..TextMetrics::default() });
            }
            _ => {}
        }
    }
}

fn run(text: &str, font: &str, sz: f64) -> Run {
    Run { text: text.into(), font: Some(font.into()), sz: Some(sz), ..Run::default() }
}

fn lines_of(paras: Vec<Paragraph>, reduction: Option<f64>, rules: SlidesRules) -> Vec<f64> {
    let mut tb = TextBody { paragraphs: paras, ..TextBody::default() };
    tb.ins_l = Some(0.0);
    tb.ins_t = Some(0.0);
    tb.ins_r = Some(0.0);
    tb.ins_b = Some(0.0);
    tb.wrap = Some("none".into());
    if reduction.is_some() {
        tb.x_mut().ln_spc_reduction = reduction;
    }
    let o = LayoutTextOptions { rules, ..LayoutTextOptions::default() };
    let mut t = MeasureTable::default();
    for _ in 0..MAX_ROUNDS {
        let need = need_measures(&tb, 1e8, 1e8, Measurer::Ppt, &t, &o);
        if need.is_empty() {
            break;
        }
        answer(&mut t, &need);
    }
    let r = layout_text_body(&tb, 1e8, 1e8, Measurer::Ppt, &t, &o).unwrap();
    r.lines.iter().map(|l| l.baseline / 12700.0).collect()
}

/// The baselines (pt from the box top) of a box of two 36-pt paragraphs `Hxg` in `font` (decks bl-*, b2-*).
fn two_lines(font: &str, spacing: Option<f64>, exact: Option<f64>, reduction: Option<f64>, rules: SlidesRules) -> Vec<f64> {
    let p = || {
        let mut p = Paragraph { runs: vec![run("Hxg", font, 36.0)], ..Paragraph::default() };
        p.line_spacing = spacing;
        p.line_spacing_pt = exact;
        p
    };
    lines_of(vec![p(), p()], reduction, rules)
}

/// PowerPoint's PDF places glyphs up to 0.0035 em lower than the rule (a drift that grows down the page, the same for
/// every font and spacing): the tolerance of the comparisons with the measured values.
const TOL_EM: f64 = 0.0045;

#[test]
fn single_spacing_first_baseline_follows_the_font_metrics() {
    // measured: the first baseline / size of a 100 % line (bl-a*, b2-gap; 1 = bold measured alike)
    let measured = [
        ("Liberation Sans", 0.9739),
        ("Liberation Serif", 0.9672),
        ("Liberation Mono", 0.8839),
        ("Carlito", 0.9372),
        ("Caladea", 0.9744),
        ("Liberation Sans Narrow", 0.9639),
        ("Aptos", 0.9258),
        ("Calibri", 0.9372),
        ("Arial", 0.9744),
        ("Times New Roman", 0.9678),
        ("Courier New", 0.8844),
        ("Segoe UI", 0.9744),
        ("Poppins", 0.9206),
        ("Gabriola", 0.9772),
    ];
    for (font, want) in measured {
        let b = two_lines(font, None, None, None, SlidesRules::default());
        let got = b[0] / 36.0;
        assert!((got - want).abs() <= TOL_EM, "{font}: {got:.4} em, PowerPoint {want:.4}");
        // the next line: 1.2 × the size below (every font)
        assert!(((b[1] - b[0]) / 36.0 - 1.2).abs() < 1e-9, "{font}: pitch");
        // the TS: 0.96 em for every font
        let ts = two_lines(font, None, None, None, SlidesRules::ts_strict());
        assert!((ts[0] / 36.0 - 0.96).abs() < 1e-9, "{font}: TS");
    }
}

#[test]
fn reduced_expanded_and_exact_spacing_as_powerpoint() {
    let fonts = ["Aptos", "Calibri", "Liberation Serif", "Liberation Sans", "Liberation Mono"];
    // measured (decks b2-p90, b2-p80, b2-red20, b2-p110, b2-x30, bl-s0, bl-x0): first baseline / 36 pt
    type Case = (&'static str, Option<f64>, Option<f64>, Option<f64>, [f64; 5]);
    let cases: [Case; 7] = [
        ("90 %", Some(0.9), None, None, [0.8106, 0.8172, 0.8472, 0.8539, 0.7944]),
        ("80 %", Some(0.8), None, None, [0.7206, 0.7206, 0.7272, 0.7339, 0.7044]),
        ("lnSpcReduction 20 %", None, None, Some(0.2), [0.7206, 0.7206, 0.7272, 0.7339, 0.7044]),
        ("110 %", Some(1.1), None, None, [0.9906, 0.9906, 0.9906, 0.9906, 0.9744]),
        ("150 %", Some(1.5), None, None, [1.3524, 1.3517, 1.3506, 1.3506, 1.3339]),
        ("exactly 30 pt", None, Some(30.0), None, [0.6272, 0.6272, 0.6272, 0.6272, 0.6078]),
        ("exactly 50 pt", None, Some(50.0), None, [1.0458, 1.0444, 1.0439, 1.0439, 1.0239]),
    ];
    for (name, pct, pts, red, want) in cases {
        for (font, w) in fonts.iter().zip(want) {
            let b = two_lines(font, pct, pts, red, SlidesRules::default());
            let got = b[0] / 36.0;
            assert!((got - w).abs() <= TOL_EM, "{name} {font}: {got:.4} em, PowerPoint {w:.4}");
        }
    }
}

#[test]
fn mixed_fonts_and_sizes_take_the_largest_ascent_and_descent() {
    // measured (deck b2-mix, one 100 % line): the baseline in pt from the box top
    let cases = [
        ("Liberation Serif", 40.0, "Aptos", 20.0, 38.650),
        ("Aptos", 40.0, "Liberation Serif", 20.0, 37.000),
        ("Courier New", 20.0, "Calibri", 40.0, 37.490),
        ("Calibri", 20.0, "Courier New", 40.0, 35.340),
        ("Segoe UI", 18.0, "Times New Roman", 36.0, 34.888),
        ("Liberation Mono", 30.0, "Caladea", 30.0, 27.216),
    ];
    for (a, sa, b, sb, want) in cases {
        let p = Paragraph { runs: vec![run("Hx", a, sa), run("Hx", b, sb)], ..Paragraph::default() };
        let got = lines_of(vec![p], None, SlidesRules::default())[0];
        // the PDF drift of the measured rows reaches 0.13 pt at the bottom of the page
        assert!((got - want).abs() <= 0.16, "{a} {sa} + {b} {sb}: {got:.3} pt, PowerPoint {want:.3}");
    }
}

#[test]
fn an_unanswered_face_box_uses_calibri_metrics() {
    // PowerPoint draws an absent family with Calibri's metrics: 0.936 em on a single line
    let b = two_lines("No Such Family", None, None, None, SlidesRules::default());
    assert!((b[0] / 36.0 - 1.2 * 1950.0 / 2500.0).abs() < 1e-9);
}

// ---------------------------------------------------------------------------------------------- date and time fields

#[test]
fn datetime_fields_match_powerpoint_in_16_languages() {
    // PowerPoint's output (decks d2-*, 2026-11-18, a Wednesday; the seconds of each column are those of its export)
    let tsv = include_str!("data/ppt_datetime_fields.tsv");
    let rows: Vec<Vec<&str>> = tsv.lines().map(|l| l.split('\t').collect()).collect();
    let header = &rows[0];
    let time_row = rows.iter().find(|r| r[0] == "datetime11").unwrap();
    let mut checked = 0;
    for (col, lang) in header.iter().enumerate().skip(1) {
        let hms: Vec<u32> = time_row[col].split(':').map(|v| v.parse().unwrap()).collect();
        let clock =
            FieldClock { year: 2026, month: 11, day: 18, hour: hms[0], minute: hms[1], second: hms[2], default_lang: "pt-BR".into() };
        let lang = (*lang != "none").then_some(*lang);
        for r in &rows[1..] {
            let got = format_datetime_field(r[0], lang, &clock).unwrap();
            assert_eq!(got, r[col], "{} {:?}", r[0], lang);
            checked += 1;
        }
    }
    assert_eq!(checked, 17 * 14);
    // the TS draws the cached text: no format at all; a type outside the list is no date field
    let clock = FieldClock { year: 2026, month: 1, day: 5, hour: 13, minute: 7, second: 9, default_lang: "en-US".into() };
    assert_eq!(format_datetime_field("slidenum", None, &clock), None);
    assert_eq!(format_datetime_field("datetime14", None, &clock), None);
    // other dates of the same patterns (not measurable: PowerPoint shows today) and unknown languages
    assert_eq!(format_datetime_field("datetime1", Some("en-US"), &clock).unwrap(), "1/5/2026");
    assert_eq!(format_datetime_field("datetime2", Some("pt-BR"), &clock).unwrap(), "segunda-feira, 5 de janeiro de 2026");
    assert_eq!(format_datetime_field("datetime12", Some("en-US"), &clock).unwrap(), "1:07 PM");
    assert_eq!(format_datetime_field("datetime1", Some("xx-YY"), &clock).unwrap(), "1/5/2026");
    assert_eq!(format_datetime_field("datetime1", Some("de-AT"), &clock).unwrap(), "05.01.2026");
    assert_eq!(format_datetime_field("datetimeFigureOut", None, &clock).unwrap(), "1/5/2026");
}

// ---------------------------------------------------------------------------------------------- derived recipes

#[test]
fn derived_recipes_round_trip() {
    let fx = RecipeFx {
        duotone: Some(["#1F3864".into(), "#FFE699".into()]),
        gray: true,
        bi_level: Some(0.5),
        bright: Some(0.3),
        contrast: Some(-0.2),
        clr_change: Some(("#FF0000".into(), "#00FF00".into(), 0.0)),
    };
    let b = encode_fx_recipe(7, 200, 100, &fx);
    assert_eq!(&b[..5], b"UOMF\x02");
    assert_eq!(decode_fx_recipe(&b), Some((7, fx.clone())));
    let only = RecipeFx { gray: true, ..RecipeFx::default() };
    assert_eq!(decode_fx_recipe(&encode_fx_recipe(0, 1, 1, &only)), Some((0, only)));
    // truncated or foreign bytes: no derived recipe
    for n in 0..b.len() {
        assert_eq!(decode_fx_recipe(&b[..n]), None, "cut at {n}");
    }
    assert_eq!(decode_fx_recipe(b"UOMF\x01aaaaaaaaaaaaaaaaaaaa"), None);
}

// ---------------------------------------------------------------------------------------------- fallback context

#[test]
fn a_run_fallback_context_rides_in_the_face_name() {
    use uo_dml_render::model::{FontFallback, RunExtras};
    let fb = FontFallback {
        lang: Some("ja-JP".into()),
        ea: "".into(),
        ea_explicit: false,
        cs: "".into(),
        major: true,
        scripts: vec![("Jpan".into(), "Yu Gothic".into())],
    };
    let mut r = run("日本語", "Aptos", 18.0);
    r.extras = Some(Box::new(RunExtras { font_fallback: Some(Box::new(fb.clone())), ..RunExtras::default() }));
    let mut tb = TextBody { paragraphs: vec![Paragraph { runs: vec![r], ..Paragraph::default() }], ..TextBody::default() };
    tb.wrap = Some("none".into());
    let o = LayoutTextOptions::default();
    let mut t = MeasureTable::default();
    for _ in 0..MAX_ROUNDS {
        let need = need_measures(&tb, 1e8, 1e8, Measurer::Ppt, &t, &o);
        if need.is_empty() {
            break;
        }
        // every request carries the context in its face
        for q in &need {
            if let MeasureReq::CharAt1000 { face, .. } = q {
                assert_eq!(split_face(&face.family), ("Aptos", Some(fb.clone())));
            }
        }
        answer(&mut t, &need);
    }
    let lay = layout_text_body(&tb, 1e8, 1e8, Measurer::Ppt, &t, &o).unwrap();
    let fam = &lay.lines[0].runs[0].font.family;
    assert_eq!(split_face(fam), ("Aptos", Some(fb)));
    assert_eq!(uo_dml_render::text::encode_face("Aptos", &split_face(fam).1.unwrap()), fam.to_string());
    assert_eq!(split_face("Aptos"), ("Aptos", None));
}

/// Baselines (pt from the box top) of two 36-pt paragraphs in a 160 x 120 pt box anchored `anchor` (decks b3-foot-*).
fn anchored(font: &str, spacing: Option<f64>, exact: Option<f64>, anchor: &str) -> Vec<f64> {
    let p = || {
        let mut p = Paragraph { runs: vec![run("Hxg", font, 36.0)], ..Paragraph::default() };
        p.line_spacing = spacing;
        p.line_spacing_pt = exact;
        p
    };
    let mut tb = TextBody { paragraphs: vec![p(), p()], ..TextBody::default() };
    (tb.ins_l, tb.ins_t, tb.ins_r, tb.ins_b) = (Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    tb.wrap = Some("none".into());
    tb.anchor = Some(anchor.into());
    let o = LayoutTextOptions::default();
    let (w, h) = (160.0 * 12700.0, 120.0 * 12700.0);
    let mut t = MeasureTable::default();
    for _ in 0..MAX_ROUNDS {
        let need = need_measures(&tb, w, h, Measurer::Ppt, &t, &o);
        if need.is_empty() {
            break;
        }
        answer(&mut t, &need);
    }
    layout_text_body(&tb, w, h, Measurer::Ppt, &t, &o).unwrap().lines.iter().map(|l| l.baseline / 12700.0).collect()
}

#[test]
fn the_block_ends_one_descent_below_the_last_baseline() {
    // measured (b3-foot-b, b3-foot-ctr): baselines / 36 pt from the top of a 120-pt box
    type Case = (&'static str, &'static str, Option<f64>, Option<f64>, [f64; 2]);
    let cases: [Case; 16] = [
        ("b", "Aptos", None, None, [1.8572, 3.0578]),
        ("b", "Aptos", Some(0.8), None, [2.0972, 3.0572]),
        ("b", "Aptos", Some(1.5), None, [1.2572, 3.0572]),
        ("b", "Aptos", None, Some(50.0), [1.6697, 3.0564]),
        ("b", "Liberation Mono", None, None, [1.8183, 3.0183]),
        ("b", "Liberation Mono", Some(1.5), None, [1.2183, 3.0183]),
        ("b", "Liberation Serif", Some(0.8), None, [2.1424, 3.1031]),
        ("b", "Liberation Serif", None, Some(50.0), [1.7144, 3.1018]),
        ("ctr", "Aptos", None, None, [1.3906, 2.5911]),
        ("ctr", "Aptos", Some(0.8), None, [1.4083, 2.3692]),
        ("ctr", "Aptos", Some(1.5), None, [1.3039, 3.1044]),
        ("ctr", "Aptos", None, Some(50.0), [1.3564, 2.7439]),
        ("ctr", "Liberation Mono", Some(1.5), None, [1.2764, 3.0772]),
        ("ctr", "Liberation Mono", Some(0.8), None, [1.3817, 2.3417]),
        ("ctr", "Liberation Serif", None, None, [1.4358, 2.6364]),
        ("ctr", "Liberation Serif", None, Some(50.0), [1.3804, 2.7671]),
    ];
    for (anchor, font, pct, pts, want) in cases {
        let got = anchored(font, pct, pts, anchor);
        for (g, w) in got.iter().zip(want) {
            // the third row of the deck (y 278 pt) carries the PDF's largest drift
            assert!((g / 36.0 - w).abs() <= 0.005, "{anchor} {font} {pct:?} {pts:?}: {:.4} em, PowerPoint {w:.4}", g / 36.0);
        }
    }
}

#[test]
fn mixed_faces_under_reduced_and_expanded_spacing() {
    // measured (b3-mix-80, -150, -x50): the first baseline in pt from the box top
    type Spacing = (f64, Option<f64>, Option<f64>, [f64; 4]);
    let cases: [Spacing; 3] = [
        (80.0, Some(0.8), None, [29.05, 28.85, 21.66, 28.88]),
        (150.0, Some(1.5), None, [54.01, 54.05, 40.62, 54.104]),
        (50.0, None, Some(50.0), [37.57, 37.61, 37.62, 37.64]),
    ];
    let mix = [
        ("Liberation Serif", 40.0, "Aptos", 20.0),
        ("Courier New", 20.0, "Calibri", 40.0),
        ("Liberation Mono", 30.0, "Caladea", 30.0),
        ("Aptos", 40.0, "Liberation Mono", 20.0),
    ];
    for (name, pct, pts, want) in cases {
        for ((a, sa, b, sb), w) in mix.iter().zip(want) {
            let mut p = Paragraph { runs: vec![run("Hx", a, *sa), run("Hx", b, *sb)], ..Paragraph::default() };
            p.line_spacing = pct;
            p.line_spacing_pt = pts;
            let got = lines_of(vec![p], None, SlidesRules::default())[0];
            assert!((got - w).abs() <= 0.16, "{name}: {a} {sa} + {b} {sb}: {got:.3} pt, PowerPoint {w:.3}");
        }
    }
}

#[test]
fn known_faces_need_no_font_box_from_the_shell() {
    // the faces of the built-in table are placed alike by every shell: no Metrics question for them
    let p = Paragraph { runs: vec![run("Hxg", "Liberation Sans Narrow", 36.0)], ..Paragraph::default() };
    let tb = TextBody { paragraphs: vec![p], ..TextBody::default() };
    let o = LayoutTextOptions::default();
    let mut t = MeasureTable::default();
    for _ in 0..MAX_ROUNDS {
        let need = need_measures(&tb, 1e8, 1e8, Measurer::Ppt, &t, &o);
        assert!(!need.iter().any(|q| matches!(q, MeasureReq::Metrics(_))), "{need:?}");
        if need.is_empty() {
            break;
        }
        answer(&mut t, &need);
    }
    assert_eq!(uo_dml_render::text::known_line_box("\"Liberation Sans Narrow\""), Some((1760.0, 431.0)));
    assert_eq!(uo_dml_render::text::known_line_box("No Such"), None);
}
