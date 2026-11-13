//! F8 (the editor-facing API and the layout oracle): memory (M1 per query, M3: the hit tests
//! allocate nothing), no panic with hostile positions, scopes and refs, the `LayoutOracle` over a
//! uo-docx `Session`, `missing_faces`, the `"font budget"` warning and the views. Over the
//! committed F8 fixture (`tests/captures/f8`, the TS answers of its transcripts).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{FontIndex, Transcript, TranscriptProvider, load_answers};
use uo_alloc::{assert_no_leak, measure};
use uo_docx::view::LayoutOracle;
use uo_docx_layout::fonts::{FontAnswer, FontProvider};
use uo_docx_layout::hit::{CharsView, EditLines, EditPos, EditScope, LineIndex, SelRect, caret_x_in, cell_at, offset_at_x};
use uo_docx_layout::hyphenate::HyphenSessions;
use uo_docx_layout::model::{
    BlockPath, DocRef, DocView, DrawingId, FieldScope, HfPartId, HfWhich, HyphenSkips, NoteKind, NumKey, StoryRef,
};
use uo_docx_layout::options::{Clock, TimeZoneRule, ViewTransform};
use uo_docx_layout::oracle::EngineOracle;
use uo_docx_layout::output::{HfSlot, LineRef, LineStory, RefPath};
use uo_docx_layout::refs::{Locator, ParaLoc};
use uo_docx_layout::run_boxes::{RunBox, RunFilter};
use uo_docx_layout::{Layout, LayoutEngine, LayoutOptions};
use uo_testkit::generate::Rng;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/captures/f8")
}

fn hyph_registry() -> Arc<uo_fonts::hyphen::HyphenationRegistry> {
    let mut reg = uo_fonts::hyphen::HyphenationRegistry::new();
    for id in uo_fonts::hyphen::HYPHENATION_PATTERN_IDS {
        if let Some(p) = uo_fonts::embedded_patterns(id) {
            reg.register(&p);
        }
    }
    Arc::new(reg)
}

fn opts() -> LayoutOptions {
    LayoutOptions {
        clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } },
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    }
}

/// The fixture documents with their transcript providers.
fn fixture() -> Vec<(String, Vec<u8>, TranscriptProvider)> {
    let d = dir();
    let mut fonts = FontIndex::open_fonts();
    let mut out = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(d.join("docs")).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    names.sort();
    for p in names {
        let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
        let tr = Transcript::read(&d.join(format!("{stem}.transcript.tsv"))).unwrap();
        fonts.add_transcript_sources(&tr);
        let loaded = load_answers(&tr, &fonts);
        assert!(loaded.missing.is_empty());
        out.push((stem, std::fs::read(&p).unwrap(), TranscriptProvider::new(&tr, loaded)));
    }
    out
}

fn read(docx: &[u8]) -> uo_docx::model::Document {
    let pkg = uo_opc::Package::open(docx, &uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default()).unwrap();
    uo_docx::read::read_document(&pkg, &uo_docx::read::ReadOptions::default()).unwrap()
}

fn layout(view: DocView<'_>, p: &dyn FontProvider, o: &LayoutOptions) -> Layout {
    let ldoc = uo_docx_layout::derive::build(view, o).unwrap();
    let mut hyph = HyphenSessions::new(hyph_registry());
    uo_docx_layout::engine::layout_document(&ldoc, o, p, &mut hyph).unwrap()
}

/// Every scope the fixture document offers (as the P4 facet builds them).
fn scopes(view: DocView<'_>, l: &Layout) -> Vec<(EditScopeOwned, bool)> {
    let mut out = vec![(EditScopeOwned::Body, false)];
    for (bi, b) in view.body().blocks().enumerate() {
        if let uo_docx_layout::model::Block::Table(t) = b {
            for (r, row) in t.rows.iter().enumerate() {
                for k in 0..row.cells.len() {
                    out.push((EditScopeOwned::Cell(BlockPath::root(bi as u32), r as u32, k as u32), false));
                }
            }
        }
    }
    for s in view.sections() {
        for (set, which) in [(&s.headers, HfWhich::Header), (&s.footers, HfWhich::Footer)] {
            for (_, &p) in set.iter() {
                out.push((EditScopeOwned::Hf(which, p, 0), true));
                out.push((EditScopeOwned::Hf(which, p, l.pages.len() as u32 + 3), true));
            }
        }
    }
    for id in view.notes().footnotes.keys() {
        out.push((EditScopeOwned::Note(NoteKind::Footnote, *id), false));
    }
    for p in &l.pages {
        for o in p.objects.as_deref().unwrap_or(&[]) {
            if let Some(d) = o.drawing {
                out.push((EditScopeOwned::Tb(vec![d], None), false));
                out.push((EditScopeOwned::Tb(vec![d, DrawingId(u32::MAX)], Some(0)), false));
            }
        }
    }
    // hostile scopes
    out.push((EditScopeOwned::Cell(BlockPath::root(u32::MAX), 9, 9), false));
    out.push((EditScopeOwned::Hf(HfWhich::Header, HfPartId(u32::MAX), u32::MAX), true));
    out.push((EditScopeOwned::Note(NoteKind::Endnote, NumKey::new(f64::NAN)), false));
    out.push((EditScopeOwned::Tb(vec![], Some(u32::MAX)), false));
    out
}

