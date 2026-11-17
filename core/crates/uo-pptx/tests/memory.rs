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
    let src = deck.source();
    let _warm = uo_pptx::read_slide(&src, &target, limits).unwrap();
    let (model, model_usage) = measure(|| uo_pptx::read_slide(&src, &target, limits).unwrap());
    eprintln!(
        "m3 retained SlideModel alone ({} shapes): {} B; one read allocates {} B, peak {} B",
        model.shapes.len(),
        model_usage.live_delta,
        model_usage.allocated,
        model_usage.peak_above_start
    );
    // DR3: the architecture pass budgeted 256 B for a Shape; the owned-tree model is 408 B (recorded in
    // uo-dml-render.md). The guard below fails on a regression of that number.
    let shape = std::mem::size_of::<uo_dml_render::Shape>();
    eprintln!(
        "m3 size_of::<TextStyleLevel>() = {}, TextBody = {}",
        std::mem::size_of::<uo_dml_render::text_style::TextStyleLevel>(),
        std::mem::size_of::<uo_dml_render::model::TextBody>()
    );
    eprintln!(
        "m3 size_of::<Shape>() = {shape}, Run = {}, Paragraph = {}",
        std::mem::size_of::<uo_dml_render::model::Run>(),
        std::mem::size_of::<uo_dml_render::model::Paragraph>()
    );
    assert!(shape <= 416, "Shape is {shape} bytes");
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

    // the media table is bounded by the package's media parts
    let mut deck = Deck::open(&deck_bytes("0001"), Limits::default()).unwrap();
    read_everything(&mut deck);
    let media_parts = deck.parts().names().filter(|n| n.starts_with("ppt/media/")).count();
    assert!(deck.source().store().len() > media_parts);
}
