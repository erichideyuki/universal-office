//! The memory gate of the Slides read path (M1, M2, M3 numbers, M5): `uo-alloc` counts live bytes per
//! thread. M1: every public operation (and each error path) returns live bytes EXACTLY to the start once
//! its outputs are dropped and the caches cleared (`Deck::clear_caches`). M2: a short soak (open → read
//! every slide → save → drop) is flat. M5: every cache has a cap and is exercised past it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_dml_render::source::PartSource;
use uo_pptx::{Deck, Limits, PartStore};

fn deck_bytes(n: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks/b1a").join(format!("{n}.pptx"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn read_everything(deck: &mut Deck) {
    let n = deck.presentation().unwrap().slides.len();
    for i in 0..n {
        let i = u32::try_from(i).unwrap();
        deck.slide(i).unwrap();
        deck.slide_notes(i).unwrap();
    }
    let _ = deck.embedded_fonts();
    let _ = deck.deck_typefaces();
}

#[test]
fn m1_open_ok_and_errors() {
    let ok = deck_bytes("0008");
    assert_no_leak(|| Deck::open(&ok, Limits::default()));
    assert_no_leak(|| Deck::open(b"not a zip at all", Limits::default()));
    assert_no_leak(|| Deck::open(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0, 0], Limits::default()));
    assert_no_leak(|| Deck::open(&ok[..ok.len() / 2], Limits::default()));
    // a package without ppt/presentation.xml
    let mut w = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    w.add("x.xml", b"<x/>").unwrap();
    let no_pres = w.finish();
    assert_no_leak(|| Deck::open(&no_pres, Limits::default()).unwrap_err());
    assert_no_leak(|| Deck::from_parts(vec![("a.xml".into(), Arc::from(&b"<a/>"[..]))], Limits::default()).unwrap_err());
}

#[test]
fn m1_reads_return_to_the_start() {
    for n in ["0001", "0008"] {
        let mut deck = Deck::open(&deck_bytes(n), Limits::default()).unwrap();
        deck.clear_caches();
        assert_no_leak(|| {
            read_everything(&mut deck);
            deck.clear_caches();
        });
        // one operation at a time
        assert_no_leak(|| {
            deck.presentation().unwrap();
            deck.clear_caches();
        });
        assert_no_leak(|| {
            deck.slide(0).unwrap();
            deck.clear_caches();
        });
        assert_no_leak(|| {
            deck.slide_notes(0).unwrap();
            deck.clear_caches();
        });
        assert_no_leak(|| deck.save().unwrap());
        // these parse presentation.xml through the deck's cache: cleared, the bytes are back
        assert_no_leak(|| {
            deck.embedded_fonts();
            deck.clear_caches();
        });
        assert_no_leak(|| deck.deck_typefaces());
        assert_no_leak(|| {
            deck.slide(99).unwrap_err();
            deck.clear_caches();
        });
        // the reading view and a media entry
        assert_no_leak(|| {
            let src = deck.source();
            let _ = src.part("ppt/presentation.xml");
            let _ = src.xml("ppt/presentation.xml").unwrap();
            let _ = src.read_set();
            drop(src);
            deck.clear_caches();
        });
    }
}

#[test]
fn m1_error_paths_of_the_reader() {
    let mut deck = Deck::open(&deck_bytes("0008"), Limits::default()).unwrap();
    let slide = deck.presentation().unwrap().slides[0].target.clone().unwrap();
    let good = deck.parts().bytes(&slide).unwrap().clone();
    // a malformed slide part, a malformed master, a missing part
    let broken: Arc<[u8]> = Arc::from(&good[..good.len() / 2]);
    deck.set_part(&slide, broken);
    deck.clear_caches();
    assert_no_leak(|| {
        assert!(deck.slide(0).is_err());
        deck.clear_caches();
    });
    deck.remove_part(&slide);
    deck.clear_caches();
    assert_no_leak(|| {
        assert!(deck.slide(0).is_err());
        deck.clear_caches();
    });
    deck.set_part(&slide, good);
    deck.clear_caches();
    assert_no_leak(|| {
        deck.slide(0).unwrap();
        deck.clear_caches();
    });
}

#[test]
fn m1_part_store() {
    let mut s = PartStore::new();
    s.set("a", Arc::from(&b"1"[..]));
    s.set("b", Arc::from(&b"2"[..]));
    assert_no_leak(|| {
        let mut t = s.clone();
        t.set("c", Arc::from(&b"3"[..]));
        t.set("a", Arc::from(&b"4"[..]));
        t.remove("b");
        t.set_text("d", "text");
        let snap = t.snapshot();
        (t.save().unwrap(), snap)
    });
}

