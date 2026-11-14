//! Frontier F5 (lines and paint): unit tests of the ported rules with numbers worked out from the
//! TS formulas, the M1 memory gate (no leak per operation) and the no-panic loops over hostile
//! paragraphs and lines. The bit-exact parity is `tests/f5_parity.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // helpers outside #[test] functions

use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_docx_layout::error::{LResult, LayoutError};
use uo_docx_layout::fonts::FaceMetrics;
use uo_docx_layout::items::PassItems;
use uo_docx_layout::keys::{LParaId, LineSeq};
use uo_docx_layout::lines::{BreakSettings, GlyphSrc, Line, LineBreaker, LineEnv, LineFrame, PtabSpec, glyph_of, line_box};
use uo_docx_layout::model::{BorderSpec, JsNumber, RPr};
use uo_docx_layout::output::{TabAlign, TabLeader, TabStop};
use uo_docx_layout::paint::{
    Band, BandInk, HrShape, PaintEnv, border_bands, border_paint_width, border_width, paint_border_in, paint_line_in, to_white,
};
use uo_docx_layout::para::{Ch, Glyph, GlyphFlags, GlyphKind, LineRule, PreparedPara, RunStyle, StyleIx};
use uo_docx_layout::underline::{UlFont, UlGeom, UlMetrics, ul_geometry};
use uo_render::types::{LineDash, RunFlags};
use uo_render::{FaceId, FaceRes, Item, OptF64, Resources, Sym, TextFxId};

// ── a small environment: one face, fixed metrics ────────────────────────────────────────────

struct TestEnv {
    res: Resources,
    settings: BreakSettings,
    seq: u32,
    work: u64,
}

impl TestEnv {
    fn new() -> TestEnv {
        let mut res = Resources::new();
        for k in 0..2 {
            let key = res.symbols.intern_str(if k == 0 { "Arial|" } else { "Arial|b" }).unwrap();
            let family = res.symbols.intern_str("Arial").unwrap();
            res.add_face(FaceRes {
                key,
                family,
                bold: k == 1,
                italic: false,
                synth_bold: k == 1,
                synth_italic: false,
                stand_in: false,
                face: None,
                bytes: None,
                advances: None,
            });
        }
        TestEnv {
            res,
            settings: BreakSettings { default_tab_stop: 720.0, compatibility_mode: 15.0, ..BreakSettings::default() },
            seq: 0,
            work: 0,
        }
    }
}

const M: FaceMetrics = FaceMetrics {
    ascent: 0.9,
    descent: 0.25,
    line_height: 1.15,
    underline_pos: 0.1,
    underline_thick: 0.05,
    bare_ascent: 0.9,
    sup_offset: None,
};

impl LineEnv for TestEnv {
    fn settings(&self) -> BreakSettings {
        self.settings
    }
    fn advance_twips(&mut self, _face: FaceId, cp: u32, size_pt: f64) -> LResult<f64> {
        Ok(if cp == 0x2d { size_pt * 6.0 } else { size_pt * 5.0 })
    }
    fn face_metrics(&mut self, _face: FaceId) -> LResult<FaceMetrics> {
        Ok(M)
    }
    fn ptab(&self, _p: &PreparedPara, _g: &Glyph) -> Option<PtabSpec> {
        None
    }
    fn charge(&mut self, units: u64) -> LResult<()> {
        self.work += units;
        Ok(())
    }
}

