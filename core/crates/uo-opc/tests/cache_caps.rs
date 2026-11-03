//! M5 (stage 4, memory gate): every cache or index the container layer keeps is bounded, and a
//! test fills it past its bound. uo-opc has no global or cross-call cache (`clear_caches()` is a
//! no-op); what it keeps lives inside one object and is bounded by an input limit:
//!
//! | cache | lives in | bound |
//! |---|---|---|
//! | ASCII index (fallback of `resolve_target`, gate 2 D6, and of `find` under `CaseFold::Ascii`) | `Package` | one entry per part; parts of an opened archive ≤ `Limits::max_entries` |
//! | inflated parts (`OnceCell` per entry) | `Package` | `Limits::max_total_bytes` over all first reads, `part_cap` per part |
//! | what a save verified (D5) | `Package` | a fixed record per entry |
//! | mini stream (read once) and the path index | `CfbFile` | the length of the file |
//! | interned names | `xml::Document` | distinct names of one document, ≤ its source length |
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

mod cfb_support;

use cfb_support::{Spec, build, pattern, root_streams};
use uo_alloc::{measure, thread_stats};
use uo_opc::cfb::CfbFile;
use uo_opc::package::{CaseFold, OpenOptions, Package};
use uo_opc::xml::{ParseOptions, parse};
use uo_opc::zip::{self, WriteOptions};
use uo_opc::{Error, LimitKind, Limits};

fn archive(names: &[String], data: impl Fn(usize) -> Vec<u8>) -> Vec<u8> {
    let parts: Vec<(String, Vec<u8>)> = names.iter().enumerate().map(|(i, n)| (n.clone(), data(i))).collect();
    zip::write(parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())), WriteOptions { level: 1, ..WriteOptions::default() }).unwrap()
}

/// The ASCII index through `find` (`CaseFold::Ascii`): built once on the first fallback lookup,
/// one entry per part (names that fold to the same key keep one), never grown by lookups, dropped
/// when the part set changes; the parts of an opened archive are capped by `max_entries`.
#[test]
fn m5_caseless_index() {
    const N: usize = 4000;
    let names: Vec<String> = (0..N).map(|i| format!("xl/worksheets/Sheet{i}.xml")).collect();
    let bytes = archive(&names, |_| b"<x/>".to_vec());
    let opts = OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() };
    // past the cap: one entry more than `max_entries` does not open
    let capped = Limits { max_entries: N as u32 - 1, ..Limits::DEFAULT };
    assert!(matches!(Package::open(&bytes, &capped, &opts), Err(Error::Limit { kind: LimitKind::Entries, .. })));
    let limits = Limits { max_entries: N as u32, ..Limits::DEFAULT };
    let mut pkg = Package::open(&bytes, &limits, &opts).unwrap();
    // the first fallback lookup builds the index
    let (hit, build) = measure(|| pkg.find_name("XL/WORKSHEETS/SHEET7.XML").map(str::to_owned));
    assert_eq!(hit.as_deref(), Some("xl/worksheets/Sheet7.xml"));
    let name_bytes: usize = names.iter().map(String::len).sum();
    println!("caseless index of {N} parts ({name_bytes} B of names): {} B kept, {} B allocated", build.live_delta, build.allocated);
    // one folded name per part plus the hash table: at most 2× the names + 48 B per part
    assert!(build.live_delta > 0 && (build.live_delta as usize) < 2 * name_bytes + 48 * N, "index {} B", build.live_delta);
    // fill past it: 100,000 lookups of distinct missing names never grow the index
    let before = thread_stats().live;
    for i in 0..100_000 {
        assert!(pkg.find_name(&format!("missing/Part{i}.XML")).is_none());
    }
    assert_eq!(thread_stats().live, before, "lookups grew the index");
    // a change of the part set drops it (the bytes return)
    let (_, dropped) = measure(|| pkg.remove_part("xl/worksheets/Sheet0.xml"));
    assert!(dropped.live_delta <= -build.live_delta + 64, "index not dropped: {} B", dropped.live_delta);
    // names that fold to the same key keep one entry
    let same: Vec<String> =
        (0..N).map(|i| (0..12).map(|b| if (i >> b) & 1 == 1 { 'A' } else { 'a' }).collect::<String>() + ".xml").collect();
    let pkg = Package::from_parts(same.iter().map(|n| (n.clone(), vec![1u8])), &limits, &opts);
    let (hit, one) = measure(|| pkg.find_name("AAAAAAAAAAAA.XML").map(str::to_owned));
    assert_eq!(hit.as_deref(), Some(same[0].as_str()));
    println!("caseless index of {N} parts folding to one key: {} B kept", one.live_delta);
    // the table is sized for the part count up front (8,192 buckets of 33 B here), the strings
    // for one key only: still bounded by the parts, and under the distinct-name index
    assert!((one.live_delta as usize) < 70 * N && one.live_delta < build.live_delta, "colliding names kept {} B", one.live_delta);
}

