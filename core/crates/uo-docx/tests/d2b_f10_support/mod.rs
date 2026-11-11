//! Shared by the D2b-F10 tests: the scripted layout oracle (what the layout answers is data, and
//! every call is counted and kept) and a document builder.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::VecDeque;

use uo_docx::read::blank::{BlankDocxOptions, blank_docx_package};
use uo_docx::session::{ClipboardRing, ExecCx, FixedEnv, OpenOptions, Session};
use uo_docx::text::Utf16;
use uo_docx::view::{
    DocRef, DocView, FieldScope, HyphenCandidate, HyphenSkips, LayoutError, LayoutErrorKind, LayoutOracle, RunRef, RunSub, StoryRef,
};

/// What a `field_values` call answers: a function of the model, called with the view and scope.
pub type FieldFn = Box<dyn FnMut(DocView<'_>, &FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError>>;

/// What a `shown_page_numbers` call answers.
pub type PagesFn = Box<dyn FnMut(&[DocRef]) -> Result<Vec<Option<f64>>, LayoutError>>;

/// A layout oracle that answers from scripts and records what it was asked.
pub struct ScriptOracle {
    pub fields: FieldFn,
    /// the page of every paragraph asked (`None` entries: no line)
    pub pages: PagesFn,
    /// answers of `next_hyphenation`, first to last; empty = no more candidates
    pub hyph: VecDeque<Result<Option<HyphenCandidate>, LayoutError>>,
    pub field_calls: usize,
    pub field_scopes: Vec<Option<usize>>,
    pub page_calls: usize,
    pub page_requests: Vec<usize>,
    pub hyph_calls: usize,
    pub hyph_from: Vec<DocRef>,
}

pub fn layout_error(kind: LayoutErrorKind) -> LayoutError {
    LayoutError { kind, detail: "scripted".into() }
}

impl ScriptOracle {
    /// Fields answer nothing, every paragraph is on page 3, no hyphenation points.
    pub fn new() -> ScriptOracle {
        ScriptOracle {
            fields: Box::new(|_, _| Ok(Vec::new())),
            pages: Box::new(|refs| Ok(vec![Some(3.0); refs.len()])),
            hyph: VecDeque::new(),
            field_calls: 0,
            field_scopes: Vec::new(),
            page_calls: 0,
            page_requests: Vec::new(),
            hyph_calls: 0,
            hyph_from: Vec::new(),
        }
    }

    /// Every SEQ field answers its order (1, 2, …) and every PAGEREF "7"; `REF` fields answer "ref".
    pub fn numbering() -> ScriptOracle {
        let mut o = ScriptOracle::new();
        o.fields = Box::new(|view, scope| {
            let mut out = Vec::new();
            let mut n = 0u32;
            let body = view.body();
            walk_body(body, &mut |rr, instr| {
                let name = uo_jsnum::text::split_js_space(uo_jsnum::text::trim(instr)).next().unwrap_or("").to_ascii_uppercase();
                let v: Option<String> = match name.as_str() {
                    "SEQ" => {
                        n += 1;
                        Some(n.to_string())
                    }
                    "PAGEREF" => Some("7".to_owned()),
                    "REF" | "NOTEREF" | "STYLEREF" => Some("ref".to_owned()),
                    _ => None,
                };
                if let Some(v) = v
                    && match scope {
                        FieldScope::All => true,
                        FieldScope::Runs(s) => s.contains(&rr),
                    }
                {
                    out.push((rr, Utf16::from(v.as_str())));
                }
            });
            Ok(out)
        });
        o
    }

    pub fn cx<'a>(&'a mut self, clip: &'a mut ClipboardRing) -> ExecCx<'a> {
        ExecCx { layout: self, clipboard: clip }
    }
}

/// Visits every field run of the top-level paragraphs of the body (cells too) with its RunRef.
fn walk_body(story: &uo_docx::model::story::Story, f: &mut dyn FnMut(RunRef, &str)) {
    use uo_docx::model::story::Block;
    fn go(
        s: &uo_docx::model::story::Story,
        base: Option<(&uo_docx::view::BlockPath, u32, u32)>,
        f: &mut dyn FnMut(RunRef, &str),
        depth: usize,
    ) {
        if depth > 8 {
            return;
        }
        for (bi, b) in s.blocks().enumerate() {
            let bi = u32::try_from(bi).unwrap();
            let path = match base {
                None => uo_docx::view::BlockPath::root(bi),
                Some((tp, r, c)) => tp.cell_block(r, c, bi),
            };
            match b {
                Block::Paragraph(p) => {
                    for (ri, r) in p.runs.iter().enumerate() {
                        if let Some(instr) = r.ext().and_then(|e| e.field.as_deref()) {
                            f(
                                RunRef { story: StoryRef::Body, path: path.clone(), run: u32::try_from(ri).unwrap(), sub: RunSub::None },
                                instr,
                            );
                        }
                        for (j, x) in r.ext().and_then(|e| e.field_result.as_ref()).into_iter().flatten().enumerate() {
                            if let Some(instr) = x.ext().and_then(|e| e.field.as_deref()) {
                                f(
                                    RunRef {
                                        story: StoryRef::Body,
                                        path: path.clone(),
                                        run: u32::try_from(ri).unwrap(),
                                        sub: RunSub::FieldResult(u32::try_from(j).unwrap()),
                                    },
                                    instr,
                                );
                            }
                        }
                    }
                }
                Block::Table(t) => {
                    for (ri, row) in t.rows.iter().enumerate() {
                        for (ci, cell) in row.cells.iter().enumerate() {
                            go(&cell.content, Some((&path, u32::try_from(ri).unwrap(), u32::try_from(ci).unwrap())), f, depth + 1);
                        }
                    }
                }
            }
        }
    }
    go(story, None, f, 0);
}

impl LayoutOracle for ScriptOracle {
    fn field_values(&mut self, doc: DocView<'_>, scope: &FieldScope) -> Result<Vec<(RunRef, Utf16)>, LayoutError> {
        self.field_calls += 1;
        self.field_scopes.push(match scope {
            FieldScope::All => None,
            FieldScope::Runs(s) => Some(s.len()),
        });
        (self.fields)(doc, scope)
    }

    fn next_hyphenation(&mut self, _: DocView<'_>, from: &DocRef, _: &HyphenSkips) -> Result<Option<HyphenCandidate>, LayoutError> {
        self.hyph_calls += 1;
        self.hyph_from.push(from.clone());
        self.hyph.pop_front().unwrap_or(Ok(None))
    }

    fn shown_page_numbers(&mut self, _: DocView<'_>, paras: &[DocRef]) -> Result<Vec<Option<f64>>, LayoutError> {
        self.page_calls += 1;
        self.page_requests.push(paras.len());
        (self.pages)(paras)
    }
}

/// A session over a blank document with `body` as the body XML.
pub fn open_body(body: &str) -> Session {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    let mut pkg = blank_docx_package(&BlankDocxOptions::default());
    pkg.set_part_text("word/document.xml", xml);
    let bytes = pkg.save(&uo_opc::package::SaveOptions::default()).unwrap();
    Session::open(bytes, OpenOptions::default(), Box::new(FixedEnv::default())).unwrap()
}

/// A paragraph with one run.
pub fn p(text: &str) -> String {
    format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
}

/// A paragraph of outline level `level` (1–9).
pub fn h(level: u32, text: &str) -> String {
    format!("<w:p><w:pPr><w:outlineLvl w:val=\"{}\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>", level - 1)
}

/// The text of body paragraph `i`.
pub fn text(s: &Session, i: usize) -> String {
    String::from_utf16_lossy(&uo_docx::session::text::para_text(s.document().body.paras.get(i).unwrap()))
}

pub fn texts(s: &Session) -> Vec<String> {
    (0..s.document().body.paras.len()).map(|i| text(s, i)).collect()
}

pub mod hostile;
pub mod names;
pub mod replay;
