//! Stage 4 memory gate of `uo-fonts`, measured with `uo-alloc` (release): M1 over the real open
//! fonts and every error and limit path, M2 soaks (fresh at 0 bytes, long-lived within the caches),
//! M3 budgets per face and per operation, M5 caches filled past their caps, and the worst case per
//! limit with hostile fonts. Run `cargo test --release -p uo-fonts --test stage4_memory --
//! --nocapture --test-threads=1` to see the numbers; `UO_FONTS_SOAK` overrides the 1,000 rounds of
//! the soaks, `UO_LOCAL_FONTS` (default `C:/Windows/Fonts`) is the opt-in corpus of the `#[ignore]`d
//! tests (never committed, numbers only in the module record).
#![allow(clippy::unwrap_used, clippy::panic)]

mod memory_support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use memory_support::*;
use uo_alloc::{assert_flat, assert_no_leak, measure, soak, thread_stats};
use uo_fonts::{
    Face, FontError, FontLimits, FontProgram, HyphenSession, HyphenationRegistry, Hyphenator, LimitKind, OutlineCmd, clear_caches,
    face_info, glyph_ids_for, odttf, sfnt_to_eot, standalone_face, subset_true_type,
};

/// The first record of stage 4 (Carlito Bold, native limits, `uo-alloc` counts): later changes may be at most 10% worse.
const BASELINE_CARLITO_KERN_PEAK: i64 = 3_590_000;
const BASELINE_CARLITO_KERN_ALLOCATED: u64 = 5_280_000;
const BASELINE_DE1996_HYPHENATOR_KEPT: i64 = 2_190_000;

const PATTERN_IDS: [&str; 7] = ["pt", "en-us", "es", "fr", "de-1996", "it", "nl"];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn open_fonts() -> Vec<(String, Vec<u8>)> {
    let dir = manifest().join("../../../shared/fonts/files");
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            let name = p.file_name()?.to_str()?.to_owned();
            name.ends_with(".ttf").then(|| (name, std::fs::read(&p).unwrap()))
        })
        .collect();
    out.sort();
    out
}

fn pattern_texts() -> Vec<String> {
    PATTERN_IDS.iter().map(|id| std::fs::read_to_string(manifest().join(format!("data/hyphen/{id}.txt"))).unwrap()).collect()
}

fn corpus() -> Corpus {
    Corpus::new(open_fonts(), pattern_texts())
}

