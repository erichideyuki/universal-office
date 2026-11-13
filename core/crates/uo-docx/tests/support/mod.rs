//! Shared helpers of the uo-docx tests: the stream adapter of the canonical digest and the
//! hand-built model equivalent to `js/canon-selftest.mjs` `sampleDocument()`/`sampleSources()`.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::canon::{CanonSink, CanonWriter};
use uo_docx::ids::{HfPartId, SourceKey};
use uo_docx::js::{JsMap, JsNumber, JsObject, JsRecord, NumKey};
use uo_docx::model::color::DrawingColor;
use uo_docx::model::document::{BookmarkEnd, BookmarkStart, Bookmarks, Comment, CoreProps, NoteSpecialIds, Notes};
use uo_docx::model::drawing::ShapeStyle;
use uo_docx::model::formatting::{DocFormatting, ResolvedStyle, Settings, StyleDef, StyleDefaults, StyleSheet, Theme, ThemeFontSet};
use uo_docx::model::media::MediaRef;
use uo_docx::model::para::{BlockSdt, ParaProps, SdtKey};
use uo_docx::model::props::{PPr, RFonts, RPr, SpacingSpec};
use uo_docx::model::run::{DrawingTextbox, InlineImage, RunExt, RunProps, TextboxView};
use uo_docx::model::section::{HfPart, HfRef, SectPr, Section, SectionProps};
use uo_docx::model::sources::{
    CellSource, NoteItemSource, NotesPartSource, NumSet, ParaSource, RowSource, RunSource, SegSource, SourcePart, SourceStore, SrcText,
    StoryGroup, StorySource, TableSource, WrapId, WrapSeg,
};
use uo_docx::model::table::{Cell, Row, Table};
use uo_docx::model::{Document, MediaStore, Paragraph, Run, Story};
use uo_docx::read::plain::default_run_props;
use uo_docx::text::Utf16;
use uo_drawingml::dml::{ColorMod, SpecHead, WordSpec};
use uo_drawingml::{CustCmd, CustPath};
use uo_testkit::Stream;

/// `uo_testkit::Stream` as a canonical sink.
pub struct Sink(pub Stream);

impl CanonSink for Sink {
    fn tag(&mut self, c: u8) {
        self.0.tag(c);
    }
    fn u32(&mut self, v: u32) {
        self.0.u32(v);
    }
    fn f64(&mut self, v: f64) {
        self.0.f64(v);
    }
    fn bool(&mut self, v: bool) {
        self.0.bool(v);
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.bytes(b);
    }
    fn digest_of(&mut self, b: &[u8]) {
        self.0.digest_of(b);
    }
}

