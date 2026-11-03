//! The test host of frontier R4b: an [`ObjectHost`] over a [`ReadCtx`] with the quirk rules of
//! `js/quirk-objects.mjs` (a relationship id resolves to an image when its UTF-16 length is even,
//! or equals `image_len`; a part id resolves to the part of the case) and the stub block reader of
//! the capture (one paragraph per `w:p` child, its text as the only run).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use uo_docx::error::DocxError;
use uo_docx::ids::{IdAlloc, MediaId};
use uo_docx::model::run::RunProps;
use uo_docx::model::{Paragraph, Run, Story};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::inline_objects::{ObjectHost, ResolvedImage, TextboxRead};
use uo_docx::read::{DOCX, ReadOptions};
use uo_docx::text::Utf16;
use uo_opc::Package;
use uo_opc::xml::{Document as XmlDocument, Element, parse};

pub struct QuirkHost<'p> {
    pub ctx: ReadCtx<'p>,
    pub parts: HashMap<String, String>,
    /// `Some(n)`: an id of exactly `n` UTF-16 units is an image (default: an even length)
    pub image_len: Option<usize>,
    /// `Some(n)`: `n` more levels may be entered (the G20 fallback past it)
    pub nested_left: Option<u32>,
}

impl<'p> QuirkHost<'p> {
    pub fn new(pkg: &'p Package, parts: HashMap<String, String>) -> QuirkHost<'p> {
        QuirkHost { ctx: ReadCtx::new(pkg, &ReadOptions::default()), parts, image_len: None, nested_left: None }
    }
}

impl ObjectHost for QuirkHost<'_> {
    fn ids(&mut self) -> &mut IdAlloc {
        &mut self.ctx.ids
    }

    fn resolve_image(&mut self, rid: &str) -> Result<Option<ResolvedImage>, DocxError> {
        let n = rid.encode_utf16().count();
        let yes = match self.image_len {
            Some(l) => n == l,
            None => n.is_multiple_of(2),
        };
        if yes {
            let id = self.ctx.media.add(format!("img:{rid}").as_bytes());
            Ok(Some(ResolvedImage { data: id, ext: "png".into() }))
        } else {
            Ok(None)
        }
    }

    fn add_media(&mut self, bytes: &[u8]) -> MediaId {
        self.ctx.media.add(bytes)
    }

    fn resolve_part(&mut self, rid: &str) -> Result<Option<XmlDocument>, DocxError> {
        match self.parts.get(rid) {
            Some(t) => Ok(Some(parse(t.as_str(), &DOCX).map_err(DocxError::from)?)),
            None => Ok(None),
        }
    }

    fn enter_nested(&mut self) -> bool {
        match &mut self.nested_left {
            Some(0) => false,
            Some(n) => {
                *n -= 1;
                true
            }
            None => self.ctx.enter_nested(),
        }
    }

    fn leave_nested(&mut self) {
        match &mut self.nested_left {
            Some(n) => *n += 1,
            None => self.ctx.leave_nested(),
        }
    }

    fn read_textbox_content(&mut self, content: Element<'_>) -> Result<TextboxRead, DocxError> {
        let mut story = Story::new();
        for p in content.children_named("w:p") {
            let mut para = Paragraph::empty(self.ctx.ids.para());
            para.runs = vec![Run { text: Utf16::from(p.text_content()), props: Arc::new(RunProps::default()), ..Run::default() }];
            story.push_paragraph(para);
        }
        Ok(TextboxRead::Blocks { story, txbx_src: None })
    }
}

/// `\`, LF, CR and TAB escapes of the corpus files.
pub fn unescape(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

/// The quirk corpus of the inline objects: `(key, fragment, parts)`.
pub fn quirk_cases() -> Vec<(String, String, HashMap<String, String>)> {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/parity/drawing-runs.quirks.txt")).unwrap();
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (key, text) = line.split_once('\t').unwrap();
        let text = unescape(text);
        let mut pieces = text.split('\u{1}');
        let frag = pieces.next().unwrap().to_owned();
        let parts: HashMap<String, String> =
            pieces.filter_map(|p| p.split_once('\u{2}')).map(|(rid, t)| (rid.to_owned(), t.to_owned())).collect();
        out.push((key.to_owned(), frag, parts));
    }
    out
}

pub fn empty_package() -> Package {
    Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
}
