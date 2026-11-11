//! The F8 (clipboard) replay of the wasm module: the step mapping and the facets, COPIED from
//! `crates/uo-docx/tests/parity_d2b_f8_session.rs` (declared: one copy of test code, nothing here is shipped; a change of the
//! native replay's tables, mapping or facets must be repeated here). Copied verbatim: `PATHS`, `f8_policy`, `part_of_path`,
//! `paras_digest`, `docxml_pair` and its masks, `clip_has_table_picture`, the paste-mode names and `kind`. Rewritten as
//! "action k of a code" (the mapping inside the test's loop): `build` and the `facets`. The shell's answers (the clipboard
//! read, the PNGs) and the job loop are the JS side's; the Office Clipboard is the module's one ring (`with_ring`), emptied
//! per group by the replay (`uo_session_clipboard_clear`); the session's random calls are counted by `env::draws`.
#![allow(dead_code, unused_imports, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::all)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use uo_docx::canon::CanonWriter;
use uo_docx::ids::HfPartId;
use uo_docx::js::JsValue;
use uo_docx::session::ops::clipboard::{
    CellsPasteMode, ClipboardCmd, ItemKind, ListPasteMode, PasteDefaults, PasteLayoutIn, PasteMode, PasteSpecialFormat, RichPick,
    clipboard_items,
};
use uo_docx::session::ops::context::{ContextCmd, saved_selection};
use uo_docx::session::ops::drawing::DrawingCmd;
use uo_docx::session::ops::header_footer::{HeaderFooterCmd, HfKind, HfSlot};
use uo_docx::session::ops::notes::NoteCmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::tables::{TableCmd, TableRect};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::{CharacterFormat, Flag, Indentation, ListKind};
use uo_docx::session::{CellHop, Command, DocPosition, FormattingPolicy, Session};
use uo_docx::text::Utf16;
use uo_testkit::{Digest, Stream};

use super::super::ring::with_ring;
use crate::support::Sink;

const PATHS: [&str; 3] = ["C:\\Docs\\fonte.docx", "/home/u/a b.docx", ""];

/// `F8_POLICY` of the capture (the second policy of the `fp` step).
fn f8_policy() -> FormattingPolicy {
    FormattingPolicy {
        paragraph_styles: Some(["Normal", "Heading1", "Heading 2", "Title"].into_iter().map(Into::into).collect()),
        character_styles: None,
        lists: Some((vec![ListKind::Bullet], Some(2.0))),
        character: Some(vec![CharacterFormat::Bold, CharacterFormat::Italic, CharacterFormat::Color]),
        indentation: Some(Indentation::None),
        denied: [Flag::Images, Flag::Shapes, Flag::Tables, Flag::Font, Flag::FontSize].into_iter().collect(),
    }
}

/// The part of a header/footer path (`hfParts` path) in the session's document.
fn part_of_path(s: &Session, path: &str) -> HfPartId {
    s.document().hf.iter().position(|h| &*h.path == path).map_or(HfPartId(u32::MAX), |i| HfPartId(u32::try_from(i).unwrap()))
}

fn sha(t: &str) -> String {
    Digest::of(t.as_bytes()).short()
}

fn paras_digest(s: &Session) -> String {
    let doc = s.document();
    let mut paras = doc.body.paras.clone();
    let mut ids = doc.ids.clone();
    uo_docx::session::normalize::normalize(&mut paras, &mut ids);
    let mut sink = Sink(Stream::new());
    for (i, p) in paras.iter().enumerate() {
        sink.0.tag(b'E').u32(u32::try_from(i).unwrap());
        let mut w = CanonWriter::new(&mut sink).with_media(&doc.media);
        p.canon(&mut w);
    }
    sink.0.finish().short()
}

