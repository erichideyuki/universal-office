//! Behaviour of the D2b-F9 commands (record `docs/modules/uo-docx-d2b/F9.md`): every `FindCmd` and
//! `NavigationCmd` variant and the queries, with their no-op, error, lock and budget paths.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::support;

use d2a_support::Cx;
use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::ops::find::engine::{FindQuery, QueryError};
use uo_docx::session::ops::find::{Direction, FindCmd, GoToWhat, Within};
use uo_docx::session::ops::format::{FormatCmd, RunToggle};
use uo_docx::session::ops::navigation::{F9Result, FindOptions, HeadingWhere, NavigationCmd, SearchLoc};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::text::para_text;
use uo_docx::session::{Command, DocPosition, DocRange, FixedEnv, OpenOptions, Outcome, Selection, Session};

fn open_body(body: &str) -> Session {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    let bytes = pkg.save(&uo_opc::package::SaveOptions::default()).unwrap();
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
}

fn h(level: u32, text: &str) -> String {
    format!("<w:p><w:pPr><w:outlineLvl w:val=\"{}\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>", level - 1)
}

fn q(text: &str) -> FindQuery {
    FindQuery { text: text.encode_utf16().collect(), ..FindQuery::default() }
}

fn exec(s: &mut Session, cx: &mut Cx, c: Command) -> Outcome {
    s.exec(c, &mut cx.cx()).unwrap()
}

fn sel(a: (u32, u32), f: (u32, u32)) -> Selection {
    Selection { anchor: DocPosition::new(a.0, a.1), focus: DocPosition::new(f.0, f.1) }
}

fn text(s: &Session, i: usize) -> String {
    String::from_utf16(&para_text(s.document().body.paras.get(i).unwrap())).unwrap()
}

fn find(s: &mut Session, cx: &mut Cx, t: &str, d: Direction) -> F9Result {
    exec(s, cx, Command::Find(FindCmd::FindNext { query: q(t), direction: d }));
    s.navigation_result().cloned().unwrap()
}

#[test]
fn find_next_moves_wraps_and_goes_up() {
    let mut s = open_body(&(p("alpha beta alpha") + &p("gamma alpha")));
    let mut cx = Cx::new();
    let r = find(&mut s, &mut cx, "alpha", Direction::All);
    assert!(matches!(r, F9Result::Find(ref o) if o.found && !o.wrapped));
    assert_eq!(s.selection(), sel((0, 0), (0, 5)));
    find(&mut s, &mut cx, "alpha", Direction::All);
    assert_eq!(s.selection(), sel((0, 11), (0, 16)));
    find(&mut s, &mut cx, "alpha", Direction::Down);
    assert_eq!(s.selection(), sel((1, 6), (1, 11)));
    let r = find(&mut s, &mut cx, "alpha", Direction::Down);
    assert!(matches!(r, F9Result::Find(ref o) if !o.found));
    let r = find(&mut s, &mut cx, "ALPHA", Direction::All);
    assert!(matches!(r, F9Result::Find(ref o) if o.found && o.wrapped));
    assert_eq!(s.selection(), sel((0, 0), (0, 5)));
    find(&mut s, &mut cx, "alpha", Direction::Up);
    assert_eq!(s.selection(), sel((0, 0), (0, 5)), "nothing before the first: the selection stays");
    // no step taken by a search
    assert!(!s.can_undo());
}

