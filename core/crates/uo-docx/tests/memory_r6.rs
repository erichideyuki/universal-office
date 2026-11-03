//! Memory gate M1 of frontier R6 (`read::sources`): every public function returns the live bytes
//! exactly to their value before the call, output dropped (`uo_alloc::assert_no_leak`), error and
//! no-position paths included; and the property the record asks of the source store (A2, F14):
//! it does not copy part text. The store keeps the XML reader's own buffer (a shared handle), the
//! records hold spans, so the bytes of a store depend on the number of paragraphs and runs, not on
//! how much text they hold. The numbers are printed (`--nocapture`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::sync::Arc;

use uo_alloc::{assert_no_leak, measure};
use uo_docx::ids::SourceKey;
use uo_docx::js::JsNumber;
use uo_docx::model::sources::{SourceStore, SrcText};
use uo_docx::model::table::{Cell, Row, Table};
use uo_docx::model::{Comment, Paragraph, Run, RunProps};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::sources::{
    ElSpan, ObjSide, PendingSeg, RunSide, capture_paragraph, cell_source, group_of, inline_wrapper, note_item, note_story_of,
    notes_part_of, open_of, row_source, story_of, table_source,
};
use uo_docx::read::{DOCX, DOCX_POS, ReadOptions};
use uo_opc::xml::{Source, parse};

fn pkg() -> uo_opc::Package {
    uo_opc::Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
}

fn run() -> Run {
    Run { props: Arc::new(RunProps { link: Some("l".into()), ..RunProps::default() }), ..Run::default() }
}

const XML: &str = concat!(
    "<w:document><w:body>",
    "<w:p w14:paraId=\"A\"><w:pPr><w:jc/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>é</w:t></w:r><w:hyperlink r:id=\"x\"><w:r><w:t>b</w:t></w:r></w:hyperlink><w:bookmarkStart/></w:p>",
    "<w:p/><w:p a=\"b\" />",
    "<w:sdt><w:sdtPr/><w:sdtContent><w:p/><w:sdt><w:sdtContent><w:tbl><w:tblPr/><w:tblGrid/><w:tr><w:trPr/><w:tc><w:tcPr/><w:p/></w:tc><w:tc/></w:tr></w:tbl></w:sdtContent></w:sdt></w:sdtContent></w:sdt>",
    "<w:customXml><w:customXmlPr/><w:p/></w:customXml>",
    "</w:body></w:document>",
);

#[test]
fn m1_every_function_returns_the_memory() {
    let doc = parse(XML.to_owned(), &DOCX_POS).unwrap();
    let plain = parse(XML.to_owned(), &DOCX).unwrap();
    let els: Vec<_> = std::iter::once(doc.root()).chain(doc.root().descendants()).collect();
    assert!(els.len() > 20);
    for el in &els {
        let el = *el;
        assert_no_leak(|| {
            let mut store = SourceStore::default();
            let pkg = pkg();
            let mut ctx = ReadCtx::new(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() });
            ctx.source_part("word/document.xml", &doc);
            let store_in_ctx = ctx.sources.take().unwrap_or_default();
            store.parts = store_in_ctx.parts.clone();
            let part = 0;
            black_box(open_of(&store, part, el));
            black_box(inline_wrapper(part, el));
            black_box(story_of(&store, part, el));
            black_box(note_story_of(part, el));
            black_box(notes_part_of(&store, part, el));
            black_box(note_item(part, el, JsNumber::Num(1.0), None));
            black_box(group_of(&store, part, el, &[SourceKey(1), SourceKey(0)]));
            black_box(table_source(&store, part, el, el.first_child("w:tblPr"), el.first_child("w:tblGrid")));
            black_box(row_source(&store, part, el, el.first_child("w:trPr"), None, Vec::new(), Vec::new()));
            black_box(cell_source(&store, part, el, el.first_child("w:tcPr"), Some(Vec::new()), Vec::new(), Vec::new()));
            let mut para = Paragraph { runs: vec![run(), run()], ..Paragraph::default() };
            let segs = vec![PendingSeg::new(part, el, 0, 1), PendingSeg::new(part, el, 1, 2)];
            let sides = [
                RunSide { rpr: ElSpan::of(el), obj: ObjSide::Head(ElSpan { start: 1, end: 3 }) },
                RunSide { rpr: None, obj: ObjSide::Follow },
            ];
            capture_paragraph(&mut store, part, &mut para, el, el.first_child("w:pPr"), segs, &sides);
            let mut cell = Cell::default();
            let mut row = Row::default();
            let mut table = Table::default();
            store.add_cell(&mut cell, Default::default());
            store.add_row(&mut row, Default::default());
            store.add_table(&mut table, Default::default());
            store.add_textbox_groups(vec![group_of(&store, part, el, &[])]);
            store.record_comments(&[Comment { id: JsNumber::Num(1.0), ..Comment::default() }]);
            let a = SrcText::Owned("x".into());
            black_box(store.concat(&a, &a));
            black_box((store, para, cell, row, table))
        });
        // without positions every function gives the empty text
        let pel = plain.root();
        assert_no_leak(|| {
            let store = SourceStore::default();
            black_box(open_of(&store, 0, pel));
            black_box(group_of(&store, 0, pel, &[SourceKey(1)]));
            black_box(story_of(&store, 0, pel));
        });
    }
}

