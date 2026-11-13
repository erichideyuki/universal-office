//! Wave 7 (A2) of the editor switch (record `core/docs/modules/uo-bindings.md` "Wave 7 — A2"): the read-only answers of
//! `session::ops::completion` (lists, table styles) and the shell edits (`ShellEdit`): each edit is ONE undo step that
//! restores the saved package exactly, the multi-command `oneUndo` collapses, and M1 (a life of every edit returns the live
//! bytes). The parity with the frozen TS is the Chromium harness `editor_switch_completion.mjs` (the editor's bars and
//! gestures on both engines).
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;

use std::hint::black_box;

use d2a_support::Cx;
use uo_alloc::assert_no_leak;
use uo_docx::model::props::BorderSpec;
use uo_docx::session::ops::lists::ListCmd;
use uo_docx::session::ops::shell_edits::ShellEdit;
use uo_docx::session::ops::tables::completion::{CellSide, FilledTable, TableGesture};
use uo_docx::session::ops::tables::{TableCmd, TableLayoutIn};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, FixedEnv, OpenOptions, Session};

fn blank() -> Session {
    Session::new_blank(OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn exec(s: &mut Session, cx: &mut Cx, c: Command) {
    s.exec(c, &mut cx.cx()).unwrap();
}

fn table(s: &mut Session, cx: &mut Cx, rows: f64, cols: f64) {
    exec(
        s,
        cx,
        Command::Table(TableCmd::InsertTable { rows, cols, autofit: None, col_width_twips: None, layout: TableLayoutIn::default() }),
    );
}

/// The edit is one undo step: undo gives the package before it back, byte for byte.
fn one_step(s: &mut Session, cx: &mut Cx, e: ShellEdit) -> bool {
    let before = s.save_docx().unwrap();
    let depth = s.history().undo_len();
    s.exec_shell_edit(e, &mut cx.cx()).unwrap();
    let changed = s.history().undo_len() > depth;
    if changed {
        assert_eq!(s.history().undo_len(), depth + 1, "one step");
        s.undo(&mut cx.cx());
        assert_eq!(s.save_docx().unwrap(), before, "undo restores the package");
        s.redo(&mut cx.cx());
    }
    changed
}

#[test]
fn lists_answer_as_the_ts() {
    let mut cx = Cx::new();
    let mut s = blank();
    assert!(s.list_info_view().is_none());
    assert!(s.document_lists_view().is_empty());
    exec(&mut s, &mut cx, Command::Text(TextCmd::Type { text: "one".into() }));
    exec(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: true }));
    let i = s.list_info_view().unwrap();
    assert!(i.bullet);
    assert_eq!(i.ilvl, 0.0);
    assert_eq!(i.levels.len(), 9);
    assert!(i.level.as_ref().is_some_and(|l| l.num_fmt == "bullet" && l.start == 1.0));
    assert!(!i.can_continue);
    assert!(s.numbering_value_view().is_none(), "a bullet has no numbering value");
    assert_eq!(s.document_lists_view().len(), 1);
    exec(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: false }));
    let (can, value, fmt) = s.numbering_value_view().unwrap();
    assert!(!can);
    assert_eq!(value, 1.0);
    assert_eq!(fmt, "decimal");
    for k in 0..3 {
        let lib = s.list_library_view(k);
        assert!(!lib.is_empty());
        assert!(lib.iter().all(|l| l.len() == 9), "kind {k}");
    }
    exec(&mut s, &mut cx, Command::List(ListCmd::CreateListStyle { name: "Minha Lista".into(), levels: Vec::new(), apply: true }));
    let styles = s.list_styles_view();
    assert!(styles.iter().any(|x| x.name == "Minha Lista" && x.levels.len() == 9), "{styles:?}");
}

