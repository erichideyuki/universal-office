//! Text boxes nested in text boxes: the deepest chain of stories the reader recurses through
//! (G20; a text box's paragraphs are read from inside the run that holds it). Shared by the
//! nested-story stack test (`tests/nested_stack.rs`), the no-panic test of R4c, the native stack
//! measurement (`examples/nested_stack_native.rs`) and the wasm32 probe
//! (`examples/nested_stack_wasm.rs`).
#![allow(dead_code, clippy::panic)]

use uo_docx::DocxError;
use uo_docx::canon::{CanonSink, CanonWriter};
use uo_docx::js::JsValue;
use uo_docx::limits::DocxLimits;
use uo_docx::model::{MediaStore, Paragraph};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::paragraph::{BlocksMode, ReadContext};
use uo_docx::read::{DOCX, DOCX_POS, ReadOptions};
use uo_opc::Package;
use uo_opc::xml::parse;

/// The native stack the nested-story tests give a read of 64 levels (and more, past the limit):
/// the G20 budget proper is 256 KiB of the wasm32 shadow stack (`js/wasm-stack.mjs`, measured
/// ≤ 105 KB); x86-64 frames are larger (callee-saved and spilled registers live in the same
/// stack; ≤ 0.31 MB measured in a release build on Windows) and a debug build has no stack
/// colouring (every temporary has its own slot: ≤ 1.04 MB). Record "Nested-story stack — fixed".
pub const NESTED_STACK_KIB: usize = if cfg!(debug_assertions) { 1536 } else { 384 };

/// Runs `f` on a thread with [`NESTED_STACK_KIB`] of stack.
pub fn on_nested_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    match std::thread::Builder::new().stack_size(NESTED_STACK_KIB * 1024).spawn(f) {
        Ok(handle) => match handle.join() {
            Ok(v) => v,
            Err(e) => std::panic::resume_unwind(e),
        },
        Err(e) => panic!("spawning the nested-story thread: {e}"),
    }
}

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:wps=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\" \
xmlns:wpg=\"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup\"";

/// The object that holds the text box of one level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nesting {
    /// `w:pict` / `v:shape` / `v:textbox`
    Vml,
    /// `w:drawing` / `wps:wsp` / `wps:txbx`
    Dml,
    /// `w:drawing` / `wpg:wgp` / `wps:wsp` / `wps:txbx` (a text box in a group)
    DmlGroup,
    /// `w:pict` / `v:group` / `v:shape` / `v:textbox`
    VmlGroup,
    /// `mc:AlternateContent`: the drawing of `mc:Choice` (a `wps` text box), a VML fallback
    Alternate,
    /// the five above in turn, one per level
    Mixed,
}

impl Nesting {
    /// Every kind.
    pub const ALL: [Nesting; 6] = [Nesting::Vml, Nesting::Dml, Nesting::DmlGroup, Nesting::VmlGroup, Nesting::Alternate, Nesting::Mixed];

    /// The opening and closing text of the run that holds level `i`'s text box (the content goes
    /// in between).
    fn level(self, i: usize) -> (&'static str, &'static str) {
        const VML: (&str, &str) = (
            "<w:p><w:r><w:pict><v:shape style=\"position:absolute;width:100pt;height:50pt\"><v:textbox><w:txbxContent>",
            "</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>",
        );
        const DML: (&str, &str) = (
            "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/><a:graphic><a:graphicData><wps:wsp><wps:txbx><w:txbxContent>",
            "</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
        );
        const DML_GROUP: (&str, &str) = (
            "<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/><a:graphic><a:graphicData><wpg:wgp><wpg:grpSpPr><a:xfrm><a:ext cx=\"1905000\" cy=\"952500\"/></a:xfrm></wpg:grpSpPr><wps:wsp><wps:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"952500\" cy=\"952500\"/></a:xfrm></wps:spPr><wps:txbx><w:txbxContent>",
            "</w:txbxContent></wps:txbx></wps:wsp></wpg:wgp></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
        );
        const VML_GROUP: (&str, &str) = (
            "<w:p><w:r><w:pict><v:group style=\"position:absolute;width:100pt;height:50pt\" coordsize=\"1000,500\"><v:shape style=\"left:0;top:0;width:500;height:500\"><v:textbox><w:txbxContent>",
            "</w:txbxContent></v:textbox></v:shape></v:group></w:pict></w:r></w:p>",
        );
        const ALTERNATE: (&str, &str) = (
            "<w:p><w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing><wp:inline><wp:extent cx=\"1905000\" cy=\"952500\"/><a:graphic><a:graphicData><wps:wsp><wps:txbx><w:txbxContent>",
            "</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing></mc:Choice><mc:Fallback><w:pict><v:rect fillcolor=\"red\" style=\"width:10pt;height:10pt\"/></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>",
        );
        match self {
            Nesting::Vml => VML,
            Nesting::Dml => DML,
            Nesting::DmlGroup => DML_GROUP,
            Nesting::VmlGroup => VML_GROUP,
            Nesting::Alternate => ALTERNATE,
            Nesting::Mixed => [VML, DML, DML_GROUP, VML_GROUP, ALTERNATE][i % 5],
        }
    }
}