/// The ASCII index of `resolve_target` (D6): built on the first fallback, one entry per part,
/// never grown by lookups, dropped when the part set changes; the same index as the fallback of
/// `find` under `CaseFold::Ascii` (one index since R2, gate 3).
#[test]
fn m5_ascii_index_of_resolve_target() {
    const N: usize = 4000;
    let names: Vec<String> = (0..N).map(|i| format!("word/media/Image{i}.png")).collect();
    let bytes = archive(&names, |_| b"png".to_vec());
    let limits = Limits { max_entries: N as u32, ..Limits::DEFAULT };
    let mut pkg = Package::open(&bytes, &limits, &OpenOptions::default()).unwrap();
    let (hit, build) = measure(|| pkg.resolve_target("word/document.xml", "media/IMAGE7.PNG").map(str::to_owned));
    assert_eq!(hit.as_deref(), Some("word/media/Image7.png"));
    let name_bytes: usize = names.iter().map(String::len).sum();
    println!("ASCII index of {N} parts ({name_bytes} B of names): {} B kept", build.live_delta);
    assert!(build.live_delta > 0 && (build.live_delta as usize) < 2 * name_bytes + 48 * N, "index {} B", build.live_delta);
    let before = thread_stats().live;
    for i in 0..100_000 {
        assert!(pkg.resolve_target("word/document.xml", &format!("media/Missing{i}.png")).is_none());
    }
    assert_eq!(thread_stats().live, before, "lookups grew the index");
    let (_, dropped) = measure(|| pkg.remove_part("word/media/Image0.png"));
    assert!(dropped.live_delta <= -build.live_delta + 64, "index not dropped: {} B", dropped.live_delta);
    // one index for both lookups
    let ascii = Package::open(&bytes, &limits, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }).unwrap();
    let (_, first) = measure(|| ascii.find_name("WORD/media/image7.png").map(str::len));
    let (_, second) = measure(|| ascii.resolve_target("", "WORD/media/image8.png").map(str::len));
    assert!(first.live_delta > 0 && second.live_delta == 0, "{} then {}", first.live_delta, second.live_delta);
}

