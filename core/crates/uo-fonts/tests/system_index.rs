//! The native system font index (feature `system`): what the scan reads, how it indexes and picks,
//! the files it must not trust, and the process-wide instance under threads. The replay against
//! the C# and Node references is `provider_parity_system.rs`.
#![cfg(feature = "system")]
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::path::PathBuf;
use std::sync::Arc;

use provider_support::{TempDir, named_font, repo_root, write_corpus};
use uo_fonts::FontLimits;
use uo_fonts::registry::{FaceCache, FaceSource, FontBudget};
use uo_fonts::system::{SystemFontEntry, SystemFontIndex, default_roots};

/// The tests that replace the process-wide index take turns.
static SHARED_INDEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cache() -> Arc<FaceCache> {
    Arc::new(FaceCache::new(FontBudget::DESKTOP, FontLimits::NATIVE))
}

fn corpus() -> (TempDir, SystemFontIndex) {
    let dir = TempDir::new("index");
    write_corpus(&dir.0);
    let index = SystemFontIndex::with_roots(vec![dir.0.clone()], cache());
    (dir, index)
}

fn entry<'a>(index: &'a SystemFontIndex, file: &str, face: u32) -> Option<&'a SystemFontEntry> {
    index.entries().iter().find(|e| e.path.file_name().is_some_and(|n| n == file) && e.index == face)
}

fn picked(index: &SystemFontIndex, family: &str, bold: bool, italic: bool) -> Option<(String, bool, bool)> {
    let p = index.pick(family, bold, italic)?;
    let tail = p.key.rsplit_once('|').unwrap().1;
    let file = tail.rsplit(['/', '\\']).next().unwrap().to_owned();
    Some((file, p.synth_bold, p.synth_italic))
}

#[test]
fn the_scan_reads_names_flags_and_weights_as_the_references_do() {
    let (_dir, index) = corpus();
    let e = entry(&index, "testsans-bold.ttf", 0).unwrap();
    assert_eq!((e.family.as_str(), e.bold, e.italic, e.weight, e.typo.as_deref()), ("Test Sans", true, false, 700, None));
    let e = entry(&index, "catalan-first.ttf", 0).unwrap();
    assert_eq!(e.family, "English Name", "the English (0x409) record wins over the first one");
    assert_eq!(e.typo.as_deref(), Some("Cat Typo"), "a typographic name with no English record is the first found");
    assert_eq!((e.bold, e.italic), (true, true), "\"Bold Italic\" in the style name");
    let e = entry(&index, "oblique.ttf", 0).unwrap();
    assert_eq!((e.bold, e.italic, e.weight), (true, true, 400), "bold from the style name, oblique is italic, weight 0 is 400");
    let e = entry(&index, "bold-in-word.ttf", 0).unwrap();
    assert_eq!((e.bold, e.weight), (false, 600), "SemiBold is not the word bold");
    assert_eq!(entry(&index, "padded.ttf", 0).unwrap().family, "Padded Name", "trimmed, NBSP included");
    assert_eq!(entry(&index, "unicode.ttf", 0).unwrap().family, "\u{dc}n\u{ef}code F\u{f6}nt");
    let e = entry(&index, "no-subfamily.ttf", 0).unwrap();
    assert_eq!((e.bold, e.italic), (false, false), "no style name is Regular");
    let e = entry(&index, "short-os2.ttf", 0).unwrap();
    assert_eq!((e.bold, e.weight), (true, 400), "an OS/2 under 64 bytes leaves the weight and the flags to the names");
    assert!(entry(&index, "deep-font.OTF", 0).is_some(), "subfolders, any case of the extension");
}

#[test]
fn files_that_are_not_trusted_are_left_out() {
    let (_dir, index) = corpus();
    for file in [
        "vertical.ttf",
        "no-family.ttf",
        "blank-family.ttf",
        "mac-only.ttf",
        "odd-length.ttf",
        "no-name-table.ttf",
        "readme.txt",
        "sans.ttf.bak",
        "truncated.ttf",
    ] {
        assert!(index.entries().iter().all(|e| e.path.file_name().unwrap() != file), "{file} is not indexed");
    }
    assert!(index.pick("@Vertical Gothic", false, false).is_none());
    assert!(index.pick("Backup Only", false, false).is_none());
    // a face that is indexed but does not open is a miss, not a panic
    assert!(entry(&index, "broken-face.ttf", 0).is_some());
    assert!(index.pick("Broken Face", false, false).is_none());
    assert!(index.pick("", false, false).is_none());
}

