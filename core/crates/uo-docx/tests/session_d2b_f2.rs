//! D2b-F2 (lists and styles) behaviour tests: the commands on a blank document, the list hooks of
//! Enter and Backspace, the stores' saves, undo of the stores, the error and refusal paths.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod d2a_support;
mod d2b_f2_support;
mod support;

use d2a_support::Cx;
use uo_docx::session::ops::lists::ListCmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::styles::{Ch, CreateStyleSpec, ModifyStyleSpec, StyleCmd, StyleFormat, UiLang};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::FormattingPolicy;
use uo_docx::session::{Command, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn blank() -> (Session, Cx) {
    let mut s = Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    s.set_ui_lang(UiLang::Pt);
    (s, Cx::new())
}

fn run(s: &mut Session, cx: &mut Cx, c: Command) -> uo_docx::session::ChangeSet {
    match s.exec(c, &mut cx.cx()).unwrap() {
        Outcome::Done(cs) => cs,
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn type_text(s: &mut Session, cx: &mut Cx, t: &str) {
    run(s, cx, Command::Text(TextCmd::Type { text: Utf16::from(t) }));
}

fn saved_part(s: &mut Session, name: &str) -> Option<String> {
    let bytes = s.save_docx().unwrap();
    let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    pkg.part_text(name).unwrap().map(|t| t.into_owned())
}

#[test]
fn toggle_list_writes_a_hybrid_list_with_tentative_levels_and_undoes() {
    let (mut s, mut cx) = blank();
    type_text(&mut s, &mut cx, "item");
    let cs = run(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: true }));
    assert!(cs.can_undo);
    let p = s.paras().unwrap().get(0).unwrap();
    assert_eq!(p.props.numbering.as_ref().map(|n| &*n.kind), Some("bullet"));
    // List Paragraph was defined (Normal → Parágrafo da Lista)
    assert!(p.props.style_id.is_some());
    assert!(s.numbering_store().dirty());
    let num = saved_part(&mut s, "word/numbering.xml").unwrap();
    assert!(num.contains("hybridMultilevel"));
    // only level 0 is used: levels 1–8 are tentative, as Word writes
    assert_eq!(num.matches("w:tentative=\"1\"").count(), 8);
    assert!(saved_part(&mut s, "word/styles.xml").unwrap().contains("w:styleId=\"PargrafodaLista\""));
    // the toggle again takes it off
    run(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: true }));
    assert!(s.paras().unwrap().get(0).unwrap().props.numbering.is_none());
    // undo twice: the stores go back to the loaded text
    s.undo(&mut cx.cx());
    s.undo(&mut cx.cx());
    assert!(!s.numbering_store().dirty());
    assert!(s.document().formatting.numbering.abstract_nums.is_empty());
    s.redo(&mut cx.cx());
    assert!(s.numbering_store().dirty());
    assert!(!s.document().formatting.numbering.abstract_nums.is_empty());
}

#[test]
fn enter_on_an_empty_item_leaves_the_list_and_backspace_takes_the_number_off() {
    let (mut s, mut cx) = blank();
    type_text(&mut s, &mut cx, "one");
    run(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: false }));
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    assert_eq!(s.paras().unwrap().len(), 2);
    assert!(s.paras().unwrap().get(1).unwrap().props.numbering.is_some());
    // Enter on the empty second item: no new paragraph, the list ends there
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    assert_eq!(s.paras().unwrap().len(), 2);
    assert!(s.paras().unwrap().get(1).unwrap().props.numbering.is_none());
    // Backspace at the start of the first item removes the number, the text does not move
    run(
        &mut s,
        &mut cx,
        Command::Text(TextCmd::SetSelection {
            anchor: uo_docx::session::DocPosition::new(0, 0),
            focus: uo_docx::session::DocPosition::new(0, 0),
        }),
    );
    run(&mut s, &mut cx, Command::Text(TextCmd::Backspace));
    assert_eq!(s.paras().unwrap().len(), 2);
    assert!(s.paras().unwrap().get(0).unwrap().props.numbering.is_none());
}

#[test]
fn level_changes_restart_and_continue() {
    let (mut s, mut cx) = blank();
    type_text(&mut s, &mut cx, "a");
    run(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: false }));
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    type_text(&mut s, &mut cx, "b");
    // Tab on a second item: the level goes down
    assert!(s.change_list_level(1).unwrap());
    let n = s.paras().unwrap().get(1).unwrap().props.numbering.clone().unwrap();
    assert_eq!(n.ilvl, uo_docx::js::JsNumber::Num(1.0));
    run(&mut s, &mut cx, Command::List(ListCmd::SetListLevel { ilvl: 0.0 }));
    // restart: a copy of the abstract and a new w:num for the item and the ones after
    let before = s.numbering_store().xml().unwrap().matches("<w:num ").count();
    run(&mut s, &mut cx, Command::List(ListCmd::RestartNumbering { start: Some(5.0) }));
    let xml = s.numbering_store().xml().unwrap().to_string();
    assert_eq!(xml.matches("<w:num ").count(), before + 1);
    assert!(xml.contains("<w:start w:val=\"5\"/>"));
    // there is no earlier list of the same format to continue now (the first one is the same list id? no: restart moved item 2)
    let _ = s.continue_numbering().unwrap();
}

