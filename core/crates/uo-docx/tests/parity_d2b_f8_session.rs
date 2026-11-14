//! Parity of D2b-F8 (the clipboard) against the frozen `<universal-write>` element in Chromium
//! (G17): replays `tests/parity/d2b-f8-session.capture`, written by `js/capture-d2b-f8-session.mjs`.
//! Four documents are loaded one after the other on one element: here one [`Cx`] (one Office
//! Clipboard) per group and one session per document (its own session id: the TS `docToken`); the
//! element's `pasteDefaults` and `documentPath` carry over to the next document of the group.
//!
//! The shell's inputs come from the capture: the paste-tree payload the page encoded (`pt`), the
//! clipboard text the stub gave (`cbt`), the PNG and size `renderClipPng` answered (`fx`). Facets:
//! the D2a ones (`paras`, `sel`, `anc`, `ev`, `flags`, `docxml`) and F8's (`cbw`, `clip`, `ring`,
//! `po`, `psf`, `nv`, `sv`) and `rc` (random calls per step; the session's random source is seeded
//! with the page's state at each load, `#rs`, or before it, `rs0`). The extended groups (`x`) add
//! the formatting policy (carried to the next document of the group, as the element's property),
//! the page's inline type (`SetInlineTypes`), float and range selections before copy/cut and the
//! note and header/footer contexts; synthetic documents come with their bytes (`doc`).
//!
//! Recapture: `UO_TS_ROOT=<frozen checkout> UO_CHROMIUM=<chrome-headless-shell.exe> node
//! core/crates/uo-docx/js/capture-d2b-f8-session.mjs`.
#![cfg(feature = "session")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(unused_imports)]
use crate::d2a_support;
#[allow(unused_imports)]
use crate::d2b_f8_support;
#[allow(unused_imports)]
use crate::support;

use std::path::Path;
use std::sync::Arc;

use d2a_support::Cx;
use d2b_f8_support::{hx, units};
use support::Sink;
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
use uo_docx::session::ops::styles::UiLang;
use uo_docx::session::ops::tables::{TableCmd, TableRect};
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::policy::{CharacterFormat, Flag, Indentation, ListKind};
use uo_docx::session::{
    CellHop, Command, DocPosition, Event, FixedEnv, FormattingPolicy, Job, JobResult, LocalNow, OpenOptions, Outcome, Session,
};
use uo_docx::text::Utf16;
use uo_docx::write::sig::SigMode;
use uo_testkit::{Capture, Digest, Replay, Stream, repo_root};

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

fn pos_json(p: DocPosition) -> String {
    format!("{{\"para\":{},\"offset\":{}}}", p.para, p.offset)
}

fn anc_json(s: &Session) -> String {
    let t: Vec<String> = s.document().body.tables.iter().map(|a| a.before_para.to_string()).collect();
    let b: Vec<String> = s.bookmark_paras().iter().map(|(k, v)| format!("[{},{v}]", uo_jsnum::json::string(k))).collect();
    format!("{{\"t\":[{}],\"b\":[{}]}}", t.join(","), b.join(","))
}

fn ev_text(evs: &[Event]) -> String {
    let e: Vec<String> = evs
        .iter()
        .map(|e| match e {
            Event::Change => "uw-change:".to_owned(),
            Event::DirtyChange { dirty } => format!("uw-dirty-change:{dirty}"),
            Event::Input { .. } => "uw-input:".to_owned(),
            Event::TrackChanges { on } => format!("uw-track-changes:{on}"),
            Event::CellMode { active } => format!("uw-cellmode:{active}"),
            Event::NoteMode { active, .. } => format!("uw-notemode:{active}"),
            Event::HfMode { active } => format!("uw-hfmode:{active}"),
            Event::TbMode { active } => format!("uw-tbmode:{active}"),
            Event::Protected { active, .. } => format!("uw-protected:{active}"),
            // the policy guard's refusal is the session's (D2a); the TS guard returns silently
            Event::Blocked { .. } => String::new(),
            Event::StylesChange => "uw-styles-change:".to_owned(),
            Event::Pasted => "uw-pasted:".to_owned(),
            other => format!("{other:?}"),
        })
        .filter(|x| x.starts_with("uw-"))
        .collect();
    if e.is_empty() { "-".to_owned() } else { e.join(" ") }
}