impl PaintEnv for TestEnv {
    fn face_metrics(&mut self, _face: FaceId) -> LResult<FaceMetrics> {
        Ok(M)
    }
    fn ul_font(&mut self, _face: FaceId, _em: f64) -> LResult<UlFont> {
        Ok(UlFont { units_per_em: 1000.0, us_win_descent: 250.0, descender: -250.0, vdmx_y_min: None })
    }
    fn advance_units(&mut self, _face: FaceId, _cp: u32) -> LResult<(f64, f64)> {
        Ok((278.0, 1000.0))
    }
    fn colr(&mut self, _face: FaceId, _cp: u32) -> LResult<bool> {
        Ok(false)
    }
    fn face_key(&mut self, face: FaceId) -> LResult<Sym> {
        self.res.face(face).map(|f| f.key).ok_or(LayoutError::Internal("face"))
    }
    fn face_synth(&mut self, face: FaceId) -> LResult<(bool, bool)> {
        self.res.face(face).map(|f| (f.synth_bold, f.synth_italic)).ok_or(LayoutError::Internal("face"))
    }
    fn fx_draws(&self, _fx: TextFxId) -> bool {
        false
    }
    fn intern(&mut self, s: &[u16]) -> LResult<Sym> {
        self.res.symbols.intern(s).ok_or(LayoutError::Internal("symbols"))
    }
    fn sym_text(&self, s: Sym) -> Vec<u16> {
        self.res.sym(s).to_vec()
    }
    fn settings(&self) -> BreakSettings {
        self.settings
    }
    fn word97_line_breaks(&self) -> bool {
        false
    }
    fn east_asia_font(&mut self, _rpr: &RPr) -> LResult<bool> {
        Ok(false)
    }
    fn next_line_seq(&mut self) -> LResult<LineSeq> {
        self.seq += 1;
        Ok(LineSeq(self.seq))
    }
    fn inline_object(&mut self, _p: &PreparedPara, _g: &Glyph, _x0: f64, _b: f64, _out: &mut PassItems) -> LResult<()> {
        Ok(())
    }
    fn hr_shape(&mut self, _p: &PreparedPara, _g: &Glyph) -> LResult<Option<HrShape>> {
        Ok(None)
    }
    fn charge(&mut self, units: u64) -> LResult<()> {
        self.work += units;
        Ok(())
    }
}

fn face(k: usize) -> FaceId {
    FaceId::from_index(k).unwrap()
}

fn style(color: &str) -> RunStyle {
    RunStyle { size_pt: 10.0, color: color.encode_utf16().collect(), ..RunStyle::default() }
}

fn glyph(kind: GlyphKind, c: char, adv: f64, offset: u32) -> Glyph {
    let mut units = [0u16; 2];
    let n = c.encode_utf16(&mut units).len();
    Glyph {
        kind,
        ch: Ch::of(&units[..n]),
        cp: u32::from(c),
        face: face(0),
        style: StyleIx(0),
        adv,
        asc: 180.0,
        desc: 50.0,
        n_asc: OptF64::NONE,
        n_desc: OptF64::NONE,
        offset,
        run_idx: 0,
        flags: GlyphFlags(if kind == GlyphKind::Space { GlyphFlags::BREAK_AFTER } else { 0 }),
        obj: None,
        hr: OptF64::NONE,
        ptab: None,
        size_pt: 10.0,
        check_box: None,
        note_id: OptF64::NONE,
        fe_shrink: OptF64::NONE,
        kern_adj: OptF64::NONE,
        lig_start: None,
        lig_end: None,
        sub_gid: None,
        math: None,
        pic_bullet: None,
    }
}

/// A paragraph of `text` (spaces break after, letters 100 twips, spaces 50).
fn para(text: &str) -> PreparedPara {
    let glyphs = text
        .chars()
        .enumerate()
        .map(|(k, c)| {
            let o = u32::try_from(k).unwrap();
            match c {
                ' ' => glyph(GlyphKind::Space, ' ', 50.0, o),
                '\t' => glyph(GlyphKind::Tab, '\t', 0.0, o),
                '\n' => glyph(GlyphKind::Lb, '\n', 0.0, o),
                _ => glyph(GlyphKind::Char, c, 100.0, o),
            }
        })
        .collect();
    PreparedPara {
        para: LParaId(0),
        ppr: Arc::default(),
        style_id: None,
        styles: vec![style("000000"), style("FF0000")],
        mark_style: StyleIx(0),
        mark_face: face(0),
        glyphs,
        marker: None,
        ind_left: 0.0,
        ind_right: 0.0,
        first_line: 0.0,
        jc: "left".into(),
        tabs: Vec::new(),
        space_before: 0.0,
        space_after: 0.0,
        before_auto: false,
        after_auto: false,
        line_rule: LineRule::Auto,
        line: 240.0,
        contextual: false,
        keep_next: false,
        keep_lines: false,
        widow_control: false,
        page_break_before: false,
        empty: false,
        rev_bars: None,
        math: Vec::new(),
        char_grid: None,
    }
}

