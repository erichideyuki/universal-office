//! The memory gate of frontier P (registry, policies, system index), measured with `uo-alloc`:
//! M1 (live bytes return exactly), M2 (soaks flat), M3 (what the operations cost, and how well
//! `Face::heap_bytes` predicts what a face keeps), M5 (every cache filled past its cap). Run
//! `cargo test --release -p uo-fonts --features system --test registry_memory -- --nocapture
//! --test-threads=1` to see the numbers.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::sync::Arc;

use provider_support::{named_font, pool_files, repo_root};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak, thread_stats};
use uo_fonts::policy::{OracleProvider, SubstituteFontProvider, SystemFontProvider, ViewerProvider};
use uo_fonts::registry::{
    AdvanceTable, AdvanceTableError, ContentKey, FaceCache, FaceLedger, FaceSource, FontBudget, FontProvider, FontRegistry, FontSource,
    HostFace, MAX_ALIASES, MAX_ATTEMPTED, MAX_MISSING_FAMILIES, MAX_UNPINNED_FACES, SourceFace, ttc_face_by_index,
};
use uo_fonts::{Face, FontLimits, Variant, clear_caches};
use uo_testkit::generate::sfnt::{SfntOptions, build_ttc};

fn big() -> FontBudget {
    FontBudget { pinned_bytes: u64::MAX, cache_bytes: u64::MAX }
}

fn small_font(n: u16) -> Arc<[u8]> {
    Arc::from(named_font(&[(0x409, 1, "Mem Fam"), (0x409, 2, "Regular")], 400, 0x40, n).build())
}

fn carlito(style: &str) -> Arc<[u8]> {
    Arc::from(std::fs::read(repo_root().join(format!("shared/fonts/files/carlito-{style}.ttf"))).unwrap())
}

fn faces4() -> Vec<HostFace> {
    [Variant::Regular, Variant::Bold, Variant::Italic, Variant::BoldItalic]
        .into_iter()
        .zip(1u16..)
        .map(|(v, n)| HostFace::new(v, small_font(n)))
        .collect()
}

struct Fake {
    cache: Arc<FaceCache>,
    fonts: Vec<(String, Arc<[u8]>)>,
}

impl FaceSource for Fake {
    fn pick(&self, family: &str, bold: bool, italic: bool) -> Option<SourceFace> {
        let bytes = &self.fonts.iter().find(|(f, _)| *f == family.to_lowercase())?.1;
        Some(SourceFace {
            key: format!("{family}||{family}.ttf#0"),
            family: family.to_owned(),
            face: self.cache.get_or_parse(Arc::clone(bytes)).ok()?,
            synth_bold: bold,
            synth_italic: italic,
        })
    }
}

