//! Fuzz target of the Slides text layout (`uo-dml-render::text`, frontier B3; record `core/docs/modules/uo-dml-render.md`
//! "B3 — text layout").
//!
//! `text_layout`: structure-aware. The bytes drive a text body (body properties with NaN/±∞/negative/huge numbers,
//! columns, autofit, paragraphs with levels −2…12 and NaN, bullets and auto-numbering, tab stops, line spacing,
//! runs from a pool of hard snippets — CJK with kinsoku, emoji ZWJ sequences, combining marks, symbol-area
//! characters the Symbol table leaves unmapped, NBSP runs, tabs and breaks — plus raw bytes as text) and a hostile
//! `MeasureTable` answering every question with a width taken from the bytes (0, NaN, negative, 1e308, ∞ or
//! missing), with the capabilities from the bytes.
//!
//! Invariants beyond "no panic": a layout is a pure function (two runs give the same canonical text), the line count
//! is bounded by the characters plus the paragraphs, and the two-phase loop only ever asks questions the table does
//! not have.
use uo_dml_render::canon::Canon;
use uo_dml_render::model::{ParaBullet, ParaExtras, Paragraph, Run, RunExtras, TabStop, TextBody};
use uo_dml_render::text::{
    LayoutError, LayoutTextOptions, MAX_ROUNDS, MeasureReq, MeasureTable, Measurer, format_auto_num, layout_text_body, need_measures,
};

use std::sync::Arc;

use uo_dml_render::ids::MediaId;
use uo_dml_render::source::{PartSource, R};
use uo_opc::xml::Document;

/// A part source with nothing in it (the layout result holds no image here).
struct EmptySource;

impl PartSource for EmptySource {
    fn part(&self, _name: &str) -> Option<Arc<[u8]>> {
        None
    }
    fn xml(&self, _name: &str) -> R<Option<Arc<Document>>> {
        Ok(None)
    }
    fn media(&self, _part: &str) -> Option<MediaId> {
        None
    }
    fn media_entry(&self, _id: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        None
    }
}

const SNIPPETS: [&str; 24] = [
    "lorem ipsum dolor sit amet",
    "Supercalifragilisticexpialidocious",
    "これは日本語のテキストです。「引用」",
    "（括弧）、。",
    "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
    "\u{1F1E7}\u{1F1F7}\u{1F1E6}",
    "e\u{301}\u{308}a\u{20DD}",
    "\u{F067}\u{F061}",
    "\u{F07F}",
    "a\u{a0}\u{a0}b",
    "\t",
    "\n",
    "x-y—z–w",
    "क्षत्रिय",
    "مرحبا بالعالم",
    "  ",
    "12.50,3",
    "",
    "ภาษาไทย",
    "\u{FFFD}\u{FEFF}\u{2028}",
    "ǟ",
    "AVATAR Tomorrow",
    "\u{E000}\u{10FFFF}",
    "ß straße",
];

struct Bytes<'a> {
    d: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn u8(&mut self) -> u8 {
        let b = self.d.get(self.at).copied().unwrap_or(0);
        self.at = self.at.saturating_add(1);
        b
    }
    fn below(&mut self, n: usize) -> usize {
        usize::from(self.u8()) % n.max(1)
    }
    fn flag(&mut self) -> bool {
        self.u8() & 1 == 1
    }
    /// A number: mostly plausible, sometimes hostile.
    fn num(&mut self, scale: f64) -> f64 {
        match self.u8() {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => f64::NEG_INFINITY,
            3 => -scale * 3.0,
            4 => 1e300,
            5 => -0.0,
            b => f64::from(b) * scale / 64.0,
        }
    }
    fn opt(&mut self, scale: f64) -> Option<f64> {
        if self.flag() { Some(self.num(scale)) } else { None }
    }
    fn text(&mut self) -> String {
        let mut s = String::new();
        for _ in 0..1 + self.below(4) {
            if self.u8() < 40 {
                let n = self.below(12);
                let raw: Vec<u8> = (0..n).map(|_| self.u8()).collect();
                s.push_str(&String::from_utf8_lossy(&raw));
            } else {
                s.push_str(SNIPPETS[self.below(SNIPPETS.len())]);
            }
        }
        s
    }
    fn pick<'s>(&mut self, v: &[&'s str]) -> &'s str {
        v[self.below(v.len())]
    }
}