fn frame(left: f64, right: f64) -> LineFrame {
    LineFrame { left, right, gap: false, next: None }
}

fn break_all(env: &mut TestEnv, p: &PreparedPara, f: LineFrame) -> Vec<Line> {
    let mut br = LineBreaker::new(p, None);
    let mut out = Vec::new();
    loop {
        out.push(br.next_in(env, p, f).unwrap());
        if br.done(p) || out.len() > 10_000 {
            break;
        }
    }
    out
}

// ── lines ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn line_box_follows_the_spacing_rules() {
    let mut p = para("a");
    p.line_rule = LineRule::Exact;
    p.line = 300.0;
    assert_eq!(line_box(200.0, 50.0, &p, None, None), (300.0, 240.0));
    p.line_rule = LineRule::AtLeast;
    assert_eq!(line_box(200.0, 50.0, &p, None, None), (300.0, 250.0));
    p.line_rule = LineRule::Auto;
    p.line = 240.0;
    assert_eq!(line_box(200.0, 50.0, &p, None, None), (250.0, 200.0));
    p.line = 480.0;
    assert_eq!(line_box(200.0, 50.0, &p, None, None), (500.0, 200.0));
    // the extra is over the TEXT height only
    assert_eq!(line_box(200.0, 50.0, &p, Some(100.0), None), (350.0, 200.0));
    // below single: the baseline scales
    p.line = 120.0;
    assert_eq!(line_box(200.0, 50.0, &p, None, None), (125.0, 100.0));
    // the document grid: one 360 step, text centred
    p.line = 240.0;
    assert_eq!(line_box(200.0, 50.0, &p, None, Some(360.0)), (360.0, 255.0));
    // a grid of 0 or NaN is no grid (`grid && grid > 0`)
    assert_eq!(line_box(200.0, 50.0, &p, None, Some(0.0)), (250.0, 200.0));
    assert_eq!(line_box(200.0, 50.0, &p, None, Some(f64::NAN)), (250.0, 200.0));
}

/// WordCraft study (fields, C1b; record uo-docx-layout.md "WordCraft study — fields"): Calibri 11 + Tahoma 11 at
/// multiple 1.5 — Word's extra is 0.5 × the LARGEST OWN height (Calibri's 1950 + 550 of 2048 em), not 0.5 × (Tahoma's
/// ascent 2049 + Calibri's descent 550): Word's pitch 20.67 pt, the frozen TS's rule 20.94 (measured `wcfonts3/c1y-*`).
#[test]
fn multiple_spacing_takes_the_largest_own_text_height() {
    let em = 220.0 / 2048.0;
    let (asc, desc) = (2049.0 * em, 550.0 * em); // max ascent (Tahoma), max descent (Calibri)
    let own = (1950.0 + 550.0) * em; // Calibri's own height (Tahoma's: 2049 + 423)
    let mut p = para("a");
    p.line_rule = LineRule::Auto;
    p.line = 360.0;
    let word = line_box(asc, desc, &p, Some(own), None).0 / 20.0;
    let ts = line_box(asc, desc, &p, Some(asc + desc), None).0 / 20.0;
    assert!((word - 20.67).abs() < 0.01, "{word}");
    assert!((ts - 20.94).abs() < 0.01, "{ts}");
}