fn docxml_pair(s: &mut Session, tag: &str) -> (String, String) {
    match s.save_docx() {
        Ok(bytes) => {
            let pkg = uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
            let t = pkg.part_text("word/document.xml").unwrap().map(|t| t.into_owned()).unwrap_or_default();
            if let Ok(dir) = std::env::var("UO_F8_XML") {
                std::fs::write(Path::new(&dir).join(format!("rust-{tag}.xml")), &t).unwrap();
            }
            (Digest::of(t.as_bytes()).short(), Digest::of(mask_img_rid(&mask_doc_pr(&t)).as_bytes()).short())
        }
        Err(_) => ("err".to_owned(), "err".to_owned()),
    }
}

/// `x.replace(/(<wp:docPr\b[^>]*?\bid=")\d+"/g, '$1#"')`.
fn mask_doc_pr(x: &str) -> String {
    let mut out = String::with_capacity(x.len());
    let mut rest = x;
    while let Some(i) = rest.find("<wp:docPr") {
        let after = &rest[i + 9..];
        if after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            out.push_str(&rest[..i + 9]);
            rest = after;
            continue;
        }
        let gt = after.find('>').unwrap_or(after.len());
        let tag = &after[..gt];
        let mut done = false;
        let mut k = 0;
        while let Some(p) = tag[k..].find("id=\"") {
            let q = k + p;
            let wb = q == 0 || !tag.as_bytes()[q - 1].is_ascii_alphanumeric() && tag.as_bytes()[q - 1] != b'_';
            let v = q + 4;
            let digits = tag[v..].bytes().take_while(u8::is_ascii_digit).count();
            if wb && digits > 0 && tag.as_bytes().get(v + digits) == Some(&b'"') {
                out.push_str(&rest[..i + 9]);
                out.push_str(&tag[..v]);
                out.push('#');
                rest = &after[v + digits..];
                done = true;
                break;
            }
            k = q + 1;
        }
        if !done {
            out.push_str(&rest[..i + 9]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `.replace(/rIdUwImg\d+/g, 'rIdUwImg#')`.
fn mask_img_rid(x: &str) -> String {
    let mut out = String::with_capacity(x.len());
    let mut rest = x;
    while let Some(i) = rest.find("rIdUwImg") {
        out.push_str(&rest[..i + 8]);
        let after = &rest[i + 8..];
        let d = after.bytes().take_while(u8::is_ascii_digit).count();
        if d > 0 {
            out.push('#');
        }
        rest = &after[d..];
    }
    out.push_str(rest);
    out
}

/// A clip with a table (between paragraphs or copied cells) that holds a picture.
fn clip_has_table_picture(c: &uo_docx::session::Clip) -> bool {
    fn story(s: &uo_docx::model::Story) -> bool {
        s.paras.iter().any(|p| p.runs.iter().any(|r| r.ext().is_some_and(|e| e.image.is_some())))
            || s.tables.iter().any(|a| table(&a.table))
    }
    fn table(t: &uo_docx::model::table::Table) -> bool {
        t.rows.iter().any(|r| r.cells.iter().any(|c| story(&c.content)))
    }
    c.tables.iter().any(|t| table(&t.table)) || c.cells.as_ref().is_some_and(|x| table(&x.table))
}

fn mode(x: &str) -> Option<PasteMode> {
    match x {
        "keep" => Some(PasteMode::Keep),
        "merge" => Some(PasteMode::Merge),
        "text" => Some(PasteMode::Text),
        "picture" => Some(PasteMode::Picture),
        "default" => Some(PasteMode::Default),
        _ => None,
    }
}

fn list(x: &str) -> Option<ListPasteMode> {
    match x {
        "continue" => Some(ListPasteMode::Continue),
        "new" => Some(ListPasteMode::New),
        "merge" => Some(ListPasteMode::Merge),
        "nomerge" => Some(ListPasteMode::NoMerge),
        _ => None,
    }
}

fn mode_name(m: PasteMode) -> &'static str {
    match m {
        PasteMode::Keep => "keep",
        PasteMode::Merge => "merge",
        PasteMode::Text => "text",
        PasteMode::Picture => "picture",
        PasteMode::Default => "default",
    }
}

fn list_name(m: ListPasteMode) -> &'static str {
    match m {
        ListPasteMode::Continue => "continue",
        ListPasteMode::New => "new",
        ListPasteMode::Merge => "merge",
        ListPasteMode::NoMerge => "nomerge",
    }
}

fn kind(k: ItemKind) -> &'static str {
    match k {
        ItemKind::Text => "text",
        ItemKind::Table => "table",
        ItemKind::Drawing => "drawing",
        ItemKind::Image => "image",
    }
}

