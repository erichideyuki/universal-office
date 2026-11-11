//! The replay mappings and facets of the families F5 (notes) and F9 (find, navigation) for the wasm replay
//! (tests/session_replay.test.mjs; follow-up 6 of the wave-2 record). Copies of the native replays'
//! `step` mappers and facet functions (`parity_d2b_f5_session.rs`, `parity_d2b_f9_session.rs`; declared: one copy
//! of test code each, nothing here is shipped), rewritten as "action k of a code" like the siblings in the
//! parent module.

use uo_docx::js::JsonWriter;
use uo_docx::session::ops::context::{ContextCmd, EditContext, saved_selection};
use uo_docx::session::ops::find::{Direction, FindCmd, GoToWhat, Within};
use uo_docx::session::ops::format::{FormatCmd, RunToggle};
use uo_docx::session::ops::navigation::{F9Result, FindOptions, HeadingWhere, NavigationCmd};
use uo_docx::session::ops::notes::{ConvertMode, NoteCmd, NoteOptionsPatch, NoteScope};
use uo_docx::session::{CellHop, Command, DocPosition, DocRange, Session};
use uo_docx::text::Utf16;
use uo_testkit::Digest;

use super::{Action, anc_json, cmd, paras_digest, units};
use crate::d2b_f9_support::{REPLS, format, hx, query};

const MARKS: [Option<&str>; 4] = [None, Some("*"), Some("  "), Some(" † ")];
const MODES: [ConvertMode; 3] = [ConvertMode::ToEndnotes, ConvertMode::ToFootnotes, ConvertMode::Swap];
const GOTO: [GoToWhat; 11] = [
    GoToWhat::Section,
    GoToWhat::Heading,
    GoToWhat::Bookmark,
    GoToWhat::Comment,
    GoToWhat::Footnote,
    GoToWhat::Endnote,
    GoToWhat::Field,
    GoToWhat::Table,
    GoToWhat::Graphic,
    GoToWhat::Equation,
    GoToWhat::Object,
];

fn nn(f: &[&str], i: usize) -> u32 {
    f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn ii(f: &[&str], i: usize) -> i64 {
    f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn at<'a>(f: &[&'a str], i: usize) -> &'a str {
    f.get(i).copied().unwrap_or("")
}

fn n(x: f64) -> String {
    uo_jsnum::format::to_js_string(x)
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn pos_json(p: DocPosition) -> String {
    format!("{{\"para\":{},\"offset\":{}}}", p.para, p.offset)
}

fn sel_of(sel: uo_docx::session::Selection) -> String {
    format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(sel.anchor), pos_json(sel.focus))
}

/// `state.s`: the body's selection (in a cell or a note, the one saved on entering).
fn sel_json(s: &Session) -> String {
    sel_of(saved_selection(s.context()).unwrap_or_else(|| s.selection()))
}

// ───────────────────────────────── F5 ─────────────────────────────────

fn patch(i: usize) -> NoteOptionsPatch {
    let mut p = NoteOptionsPatch::default();
    let s = |x: &str| Some(x.to_owned());
    match i {
        0 => p.number_format = s("lowerLetter"),
        1 => p.start_at = Some(5.0),
        2 => p.restart = s("eachSect"),
        3 => p.restart = s("eachPage"),
        4 => p.position = s("beneathText"),
        5 => p.position = s("sectEnd"),
        6 => p.columns = Some(2.0),
        7 => p.columns = Some(0.0),
        8 => {
            p.number_format = s("decimal");
            p.start_at = Some(1.0);
            p.restart = s("continuous");
            p.position = s("pageBottom");
        }
        9 => p.start_at = Some(0.4),
        10 => p.start_at = Some(-3.0),
        11 => {
            p.number_format = s("upperRoman");
            p.restart = s("eachSect");
            p.start_at = Some(7.0);
        }
        12 => p.position = s("docEnd"),
        13 => {
            p.number_format = s("chicago");
            p.start_at = Some(2.5);
        }
        _ => {}
    }
    p
}

