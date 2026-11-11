//! Test-only exports of the session (feature `session-selftest`, never shipped): the capture replay of
//! the D2a/D2b families through the module's exports (tests/session_replay.test.mjs, verification (a)
//! of the record), the facets the JS side cannot compute from the wire, and random messages for the
//! hostile test.
//!
//! The op codes of each family's capture become the actions the native replay runs
//! (`tests/parity_d2a_session.rs`, `d2b_f1_support::step`, `parity_d2b_f2_session.rs`,
//! `parity_d2b_f6_session.rs`): the mappings are copied here (declared: one copy of test code each),
//! rewritten as "action k of a code" so that every action goes through the exports from JS: a code like
//! F1 `pt` is four actions (selection, the `CopyFormat` query, selection, `ApplyPainter`). The facets are
//! one copy of the native replays' `paras_digest`, `anc_json` and `docxml` (declared).

use std::cell::RefCell;
use std::collections::HashMap;

use uo_docx::canon::CanonWriter;
use uo_docx::js::JsValue;
use uo_docx::session::ops::bookmarks::{BookmarkCmd, HyperlinkOptions};
use uo_docx::session::ops::comments::CommentCmd;
use uo_docx::session::ops::context::{EditContext, saved_selection};
use uo_docx::session::ops::format::{FormatCmd, VertKind};
use uo_docx::session::ops::lists::{ListCmd, ListLevelFormat, ListLevelSpec};
use uo_docx::session::ops::paragraph::{ParagraphCmd, SpecialIndent};
use uo_docx::session::ops::revision_cmds::{Balloons, MarkupPatch, RevisionCmd};
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::styles::{Ch, CreateStyleSpec, DefaultFont, ModifyStyleSpec, StyleCmd, StyleFormat};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{Command, DocPosition, MarkupMode, Session};
use uo_docx::text::Utf16;
use uo_jsnum::parse::to_number;
use uo_testkit::{Digest, Stream};

use super::codec as w;
use super::handle::UoWriteSession;
use super::wire::IntoWire;
use crate::d2b_f1_support as f1;
use crate::render::{NONE, NULL_HANDLE, OK, UoBuf, buf, take_bytes};
use crate::support::Sink;

mod f10;
mod f7;
mod f8;
mod fam;

thread_local! {
    /// the shell state of the F1 harness per handle (`ulButton`). The documents themselves open through the
    /// production `uo_session_open` (the replay passes the native environment as a `SessionOpen` message)
    static SHELL: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
    /// the commands of the F3 op code in progress per handle: the native replay computes them ONCE from the
    /// state before the step (`d2b_f3_support::session_codes::commands`) and runs them in order
    static F3: RefCell<HashMap<usize, Vec<Command>>> = RefCell::new(HashMap::new());
}

/// What action k of a code is (one value at a time: a box would only add noise).
#[allow(clippy::large_enum_variant)]
enum Action {
    Exec(w::ExecOp),
    Query(w::QueryOp),
}

fn cmd(c: Command) -> Option<Action> {
    c.into_wire().ok().map(|x| Action::Exec(w::ExecOp::Command(x)))
}

fn units(hex: &str) -> Vec<u16> {
    hex.split('.').filter(|h| !h.is_empty()).filter_map(|h| u16::from_str_radix(h, 16).ok()).collect()
}