#[test]
fn table_styles_answer_as_the_ts() {
    let s = blank();
    let list = s.table_style_listing();
    assert!(list.iter().all(|t| t.group != "custom"), "a blank document has no custom table style");
    assert!(list.iter().any(|t| t.name == "Table Grid" && t.group == "plain"));
    assert!(list.iter().any(|t| t.name == "Grid Table 4 Accent 1" && t.group == "grid" && t.en == "Grid Table 4 - Accent 1"));
    assert!(list.iter().any(|t| t.group == "list"));
    assert!(list.iter().all(|t| t.name != "Normal Table"));
    let (id, xml, in_doc) = s.table_style_xml("Grid Table 4 Accent 1").unwrap();
    assert!(!in_doc);
    assert!(xml.contains(&format!("w:styleId=\"{id}\"")));
    assert_eq!(s.new_table_style_id("Meu Estilo!"), "MeuEstilo");
    assert!(s.table_style_bases().iter().all(|(id, name)| !id.is_empty() && !name.is_empty()));
}

#[test]
fn shell_edits_are_one_undo_step_each() {
    let mut cx = Cx::new();
    let mut s = blank();
    exec(&mut s, &mut cx, Command::Text(TextCmd::Type { text: "before".into() }));
    // a Quick Table: the insert, the style, the text and the merge in one step
    let q = FilledTable {
        rows: vec![vec!["Title".into(), String::new(), String::new()], vec!["a".into(), "1".into(), "2".into()]],
        style: "Grid Table 4 Accent 1".into(),
        first_column_off: true,
        merges: vec![(0, 0, 2)],
        center_body: true,
    };
    assert!(one_step(&mut s, &mut cx, ShellEdit::FilledTable(Box::new(q))));
    // the gestures on the table (anchor 0, body)
    let pen = BorderSpec { val: "double".into(), ..BorderSpec::default() };
    let gestures = vec![
        TableGesture::Edge { col: true, anchor: 0, path: Vec::new(), boundary: 1, delta: 300.0, row_height: 0.0 },
        TableGesture::Edge { col: false, anchor: 0, path: Vec::new(), boundary: 0, delta: 200.0, row_height: 280.0 },
        TableGesture::Size { anchor: 0, dx: 500.0, dy: 100.0, row_heights: Some(vec![280.0, 280.0]) },
        TableGesture::Paint { anchor: 0, path: Vec::new(), r: 1, c: 0, side: CellSide::Bottom, pen },
        TableGesture::Erase { anchor: 0, path: Vec::new(), r: 1, c: 0, side: CellSide::Right },
        TableGesture::DrawSplit {
            anchor: 0,
            path: Vec::new(),
            r: 1,
            c: 0,
            vertical: true,
            left: 600.0,
            box_w: 3000.0,
            row_heights: vec![280.0],
        },
        TableGesture::Move { anchor: 0, dx: 400.0, dy: 600.0 },
    ];
    for g in gestures {
        let name = format!("{g:?}");
        assert!(one_step(&mut s, &mut cx, ShellEdit::Table(Box::new(g))), "{name} changed nothing");
    }
    // a click on the move handle selects the table (no step)
    let depth = s.history().undo_len();
    s.exec_shell_edit(ShellEdit::Table(Box::new(TableGesture::Move { anchor: 0, dx: 3.0, dy: 2.0 })), &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), depth);
}

#[test]
fn draw_new_takes_two_steps_as_the_ts() {
    let mut cx = Cx::new();
    let mut s = blank();
    let depth = s.history().undo_len();
    s.exec_shell_edit(ShellEdit::Table(Box::new(TableGesture::DrawNew { w: 2400.0, h: 900.0 })), &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), depth + 2, "insertTable, then the height (two steps in the TS)");
    let depth = s.history().undo_len();
    s.exec_shell_edit(ShellEdit::Table(Box::new(TableGesture::DrawNew { w: 2400.0, h: 100.0 })), &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), depth + 1);
}

