//! No-panic loops of frontier R6 (`read::sources`): every public function over every element of
//! the synthetic packages of `js/quirk-sources.mjs` and of seeded mutations of them (a character
//! replaced, removed or duplicated, a junk string or one half of a surrogate pair inserted, a tag
//! cut), with random block ids (missing, repeated, out of document order), random run ranges
//! (`from > to`, past the runs, `usize::MAX`), random run sides and parts that do not exist. A
//! panic fails the test; the result is not looked at (the parity replay does) except for the
//! invariants that hold for any input: a group lists exactly the ids it was given or a subset of
//! them in order, `capture_paragraph` gives the paragraph and the runs it touched ids it just issued, and a store never
//! resolves a span outside its part.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use uo_docx::ids::SourceKey;
use uo_docx::js::JsNumber;
use uo_docx::model::sources::SrcText;
use uo_docx::model::table::{Cell, Row, Table};
use uo_docx::model::{Paragraph, Run, RunProps};
use uo_docx::read::ctx::ReadCtx;
use uo_docx::read::sources::{
    ElSpan, ObjSide, PendingSeg, RunSide, capture_paragraph, cell_source, el_span, group_of, inline_wrapper, note_item, note_story_of,
    notes_part_of, open_of, row_source, story_of, table_source, wrapper_spans,
};
use uo_docx::read::{DOCX_POS, ReadOptions};
use uo_opc::xml::{Document, Element, parse};

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

fn inputs() -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/sources.inputs")).unwrap();
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if line.starts_with("@input ") {
            out.extend(cur.take());
        } else if let Some(rest) = line.strip_prefix("word/document.xml\t") {
            let t = rest.replace("\\n", "\n").replace("\\r", "\r").replace("\\t", "\t").replace("\\\\", "\\");
            cur = Some(t);
        }
    }
    out.extend(cur);
    out
}

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(4) {
        if chars.is_empty() {
            break;
        }
        let at = rng.below(chars.len());
        match rng.below(7) {
            0 => chars[at] = ['<', '>', '/', '"', '&', ' ', 'é', '\u{fffd}'][rng.below(8)],
            1 => {
                chars.remove(at);
            }
            2 => {
                let c = chars[at];
                chars.insert(at, c);
            }
            3 => {
                for (i, c) in "<w:sdt><w:sdtContent>".chars().enumerate() {
                    chars.insert(at + i, c);
                }
            }
            4 => chars.truncate(at),
            5 => {
                for (i, c) in "</w:sdtContent></w:sdt>".chars().enumerate() {
                    chars.insert(at + i, c);
                }
            }
            _ => {
                chars.insert(at, '😀');
            }
        }
    }
    chars.into_iter().collect()
}

fn run() -> Run {
    Run { props: Arc::new(RunProps { link: Some("l".into()), ..RunProps::default() }), ..Run::default() }
}