fn num(s: Option<&&str>) -> u32 {
    s.and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn idx(s: Option<&&str>) -> usize {
    s.and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn pos(f: &[&str], k: usize) -> DocPosition {
    DocPosition::new(num(f.get(k)), num(f.get(k + 1)))
}

/// The D2a op codes (every family's capture uses them).
fn d2a(f: &[&str]) -> Option<Option<Action>> {
    Some(match *f.first()? {
        "ty" => cmd(Command::Text(TextCmd::Type { text: Utf16::from_units(units(f.get(1)?)) })),
        "en" => cmd(Command::Text(TextCmd::Enter)),
        "bs" => cmd(Command::Text(TextCmd::Backspace)),
        "de" => cmd(Command::Text(TextCmd::Delete)),
        "se" => cmd(Command::Text(TextCmd::SetSelection { anchor: pos(f, 1), focus: pos(f, 3) })),
        "tk" => cmd(Command::Settings(SettingsCmd::SetTracking { on: f.get(1) == Some(&"1") })),
        "un" => Some(Action::Exec(w::ExecOp::Undo)),
        "re" => Some(Action::Exec(w::ExecOp::Redo)),
        _ => return None,
    })
}

fn num_or_none(s: Option<&&str>) -> Option<f64> {
    match s {
        Some(&"-") | None => None,
        Some(v) => Some(to_number(v)),
    }
}

/// `d2b_f1_support::step` as actions (`sh`: the shell's `ulButton`, updated before a format command
/// as the harness does).
fn f1(f: &[&str], k: u32, h: &UoWriteSession, sh: &mut String) -> Option<Action> {
    let s = &h.session;
    let fmt = |c: FormatCmd, sh: &mut String| {
        *sh = c.underline_button_after(sh);
        cmd(Command::Format(c))
    };
    let para = |c: ParagraphCmd| cmd(Command::Paragraph(c));
    let code = *f.first()?;
    // the multi-action codes
    match code {
        "pt" => {
            return match k {
                0 => cmd(Command::Text(TextCmd::SetSelection { anchor: pos(f, 1), focus: pos(f, 1) })),
                1 => Some(Action::Query(w::QueryOp::CopyFormat)),
                2 => cmd(Command::Text(TextCmd::SetSelection { anchor: pos(f, 3), focus: pos(f, 5) })),
                3 => match &h.painter {
                    Some(p) => {
                        *sh = FormatCmd::ApplyPainter { painter: Box::new(p.clone()) }.underline_button_after(sh);
                        Some(Action::Exec(w::ExecOp::ApplyPainter))
                    }
                    None => cmd(Command::Text(TextCmd::SetSelection { anchor: pos(f, 3), focus: pos(f, 5) })),
                },
                _ => None,
            };
        }
        "ss" => {
            return match k {
                0 => fmt(FormatCmd::SelectSimilarFormatting, sh),
                1 => {
                    let sel = s.selection();
                    cmd(Command::Text(TextCmd::SetSelection { anchor: sel.anchor, focus: sel.focus }))
                }
                _ => None,
            };
        }
        _ => {}
    }
    if k > 0 {
        return None;
    }
    if let Some(a) = d2a(f) {
        return a;
    }
    let i = |n: usize| idx(f.get(n));
    match code {
        "tg" => fmt(FormatCmd::ToggleRunProp { key: *f1::TOGGLES.get(i(1))?, button: sh.clone() }, sh),
        "tu" => {
            let style = if f.get(1) == Some(&"-") { None } else { Some((*f.get(1)?).to_owned()) };
            fmt(FormatCmd::ToggleUnderline { style, button: sh.clone() }, sh)
        }
        "su" => fmt(FormatCmd::SetUnderline { style: (*f1::UL.get(i(1))?).to_owned() }, sh),
        "uc" => fmt(FormatCmd::SetUnderlineColor { color: f1::color(i(1)), button: sh.clone() }, sh),
        "fx" => fmt(FormatCmd::SetTextEffects { patch: f1::fx(i(1)) }, sh),
        "fp" => {
            let v: f64 = to_number(f.get(1)?);
            fmt(FormatCmd::ApplyTextEffectPreset { index: if v < 0.0 { None } else { Some(v) } }, sh)
        }
        "ot" => fmt(FormatCmd::SetOpenType { patch: f1::ot(i(1)) }, sh),
        "tgr" => fmt(FormatCmd::SetTextGradient { v: f1::grad(i(1)) }, sh),
        "ff" => fmt(FormatCmd::SetFontFormat { f: Box::new(f1::ff(i(1))) }, sh),
        "va" => fmt(FormatCmd::ToggleVertAlign { kind: if i(1) == 1 { VertKind::Subscript } else { VertKind::Superscript } }, sh),
        "fs" => fmt(FormatCmd::SetFontSize { pt: *f1::SIZES.get(i(1))? }, sh),
        "gf" => fmt(FormatCmd::GrowFont { delta: to_number(f.get(1)?) }, sh),
        "fam" => fmt(FormatCmd::SetFontFamily { name: (*f1::FAMILIES.get(i(1))?).to_owned() }, sh),
        "co" => fmt(FormatCmd::SetColor { color: f1::color(i(1)) }, sh),
        "hl" => {
            let h = *f1::HL.get(i(1))?;
            fmt(FormatCmd::SetHighlight { name: if h == "-" { None } else { Some(h.to_owned()) } }, sh)
        }
        "cf" => fmt(FormatCmd::ClearFormatting, sh),
        "cc" => fmt(FormatCmd::ChangeCase { mode: *f1::CASES.get(i(1))? }, sh),
        "cs" => fmt(FormatCmd::SetCharSpacing { twips: num_or_none(f.get(1)) }, sh),
        "al" => para(ParagraphCmd::SetAlignment { alignment: (*f1::ALIGN.get(i(1))?).to_owned() }),
        "in" => para(ParagraphCmd::Indent { delta: if f.get(1) == Some(&"1") { 1 } else { -1 } }),
        "ls" => para(ParagraphCmd::SetLineSpacing { line: to_number(f.get(1)?) }),
        "sa" => para(ParagraphCmd::SetSpaceAfter { twips: num_or_none(f.get(1)) }),
        "sb" => para(ParagraphCmd::SetSpaceBefore { twips: num_or_none(f.get(1)) }),
        "sh" => para(ParagraphCmd::SetShading { hex: if f.get(1) == Some(&"-") { None } else { Some((*f.get(1)?).to_owned()) } }),
        "si" => para(ParagraphCmd::SetSpecialIndent {
            kind: *[SpecialIndent::None, SpecialIndent::FirstLine, SpecialIndent::Hanging].get(i(1))?,
        }),
        "ir" => para(ParagraphCmd::SetIndentRight { twips: num_or_none(f.get(1)) }),
        "pi" => para(ParagraphCmd::SetParagraphIndents { ind: f1::indents(i(1)) }),
        "ts" => {
            let (tabs, only) = f1::tabs(i(1));
            para(ParagraphCmd::SetTabStops { tabs, only })
        }
        "pf" => para(ParagraphCmd::SetParagraphFlow { f: f1::flow(i(1)) }),
        "pl" => para(ParagraphCmd::SetParagraphLayout { v: f1::layout(i(1)) }),
        "pb" => para(ParagraphCmd::SetParagraphBorders { sides: f1::borders(i(1)) }),
        "bp" => para(ParagraphCmd::ParagraphBorderPreset { kind: *f1::PRESETS.get(i(1))? }),
        "tb" => para(ParagraphCmd::ToggleBorder),
        "hr" => para(ParagraphCmd::InsertHorizontalLine),
        "cp" => para(ParagraphCmd::ClearParagraphFormatting),
        _ => None,
    }
}

const F2_NAMES: [&str; 4] = ["Meu Estilo", "Destaque X", "Citação", "Nota 1"];

fn lvl(i: u32, num_fmt: &str, text: Option<&str>) -> ListLevelSpec {
    ListLevelSpec {
        ilvl: f64::from(i),
        num_fmt: num_fmt.to_owned(),
        lvl_text: text.map_or_else(|| format!("%{}.", i + 1), str::to_owned),
        ind_left: 720.0 * f64::from(i + 1),
        hanging: 360.0,
        ..ListLevelSpec::default()
    }
}

/// `parity_d2b_f2_session.rs` `command(code)` (the code is split in at most 5 fields there).
fn f2(code: &str) -> Option<Action> {
    let f: Vec<&str> = code.splitn(5, ':').collect();
    if let Some(a) = d2a(&f) {
        return a;
    }
    let n = |i: usize| num(f.get(i));
    let ix = |i: usize| (f.get(i) != Some(&"-")).then(|| idx(f.get(i)));
    let name = |i: usize| F2_NAMES.get(n(i) as usize).map(|s| (*s).to_owned());
    let field = |i: usize| f.get(i).map(|s| (*s).to_owned());
    cmd(match *f.first()? {
        "tl" => Command::List(ListCmd::ToggleList { bullet: f.get(1) == Some(&"b") }),
        "al" => Command::List(ListCmd::ApplyListLibrary { bullet: f.get(1) == Some(&"b"), index: ix(2) }),
        "rl" => Command::List(ListCmd::RemoveList),
        "ml" => Command::List(ListCmd::ApplyMultilevelLibrary { index: ix(1) }),
        "sl" => Command::List(ListCmd::SetListLevel { ilvl: f64::from(n(1)) }),
        "ci" => Command::List(ListCmd::ChangeListLevel { delta: f.get(1)?.parse::<i8>().ok()? }),
        "rs" => Command::List(ListCmd::RestartNumbering { start: f.get(1).map(|v| uo_jsnum::parse::parse_int(v, 10)) }),
        "cn" => Command::List(ListCmd::ContinueNumbering),
        "nv" => Command::List(ListCmd::SetNumberingValue {
            new_list: f.get(1) == Some(&"n"),
            value: (f.get(2) != Some(&"-")).then(|| uo_jsnum::parse::parse_int(f.get(2).copied().unwrap_or(""), 10)),
        }),
        "dl" => Command::List(ListCmd::DefineListLevel {
            format: Box::new(if f.get(1) == Some(&"b") {
                ListLevelFormat {
                    bullet: Some(char::from_u32(u32::from_str_radix(f.get(2)?, 16).ok()?)?.to_string()),
                    font: Some("Arial".to_owned()),
                    ..ListLevelFormat::default()
                }
            } else {
                ListLevelFormat { num_fmt: field(2), lvl_text: field(3), bold: true, ..ListLevelFormat::default() }
            }),
        }),
        "as" => Command::Style(StyleCmd::ApplyStyle { style_id: (f.get(1) != Some(&"-")).then(|| field(1)).flatten() }),
        "cs" => Command::Style(StyleCmd::CreateStyle {
            spec: Box::new(CreateStyleSpec { name: name(1)?, kind: field(2), ..CreateStyleSpec::default() }),
        }),
        "ms" => {
            let spec = match n(2) {
                0 => {
                    ModifyStyleSpec { format: StyleFormat { bold: Ch::Val(true), ..StyleFormat::default() }, ..ModifyStyleSpec::default() }
                }
                1 => ModifyStyleSpec {
                    format: StyleFormat { sz_half_pt: Ch::Val(28.0), alignment: Ch::Val("center".to_owned()), ..StyleFormat::default() },
                    ..ModifyStyleSpec::default()
                },
                2 => ModifyStyleSpec { quick: Some(false), auto_update: Some(true), ..ModifyStyleSpec::default() },
                _ => ModifyStyleSpec {
                    next: Ch::Null,
                    format: StyleFormat { italic: Ch::Val(true), color: Ch::Val("FF0000".to_owned()), ..StyleFormat::default() },
                    ..ModifyStyleSpec::default()
                },
            };
            Command::Style(StyleCmd::ModifyStyle { id: field(1)?, spec: Box::new(spec) })
        }
        "rn" => Command::Style(StyleCmd::RenameStyle { id: field(1)?, name: name(2)? }),
        "dd" => Command::Style(StyleCmd::DeleteStyle { id: field(1)? }),
        "sg" => Command::Style(StyleCmd::SetStyleInGallery { id: field(1)?, on: f.get(2) == Some(&"1") }),
        "us" => Command::Style(StyleCmd::UpdateStyleFromSelection { id: None }),
        "cp" => Command::Style(StyleCmd::ClearParagraphStyle),
        "cc" => Command::Style(StyleCmd::ClearCharacterStyle),
        "pf" => Command::Style(StyleCmd::ClearParagraphFormatting),
        "cf" => Command::Style(StyleCmd::ClearCharacterFormatting),
        "sf" => Command::Style(StyleCmd::SetDefaultFont {
            font: Box::new(match n(1) {
                0 => DefaultFont { size_pt: Some(12.0), ..DefaultFont::default() },
                1 => DefaultFont { font_family: Some("Arial".to_owned()), bold: Some(true), ..DefaultFont::default() },
                2 => DefaultFont { italic: Some(true), color: Ch::Val("00FF00".to_owned()), ..DefaultFont::default() },
                _ => DefaultFont { spacing_twips: Some(0.0), ..DefaultFont::default() },
            }),
        }),
        "ls" => Command::List(ListCmd::CreateListStyle {
            name: name(1)?,
            levels: if f.get(1) == Some(&"0") { vec![] } else { vec![lvl(0, "upperRoman", None), lvl(1, "lowerLetter", Some("%2)"))] },
            apply: true,
        }),
        "la" => Command::List(ListCmd::ApplyListStyle { style_id: field(1)? }),
        "dm" => Command::List(ListCmd::DefineMultilevelList {
            levels: vec![lvl(0, "upperLetter", None), lvl(1, "decimal", Some("%1.%2."))],
            forward: f.get(1) == Some(&"1"),
        }),
        _ => return None,
    })
}

const F6_NAMES: [&str; 9] = ["Alfa", "alfa", "Beta", "_oculto", "Gama1", "1ruim", "é2", "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "a b"];
const F6_URLS: [&str; 3] = ["https://example.com/a", " https://example.org ", "mailto:a@b.c"];

fn markup_patch(i: u32) -> Option<MarkupPatch> {
    let mut p = MarkupPatch::default();
    match i {
        0 => p.ins_del = Some(false),
        1 => p.ins_del = Some(true),
        2 => p.authors = Some(Some(vec!["A".into()])),
        3 => p.authors = Some(Some(vec!["B".into()])),
        4 => p.authors = Some(None),
        5 => p.formatting = Some(false),
        6 => p.comments = Some(false),
        7 => p.balloons = Some(Balloons::Inline),
        _ => return None,
    }
    Some(p)
}

/// `parity_d2b_f6_session.rs` `step`.
fn f6(f: &[&str]) -> Option<Action> {
    if let Some(a) = d2a(f) {
        return a;
    }
    let nn = |i: usize| num(f.get(i));
    let one = |i: usize| f.get(i) == Some(&"1");
    cmd(match *f.first()? {
        "au" => Command::Settings(SettingsCmd::SetAuthor { name: (*f.get(1)?).into() }),
        "ac" => Command::Revision(RevisionCmd::AcceptAt { index: nn(1) }),
        "rj" => Command::Revision(RevisionCmd::RejectAt { index: nn(1) }),
        "ach" => Command::Revision(RevisionCmd::Accept { move_next: one(1) }),
        "rch" => Command::Revision(RevisionCmd::Reject { move_next: one(1) }),
        "aal" => Command::Revision(RevisionCmd::AcceptAll { shown: one(1), stop_tracking: one(2) }),
        "ral" => Command::Revision(RevisionCmd::RejectAll { shown: one(1), stop_tracking: one(2) }),
        "nx" => Command::Revision(RevisionCmd::NextChange),
        "pv" => Command::Revision(RevisionCmd::PreviousChange),
        "sr" => Command::Revision(RevisionCmd::SelectRevision { index: nn(1) }),
        "mo" => Command::Revision(RevisionCmd::SetMarkupOptions(markup_patch(nn(1))?)),
        "mm" => Command::Settings(SettingsCmd::SetMarkup {
            mode: match *f.get(1)? {
                "all" => MarkupMode::All,
                "simple" => MarkupMode::Simple,
                "none" => MarkupMode::None,
                _ => MarkupMode::Original,
            },
        }),
        "cm" => Command::Comment(CommentCmd::Add { text: Utf16::from_units(units(f.get(1)?)), author: None }),
        "cr" => Command::Comment(CommentCmd::Remove { id: f64::from(nn(1)) }),
        "bm" => Command::Bookmark(BookmarkCmd::Insert { name: (*F6_NAMES.get(nn(1) as usize)?).into() }),
        "bd" => Command::Bookmark(BookmarkCmd::Delete { name: (*F6_NAMES.get(nn(1) as usize)?).into() }),
        "sl" => Command::Bookmark(BookmarkCmd::SetLink { url: F6_URLS.get(nn(1) as usize).map(|u| (*u).into()) }),
        "hl" => Command::Bookmark(BookmarkCmd::InsertHyperlink(HyperlinkOptions {
            address: Some((*F6_URLS.get(nn(1) as usize)?).into()),
            tip: one(2).then(|| "tip".into()),
            text: match *f.get(3)? {
                "1" => Some(Utf16::from_units("texto".encode_utf16().collect())),
                "2" => Some(Utf16::from_units(Vec::new())),
                _ => None,
            },
            ..HyperlinkOptions::default()
        })),
        "rl" => Command::Bookmark(BookmarkCmd::RemoveLink),
        _ => return None,
    })
}

/// The F1/F6 hook (`d2b_hook` of their replays): a step that reaches a list rule of F2 — the facets
/// from there on are accepted by name, as the native replays do.
fn list_hook(s: &Session, code: &str) -> Option<&'static str> {
    let sel = s.selection();
    if sel.anchor != sel.focus {
        return None;
    }
    let p = s.paras()?.get(sel.focus.para as usize)?;
    let len = uo_docx::session::text::para_length(p);
    let style = p.props.style_id.as_deref().and_then(|id| s.document().styles.get(id));
    let sheet = &s.document().formatting.styles.styles;
    let mut id = p.props.style_id.clone();
    let mut style_num = false;
    for _ in 0..32 {
        let Some(def) = id.as_deref().and_then(|i| sheet.get(i)) else { break };
        if def.p_pr.as_deref().is_some_and(|pp| pp.num_pr.is_some()) {
            style_num = true;
            break;
        }
        id = def.based_on.clone();
    }
    let numbered = p.props.numbering.is_some() || style_num;
    let list_like = numbered
        || style.and_then(|st| st.name.as_deref()).is_some_and(|n| n.eq_ignore_ascii_case("list paragraph"))
        || p.props.indent_left_twips == Some(uo_docx::js::JsNumber::Num(360.0));
    match code {
        "en" if numbered && len == 0 => Some("D2b-F2 list rule: listEnterOnEmpty (D2a-T4)"),
        "bs" if list_like && sel.focus.offset == 0 => Some("D2b-F2 list rule: listBackspaceAtStart (D2a-T4)"),
        _ => None,
    }
}

fn utf8(b: &[u8]) -> &str {
    std::str::from_utf8(b).unwrap_or("")
}

/// The accept reason of the family's hook for `code` before it runs (status 0, UTF-8), or status 1.
///
/// # Safety
/// `h` null or live; each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_hook(
    h: *mut UoWriteSession,
    family_ptr: *mut u8,
    family_len: usize,
    code_ptr: *mut u8,
    code_len: usize,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (family, code) = unsafe { (take_bytes(family_ptr, family_len), take_bytes(code_ptr, code_len)) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    match utf8(&family) {
        "f1" | "f5" | "f6" | "f7" => match list_hook(&s.session, utf8(&code)) {
            Some(why) => buf(OK, why.as_bytes().to_vec()),
            None => buf(NONE, Vec::new()),
        },
        _ => buf(NONE, Vec::new()),
    }
}

/// Action `k` of op code `code` of `family`, computed from the session's state after actions 0..k-1
/// ran through the exports: status 0 with `E` + an `Exec` message or `Q` + a `Query` message; status 1
/// when the code has no action `k`.
///
/// # Safety
/// As [`uo_session_test_hook`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_next(
    h: *mut UoWriteSession,
    family_ptr: *mut u8,
    family_len: usize,
    code_ptr: *mut u8,
    code_len: usize,
    k: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (family, code) = unsafe { (take_bytes(family_ptr, family_len), take_bytes(code_ptr, code_len)) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_ref() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let code = utf8(&code);
    let f: Vec<&str> = code.split(':').collect();
    let unbindable = std::cell::Cell::new(false);
    let action = match utf8(&family) {
        "f1" => SHELL.with(|m| {
            let mut m = m.borrow_mut();
            let sh = m.entry(h as usize).or_insert_with(|| "single".to_owned());
            f1(&f, k, s, sh)
        }),
        "f3" => {
            // the code argument is `<op>` or `<op>\t<lay>` (the step record's `lay`, which the TS read from the layout)
            let (op, lay) = code.split_once('\t').map_or((code, None), |(a, b)| (a, Some(b)));
            match op.split(':').next() {
                _ if k > 0 && !F3.with(|m| m.borrow().contains_key(&(h as usize))) => None,
                Some("un") => (k == 0).then_some(Action::Exec(w::ExecOp::Undo)),
                Some("re") => (k == 0).then_some(Action::Exec(w::ExecOp::Redo)),
                _ => F3.with(|m| {
                    let mut m = m.borrow_mut();
                    if k == 0 {
                        m.insert(h as usize, crate::d2b_f3_support::session_codes::commands(&s.session, op, lay));
                    }
                    let c = m.get(&(h as usize)).and_then(|v| v.get(k as usize)).cloned();
                    if c.is_none() {
                        m.remove(&(h as usize));
                    }
                    // a command that has no wire form is an error (status 5), not "the code has no more actions"
                    match c {
                        Some(c) => {
                            let a = cmd(c);
                            unbindable.set(a.is_none());
                            a
                        }
                        None => None,
                    }
                }),
            }
        }
        // the code is `<op>` or `<op> TAB <hf>`
        "f8" => f8::action(h as usize, &s.session, code, k),
        "f7" => f7::action(h as usize, &s.session, code, k),
        _ if k > 0 => None,
        "f2" => f2(code),
        "f6" => f6(&f),
        "f5" => d2a(&f).or_else(|| fam::f5_action(&f, &s.session)).flatten(),
        "f9" => d2a(&f).or_else(|| fam::f9_action(&f, &s.session)).flatten(),
        "f10" => match f.first() {
            Some(&("un" | "re")) => d2a(&f).flatten(),
            _ => f10::action(code),
        },
        _ => d2a(&f).flatten(),
    };
    let mut out = Vec::new();
    let r = match action {
        None if unbindable.get() => return buf(5, b"the command has no wire form (into_wire failed)".to_vec()),
        None => return buf(NONE, Vec::new()),
        Some(Action::Exec(op)) => {
            out.push(b'E');
            w::encode_exec(&mut out, &w::Delta::default(), &w::Exec { op })
        }
        Some(Action::Query(q)) => {
            out.push(b'Q');
            w::encode_query(&mut out, &w::Delta::default(), &w::Query { q })
        }
    };
    match r {
        Ok(()) => buf(OK, out),
        Err(e) => buf(5, format!("{e:?}").into_bytes()),
    }
}

fn paras_digest(s: &Session) -> String {
    let doc = s.document();
    let mut paras = doc.body.paras.clone();
    let mut ids = doc.ids.clone();
    uo_docx::session::normalize::normalize(&mut paras, &mut ids);
    let mut sink = Sink(Stream::new());
    for (i, p) in paras.iter().enumerate() {
        sink.0.tag(b'E').u32(u32::try_from(i).unwrap_or(u32::MAX));
        let mut cw = CanonWriter::new(&mut sink).with_media(&doc.media);
        p.canon(&mut cw);
    }
    sink.0.finish().short()
}

fn anc_json(s: &Session) -> String {
    let t: Vec<String> = s.document().body.tables.iter().map(|a| a.before_para.to_string()).collect();
    let b: Vec<String> = s.bookmark_paras().iter().map(|(k, v)| format!("[{},{v}]", uo_jsnum::json::string(k))).collect();
    format!("{{\"t\":[{}],\"b\":[{}]}}", t.join(","), b.join(","))
}

fn docxml(s: &mut Session) -> String {
    match s.save_docx() {
        Ok(bytes) => match uo_opc::Package::open(&bytes, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()) {
            Ok(pkg) => {
                let t = pkg.part_text("word/document.xml").ok().flatten().map(|t| t.into_owned()).unwrap_or_default();
                Digest::of(t.as_bytes()).short()
            }
            Err(_) => "err".to_owned(),
        },
        Err(_) => "err".to_owned(),
    }
}

/// The facets the wire does not carry: UTF-8 `paras\tanc\tdocxml` (`docxml` empty unless asked).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_facets(h: *mut UoWriteSession, want_docxml: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let d = if want_docxml != 0 { docxml(&mut s.session) } else { String::new() };
    buf(OK, format!("{}\t{}\t{d}", paras_digest(&s.session), anc_json(&s.session)).into_bytes())
}

/// The F3 facets beyond the common ones (copies of `parity_d2b_f3_session.rs`, declared): UTF-8
/// `paras\tanc\tdocxml\ttbl\tsel\tctx\tts`. `sel` is the BODY's selection (the saved one while a cell is edited).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_facets_f3(h: *mut UoWriteSession, want_docxml: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let d = if want_docxml != 0 { docxml(&mut s.session) } else { String::new() };
    let s = &s.session;
    let sel_text = |sel: uo_docx::session::Selection| {
        format!(
            "{{\"anchor\":{{\"para\":{},\"offset\":{}}},\"focus\":{{\"para\":{},\"offset\":{}}}}}",
            sel.anchor.para, sel.anchor.offset, sel.focus.para, sel.focus.offset
        )
    };
    let hop = |h: &uo_docx::session::CellHop| match h.block {
        Some(b) => format!("{{\"r\":{},\"c\":{},\"b\":{b}}}", h.row, h.col),
        None => format!("{{\"r\":{},\"c\":{}}}", h.row, h.col),
    };
    let doc = s.document();
    let mut ids = doc.ids.clone();
    let mut sink = Sink(Stream::new());
    for (i, a) in doc.body.tables.iter().enumerate() {
        let mut t = (*a.table).clone();
        uo_docx::session::normalize::normalize(&mut t, &mut ids);
        sink.0.tag(b'E').u32(u32::try_from(i).unwrap_or(u32::MAX));
        let mut cw = CanonWriter::new(&mut sink).with_media(&doc.media);
        t.canon(&mut cw);
    }
    let tbl = sink.0.finish().short();
    let sel = sel_text(saved_selection(s.context()).unwrap_or_else(|| s.selection()));
    let ctx = match s.context() {
        EditContext::Cell { anchor, hops, .. } => {
            let h: Vec<String> = hops.iter().map(hop).collect();
            format!("{{\"a\":{anchor},\"h\":[{}],\"s\":{}}}", h.join(","), sel_text(s.selection()))
        }
        _ => "null".to_owned(),
    };
    let ts = match s.table_selection() {
        None => "null".to_owned(),
        Some(ts) => {
            let h: Vec<String> = ts.path.iter().map(hop).collect();
            format!(
                "{{\"anchor\":{},\"path\":[{}],\"rect\":{{\"r1\":{},\"c1\":{},\"r2\":{},\"c2\":{}}},\"origin\":{{\"r\":{},\"c\":{}}}}}",
                ts.anchor,
                h.join(","),
                ts.row0,
                ts.col0,
                ts.row1,
                ts.col1,
                ts.origin_row,
                ts.origin_col
            )
        }
    };
    buf(OK, format!("{}\t{}\t{d}\t{tbl}\t{sel}\t{ctx}\t{ts}", paras_digest(s), anc_json(s)).into_bytes())
}

