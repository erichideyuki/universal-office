//! The registry and the `viewer` policy, case by case (the corpus-wide replay against the TS is
//! `provider_parity.rs`): the order of the variants, the assets before the resolver, the licence
//! substitution, the missing notes, the budget of what is pushed.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;
mod provider_support;

use std::sync::Arc;

use provider_support::{named_font, pool_files};
use uo_fonts::policy::{PolicyError, PolicyName, ViewerProvider, provider};
use uo_fonts::registry::{AdvanceTable, FaceCache, FontBudget, FontProvider, FontRegistry, FontSource, HostFace, PushError};
use uo_fonts::{FontLimits, Variant};
use uo_testkit::generate::sfnt::build_ttc;

fn registry(pinned: u64) -> Arc<FontRegistry> {
    let cache = Arc::new(FaceCache::new(FontBudget { pinned_bytes: pinned, cache_bytes: u64::MAX }, FontLimits::NATIVE));
    Arc::new(FontRegistry::new(FontBudget { pinned_bytes: pinned, cache_bytes: u64::MAX }, cache))
}

fn font(variant: u16) -> Arc<[u8]> {
    Arc::from(named_font(&[(0x409, 1, "Reg Fam"), (0x409, 2, "Regular")], 400, 0x40, variant).build())
}

fn faces(list: &[(Variant, u16)]) -> Vec<HostFace> {
    list.iter().map(|(v, n)| HostFace::new(*v, font(*n))).collect()
}

const R: Variant = Variant::Regular;
const B: Variant = Variant::Bold;
const I: Variant = Variant::Italic;
const Z: Variant = Variant::BoldItalic;

