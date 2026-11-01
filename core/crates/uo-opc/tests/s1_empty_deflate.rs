//! Security proposal S1 of the module record, measured (stage 4): a deflated zip entry with an
//! EMPTY compressed stream and a declared size returns that many zeros (fflate's
//! `new u8(declared)`), up to the part cap. These tests measure what a tiny crafted archive makes
//! the library allocate. They do not decide the behaviour: if S1 is fixed (an empty stream with a
//! declared size becomes `Corrupt`), the same tests check that nothing is allocated instead.
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

use uo_alloc::measure;
use uo_opc::package::{OpenOptions, Package};
use uo_opc::zip::{RawEntry, WriteOptions, ZipWriter};
use uo_opc::{Error, LimitKind, Limits};

/// An archive whose entries are deflated with no compressed bytes and the given declared sizes.
fn crafted(entries: &[(&str, u64)]) -> Vec<u8> {
    let mut w = ZipWriter::new(WriteOptions::default());
    for (name, size) in entries {
        w.add_raw(name, RawEntry { method: 8, crc32: 0, size: *size, data: &[] }).unwrap();
    }
    w.finish()
}

/// Reads `name` and returns (bytes produced, allocated, peak); `None` when the read fails.
fn read(pkg: &Package, name: &str) -> (Option<usize>, u64, i64) {
    let (r, u) = measure(|| pkg.part(name).map(|b| b.map(<[u8]>::len)));
    match r {
        Ok(n) => (n, u.allocated, u.peak_above_start),
        Err(Error::Corrupt(_)) => (None, u.allocated, u.peak_above_start),
        Err(e) => panic!("{name}: {e:?}"),
    }
}

#[test]
fn s1_xml_part_at_the_cap() {
    let cap = Limits::DEFAULT.max_xml_part_bytes;
    let zip = crafted(&[("word/document.xml", cap)]);
    let pkg = Package::open(&zip, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let (n, allocated, peak) = read(&pkg, "word/document.xml");
    println!("S1: a {} B archive, XML part declaring {cap} B: produced {n:?}, allocated {allocated} B, peak {peak} B", zip.len());
    match n {
        // current behaviour: the whole declared size, zeros, in one allocation
        Some(n) => {
            assert_eq!(n as u64, cap);
            assert!(allocated >= cap && allocated < cap + 4096);
        }
        // S1 fixed: rejected without allocating the declared size
        None => assert!(allocated < 4096),
    }
    // one byte over the cap: refused before allocating
    let zip = crafted(&[("word/document.xml", cap + 1)]);
    let pkg = Package::open(&zip, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let (r, u) = measure(|| pkg.part("word/document.xml").map(|b| b.map(<[u8]>::len)));
    println!("S1: XML part declaring {} B: {r:?}, allocated {} B", cap + 1, u.allocated);
    assert!(matches!(r, Err(Error::Limit { kind: LimitKind::PartBytes, .. }) | Err(Error::Corrupt(_))));
    assert!(u.allocated < 4096);
    // Slides opens with `skip_impossible_ratio` (`slides-core/src/package.ts:26`): an entry that
    // declares more than max(1 MiB, 1100 × compressed) is left out, so S1 stops there
    let zip = crafted(&[("ppt/slides/slide1.xml", cap), ("ppt/x.xml", 1 << 20)]);
    let slides = Package::open(&zip, &Limits::DEFAULT, &OpenOptions { skip_impossible_ratio: true, ..OpenOptions::default() }).unwrap();
    assert!(!slides.contains("ppt/slides/slide1.xml") && slides.contains("ppt/x.xml"));
    let (n, allocated, _) = read(&slides, "ppt/x.xml");
    println!("S1 with the Slides filter: the 1 MiB entry left in: produced {n:?}, allocated {allocated} B");
    // verify() (the eager TS-like check) allocates the same zeros and drops them
    let zip = crafted(&[("a.xml", 64 << 20), ("b.xml", 64 << 20)]);
    let pkg = Package::open(&zip, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let (r, u) = measure(|| pkg.verify());
    println!("S1: verify() of two XML parts declaring 64 MiB: {r:?}, allocated {} B, peak {} B", u.allocated, u.peak_above_start);
    if r.is_ok() {
        assert!(u.allocated >= 2 * (64 << 20) && u.peak_above_start < (64 << 20) + 4096);
    }
}

/// The binary cap (2 GiB per part, 4 GiB in total): a ~200-byte archive holds 4 GiB of zeros
/// live. Ignored by default: it commits 4 GiB (`alloc_zeroed`, fast, but CI runners may not have
/// the memory). `cargo test -p uo-opc --release --test s1_empty_deflate -- --ignored --nocapture`.
#[test]
#[ignore = "allocates 4 GiB; run by hand with --ignored"]
fn s1_binary_parts_up_to_the_total() {
    let part = Limits::DEFAULT.max_binary_part_bytes;
    let zip = crafted(&[("ppt/media/a.bin", part), ("ppt/media/b.bin", part), ("ppt/media/c.bin", part)]);
    let pkg = Package::open(&zip, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let (a, alloc_a, _) = read(&pkg, "ppt/media/a.bin");
    let (b, alloc_b, _) = read(&pkg, "ppt/media/b.bin");
    let (r, u) = measure(|| pkg.part("ppt/media/c.bin").map(|b| b.map(<[u8]>::len)));
    println!(
        "S1: a {} B archive: a.bin {a:?} ({alloc_a} B allocated), b.bin {b:?} ({alloc_b} B allocated), held {} B; c.bin {r:?} ({} B allocated)",
        zip.len(),
        pkg.produced_bytes(),
        u.allocated
    );
    if a.is_some() {
        assert_eq!(pkg.produced_bytes(), 2 * part);
        assert!(matches!(r, Err(Error::Limit { kind: LimitKind::TotalBytes, .. })));
    }
}
