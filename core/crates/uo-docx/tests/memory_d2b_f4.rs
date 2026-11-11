//! Memory gate of D2b-F4 (sections, page setup, headers/footers, page colour, text layout options;
//! record `core/docs/modules/uo-docx-d2b/F4.md`):
//!
//! - **M1**: whole session lives with the F4 commands (plausible, hostile and the error paths: the
//!   locked editor, parts past the end, a link that is already made, a column count of NaN) over
//!   corpus documents and the constructed ones, with undo and redo and a save, return the live bytes
//!   exactly.
//! - **M2**: a soak of unlink / relink / undo cycles and page-colour changes stays flat after the
//!   warm-up (the parts an unlink creates stay in the index, as the TS `hfParts`, up to the cap).
//! - **M3**: the cost of one section command on a generated 500-page document with 15,000 paragraphs
//!   and 50 sections: a section break, a page break, a column preset, a page setup of the whole
//!   document.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f4_support;
#[allow(unused_imports)]
use crate::support;

use std::fmt::Write as _;
use std::hint::black_box;

use d2a_support::{Cx, sample};
use d2b_f4_support::{doc, doc_with_props, f4_cmd, hostile_cmd};
use uo_alloc::{assert_no_leak, measure};
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::format::ColorArg;
use uo_docx::session::ops::header_footer::hf_templates::HfGallery;
use uo_docx::session::ops::header_footer::{HeaderFooterCmd, HfAt, HfEnter, HfKind, HfReenter, HfSlot};
use uo_docx::session::ops::page::watermark::WatermarkArg;
use uo_docx::session::ops::page::{PageCmd, PageLang};
use uo_docx::session::ops::sections::section_ops::{ColumnPreset, Orientation};
use uo_docx::session::ops::sections::{ColumnsArg, PageSetup, SectionApplyTo, SectionCmd, SectionType};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, HistoryLimits, OpenOptions, Session};
use uo_docx::write::emit::background::{WatermarkLayout, WatermarkPictureSpec, WatermarkTextSpec};
use uo_testkit::generate::Rng;

fn open_with(bytes: Vec<u8>, history: HistoryLimits) -> Option<Session> {
    Session::open(bytes, OpenOptions { history, ..OpenOptions::default() }, Box::new(FixedEnv::default())).ok()
}

fn life(bytes: &[u8], seed: u32, hostile: bool) -> usize {
    let mut cx = Cx::new();
    let Some(mut s) = open_with(bytes.to_vec(), HistoryLimits::default()) else { return 0 };
    let mut rng = Rng::new(seed);
    for _ in 0..60 {
        let c = if hostile { hostile_cmd(&mut rng, &s) } else { f4_cmd(&mut rng, &s) };
        let _ = s.exec(c, &mut cx.cx());
    }
    for _ in 0..30 {
        s.undo(&mut cx.cx());
    }
    for _ in 0..30 {
        s.redo(&mut cx.cx());
    }
    black_box((s.document().sections.len(), s.document().hf.len(), s.even_odd_headers()));
    s.save_docx().map_or(0, |b| b.len())
}

