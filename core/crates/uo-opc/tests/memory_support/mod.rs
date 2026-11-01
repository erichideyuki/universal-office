//! Shared scenarios of the stage-4 memory tests (M2 soak, M3 budgets): the real files of the
//! repository, the app-like round trip (open → read and parse every XML part → edit → save →
//! reopen) and the generated deck with large media.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

use uo_opc::cfb::CfbFile;
use uo_opc::content_types;
use uo_opc::package::{CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::rels::{self, Relationship};
use uo_opc::xml::raw::{self, Insert};
use uo_opc::xml::{ParseOptions, parse};
use uo_opc::zip::{RawEntry, WriteOptions, ZipWriter};
use uo_opc::{Limits, limits};

use crate::xml_support::{DOCX_POS, SHEET, SHEET_CHART, SLIDES};

/// The repository root (the worktree that holds `core/`).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn read_repo_file(rel: &str) -> Vec<u8> {
    let p = repo_root().join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The real files of the soak (representative of each product).
pub const SOAK_DOCX: &str = "write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx";
pub const SOAK_DOCX_FONT: &str = "write/packages/docx-layout/test/oracle/emwidth/ew-emb.docx";
pub const SOAK_XLSX: &str = "sheet/samples/showcase.xlsx";
pub const SOAK_PPTX: &str = "slides/samples/stress-media.pptx";
pub const SOAK_PPT: &str = "slides/samples/legacy/lo-reference.ppt";

/// Which product reads the package (its open options, XML preset and edit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docx,
    Xlsx,
    Pptx,
}

pub fn open_options(kind: Kind) -> OpenOptions {
    match kind {
        Kind::Docx => OpenOptions::default(),
        Kind::Xlsx => OpenOptions { case_fold: CaseFold::Unicode, ..OpenOptions::default() },
        Kind::Pptx => OpenOptions { skip_impossible_ratio: true, ..OpenOptions::default() },
    }
}

/// The preset each app parses a part with (docx keeps positions for the lossless save; sheet
/// charts go through the canonical chart table).
pub fn preset(kind: Kind, part: &str) -> ParseOptions<'static> {
    match kind {
        Kind::Docx => DOCX_POS,
        Kind::Xlsx if part.contains("/charts/") => SHEET_CHART,
        Kind::Xlsx => SHEET,
        Kind::Pptx => SLIDES,
    }
}

/// Reads every part and parses every XML-like one with the app's preset, through
/// `Package::xml_part` as the format crates do (gate 2, D3); returns (parts read, elements
/// parsed).
pub fn read_and_parse_all(pkg: &Package, kind: Kind) -> (usize, usize) {
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let mut elements = 0;
    for name in &names {
        if limits::is_xml_like(name) {
            let doc = pkg.xml_part(name, &preset(kind, name)).unwrap();
            elements += doc.element_count();
        } else {
            pkg.part(name).unwrap().unwrap();
        }
    }
    (names.len(), elements)
}

/// The main part each app edits.
pub fn main_part(kind: Kind) -> &'static str {
    match kind {
        Kind::Docx => "word/document.xml",
        Kind::Xlsx => "xl/worksheets/sheet1.xml",
        Kind::Pptx => "ppt/slides/slide1.xml",
    }
}

/// An edit like the apps make: the main part's text changes (an element inserted at the end of
/// its root), a relationship and a content-type override are added, a new part is written.
pub fn edit(pkg: &mut Package, kind: Kind, i: usize) {
    let main = main_part(kind);
    let text = pkg.part_text(main).unwrap().unwrap().into_owned();
    let doc = parse(text.as_str(), &preset(kind, main)).unwrap();
    let root = doc.root().name().to_owned();
    drop(doc);
    let child = format!("<uo:soak xmlns:uo=\"urn:uo\" n=\"{i}\"/>");
    let edited = raw::insert_child(&text, &root, &child, Insert::EndOfRoot).unwrap().unwrap();
    pkg.set_part_text(main, edited);
    let rels_name = rels::rels_part_for(main);
    let rels_text = pkg.part_text(&rels_name).unwrap().map(|t| t.into_owned());
    let list = rels_text.as_deref().map(|t| rels::parse_rels(t, &preset(kind, &rels_name)).unwrap()).unwrap_or_default();
    let id = rels::next_rel_id_after_max(&list);
    let rel =
        Relationship::new(&id, "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml", "../customXml/soak.xml");
    let rels_new = rels::add_rel(rels_text.as_deref(), &rel, Insert::BeforeLastCloseTrimmed).unwrap();
    pkg.set_part_text(rels_name, rels_new);
    let ct = pkg.part_text(content_types::CONTENT_TYPES_PART).unwrap().unwrap().into_owned();
    let ct = content_types::add_override(&ct, "customXml/soak.xml", "application/xml", Insert::BeforeLastCloseTrimmed).unwrap();
    pkg.set_part_text(content_types::CONTENT_TYPES_PART, ct);
    pkg.set_part_text("customXml/soak.xml", format!("<soak n=\"{i}\"/>"));
}