/// The facets of a family that has its own table (F5, F9; copies of the native replays' functions, declared): UTF-8
/// `name=value` pairs separated by tabs, for the code just run (`code` is the step's op code; `want`: bit 0 = the saved
/// `docxml`, bit 1 = the date-format table `dtf`).
///
/// # Safety
/// `h` null or live; each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_facets_fam(
    h: *mut UoWriteSession,
    family_ptr: *mut u8,
    family_len: usize,
    code_ptr: *mut u8,
    code_len: usize,
    want: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (family, code) = unsafe { (take_bytes(family_ptr, family_len), take_bytes(code_ptr, code_len)) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    match fam::facets(utf8(&family), h as usize, &mut s.session, utf8(&code), want) {
        Some(t) => buf(OK, t.into_bytes()),
        None => buf(NONE, Vec::new()),
    }
}

/// F10 setup of a handle: the scripted oracle (`ReplayOracle`), the reference seed the capture recorded and the
/// hyphenation patterns of the element (pt-BR, en, es). The session must have been opened with the F10 environment (the
/// replay's one extra draw, `setUiLang('pt')`).
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_f10_setup(h: *mut UoWriteSession, ref_seed: f64) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    s.session.set_ref_seed(ref_seed);
    s.session.set_hyphenation_registry(Some(f10::hyphenation_registry()));
    s.test_oracle = Some(Box::new(crate::d2b_f10_support::replay::ReplayOracle::new()));
    buf(OK, Vec::new())
}