#[test]
fn words_break_at_spaces_and_trailing_spaces_hang() {
    let mut env = TestEnv::new();
    // "aa bb cc": 200 + 50 + 200 + 50 + 200; a 500-twip line holds "aa bb " (the space hangs)
    let p = para("aa bb cc");
    let lines = break_all(&mut env, &p, frame(0.0, 500.0));
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].start, lines[0].end), (0, 6));
    assert_eq!(lines[0].width, 450.0);
    assert!(!lines[0].last);
    assert_eq!((lines[1].start, lines[1].end), (6, 8));
    assert!(lines[1].last);
    // vertical metrics: glyph ascent/descent, single spacing
    assert_eq!((lines[0].asc, lines[0].desc, lines[0].height, lines[0].baseline), (180.0, 50.0, 230.0, 180.0));
    assert!(env.work > 0);
}

#[test]
fn a_word_longer_than_the_line_breaks_by_character_and_progress_is_guaranteed() {
    let mut env = TestEnv::new();
    let p = para("abcdef");
    let lines = break_all(&mut env, &p, frame(0.0, 250.0));
    let ends: Vec<u32> = lines.iter().map(|l| l.end).collect();
    assert_eq!(ends, [2, 4, 6]);
    // a frame of no width still advances one glyph per line
    let lines = break_all(&mut env, &p, frame(0.0, 0.0));
    assert_eq!(lines.len(), 6);
}

#[test]
fn alignment_and_justification() {
    let mut env = TestEnv::new();
    let mut p = para("aa bb cc");
    p.jc = "center".into();
    let l = &break_all(&mut env, &p, frame(0.0, 1000.0))[0];
    // width 700, avail 1000: dx = 150
    assert_eq!(l.x_start, 150.0);
    assert_eq!(l.placed[0].x, 150.0);
    p.jc = "right".into();
    let l = &break_all(&mut env, &p, frame(0.0, 1000.0))[0];
    assert_eq!(l.placed[0].x, 300.0);
    // justified: the inner spaces of a non-last line share the extra
    p.jc = "both".into();
    let lines = break_all(&mut env, &p, frame(0.0, 600.0));
    let l = &lines[0];
    assert_eq!((l.start, l.end), (0, 6));
    // "aa bb" = 450, avail 600; the space between aa and bb is the only inner one: +150
    assert_eq!(l.placed[2].w, 200.0);
    assert_eq!(l.placed[3].x, 400.0);
}

#[test]
fn default_tab_stops_without_custom_tabs() {
    let mut env = TestEnv::new();
    // `Math.max(...[])` = −Infinity: the default grid from 0
    let p = para("a\tb");
    let l = &break_all(&mut env, &p, frame(0.0, 5000.0))[0];
    assert_eq!(l.placed[1].x, 100.0);
    assert_eq!(l.placed[1].w, 620.0);
    assert_eq!(l.placed[2].x, 720.0);
    // a custom right stop at 2000 aligns the text after it
    let mut p = para("a\tbb");
    p.tabs.push(TabStop { val: TabAlign::Right, pos: 2000.0, leader: Some(TabLeader::Dot) });
    let l = &break_all(&mut env, &p, frame(0.0, 5000.0))[0];
    assert_eq!(l.placed[1].leader, Some(TabLeader::Dot));
    assert_eq!(l.placed[2].x, 1800.0);
    assert_eq!(l.placed[1].w, 1700.0);
}

#[test]
fn automatic_hyphen_is_a_derived_glyph() {
    let mut env = TestEnv::new();
    // "aaaa bbbbbb": the second word hyphenates after its 3rd letter in a 900-twip line
    let mut p = para("aaaa bbbbbb");
    for g in &mut p.glyphs[5..10] {
        g.flags.0 |= GlyphFlags::HYPH;
    }
    let lines = break_all(&mut env, &p, frame(0.0, 900.0));
    let first = &lines[0];
    let last = first.placed.last().unwrap();
    let GlyphSrc::AutoHyphen(k) = last.g else { panic!("{:?}", first.placed) };
    // the hyphen width is advanceTwips(face, '-', size) = 60
    assert_eq!(last.w, 60.0);
    let g = glyph_of(&p, last.g).unwrap();
    assert_eq!((g.kind, g.cp, g.adv, g.ch.len), (GlyphKind::Shy, 0xad, 0.0, 0));
    assert_eq!(g.offset, p.glyphs[k as usize].offset + 1);
    assert!(g.flags.has(GlyphFlags::AUTO_HYPHEN) && !g.flags.has(GlyphFlags::HYPH));
    assert_eq!(first.end, k + 1);
}