#[test]
fn a_collection_lists_every_face_and_one_bad_face_does_not_hide_the_others() {
    let (_dir, index) = corpus();
    let coll: Vec<(u32, &str, u16)> = index
        .entries()
        .iter()
        .filter(|e| e.path.file_name().unwrap() == "coll.ttc")
        .map(|e| (e.index, e.family.as_str(), e.weight))
        .collect();
    assert_eq!(coll, [(0, "Coll A", 400), (1, "Coll A", 700), (3, "Coll B", 400), (4, "Coll A", 400)], "face 2 has no name table");
    // the duplicate (same family, style and weight) is listed but the first face is the one picked
    let (file, ..) = picked(&index, "coll a", false, false).unwrap();
    assert_eq!(file, "coll.ttc#0");
    assert_eq!(picked(&index, "Coll A", true, false).unwrap().0, "coll.ttc#1");
    assert_eq!(picked(&index, "Coll B", false, false).unwrap().0, "coll.ttc#3");
    let face = index.pick("Coll B", false, false).unwrap();
    assert_eq!(face.face.face().num_glyphs(), 5, "the face of the collection is an sfnt of its own");
    assert_ne!(face.face.bytes().len(), 0);
}

#[test]
fn the_pick_is_the_lowest_score_and_the_first_on_a_tie() {
    let (_dir, index) = corpus();
    // style mismatches cost 4 (bold) and 2 (italic); the weight distance costs |w - 400 or 700| / 1000
    assert_eq!(picked(&index, "Test Sans", false, false).unwrap(), ("testsans-regular.ttf#0".into(), false, false));
    assert_eq!(picked(&index, "Test Sans", true, false).unwrap(), ("testsans-bold.ttf#0".into(), false, false));
    assert_eq!(picked(&index, "Test Sans", false, true).unwrap(), ("testsans-italic.ttf#0".into(), false, false));
    assert_eq!(picked(&index, "Test Sans", true, true).unwrap(), ("testsans-bold.ttf#0".into(), false, true), "bold beats italic: 2 < 4");
    assert_eq!(picked(&index, "Semi Co", false, false).unwrap().0, "semi-regular.ttf#0", "600 regular: 0.2 against 4.3");
    assert_eq!(picked(&index, "Wt Fam", false, false).unwrap().0, "wt-regular.ttf#0");
    assert_eq!(picked(&index, "Wt Fam", true, false).unwrap().0, "wt-medium.ttf#0", "both are not bold; 500 is nearer 700");
    // Tie Fam: 300 and 500 are both 0.1 from 400; the first face of the collection wins
    assert_eq!(picked(&index, "Tie Fam", false, false).unwrap().0, "tie.ttc#0");
    // names are case-insensitive, the lookup is on the lower-cased family
    assert_eq!(picked(&index, "TEST SANS", false, false).unwrap().0, "testsans-regular.ttf#0");
    assert_eq!(picked(&index, "\u{dc}N\u{cf}CODE F\u{d6}NT", false, false).unwrap().0, "unicode.ttf#0");
}

#[test]
fn the_typographic_name_is_a_second_index_that_never_shadows_the_first() {
    let (_dir, index) = corpus();
    assert_eq!(picked(&index, "Typo", false, false).unwrap().0, "typo-display.ttf#0", "found by the typographic name");
    assert_eq!(
        picked(&index, "Typo Display", false, false).unwrap().0,
        "typo-display.ttf#0",
        "the RIBBI name first: Typo Shadowed's ID 16 is the same string"
    );
    assert_eq!(picked(&index, "Typo Shadowed", false, false).unwrap().0, "typo-shadowed.ttf#0");
    // Aptos Like Display has ID 16 = "Aptos Like" and must not shadow the face named Aptos Like
    assert_eq!(picked(&index, "Aptos Like", false, false).unwrap().0, "aptos-like.ttf#0");
    assert_eq!(picked(&index, "Aptos Like Display", false, false).unwrap().0, "aptos-like-display.ttf#0");
    // the key and the family of the answer are the picked entry's: in the typographic index the entry's
    // family IS the typographic name (the JS pushes `family: fam` into both maps)
    let p = index.pick("Typo", false, false).unwrap();
    assert_eq!(p.family, "Typo");
    assert!(p.key.starts_with("Typo||"), "{}", p.key);
    let p = index.pick("Test Sans", true, false).unwrap();
    assert!(p.key.starts_with("Test Sans|b|"), "{}", p.key);
    assert!(p.key.ends_with("testsans-bold.ttf#0"), "{}", p.key);
}

#[test]
fn the_families_come_in_index_order_without_the_typographic_names() {
    let (_dir, index) = corpus();
    let mut families = index.families();
    assert!(families.contains(&"Typo Display".to_owned()) && !families.contains(&"Typo".to_owned()));
    families.sort();
    assert_eq!(families.len(), 19, "{families:?}");
    assert_eq!(index.roots().len(), 1);
}