fn rounds() -> usize {
    std::env::var("UO_FONTS_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(if cfg!(debug_assertions) { 40 } else { 1_000 })
}

fn kib(n: i64) -> String {
    format!("{:.1} KB", n as f64 / 1024.0)
}

fn mib(n: i64) -> String {
    format!("{:.2} MB", n as f64 / 1_048_576.0)
}

// ---------------------------------------------------------------------------------------
// M1
// ---------------------------------------------------------------------------------------

#[test]
fn m1_every_operation_on_every_real_and_generated_face_leaks_nothing() {
    let c = corpus();
    clear_caches();
    let limits = FontLimits::DEFAULT;
    for (name, bytes) in c.names.iter().zip(&c.faces) {
        let b = bytes.clone();
        assert_no_leak(|| Face::parse(b.clone(), &limits).map(|f| f.num_glyphs()));
        assert_no_leak(|| {
            let f = Face::parse(b.clone(), &limits).unwrap();
            (query_face(&f, 0), query_face(&f, 0), query_face(&f, 3_000))
        });
        let face = Face::parse(b.clone(), &limits).unwrap();
        let gids = text_gids(&face);
        let cps: Vec<u32> = ORACLE_TEXT.chars().map(|c| c as u32).collect();
        assert_no_leak(|| face_info(&b).map(|i| i.names.family.len()));
        assert_no_leak(|| glyph_ids_for(&b, cps.iter().copied(), &limits).map(|v| v.len()));
        assert_no_leak(|| FontProgram::parse(b.clone(), &limits).map(|p| (p.units_per_em(), p.legacy_kern(1, 2), p.advance_width(3))));
        assert_no_leak(|| subset_true_type(&b, gids.iter().copied(), true, &limits).len());
        assert_no_leak(|| subset_true_type(&b, gids.iter().copied(), false, &limits).len());
        assert_no_leak(|| sfnt_to_eot(&b, false).map(|v| v.len()));
        assert_no_leak(|| sfnt_to_eot(&b, true).map(|v| v.len()));
        let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
        assert_no_leak(|| odttf::deobfuscate(&odttf::obfuscate(&b, key), key).len());
        assert_no_leak(|| standalone_face(&b, None, &limits).map(|f| f.len()));
        assert_no_leak(|| bytes_ops(&face, &b, &limits));
        eprintln!("# M1 ok: {name} ({} KB)", b.len() / 1024);
    }
    assert_no_leak(|| standalone_face(&c.ttc, None, &limits).map(|f| f.len()));
    assert_no_leak(|| standalone_face(&c.ttc, Some("Missing-PS"), &limits).map(|f| f.len()));
    assert_no_leak(|| standalone_face(&c.ttc[..c.ttc.len() / 2], None, &limits).map(|f| f.len()));
    assert_no_leak(|| embedding(&c, &limits));
}

#[test]
fn m1_hyphenators_registry_and_sessions_of_the_seven_languages_leak_nothing() {
    let texts = pattern_texts();
    let words = pool();
    for (id, text) in PATTERN_IDS.iter().zip(&texts) {
        assert_no_leak(|| parse_and_build(text).id().len());
        let h = Arc::new(parse_and_build(text));
        assert_no_leak(|| words.iter().map(|w| h.points(w).len() + h.hyphenate(w, 2, 3).len()).sum::<usize>());
        assert_no_leak(|| {
            let mut s = HyphenSession::new(h.clone());
            words.iter().map(|w| s.hyphenate(w, 2, 3).len() + s.points(w).len()).sum::<usize>() + s.cached_words()
        });
        eprintln!("# M1 ok: hyphenator {id}");
    }
    assert_no_leak(|| {
        let mut r = HyphenationRegistry::new();
        for t in &texts {
            r.register(&uo_fonts::hyphen::parse_patterns(t).unwrap());
        }
        (r.ids().len(), r.for_lang(Some("pt-BR")).map(|h| h.id().len()), r.contains("es"), r.get("nl").is_some())
    });
    assert_no_leak(|| {
        let mut r = HyphenationRegistry::new();
        r.register(&uo_fonts::hyphen::parse_patterns(&texts[0]).unwrap());
        uo_fonts::pattern_ids_to_load([Some("pt"), Some("de"), None, Some("xx")], &r)
    });
}

fn parse_and_build(text: &str) -> Hyphenator {
    Hyphenator::new(&uo_fonts::hyphen::parse_patterns(text).unwrap())
}

#[test]
fn m1_limit_and_error_paths_over_real_fonts_leak_nothing() {
    let fonts = open_fonts();
    let carlito: Arc<[u8]> = Arc::from(fonts.iter().find(|(n, _)| n == "carlito-regular.ttf").unwrap().1.clone());
    let mono: Arc<[u8]> = Arc::from(fonts.iter().find(|(n, _)| n == "liberationmono-regular.ttf").unwrap().1.clone());
    let run = |bytes: &Arc<[u8]>, limits: &FontLimits| {
        Face::parse(bytes.clone(), limits).map(|f| {
            let _ = (query_face(&f, 0), f.kern_units(40, 41), f.kern_in_latin());
            (bytes_ops(&f, bytes, limits), f.num_glyphs())
        })
    };
    let base = FontLimits::NATIVE;
    let mut failures = 0;
    for bytes in [&carlito, &mono] {
        for work in [0u64, 1, 2, 3, 5, 10, 50, 200, 1_000, 10_000, 100_000, 1_000_000] {
            assert_no_leak(|| run(bytes, &FontLimits { max_table_work: work, ..base }));
        }
        for pairs in [0u32, 1, 10, 1_000, 100_000] {
            assert_no_leak(|| run(bytes, &FontLimits { max_kern_pairs: pairs, ..base }));
        }
        for outline_work in [0u32, 1, 2, 3] {
            assert_no_leak(|| run(bytes, &FontLimits { max_outline_work: outline_work, ..base }));
        }
        for points in [0u32, 1, 10, 100] {
            assert_no_leak(|| run(bytes, &FontLimits { max_outline_points: points, ..base }));
        }
        for ranges in [0u32, 1, 10, 100] {
            assert_no_leak(|| run(bytes, &FontLimits { max_cmap_ranges: ranges, ..base }));
        }
        for entries in [0u32, 1, 7] {
            assert_no_leak(|| run(bytes, &FontLimits { outline_cache_entries: entries, ..base }));
        }
        for output in [0u64, 100, 10_000, 100_000] {
            assert_no_leak(|| run(bytes, &FontLimits { max_output_bytes: output, ..base }));
        }
        // truncations: every kind of cut, the directory, each table, the middle of the file
        let n = bytes.len();
        let mut cuts: Vec<usize> = vec![0, 1, 4, 11, 12, 20, 100, 400];
        cuts.extend((1..=40).map(|k| n * k / 41));
        cuts.extend([n - 1, n - 2, n - 100]);
        for cut in cuts {
            let cut_bytes: Arc<[u8]> = Arc::from(bytes[..cut].to_vec());
            assert_no_leak(|| {
                if run(&cut_bytes, &base).is_err() {
                    failures += 1;
                }
            });
        }
    }
    assert!(failures > 0, "some truncation must fail to parse");
    let e = Face::parse(carlito.clone(), &FontLimits { max_table_work: 0, ..base });
    eprintln!("# work budget 0 on Carlito: {:?}", e.as_ref().err());
    assert!(matches!(e.as_ref().err(), None | Some(FontError::Limit(LimitKind::TableWork))));
}

// ---------------------------------------------------------------------------------------
// M2
// ---------------------------------------------------------------------------------------

#[test]
fn m2_fresh_soak_is_flat_at_zero_bytes() {
    let c = corpus();
    let n = rounds();
    let t = Instant::now();
    fresh_round(&c, 0);
    fresh_round(&c, 1);
    let one = t.elapsed() / 2;
    let series = soak(n, 50, |i| {
        std::hint::black_box(fresh_round(&c, i));
    });
    eprintln!(
        "# M2 fresh: {n} rounds ({} faces, 7 hyphenation languages, {} words each) in {:?} ({one:?} per round); live every 50: {:?}",
        c.faces.len(),
        10_000,
        t.elapsed(),
        series.iter().map(|v| v - series[0]).collect::<Vec<_>>()
    );
    assert_flat(&series, 0, 0);
    let before = thread_stats().live;
    clear_caches();
    assert_eq!(thread_stats().live, before, "clear_caches is a no-op: nothing process-wide is kept");
}

#[test]
fn m2_long_lived_engine_stays_within_the_cache_caps_and_returns_to_the_start() {
    let c = corpus();
    let n = rounds();
    let before = thread_stats().live;
    let mut engine = Engine::new(&c);
    let built = thread_stats().live - before;
    let t = Instant::now();
    let series = soak(n, 50, |_| {
        std::hint::black_box(engine.round(&c));
    });
    let elapsed = t.elapsed();
    // the bound: every outline cache full of the largest glyphs of its face plus every session full
    let outline_bound: i64 = engine.faces.iter().map(|(f, _)| outline_cache_bound(f)).sum();
    let session_bound = engine.sessions.len() as i64 * session_cache_bound();
    let warm = &series[series.len() / 2..];
    let top = warm.iter().max().unwrap() - before - built;
    let spread = warm.iter().max().unwrap() - warm.iter().min().unwrap();
    let third = (series.len() / 3).max(1);
    let mid = series[(third).min(series.len() - 1)..(2 * third).min(series.len())].iter().max().unwrap_or(&0);
    let last = series[(2 * third).min(series.len() - 1)..].iter().max().unwrap();
    let report = format!(
        "# M2 long-lived: engine built = {}; {n} rounds in {elapsed:?}; cached words {}; live above the built engine every 50: {:?}
         # M2 long-lived: top above the built engine {} <= caps (outlines {} + sessions {}); mid third peaks at {}, last third at {}",
        mib(built),
        engine.cached_words(),
        series.iter().map(|v| v - before - built).collect::<Vec<_>>(),
        mib(top),
        mib(outline_bound),
        mib(session_bound),
        mib(*mid - before - built),
        mib(*last - before - built)
    );
    let trend_ok = *last <= *mid + (4 << 20);
    let bounded = top <= outline_bound + session_bound && spread <= outline_bound + session_bound;
    let report_bytes = report.capacity() as i64;
    drop(series);
    drop(engine);
    let after = thread_stats().live - report_bytes;
    eprintln!("{report}");
    assert!(bounded, "long-lived growth exceeds the cache caps: {report}");
    assert!(trend_ok, "growth after warm-up: {report}");
    assert_eq!(after, before, "dropping the engine returns every byte");
}

/// The largest outlines a face could keep: its `outline_cache_entries` biggest glyphs.
fn outline_cache_bound(face: &Face) -> i64 {
    let mut sizes: Vec<usize> =
        (0..u32::from(face.num_glyphs())).map(|g| face.glyph_outline(g).map_or(0, |o| o.len() * size_of::<OutlineCmd>())).collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    let cap = FontLimits::DEFAULT.outline_cache_entries as usize;
    // the cache of the probe itself (a full face cache is kept by this face) is not part of the bound
    sizes.iter().take(cap).sum::<usize>() as i64 + (cap.min(sizes.len()) * 64) as i64
}

/// A full session cache of the longest words of the soak (compounds of two pool words).
fn session_cache_bound() -> i64 {
    let h = Arc::new(parse_and_build(&std::fs::read_to_string(manifest().join("data/hyphen/de-1996.txt")).unwrap()));
    let (s, usage) = measure(|| {
        let mut s = HyphenSession::new(h.clone());
        for i in 0..50_001u32 {
            s.points(&format!("Donaudampfschifffahrtsgesellschaft{i}"));
        }
        s
    });
    assert_eq!(s.cached_words(), 50_001);
    usage.live_delta
}

// ---------------------------------------------------------------------------------------
// M3
// ---------------------------------------------------------------------------------------

#[test]
fn m3_budget_per_face_and_per_operation_of_the_open_fonts() {
    let c = corpus();
    let limits = FontLimits::NATIVE;
    eprintln!("# M3 per face (native limits; kept = live bytes above the shared file bytes; Face is lazy, GPOS/GSUB parse on first use)");
    eprintln!(
        "# face | file KB | glyphs | parse alloc/kept | +first kern kept (peak) | pairs-ish | +ligate kept | +200 outlines kept | parse+kern ms"
    );
    let mut carlito_kept = 0;
    for (name, bytes) in c.names.iter().zip(&c.faces).take(c.open_count) {
        let (face, parse) = measure(|| Face::parse(bytes.clone(), &limits).unwrap());
        let (k, kern) = measure(|| face.kern_units(40, 41));
        let (_, lig) = measure(|| face.ligate(&[40, 41, 42], &[uo_fonts::Tag(*b"liga")]));
        let (_, outlines) = measure(|| (0..200u32).map(|g| face.glyph_outline(g + 3).map_or(0, |o| o.len())).sum::<usize>());
        std::hint::black_box(k);
        let t = Instant::now();
        for _ in 0..20 {
            let f = Face::parse(bytes.clone(), &limits).unwrap();
            std::hint::black_box(f.kern_units(40, 41));
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        eprintln!(
            "# {name} | {} | {} | {}/{} | {} ({}) | | {} | {} | {ms:.2}",
            bytes.len() / 1024,
            face.num_glyphs(),
            kib(parse.allocated as i64),
            kib(parse.live_delta),
            kib(kern.live_delta),
            mib(kern.peak_above_start),
            kib(lig.live_delta),
            kib(outlines.live_delta),
        );
        let kept = parse.live_delta + kern.live_delta + lig.live_delta;
        if name == "carlito-bold.ttf" {
            carlito_kept = kept;
        }
        if name.starts_with("liberationmono") {
            assert!(kept <= 64 << 10, "{name}: a font without kerning keeps {kept} B beyond the shared bytes (target 64 KB)");
        }
        assert!(kept <= 2_621_440 + (2 << 20) * i64::from(name.starts_with("carlito")), "{name} keeps {kept}");
    }
    eprintln!("# Carlito Bold kept (parse + first kern + ligate): {} (TS SfntFont retains ~7.1 MB; target <= 2.5 MB)", mib(carlito_kept));
    assert!(carlito_kept <= 2_621_440, "Carlito Bold keeps {carlito_kept} B beyond the shared bytes, target 2.5 MB");
    assert!(thread_stats().live >= 0);
}

#[test]
fn m3_per_operation_allocated_and_peak_on_carlito_and_the_hyphenators() {
    let c = corpus();
    let limits = FontLimits::NATIVE;
    let (name, bytes) = (&c.names[4], &c.faces[4]);
    assert_eq!(name, "carlito-bold.ttf");
    let face = Face::parse(bytes.clone(), &limits).unwrap();
    let gids = text_gids(&face);
    let cps: Vec<u32> = ORACLE_TEXT.chars().map(|c| c as u32).collect();
    let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
    let row = |op: &str, u: uo_alloc::Usage| {
        eprintln!(
            "# {op:<34} allocated {:>12} peak above start {:>12} kept {:>10}",
            kib(u.allocated as i64),
            kib(u.peak_above_start),
            kib(u.live_delta)
        )
    };
    eprintln!("# M3 per operation on {name} ({} KB, native limits)", bytes.len() / 1024);
    row("Face::parse", measure(|| Face::parse(bytes.clone(), &limits).unwrap()).1);
    let f2 = Face::parse(bytes.clone(), &limits).unwrap();
    row("first kern_units (GPOS parse)", measure(|| f2.kern_units(40, 41)).1);
    row("next kern_units", measure(|| f2.kern_units(41, 42)).1);
    let first = measure(|| Face::parse(bytes.clone(), &limits).unwrap().kern_units(40, 41)).1;
    assert!(first.peak_above_start <= BASELINE_CARLITO_KERN_PEAK * 11 / 10, "first kern peak {} B", first.peak_above_start);
    assert!(first.allocated <= BASELINE_CARLITO_KERN_ALLOCATED * 11 / 10, "first kern allocated {} B", first.allocated);
    row("first ligate (GSUB parse)", measure(|| f2.ligate(&[40, 41], &[uo_fonts::Tag(*b"liga")])).1);
    row("oracle text measure with kerning", measure(|| query_face(&f2, 0)).1);
    row("glyph_ids_for (oracle text)", measure(|| glyph_ids_for(bytes, cps.iter().copied(), &limits)).1);
    row("face_info", measure(|| face_info(bytes)).1);
    row("FontProgram::parse", measure(|| FontProgram::parse(bytes.clone(), &limits)).1);
    let (sub, u) = measure(|| subset_true_type(bytes, gids.iter().copied(), true, &limits).into_owned());
    row("subset (oracle text, keep layout)", u);
    eprintln!(
        "#   subset output {} B; peak {} vs 2 x output + loca {}",
        sub.len(),
        u.peak_above_start,
        2 * sub.len() + 2 * usize::from(face.num_glyphs())
    );
    let (sub, u) = measure(|| subset_true_type(bytes, gids.iter().copied(), false, &limits).into_owned());
    row("subset (oracle text, drop layout)", u);
    eprintln!("#   subset output {} B; peak {}", sub.len(), u.peak_above_start);
    assert!(
        u.peak_above_start <= 2 * sub.len() as i64 + 4 * i64::from(face.num_glyphs()) + (1 << 20),
        "subset peak {}",
        u.peak_above_start
    );
    row("sfnt_to_eot (whole file)", measure(|| sfnt_to_eot(bytes, false)).1);
    row("odttf obfuscate + deobfuscate", measure(|| odttf::deobfuscate(&odttf::obfuscate(bytes, key), key).len()).1);
    row("standalone_face (plain sfnt)", measure(|| standalone_face(bytes, None, &limits).map(|f| f.len())).1);
    row("standalone_face (2-face ttc)", measure(|| standalone_face(&c.ttc, None, &limits).map(|f| f.len())).1);
    row("embedding (requests + plan)", measure(|| embedding(&c, &limits)).1);
    eprintln!("# M3 hyphenators (Hyphenator::new from the pattern file; TS Hyphenator measured by ts_budgets.mjs)");
    let texts = pattern_texts();
    let mut total_kept = 0;
    for (id, text) in PATTERN_IDS.iter().zip(&texts) {
        let patterns = uo_fonts::hyphen::parse_patterns(text).unwrap();
        let (h, u) = measure(|| Hyphenator::new(&patterns));
        total_kept += u.live_delta;
        eprintln!(
            "#   {id:<8} file {:>7} B | allocated {:>10} | peak {:>10} | kept {:>10}",
            text.len(),
            kib(u.allocated as i64),
            kib(u.peak_above_start),
            kib(u.live_delta)
        );
        if *id == "de-1996" {
            assert!(u.live_delta <= 4 << 20, "de-1996 keeps {}", u.live_delta);
            assert!(u.live_delta <= BASELINE_DE1996_HYPHENATOR_KEPT * 11 / 10, "de-1996 keeps {}", u.live_delta);
        }
        drop(h);
    }
    eprintln!("#   seven hyphenators kept {}", mib(total_kept));
    let h = Arc::new(parse_and_build(&texts[4]));
    let (s, u) = measure(|| {
        let mut s = HyphenSession::new(h.clone());
        for w in pool() {
            s.hyphenate(&w, 2, 3);
        }
        s
    });
    eprintln!("#   session after {} pool words: kept {} ({} cached)", pool().len(), kib(u.live_delta), s.cached_words());
}

fn local_dir() -> PathBuf {
    PathBuf::from(std::env::var("UO_LOCAL_FONTS").unwrap_or_else(|_| "C:/Windows/Fonts".into()))
}

/// The opt-in corpus (local fonts, never committed): Gabriola, the Calibri family, CJK collections.
#[test]
#[ignore = "opt-in: reads C:/Windows/Fonts (or UO_LOCAL_FONTS); numbers go to the module record, the fonts are never committed"]
fn m3_opt_in_local_fonts_gabriola_calibri_and_cjk() {
    let dir = local_dir();
    let limits = FontLimits::NATIVE;
    let wasm = FontLimits::WASM32;
    eprintln!("# face | file MB | glyphs | pairs | parse kept | +first kern kept | peak | kern ms | wasm32-limits first kern kept | peak");
    for file in [
        "Gabriola.ttf",
        "calibri.ttf",
        "calibrib.ttf",
        "calibrii.ttf",
        "calibriz.ttf",
        "calibril.ttf",
        "calibrili.ttf",
        "msgothic.ttc",
        "YuGothR.ttc",
        "SimsunExtG.ttf",
        "msyh.ttc",
        "meiryo.ttc",
        "seguiemj.ttf",
    ] {
        let Ok(raw) = std::fs::read(dir.join(file)) else {
            eprintln!("# {file}: not found");
            continue;
        };
        let sfnt = standalone_face(&raw, None, &limits).unwrap().into_owned();
        let bytes: Arc<[u8]> = Arc::from(sfnt);
        let dir_of = uo_fonts::TableDir::parse(uo_fonts::View::new(&bytes)).unwrap();
        let pairs = uo_fonts::gpos::Kerning::of_font(&bytes, &dir_of, &limits).pairs.len();
        let (face, parse) = measure(|| Face::parse(bytes.clone(), &limits).unwrap());
        let t = Instant::now();
        let (_, kern) = measure(|| face.kern_units(40, 41));
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let (wf, wk) = measure(|| {
            let f = Face::parse(bytes.clone(), &wasm).unwrap();
            f.kern_units(40, 41);
            f
        });
        eprintln!(
            "# {file} | {:.2} | {} | {pairs} | {} | {} | {} | {ms:.1} | {} | {}",
            bytes.len() as f64 / 1_048_576.0,
            face.num_glyphs(),
            kib(parse.live_delta),
            mib(kern.live_delta),
            mib(kern.peak_above_start),
            mib(wk.live_delta),
            mib(wk.peak_above_start),
        );
        drop(wf);
        let (n, usage) = measure(|| glyph_ids_for(&bytes, 0..0x3_0000u32, &limits).map(|v| v.len()));
        eprintln!(
            "#   glyph_ids_for 0..0x30000 ({n:?} glyphs): allocated {} peak {}",
            mib(usage.allocated as i64),
            mib(usage.peak_above_start)
        );
        let base = thread_stats().live;
        let total: usize = (0..u32::from(face.num_glyphs())).map(|g| face.glyph_outline(g).map_or(0, |o| o.len())).sum();
        eprintln!("#   all outlines: {total} commands; cache after the walk keeps {}", mib(thread_stats().live - base));
    }
}

/// The largest outline of every font of the directory, to size `max_outline_points` for a phone.
#[test]
#[ignore = "opt-in: scans every font of C:/Windows/Fonts (or UO_LOCAL_FONTS)"]
fn m3_opt_in_the_largest_real_outline_and_the_largest_kerning() {
    let limits = FontLimits::NATIVE;
    let mut worst = (0usize, String::new(), 0u32);
    let mut most_pairs = (0usize, String::new());
    let mut faces = 0;
    let mut files: Vec<_> = std::fs::read_dir(local_dir()).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".ttc")) {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else { continue };
        let Ok(sfnt) = standalone_face(&raw, None, &limits) else { continue };
        let bytes: Arc<[u8]> = Arc::from(sfnt.into_owned());
        let Ok(face) = Face::parse(bytes.clone(), &limits) else { continue };
        faces += 1;
        if let Ok(dir) = uo_fonts::TableDir::parse(uo_fonts::View::new(&bytes)) {
            let pairs = uo_fonts::gpos::Kerning::of_font(&bytes, &dir, &limits).pairs.len();
            if pairs > most_pairs.0 {
                most_pairs = (pairs, name.clone());
            }
        }
        for g in 0..u32::from(face.num_glyphs()) {
            let n = face.glyph_outline(g).map_or(0, |o| o.len());
            if n > worst.0 {
                worst = (n, name.clone(), g);
            }
        }
    }
    eprintln!(
        "# {faces} local faces: largest outline {} commands ({} KB) in {} glyph {}; most kerning pairs {} in {}",
        worst.0,
        worst.0 * 40 / 1024,
        worst.1,
        worst.2,
        most_pairs.0,
        most_pairs.1
    );
}

