//! The face cache and the document ledger (LQ8(b)): dedup by content, least recently used out
//! first, faces held by a handle pinned, the per-document charge, the aliases of a source.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::sync::Arc;

use provider_support::named_font;
use provider_support::repo_root;
use uo_fonts::registry::{ContentKey, FaceCache, FaceLedger, FontBudget, FontProfile, MAX_UNPINNED_FACES};
use uo_fonts::{FontError, FontLimits};

fn cache(cache_bytes: u64) -> FaceCache {
    FaceCache::new(FontBudget { pinned_bytes: u64::MAX, cache_bytes }, FontLimits::NATIVE)
}

fn font(variant: u16) -> Arc<[u8]> {
    Arc::from(named_font(&[(0x409, 1, "Cache Fam"), (0x409, 2, "Regular")], 400, 0x40, variant).build())
}

#[test]
fn what_a_host_shares_between_threads_is_send_and_sync() {
    fn shared<T: Send + Sync>() {}
    shared::<FaceCache>();
    shared::<FaceLedger>();
    shared::<uo_fonts::registry::FontRegistry>();
    shared::<uo_fonts::registry::FontAnswer>();
    shared::<uo_fonts::registry::CachedFace>();
    shared::<uo_fonts::policy::ViewerProvider>();
    shared::<uo_fonts::policy::SystemFontProvider>();
    shared::<uo_fonts::policy::SubstituteFontProvider>();
    shared::<uo_fonts::policy::OracleProvider>();
    shared::<Box<dyn uo_fonts::registry::FontProvider>>();
    #[cfg(feature = "system")]
    shared::<uo_fonts::system::SystemFontIndex>();
}

#[test]
fn the_profiles_are_the_plan_numbers() {
    const MIB: u64 = 1 << 20;
    assert_eq!(FontBudget::PHONE, FontBudget { pinned_bytes: 48 * MIB, cache_bytes: 16 * MIB });
    assert_eq!(FontBudget::WASM_DESKTOP, FontBudget { pinned_bytes: 128 * MIB, cache_bytes: 64 * MIB });
    assert_eq!(FontBudget::NATIVE_DESKTOP, FontBudget { pinned_bytes: 1024 * MIB, cache_bytes: 512 * MIB });
    assert_eq!(FontBudget::of(FontProfile::Phone), FontBudget::PHONE);
    assert_eq!(FontBudget::of(FontProfile::Desktop), FontBudget::DESKTOP);
    #[cfg(target_pointer_width = "64")]
    assert_eq!(FontBudget::DESKTOP, FontBudget::NATIVE_DESKTOP);
}

#[test]
fn equal_content_is_one_entry_and_one_copy_of_the_bytes() {
    let c = cache(u64::MAX);
    let (a, b) = (font(1), font(1));
    assert!(!Arc::ptr_eq(&a, &b));
    let first = c.get_or_parse(Arc::clone(&a)).unwrap();
    let second = c.get_or_parse(Arc::clone(&b)).unwrap();
    assert!(first.same_entry(&second));
    assert!(Arc::ptr_eq(first.bytes(), second.bytes()), "the second buffer is dropped for the cached one");
    assert!(Arc::ptr_eq(first.bytes(), &a), "the first buffer is the one kept");
    assert_eq!(Arc::strong_count(&b), 1, "nothing keeps the duplicate");
    let other = c.get_or_parse(font(2)).unwrap();
    assert!(!first.same_entry(&other));
    assert_eq!(c.stats().entries, 2);
    assert_eq!(first.content_key(), ContentKey::of(&a));
    assert_ne!(first.content_key(), other.content_key());
    assert_eq!(first.content_key().len(), a.len() as u64);
    assert_eq!(first.content_key().short_hex().len(), 32);
}

#[test]
fn a_parse_failure_is_an_error_and_is_not_cached() {
    let c = cache(u64::MAX);
    let garbage: Arc<[u8]> = Arc::from(vec![7u8; 64]);
    assert!(c.get_or_parse(Arc::clone(&garbage)).is_err());
    assert!(matches!(c.get_or_parse(Arc::from(Vec::<u8>::new())), Err(FontError::OutOfBounds | FontError::MissingTable(_))));
    assert_eq!(c.stats().entries, 0);
    assert_eq!(c.stats().misses, 0);
}

#[test]
fn unpinned_faces_go_least_recently_used_first() {
    let one = font(1);
    let cost = {
        let c = cache(u64::MAX);
        c.get_or_parse(Arc::clone(&one)).unwrap().cost()
    };
    // room for two faces and a bit: the third insert evicts the least recently used
    let c = cache(cost * 2 + cost / 2);
    for v in 1..=3u16 {
        c.get_or_parse(font(v)).unwrap();
    }
    assert_eq!(c.stats().entries, 3, "the face just inserted is pinned by its own handle while the cache trims");
    c.trim_now();
    assert_eq!(c.stats().entries, 2);
    assert_eq!(c.stats().evictions, 1);
    let hits = c.stats().hits;
    c.get_or_parse(font(2)).unwrap();
    c.get_or_parse(font(3)).unwrap();
    assert_eq!(c.stats().hits, hits + 2, "2 and 3 stayed, 1 went");
    // 3 was used last: asking for 2 then adding 4 evicts 3's older neighbour (2 is newer than 3? 3 was touched after 2)
    c.get_or_parse(font(2)).unwrap();
    c.get_or_parse(font(4)).unwrap();
    c.trim_now();
    let hits = c.stats().hits;
    c.get_or_parse(font(2)).unwrap();
    assert_eq!(c.stats().hits, hits + 1, "2 was the most recent and stayed");
    let misses = c.stats().misses;
    c.get_or_parse(font(3)).unwrap();
    assert_eq!(c.stats().misses, misses + 1, "3 was the least recent and went");
    assert!(c.stats().cached_bytes <= cost * 2 + cost / 2 + cost, "the budget holds to within the face just added");
}