#[test]
fn m1_the_store_does_not_copy_part_text() {
    let mut text = String::from("<w:document><w:body>");
    for i in 0..40_000 {
        text.push_str(&format!("<w:p><w:r><w:t>paragraph {i} with some text</w:t></w:r></w:p>"));
    }
    text.push_str("</w:body></w:document>");
    text.shrink_to_fit();
    let len = text.len();
    assert!(len > 2_000_000);
    let pkg = pkg();
    let ((store, handle), usage) = measure(|| {
        let doc = parse(text, &DOCX_POS).unwrap();
        let mut ctx = ReadCtx::new(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() });
        let idx = ctx.source_part("word/document.xml", &doc).unwrap();
        assert_eq!(idx, 0);
        let store = ctx.sources.take().unwrap();
        assert!(Source::ptr_eq(&store.parts[0].text, doc.source()), "the store holds the reader's buffer");
        let handle = doc.source().clone();
        drop(doc);
        (store, handle)
    });
    // the buffer was allocated before the measurement (the tree took it over) and is still alive
    // through the store: what the registration kept is a few handles, a copy would add `len`
    println!(
        "part {len} B; kept by the registration after dropping the tree: {} B (peak above start {} B)",
        usage.live_delta, usage.peak_above_start
    );
    assert!(usage.live_delta < 4096, "the store must not copy the text: kept {} B of a {len} B part", usage.live_delta);
    assert_eq!(store.parts[0].text.len(), len);
    drop((store, handle));
}

/// The store of a document of `n` paragraphs whose text is `pad` characters long: its heap bytes.
fn store_bytes(n: usize, pad: usize) -> (i64, usize) {
    let filler = "x".repeat(pad);
    let mut text = String::from("<w:document><w:body>");
    for i in 0..n {
        text.push_str(&format!("<w:p w14:paraId=\"{i}\"><w:pPr><w:jc/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>{filler}</w:t></w:r></w:p>"));
    }
    text.push_str("</w:body></w:document>");
    let doc = parse(text, &DOCX_POS).unwrap();
    let pkg = pkg();
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() });
    let part = ctx.source_part("word/document.xml", &doc).unwrap();
    let mut store = ctx.sources.take().unwrap();
    let text_len = doc.source().len();
    let (store, usage) = measure(|| {
        for p_el in doc.root().descendants().filter(|e| e.name() == "w:p") {
            let mut para = Paragraph { runs: vec![run()], ..Paragraph::default() };
            let kids: Vec<_> = p_el.child_elements().filter(|k| k.name() != "w:pPr").collect();
            let rpr = kids[0].first_child("w:rPr").and_then(ElSpan::of);
            let segs = vec![PendingSeg::new(part, kids[0], 0, 1)];
            capture_paragraph(&mut store, part, &mut para, p_el, p_el.first_child("w:pPr"), segs, &[RunSide { rpr, obj: ObjSide::None }]);
        }
        store
    });
    assert_eq!(store.paras.len(), n);
    black_box(&store);
    (usage.live_delta, text_len)
}

#[test]
fn m1_store_bytes_do_not_depend_on_the_text() {
    let n = 20_000;
    let (short, short_text) = store_bytes(n, 10);
    let (long, long_text) = store_bytes(n, 2000);
    println!(
        "store of {n} paragraphs: {short} B for {short_text} B of XML ({:.1} B per paragraph); {long} B for {long_text} B of XML ({:.1} B per paragraph)",
        short as f64 / n as f64,
        long as f64 / n as f64
    );
    assert!(long_text > 10 * short_text);
    let drift = (long - short).abs();
    assert!(
        drift * 100 < short,
        "a store of 2,000-character paragraphs differs by {drift} B from one of 10-character ones: text is copied"
    );
}
