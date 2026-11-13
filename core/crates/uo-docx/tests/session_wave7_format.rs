//! Wave 7, frontier A1 of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7 — A1"): the session
//! accessors and native-only commands the shell's File, Font and Paragraph families read — the Paragraph dialogs' getters
//! (`getParagraphFlow`, `getParagraphLayout`, `getParagraphBorders`, `getDefaultTabStop`), `styleKeyOf`,
//! `ensureBuiltinStyle`, the plain `replaceAll`, `insertCustomInline` and the `.odt` open — each against the TS rule it
//! ports (the line of universal-write.ts in the source's doc comment).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;

use d2a_support::Cx;
use uo_docx::session::ops::find::FindCmd;
use uo_docx::session::ops::navigation::F9Result;
use uo_docx::session::ops::paragraph::{FlowArgs, LayoutArgs, ParaBorderSide, ParagraphCmd, SpacingArg};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::styles::StyleCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ChangeSet, Command, DocPosition, Event, FixedEnv, OpenOptions, Outcome, Session};
use uo_docx::text::Utf16;

fn blank() -> Session {
    Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn run(s: &mut Session, cx: &mut Cx, c: Command) -> ChangeSet {
    match s.exec(c, &mut cx.cx()).unwrap() {
        Outcome::Done(cs) => cs,
        Outcome::NeedsJobs(_) => panic!("no jobs"),
    }
}

fn typed(s: &mut Session, cx: &mut Cx, t: &str) {
    run(s, cx, Command::Text(TextCmd::Type { text: Utf16::from(t) }));
}

fn select(s: &mut Session, cx: &mut Cx, a: (u32, u32), f: (u32, u32)) {
    run(s, cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }));
}

/// "Hello World" / "second line" in two paragraphs, the caret at the start.
fn doc() -> (Session, Cx) {
    let mut cx = Cx::new();
    let mut s = blank();
    typed(&mut s, &mut cx, "Hello World");
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter));
    typed(&mut s, &mut cx, "second line");
    select(&mut s, &mut cx, (0, 0), (0, 0));
    (s, cx)
}

fn text(s: &Session, para: usize) -> String {
    s.document().body.paras.get(para).unwrap().runs.iter().map(|r| r.text.to_string_lossy()).collect()
}

#[test]
fn flow_getter_is_common_or_none() {
    let (mut s, mut cx) = doc();
    let f = s.paragraph_flow().unwrap();
    // widow control is on unless something turns it off; the rest off; no tight wrap
    assert_eq!(f.flags, [Some(true), Some(false), Some(false), Some(false), Some(false), Some(false)]);
    assert_eq!(f.textbox_tight_wrap.as_deref(), Some("none"));
    run(
        &mut s,
        &mut cx,
        Command::Paragraph(ParagraphCmd::SetParagraphFlow { f: FlowArgs { keep_next: Some(true), ..FlowArgs::default() } }),
    );
    assert_eq!(s.paragraph_flow().unwrap().flags[1], Some(true));
    // both paragraphs: they differ in keepNext (the TS `null`)
    select(&mut s, &mut cx, (0, 0), (1, 2));
    let f = s.paragraph_flow().unwrap();
    assert_eq!(f.flags[1], None);
    assert_eq!(f.flags[0], Some(true));
}

#[test]
fn layout_getter_reads_auto_spacing() {
    let (mut s, mut cx) = doc();
    run(
        &mut s,
        &mut cx,
        Command::Paragraph(ParagraphCmd::SetParagraphLayout {
            v: LayoutArgs {
                left: Some(360.0),
                before: Some(SpacingArg::Auto),
                after: Some(SpacingArg::Twips(240.0)),
                ..LayoutArgs::default()
            },
        }),
    );
    let l = s.paragraph_layout().unwrap();
    assert_eq!(l.left, Some(360.0));
    assert_eq!(l.before, Some(SpacingArg::Auto));
    assert_eq!(l.after, Some(SpacingArg::Twips(240.0)));
    select(&mut s, &mut cx, (0, 0), (1, 1));
    assert_eq!(s.paragraph_layout().unwrap().left, None, "the paragraphs differ");
}

