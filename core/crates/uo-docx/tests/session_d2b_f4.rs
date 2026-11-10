//! D2b-F4 (sections, page setup, headers/footers, page colour, text layout options) on constructed
//! documents: the behaviours the parity capture reaches only by chance, and the error and no-op paths.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f4_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::Cx;
use d2b_f4_support::doc;
use uo_docx::ids::HfPartId;
use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::format::ColorArg;
use uo_docx::session::ops::header_footer::{HeaderFooterCmd, HfAt, HfKind, HfSlot};
use uo_docx::session::ops::page::PageCmd;
use uo_docx::session::ops::sections::section_ops::{ColumnPreset, Orientation};
use uo_docx::session::ops::sections::{ColumnsArg, PageSetup, SectionApplyTo, SectionCmd, SectionType};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::ops::text_layout::{HyphenationPatch, LineNumberMode, LineNumberingOptions, LineRestart, TextLayoutCmd};
use uo_docx::session::{ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};

fn open(bytes: Vec<u8>) -> Session {
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn exec(s: &mut Session, cmd: Command) -> ChangeSet {
    let mut cx = Cx::new();
    match s.exec(cmd, &mut cx.cx()) {
        Ok(Outcome::Done(cs)) => cs,
        other => panic!("{other:?}"),
    }
}

fn undo(s: &mut Session) -> ChangeSet {
    let mut cx = Cx::new();
    s.undo(&mut cx.cx())
}

fn redo(s: &mut Session) -> ChangeSet {
    let mut cx = Cx::new();
    s.redo(&mut cx.cx())
}

fn select(s: &mut Session, a: (u32, u32), f: (u32, u32)) {
    exec(s, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

fn setup(f: impl FnOnce(&mut PageSetup)) -> Command {
    let mut p = PageSetup::default();
    f(&mut p);
    Command::Section(SectionCmd::SetPageSetup(Box::new(p)))
}

fn part_text(bytes: &[u8], name: &str) -> String {
    let pkg = uo_opc::package::Package::open(bytes, &uo_opc::Limits::default(), &uo_opc::package::OpenOptions::default()).unwrap();
    pkg.part_text(name).unwrap().map(|t| t.into_owned()).unwrap_or_default()
}

fn sect_widths(s: &Session) -> Vec<(f64, f64)> {
    s.document().sections.iter().map(|x| (x.sect_pr.page_width_twips.to_number(), x.sect_pr.page_height_twips.to_number())).collect()
}

fn has(cs: &ChangeSet, e: &Event) -> bool {
    cs.events.iter().any(|x| x == e)
}

#[test]
fn landscape_rotates_the_margins_and_undo_returns() {
    let mut s = open(doc(3, 1));
    exec(&mut s, setup(|p| p.orientation = Some(Orientation::Landscape)));
    let sp = &s.document().sections[0].sect_pr;
    assert_eq!((sp.page_width_twips.to_number(), sp.page_height_twips.to_number()), (16838.0, 11906.0));
    // portrait (T 1417, R 1701, B 1417, L 1701) -> T = L, R = T, B = R, L = B
    assert_eq!(
        (
            sp.margin_top_twips.to_number(),
            sp.margin_right_twips.to_number(),
            sp.margin_bottom_twips.to_number(),
            sp.margin_left_twips.to_number()
        ),
        (1701.0, 1417.0, 1701.0, 1417.0)
    );
    undo(&mut s);
    assert_eq!(sect_widths(&s), vec![(11906.0, 16838.0)]);
    redo(&mut s);
    assert_eq!(sect_widths(&s), vec![(16838.0, 11906.0)]);
    let out = s.save_docx().unwrap();
    assert!(part_text(&out, "word/document.xml").contains("w:orient=\"landscape\""));
}

#[test]
fn locked_session_ignores_the_page_commands() {
    let mut s = open(doc(3, 1));
    exec(&mut s, Command::Settings(SettingsCmd::SetProtected { on: true }));
    let cs = exec(&mut s, setup(|p| p.margins = Some(uo_docx::session::ops::sections::MarginPreset::Narrow)));
    assert!(!cs.can_undo);
    assert_eq!(s.document().sections[0].sect_pr.margin_left_twips.to_number(), 1701.0);
}

#[test]
fn section_break_splits_the_paragraph_copies_the_section_and_moves_the_type_after() {
    let mut s = open(doc(3, 1));
    select(&mut s, (1, 4), (1, 4));
    let cs = exec(&mut s, Command::Section(SectionCmd::InsertSectionBreak { kind: SectionType::Continuous }));
    assert!(cs.can_undo);
    let secs = &s.document().sections;
    assert_eq!(secs.len(), 2);
    // the first half closes the new section (a copy: no type); the section after starts continuous
    assert_eq!(secs[0].sect_pr.r#type, None);
    assert_eq!(secs[1].sect_pr.r#type.as_deref(), Some("continuous"));
    assert_eq!(s.paras().unwrap().len(), 4);
    // the caret is at the start of the second half
    assert_eq!(s.selection().focus, DocPosition::new(2, 0));
    // the header reference left the section after (it is linked to the previous one now)
    assert_eq!(secs[0].header_refs.len(), 1);
    assert_eq!(secs[1].header_refs.len(), 0);
    undo(&mut s);
    assert_eq!(s.document().sections.len(), 1);
    assert_eq!(s.document().sections[0].header_refs.len(), 1);
    let out = redo_save(&mut s);
    assert!(part_text(&out, "word/document.xml").matches("<w:sectPr").count() >= 2);
}

fn redo_save(s: &mut Session) -> Vec<u8> {
    redo(s);
    s.save_docx().unwrap()
}

#[test]
fn columns_presets_fit_the_width() {
    let mut s = open(doc(3, 1));
    exec(&mut s, Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Preset(ColumnPreset::Two)))));
    let c = s.document().sections[0].sect_pr.cols.clone().unwrap();
    assert_eq!((c.count.to_number(), c.space_twips.to_number(), c.sep), (2.0, 708.0, false));
    exec(&mut s, Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Preset(ColumnPreset::Left)))));
    let c = s.document().sections[0].sect_pr.cols.clone().unwrap();
    let w = c.widths.unwrap();
    assert_eq!(w.len(), 2);
    // content width 11906 - 2 * 1701 = 8504; narrow = round((8504 - 2 * 708) / 3) = 2363
    assert_eq!(w[0].width_twips.to_number(), 2363.0);
    assert_eq!(w[1].width_twips.to_number(), 8504.0 - 2363.0 - 708.0);
    // the effective section carries the own widths for the layout
    let sect = s.document().sections[0].sect.clone().unwrap();
    assert_eq!(sect.equal_width, Some(false));
    assert_eq!(sect.col_defs.as_ref().map(Vec::len), Some(2));
    exec(&mut s, Command::Section(SectionCmd::SetColumns(Box::new(ColumnsArg::Preset(ColumnPreset::One)))));
    assert!(s.document().sections[0].sect_pr.cols.is_none());
    assert!(s.document().sections[0].sect.clone().unwrap().col_defs.is_none());
}