enum EditScopeOwned {
    Body,
    Cell(BlockPath, u32, u32),
    Hf(HfWhich, HfPartId, u32),
    Note(NoteKind, NumKey),
    Tb(Vec<DrawingId>, Option<u32>),
}

impl EditScopeOwned {
    fn get(&self) -> EditScope<'_> {
        match self {
            EditScopeOwned::Body => EditScope::Body,
            EditScopeOwned::Cell(t, r, c) => EditScope::Cell { table: t.clone(), row: *r, cell: *c },
            EditScopeOwned::Hf(w, p, pg) => EditScope::HeaderFooter { which: *w, part: *p, page: *pg },
            EditScopeOwned::Note(k, id) => EditScope::Note { kind: *k, id: *id },
            EditScopeOwned::Tb(c, p) => EditScope::Textbox { chain: c, page: *p },
        }
    }
}

const HOSTILE: [f64; 9] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 0.0, -1e300, 1e300, 5e-324, 12345.678];

/// The hot queries of an edit scope over hostile points and positions.
fn poke(el: &EditLines, li: &LineIndex, loc: &Locator<'_>, l: &Layout, rng: &mut Rng, rects: &mut Vec<SelRect>) {
    let pages = l.pages.len() as u32;
    for _ in 0..200 {
        let pg = if rng.below(4) == 0 { u32::MAX - rng.below(3) as u32 } else { rng.below(pages as usize + 2) as u32 };
        let x = HOSTILE[rng.below(HOSTILE.len())] * if rng.below(2) == 0 { 1.0 } else { rng.below(20000) as f64 / 1000.0 };
        let y = HOSTILE[rng.below(HOSTILE.len())];
        let h = el.hit(pg, x, y);
        let _ = el.caret(h);
        let _ = li.line_at(l, pg, x, y);
        let _ = cell_at(loc, l, pg, x, y);
        let pos = EditPos {
            para: if rng.below(3) == 0 { u32::MAX } else { rng.below(el.paras.len() + 2) as u32 },
            offset: rng.below(5000) as u32,
        };
        let _ = el.caret(pos);
        let _ = el.doc_ref(pos);
        rects.clear();
        let end = EditPos { para: pos.para.saturating_add(rng.below(3) as u32), offset: u32::MAX };
        el.selection_rects(pg, pos, end, rects);
        el.selection_rects(pg, end, pos, rects);
    }
}