#[test]
fn m1_completion_edits_return_the_memory() {
    let life = || {
        let mut cx = Cx::new();
        let mut s = blank();
        exec(&mut s, &mut cx, Command::Text(TextCmd::Type { text: "one".into() }));
        exec(&mut s, &mut cx, Command::List(ListCmd::ToggleList { bullet: false }));
        black_box(s.list_info_view());
        black_box(s.document_lists_view());
        black_box(s.list_styles_view());
        black_box(s.numbering_value_view());
        black_box(s.list_library_view(2));
        black_box(s.table_style_listing());
        black_box(s.table_style_xml("List Table 4 Accent 1"));
        table(&mut s, &mut cx, 2.0, 2.0);
        let _ = s.exec_shell_edit(
            ShellEdit::Table(Box::new(TableGesture::Edge {
                col: true,
                anchor: 0,
                path: Vec::new(),
                boundary: 1,
                delta: 300.0,
                row_height: 0.0,
            })),
            &mut cx.cx(),
        );
        let _ = s.exec_shell_edit(
            ShellEdit::Table(Box::new(TableGesture::Erase { anchor: 0, path: Vec::new(), r: 0, c: 0, side: CellSide::Right })),
            &mut cx.cx(),
        );
        let _ = s.exec_shell_edit(ShellEdit::DeleteSelectedObjects, &mut cx.cx());
        s.undo(&mut cx.cx());
        s.clear_caches();
        black_box(s.save_docx().map_or(0, |b| b.len()))
    };
    black_box(life());
    assert_no_leak(life);
}

fn table_dims(s: &Session) -> usize {
    s.document().body.tables.len()
}

#[test]
fn quick_table_lands_in_the_body() {
    let mut cx = Cx::new();
    let mut s = blank();
    let q = FilledTable { rows: vec![vec!["A".into(), "B".into()]; 3], style: "Plain Table 2".into(), ..FilledTable::default() };
    s.exec_shell_edit(ShellEdit::FilledTable(Box::new(q)), &mut cx.cx()).unwrap();
    assert_eq!(table_dims(&s), 1);
    let xml = String::from_utf8_lossy(&s.save_docx().unwrap()).len();
    assert!(xml > 0);
}

// ───────────────────────── wave 7 (A2), second leva: ED-202/204/215, the picture bullet, the queries ─────────────────────────

/// The package's part `name` as text.
fn part_of(bytes: &[u8], name: &str) -> Option<String> {
    let pkg = uo_opc::Package::open(bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).ok()?;
    pkg.part_text(name).ok().flatten().map(std::borrow::Cow::into_owned)
}

fn objects_doc() -> Session {
    let bytes = crate::d2b_f7_support::doc_with_objects(2, 1, 8);
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

#[test]
fn inline_picture_moves_in_the_text() {
    use uo_docx::session::DocPosition;
    let mut cx = Cx::new();
    let mut s = objects_doc();
    let (_, para, run) = s.inline_pictures()[0];
    // a drop on itself: no step, the caret before the picture
    let depth = s.history().undo_len();
    s.exec_shell_edit(ShellEdit::MoveInlineRun { para, run, to: DocPosition::new(para, 5) }, &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), depth);
    // a drop at the start of the paragraph: one step, the picture first and selected
    let depth = s.history().undo_len();
    s.exec_shell_edit(ShellEdit::MoveInlineRun { para, run, to: DocPosition::new(para, 0) }, &mut cx.cx()).unwrap();
    assert_eq!(s.history().undo_len(), depth + 1);
    let (_, p2, r2) = s.inline_pictures()[0];
    assert_eq!((p2, r2), (para, 0), "the picture is the first run now");
    assert_eq!(s.selected_drawings().len(), 1, "the moved picture is selected");
    s.undo(&mut cx.cx());
    // a drop in the next paragraph
    let (_, para, run) = s.inline_pictures()[0];
    assert!(one_step(&mut s, &mut cx, ShellEdit::MoveInlineRun { para, run, to: DocPosition::new(para + 1, 2) }));
}