#[test]
fn m2_soak_is_flat() {
    let bytes = deck_bytes("0008");
    let series = soak(120, 10, |_| {
        let mut deck = Deck::open(&bytes, Limits::default()).unwrap();
        read_everything(&mut deck);
        let _ = deck.save().unwrap();
        deck.clear_caches();
    });
    eprintln!("m2 soak (120 × open/read/save/drop): {series:?}");
    assert_flat(&series, 2, 0);
}

#[test]
fn m2_soak_with_a_changing_deck() {
    // edits between reads: set_part, a stale slide re-read, remove and add back
    let mut deck = Deck::open(&deck_bytes("0001"), Limits::default()).unwrap();
    let slide = deck.presentation().unwrap().slides[0].target.clone().unwrap();
    let original = deck.parts().bytes(&slide).unwrap().clone();
    let series = soak(120, 10, |i| {
        let text = String::from_utf8_lossy(&original).replace("<p:sld ", &format!("<!-- {i} --><p:sld "));
        deck.set_part(&slide, Arc::from(text.as_bytes()));
        let _ = deck.slide(0);
        deck.set_part(&slide, Arc::clone(&original));
        deck.slide(0).unwrap();
        deck.clear_caches();
    });
    assert_flat(&series, 2, 0);
}

#[test]
fn m3_budgets_are_recorded() {
    let bytes = deck_bytes("0008");
    let (deck, open) = measure(|| Deck::open(&bytes, Limits::default()).unwrap());
    let mut deck = deck;
    let (_p, pres) = measure(|| deck.presentation().unwrap().slides.len());
    let (s0, slide0) = measure(|| deck.slide(0).unwrap());
    let shapes = s0.shapes.len();
    let (_, again) = measure(|| deck.slide(0).unwrap());
    let (_, save) = measure(|| deck.save().unwrap());
    eprintln!(
        "m3 open: {open:?}\nm3 presentation: {pres:?}\nm3 slide(0) ({shapes} shapes): {slide0:?}\nm3 slide(0) cached: {again:?}\nm3 save: {save:?}"
    );
    assert!(again.allocated < slide0.allocated, "a cached slide must not re-read");
    // the retained SlideModel alone: read once more over a warm parse cache (nothing else is kept)
    let target = deck.presentation().unwrap().slides[0].target.clone().unwrap();
    let limits = deck.limits().dml();
    let _warm = uo_pptx::read_slide(&deck.source(), &target, limits).unwrap();
    // a fresh source inside the measured read (a source records every part access: its read set is not the model)
    let (model, model_usage) = measure(|| uo_pptx::read_slide(&deck.source(), &target, limits).unwrap());
    eprintln!(
        "m3 retained SlideModel alone ({} shapes): {} B; one read allocates {} B, peak {} B",
        model.shapes.len(),
        model_usage.live_delta,
        model_usage.allocated,
        model_usage.peak_above_start
    );
    // the plan's budget (`uo-pptx.md` "Memory plan"): the retained model at most half the TS `Slide` heap of
    // the same slide (`js/measure-memory.mjs`: 143,563 B, 20 copies of slide 0 of b1a/0008, median of 5)
    let half_ts = TS_SLIDE_0008_0 / 2;
    assert!(model_usage.live_delta <= half_ts, "retained SlideModel {} B > 0.5 × TS ({half_ts} B)", model_usage.live_delta);
}

/// The TS `Slide` heap of slide 0 of b1a/0008 (`js/measure-memory.mjs`, frozen TS `readSlide`, Node 24.15).
const TS_SLIDE_0008_0: i64 = 143_563;