#[test]
fn hostile_positions_scopes_and_refs_do_not_panic() {
    let mut rng = Rng::new(8);
    for (_, docx, p) in fixture() {
        let doc = read(&docx);
        let view = DocView::new(&doc);
        let l = layout(view, &p, &opts());
        let loc = Locator::new(view);
        let li = LineIndex::new(&loc, &l);
        let mut rects = Vec::new();
        for (s, _) in scopes(view, &l) {
            let el = EditLines::new(&loc, &l, &li, &s.get());
            poke(&el, &li, &loc, &l, &mut rng, &mut rects);
        }
        // hostile line refs
        for _ in 0..300 {
            let n = rng.below(12);
            let path: Vec<u32> =
                (0..n).map(|_| if rng.below(3) == 0 { u32::MAX - rng.below(2) as u32 } else { rng.below(6) as u32 }).collect();
            let story = match rng.below(5) {
                0 => LineStory::Body,
                1 => LineStory::Note(NoteKind::Footnote, NumKey::new([1.0, -1.0, 0.5, f64::NAN, 1e22][rng.below(5)])),
                2 => LineStory::HeaderFooter { which: HfWhich::Footer, slot: HfSlot::Even, part: HfPartId(rng.below(4) as u32) },
                3 => LineStory::Preview,
                _ => LineStory::Body,
            };
            let at = LineRef { story, path: RefPath(path.clone().into()) };
            let _ = loc.locate(rng.below(5) as u32 * 1_000_000, &at);
            let _ = loc.cell_hops(&path);
            let _ = loc.body_path(&path);
            let _ = li.page_of(&l, &ParaLoc { story: StoryRef::Body, path: BlockPath::root(rng.below(9) as u32) }, HOSTILE[rng.below(9)]);
        }
        // hostile characters and faces
        for x in HOSTILE {
            let cs = CharsView { o: &[3, 1, 2, u32::MAX], x: &[x, 5.0, f64::NAN, -0.0], w: &[1.0, x, 2.0] };
            let _ = caret_x_in(cs, x, x);
            let _ = offset_at_x(cs, 7, x, u32::MAX);
            let _ = offset_at_x(CharsView::EMPTY, 7, x, 0);
            let _ = l.measure_text(Some(uo_render::FaceId::from_index(9999).unwrap()), x, &[0xd800, 0xdfff, 0xd83d]);
            let b = RunBox {
                page: u32::MAX,
                x,
                y: x,
                w: x,
                h: x,
                baseline: x,
                run: uo_docx_layout::model::RunRef { story: StoryRef::Body, path: BlockPath::root(0), run: 0, sub: Default::default() },
                props: Arc::default(),
                value: Box::default(),
            };
            let _ = l.custom_inline_face(&b);
            let _ = uo_docx_layout::navigation::block_line(&l, x);
            let _ = uo_docx_layout::view_transform::web_document(&doc, x);
        }
    }
}

#[test]
fn queries_leak_nothing_and_hit_tests_allocate_nothing() {
    let fx = fixture();
    let (_, docx, p) = &fx[fx.len() / 2];
    let doc = read(docx);
    let view = DocView::new(&doc);
    let l = layout(view, p, &opts());
    // M1: every query, its outputs dropped
    let u = assert_no_leak(|| {
        let loc = Locator::new(view);
        let li = LineIndex::new(&loc, &l);
        for (s, _) in scopes(view, &l) {
            let el = EditLines::new(&loc, &l, &li, &s.get());
            let mut rects = Vec::new();
            if !el.paras.is_empty() {
                el.selection_rects(0, EditPos { para: 0, offset: 0 }, EditPos { para: el.paras.len() as u32 - 1, offset: 9 }, &mut rects);
            }
        }
        let _ = l.run_boxes(view, RunFilter::Links);
        let _ = l.run_boxes(view, RunFilter::CustomInlines);
        let _ = l.navigation(view);
    });
    eprintln!("M1 queries: {u:?}");
    // M3: the hot paths allocate nothing (built once per layout)
    let loc = Locator::new(view);
    let li = LineIndex::new(&loc, &l);
    let el = EditLines::new(&loc, &l, &li, &EditScope::Body);
    let ((), u) = measure(|| {
        for pg in 0..l.pages.len() as u32 {
            let page = &l.pages[pg as usize];
            for k in 0..200 {
                let (x, y) = (f64::from(k) * 61.0, f64::from(k) * 83.0);
                let h = el.hit(pg, x, y);
                let _ = el.caret(h);
                let _ = li.line_at(&l, pg, x, y);
                let _ = cell_at(&loc, &l, pg, x, y);
            }
            for line in &page.lines {
                let cs = CharsView::of(page, line);
                for k in 0..20 {
                    let _ = caret_x_in(cs, line.x, f64::from(line.start + k));
                    let _ = offset_at_x(cs, line.start, line.x + f64::from(k) * 100.0, line.end);
                }
            }
        }
    });
    eprintln!("M3 hit tests over {} pages: {u:?}", l.pages.len());
    assert_eq!(u.allocated, 0, "a hit test allocates nothing");
    // M3 numbers of the index and the edit lines
    let (_, ui) = measure(|| LineIndex::new(&loc, &l));
    let (_, ue) = measure(|| EditLines::new(&loc, &l, &li, &EditScope::Body));
    eprintln!("M3 LineIndex {ui:?}; EditLines(body) {ue:?}");
}

