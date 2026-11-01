//! M3 (stage 4, memory gate): allocated bytes and peak live bytes of the package operations on
//! the large inputs of the plan, asserted against the numbers in the module record
//! (`core/docs/modules/uo-opc-container.md`, "Stage 4 — memory") with a 10% margin. Allocation
//! counts are deterministic (same on every OS and in debug and release); a reallocation counts as
//! a new allocation of the new size.
//!
//! The TS and C# references of the record come from `memory_support/ts_budgets.mjs` and
//! `memory_support/cs_budgets.cs`.
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

mod memory_support;
mod xml_support;

use memory_support::{
    DECK_DEFLATED_MEDIA, DECK_STORED_MEDIA, Kind, deck_with_large_media, edit, open_options, preset, read_and_parse_all, read_repo_file,
};
use std::time::Instant;

use uo_alloc::{Usage, measure};
use uo_opc::Limits;
use uo_opc::limits::is_xml_like;
use uo_opc::package::{Package, SaveOptions};
use uo_opc::xml::parse;

/// Asserts a measured number is within 10% above the recorded budget (M3), and prints both.
fn within(what: &str, measured: i64, budget: i64) {
    println!("{what}: {measured} B (budget {budget} B)");
    // UO_M3_RECORD=1 prints the numbers without asserting (to record new budgets)
    if std::env::var("UO_M3_RECORD").as_deref() == Ok("1") {
        return;
    }
    assert!(measured as f64 <= budget as f64 * 1.10, "M3: {what} = {measured} B, more than 10% over the budget of {budget} B");
}

fn check(what: &str, u: Usage, allocated: u64, peak: i64) {
    within(&format!("{what} allocated"), u.allocated as i64, allocated as i64);
    within(&format!("{what} peak"), u.peak_above_start, peak);
}

/// `sheet/samples/giant-1m.xlsx`: 2.85 MB on disk, `xl/worksheets/sheet1.xml` 31.7 MB inflated.
#[test]
fn m3_giant_xlsx() {
    let bytes = read_repo_file("sheet/samples/giant-1m.xlsx");
    let ((pkg, parts, elements), open_read_parse) = measure(|| {
        let pkg = Package::open(&bytes, &Limits::DEFAULT, &open_options(Kind::Xlsx)).unwrap();
        let (parts, elements) = read_and_parse_all(&pkg, Kind::Xlsx);
        (pkg, parts, elements)
    });
    println!("giant-1m: {parts} parts, {elements} elements, produced {} B, kept {} B", pkg.produced_bytes(), open_read_parse.live_delta);
    check("giant-1m open + read all + parse all", open_read_parse, 355_418_000, 146_113_000);
    // what the open package keeps: the input copy and the inflated parts
    within("giant-1m kept after open + read + parse", open_read_parse.live_delta, 34_571_000);
    // save untouched: every entry copied raw (nothing deflated again), after the CRC-32 of the
    // parts already read (D5)
    let started = Instant::now();
    let (saved, save_raw) = measure(|| pkg.save(&SaveOptions::default()).unwrap());
    println!("giant-1m save untouched: output {} B, capacity {} B, {:?}", saved.len(), saved.capacity(), started.elapsed());
    check("giant-1m save untouched", save_raw, 8_550_000, 5_697_000);
    drop(saved);
    // the verification is kept: the same save without it (the time it costs)
    let started = Instant::now();
    drop(pkg.save(&SaveOptions::default()).unwrap());
    println!("giant-1m save untouched again (verification kept): {:?}", started.elapsed());
    // the parse of the 31.7 MB sheet alone (source copy + tree)
    let text = pkg.part_text("xl/worksheets/sheet1.xml").unwrap().unwrap();
    let (doc, parse_sheet) = measure(|| parse(text.as_ref(), &preset(Kind::Xlsx, "xl/worksheets/sheet1.xml")).unwrap());
    println!("giant-1m sheet1 tree: {} elements, heap {} B", doc.element_count(), doc.heap_bytes());
    // D2: the default element cap is about 10 × the largest real part of the repository
    assert!(u64::from(Limits::DEFAULT.max_xml_elements) >= 9 * doc.element_count() as u64);
    check("giant-1m parse sheet1.xml", parse_sheet, 259_726_000, 111_543_000);
    drop(doc);
    drop(text);
    drop(pkg);
}