/// The inline sizes of the model types (`size_of`, 64-bit), with the budgets the compaction pinned
/// (`uo-pptx.md` "Model memory"; DR3 of `uo-dml-render.md`: `Shape` <= 256 B). A field added inline that
/// pushes a type over its budget fails here: rare fields go into the type's boxed group instead.
#[test]
fn m3_model_type_sizes_are_within_budget() {
    use std::mem::size_of;
    use uo_dml_render::model::{
        BodyExtras, ParaBullet, ParaExtras, Paragraph, Run, RunExtras, RunFx, Shape, ShapeExtras, ShapeFx, ShapePaint, TextBody,
    };
    use uo_dml_render::text_style::TextStyleLevel;
    use uo_pptx::timing::{BhvrExtras, CtnExtras, TimeBhvr, TimeNode};
    let rows: [(&str, usize, usize); 17] = [
        ("Shape", size_of::<Shape>(), 256),
        ("ShapeExtras", size_of::<ShapeExtras>(), 200),
        ("ShapePaint", size_of::<ShapePaint>(), 96),
        ("ShapeFx", size_of::<ShapeFx>(), 648),
        ("TextBody", size_of::<TextBody>(), 144),
        ("BodyExtras", size_of::<BodyExtras>(), 144),
        ("Paragraph", size_of::<Paragraph>(), 240),
        ("ParaBullet", size_of::<ParaBullet>(), 112),
        ("ParaExtras", size_of::<ParaExtras>(), 40),
        ("Run", size_of::<Run>(), 120),
        ("RunExtras", size_of::<RunExtras>(), 152),
        ("RunFx", size_of::<RunFx>(), 304),
        ("TextStyleLevel", size_of::<TextStyleLevel>(), 24),
        ("TimeNode", size_of::<TimeNode>(), 192),
        ("CtnExtras", size_of::<CtnExtras>(), 176),
        ("TimeBhvr", size_of::<TimeBhvr>(), 352),
        ("BhvrExtras", size_of::<BhvrExtras>(), 360),
    ];
    for (name, size, budget) in rows {
        eprintln!("m3 size_of::<{name}>() = {size} (budget {budget})");
    }
    for (name, size, budget) in rows {
        assert!(size <= budget, "size_of::<{name}>() = {size} B, budget {budget} B");
    }
}

/// The TS reference of the retained slide model, per corpus group (`js/measure-memory.mjs`, frozen TS
/// `readSlide`, `node --expose-gc`, Node 24.15; median of 3): (group, decks, slides, shapes, TS heap bytes).
const TS_GROUP_HEAP: [(&str, usize, usize, usize, i64); 7] = [
    ("b1a", 200, 500, 5360, 44_685_352),
    ("b3a", 120, 249, 1959, 31_668_800),
    ("b5a", 150, 220, 3890, 8_950_320),
    ("b5b", 40, 81, 506, 1_121_552),
    ("b6a", 100, 202, 1480, 13_160_528),
    ("c1d", 60, 91, 234, 5_070_976),
    ("samples", 28, 307, 1742, 7_506_768),
];

fn group_decks(group: &str) -> Vec<Vec<u8>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<PathBuf> = Vec::new();
    if group == "samples" {
        // the tracked samples (every .pptx under slides/samples, two levels)
        let root = manifest.join("../../../slides/samples");
        let mut dirs = vec![root];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pptx")) {
                    files.push(p);
                }
            }
        }
    } else {
        for e in std::fs::read_dir(manifest.join("tests/captures/decks").join(group)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "pptx") {
                files.push(p);
            }
        }
    }
    files.sort();
    files.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

/// Every readable slide of every deck of a group, read once over a warm parse cache and kept alive: the
/// retained bytes, with the slide and shape counts. Decks or slides the reader refuses are skipped (the TS
/// script skips the same ones).
fn retained_group(group: &str) -> (usize, usize, usize, i64) {
    let mut decks: Vec<(Deck, Vec<Box<str>>)> = Vec::new();
    for bytes in group_decks(group) {
        let Ok(mut deck) = Deck::open(&bytes, Limits::default()) else { continue };
        let Ok(pres) = deck.presentation() else { continue };
        let targets: Vec<Box<str>> = pres.slides.iter().filter_map(|r| r.target.clone()).collect();
        let limits = deck.limits().dml();
        let src = deck.source();
        let ok: Vec<Box<str>> = targets.into_iter().filter(|t| uo_pptx::read_slide(&src, t, limits).is_ok()).collect();
        drop(src);
        decks.push((deck, ok));
    }
    let slides: usize = decks.iter().map(|(_, t)| t.len()).sum();
    let mut keep: Vec<uo_pptx::SlideModel> = Vec::with_capacity(slides);
    let (shapes, usage) = measure(|| {
        let mut shapes = 0;
        for (deck, targets) in &decks {
            let limits = deck.limits().dml();
            for t in targets {
                // a fresh source per read: a source records every part access (its read set), which is not
                // the slide model
                let m = uo_pptx::read_slide(&deck.source(), t, limits).unwrap();
                shapes += m.shapes.len();
                keep.push(m);
            }
        }
        shapes
    });
    (decks.len(), slides, shapes, usage.live_delta)
}

