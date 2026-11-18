//! Frontier B3 unit, memory and robustness tests of the text layout (`uo_dml_render::text`): `formatAutoNum`, the
//! measurement protocol (two phases, capabilities, the table caps), M1 (live bytes back after every operation), M5
//! (the table's cap filled past it) and bounded no-panic over random bodies with hostile measure tables
//! (`UO_DML_LONG=1` runs 20× longer). Parity with the TS is `core/crates/uo-pptx/tests/text_layout.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use uo_dml_render::model::{Paragraph, Run, RunExtras, TabStop, TableCell, TextBody};
use uo_dml_render::text::{
    Bits, FaceKey, Kerning, LayoutError, LayoutTextOptions, MAX_ROUNDS, MeasureReq, MeasureTable, Measurer, StringKey, cell_body,
    format_auto_num, layout_text_body, layout_text_body_traced, need_measures,
};

fn run(text: &str) -> Run {
    Run { text: text.into(), ..Run::default() }
}

fn body(paras: Vec<Paragraph>) -> TextBody {
    TextBody { paragraphs: paras, ..TextBody::default() }
}

fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph { runs, ..Paragraph::default() }
}

/// Answers every question with a fixed advance of 500/1000 em per character and string widths of 0.5 em per char.
fn answer_all(t: &mut MeasureTable, need: &[MeasureReq]) {
    for r in need {
        match r {
            MeasureReq::CharAt1000 { face, ch } => t.set_char_at_1000(face, *ch, if *ch == ' ' { 250.0 } else { 500.0 }),
            MeasureReq::KernAt1000 { face, text } => t.set_kern_at_1000(face, text, 1000.0, 990.0),
            MeasureReq::Width(k) => t.set_width(k, k.size_px.value() * 0.5 * k.text.chars().count() as f64),
            _ => {}
        }
    }
}

fn settle(tb: &TextBody, w: f64, h: f64, m: Measurer, t: &mut MeasureTable, o: &LayoutTextOptions) -> u32 {
    let mut rounds = 0;
    loop {
        let need = need_measures(tb, w, h, m, t, o);
        if need.is_empty() {
            return rounds;
        }
        rounds += 1;
        assert!(rounds <= MAX_ROUNDS);
        answer_all(t, &need);
    }
}

#[test]
fn format_auto_num_schemes() {
    let cases = [
        ("arabicPeriod", 3, "3."),
        ("arabicParenR", 12, "12)"),
        ("arabicPlain", 7, "7"),
        ("alphaLcPeriod", 1, "a."),
        ("alphaLcPeriod", 27, "a."),
        ("alphaUcPeriod", 26, "Z."),
        ("alphaLcParenR", 2, "b)"),
        ("alphaUcParenR", 28, "B)"),
        ("romanLcPeriod", 1994, "mcmxciv."),
        ("romanUcPeriod", 4, "IV."),
        ("romanUcPeriod", 0, "."),
        // `%` keeps the dividend's sign: n = 0 gives '`', and negative numbers go below it
        ("alphaLcPeriod", 0, "`."),
        ("alphaLcPeriod", -1, "_."),
        ("circleNumDbPlain", 5, "5."),
    ];
    for (s, n, want) in cases {
        assert_eq!(format_auto_num(s, n), want, "{s} {n}");
    }
}

