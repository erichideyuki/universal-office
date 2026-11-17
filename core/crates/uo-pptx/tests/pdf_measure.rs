//! R-S5, the `uo-fonts`-backed `MeasureTable` filler against the B3 capture logs: the capture's measurer
//! (`js/capture-textlayout.mjs` `fakeWidth`) is "advances plus pair kerning, `units * px / upem`" over the open fonts,
//! which is what `uo_pptx::pdf::text_width` computes from `uo-fonts`; every line of every `*.measures.tsv.gz` log
//! (one distinct `measureText` key and the width the TS context answered) must come out bit-equal. `fill_measures`
//! (the two-phase filler) is exercised end to end by `pdf_slides.rs`, whose decks are laid out through it.
#![cfg(feature = "pdf")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_fonts::Face;
use uo_jsnum::parse::to_number;
use uo_pptx::pdf::{FaceSet, FontSource, text_width};

/// The text-layout logs (open-font measurer). The `drawops-*` logs hold the deterministic stub's answers
/// (`record-ctx.mjs`), not font widths: they are not comparable.
const LOGS: [&str; 4] = [
    "textlayout-samples.measures.tsv.gz",
    "textlayout-b1a.measures.tsv.gz",
    "textlayout-b3a.measures.tsv.gz",
    "textlayout-b3a-nokerning-noseg.measures.tsv.gz",
];

fn captures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures")
}

#[path = "support/open_fonts.rs"]
mod open_fonts_support;
use open_fonts_support::open_fonts;

/// The capture's own substitution (`openFont` of `capture-textlayout.mjs`): the family if bundled, else the small
/// `SUBST` table, else Liberation Sans.
struct CaptureRule(FaceSet);

impl FontSource for CaptureRule {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<Arc<Face>> {
        let f: String = family.chars().flat_map(char::to_lowercase).collect();
        let bundled = ["liberation sans", "liberation serif", "liberation mono", "carlito", "caladea"];
        let name = if bundled.contains(&f.as_str()) {
            f
        } else {
            match f.as_str() {
                "calibri" => "carlito",
                "cambria" => "caladea",
                "times new roman" | "times" => "liberation serif",
                "courier new" | "courier" => "liberation mono",
                _ => "liberation sans",
            }
            .to_owned()
        };
        self.0.face(&name, bold, italic)
    }
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    assert!(b.len() > 18 && b[0] == 0x1f && b[1] == 0x8b && b[3] == 0, "plain gzip from zlib.gzipSync");
    uo_opc::flate::inflate_raw(&b[10..b.len() - 8], None, 1 << 30).unwrap()
}

/// `JSON.parse` of a string literal (the log writes lone surrogates as U+FFFD).
fn json_string(s: &str) -> String {
    let inner = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap();
    let mut units: Vec<u16> = Vec::new();
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            let mut b = [0u16; 2];
            units.extend_from_slice(c.encode_utf16(&mut b));
            continue;
        }
        let u = match it.next().unwrap() {
            '"' => 0x22,
            '\\' => 0x5c,
            '/' => 0x2f,
            'b' => 8,
            'f' => 12,
            'n' => 10,
            'r' => 13,
            't' => 9,
            'u' => u16::from_str_radix(&it.by_ref().take(4).collect::<String>(), 16).unwrap(),
            e => panic!("bad escape {e}"),
        };
        units.push(u);
    }
    String::from_utf16_lossy(&units)
}

#[test]
fn filler_equals_the_capture_logs() {
    let source = CaptureRule(open_fonts());
    let mut total = 0u64;
    let mut kerned = 0u64;
    for log in LOGS {
        let path = captures().join(log);
        let Ok(raw) = std::fs::read(&path) else { panic!("{}: missing capture log", path.display()) };
        let text = String::from_utf8(gunzip(&raw)).unwrap();
        let (mut lines, mut bad) = (0u64, Vec::new());
        for line in text.lines().filter(|l| !l.is_empty()) {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 7, "{line}");
            let (family, bold, italic) = (f[0], f[1] == "1", f[2] == "1");
            let px = to_number(f[3]);
            let kerning = f[4] != "none";
            let s = json_string(f[5]);
            let want = to_number(f[6]);
            let face = source.face(family, bold, italic).unwrap();
            let got = text_width(&face, px, kerning, 0.0, &s);
            lines += 1;
            if kerning && s.chars().count() > 1 {
                kerned += 1;
            }
            if got.to_bits() != want.to_bits() && bad.len() < 5 {
                bad.push(format!("{family} {bold} {italic} {px}px {:?} {kerning}: got {got} want {want}", s));
            } else if got.to_bits() != want.to_bits() {
                bad.push(String::new());
            }
        }
        eprintln!("{log}: {lines} measurements, {} mismatches", bad.len());
        assert!(bad.is_empty(), "{log}: {} mismatches, first: {:?}", bad.len(), bad.iter().take(5).collect::<Vec<_>>());
        total += lines;
    }
    eprintln!("total {total} measurements equal ({kerned} kerned strings)");
    assert!(total > 100_000, "the logs are not the expected size: {total}");
}

/// The catalog substitution (`FaceSet`) answers the same face as the capture's rule for every family of the logs:
/// the filler of a host that uses `FaceSet` measures what the capture measured.
#[test]
fn face_set_agrees_with_the_capture_rule_on_the_logged_families() {
    let rule = CaptureRule(open_fonts());
    let set = open_fonts();
    let mut families = std::collections::BTreeSet::new();
    for log in LOGS {
        let text = String::from_utf8(gunzip(&std::fs::read(captures().join(log)).unwrap())).unwrap();
        for line in text.lines().filter(|l| !l.is_empty()) {
            let f: Vec<&str> = line.split('\t').collect();
            families.insert(f[0].to_owned());
        }
    }
    let mut differ = Vec::new();
    for fam in &families {
        for (b, i) in [(false, false), (true, false), (false, true), (true, true)] {
            let (a, c) = (rule.face(fam, b, i).unwrap(), set.face(fam, b, i).unwrap());
            if a.bytes() != c.bytes() {
                differ.push(format!("{fam} {b} {i}"));
            }
        }
    }
    eprintln!("{} families in the logs, {} (family, variant) pairs differ between the rules", families.len(), differ.len());
    assert!(differ.is_empty(), "{differ:?}");
}