#[test]
fn the_real_open_fonts_load() {
    let index = SystemFontIndex::with_roots(vec![repo_root().join("shared/fonts/files")], cache());
    assert_eq!(index.entries().len(), 16);
    let p = index.pick("Carlito", true, true).unwrap();
    assert_eq!((p.synth_bold, p.synth_italic), (false, false));
    assert!(p.face.face().glyph_id(u32::from('A')) != 0);
    assert!(p.key.starts_with("Carlito|bi|"), "{}", p.key);
    let p = index.pick("liberation serif", false, true).unwrap();
    assert_eq!(p.family, "Liberation Serif");
    // the same file twice is one cached face
    let again = index.pick("liberation serif", false, true).unwrap();
    assert!(again.face.same_entry(&p.face));
    assert!(index.cache().stats().hits > 0);
}

#[test]
fn subfolders_are_walked_last_in_first_out_after_the_files_of_their_parent() {
    let dir = TempDir::new("order");
    let font = |name: &str| named_font(&[(0x409, 1, name), (0x409, 2, "Regular")], 400, 0x40, 1).build();
    for sub in ["d1", "d2", "d3"] {
        std::fs::create_dir_all(dir.0.join(sub).join("inner")).unwrap();
        std::fs::write(dir.0.join(sub).join(format!("{sub}.ttf")), font(sub)).unwrap();
        std::fs::write(dir.0.join(sub).join("inner").join(format!("{sub}-inner.ttf")), font(&format!("{sub}-inner"))).unwrap();
    }
    std::fs::write(dir.0.join("root.ttf"), font("root")).unwrap();
    let index = SystemFontIndex::with_roots(vec![dir.0.clone()], cache());
    let order: Vec<String> = index.entries().iter().map(|e| e.family.clone()).collect();
    assert_eq!(order[0], "root", "files met in a folder are handled at once, subfolders wait");
    // the subfolders in the reverse of the listing order, each finished (its own subfolder too) before the next
    let mut listed: Vec<String> = std::fs::read_dir(&dir.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    listed.reverse();
    let expect: Vec<String> =
        std::iter::once("root".to_owned()).chain(listed.iter().flat_map(|d| [d.clone(), format!("{d}-inner")])).collect();
    assert_eq!(order, expect);
}

#[test]
fn the_first_file_found_wins_a_duplicate_and_every_face_is_listed() {
    let dir = TempDir::new("dups");
    for (name, variant) in [("a.ttf", 1), ("b.ttf", 2)] {
        std::fs::write(dir.0.join(name), named_font(&[(0x409, 1, "Dup Fam"), (0x409, 2, "Regular")], 400, 0x40, variant).build()).unwrap();
    }
    let index = SystemFontIndex::with_roots(vec![dir.0.clone()], cache());
    assert_eq!(index.entries().len(), 2);
    let first = index.entries()[0].path.clone();
    let p = index.pick("Dup Fam", false, false).unwrap();
    assert!(p.key.ends_with(&format!("{}#0", first.file_name().unwrap().to_string_lossy())), "{}", p.key);
}

#[test]
fn a_root_that_is_missing_or_unreadable_is_skipped() {
    let dir = TempDir::new("roots");
    let missing: PathBuf = dir.0.join("nowhere");
    let index = SystemFontIndex::with_roots(vec![missing.clone(), dir.0.clone()], cache());
    assert!(index.entries().is_empty());
    let kept = default_roots(&[dir.0.clone(), missing, PathBuf::new()]);
    assert!(kept.contains(&dir.0));
    assert!(kept.iter().all(|p| p.exists()));
}

#[cfg(unix)]
#[test]
fn a_symlinked_folder_is_not_followed() {
    let dir = TempDir::new("links");
    let real = dir.0.join("real");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("f.ttf"), named_font(&[(0x409, 1, "Linked"), (0x409, 2, "Regular")], 400, 0x40, 1).build()).unwrap();
    std::os::unix::fs::symlink(&real, dir.0.join("alias")).unwrap();
    let index = SystemFontIndex::with_roots(vec![dir.0.clone()], cache());
    assert_eq!(index.entries().len(), 1, "once, through the real folder");
}

#[test]
fn a_font_over_the_output_limit_is_not_loaded() {
    let dir = TempDir::new("limit");
    write_corpus(&dir.0);
    let small = Arc::new(FaceCache::new(FontBudget::DESKTOP, FontLimits { max_output_bytes: 100, ..FontLimits::NATIVE }));
    let index = SystemFontIndex::with_roots(vec![dir.0.clone()], small);
    assert!(index.entries().len() > 20, "they are indexed");
    assert!(index.pick("Test Sans", false, false).is_none(), "files over the limit are not read into memory");
}

