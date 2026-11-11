//! No panic on hostile input for D2b-F10 (record `docs/modules/uo-docx-d2b/F10.md`, "Security"):
//! corpus documents x hostile layout-family commands mixed with the D2a commands (items past every
//! range, labels with quotes and backslashes, TOC options with NaN/infinite/1e300 levels, a layout
//! oracle that answers run references past every story, `FieldResult(u32::MAX)`, candidates outside
//! the body, short or long page vectors, and every kind of error), undo/redo storms, then a save
//! that reopens. By default 30 documents x 60 steps (seconds); `UO_NOPANIC_BIG=1` raises it to
//! 250 x 150.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f10_support;

use d2a_support::{Cx, Step, next_step, open, run_step, sample};
use d2b_f10_support::ScriptOracle;
use d2b_f10_support::hostile::{cross_ref, numbering, toc_dialog};
use uo_docx::ids::{DrawingId, HfPartId};
use uo_docx::session::ops::references::{
    CaptionOptions, CaptionPosition, CaptionSettings, DocInfoKind, FieldKind, FieldScopeSel, ReferenceCmd, TableOfFiguresOptions, TocMode,
    TocTemplateId,
};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::{ClipboardRing, Command, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_docx::view::{BlockPath, DocRef, HyphenCandidate, LayoutErrorKind, RunRef, RunSub, StoryRef};
use uo_testkit::generate::Rng;

const STRS: [&str; 10] = ["", "x", "a b", "q\"uote", "back\\slash", "\u{fffd}", "\u{1F600}", "Figure", "none", "\t\n"];
const LEVELS: [f64; 9] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1.0, 0.0, 1.0, 9.0, 2.5];

fn s(rng: &mut Rng) -> String {
    STRS[rng.below(STRS.len())].to_owned()
}

fn story(rng: &mut Rng) -> StoryRef {
    match rng.below(5) {
        0 => StoryRef::Body,
        1 => StoryRef::HeaderFooter(HfPartId(u32::try_from(rng.below(5)).unwrap())),
        2 => StoryRef::Note(uo_docx::view::NoteKind::Footnote, uo_docx::js::NumKey::new(rng.below(4) as f64)),
        3 => StoryRef::Textbox(DrawingId(u32::try_from(rng.below(6)).unwrap())),
        _ => StoryRef::Context,
    }
}

fn path(rng: &mut Rng) -> BlockPath {
    let mut p = BlockPath::root(u32::try_from(rng.below(40)).unwrap().wrapping_sub(u32::from(rng.below(8) == 0)));
    for _ in 0..rng.below(3) {
        p = p.cell_block(u32::try_from(rng.below(4)).unwrap(), u32::try_from(rng.below(4)).unwrap(), u32::try_from(rng.below(6)).unwrap());
    }
    p
}

fn run_ref(rng: &mut Rng) -> RunRef {
    let sub = match rng.below(4) {
        0 => RunSub::FieldResult(u32::MAX),
        1 => RunSub::FieldResult(u32::try_from(rng.below(3)).unwrap()),
        2 => RunSub::GroupItem(u32::try_from(rng.below(3)).unwrap()),
        _ => RunSub::None,
    };
    RunRef { story: story(rng), path: path(rng), run: [0, 1, 5, u32::MAX][rng.below(4)], sub }
}

/// An oracle that lies: wild references, wrong lengths, errors.
fn hostile_oracle(seed: u32) -> ScriptOracle {
    let mut o = ScriptOracle::new();
    let mut rng = Rng::new(seed);
    o.fields = Box::new(move |_, _| match rng.below(8) {
        0 => Err(d2b_f10_support::layout_error(LayoutErrorKind::Budget)),
        1 => Err(d2b_f10_support::layout_error(LayoutErrorKind::Internal)),
        2 => Ok(Vec::new()),
        _ => Ok((0..rng.below(12)).map(|_| (run_ref(&mut rng), Utf16::from(STRS[rng.below(STRS.len())]))).collect()),
    });
    let mut r2 = Rng::new(seed ^ 0x5555);
    o.pages = Box::new(move |refs| match r2.below(6) {
        0 => Err(d2b_f10_support::layout_error(LayoutErrorKind::Limit)),
        1 => Ok(Vec::new()),
        2 => Ok(vec![Some(f64::NAN); refs.len() + 3]),
        _ => Ok(refs.iter().map(|_| [None, Some(1.0), Some(-3.0), Some(1e300)][r2.below(4)]).collect()),
    });
    o
}

fn push_hyph(o: &mut ScriptOracle, rng: &mut Rng) {
    for _ in 0..rng.below(4) {
        let r = match rng.below(5) {
            0 => Err(d2b_f10_support::layout_error(LayoutErrorKind::Budget)),
            1 => Ok(None),
            _ => Ok(Some(HyphenCandidate { at: DocRef { story: story(rng), path: path(rng), offset: [0, 1, 7, u32::MAX][rng.below(4)] } })),
        };
        o.hyph.push_back(r);
    }
}