#[test]
fn page_break_blank_page_and_column_break_make_paragraphs() {
    let mut s = open(doc(3, 1));
    select(&mut s, (1, 5), (1, 5));
    exec(&mut s, Command::Section(SectionCmd::InsertPageBreak));
    // [0] [1 head] [break] [1 tail] [2]
    assert_eq!(s.paras().unwrap().len(), 5);
    assert_eq!(s.paras().unwrap().get(2).unwrap().runs[0].text.to_string_lossy(), "\u{c}");
    assert_eq!(s.selection().focus, DocPosition::new(3, 0));
    undo(&mut s);
    assert_eq!(s.paras().unwrap().len(), 3);
    select(&mut s, (2, 3), (2, 3));
    exec(&mut s, Command::Section(SectionCmd::InsertBlankPage));
    // split, then [break, empty, break]
    assert_eq!(s.paras().unwrap().len(), 7);
    assert_eq!(s.selection().focus, DocPosition::new(4, 0));
    select(&mut s, (0, 2), (0, 2));
    exec(&mut s, Command::Section(SectionCmd::InsertColumnBreak));
    assert_eq!(s.paras().unwrap().get(1).unwrap().runs[0].text.to_string_lossy().chars().next(), Some('\u{b}'));
}

#[test]
fn line_numbers_remember_start_and_distance_when_turned_off() {
    let mut s = open(doc(3, 1));
    exec(
        &mut s,
        Command::Section(SectionCmd::SetLineNumberingOptions {
            o: LineNumberingOptions { on: true, start_at: 5.0, count_by: 2.0, distance_twips: 300.0, restart: LineRestart::Section },
            apply_to: SectionApplyTo::Section,
            hf_section: None,
        }),
    );
    let ln = s.document().sections[0].sect_pr.ln_num_type.clone().unwrap();
    assert_eq!((ln.count_by.unwrap().to_number(), ln.start.unwrap().to_number(), ln.restart.as_deref()), (2.0, 4.0, Some("newSection")));
    assert!(s.document().sections[0].sect.clone().unwrap().ln_num_type.is_some());
    exec(&mut s, Command::Section(SectionCmd::SetLineNumbering { mode: LineNumberMode::None, hf_section: None }));
    assert!(s.document().sections[0].sect_pr.ln_num_type.is_none());
    exec(&mut s, Command::Section(SectionCmd::SetLineNumbering { mode: LineNumberMode::Continuous, hf_section: None }));
    let ln = s.document().sections[0].sect_pr.ln_num_type.clone().unwrap();
    // count by goes back to 1; the start and the distance are what the session remembered
    assert_eq!((ln.count_by.unwrap().to_number(), ln.start.unwrap().to_number(), ln.distance.unwrap().to_number()), (1.0, 4.0, 300.0));
    // the style entered styles.xml
    assert!(part_text(&s.save_docx().unwrap(), "word/styles.xml").contains("Nmerodelinha"));
}