/// `step` of `parity_d2b_f5_session.rs` for the codes that are not D2a's (one action each).
pub(super) fn f5_action(f: &[&str], s: &Session) -> Option<Option<Action>> {
    let refs = s.note_refs(None);
    let ref_at = |i: i64| usize::try_from(i).ok().and_then(|i| refs.get(i)).map(|r| (r.footnote, r.id));
    let c = match *f.first()? {
        "nf" => Command::Note(NoteCmd::Insert {
            footnote: nn(f, 1) == 0,
            mark: MARKS.get(nn(f, 2) as usize).copied().flatten().map(Utf16::from),
        }),
        "no" => Command::Note(NoteCmd::SetOptions {
            footnote: nn(f, 1) == 0,
            patch: patch(nn(f, 2) as usize),
            scope: if at(f, 3) == "1" { NoteScope::Section } else { NoteScope::Document },
        }),
        "cv" => Command::Note(NoteCmd::Convert { mode: *MODES.get(nn(f, 1) as usize)?, only: ref_at(ii(f, 2)) }),
        "on" => {
            let (footnote, id) = ref_at(ii(f, 1)).unwrap_or((true, 99999.0));
            Command::Note(NoteCmd::Open { footnote, id })
        }
        "gr" => {
            let (footnote, id) = ref_at(ii(f, 1)).unwrap_or((true, 99999.0));
            Command::Note(NoteCmd::GoToRef { footnote, id })
        }
        "nn" => Command::Note(NoteCmd::Next { footnote: nn(f, 1) == 0, forward: nn(f, 2) == 1 }),
        "sh" => Command::Note(NoteCmd::Show { footnote: if nn(f, 1) == 2 { None } else { Some(nn(f, 1) == 0) } }),
        "xn" => Command::Context(ContextCmd::ExitNote),
        "ec" => {
            Command::Context(ContextCmd::EnterCell { anchor: nn(f, 1), hops: vec![CellHop { row: 0, col: 0, block: None }], caret: None })
        }
        "xc" => Command::Context(ContextCmd::ExitCell),
        _ => return None,
    };
    Some(cmd(c))
}

fn csel_json(s: &Session) -> String {
    match s.context() {
        EditContext::Cell { .. } | EditContext::Note { .. } => sel_of(s.selection()),
        _ => "null".to_owned(),
    }
}

fn ctx5(s: &Session) -> String {
    match s.context() {
        EditContext::Note { footnote, id, .. } => format!("note:{}:{}", if *footnote { "footnote" } else { "endnote" }, n(*id)),
        EditContext::Cell { anchor, hops, .. } => {
            let h: Vec<String> = hops.iter().map(|h| format!("[{},{}]", h.row, h.col)).collect();
            format!("cell:{anchor}:[{}]", h.join(","))
        }
        _ => "body".to_owned(),
    }
}

fn no_text(s: &Session) -> String {
    let o = |footnote: bool| {
        let o = s.note_options(footnote);
        format!("{},{},{},{},{}", o.position, o.number_format, n(o.start_at), o.restart, o.columns.map_or("-".to_owned(), n))
    };
    format!("{}#{}#{}#{}", o(true), o(false), s.note_count(true), s.note_count(false))
}