fn hostile(rng: &mut Rng, i: usize) -> ReferenceCmd {
    match rng.below(24) {
        0 => ReferenceCmd::InsertField { kind: [FieldKind::Page, FieldKind::NumPages, FieldKind::Date][rng.below(3)] },
        1 => {
            ReferenceCmd::InsertDateTime { pattern: s(rng).encode_utf16().chain([0xD800]).collect(), update: rng.below(2) == 0, lang: None }
        }
        2 => ReferenceCmd::InsertDocumentInfo {
            kind: [DocInfoKind::Author, DocInfoKind::Title, DocInfoKind::FileName, DocInfoKind::FilePath][rng.below(4)],
            file_name: s(rng),
        },
        3 => ReferenceCmd::InsertAlignmentTab { alignment: s(rng), relative_to: s(rng), leader: s(rng) },
        4 | 5 => ReferenceCmd::UpdateFields {
            scope: [FieldScopeSel::Selection, FieldScopeSel::All][rng.below(2)],
            toc: [TocMode::Pages, TocMode::All][rng.below(2)],
        },
        6 | 7 => ReferenceCmd::InsertCrossReference(cross_ref(i)),
        8 => ReferenceCmd::InsertTableOfFigures(TableOfFiguresOptions {
            label: s(rng),
            include_label: [None, Some(true), Some(false)][rng.below(3)],
            page_numbers: [None, Some(false)][rng.below(2)],
            right_align: [None, Some(false)][rng.below(2)],
            leader: Some(s(rng)),
            hyperlinks: [None, Some(false)][rng.below(2)],
            format: None,
        }),
        9 => ReferenceCmd::UpdateTableOfFigures,
        10 => ReferenceCmd::UpdateToc { mode: [TocMode::Pages, TocMode::All][rng.below(2)] },
        11 => ReferenceCmd::InsertTocTemplate { id: [TocTemplateId::Auto1, TocTemplateId::Auto2, TocTemplateId::Manual][rng.below(3)] },
        12 | 13 => ReferenceCmd::InsertTableOfContents { opts: toc_dialog(i), replace: [None, Some(true), Some(false)][rng.below(3)] },
        14 => ReferenceCmd::RemoveTableOfContents,
        15 => ReferenceCmd::SetTocLevel { level: LEVELS[rng.below(LEVELS.len())] },
        16 => ReferenceCmd::MarkTocEntry { text: s(rng), identifier: Some(s(rng)), level: Some(LEVELS[rng.below(LEVELS.len())]) },
        17 => match rng.below(5) {
            0 => ReferenceCmd::SetCaptionSettings(CaptionSettings::default()),
            1 => ReferenceCmd::AddCaptionLabel { name: s(rng) },
            2 => ReferenceCmd::DeleteCaptionLabel { name: s(rng) },
            3 => ReferenceCmd::SetCaptionNumbering { label: s(rng), numbering: numbering(i) },
            _ => ReferenceCmd::SetAutoCaption { auto: Some((s(rng), [CaptionPosition::Above, CaptionPosition::Below][rng.below(2)])) },
        },
        18 | 19 => ReferenceCmd::InsertCaption(CaptionOptions {
            label: s(rng),
            title: Some(s(rng)),
            position: [None, Some(CaptionPosition::Above), Some(CaptionPosition::Below)][rng.below(3)],
            exclude_label: [None, Some(true)][rng.below(2)],
            numbering: [None, Some(numbering(i))][rng.below(2)],
        }),
        20 => ReferenceCmd::NextManualHyphenation,
        21 => ReferenceCmd::ApplyManualHyphenation { letters: [0.0, 1.0, 3.0, f64::from(u32::MAX), f64::NAN, -1.0][rng.below(6)] },
        22 => ReferenceCmd::SkipManualHyphenation,
        _ => ReferenceCmd::EndManualHyphenation,
    }
}