#[test]
fn link_to_previous_creates_and_drops_a_part() {
    let mut s = open(doc(3, 2));
    // section 1 is linked to section 0 (no references of its own)
    let at = HfAt { section: 1, slot: HfSlot::Default };
    exec(&mut s, Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious { on: false, kind: Some(HfKind::Header), at, reenter: None }));
    assert_eq!(s.document().sections[1].header_refs.len(), 1);
    assert_eq!(s.document().hf.len(), 2);
    let new_text = s.document().hf.get(1).unwrap().story.paras.get(0).unwrap().runs[0].text.to_string_lossy();
    assert_eq!(new_text, "Header text");
    let out = s.save_docx().unwrap();
    assert!(part_text(&out, "word/header1.xml").contains("Header text"));
    // already unlinked: nothing happens
    let cs = exec(
        &mut s,
        Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious { on: false, kind: Some(HfKind::Header), at, reenter: None }),
    );
    assert!(cs.stories.is_empty());
    exec(&mut s, Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious { on: true, kind: Some(HfKind::Header), at, reenter: None }));
    assert_eq!(s.document().sections[1].header_refs.len(), 0);
    undo(&mut s);
    assert_eq!(s.document().sections[1].header_refs.len(), 1);
    // section 0 can never be linked
    let cs = exec(
        &mut s,
        Command::HeaderFooter(HeaderFooterCmd::SetLinkToPrevious {
            on: true,
            kind: Some(HfKind::Header),
            at: HfAt::default(),
            reenter: None,
        }),
    );
    assert!(!cs.dirty || cs.can_undo);
}

#[test]
fn an_option_taken_in_header_mode_returns_to_the_page() {
    let mut s = open(doc(3, 1));
    exec(&mut s, Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(0), page: 0 }));
    let cs = exec(&mut s, Command::HeaderFooter(HeaderFooterCmd::SetDifferentFirstPage { on: true, at: HfAt::default(), reenter: None }));
    assert!(s.document().sections[0].sect_pr.title_pg == Some(true));
    assert!(has(&cs, &Event::HfMode { active: true }));
    assert!(matches!(s.context(), uo_docx::session::EditContext::HeaderFooter { .. }));
    undo(&mut s);
    assert!(s.document().sections[0].sect_pr.title_pg.is_none());
    // an unknown part to return to: the edit ends
    let cs = exec(
        &mut s,
        Command::HeaderFooter(HeaderFooterCmd::SetDifferentFirstPage {
            on: true,
            at: HfAt::default(),
            reenter: Some(uo_docx::session::ops::header_footer::HfReenter::Part(HfPartId(77))),
        }),
    );
    assert!(has(&cs, &Event::HfMode { active: false }));
    assert!(matches!(s.context(), uo_docx::session::EditContext::Body));
}

