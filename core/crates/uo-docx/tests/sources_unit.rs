//! Unit tests of frontier R6 (`read::sources`): the source building of the TS `source.ts`
//! (`id`, `addPara`/`addTable`/`addRow`/`addCell`/`addTextboxGroups`), `captureParagraph`,
//! `storyOf`/`groupOf`/`wrapOf`/`openOf` and the notes part pieces, on hand-written parts whose
//! expected strings were worked out from the TS code. The parity replay (every docx of the
//! corpus against the frozen TS) is `parity_sources.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use uo_docx::ids::{SourceKey, TxbxSrc};
use uo_docx::js::{JsNumber, JsObject, JsonWriter};
use uo_docx::model::Comment;
use uo_docx::model::sources::{SourceStore, SrcText, StoryGroup, WrapSeg};
use uo_docx::model::table::{Cell, Row, Table};
use uo_docx::model::{Paragraph, Run, RunProps};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::sources::{
    ObjSide, PendingSeg, RunSide, capture_paragraph, cell_source, el_span, group_of, inline_wrapper, note_item, note_story_of,
    notes_part_of, open_of, row_source, story_of, table_source, text_of, wrapper_spans,
};
use uo_docx::read::{DOCX_POS, ReadOptions};
use uo_opc::xml::{Document, Element, parse};

fn pkg() -> uo_opc::Package {
    uo_opc::Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default())
}

/// The part parsed with positions and registered in a fresh store (as `ReadCtx::source_part`).
struct Rig {
    doc: Document,
    store: SourceStore,
}

fn rig(xml: &str) -> Rig {
    let pkg = pkg();
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() });
    let doc = parse(xml.to_owned(), &DOCX_POS).unwrap();
    assert_eq!(ctx.source_part("word/document.xml", &doc), Some(0));
    Rig { doc, store: ctx.sources.take().unwrap() }
}

fn first<'d>(doc: &'d Document, name: &str) -> Element<'d> {
    doc.root().descendants().find(|e| e.name() == name).unwrap()
}

fn t(r: &Rig, text: &SrcText) -> String {
    text_of(&r.store, text).to_string()
}

fn run(link: Option<&str>) -> Run {
    let props = RunProps { link: link.map(Into::into), ..RunProps::default() };
    Run { props: Arc::new(props), ..Run::default() }
}

fn n(x: u32) -> JsNumber {
    JsNumber::Num(f64::from(x))
}

#[test]
fn ids_start_at_one() {
    let mut s = SourceStore::default();
    assert_eq!(s.next, 1);
    assert_eq!(s.id(), SourceKey(1));
    assert_eq!(s.id(), SourceKey(2));
    assert_eq!(s.next, 3);
}

