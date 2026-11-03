//! Builds the seed corpora of the fuzz targets (`corpus/<target>/`, not versioned) from small
//! documents of the repository: the Write oracle (.docx), the Sheet and Slides samples
//! (.xlsx/.pptx), the legacy .doc/.xls/.ppt fixtures, their parts, and a few generated
//! container quirks.
//!
//!   cargo run --release --no-default-features --features fallback --bin seed
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use uo_opc::flate;
use uo_opc::package::{OpenOptions, Package};
use uo_opc::rels;
use uo_opc::zip::{RawEntry, WriteOptions, ZipWriter};
use uo_opc::{Limits, zip};

/// Largest XML part kept as a seed (larger parts are cut, the fuzzer's inputs stay small).
const MAX_XML: usize = 24 * 1024;

struct Corpus {
    root: PathBuf,
    seen: HashSet<(String, u64)>,
    count: usize,
}

fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

impl Corpus {
    fn add(&mut self, target: &str, data: &[u8]) {
        let h = fnv(data);
        if !self.seen.insert((target.to_owned(), h)) {
            return;
        }
        let dir = self.root.join(target);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{h:016x}")), data).unwrap();
        self.count += 1;
    }

    fn add_sel(&mut self, target: &str, sel: u8, data: &[u8]) {
        let mut v = Vec::with_capacity(data.len() + 1);
        v.push(sel);
        v.extend_from_slice(data);
        self.add(target, &v);
    }
}

fn files(dir: &Path, ext: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, ext, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| ext.contains(&x.to_ascii_lowercase().as_str())) {
            out.push(p);
        }
    }
}

fn cut(s: &[u8], n: usize) -> &[u8] {
    &s[..s.len().min(n)]
}