#[test]
fn borders_getter_and_default_tab() {
    let (mut s, mut cx) = doc();
    assert!(s.paragraph_borders().is_empty());
    run(&mut s, &mut cx, Command::Paragraph(ParagraphCmd::ToggleBorder));
    let b = s.paragraph_borders();
    assert_eq!(
        b.iter().map(|(sd, _)| *sd).collect::<Vec<_>>(),
        vec![ParaBorderSide::Top, ParaBorderSide::Left, ParaBorderSide::Bottom, ParaBorderSide::Right]
    );
    assert_eq!(&*b[0].1.style, "single");
    // the blank document's w:defaultTabStop
    assert_eq!(s.default_tab_stop_now(), 708.0);
}

#[test]
fn style_keys_and_builtin_ids() {
    let (mut s, mut cx) = doc();
    // by w:name (Heading 1 → heading1), unknown ids by themselves
    let h1 = s.builtin_style_id_of("heading 1");
    assert_eq!(s.style_key_for(&h1), "heading1");
    assert_eq!(s.style_key_for("NoSuchStyle"), "nosuchstyle");
    // ensureBuiltinStyle: defined once, no undo step, the document dirty
    let id = s.builtin_style_id_of("caption");
    let cs = run(&mut s, &mut cx, Command::Style(StyleCmd::EnsureBuiltinStyle { name: "caption".into() }));
    assert!(
        cs.events.iter().any(|e| matches!(e, Event::StylesChange))
            || s.styles_store().xml().is_some_and(|x| x.contains(&format!("w:styleId=\"{id}\"")))
    );
    assert!(s.styles_store().xml().unwrap().contains(&format!("w:styleId=\"{id}\"")));
    assert_eq!(s.builtin_style_id_of("caption"), id);
}

#[test]
fn replace_all_plain_counts_and_undoes_in_one_step() {
    let (mut s, mut cx) = doc();
    run(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceAllPlain { needle: "L".encode_utf16().collect(), replacement: "ll".encode_utf16().collect() }),
    );
    // case-insensitive, past each replacement (no re-match inside "ll")
    assert_eq!(text(&s, 0), "Hellllo Worlld");
    assert_eq!(text(&s, 1), "second lline");
    assert!(matches!(s.navigation_result(), Some(F9Result::Count(4))));
    run(&mut s, &mut cx, Command::Text(TextCmd::Enter)); // a later step
    let _ = s.undo(&mut cx.cx());
    let _ = s.undo(&mut cx.cx());
    assert_eq!(text(&s, 0), "Hello World", "one undo step");
    // an empty needle: nothing, count 0
    run(&mut s, &mut cx, Command::Find(FindCmd::ReplaceAllPlain { needle: Vec::new(), replacement: "x".encode_utf16().collect() }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Count(0))));
}

#[test]
fn replace_all_plain_tracked_skips_deleted_text() {
    let (mut s, mut cx) = doc();
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetTracking { on: true }));
    run(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceAllPlain { needle: "o".encode_utf16().collect(), replacement: "0".encode_utf16().collect() }),
    );
    let n = match s.navigation_result() {
        Some(F9Result::Count(n)) => *n,
        other => panic!("{other:?}"),
    };
    assert_eq!(n, 3, "the two o's of Hello World and the one of second");
    // again: the deleted o's are skipped, the inserted 0s do not match
    run(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceAllPlain { needle: "o".encode_utf16().collect(), replacement: "0".encode_utf16().collect() }),
    );
    assert!(matches!(s.navigation_result(), Some(F9Result::Count(0))));
}