/// Starts an F10 step: the TS's script (`i-fv`, `i-pg`, `i-mh` of the capture) as `fv TAB pg TAB mh` (an absent one is
/// empty), a clean record.
///
/// # Safety
/// As [`uo_session_test_hook`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_f10_begin(h: *mut UoWriteSession, ptr: *mut u8, len: usize) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let script = unsafe { take_bytes(ptr, len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(o) = s.test_oracle.as_deref_mut() else { return buf(NONE, Vec::new()) };
    let text = utf8(&script);
    let mut it = text.split('\t').map(|x| (!x.is_empty()).then_some(x));
    let (fv, pg, mh) = (it.next().flatten(), it.next().flatten(), it.next().flatten());
    o.begin(crate::d2b_f10_support::replay::StepScript::parse(fv, pg, mh));
    buf(OK, Vec::new())
}

/// The record of the F10 step: `fvs TAB pg_calls TAB pg_requested (sorted, comma) TAB mh_calls TAB fv_missing`.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_f10_record(h: *mut UoWriteSession) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    let Some(o) = s.test_oracle.as_deref() else { return buf(NONE, Vec::new()) };
    let mut asked: Vec<u32> = o.pg_requested.iter().copied().collect();
    asked.sort_unstable();
    let asked: Vec<String> = asked.iter().map(u32::to_string).collect();
    buf(OK, format!("{}\t{}\t{}\t{}\t{}", o.fvs(), o.pg_calls, asked.join(","), o.mh_calls, o.fv_missing).into_bytes())
}