#[test]
fn two_phase_protocol_asks_advances_first_then_the_rest() {
    let mut p = para(vec![run("Hello wonderful world of text layout")]);
    p.runs[0].kern = Some(10.0);
    let tb = body(vec![p, para(vec![run("مرحبا بالعالم")])]);
    let o = LayoutTextOptions::default();
    let mut t = MeasureTable::default();
    let first = need_measures(&tb, 914_400.0, 914_400.0, Measurer::Ppt, &t, &o);
    assert!(!first.is_empty() && first.iter().all(|r| matches!(r, MeasureReq::CharAt1000 { .. })), "round 1 asks only advances");
    assert!(matches!(layout_text_body(&tb, 914_400.0, 914_400.0, Measurer::Ppt, &t, &o), Err(LayoutError::NeedMeasures(_))));
    answer_all(&mut t, &first);
    let second = need_measures(&tb, 914_400.0, 914_400.0, Measurer::Ppt, &t, &o);
    assert!(second.iter().any(|r| matches!(r, MeasureReq::KernAt1000 { .. })), "kerning on: the whole-string delta");
    assert!(second.iter().any(|r| matches!(r, MeasureReq::Width(k) if k.kerning == Kerning::None)), "Arabic is measured whole");
    let rounds = 1 + settle(&tb, 914_400.0, 914_400.0, Measurer::Ppt, &mut t, &o);
    let r = layout_text_body(&tb, 914_400.0, 914_400.0, Measurer::Ppt, &t, &o).unwrap();
    assert!(r.lines.len() >= 3, "a 0.72 in box wraps the sentence: {} lines", r.lines.len());
    assert!(rounds <= 4);
    // without `fontKerning` the delta is never asked and whole strings use the current state
    let mut t2 = MeasureTable::default();
    t2.caps.font_kerning = false;
    settle(&tb, 914_400.0, 914_400.0, Measurer::Ppt, &mut t2, &o);
    let (res, asked) = layout_text_body_traced(&tb, 914_400.0, 914_400.0, Measurer::Ppt, &t2, &o);
    assert!(res.is_ok());
    assert!(!asked.iter().any(|r| matches!(r, MeasureReq::KernAt1000 { .. })));
    assert!(asked.iter().any(|r| matches!(r, MeasureReq::Width(k) if k.kerning == Kerning::Current)));
    // the plain measurer asks whole strings only
    let mut t3 = MeasureTable::default();
    settle(&tb, 914_400.0, 914_400.0, Measurer::Plain, &mut t3, &o);
    let (_, asked) = layout_text_body_traced(&tb, 914_400.0, 914_400.0, Measurer::Plain, &t3, &o);
    assert!(asked.iter().all(|r| matches!(r, MeasureReq::Width(k) if k.kerning == Kerning::Current)));
}

#[test]
fn bullets_numbering_tabs_and_columns() {
    let mut ps = Vec::new();
    for (i, lvl) in [0.0, 0.0, 1.0, 1.0, 0.0, -1.0, -1.0, f64::NAN].into_iter().enumerate() {
        let mut p = para(vec![run(&format!("item {i}\tvalue 1.25"))]);
        p.level = Some(lvl);
        p.bullet = Some(true);
        p.bu_mut().bullet_auto_num = Some("arabicPeriod".into());
        p.x_mut().tabs = Some(vec![TabStop { pos: 1_000_000.0, algn: "dec".into() }].into());
        p.mar_l = Some(342_900.0);
        p.indent = Some(-342_900.0);
        ps.push(p);
    }
    let mut tb = body(ps);
    tb.x_mut().num_col = Some(2.0);
    tb.x_mut().spc_col = Some(100_000.0);
    let o = LayoutTextOptions::default();
    let mut t = MeasureTable::default();
    settle(&tb, 4_000_000.0, 600_000.0, Measurer::Ppt, &mut t, &o);
    let r = layout_text_body(&tb, 4_000_000.0, 600_000.0, Measurer::Ppt, &t, &o).unwrap();
    let nums: Vec<&str> = r.lines.iter().filter_map(|l| l.bullet.as_ref()).map(|b| b.text.as_str()).collect();
    // levels restart deeper counters; a negative level is an array PROPERTY that persists, NaN another
    assert_eq!(nums, ["1.", "2.", "1.", "2.", "3.", "1.", "2.", "1."]);
    assert!(r.lines.iter().any(|l| l.runs.first().is_some_and(|x| x.x > 2_000_000.0)), "the second column is used");
}

#[test]
fn symbol_area_recursion_throws_like_the_ts() {
    // U+F07F: the Symbol table leaves it unmapped, the TS recursion never ends (RangeError, the shape is skipped)
    let tb = body(vec![para(vec![run("a\u{F07F}b")])]);
    let t = MeasureTable::default();
    let r = layout_text_body(&tb, 1e6, 1e6, Measurer::Ppt, &t, &LayoutTextOptions::default());
    assert_eq!(r, Err(LayoutError::Throws("RangeError")));
    assert!(need_measures(&tb, 1e6, 1e6, Measurer::Ppt, &t, &LayoutTextOptions::default()).is_empty());
    // mapped symbol-area characters convert and lay out
    let tb = body(vec![para(vec![run("\u{F067}x")])]);
    let mut t = MeasureTable::default();
    settle(&tb, 1e6, 1e6, Measurer::Ppt, &mut t, &LayoutTextOptions::default());
    let r = layout_text_body(&tb, 1e6, 1e6, Measurer::Ppt, &t, &LayoutTextOptions::default()).unwrap();
    assert_eq!(r.lines[0].runs[0].text, "γ");
    assert_eq!(&*r.lines[0].runs[0].font.family, "Segoe UI Symbol");
}

