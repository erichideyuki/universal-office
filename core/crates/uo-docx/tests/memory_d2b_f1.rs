//! Memory gate of D2b-F1 (record `docs/modules/uo-docx-d2b/F1.md`, "Memory"):
//!
//! - **M1**: whole session lives with the F1 commands (open -> scripted and hostile formatting,
//!   undo, redo, save -> drop) return the live bytes exactly.
//! - **M2**: 400 x (open -> 60 F1 commands -> 30 undo -> 30 redo -> save -> close) over corpus
//!   documents: the live-bytes series is flat after warm-up.
//! - **M3**: one formatting command on a generated 500-page document (15,000 paragraphs): each
//!   command allocates <= 64 KiB (U19: the step is O(touched)); numbers printed with `--nocapture`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f1_support;
#[allow(unused_imports)]
use crate::support;

use std::fmt::Write as _;
use std::hint::black_box;

use d2a_support::{Cx, Step, run_step, sample};
use d2b_f1_support::hostile::hostile_f1;
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::format::{FormatCmd, RunToggle};
use uo_docx::session::ops::paragraph::{LayoutArgs, ParagraphCmd, SpacingArg};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, HistoryLimits, OpenOptions, Session};
use uo_testkit::generate::Rng;

fn open_with(bytes: Vec<u8>, history: HistoryLimits) -> Option<Session> {
    Session::open(bytes, OpenOptions { history, ..OpenOptions::default() }, Box::new(FixedEnv::default())).ok()
}

fn select_some(s: &mut Session, cx: &mut Cx, rng: &mut Rng) {
    let paras = s.paras().map_or(0, uo_docx::pvec::PVec::len);
    if paras == 0 {
        return;
    }
    let pa = rng.below(paras);
    let len = s.paras().and_then(|p| p.get(pa)).map_or(0, uo_docx::session::text::para_length);
    let a = DocPosition::new(u32::try_from(pa).unwrap(), u32::try_from(rng.below(len + 1)).unwrap());
    let f = DocPosition::new(a.para, u32::try_from(rng.below(len + 1)).unwrap());
    run_step(s, Step::Cmd(Command::Text(TextCmd::SetSelection { anchor: a, focus: f })), cx);
}

fn f1_life(bytes: &[u8], seed: u32) -> usize {
    let mut cx = Cx::new();
    let Some(mut s) = open_with(bytes.to_vec(), HistoryLimits::default()) else { return 0 };
    let mut rng = Rng::new(seed);
    for i in 0..60 {
        if i % 3 == 0 {
            select_some(&mut s, &mut cx, &mut rng);
        }
        run_step(&mut s, Step::Cmd(hostile_f1(&mut rng)), &mut cx);
    }
    for _ in 0..30 {
        run_step(&mut s, Step::Undo, &mut cx);
    }
    for _ in 0..30 {
        run_step(&mut s, Step::Redo, &mut cx);
    }
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_f1_session_lives_return_the_memory() {
    let docs = sample(150);
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(f1_life(&bytes, 0xf1 + u32::try_from(i).unwrap())));
    }
    println!("M1 d2b-f1: {} session lives", docs.len());
    assert!(docs.len() > 10);
}

#[test]
fn m2_soak_f1_edit_undo_redo_save() {
    let docs: Vec<Vec<u8>> = sample(250).iter().map(|f| std::fs::read(f).unwrap()).filter(|b| b.len() < 60_000).take(10).collect();
    assert!(docs.len() >= 5);
    let mut out = 0usize;
    let series = soak(400, 25, |i| {
        let bytes = &docs[i % docs.len()];
        out = out.wrapping_add(f1_life(bytes, u32::try_from(i % 7).unwrap()));
    });
    println!("M2 soak d2b-f1: 400 lives over {} documents: {:?}", docs.len(), &series[..4]);
    black_box(out);
    assert_flat(&series, 1, 0);
}

/// A generated 500-page document (U19): 15,000 paragraphs, a bookmark every 7 paragraphs, a 3 × 3
/// table every 75, a section break every 300, a PAGEREF field every 30.
fn big_document() -> Vec<u8> {
    let mut body = String::with_capacity(4 << 20);
    for i in 0..15_000u32 {
        if i % 75 == 37 {
            body.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/><w:gridCol w:w=\"3000\"/></w:tblGrid>");
            for r in 0..3 {
                body.push_str("<w:tr>");
                for c in 0..3 {
                    let _ = write!(
                        body,
                        "<w:tc><w:tcPr><w:tcW w:w=\"3000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>cell {r}.{c}</w:t></w:r></w:p></w:tc>"
                    );
                }
                body.push_str("</w:tr>");
            }
            body.push_str("</w:tbl>");
        }
        body.push_str("<w:p>");
        if i % 300 == 299 {
            body.push_str("<w:pPr><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/><w:type w:val=\"nextPage\"/></w:sectPr></w:pPr>");
        }
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkStart w:id=\"{i}\" w:name=\"_Toc{i:08}\"/>");
        }
        let _ =
            write!(body, "<w:r><w:t xml:space=\"preserve\">Paragraph {i} of the generated document with some words to wrap. </w:t></w:r>");
        if i % 30 == 3 {
            let _ = write!(body, "<w:fldSimple w:instr=\" PAGEREF _Toc{:08} \\h \"><w:r><w:t>1</w:t></w:r></w:fldSimple>", i / 7 * 7);
        }
        if i % 7 == 0 {
            let _ = write!(body, "<w:bookmarkEnd w:id=\"{i}\"/>");
        }
        body.push_str("<w:r><w:rPr><w:b/></w:rPr><w:t>bold.</w:t></w:r></w:p>");
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

#[test]
fn m3_per_command_on_a_500_page_document() {
    const BUDGET: u64 = 64 << 10;
    let bytes = big_document();
    let mut s = open_with(bytes, HistoryLimits::default()).unwrap();
    assert_eq!(s.document().body.paras.len(), 15_000);
    let mut cx = Cx::new();
    let at = DocPosition::new(7_500, 10);
    let to = DocPosition::new(7_500, 30);
    s.exec(Command::Text(TextCmd::SetSelection { anchor: at, focus: to }), &mut cx.cx()).unwrap();
    // warm-up
    for _ in 0..10 {
        s.exec(Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() }), &mut cx.cx()).unwrap();
    }
    let mut worst = [0u64; 4];
    for round in 0..20 {
        let cmds = [
            Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::Italic, button: "single".into() }),
            Command::Format(FormatCmd::SetFontSize { pt: 10.0 + f64::from(round) }),
            Command::Paragraph(ParagraphCmd::SetAlignment { alignment: if round % 2 == 0 { "center" } else { "left" }.to_owned() }),
            Command::Paragraph(ParagraphCmd::SetParagraphLayout {
                v: LayoutArgs {
                    left: Some(f64::from(round) * 10.0),
                    before: Some(SpacingArg::Twips(f64::from(round))),
                    ..LayoutArgs::default()
                },
            }),
        ];
        for (k, c) in cmds.into_iter().enumerate() {
            let (r, u) = measure(|| s.exec(c, &mut cx.cx()));
            drop(r);
            worst[k] = worst[k].max(u.allocated);
        }
    }
    println!(
        "M3 d2b-f1 per command on 15,000 paragraphs: toggle {} B, font size {} B, alignment {} B, layout boxes {} B",
        worst[0], worst[1], worst[2], worst[3]
    );
    for (k, w) in worst.iter().enumerate() {
        assert!(*w <= BUDGET, "command {k}: {w} B allocated");
    }
}
