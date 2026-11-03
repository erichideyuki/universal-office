//! The system-fed policies over an in-memory font source (the native index has its own tests):
//! the identity rules of `server-substitute` (C# reference) and `oracle` (node-fonts), the cache of
//! answers that does not pin memory, the document budget, threads.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use provider_support::named_font;
use uo_fonts::FontLimits;
use uo_fonts::policy::{OracleProvider, SubstituteFontProvider, SystemFontProvider};
use uo_fonts::registry::{FaceCache, FaceLedger, FaceSource, FontBudget, FontProvider, MAX_ANSWER_SLOTS, SourceFace};

/// Families by lower-case name; a request for a style the family lacks gets its one face.
struct Fake {
    cache: Arc<FaceCache>,
    fonts: HashMap<String, Arc<[u8]>>,
    picks: AtomicUsize,
}

impl Fake {
    fn new(families: &[(&str, u16)]) -> Arc<Fake> {
        let cache = Arc::new(FaceCache::new(FontBudget { pinned_bytes: u64::MAX, cache_bytes: u64::MAX }, FontLimits::NATIVE));
        let fonts = families
            .iter()
            .map(|(f, n)| ((*f).to_owned(), Arc::from(named_font(&[(0x409, 1, f), (0x409, 2, "Regular")], 400, 0x40, *n).build())))
            .collect();
        Arc::new(Fake { cache, fonts, picks: AtomicUsize::new(0) })
    }
}

impl FaceSource for Fake {
    fn pick(&self, family: &str, bold: bool, italic: bool) -> Option<SourceFace> {
        self.picks.fetch_add(1, Ordering::SeqCst);
        let bytes = self.fonts.get(&family.to_lowercase())?;
        Some(SourceFace {
            key: format!("{family}||{family}.ttf#0"),
            family: family.to_owned(),
            face: self.cache.get_or_parse(Arc::clone(bytes)).ok()?,
            synth_bold: bold,
            synth_italic: italic,
        })
    }
}

fn source(f: &Arc<Fake>) -> Arc<dyn FaceSource> {
    Arc::clone(f) as Arc<dyn FaceSource>
}

#[test]
fn the_system_provider_keys_by_the_exact_request_and_does_not_pin_what_nobody_holds() {
    let fake = Fake::new(&[("arial", 1)]);
    let p = SystemFontProvider::new(source(&fake), None);
    let a = p.face("Arial", false, false).unwrap();
    assert!(Arc::ptr_eq(&a, &p.face("Arial", false, false).unwrap()), "same request, same answer while held");
    let upper = p.face("ARIAL", false, false).unwrap();
    assert_ne!(a.id(), upper.id(), "the C# cache is case-sensitive: two FontFace objects");
    assert!(a.cached_face().same_entry(upper.cached_face()), "over one file");
    let bold = p.face("Arial", true, false).unwrap();
    assert_ne!(a.id(), bold.id());
    assert!(bold.synth_bold && !a.synth_bold);
    assert_eq!(fake.picks.load(Ordering::SeqCst), 3);
    // answers are remembered weakly: once nobody holds one, the next ask resolves again
    let id = a.id();
    drop(a);
    let again = p.face("Arial", false, false).unwrap();
    assert_ne!(again.id(), id, "a dropped answer is not kept alive by the provider");
    assert_eq!(fake.picks.load(Ordering::SeqCst), 4);
    // a request that found nothing is remembered
    assert!(p.face("Nope", false, false).is_none());
    assert!(p.face("Nope", false, false).is_none());
    assert_eq!(fake.picks.load(Ordering::SeqCst), 5);
}

#[test]
fn a_dropped_provider_leaves_the_faces_to_the_cache() {
    let fake = Fake::new(&[("arial", 1)]);
    let p = SystemFontProvider::new(source(&fake), None);
    let a = p.face("Arial", false, false).unwrap();
    assert_eq!(fake.cache.stats().pinned_entries, 1);
    drop(a);
    drop(p);
    assert_eq!(fake.cache.stats().pinned_entries, 0);
    assert_eq!(fake.cache.stats().entries, 1, "kept, unpinned, until the budget needs the room");
}