#[test]
fn m1_session_lives_with_the_f4_commands_return_the_memory() {
    let mut n = 0;
    for (i, f) in sample(80).iter().enumerate() {
        let bytes = std::fs::read(f).unwrap();
        assert_no_leak(|| black_box(life(&bytes, 0xf400 + u32::try_from(i).unwrap(), i % 2 == 1)));
        n += 1;
    }
    for (i, bytes) in [doc(4, 2), doc(3, 1), doc(6, 2)].iter().enumerate() {
        assert_no_leak(|| black_box(life(bytes, 0xf4a0 + u32::try_from(i).unwrap(), i == 2)));
        n += 1;
    }
    // the error and refusal paths of every command on one session
    assert_no_leak(|| {
        let mut cx = Cx::new();
        let mut s = open_with(doc(4, 2), HistoryLimits::default()).unwrap();
        let at = |section: u32, slot| HfAt { section, slot };
        for c in [
            Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious { on: true, kind: None, at: at(0, HfSlot::Default), reenter: None }),
            Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious {
                on: true,
                kind: Some(HfKind::Footer),
                at: at(1, HfSlot::Default),
                reenter: None,
            }),
            Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious {
                on: false,
                kind: Some(HfKind::Header),
                at: at(1, HfSlot::Default),
                reenter: None,
            }),
            Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious {
                on: false,
                kind: Some(HfKind::Header),
                at: at(1, HfSlot::Default),
                reenter: None,
            }),
            Command::HeaderFooter(HeaderFooterCmd::RemoveHeaderFooter { kind: HfKind::Footer, at: at(9, HfSlot::First) }),
            Command::HeaderFooter(HeaderFooterCmd::RemovePageNumbers { at: at(0, HfSlot::Default) }),
            Command::HeaderFooter(HeaderFooterCmd::SetDifferentOddEven { on: false, reenter: None }),
            Command::HeaderFooter(HeaderFooterCmd::SetDifferentOddEven { on: true, reenter: None }),
            Command::Section(SectionCmd::InsertSectionBreak { kind: SectionType::Continuous }),
            Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Preset(ColumnPreset::Three)))),
            Command::Section(SectionCmd::SetPageSetup(Box::new(PageSetup {
                orientation: Some(Orientation::Landscape),
                apply_to: Some(SectionApplyTo::Forward),
                ..PageSetup::default()
            }))),
            Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(2, 3), focus: DocPosition::new(3, 4) }),
            Command::Section(SectionCmd::InsertBlankPage),
            Command::Section(SectionCmd::InsertPageBreak),
            Command::Page(PageCmd::SetPageColor { color: None }),
            Command::Settings(SettingsCmd::SetProtected { on: true }),
            Command::Section(SectionCmd::InsertColumnBreak),
            Command::HeaderFooter(HeaderFooterCmd::SetDifferentFirstPage { on: true, at: at(0, HfSlot::Default), reenter: None }),
        ]
        .into_iter()
        .chain(refusals())
        {
            let _ = s.exec(c, &mut cx.cx());
        }
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        s.clear_caches();
        black_box(s.save_docx().is_ok())
    });
    println!("M1 d2b-f4: {n} session lives + the error paths");
    assert!(n >= 6);
}

#[test]
fn m2_soak_unlink_relink_undo_and_colour_is_flat() {
    let cycle = |s: &mut Session, cx: &mut Cx| {
        let at = HfAt { section: 1, slot: HfSlot::Default };
        let mk = |on| Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious { on, kind: Some(HfKind::Header), at, reenter: None });
        let _ = s.exec(mk(false), &mut cx.cx());
        let _ = s.exec(mk(true), &mut cx.cx());
        s.undo(&mut cx.cx());
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        s.redo(&mut cx.cx());
        let _ = s.exec(Command::Page(PageCmd::SetPageColor { color: Some(ColorArg::Hex("ffcc00".into())) }), &mut cx.cx());
        s.undo(&mut cx.cx());
        let _ = s.exec(Command::Section(SectionCmd::InsertSectionBreak { kind: SectionType::Continuous }), &mut cx.cx());
        s.undo(&mut cx.cx());
    };
    let mut cx = Cx::new();
    let mut s = open_with(doc(6, 2), HistoryLimits::default()).unwrap();
    for _ in 0..30 {
        cycle(&mut s, &mut cx);
    }
    let base = uo_alloc::thread_stats().live;
    for _ in 0..300 {
        cycle(&mut s, &mut cx);
    }
    let after = uo_alloc::thread_stats().live;
    let parts = s.document().hf.len();
    println!("M2 d2b-f4: live {base} -> {after} B after 300 cycles, {parts} header parts");
    // the unlink parts are the only growth (each is small); the history is capped
    assert!(after - base < 4 << 20, "grew by {} B", after - base);
}