/// The 31.7 MB sheet deflated again on save (an edit of the sheet): miniz_oxide's state and the
/// growing output (about 8 s in debug).
#[test]
fn m3_giant_xlsx_save_edited_sheet() {
    let bytes = read_repo_file("sheet/samples/giant-1m.xlsx");
    let mut pkg = Package::open(&bytes, &Limits::DEFAULT, &open_options(Kind::Xlsx)).unwrap();
    let text = pkg.part_text("xl/worksheets/sheet1.xml").unwrap().unwrap().into_owned();
    pkg.set_part_text("xl/worksheets/sheet1.xml", text);
    // the other entries were never read: the save verifies them without keeping them (D5)
    let (saved, u) = measure(|| pkg.save(&SaveOptions::default()).unwrap());
    println!("giant-1m save with sheet1 deflated: output {} B", saved.len());
    check("giant-1m save edited sheet", u, 25_021_000, 18_717_000);
}

/// The largest `document.xml` of the oracle corpus (`justify4/sw-16-8.docx`, 188 KB).
#[test]
fn m3_largest_oracle_document() {
    let bytes = read_repo_file(memory_support::SOAK_DOCX);
    let ((pkg, doc), open_read_parse) = measure(|| {
        let pkg = Package::open(&bytes, &Limits::DEFAULT, &open_options(Kind::Docx)).unwrap();
        let text = pkg.part_text("word/document.xml").unwrap().unwrap().into_owned();
        let doc = parse(text, &preset(Kind::Docx, "word/document.xml")).unwrap();
        (pkg, doc)
    });
    println!(
        "sw-16-8 document.xml: {} elements, source {} B, tree heap {} B, kept {} B",
        doc.element_count(),
        doc.source().len(),
        doc.heap_bytes(),
        open_read_parse.live_delta
    );
    check("sw-16-8 open + read + parse document.xml", open_read_parse, 2_311_000, 822_000);
    drop(doc);
    let mut pkg = pkg;
    let (saved, save_edited) = measure(|| {
        edit(&mut pkg, Kind::Docx, 1);
        pkg.save(&SaveOptions::default()).unwrap()
    });
    println!("sw-16-8 edit + save: output {} B", saved.len());
    check("sw-16-8 edit + save", save_edited, 4_494_000, 897_000);
}