#[test]
fn the_substitute_returns_the_inner_answer_or_a_fresh_marked_copy() {
    let fake = Fake::new(&[("carlito", 1), ("liberation sans", 2), ("calibri", 3)]);
    let inner = Arc::new(SystemFontProvider::new(source(&fake), None));
    let p = SubstituteFontProvider::new(Arc::clone(&inner) as Arc<dyn FontProvider>);
    // the family exists: the inner answer itself, not a stand-in
    let direct = p.face("Calibri", false, false).unwrap();
    assert!(!direct.stand_in);
    assert!(Arc::ptr_eq(&direct, &inner.face("Calibri", false, false).unwrap()));
    // absent: the catalog substitute, a copy marked stand-in, with a new identity at every call
    let s1 = p.face("Arial", true, false).unwrap();
    let s2 = p.face("Arial", true, false).unwrap();
    assert!(s1.stand_in && s2.stand_in);
    assert_ne!(s1.id(), s2.id(), "the C# builds a new FontFace each time");
    assert_eq!((s1.family.as_str(), s1.key.as_str()), ("Liberation Sans", "Liberation Sans||Liberation Sans.ttf#0"));
    assert!(s1.cached_face().same_entry(s2.cached_face()));
    assert!(s1.bold && s1.synth_bold, "the requested style is kept, as the C# copies it");
    let inner_answer = inner.face("Liberation Sans", true, false).unwrap();
    assert!(!inner_answer.stand_in, "the inner provider's own answer is not marked");
    assert_ne!(inner_answer.id(), s1.id());
    // no substitute in the catalog, or the substitute is not installed
    assert!(p.face("Aptos", false, false).is_none());
    assert!(p.face("Cambria", false, false).is_none(), "Caladea is not in the source");
    // spellings the C# catalog normalises
    assert!(p.face("  ARIAL\u{a0}", false, false).unwrap().stand_in);
    assert!(p.face("\u{feff}arial", false, false).is_none(), "U+FEFF is not white space to C#");
}

#[test]
fn the_oracle_keys_by_the_lower_case_request() {
    let fake = Fake::new(&[("calibri", 1)]);
    let p = OracleProvider::new(source(&fake), None, &["Meiryo UI"]);
    let a = p.face("Calibri", false, false).unwrap();
    let b = p.face("CALIBRI", false, false).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "node-fonts caches by family.toLowerCase()");
    assert_eq!(fake.picks.load(Ordering::SeqCst), 1);
    assert!(!Arc::ptr_eq(&a, &p.face("calibri", true, false).unwrap()));
    assert!(!a.stand_in);
    // `missing` families are not installed, whatever the source has
    let fake = Fake::new(&[("meiryo ui", 4)]);
    let p = OracleProvider::new(source(&fake), None, &["Meiryo UI"]);
    assert!(p.face("meiryo ui", false, false).is_none());
    assert_eq!(fake.picks.load(Ordering::SeqCst), 0);
}

#[test]
fn the_document_ledger_refuses_a_face_past_the_budget_and_asks_again_later() {
    let fake = Fake::new(&[("one", 1), ("two", 2)]);
    let size = fake.cache.get_or_parse(Arc::clone(&fake.fonts["one"])).unwrap().cost();
    let ledger = FaceLedger::new(size + size / 2);
    let p = SystemFontProvider::new(source(&fake), Some(Arc::clone(&ledger)));
    let one = p.face("one", false, false).unwrap();
    assert_eq!(ledger.used(), size);
    assert!(one.ticket().is_some());
    assert!(p.face("two", false, false).is_none(), "past the budget it is a miss of the provider");
    assert_eq!(ledger.used(), size);
    // the same content under another request is the same charge
    assert!(p.face("one", true, true).is_some());
    assert_eq!(ledger.used(), size);
    drop(one);
    // nothing held it; the refused face is not remembered as missing: it answers now
    let two = p.face("two", false, false);
    assert!(two.is_some(), "the budget is free again");
}

#[test]
fn threads_asking_for_one_request_get_one_answer() {
    let fake = Fake::new(&[("arial", 1)]);
    let p = Arc::new(SystemFontProvider::new(source(&fake), None));
    let answers: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let p = Arc::clone(&p);
                s.spawn(move || p.face("Arial", false, false).unwrap())
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(answers.iter().all(|a| Arc::ptr_eq(a, &answers[0])), "the first answer inserted wins for everyone");
}

#[test]
fn the_answer_cache_has_a_cap() {
    let fake = Fake::new(&[]);
    let p = SystemFontProvider::new(source(&fake), None);
    for i in 0..MAX_ANSWER_SLOTS * 2 + 5 {
        assert!(p.face(&format!("family {i}"), false, false).is_none());
    }
    assert!(p.remembered() <= MAX_ANSWER_SLOTS, "{} slots", p.remembered());
    let o = OracleProvider::new(source(&fake), None, &[]);
    for i in 0..MAX_ANSWER_SLOTS + 10 {
        assert!(o.face(&format!("family {i}"), false, false).is_none());
    }
    assert!(o.remembered() <= MAX_ANSWER_SLOTS);
}