fn body(b: &mut Bytes<'_>) -> TextBody {
    // the input bytes are drawn in the order of the flat literal before the model compaction
    let anchor = b.flag().then(|| b.pick(&["t", "ctr", "b", "just"]).into());
    let wrap = b.flag().then(|| b.pick(&["none", "square"]).into());
    let font_scale = b.opt(1.0);
    let ln_spc_reduction = b.opt(0.3);
    let spc_first_last_para = b.flag().then_some(true);
    let (ins_l, ins_t, ins_r, ins_b) = (b.opt(200_000.0), b.opt(100_000.0), b.opt(200_000.0), b.opt(100_000.0));
    let num_col = b.opt(4.0);
    let spc_col = b.opt(400_000.0);
    let mut tb = TextBody { anchor, wrap, ins_l, ins_t, ins_r, ins_b, ..TextBody::default() };
    {
        let x = tb.x_mut();
        x.font_scale = font_scale;
        x.ln_spc_reduction = ln_spc_reduction;
        x.spc_first_last_para = spc_first_last_para;
        x.num_col = num_col;
        x.spc_col = spc_col;
    }
    for _ in 0..b.below(6) {
        let align = b.flag().then(|| b.pick(&["l", "ctr", "r", "just", "dist"]).into());
        let level = b.flag().then(|| f64::from(b.u8() % 16) - 3.0).map(|l| if l > 11.0 { f64::NAN } else { l });
        let bullet = b.flag().then_some(true);
        let bu = ParaBullet {
            bullet_char: b.flag().then(|| b.pick(&["•", "", "\u{F0B7}", "ab"]).into()),
            bullet_auto_num: b.flag().then(|| b.pick(&["arabicPeriod", "romanUcPeriod", "alphaLcParenR", "x"]).into()),
            bullet_font: b.flag().then(|| b.pick(&["Wingdings", "Symbol", "Monotype Sorts", "Liberation Sans"]).into()),
            bullet_size_pct: b.opt(2.0),
            bullet_size_pt: b.opt(40.0),
            ..ParaBullet::default()
        };
        let mut p = Paragraph {
            align,
            level,
            bullet,
            bullet_props: Some(Box::new(bu)),
            mar_l: b.opt(1_000_000.0),
            indent: b.opt(-500_000.0),
            line_spacing: b.opt(2.0),
            line_spacing_pt: b.opt(40.0),
            def_tab_sz: b.opt(914_400.0),
            space_before_pt: b.opt(20.0),
            space_after_pct: b.opt(1.0),
            end_sz: b.opt(40.0),
            ..Paragraph::default()
        };
        p.extras = Some(Box::new(ParaExtras {
            ea_ln_brk: b.flag().then(|| b.flag()),
            hanging_punct: b.flag().then(|| b.flag()),
            font_algn: b.flag().then(|| b.pick(&["t", "ctr", "b"]).into()),
            tabs: None,
        }));
        if b.flag() {
            let n = b.below(4);
            p.x_mut().tabs =
                Some((0..n).map(|_| TabStop { pos: b.num(600_000.0), algn: b.pick(&["l", "r", "ctr", "dec"]).into() }).collect());
        }
        for _ in 0..b.below(5) {
            let mut kern = None;
            let extras = b.flag().then(|| {
                let baseline = b.opt(0.5);
                let cap = b.flag().then(|| b.pick(&["all", "small"]).into());
                let spc = b.opt(10.0);
                kern = b.opt(30.0);
                Box::new(RunExtras {
                    baseline,
                    cap,
                    spc,
                    sym_font: b.flag().then(|| b.pick(&["Wingdings", "Symbol", "Webdings"]).into()),
                    strike: b.flag().then_some(true),
                    ..RunExtras::default()
                })
            });
            p.runs.push(Run {
                text: b.text().into(),
                sz: b.opt(60.0),
                bold: b.flag().then(|| b.flag()),
                italic: b.flag().then(|| b.flag()),
                font: b.flag().then(|| b.pick(&["Liberation Sans", "Wingdings", "Symbol", "", "constructor"]).into()),
                lang: b.flag().then(|| b.pick(&["ja-JP", "zh", "ko_KR", "en-US", "JA"]).into()),
                kern,
                extras,
                ..Run::default()
            });
        }
        tb.paragraphs.push(p);
    }
    tb
}