// ---------------------------------------------------------------------------------------
// M5 and the worst case per limit
// ---------------------------------------------------------------------------------------

#[test]
fn m5_the_outline_cache_is_bounded_past_its_entry_cap() {
    let fonts = open_fonts();
    let bytes: Arc<[u8]> = Arc::from(fonts.iter().find(|(n, _)| n == "carlito-regular.ttf").unwrap().1.clone());
    let limits = FontLimits { outline_cache_entries: 100, ..FontLimits::NATIVE };
    let face = Face::parse(bytes, &limits).unwrap();
    let n = u32::from(face.num_glyphs());
    let mut series = Vec::new();
    let base = thread_stats().live;
    for g in 0..n.min(2_000) {
        std::hint::black_box(face.glyph_outline(g));
        if g % 50 == 0 {
            series.push(thread_stats().live - base);
        }
    }
    let top = series.iter().max().unwrap();
    eprintln!("# M5 outline cache (cap 100 entries) over {} distinct glyphs: kept at most {}", n.min(2_000), kib(*top));
    assert!(*top < 100 * 4096, "100 Carlito outlines must stay small, got {top}");
    let full = FontLimits::NATIVE;
    let face = Face::parse(Arc::from(fonts.iter().find(|(n, _)| n == "carlito-regular.ttf").unwrap().1.clone()), &full).unwrap();
    let (_, u) = measure(|| (0..n).map(|g| face.glyph_outline(g).map_or(0, |o| o.len())).sum::<usize>());
    eprintln!("# M5 outline cache (cap 2,048) over all {n} Carlito glyphs: kept {} (the cache holds at most 2,048)", mib(u.live_delta));
    assert!(u.live_delta > 0);
}