/// F8: a new group of documents (the carried paste defaults, path and policy, and the Office Clipboard start over).
#[unsafe(no_mangle)]
pub extern "C" fn uo_session_test_f8_group() {
    f8::new_group();
}

/// F8: the carried state of the group goes into the freshly opened session; `ext` for an extended group.
///
/// # Safety
/// `h` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_f8_open(h: *mut UoWriteSession, ext: u32) -> *mut UoBuf {
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    f8::setup(s, ext != 0);
    buf(OK, Vec::new())
}

/// F8: the shell's inputs of the step (`cbt` as hex UTF-16 units, the paste-tree bytes when `has_tree`); the random counter starts at 0.
///
/// # Safety
/// As [`uo_session_test_hook`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_f8_step(
    h: *mut UoWriteSession,
    cbt_ptr: *mut u8,
    cbt_len: usize,
    tree_ptr: *mut u8,
    tree_len: usize,
    has_tree: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (cbt, tree) = unsafe { (take_bytes(cbt_ptr, cbt_len), take_bytes(tree_ptr, tree_len)) };
    if h.is_null() {
        return buf(NULL_HANDLE, Vec::new());
    }
    f8::begin_step(h as usize, Utf16::from_units(units(utf8(&cbt))), (has_tree != 0).then(|| std::sync::Arc::<[u8]>::from(tree)));
    buf(OK, Vec::new())
}

