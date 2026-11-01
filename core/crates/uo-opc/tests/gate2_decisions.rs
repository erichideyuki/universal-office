//! The gate-2 decisions of the module record (`core/docs/modules/uo-opc-container.md`, "Gate 2 —
//! summary and decisions"), one group of tests per decision. D1 is covered by `flate_fflate.rs`
//! and `s1_empty_deflate.rs`, D4 by the replay (`opc_parity.rs`), D7 by `limits.rs` and the wasm
//! build (`uo-wasm/tests/opc_memory.test.mjs`).
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

mod parity_support;
mod xml_support;

use uo_alloc::measure;
use uo_opc::content_types::ContentTypes;
use uo_opc::package::{CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::xml::{ParseOptions, parse};
use uo_opc::zip::{self, RawEntry, WriteOptions, ZipWriter};
use uo_opc::{Error, LimitKind, Limits, flate, part_name, rels};
use uo_testkit::generate::{RawZipEntry, RawZipOptions, build_zip};

use xml_support::{DOCX_POS, SHEET, dump};

fn open(bytes: &[u8], limits: &Limits) -> Package {
    Package::open(bytes, limits, &OpenOptions::default()).unwrap()
}

// ───────────────────────────── D2: elements per part ─────────────────────────────

#[test]
fn d2_element_cap_is_an_error_and_changes_no_tree() {
    let sources = [
        (
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p/></w:body></w:document>",
            DOCX_POS,
        ),
        ("<worksheet><sheetData><row r=\"1\"><c r=\"A1\"><v>1</v></c></row></sheetData></worksheet>", SHEET),
        ("<a xmlns=\"urn:x\"><b/><!-- c --><c><d/></c></a>", ParseOptions::STRICT),
    ];
    for (src, preset) in sources {
        let free = parse(src, &preset).unwrap();
        let n = u32::try_from(free.element_count()).unwrap();
        let (capped, with_cap) = measure(|| parse(src, &ParseOptions { max_elements: Some(n), ..preset }).unwrap());
        assert_eq!(dump(capped.root(), preset.positions), dump(free.root(), preset.positions), "{src}");
        // the check allocates nothing
        let (_, without) = measure(|| parse(src, &preset).unwrap());
        assert_eq!(with_cap.allocated, without.allocated);
        assert_eq!(
            parse(src, &ParseOptions { max_elements: Some(n - 1), ..preset }).unwrap_err(),
            Error::Limit { kind: LimitKind::XmlElements, max: u64::from(n - 1), part: None }
        );
    }
    // a hostile shape: minimal elements stop at the cap
    let flat = format!("<r>{}</r>", "<a/>".repeat(100_000));
    let r = parse(flat.as_str(), &ParseOptions { max_elements: Some(1_000), ..ParseOptions::LENIENT });
    assert_eq!(r.unwrap_err(), Error::Limit { kind: LimitKind::XmlElements, max: 1_000, part: None });
}

// ───────────────────────────── D3: one XML entry point ─────────────────────────────

#[test]
fn d3_xml_part_applies_the_xml_cap_whatever_the_name() {
    let xml = format!("<document>{}</document>", "<p>text</p>".repeat(400));
    let bytes = zip::write([("word/document.bin", xml.as_bytes()), ("word/small.bin", b"<a/>")], WriteOptions::default()).unwrap();
    let limits = Limits { max_xml_part_bytes: 1000, max_binary_part_bytes: 1 << 20, ..Limits::DEFAULT };
    let over = Error::Limit { kind: LimitKind::PartBytes, max: 1000, part: Some("word/document.bin".into()) };
    // parsed first: the inflate stops at the XML cap
    let p = open(&bytes, &limits);
    let (r, u) = measure(|| p.xml_part("word/document.bin", &ParseOptions::LENIENT).map(|d| d.element_count()));
    assert_eq!(r, Err(over.clone()));
    assert!(u.peak_above_start < 64 << 10, "peak {}", u.peak_above_start);
    // read raw first (binary cap of the name), then parsed: refused all the same
    let p = open(&bytes, &limits);
    assert_eq!(p.part("word/document.bin").unwrap().unwrap().len(), xml.len());
    assert_eq!(p.xml_part("word/document.bin", &ParseOptions::LENIENT).map(|d| d.element_count()), Err(over));
    assert_eq!(p.xml_part("word/small.bin", &ParseOptions::LENIENT).unwrap().root().name(), "a");
    // a part set by the caller too
    let mut p = open(&bytes, &limits);
    p.set_part("word/set.xml", xml.clone().into_bytes());
    assert!(matches!(p.xml_part("word/set.xml", &ParseOptions::LENIENT), Err(Error::Limit { kind: LimitKind::PartBytes, .. })));
    assert_eq!(p.xml_part("nope.xml", &ParseOptions::LENIENT).unwrap_err(), Error::MissingPart("nope.xml".into()));
}

#[test]
fn d3_xml_part_applies_the_package_depth_and_elements() {
    let bytes = zip::write(
        [("deep.xml", &b"<a><b><c><d/></c></b></a>"[..]), ("flat.xml", b"<r><x/><x/><x/><x/><x/></r>"), ("ok.xml", b"<r><x/></r>")],
        WriteOptions::default(),
    )
    .unwrap();
    let limits = Limits { max_xml_depth: 3, max_xml_elements: 5, ..Limits::DEFAULT };
    let p = open(&bytes, &limits);
    // the TS lenient presets have no limits: the package's apply
    assert_eq!(
        p.xml_part("deep.xml", &ParseOptions::LENIENT).unwrap_err(),
        Error::Limit { kind: LimitKind::XmlDepth, max: 3, part: Some("deep.xml".into()) }
    );
    assert_eq!(
        p.xml_part("flat.xml", &ParseOptions::LENIENT).unwrap_err(),
        Error::Limit { kind: LimitKind::XmlElements, max: 5, part: Some("flat.xml".into()) }
    );
    // a lower limit of the caller stays
    assert!(p.xml_part("ok.xml", &ParseOptions::LENIENT).is_ok());
    assert_eq!(
        p.xml_part("ok.xml", &ParseOptions { max_elements: Some(1), ..ParseOptions::LENIENT }).unwrap_err(),
        Error::Limit { kind: LimitKind::XmlElements, max: 1, part: Some("ok.xml".into()) }
    );
    // the defaults leave the strict depth (256) and the real documents alone
    let p = open(&bytes, &Limits::DEFAULT);
    assert_eq!(p.xml_part("deep.xml", &ParseOptions::STRICT).unwrap().element_count(), 4);
}

#[test]
fn d3_xml_part_finds_and_decodes_like_the_text_readers() {
    let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("<é/>".encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let bytes =
        zip::write([("xl/Workbook.xml", &b"\xEF\xBB\xBF<workbook/>"[..]), ("u16.xml", &utf16[..])], WriteOptions::default()).unwrap();
    let sheet =
        Package::open(&bytes, &Limits::DEFAULT, &OpenOptions { case_fold: CaseFold::Unicode, utf16_text: true, ..OpenOptions::default() })
            .unwrap();
    assert_eq!(sheet.xml_part("xl/workbook.xml", &SHEET).unwrap().root().name(), "workbook");
    assert_eq!(sheet.xml_part("u16.xml", &ParseOptions::LENIENT).unwrap().root().name(), "é");
    let exact = open(&bytes, &Limits::DEFAULT);
    assert_eq!(exact.xml_part("xl/workbook.xml", &SHEET).unwrap_err(), Error::MissingPart("xl/workbook.xml".into()));
}

#[test]
fn d3_rels_and_content_types_from_the_parsed_part() {
    let rels_xml = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" Type=\"t\" Target=\"media/a.png\"/><Relationship Id=\"rId2\" Type=\"u\" Target=\"http://x\" TargetMode=\"External\"/></Relationships>";
    let ct_xml = "<Types><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/word/document.xml\" ContentType=\"x\"/></Types>";
    let bytes = zip::write(
        [("word/_rels/document.xml.rels", rels_xml.as_bytes()), ("[Content_Types].xml", ct_xml.as_bytes())],
        WriteOptions::default(),
    )
    .unwrap();
    let p = open(&bytes, &Limits::DEFAULT);
    let from_part = rels::from_document(&p.xml_part("word/_rels/document.xml.rels", &ParseOptions::LENIENT).unwrap());
    assert_eq!(from_part, rels::parse_rels(rels_xml, &ParseOptions::LENIENT).unwrap());
    assert_eq!(from_part.len(), 2);
    let ct = ContentTypes::from_document(&p.xml_part("[Content_Types].xml", &ParseOptions::LENIENT).unwrap());
    assert_eq!(ct, ContentTypes::parse(ct_xml, &ParseOptions::LENIENT).unwrap());
    assert_eq!(ct.content_type_of("word/document.xml"), Some("x"));
    for (rels_part, source) in [
        ("word/_rels/document.xml.rels", Some("word/document.xml")),
        ("_rels/.rels", Some("")),
        ("_rels/x.xml.rels", Some("x.xml")),
        ("ppt/slides/_rels/slide1.xml.rels", Some("ppt/slides/slide1.xml")),
        ("word/document.xml", None),
        ("word/_rels/a/b.rels", None),
        ("word/x_rels/a.rels", None),
        ("a.rels", None),
    ] {
        assert_eq!(rels::source_part_of(rels_part).as_deref(), source, "{rels_part}");
        if let Some(s) = source {
            assert_eq!(rels::rels_part_for(s), rels_part);
        }
    }
}

// ───────────────────────────── D5: raw copy only when verified ─────────────────────────────

/// Every entry of a saved archive is self-consistent: what it inflates to (with no declared size)
/// has the declared size and CRC-32. Returns (name, method, bytes).
fn well_formed(saved: &[u8]) -> Vec<(String, u16, Vec<u8>)> {
    zip::read(saved, u32::MAX)
        .unwrap()
        .into_iter()
        .map(|e| {
            let raw = e.compressed(saved);
            assert_eq!(e.data_start + e.compressed_size, e.data_start + raw.len() as u64, "{}: range", e.name);
            let out = if e.method == 0 { raw.to_vec() } else { flate::inflate_raw(raw, None, 1 << 30).unwrap() };
            assert_eq!((out.len() as u64, flate::crc32(&out)), (e.size, e.crc32), "{}", e.name);
            (e.name, e.method, out)
        })
        .collect()
}

#[test]
fn d5_verified_entries_are_copied_raw_and_lying_ones_written_again() {
    let body = b"abcdefghij".repeat(50);
    let comp = flate::deflate_raw(&body, 6);
    let crc = flate::crc32(&body);
    let entry = |name: &str, method: u16, crc32: u32, size: u32, data: &[u8]| RawZipEntry {
        crc32,
        ..if method == 0 { RawZipEntry::stored(name, data.to_vec()).size(size) } else { RawZipEntry::deflated(name, data.to_vec(), size) }
    };
    let archive = build_zip(
        &[
            entry("good.bin", 8, crc, 500, &comp),
            entry("good-stored.bin", 0, crc, 500, &body),
            entry("crc-lies.bin", 8, crc ^ 1, 500, &comp),
            entry("short.bin", 8, flate::crc32(&body[..7]), 7, &comp),
            entry("long.bin", 8, crc, 4000, &comp),
            entry("stored-crc-lies.bin", 0, 0, 500, &body),
            entry("stored-size-lies.bin", 0, crc, 3, &body),
            entry("beyond.bin", 0, flate::crc32(b"end"), 3, b"end").csize(5000),
        ],
        &RawZipOptions::default(),
    );
    let p = open(&archive, &Limits::DEFAULT);
    let read: Vec<(String, Vec<u8>)> = p.part_names().map(|n| (n.to_owned(), p.part(n).unwrap().unwrap().to_vec())).collect();
    for level in [6, 0] {
        let fresh = open(&archive, &Limits::DEFAULT);
        let saved = fresh.save(&SaveOptions { level, ..SaveOptions::default() }).unwrap();
        let back = well_formed(&saved);
        // the bytes are what the reader produced from the source
        assert_eq!(back.iter().map(|(n, _, b)| (n.clone(), b.clone())).collect::<Vec<_>>(), read);
        let source = zip::read(&archive, u32::MAX).unwrap();
        let out = zip::read(&saved, u32::MAX).unwrap();
        for (s, o) in source.iter().zip(&out) {
            let raw = s.name.starts_with("good");
            // a deflated entry written again at level 6 may have the source's compressed bytes:
            // a raw copy keeps the declared CRC-32 and size as well
            let copied = (s.crc32, s.size, s.compressed(&archive)) == (o.crc32, o.size, o.compressed(&saved));
            assert_eq!(copied, raw, "{} copied raw", s.name);
            // written again: deflated at the save level, or stored if it was stored
            let method = match (raw, s.method, level) {
                (true, m, _) | (false, m @ 0, _) => m,
                (false, _, 0) => 0,
                _ => 8,
            };
            assert_eq!(o.method, method, "{} at level {level}", s.name);
        }
    }
}

#[test]
fn d5_an_entry_that_cannot_be_read_fails_the_save() {
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add("ok.xml", b"<ok/>").unwrap();
    w.add_raw("bad.bin", RawEntry { method: 8, crc32: 0, size: 10, data: &[7] }).unwrap();
    let p = open(&w.finish(), &Limits::DEFAULT);
    assert_eq!(p.save(&SaveOptions::default()), Err(Error::Corrupt("invalid block type")));
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add_raw("empty.bin", RawEntry { method: 8, crc32: 0, size: 64, data: &[] }).unwrap();
    let p = open(&w.finish(), &Limits::DEFAULT);
    assert_eq!(p.save(&SaveOptions::default()), Err(Error::Corrupt("empty deflate stream with a declared size")));
    // an entry that is not intact is read, under the caps of a read
    let big = vec![b'a'; 4096];
    let mut w = ZipWriter::new(WriteOptions::default());
    let comp = flate::deflate_raw(&big, 6);
    w.add_raw("a.bin", RawEntry { method: 8, crc32: 0, size: 4096, data: &comp }).unwrap();
    let p = open(&w.finish(), &Limits { max_binary_part_bytes: 1000, ..Limits::DEFAULT });
    assert_eq!(p.save(&SaveOptions::default()), Err(Error::Limit { kind: LimitKind::PartBytes, max: 1000, part: Some("a.bin".into()) }));
}

#[test]
fn d5_intact_entries_save_under_any_limits_without_counting() {
    // the check holds no memory: untouched media far over the part and total caps are verified
    // and copied raw (a deck with a large video still saves on wasm32)
    let zeros = vec![0u8; 8 << 20];
    let bytes =
        zip::write([("media/a.bin", &zeros[..]), ("media/b.bin", &zeros[..]), ("doc.xml", b"<doc/>")], WriteOptions::default()).unwrap();
    drop(zeros);
    let small = Limits { max_binary_part_bytes: 1 << 20, max_total_bytes: 2 << 20, ..Limits::DEFAULT };
    let p = open(&bytes, &small);
    let saved = p.save(&SaveOptions::default()).unwrap();
    assert_eq!(p.produced_bytes(), 0, "verification is not counted");
    let source = zip::read(&bytes, u32::MAX).unwrap();
    let out = zip::read(&saved, u32::MAX).unwrap();
    for (s, o) in source.iter().zip(&out) {
        assert_eq!((s.crc32, s.size, s.compressed(&bytes)), (o.crc32, o.size, o.compressed(&saved)), "{} copied raw", s.name);
    }
    // reading them still hits the caps
    assert!(matches!(p.part("media/a.bin"), Err(Error::Limit { kind: LimitKind::PartBytes, .. })));
}

#[test]
fn d5_overlapping_entries_are_read_under_the_limits_never_verified() {
    // a crafted archive points many entries at one stream: each would be verified at full cost;
    // they are read instead, so the total bounds the work
    let zeros = vec![0u8; 1 << 20];
    let comp = flate::deflate_raw(&zeros, 9);
    let crc = flate::crc32(&zeros);
    let one = RawZipEntry { crc32: crc, ..RawZipEntry::deflated("first.bin", comp.clone(), 1 << 20) };
    let first = build_zip(std::slice::from_ref(&one), &RawZipOptions::default());
    // the data of "first.bin" starts after its local header (30 bytes + name)
    let offset = 0u32;
    let entries: Vec<RawZipEntry> = std::iter::once(one)
        .chain((0..200).map(|i| RawZipEntry {
            crc32: crc,
            offset: Some(offset),
            local_name: Some(b"first.bin".to_vec()),
            ..RawZipEntry::deflated(format!("copy{i:03}.bin"), Vec::new(), 1 << 20).csize(comp.len() as u32)
        }))
        .collect();
    let bytes = build_zip(&entries, &RawZipOptions::default());
    assert!(bytes.len() < first.len() + 200 * 200, "the copies add no data");
    let p = open(&bytes, &Limits { max_total_bytes: 16 << 20, ..Limits::DEFAULT });
    assert_eq!(p.part("copy000.bin").unwrap().map(<[u8]>::len), Some(1 << 20), "every entry reads the one stream");
    let p = open(&bytes, &Limits { max_total_bytes: 16 << 20, ..Limits::DEFAULT });
    assert_eq!(p.save(&SaveOptions::default()), Err(Error::Limit { kind: LimitKind::TotalBytes, max: 16 << 20, part: None }));
    // under a total that fits them, they are written again from the bytes read
    let p = open(&bytes, &Limits { max_total_bytes: 512 << 20, ..Limits::DEFAULT });
    let back = well_formed(&p.save(&SaveOptions::default()).unwrap());
    assert_eq!(back.len(), 201);
    assert!(back.iter().all(|(_, m, b)| *m == 8 && b.len() == 1 << 20));
    // an entry next to them, not overlapping, is still copied raw
    let lone = zip::write([("lone.bin", &zeros[..])], WriteOptions::default()).unwrap();
    let p = open(&lone, &Limits { max_total_bytes: 1, ..Limits::DEFAULT });
    assert!(p.save(&SaveOptions::default()).is_ok());
}

#[test]
fn d5_never_read_entries_are_verified_once_without_their_output() {
    let zeros = vec![0u8; 8 << 20];
    let bytes = zip::write([("media/zeros.bin", &zeros[..]), ("doc.xml", b"<doc/>")], WriteOptions::default()).unwrap();
    drop(zeros);
    let p = open(&bytes, &Limits::DEFAULT);
    let (first, u1) = measure(|| p.save(&SaveOptions::default()).unwrap());
    // the 8 MiB are verified through the 64 KiB window, never held
    assert!(u1.peak_above_start < 2 * first.len() as i64 + (256 << 10), "peak {}", u1.peak_above_start);
    assert_eq!(p.produced_bytes(), 0, "verification holds no memory and is not counted");
    // the result is kept: the second save allocates the output only (no window)
    let (second, u2) = measure(|| p.save(&SaveOptions::default()).unwrap());
    assert_eq!(first, second);
    assert!(u2.allocated + (64 << 10) <= u1.allocated, "{} vs {}", u2.allocated, u1.allocated);
    // `verify` keeps it as well, and a save after it does not inflate again
    let p = open(&bytes, &Limits::DEFAULT);
    p.verify().unwrap();
    let (_, u3) = measure(|| p.save(&SaveOptions::default()).unwrap());
    assert_eq!(u3.allocated, u2.allocated);
    // parts read before: their CRC-32 is computed from the kept output, nothing inflated again
    let p = open(&bytes, &Limits::DEFAULT);
    p.part("media/zeros.bin").unwrap();
    p.part("doc.xml").unwrap();
    let (_, u4) = measure(|| p.save(&SaveOptions::default()).unwrap());
    assert_eq!(u4.allocated, u2.allocated);
}

// ───────────────────────────── D6: one resolution path ─────────────────────────────

#[test]
fn d6_resolve_target_returns_the_name_the_package_opens() {
    let parts = [
        ("word/document.xml", &b"<d/>"[..]),
        ("word/media/Image1.PNG", b"png"),
        ("customXml/item1.xml", b"<i/>"),
        ("word/media/k.png", b"k"),
        ("a.xml", b"lower"),
        ("A.xml", b"upper"),
    ];
    let bytes = zip::write(parts, WriteOptions::default()).unwrap();
    for case_fold in [CaseFold::None, CaseFold::Unicode, CaseFold::Ascii] {
        let mut p = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions { case_fold, ..OpenOptions::default() }).unwrap();
        let r = |p: &Package, t: &str| p.resolve_target("word/document.xml", t).map(str::to_owned);
        assert_eq!(r(&p, "media/image1.png").as_deref(), Some("word/media/Image1.PNG"));
        assert_eq!(r(&p, "/WORD/media/IMAGE1.png").as_deref(), Some("word/media/Image1.PNG"));
        assert_eq!(r(&p, "./media/Image1.PNG#frag").as_deref(), Some("word/media/Image1.PNG"));
        assert_eq!(r(&p, "../customXml/item1.xml").as_deref(), Some("customXml/item1.xml"));
        assert_eq!(p.resolve_target("", "a.xml"), Some("a.xml"));
        assert_eq!(p.resolve_target("", "A.xml"), Some("A.xml"), "the exact name first");
        assert_eq!(p.resolve_target("", "a.XML"), Some("a.xml"), "else the first in order");
        for none in ["http://example.com/x", "media/missing.png", "../../x", "media\\Image1.PNG", "media/%zz", ""] {
            assert_eq!(r(&p, none), None, "{none:?}");
        }
        // security P3: U+212A KELVIN SIGN lowercases to `k` in Unicode; the OPC rule folds ASCII
        // only, so the target names no part, whatever the package's lookup policy
        assert_eq!(r(&p, "media/\u{212A}.png"), None);
        assert_eq!(p.find_name("word/media/\u{212A}.png").is_some(), case_fold == CaseFold::Unicode);
        // what the validator checks is what `part` opens
        let name = r(&p, "media/IMAGE1.png").unwrap();
        assert_eq!(p.part(&name).unwrap().unwrap(), b"png");
        // the index follows the part set
        p.set_part("word/media/new.png", b"new".to_vec());
        assert_eq!(r(&p, "media/NEW.png").as_deref(), Some("word/media/new.png"));
        p.remove_part("word/media/Image1.PNG");
        assert_eq!(r(&p, "media/image1.png"), None);
        p.retain(|n| n != "customXml/item1.xml");
        assert_eq!(r(&p, "../customxml/ITEM1.xml"), None);
    }
}

// ───────────────────────────── D8: safe relative paths ─────────────────────────────

/// Every name of the generated quirk corpus (`parity_support/quirks.rs`): what
/// `safe_relative_path` accepts is a plain relative path, and the hostile names are refused.
#[test]
fn d8_safe_relative_path_over_the_quirk_corpus() {
    let mut names: Vec<String> = Vec::new();
    for (_, bytes) in parity_support::quirks::quirk_corpus() {
        if let Ok(entries) = zip::read(&bytes, u32::MAX) {
            names.extend(entries.into_iter().map(|e| e.name));
        }
    }
    names.sort();
    names.dedup();
    let (mut safe, mut refused) = (0, Vec::new());
    for name in &names {
        match part_name::safe_relative_path(name) {
            Some(path) => {
                safe += 1;
                assert_eq!(&path, name);
                assert!(!path.starts_with('/') && !path.contains(['\\', ':', '\0']) && !path.chars().any(char::is_control), "{path:?}");
                for s in path.split('/') {
                    assert!(!s.is_empty() && s != "." && s != ".." && !s.ends_with(['.', ' ']), "{path:?}");
                }
            }
            None => refused.push(name.as_str()),
        }
    }
    println!("quirk corpus: {} distinct names, {safe} safe, {} refused: {refused:?}", names.len(), refused.len());
    assert!(safe > 100);
    for hostile in ["word\\media\\image2.png", "/abs.xml", "a/../b.xml", "./word/media/image1.png", "word/document.xml/"] {
        assert!(refused.contains(&hostile), "{hostile:?} is in the corpus and refused");
    }
}

#[test]
fn new_entry_points_do_not_leak() {
    let bytes = zip::write([("word/document.xml", &b"<a><b/></a>"[..]), ("media/x.png", b"png")], WriteOptions::default()).unwrap();
    let p = open(&bytes, &Limits::DEFAULT);
    // first calls fill the per-package caches (the part, the ASCII index, what the save verified)
    p.xml_part("word/document.xml", &ParseOptions::LENIENT).unwrap();
    p.resolve_target("word/document.xml", "../MEDIA/x.png").unwrap();
    p.save(&SaveOptions::default()).unwrap();
    uo_alloc::assert_no_leak(|| p.xml_part("word/document.xml", &ParseOptions::LENIENT).unwrap());
    uo_alloc::assert_no_leak(|| p.xml_part("missing", &ParseOptions::LENIENT).unwrap_err());
    uo_alloc::assert_no_leak(|| p.resolve_target("word/document.xml", "../MEDIA/x.png").map(str::len));
    uo_alloc::assert_no_leak(|| p.save(&SaveOptions::default()).unwrap());
    uo_alloc::assert_no_leak(|| part_name::safe_relative_path("a/b/c.xml"));
    uo_alloc::assert_no_leak(|| rels::source_part_of("word/_rels/document.xml.rels"));
    uo_alloc::assert_no_leak(|| flate::check_raw(&flate::deflate_raw(b"abc", 6), Some(3), 10).unwrap());
}