#[test]
fn a_trailing_line_break_gives_the_mark_its_own_line() {
    let mut env = TestEnv::new();
    let p = para("ab\n");
    let lines = break_all(&mut env, &p, frame(0.0, 1000.0));
    assert_eq!(lines.len(), 2);
    assert!(!lines[0].last && lines[1].last);
    assert!(lines[1].placed.is_empty());
    // the empty line takes the mark's face height: 0.9 × 10 pt × 20 and 0.25 × 200
    assert_eq!((lines[1].asc, lines[1].desc), (180.0, 50.0));
}

// ── underline, borders ──────────────────────────────────────────────────────────────────────

#[test]
fn underline_geometry_is_word_s() {
    assert_eq!(ul_geometry(UlMetrics { p: 10.0, t: 4.0, l: 20.0 }), UlGeom { p: 10.0, t: 4.0, td: 2.0, top1: 5.0, top2: 15.0, b: 10.0 });
    // the line would pass the descent: moved up
    let g = ul_geometry(UlMetrics { p: 18.0, t: 4.0, l: 20.0 });
    assert_eq!((g.p, g.top1, g.top2), (16.0, 8.0, 18.0));
}

fn border(val: &str, sz: Option<f64>) -> BorderSpec {
    BorderSpec { val: val.into(), sz: sz.map(JsNumber::Num), ..BorderSpec::default() }
}

#[test]
fn border_widths_and_bands() {
    assert_eq!(border_width(Some(&border("single", Some(4.0)))), 10.0);
    assert_eq!(border_width(Some(&border("single", Some(1.0)))), 2.0);
    assert_eq!(border_width(Some(&border("single", Some(3.0)))), 7.0);
    assert_eq!(border_width(Some(&border("double", None))), 30.0);
    assert_eq!(border_width(Some(&border("threeDEmboss", Some(4.0)))), 38.0);
    assert_eq!(border_width(Some(&border("nil", Some(4.0)))), 0.0);
    assert_eq!(border_width(None), 0.0);
    assert_eq!(border_paint_width(&border("single", Some(4.0))), 4.0 * 2.4);
    assert_eq!(border_paint_width(&border("single", Some(12.0))), 12.0 * 2.4);
    let ink = |t| Band { t, c: Some(BandInk::Ink) };
    assert_eq!(border_bands("double", 9.6), [ink(9.6), Band { t: 9.6, c: None }, ink(9.6)]);
    assert_eq!(border_bands("thinThickSmallGap", 28.8), [ink(28.8), Band { t: 14.4, c: None }, ink(14.4)]);
    assert_eq!(to_white(&"000000".encode_utf16().collect::<Vec<_>>(), 192.0 / 255.0), "C0C0C0".encode_utf16().collect::<Vec<_>>());
    assert_eq!(to_white(&"000000".encode_utf16().collect::<Vec<_>>(), 96.0 / 255.0), "606060".encode_utf16().collect::<Vec<_>>());
    // `parseInt('auto', 16)` reads the leading 'a' (10): the blue channel starts at 10
    assert_eq!(to_white(&"auto".encode_utf16().collect::<Vec<_>>(), 96.0 / 255.0), "606066".encode_utf16().collect::<Vec<_>>());
    // `parseInt('xyz', 16)` is NaN → 0 through `>>`
    assert_eq!(to_white(&"xyz".encode_utf16().collect::<Vec<_>>(), 96.0 / 255.0), "606060".encode_utf16().collect::<Vec<_>>());
}