// ── the wasm wrappers (rewritten from the test's loop, not copied) ──

use super::{Action, anc_json, cmd};

/// What a step does, in order (`EnterHf` is resolved by path when it is reached: the part exists once the creation ran).
pub(super) enum Step {
    Cmd(Command),
    Undo,
    Redo,
    EnterHf(String),
}

/// What the element carries from one document of a group to the next (`pasteDefaults`, `documentPath`, `formattingPolicy`).
#[derive(Default)]
struct Group {
    defaults: PasteDefaults,
    path: Option<String>,
    policy: Option<FormattingPolicy>,
}

/// The state of a handle's current step.
#[derive(Default)]
struct Hs {
    text: Utf16,
    tree: Option<Arc<[u8]>>,
    script: Vec<Step>,
    copied: bool,
    ext: bool,
}

thread_local! {
    static GROUP: RefCell<Group> = RefCell::new(Group::default());
    static HS: RefCell<HashMap<usize, Hs>> = RefCell::new(HashMap::new());
}

pub(super) fn forget(h: usize) {
    HS.with(|m| m.borrow_mut().remove(&h));
}

/// A new group of documents: the carried state and the Office Clipboard start over.
pub(super) fn new_group() {
    GROUP.with(|g| *g.borrow_mut() = Group::default());
    // a fresh ring, as the native replay's new `Cx`: the items AND the current rich copy
    with_ring(|r| {
        r.clear();
        r.set_current(None);
    });
}

/// A document of the group: the carried state goes into the session AS EXEC ACTIONS of the pseudo-code `@setup` (the element's
/// properties: `pasteDefaults`, `documentPath`, `formattingPolicy`, and the page's plugin when the group is an extended one), so
/// that the four commands pass the wire codec and `exec` like every other action of the replay.
pub(super) fn setup(h: &mut super::UoWriteSession, ext: bool) {
    let key = std::ptr::from_mut(h) as usize;
    HS.with(|m| m.borrow_mut().insert(key, Hs { ext, ..Hs::default() }));
}

fn setup_script(hs: &Hs) -> Vec<Step> {
    let (defaults, path, policy) = GROUP.with(|g| {
        let g = g.borrow();
        (g.defaults, g.path.clone(), g.policy.clone())
    });
    let mut v = vec![
        Step::Cmd(Command::Clipboard(ClipboardCmd::SetPasteDefaults { defaults })),
        Step::Cmd(Command::Clipboard(ClipboardCmd::SetDocumentPath { path: path.map(Into::into) })),
    ];
    if hs.ext {
        v.push(Step::Cmd(Command::Clipboard(ClipboardCmd::SetInlineTypes { types: vec!["f8.tag".into()] })));
    }
    if policy.is_some() {
        v.push(Step::Cmd(Command::Settings(SettingsCmd::SetPolicy { policy })));
    }
    v
}

/// The shell's inputs of a step (`cbt`, `pt`); the draws counter starts at 0.
pub(super) fn begin_step(h: usize, text: Utf16, tree: Option<Arc<[u8]>>) {
    crate::session::env::reset_draws();
    HS.with(|m| {
        let mut m = m.borrow_mut();
        let hs = m.entry(h).or_default();
        hs.text = text;
        hs.tree = tree;
        hs.script.clear();
        hs.copied = false;
    });
}

fn nn(f: &[&str], i: usize) -> u32 {
    f.get(i).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn at<'a>(f: &[&'a str], i: usize) -> &'a str {
    f.get(i).copied().unwrap_or("")
}

