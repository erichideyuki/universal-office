//! No panic on hostile input in frontier P: mutated font files through the system index scan and
//! the pick, mutated collections through the face extraction, mutated `*-advances.json` text, and
//! mutated fonts pushed into a registry and asked for. A result of any kind is fine; a panic is
//! the failure (the loops run under the test harness, which reports it).
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::sync::Arc;

use b_support::mutate;
use provider_support::{corpus_files, pool_files};
use uo_fonts::policy::ViewerProvider;
use uo_fonts::registry::{AdvanceTable, FaceCache, FontBudget, FontProvider, FontRegistry, FontSource, HostFace, ttc_face_by_index};
use uo_fonts::{Face, FontLimits, Variant};
use uo_testkit::generate::Rng;

#[cfg(all(feature = "system", not(target_family = "wasm")))]
#[test]
fn mutated_fonts_never_panic_the_scan_and_the_pick() {
    use provider_support::TempDir;
    use uo_fonts::registry::FaceSource;
    use uo_fonts::system::SystemFontIndex;

    let dir = TempDir::new("nopanic-scan");
    let mut written = 0;
    for (n, (rel, bytes)) in corpus_files().into_iter().enumerate() {
        if rel.ends_with(".txt") {
            continue;
        }
        for (k, m) in mutate(&bytes, 0x5151 + u32::try_from(n).unwrap(), 40).into_iter().enumerate() {
            std::fs::write(dir.0.join(format!("m{n:03}-{k:03}.{}", if rel.ends_with(".ttc") { "ttc" } else { "ttf" })), m).unwrap();
            written += 1;
        }
    }
    let cache = Arc::new(FaceCache::new(FontBudget::DESKTOP, FontLimits::NATIVE));
    let index = SystemFontIndex::with_roots(vec![dir.0.clone()], cache);
    let (mut picks, mut loaded) = (0usize, 0usize);
    for family in index.families().into_iter().chain(index.entries().iter().filter_map(|e| e.typo.clone())) {
        for (b, i) in [(false, false), (true, false), (false, true), (true, true)] {
            picks += 1;
            if let Some(p) = index.pick(&family, b, i) {
                loaded += 1;
                let _ = (p.face.face().glyph_id(u32::from('a')), p.face.cost());
            }
        }
    }
    eprintln!("scan of {written} mutated files: {} faces indexed, {picks} picks, {loaded} loaded", index.entries().len());
    assert!(index.entries().len() > 300 && loaded > 100);
}

#[test]
fn mutated_collections_never_panic_the_face_extraction() {
    let ttcs: Vec<Vec<u8>> = corpus_files().into_iter().filter(|(r, _)| r.ends_with(".ttc")).map(|(_, b)| b).collect();
    assert_eq!(ttcs.len(), 2);
    let limits = FontLimits { max_output_bytes: 1 << 20, ..FontLimits::NATIVE };
    let (mut ok, mut err) = (0, 0);
    for (n, ttc) in ttcs.iter().enumerate() {
        for m in mutate(ttc, 0x7C7C + n as u32, 600) {
            for index in [0u32, 1, 2, 3, 5, 64, u32::MAX] {
                match ttc_face_by_index(&m, index, &limits) {
                    Ok(face) => {
                        ok += 1;
                        let _ = Face::parse(Arc::from(face), &limits);
                    }
                    Err(_) => err += 1,
                }
            }
        }
    }
    eprintln!("extraction over mutated collections: {ok} extracted, {err} refused");
    assert!(ok > 100 && err > 100);
}

#[test]
fn mutated_advance_tables_never_panic_the_parser() {
    let base = r#"{"10.5":{"a":91.5," ":46,"é":90.25,"😀":3},"11":{"a":-0.5e1,"b":1E+2,"c":7,"x":[1,{"y":null}]},"12":{}, "z":"s", "n":null, "t":true}"#;
    let mut rng = Rng::new(0x1234_5678);
    let (mut ok, mut err) = (0, 0);
    for round in 0..30_000 {
        let mut b = base.as_bytes().to_vec();
        for _ in 0..1 + rng.below(6) {
            let i = rng.below(b.len());
            match rng.below(5) {
                0 => b[i] = b"{}[]\":,-.e\\u0123 tfn"[rng.below(20)],
                1 => {
                    b.remove(i);
                }
                2 => b.insert(i, b"{}[]\":,\\"[rng.below(8)]),
                3 => b.truncate(i),
                _ => b[i] ^= 1 << rng.below(8),
            }
            if b.is_empty() {
                b.push(b'{');
            }
        }
        // the parser takes text: invalid UTF-8 is the caller's `from_utf8`; test the lossy text
        let text = String::from_utf8_lossy(&b);
        match AdvanceTable::parse_json(&text) {
            Ok(t) => {
                ok += 1;
                let _ = (t.get("11", 97), t.get("10.5", 0x1F600), t.heap_bytes(), t.len());
            }
            Err(_) => err += 1,
        }
        if round % 10_000 == 0 {
            let _ = AdvanceTable::parse_json(&format!("{}1{}", "[".repeat(round / 100), "]".repeat(round / 100)));
        }
    }
    eprintln!("advance tables: {ok} parsed, {err} refused over 30,000 mutations");
    assert!(ok > 1000 && err > 1000);
}

#[test]
fn mutated_fonts_pushed_into_a_registry_never_panic_and_release_everything() {
    let cache = Arc::new(FaceCache::new(FontBudget::DESKTOP, FontLimits::NATIVE));
    let registry = Arc::new(FontRegistry::new(FontBudget { pinned_bytes: 4 << 20, cache_bytes: 1 << 20 }, Arc::clone(&cache)));
    let mut rng = Rng::new(0xABCD);
    let (mut asks, mut answered, mut refused) = (0usize, 0usize, 0usize);
    for (n, (_, font)) in pool_files().into_iter().enumerate() {
        for (k, m) in mutate(&font, 0x3131 + u32::try_from(n).unwrap(), 150).into_iter().enumerate() {
            let m: Arc<[u8]> = Arc::from(m);
            let v = [Variant::Regular, Variant::Bold, Variant::Italic, Variant::BoldItalic][rng.below(4)];
            let name = format!("Fam {}", k % 7);
            let source = [FontSource::Host, FontSource::Substitute, FontSource::Default, FontSource::Installed][rng.below(4)];
            let mut face = HostFace::new(v, Arc::clone(&m));
            if rng.below(6) == 0 {
                face.ttc_index = Some(u32::try_from(rng.below(4)).unwrap());
            }
            let pushed = if rng.below(4) == 0 {
                registry.push_asset(&name, face)
            } else {
                registry.push_family(&name, source, rng.below(3) != 0, vec![face])
            };
            refused += usize::from(pushed.is_err());
            let p = ViewerProvider::new(Arc::clone(&registry));
            for _ in 0..3 {
                asks += 1;
                if let Some(a) = p.face(&name, rng.below(2) == 0, rng.below(2) == 0) {
                    answered += 1;
                    let _ = (a.face().glyph_id(97), a.face().advance_width(1), a.face().kern_units(1, 2), a.face().heap_bytes());
                }
            }
            let _ = registry.take_missing();
        }
    }
    eprintln!("registry over mutated fonts: {asks} asks, {answered} answered, {refused} pushes refused");
    registry.clear();
    cache.clear();
    assert_eq!(registry.pinned_bytes(), 0, "everything pushed is released");
    assert_eq!(cache.stats().entries, 0);
    assert!(answered > 100);
}