#[test]
fn borders_paint_as_rectangles_or_dashed_strokes() {
    let mut env = TestEnv::new();
    let black: Vec<u16> = "000000".encode_utf16().collect();
    let mut out = PassItems::new();
    paint_border_in(&mut env, &mut out, &border("double", Some(4.0)), &black, [0.0, 100.0, 1000.0, 100.0], None).unwrap();
    assert_eq!(out.len(), 2);
    let Item::Rect { y, h, w, .. } = out.list.items[0] else { panic!() };
    assert_eq!((y, h, w), (100.0 - 14.4, 9.6, 1000.0));
    let mut out = PassItems::new();
    paint_border_in(&mut env, &mut out, &border("dotted", Some(4.0)), &black, [0.0, 100.0, 1000.0, 100.0], None).unwrap();
    let Item::Line { dash, width_pt, .. } = out.list.items[0] else { panic!() };
    assert_eq!((dash, width_pt), (Some(LineDash::BdrDot), 9.6 / 20.0));
    let mut out = PassItems::new();
    paint_border_in(&mut env, &mut out, &border("wave", Some(4.0)), &black, [0.0, 100.0, 600.0, 100.0], None).unwrap();
    assert!(matches!(out.list.items[0], Item::Path { .. }));
    assert_eq!(out.side.len(), out.len());
}

// ── paint ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn glyph_runs_merge_by_key_and_split_by_colour() {
    let mut env = TestEnv::new();
    let mut p = para("ab cd");
    // "cd" in red
    p.glyphs[3].style = StyleIx(1);
    p.glyphs[4].style = StyleIx(1);
    let l = &break_all(&mut env, &p, frame(0.0, 5000.0))[0];
    let mut out = PassItems::new();
    paint_line_in(&mut env, &p, l, 1440.0, 1000.0, &mut out).unwrap();
    // two runs: "ab" (+ the space is skipped, without breaking the run) and "cd"
    let runs: Vec<_> = out.list.items.iter().filter(|it| matches!(it, Item::Glyphs { .. })).collect();
    assert_eq!(runs.len(), 2);
    let Item::Glyphs { y, run, .. } = runs[0] else { panic!() };
    assert_eq!(*y, 1000.0 + l.baseline);
    assert_eq!(out.list.text_of(out.list.runs[*run as usize].text), "ab".encode_utf16().collect::<Vec<_>>());
    let xs: Vec<f64> = out.list.glyphs.x.clone();
    assert_eq!(xs, [1440.0, 1540.0, 1690.0, 1790.0]);
    // the line box goes in the side data, one line id per paintLine
    let ids: Vec<u32> = out.side.iter().filter_map(|s| s.glyph.and_then(|m| m.line_box)).map(|b| b.line.0).collect();
    assert_eq!(ids, [1, 1]);
}

#[test]
fn underline_strike_and_leader_items() {
    let mut env = TestEnv::new();
    let mut p = para("ab\tc");
    p.styles[0].underline = Some(("single".into(), None));
    p.styles[0].strike = true;
    p.tabs.push(TabStop { val: TabAlign::Left, pos: 3000.0, leader: Some(TabLeader::Dot) });
    let l = &break_all(&mut env, &p, frame(0.0, 6000.0))[0];
    let mut out = PassItems::new();
    paint_line_in(&mut env, &p, l, 0.0, 0.0, &mut out).unwrap();
    let kinds: Vec<&str> = out
        .list
        .items
        .iter()
        .map(|it| match it {
            Item::Glyphs { .. } => "g",
            Item::Rect { .. } => "r",
            Item::Line { .. } => "l",
            _ => "?",
        })
        .collect();
    // "ab", the leader dots, "c", the underline (one stretch: the tab joins it), the strike
    assert_eq!(kinds, ["g", "g", "g", "r", "l"]);
    // the leader run: no line box, no synthetic flags
    let Item::Glyphs { flags, .. } = out.list.items[1] else { panic!() };
    assert_eq!(flags, RunFlags::default());
    assert!(out.side[1].glyph.is_none());
}

// ── M1: no leak per operation ───────────────────────────────────────────────────────────────