#[test]
fn the_shared_substitute_provider_answers_from_the_shared_index() {
    use uo_fonts::policy::SubstituteFontProvider;
    use uo_fonts::registry::FontProvider;
    let _turn = SHARED_INDEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    SystemFontIndex::release_shared();
    let dir = TempDir::new("shared-sub");
    write_corpus(&dir.0);
    SystemFontIndex::rebuild_shared(std::slice::from_ref(&dir.0));
    let (a, b) = (SubstituteFontProvider::shared(), SubstituteFontProvider::shared());
    assert!(Arc::ptr_eq(&a, &b));
    // the provider is built once, over whichever shared index existed at its first call: here ours
    // unless another test of this binary got there first, so only the contract is checked
    let calibri = a.face("Calibri", false, false);
    assert!(calibri.as_ref().is_none_or(|c| c.stand_in || c.family == "Calibri"));
    SystemFontIndex::release_shared();
}

#[test]
fn the_shared_index_is_built_once_under_threads() {
    let _turn = SHARED_INDEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    SystemFontIndex::release_shared();
    let built: Vec<Arc<SystemFontIndex>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..12).map(|_| s.spawn(SystemFontIndex::shared)).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(built.iter().all(|i| Arc::ptr_eq(i, &built[0])), "one scan for every caller");
    let dir = TempDir::new("shared");
    write_corpus(&dir.0);
    let replaced = SystemFontIndex::rebuild_shared(std::slice::from_ref(&dir.0));
    assert!(!Arc::ptr_eq(&replaced, &built[0]));
    assert!(Arc::ptr_eq(&SystemFontIndex::shared(), &replaced));
    assert!(replaced.pick("Test Sans", false, false).is_some());
    // parallel picks from many threads over the shared index and the shared cache agree
    let families = ["Test Sans", "Semi Co", "Typo", "Coll A", "Tie Fam", "Carlito", "Nope"];
    let results: Vec<Vec<Option<String>>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..16)
            .map(|t| {
                let index = Arc::clone(&replaced);
                s.spawn(move || {
                    (0..families.len() * 4)
                        .map(|i| {
                            let f = families[(i + t) % families.len()];
                            index.pick(f, i % 2 == 0, i % 3 == 0).map(|p| p.key)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (t, r) in results.iter().enumerate() {
        for (i, key) in r.iter().enumerate() {
            let f = families[(i + t) % families.len()];
            let expected = replaced.pick(f, i % 2 == 0, i % 3 == 0).map(|p| p.key);
            assert_eq!(key, &expected, "{f}");
        }
    }
    SystemFontIndex::release_shared();
}

/// Opt-in (`UO_LOCAL_FONTS=1`): the index over this machine's own font folders, the numbers of the
/// module record. The fonts are read here and never written anywhere.
#[test]
fn local_machine_index_numbers() {
    if !uo_testkit::opt_in("UO_LOCAL_FONTS", "the system index over the machine's fonts") {
        return;
    }
    let started = std::time::Instant::now();
    let index = SystemFontIndex::with_roots(default_roots(&[]), cache());
    let scan = started.elapsed();
    let families = index.families();
    eprintln!(
        "local index: {} roots, {} faces, {} families, scanned in {:.1} ms",
        index.roots().len(),
        index.entries().len(),
        families.len(),
        scan.as_secs_f64() * 1000.0
    );
    let started = std::time::Instant::now();
    let mut loaded = 0usize;
    let mut bytes = 0usize;
    for f in &families {
        if let Some(p) = index.pick(f, false, false) {
            loaded += 1;
            bytes += p.face.bytes().len();
        }
    }
    let first = started.elapsed();
    let started = std::time::Instant::now();
    for f in &families {
        let _ = index.pick(f, false, false);
    }
    let again = started.elapsed();
    let stats = index.cache().stats();
    eprintln!(
        "local index: {loaded} regular faces loaded ({} MiB of files, {} MiB cached as bytes + heap) in {:.1} ms; the same picks again from the cache in {:.1} ms ({} hits, {} evictions)",
        bytes >> 20,
        (stats.pinned_bytes + stats.cached_bytes) >> 20,
        first.as_secs_f64() * 1000.0,
        again.as_secs_f64() * 1000.0,
        stats.hits,
        stats.evictions
    );
    let index = Arc::new(index);
    let started = std::time::Instant::now();
    std::thread::scope(|s| {
        for t in 0..8 {
            let index = Arc::clone(&index);
            let families = &families;
            s.spawn(move || {
                for f in families.iter().skip(t).step_by(3) {
                    let _ = index.pick(f, t % 2 == 0, false);
                }
            });
        }
    });
    eprintln!("local index: 8 threads picking in parallel in {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
}