/// The engine as the session's oracle: the field values and the forced hyphenation a
/// `Session` gets equal what the replayed layout reports (itself equal to the TS, `f8_parity`).
#[test]
fn the_oracle_answers_a_session() {
    for (stem, docx, p) in fixture().into_iter().take(4) {
        let s = uo_docx::session::Session::open(
            docx.clone(),
            uo_docx::session::OpenOptions::default(),
            Box::new(uo_docx::session::FixedEnv::default()),
        )
        .unwrap();
        let mut o = opts();
        let mut oracle = EngineOracle::new(&p, hyph_registry(), o.clone());
        let got = oracle.field_values(s.view(), &FieldScope::All).unwrap();
        // the same layout run directly over the read document
        o.collect_fields = Some(FieldScope::All);
        let doc = read(&docx);
        let want = layout(DocView::new(&doc), &p, &o).fields.unwrap();
        // the session's model holds the editor's mark runs (bookmarks, field ends): the run
        // indices differ from the read document's, the values and the fields they name do not
        assert_eq!(got.len(), want.len(), "{stem}");
        let field = |v: DocView<'_>, r: &uo_docx_layout::model::RunRef| v.run(r).and_then(|x| x.ext()).and_then(|e| e.field.clone());
        for ((gr, gv), (wr, wv)) in got.iter().zip(&want) {
            assert_eq!(gv, wv, "{stem}: field values through the session's view");
            assert_eq!((gr.story, &gr.path), (wr.story, &wr.path), "{stem}");
            assert_eq!(field(s.view(), gr), field(DocView::new(&doc), wr), "{stem}");
        }
        // a subset scope keeps the order and only those runs
        let some: std::collections::HashSet<_> = got.iter().step_by(2).map(|(r, _)| r.clone()).collect();
        let sub = oracle.field_values(s.view(), &FieldScope::Runs(some.clone())).unwrap();
        assert_eq!(sub, got.iter().filter(|(r, _)| some.contains(r)).cloned().collect::<Vec<_>>());
        // forced hyphenation: the chain of `next_hyphenation` = the points of a forced layout
        let mut of = opts();
        of.hyphenation = uo_docx_layout::options::Hyphenation::Force;
        // (over the session's view: its paragraphs hold the editor's mark runs)
        let forced = layout(s.view(), &p, &of);
        let mut from = DocRef { story: StoryRef::Body, path: BlockPath::root(0), offset: 0 };
        let mut n = 0;
        while let Some(c) = oracle.next_hyphenation(s.view(), &from, &HyphenSkips::default()).unwrap() {
            assert_eq!(Some(c.clone()), uo_docx_layout::oracle::first_hyphenation(s.view(), &forced, &from));
            from = DocRef { offset: c.at.offset + 1, ..c.at };
            n += 1;
            if n > 6 {
                break;
            }
        }
        eprintln!("{stem}: {} field values, {n} hyphenation points walked", want.len());
    }
}

/// D2b-F10 (`pageLabelOf`): the engine answers each paragraph's first-line page number in a
/// layout of the same view (`None` for a path without lines), and a TOC built through a session
/// over the REAL oracle succeeds (until this method existed it failed with a layout error).
#[test]
fn the_oracle_answers_the_shown_page_numbers() {
    use uo_docx::session::ops::references::{ReferenceCmd, TableOfContentsOptions};
    use uo_docx::session::{ClipboardRing, Command, ExecCx};
    for (stem, docx, p) in fixture().into_iter().take(4) {
        let mut s = uo_docx::session::Session::open(
            docx.clone(),
            uo_docx::session::OpenOptions::default(),
            Box::new(uo_docx::session::FixedEnv::default()),
        )
        .unwrap();
        let mut oracle = EngineOracle::new(&p, hyph_registry(), opts());
        // the expectation, from a layout of the same view: first line of each paragraph
        let l = layout(s.view(), &p, &opts());
        let loc = Locator::new(s.view());
        let mut refs: Vec<DocRef> = Vec::new();
        let mut want: Vec<Option<f64>> = Vec::new();
        for page in &l.pages {
            for line in &page.lines {
                let Some(para) = loc.locate(page.section, &line.at).para else { continue };
                if para.story != StoryRef::Body {
                    continue;
                }
                let r = DocRef { story: para.story, path: para.path, offset: 0 };
                if !refs.contains(&r) {
                    refs.push(r);
                    want.push(Some(page.number));
                }
            }
        }
        // a path the layout has no line for, and a paragraph asked twice
        refs.push(DocRef { story: StoryRef::Body, path: BlockPath::root(1_000_000), offset: 0 });
        want.push(None);
        if let Some(r) = refs.first().cloned() {
            refs.push(r);
            want.push(want[0]);
        }
        let got = oracle.shown_page_numbers(s.view(), &refs).unwrap();
        assert_eq!(got, want, "{stem}");
        // end to end: the TOC command asks the engine and succeeds
        let mut clip = ClipboardRing::default();
        let out = s.exec(
            Command::Reference(ReferenceCmd::InsertTableOfContents { opts: TableOfContentsOptions::default(), replace: None }),
            &mut ExecCx { layout: &mut oracle, clipboard: &mut clip },
        );
        assert!(out.is_ok(), "{stem}: {out:?}");
        eprintln!("{stem}: {} paragraphs with a page, TOC inserted: {}", want.len() - 2, s.has_table_of_contents());
    }
}

