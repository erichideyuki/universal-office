//! Memory gate of D2b-F9 (record `docs/modules/uo-docx-d2b/F9.md`, "Memory"):
//!
//! - **M1**: whole session lives with the F9 commands (open -> scripted and hostile find, replace,
//!   go to, headings, search, multi-selections, undo, redo, save -> drop) return the live bytes
//!   exactly; a search that runs out of its budget too.
//! - **M3**: find-all on the generated 500-page document (15,000 paragraphs): the bytes allocated and
//!   the peak of each command, and what the session KEEPS after it (no state beyond the last query:
//!   counting keeps nothing, find-next keeps nothing, the Navigation pane keeps its result list, a
//!   multi-selection its ranges); numbers printed with `--nocapture`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f9_support;
mod support;

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

use d2a_support::{Cx, Step, next_step, run_step, sample};
use d2b_f9_support::hostile::hostile_f9;
use uo_alloc::{assert_no_leak, measure};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::find::engine::FindQuery;
use uo_docx::session::ops::find::{Direction, FindCmd, Within};
use uo_docx::session::ops::navigation::{FindOptions, NavigationCmd};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, OpenOptions, Session};
use uo_testkit::generate::Rng;

fn q(text: &str) -> FindQuery {
    FindQuery { text: text.encode_utf16().collect(), ..FindQuery::default() }
}

fn wild(text: &str) -> FindQuery {
    FindQuery { text: text.encode_utf16().collect(), wildcards: true, ..FindQuery::default() }
}

fn f9_life(bytes: &[u8], seed: u32) -> usize {
    let mut cx = Cx::new();
    let Ok(mut s) = Session::open(bytes.to_vec(), OpenOptions::default(), Box::new(FixedEnv::default())) else { return 0 };
    s.set_find_budget(Some(300_000));
    let mut rng = Rng::new(seed);
    let scripted = [
        Command::Find(FindCmd::FindNext { query: q("a"), direction: Direction::All }),
        Command::Find(FindCmd::ReplaceAll {
            query: q("e"),
            replacement: "E^p".encode_utf16().collect(),
            within: Within::Document,
            replace_format: None,
        }),
        Command::Find(FindCmd::SelectAllMatches { query: wild("<[a-z]@>"), within: Within::Document }),
        Command::Navigation(NavigationCmd::Search { text: "o".encode_utf16().collect(), opts: FindOptions::default() }),
        Command::Navigation(NavigationCmd::GoToSearchResult { index: 1 }),
        Command::Navigation(NavigationCmd::DemoteHeading { index: 0 }),
        Command::Navigation(NavigationCmd::MoveHeading { index: 1, before: 0 }),
        Command::Navigation(NavigationCmd::SelectAll),
        Command::Find(FindCmd::FindNext { query: wild("(a@)@b"), direction: Direction::Up }),
    ];
    for c in scripted {
        run_step(&mut s, Step::Cmd(c), &mut cx);
    }
    for i in 0..60 {
        let step = if i % 3 == 0 { next_step(&mut rng, &s) } else { Step::Cmd(hostile_f9(&mut rng)) };
        run_step(&mut s, step, &mut cx);
    }
    let _ = s.count_matches(&q("de"), Within::Document);
    let _ = s.headings();
    for _ in 0..30 {
        run_step(&mut s, Step::Undo, &mut cx);
    }
    for _ in 0..30 {
        run_step(&mut s, Step::Redo, &mut cx);
    }
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_f9_session_lives_return_the_memory() {
    let docs = sample(150);
    for (i, f) in docs.iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(f9_life(&bytes, 0xf9 + u32::try_from(i).unwrap())));
    }
    println!("M1 d2b-f9: {} session lives", docs.len());
    assert!(docs.len() > 10);
}