#[test]
fn drag_commits_are_one_step_with_the_effect_extent() {
    use uo_docx::session::ops::drawing::DrawingCmd;
    use uo_docx::session::ops::shell_edits::DragCommit;
    let mut cx = Cx::new();
    let mut s = objects_doc();
    let (_, para, run) = s.inline_pictures()[0];
    exec(&mut s, &mut cx, Command::Drawing(DrawingCmd::SelectInline { para, run }));
    // the resize (ED-202): the size and the effect extent recomputed in one step
    let d = DragCommit { size: Some((900.0, 700.0)), reset_rel: true, ..DragCommit::default() };
    assert!(one_step(&mut s, &mut cx, ShellEdit::CommitDrag(Box::new(d))));
    let xml = part_of(&s.save_docx().unwrap(), "word/document.xml").unwrap();
    assert!(xml.contains("cx=\"571500\"") && xml.contains("cy=\"444500\""), "the new size in EMU");
    // the rotation (the selection again: undo/redo of `one_step` leave the caret)
    exec(&mut s, &mut cx, Command::Drawing(DrawingCmd::SelectInline { para, run }));
    assert!(one_step(&mut s, &mut cx, ShellEdit::CommitDrag(Box::new(DragCommit { rot: Some(30.0), ..DragCommit::default() }))));
    let xml = part_of(&s.save_docx().unwrap(), "word/document.xml").unwrap();
    assert!(xml.contains("rot=\"1800000\""), "30 degrees");
    // the Crop drag (ED-215): the source rectangle and the size in one step
    exec(&mut s, &mut cx, Command::Drawing(DrawingCmd::SelectInline { para, run }));
    let d = DragCommit { src_rect: Some((0.1, 0.0, 0.0, 0.2)), size: Some((800.0, 560.0)), ..DragCommit::default() };
    assert!(one_step(&mut s, &mut cx, ShellEdit::CommitDrag(Box::new(d))));
    let xml = part_of(&s.save_docx().unwrap(), "word/document.xml").unwrap();
    assert!(xml.contains("a:srcRect l=\"10000\"") && xml.contains("b=\"20000\""), "the crop in thousandths of a percent");
}

#[test]
fn picture_bullet_writes_the_part_and_the_media() {
    use uo_docx::session::ops::lists::{ListLevelFormat, PicBulletIn};
    let mut cx = Cx::new();
    let mut s = blank();
    exec(&mut s, &mut cx, Command::Text(TextCmd::Type { text: "item".into() }));
    let png: std::sync::Arc<[u8]> = std::sync::Arc::from(crate::d2b_f7_support::PNG);
    let e = ShellEdit::DefineListLevelPicture {
        format: Box::new(ListLevelFormat::default()),
        pic: Box::new(PicBulletIn { data: png, ext: "png".into(), w_pt: 9.0, h_pt: 9.0, name: "bola".into() }),
    };
    s.exec_shell_edit(e, &mut cx.cx()).unwrap();
    let info = s.list_info_view().unwrap();
    assert!(info.bullet);
    let bytes = s.save_docx().unwrap();
    let num = part_of(&bytes, "word/numbering.xml").unwrap();
    assert!(num.contains("<w:numPicBullet w:numPicBulletId=\"0\">") && num.contains("o:title=\"bola\""), "{num}");
    assert!(num.contains("<w:lvlPicBulletId w:val=\"0\"/>"));
    assert!(num.contains("w:lvlText w:val=\"\u{F0B7}\""), "the Symbol character in reserve");
    let rels = part_of(&bytes, "word/_rels/numbering.xml.rels").unwrap();
    assert!(rels.contains("Id=\"rIdUwPb1\"") && rels.contains("Target=\"media/uwbullet1.png\""), "{rels}");
    assert!(
        part_of(&bytes, "word/media/uwbullet1.png").is_some()
            || uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
                .unwrap()
                .contains("word/media/uwbullet1.png")
    );
    // the layout model has the picture
    assert!(s.document().formatting.numbering.pic_bullets.values().any(|p| p.image.is_some()));
}