#[test]
fn query_errors_are_answers_not_changes() {
    let mut s = open_body(&p("e-mail here"));
    let mut cx = Cx::new();
    for (t, wild, kind) in [("^x", false, "special"), ("e-mail", false, "syntax"), ("[z-a]", true, "pattern"), ("(a", true, "pattern")] {
        let query = FindQuery { text: t.encode_utf16().collect(), wildcards: wild, ..FindQuery::default() };
        let e = s.find_query_error(&query).unwrap();
        match (&e, kind) {
            (QueryError::Special(m), "special") => assert_eq!(String::from_utf16(m).unwrap(), "^x"),
            (QueryError::Syntax(m), "syntax") => {
                assert_eq!(String::from_utf16(m).unwrap(), "SyntaxError: Invalid regular expression: /e\\-mail/giu: Invalid escape");
            }
            (QueryError::Pattern, "pattern") => {}
            other => panic!("{t}: {other:?}"),
        }
        exec(&mut s, &mut cx, Command::Find(FindCmd::FindNext { query: query.clone(), direction: Direction::All }));
        assert!(matches!(s.navigation_result(), Some(F9Result::Find(o)) if o.error.as_ref() == Some(&e)));
        assert_eq!(s.count_matches(&query, Within::Document).unwrap(), 0);
    }
    let pt = String::from_utf16(&QueryError::Pattern.message(true)).unwrap();
    assert!(pt.starts_with("O texto em"));
    assert!(!s.can_undo());
}

#[test]
fn replace_match_replace_all_undo_and_lock() {
    let mut s = open_body(&(p("Beta beta BETA") + &p("beta")));
    let mut cx = Cx::new();
    // not on a hit: replaceMatch only finds
    exec(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceMatch {
            query: q("beta"),
            replacement: "gamma".encode_utf16().collect(),
            direction: Direction::All,
            replace_format: None,
        }),
    );
    assert!(matches!(s.navigation_result(), Some(F9Result::Replace(o)) if !o.replaced && o.find.found));
    assert_eq!(s.selection(), sel((0, 0), (0, 4)));
    // on a hit: replaced with the case adapted, then the next one found
    exec(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceMatch {
            query: q("beta"),
            replacement: "gamma".encode_utf16().collect(),
            direction: Direction::All,
            replace_format: None,
        }),
    );
    assert_eq!(text(&s, 0), "Gamma beta BETA");
    assert_eq!(s.selection(), sel((0, 6), (0, 10)));
    exec(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceAll {
            query: q("beta"),
            replacement: "x^py".encode_utf16().collect(),
            within: Within::Document,
            replace_format: None,
        }),
    );
    assert!(matches!(s.navigation_result(), Some(F9Result::Replace(o)) if o.count == 3));
    assert_eq!(s.document().body.paras.len(), 5);
    assert_eq!(text(&s, 1), "y X");
    s.undo(&mut cx.cx());
    assert_eq!(text(&s, 0), "Gamma beta BETA");
    s.undo(&mut cx.cx());
    assert_eq!(text(&s, 0), "Beta beta BETA");
    // locked: nothing
    exec(&mut s, &mut cx, Command::Settings(SettingsCmd::SetReadView { on: true }));
    exec(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceAll { query: q("beta"), replacement: Vec::new(), within: Within::Document, replace_format: None }),
    );
    assert!(matches!(s.navigation_result(), Some(F9Result::Replace(o)) if o.count == 0));
    assert_eq!(text(&s, 0), "Beta beta BETA");
}

#[test]
fn select_all_matches_is_a_multi_selection() {
    let mut s = open_body(&(p("one two one") + &p("one")));
    let mut cx = Cx::new();
    exec(&mut s, &mut cx, Command::Find(FindCmd::SelectAllMatches { query: q("one"), within: Within::Document }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Count(3))));
    assert_eq!(s.selection_ranges().len(), 3);
    // the formatting goes to every range
    exec(&mut s, &mut cx, Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".into() }));
    let bold_runs = s.document().body.paras.iter().flat_map(|p| p.runs.iter()).filter(|r| r.props.bold == Some(true)).count();
    assert_eq!(bold_runs, 3);
    // Backspace deletes every range
    exec(&mut s, &mut cx, Command::Text(TextCmd::Backspace));
    assert_eq!(text(&s, 0), " two ");
    assert_eq!(text(&s, 1), "");
    assert_eq!(s.selection_ranges().len(), 1);
    // undo drops the extra ranges (a new selection object)
    s.undo(&mut cx.cx());
    assert_eq!(s.selection_ranges().len(), 1);
}