/// 15,000 paragraphs (500 pages), a section break every 300 paragraphs (50 sections).
fn sectioned_document() -> Vec<u8> {
    let mut body = String::with_capacity(8 << 20);
    for i in 0..15_000u32 {
        let sect = if i % 300 == 299 {
            "<w:pPr><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:pPr>"
        } else {
            ""
        };
        let _ = write!(
            body,
            "<w:p>{sect}<w:r><w:t xml:space=\"preserve\">Paragraph {i} of the generated document, with some words.</w:t></w:r></w:p>"
        );
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    pkg.save(&uo_opc::package::SaveOptions::default()).unwrap()
}

#[test]
fn m3_section_commands_on_a_500_page_document() {
    let bytes = sectioned_document();
    let (s, open_usage) = measure(|| open_with(bytes.clone(), HistoryLimits::default()).unwrap());
    let mut s = s;
    let model = u64::try_from(open_usage.live_delta).unwrap();
    assert_eq!(s.document().sections.len(), 51);
    let mut cx = Cx::new();
    let mut run = |name: &str, c: Command, budget: u64| {
        s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(7000, 5), focus: DocPosition::new(7000, 5) }), &mut cx.cx())
            .unwrap();
        let t = std::time::Instant::now();
        let (r, u) = measure(|| s.exec(c, &mut cx.cx()));
        r.unwrap();
        println!("M3 d2b-f4: {name}: {} B allocated in {} ms", u.allocated, t.elapsed().as_millis());
        assert!(u.allocated <= budget, "{name} allocated {} B (budget {budget} B)", u.allocated);
    };
    // one section break: a split paragraph, a new section, `syncSections` (no 15,000-entry index rebuild)
    run("insert section break", Command::Section(SectionCmd::InsertSectionBreak { kind: SectionType::Continuous }), 256 << 10);
    run("insert page break", Command::Section(SectionCmd::InsertPageBreak), 256 << 10);
    run("columns preset", Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Preset(ColumnPreset::Two)))), 256 << 10);
    // the whole document: 52 sections' `sectPr`, the effective sections rebuilt once
    run(
        "page setup of the document",
        Command::Section(SectionCmd::SetPageSetup(Box::new(PageSetup {
            orientation: Some(Orientation::Landscape),
            apply_to: Some(SectionApplyTo::Document),
            ..PageSetup::default()
        }))),
        2 << 20,
    );
    let hist = s.history_bytes() as u64;
    assert!(hist <= model, "history {hist} B against a model of {model} B");
}

/// The refusal and no-op paths of the watermark, cover page and template commands.
fn refusals() -> Vec<Command> {
    let at = HfAt { section: 0, slot: HfSlot::Default };
    let wm = |arg: WatermarkArg| Command::Page(PageCmd::SetWatermark { spec: Box::new(arg), at, lang: PageLang::En, reenter: None });
    let pic = |ext: &str, info| WatermarkArg::Picture {
        spec: WatermarkPictureSpec {
            data: std::sync::Arc::from(vec![1u8, 2].into_boxed_slice()),
            ext: ext.into(),
            title: "a".into(),
            scale: None,
            washout: false,
        },
        pixel_info: info,
    };
    let blank = WatermarkArg::Text {
        spec: WatermarkTextSpec {
            text: " ".into(),
            font: "calibri".into(),
            size_pt: None,
            color: "C0C0C0".into(),
            semitransparent: false,
            layout: WatermarkLayout::Horizontal,
            lang: None,
        },
        measured_box: None,
    };
    let tpl = |gallery, id: &str, enter| {
        Command::HeaderFooter(HeaderFooterCmd::InsertTemplate {
            gallery,
            id: id.into(),
            lang: PageLang::Es,
            enter,
            page_numbers: Some((f64::NAN, 1.0)),
        })
    };
    vec![
        wm(WatermarkArg::Gallery("nope".into())),
        wm(blank),
        wm(pic("png", None)),
        wm(pic("../x", Some([1.0, 1.0, 96.0]))),
        wm(pic("png", Some([0.0, 0.0, 0.0]))),
        Command::Page(PageCmd::RemoveWatermark { reenter: None }),
        Command::Page(PageCmd::RemoveCoverPage),
        Command::Page(PageCmd::InsertCoverPage { id: "nope".into(), lang: PageLang::En }),
        tpl(HfGallery::Header, "nope", None),
        tpl(HfGallery::Footer, "blank", None),
        tpl(HfGallery::Footer, "blank", Some(HfEnter { part: HfReenter::Part(uo_docx::ids::HfPartId(u32::MAX)), page: u32::MAX })),
        tpl(HfGallery::PageCurrent, "nope", None),
        tpl(HfGallery::PageCurrent, "plain", None),
        wm(WatermarkArg::Gallery("draft1".into())),
        Command::Page(PageCmd::InsertCoverPage { id: "band".into(), lang: PageLang::En }),
        tpl(
            HfGallery::Header,
            "band",
            Some(HfEnter { part: HfReenter::Create { kind: HfKind::Header, section: 0, slot: HfSlot::Default }, page: 0 }),
        ),
        Command::Page(PageCmd::RemoveWatermark { reenter: None }),
        Command::Page(PageCmd::RemoveCoverPage),
    ]
}

