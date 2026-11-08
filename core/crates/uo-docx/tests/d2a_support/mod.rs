//! Shared helpers of the D2a session tests: a layout oracle that is never needed by the D2a
//! commands, the corpus, opening a session, and the seeded command scripts.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use uo_docx::session::ops::context::ContextCmd;
use uo_docx::session::ops::settings::SettingsCmd;
use uo_docx::session::ops::text::TextCmd;
use uo_docx::session::{ClipboardRing, Command, DocPosition, ExecCx, FixedEnv, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_docx::view::{DocRef, DocView, FieldScope, HyphenCandidate, HyphenSkips, LayoutError, LayoutOracle, RunRef};
use uo_testkit::generate::Rng;

/// The D2a commands never call the layout; a call is a test failure.
pub struct NoLayout;

impl LayoutOracle for NoLayout {
    fn field_values(&mut self, _: DocView<'_>, _: &FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError> {
        panic!("the D2a commands do not call the layout")
    }
    fn next_hyphenation(&mut self, _: DocView<'_>, _: &DocRef, _: &HyphenSkips) -> Result<Option<HyphenCandidate>, LayoutError> {
        panic!("the D2a commands do not call the layout")
    }
}

/// A layout + clipboard pair for one call.
pub struct Cx {
    pub layout: NoLayout,
    pub clip: ClipboardRing,
}

impl Cx {
    pub fn new() -> Cx {
        Cx { layout: NoLayout, clip: ClipboardRing::default() }
    }
    pub fn cx(&mut self) -> ExecCx<'_> {
        ExecCx { layout: &mut self.layout, clipboard: &mut self.clip }
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|e| e == "docx") {
            out.push(p);
        }
    }
}

/// The oracle documents of docx-layout and the docx-core fixtures.
pub fn corpus() -> Vec<PathBuf> {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    walk(&root.join("write/packages/docx-layout/test/oracle"), &mut out);
    walk(&root.join("write/packages/docx-core/test/fixtures"), &mut out);
    out
}

/// Every `step`-th corpus document.
pub fn sample(step: usize) -> Vec<PathBuf> {
    corpus().into_iter().step_by(step.max(1)).collect()
}

pub fn open(bytes: Vec<u8>) -> Option<Session> {
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).ok()
}

fn pos(rng: &mut Rng, s: &Session) -> DocPosition {
    let paras = s.paras().unwrap();
    let para = rng.below(paras.len());
    let len = paras.get(para).map_or(0, uo_docx::session::text::para_length);
    DocPosition::new(u32::try_from(para).unwrap(), u32::try_from(rng.below(len + 1)).unwrap())
}

const TEXTS: [&str; 7] = ["a", "Olá ", " ", "ção", "😀", "𝒜!", "abc def"];

/// One seeded command (or `None` for an undo/redo, which the caller runs): `Ok(cmd)`.
#[allow(clippy::large_enum_variant)] // a test generator: one value at a time, a box would only add noise
pub enum Step {
    Cmd(Command),
    Undo,
    Redo,
}

/// A command a user could give now (positions inside the story).
pub fn next_step(rng: &mut Rng, s: &Session) -> Step {
    let k = rng.below(100);
    if k < 30 {
        let t = TEXTS[rng.below(TEXTS.len())];
        return Step::Cmd(Command::Text(TextCmd::Type { text: Utf16::from(t) }));
    }
    if k < 40 {
        return Step::Cmd(Command::Text(TextCmd::Enter));
    }
    if k < 52 {
        return Step::Cmd(Command::Text(TextCmd::Backspace));
    }
    if k < 58 {
        return Step::Cmd(Command::Text(TextCmd::Delete));
    }
    if k < 72 {
        let a = pos(rng, s);
        let f = if rng.below(2) == 0 { a } else { pos(rng, s) };
        return Step::Cmd(Command::Text(TextCmd::SetSelection { anchor: a, focus: f }));
    }
    if k < 76 {
        return Step::Cmd(Command::Settings(SettingsCmd::SetTracking { on: rng.below(2) == 0 }));
    }
    if k < 78 {
        let n = s.document().notes.footnotes.len();
        if n > 0 && rng.below(2) == 0 {
            let id = s.document().notes.footnotes.keys().nth(rng.below(n)).map_or(0.0, |k| k.get());
            return Step::Cmd(Command::Context(ContextCmd::EnterNote { footnote: true, id, caret: None, at_end: true }));
        }
        return Step::Cmd(Command::Context(ContextCmd::ExitNote));
    }
    if k < 80 {
        let n = s.document().body.tables.len();
        if n > 0 && rng.below(2) == 0 {
            let anchor = u32::try_from(rng.below(n)).unwrap();
            let hops = vec![uo_docx::session::CellHop { row: 0, col: 0, block: None }];
            return Step::Cmd(Command::Context(ContextCmd::EnterCell { anchor, hops, caret: None }));
        }
        return Step::Cmd(Command::Context(ContextCmd::ExitCell));
    }
    if k < 82 {
        let n = s.document().hf.len();
        if n > 0 && rng.below(2) == 0 {
            let part = uo_docx::ids::HfPartId(u32::try_from(rng.below(n)).unwrap());
            return Step::Cmd(Command::Context(ContextCmd::EnterHeaderFooter { part, page: 0 }));
        }
        return Step::Cmd(Command::Context(ContextCmd::ExitHeaderFooter));
    }
    if k < 91 {
        return Step::Undo;
    }
    Step::Redo
}

/// Runs one step.
pub fn run_step(s: &mut Session, step: Step, cx: &mut Cx) {
    match step {
        Step::Cmd(c) => {
            let _ = s.exec(c, &mut cx.cx());
        }
        Step::Undo => {
            let _ = s.undo(&mut cx.cx());
        }
        Step::Redo => {
            let _ = s.redo(&mut cx.cx());
        }
    }
}

/// The canonical digest of the whole document (A6).
pub fn doc_digest(s: &Session) -> String {
    let doc = s.document();
    let mut sink = super::support::Sink(uo_testkit::Stream::new());
    {
        let mut w = uo_docx::canon::CanonWriter::new(&mut sink).with_media(&doc.media);
        doc.canon_full(&mut w);
    }
    sink.0.finish().short()
}