fn flags(s: &Session) -> String {
    format!("{}{}{}:{}", u8::from(s.is_dirty()), u8::from(s.can_undo()), u8::from(s.can_redo()), s.edits_since_load())
}

/// The saved document.xml digest and the digest with the docPr ids masked (ONE save: a save
/// advances the session's fresh-id counter, as the TS's).
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

fn b64(s: &str) -> Vec<u8> {
    crate::parity_d2b_f8_html::b64_decode(s)
}

/// The shell's answers of a step: the clipboard read, then the PNGs in order.
struct Shell {
    text: Utf16,
    tree: Option<Arc<[u8]>>,
    fx: Vec<Option<(Vec<u8>, f64, f64)>>,
}

fn fx_of(cap: &Capture, key: &str) -> Vec<Option<(Vec<u8>, f64, f64)>> {
    let Some(f) = cap.get(key, "fx") else { return Vec::new() };
    let json = String::from_utf8(b64(f)).unwrap();
    // `[{"b64":"…","w":1,"h":2},null,…]` (a tiny reader of this shape)
    let mut out = Vec::new();
    for part in json.trim_start_matches('[').trim_end_matches(']').split("},") {
        let p = uo_jsnum::text::trim(part);
        if p.is_empty() {
            continue;
        }
        if p.starts_with("null") {
            out.push(None);
            continue;
        }
        let get = |k: &str| {
            let i = p.find(&format!("\"{k}\":")).unwrap() + k.len() + 3;
            let rest = &p[i..];
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            rest[..end].trim_matches('"').to_owned()
        };
        out.push(Some((b64(&get("b64")), uo_jsnum::parse::to_number(&get("w")), uo_jsnum::parse::to_number(&get("h")))));
    }
    out
}

fn drive(s: &mut Session, cx: &mut Cx, mut out: Outcome, sh: &mut Shell, evs: &mut Vec<Event>) {
    for _ in 0..32 {
        let Outcome::NeedsJobs(jobs) = out else {
            if let Outcome::Done(cs) = out {
                evs.extend(cs.events);
            }
            return;
        };
        if jobs.is_empty() {
            return;
        }
        let mut last = Outcome::NeedsJobs(Vec::new());
        for j in jobs {
            let r = match j {
                Job::ReadClipboard { .. } => {
                    JobResult::Clipboard { text: Some(sh.text.clone()), html_tree: sh.tree.clone(), fragment: None, image: None }
                }
                Job::RenderClip { .. } => match if sh.fx.is_empty() { None } else { sh.fx.remove(0) } {
                    Some((png, w, h)) => JobResult::Rendered { png: Arc::from(png), width_twips: w, height_twips: h },
                    None => JobResult::Failed,
                },
                _ => JobResult::Failed,
            };
            last = s.complete(j.id(), r, &mut cx.cx()).unwrap();
        }
        out = last;
    }
    s.cancel_pending();
}

/// The session's random source, counting its calls (the `rc` facet).
struct CountEnv {
    inner: FixedEnv,
    n: std::rc::Rc<std::cell::Cell<u32>>,
}

