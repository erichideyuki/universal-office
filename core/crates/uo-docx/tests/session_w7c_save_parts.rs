//! Wave 7c, frontier S (record `core/docs/modules/uo-bindings.md` "Wave 7c — S"): what a save writes outside the writer.
//!
//! - `pendingTableStyles` / `addStylesXml` (universal-write.ts 19283–19293, 8641; table-styles.ts 149–171): a table
//!   inserted where the style sheet is not editable (an `.odt`) or the package has no styles part gets "Table Grid" and
//!   "Normal Table" written into a styles part the save makes.
//! - Word manda sobre o TS (measured in Word 16.0 pt-BR, `core/target/m2-w7b/s-plan/w-out`, `core/target/m2-w7b/s7c/w-out`):
//!   after an undo the saved file has no part of the undone edit — the table style, a created style, a list's numbering
//!   part, the even/odd pages flag of `settings.xml`. `OpenOptions::ts_table_style_survives_undo` / `ts_stale_parts` keep
//!   the frozen TS (the style and the parts stay).
//! - The `.odt`'s scratch package (`this.pkg ?? DocxPackage.empty()`, 8552): each save from an empty package.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;

use d2a_support::Cx;
use uo_docx::session::ops::header_footer::HeaderFooterCmd;
use uo_docx::session::ops::lists::ListCmd;
use uo_docx::session::ops::notes::{NoteCmd, NoteOptionsPatch, NoteScope};
use uo_docx::session::ops::styles::{CreateStyleSpec, StyleCmd};
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn run(s: &mut Session, cx: &mut Cx, c: Command) {
    match s.exec(c, &mut cx.cx()).unwrap() {
        Outcome::Done(_) => {}
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn select(s: &mut Session, cx: &mut Cx, a: (u32, u32), f: (u32, u32)) {
    run(s, cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

fn insert_table(s: &mut Session, cx: &mut Cx, rows: f64, cols: f64) {
    select(s, cx, (0, 0), (0, 0));
    run(
        s,
        cx,
        Command::Table(TableCmd::InsertTable { rows, cols, autofit: None, col_width_twips: None, layout: TableLayoutIn::default() }),
    );
}

/// A blank document with two typed paragraphs: its `.docx` bytes (styles, settings; no numbering part).
fn blank_docx() -> Vec<u8> {
    let mut cx = Cx::new();
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("Hello World") }));
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    run(&mut s, &mut cx, Command::Text(TextCmd::Type { text: Utf16::from("second line") }));
    s.save_docx().unwrap()
}

#[cfg(feature = "odf")]
fn odt() -> Vec<u8> {
    let s = Session::open(blank_docx(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    uo_docx::odf::write_odt::write_odt(s.document(), &uo_docx::odf::write_odt::OdtOptions::default()).unwrap()
}

struct Saved {
    names: Vec<String>,
    styles: Option<String>,
    numbering: Option<String>,
    settings: Option<String>,
    ct: String,
    rels: String,
    document: String,
}

fn saved(s: &mut Session) -> Saved {
    let bytes = s.save_docx().unwrap();
    let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    let t = |n: &str| pkg.part_text(n).unwrap().map(|c| c.into_owned());
    let mut names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    names.sort();
    Saved {
        names,
        styles: t("word/styles.xml"),
        numbering: t("word/numbering.xml"),
        settings: t("word/settings.xml"),
        ct: t("[Content_Types].xml").unwrap_or_default(),
        rels: t("word/_rels/document.xml.rels").unwrap_or_default(),
        document: t("word/document.xml").unwrap_or_default(),
    }
}

fn style_ids(x: &Saved) -> Vec<String> {
    let Some(st) = &x.styles else { return Vec::new() };
    st.split("w:styleId=\"").skip(1).map(|r| r.split('"').next().unwrap_or("").to_owned()).collect()
}

fn tbl_styles(x: &Saved) -> Vec<String> {
    x.document.split("<w:tblStyle w:val=\"").skip(1).map(|r| r.split('"').next().unwrap_or("").to_owned()).collect()
}

/// Every table of the body refers to a style the saved package defines.
fn styles_defined(x: &Saved) {
    let ids = style_ids(x);
    for t in tbl_styles(x) {
        assert!(ids.contains(&t), "table style {t} not defined in {ids:?}");
    }
}

fn opts(ts: bool) -> OpenOptions {
    OpenOptions { ts_table_style_survives_undo: ts, ts_stale_parts: ts, ..OpenOptions::default() }
}

/// The insert / save / undo / save / redo + insert / save scenario on a session whose style sheet the table cannot use.
fn table_style_cycle(mut s: Session, ts: bool) {
    let mut cx = Cx::new();
    insert_table(&mut s, &mut cx, 2.0, 2.0);
    let a = saved(&mut s);
    let ids = style_ids(&a);
    assert_eq!(tbl_styles(&a).len(), 1);
    assert!(ids.len() == 2 && ids.iter().any(|i| tbl_styles(&a).contains(i)), "Normal Table + Table Grid: {ids:?}");
    assert!(a.ct.contains("PartName=\"/word/styles.xml\""), "content type override");
    assert!(a.rels.contains("relationships/styles\""), "styles relationship");
    styles_defined(&a);
    s.undo(&mut cx.cx());
    let b = saved(&mut s);
    assert!(tbl_styles(&b).is_empty());
    if ts {
        // the frozen TS: the pending map is outside the history, the style is still written
        assert_eq!(style_ids(&b), ids, "TS: the style survives the undo");
    } else {
        // Word: the undone table's style is not in the file (no styles part at all, as before the insert)
        assert!(b.styles.is_none(), "Word: no styles part after the undo: {:?}", style_ids(&b));
        assert!(!b.ct.contains("/word/styles.xml") && !b.rels.contains("relationships/styles\""));
    }
    s.redo(&mut cx.cx());
    insert_table(&mut s, &mut cx, 1.0, 1.0);
    let c = saved(&mut s);
    assert_eq!(tbl_styles(&c).len(), 2, "two tables");
    styles_defined(&c);
    // undo the second insert, undo the first, then a NEW table (not a redo): it must find its style written
    s.undo(&mut cx.cx());
    s.undo(&mut cx.cx());
    insert_table(&mut s, &mut cx, 1.0, 1.0);
    let d = saved(&mut s);
    assert_eq!(tbl_styles(&d).len(), 1);
    styles_defined(&d);
}

#[cfg(feature = "odf")]
#[test]
fn odt_table_style_is_written_and_undone_with_the_table() {
    for ts in [false, true] {
        let s = Session::open_odt(&odt(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        assert!(s.package_is_scratch());
        table_style_cycle(s, ts);
    }
}

#[test]
fn docx_without_styles_part_gets_the_table_style() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/d3/fixtures/hyperlinks.docx");
    let bytes = std::fs::read(fixture).unwrap();
    for ts in [false, true] {
        let s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        assert!(s.styles_store().xml().is_none(), "the fixture has no styles part");
        table_style_cycle(s, ts);
    }
}

/// An `.odt` saved twice with an undone list between: the second save is from scratch (the TS `this.pkg === null`), so
/// no numbering part — the TS's and Word's result.
#[cfg(feature = "odf")]
#[test]
fn odt_saves_from_an_empty_package() {
    let mut cx = Cx::new();
    let mut s = Session::open_odt(&odt(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    select(&mut s, &mut cx, (0, 0), (0, 3));
    run(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: false }));
    let a = saved(&mut s);
    assert!(a.numbering.is_some_and(|n| n.contains("<w:num ")), "the list's numbering");
    s.undo(&mut cx.cx());
    let b = saved(&mut s);
    assert_eq!(b.names, ["[Content_Types].xml", "_rels/.rels", "word/document.xml"], "{:?}", b.names);
}

/// An editable `.docx`: the table style goes into the style sheet (undone with it, as before); a save between the insert
/// and the undo leaves the loaded styles part at the next save (Word; the TS keeps the first save's).
#[test]
fn docx_styles_part_after_save_undo_save() {
    let bytes = blank_docx();
    let loaded = {
        let mut s = Session::open(bytes.clone(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        saved(&mut s).styles.unwrap()
    };
    for ts in [false, true] {
        let mut cx = Cx::new();
        let mut s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        insert_table(&mut s, &mut cx, 2.0, 2.0);
        let a = saved(&mut s);
        styles_defined(&a);
        assert_ne!(a.styles.as_deref(), Some(loaded.as_str()));
        s.undo(&mut cx.cx());
        let b = saved(&mut s);
        if ts {
            assert_eq!(b.styles, a.styles, "TS: the first save's part stays");
        } else {
            assert_eq!(b.styles.as_deref(), Some(loaded.as_str()), "Word: the loaded part");
        }
    }
}

/// `createStyle` and `toggleList` undone after a save (Word: style gone, numbering part gone), and different odd and even
/// pages (Word: `w:evenAndOddHeaders` gone).
#[test]
fn style_list_and_settings_undone_after_a_save() {
    let bytes = blank_docx();
    let base = {
        let mut s = Session::open(bytes.clone(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        saved(&mut s)
    };
    assert!(base.numbering.is_none());
    for ts in [false, true] {
        // a style
        let mut cx = Cx::new();
        let mut s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        select(&mut s, &mut cx, (0, 0), (0, 3));
        let spec = CreateStyleSpec { name: "Meu".into(), kind: Some("paragraph".into()), ..CreateStyleSpec::default() };
        run(&mut s, &mut cx, Command::Style(StyleCmd::CreateStyle { spec: Box::new(spec) }));
        let a = saved(&mut s);
        assert!(style_ids(&a).len() > style_ids(&base).len(), "the new style");
        s.undo(&mut cx.cx());
        let b = saved(&mut s);
        if ts {
            assert_eq!(b.styles, a.styles);
        } else {
            assert_eq!(b.styles, base.styles, "Word: the style is gone");
        }
        // a list
        let mut cx = Cx::new();
        let mut s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        select(&mut s, &mut cx, (0, 0), (0, 3));
        run(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: false }));
        let a = saved(&mut s);
        assert!(a.numbering.as_deref().is_some_and(|n| n.contains("<w:num ")));
        s.undo(&mut cx.cx());
        let b = saved(&mut s);
        if ts {
            assert_eq!(b.numbering, a.numbering, "TS: the first save's numbering stays");
        } else {
            assert!(b.numbering.is_none(), "Word: no numbering part");
            assert!(!b.names.contains(&"word/numbering.xml".to_owned()));
            assert_eq!(b.ct, base.ct);
            assert_eq!(b.rels, base.rels);
        }
        // different odd and even pages
        let mut cx = Cx::new();
        let mut s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        run(&mut s, &mut cx, Command::HeaderFooter(HeaderFooterCmd::SetDifferentOddEven { on: true, reenter: None }));
        let a = saved(&mut s);
        assert!(a.settings.as_deref().is_some_and(|x| x.contains("<w:evenAndOddHeaders")));
        s.undo(&mut cx.cx());
        let b = saved(&mut s);
        // Word: the flag is gone; the TS too (the undo makes `evenOdd` false, not null, and the save writes it off)
        assert_eq!(b.settings, base.settings, "the flag is gone (ts {ts})");
        // the footnote number format of the document (`settings.xml` `w:footnotePr`): Word takes it back with the undo
        // (measured: `w:numFmt lowerRoman` gone); the TS writes the options only when they differ from the loaded ones, so
        // after the undo the first save's stays
        let mut cx = Cx::new();
        let mut s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        let patch = NoteOptionsPatch { number_format: Some("lowerRoman".into()), ..NoteOptionsPatch::default() };
        run(&mut s, &mut cx, Command::Note(NoteCmd::SetOptions { footnote: true, patch, scope: NoteScope::Document }));
        let a = saved(&mut s);
        assert!(a.settings.as_deref().is_some_and(|x| x.contains("lowerRoman")), "the format in settings.xml");
        s.undo(&mut cx.cx());
        let b = saved(&mut s);
        if ts {
            assert_eq!(b.settings, a.settings, "TS: the first save's footnotePr stays");
        } else {
            assert_eq!(b.settings, base.settings, "Word: the loaded settings");
        }
    }
}

/// Fix round 1 (the reviewer's minors), Word measured (`core/target/m2-w7b/s7d/w-out`): a footnote inserted and undone
/// keeps its note styles in the file with no save in between (and with one); the TS writes the sheet only when it
/// changed, so with no save in between they are gone. A note option set and then SET BACK (a change, not an undo):
/// Word's second save has the current options (`w:numFmt` gone); the TS writes the options only when they differ from
/// the load, so the first save's stay.
#[test]
fn note_styles_after_undo_and_note_options_set_back() {
    let bytes = blank_docx();
    let base = {
        let mut s = Session::open(bytes.clone(), OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        saved(&mut s)
    };
    let notes = ["Textodenotaderodap", "TextodenotaderodapChar", "Refdenotaderodap"];
    assert!(style_ids(&base).iter().all(|i| !notes.contains(&i.as_str())), "no note styles at the open");
    for ts in [false, true] {
        for save_between in [false, true] {
            let mut cx = Cx::new();
            let mut s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
            select(&mut s, &mut cx, (0, 5), (0, 5));
            run(&mut s, &mut cx, Command::Note(NoteCmd::Insert { footnote: true, mark: None }));
            if save_between {
                let a = saved(&mut s);
                assert!(notes.iter().all(|n| style_ids(&a).iter().any(|i| i == n)), "the note styles: {:?}", style_ids(&a));
            }
            s.undo(&mut cx.cx());
            let b = saved(&mut s);
            assert!(!b.document.contains("w:footnoteReference"), "the insertion is undone");
            let ids = style_ids(&b);
            let kept = notes.iter().all(|n| ids.iter().any(|i| i == n));
            // Word: kept either way; the TS: kept only through its reused package (a save in between)
            assert_eq!(kept, !ts || save_between, "ts {ts}, save between {save_between}: {ids:?}");
            if !ts {
                // nothing but the note styles was added to the loaded sheet
                let extra: Vec<&String> = ids.iter().filter(|i| !style_ids(&base).contains(i)).collect();
                assert_eq!(extra.len(), notes.len(), "{extra:?}");
            }
        }
        // the option set back
        let mut cx = Cx::new();
        let mut s = Session::open(bytes.clone(), opts(ts), Box::new(FixedEnv::default())).unwrap();
        let roman = NoteOptionsPatch { number_format: Some("lowerRoman".into()), ..NoteOptionsPatch::default() };
        run(&mut s, &mut cx, Command::Note(NoteCmd::SetOptions { footnote: true, patch: roman, scope: NoteScope::Document }));
        let a = saved(&mut s);
        assert!(a.settings.as_deref().is_some_and(|x| x.contains("lowerRoman")));
        let back = NoteOptionsPatch { number_format: Some("decimal".into()), ..NoteOptionsPatch::default() };
        run(&mut s, &mut cx, Command::Note(NoteCmd::SetOptions { footnote: true, patch: back, scope: NoteScope::Document }));
        let b = saved(&mut s);
        if ts {
            assert_eq!(b.settings, a.settings, "TS: the first save's footnotePr stays");
        } else {
            assert!(b.settings.as_deref().is_some_and(|x| !x.contains("lowerRoman")), "Word: the current options");
            assert_eq!(b.settings, base.settings, "Word: the options are the loaded ones again");
        }
    }
}

/// M1: the new state (the pending map in the history, the save records) leaks nothing.
#[cfg(feature = "odf")]
#[test]
fn save_parts_no_leak() {
    let odt = odt();
    let life = || {
        let mut cx = Cx::new();
        let mut s = Session::open_odt(&odt, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
        insert_table(&mut s, &mut cx, 2.0, 2.0);
        let mut n = s.save_docx().unwrap().len();
        s.undo(&mut cx.cx());
        n += s.save_docx().unwrap().len();
        s.redo(&mut cx.cx());
        n += s.save_docx().unwrap().len();
        s.clear_caches();
        std::hint::black_box(n)
    };
    std::hint::black_box(life());
    uo_alloc::assert_no_leak(life);
}