#[test]
fn m1_break_and_paint_return_every_byte() {
    let mut p = para("lorem ipsum dolor sit amet, consectetur adipiscing elit\tsed do eiusmod tempor");
    p.jc = "both".into();
    p.tabs.push(TabStop { val: TabAlign::Right, pos: 4000.0, leader: Some(TabLeader::Dot) });
    p.styles[0].underline = Some(("wave".into(), Some("FF0000".into())));
    let mut env = TestEnv::new();
    // warm the symbol table so its growth is not counted as a leak of the operation
    let lines = break_all(&mut env, &p, frame(0.0, 3000.0));
    for l in &lines {
        paint_line_in(&mut env, &p, l, 0.0, 0.0, &mut PassItems::new()).unwrap();
    }
    let usage = assert_no_leak(|| {
        let lines = break_all(&mut env, &p, frame(0.0, 3000.0));
        let mut out = PassItems::new();
        for l in &lines {
            paint_line_in(&mut env, &p, l, 0.0, 0.0, &mut out).unwrap();
        }
        (lines.len(), out.len())
    });
    eprintln!("M1 break + paint: {usage:?}");
    assert_no_leak(|| line_box(1.0, 2.0, &p, None, Some(360.0)));
    assert_no_leak(|| border_bands("thinThickThinLargeGap", 9.6).len());
}

// ── no panic on hostile input ───────────────────────────────────────────────────────────────

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn num(&mut self) -> f64 {
        match self.below(12) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => f64::NEG_INFINITY,
            3 => -0.0,
            4 => -(self.below(5000) as f64),
            5 => 1e300,
            _ => self.below(3000) as f64 / 3.0,
        }
    }
}

#[test]
fn no_panic_on_hostile_paragraphs() {
    let kinds = [
        GlyphKind::Char,
        GlyphKind::Space,
        GlyphKind::Nbsp,
        GlyphKind::Tab,
        GlyphKind::Lb,
        GlyphKind::Pb,
        GlyphKind::Cb,
        GlyphKind::Obj,
        GlyphKind::Shy,
        GlyphKind::Box,
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..400 {
        let mut env = TestEnv::new();
        env.settings.compatibility_mode = if rng.below(2) == 0 { 14.0 } else { 15.0 };
        env.settings.consecutive_hyphen_limit = rng.num();
        env.settings.default_tab_stop = rng.num();
        let mut p = para("");
        let n = rng.below(60);
        for k in 0..n {
            let kind = kinds[usize::try_from(rng.below(kinds.len() as u64)).unwrap()];
            let mut g = glyph(kind, if kind == GlyphKind::Space { ' ' } else { 'x' }, rng.num(), u32::try_from(k).unwrap());
            g.flags.0 = u16::try_from(rng.below(128)).unwrap();
            g.asc = rng.num();
            g.desc = rng.num();
            if rng.below(8) == 0 {
                g.hr = OptF64::some(rng.num());
            }
            if rng.below(8) == 0 {
                g.fe_shrink = OptF64::some(rng.num());
            }
            if rng.below(10) == 0 {
                g.check_box = Some((rng.num(), rng.below(2) == 0));
            }
            g.style = StyleIx(u32::try_from(rng.below(2)).unwrap());
            p.glyphs.push(g);
        }
        p.first_line = rng.num();
        p.ind_left = rng.num();
        p.ind_right = rng.num();
        p.line = rng.num();
        p.line_rule = [LineRule::Auto, LineRule::Exact, LineRule::AtLeast][usize::try_from(rng.below(3)).unwrap()];
        p.jc = ["left", "both", "distribute", "center", "right"][usize::try_from(rng.below(5)).unwrap()].into();
        for _ in 0..rng.below(4) {
            let val = [TabAlign::Left, TabAlign::Right, TabAlign::Center, TabAlign::Decimal, TabAlign::Bar, TabAlign::Clear]
                [usize::try_from(rng.below(6)).unwrap()];
            p.tabs.push(TabStop { val, pos: rng.num(), leader: Some(TabLeader::Dot) });
        }
        p.styles[0].underline =
            Some((["single", "double", "wave", "wavyDouble", "dash", "thick"][usize::try_from(rng.below(6)).unwrap()].into(), None));
        p.styles[1].strike = true;
        let f = LineFrame { left: rng.num(), right: rng.num(), gap: rng.below(2) == 0, next: Some((rng.num(), rng.num())) };
        let mut br = LineBreaker::new(&p, if rng.below(3) == 0 { Some(rng.num()) } else { None });
        let mut lines = 0;
        loop {
            let before = br.pos;
            let l = br.next_in(&mut env, &p, f).unwrap();
            // progress: the position never goes back and a line never repeats forever
            assert!(br.pos >= before || p.glyphs.is_empty(), "round {round}");
            assert!(l.placed.iter().all(|q| glyph_of(&p, q.g).is_some()), "round {round}");
            let mut out = PassItems::new();
            // paint may stop at the budget of an absurd coordinate (charged); it must not panic
            let mut capped = CappedEnv { inner: &mut env, left: 200_000 };
            let _ = paint_line_in(&mut capped, &p, &l, rng.num(), rng.num(), &mut out);
            assert_eq!(out.side.len(), out.len(), "round {round}");
            lines += 1;
            if br.done(&p) || lines > 200 {
                break;
            }
        }
        assert!(lines <= usize::try_from(n).unwrap() + 2, "round {round}: {lines} lines for {n} glyphs");
    }
}

/// The test environment with a work budget (the engine's `Budget` stops the TS hangs, LQ14).
struct CappedEnv<'e> {
    inner: &'e mut TestEnv,
    left: u64,
}