#[test]
fn styles_create_rename_modify_delete_follow_the_document() {
    let (mut s, mut cx) = blank();
    type_text(&mut s, &mut cx, "text");
    let id = s.create_style(&CreateStyleSpec { name: "Meu Estilo".into(), ..CreateStyleSpec::default() }).unwrap().unwrap();
    assert_eq!(id, "MeuEstilo");
    assert_eq!(s.paras().unwrap().get(0).unwrap().props.style_id.as_deref(), Some("MeuEstilo"));
    let xml = s.styles_store().xml().unwrap().to_string();
    assert!(xml.contains("w:styleId=\"MeuEstiloChar\"") && xml.contains("<w:link w:val=\"MeuEstiloChar\"/>"));
    // the name exists: refused
    assert_eq!(s.create_style(&CreateStyleSpec { name: " meu estilo ".into(), ..CreateStyleSpec::default() }).unwrap(), None);
    // modify: bold goes into the style and the Char twin
    assert!(
        s.modify_style(
            "MeuEstilo",
            &ModifyStyleSpec { format: StyleFormat { bold: Ch::Val(true), ..StyleFormat::default() }, ..ModifyStyleSpec::default() }
        )
        .unwrap()
    );
    assert!(s.styles_store().xml().unwrap().matches("<w:b/>").count() >= 2);
    // rename: the styleId and the paragraph follow
    assert!(s.rename_style("MeuEstilo", "Outro").unwrap());
    assert_eq!(s.paras().unwrap().get(0).unwrap().props.style_id.as_deref(), Some("Outro"));
    // delete: the paragraph goes back to Normal
    assert!(s.delete_style("Outro").unwrap());
    assert!(s.paras().unwrap().get(0).unwrap().props.style_id.is_none());
    assert!(!s.styles_store().xml().unwrap().contains("w:styleId=\"Outro\""));
    // undo brings it back, with the document
    s.undo(&mut cx.cx());
    assert_eq!(s.paras().unwrap().get(0).unwrap().props.style_id.as_deref(), Some("Outro"));
    assert!(s.document().formatting.styles.styles.get("Outro").is_some());
    // the saved part is the store's text
    assert_eq!(saved_part(&mut s, "word/styles.xml").as_deref(), s.styles_store().xml().map(|x| &**x));
}

#[test]
fn error_and_refusal_paths() {
    let (mut s, mut cx) = blank();
    type_text(&mut s, &mut cx, "x");
    // nothing to do / unknown / built-ins / empty names answer false without a step
    let undo_before = s.history().undo_len();
    assert!(!s.delete_style("NoSuch").unwrap());
    assert!(!s.delete_style("Normal").unwrap());
    assert!(!s.rename_style("Normal", "  ").unwrap());
    assert!(!s.modify_style("NoSuch", &ModifyStyleSpec::default()).unwrap());
    assert_eq!(s.create_style(&CreateStyleSpec { name: String::new(), ..CreateStyleSpec::default() }).unwrap(), None);
    assert_eq!(s.history().undo_len(), undo_before);
    // out-of-range levels and indexes are clamped / ignored
    run(&mut s, &mut cx, Command::List(ListCmd::ApplyListLibrary { bullet: true, index: Some(99) }));
    run(&mut s, &mut cx, Command::List(ListCmd::SetListLevel { ilvl: f64::NAN }));
    run(&mut s, &mut cx, Command::List(ListCmd::ChangeListLevel { delta: 1 }));
    // locked: no change
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetProtected { on: true }));
    let cs = run(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: true }));
    assert!(!cs.events.iter().any(|e| matches!(e, Event::Change)));
    assert!(s.paras().unwrap().get(0).unwrap().props.numbering.is_none());
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetProtected { on: false }));
    // the template policy refuses style editing and free lists: uw-blocked, no effect
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetPolicy { policy: Some(FormattingPolicy::template_document()) }));
    let cs = run(&mut s, &mut cx, Command::Style(StyleCmd::DeleteStyle { id: "Normal".into() }));
    assert!(cs.events.iter().any(|e| matches!(e, Event::Blocked { .. })));
}

#[test]
fn hostile_parts_do_not_panic() {
    let bad_num =
        "<w:numbering><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"></w:abstractNum><w:num w:numId=\"x\"/></w:numbering>";
    let bad_styles = "<w:styles><w:style w:styleId=\"A\"><w:name w:val=\"a\"/></w:style><w:style w:styleId=\"A\"><w:basedOn w:val=\"A\"/><w:link w:val=\"B\"/></w:style></w:styles>";
    for (n, st) in [(Some(bad_num), None), (None, Some(bad_styles)), (Some("<w:numbering"), Some("not xml"))] {
        let bytes = d2b_f2_support::doc_with_parts(n, st);
        let Ok(mut s) = Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())) else { continue };
        let mut cx = Cx::new();
        let mut rng = uo_testkit::generate::Rng::new(7);
        for _ in 0..60 {
            let c = d2b_f2_support::f2_cmd(&mut rng, true);
            let _ = s.exec(c, &mut cx.cx());
        }
        let _ = s.save_docx();
    }
}