#[test]
fn m5_the_compact_cmap_index_at_the_wasm32_cap_is_four_bytes_per_range() {
    use b_support::{Font, cmap, format12};
    let n = FontLimits::WASM32.max_cmap_ranges;
    let groups: Vec<(u32, u32, u32)> = (0..n).map(|i| (i * 2, i * 2, i)).collect();
    let bytes: Arc<[u8]> = Arc::from(Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&groups))])).build());
    drop(groups);
    let (face, usage) = measure(|| Face::parse(bytes.clone(), &FontLimits::WASM32).unwrap());
    eprintln!(
        "# M5 cmap index at the wasm32 cap ({n} ranges, {} MB of file): kept {}, peak {}, allocated {}",
        bytes.len() >> 20,
        mib(usage.live_delta),
        mib(usage.peak_above_start),
        mib(usage.allocated as i64)
    );
    assert_eq!(face.glyph_id(4_000), 2_000);
    assert!(usage.live_delta <= i64::from(n) * 4 + (64 << 10), "kept {}", usage.live_delta);
    drop(face);
    let over: Vec<(u32, u32, u32)> = (0..=n).map(|i| (i * 2, i * 2, i)).collect();
    let over_bytes: Arc<[u8]> = Arc::from(Font::basic().set(b"cmap", cmap(&[(3, 10, format12(&over))])).build());
    drop(over);
    let (r, usage) = measure(|| Face::parse(over_bytes.clone(), &FontLimits::WASM32).err());
    assert_eq!(r, Some(FontError::Limit(LimitKind::CmapRanges)));
    assert!(usage.peak_above_start <= 20 << 20, "the range past the cap peaked at {}", mib(usage.peak_above_start));
}