#[test]
fn long_word_breaks_by_grapheme_or_code_point() {
    let word = "e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}";
    let tb = body(vec![para(vec![run(word)])]);
    let o = LayoutTextOptions { ins_l: Some(0.0), ins_r: Some(0.0), ..LayoutTextOptions::default() };
    for seg in [true, false] {
        let mut t = MeasureTable::default();
        t.caps.segmenter = seg;
        settle(&tb, 30_000.0, 1e6, Measurer::Ppt, &mut t, &o);
        let r = layout_text_body(&tb, 30_000.0, 1e6, Measurer::Ppt, &t, &o).unwrap();
        // a combining mark never starts a line with the segmenter; without it the code points split
        let starts_with_mark = r.lines.iter().any(|l| l.runs.first().is_some_and(|x| x.text.starts_with('\u{301}')));
        assert_eq!(
            starts_with_mark,
            !seg && r.lines.len() > 1,
            "segmenter {seg}: {:?}",
            r.lines.iter().map(|l| &l.runs[0].text).collect::<Vec<_>>()
        );
    }
}

#[test]
fn cell_body_applies_margins_anchor_and_style_defaults() {
    let mut cell = TableCell { tx_body: Some(Box::new(body(vec![para(vec![run("x")])]))), ..TableCell::default() };
    cell.tx_bold = Some(true);
    cell.mar_l = Some(1.0);
    let b = cell_body(&cell).unwrap();
    assert_eq!(b.anchor.as_deref(), Some("t"));
    assert_eq!((b.ins_l, b.ins_r, b.ins_t, b.ins_b), (Some(1.0), Some(91440.0), Some(45720.0), Some(45720.0)));
    assert_eq!(b.paragraphs[0].runs[0].bold, Some(true));
    assert!(cell_body(&TableCell::default()).is_none());
}

// ---------------------------------------------------------------- memory

fn face() -> FaceKey {
    FaceKey { family: "Liberation Sans".into(), bold: false, italic: false }
}

#[test]
fn m1_layout_and_table_return_every_byte() {
    let mut p = para(vec![run("Hello wonderful\tworld 1.5"), run("これは日本語。"), run("\u{1F468}\u{200D}\u{1F469}")]);
    p.bullet = Some(true);
    p.bu_mut().bullet_auto_num = Some("romanUcPeriod".into());
    let tb = body(vec![p, para(vec![]), para(vec![run("a\u{F07F}")])]);
    let tb_ok = body(vec![tb.paragraphs[0].clone(), tb.paragraphs[1].clone()]);
    let o = LayoutTextOptions::default();
    let mut t = MeasureTable::default();
    settle(&tb_ok, 2e6, 1e6, Measurer::Ppt, &mut t, &o);
    let empty = MeasureTable::default();
    uo_alloc::assert_no_leak(|| layout_text_body(&tb_ok, 2e6, 1e6, Measurer::Ppt, &t, &o).unwrap());
    uo_alloc::assert_no_leak(|| layout_text_body(&tb_ok, 2e6, 1e6, Measurer::Ppt, &empty, &o).unwrap_err());
    uo_alloc::assert_no_leak(|| layout_text_body(&tb, 2e6, 1e6, Measurer::Ppt, &t, &o).unwrap_err());
    uo_alloc::assert_no_leak(|| need_measures(&tb_ok, 2e6, 1e6, Measurer::Plain, &empty, &o));
    uo_alloc::assert_no_leak(|| layout_text_body_traced(&tb_ok, 2e6, 1e6, Measurer::Ppt, &t, &o));
    uo_alloc::assert_no_leak(|| format_auto_num("romanUcPeriod", 3888));
    uo_alloc::assert_no_leak(|| {
        let mut t = MeasureTable::default();
        t.set_char_at_1000(&face(), 'a', 500.0);
        t.set_kern_at_1000(&face(), "AV", 1.0, 2.0);
        t.set_width(
            &StringKey {
                face: face(),
                size_px: Bits::of(12.0),
                kerning: Kerning::None,
                letter_spacing_px: Bits::of(0.0),
                text: "x".into(),
            },
            1.0,
        );
        t.set_generation(3);
        t
    });
}