#[test]
fn a_face_a_handle_holds_is_never_evicted() {
    let cost = cache(u64::MAX).get_or_parse(font(1)).unwrap().cost();
    let c = cache(cost / 2);
    let pinned = c.get_or_parse(font(1)).unwrap();
    for v in 2..=6u16 {
        c.get_or_parse(font(v)).unwrap();
    }
    c.trim_now();
    let st = c.stats();
    assert_eq!(st.pinned_entries, 1);
    assert_eq!(st.pinned_bytes, pinned.cost());
    assert_eq!(st.entries, 1, "everything unpinned went (each is over the half-face budget)");
    let again = c.get_or_parse(font(1)).unwrap();
    assert!(again.same_entry(&pinned), "the pinned face is still the cached one");
    drop((pinned, again));
    c.trim_now();
    assert_eq!(c.stats().entries, 0, "unpinned, it is over the budget like the rest");
}

#[test]
fn the_entry_cap_holds_whatever_the_bytes() {
    let c = cache(u64::MAX);
    for v in 0..(MAX_UNPINNED_FACES as u16 + 20) {
        c.get_or_parse(font(v)).unwrap();
    }
    c.trim_now();
    let st = c.stats();
    assert_eq!(st.entries, MAX_UNPINNED_FACES);
    assert_eq!(st.evictions, 20);
}

#[test]
fn an_alias_answers_without_reading_the_source_again() {
    let c = cache(u64::MAX);
    let bytes = font(5);
    let mut reads = 0;
    let first = c
        .get_or_load("path#0", || {
            reads += 1;
            Some(bytes.to_vec())
        })
        .unwrap();
    let second = c
        .get_or_load("path#0", || {
            reads += 1;
            Some(bytes.to_vec())
        })
        .unwrap();
    assert_eq!(reads, 1);
    assert!(first.same_entry(&second));
    assert!(c.get_or_load("other#0", || None).is_none());
    assert!(c.get_or_load("junk#0", || Some(vec![1; 10])).is_none());
    drop((first, second));
    c.clear();
    assert_eq!(c.stats().entries, 0);
    let mut reads_again = 0;
    c.get_or_load("path#0", || {
        reads_again += 1;
        Some(bytes.to_vec())
    })
    .unwrap();
    assert_eq!(reads_again, 1, "a cleared cache reads the source again");
}

#[test]
fn the_ledger_charges_once_per_content_and_releases_with_the_last_ticket() {
    let ledger = FaceLedger::new(1000);
    let (k1, k2, k3) = (ContentKey::of(b"one"), ContentKey::of(b"two"), ContentKey::of(b"three"));
    let t1 = ledger.charge(k1, 600).unwrap();
    let t1b = ledger.charge(k1, 600).unwrap();
    assert_eq!(ledger.used(), 600, "the same content is charged once, however many answers use it");
    assert!(ledger.charge(k2, 500).is_none(), "600 + 500 passes the budget");
    assert_eq!(ledger.used(), 600, "a refusal charges nothing");
    let t2 = ledger.charge(k2, 400).unwrap();
    assert_eq!(ledger.used(), 1000);
    assert!(ledger.charge(k3, 1).is_none());
    drop(t1);
    assert_eq!(ledger.used(), 1000, "a clone of the ticket still holds the charge");
    drop(t1b);
    assert_eq!(ledger.used(), 400);
    assert!(ledger.charge(k3, 600).is_some());
    drop(t2);
    assert_eq!(ledger.used(), 0, "every ticket dropped: the document pins nothing");
    assert_eq!(ledger.charge(ContentKey::of(b"big"), 1001).map(|t| t.cost()), None, "a face over the whole budget is always refused");
    assert_eq!(ledger.budget(), 1000);
}

#[test]
fn a_real_face_costs_its_bytes_plus_its_heap() {
    let root = repo_root();
    let bytes: Arc<[u8]> = Arc::from(std::fs::read(root.join("shared/fonts/files/carlito-regular.ttf")).unwrap());
    let bytes = &bytes;
    let c = cache(u64::MAX);
    let held = c.get_or_parse(Arc::clone(bytes)).unwrap();
    let cold = held.cost();
    assert!(cold > bytes.len() as u64, "the directory and the cmap index are heap");
    let heap_before = held.face().heap_bytes();
    let g = held.face().glyph_id(u32::from('A'));
    assert_ne!(g, 0);
    held.face().kern_units(g, held.face().glyph_id(u32::from('V')));
    assert!(held.face().heap_bytes() > heap_before, "the kerning data is heap once it is read");
    assert!(held.cost() > cold, "and the budgets see it the next time they ask");
}
