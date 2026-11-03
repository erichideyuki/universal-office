//! Memory gate M1 of the D1a foundation: every public operation that allocates returns the live
//! bytes exactly to their value before it, outputs dropped (`uo_alloc::assert_no_leak`), error
//! paths included. The reader's own M1/M2 rows come with R1–R6.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::hint::black_box;
use std::sync::Arc;

use uo_alloc::assert_no_leak;
use uo_docx::canon::{BytesSink, CanonWriter};
use uo_docx::ids::{DrawingId, TableId};
use uo_docx::js::{JsObject, JsRecord, JsValue, JsonWriter, KeyOrder};
use uo_docx::model::{MediaStore, Paragraph, Story};
use uo_docx::pvec::PVec;
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::{ReadOptions, matchers, plain, read_document};
use uo_docx::text::Utf16;
use uo_docx::view::{BlockPath, CellKey, DocView, RunRef, RunSub, StoryRef};

fn empty_pkg() -> uo_opc::Package {
    uo_opc::Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
}

#[test]
fn m1_values() {
    assert_no_leak(|| black_box(Utf16::from("Olá 😀")));
    assert_no_leak(|| {
        let mut m = MediaStore::new();
        let a = m.add(&[1, 2, 3]);
        let b = m.add(&[1, 2, 3]);
        let c = m.add(&[9; 4096]);
        black_box((a, b, c, m))
    });
    assert_no_leak(|| {
        let mut r = JsRecord::new();
        for k in ["b", "10", "a", "2", "b"] {
            r.insert(k, k.len());
        }
        r.remove("10");
        black_box(r)
    });
    assert_no_leak(|| {
        let mut k = KeyOrder::default();
        for i in [3, 1, 3, 7] {
            k.touch(i);
        }
        k.forget(1);
        black_box(k)
    });
}

#[test]
fn m1_pvec() {
    assert_no_leak(|| {
        let mut v: PVec<u64> = (0..1000).collect();
        let snap = v.clone();
        *v.make_mut(500).unwrap() = 7;
        for i in 0..300 {
            v.insert(10, Arc::new(i));
        }
        for _ in 0..200 {
            v.remove(3);
        }
        v.replace(0, Arc::new(1));
        black_box((v, snap))
    });
}

#[test]
fn m1_model_and_view() {
    let doc = support::sample_document();
    assert_no_leak(|| black_box(support::sample_document()));
    assert_no_leak(|| black_box(doc.clone()));
    let view = DocView::new(&doc);
    assert_no_leak(|| black_box(view.revision_authors()));
    assert_no_leak(|| black_box(view.find_table(TableId(0)).is_some()));
    assert_no_leak(|| black_box(view.find_textbox(DrawingId(0)).is_some()));
    assert_no_leak(|| black_box(view.cell(CellKey { table: TableId(0), row: 0, cell: 0 }).is_some()));
    assert_no_leak(|| {
        let r = RunRef { story: StoryRef::Body, path: BlockPath::root(0), run: 0, sub: RunSub::None };
        black_box(view.run(&r).is_some())
    });
    assert_no_leak(|| black_box(BlockPath::root(1).cell_block(0, 0, 0).cell_block(1, 1, 1).cell_block(2, 2, 2).to_string()));
    assert_no_leak(|| black_box(doc.body.flat_paragraphs().len()));
    assert_no_leak(|| {
        let mut s = Story::new();
        for _ in 0..200 {
            s.push_paragraph(Paragraph::default());
        }
        black_box(s.blocks().count())
    });
}

#[test]
fn m1_digest_and_json() {
    let doc = support::sample_document();
    let sources = support::sample_sources();
    assert_no_leak(|| black_box(support::digests(&doc, &sources, true)));
    assert_no_leak(|| {
        let mut sink = BytesSink::default();
        let mut order = BytesSink::default();
        let mut w = CanonWriter::new(&mut sink).with_order(&mut order).with_media(&doc.media);
        doc.canon_full(&mut w);
        black_box((sink, order))
    });
    assert_no_leak(|| black_box(JsonWriter::stringify(doc.body.paras.get(0).unwrap(), Some(&doc.media))));
    assert_no_leak(|| black_box(JsonWriter::stringify(&*doc.formatting, None)));
    assert_no_leak(|| {
        let mut p = doc.body.paras.get(0).unwrap().clone();
        p.mark("ppr");
        p.mark_undefined("pprEdited");
        black_box(JsonWriter::stringify(&p, None))
    });
}

#[test]
fn m1_read_foundation() {
    let pkg = empty_pkg();
    let opts = ReadOptions::default();
    assert_no_leak(|| black_box(read_document(&pkg, &opts)));
    assert_no_leak(|| {
        let mut ctx = ReadCtx::new(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() });
        let k = ctx.next_block_sdt_key();
        let w = ctx.next_wrap_id();
        let m = ctx.media_part("word/media/none.png");
        let x = ctx.xml_part("word/document.xml");
        let r = ctx.rels("word/_rels/document.xml.rels");
        ctx.begin_part("word/document.xml");
        let c = ctx.count_paragraph();
        black_box((k, w, m, x, r, c))
    });
    let xml = "<w:body><w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t></w:r><w:r><w:pict><w:txbxContent><w:p><w:r><w:t>in</w:t></w:r></w:p></w:txbxContent></w:pict></w:r></w:p><w:p/></w:body>";
    let parsed = uo_opc::xml::parse(xml, &uo_docx::read::DOCX).unwrap();
    assert_no_leak(|| {
        let mut ids = uo_docx::ids::IdAlloc::new();
        black_box(plain::plain_paragraphs(parsed.root(), &mut ids))
    });
    let mut ids = uo_docx::ids::IdAlloc::new();
    let paras = plain::plain_paragraphs(parsed.root(), &mut ids);
    let texts: Vec<String> = paras.iter().map(|p| p.runs.iter().map(|r| r.text.to_string_lossy()).collect()).collect();
    assert_eq!(texts, ["a\tb", "in", ""]);
    assert_no_leak(|| black_box(matchers::self_closing_to_open("<w:p a=\"1\" />")));
    assert_no_leak(|| black_box(matchers::newlines_to_space("a\r\nb")));
    assert_no_leak(|| black_box(matchers::find_word("x TOC", "TOC", true)));
    assert_no_leak(|| black_box(matchers::keep_ascii_digits("_x0000_s1026")));
}

#[test]
fn m1_js_value_of_every_shape() {
    // a value of each kind through the digest (arrays, maps, records, nullables, json values)
    let doc = support::sample_document();
    assert_no_leak(|| {
        let mut sink = BytesSink::default();
        let mut w = CanonWriter::new(&mut sink);
        doc.styles.canon(&mut w);
        doc.bookmarks.ends.canon(&mut w);
        uo_docx::js::JsonValue::Array(vec![uo_docx::js::JsonValue::Num(1.0), uo_docx::js::JsonValue::Str(Utf16::from("x"))]).canon(&mut w);
        black_box(sink)
    });
}
