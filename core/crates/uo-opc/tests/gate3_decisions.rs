//! The gate-3 decisions of the module record (`core/docs/modules/uo-opc-container.md`, "Gate 3 —
//! decision"), one group of tests per decision. R1 and R3 (the D6 path for the relationship
//! targets of the three apps) are pinned by the replay (`opc_parity.rs`, accepted by name with
//! the counts of the stage-5 evidence) and by `gate2_decisions::d6_resolve_target_*`.
#![allow(clippy::unwrap_used, clippy::panic)] // helpers outside #[test] functions

mod xml_support;

use uo_opc::package::{CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::xml::{ParseOptions, parse};
use uo_opc::zip::{self, DEFAULT_LEVEL, WriteOptions, ZipWriter};
use uo_opc::{Error, LimitKind, Limits, flate};

use xml_support::{DOCX_POS, SHEET, SHEET_CHART, SLIDES};

// ───────────────────────────── R2: ASCII case folding ─────────────────────────────

/// Sheet's fallback folds ASCII only (the OPC rule). 0.9 used JS `toLowerCase`, where 'É'
/// finds 'é' and U+212A KELVIN SIGN finds `k` (security P3); no real file and no quirk of the
/// parity corpus has a lookup that differs.
#[test]
fn r2_sheet_folds_ascii_only() {
    let bytes = zip::write([("É.xml", &b"x"[..]), ("A.xml", b"y"), ("xl/media/k.png", b"k")], WriteOptions::default()).unwrap();
    let sheet = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }).unwrap();
    assert_eq!(sheet.find_name("é.xml"), None);
    assert_eq!(sheet.find_name("xl/media/\u{212A}.png"), None);
    assert_eq!(sheet.find_name("a.XML"), Some("A.xml"));
    assert_eq!(sheet.find_name("XL/media/K.PNG"), Some("xl/media/k.png"));
    // the same index answers `resolve_target`, with the same folding
    assert_eq!(sheet.resolve_target("xl/workbook.xml", "media/K.png"), Some("xl/media/k.png"));
    assert_eq!(sheet.resolve_target("xl/workbook.xml", "media/\u{212A}.png"), None);
}

// ───────────────────────────── R4: the save level ─────────────────────────────

/// Text that deflates differently at levels 4 and 6 (lazy matching).
fn text(len: usize) -> Vec<u8> {
    let words = ["<c r=\"A", "\"><v>", "</v></c>", "<row r=\"", "\" spans=\"1:9\">", "</row>"];
    let mut x: u32 = 0x9E37_79B9;
    let mut out = Vec::with_capacity(len + 16);
    while out.len() < len {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        out.extend_from_slice(words[(x % 6) as usize].as_bytes());
        out.extend_from_slice((x % 997).to_string().as_bytes());
    }
    out
}

/// The deflated bytes of every entry of a zip, in order.
fn streams(saved: &[u8]) -> Vec<Vec<u8>> {
    zip::read(saved, u32::MAX).unwrap().iter().map(|e| e.compressed(saved).to_vec()).collect()
}

/// The default save level is miniz_oxide level 4; an explicit `level` is that miniz_oxide level,
/// with no remapping, in `Package::save`, `ZipWriter` and `zip::write` alike.
#[test]
fn r4_default_level_is_4_and_a_level_is_the_miniz_level() {
    assert_eq!(DEFAULT_LEVEL, 4);
    assert_eq!(WriteOptions::default().level, 4);
    assert_eq!(SaveOptions::default().level, 4);
    let data = text(200_000);
    let (l4, l6) = (flate::deflate_raw(&data, 4), flate::deflate_raw(&data, 6));
    assert_ne!(l4, l6, "the sample must tell the levels apart");
    let pkg = Package::from_parts([("xl/worksheets/sheet1.xml".to_string(), data.clone())], &Limits::DEFAULT, &OpenOptions::default());
    assert_eq!(streams(&pkg.save(&SaveOptions::default()).unwrap()), std::slice::from_ref(&l4));
    for level in 1..=9 {
        let expected = flate::deflate_raw(&data, level);
        assert_eq!(
            streams(&pkg.save(&SaveOptions { level, ..SaveOptions::default() }).unwrap()),
            std::slice::from_ref(&expected),
            "save, level {level}"
        );
        let mut w = ZipWriter::new(WriteOptions { level, ..WriteOptions::default() });
        w.add("a.xml", &data).unwrap();
        assert_eq!(streams(&w.finish()), std::slice::from_ref(&expected), "ZipWriter, level {level}");
        let z = zip::write([("a.xml", &data[..])], WriteOptions { level, ..WriteOptions::default() }).unwrap();
        assert_eq!(streams(&z), [expected], "zip::write, level {level}");
    }
    let z = zip::write([("a.xml", &data[..])], WriteOptions::default()).unwrap();
    assert_eq!(streams(&z), [l4]);
    // the decompressed bytes do not depend on the level
    let back = Package::open(&pkg.save(&SaveOptions::default()).unwrap(), &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    assert_eq!(back.part("xl/worksheets/sheet1.xml").unwrap().unwrap(), &data[..]);
}

// ───────────────────────────── R5: depth 1,000 for Sheet and Slides ─────────────────────────────

fn nested(depth: usize) -> String {
    format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth))
}

/// The nesting limit of 1,000 levels (`Limits::max_xml_depth`, Write's own limit in 0.9) applies
/// to the parts of Sheet and Slides through `Package::xml_part`, whose presets have no limit of
/// their own: 1,000 levels parse, 1,001 are `Limit { XmlDepth }` naming the part. The deepest
/// real part is 25 levels; the 0.9 parsers overflowed the JS stack past about 8,000.
#[test]
fn r5_depth_limit_of_1000_for_sheet_and_slides() {
    assert_eq!(Limits::NATIVE.max_xml_depth, 1_000);
    assert_eq!(Limits::WASM32.max_xml_depth, 1_000);
    assert_eq!(Limits::DEFAULT.max_xml_depth, 1_000);
    let (ok, deep) = (nested(1_000), nested(1_001));
    let bytes = zip::write([("ok.xml", ok.as_bytes()), ("deep.xml", deep.as_bytes())], WriteOptions::default()).unwrap();
    let presets: [(&str, ParseOptions<'static>); 4] =
        [("sheet", SHEET), ("sheet-chart", SHEET_CHART), ("slides", SLIDES), ("docx-pos", DOCX_POS)];
    for (name, preset) in presets {
        let options = if name.starts_with("sheet") {
            OpenOptions { case_fold: CaseFold::Ascii, ..OpenOptions::default() }
        } else {
            OpenOptions::default()
        };
        let p = Package::open(&bytes, &Limits::DEFAULT, &options).unwrap();
        assert_eq!(p.xml_part("ok.xml", &preset).unwrap().element_count(), 1_000, "{name}");
        assert_eq!(
            p.xml_part("deep.xml", &preset).unwrap_err(),
            Error::Limit { kind: LimitKind::XmlDepth, max: 1_000, part: Some("deep.xml".into()) },
            "{name}"
        );
        if name != "docx-pos" {
            // the preset alone has no limit: the package's applies
            assert!(preset.max_depth.is_none(), "{name}");
            assert_eq!(parse(deep.as_str(), &preset).unwrap().element_count(), 1_001, "{name}");
        }
    }
}