#[test]
fn insert_custom_inline_one_object_run() {
    let (mut s, mut cx) = doc();
    select(&mut s, &mut cx, (0, 0), (0, 5));
    let cs = run(
        &mut s,
        &mut cx,
        Command::Text(TextCmd::InsertCustomInline {
            r#type: "tag".into(),
            data_json: "{\"name\":\"x\",\"n\":[1,2]}".into(),
            fallback_text: Utf16::from("#x"),
        }),
    );
    assert!(cs.events.iter().any(|e| matches!(e, Event::CustomInlineInserted { r#type } if &**r#type == "tag")));
    assert_eq!(text(&s, 0), "\u{FFFC} World");
    assert_eq!(s.selection().focus, DocPosition::new(0, 1));
    let inl = uo_docx::read::inspect::document_custom_inlines(&s.document().body, Some("tag"));
    assert_eq!(inl.len(), 1);
    assert_eq!(inl[0].fallback_text.to_string_lossy(), "#x");
    assert_eq!(uo_docx::js::JsonWriter::stringify(&inl[0].data, None).unwrap(), "{\"name\":\"x\",\"n\":[1,2]}");
    let _ = s.undo(&mut cx.cx());
    assert_eq!(text(&s, 0), "Hello World");
    // locked: nothing
    run(&mut s, &mut cx, Command::Settings(SettingsCmd::SetProtected { on: true }));
    run(
        &mut s,
        &mut cx,
        Command::Text(TextCmd::InsertCustomInline { r#type: "tag".into(), data_json: "1".into(), fallback_text: Utf16::from("1") }),
    );
    assert_eq!(text(&s, 0), "Hello World");
}

#[cfg(feature = "odf")]
#[test]
fn open_odt_round_trip() {
    let (mut s, _cx) = doc();
    let _ = &mut s;
    // the .odt of the session's document, opened again: the same paragraphs, saved as a .docx from scratch
    let odt = uo_docx::odf::write_odt::write_odt(s.document(), &uo_docx::odf::write_odt::OdtOptions::default()).unwrap();
    let mut o = Session::open_odt(&odt, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    assert_eq!(text(&o, 0), "Hello World");
    assert_eq!(text(&o, 1), "second line");
    assert!(o.styles_store().xml().is_none(), "no styles part (the TS DocStyles(null))");
    let docx = o.save_docx().unwrap();
    let back = Session::open(docx, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    assert_eq!(text(&back, 1), "second line");
}

/// `stylesEditable()` (universal-write.ts 4582) after `loadOdt`: `listStyles` lists the ODF's paragraph styles (4156),
/// `styleFormatting` is null (4176), the style commands refuse and using a built-in defines nothing (fix round 2).
#[cfg(feature = "odf")]
#[test]
fn odt_styles_are_read_only() {
    use uo_docx::session::ops::styles::{CreateStyleSpec, DefaultFont};
    let (s, _cx) = doc();
    assert_eq!(s.loaded_format(), "docx");
    assert!(s.styles_editable() && !s.odt_style_list() && s.style_formatting_available());
    let odt = uo_docx::odf::write_odt::write_odt(s.document(), &uo_docx::odf::write_odt::OdtOptions::default()).unwrap();
    let mut o = Session::open_odt(&odt, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap();
    let mut cx = Cx::new();
    assert_eq!(o.loaded_format(), "odt");
    assert!(!o.styles_editable() && o.odt_style_list() && !o.style_formatting_available());
    let list = o.style_list_entries();
    assert!(!list.is_empty(), "the ODF's paragraph styles");
    assert!(list.iter().all(|e| !e.built_in && e.defined && e.kind == "paragraph" && e.quick && !e.in_use && e.id != "docDefaults"));
    assert_eq!(o.create_style(&CreateStyleSpec { name: "Meu Estilo".into(), ..CreateStyleSpec::default() }).unwrap(), None);
    assert!(!o.rename_style(&list[0].id, "Outro").unwrap());
    assert!(!o.delete_style(&list[0].id).unwrap());
    assert!(!o.set_default_font(&DefaultFont { size_pt: Some(14.0), ..DefaultFont::default() }).unwrap());
    assert!(!o.update_style_from_selection(None).unwrap());
    // a built-in applied: the paragraph takes the id, the sheet stays absent (no `ensureDefined`)
    run(&mut o, &mut cx, Command::Style(StyleCmd::ApplyStyle { style_id: Some("Ttulo1".into()) }));
    run(&mut o, &mut cx, Command::Style(StyleCmd::EnsureBuiltinStyle { name: "heading 2".into() }));
    assert!(o.styles_store().xml().is_none(), "the .odt sheet is not edited");
    assert_eq!(o.style_list_entries(), list);
}