#[test]
fn m3_retained_slide_model_by_group() {
    let mut total_rust = 0i64;
    let mut total_ts = 0i64;
    for (group, ts_decks, ts_slides, ts_shapes, ts_heap) in TS_GROUP_HEAP {
        let (decks, slides, shapes, rust) = retained_group(group);
        assert_eq!((decks, slides, shapes), (ts_decks, ts_slides, ts_shapes), "{group}: not the TS's slides");
        let ratio = rust as f64 / ts_heap as f64;
        eprintln!("m3 group {group}: {decks} decks, {slides} slides, {shapes} shapes: Rust {rust} B, TS {ts_heap} B, {ratio:.3}×");
        total_rust += rust;
        total_ts += ts_heap;
        // the plan's budget, per group: at most half the TS heap of the same slides
        assert!(rust <= ts_heap / 2, "{group}: retained {rust} B > 0.5 × TS ({} B)", ts_heap / 2);
    }
    eprintln!("m3 all groups: Rust {total_rust} B, TS {total_ts} B, {:.3}×", total_rust as f64 / total_ts as f64);
}

#[test]
fn m5_caches_are_capped() {
    // the slide cache: one slot, four slides read
    let limits = Limits { max_cached_slides: 1, ..Limits::default() };
    let mut deck = Deck::open(&deck_bytes("0008"), limits).unwrap();
    for i in 0..4u32 {
        deck.slide(i).unwrap();
        assert!(deck.cached_slides() <= 1);
    }
    // two slots: the least recently used goes
    let limits = Limits { max_cached_slides: 2, ..Limits::default() };
    let mut deck = Deck::open(&deck_bytes("0008"), limits).unwrap();
    for i in [0u32, 1, 2, 3, 0, 1] {
        deck.slide(i).unwrap();
        assert!(deck.cached_slides() <= 2);
    }
    assert_eq!(deck.cached_slides(), 2);

    // the parse cache: a cap smaller than the package's XML, filled past it
    let big = Deck::open(&deck_bytes("0008"), Limits::default()).unwrap();
    let total: usize = {
        let mut d = big;
        read_everything(&mut d);
        let n = d.xml_cache().bytes();
        assert!(n > 0);
        n
    };
    for cap in [1usize << 10, total / 3, total / 2, total] {
        let limits = Limits { max_xml_cache_bytes: cap, ..Limits::default() };
        let mut deck = Deck::open(&deck_bytes("0008"), limits).unwrap();
        read_everything(&mut deck);
        read_everything(&mut deck);
        assert!(deck.xml_cache().bytes() <= cap, "cap {cap}: {} bytes cached", deck.xml_cache().bytes());
    }
    // the cap still lets a part bigger than the cache be read (it is just not kept)
    let limits = Limits { max_xml_cache_bytes: 16, ..Limits::default() };
    let mut deck = Deck::open(&deck_bytes("0008"), limits).unwrap();
    read_everything(&mut deck);
    assert!(deck.xml_cache().is_empty());

    // the shared themes: one per distinct theme value, capped (`MAX_SHARED_THEMES`); the slides of a deck
    // with one master share one copy
    let mut deck = Deck::open(&deck_bytes("0008"), Limits::default()).unwrap();
    read_everything(&mut deck);
    let shared = deck.theme_table().len();
    assert!((1..=uo_pptx::deck::MAX_SHARED_THEMES).contains(&shared), "{shared} themes");
    let a = deck.slide(0).unwrap().theme.clone().unwrap();
    let b = deck.slide(1).unwrap().theme.clone().unwrap();
    assert_eq!(Arc::ptr_eq(&a, &b), *a == *b, "equal themes are one copy");
    // past the cap: a table filled by more distinct themes than it holds keeps at most the cap
    let mut table = uo_pptx::deck::ThemeTable::default();
    for i in 0..uo_pptx::deck::MAX_SHARED_THEMES + 8 {
        let t = uo_dml_render::theme::Theme { part: Some(format!("ppt/theme/theme{i}.xml").into()), ..Default::default() };
        let shared = table.share(t.clone());
        assert!(Arc::ptr_eq(&shared, &table.share(t)), "an equal theme is the held copy");
        assert!(table.len() <= uo_pptx::deck::MAX_SHARED_THEMES);
    }
    assert_eq!(table.len(), uo_pptx::deck::MAX_SHARED_THEMES);
    deck.clear_caches();
    assert!(deck.theme_table().is_empty());

    // the media table is bounded by the package's media parts
    let mut deck = Deck::open(&deck_bytes("0001"), Limits::default()).unwrap();
    read_everything(&mut deck);
    let media_parts = deck.parts().names().filter(|n| n.starts_with("ppt/media/")).count();
    assert!(deck.source().store().len() > media_parts);
}