impl uo_docx::session::Env for CountEnv {
    fn now(&self) -> LocalNow {
        self.inner.now()
    }
    fn random_u32(&mut self) -> u32 {
        self.n.set(self.n.get() + 1);
        self.inner.random_u32()
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

#[test]
fn d2b_f8_session_matches_the_element() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity");
    // (UO_F8_CAPTURE: a debugging capture, see the script's UO_F8_OUT)
    let cap = Capture::read(std::env::var("UO_F8_CAPTURE").map_or_else(|_| dir.join("d2b-f8-session.capture"), Into::into)).unwrap();
    let root = repo_root(env!("CARGO_MANIFEST_DIR"));
    let steps: usize = cap.meta("steps").unwrap().parse().unwrap();
    let mut replay = Replay::new(&cap);
    let mut cx = Cx::new();
    let mut group = String::new();
    let mut defaults = PasteDefaults::default();
    let mut path: Option<String> = None;
    // the element's `formattingPolicy` (the extended groups' `fp` step; carries to the next document)
    let mut policy: Option<FormattingPolicy> = None;
    let mut in_group = 0usize;
    let mut docs = 0;
    let lay = PasteLayoutIn::default();
    for (di, rec) in cap.records().iter().filter(|r| r.key.ends_with("#rs")).enumerate() {
        let rel = rec.key.trim_end_matches("#rs");
        let g = rec.get("g").unwrap().to_owned();
        if g != group {
            group = g;
            cx = Cx::new();
            defaults = PasteDefaults::default();
            path = None;
            policy = None;
            in_group = 0;
        } else {
            in_group += 1;
        }
        replay.skip(&rec.key, "rs");
        replay.skip(&rec.key, "g");
        let ext = rec.get("x").is_some();
        if ext {
            replay.skip(&rec.key, "x");
        }
        // a synthetic document (built by the TS API in the page): its bytes are in the capture
        let syn = rec.get("doc").is_some();
        let bytes = match rec.get("doc") {
            Some(d) => {
                replay.skip(&rec.key, "doc");
                b64(d)
            }
            None => std::fs::read(root.join(rel)).unwrap(),
        };
        // the element's save counters (`freshId` of serialize.ts) start at the page's first load,
        // unless the page saved before (the synthetic builds)
        let fresh_counters = in_group == 0 && !syn;
        // the page's random state before the load when the load drew from it (`rs0`), else after it
        let seed: u32 = rec.get("rs0").unwrap_or_else(|| rec.get("rs").unwrap()).parse().unwrap();
        if rec.get("rs0").is_some() {
            replay.skip(&rec.key, "rs0");
        }
        let t = uo_docx::session::DateParts { year: 2026, month: 1, day: 2, hour: 3, minute: 4 };
        let rc = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let env = CountEnv { inner: FixedEnv::new(LocalNow { local: t, utc: t }, seed), n: std::rc::Rc::clone(&rc) };
        let mut s = Session::open(
            bytes,
            OpenOptions {
                sig_mode: SigMode::Ts,
                session_id: u32::try_from(di).unwrap() + 1,
                ts_formula_errors: true,
                ts_numbering: true,
                ..OpenOptions::default()
            },
            Box::new(env),
        )
        .unwrap();
        // the page's `navigator.languages` (context locale pt-BR): the interface language
        s.set_ui_lang(UiLang::Pt);
        s.exec(Command::Settings(SettingsCmd::SetAuthor { name: "A".into() }), &mut cx.cx()).unwrap();
        s.exec(Command::Clipboard(ClipboardCmd::SetPasteDefaults { defaults }), &mut cx.cx()).unwrap();
        s.exec(Command::Clipboard(ClipboardCmd::SetDocumentPath { path: path.clone().map(Into::into) }), &mut cx.cx()).unwrap();
        if ext {
            // the page's plugin (`el.use({ inlineRenderers: [{ type: 'f8.tag' }] })`)
            s.exec(Command::Clipboard(ClipboardCmd::SetInlineTypes { types: vec!["f8.tag".into()] }), &mut cx.cx()).unwrap();
        }
        if policy.is_some() {
            s.exec(Command::Settings(SettingsCmd::SetPolicy { policy: policy.clone() }), &mut cx.cx()).unwrap();
        }
        // a named acceptance from a step on (the rest of the document)
        let mut accept_from: Option<&str> = None;
        // a picture of a copied table went into this document (F8-D14)
        let mut doc_tainted = false;
        for j in 0..=steps {
            let key = format!("{rel}#{j}");
            let Some(code) = cap.get(&key, "op") else { break };
            replay.check(&key, "op", code);
            if cap.get(&key, "thrown").is_some() {
                replay.skip(&key, "thrown");
                break;
            }
            rc.set(0);
            let tree: Option<Arc<[u8]>> = cap.get(&key, "pt").map(|p| Arc::from(b64(p)));
            if tree.is_some() {
                replay.skip(&key, "pt");
            }
            let cbt = cap.get(&key, "cbt").map(|h| Utf16::from_units(units(h)));
            if cbt.is_some() {
                replay.skip(&key, "cbt");
            }
            if cap.get(&key, "fx").is_some() {
                replay.skip(&key, "fx");
            }
            let mut sh = Shell { text: cbt.unwrap_or_default(), tree: tree.clone(), fx: fx_of(&cap, &key) };
            let mut evs: Vec<Event> = Vec::new();
            let mut copied = false;
            let mut hf_path: Option<String> = None;
            if j > 0 {
                // `xf+`: the float selection is dropped first (the capture's `el.floatSel = null`)
                let (clear_fs, body) = match code.strip_prefix("xf+") {
                    Some(b) => (true, b),
                    None => (false, code),
                };
                let f: Vec<&str> = body.split(':').collect();
                let nn = |i: usize| f[i].parse::<u32>().unwrap();
                let mut more: Vec<Command> = Vec::new();
                let cmd: Option<Command> = match f[0] {
                    "ty" => Some(Command::Text(TextCmd::Type { text: Utf16::from_units(units(f[1])) })),
                    "en" => Some(Command::Text(TextCmd::Enter)),
                    "bs" => Some(Command::Text(TextCmd::Backspace)),
                    "se" => Some(Command::Text(TextCmd::SetSelection {
                        anchor: DocPosition::new(nn(1), nn(2)),
                        focus: DocPosition::new(nn(3), nn(4)),
                    })),
                    "tk" => Some(Command::Settings(SettingsCmd::SetTracking { on: f[1] == "1" })),
                    "co" => {
                        copied = true;
                        Some(Command::Clipboard(ClipboardCmd::Copy))
                    }
                    "cu" => {
                        copied = true;
                        Some(Command::Clipboard(ClipboardCmd::Cut))
                    }
                    "pr" => {
                        let text = cx.clip.current().map(|c| c.text.clone()).unwrap_or_default();
                        Some(Command::Clipboard(ClipboardCmd::PasteData {
                            text,
                            html: None,
                            image: None,
                            rich: RichPick::WithRich,
                            mode: mode(f[1]),
                            list: list(f[2]),
                            layout: lay,
                        }))
                    }
                    "ph" => Some(Command::Clipboard(ClipboardCmd::PasteData {
                        text: sh.text.clone(),
                        html: tree.clone(),
                        image: None,
                        rich: RichPick::None,
                        mode: mode(f[1]),
                        list: None,
                        layout: lay,
                    })),
                    "pt" => Some(Command::Clipboard(ClipboardCmd::PasteData {
                        text: Utf16::from_units(units(f[2])),
                        html: None,
                        image: None,
                        rich: RichPick::None,
                        mode: mode(f[1]),
                        list: None,
                        layout: lay,
                    })),
                    "px" => Some(Command::Clipboard(ClipboardCmd::PasteText { text: Utf16::from_units(units(f[1])) })),
                    "pv" => Some(Command::Clipboard(ClipboardCmd::Paste { mode: mode(f[1]), layout: lay })),
                    "rp" => Some(Command::Clipboard(ClipboardCmd::Repaste { mode: mode(f[1]).unwrap(), list: list(f[2]), layout: lay })),
                    "ts" => {
                        let (r1, c1, r2, c2) = (i64::from(nn(2)), i64::from(nn(3)), i64::from(nn(4)), i64::from(nn(5)));
                        Some(Command::Table(TableCmd::SetTableSelection {
                            anchor: nn(1),
                            path: Vec::new(),
                            rect: TableRect { r1, c1, r2, c2 },
                            origin: (r1, c1),
                        }))
                    }
                    "pc" => Some(Command::Clipboard(ClipboardCmd::PasteCells {
                        mode: match f[1] {
                            "rows" => CellsPasteMode::Rows,
                            "nest" => CellsPasteMode::Nest,
                            "text" => CellsPasteMode::Text,
                            _ => CellsPasteMode::Overwrite,
                        },
                        layout: lay,
                    })),
                    "sf" => Some(Command::Clipboard(ClipboardCmd::PasteSpecialFormats)),
                    "ps" => Some(Command::Clipboard(ClipboardCmd::PasteSpecial {
                        format: match f[1] {
                            "rtf" => PasteSpecialFormat::Rtf,
                            "text" => PasteSpecialFormat::Text,
                            "picture" => PasteSpecialFormat::Picture,
                            "html" => PasteSpecialFormat::Html,
                            "unicode" => PasteSpecialFormat::Unicode,
                            _ => PasteSpecialFormat::Image,
                        },
                        link: f[2] == "1",
                        layout: lay,
                    })),
                    "pi" => {
                        let id = clipboard_items(&cx.clip)[nn(1) as usize].id;
                        Some(Command::Clipboard(ClipboardCmd::PasteClipboardItem { id, mode: mode(f[2]), layout: lay }))
                    }
                    "pa" => Some(Command::Clipboard(ClipboardCmd::PasteAllClipboardItems { layout: lay })),
                    "rm" => {
                        let id = clipboard_items(&cx.clip)[nn(1) as usize].id;
                        Some(Command::Clipboard(ClipboardCmd::RemoveClipboardItem { id }))
                    }
                    "cl" => Some(Command::Clipboard(ClipboardCmd::ClearClipboardItems)),
                    "pd" => {
                        defaults = PasteDefaults { within: mode(f[1]).unwrap(), between: mode(f[2]).unwrap(), other: mode(f[3]).unwrap() };
                        Some(Command::Clipboard(ClipboardCmd::SetPasteDefaults { defaults }))
                    }
                    "dp" => {
                        let p = PATHS[nn(1) as usize];
                        path = if p.is_empty() { None } else { Some(p.to_owned()) };
                        Some(Command::Clipboard(ClipboardCmd::SetDocumentPath { path: path.clone().map(Into::into) }))
                    }
                    "ec" => {
                        let anchor = nn(1);
                        let gb = s
                            .document()
                            .body
                            .tables
                            .get(anchor as usize)
                            .and_then(|a| a.table.rows.first())
                            .and_then(|r| r.tr_pr.as_deref())
                            .and_then(|p| p.grid_before)
                            .map_or(0, |g| uo_jsnum::conv::to_uint32(g.to_number()));
                        Some(Command::Context(ContextCmd::EnterCell {
                            anchor,
                            hops: vec![CellHop { row: 0, col: gb, block: None }],
                            caret: Some(DocPosition::new(0, 0)),
                        }))
                    }
                    "xc" => Some(Command::Context(ContextCmd::ExitCell)),
                    // the extended groups
                    "fp" => {
                        policy = match nn(1) {
                            0 => None,
                            1 => FormattingPolicy::preset("template-document"),
                            _ => Some(f8_policy()),
                        };
                        Some(Command::Settings(SettingsCmd::SetPolicy { policy: policy.clone() }))
                    }
                    "so" | "sm" => {
                        let a = f[f.len() - 1];
                        if a != "n" {
                            copied = true;
                            more.push(Command::Clipboard(if a == "c" { ClipboardCmd::Copy } else { ClipboardCmd::Cut }));
                        }
                        Some(if f[0] == "so" {
                            Command::Drawing(DrawingCmd::SelectObject { index: Some(nn(1)) })
                        } else {
                            Command::Text(TextCmd::SetSelection {
                                anchor: DocPosition::new(nn(1), nn(2)),
                                focus: DocPosition::new(nn(3), nn(4)),
                            })
                        })
                    }
                    "on" => Some(Command::Note(NoteCmd::Open { footnote: f[1] == "0", id: uo_jsnum::parse::to_number(f[2]) })),
                    "xn" => Some(Command::Context(ContextCmd::ExitNote)),
                    "xh" => Some(Command::Context(ContextCmd::ExitHeaderFooter)),
                    "eh" => {
                        let hf = cap.get(&key, "hf").unwrap();
                        replay.skip(&key, "hf");
                        let (p, cr) = hf.split_once('|').unwrap();
                        let mut first = None;
                        if cr != "-" {
                            let g: Vec<&str> = cr.split(':').collect();
                            let slot = match g[2] {
                                "first" => HfSlot::First,
                                "even" => HfSlot::Even,
                                _ => HfSlot::Default,
                            };
                            let kind = if g[0] == "header" { HfKind::Header } else { HfKind::Footer };
                            first = Some(Command::HeaderFooter(HeaderFooterCmd::CreateHeaderFooterPart {
                                kind,
                                section: g[1].parse().unwrap(),
                                slot,
                            }));
                        }
                        let denied = policy.as_ref().is_some_and(|x| x.denied.contains(&Flag::HeadersFooters));
                        if p != "-" {
                            // the part exists once the creation ran: resolved by path after it
                            more.push(Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(u32::MAX), page: 0 }));
                            hf_path = Some(p.to_owned());
                        } else if denied {
                            // the guard refuses (`uw-blocked`)
                            more.push(Command::Context(ContextCmd::EnterHeaderFooter { part: HfPartId(0), page: 0 }));
                        }
                        first.or_else(|| (!more.is_empty()).then(|| more.remove(0)))
                    }
                    "un" => {
                        evs.extend(s.undo(&mut cx.cx()).events);
                        None
                    }
                    "re" => {
                        evs.extend(s.redo(&mut cx.cx()).events);
                        None
                    }
                    _ => panic!("step {code}"),
                };
                let pre = clear_fs.then_some(Command::Drawing(DrawingCmd::SelectObject { index: None }));
                for c in pre.into_iter().chain(cmd).chain(more) {
                    let c = match (c, hf_path.as_deref()) {
                        (Command::Context(ContextCmd::EnterHeaderFooter { part, page }), Some(p)) if part == HfPartId(u32::MAX) => {
                            Command::Context(ContextCmd::EnterHeaderFooter { part: part_of_path(&s, p), page })
                        }
                        (c, _) => c,
                    };
                    match s.exec(c, &mut cx.cx()) {
                        Ok(o) => drive(&mut s, &mut cx, o, &mut sh, &mut evs),
                        Err(e) => panic!("{key} {code}: {e}"),
                    }
                }
            }
            if std::env::var("UO_F8_TRACE").is_ok_and(|x| rel.contains(&x)) {
                let texts: Vec<String> =
                    s.paras().unwrap().iter().map(|p| String::from_utf16_lossy(&uo_docx::session::text::para_text(p))).collect();
                println!("#{j} {code} {texts:?} ts={:?} sel={:?} ctx={:?}", s.table_selection(), s.selection(), s.context());
                if std::env::var("UO_F8_FULL").is_ok() {
                    let mut paras = s.document().body.paras.clone();
                    let mut ids = s.document().ids.clone();
                    uo_docx::session::normalize::normalize(&mut paras, &mut ids);
                    for q in paras.iter().filter(|q| q.runs.iter().any(|r| r.ext().is_some_and(|e| e.image.is_some()))) {
                        println!("  {}", uo_docx::word::obj::json_of(Some(q)).unwrap_or_default());
                    }
                }
            }
            let sel = s.selection();
            let body_sel = saved_selection(s.context()).unwrap_or(sel);
            let cbw = match (copied, s.copied()) {
                (true, Some(c)) => {
                    let t = hx(c.text.units());
                    let t = if t.len() > 200 { sha(&t) } else { t };
                    if let Ok(d) = std::env::var("UO_F8_XML") {
                        std::fs::write(Path::new(&d).join(format!("rust-cbw-{j}.html")), c.html.as_deref().unwrap_or("-")).unwrap();
                    }
                    format!("{t}|{}", c.html.as_deref().map_or_else(|| "-".to_owned(), sha))
                }
                _ => "-".to_owned(),
            };
            let clip = match cx.clip.current() {
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
                                p.marker.as_ref().map_or(String::new(), |m| format!("[{}]", hx(m.units())))
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let mut lists: Vec<f64> = c.lists.iter().map(|(k, _)| *k).collect();
                    lists.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let lists = lists.iter().map(|k| uo_jsnum::format::to_js_string(*k)).collect::<Vec<_>>().join(",");
                    let t = format!(
                        "{}|{}|{kind}|{paras}|{lists}|{}|{}|{}",
                        hx(c.text.units()),
                        c.paras.len(),
                        c.tables.len(),
                        c.path.as_deref().unwrap_or(""),
                        u8::from(c.src.is_some())
                    );
                    if t.len() > 300 { sha(&t) } else { t }
                }
                None => "-".to_owned(),
            };
            let ring =
                clipboard_items(&cx.clip).iter().map(|x| format!("{}:{}", hx(x.text.units()), kind(x.kind))).collect::<Vec<_>>().join(" ");
            if let Some(raw) = cap.get(&key, "ringraw") {
                // (a debugging capture, UO_F8_RING)
                replay.skip(&key, "ringraw");
                if raw != ring {
                    println!("{key} ring ts={raw} rust={ring}");
                }
            }
            let po = s.paste_options();
            let po = format!(
                "{}|{}|{}|{}",
                po.modes.iter().map(|m| mode_name(*m)).collect::<Vec<_>>().join(","),
                po.list.iter().map(|m| list_name(*m)).collect::<Vec<_>>().join(","),
                po.current.map_or("", |c| mode_name(c.0)),
                po.current.and_then(|c| c.1).map_or("", list_name)
            );
            let psf = if code.strip_prefix("xf+").unwrap_or(code).starts_with("sf:") {
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
            if let Ok(d) = std::env::var("UO_F8_XML") {
                std::fs::write(Path::new(&d).join(format!("rust-nv-{docs}-{j}.xml")), s.numbering_store().xml().map_or("", |x| &**x))
                    .unwrap();
            }
            let mut facets = vec![
                ("paras", paras_digest(&s)),
                ("sel", format!("{{\"anchor\":{},\"focus\":{}}}", pos_json(body_sel.anchor), pos_json(body_sel.focus))),
                ("anc", anc_json(&s)),
                ("flags", flags(&s)),
                ("cbw", cbw),
                ("clip", clip),
                ("ring", sha(if ring.is_empty() { "-" } else { &ring })),
                ("po", po),
                ("psf", psf),
                ("nv", sha(s.numbering_store().xml().map_or("", |x| &**x))),
                ("sv", sha(s.styles_store().xml().map_or("", |x| &**x))),
                ("rc", if j == 0 { cap.get(&key, "rc").unwrap_or("0").to_owned() } else { rc.get().to_string() }),
                ("ev", if j == 0 { "-".to_owned() } else { ev_text(&evs) }),
            ];
            if cap.get(&key, "docxml").is_some() {
                let (d, masked) = docxml_pair(&mut s, &format!("{docs}-{j}"));
                replay.skip(&key, "docxmlm");
                // G7/F8-O2: the TS element's fresh docPr ids of the save (`freshId`, a module counter of
                // serialize.ts) carry across the documents of a page: only a later document of a page
                if !fresh_counters && cap.get(&key, "docxml") != Some(d.as_str()) && cap.get(&key, "docxmlm") == Some(masked.as_str()) {
                    replay.accept(
                        &key,
                        "docxml",
                        &d,
                        "G7/F8-O2: the docPr ids and the image relationship numbers of the save (element counters)",
                    );
                } else {
                    facets.push(("docxml", d));
                }
            }
            let paste_step =
                ["pr", "pi", "pa", "pv", "ps", "pc", "rp"].iter().any(|p| code.strip_prefix("xf+").unwrap_or(code).starts_with(p));
            if paste_step
                && (cx.clip.current().is_some_and(|c| clip_has_table_picture(c))
                    || cx.clip.items().any(|x| clip_has_table_picture(&x.clip)))
            {
                doc_tainted = true;
            }
            let tainted = doc_tainted || cx.clip.current().is_some_and(|c| clip_has_table_picture(c));
            for (f, actual) in facets {
                let differs = cap.get(&key, f) != Some(actual.as_str());
                // F8-D14: the TS copies tables by JSON, so a picture in a copied table loses its bytes;
                // only the facets those bytes reach are accepted (the content, its save, what the copy
                // wrote, the clip), every other facet is still compared
                let d14 = matches!(f, "cbw" | "paras" | "docxml" | "clip");
                if differs && accept_from.is_none() && tainted && d14 {
                    accept_from = Some("F8-D14: a picture in a copied table keeps its bytes (the TS JSON copy turns them into an object)");
                }
                match accept_from {
                    Some(r) if d14 && differs => {
                        replay.accept(&key, f, &actual, r);
                    }
                    _ => {
                        replay.check(&key, f, &actual);
                    }
                }
            }
        }
        docs += 1;
    }
    let report = replay.finish();
    println!("d2b-f8-session: {docs} documents, {}", report.summary());
    if !report.is_clean() {
        println!("{}", report.describe(std::env::var("UO_F8_ALL").map_or(40, |_| 100_000)));
    }
    report.assert_clean();
}