#[test]
fn set_selection_ranges_and_select_all() {
    let mut s = open_body(&(p("abcdef") + &p("xyz")));
    let mut cx = Cx::new();
    let r = |a: (u32, u32), b: (u32, u32)| DocRange::new(DocPosition::new(a.0, a.1), DocPosition::new(b.0, b.1));
    exec(
        &mut s,
        &mut cx,
        Command::Navigation(NavigationCmd::SetSelectionRanges { ranges: vec![r((1, 2), (1, 0)), r((0, 99), (0, 1)), r((0, 1), (0, 3))] }),
    );
    assert_eq!(s.selection(), sel((0, 1), (0, 3)));
    assert_eq!(s.selection_ranges(), vec![r((0, 1), (0, 3)), r((1, 0), (1, 2))]);
    // a plain setSelection drops them
    exec(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(0, 1), focus: DocPosition::new(0, 3) }));
    assert_eq!(s.selection_ranges().len(), 1);
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::SetSelectionRanges { ranges: vec![r((9, 0), (9, 1))] }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(false))));
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::SelectAll));
    assert_eq!(s.selection(), sel((0, 0), (1, 3)));
}

#[test]
fn heading_commands() {
    let body = h(1, "A") + &p("a1") + &h(2, "A.1") + &p("a2") + &h(1, "B") + &p("b1");
    let mut s = open_body(&body);
    let mut cx = Cx::new();
    let heads = s.headings();
    assert_eq!(heads.iter().map(|x| (x.level, x.para)).collect::<Vec<_>>(), vec![(1.0, 0), (2.0, 2), (1.0, 4)]);
    // move section B before A
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::MoveHeading { index: 2, before: 0 }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(true))));
    assert_eq!((text(&s, 0), text(&s, 2)), ("B".to_owned(), "A".to_owned()));
    assert_eq!(s.selection(), sel((0, 0), (0, 0)));
    // into itself: refused
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::MoveHeading { index: 1, before: 2 }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(false))));
    s.undo(&mut cx.cx());
    assert_eq!(text(&s, 0), "A");
    // select, go to, insert, delete
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::SelectHeading { index: 0 }));
    assert_eq!(s.selection(), sel((0, 0), (3, 2)));
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::GoToHeading { index: 2 }));
    assert_eq!(s.selection(), sel((4, 0), (4, 0)));
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::InsertHeading { index: 0, at: HeadingWhere::After }));
    assert_eq!(s.document().body.paras.len(), 7);
    assert_eq!(s.selection(), sel((4, 0), (4, 0)));
    let before = s.document().body.paras.len();
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::DeleteHeading { index: 0 }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(true))));
    assert!(s.document().body.paras.len() < before);
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::PromoteHeading { index: 0 }));
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::DemoteHeading { index: 9 }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(false))));
    // locked: false, nothing changes
    exec(&mut s, &mut cx, Command::Settings(SettingsCmd::SetReadView { on: true }));
    let n = s.document().body.paras.len();
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::DeleteHeading { index: 0 }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(false))));
    assert_eq!(s.document().body.paras.len(), n);
}

#[test]
fn navigation_pane_search_and_results() {
    let tbl = "<w:tbl><w:tblGrid><w:gridCol w:w=\"3000\"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>cell word</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let mut s = open_body(&(h(1, "Top") + &p("a word here") + tbl + &p("last word")));
    let mut cx = Cx::new();
    exec(
        &mut s,
        &mut cx,
        Command::Navigation(NavigationCmd::Search { text: "word".encode_utf16().collect(), opts: FindOptions::default() }),
    );
    let r = s.search_results();
    assert_eq!(r.len(), 3);
    assert!(matches!(r[1].loc, SearchLoc::Cell { .. }) && r[1].in_table);
    assert_eq!(r[0].heading, 0);
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::GoToSearchResult { index: 1 }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(true))));
    assert!(matches!(s.context(), uo_docx::session::EditContext::Cell { .. }));
    assert_eq!(s.selection(), sel((0, 5), (0, 9)));
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::GoToSearchResult { index: 2 }));
    assert_eq!(s.selection(), sel((2, 5), (2, 9)));
    exec(&mut s, &mut cx, Command::Navigation(NavigationCmd::ClearSearch));
    assert!(s.search_results().is_empty());
    exec(&mut s, &mut cx, Command::Find(FindCmd::HighlightMatches { query: q("word"), within: Within::Document }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Count(3))));
    assert_eq!(s.highlight_spans().len(), 3);
}