/// A part whose body is one paragraph holding a text box `levels` deep (`levels` = 0: the core
/// paragraph alone).
pub fn nested_part(levels: usize, kind: Nesting) -> String {
    let mut text = format!("<w:document {NS}><w:body>");
    for i in 0..levels {
        text.push_str(kind.level(i).0);
    }
    text.push_str("<w:p><w:r><w:t>core</w:t></w:r></w:p>");
    for i in (0..levels).rev() {
        text.push_str(kind.level(i).1);
    }
    text.push_str("</w:body></w:document>");
    text
}

struct Count(u64);

impl CanonSink for Count {
    fn tag(&mut self, _: u8) {
        self.0 += 1;
    }
    fn u32(&mut self, _: u32) {
        self.0 += 1;
    }
    fn f64(&mut self, _: f64) {
        self.0 += 1;
    }
    fn bool(&mut self, _: bool) {
        self.0 += 1;
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0 += b.len() as u64;
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0 += b.len() as u64;
    }
}

/// How the nested stories are read.
#[derive(Debug, Clone, Copy)]
pub struct NestedRead {
    pub kind: Nesting,
    /// positions kept (`ReadOptions::sources`)
    pub sources: bool,
    /// the text boxes read by the block reader of the full document (`BlocksMode::Blocks`,
    /// `Grouped` with sources), else as paragraphs
    pub blocks: bool,
}

/// Reads the body paragraphs of [`nested_part`] (`levels` deep) with their media (`None` when the
/// read fails).
pub fn read_nested(levels: usize, how: NestedRead) -> Option<(Vec<Paragraph>, MediaStore)> {
    let text = nested_part(levels, how.kind);
    let d = parse(text, if how.sources { &DOCX_POS } else { &DOCX }).ok()?;
    let pkg = Package::from_parts(
        [("word/_rels/document.xml.rels".to_owned(), b"<Relationships/>".to_vec())],
        &uo_opc::Limits::DEFAULT,
        &uo_opc::package::OpenOptions::default(),
    );
    let opts = ReadOptions { sources: how.sources, limits: DocxLimits::DEFAULT };
    let mut ctx = ReadCtx::new(&pkg, &opts);
    let part = if how.sources { ctx.source_part("word/document.xml", &d) } else { None };
    let mut paras: Vec<Paragraph> = Vec::new();
    {
        let mut rc = ReadContext::new(&mut ctx, "word/_rels/document.xml.rels").ok()?;
        rc.set_source_part(part);
        rc.blocks = match (how.blocks, how.sources) {
            (false, _) => BlocksMode::None,
            (true, false) => BlocksMode::Blocks,
            (true, true) => BlocksMode::Grouped,
        };
        let body = d.root().child_elements().find(|e| e.name() == "w:body");
        for p in body.iter().flat_map(|b| b.child_elements()).filter(|e| e.name() == "w:p") {
            match rc.read_paragraph(p) {
                Ok(p) => paras.push(p),
                Err(DocxError::Limit { .. }) | Err(_) => return None,
            }
        }
    }
    Some((paras, std::mem::take(&mut ctx.media)))
}

/// The digest of what [`read_nested`] read: the count of items the canonical writer wrote.
pub fn digest(paras: &[Paragraph], media: &MediaStore) -> u64 {
    let mut sink = Count(0);
    {
        let mut w = CanonWriter::new(&mut sink).with_media(media);
        for p in paras {
            p.canon(&mut w);
        }
    }
    sink.0
}

/// [`read_nested`], [`digest`] and drop (0 when the read fails).
pub fn read_nested_textboxes(levels: usize, how: NestedRead) -> u64 {
    read_nested(levels, how).map_or(0, |(paras, media)| digest(&paras, &media))
}

/// The depth of text boxes in the first paragraph of a read: how many levels were read as stories
/// (a text box past `max_nesting` holds plain paragraphs without objects).
pub fn story_depth(paras: &[Paragraph]) -> usize {
    let mut depth = 0;
    let mut cur = paras.first();
    while let Some(p) = cur {
        let ext = p.runs.iter().filter_map(|r| r.ext.as_deref()).find(|e| e.textbox.is_some() || e.group.is_some());
        let tb = ext.and_then(|e| {
            e.textbox.as_deref().or_else(|| {
                e.group.as_deref().and_then(|g| g.items.iter().find_map(|it| it.run.ext.as_deref().and_then(|x| x.textbox.as_deref())))
            })
        });
        let Some(tb) = tb else { break };
        depth += 1;
        cur = tb.content.flat_paragraphs().into_iter().next();
    }
    depth
}