fn main() {
    let fuzz = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = fuzz.join("../..");
    let mut c = Corpus { root: fuzz.join("corpus"), seen: HashSet::new(), count: 0 };

    // ── packages: the smallest oracle .docx (one in four, for variety) and the small samples ──
    let mut docx = Vec::new();
    files(&repo.join("write/packages/docx-layout/test/oracle"), &["docx"], &mut docx);
    docx.sort_by_key(|p| (fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX), p.clone()));
    let mut packages: Vec<PathBuf> = docx.into_iter().take(240).step_by(4).collect();
    for dir in ["sheet/samples", "slides/samples"] {
        let mut v = Vec::new();
        files(&repo.join(dir), &["xlsx", "pptx"], &mut v);
        v.retain(|p| fs::metadata(p).is_ok_and(|m| m.len() <= 120 * 1024));
        packages.extend(v);
    }
    for (k, path) in packages.iter().enumerate() {
        let bytes = fs::read(path).unwrap();
        let sel = k as u8;
        c.add_sel("zip_open", sel, &bytes);
        c.add_sel("zip_roundtrip", sel, &bytes);
        let Ok(pkg) = Package::open(&bytes, &Limits::DEFAULT, &OpenOptions::default()) else { continue };
        let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
        c.add_sel("caseless", sel, names.join("\0").as_bytes());
        // compressed streams of the entries, for the inflate modes
        for e in zip::read(&bytes, 65_535).unwrap().iter().filter(|e| e.method == 8).take(6) {
            let raw = e.compressed(&bytes);
            if raw.len() > 16 * 1024 {
                continue;
            }
            c.add_sel("inflate", 0, raw);
            let mut declared = (e.size as u32).to_le_bytes().to_vec();
            declared.extend_from_slice(raw);
            c.add_sel("inflate", 1, &declared);
            c.add_sel("inflate", 3, raw);
            if let Ok(plain) = flate::inflate_raw(raw, Some(e.size), 1 << 20) {
                c.add_sel("inflate", 2, &flate::zlib(&plain, 6));
                let mut capped = vec![200];
                capped.extend_from_slice(&flate::zlib(&plain, 1));
                c.add_sel("inflate", 4, &capped);
                let mut round = vec![9];
                round.extend_from_slice(cut(&plain, 4096));
                c.add_sel("inflate", 5, &round);
            }
        }
        for name in &names {
            let Ok(Some(part)) = pkg.part(name) else { continue };
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".xml") || lower.ends_with(".rels")) {
                continue;
            }
            let xml = cut(part, MAX_XML);
            // one preset per file and part kind, rotating, so every preset sees every kind
            let sel = (k + name.len()) as u8 % 32;
            c.add_sel("xml_lenient", sel, xml);
            c.add_sel("xml_strict", sel % 8, xml);
            if part.len() <= 4096 {
                c.add("part_text", part);
                let mut le = vec![0xFF, 0xFE];
                le.extend(String::from_utf8_lossy(part).encode_utf16().flat_map(u16::to_le_bytes));
                c.add("part_text", &le);
            }
            if lower.ends_with(".rels") {
                c.add_sel("rels", sel % 4, part);
                let source = name.replace("_rels/", "").trim_end_matches(".rels").to_owned();
                if let Ok(list) =
                    rels::parse_rels(&String::from_utf8_lossy(part), &uo_opc::xml::ParseOptions::LENIENT, &uo_opc::Limits::DEFAULT)
                {
                    for r in list.iter().take(8) {
                        c.add("resolver", format!("{source}\0{}", r.target).as_bytes());
                    }
                }
            }
            if lower == "[content_types].xml" {
                let mut v = part.to_vec();
                v.push(0);
                v.extend_from_slice(b"word/media/image1.png");
                c.add_sel("content_types", sel % 4, &v);
            }
            if lower.ends_with("document.xml") || lower.ends_with("styles.xml") {
                // property elements for raw surgery: <w:rPr>, <w:pPr>, <w:sectPr>
                let s = String::from_utf8_lossy(part);
                for tag in ["w:rPr", "w:pPr", "w:sectPr", "w:tblPr"] {
                    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
                    if let Some(a) = s.find(&open)
                        && let Some(b) = s[a..].find(&close)
                    {
                        let el = &s[a..a + b + close.len()];
                        c.add_sel("raw_surgery", sel % 64, format!("{el}\0w:b\0w:val\0x\"&<").as_bytes());
                    }
                }
                // text runs for the escape round trip
                for t in s.split("<w:t").skip(1).take(4) {
                    if let Some(body) = t.split_once('>').and_then(|(_, r)| r.split_once("</w:t>")).map(|(b, _)| b) {
                        c.add("escape_roundtrip", body.as_bytes());
                    }
                }
            }
        }
    }

    // ── legacy binaries (CFB): whole when small, and their first 64 KB ──
    let mut legacy = Vec::new();
    for dir in ["write/packages/docx-core/test/fixtures", "sheet", "slides/samples"] {
        files(&repo.join(dir), &["doc", "xls", "ppt"], &mut legacy);
    }
    legacy.retain(|p| !p.components().any(|x| x.as_os_str() == "node_modules"));
    legacy.sort();
    for p in legacy.iter().take(40) {
        let bytes = fs::read(p).unwrap();
        if bytes.len() <= 256 * 1024 {
            c.add("cfb", &bytes);
        }
        c.add("cfb", cut(&bytes, 64 * 1024));
        c.add("cfb", cut(&bytes, 4096));
    }

    // ── generated container quirks ──
    type Entries<'a> = Vec<(&'a str, &'a [u8])>;
    let quirks: Vec<(Entries<'_>, u8)> = vec![
        (vec![("a.xml", b"<a/>"), ("a.xml", b"<b/>"), ("2", b"x"), ("10", b"y"), ("__proto__", b"z")], 6),
        (vec![("mimetype", b"application/vnd.oasis.opendocument.text"), ("content.xml", b"<office:document/>")], 0),
        (vec![("caf\u{e9}/\u{feff}x.xml", b"<x/>"), ("../evil.xml", b"<e/>"), ("/abs\\back.xml", b"<y/>"), ("", b"")], 6),
    ];
    for (k, (entries, level)) in quirks.iter().enumerate() {
        let mut w = ZipWriter::new(WriteOptions { level: *level, ..WriteOptions::default() });
        for (n, d) in entries {
            w.add(n, d).unwrap();
        }
        let z = w.finish();
        c.add_sel("zip_open", k as u8, &z);
        c.add_sel("zip_roundtrip", k as u8 | 48, &z);
        let mut rec = Vec::new();
        for (n, d) in entries {
            rec.push(1);
            rec.extend_from_slice(n.as_bytes());
            rec.push(0);
            rec.extend_from_slice(d);
            rec.push(0);
        }
        c.add_sel("zip_write", *level, &rec);
    }
    // an empty deflate stream with a declared size (S1) and a declared size smaller than the data
    let mut w = ZipWriter::new(WriteOptions::default());
    w.add_raw("word/document.xml", RawEntry { method: 8, crc32: 0, size: 1000, data: &[] }).unwrap();
    let d = flate::deflate_raw(b"<w:document>hello hello hello</w:document>", 6);
    w.add_raw("word/styles.xml", RawEntry { method: 8, crc32: 0, size: 5, data: &d }).unwrap();
    let z = w.finish();
    c.add_sel("zip_open", 0, &z);
    c.add_sel("zip_roundtrip", 16, &z);

    // ── hand-written seeds of the small targets ──
    for s in ["", "a & b < c > d \" e ' f", "\r\n\t\u{0}\u{1f}\u{fffe}\u{ffff}]]>", "caf\u{e9} \u{1F600} &amp; &#x41;"] {
        c.add("escape_roundtrip", s.as_bytes());
    }
    for (src, t) in [
        ("word/document.xml", "media/image1.png"),
        ("word/document.xml", "../customXml/item1.xml"),
        ("", "/word/styles.xml"),
        ("word/document.xml", "media/caf%C3%A9.png#frag"),
        ("word/document.xml", "%2e%2e/%2e%2e/x"),
        ("ppt/slides/slide1.xml", "../media/%FF.png"),
    ] {
        c.add("resolver", format!("{src}\0{t}").as_bytes());
    }
    c.add_sel("raw_surgery", 0, b"<w:rPr><w:b/><w:i w:val=\"0\"/><w:b/></w:rPr>\0w:b\0w:val\0x".as_slice());
    c.add_sel("caseless", 1, "Word/Document.xml\0word/document.xml\0\u{212a}.xml\0k.xml\0\u{130}.xml".as_bytes());
    c.add_sel("rels", 0, br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#);
    c.add_sel("content_types", 0, b"<Types><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>\0a.png");
    c.add_sel(
        "xml_strict",
        0,
        br#"<?xml version="1.0"?><!-- c --><a xmlns="urn:a" xmlns:p="urn:p" p:x="1"><?pi d?><![CDATA[<x>]]>&#x1F600;</a>"#,
    );
    c.add_sel(
        "xml_lenient",
        1,
        b"<x:worksheet xmlns:x=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><x:c r=\"A1\">\r\n</x:c></x:worksheet>",
    );
    jsnum_seeds(&mut c, &repo.join("core/crates/uo-jsnum/tests/captures"));
    // uo-fonts: generated fonts, their tables and the open fonts of the repository, cut small
    for (target, data) in uo_opc_fuzz::fonts_seeds(&repo) {
        c.add(target, &data);
    }
    // uo-drawingml: every preset, a program per guide operator, every modifier and pattern, masks
    // with every operation, every light rig and bevel preset (valid inputs of each layout)
    for (target, data) in uo_opc_fuzz::dml_seeds() {
        c.add(target, &data);
    }
    // the schema-driven codec: valid messages of every schema (raw and framed), and seeds of the structure-aware round trip
    for (target, data) in uo_opc_fuzz::codec_seeds() {
        c.add(target, &data);
    }
    // uo-image: the small images of the generated pack, in the all-decoders mode
    for (target, data) in uo_opc_fuzz::image_seeds(&repo) {
        c.add(target, &data);
    }
    for target in uo_opc_fuzz::TARGETS.iter().map(|(n, _)| *n) {
        c.add(target, b"");
    }
    println!("{} seeds in {}", c.count, c.root.display());
}