#[test]
fn m5_the_pair_map_kept_at_its_cap() {
    for (tag, limits) in [("wasm32", FontLimits::WASM32), ("native", FontLimits::NATIVE)] {
        let side = f64::from(limits.max_kern_pairs).sqrt() as u16;
        let bytes: Arc<[u8]> = Arc::from(pair_map_font(side));
        let (face, usage) = measure(|| {
            let f = Face::parse(bytes.clone(), &limits).unwrap();
            f.kern_units(1, 2);
            f
        });
        eprintln!(
            "# M5 pair map at the {tag} cap ({} pairs): kept {} ({:.1} B per pair), peak {}",
            u64::from(side) * u64::from(side),
            mib(usage.live_delta),
            usage.live_delta as f64 / (f64::from(side) * f64::from(side)),
            mib(usage.peak_above_start)
        );
        assert_eq!(face.kern_units(5, 6), 9);
        assert!(usage.live_delta <= i64::from(limits.max_kern_pairs) * 20, "kept {}", usage.live_delta);
    }
}

#[test]
fn m5_the_hyphen_session_cache_holds_at_its_cap_and_drops_with_the_session() {
    let h = Arc::new(parse_and_build(&pattern_texts()[0]));
    let before = thread_stats().live;
    let mut s = HyphenSession::new(h.clone());
    let mut top = 0;
    for i in 0..120_000u32 {
        s.points(&format!("palavra{i}"));
        assert!(s.cached_words() <= 50_001);
        if i % 5_000 == 0 {
            top = top.max(thread_stats().live - before);
        }
    }
    let max_kept = top.max(thread_stats().live - before);
    let report = format!(
        "# M5 session cache (120,000 distinct words, cap 50,001): cached {} words, kept at most {}",
        s.cached_words(),
        mib(max_kept)
    );
    let report_bytes = report.capacity() as i64;
    drop(s);
    let after = thread_stats().live - report_bytes;
    eprintln!("{report}");
    assert_eq!(after, before);
}