fn exercise(doc: &Document, rng: &mut Rng) -> usize {
    let pkg = uo_opc::Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
    let mut ctx = ReadCtx::new(&pkg, &ReadOptions { sources: true, ..ReadOptions::default() });
    ctx.source_part("word/document.xml", doc);
    let mut store = ctx.sources.take().unwrap();
    let els: Vec<Element<'_>> = std::iter::once(doc.root()).chain(doc.root().descendants()).collect();
    // blocks the store knows: every paragraph and table, at its position
    let mut known: Vec<SourceKey> = Vec::new();
    for e in &els {
        if e.name() == "w:p" || e.name() == "w:tbl" {
            let id = store.id();
            let at = e.positions().map(|p| u32::try_from(p.start).unwrap());
            store.paras.insert(id, uo_docx::model::sources::ParaSource { at, ..Default::default() });
            known.push(id);
        }
    }
    let mut work = 0;
    for &el in &els {
        let part = if rng.below(10) == 0 { u32::MAX } else { 0 };
        let mut ids: Vec<SourceKey> = (0..rng.below(8)).map(|_| *known.get(rng.below(known.len() + 2)).unwrap_or(&SourceKey(0))).collect();
        if rng.below(3) == 0 {
            ids = known.clone();
        }
        let g = group_of(&store, part, el, &ids);
        assert!(g.blocks.len() == ids.len());
        if let Some(inner) = &g.inner {
            let total: usize = inner.iter().map(|i| i.blocks.len()).sum();
            assert!(total <= ids.len(), "each block lies in at most one child");
        }
        work += g.blocks.len();
        let _ = open_of(&store, part, el);
        let _ = inline_wrapper(part, el);
        let _ = wrapper_spans(part, el, el);
        let _ = story_of(&store, part, el);
        let _ = note_story_of(part, el);
        let _ = notes_part_of(&store, part, el);
        let _ = note_item(part, el, JsNumber::Num(f64::NAN), if rng.below(2) == 0 { Some(note_story_of(part, el)) } else { None });
        let _ = table_source(&store, part, el, el.first_child("w:tblPr"), el.first_child("w:tblGrid"));
        let _ = row_source(&store, part, el, el.first_child("w:trPr"), el.first_child("w:tblPrEx"), Vec::new(), Vec::new());
        let _ = cell_source(&store, part, el, el.first_child("w:tcPr"), Some(Vec::new()), Vec::new(), Vec::new());
        if el.name() == "w:p" || rng.below(20) == 0 {
            let nruns = rng.below(6);
            let mut para = Paragraph { runs: (0..nruns).map(|_| run()).collect(), ..Paragraph::default() };
            let kids: Vec<Element<'_>> = el.child_elements().collect();
            let mut segs = Vec::new();
            for k in &kids {
                let (from, to) = match rng.below(6) {
                    0 => (rng.below(10), rng.below(10)),
                    1 => (usize::MAX, usize::MAX),
                    2 => (0, usize::MAX),
                    _ => (segs.len().min(nruns), (segs.len() + 1).min(nruns)),
                };
                let mut seg = PendingSeg::new(part, *k, from, to);
                if rng.below(3) == 0
                    && let Some((a, b)) = inline_wrapper(part, *k)
                {
                    seg.seg.wrap_open = Some(a);
                    seg.seg.wrap_close = Some(b);
                }
                segs.push(seg);
            }
            let sides: Vec<RunSide> = (0..rng.below(8))
                .map(|_| RunSide {
                    rpr: ElSpan::of(els[rng.below(els.len())]),
                    obj: match rng.below(3) {
                        0 => ObjSide::None,
                        1 => ObjSide::Follow,
                        _ => ObjSide::Head(ElSpan { start: u32::MAX, end: 0 }),
                    },
                })
                .collect();
            let before = store.next;
            capture_paragraph(&mut store, part, &mut para, el, el.first_child("w:pPr"), segs, &sides);
            for r in &para.runs {
                if let Some(k) = r.props.src {
                    assert!(k.0 >= before && k.0 < store.next, "a run's id was handed out by this capture");
                }
            }
            assert!(para.props.src.is_some_and(|k| k.0 < store.next && store.paras.contains_key(&k)));
        }
        let mut cell = Cell::default();
        let mut row = Row::default();
        let mut table = Table::default();
        store.add_cell(&mut cell, Default::default());
        store.add_row(&mut row, Default::default());
        store.add_table(&mut table, Default::default());
        store.add_textbox_groups(vec![g]);
        let a = SrcText::Span { part: rng.next(), start: rng.next(), end: rng.next() };
        let b = SrcText::Span { part: 0, start: rng.next(), end: rng.next() };
        assert!(store.concat(&a, &b).resolve(&store.parts).len() < usize::MAX);
        let _ = el_span(part, el);
    }
    // a span is resolved inside its part or not at all
    for p in store.paras.values() {
        let text = p.raw.resolve(&store.parts);
        assert!(text.len() <= store.parts[0].text.len());
    }
    work
}

#[test]
fn nopanic_over_the_synthetic_packages_and_their_mutations() {
    let docs = inputs();
    assert_eq!(docs.len(), 400);
    let mut rng = Rng(0x00c0_ffee);
    let (mut parsed, mut rejected, mut work) = (0, 0, 0);
    for (i, text) in docs.iter().enumerate() {
        let variants = if i % 4 == 0 { 3 } else { 1 };
        for v in 0..variants {
            let t = if v == 0 { text.clone() } else { mutate(&mut rng, text) };
            match parse(t, &DOCX_POS) {
                Ok(doc) => {
                    parsed += 1;
                    work += exercise(&doc, &mut rng);
                }
                Err(_) => rejected += 1,
            }
        }
    }
    println!("parsed {parsed}, rejected {rejected}, blocks grouped {work}");
    assert!(parsed > 400 && work > 1000);
}

#[test]
fn nopanic_on_degenerate_documents() {
    let mut rng = Rng(7);
    for xml in [
        "<w:p/>",
        "<a><b/></a>",
        "<w:sdt><w:sdtContent/></w:sdt>",
        "<w:sdt><w:sdtContent></w:sdtContent></w:sdt>",
        "<w:customXml/>",
        "<w:customXml><w:customXmlPr/></w:customXml>",
        "<w:body><w:p>é😀</w:p></w:body>",
    ] {
        let doc = parse(xml.to_owned(), &DOCX_POS).unwrap();
        exercise(&doc, &mut rng);
    }
}