impl PaintEnv for CappedEnv<'_> {
    fn face_metrics(&mut self, f: FaceId) -> LResult<FaceMetrics> {
        PaintEnv::face_metrics(self.inner, f)
    }
    fn ul_font(&mut self, f: FaceId, em: f64) -> LResult<UlFont> {
        self.inner.ul_font(f, em)
    }
    fn advance_units(&mut self, f: FaceId, cp: u32) -> LResult<(f64, f64)> {
        self.inner.advance_units(f, cp)
    }
    fn colr(&mut self, f: FaceId, cp: u32) -> LResult<bool> {
        self.inner.colr(f, cp)
    }
    fn face_key(&mut self, f: FaceId) -> LResult<Sym> {
        self.inner.face_key(f)
    }
    fn face_synth(&mut self, f: FaceId) -> LResult<(bool, bool)> {
        self.inner.face_synth(f)
    }
    fn fx_draws(&self, fx: TextFxId) -> bool {
        self.inner.fx_draws(fx)
    }
    fn intern(&mut self, s: &[u16]) -> LResult<Sym> {
        self.inner.intern(s)
    }
    fn sym_text(&self, s: Sym) -> Vec<u16> {
        self.inner.sym_text(s)
    }
    fn settings(&self) -> BreakSettings {
        PaintEnv::settings(self.inner)
    }
    fn word97_line_breaks(&self) -> bool {
        self.inner.word97_line_breaks()
    }
    fn east_asia_font(&mut self, rpr: &RPr) -> LResult<bool> {
        self.inner.east_asia_font(rpr)
    }
    fn next_line_seq(&mut self) -> LResult<LineSeq> {
        self.inner.next_line_seq()
    }
    fn inline_object(&mut self, p: &PreparedPara, g: &Glyph, x0: f64, b: f64, out: &mut PassItems) -> LResult<()> {
        self.inner.inline_object(p, g, x0, b, out)
    }
    fn hr_shape(&mut self, p: &PreparedPara, g: &Glyph) -> LResult<Option<HrShape>> {
        self.inner.hr_shape(p, g)
    }
    fn charge(&mut self, units: u64) -> LResult<()> {
        self.left = self.left.checked_sub(units).ok_or(LayoutError::Limit(uo_docx_layout::LimitKind::Work))?;
        Ok(())
    }
}