#[test]
fn go_to_targets() {
    let body = p("one")
        + "<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>two</w:t></w:r></w:p>"
        + &p("three")
        + "<w:p><w:bookmarkStart w:id=\"1\" w:name=\"Mark\"/><w:r><w:t>four</w:t></w:r><w:bookmarkEnd w:id=\"1\"/></w:p>"
        + "<w:p><w:fldSimple w:instr=\" PAGE \"><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>";
    let mut s = open_body(&body);
    let mut cx = Cx::new();
    let go = |s: &mut Session, cx: &mut Cx, what: GoToWhat, t: &str| {
        exec(s, cx, Command::Find(FindCmd::GoTo { what, target: t.encode_utf16().collect() }));
        matches!(s.navigation_result(), Some(F9Result::Bool(true)))
    };
    assert!(go(&mut s, &mut cx, GoToWhat::Section, ""));
    assert_eq!(s.selection(), sel((2, 0), (2, 0)));
    assert!(go(&mut s, &mut cx, GoToWhat::Section, "1"));
    assert_eq!(s.selection(), sel((0, 0), (0, 0)));
    assert!(go(&mut s, &mut cx, GoToWhat::Bookmark, "mark"));
    assert_eq!(s.selection(), sel((3, 0), (3, 4)));
    assert!(go(&mut s, &mut cx, GoToWhat::Field, "page"));
    assert_eq!(s.selection(), sel((4, 0), (4, 0)));
    assert!(!go(&mut s, &mut cx, GoToWhat::Field, "TOC"));
    assert!(!go(&mut s, &mut cx, GoToWhat::Table, ""));
    assert!(!go(&mut s, &mut cx, GoToWhat::Bookmark, ""));
    assert!(s.exec(Command::Find(FindCmd::GoTo { what: GoToWhat::Page, target: Vec::new() }), &mut cx.cx()).is_err());
    exec(&mut s, &mut cx, Command::Find(FindCmd::GoToComment { id: f64::NAN }));
    exec(&mut s, &mut cx, Command::Find(FindCmd::GoToBookmark { name: "nope".into() }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Bool(false))));
}

#[test]
fn a_search_past_its_budget_changes_nothing() {
    let mut s = open_body(&(p(&"a".repeat(40)) + &p("b")));
    let mut cx = Cx::new();
    s.set_find_budget(Some(20_000));
    let query = FindQuery { text: "(a@)@#".encode_utf16().collect(), wildcards: true, ..FindQuery::default() };
    // a '#' exists after the run of a: no prune skips the exponential search
    exec(&mut s, &mut cx, Command::Text(TextCmd::SetSelection { anchor: DocPosition::new(1, 1), focus: DocPosition::new(1, 1) }));
    exec(&mut s, &mut cx, Command::Text(TextCmd::Type { text: "#".into() }));
    let before = s.selection();
    exec(&mut s, &mut cx, Command::Find(FindCmd::FindNext { query: query.clone(), direction: Direction::All }));
    assert!(matches!(s.navigation_result(), Some(F9Result::Find(o)) if o.budget && !o.found));
    assert_eq!(s.selection(), before);
    exec(
        &mut s,
        &mut cx,
        Command::Find(FindCmd::ReplaceAll { query, replacement: Vec::new(), within: Within::Document, replace_format: None }),
    );
    assert!(matches!(s.navigation_result(), Some(F9Result::Replace(o)) if o.find.budget && o.count == 0));
}