fn peak_of(label: &str, limits: &FontLimits, bytes: &[u8], f: impl FnOnce(&Face)) -> (i64, f64) {
    let arc: Arc<[u8]> = Arc::from(bytes.to_vec());
    let t = Instant::now();
    let (_, u) = measure(|| {
        let face = Face::parse(arc.clone(), limits).unwrap();
        f(&face);
    });
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "# worst case: {label}: file {} KB; peak above start {}; kept {} B; {ms:.0} ms",
        bytes.len() / 1024,
        mib(u.peak_above_start),
        u.live_delta
    );
    (u.peak_above_start, ms)
}

#[test]
fn worst_case_per_limit_with_hostile_fonts() {
    let lig = hostile_gsub();
    let class0 = hostile_class0_gpos();
    for (tag, limits) in [("native", FontLimits::NATIVE), ("wasm32 limits", FontLimits::WASM32)] {
        peak_of(&format!("{} ({tag})", HOSTILE[0]), &limits, &lig, |f| {
            std::hint::black_box(f.ligate(&[1, 2], &[uo_fonts::Tag(*b"liga")]));
        });
        peak_of(&format!("{} ({tag})", HOSTILE[1]), &limits, &class0, |f| {
            std::hint::black_box(f.kern_units(1, 2));
        });
        // the pair map filled to exactly its cap (a class matrix of side x side)
        let side = (f64::from(limits.max_kern_pairs).sqrt()) as u16;
        let pairs = pair_map_font(side);
        peak_of(&format!("{} {side}x{side} = {} pairs ({tag})", HOSTILE[2], u64::from(side) * u64::from(side)), &limits, &pairs, |f| {
            std::hint::black_box(f.kern_units(1, 2));
        });
        let over = pair_map_font(side + 1);
        peak_of(&format!("{} one pair past the cap ({tag})", HOSTILE[2]), &limits, &over, |f| {
            std::hint::black_box(f.kern_units(1, 2));
        });
    }
}