/// A deck with large media: `reference.pptx` plus 64 MiB of deflated zeros and a 16 MiB stored
/// "video". The package is lazy: opening, reading and parsing every XML part, editing a slide and
/// saving never inflate nor copy the media apart from the output archive itself.
#[test]
fn m3_deck_with_large_media() {
    let deck = deck_with_large_media();
    let deck_len = deck.len();
    let (pkg, open_parse) = measure(|| {
        // the deck is moved in: the package keeps it as its source, no copy
        let pkg = Package::open_owned(deck, &Limits::DEFAULT, &open_options(Kind::Pptx)).unwrap();
        let mut elements = 0;
        let names: Vec<String> = pkg.part_names().filter(|n| is_xml_like(n)).map(str::to_owned).collect();
        for n in &names {
            let text = pkg.part_text(n).unwrap().unwrap();
            elements += parse(text.as_ref(), &preset(Kind::Pptx, n)).unwrap().element_count();
        }
        assert!(elements > 0);
        pkg
    });
    println!(
        "deck: {deck_len} B archive, produced {} B (XML only), {} parts, kept {} B",
        pkg.produced_bytes(),
        pkg.len(),
        open_parse.live_delta
    );
    // nothing near the media size was produced or allocated
    assert!(pkg.produced_bytes() < 200_000);
    check("deck open_owned + parse all XML", open_parse, 1_189_000, 148_000);
    let mut pkg = pkg;
    let produced_before = pkg.produced_bytes();
    let started = Instant::now();
    let (saved, save_edited) = measure(|| {
        edit(&mut pkg, Kind::Pptx, 1);
        pkg.save(&SaveOptions::default()).unwrap()
    });
    let first_save = started.elapsed();
    // the output holds the media raw: about the input size
    println!("deck edit + save: output {} B, capacity {} B, {first_save:?}", saved.len(), saved.capacity());
    // gate 2, D5: before copying them raw the save verified the media (the 64 MiB part inflated
    // through a 64 KiB window, the stored one by its CRC): they count toward the total as a
    // read does, and nothing is held
    let media = (DECK_DEFLATED_MEDIA + DECK_STORED_MEDIA) as u64;
    assert!(pkg.produced_bytes() >= produced_before + media && pkg.produced_bytes() < produced_before + media + 300_000);
    // the save allocates only the growing output: each growth of the Vec counts as a new
    // allocation (at most 2× the final capacity, itself at most 2× the length), plus the
    // verification window; never the 64 MiB of zeros
    assert!(save_edited.allocated < 4 * saved.len() as u64);
    assert!((save_edited.peak_above_start as usize) < 2 * saved.len() + (1 << 20));
    // holding the zeros would add 64 MiB to the peak
    assert!((save_edited.peak_above_start as usize) < DECK_DEFLATED_MEDIA);
    check("deck edit + save", save_edited, 54_422_000, 34_479_000);
    // the media were copied raw: the source bytes are in the output
    let zeros = uo_opc::zip::read(&saved, u32::MAX).unwrap().into_iter().find(|e| e.name == "ppt/media/zeros.bin").unwrap();
    assert_eq!(zeros.method, 8);
    assert_eq!(zeros.compressed(&saved).len(), memory_support::zeros_deflate(DECK_DEFLATED_MEDIA).len());
    drop(saved);
    // the verification is kept per entry: a second save does not inflate the media again
    let started = Instant::now();
    let (saved, save_again) = measure(|| pkg.save(&SaveOptions::default()).unwrap());
    let second_save = started.elapsed();
    println!(
        "deck save again (verification kept): allocated {} B, peak {} B, {second_save:?} (first save {first_save:?})",
        save_again.allocated, save_again.peak_above_start
    );
    assert!(save_again.peak_above_start <= save_edited.peak_above_start);
    drop(saved);
    // the contrast: reading the deflated media inflates 64 MiB
    let (n, read_media) = measure(|| pkg.part("ppt/media/zeros.bin").unwrap().unwrap().len());
    assert_eq!(n, DECK_DEFLATED_MEDIA);
    // the stored media is a slice of the source: reading it allocates nothing
    let (m, read_video) = measure(|| pkg.part("ppt/media/video.mp4").unwrap().unwrap().len());
    assert_eq!(m, DECK_STORED_MEDIA);
    println!(
        "deck read zeros.bin: allocated {} B, peak {} B; read video.mp4 (stored): allocated {} B",
        read_media.allocated, read_media.peak_above_start, read_video.allocated
    );
    // the stored part is a slice of the source (the only allocation is the lowercased extension
    // checked by `Limits::part_cap`)
    assert!(read_video.allocated < 64);
    // inflating without trusting the declared size: the output Vec doubles from 3× the input up
    // past 64 MiB and is shrunk to fit (each step a new allocation)
    check("deck read 64 MiB deflated media", read_media, 228_154_000, 81_157_000);
}

/// What one deflated entry costs regardless of its size: miniz_oxide's compressor state, which
/// every `ZipWriter::add` of an edited part allocates (and frees) again. Explains most of the
/// save numbers of small edits.
#[test]
fn m3_deflate_state_per_entry() {
    let (_, u) = measure(|| uo_opc::flate::deflate_raw(b"<a/>", 6));
    check("deflate of a 4-byte part", u, 253_624, 253_622);
}