#[test]
fn m5_the_table_cap_clears_past_it() {
    let mut t = MeasureTable::with_cap(1_000, usize::MAX);
    for i in 0..5_000u32 {
        t.set_char_at_1000(&face(), char::from_u32(0x4E00 + i).unwrap(), 1.0);
        assert!(t.len() <= 1_000);
    }
    // the 1,001st insert clears: 5,000 inserts leave the last 1,000
    assert_eq!(t.len(), 1_000);
    // the byte cap
    let mut t = MeasureTable::with_cap(usize::MAX, 64 << 10);
    let long = "x".repeat(1_000);
    for i in 0..1_000 {
        t.set_width(
            &StringKey {
                face: face(),
                size_px: Bits::of(f64::from(i)),
                kerning: Kerning::None,
                letter_spacing_px: Bits::of(0.0),
                text: long.as_str().into(),
            },
            1.0,
        );
        assert!(t.counted_bytes() <= 64 << 10);
    }
    // the default caps are the recorded ones
    let mut t = MeasureTable::default();
    for i in 0..70_000u32 {
        t.set_char_at_1000(&face(), char::from_u32(0x10000 + i).unwrap(), 1.0);
    }
    assert!(t.len() <= 65_536);
    // a new font generation empties the table, the same one keeps it
    t.set_generation(t.generation());
    assert!(!t.is_empty());
    t.set_generation(t.generation() + 1);
    assert!(t.is_empty());
}

// ---------------------------------------------------------------- no panic

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n.max(1)
    }
    fn chance(&mut self, p: u32) -> bool {
        self.next() % 100 < p
    }
    fn num(&mut self, scale: f64) -> f64 {
        match self.below(12) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => -scale,
            3 => 1e300,
            4 => -0.0,
            _ => f64::from(self.next() % 1000) * scale / 500.0,
        }
    }
    fn opt(&mut self, scale: f64) -> Option<f64> {
        self.chance(40).then(|| self.num(scale))
    }
}

const POOL: [&str; 16] = [
    "lorem ipsum ",
    "Supercalifragilistic",
    "日本語のテキスト。",
    "「",
    "\u{1F468}\u{200D}\u{1F469}",
    "e\u{301}",
    "\u{F067}",
    "\t",
    "\n",
    "a\u{a0}\u{a0}b ",
    "x-y—z",
    "क्ष",
    "مرحبا ",
    "  ",
    "1.5,2",
    "",
];