#[test]
fn worst_case_of_one_outline_and_of_the_outline_cache() {
    let limits = FontLimits::NATIVE;
    // a composite that asks for 16 x 65,535 points (about a million path commands, 40 MB, before D3):
    // the point cap of 2^16 stops it, and the peak is what was built before the stop
    let font = hostile_outline_font(40, 16, 65_535);
    let face = Face::parse(Arc::from(font.clone()), &limits).unwrap();
    let (none, over) = measure(|| face.glyph_outline(2));
    eprintln!(
        "# worst case: one composite asking for 16 x 65,535 points of a {} KB file: {} at max_outline_points = 2^16, peak above start {}",
        font.len() / 1024,
        if none.is_none() { "None" } else { "an outline" },
        mib(over.peak_above_start)
    );
    assert!(none.is_none(), "the composite is over the point cap");
    assert!(over.live_delta <= 4096, "kept {} B after the refusal", over.live_delta);
    // the largest outline the cap lets through: a 65,535-point glyph, alone and drawn by a composite
    let font = hostile_outline_font(40, 1, 65_535);
    let face = Face::parse(Arc::from(font.clone()), &limits).unwrap();
    let (o, one) = measure(|| face.glyph_outline(2).unwrap());
    eprintln!(
        "# worst case: one composite glyph of a {} KB file: {} commands ({}), peak above start {}, kept by the cache {}",
        font.len() / 1024,
        o.len(),
        mib((o.len() * size_of::<OutlineCmd>()) as i64),
        mib(one.peak_above_start),
        mib(one.live_delta - (o.len() * size_of::<OutlineCmd>()) as i64)
    );
    drop(o);
    let base = thread_stats().live;
    for g in 2..42u32 {
        std::hint::black_box(face.glyph_outline(g));
    }
    let kept = thread_stats().live - base;
    eprintln!(
        "# worst case: 40 distinct hostile glyphs of 2.5 MB each: the cache keeps {} (before the byte cap: 1560 MB at 40 MB each, 80 GB at the 2,048-entry cap)",
        mib(kept)
    );
    assert_eq!(kept, 0, "an outline over 256 KiB of commands is not cached");

    // glyphs just under the per-entry cap (6,000 commands = 240 KB): the byte cap clears the cache
    let mid = hostile_outline_font(200, 1, 6_000);
    let face = Face::parse(Arc::from(mid), &limits).unwrap();
    let base = thread_stats().live;
    let mut top = 0;
    for g in 2..202u32 {
        std::hint::black_box(face.glyph_outline(g));
        top = top.max(thread_stats().live - base);
    }
    eprintln!("# worst case: 200 distinct glyphs of 240 KB each: the cache keeps at most {} (cap 4 MiB)", mib(top));
    assert!(top <= (4 << 20) + (256 << 10), "the cache kept {top} B");
}

#[test]
fn worst_case_of_the_output_caps() {
    let subset = hostile_subset_font(2_000);
    let ttc = hostile_ttc(400);
    for (tag, limits) in [("native", FontLimits::NATIVE), ("wasm32 limits", FontLimits::WASM32)] {
        let all: Vec<u32> = (0..2_000).collect();
        let (out, u) = measure(|| subset_true_type(&subset, all.iter().copied(), false, &limits).len());
        eprintln!(
            "# worst case: subset of 1,000 overlapping 60 KB glyphs ({tag}): output {} B, peak above start {}",
            out,
            mib(u.peak_above_start)
        );
        let (r, u) = measure(|| standalone_face(&ttc, None, &limits).map(|f| f.len()));
        eprintln!("# worst case: ttc with 400 tables of the whole file ({tag}): {r:?}, peak above start {}", mib(u.peak_above_start));
        assert!(u.peak_above_start <= 2 * limits.max_output_bytes as i64);
    }
}
