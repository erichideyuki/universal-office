//! The emission of a slide as the Slides display list (frontier B4): wire round trip, the memory gate (M1, M2, M5) and
//! bounded no-panic over generated, mutated and hostile input. The exact draw-op parity against the frozen TS is
//! `js/emit-uolf.mjs` + `js/compare-drawops.mjs` (see the record); these tests need no Node.
//! `UO_EMIT_NOPANIC_ROUNDS=<n>` multiplies the mutants for a long run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_dml_render::emit::{Emission, EmitCaches};
use uo_dml_render::text::{MAX_ROUNDS, MeasureTable};
use uo_pptx::{Deck, EmitFail, Limits, View, emit_slide};
use uo_render::slides::{SlidesDecoder, SlidesEncoder, SlidesResources};

#[path = "support/stub_measure.rs"]
mod stub;

fn deck_path(group: &str, n: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures/decks").join(group).join(format!("{n}.pptx"))
}

fn deck_bytes(group: &str, n: &str) -> Vec<u8> {
    let p = deck_path(group, n);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The two views of the captures for a slide of `slide_w` EMU.
fn views(slide_w: f64) -> [View; 2] {
    [View { scale: 1280.0 / slide_w, ox: 0.0, oy: 0.0 }, View { scale: 640.0 / slide_w, ox: 17.5, oy: 9.25 }]
}

/// Emits slide `i` answering the measurement rounds from the stub; `None` when the deck cannot give the slide or the
/// rounds run out.
fn emit(
    deck: &mut Deck,
    i: u32,
    view: View,
    table: &mut MeasureTable,
    res: &mut SlidesResources,
    caches: &mut EmitCaches,
) -> Option<Emission> {
    for _ in 0..=MAX_ROUNDS {
        match emit_slide(deck, i, view, table, res, caches) {
            Ok(e) => return Some(e),
            Err(EmitFail::Need(n)) => stub::answer(deck, table, &n.0),
            Err(EmitFail::Deck(_)) => return None,
        }
    }
    None
}

fn emit_deck(deck: &mut Deck, only_first_view: bool) -> usize {
    let (n, w) = {
        let p = deck.presentation().unwrap();
        (p.slides.len(), p.slide_w)
    };
    let mut table = MeasureTable::default();
    let mut res = SlidesResources::new();
    let mut items = 0;
    for i in 0..n {
        for v in views(w).into_iter().take(if only_first_view { 1 } else { 2 }) {
            let mut caches = EmitCaches::new();
            if let Some(e) = emit(deck, u32::try_from(i).unwrap(), v, &mut table, &mut res, &mut caches) {
                items += e.list.items.len();
            }
        }
    }
    items
}

/// B5b: the pictures of the `b5b` decks (EMF, EMF+, WMF, PICT, TIFF next to PNG) emit as recipe bitmaps, the list round-trips on the
/// wire with the recipes flagged, a missing measurement is asked (rounds), the raster cache holds one picture per media, and nothing is left
/// behind. The exact stream parity is `compare-drawops.mjs` over `drawops-b5b`.
#[test]
fn b5b_pictures_are_recipe_bitmaps_and_leak_nothing() {
    let mut recipes = 0usize;
    for n in ["0000", "0002", "0005", "0011", "0017", "0023"] {
        let bytes = deck_bytes("b5b", n);
        let mut deck = Deck::open(&bytes, Limits::default()).unwrap();
        let w = deck.presentation().unwrap().slide_w;
        let count = deck.presentation().unwrap().slides.len();
        let mut table = MeasureTable::default();
        let mut res = SlidesResources::new();
        let mut enc = SlidesEncoder::new(1);
        let mut dec = SlidesDecoder::new();
        for i in 0..count {
            let mut caches = EmitCaches::new();
            let e = emit(&mut deck, u32::try_from(i).unwrap(), views(w)[0], &mut table, &mut res, &mut caches).expect("emits");
            let mut msg = Vec::new();
            enc.encode(&e.list, &res, &mut msg).unwrap();
            dec.decode(&msg).unwrap();
            assert!(caches.sizes().0 <= 256);
        }
        for h in 0..u32::try_from(res.lens()[4]).unwrap() {
            if res.is_recipe(h) {
                recipes += 1;
                assert!(res.bitmap_bytes(h).unwrap().starts_with(b"UOMF"));
            }
        }
    }
    assert!(recipes > 0, "no picture of the b5b decks became a recipe");
    // M1 over the whole emission of one deck
    let bytes = deck_bytes("b5b", "0002");
    let mut warm = Deck::open(&bytes, Limits::default()).unwrap();
    emit_deck(&mut warm, true);
    drop(warm);
    assert_no_leak(|| {
        let mut deck = Deck::open(&bytes, Limits::default()).unwrap();
        emit_deck(&mut deck, true);
    });
}

#[test]
fn every_slide_emits_and_round_trips_on_the_wire() {
    for (g, n) in [("b1a", "0001"), ("b1a", "0008"), ("b3a", "0002"), ("b5a", "0003"), ("c1d", "0003"), ("c1d", "0011")] {
        let mut deck = Deck::open(&deck_bytes(g, n), Limits::default()).unwrap();
        let count = deck.presentation().unwrap().slides.len();
        let w = deck.presentation().unwrap().slide_w;
        let mut table = MeasureTable::default();
        let mut res = SlidesResources::new();
        let mut enc = SlidesEncoder::new(1);
        let mut dec = SlidesDecoder::new();
        let mut total = 0;
        for i in 0..count {
            for v in views(w) {
                let mut caches = EmitCaches::new();
                let e = emit(&mut deck, u32::try_from(i).unwrap(), v, &mut table, &mut res, &mut caches).expect("emits");
                assert!(!e.list.items.is_empty(), "{g}/{n} slide {i}: an empty list");
                let mut bytes = Vec::new();
                enc.encode(&e.list, &res, &mut bytes).expect("a list the emitter built encodes (validate passes)");
                let back = dec.decode(&bytes).expect("and decodes");
                assert_eq!(back, e.list, "{g}/{n} slide {i}: wire round trip");
                total += back.items.len();
            }
        }
        assert!(total > 50, "{g}/{n}: {total} items");
    }
}

#[test]
fn the_list_is_deterministic_and_per_view() {
    let mut deck = Deck::open(&deck_bytes("b1a", "0008"), Limits::default()).unwrap();
    let w = deck.presentation().unwrap().slide_w;
    let [a, b] = views(w);
    // each run has its own table and resources: the ids of the symbol table are first-use positions, so the same
    // sequence of rounds gives the same ids
    let run = |deck: &mut Deck, v: View| {
        let mut table = MeasureTable::default();
        let mut res = SlidesResources::new();
        emit(deck, 0, v, &mut table, &mut res, &mut EmitCaches::new()).unwrap().list
    };
    let l1 = run(&mut deck, a);
    let l2 = run(&mut deck, a);
    let lb = run(&mut deck, b);
    assert_eq!(l1, l2, "same view, same list");
    assert_ne!(l1, lb, "another scale and origin is another list");
    assert!((l1.view.scale - a.scale).abs() == 0.0 && (lb.view.ox - 17.5).abs() == 0.0);
}

#[test]
fn needs_are_batched_and_the_rounds_are_bounded() {
    let mut deck = Deck::open(&deck_bytes("b3a", "0002"), Limits::default()).unwrap();
    let w = deck.presentation().unwrap().slide_w;
    let table = MeasureTable::default();
    let mut res = SlidesResources::new();
    let first = match emit_slide(&mut deck, 0, views(w)[0], &table, &mut res, &mut EmitCaches::new()) {
        Err(EmitFail::Need(n)) => n,
        other => panic!("an empty table cannot give a slide with text: {:?}", other.map(|e| e.list.items.len())),
    };
    // round 1 asks the advances of every body of the slide at once (not one body per round)
    assert!(first.0.len() > 5, "{} requests", first.0.len());
    let mut table = MeasureTable::default();
    let mut rounds = 0;
    loop {
        match emit_slide(&mut deck, 0, views(w)[0], &table, &mut res, &mut EmitCaches::new()) {
            Ok(_) => break,
            Err(EmitFail::Need(n)) => {
                rounds += 1;
                assert!(rounds <= MAX_ROUNDS, "past the cap");
                stub::answer(&deck, &mut table, &n.0);
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!((1..=6).contains(&rounds), "{rounds} rounds");
}

#[test]
fn m1_emission_returns_live_bytes() {
    let bytes = deck_bytes("b1a", "0008");
    let mut deck = Deck::open(&bytes, Limits::default()).unwrap();
    // one emission first: the lazily built tables of the engines (grapheme data, presets) are not a leak
    emit_deck(&mut deck, false);
    deck.clear_caches();
    assert_no_leak(|| {
        let items = emit_deck(&mut deck, false);
        assert!(items > 0);
        deck.clear_caches();
    });
    // the error paths: an unknown slide, a table that never gets its answers
    assert_no_leak(|| {
        let table = MeasureTable::default();
        let mut res = SlidesResources::new();
        let view = View { scale: 0.0001, ox: 0.0, oy: 0.0 };
        assert!(matches!(emit_slide(&mut deck, 999, view, &table, &mut res, &mut EmitCaches::new()), Err(EmitFail::Deck(_))));
        let _ = emit_slide(&mut deck, 0, view, &table, &mut res, &mut EmitCaches::new());
        deck.clear_caches();
    });
    // text, tables and SmartArt decks
    for (g, n) in [("b3a", "0002"), ("b5a", "0003"), ("c1d", "0003"), ("c1d", "0011")] {
        let mut d = Deck::open(&deck_bytes(g, n), Limits::default()).unwrap();
        emit_deck(&mut d, true);
        d.clear_caches();
        assert_no_leak(|| {
            emit_deck(&mut d, true);
            d.clear_caches();
        });
    }
}

#[test]
fn m2_emission_soak_is_flat() {
    let bytes = deck_bytes("b1a", "0001");
    let mut deck = Deck::open(&bytes, Limits::default()).unwrap();
    let series = soak(60, 10, |_| {
        emit_deck(&mut deck, false);
        deck.clear_caches();
    });
    assert_flat(&series, 2, 0);
}

#[test]
fn m3_the_cost_of_a_slide_is_recorded() {
    let mut deck = Deck::open(&deck_bytes("b1a", "0008"), Limits::default()).unwrap();
    let (items, usage) = measure(|| emit_deck(&mut deck, true));
    eprintln!("emit b1a/0008 first view: {items} items, {usage:?}");
    assert!(items > 0);
}

// ------------------------------------------------------------------------------------------------ no panic

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { usize::try_from(self.next() % n as u64).unwrap_or(0) }
    }
}

const TOKENS: &[&str] = &[
    "val=\"-1\"",
    "val=\"NaN\"",
    "w=\"-9\"",
    "gridSpan=\"99999999\"",
    "rowSpan=\"4294967295\"",
    "<a:ext cx=\"1e999\" cy=\"-5\"/>",
    "<a:off x=\"-99999999999\" y=\"1e300\"/>",
    "rot=\"2147483647\"",
    "<a:avLst><a:gd name=\"adj\" fmla=\"val -99999999\"/></a:avLst>",
    "<a:gradFill><a:gsLst><a:gs pos=\"-5\"><a:srgbClr val=\"zzzzzz\"/></a:gs><a:gs pos=\"1e9\"><a:srgbClr val=\"123\"/></a:gs></a:gsLst><a:path path=\"circle\"><a:fillToRect l=\"-1\" r=\"9\"/></a:path></a:gradFill>",
    "<a:pattFill prst=\"constructor\"><a:fgClr><a:srgbClr val=\"FF0000\"/></a:fgClr></a:pattFill>",
    "<a:ln w=\"-1\"><a:prstDash val=\"sysDash\"/></a:ln>",
    "<a:effectLst><a:glow rad=\"1e12\"><a:srgbClr val=\"00FF00\"/></a:glow><a:softEdge rad=\"-3\"/></a:effectLst>",
    "<a:bodyPr rot=\"-99999999\" vert=\"vert270\" numCol=\"99999\"/>",
    "<a:normAutofit fontScale=\"0\"/>",
    "<a:tbl>",
    "<a:tr h=\"-1\"><a:tc/></a:tr>",
    "<a:t>\u{FFFD}\u{0}</a:t>",
    "<p:ph type=\"title\" idx=\"1\"/>",
];

fn mutate(rng: &mut Rng, src: &[u8]) -> Vec<u8> {
    let mut b = src.to_vec();
    if b.is_empty() {
        return b;
    }
    match rng.below(5) {
        0 => {
            let a = rng.below(b.len());
            let e = (a + rng.below(200)).min(b.len());
            b.drain(a..e);
        }
        1 => {
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(b.len());
                b[i] = u8::try_from(rng.next() & 0xFF).unwrap_or(0);
            }
        }
        2 | 3 => {
            let t = TOKENS[rng.below(TOKENS.len())].as_bytes().to_vec();
            let at = rng.below(b.len());
            b.splice(at..at, t);
        }
        _ => {
            // a number replaced by a hostile one
            if let Some(p) = b.windows(2).position(|w| w[0] == b'=' && w[1] == b'"') {
                let hostile = ["-1", "0", "99999999999", "1e308", "NaN", "-0"][rng.below(6)];
                let end = b[p + 2..].iter().position(|c| *c == b'"').map_or(b.len(), |e| p + 2 + e);
                b.splice(p + 2..end, hostile.bytes());
            }
        }
    }
    b
}

fn emit_bounded(deck: &mut Deck, rng: &mut Rng) {
    let Ok(p) = deck.presentation().cloned() else { return };
    let scales = [1280.0 / p.slide_w, 1e-9, 1e9, f64::NAN, f64::INFINITY, -1.0, 0.0];
    for i in 0..p.slides.len().min(3) {
        let v = View { scale: scales[rng.below(scales.len())], ox: if rng.below(4) == 0 { f64::NAN } else { 3.5 }, oy: 0.0 };
        let mut table = MeasureTable::default();
        let mut res = SlidesResources::new();
        if let Some(e) = emit(deck, u32::try_from(i).unwrap(), v, &mut table, &mut res, &mut EmitCaches::new()) {
            // a list the emitter built always encodes and its size is bounded by the input
            let mut out = Vec::new();
            SlidesEncoder::new(1).encode(&e.list, &res, &mut out).expect("the emitter never builds an invalid list");
            assert!(e.list.items.len() < 4_000_000, "item count");
        }
    }
}

#[test]
fn mutated_decks_emit_without_panic() {
    let rounds: usize = std::env::var("UO_EMIT_NOPANIC_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let limits = Limits { max_group_depth: 32, max_shapes_per_slide: 2_000, max_table_cells: 20_000, ..Limits::default() };
    let t0 = Instant::now();
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut emitted = 0usize;
    for (g, n) in [
        ("b1a", "0001"),
        ("b1a", "0003"),
        ("b1a", "0008"),
        ("b3a", "0002"),
        ("b3a", "0007"),
        ("b5a", "0003"),
        ("c1d", "0003"),
        ("c1d", "0011"),
    ] {
        let base = Deck::open(&deck_bytes(g, n), limits).unwrap();
        let parts: Vec<(Box<str>, Arc<[u8]>)> = base.parts().entries().iter().map(|e| (e.name.clone(), Arc::clone(&e.bytes))).collect();
        for (pi, (name, bytes)) in parts.iter().enumerate() {
            if !(name.starts_with("ppt/slides/slide")
                || name.starts_with("ppt/slideLayouts/")
                || name.starts_with("ppt/theme/")
                || name.starts_with("ppt/charts/"))
                || !name.ends_with(".xml")
            {
                continue;
            }
            for _ in 0..rounds {
                let mut all = parts.clone();
                if let Some(slot) = all.get_mut(pi) {
                    slot.1 = Arc::from(mutate(&mut rng, bytes));
                }
                if let Ok(mut deck) = Deck::from_parts(all, limits) {
                    emit_bounded(&mut deck, &mut rng);
                    emitted += 1;
                }
            }
        }
        assert!(t0.elapsed().as_secs() < 120, "the bounded run must stay in seconds");
    }
    eprintln!("emit nopanic: {emitted} mutants in {:?}", t0.elapsed());
    assert!(emitted > 30);
}

#[test]
fn hostile_viewports_do_not_panic() {
    let mut deck = Deck::open(&deck_bytes("b1a", "0008"), Limits::default()).unwrap();
    let mut table = MeasureTable::default();
    for scale in [0.0, -0.0, 1e-300, 1e300, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -5.0] {
        for ox in [0.0, f64::NAN, 1e300, -1e300] {
            let mut res = SlidesResources::new();
            let e = emit(&mut deck, 0, View { scale, ox, oy: ox }, &mut table, &mut res, &mut EmitCaches::new());
            if let Some(e) = e {
                let mut out = Vec::new();
                SlidesEncoder::new(1).encode(&e.list, &res, &mut out).expect("valid list");
            }
        }
    }
}