fn random_body(r: &mut Rng) -> TextBody {
    // the same draws in the same order as the flat literal before the compaction
    let wrap = r.chance(20).then(|| "none".into());
    let font_scale = r.opt(1.0);
    let ln_spc_reduction = r.opt(0.3);
    let num_col = r.opt(3.0);
    let spc_col = r.opt(1e5);
    let ins_l = r.opt(1e5);
    let anchor = r.chance(50).then(|| ["t", "ctr", "b"][r.below(3)].into());
    let spc_first_last_para = r.chance(10).then_some(true);
    let mut tb = TextBody { wrap, ins_l, anchor, ..TextBody::default() };
    if font_scale.is_some() || ln_spc_reduction.is_some() || num_col.is_some() || spc_col.is_some() || spc_first_last_para.is_some() {
        let x = tb.x_mut();
        x.font_scale = font_scale;
        x.ln_spc_reduction = ln_spc_reduction;
        x.num_col = num_col;
        x.spc_col = spc_col;
        x.spc_first_last_para = spc_first_last_para;
    }
    for _ in 0..r.below(5) {
        // the same draws in the same order as the flat literal before the compaction
        let level = r.chance(50).then(|| f64::from(r.next() % 14) - 3.0);
        let bullet = r.chance(50).then_some(true);
        let bullet_auto_num = r.chance(40).then(|| ["arabicPeriod", "alphaUcParenR", "romanLcPeriod"][r.below(3)].into());
        let bullet_font = r.chance(20).then(|| "Wingdings".into());
        let bullet_size_pct = r.opt(2.0);
        let mut p = Paragraph {
            level,
            bullet,
            align: r.chance(60).then(|| ["l", "ctr", "r", "just", "dist"][r.below(5)].into()),
            mar_l: r.opt(1e6),
            indent: r.opt(-4e5),
            line_spacing: r.opt(2.0),
            line_spacing_pt: r.chance(20).then(|| r.num(30.0)),
            space_before_pct: r.opt(1.0),
            space_after_pt: r.opt(20.0),
            def_tab_sz: r.opt(9e5),
            ..Paragraph::default()
        };
        let ea_ln_brk = r.chance(20).then(|| r.chance(50));
        if bullet_auto_num.is_some() || bullet_font.is_some() || bullet_size_pct.is_some() {
            let b = p.bu_mut();
            b.bullet_auto_num = bullet_auto_num;
            b.bullet_font = bullet_font;
            b.bullet_size_pct = bullet_size_pct;
        }
        if ea_ln_brk.is_some() {
            p.x_mut().ea_ln_brk = ea_ln_brk;
        }
        if r.chance(30) {
            p.x_mut().tabs =
                Some((0..r.below(4)).map(|_| TabStop { pos: r.num(1e6), algn: ["l", "r", "ctr", "dec"][r.below(4)].into() }).collect());
        }
        for _ in 0..r.below(5) {
            let mut text = String::new();
            for _ in 0..1 + r.below(6) {
                text.push_str(POOL[r.below(POOL.len())]);
            }
            let mut kern = None;
            let extras = r.chance(40).then(|| {
                let baseline = r.opt(0.5);
                let cap = r.chance(30).then(|| ["all", "small"][r.below(2)].into());
                let spc = r.opt(10.0);
                kern = r.opt(20.0);
                Box::new(RunExtras { baseline, cap, spc, link_color: None, ..RunExtras::default() })
            });
            p.runs.push(Run {
                text: text.into(),
                sz: r.opt(60.0),
                bold: r.chance(30).then(|| r.chance(50)),
                lang: r.chance(30).then(|| ["ja-JP", "zh", "en-US"][r.below(3)].into()),
                font: r.chance(30).then(|| ["Liberation Serif", "Symbol", "Wingdings"][r.below(3)].into()),
                kern,
                extras,
                ..Run::default()
            });
        }
        tb.paragraphs.push(p);
    }
    tb
}

fn hostile_answers(t: &mut MeasureTable, need: &[MeasureReq], r: &mut Rng) {
    for q in need {
        if r.chance(10) {
            continue; // missing: the next round asks again
        }
        let w = match r.below(6) {
            0 => f64::NAN,
            1 => -300.0,
            2 => 1e308,
            3 => 0.0,
            4 => f64::INFINITY,
            _ => f64::from(r.next() % 2000),
        };
        match q {
            MeasureReq::CharAt1000 { face, ch } => t.set_char_at_1000(face, *ch, w),
            MeasureReq::KernAt1000 { face, text } => t.set_kern_at_1000(face, text, w, -w),
            MeasureReq::Width(k) => t.set_width(k, w),
            _ => {}
        }
    }
}

#[test]
fn no_panic_over_random_bodies_and_hostile_tables() {
    let n = if std::env::var_os("UO_DML_LONG").is_some() { 20_000 } else { 1_000 };
    let mut r = Rng(0x2545_F491);
    let t0 = std::time::Instant::now();
    let mut laid = 0;
    for i in 0..n {
        let tb = random_body(&mut r);
        let mut t = MeasureTable::with_cap(2_048, 1 << 20);
        t.caps.font_kerning = r.chance(70);
        t.caps.segmenter = r.chance(70);
        let m = if r.chance(20) { Measurer::Plain } else { Measurer::Ppt };
        let o = LayoutTextOptions { default_sz_pt: r.opt(40.0), size_scale: r.opt(2.0), ..LayoutTextOptions::default() };
        let (w, h) = (r.num(5e6), r.num(5e6));
        for _ in 0..MAX_ROUNDS {
            let need = need_measures(&tb, w, h, m, &t, &o);
            if need.is_empty() {
                break;
            }
            hostile_answers(&mut t, &need, &mut r);
        }
        let chars: usize = tb.paragraphs.iter().flat_map(|p| &p.runs).map(|x| x.text.encode_utf16().count()).sum();
        if let Ok(res) = layout_text_body(&tb, w, h, m, &t, &o) {
            assert!(res.lines.len() <= chars + tb.paragraphs.len() + 1, "case {i}");
            laid += 1;
        }
    }
    eprintln!("{n} random bodies, {laid} laid out, {:.2} s", t0.elapsed().as_secs_f64());
    assert!(laid > n / 4);
}