#[test]
fn capture_paragraph_with_hyperlink_marker_and_object() {
    let xml = concat!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>"#,
        r#"<w:p w14:paraId="A1"><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>a</w:t></w:r>"#,
        r#"<w:hyperlink r:id="rId5"><w:r><w:t>é</w:t></w:r></w:hyperlink><w:bookmarkStart w:id="1" w:name="b"/>"#,
        r#"<w:r><w:drawing/></w:r></w:p></w:body></w:document>"#,
    );
    let mut r = rig(xml);
    let p_el = first(&r.doc, "w:p");
    let ppr = p_el.first_child("w:pPr");
    let kids: Vec<Element<'_>> = p_el.child_elements().filter(|k| k.name() != "w:pPr").collect();
    let run_el = kids[0];
    let rpr = run_el.first_child("w:rPr").unwrap();
    let link_el = kids[1];
    let draw_run = kids[3];
    let drawing = draw_run.first_child("w:drawing").unwrap();
    let mut para = Paragraph { runs: vec![run(None), run(Some("https://x")), run(None)], ..Paragraph::default() };
    let mut s0 = PendingSeg::new(0, run_el, 0, 1);
    s0.seg.sids.clear();
    let mut s1 = PendingSeg::new(0, link_el, 1, 2);
    let (open, close) = inline_wrapper(0, link_el).unwrap();
    s1.seg.wrap_open = Some(open);
    s1.seg.wrap_close = Some(close);
    let s2 = PendingSeg::new(0, kids[2], 2, 2);
    let s3 = PendingSeg::new(0, draw_run, 2, 3);
    let sides = [
        RunSide { rpr: uo_docx::read::sources::ElSpan::of(rpr), obj: ObjSide::None },
        RunSide::default(),
        RunSide { rpr: None, obj: ObjSide::Head(uo_docx::read::sources::ElSpan::of(drawing).unwrap()) },
    ];
    capture_paragraph(&mut r.store, 0, &mut para, p_el, ppr, vec![s0, s1, s2, s3], &sides);
    // run ids 1..=3, then the paragraph's
    assert_eq!(r.store.next, 5);
    let ps = r.store.paras.get(&SourceKey(4)).unwrap();
    assert_eq!(para.props.src, Some(SourceKey(4)));
    assert_eq!(para.runs[0].props.src, Some(SourceKey(1)));
    assert_eq!(para.runs[2].props.src, Some(SourceKey(3)));
    assert_eq!(t(&r, &ps.raw), &xml[xml.find("<w:p ").unwrap()..xml.find("</w:p>").unwrap() + 6]);
    assert_eq!(t(&r, &ps.open), r#"<w:p w14:paraId="A1">"#);
    assert_eq!(t(&r, ps.p_pr.as_ref().unwrap()), r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#);
    assert_eq!(ps.at, Some(u32::try_from(xml.find("<w:p ").unwrap()).unwrap()));
    assert_eq!(ps.segs.len(), 4);
    assert_eq!(ps.segs[0].sids, vec![n(1)]);
    assert_eq!(ps.segs[0].link, None);
    let h = &ps.segs[1];
    assert_eq!(h.sids, vec![n(2)]);
    assert_eq!(t(&r, h.wrap_open.as_ref().unwrap()), r#"<w:hyperlink r:id="rId5">"#);
    assert_eq!(t(&r, h.wrap_close.as_ref().unwrap()), "</w:hyperlink>");
    assert_eq!(h.link.as_deref(), Some("https://x"));
    assert_eq!(h.tip.as_deref(), Some(""));
    assert!(ps.segs[2].sids.is_empty());
    assert_eq!(ps.segs[3].sids, vec![n(3)]);
    assert_eq!(t(&r, r.store.runs.get(&SourceKey(1)).unwrap().r_pr.as_ref().unwrap()), "<w:rPr><w:b/></w:rPr>");
    assert_eq!(t(&r, r.store.runs.get(&SourceKey(3)).unwrap().obj.as_ref().unwrap()), "<w:drawing/>");
    assert_eq!(r.store.runs.get(&SourceKey(2)).unwrap().r_pr, None);
}

#[test]
fn capture_paragraph_self_closing_and_extra_runs_and_obj_head() {
    let xml = r#"<w:document><w:body><w:p a="b" /><w:p/></w:body></w:document>"#;
    let mut r = rig(xml);
    let ps: Vec<Element<'_>> = r.doc.root().descendants().filter(|e| e.name() == "w:p").collect();
    // `<w:p a="b" />`: the TS `slice(s, e).replace(/\s*\/>$/, '>')`
    let mut p0 = Paragraph::default();
    capture_paragraph(&mut r.store, 0, &mut p0, ps[0], None, Vec::new(), &[]);
    let src = r.store.paras.get(&SourceKey(1)).unwrap();
    assert_eq!(t(&r, &src.open), r#"<w:p a="b">"#);
    assert!(matches!(src.open, SrcText::Owned(_)));
    assert_eq!(t(&r, &src.raw), r#"<w:p a="b" />"#);
    assert_eq!(src.p_pr, None);
    // runs emitted by the end of the paragraph (a field that goes on) belong to the last segment: `open`
    let mut p1 = Paragraph { runs: vec![run(None), run(None), run(None), run(None)], ..Paragraph::default() };
    let seg = PendingSeg::new(0, ps[1], 0, 1);
    let sides = [
        RunSide { rpr: None, obj: ObjSide::Head(uo_docx::read::sources::ElSpan { start: 5, end: 9 }) },
        RunSide { rpr: None, obj: ObjSide::Follow },
        RunSide { rpr: None, obj: ObjSide::Follow },
        RunSide { rpr: None, obj: ObjSide::Head(uo_docx::read::sources::ElSpan { start: 1, end: 2 }) },
    ];
    capture_paragraph(&mut r.store, 0, &mut p1, ps[1], None, vec![seg], &sides);
    let seg = &r.store.paras.get(&SourceKey(6)).unwrap().segs[0];
    assert_eq!(seg.open, Some(true));
    assert_eq!(seg.sids, vec![n(2), n(3), n(4), n(5)]);
    assert_eq!(t(&r, &r.store.paras.get(&SourceKey(6)).unwrap().open), "<w:p>");
    let rs = |id| r.store.runs.get(&SourceKey(id)).unwrap();
    assert_eq!(t(&r, rs(2).obj.as_ref().unwrap()), "cume");
    assert_eq!(rs(3).obj_head, Some(n(2)));
    assert_eq!(rs(4).obj_head, Some(n(2)));
    assert_eq!(rs(5).obj_head, None);
    assert_eq!(p1.props.src, Some(SourceKey(6)));
}

#[test]
fn capture_survives_out_of_range_segments() {
    let mut r = rig("<w:document><w:body><w:p><w:r/></w:p></w:body></w:document>");
    let p_el = first(&r.doc, "w:p");
    let mut para = Paragraph { runs: vec![run(None)], ..Paragraph::default() };
    let segs = vec![
        PendingSeg { from: 5, to: 2, ..PendingSeg::new(0, p_el, 0, 0) },
        PendingSeg { from: 0, to: 99, ..PendingSeg::new(0, p_el, 0, 0) },
        PendingSeg { from: usize::MAX - 1, to: usize::MAX, ..PendingSeg::new(0, p_el, 0, 0) },
    ];
    capture_paragraph(&mut r.store, 9, &mut para, p_el, None, segs, &[]);
    // the segment that asked for run 0..99 got the one run that exists; part 9 does not exist: empty text
    assert_eq!(r.store.paras.len(), 1);
    assert_eq!(r.store.runs.len(), 1);
}

#[test]
fn inline_wrapper_rules() {
    let xml = concat!(
        "<w:document><w:body><w:p>",
        "<w:sdt><w:sdtPr><w:alias/></w:sdtPr><w:sdtContent><w:r/></w:sdtContent></w:sdt>",
        "<w:sdt><w:sdtPr/></w:sdt>",
        "<w:sdt><w:sdtContent/></w:sdt>",
        r#"<w:customXml w:uri="u"><w:customXmlPr><w:attr/></w:customXmlPr><w:r/></w:customXml>"#,
        "<w:smartTag/>",
        "<w:bdo><w:r/></w:bdo>",
        "<w:r><w:t/></w:r>",
        "</w:p></w:body></w:document>",
    );
    let r = rig(xml);
    let kids: Vec<Element<'_>> = first(&r.doc, "w:p").child_elements().collect();
    let w = |i: usize| inline_wrapper(0, kids[i]).map(|(a, b)| (t(&r, &a), t(&r, &b)));
    assert_eq!(w(0), Some(("<w:sdt><w:sdtPr><w:alias/></w:sdtPr><w:sdtContent>".into(), "</w:sdtContent></w:sdt>".into())));
    assert_eq!(w(1), None, "no w:sdtContent");
    assert_eq!(w(2), None, "empty content");
    assert_eq!(w(3), Some((r#"<w:customXml w:uri="u"><w:customXmlPr><w:attr/></w:customXmlPr>"#.into(), "</w:customXml>".into())));
    assert_eq!(w(4), None, "self-closing");
    assert_eq!(w(5), Some(("<w:bdo>".into(), "</w:bdo>".into())));
    assert_eq!(w(6), None, "not a wrapper");
    assert!(wrapper_spans(0, kids[6], kids[6]).is_some());
}

#[test]
fn concat_joins_adjacent_spans_and_builds_the_rest() {
    let r = rig("<w:document><w:body><w:p><w:r/> <w:r/><w:r/></w:p></w:body></w:document>");
    let kids: Vec<Element<'_>> = first(&r.doc, "w:p").child_elements().collect();
    let (a, b, c) = (el_span(0, kids[0]), el_span(0, kids[1]), el_span(0, kids[2]));
    let ab = r.store.concat(&a, &b);
    assert!(matches!(ab, SrcText::Owned(_)), "a space between: not adjacent");
    assert_eq!(t(&r, &ab), "<w:r/><w:r/>");
    let bc = r.store.concat(&b, &c);
    assert!(matches!(bc, SrcText::Span { .. }));
    assert_eq!(t(&r, &bc), "<w:r/><w:r/>");
    let abc = r.store.concat(&ab, &c);
    assert_eq!(t(&r, &abc), "<w:r/><w:r/><w:r/>");
}

#[test]
fn story_and_notes_pieces() {
    let xml = "<?xml version=\"1.0\"?>\n<w:footnotes><w:footnote w:id=\"1\" w:type=\"separator\"><w:p/></w:footnote><w:footnote w:id=\"2\"><w:p><w:r/></w:p></w:footnote></w:footnotes>\n";
    let r = rig(xml);
    let root = r.doc.root();
    let st = story_of(&r.store, 0, root);
    assert_eq!(t(&r, &st.open), "<?xml version=\"1.0\"?>\n<w:footnotes>");
    assert_eq!(t(&r, &st.close), "</w:footnotes>\n");
    let part = notes_part_of(&r.store, 0, root);
    assert_eq!(part.open, st.open);
    assert!(part.items.is_empty() && part.refs_at_load.0.is_empty());
    let notes: Vec<Element<'_>> = root.child_elements().collect();
    let sep = note_item(0, notes[0], n(1), None);
    assert_eq!(sep.separator, Some(true));
    assert!(sep.story.is_none());
    assert_eq!(t(&r, &sep.raw), r#"<w:footnote w:id="1" w:type="separator"><w:p/></w:footnote>"#);
    let story = note_story_of(0, notes[1]);
    assert_eq!(t(&r, &story.open), r#"<w:footnote w:id="2">"#);
    assert_eq!(t(&r, &story.close), "</w:footnote>");
    let item = note_item(0, notes[1], n(2), Some(story));
    assert_eq!(item.separator, None);
    assert!(item.story.is_some());
}

#[test]
fn group_of_plain_child_and_nested_sdt() {
    let xml = concat!(
        "<w:document><w:body>",
        "<w:p><w:r/></w:p>",
        "<w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr><w:sdtContent>",
        "<w:p/><w:sdt><w:sdtContent><w:p><w:r/></w:p><w:tbl/></w:sdtContent></w:sdt><w:p/>",
        "</w:sdtContent></w:sdt>",
        "<w:customXml><w:customXmlPr/><w:p/></w:customXml>",
        "<w:sectPr/>",
        "</w:body></w:document>",
    );
    let mut r = rig(xml);
    let body = first(&r.doc, "w:body");
    let kids: Vec<Element<'_>> = body.child_elements().collect();
    // register one paragraph or table source per block, at its position, in document order
    let mut ids = Vec::new();
    for (i, e) in r.doc.root().descendants().filter(|e| e.name() == "w:p" || e.name() == "w:tbl").enumerate() {
        let id = r.store.id();
        let at = u32::try_from(e.positions().unwrap().start).unwrap();
        if e.name() == "w:p" {
            r.store.paras.insert(id, uo_docx::model::sources::ParaSource { at: Some(at), ..Default::default() });
        } else {
            r.store.tables.insert(id, uo_docx::model::sources::TableSource { at: Some(at), ..Default::default() });
        }
        ids.push(id);
        assert_eq!(r.store.at_of(id), Some(at), "block {i}");
    }
    assert_eq!(ids.len(), 6);
    // the plain paragraph
    let g = group_of(&r.store, 0, kids[0], &ids[0..1]);
    assert_eq!(t(&r, &g.raw), "<w:p><w:r/></w:p>");
    assert_eq!(g.blocks, vec![n(1)]);
    assert!(g.wrap_open.is_none() && g.inner.is_none());
    // the sdt: three inner groups; the nested sdt carries two blocks
    let g = group_of(&r.store, 0, kids[1], &ids[1..5]);
    assert_eq!(t(&r, g.wrap_open.as_ref().unwrap()), "<w:sdt><w:sdtPr><w:docPartObj/></w:sdtPr><w:sdtContent>");
    assert_eq!(t(&r, g.wrap_close.as_ref().unwrap()), "</w:sdtContent></w:sdt>");
    let inner = g.inner.as_ref().unwrap();
    assert_eq!(inner.len(), 3);
    assert_eq!(inner[0].blocks, vec![n(2)]);
    assert_eq!(inner[1].blocks, vec![n(3), n(4)]);
    assert_eq!(inner[2].blocks, vec![n(5)]);
    let nested = inner[1].inner.as_ref().unwrap();
    assert_eq!(nested.len(), 2);
    assert_eq!((nested[0].blocks.clone(), nested[1].blocks.clone()), (vec![n(3)], vec![n(4)]));
    assert_eq!(t(&r, &nested[1].raw), "<w:tbl/>");
    assert!(nested[0].inner.is_none());
    // customXml: the content is the element, minus the leading *Pr; `customXmlPr` is not an inner group
    let g = group_of(&r.store, 0, kids[2], &ids[5..6]);
    assert_eq!(t(&r, g.wrap_open.as_ref().unwrap()), "<w:customXml><w:customXmlPr/>");
    assert_eq!(g.inner.as_ref().unwrap().len(), 1);
    // a w:sectPr and a wrapper with nothing in it
    let g = group_of(&r.store, 0, kids[3], &[]);
    assert!(g.blocks.is_empty() && g.inner.is_none());
}

#[test]
fn group_of_keeps_an_empty_inner_list() {
    let r = rig("<w:document><w:body><w:sdt><w:sdtContent><w:sdtPr/></w:sdtContent></w:sdt></w:body></w:document>");
    let g = group_of(&r.store, 0, first(&r.doc, "w:sdt"), &[SourceKey(0), SourceKey(7)]);
    assert_eq!(g.blocks, vec![n(0), n(7)]);
    assert_eq!(g.inner, Some(Vec::new()), "`inner: []` is a key the TS writes");
}

/// The TS `groupOf`, recursive and with the `filter`, as the oracle of the iterative one.
fn naive(store: &SourceStore, child: Element<'_>, produced: &[SourceKey]) -> StoryGroup {
    let mut g = StoryGroup {
        raw: el_span(0, child),
        blocks: produced.iter().map(|k| JsNumber::Num(f64::from(k.0))).collect(),
        ..StoryGroup::default()
    };
    let content = match child.name() {
        "w:sdt" => child.first_child("w:sdtContent"),
        "w:customXml" => Some(child),
        _ => None,
    };
    if let Some(content) = content
        && let Some((open, close)) = wrapper_spans(0, child, content)
    {
        g.wrap_open = Some(open);
        g.wrap_close = Some(close);
        g.inner = Some(
            content
                .child_elements()
                .filter(|k| !k.name().ends_with("Pr"))
                .map(|k| {
                    let p = k.positions().unwrap();
                    let inside: Vec<SourceKey> = produced
                        .iter()
                        .copied()
                        .filter(|id| store.at_of(*id).is_some_and(|a| (a as usize) >= p.start && (a as usize) < p.end))
                        .collect();
                    naive(store, k, &inside)
                })
                .collect(),
        );
    }
    g
}

#[test]
fn group_of_equals_the_naive_filter_on_random_trees_and_orders() {
    let mut seed = 0x6d2b_79f5_u32;
    let mut next = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as usize) % n.max(1)
    };
    fn gen_blocks(depth: u32, next: &mut dyn FnMut(usize) -> usize, out: &mut String) {
        for _ in 0..(1 + next(4)) {
            match next(if depth > 3 { 2 } else { 4 }) {
                0 => out.push_str("<w:p><w:r/></w:p>"),
                1 => out.push_str("<w:tbl/>"),
                2 => {
                    out.push_str("<w:sdt><w:sdtPr/><w:sdtContent>");
                    gen_blocks(depth + 1, next, out);
                    out.push_str("</w:sdtContent></w:sdt>");
                }
                _ => {
                    out.push_str("<w:customXml><w:customXmlPr/>");
                    gen_blocks(depth + 1, next, out);
                    out.push_str("</w:customXml>");
                }
            }
        }
    }
    for round in 0..300 {
        let mut body = String::new();
        gen_blocks(0, &mut next, &mut body);
        let xml = format!("<w:document><w:body>{body}</w:body></w:document>");
        let mut r = rig(&xml);
        let mut ids: Vec<SourceKey> = Vec::new();
        for e in r.doc.root().descendants().filter(|e| e.name() == "w:p" || e.name() == "w:tbl") {
            let id = r.store.id();
            let at = Some(u32::try_from(e.positions().unwrap().start).unwrap());
            r.store.paras.insert(id, uo_docx::model::sources::ParaSource { at, ..Default::default() });
            ids.push(id);
        }
        // every few rounds the blocks come out of document order and with ids that have no source
        match round % 3 {
            1 => ids.reverse(),
            2 => {
                for i in 0..ids.len() {
                    let j = next(ids.len());
                    ids.swap(i, j);
                }
                ids.push(SourceKey(0));
                ids.push(SourceKey(9999));
            }
            _ => {}
        }
        for kid in first(&r.doc, "w:body").child_elements() {
            let want = naive(&r.store, kid, &ids);
            assert_eq!(group_of(&r.store, 0, kid, &ids), want, "round {round}");
        }
    }
}

#[test]
fn deep_sdt_nesting_does_not_recurse() {
    let depth = 400;
    let xml = format!(
        "<w:document><w:body>{}<w:p/>{}</w:body></w:document>",
        "<w:sdt><w:sdtContent>".repeat(depth),
        "</w:sdtContent></w:sdt>".repeat(depth)
    );
    let mut r = rig(&xml);
    let id = r.store.id();
    let at = u32::try_from(first(&r.doc, "w:p").positions().unwrap().start).unwrap();
    r.store.paras.insert(id, uo_docx::model::sources::ParaSource { at: Some(at), ..Default::default() });
    let mut g = group_of(&r.store, 0, first(&r.doc, "w:sdt"), &[id]);
    let mut levels = 0;
    while let Some(inner) = g.inner.take() {
        levels += 1;
        assert_eq!(g.blocks, vec![n(1)]);
        match inner.into_iter().next() {
            Some(x) => g = x,
            None => break,
        }
    }
    assert_eq!(levels, depth);
}

#[test]
fn table_row_cell_sources_and_the_add_functions() {
    let xml = concat!(
        "<w:document><w:body>",
        r#"<w:tbl a="1"><w:tblPr><w:tblW/></w:tblPr><w:tblGrid><w:gridCol w:w="10"/></w:tblGrid>"#,
        r#"<w:tr w14:paraId="T"><w:trPr><w:cantSplit/></w:trPr><w:tblPrEx/><w:tc><w:tcPr><w:tcW/></w:tcPr><w:p/></w:tc><w:tc /></w:tr>"#,
        "</w:tbl></w:body></w:document>",
    );
    let mut r = rig(xml);
    let tbl = first(&r.doc, "w:tbl");
    let tr = first(&r.doc, "w:tr");
    let tcs: Vec<Element<'_>> = tr.children_named("w:tc").collect();
    let wrap = vec![WrapSeg { raw: SrcText::Owned("<!--x-->".into()), ..WrapSeg::default() }];
    let cs0 = cell_source(&r.store, 0, tcs[0], tcs[0].first_child("w:tcPr"), None, Vec::new(), wrap.clone());
    assert_eq!(t(&r, &cs0.open), "<w:tc>");
    assert_eq!(t(&r, cs0.tc_pr.as_ref().unwrap()), "<w:tcPr><w:tcW/></w:tcPr>");
    assert!(cs0.pre.is_none() && cs0.groups.is_none());
    assert_eq!(cs0.post, Some(wrap.clone()));
    let cs1 = cell_source(&r.store, 0, tcs[1], None, Some(Vec::new()), wrap.clone(), Vec::new());
    assert_eq!(t(&r, &cs1.open), "<w:tc>");
    assert_eq!(cs1.pre, Some(wrap));
    assert_eq!(cs1.groups, Some(Vec::new()));
    let rs = row_source(&r.store, 0, tr, tr.first_child("w:trPr"), tr.first_child("w:tblPrEx"), Vec::new(), Vec::new());
    assert_eq!(t(&r, &rs.open), r#"<w:tr w14:paraId="T">"#);
    assert_eq!(t(&r, rs.tr_pr.as_ref().unwrap()), "<w:trPr><w:cantSplit/></w:trPr>");
    assert_eq!(t(&r, rs.tbl_pr_ex.as_ref().unwrap()), "<w:tblPrEx/>");
    let ts = table_source(&r.store, 0, tbl, tbl.first_child("w:tblPr"), tbl.first_child("w:tblGrid"));
    assert_eq!(t(&r, &ts.open), r#"<w:tbl a="1">"#);
    assert!(matches!(ts.open, SrcText::Span { .. }), "an open tag unchanged by the regex stays a span");
    assert_eq!(t(&r, ts.tbl_grid.as_ref().unwrap()), r#"<w:tblGrid><w:gridCol w:w="10"/></w:tblGrid>"#);
    assert_eq!(ts.at, Some(u32::try_from(xml.find("<w:tbl ").unwrap()).unwrap()));
    assert_eq!(t(&r, &ts.raw), &xml[xml.find("<w:tbl ").unwrap()..xml.find("</w:tbl>").unwrap() + 8]);
    // `openOf` applies the regex to every element: a self-closing cell
    assert_eq!(t(&r, &open_of(&r.store, 0, tcs[1])), "<w:tc>");
    assert!(matches!(open_of(&r.store, 0, tcs[1]), SrcText::Owned(_)));

    let mut cell = Cell::default();
    let mut row = Row::default();
    let mut table = Table::default();
    assert_eq!(r.store.add_cell(&mut cell, cs0), SourceKey(1));
    assert_eq!(r.store.add_row(&mut row, rs), SourceKey(2));
    assert_eq!(r.store.add_table(&mut table, ts), SourceKey(3));
    assert_eq!((cell.src, row.src, table.src), (Some(SourceKey(1)), Some(SourceKey(2)), Some(SourceKey(3))));
    assert!(r.store.tcs.contains_key(&SourceKey(1)) && r.store.trs.contains_key(&SourceKey(2)));
    assert_eq!(r.store.at_of(SourceKey(3)), table.src.and_then(|k| r.store.at_of(k)));
    assert!(r.store.at_of(SourceKey(1)).is_none(), "a cell has no position");
    assert_eq!(r.store.add_textbox_groups(vec![]), TxbxSrc(0));
    assert_eq!(r.store.add_textbox_groups(vec![StoryGroup::default()]), TxbxSrc(1));
}

#[test]
fn src_key_goes_last_when_the_order_is_recorded() {
    let mut s = SourceStore::default();
    let mut ordered = Paragraph::default();
    {
        let p = Arc::make_mut(&mut ordered.props);
        p.alignment = Some("center".into());
        p.mark("styleId");
        p.mark("alignment");
    }
    s.add_para(&mut ordered, Default::default());
    let rec: Vec<&str> =
        ordered.props.key_order().recorded().iter().map(|i| <uo_docx::model::ParaProps as JsObject>::keys()[usize::from(*i)]).collect();
    assert_eq!(rec, ["styleId", "alignment", "src"]);
    // in declaration order nothing is recorded (src is declared after what the reader assigns)
    let mut plain = Paragraph::default();
    s.add_para(&mut plain, Default::default());
    assert!(plain.props.key_order().is_empty());
}

#[test]
fn comments_are_recorded_by_id_and_json() {
    let mut s = SourceStore::default();
    let comments = [
        Comment { id: JsNumber::Num(2.0), author: Some("a\"b".into()), text: "x\ny".into(), ..Comment::default() },
        Comment { id: JsNumber::Num(f64::NAN), author: None, text: "".into(), ..Comment::default() },
    ];
    s.record_comments(&comments);
    assert_eq!(s.comment_ids.0.len(), 2);
    let want = JsonWriter::stringify(&comments.to_vec(), None).unwrap();
    assert_eq!(s.comments_sig.as_deref(), Some(want.as_str()));
    s.record_comments(&[]);
    assert!(s.comment_ids.0.is_empty());
    assert_eq!(s.comments_sig.as_deref(), Some("[]"));
}

#[test]
fn elements_without_positions_give_empty_text() {
    let doc = parse("<w:p/>".to_owned(), &uo_docx::read::DOCX).unwrap();
    let mut store = SourceStore::default();
    assert_eq!(el_span(0, doc.root()), SrcText::default());
    assert_eq!(open_of(&store, 0, doc.root()), SrcText::default());
    assert!(inline_wrapper(0, doc.root()).is_none());
    assert_eq!(story_of(&store, 0, doc.root()), Default::default());
    assert_eq!(note_story_of(0, doc.root()), Default::default());
    let g = group_of(&store, 0, doc.root(), &[SourceKey(1)]);
    assert_eq!(g.raw, SrcText::default());
    let mut para = Paragraph { runs: vec![run(None)], ..Paragraph::default() };
    capture_paragraph(&mut store, 0, &mut para, doc.root(), None, vec![PendingSeg::new(0, doc.root(), 0, 1)], &[]);
    let ps = store.paras.values().next().unwrap();
    assert_eq!(ps.at, None);
    assert_eq!(ps.raw, SrcText::default());
}

#[test]
fn group_of_scales_with_blocks_not_with_blocks_times_children() {
    let n = 60_000;
    let xml = format!("<w:document><w:body><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt></w:body></w:document>", "<w:p/>".repeat(n));
    let mut r = rig(&xml);
    let mut ids = Vec::new();
    for e in r.doc.root().descendants().filter(|e| e.name() == "w:p") {
        let id = r.store.id();
        let at = Some(u32::try_from(e.positions().unwrap().start).unwrap());
        r.store.paras.insert(id, uo_docx::model::sources::ParaSource { at, ..Default::default() });
        ids.push(id);
    }
    let started = std::time::Instant::now();
    let g = group_of(&r.store, 0, first(&r.doc, "w:sdt"), &ids);
    let inner = g.inner.unwrap();
    assert_eq!(inner.len(), n);
    assert!(inner.iter().zip(&ids).all(|(g, id)| g.blocks == vec![n_of(*id)]));
    // the TS filter is O(children x blocks): 3.6e9 comparisons here; the pointer walk is linear
    assert!(started.elapsed().as_secs() < 5, "group_of took {:?}", started.elapsed());
}

fn n_of(id: SourceKey) -> JsNumber {
    JsNumber::Num(f64::from(id.0))
}