#[test]
fn m1_the_cache_the_ledger_and_the_tables() {
    clear_caches();
    let (a, b, garbage) = (small_font(1), small_font(2), Arc::<[u8]>::from(vec![7u8; 64]));
    assert_no_leak(|| {
        let c = FaceCache::new(big(), FontLimits::NATIVE);
        let h1 = c.get_or_parse(Arc::clone(&a)).unwrap();
        let h2 = c.get_or_parse(Arc::clone(&a)).unwrap();
        let h3 = c.get_or_parse(Arc::clone(&b)).unwrap();
        assert!(c.get_or_parse(Arc::clone(&garbage)).is_err());
        assert!(c.get_or_load("x#0", || Some(b.to_vec())).is_some());
        assert!(c.get_or_load("y#0", || None).is_none());
        let _ = (c.stats(), h1.cost(), h2.cost(), h3.cost(), h3.content_key().short_hex());
        c.trim_now();
        c.clear();
    });
    assert_no_leak(|| {
        let ledger = FaceLedger::new(1000);
        let t = ledger.charge(ContentKey::of(b"one"), 600).unwrap();
        let t2 = ledger.charge(ContentKey::of(b"one"), 600).unwrap();
        assert!(ledger.charge(ContentKey::of(b"two"), 600).is_none());
        let _ = (t.cost(), t2.cost(), ledger.used());
    });
    assert_no_leak(|| {
        let t = AdvanceTable::parse_json(r#"{"11":{"a":1.5,"é":2},"12":{"😀":3},"x":[1,{"a":2}]}"#).unwrap();
        let _ = (t.get("11", 97), t.heap_bytes(), t.len());
        let mut built = AdvanceTable::new();
        built.insert("11", 97, 1.0).unwrap();
        for bad in ["", "[", r#"{"11":{"a":}}"#, r#"{"11":{"a":1"#] {
            assert_eq!(AdvanceTable::parse_json(bad), Err(AdvanceTableError::Syntax));
        }
    });
    let ttc =
        build_ttc(&[(named_font(&[(0x409, 1, "T"), (0x409, 2, "Regular")], 400, 0x40, 1).tables, SfntOptions::default())], 0x0001_0000);
    assert_no_leak(|| {
        (
            ttc_face_by_index(&ttc, 0, &FontLimits::NATIVE),
            ttc_face_by_index(&ttc, 9, &FontLimits::NATIVE),
            ttc_face_by_index(&ttc[..5], 0, &FontLimits::NATIVE),
        )
    });
    assert_no_leak(|| Face::parse(Arc::clone(&a), &FontLimits::NATIVE).unwrap().heap_bytes());
}

#[test]
fn m1_the_registry_and_the_viewer_policy() {
    clear_caches();
    let pool = pool_files();
    let garbage: Arc<[u8]> = Arc::from(pool.iter().find(|(n, _)| *n == "garbage").unwrap().1.clone());
    let table = Arc::new(AdvanceTable::parse_json(r#"{"11":{"a":123.5}}"#).unwrap());
    assert_no_leak(|| {
        let cache = Arc::new(FaceCache::new(big(), FontLimits::NATIVE));
        let reg = Arc::new(FontRegistry::new(big(), Arc::clone(&cache)));
        reg.push_family("Calibri", FontSource::Installed, false, faces4()).unwrap();
        reg.push_family("Carlito", FontSource::Bundled, true, faces4()).unwrap();
        reg.push_family("Bad", FontSource::Host, true, vec![HostFace::new(Variant::Regular, Arc::clone(&garbage))]).unwrap();
        let mut asset = HostFace::new(Variant::Regular, small_font(9));
        asset.advances = Some(Arc::clone(&table));
        reg.push_asset("Aptos", asset).unwrap();
        assert!(reg.push_asset("Aptos", HostFace::new(Variant::Bold, Arc::clone(&garbage))).is_err());
        {
            let p = ViewerProvider::new(Arc::clone(&reg));
            for (f, b, i) in [
                ("Calibri", false, false),
                ("calibri", true, true),
                ("Aptos", true, false),
                ("Bad", false, false),
                ("Nope", false, true),
                ("Nope", false, true),
            ] {
                let _ = p.face(f, b, i);
            }
        }
        let _ = (reg.missing(), reg.take_missing(), reg.take_missing(), reg.generation(), reg.pinned_bytes(), reg.counts());
        reg.remove_family("carlito");
        reg.remove_asset("aptos");
        reg.clear();
        assert_eq!(reg.pinned_bytes(), 0);
        cache.clear();
    });
}

#[test]
fn m1_the_system_fed_policies() {
    clear_caches();
    let cache = Arc::new(FaceCache::new(big(), FontLimits::NATIVE));
    let fake = Arc::new(Fake {
        cache: Arc::clone(&cache),
        fonts: vec![("arial".to_owned(), small_font(1)), ("carlito".to_owned(), small_font(2))],
    });
    // one warm-up: the first use of the thread-local and hash seeds is not the operation's
    let run = |fake: &Arc<Fake>| {
        let source = Arc::clone(fake) as Arc<dyn FaceSource>;
        let ledger = FaceLedger::new(u64::MAX);
        let system = Arc::new(SystemFontProvider::new(Arc::clone(&source), Some(Arc::clone(&ledger))));
        let server = SubstituteFontProvider::new(Arc::clone(&system) as Arc<dyn FontProvider>);
        let oracle = OracleProvider::new(source, Some(ledger), &["x"]);
        let mut held = Vec::new();
        for (f, b, i) in [
            ("Arial", false, false),
            ("ARIAL", true, false),
            ("Calibri", false, true),
            ("Calibri", false, true),
            ("Nope", true, true),
            ("x", false, false),
        ] {
            held.push(system.face(f, b, i));
            held.push(server.face(f, b, i));
            held.push(oracle.face(f, b, i));
        }
        let _ = (system.remembered(), oracle.remembered());
    };
    run(&fake);
    cache.clear();
    assert_no_leak(|| {
        run(&fake);
        cache.clear();
    });
}

#[cfg(all(feature = "system", not(target_family = "wasm")))]
#[test]
fn m1_the_system_index() {
    use provider_support::{TempDir, write_corpus};
    use uo_fonts::system::SystemFontIndex;
    let dir = TempDir::new("m1-index");
    write_corpus(&dir.0);
    let scan = || {
        let cache = Arc::new(FaceCache::new(big(), FontLimits::NATIVE));
        let index = Arc::new(SystemFontIndex::with_roots(vec![dir.0.clone(), repo_root().join("shared/fonts/files")], cache));
        let mut picked = Vec::new();
        for f in index.families() {
            picked.push(index.pick(&f, true, false));
            picked.push(index.pick(&f.to_uppercase(), false, true));
        }
        let provider = OracleProvider::new(Arc::clone(&index) as Arc<dyn FaceSource>, None, &[]);
        let _ = provider.face("Carlito", true, true);
        let _ = (index.entries().len(), index.roots().len());
        drop(picked);
        index.cache().clear();
    };
    scan();
    let usage = assert_no_leak(scan);
    eprintln!(
        "M1 system index over the corpus and shared/fonts/files: allocated {} B, peak {} B above the start, 0 B left",
        usage.allocated, usage.peak_above_start
    );
    // the process-wide index and cache: released and cleared, back to the start
    SystemFontIndex::release_shared();
    clear_caches();
    let usage = assert_no_leak(|| {
        let shared = SystemFontIndex::rebuild_shared(std::slice::from_ref(&dir.0));
        assert!(shared.pick("Test Sans", false, false).is_some());
        drop(shared);
        SystemFontIndex::release_shared();
        clear_caches();
    });
    eprintln!("M1 shared index: allocated {} B, peak {} B, 0 B left", usage.allocated, usage.peak_above_start);
}

#[test]
fn m5_every_cache_filled_past_its_cap() {
    clear_caches();
    // faces: the entry cap (bytes budget infinite) and the alias cap
    let cache = FaceCache::new(big(), FontLimits::NATIVE);
    let font = small_font(1);
    for i in 0..MAX_ALIASES + 100 {
        let n = u16::try_from(i % 200).unwrap();
        cache.get_or_load(&format!("path-{i}#0"), || Some(small_font(n).to_vec())).unwrap();
    }
    cache.trim_now();
    let st = cache.stats();
    assert!(st.entries <= MAX_UNPINNED_FACES, "{} entries", st.entries);
    assert!(st.aliases <= MAX_ALIASES, "{} aliases", st.aliases);
    cache.clear();
    assert_eq!((cache.stats().entries, cache.stats().aliases), (0, 0));
    let held = cache.get_or_parse(Arc::clone(&font)).unwrap();
    drop(held);

    // answers of the system-fed policies and the viewer's per-call cache
    let source = Arc::new(Fake { cache: Arc::new(FaceCache::new(big(), FontLimits::NATIVE)), fonts: Vec::new() }) as Arc<dyn FaceSource>;
    let (system, oracle) = (SystemFontProvider::new(Arc::clone(&source), None), OracleProvider::new(source, None, &[]));
    let ((), usage) = measure(|| {
        for i in 0..uo_fonts::registry::MAX_ANSWER_SLOTS * 3 {
            let _ = system.face(&format!("fam {i}"), false, false);
            let _ = oracle.face(&format!("fam {i}"), true, true);
        }
    });
    assert!(system.remembered() <= uo_fonts::registry::MAX_ANSWER_SLOTS && oracle.remembered() <= uo_fonts::registry::MAX_ANSWER_SLOTS);
    let per_slot = usage.live_delta / i64::try_from(system.remembered() + oracle.remembered()).unwrap();
    eprintln!("M5 answer caches: {} + {} slots kept, {} B each", system.remembered(), oracle.remembered(), per_slot);
    assert!(per_slot < 200, "a remembered miss is a key and a marker");

    let reg = Arc::new(FontRegistry::new(big(), Arc::new(FaceCache::new(big(), FontLimits::NATIVE))));
    let viewer = ViewerProvider::new(Arc::clone(&reg));
    for i in 0..uo_fonts::policy::MAX_VIEWER_REQUESTS + 500 {
        assert!(viewer.face(&format!("family {i}"), false, false).is_none());
    }
    assert_eq!(viewer.remembered(), uo_fonts::policy::MAX_VIEWER_REQUESTS);
    assert_eq!(reg.missing().len(), MAX_MISSING_FAMILIES, "the notes stop at their cap until the host takes them");
    assert_eq!(reg.take_missing().len(), MAX_MISSING_FAMILIES);
    // the attempts: past the cap the set starts again instead of growing
    for round in 0..(MAX_ATTEMPTED / MAX_MISSING_FAMILIES) + 2 {
        for i in 0..MAX_MISSING_FAMILIES {
            let _ = ViewerProvider::new(Arc::clone(&reg)).face(&format!("round {round} family {i}"), false, false);
        }
        assert!(!reg.take_missing().is_empty());
    }

    // advance tables: the entry cap
    let chars: Vec<char> = (0x23u8..=0x5B).chain(0x5D..=0x7E).map(char::from).collect();
    let body = chars.iter().map(|c| format!("\"{c}\":1")).collect::<Vec<_>>().join(",");
    let mut json = String::from("{");
    for size in 0..11_500 {
        json.push_str(&format!("{}\"{size}\":{{{body}}}", if size == 0 { "" } else { "," }));
    }
    json.push('}');
    assert!(json.len() < uo_fonts::registry::MAX_ADVANCE_JSON_BYTES, "{} bytes of JSON", json.len());
    let (res, usage) = measure(|| AdvanceTable::parse_json(&json));
    assert_eq!(res, Err(AdvanceTableError::TooLarge));
    eprintln!("M5 advance table past its entry cap: refused, peak {} B above the start", usage.peak_above_start);
    assert!(usage.peak_above_start < 100 << 20);
    assert_eq!(AdvanceTable::parse_json(&" ".repeat(uo_fonts::registry::MAX_ADVANCE_JSON_BYTES + 1)), Err(AdvanceTableError::TooLarge));
}

#[test]
fn m5_the_document_ledger_and_the_cache_budget_hold_real_fonts() {
    clear_caches();
    // the phone profile: pushing real faces past 48 MiB pinned is refused; the cache keeps at most 16 MiB unpinned
    let faces: Vec<Arc<[u8]>> = ["regular", "bold", "italic", "bolditalic"].iter().map(|s| carlito(s)).collect();
    let one = faces[0].len() as u64;
    let phone = FontBudget::PHONE;
    let cache = Arc::new(FaceCache::new(phone, FontLimits::WASM32));
    let reg = FontRegistry::new(phone, Arc::clone(&cache));
    let mut pushed = 0u64;
    let mut refused = 0;
    for i in 0..200 {
        // a distinct file each time: the same bytes with one changed byte in a name-table padding position
        let mut bytes = faces[i % 4].to_vec();
        let at = bytes.len() - 1 - i;
        bytes[at] ^= 0x5A;
        match reg.push_family(&format!("Family {i}"), FontSource::Host, true, vec![HostFace::new(Variant::Regular, Arc::from(bytes))]) {
            Ok(()) => pushed += 1,
            Err(_) => refused += 1,
        }
    }
    let used = reg.pinned_bytes();
    eprintln!("M5 phone profile with {} KiB faces: {pushed} pushed, {refused} refused, {} MiB pinned of 48", one / 1024, used >> 20);
    assert!(used <= phone.pinned_bytes);
    assert!(refused > 0 && used + 800 * 1024 > phone.pinned_bytes, "the budget is filled to within one face (the largest is 797 KiB)");
    assert!(one > 0);
    // answers parse the faces lazily: all of them held, the cache pins every one
    let p = ViewerProvider::new(Arc::new(reg));
    let _held: Vec<_> = (0..pushed).filter_map(|i| p.face(&format!("Family {i}"), false, false)).collect();
    let st = cache.stats();
    eprintln!("M5 phone: {} faces pinned in the cache, {} MiB (bytes + heap)", st.pinned_entries, st.pinned_bytes >> 20);
    assert_eq!(st.cached_bytes, 0, "nothing unpinned is kept past the budget");
}

#[test]
fn m3_heap_bytes_against_what_a_face_keeps() {
    clear_caches();
    let limits = FontLimits::NATIVE;
    for style in ["regular", "bold", "italic", "bolditalic"] {
        let bytes = carlito(style);
        let (face, parsed) = measure(|| Face::parse(Arc::clone(&bytes), &limits).unwrap());
        let predicted_parse = face.heap_bytes() as i64;
        let a = face.glyph_id(u32::from('A'));
        let v = face.glyph_id(u32::from('V'));
        let (_, kern) = measure(|| face.kern_units(a, v));
        let after_kern = face.heap_bytes() as i64;
        let (_, lig) = measure(|| face.ligate(&[face.glyph_id(u32::from('f')), face.glyph_id(u32::from('i'))], &[uo_fonts::Tag(*b"liga")]));
        let after_lig = face.heap_bytes() as i64;
        let (_, outline) = measure(|| (0..64).filter_map(|g| face.glyph_outline(g)).count());
        let after_outline = face.heap_bytes() as i64;
        eprintln!(
            "M3 Carlito {style} ({} KiB): parse kept {} B (heap_bytes {predicted_parse}); kerning kept {} B (heap_bytes +{}); ligatures kept {} B (+{}); 64 outlines kept {} B (+{})",
            bytes.len() / 1024,
            parsed.live_delta,
            kern.live_delta,
            after_kern - predicted_parse,
            lig.live_delta,
            after_lig - after_kern,
            outline.live_delta,
            after_outline - after_lig
        );
        let close = |measured: i64, predicted: i64| (measured - predicted).abs() <= measured / 5 + 256;
        assert!(close(parsed.live_delta, predicted_parse), "parse: kept {} predicted {predicted_parse}", parsed.live_delta);
        assert!(
            close(kern.live_delta, after_kern - predicted_parse),
            "kerning: kept {} predicted {}",
            kern.live_delta,
            after_kern - predicted_parse
        );
        assert!(close(lig.live_delta, after_lig - after_kern), "ligatures: kept {} predicted {}", lig.live_delta, after_lig - after_kern);
        assert!(
            close(outline.live_delta, after_outline - after_lig),
            "outlines: kept {} predicted {}",
            outline.live_delta,
            after_outline - after_lig
        );
        assert!(kern.live_delta > 0 && outline.live_delta > 0);
    }
}

#[test]
fn m3_what_the_operations_cost() {
    clear_caches();
    let bytes = carlito("regular");
    let limits = FontLimits::NATIVE;
    let cache = Arc::new(FaceCache::new(big(), limits));
    let reg = Arc::new(FontRegistry::new(big(), Arc::clone(&cache)));
    let faces: Vec<HostFace> = ["regular", "bold", "italic", "bolditalic"]
        .iter()
        .zip([Variant::Regular, Variant::Bold, Variant::Italic, Variant::BoldItalic])
        .map(|(s, v)| HostFace::new(v, carlito(s)))
        .collect();
    let (_, push) = measure(|| reg.push_family("Calibri", FontSource::Installed, true, faces).unwrap());
    let provider = ViewerProvider::new(Arc::clone(&reg));
    let (first, first_cost) = measure(|| provider.face("Calibri", true, true).unwrap());
    let (_, again) = measure(|| provider.face("Calibri", true, true).unwrap());
    let (_, new_call) = measure(|| ViewerProvider::new(Arc::clone(&reg)).face("Calibri", false, false).unwrap());
    let (_, hit) = measure(|| cache.get_or_parse(Arc::clone(&bytes)).unwrap());
    let (_, hash) = measure(|| ContentKey::of(&bytes));
    eprintln!(
        "M3 registry over Carlito (4 faces of {} KiB): push_family allocated {} B (the shared buffers are the host's; peak {} B); first answer allocated {} B kept {} B (the parse); repeated ask allocated {} B; a new call's answer {} B; a cache hit {} B; hashing a face {} B",
        bytes.len() / 1024,
        push.allocated,
        push.peak_above_start,
        first_cost.allocated,
        first_cost.live_delta,
        again.allocated,
        new_call.allocated,
        hit.allocated,
        hash.allocated
    );
    let started = std::time::Instant::now();
    for _ in 0..20 {
        std::hint::black_box(ContentKey::of(&bytes));
    }
    let mib_s = 20.0 * bytes.len() as f64 / (1 << 20) as f64 / started.elapsed().as_secs_f64();
    eprintln!("M3 content key (SHA-256) of a {} KiB face: {mib_s:.0} MiB/s", bytes.len() / 1024);
    if !cfg!(debug_assertions) {
        assert!(mib_s > 20.0, "hashing a font at push time must not dominate it");
    }
    assert!(first.cached_face().cost() > bytes.len() as u64);
    assert_eq!(hash.allocated, 0, "SHA-256 over a buffer allocates nothing");
    assert_eq!(again.allocated, "Calibri".len() as u64, "a repeated ask is a map hit; the key string of the request is all it allocates");
    assert!(push.allocated < 4096, "pushing shares the host's buffers: only the records are allocated");
}

#[test]
fn m2_soak_viewer_cycle_is_flat_and_returns_to_the_start() {
    clear_caches();
    let cache = Arc::new(FaceCache::new(FontBudget { pinned_bytes: u64::MAX, cache_bytes: 1 << 20 }, FontLimits::NATIVE));
    let reg = Arc::new(FontRegistry::new(big(), Arc::clone(&cache)));
    let pool: Vec<Arc<[u8]>> = (1..=8).map(small_font).collect();
    let cycle = |i: usize| {
        let faces: Vec<HostFace> = [Variant::Regular, Variant::Bold, Variant::Italic, Variant::BoldItalic]
            .into_iter()
            .enumerate()
            .map(|(k, v)| HostFace::new(v, Arc::clone(&pool[(i + k) % pool.len()])))
            .collect();
        reg.push_family(&format!("Family {}", i % 7), FontSource::Host, !i.is_multiple_of(3), faces).unwrap();
        let p = ViewerProvider::new(Arc::clone(&reg));
        for k in 0..6 {
            let _ = p.face(&format!("Family {}", (i + k) % 9), k % 2 == 0, k % 3 == 0);
        }
        if i.is_multiple_of(5) {
            let _ = reg.take_missing();
        }
        cache.trim_now();
    };
    let rounds = std::env::var("UO_FONTS_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    for i in 0..50 {
        cycle(i);
    }
    let series = soak(rounds, 50, |i| cycle(i + 50));
    eprintln!("M2 viewer cycle, {rounds} rounds: live bytes {:?}", series.iter().take(6).collect::<Vec<_>>());
    assert_flat(&series, 2, 128 << 10);
    let before = thread_stats().live;
    reg.clear();
    cache.clear();
    eprintln!("M2 after clear: {} B released of {}", before - thread_stats().live, before);
}

#[test]
fn m2_soak_cache_under_budget_pressure_is_flat() {
    clear_caches();
    let one = small_font(1);
    let cost = FaceCache::new(big(), FontLimits::NATIVE).get_or_parse(Arc::clone(&one)).unwrap().cost();
    let cache = FaceCache::new(FontBudget { pinned_bytes: u64::MAX, cache_bytes: cost * 8 }, FontLimits::NATIVE);
    for i in 0..200u16 {
        cache.get_or_parse(small_font(i)).unwrap();
    }
    let series = soak(1000, 50, |i| {
        let n = u16::try_from((i * 7) % 300).unwrap();
        let held = cache.get_or_parse(small_font(n)).unwrap();
        let _ = held.face().kern_units(1, 2);
        if i % 10 == 0 {
            cache.trim_now();
        }
    });
    assert_flat(&series, 2, 64 << 10);
    let st = cache.stats();
    eprintln!(
        "M2 cache under pressure: {} entries, {} KiB cached of {} KiB budget, {} evictions",
        st.entries,
        st.cached_bytes >> 10,
        (cost * 8) >> 10,
        st.evictions
    );
    assert!(st.cached_bytes <= cost * 9);
}