/// A provider that answers nothing for one family, and reports budget refusals.
struct Picky<'p> {
    inner: &'p dyn FontProvider,
    refusals: u64,
}

impl FontProvider for Picky<'_> {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<FontAnswer>> {
        if family.eq_ignore_ascii_case("Liberation Sans") { None } else { self.inner.face(family, bold, italic) }
    }
    fn budget_refusals(&self) -> u64 {
        self.refusals
    }
}

#[test]
fn missing_faces_and_the_font_budget_warning() {
    let fx = fixture();
    let (_, docx, p) = &fx[0];
    let doc = read(docx);
    let view = DocView::new(&doc);
    let plain = layout(view, p, &opts());
    assert!(!plain.warnings.contains("font budget"));
    let picky = Picky { inner: p, refusals: 0 };
    let l = layout(view, &picky, &opts());
    assert!(l.missing_faces.iter().any(|f| f.family.eq_ignore_ascii_case("Liberation Sans")), "{:?}", l.missing_faces);
    let unique: std::collections::HashSet<_> = l.missing_faces.iter().collect();
    assert_eq!(unique.len(), l.missing_faces.len(), "each miss once");
    let over = Picky { inner: p, refusals: 2 };
    let l = layout(view, &over, &opts());
    assert_eq!(l.warnings.last().map(|w| &**w), Some("font budget"));
    // the registry signal itself
    let ledger = uo_fonts::registry::FaceLedger::new(10);
    let face = uo_fonts::registry::FaceCache::new(
        uo_fonts::registry::FontBudget::of(uo_fonts::registry::FontProfile::Desktop),
        uo_fonts::FontLimits::default(),
    );
    let _ = face;
    assert_eq!(ledger.refused(), 0);
}

#[test]
fn views_lay_out_a_transformed_document() {
    let fx = fixture();
    let (_, docx, p) = &fx[1];
    let doc = read(docx);
    let mut eng = LayoutEngine::with_hyphenation(hyph_registry());
    let mut o = opts();
    o.view = ViewTransform::Web { width_tw: 9000.0 };
    let web = eng.layout(DocView::new(&doc), p, &o).unwrap();
    assert!(web.pages.iter().all(|pg| pg.page.w == 9000.0 && pg.page.h == uo_docx_layout::view_transform::WEB_PAGE_H));
    assert!(web.pages.iter().all(|pg| pg.lines.iter().all(|l| !matches!(l.at.story, LineStory::HeaderFooter { .. }))));
    o.view = ViewTransform::Draft;
    let draft = eng.layout(DocView::new(&doc), p, &o).unwrap();
    let s0 = &doc.sections[0].sect_pr;
    let text_w = s0.page_width_twips.to_number() - s0.margin_left_twips.to_number() - s0.margin_right_twips.to_number();
    assert_eq!(draft.pages[0].page.w, uo_jsnum::math::max(2880.0, uo_jsnum::math::round(text_w + 576.0)));
    // wave 7 (B): Outline = the Draft sheet with the TS editor's outline indent (view_transform.rs), no longer an error
    let draft_w = draft.pages[0].page.w;
    o.view = ViewTransform::Outline;
    let outline = eng.layout(DocView::new(&doc), p, &o).unwrap();
    assert_eq!(outline.pages[0].page.w, draft_w);
    eng.clear_caches();
}