/// F7: the cumulative random-call counter starts over (call it right BEFORE the document opens: the open's own draws count, as the
/// native `SharedEnv`).
#[unsafe(no_mangle)]
pub extern "C" fn uo_session_test_f7_reset() {
    crate::session::env::reset_draws();
}

/// F7: one image of the capture (`img#k`): the extension and the bytes.
///
/// # Safety
/// Each `(ptr, len)` a buffer of `uo_alloc_bytes(len)` filled by JS.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_f7_image(k: u32, ext_ptr: *mut u8, ext_len: usize, data_ptr: *mut u8, data_len: usize) {
    // SAFETY: the caller guarantees the buffers; both are consumed here.
    let (ext, data) = unsafe { (take_bytes(ext_ptr, ext_len), take_bytes(data_ptr, data_len)) };
    f7::set_image(k as usize, utf8(&ext).to_owned(), data);
}

/// F7: the step's inputs: the `fx` fact log (UTF-8) and the target of the session's random count (`has_target`; the editor's own
/// uses of `Math.random` between the steps: the session's environment is advanced to it).
///
/// # Safety
/// As [`uo_session_test_hook`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_session_test_f7_begin(
    h: *mut UoWriteSession,
    fx_ptr: *mut u8,
    fx_len: usize,
    has_target: u32,
    target: u32,
) -> *mut UoBuf {
    // SAFETY: the caller guarantees the buffer; it is consumed here.
    let fx = unsafe { take_bytes(fx_ptr, fx_len) };
    // SAFETY: the caller guarantees `h` is null or live.
    let Some(s) = (unsafe { h.as_mut() }) else { return buf(NULL_HANDLE, Vec::new()) };
    f7::begin(h as usize, &mut s.session, utf8(&fx), (has_target != 0).then_some(target));
    buf(OK, Vec::new())
}