#[test]
fn a_family_answers_its_own_variant_with_the_key_family_and_variant() {
    let reg = registry(u64::MAX);
    reg.push_family("Calibri", FontSource::Installed, true, faces(&[(R, 1), (B, 2), (I, 3), (Z, 4)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    for (bold, italic, key) in [(false, false, "regular"), (true, false, "bold"), (false, true, "italic"), (true, true, "boldItalic")] {
        let a = p.face("Calibri", bold, italic).unwrap();
        assert_eq!(a.key, format!("Calibri|{key}"));
        assert_eq!(a.family, "Calibri");
        assert_eq!((a.bold, a.italic, a.synth_bold, a.synth_italic, a.stand_in), (bold, italic, false, false, false));
    }
    assert!(reg.missing().is_empty(), "nothing was missing");
}

#[test]
fn a_missing_variant_falls_back_in_the_ts_order_and_is_synthesised() {
    let reg = registry(u64::MAX);
    reg.push_family("Only Regular", FontSource::Host, true, faces(&[(R, 1)])).unwrap();
    reg.push_family("No Bold", FontSource::Host, true, faces(&[(R, 2), (I, 3)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    let a = p.face("Only Regular", true, true).unwrap();
    assert_eq!((a.key.as_str(), a.synth_bold, a.synth_italic), ("Only Regular|regular", true, true));
    // bold italic wanted: order is boldItalic, bold, italic, regular: italic is the first found
    let b = p.face("No Bold", true, true).unwrap();
    assert_eq!((b.key.as_str(), b.synth_bold, b.synth_italic), ("No Bold|italic", true, false));
    let missing = reg.missing();
    assert_eq!(missing.len(), 2);
    assert_eq!((missing[0].family.as_str(), missing[0].variants.clone()), ("Only Regular", vec![Z]));
    assert_eq!((missing[1].family.as_str(), missing[1].variants.clone()), ("No Bold", vec![Z]));
}

#[test]
fn an_unknown_family_is_none_and_noted_and_a_default_source_is_none_without_a_note() {
    let reg = registry(u64::MAX);
    reg.push_family("Fallback Co", FontSource::Default, true, faces(&[(R, 1)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    assert!(p.face("Unknown Co", false, true).is_none());
    assert!(p.face("Fallback Co", false, false).is_none(), "the engine follows its own chain");
    let missing = reg.missing();
    assert_eq!(missing.len(), 1);
    assert_eq!((missing[0].family.as_str(), missing[0].variants.clone()), ("Unknown Co", vec![I]));
    let taken = reg.take_missing();
    assert_eq!(taken, missing);
    assert!(reg.missing().is_empty());
    assert!(reg.take_missing().is_empty());
    // asking again notes it again, but a pair is requested once
    assert!(p.face("Unknown Co", false, true).is_none(), "the provider remembers the answer of its call");
    let p2 = ViewerProvider::new(Arc::clone(&reg));
    assert!(p2.face("unknown co", false, true).is_none());
    assert_eq!(reg.missing().len(), 1);
    assert!(reg.take_missing().is_empty(), "unknown co italic was already requested, in any case");
    assert!(p2.face("Unknown Co", true, false).is_none());
    assert_eq!(reg.take_missing().len(), 1, "another variant is a new request");
}

#[test]
fn hosted_assets_come_first_and_keep_the_family_as_asked() {
    let reg = registry(u64::MAX);
    reg.push_family("Aptos", FontSource::Bundled, true, faces(&[(R, 1), (B, 2)])).unwrap();
    let table = Arc::new(AdvanceTable::parse_json(r#"{"11":{"a":123.5}}"#).unwrap());
    let mut asset = HostFace::new(R, font(9));
    asset.advances = Some(Arc::clone(&table));
    reg.push_asset("Aptos", asset).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    let a = p.face("APTOS", false, false).unwrap();
    assert_eq!(a.key, "APTOS|regular");
    assert_eq!(a.advances.as_ref().unwrap().get("11", u32::from('a')), Some(123.5));
    // bold: the asset order is bold, regular: the regular asset answers (synthetic bold), the resolver's bold is not used
    let b = p.face("Aptos", true, false).unwrap();
    assert_eq!((b.key.as_str(), b.synth_bold), ("Aptos|regular", true));
    assert_eq!(b.cached_face().content_key(), a.cached_face().content_key());
}

#[test]
fn assets_must_be_font_files_that_parse() {
    let reg = registry(u64::MAX);
    let pool = pool_files();
    let get = |name: &str| pool.iter().find(|(n, _)| *n == name).map(|(_, b)| Arc::<[u8]>::from(b.clone())).unwrap();
    assert_eq!(reg.push_asset("A", HostFace::new(R, get("garbage"))), Err(PushError::NotAFont), "not a font file: the TS isFontFile");
    assert_eq!(reg.push_asset("A", HostFace::new(R, get("cut"))), Err(PushError::NotAFont), "a font file that does not open");
    assert!(reg.push_asset("A", HostFace::new(R, get("carlito-regular"))).is_ok());
    assert_eq!(reg.counts(), (0, 1));
    assert!(reg.remove_asset("a"));
    assert_eq!(reg.counts(), (0, 0));
}

#[test]
fn a_licence_that_forbids_embedding_gives_the_metric_substitute() {
    let reg = registry(u64::MAX);
    reg.push_family("Calibri", FontSource::LocalAccess, false, faces(&[(R, 1), (B, 2)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    assert!(p.face("Calibri", false, false).is_none(), "Carlito is not there yet");
    let m = reg.missing();
    assert_eq!((m[0].family.as_str(), m[0].variants.clone()), ("Carlito", vec![R]));
    reg.push_family("Carlito", FontSource::Bundled, true, faces(&[(R, 5), (B, 6)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    let a = p.face("Calibri", true, false).unwrap();
    assert_eq!((a.key.as_str(), a.family.as_str(), a.stand_in), ("Carlito|bold", "Carlito", false));
    // the substitute lacks the variant: its regular one
    let i = p.face("Calibri", false, true).unwrap();
    assert_eq!((i.key.as_str(), i.synth_italic), ("Carlito|regular", true));
    assert!(p.face("Constructor", false, false).is_none(), "own keys only");
}

#[test]
fn a_family_the_resolver_substituted_is_a_stand_in() {
    let reg = registry(u64::MAX);
    reg.push_family("Missing Co", FontSource::Substitute, true, faces(&[(R, 1)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    let a = p.face("Missing Co", false, false).unwrap();
    assert!(a.stand_in);
}

#[test]
fn a_file_that_does_not_parse_is_skipped_for_the_next_variant() {
    let reg = registry(u64::MAX);
    let pool = pool_files();
    let get = |name: &str| pool.iter().find(|(n, _)| *n == name).map(|(_, b)| Arc::<[u8]>::from(b.clone())).unwrap();
    let broken = vec![HostFace::new(B, get("garbage")), HostFace::new(R, get("carlito-regular"))];
    reg.push_family("Broken Bold", FontSource::Host, true, broken).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    let a = p.face("Broken Bold", true, false).unwrap();
    assert_eq!((a.key.as_str(), a.synth_bold), ("Broken Bold|regular", true));
}

#[test]
fn identity_is_per_provider_object_and_per_spelling() {
    let reg = registry(u64::MAX);
    reg.push_family("Calibri", FontSource::Host, true, faces(&[(R, 1)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    let a = p.face("Calibri", false, false).unwrap();
    let again = p.face("Calibri", false, false).unwrap();
    assert_eq!(a.id(), again.id());
    assert!(Arc::ptr_eq(&a, &again));
    let other_case = p.face("calibri", false, false).unwrap();
    assert_ne!(a.id(), other_case.id(), "the TS cache is keyed by the family as asked");
    assert!(a.cached_face().same_entry(other_case.cached_face()), "but the file is one face in the cache");
    let bold_over_regular = p.face("Calibri", true, false).unwrap();
    assert_ne!(a.id(), bold_over_regular.id(), "the same file answers two requests with two objects");
    let next_call = ViewerProvider::new(Arc::clone(&reg)).face("Calibri", false, false).unwrap();
    assert_ne!(a.id(), next_call.id());
    assert!(p.face("Nope", false, false).is_none());
    assert_eq!(p.remembered(), 4, "Calibri x3 spellings or variants and the unknown family");
}

#[test]
fn what_is_pushed_is_charged_once_per_content_and_refused_past_the_budget() {
    let one = font(1);
    let size = one.len() as u64;
    let reg = registry(size * 3 + size / 2);
    reg.push_family("A", FontSource::Host, true, vec![HostFace::new(R, Arc::clone(&one)), HostFace::new(B, font(1))]).unwrap();
    assert_eq!(reg.pinned_bytes(), size, "two buffers of one content cost once");
    reg.push_family("B", FontSource::Host, true, faces(&[(R, 2), (B, 3)])).unwrap();
    assert_eq!(reg.pinned_bytes(), size * 3);
    let over = reg.push_family("C", FontSource::Host, true, faces(&[(R, 4)]));
    assert_eq!(over, Err(PushError::OverBudget));
    assert_eq!(reg.pinned_bytes(), size * 3, "a refused push changes nothing");
    assert_eq!(reg.counts(), (2, 0));
    // a replacement is charged while the old family is still held: refused here, fine once it is removed
    assert_eq!(reg.push_family("B", FontSource::Host, true, faces(&[(R, 4)])), Err(PushError::OverBudget));
    assert!(reg.remove_family("B"));
    assert_eq!(reg.pinned_bytes(), size);
    reg.push_family("B", FontSource::Host, true, faces(&[(R, 4)])).unwrap();
    assert_eq!(reg.pinned_bytes(), size * 2);
    assert!(reg.remove_family("a"));
    assert_eq!(reg.pinned_bytes(), size);
    let g = reg.generation();
    reg.clear();
    assert_eq!(reg.pinned_bytes(), 0);
    assert!(reg.generation() > g);
}

#[test]
fn an_answer_keeps_what_was_pushed_alive_until_it_drops() {
    let one = font(1);
    let size = one.len() as u64;
    let reg = registry(size * 2);
    reg.push_family("A", FontSource::Host, true, faces(&[(R, 1)])).unwrap();
    let p = ViewerProvider::new(Arc::clone(&reg));
    let answer = p.face("A", false, false).unwrap();
    reg.clear();
    assert_eq!(reg.pinned_bytes(), size, "the answer still holds the face and its charge");
    assert_eq!(answer.face().num_glyphs(), 5);
    drop(answer);
    drop(p);
    assert_eq!(reg.pinned_bytes(), 0);
}

#[test]
fn a_collection_is_pushed_with_its_face_index() {
    let face = |n: u16, weight: u16| {
        let f = named_font(&[(0x409, 1, "Coll"), (0x409, 2, "Regular")], weight, 0x40, n);
        (f.tables.clone(), uo_testkit::generate::sfnt::SfntOptions::default())
    };
    let ttc: Arc<[u8]> = Arc::from(build_ttc(&[face(1, 400), face(2, 700)], 0x0001_0000));
    let reg = registry(u64::MAX);
    let mut second = HostFace::new(R, Arc::clone(&ttc));
    second.ttc_index = Some(1);
    reg.push_family("Coll", FontSource::Host, true, vec![second]).unwrap();
    let a = ViewerProvider::new(Arc::clone(&reg)).face("Coll", false, false).unwrap();
    assert_ne!(a.bytes().len(), ttc.len(), "the face, not the collection");
    let mut bad = HostFace::new(R, Arc::clone(&ttc));
    bad.ttc_index = Some(1000);
    assert_eq!(reg.push_family("Bad", FontSource::Host, true, vec![bad]), Err(PushError::Unreadable));
}

#[test]
fn the_named_policies() {
    let reg = registry(u64::MAX);
    assert_eq!(PolicyName::from_name("viewer"), Some(PolicyName::Viewer));
    assert_eq!(PolicyName::from_name("server-substitute"), Some(PolicyName::ServerSubstitute));
    assert_eq!(PolicyName::from_name("oracle"), Some(PolicyName::Oracle));
    assert_eq!(PolicyName::from_name("editor"), Some(PolicyName::Editor));
    assert_eq!(PolicyName::from_name("Viewer"), None);
    assert_eq!(PolicyName::ServerSubstitute.as_str(), "server-substitute");
    assert!(provider(&reg, PolicyName::Viewer).is_ok());
    assert!(matches!(provider(&reg, PolicyName::ServerSubstitute), Err(PolicyError::NoSource)));
    assert!(matches!(provider(&reg, PolicyName::Oracle), Err(PolicyError::NoSource)));
    assert!(matches!(provider(&reg, PolicyName::Editor), Err(PolicyError::NotBuilt)));
}