/// The generated 500-page document of the D2a and F1 gates (15,000 paragraphs, tables, sections,
/// bookmarks, fields), with a heading every 100 paragraphs.
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
        if i % 100 == 0 {
            body.push_str("<w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr>");
        } else if i % 300 == 299 {
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
fn m3_find_all_on_a_500_page_document() {
    let bytes = big_document();
    let mut s = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    assert_eq!(s.document().body.paras.len(), 15_000);
    let mut cx = Cx::new();
    s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(7_500, 3), focus: DocPosition::new(7_500, 3) }), &mut cx.cx())
        .unwrap();
    let mut rows: Vec<String> = Vec::new();
    let mut run = |name: &str, s: &mut Session, cx: &mut Cx, f: &mut dyn FnMut(&mut Session, &mut Cx)| {
        let t0 = Instant::now();
        let ((), u) = measure(|| f(s, cx));
        let ms = t0.elapsed().as_millis();
        rows.push(format!(
            "{name}: allocated {} KiB, peak {} KiB, kept {} B, {ms} ms",
            u.allocated >> 10,
            u.peak_above_start >> 10,
            u.live_delta
        ));
        u
    };
    // counting keeps nothing
    let u = run("countMatches 'Paragraph' (15,000 hits)", &mut s, &mut cx, &mut |s, _| {
        assert_eq!(s.count_matches(&q("Paragraph"), Within::Document).unwrap(), 15_000);
    });
    assert_eq!(u.live_delta, 0);
    let u = run("countMatches wildcard '<[0-9]@>'", &mut s, &mut cx, &mut |s, _| {
        black_box(s.count_matches(&wild("<[0-9]@>"), Within::Document).unwrap());
    });
    assert_eq!(u.live_delta, 0);
    let u = run("countMatches absent 'zzz'", &mut s, &mut cx, &mut |s, _| {
        assert_eq!(s.count_matches(&q("zzz"), Within::Document).unwrap(), 0);
    });
    assert_eq!(u.live_delta, 0);
    // find next: the selection moves, nothing else is kept
    let u = run("findNextMatch 'generated' (down)", &mut s, &mut cx, &mut |s, cx| {
        drop(s.exec(Command::Find(FindCmd::FindNext { query: q("generated"), direction: Direction::Down }), &mut cx.cx()).unwrap());
    });
    assert!(u.live_delta <= 256, "find next kept {} B", u.live_delta);
    run("findNextMatch 'zzz' (all, wraps the whole document)", &mut s, &mut cx, &mut |s, cx| {
        drop(s.exec(Command::Find(FindCmd::FindNext { query: q("zzz"), direction: Direction::All }), &mut cx.cx()).unwrap());
    });
    // select all matches: the multi-selection keeps its ranges (16 B each)
    let u = run("selectAllMatches 'bold.' (15,000 ranges)", &mut s, &mut cx, &mut |s, cx| {
        drop(s.exec(Command::Find(FindCmd::SelectAllMatches { query: q("bold."), within: Within::Document }), &mut cx.cx()).unwrap());
    });
    assert!(u.live_delta <= 15_000 * 16 + 4096, "multi-selection kept {} B", u.live_delta);
    assert_eq!(s.selection_ranges().len(), 15_000);
    // the Navigation pane's search keeps its list
    run("search 'Paragraph 7' (Navigation pane)", &mut s, &mut cx, &mut |s, cx| {
        drop(
            s.exec(
                Command::Navigation(NavigationCmd::Search { text: "Paragraph 7".encode_utf16().collect(), opts: FindOptions::default() }),
                &mut cx.cx(),
            )
            .unwrap(),
        );
    });
    // headings
    let u = run("getHeadings (150 headings)", &mut s, &mut cx, &mut |s, _| {
        assert_eq!(s.headings().len(), 150);
    });
    assert_eq!(u.live_delta, 0);
    // replace all: one undo step
    run("replaceAllMatches 'words' -> 'WORDS' (15,000 replacements)", &mut s, &mut cx, &mut |s, cx| {
        drop(
            s.exec(
                Command::Find(FindCmd::ReplaceAll {
                    query: q("words"),
                    replacement: "WORDS".encode_utf16().collect(),
                    within: Within::Document,
                    replace_format: None,
                }),
                &mut cx.cx(),
            )
            .unwrap(),
        );
    });
    for r in &rows {
        println!("M3 d2b-f9 on 15,000 paragraphs: {r}");
    }
}