/// The inflated copies of the parts: capped by `max_total_bytes` over all first reads; releasing
/// and reading again neither counts twice nor holds more than the cap.
#[test]
fn m5_inflated_parts_are_capped_by_the_total() {
    const PART: usize = 100_000;
    let names: Vec<String> = (0..40).map(|i| format!("ppt/media/image{i}.bin")).collect();
    let bytes = archive(&names, |i| pattern(PART, i as u32));
    let limits = Limits { max_total_bytes: 1_000_000, ..Limits::DEFAULT };
    let pkg = Package::open(&bytes, &limits, &OpenOptions::default()).unwrap();
    let start = thread_stats().live;
    let mut read = 0;
    let mut err = None;
    for n in &names {
        match pkg.part(n) {
            Ok(_) => read += 1,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    let held = thread_stats().live - start;
    assert_eq!(read, 10);
    assert!(matches!(err, Some(Error::Limit { kind: LimitKind::TotalBytes, .. })));
    assert!(held as usize <= 1_000_000 + 64 * 40, "held {held} B over the 1,000,000 B total");
    // past the cap again: release everything and read the same parts in a loop
    let mut pkg = pkg;
    for _ in 0..20 {
        pkg.release_all();
        for n in names.iter().take(10) {
            pkg.part(n).unwrap();
        }
        assert!(pkg.part(&names[10]).is_err());
    }
    assert_eq!(pkg.produced_bytes(), 1_000_000);
    assert_eq!(thread_stats().live - start, held);
    // printed last: the captured test output is a buffer of this thread
    println!("inflated parts: {read} read ({held} B held) before {err:?}");
}

/// The CFB mini stream and stream reads never exceed the file, whatever the directory declares;
/// the path index holds one entry per directory entry.
#[test]
fn m5_cfb_mini_stream_is_bounded_by_the_file() {
    let small = pattern(3000, 1);
    let big = pattern(9000, 2);
    let streams: [(&str, &[u8]); 2] = [("Big", &big), ("Small", &small)];
    let (child, nodes) = root_streams(&streams);
    let mut f = build(&Spec::v4(child, nodes));
    // the root (the mini stream) and "Big" declare 2^40 + 2^32 − 1 bytes
    for id in [0u32, 1] {
        let at = f.dir_entry(id) + 0x78;
        f.set_u32(at, u32::MAX);
        f.set_u32(at + 4, 0x100);
    }
    let len = f.bytes.len();
    let file = CfbFile::open(&f.bytes).unwrap();
    let ((s, b), u) = measure(|| (file.stream("Small").unwrap().unwrap().len(), file.stream("Big").unwrap().unwrap().len()));
    println!(
        "CFB of {len} B declaring 1 TiB streams: Small {s} B, Big {b} B; allocated {} B, kept {} B (mini stream)",
        u.allocated, u.live_delta
    );
    assert!(u.allocated as usize <= 3 * len, "allocated {} B for a {len} B file", u.allocated);
    assert!(u.live_delta as usize <= len);
    assert!(s <= len && b <= len);
    // reading many times keeps one mini stream
    let before = thread_stats().live;
    for _ in 0..1000 {
        file.stream("Small").unwrap();
    }
    assert_eq!(thread_stats().live, before);
}

/// Interned names: one entry per distinct name of a document; repeated names add nothing, and
/// distinct names are bounded by the source length. The adversarial cases (shortest distinct
/// names, minimal elements) give the worst bytes-per-source-byte of the tree, recorded in the
/// module record.
#[test]
fn m5_interned_names() {
    const N: usize = 50_000;
    let wrap = |body: String| format!("<r>{body}</r>");
    let repeated = wrap((0..N).map(|_| "<cell a=\"1\"/>").collect());
    let distinct = wrap((0..N).map(|i| format!("<c{i:07} a{i:07}=\"1\"/>")).collect());
    // the shortest distinct names: 3 characters from a 52-letter alphabet
    let letters: Vec<char> = ('a'..='z').chain('A'..='Z').collect();
    let short = |i: usize| -> String { [letters[i % 52], letters[(i / 52) % 52], letters[(i / 2704) % 52]].iter().collect() };
    let short_distinct = wrap((0..N).map(|i| format!("<{}/>", short(i))).collect());
    let minimal = wrap("<a/>".repeat(N));
    let mut factors = Vec::new();
    let last_short = short(N - 1);
    let cases = [
        ("repeated name", &repeated, 3, "cell"),
        ("distinct names", &distinct, 2 * N + 1, "a0049999"),
        ("short distinct names", &short_distinct, N + 1, last_short.as_str()),
        ("minimal elements", &minimal, 2, "a"),
    ];
    for (what, src, names, probe) in cases {
        let (doc, u) = measure(|| parse(src.as_str(), &ParseOptions::LENIENT).unwrap());
        assert_eq!(doc.element_count(), N + 1);
        let f = u.live_delta as f64 / src.len() as f64;
        println!("{what}: {} B source, {names} names, {} B kept ({f:.2}x), peak {} B", src.len(), u.live_delta, u.peak_above_start);
        assert!(doc.name_id(probe).is_some());
        factors.push(f);
    }
    // repeated names add nothing to the table: the tree is the source copy plus ≈ 40 B per element
    // (measured 3.77x, +10%)
    assert!(factors[0] < 3.77 * 1.1);
    // distinct names cost ≈ 85 B each (text, Arc header, list slot, hash slot): linear in the
    // source (measured 8.56x, +10%)
    assert!(factors[1] < 8.56 * 1.1, "distinct names: {:.2}x", factors[1]);
    // the worst cases, linear too (measured 17.13x and 7.00x, +10%): short distinct names and
    // minimal elements
    assert!(factors[2] < 17.13 * 1.1 && factors[3] < 7.0 * 1.1, "worst cases: {:.2}x, {:.2}x", factors[2], factors[3]);
}