#[test]
fn wave7_queries_answer() {
    use uo_docx::session::ops::drawing::DrawingCmd;
    use uo_docx::session::ops::header_footer::hf_templates::HfGallery;
    use uo_docx::session::ops::page::PageLang;
    use uo_docx::session::ops::previews::PreviewKind;
    let mut cx = Cx::new();
    let mut s = blank();
    let sp = s.sect_pr_json(0);
    assert!(sp.contains("\"pageWidthTwips\"") && sp.contains("\"marginLeftTwips\""), "{sp}");
    assert_eq!(s.sect_pr_json(99), sp, "past the end: the document's sectPr");
    let x = s.header_footer_template_xml(HfGallery::Header, "blank", PageLang::En).unwrap();
    assert!(x.contains("<w:p"));
    for g in [HfGallery::Header, HfGallery::Footer, HfGallery::PageCurrent] {
        let id = s.header_footer_templates(g, PageLang::Pt)[0].0.to_owned();
        let p = s.preview_document(&PreviewKind::HeaderFooter { gallery: g, id, lang: PageLang::Pt, section: 0 });
        let p = p.unwrap_or_else(|| panic!("{g:?}"));
        assert_eq!(p.document.sections.len(), 1);
        assert_eq!(p.document.hf.len(), usize::from(g != HfGallery::PageCurrent));
    }
    assert!(s.selected_shape_parts_json().is_none(), "no drawing selected");
    exec(
        &mut s,
        &mut cx,
        Command::Drawing(DrawingCmd::InsertTextbox { id: "simple".into(), page: None, theme_accent1: None, lang: "en".into(), user: None }),
    );
    // the box selected as a shape (the selection pane's item: no layout here to enter it)
    let key = s.pane_keys().into_iter().next().unwrap();
    exec(&mut s, &mut cx, Command::Drawing(DrawingCmd::SelectPaneItem { key: key.into(), add: false }));
    let parts = s.selected_shape_parts_json().unwrap();
    assert!(parts.starts_with("{\"fill\":") && parts.contains("\"textbox\":true") && parts.contains("\"body\":{\"lIns\":91440"), "{parts}");
    let (xml, name) = s.textbox_gallery_item().unwrap();
    assert!(xml.starts_with("<w:drawing") || xml.contains("<w:txbxContent>"), "{xml}");
    assert!(name.is_some());
}

// ───────────────────────── wave 7 (A2), fix round: the formatting policy on the shell edits ─────────────────────────

/// The shell edits that reach a guarded TS method (`insertQuickTable` and `finishDraw` → `insertTable`, a picture bullet →
/// `defineListLevel`) are refused under a policy that does not allow them: no step, the model unchanged, `Blocked` raised
/// (the TS `installPolicyGuards` wrapper returns without effect; the session's `exec` does the same). The other edits run.
#[test]
fn shell_edits_follow_the_formatting_policy() {
    use uo_docx::session::Event;
    use uo_docx::session::command::Outcome;
    use uo_docx::session::ops::lists::{ListLevelFormat, PicBulletIn};
    use uo_docx::session::ops::settings::SettingsCmd;
    use uo_docx::session::policy::{Flag, FormattingPolicy, ListKind};
    let mut cx = Cx::new();
    let mut s = blank();
    exec(&mut s, &mut cx, Command::Text(TextCmd::Type { text: "item".into() }));
    let policy = FormattingPolicy {
        lists: Some((vec![ListKind::Bullet], Some(9.0))),
        denied: [Flag::Tables].into_iter().collect(),
        ..FormattingPolicy::default()
    };
    exec(&mut s, &mut cx, Command::Settings(SettingsCmd::SetPolicy { policy: Some(policy) }));
    let png: std::sync::Arc<[u8]> = std::sync::Arc::from(crate::d2b_f7_support::PNG);
    let refused = [
        (
            ShellEdit::FilledTable(Box::new(FilledTable {
                rows: vec![vec!["A".into(), "B".into()]; 2],
                style: "Plain Table 2".into(),
                ..FilledTable::default()
            })),
            "insertTable",
        ),
        (ShellEdit::Table(Box::new(TableGesture::DrawNew { w: 2400.0, h: 900.0 })), "insertTable"),
        (
            ShellEdit::DefineListLevelPicture {
                format: Box::new(ListLevelFormat::default()),
                pic: Box::new(PicBulletIn { data: png, ext: "png".into(), w_pt: 9.0, h_pt: 9.0, name: "bola".into() }),
            },
            "defineListLevel",
        ),
    ];
    for (e, method) in refused {
        let before = s.save_docx().unwrap();
        let depth = s.history().undo_len();
        let out = s.exec_shell_edit(e, &mut cx.cx()).unwrap();
        let Outcome::Done(cs) = out else { panic!("a refused edit is done") };
        assert!(cs.events.iter().any(|ev| matches!(ev, Event::Blocked { command } if &**command == method)), "{method}: {:?}", cs.events);
        assert_eq!(s.history().undo_len(), depth, "{method}: no step");
        assert_eq!(s.save_docx().unwrap(), before, "{method}: the model unchanged");
    }
    assert_eq!(table_dims(&s), 0);
    assert!(s.list_info_view().is_none());
    // the gestures that reach no guarded TS method run under the same policy (the move handle's click: no step, no Blocked)
    let out = s.exec_shell_edit(ShellEdit::Table(Box::new(TableGesture::Move { anchor: 0, dx: 0.0, dy: 0.0 })), &mut cx.cx()).unwrap();
    let Outcome::Done(cs) = out else { panic!() };
    assert!(!cs.events.iter().any(|ev| matches!(ev, Event::Blocked { .. })));
    // without the policy the same edits run
    exec(&mut s, &mut cx, Command::Settings(SettingsCmd::SetPolicy { policy: None }));
    let q = FilledTable { rows: vec![vec!["A".into(), "B".into()]; 2], style: "Plain Table 2".into(), ..FilledTable::default() };
    s.exec_shell_edit(ShellEdit::FilledTable(Box::new(q)), &mut cx.cx()).unwrap();
    assert_eq!(table_dims(&s), 1);
}