#[test]
fn m2b_soak_watermark_cover_and_template_cycles() {
    let cycle = |s: &mut Session, cx: &mut Cx| {
        let at = HfAt { section: 0, slot: HfSlot::Default };
        let wm = Command::Page(PageCmd::SetWatermark {
            spec: Box::new(WatermarkArg::Gallery("draft1".into())),
            at,
            lang: PageLang::En,
            reenter: None,
        });
        let _ = s.exec(wm, &mut cx.cx());
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        let _ = s.exec(Command::Page(PageCmd::RemoveWatermark { reenter: None }), &mut cx.cx());
        s.undo(&mut cx.cx());
        let _ = s.exec(Command::Page(PageCmd::InsertCoverPage { id: "band".into(), lang: PageLang::En }), &mut cx.cx());
        s.undo(&mut cx.cx());
        s.redo(&mut cx.cx());
        let _ = s.exec(Command::Page(PageCmd::RemoveCoverPage), &mut cx.cx());
        s.undo(&mut cx.cx());
        let tpl = Command::HeaderFooter(HeaderFooterCmd::InsertTemplate {
            gallery: HfGallery::PageCurrent,
            id: "pageXofY".into(),
            lang: PageLang::En,
            enter: None,
            page_numbers: Some((3.0, 1.0)),
        });
        let _ = s.exec(tpl, &mut cx.cx());
        s.undo(&mut cx.cx());
    };
    let mut cx = Cx::new();
    let mut s = open_with(doc_with_props(), HistoryLimits::default()).unwrap();
    for _ in 0..20 {
        cycle(&mut s, &mut cx);
    }
    let base = uo_alloc::thread_stats().live;
    for _ in 0..100 {
        cycle(&mut s, &mut cx);
    }
    let after = uo_alloc::thread_stats().live;
    let per = (after - base) / 100;
    println!("M2b d2b-f4: live {base} -> {after} B after 100 cycles ({per} B per cycle), {} header parts", s.document().hf.len());
    // an undone cover keeps its source groups and an undone picture mark its media (the TS does the same, F4-D17)
    assert!(after - base < 16 << 20, "grew by {} B", after - base);
}

#[test]
fn m3b_watermark_cover_and_template_on_a_500_page_document() {
    let bytes = sectioned_document();
    let mut s = open_with(bytes, HistoryLimits::default()).unwrap();
    let mut cx = Cx::new();
    let mut run = |name: &str, c: Command, budget: u64| {
        s.exec(Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(7000, 5), focus: DocPosition::new(7000, 5) }), &mut cx.cx())
            .unwrap();
        let t = std::time::Instant::now();
        let (r, u) = measure(|| s.exec(c, &mut cx.cx()));
        r.unwrap();
        println!("M3b d2b-f4: {name}: {} B allocated in {} ms", u.allocated, t.elapsed().as_millis());
        assert!(u.allocated <= budget, "{name} allocated {} B (budget {budget} B)", u.allocated);
    };
    let at = |section| HfAt { section, slot: HfSlot::Default };
    for section in [0u32, 25, 50] {
        run(
            "set watermark",
            Command::Page(PageCmd::SetWatermark {
                spec: Box::new(WatermarkArg::Gallery("confidential2".into())),
                at: at(section),
                lang: PageLang::En,
                reenter: None,
            }),
            4 << 20,
        );
    }
    run("remove watermark", Command::Page(PageCmd::RemoveWatermark { reenter: None }), 4 << 20);
    run("insert cover page", Command::Page(PageCmd::InsertCoverPage { id: "band".into(), lang: PageLang::En }), 8 << 20);
    run("remove cover page", Command::Page(PageCmd::RemoveCoverPage), 8 << 20);
    run(
        "header template",
        Command::HeaderFooter(HeaderFooterCmd::InsertTemplate {
            gallery: HfGallery::Header,
            id: "band".into(),
            lang: PageLang::En,
            enter: Some(HfEnter { part: HfReenter::Create { kind: HfKind::Header, section: 0, slot: HfSlot::Default }, page: 0 }),
            page_numbers: None,
        }),
        4 << 20,
    );
}