fn ref_text(s: &Session) -> String {
    s.note_refs(None)
        .iter()
        .map(|r| {
            let cell = r.cell.map_or("-".to_owned(), |(a, h)| format!("{a}.{}.{}", h.row, h.col));
            format!("{}:{}:{}:{cell}:{}", if r.footnote { "footnote" } else { "endnote" }, n(r.id), r.para, r.offset)
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn run_sig(r: &uo_docx::model::run::Run) -> String {
    let p = &r.props;
    let field = r
        .ext()
        .and_then(|e| e.field.as_deref())
        .filter(|f| !f.is_empty())
        .map_or(String::new(), |f| hx(&f.encode_utf16().collect::<Vec<_>>()));
    let note = r
        .ext()
        .and_then(|e| e.note_ref.as_ref())
        .map_or(String::new(), |nr| format!("{}:{}:{}", nr.kind, n(nr.id.to_number()), u8::from(nr.custom == Some(true))));
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        hx(r.text.units()),
        p.char_style.as_deref().unwrap_or(""),
        p.vert_align.as_deref().unwrap_or(""),
        p.ins.as_deref().map_or("", |i| &i.author),
        p.del.as_deref().map_or("", |i| &i.author),
        field,
        note
    )
}

fn nts_text(s: &Session) -> String {
    let side = |footnote: bool| -> String {
        let doc = s.document();
        let map = if footnote { &doc.notes.footnotes } else { &doc.notes.endnotes };
        let mut items: Vec<_> = map.iter().collect();
        items.sort_by(|a, b| a.0.get().total_cmp(&b.0.get()));
        items
            .iter()
            .map(|(id, st)| {
                let paras: Vec<String> = st
                    .paras
                    .iter()
                    .map(|p| {
                        format!(
                            "{}:{}",
                            p.props.style_id.as_deref().unwrap_or(""),
                            p.runs.iter().map(run_sig).collect::<Vec<_>>().join(",")
                        )
                    })
                    .collect();
                format!("{}={}", n(id.get()), paras.join(";"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("{}\n#\n{}", side(true), side(false))
}

fn sect_text(s: &Session) -> String {
    let of = |sp: &uo_docx::model::section::SectPr| {
        let j = |p: &Option<uo_docx::model::formatting::NoteProps>| {
            p.as_ref().and_then(|p| JsonWriter::stringify(p, None)).unwrap_or_else(|| "null".to_owned())
        };
        format!("{}|{}|{}", j(&sp.footnote_pr), j(&sp.endnote_pr), sp.footnote_columns.map_or(String::new(), |c| n(c.to_number())))
    };
    let mut out: Vec<String> = Vec::new();
    for p in s.document().body.paras.iter() {
        if let Some(sb) = p.props.section_break.as_deref() {
            out.push(of(&sb.sect_pr));
        }
    }
    out.push(of(&s.section_state().body_sect_pr));
    out.join("\n")
}

fn saved(s: &mut Session) -> Option<uo_opc::Package> {
    let bytes = s.save_docx().ok()?;
    uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).ok()
}

fn part_digest(pkg: &uo_opc::Package, name: &str) -> String {
    let t = pkg.part_text(name).ok().flatten().map(|t| t.into_owned());
    sha(t.as_deref().unwrap_or("-"))
}

fn facets_f5(s: &mut Session, want_docxml: bool) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("paras", paras_digest(s)),
        ("sel", sel_json(s)),
        ("csel", csel_json(s)),
        ("anc", anc_json(s)),
        ("ctx", ctx5(s)),
        ("no", no_text(s)),
        ("np", s.note_settings().json()),
        ("ref", sha(&ref_text(s))),
        ("nts", sha(&nts_text(s))),
        ("sect", sha(&sect_text(s))),
    ];
    if want_docxml {
        match saved(s) {
            Some(pkg) => {
                v.push(("docxml", part_digest(&pkg, "word/document.xml")));
                let parts: Vec<String> = [
                    "word/footnotes.xml",
                    "word/endnotes.xml",
                    "word/settings.xml",
                    "word/styles.xml",
                    "[Content_Types].xml",
                    "word/_rels/document.xml.rels",
                ]
                .iter()
                .map(|p| format!("{p}={}", part_digest(&pkg, p)))
                .collect();
                v.push(("parts", parts.join(",")));
            }
            None => {
                v.push(("docxml", "err".to_owned()));
                v.push(("parts", "err".to_owned()));
            }
        }
    }
    v
}

// ───────────────────────────────── F9 ─────────────────────────────────

/// `step` of `parity_d2b_f9_session.rs` for the codes that are not D2a's (one action each; `cm` is answered by the
/// facets, it is a direct call of the session that is not on the wire).
pub(super) fn f9_action(f: &[&str], s: &Session) -> Option<Option<Action>> {
    let q = || query(at(f, 1), &units(at(f, 2)), at(f, 3).parse().unwrap_or(0));
    let within = |x: &str| if x == "1" { Within::Selection } else { Within::Document };
    let fmt = |x: &str| format(x.parse().unwrap_or(0)).map(Box::new);
    let nav = Command::Navigation;
    let c = match *f.first()? {
        "fn" => Command::Find(FindCmd::FindNext {
            query: q(),
            direction: match at(f, 4) {
                "down" => Direction::Down,
                "up" => Direction::Up,
                _ => Direction::All,
            },
        }),
        "rm" => Command::Find(FindCmd::ReplaceMatch {
            query: q(),
            replacement: REPLS.get(nn(f, 4) as usize)?.encode_utf16().collect(),
            direction: Direction::All,
            replace_format: fmt(at(f, 5)),
        }),
        "ra" => Command::Find(FindCmd::ReplaceAll {
            query: q(),
            replacement: REPLS.get(nn(f, 4) as usize)?.encode_utf16().collect(),
            within: within(at(f, 5)),
            replace_format: fmt(at(f, 6)),
        }),
        "sm" => Command::Find(FindCmd::SelectAllMatches { query: q(), within: within(at(f, 4)) }),
        "cm" => return Some(None),
        "hm" => Command::Find(FindCmd::HighlightMatches { query: q(), within: Within::Document }),
        "sa" => nav(NavigationCmd::SelectAll),
        "sr" => {
            let ranges = at(f, 1)
                .split(',')
                .map(|r| {
                    let v: Vec<u32> = r.split('.').map(|x| x.parse().unwrap_or(0)).collect();
                    let g = |i: usize| v.get(i).copied().unwrap_or(0);
                    DocRange::new(DocPosition::new(g(0), g(1)), DocPosition::new(g(2), g(3)))
                })
                .collect();
            nav(NavigationCmd::SetSelectionRanges { ranges })
        }
        "tb" => Command::Format(FormatCmd::ToggleRunProp { key: RunToggle::Bold, button: "single".to_owned() }),
        "mh" => nav(NavigationCmd::MoveHeading { index: ii(f, 1), before: ii(f, 2) }),
        "ph" => nav(NavigationCmd::PromoteHeading { index: ii(f, 1) }),
        "dh" => nav(NavigationCmd::DemoteHeading { index: ii(f, 1) }),
        "ih" => nav(NavigationCmd::InsertHeading {
            index: ii(f, 1),
            at: match at(f, 3) {
                "before" => HeadingWhere::Before,
                "after" => HeadingWhere::After,
                _ => HeadingWhere::Sub,
            },
        }),
        "xh" => nav(NavigationCmd::DeleteHeading { index: ii(f, 1) }),
        "sh" => nav(NavigationCmd::SelectHeading { index: ii(f, 1) }),
        "gh" => nav(NavigationCmd::GoToHeading { index: ii(f, 1) }),
        "go" => Command::Find(FindCmd::GoTo { what: *GOTO.get(nn(f, 1) as usize)?, target: units(at(f, 2)) }),
        "ns" => {
            let o = at(f, 2).as_bytes();
            nav(NavigationCmd::Search {
                text: units(at(f, 1)),
                opts: FindOptions { match_case: o.first() == Some(&b'1'), whole_word: o.get(1) == Some(&b'1') },
            })
        }
        "nr" => nav(NavigationCmd::GoToSearchResult { index: ii(f, 1) }),
        "gb" => Command::Find(FindCmd::GoToBookmark { name: String::from_utf16(&units(at(f, 1))).ok()? }),
        "ec" => {
            // `EnterCell` with the table's `gridBefore` as the column and the caret at 0:0
            let anchor = nn(f, 1);
            let gb = s
                .document()
                .body
                .tables
                .get(anchor as usize)
                .and_then(|a| a.table.rows.first())
                .and_then(|r| r.tr_pr.as_deref())
                .and_then(|p| p.grid_before)
                .map_or(0, |g| uo_jsnum::conv::to_uint32(g.to_number()));
            Command::Context(ContextCmd::EnterCell {
                anchor,
                hops: vec![CellHop { row: 0, col: gb, block: None }],
                caret: Some(DocPosition::new(0, 0)),
            })
        }
        "xc" => Command::Context(ContextCmd::ExitCell),
        _ => return None,
    };
    Some(cmd(c))
}

fn err_hex(e: Option<&uo_docx::session::ops::find::engine::QueryError>) -> String {
    e.map(|e| hx(&e.message(true))).unwrap_or_default()
}

/// What the op returned, as the TS harness writes it.
fn res_of(s: &Session, code: &str) -> String {
    let op = code.split(':').next().unwrap_or("");
    let r = s.navigation_result();
    let b = |v: Option<&F9Result>| match v {
        Some(F9Result::Bool(x)) => x.to_string(),
        other => format!("{other:?}"),
    };
    match op {
        "fn" => match r {
            Some(F9Result::Find(o)) if !o.budget => format!("{}{}|{}", u8::from(o.found), u8::from(o.wrapped), err_hex(o.error.as_ref())),
            other => format!("{other:?}"),
        },
        "rm" => match r {
            Some(F9Result::Replace(o)) if !o.find.budget => {
                format!("{}{}{}|{}", u8::from(o.replaced), u8::from(o.find.found), u8::from(o.find.wrapped), err_hex(o.find.error.as_ref()))
            }
            other => format!("{other:?}"),
        },
        "ra" => match r {
            Some(F9Result::Replace(o)) if !o.find.budget => format!("{}|{}", o.count, err_hex(o.find.error.as_ref())),
            other => format!("{other:?}"),
        },
        "sm" | "hm" => match r {
            Some(F9Result::Count(n)) => n.to_string(),
            other => format!("{other:?}"),
        },
        "mh" | "ph" | "dh" | "ih" | "xh" | "go" | "nr" | "gb" => b(r),
        "ns" => {
            let v: Vec<String> = s
                .search_results()
                .iter()
                .map(|x| {
                    format!(
                        "{},{},{},{},{}|{}|{}",
                        x.start,
                        x.end,
                        x.heading,
                        u8::from(x.in_table),
                        hx(&x.snippet.before),
                        hx(&x.snippet.r#match),
                        hx(&x.snippet.after)
                    )
                })
                .collect();
            if v.is_empty() { "-".to_owned() } else { v.join(";") }
        }
        _ => "-".to_owned(),
    }
}

/// The selection is outside the edited story (a paragraph past the end, an offset past its length).
fn out_of_story(s: &Session) -> bool {
    let sel = s.selection();
    let Some(paras) = s.paras() else { return false };
    [sel.anchor, sel.focus]
        .iter()
        .any(|p| paras.get(p.para as usize).is_none_or(|x| p.offset as usize > uo_docx::session::text::para_length(x)))
}

fn ctx9(s: &Session) -> String {
    match s.context() {
        EditContext::Cell { anchor, hops, .. } => {
            let h: Vec<String> =
                hops.iter().map(|h| format!("{}.{}.{}", h.row, h.col, h.block.map_or(String::new(), |b| b.to_string()))).collect();
            format!("c{anchor}:{}", h.join("/"))
        }
        EditContext::Body => "b".to_owned(),
        _ => "o".to_owned(),
    }
}

fn ext_json(s: &Session) -> String {
    let v: Vec<String> =
        s.selection_ranges().iter().map(|r| format!("{{\"start\":{},\"end\":{}}}", pos_json(r.start), pos_json(r.end))).collect();
    format!("[{}]", v.join(","))
}

fn hd_text(s: &Session) -> String {
    s.headings().iter().map(|h| format!("{}|{}|{}", n(h.level), hx(h.text.units()), h.para)).collect::<Vec<_>>().join("\n")
}

fn facets_f9(s: &mut Session, code: &str, want_docxml: bool) -> Vec<(&'static str, String)> {
    let op = code.split(':').next().unwrap_or("");
    let res = if op == "cm" {
        let f: Vec<&str> = code.split(':').collect();
        let within = if at(&f, 4) == "1" { Within::Selection } else { Within::Document };
        match s.count_matches(&query(at(&f, 1), &units(at(&f, 2)), at(&f, 3).parse().unwrap_or(0)), within) {
            Ok(n) => n.to_string(),
            Err(e) => format!("{e:?}"),
        }
    } else {
        res_of(s, code)
    };
    let res = if res.len() > 200 { sha(&res) } else { res };
    let sel = s.selection();
    let mut v = vec![
        ("paras", paras_digest(s)),
        ("sel", sel_json(s)),
        ("cursel", sel_of(sel)),
        ("anc", anc_json(s)),
        ("res", res),
        ("ext", ext_json(s)),
        ("ctx", ctx9(s)),
        ("hd", sha(&hd_text(s))),
        // not a facet of the capture: the F9-A2 condition (a stale search result left the selection outside the story)
        ("_oos", u8::from(out_of_story(s)).to_string()),
    ];
    if want_docxml {
        v.push(("docxml", super::docxml(s)));
    }
    v
}

/// The family facets as `name=value` pairs separated by tabs (a value never holds a tab), or `None` for a family
/// without a facet table.
pub(super) fn facets(family: &str, h: usize, s: &mut Session, code: &str, want: u32) -> Option<String> {
    let want_docxml = want & 1 != 0;
    let pairs = match family {
        "f5" => facets_f5(s, want_docxml),
        "f9" => facets_f9(s, code, want_docxml),
        "f10" => super::f10::facets(s, code, want),
        "f8" => super::f8::facets(h, s, code, want_docxml),
        "f7" => super::f7::facets(h, s, code, want_docxml),
        _ => return None,
    };
    Some(pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\t"))
}