/// Fix F1 (wave 7 final review, A2 minor): the inline picture selection is the run OBJECT (TS `picSel`). A body
/// picture at `[0, 0]` stays the selected one when the edit context becomes a table cell whose paragraph 0 also holds
/// an inline picture at run 0 under the text selection: the TS `selectedFloat()` does not find `picSel` in the cell's
/// paragraphs, so nothing is selected. Compared by the paragraph's identity (`ParaId`), not by the index.
#[test]
fn inline_selection_is_the_run_not_its_index() {
    use std::sync::Arc;
    use uo_docx::session::ops::context::ContextCmd;
    use uo_docx::session::ops::pictures::PictureCmd;
    use uo_docx::session::state::CellHop;
    use uo_docx::session::{DocPosition, JobResult, Outcome};
    let mut cx = Cx::new();
    let mut s = blank();
    let picture = |s: &mut Session, cx: &mut Cx| {
        let c = Command::Picture(PictureCmd::Insert {
            data: Arc::from(crate::d2b_f7_support::PNG),
            ext: "png".into(),
            ole: None,
            max_width_twips: None,
        });
        let Outcome::NeedsJobs(jobs) = s.exec(c, &mut cx.cx()).unwrap() else { panic!("the insert asks for the size") };
        s.complete(jobs[0].id(), JobResult::Image { width: 100, height: 50, rgba: None }, &mut cx.cx()).unwrap();
    };
    // a 1 x 1 table (the caret goes into its cell), a picture in the cell, then back to the body
    table(&mut s, &mut cx, 1.0, 1.0);
    picture(&mut s, &mut cx);
    exec(&mut s, &mut cx, Command::Context(ContextCmd::ExitCell));
    // a picture at the start of body paragraph 0: selected
    let p0 = DocPosition::new(0, 0);
    exec(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: p0, focus: p0 }));
    picture(&mut s, &mut cx);
    let (_, para, run) = s.inline_pictures()[0];
    assert_eq!((para, run), (0, 0));
    exec(&mut s, &mut cx, Command::Drawing(uo_docx::session::ops::drawing::DrawingCmd::SelectInline { para, run }));
    assert_eq!(s.selected_drawings().len(), 1, "the body picture is selected");
    // into the cell, the text selection over ITS picture (also `[0, 0]`)
    let hops = vec![CellHop { row: 0, col: 0, block: None }];
    exec(&mut s, &mut cx, Command::Context(ContextCmd::EnterCell { anchor: 0, hops, caret: None }));
    assert_eq!(s.inline_pictures().iter().map(|p| (p.1, p.2)).collect::<Vec<_>>(), [(0, 0)]);
    exec(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: p0, focus: DocPosition::new(0, 1) }));
    assert!(s.selected_drawings().is_empty(), "the body picture is not the cell's");
}