pub fn n(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

pub fn s(x: &str) -> Box<str> {
    x.into()
}

fn para(id: u32, runs: Vec<Run>) -> Paragraph {
    Paragraph { id: uo_docx::ids::ParaId(id), runs, ..Paragraph::default() }
}

fn text_run(t: &str) -> Run {
    Run { text: Utf16::from(t), props: Arc::new(default_run_props()), ..Run::default() }
}

fn story(paras: Vec<Paragraph>) -> Story {
    let mut st = Story::new();
    for p in paras {
        st.push_paragraph(p);
    }
    st
}

fn marked<T: JsObject>(mut o: T, keys: &[&str]) -> T {
    for k in keys {
        o.mark(k);
    }
    o
}

/// The Rust equivalent of `sampleDocument()`.
pub fn sample_document() -> Document {
    let mut media = MediaStore::new();
    let mut units: Vec<u16> = "Olá 😀".encode_utf16().collect();
    units.push(0xd800);
    let mut props1 = RunProps { bold: Some(true), src: Some(SourceKey(2)), ..default_run_props() };
    props1.mark_undefined("fontAscii");
    // readRPr assigns in XML child order: the reader records it
    let mut rpr = RPr {
        b: Some(true),
        sz: Some(n(f64::NAN)),
        r_fonts: Some(Box::new(RFonts { ascii: Some(s("Aptos")), ..RFonts::default() })),
        ..RPr::default()
    };
    for k in ["b", "sz", "rFonts"] {
        rpr.mark(k);
    }
    let run1 = Run { text: Utf16::from_units(units), props: Arc::new(props1), rpr: Some(Arc::new(rpr)), ..Run::default() };
    let p1 = Paragraph {
        props: Arc::new(ParaProps {
            style_id: Some(s("Heading1")),
            src: Some(SourceKey(1)),
            block_sdt: Some(Arc::new(BlockSdt { key: SdtKey(s("s7")), gallery: Some(s("Table of Contents")), ..BlockSdt::default() })),
            ..ParaProps::default()
        }),
        ppr: Some(Arc::new(PPr {
            p_style: Some(s("Heading1")),
            spacing: Some(Box::new(SpacingSpec { before: Some(n(-0.0)), ..SpacingSpec::default() })),
            ..PPr::default()
        })),
        ..para(0, vec![run1])
    };
    let mut cell = Cell { grid_span: n(1.0), content: story(vec![para(1, vec![text_run("cell")])]), ..Cell::default() };
    cell.mark_undefined("widthTwips");
    cell.mark_undefined("shadingHex");
    cell.mark_undefined("borders");
    let mut table = Table { rows: vec![Row { cells: vec![cell], ..Row::default() }], grid_cols_twips: vec![n(2000.0)], ..Table::default() };
    table.mark_undefined("borders");

    let obj = || Run { text: Utf16::from("\u{fffc}"), props: Arc::new(default_run_props()), ..Run::default() };
    let mut tb_run = obj();
    tb_run.ext_mut().textbox = Some(Box::new(DrawingTextbox {
        width_twips: n(100.0),
        height_twips: n(50.0),
        content: story(vec![para(2, vec![])]),
        view: TextboxView::Synced,
        ..DrawingTextbox::default()
    }));
    let mut phantom = obj();
    phantom.ext_mut().textbox = Some(Box::new(DrawingTextbox {
        width_twips: n(1.0),
        height_twips: n(1.0),
        view: TextboxView::Phantom,
        ..DrawingTextbox::default()
    }));
    let mut img_run = obj();
    let mut adj = JsRecord::new();
    adj.insert("adj", n(5000.0));
    adj.insert("2", n(1.0));
    img_run.ext = Some(Box::new(RunExt {
        image: Some(Box::new(InlineImage {
            data: MediaRef(media.add(&[1, 2, 3])),
            ext: s("png"),
            width_twips: n(10.0),
            height_twips: n(20.0),
            style: Some(Box::new(marked(
                ShapeStyle {
                    geom: s("rect"),
                    fill: Some(DrawingColor(WordSpec {
                        head: SpecHead::Hex(s("4472C4")),
                        mods: vec![ColorMod { name: s("a:lumMod"), val: Some(s("75000")) }],
                        alpha: None,
                    })),
                    cust: Some(vec![CustPath {
                        w: 1.0,
                        h: 2.0,
                        fill: true,
                        stroke: false,
                        cmds: vec![CustCmd::MoveTo { x: 0.0, y: 0.0 }, CustCmd::Close],
                    }]),
                    adj: Some(adj),
                    ..ShapeStyle::default()
                },
                &["geom", "fill", "cust", "adj"],
            ))),
            ..InlineImage::default()
        })),
        ..RunExt::default()
    }));
    let p2 = para(3, vec![tb_run, phantom, img_run]);

    let mut body = Story::new();
    body.push_paragraph(p1);
    body.push_table(Arc::new(table));
    body.push_paragraph(p2);

    let mut headers = JsRecord::new();
    headers.insert("default", HfPartId(0));
    let section = Section {
        sect_pr: Arc::new(SectPr {
            page_width_twips: n(11906.0),
            page_height_twips: n(16838.0),
            margin_left_twips: n(1701.0),
            margin_right_twips: n(1701.0),
            margin_top_twips: n(1417.0),
            margin_bottom_twips: n(1417.0),
            ..SectPr::default()
        }),
        sect: Some(Arc::new(SectionProps { r#type: s("nextPage"), ..SectionProps::default() })),
        block_start: n(0.0),
        block_end: n(2.0),
        header_refs: vec![HfRef {
            r#type: s("default"),
            rid: s("rId8"),
            path: s("word/header1.xml"),
            part: HfPartId(0),
            ..HfRef::default()
        }],
        headers,
        ..Section::default()
    };
    let hf = [HfPart { path: s("word/header1.xml"), story: story(vec![para(4, vec![text_run("H")])]), ..HfPart::default() }]
        .into_iter()
        .collect();

    let mut footnotes = JsMap::new();
    footnotes.insert(NumKey::new(1.0), story(vec![para(5, vec![text_run("note")])]));
    let notes = Notes {
        footnotes,
        endnotes: JsMap::new(),
        special_ids: NoteSpecialIds { footnote: vec![n(-1.0), n(0.0)], ..NoteSpecialIds::default() },
        ..Notes::default()
    };

    let mut comment = Comment { id: n(0.0), text: Utf16::from("c"), ..Comment::default() };
    comment.mark_undefined("author");

    let mut styles = JsMap::new();
    styles.insert(
        s("Normal"),
        ResolvedStyle {
            run: RunProps { bold: Some(false), ..RunProps::default() },
            name: Some(s("Normal")),
            r#type: Some(s("paragraph")),
            ..ResolvedStyle::default()
        },
    );

    let mut bookmarks = Bookmarks::default();
    bookmarks.starts.insert(s("_Toc1"), BookmarkStart { block: n(0.0), offset: n(3.0) });
    bookmarks.ends.insert(s("_Toc1"), BookmarkEnd { block: n(0.0), offset: n(5.0), ..BookmarkEnd::default() });

    let mut sheet_styles = JsRecord::new();
    sheet_styles
        .insert("Normal", StyleDef { style_id: s("Normal"), r#type: s("paragraph"), name: Some(s("Normal")), ..StyleDef::default() });
    sheet_styles.insert("10", StyleDef { style_id: s("10"), r#type: s("paragraph"), ..StyleDef::default() });
    let mut scripts = JsRecord::new();
    scripts.insert("Jpan", s("X"));
    let mut colors = JsRecord::new();
    colors.insert("dk1", s("000000"));
    let formatting = DocFormatting {
        styles: StyleSheet {
            styles: sheet_styles,
            defaults: StyleDefaults { paragraph: Some(s("Normal")), ..StyleDefaults::default() },
            ..StyleSheet::default()
        },
        theme: Theme { minor: ThemeFontSet { scripts, latin: Some(s("Aptos")), ..ThemeFontSet::default() }, colors, ..Theme::default() },
        settings: Settings { default_tab_stop: n(720.0), compatibility_mode: n(15.0), ..Settings::default() },
        ..DocFormatting::default()
    };

    Document {
        body,
        sections: Arc::from(vec![section]),
        hf,
        notes: Arc::new(notes),
        comments: Arc::from(vec![comment]),
        styles: Arc::new(styles),
        bookmarks: Arc::new(bookmarks),
        formatting: Arc::new(formatting),
        core_props: Some(CoreProps { created: Some(s("2024-01-01T00:00:00Z")), ..CoreProps::default() }),
        media,
        ..Document::default()
    }
}

pub const DOC_XML: &str = "<w:document>é<w:body><w:p><w:r><w:t>Olá</w:t></w:r></w:p><w:tbl/></w:body></w:document>";

fn span(part_text: &str, piece: &str) -> SrcText {
    let start = part_text.find(piece).unwrap();
    SrcText::Span { part: 0, start: start as u32, end: (start + piece.len()) as u32 }
}

fn o(x: &str) -> SrcText {
    SrcText::Owned(s(x))
}

/// The Rust equivalent of `sampleSources()`.
pub fn sample_sources() -> SourceStore {
    let para_raw = "<w:p><w:r><w:t>Olá</w:t></w:r></w:p>";
    let mut st = SourceStore { next: 3, ..SourceStore::default() };
    st.parts.push(SourcePart { path: s("word/document.xml"), text: DOC_XML.into() });
    st.paras.insert(
        SourceKey(1),
        ParaSource {
            raw: span(DOC_XML, para_raw),
            open: o("<w:p>"),
            at: Some(DOC_XML.find(para_raw).unwrap() as u32),
            segs: vec![SegSource {
                raw: o("<w:r><w:t>Olá</w:t></w:r>"),
                sids: vec![n(2.0)],
                open: Some(true),
                link: Some(s("https://x")),
                ..SegSource::default()
            }],
            ..ParaSource::default()
        },
    );
    st.runs.insert(SourceKey(2), RunSource { r_pr: Some(o("<w:rPr/>")), ..RunSource::default() });
    st.tables.insert(
        SourceKey(3),
        TableSource {
            raw: span(DOC_XML, "<w:tbl/>"),
            open: o("<w:tbl>"),
            at: Some(DOC_XML.find("<w:tbl/>").unwrap() as u32),
            ..TableSource::default()
        },
    );
    st.trs.insert(
        SourceKey(4),
        RowSource {
            open: o("<w:tr>"),
            pre: Some(vec![WrapSeg { raw: o("<w:sdt>"), open: Some(WrapId(37)), ..WrapSeg::default() }]),
            post: Some(vec![WrapSeg { raw: o("</w:sdt>"), close: Some(WrapId(37)), ..WrapSeg::default() }]),
            ..RowSource::default()
        },
    );
    st.tcs.insert(
        SourceKey(5),
        CellSource {
            open: o("<w:tc>"),
            groups: Some(vec![StoryGroup { raw: o("<w:p/>"), blocks: vec![n(0.0)], ..StoryGroup::default() }]),
            ..CellSource::default()
        },
    );
    st.stories.insert(
        s("word/document.xml"),
        StorySource {
            open: o("<w:document>é<w:body>"),
            close: o("</w:body></w:document>"),
            groups: vec![StoryGroup {
                raw: span(DOC_XML, para_raw),
                blocks: vec![n(1.0)],
                sdt_key: Some(SdtKey(s("s7"))),
                inner: Some(vec![]),
                ..StoryGroup::default()
            }],
            ..StorySource::default()
        },
    );
    let mut refs = NumSet::default();
    refs.0.insert(NumKey::new(1.0));
    st.note_parts.insert(
        s("word/footnotes.xml"),
        NotesPartSource {
            open: o("<w:footnotes>"),
            close: o("</w:footnotes>"),
            items: vec![NoteItemSource { id: n(-1.0), raw: o("<w:footnote/>"), separator: Some(true), ..NoteItemSource::default() }],
            refs_at_load: refs,
            ..NotesPartSource::default()
        },
    );
    st.comment_ids.0.insert(NumKey::new(0.0));
    st.comments_sig = Some(s("[{\"id\":0,\"text\":\"c\"}]"));
    st
}

/// Digests (hex) of the full stream, the key-order facet and the sources facet, and their traces.
pub fn digests(doc: &Document, sources: &SourceStore, traced: bool) -> ([String; 3], [String; 3]) {
    let mk = || Sink(if traced { Stream::traced() } else { Stream::new() });
    let (mut full, mut order, mut src) = (mk(), mk(), mk());
    {
        let mut w = CanonWriter::new(&mut full).with_order(&mut order).with_media(&doc.media);
        doc.canon_full(&mut w);
    }
    {
        let mut ignore = mk();
        let mut w = CanonWriter::new(&mut src).with_order(&mut ignore).with_parts(&sources.parts);
        uo_docx::js::JsValue::canon(sources, &mut w);
    }
    let traces = [full.0.trace().to_string(), order.0.trace().to_string(), src.0.trace().to_string()];
    ([full.0.finish().hex(), order.0.finish().hex(), src.0.finish().hex()], traces)
}