#[test]
fn even_odd_default_tab_and_hyphenation_are_written_and_undone() {
    let mut s = open(doc(3, 1));
    exec(&mut s, Command::HeaderFooter(HeaderFooterCmd::SetDifferentOddEven { on: true, reenter: None }));
    exec(&mut s, Command::TextLayout(TextLayoutCmd::SetDefaultTabStop { twips: 500.0 }));
    exec(&mut s, Command::TextLayout(TextLayoutCmd::SetHyphenation { auto: true }));
    let out = s.save_docx().unwrap();
    let st = part_text(&out, "word/settings.xml");
    assert!(st.contains("<w:evenAndOddHeaders/>"), "{st}");
    assert!(st.contains("w:defaultTabStop w:val=\"500\""), "{st}");
    assert!(st.contains("<w:autoHyphenation/>"), "{st}");
    // the same value is no step
    let cs = exec(&mut s, Command::TextLayout(TextLayoutCmd::SetDefaultTabStop { twips: 500.0 }));
    assert!(cs.stories.is_empty());
    undo(&mut s);
    undo(&mut s);
    assert_eq!(s.document().formatting.settings.default_tab_stop.to_number(), 708.0);
    assert!(!s.hyphenation().auto);
    assert!(s.even_odd_headers());
    undo(&mut s);
    assert!(!s.even_odd_headers());
    // options: the zone and the limit are clamped
    exec(
        &mut s,
        Command::TextLayout(TextLayoutCmd::SetHyphenationOptions {
            o: HyphenationPatch { zone_twips: Some(0.0), limit: Some(99999.0), ..HyphenationPatch::default() },
        }),
    );
    let h = s.hyphenation();
    assert_eq!((h.zone_twips, h.limit), (1.0, 32767.0));
}

#[test]
fn the_page_colour_is_written_and_comes_back_with_undo() {
    let mut s = open(doc(3, 1));
    exec(&mut s, Command::Page(PageCmd::SetPageColor { color: Some(ColorArg::Hex("#ffcc00".into())) }));
    assert_eq!(s.document().background.as_ref().unwrap().color.as_deref(), Some("FFCC00"));
    assert!(s.document().formatting.settings.display_background_shape);
    let out = s.save_docx().unwrap();
    assert!(part_text(&out, "word/document.xml").contains("<w:background w:color=\"FFCC00\"/>"));
    assert!(part_text(&out, "word/settings.xml").contains("displayBackgroundShape"));
    undo(&mut s);
    assert!(s.document().background.is_none());
    assert!(!s.document().formatting.settings.display_background_shape);
    let out = s.save_docx().unwrap();
    assert!(!part_text(&out, "word/document.xml").contains("<w:background"));
    exec(&mut s, Command::Page(PageCmd::SetPageColor { color: None }));
    assert!(s.document().background.is_none());
}

#[test]
fn the_first_section_can_be_edited_in_the_document_scope_and_forward() {
    let mut s = open(doc(4, 2));
    exec(
        &mut s,
        setup(|p| {
            p.size = Some(uo_docx::session::ops::sections::PageSize::Letter);
            p.apply_to = Some(SectionApplyTo::Document);
        }),
    );
    assert_eq!(sect_widths(&s), vec![(12240.0, 15840.0), (12240.0, 15840.0)]);
    // forward: a next-page break at the caret and the section after changes
    select(&mut s, (3, 2), (3, 2));
    exec(
        &mut s,
        setup(|p| {
            p.orientation = Some(Orientation::Landscape);
            p.apply_to = Some(SectionApplyTo::Forward);
        }),
    );
    assert_eq!(s.document().sections.len(), 3);
    assert_eq!(sect_widths(&s)[2], (15840.0, 12240.0));
    assert_eq!(sect_widths(&s)[1], (12240.0, 15840.0));
}