/// A hostile answer for a question: the width comes from the bytes.
fn answer(t: &mut MeasureTable, r: &MeasureReq, b: &mut Bytes<'_>) {
    let w = match b.u8() % 8 {
        0 => f64::NAN,
        1 => -500.0,
        2 => 1e308,
        3 => f64::INFINITY,
        4 => 0.0,
        _ => f64::from(b.u8()) * 7.5,
    };
    match r {
        MeasureReq::CharAt1000 { face, ch } => t.set_char_at_1000(face, *ch, w),
        MeasureReq::KernAt1000 { face, text } => t.set_kern_at_1000(face, text, w, w * 1.01),
        MeasureReq::Width(k) => t.set_width(k, w),
        _ => {}
    }
}

pub fn text_layout(data: &[u8]) {
    let mut b = Bytes { d: data, at: 0 };
    let sel = b.u8();
    let tb = body(&mut b);
    let mut table = MeasureTable::with_cap(4_096 >> (sel & 3), 1 << 20);
    table.caps.font_kerning = sel & 4 != 0;
    table.caps.segmenter = sel & 8 != 0;
    let measurer = if sel & 16 != 0 { Measurer::Plain } else { Measurer::Ppt };
    let opts = LayoutTextOptions {
        default_sz_pt: Some(b.num(40.0)),
        size_scale: b.opt(2.0),
        default_anchor: b.flag().then(|| "ctr".into()),
        ..LayoutTextOptions::default()
    };
    let (w, h) = (b.num(9_000_000.0), b.num(5_000_000.0));
    let chars: usize = tb.paragraphs.iter().flat_map(|p| p.runs.iter()).map(|r| r.text.encode_utf16().count()).sum();
    let paras = tb.paragraphs.len();
    for _ in 0..MAX_ROUNDS {
        let need = need_measures(&tb, w, h, measurer, &table, &opts);
        if need.is_empty() {
            break;
        }
        for r in &need {
            answer(&mut table, r, &mut b);
        }
    }
    let src = EmptySource;
    let canon = Canon { src: &src };
    let first = layout_text_body(&tb, w, h, measurer, &table, &opts);
    let second = layout_text_body(&tb, w, h, measurer, &table, &opts);
    let text = |r: &Result<_, LayoutError>| match r {
        Ok(v) => canon.layout(v).to_text(),
        Err(e) => format!("{e:?}"),
    };
    assert!(text(&first) == text(&second), "two layouts of the same body differ");
    if let Ok(r) = &first {
        assert!(r.lines.len() <= chars + paras + 1, "{} lines for {chars} characters", r.lines.len());
    }
    let _ = format_auto_num("romanUcPeriod", i64::from(sel) * 37 - 100);
}

/// Seeds: a few bodies (the generator reads zeros past the end, so short inputs are bodies too).
pub fn text_layout_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for sel in [0u8, 4, 12, 28, 31] {
        let mut v = vec![sel];
        for i in 0..400u32 {
            v.push(u8::try_from((i * 37 + u32::from(sel)) % 251).unwrap_or(0));
        }
        out.push(v);
    }
    out
}