/// The steps of `full` (`<op>` or `<op> TAB <hf>`: the step record's `hf`), from the session's state before the step.
fn build(s: &Session, full: &str, hs: &mut Hs, group: &mut Group) -> Option<Vec<Step>> {
    let lay = PasteLayoutIn::default();
    let (code, hf) = full.split_once('\t').map_or((full, ""), |(a, b)| (a, b));
    let (clear_fs, body) = match code.strip_prefix("xf+") {
        Some(b) => (true, b),
        None => (false, code),
    };
    let f: Vec<&str> = body.split(':').collect();
    let mut more: Vec<Step> = Vec::new();
    let mut first: Vec<Step> = Vec::new();
    let mut undo_redo = None;
    let c = Step::Cmd;
    let cmd0: Option<Step> = match at(&f, 0) {
        "ty" => Some(c(Command::Text(TextCmd::Type { text: Utf16::from_units(super::units(at(&f, 1))) }))),
        "en" => Some(c(Command::Text(TextCmd::Enter))),
        "bs" => Some(c(Command::Text(TextCmd::Backspace))),
        "se" => Some(c(Command::Text(TextCmd::SetSelection {
            anchor: DocPosition::new(nn(&f, 1), nn(&f, 2)),
            focus: DocPosition::new(nn(&f, 3), nn(&f, 4)),
        }))),
        "tk" => Some(c(Command::Settings(SettingsCmd::SetTracking { on: at(&f, 1) == "1" }))),
        "co" => {
            hs.copied = true;
            Some(c(Command::Clipboard(ClipboardCmd::Copy)))
        }
        "cu" => {
            hs.copied = true;
            Some(c(Command::Clipboard(ClipboardCmd::Cut)))
        }
        "pr" => {
            let text = with_ring(|r| r.current().map(|c| c.text.clone()).unwrap_or_default());
            Some(c(Command::Clipboard(ClipboardCmd::PasteData {
                text,
                html: None,
                image: None,
                rich: RichPick::WithRich,
                mode: mode(at(&f, 1)),
                list: list(at(&f, 2)),
                layout: lay,
            })))
        }
        "ph" => Some(c(Command::Clipboard(ClipboardCmd::PasteData {
            text: hs.text.clone(),
            html: hs.tree.clone(),
            image: None,
            rich: RichPick::None,
            mode: mode(at(&f, 1)),
            list: None,
            layout: lay,
        }))),
        "pt" => Some(c(Command::Clipboard(ClipboardCmd::PasteData {
            text: Utf16::from_units(super::units(at(&f, 2))),
            html: None,
            image: None,
            rich: RichPick::None,
            mode: mode(at(&f, 1)),
            list: None,
            layout: lay,
        }))),
        "px" => Some(c(Command::Clipboard(ClipboardCmd::PasteText { text: Utf16::from_units(super::units(at(&f, 1))) }))),
        "pv" => Some(c(Command::Clipboard(ClipboardCmd::Paste { mode: mode(at(&f, 1)), layout: lay }))),
        "rp" => Some(c(Command::Clipboard(ClipboardCmd::Repaste { mode: mode(at(&f, 1))?, list: list(at(&f, 2)), layout: lay }))),
        "ts" => {
            let (r1, c1, r2, c2) = (i64::from(nn(&f, 2)), i64::from(nn(&f, 3)), i64::from(nn(&f, 4)), i64::from(nn(&f, 5)));
            Some(c(Command::Table(TableCmd::SetTableSelection {
                anchor: nn(&f, 1),
                path: Vec::new(),
                rect: TableRect { r1, c1, r2, c2 },
                origin: (r1, c1),
            })))
        }
        "pc" => Some(c(Command::Clipboard(ClipboardCmd::PasteCells {
            mode: match at(&f, 1) {
                "rows" => CellsPasteMode::Rows,
                "nest" => CellsPasteMode::Nest,
                "text" => CellsPasteMode::Text,
                _ => CellsPasteMode::Overwrite,
            },
            layout: lay,
        }))),
        "sf" => Some(c(Command::Clipboard(ClipboardCmd::PasteSpecialFormats))),
        "ps" => Some(c(Command::Clipboard(ClipboardCmd::PasteSpecial {
            format: match at(&f, 1) {
                "rtf" => PasteSpecialFormat::Rtf,
                "text" => PasteSpecialFormat::Text,
                "picture" => PasteSpecialFormat::Picture,
                "html" => PasteSpecialFormat::Html,
                "unicode" => PasteSpecialFormat::Unicode,
                _ => PasteSpecialFormat::Image,
            },
            link: at(&f, 2) == "1",
            layout: lay,
        }))),
        "pi" => {
            let id = with_ring(|r| clipboard_items(r).get(nn(&f, 1) as usize).map(|x| x.id))?;
            Some(c(Command::Clipboard(ClipboardCmd::PasteClipboardItem { id, mode: mode(at(&f, 2)), layout: lay })))
        }
        "pa" => Some(c(Command::Clipboard(ClipboardCmd::PasteAllClipboardItems { layout: lay }))),
        "rm" => {
            let id = with_ring(|r| clipboard_items(r).get(nn(&f, 1) as usize).map(|x| x.id))?;
            Some(c(Command::Clipboard(ClipboardCmd::RemoveClipboardItem { id })))
        }
        "cl" => Some(c(Command::Clipboard(ClipboardCmd::ClearClipboardItems))),
        "pd" => {
            group.defaults = PasteDefaults { within: mode(at(&f, 1))?, between: mode(at(&f, 2))?, other: mode(at(&f, 3))? };
            Some(c(Command::Clipboard(ClipboardCmd::SetPasteDefaults { defaults: group.defaults })))
        }
        "dp" => {
            let p = *PATHS.get(nn(&f, 1) as usize)?;
            group.path = if p.is_empty() { None } else { Some(p.to_owned()) };
            Some(c(Command::Clipboard(ClipboardCmd::SetDocumentPath { path: group.path.clone().map(Into::into) })))
        }
        "ec" => {
            let anchor = nn(&f, 1);
            let gb = s
                .document()
                .body
                .tables
                .get(anchor as usize)
                .and_then(|a| a.table.rows.first())
                .and_then(|r| r.tr_pr.as_deref())
                .and_then(|p| p.grid_before)
                .map_or(0, |g| uo_jsnum::conv::to_uint32(g.to_number()));
            Some(c(Command::Context(ContextCmd::EnterCell {
                anchor,
                hops: vec![CellHop { row: 0, col: gb, block: None }],
                caret: Some(DocPosition::new(0, 0)),
            })))
        }
        "xc" => Some(c(Command::Context(ContextCmd::ExitCell))),
        // the extended groups
        "fp" => {
            group.policy = match nn(&f, 1) {
                0 => None,
                1 => FormattingPolicy::preset("template-document"),
                _ => Some(f8_policy()),
            };
            Some(c(Command::Settings(SettingsCmd::SetPolicy { policy: group.policy.clone() })))
        }
        "so" | "sm" => {
            let a = f.last().copied().unwrap_or("n");
            if a != "n" {
                hs.copied = true;
                more.push(c(Command::Clipboard(if a == "c" { ClipboardCmd::Copy } else { ClipboardCmd::Cut })));
            }
            Some(c(if at(&f, 0) == "so" {
                Command::Drawing(DrawingCmd::SelectObject { index: Some(nn(&f, 1)) })
            } else {
                Command::Text(TextCmd::SetSelection {
                    anchor: DocPosition::new(nn(&f, 1), nn(&f, 2)),
                    focus: DocPosition::new(nn(&f, 3), nn(&f, 4)),
                })
            }))
        }
        "on" => Some(c(Command::Note(NoteCmd::Open { footnote: at(&f, 1) == "0", id: uo_jsnum::parse::to_number(at(&f, 2)) }))),
        "xn" => Some(c(Command::Context(ContextCmd::ExitNote))),
        "xh" => Some(c(Command::Context(ContextCmd::ExitHeaderFooter))),
        "eh" => {
            let (p, cr) = hf.split_once('|')?;
            if cr != "-" {
                let g: Vec<&str> = cr.split(':').collect();
                let slot = match at(&g, 2) {
                    "first" => HfSlot::First,
                    "even" => HfSlot::Even,
                    _ => HfSlot::Default,
                };
                let kind = if at(&g, 0) == "header" { HfKind::Header } else { HfKind::Footer };
                first.push(c(Command::HeaderFooter(HeaderFooterCmd::CreateHeaderFooterPart {
                    kind,
                    section: at(&g, 1).parse().unwrap_or(0),
                    slot,
                })));
            }
            let denied = group.policy.as_ref().is_some_and(|x| x.denied.contains(&Flag::HeadersFooters));
            if p != "-" {
                // the part exists once the creation ran: resolved by path after it
                more.push(Step::EnterHf(p.to_owned()));
            } else if denied {
                // the guard refuses (`uw-blocked`)
                more.push(c(Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(0), page: 0 })));
            }
            None
        }
        "un" => {
            undo_redo = Some(Step::Undo);
            None
        }
        "re" => {
            undo_redo = Some(Step::Redo);
            None
        }
        _ => return None,
    };
    let mut out = Vec::new();
    // the native loop: the undo/redo first, then `pre` (the float selection dropped), the command, the rest
    out.extend(undo_redo);
    if clear_fs {
        out.push(c(Command::Drawing(DrawingCmd::SelectObject { index: None })));
    }
    out.extend(cmd0);
    out.extend(first);
    out.extend(more);
    Some(out)
}