#[test]
fn hostile_f10_commands_on_the_corpus() {
    let big = std::env::var("UO_NOPANIC_BIG").is_ok();
    let (n_docs, n_steps) = if big { (250usize, 150usize) } else { (30usize, 60usize) };
    let docs = sample(if big { 10 } else { 60 });
    let mut cx = Cx::new();
    let (mut lives, mut reopened, mut errs, mut oks) = (0usize, 0usize, 0usize, 0usize);
    let t0 = std::time::Instant::now();
    // UO_NOPANIC_DOC=<i>: only that document, saving and reopening after every step (finds the
    // command that writes a file that does not reopen)
    let only: Option<usize> = std::env::var("UO_NOPANIC_DOC").ok().and_then(|v| v.parse().ok());
    for (i, f) in docs.iter().take(n_docs).enumerate() {
        if only.is_some_and(|o| o != i) {
            continue;
        }
        let bytes = std::fs::read(f).unwrap();
        let Some(mut sess) = open(bytes) else { continue };
        let mut rng = Rng::new(0xf10 + u32::try_from(i).unwrap());
        let mut oracle = hostile_oracle(0xabc + u32::try_from(i).unwrap());
        let mut clip = ClipboardRing::default();
        let mut last = String::new();
        for k in 0..n_steps {
            match rng.below(10) {
                0..=5 => {
                    if rng.below(3) == 0 {
                        push_hyph(&mut oracle, &mut rng);
                    }
                    let cmd = hostile(&mut rng, k + i);
                    let shown = only.map(|_| format!("{cmd:?}"));
                    last = shown.clone().unwrap_or_default();
                    match sess.exec(Command::Reference(cmd), &mut oracle.cx(&mut clip)) {
                        Ok(_) => oks += 1,
                        Err(_) => errs += 1,
                    }
                }
                6 => run_step(&mut sess, Step::Cmd(Command::Settings(SettingsCmd::SetTracking { on: rng.below(2) == 0 })), &mut cx),
                _ => {
                    let st = next_step(&mut rng, &sess);
                    run_step(&mut sess, st, &mut cx);
                }
            }
            if only.is_some()
                && let Ok(b) = sess.save_docx()
                && let Err(e) = Session::open(b.clone(), OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default()))
            {
                let pkg = uo_opc::Package::open(&b, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
                let xml = pkg.part_text("word/document.xml").unwrap().unwrap();
                let off = format!("{e:?}")
                    .split("offset: ")
                    .nth(1)
                    .and_then(|r| r.split(',').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0);
                let from = off.saturating_sub(200);
                println!(
                    "step {k}: after {last}
  the saved file does not reopen ({e:?}):
  ...{}",
                    xml.get(from..(off + 120).min(xml.len())).unwrap_or("")
                );
                break;
            }
            if k % 30 == 29 {
                for u in 0..40 {
                    let _ = sess.undo(&mut oracle.cx(&mut clip));
                    if only.is_some()
                        && let Ok(b) = sess.save_docx()
                        && Session::open(b, OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).is_err()
                    {
                        println!("step {k}: undo {u} (last command {last}): the saved file does not reopen");
                        break;
                    }
                }
                for r in 0..40 {
                    let _ = sess.redo(&mut oracle.cx(&mut clip));
                    if only.is_some()
                        && let Ok(b) = sess.save_docx()
                        && Session::open(b, OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).is_err()
                    {
                        println!("step {k}: redo {r} (last command {last}): the saved file does not reopen");
                        break;
                    }
                }
            }
            let reopens = |s: &mut Session| {
                s.save_docx()
                    .is_ok_and(|b| Session::open(b, OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).is_ok())
            };
            let before_queries = only.is_some() && reopens(&mut sess);
            let _ = sess.caption_labels();
            let _ = sess.table_of_contents_options();
            let _ = sess.table_of_figures_options();
            let _ = sess.in_table_of_contents();
            let _ = sess.toc_level();
            let _ = sess.cross_reference_items(uo_docx::session::ops::references::CrossRefType::Heading, None);
            let _ = sess.cross_reference_items(uo_docx::session::ops::references::CrossRefType::Footnote, None);
            let _ = sess.cross_reference_items(uo_docx::session::ops::references::CrossRefType::Caption, Some("Figure"));
            let _ = sess.next_caption_number("Figure");
            if only.is_some() && before_queries && !reopens(&mut sess) {
                println!("step {k}: a QUERY broke the saved file");
            }
        }
        if only.is_some() {
            let ok = |s: &mut Session| {
                s.save_docx()
                    .is_ok_and(|b| Session::open(b, OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).is_ok())
            };
            println!("before the preview: reopens {}", ok(&mut sess));
        }
        let _ = sess.toc_template_preview(TocTemplateId::Auto1);
        if only.is_some() {
            let ok = |s: &mut Session| {
                s.save_docx()
                    .is_ok_and(|b| Session::open(b, OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())).is_ok())
            };
            println!("after the preview: reopens {}", ok(&mut sess));
        }
        lives += 1;
        match sess.save_docx() {
            Ok(b) => match Session::open(b, OpenOptions::default(), Box::new(uo_docx::session::FixedEnv::default())) {
                Ok(_) => reopened += 1,
                Err(e) => println!("doc {i} ({}): the saved file does not reopen: {e:?}", f.display()),
            },
            Err(e) => println!("doc {i} ({}): save failed: {e:?}", f.display()),
        }
    }
    println!(
        "nopanic d2b-f10: {lives} documents x {n_steps} hostile steps ({oks} ok, {errs} errors), {reopened} saves reopened, {} ms",
        t0.elapsed().as_millis()
    );
    assert!(only.is_some() || lives > 10);
    assert_eq!(lives, reopened);
}