/// The JSON string of a capture facet (`s="…"`), decoded; a lone surrogate escape becomes U+FFFD.
fn json_unescape(s: &str) -> String {
    let inner = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s);
    let mut out = String::new();
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let code: String = it.by_ref().take(4).collect();
                let unit = u32::from_str_radix(&code, 16).unwrap_or(0xFFFD);
                out.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Seeds of `jsnum_parse` (the 199 strings of the parity corpus, whose `Number`/`parseInt`
/// results the capture holds) and `jsnum_math` (the special values and the fdlibm high-word
/// thresholds of `inputs.txt`, in triples).
fn jsnum_seeds(c: &mut Corpus, captures: &Path) {
    let capture = fs::read_to_string(captures.join("jsnum.capture")).unwrap();
    for line in capture.lines().filter(|l| l.starts_with("corpus/")) {
        let Some(s) = line.split('\t').find_map(|f| f.strip_prefix("s=")) else { continue };
        c.add_sel("jsnum_parse", 0, json_unescape(s).as_bytes());
    }
    for s in ["0x1fffffffffffff8", "0b1e1", "-0", "  +.5e-0010\u{feff}", "1e-400", "0o777", "1_0", "Infinityx"] {
        c.add_sel("jsnum_parse", 0, s.as_bytes());
    }
    let inputs = fs::read_to_string(captures.join("inputs.txt")).unwrap();
    let mut values: Vec<u64> = Vec::new();
    for line in inputs.lines() {
        if let Some(hex) = line.strip_prefix("special ") {
            values.push(u64::from_str_radix(hex, 16).unwrap());
        } else if let Some(hex) = line.strip_prefix("threshold ") {
            let hi = u64::from_str_radix(hex, 16).unwrap();
            values.extend([hi << 32, (hi.wrapping_sub(1) << 32) | 0xFFFF_FFFF, (hi | 0x8000_0000) << 32]);
        }
    }
    for (k, triple) in values.chunks(3).enumerate() {
        let bytes: Vec<u8> = triple.iter().flat_map(|v| v.to_le_bytes()).collect();
        c.add_sel("jsnum_math", k as u8, &bytes);
    }
}