/// The soak step for a zip package: open → read and parse every part → edit → save → reopen →
/// read and parse every part of the saved package → drop. Returns the saved size.
pub fn zip_round_trip(bytes: &[u8], kind: Kind, i: usize) -> usize {
    let mut pkg = Package::open(bytes, &Limits::DEFAULT, &open_options(kind)).unwrap();
    read_and_parse_all(&pkg, kind);
    edit(&mut pkg, kind, i);
    let saved = pkg.save(&SaveOptions::default()).unwrap();
    drop(pkg);
    let n = saved.len();
    let again = Package::open_owned(saved, &Limits::DEFAULT, &open_options(kind)).unwrap();
    again.verify().unwrap();
    read_and_parse_all(&again, kind);
    assert!(again.contains("customXml/soak.xml"));
    n
}

/// The soak step for a legacy compound file: open → read every stream → drop.
pub fn cfb_read_all(bytes: &[u8]) -> usize {
    let f = CfbFile::open(bytes).unwrap();
    let mut total = 0;
    for e in f.entries() {
        if let Ok(Some(s)) = f.stream(&e.path) {
            total += s.len();
        }
    }
    total
}

// ───────────────────────────── the generated deck with large media ─────────────────────────────

/// A raw DEFLATE stream (fixed Huffman codes) that inflates to `len` zero bytes: one literal 0,
/// then <length 258, distance 1> copies and a last shorter copy. Built without compressing
/// anything, so a test can make a 64 MiB part in microseconds. About 13 bits per 258 bytes.
pub fn zeros_deflate(len: usize) -> Vec<u8> {
    struct Bits {
        out: Vec<u8>,
        acc: u64,
        n: u32,
    }
    impl Bits {
        fn put(&mut self, v: u32, n: u32) {
            self.acc |= u64::from(v) << self.n;
            self.n += n;
            while self.n >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.n -= 8;
            }
        }
        /// a Huffman code, most significant bit first
        fn code(&mut self, code: u32, n: u32) {
            let mut r = 0;
            for k in 0..n {
                r |= ((code >> k) & 1) << (n - 1 - k);
            }
            self.put(r, n);
        }
    }
    let mut b = Bits { out: Vec::new(), acc: 0, n: 0 };
    b.put(1, 1); // final
    b.put(1, 2); // fixed Huffman
    assert!(len > 0);
    // literal 0: code 0x30, 8 bits
    b.code(0x30, 8);
    let mut left = len - 1;
    while left > 0 {
        // a length of 3..=258; never leave 1 or 2 (shorter than a match)
        let mut l = left.min(258);
        if left - l > 0 && left - l < 3 {
            l = left - 3;
        }
        // lengths 3..=258 → symbols 257..=285 with extra bits (RFC 1951 3.2.5)
        let (sym, base, extra) = length_symbol(l as u32);
        // symbols 256..=279: 7-bit codes from 0; 280..=287: 8-bit codes from 0xC0
        if sym <= 279 {
            b.code(sym - 256, 7);
        } else {
            b.code(0xC0 + sym - 280, 8);
        }
        if extra > 0 {
            b.put(l as u32 - base, extra);
        }
        // distance 1: code 0, 5 bits, no extra
        b.code(0, 5);
        left -= l;
    }
    b.code(0, 7); // end of block (256)
    if b.n > 0 {
        b.out.push(b.acc as u8);
    }
    b.out
}

fn length_symbol(l: u32) -> (u32, u32, u32) {
    const BASE: [u32; 29] =
        [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
    const EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    let mut i = 0;
    while i + 1 < BASE.len() && BASE[i + 1] <= l {
        i += 1;
    }
    (257 + i as u32, BASE[i], EXTRA[i])
}

/// Pseudo-random bytes (xorshift): an incompressible "video".
pub fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.extend_from_slice(&x.to_le_bytes());
    }
    out.truncate(len);
    out
}

pub const DECK_DEFLATED_MEDIA: usize = 64 << 20;
pub const DECK_STORED_MEDIA: usize = 16 << 20;

/// `reference.pptx` plus two media parts: `ppt/media/zeros.bin` (64 MiB, deflated to a few
/// hundred KiB) and `ppt/media/video.mp4` (16 MiB of noise, stored). The original parts are
/// copied raw. The CRC of the zeros part is not checked by the reader (fflate does not either).
pub fn deck_with_large_media() -> Vec<u8> {
    let base = read_repo_file("slides/samples/reference.pptx");
    let pkg = Package::open(&base, &Limits::DEFAULT, &OpenOptions::default()).unwrap();
    let mut w = ZipWriter::new(WriteOptions::default());
    for name in pkg.part_names() {
        w.add(name, pkg.part(name).unwrap().unwrap()).unwrap();
    }
    let z = zeros_deflate(DECK_DEFLATED_MEDIA);
    // the real CRC-32: a save copies the entry raw only when it verifies (gate 2, D5)
    let crc = (0..DECK_DEFLATED_MEDIA >> 16).fold(0, |c, _| uo_opc::flate::crc32_update(c, &[0; 1 << 16]));
    w.add_raw("ppt/media/zeros.bin", RawEntry { method: 8, crc32: crc, size: DECK_DEFLATED_MEDIA as u64, data: &z }).unwrap();
    let video = noise(DECK_STORED_MEDIA, 7);
    w.add_raw("ppt/media/video.mp4", RawEntry { method: 0, crc32: uo_opc::flate::crc32(&video), size: video.len() as u64, data: &video })
        .unwrap();
    w.finish()
}