/// A random `Exec` message (the generated test support), for the hostile test.
#[unsafe(no_mangle)]
pub extern "C" fn uo_session_test_random_exec(seed: u32) -> *mut UoBuf {
    let mut rng = uo_codec::testing::Rng(u64::from(seed).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let m = w::Exec::random(&mut rng, &w::generated_support::Bounds::default(), 0);
    let mut out = Vec::new();
    match w::encode_exec(&mut out, &w::Delta::default(), &m) {
        Ok(()) => buf(OK, out),
        Err(e) => buf(5, format!("{e:?}").into_bytes()),
    }
}

/// A random `Query` message (the generated test support), for the hostile test (WS follow-up 2).
#[unsafe(no_mangle)]
pub extern "C" fn uo_session_test_random_query(seed: u32) -> *mut UoBuf {
    let mut rng = uo_codec::testing::Rng(u64::from(seed).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let m = w::Query::random(&mut rng, &w::generated_support::Bounds::default(), 0);
    let mut out = Vec::new();
    match w::encode_query(&mut out, &w::Delta::default(), &m) {
        Ok(()) => buf(OK, out),
        Err(e) => buf(5, format!("{e:?}").into_bytes()),
    }
}

/// Forgets the shell state of a handle the test frees.
#[unsafe(no_mangle)]
pub extern "C" fn uo_session_test_forget(h: *mut UoWriteSession) {
    SHELL.with(|m| m.borrow_mut().remove(&(h as usize)));
    F3.with(|m| m.borrow_mut().remove(&(h as usize)));
    f8::forget(h as usize);
    f7::forget(h as usize);
}