/// Action `k` of an F8 `code`; `None` when the code has no more.
pub(super) fn action(h: usize, s: &Session, full: &str, k: u32) -> Option<Action> {
    HS.with(|m| {
        let mut m = m.borrow_mut();
        let hs = m.entry(h).or_default();
        if k == 0 {
            let script = if full == "@setup" {
                Some(setup_script(hs))
            } else {
                GROUP.with(|g| {
                    let mut g = g.borrow_mut();
                    build(s, full, hs, &mut g)
                })
            };
            hs.script = script.unwrap_or_default();
        }
        let step = hs.script.get_mut(k as usize)?;
        match step {
            Step::Undo => Some(Action::Exec(super::w::ExecOp::Undo)),
            Step::Redo => Some(Action::Exec(super::w::ExecOp::Redo)),
            Step::EnterHf(p) => cmd(Command::Context(ContextCmd::EnterHeaderFooter { part: part_of_path(s, p), page: 0 })),
            Step::Cmd(c) => cmd(std::mem::replace(c, Command::Text(TextCmd::Enter))),
        }
    })
}

/// `po`, `psf`, `cbw`, `clip`, `ring`, `nv`, `sv`, `rc`, `docxml` and the masked one, the paste/taint conditions.
pub(super) fn facets(h: usize, s: &mut Session, full: &str, want_docxml: bool) -> Vec<(&'static str, String)> {
    let code = full.split('\t').next().unwrap_or("");
    let body = code.strip_prefix("xf+").unwrap_or(code);
    let copied = HS.with(|m| m.borrow().get(&h).is_some_and(|x| x.copied));
    let sel = s.selection();
    let body_sel = saved_selection(s.context()).unwrap_or(sel);
    let cbw = match (copied, s.copied()) {
        (true, Some(c)) => {
            let t = hx_units(c.text.units());
            let t = if t.len() > 200 { sha(&t) } else { t };
            format!("{t}|{}", c.html.as_deref().map_or_else(|| "-".to_owned(), sha))
        }
        _ => "-".to_owned(),
    };
    let (clip, ring, taint_cur, taint_any) = with_ring(|ring| {
        let clip = match ring.current() {
            Some(c) => {
                let kind = if c.drawing.is_some() {
                    "d".to_owned()
                } else if let Some(cc) = &c.cells {
                    format!("c{}{}", u8::from(cc.rows), u8::from(cc.whole))
                } else {
                    "p".to_owned()
                };
                let paras = c
                    .paras
                    .iter()
                    .map(|p| {
                        format!(
                            "{}{}{}",
                            p.runs.len(),
                            if p.mark.is_some() { "m" } else { "" },
                            p.marker.as_ref().map_or(String::new(), |m| format!("[{}]", hx_units(m.units())))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let mut lists: Vec<f64> = c.lists.iter().map(|(k, _)| *k).collect();
                lists.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let lists = lists.iter().map(|k| uo_jsnum::format::to_js_string(*k)).collect::<Vec<_>>().join(",");
                let t = format!(
                    "{}|{}|{kind}|{paras}|{lists}|{}|{}|{}",
                    hx_units(c.text.units()),
                    c.paras.len(),
                    c.tables.len(),
                    c.path.as_deref().unwrap_or(""),
                    u8::from(c.src.is_some())
                );
                if t.len() > 300 { sha(&t) } else { t }
            }
            None => "-".to_owned(),
        };
        let ring_t =
            clipboard_items(ring).iter().map(|x| format!("{}:{}", hx_units(x.text.units()), kind(x.kind))).collect::<Vec<_>>().join(" ");
        let cur = ring.current().is_some_and(|c| clip_has_table_picture(c));
        let any = ring.items().any(|x| clip_has_table_picture(&x.clip));
        (clip, ring_t, cur, any)
    });
    let po = s.paste_options();
    let po = format!(
        "{}|{}|{}|{}",
        po.modes.iter().map(|m| mode_name(*m)).collect::<Vec<_>>().join(","),
        po.list.iter().map(|m| list_name(*m)).collect::<Vec<_>>().join(","),
        po.current.map_or("", |c| mode_name(c.0)),
        po.current.and_then(|c| c.1).map_or("", list_name)
    );
    let psf = if body.starts_with("sf:") {
        let f = s.paste_special_formats().cloned().unwrap_or_default();
        let names: Vec<&str> = f
            .formats
            .iter()
            .map(|x| match x {
                PasteSpecialFormat::Rtf => "rtf",
                PasteSpecialFormat::Text => "text",
                PasteSpecialFormat::Picture => "picture",
                PasteSpecialFormat::Html => "html",
                PasteSpecialFormat::Unicode => "unicode",
                PasteSpecialFormat::Image => "image",
            })
            .collect();
        format!("{}|{}|{}", names.join(","), u8::from(f.link), f.source)
    } else {
        "-".to_owned()
    };
    let paste_step = ["pr", "pi", "pa", "pv", "ps", "pc", "rp"].iter().any(|p| body.starts_with(p));
    let paste_taint = paste_step && (taint_cur || taint_any);
    let mut v = vec![
        ("paras", paras_digest(s)),
        ("sel", format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(body_sel.anchor), pos_json(body_sel.focus))),
        ("anc", anc_json(s)),
        ("cbw", cbw),
        ("clip", clip),
        ("ring", sha(if ring.is_empty() { "-" } else { &ring })),
        ("po", po),
        ("psf", psf),
        ("nv", sha(s.numbering_store().xml().map_or("", |x| &**x))),
        ("sv", sha(s.styles_store().xml().map_or("", |x| &**x))),
        ("rc", crate::session::env::draws().to_string()),
        ("_paste_taint", u8::from(paste_taint).to_string()),
        ("_cur_taint", u8::from(taint_cur).to_string()),
    ];
    if want_docxml {
        let (d, masked) = docxml_pair(s, "wasm");
        v.push(("docxml", d));
        v.push(("docxmlm", masked));
    }
    v
}

fn pos_json(p: DocPosition) -> String {
    format!("{{\"para\":{},\"offset\":{}}}", p.para, p.offset)
}

pub(super) fn hx_units(u: &[u16]) -> String {
    u.iter().map(|c| format!("{c:x}")).collect::<Vec<_>>().join(".")
}
