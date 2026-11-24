//! Fuzz targets of `uo-opc` (stage 4 security, record `core/docs/modules/uo-opc-container.md`)
//! and of `uo-jsnum` (`jsnum_parse`, `jsnum_math` in `src/jsnum.rs`, record
//! `core/docs/modules/uo-jsnum.md`), of `uo-fonts` (the `fonts_*` targets in `src/fonts.rs`,
//! record `core/docs/modules/uo-fonts.md`) and of `uo-drawingml` (the `dml_*` targets in
//! `src/drawingml.rs`, and `odraw_records` in `src/odraw.rs`, record `core/docs/modules/uo-drawingml.md`) and of the schema-driven codec (`codec_decode`,
//! `codec_roundtrip` in `src/codec.rs`, record `core/docs/modules/uo-render.md`, "Milestone 1 — frontier C").
//! The docx reader (`read_docx` in `src/docx.rs`) is recorded in `core/docs/modules/uo-docx.md`,
//! "R5 follow-ups"; the D5 targets (`odt_read`, `export_docx`, `compare_docx` in `src/d5.rs`) in
//! `core/docs/modules/uo-docx-d5/D5a.md` and `D5b.md`, "Security (continuous)".
//!
//! Every target is a plain `fn(&[u8])`: libFuzzer runs them through `fuzz_targets/*.rs`
//! (`cargo +nightly fuzz run <name>`), the stable driver `src/bin/fallback.rs` through
//! [`TARGETS`]. A target fails by panicking: the library must never panic on any input, and
//! the `assert!`s below are invariants (limits respected, round trips exact, differential
//! results equal) whose failure is a finding too.
//!
//! Inputs start with one or more selector bytes (preset, mode, limits); the rest is the
//! document data. Limits are small so that each execution stays fast; the limit logic is the
//! same at any size.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;

use uo_opc::Limits;
use uo_opc::cfb::CfbFile;
use uo_opc::content_types::{self, ContentTypes};
use uo_opc::error::{Error, LimitKind};
use uo_opc::flate::{self, Framing};
use uo_opc::package::{self, CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::part_name;
use uo_opc::rels::{self, Relationship};
use uo_opc::xml::raw::{self, Insert, Kid};
use uo_opc::xml::write::{self, Escape, XmlWriter};
use uo_opc::xml::{CanonicalPrefixes, CharRefPolicy, ContentPi, Document, Node, ParseOptions, parse};
use uo_opc::zip::{self, WriteOptions, ZipWriter};

mod jsnum;
pub use jsnum::{jsnum_math, jsnum_parse};

mod numfmt;
pub use numfmt::{numfmt_format, numfmt_seeds};

mod xlsx;
pub use xlsx::{xlsx_seeds, xlsx_write};

mod calc;
pub use calc::{calc_seeds, store_ops};

mod codec;
pub use codec::{CODEC_LIMITS, codec_decode, codec_roundtrip, codec_seeds};

mod pptx;
pub use pptx::{dml_shapes, pptx_open, pptx_seeds};

mod text_layout;
pub use text_layout::{text_layout, text_layout_seeds};

mod slides;
mod slides_edit;
mod slides_edit_wire;
mod slides_emit;
mod slides_wire;
pub use slides::{slides_decode, slides_roundtrip, slides_seeds};
pub use slides_edit::{slides_edit, slides_edit_seeds};
pub use slides_edit_wire::{slides_edit_wire, slides_edit_wire_seeds};
pub use slides_emit::{slides_emit, slides_emit_seeds};

mod charts;
pub use charts::{chart_parse, chart_parse_seeds};
mod charts_render;
pub use charts_render::{chart_emit, chart_emit_seeds, chart_scale, chart_scale_seeds};

mod timing;
pub use timing::{anim_formula, slide_timing, timing_seeds};

mod metafile;
pub use metafile::{emf_play, eot_read, metafile_seeds, pict_play, tiff_slides};

mod ppt;
pub use ppt::{ppt_convert, ppt_seeds};

mod drawingml;
pub use drawingml::{
    dml_adjust, dml_bevel, dml_blip, dml_color, dml_custom_guides, dml_effects, dml_flatten, dml_light, dml_patterns, dml_preset,
    dml_preset_path, dml_raster, dml_seeds,
};

mod odraw;
pub use odraw::{ODRAW_LIMITS, odraw_records, odraw_seeds};

mod doc;
pub use doc::{DOC_LIMITS, doc_convert, doc_seeds};

mod docx;
pub use docx::{DOCX_PACKAGE_LIMITS, DOCX_READ_LIMITS, docx_seeds, read_docx};

mod d5;
pub use d5::{compare_docx, d5_seeds, export_docx, odt_read};

mod d3;
pub use d3::{api_open_sanitize_save, api_template_apply, d3_seeds};

mod pipeline;
pub use pipeline::{ffi_docx_to_pdf, pipeline_seeds};

mod image;
pub use image::{image_decode, image_seeds};

mod fonts;
pub use fonts::{
    fonts_catalog, fonts_cmap, fonts_colr_vdmx, fonts_embed, fonts_eot, fonts_gpos, fonts_gsub, fonts_hyphen_build, fonts_hyphen_words,
    fonts_info, fonts_odttf, fonts_outline, fonts_parse, fonts_program, fonts_seeds, fonts_sfnt_dir, fonts_subset, fonts_ttc,
};

/// A fuzz target.
pub type Target = fn(&[u8]);

/// Every target, by name (the fallback driver and the seed builder use this list).
pub const TARGETS: &[(&str, Target)] = &[
    ("zip_open", zip_open),
    ("zip_roundtrip", zip_roundtrip),
    ("zip_write", zip_write),
    ("inflate", inflate),
    ("cfb", cfb),
    ("xml_lenient", xml_lenient),
    ("xml_strict", xml_strict),
    ("escape_roundtrip", escape_roundtrip),
    ("raw_surgery", raw_surgery),
    ("rels", rels),
    ("content_types", content_types),
    ("caseless", caseless),
    ("part_text", part_text),
    ("resolver", resolver),
    ("jsnum_parse", jsnum_parse),
    ("jsnum_math", jsnum_math),
    ("numfmt_format", numfmt_format),
    ("xlsx_write", xlsx_write),
    ("store_ops", store_ops),
    ("fonts_parse", fonts_parse),
    ("fonts_sfnt_dir", fonts_sfnt_dir),
    ("fonts_cmap", fonts_cmap),
    ("fonts_gpos", fonts_gpos),
    ("fonts_gsub", fonts_gsub),
    ("fonts_outline", fonts_outline),
    ("fonts_colr_vdmx", fonts_colr_vdmx),
    ("fonts_program", fonts_program),
    ("fonts_info", fonts_info),
    ("fonts_ttc", fonts_ttc),
    ("fonts_subset", fonts_subset),
    ("fonts_eot", fonts_eot),
    ("fonts_odttf", fonts_odttf),
    ("fonts_catalog", fonts_catalog),
    ("fonts_hyphen_build", fonts_hyphen_build),
    ("fonts_hyphen_words", fonts_hyphen_words),
    ("fonts_embed", fonts_embed),
    ("dml_preset", dml_preset),
    ("dml_preset_path", dml_preset_path),
    ("dml_custom_guides", dml_custom_guides),
    ("dml_color", dml_color),
    ("dml_patterns", dml_patterns),
    ("dml_adjust", dml_adjust),
    ("dml_blip", dml_blip),
    ("dml_flatten", dml_flatten),
    ("dml_raster", dml_raster),
    ("dml_effects", dml_effects),
    ("dml_light", dml_light),
    ("dml_bevel", dml_bevel),
    ("odraw_records", odraw_records),
    ("doc_convert", doc_convert),
    ("read_docx", read_docx),
    ("ffi_docx_to_pdf", ffi_docx_to_pdf),
    ("odt_read", odt_read),
    ("export_docx", export_docx),
    ("compare_docx", compare_docx),
    ("api_open_sanitize_save", api_open_sanitize_save),
    ("api_template_apply", api_template_apply),
    ("image_decode", image_decode),
    ("codec_decode", codec_decode),
    ("codec_roundtrip", codec_roundtrip),
    ("pptx_open", pptx_open),
    ("dml_shapes", dml_shapes),
    ("text_layout", text_layout),
    ("slides_decode", slides_decode),
    ("slides_roundtrip", slides_roundtrip),
    ("slides_emit", slides_emit),
    ("slides_edit", slides_edit),
    ("slides_edit_wire", slides_edit_wire),
    ("ppt_convert", ppt_convert),
    ("chart_parse", chart_parse),
    ("chart_scale", chart_scale),
    ("chart_emit", chart_emit),
    ("slide_timing", slide_timing),
    ("emf_play", emf_play),
    ("pict_play", pict_play),
    ("tiff_slides", tiff_slides),
    ("eot_read", eot_read),
    ("anim_formula", anim_formula),
];

/// Small limits: every check of `Limits` is exercised without slow executions.
pub const FUZZ_LIMITS: Limits = Limits {
    max_xml_part_bytes: 1 << 20,
    max_binary_part_bytes: 2 << 20,
    max_total_bytes: 4 << 20,
    max_entries: 4_096,
    max_xml_depth: 200,
    max_xml_elements: 20_000,
};

/// Output cap of the inflate target.
const INFLATE_CAP: u64 = 1 << 20;

fn split1(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((&b, rest)) => (b, rest),
        None => (0, &[]),
    }
}

fn text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

/// Splits on 0 bytes into at most `n` fields (the last takes the rest).
fn fields(data: &[u8], n: usize) -> Vec<&[u8]> {
    data.splitn(n, |&b| b == 0).collect()
}

fn open_options(sel: u8) -> OpenOptions {
    OpenOptions {
        case_fold: if sel.is_multiple_of(2) { CaseFold::None } else { CaseFold::Ascii },
        skip_impossible_ratio: sel & 4 != 0,
        utf16_text: sel & 8 != 0,
    }
}

// ───────────────────────────── zip / package ─────────────────────────────

/// Opens a package and reads everything: each part within its cap, the total within the
/// budget, lookups consistent, `verify` and the text decoders without panics; every part parsed
/// through `xml_part` within the XML cap, depth and element limits (D3); names resolved to parts
/// the package holds (D6); the safe-path helper consistent (D8).
pub fn zip_open(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let limits = FUZZ_LIMITS;
    let Ok(pkg) = Package::open(bytes, &limits, &open_options(sel)) else { return };
    assert!(pkg.len() <= limits.max_entries as usize);
    let _ = pkg.verify();
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    for name in &names {
        assert!(pkg.contains(name));
        assert_eq!(pkg.find_name(name), Some(name.as_str()));
        match pkg.part(name) {
            Ok(Some(b)) => {
                assert!(b.len() as u64 <= limits.part_cap(name), "{name}: {} B over the part cap", b.len());
                let again = pkg.find(name).unwrap().unwrap();
                assert_eq!(again, b);
            }
            Ok(None) => panic!("{name} listed but missing"),
            Err(Error::Limit { kind: LimitKind::PartBytes | LimitKind::TotalBytes, .. }) | Err(Error::Corrupt(_)) => {}
            Err(e) => panic!("{name}: unexpected error {e:?}"),
        }
        assert!(pkg.produced_bytes() <= limits.max_total_bytes);
        let _ = pkg.part_text(name);
        let _ = pkg.find_name(&name.to_uppercase());
        let _ = pkg.find(&name.to_lowercase());
        match pkg.xml_part(name, &lenient_preset(sel)) {
            Ok(doc) => {
                assert!(doc.element_count() <= limits.max_xml_elements as usize);
                assert!(doc.source().len() as u64 <= limits.max_xml_part_bytes);
            }
            Err(Error::Limit { kind: LimitKind::PartBytes, max, part }) => {
                assert!(max == limits.max_xml_part_bytes && part.as_deref() == Some(name.as_str()));
            }
            Err(Error::Limit { kind: LimitKind::XmlElements | LimitKind::XmlDepth, part, .. }) => {
                assert_eq!(part.as_deref(), Some(name.as_str()))
            }
            Err(Error::Limit { kind: LimitKind::TotalBytes, .. } | Error::Corrupt(_) | Error::Xml { .. }) => {}
            Err(e) => panic!("{name}: unexpected xml_part error {e:?}"),
        }
        assert!(pkg.produced_bytes() <= limits.max_total_bytes);
        if let Some(found) = pkg.resolve_target("", &part_name::escape_segment(name).replace("%2F", "/")) {
            assert!(pkg.contains(found));
        }
        if let Some(path) = part_name::safe_relative_path(name) {
            assert_eq!(&path, name);
        }
    }
}

/// Opens, edits and saves a package, then reopens the result: same names (in order) and the
/// same bytes for every part that could be read.
pub fn zip_roundtrip(data: &[u8]) {
    let (sel, bytes) = split1(data);
    let limits = FUZZ_LIMITS;
    let options = OpenOptions { case_fold: CaseFold::None, ..open_options(sel) };
    let Ok(mut pkg) = Package::open(bytes, &limits, &options) else { return };
    // an edit: one part replaced (compressed on save), one removed
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    if sel & 16 != 0
        && let Some(n) = names.first()
    {
        pkg.set_part(n.clone(), b"<edited/>".to_vec());
    }
    if sel & 32 != 0
        && let Some(n) = names.last()
    {
        pkg.remove_part(n);
    }
    let before: Vec<(String, Option<Vec<u8>>)> = pkg
        .part_names()
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .into_iter()
        .map(|n| (n.clone(), pkg.part(&n).ok().flatten().map(<[u8]>::to_vec)))
        .collect();
    let save = SaveOptions { level: sel % 10, ..SaveOptions::default() };
    let saved = match pkg.save(&save) {
        Ok(s) => s,
        Err(Error::NameTooLong { .. }) => return,
        // D5: an entry that cannot be read is not copied raw; the save fails with the read's error
        Err(Error::Corrupt(_) | Error::Limit { .. }) if before.iter().any(|(_, b)| b.is_none()) => return,
        Err(e) => panic!("save failed: {e:?}"),
    };
    // reserved at its exact length (stage 5 C6, stage-6 finding S2)
    assert_eq!(saved.capacity(), saved.len());
    // D5: every entry written is well formed: it inflates exactly to its declared size and CRC
    for e in zip::read(&saved, u32::MAX).expect("a saved archive reads") {
        let raw = e.compressed(&saved);
        let (len, stream_len, crc) = if e.method == 0 {
            (raw.len() as u64, raw.len() as u64, flate::crc32(raw))
        } else {
            let c = flate::check_raw(raw, Some(e.size), u64::MAX).expect("a saved entry inflates");
            (c.len, c.stream_len, c.crc32)
        };
        assert_eq!((len, stream_len, crc), (e.size, e.size, e.crc32), "{}", e.name);
    }
    // the same part caps; no total, since the edited part counts there now; no ratio filter,
    // since an entry whose data the archive cut short is saved with its real compressed size
    // and its declared size (and the filter compares the two)
    let reopen = OpenOptions { skip_impossible_ratio: false, ..options };
    let reopened = Package::open(&saved, &Limits { max_total_bytes: u64::MAX, ..limits }, &reopen).expect("a saved package reopens");
    // fflate quirk kept: a UTF-8 name loses ONE leading BOM per read, so a name that still
    // starts with one changes on the next read
    if before.iter().any(|(n, _)| n.starts_with('\u{FEFF}')) {
        return;
    }
    let after: Vec<&str> = reopened.part_names().collect();
    let order = zip::js_object_order(before.iter().map(|(n, _)| (n.clone(), ())).collect());
    assert_eq!(after, order.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>());
    for (name, bytes) in &before {
        if let Some(b) = bytes {
            assert_eq!(reopened.part(name).unwrap().unwrap(), &b[..], "{name}");
        }
    }
}

/// Writes entries with hostile names (any text, `..`, `/`, `\`, NUL, very long by repetition)
/// and reads them back: the names and bytes come back as `unzipSync` would give them.
pub fn zip_write(data: &[u8]) {
    let (sel, rest) = split1(data);
    let level = sel % 10;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    // records: [repeat][name...]\0[content...]\0
    let mut parts = rest.split(|&b| b == 0);
    while let (Some(n), Some(c)) = (parts.next(), parts.next()) {
        let (rep, n) = split1(n);
        let name = text(n).repeat(1 + usize::from(rep) * usize::from(rep & 0x80 != 0) * 20);
        entries.push((name, c.to_vec()));
        if entries.len() >= 64 {
            break;
        }
    }
    let stored: Vec<String> = entries.iter().step_by(3).map(|(n, _)| n.clone()).collect();
    let mut w = ZipWriter::new(WriteOptions { level, stored, ..WriteOptions::default() });
    for (name, content) in &entries {
        match w.add(name, content) {
            Ok(()) => assert!(name.len() <= 65_535),
            Err(Error::NameTooLong { len }) => {
                assert!(len > 65_535);
                return;
            }
            Err(e) => panic!("write failed: {e:?}"),
        }
    }
    let archive = w.finish();
    // the model: names as decoded on read (one leading BOM dropped from UTF-8 names), a repeated
    // name keeps its first position with its last bytes, `__proto__` dropped, JS key order
    let mut model: Vec<(String, Vec<u8>)> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for (name, content) in &entries {
        let name = if name.is_ascii() { name.clone() } else { name.strip_prefix('\u{FEFF}').unwrap_or(name).to_owned() };
        if name == "__proto__" {
            continue;
        }
        match at.get(&name) {
            Some(&i) => model[i].1 = content.clone(),
            None => {
                at.insert(name.clone(), model.len());
                model.push((name, content.clone()));
            }
        }
    }
    let model = zip::js_object_order(model);
    let read = zip::read(&archive, u32::MAX).expect("a written archive reads");
    assert_eq!(read.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), model.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>());
    let limits = Limits { max_entries: u32::MAX, ..Limits::DEFAULT };
    let pkg = Package::open(&archive, &limits, &OpenOptions::default()).unwrap();
    for (name, content) in &model {
        assert_eq!(pkg.part(name).unwrap().unwrap(), &content[..], "{name}");
    }
}

// ───────────────────────────── flate ─────────────────────────────

/// `check_raw` (D5) describes exactly what `inflate_raw` returns, or fails the same way.
fn same_check(body: &[u8], declared: Option<u64>, cap: u64) {
    let check = flate::check_raw(body, declared, cap);
    match flate::inflate_raw(body, declared, cap) {
        Ok(o) => {
            let c = check.expect("check_raw fails where inflate_raw succeeds");
            assert_eq!((c.len, c.crc32), (o.len() as u64, flate::crc32(&o)));
            assert!(c.stream_len >= c.len);
            assert!(declared.is_none_or(|d| c.len <= d));
        }
        Err(e) => assert_eq!(check, Err(e)),
    }
}

/// Inflate in every mode with caps, differential against miniz_oxide on the streams it
/// accepts, and the deflate/zlib round trips.
pub fn inflate(data: &[u8]) {
    let (sel, rest) = split1(data);
    let cap = INFLATE_CAP;
    match sel % 6 {
        0 => {
            let ours = flate::inflate_raw(rest, None, cap);
            if let Ok(o) = &ours {
                assert!(o.len() as u64 <= cap);
            }
            if let Ok(theirs) = miniz_oxide::inflate::decompress_to_vec_with_limit(rest, cap as usize) {
                assert_eq!(
                    ours.as_deref().ok(),
                    Some(&theirs[..]),
                    "miniz_oxide accepts a raw stream that inflate_raw does not inflate the same"
                );
            }
        }
        1 => {
            let declared = rest.get(..4).map_or(0, |b| u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))) % (2 * cap);
            let body = rest.get(4..).unwrap_or(&[]);
            same_check(body, None, cap);
            same_check(body, Some(declared), cap);
            match flate::inflate_raw(body, Some(declared), cap) {
                Ok(o) => {
                    assert!(o.len() as u64 <= declared.min(cap));
                    // the declared size only truncates: a prefix of the undeclared output
                    if let Ok(full) = flate::inflate_raw(body, None, cap)
                        && !body.is_empty()
                    {
                        assert!(full.starts_with(&o));
                    }
                }
                Err(Error::Limit { kind: LimitKind::PartBytes, max, .. }) => assert_eq!(max, cap),
                Err(Error::Corrupt(_)) => {}
                Err(e) => panic!("{e:?}"),
            }
        }
        2 => {
            let ours = flate::inflate_zlib(rest, None, cap);
            if let Ok(theirs) = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(rest, cap as usize) {
                assert_eq!(
                    ours.as_deref().ok(),
                    Some(&theirs[..]),
                    "miniz_oxide accepts a zlib stream that inflate_zlib does not inflate the same"
                );
            }
        }
        3 => {
            let raw = flate::inflate_stream(rest, Framing::Raw, cap);
            let z = flate::inflate_stream(rest, Framing::Zlib, cap);
            for o in [raw, z].into_iter().flatten() {
                assert!(o.len() as u64 <= cap);
            }
        }
        4 => {
            let small = rest.first().map_or(0, |&b| u64::from(b) * 64);
            if let Some(o) = flate::inflate_capped(rest.get(1..).unwrap_or(&[]), small) {
                assert!(o.len() as u64 <= small);
            }
        }
        _ => {
            let level = rest.first().map_or(6, |&b| b % 12);
            let body = rest.get(1..).unwrap_or(&[]);
            let d = flate::deflate_raw(body, level);
            assert_eq!(flate::inflate_raw(&d, None, u64::MAX).unwrap(), body);
            assert_eq!(flate::inflate_raw(&d, Some(body.len() as u64), u64::MAX).unwrap(), body);
            let z = flate::zlib(body, level);
            assert_eq!(flate::inflate_zlib(&z, None, u64::MAX).unwrap(), body);
            assert_eq!(flate::inflate_stream(&z, Framing::Zlib, u64::MAX).unwrap(), body);
            assert_eq!(&z[z.len() - 4..], &flate::adler32(body).to_be_bytes());
            let (a, b) = body.split_at(body.len() / 2);
            assert_eq!(flate::crc32_update(flate::crc32(a), b), flate::crc32(body));
        }
    }
}

// ───────────────────────────── CFB ─────────────────────────────

/// Opens a compound file and reads its streams: never more bytes than the file per stream.
pub fn cfb(data: &[u8]) {
    let Ok(f) = CfbFile::open(data) else { return };
    let _ = format!("{f:?}");
    let entries = f.entries().to_vec();
    for e in entries.iter().take(64) {
        assert!(f.has(&e.path) || e.path.is_empty());
        let _ = f.entry(&e.path.to_uppercase());
        if let Ok(Some(s)) = f.stream(&e.path) {
            assert!(s.len() <= data.len(), "stream {} of {} B in a {} B file", e.path, s.len(), data.len());
        }
    }
}

// ───────────────────────────── XML ─────────────────────────────

const PREFIXES: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "w"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://schemas.openxmlformats.org/drawingml/2006/chart", "c"),
    ("urn:x", "x"),
];
const STRIP: &[&str] = &["http://schemas.openxmlformats.org/spreadsheetml/2006/main", "urn:x"];
const RAW: &[&str] = &["w:sectPr", "w:rPr", "a", "x:y"];

/// The lenient presets (docx, sheet, slides, charts) with the per-input toggles.
fn lenient_preset(sel: u8) -> ParseOptions<'static> {
    let base = ParseOptions::LENIENT;
    let mut o = match sel % 4 {
        // docx-core: canonical prefixes, depth limit, raw capture, positions
        0 => ParseOptions {
            canonical: Some(CanonicalPrefixes { table: PREFIXES, default_namespace: false }),
            max_depth: Some(FUZZ_LIMITS.max_xml_depth),
            raw_capture: RAW,
            positions: true,
            ..base
        },
        // sheet-core: root prefix stripped, EOL normalization, no depth limit
        1 => ParseOptions { strip_root_prefixes: STRIP, normalize_eol: true, ..base },
        // slides-core: bad references replaced, processing instructions skipped
        2 => ParseOptions { bad_char_ref: CharRefPolicy::Replace, content_pi: ContentPi::Skip, ..base },
        // charts canonicalNs
        _ => ParseOptions { canonical: Some(CanonicalPrefixes { table: PREFIXES, default_namespace: true }), ..base },
    };
    o.keep_comments = sel & 4 != 0;
    o.keep_pis = sel & 8 != 0 && o.content_pi == ContentPi::Skip;
    o.positions |= sel & 16 != 0;
    o
}

/// Walks the whole tree through the public API, checking spans and positions.
fn walk(doc: &Document) {
    let src = doc.source();
    let root = doc.root();
    let _ = root.text_content();
    let mut n = 0usize;
    for e in std::iter::once(root).chain(root.descendants()) {
        n += 1;
        let name = e.name();
        assert_eq!(doc.name(e.name_id()), name);
        let _ = (e.local_name(), e.namespace(), e.raw(), e.child_count(), e.attr_count());
        for a in e.attrs() {
            assert!(e.attr(a.name).is_some());
            if let Some(ns) = a.namespace {
                let _ = e.attr_ns(ns, a.name);
            }
        }
        for c in e.children() {
            match c {
                Node::Element(k) => assert!(k.id().index() > e.id().index()),
                Node::Text(t) | Node::CData(t) | Node::Comment(t) | Node::Pi(t) => {
                    let _ = t.len();
                }
            }
        }
        if let Some(p) = e.positions() {
            assert!(
                p.start <= p.content_start && p.content_start <= p.content_end && p.content_end <= p.end && p.end <= src.len(),
                "{p:?}"
            );
            assert!(e.source_slice().is_some(), "positions off a char boundary: {p:?}");
        }
        if let Some(r) = e.raw() {
            assert!(r.starts_with('<'));
        }
        let _ = e.first_child(name);
        let _ = e.last_child(name);
        let _ = e.descend(name);
    }
    assert_eq!(n, doc.element_count());
    let _ = doc.heap_bytes();
}

/// The lenient reader with each preset.
pub fn xml_lenient(data: &[u8]) {
    let (sel, rest) = split1(data);
    let opts = lenient_preset(sel);
    match parse(text(rest), &opts) {
        Ok(doc) => walk(&doc),
        Err(Error::Limit { kind: LimitKind::XmlDepth, .. }) => assert!(opts.max_depth.is_some()),
        Err(Error::Xml { offset, .. }) => assert!(offset <= rest.len() * 3 + 3),
        Err(Error::Limit { .. }) => {}
        Err(e) => panic!("{e:?}"),
    }
}

/// The strict reader (Editing API, sanitizer): what it accepts is namespace-well-formed.
pub fn xml_strict(data: &[u8]) {
    let (sel, rest) = split1(data);
    let opts = ParseOptions { keep_comments: sel & 1 != 0, keep_pis: sel & 2 != 0, positions: sel & 4 != 0, ..ParseOptions::STRICT };
    let source = text(rest);
    match parse(source.as_str(), &opts) {
        Ok(doc) => {
            walk(&doc);
            // strict trees have every prefixed element and attribute name bound
            for e in std::iter::once(doc.root()).chain(doc.root().descendants()) {
                if e.name().contains(':') {
                    assert!(e.namespace().is_some(), "{} unbound", e.name());
                }
                for a in e.attrs() {
                    if a.name.contains(':') {
                        assert!(a.namespace.is_some(), "{} unbound", a.name);
                    }
                }
            }
        }
        Err(Error::Limit { kind: LimitKind::XmlDepth, max, .. }) => assert_eq!(max, 256),
        Err(Error::Xml { .. } | Error::Limit { .. }) => {}
        Err(e) => panic!("{e:?}"),
    }
}

/// XML 1.0 strict normalization of an attribute value (TAB, LF, CR and CRLF → one space).
fn norm_attr(s: &str) -> String {
    s.replace("\r\n", " ").replace(['\r', '\n', '\t'], " ")
}

/// XML 1.0 end-of-line normalization of text.
fn norm_eol(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Escapes then parses: every escape variant gives back the text (lenient) or its normalized,
/// valid-characters form (strict).
pub fn escape_roundtrip(data: &[u8]) {
    let s = text(data);
    // lenient (docx preset without tables): exact text and attribute value
    for (attr_esc, text_esc) in [(Escape::ATTR, Escape::TEXT), (Escape::ALL, Escape::ALL), (Escape::ATTR_NO_GT, Escape::TEXT)] {
        let xml = format!("<a x=\"{}\">{}</a>", write::escape(&s, attr_esc), write::escape(&s, text_esc));
        let doc = parse(xml, &ParseOptions::LENIENT).unwrap();
        let root = doc.root();
        assert_eq!(root.attr("x"), Some(s.as_str()));
        assert_eq!(root.text_content(), s);
    }
    // strict with the stripping escapes
    let valid = write::strip_invalid(&s).into_owned();
    let mut w = XmlWriter::with_declaration(write::XML_DECL);
    w.start("a").attr("x", &s).start("b").text(&s).end().start("c").end();
    let xml = w.finish();
    let doc = parse(xml.as_str(), &ParseOptions::STRICT).unwrap_or_else(|e| panic!("{e:?} in {xml:?}"));
    let root = doc.root();
    assert_eq!(root.attr("x"), Some(norm_attr(&valid).as_str()));
    assert_eq!(root.first_child("b").unwrap().text_content(), norm_eol(&valid));
    // the element helpers
    let el = write::el("p", &[("v", Some(&s)), ("n", None)], &write::esc_text_valid(&s));
    let doc = parse(el, &ParseOptions::STRICT).unwrap();
    assert_eq!(doc.root().attr("v"), Some(norm_attr(&valid).as_str()));
    assert_eq!(doc.root().attr("n"), None);
}

// ───────────────────────────── raw surgery ─────────────────────────────

/// `split_element`, `set_kid`, `join_kids`, `with_attrs`, `insert_child` (every anchor),
/// `expand_self_closing` and `remove_range` on any text.
pub fn raw_surgery(data: &[u8]) {
    let (sel, rest) = split1(data);
    let f = fields(rest, 4);
    let xml = text(f.first().copied().unwrap_or(&[]));
    let name = text(f.get(1).copied().unwrap_or(b"w:b"));
    let key = text(f.get(2).copied().unwrap_or(b"w:val"));
    let value = text(f.get(3).copied().unwrap_or(b"1"));
    let opts = lenient_preset(sel);
    if let Ok(mut split) = raw::split_element(&xml, &opts, &FUZZ_LIMITS) {
        let order = ["w:b", "w:i", "w:sz", name.as_str()];
        raw::set_kid(&mut split.kids, &order, &name, if sel & 32 != 0 { None } else { Some(&value) });
        raw::set_kid(&mut split.kids, &[], "w:i", Some("<w:i/>"));
        let joined = raw::join_kids(&split.open, &split.kids, &split.close);
        if !split.kids.is_empty() {
            assert!(joined.starts_with(&split.open) && joined.ends_with(&split.close));
        }
        let names: Vec<&str> = split.kids.iter().map(|k: &Kid| k.name.as_str()).collect();
        assert!(names.iter().filter(|n| **n == name).count() <= 1);
    }
    let changes = [(key.as_str(), Some(value.as_str())), ("w:x", None), (key.as_str(), None)];
    if let Some(t) = raw::with_attrs(&xml, &name, &changes) {
        assert!(t.starts_with('<'));
    }
    let _ = raw::with_attrs("", &name, &changes);
    for at in
        [Insert::BeforeFirstClose, Insert::AfterRootOpen, Insert::BeforeLastCloseTrimmed, Insert::EndOfRoot, Insert::BeforeFirst(&name)]
    {
        if let Ok(Some(out)) = raw::insert_child(&xml, &key, &value, at, &FUZZ_LIMITS) {
            assert!(out.contains(value.as_str()));
        }
    }
    let _ = raw::expand_self_closing(&xml, &key);
    let (a, b) = (usize::from(sel) * 7 % (xml.len() + 1), xml.len() / 2);
    let _ = raw::remove_range(&xml, a.min(b), a.max(b));
}

// ───────────────────────────── rels / content types ─────────────────────────────

/// `.rels` reading and every edit; written relationships read back equal.
pub fn rels(data: &[u8]) {
    let (sel, rest) = split1(data);
    let xml = text(rest);
    let opts = lenient_preset(sel);
    if let Ok(list) = rels::parse_rels(&xml, &opts, &FUZZ_LIMITS) {
        let _ = rels::find_by_id(&list, "rId1");
        let _ = rels::find_by_type_suffix(&list, "/image");
        let _ = rels::next_rel_id_after_max(&list);
        let id = rels::first_free_rel_id(&list, "rId", u64::from(sel));
        assert!(!list.iter().any(|r| r.id == id));
        // write, read back: values come back with invalid characters stripped
        let out = rels::write_rels(&list, write::XML_DECL);
        let back = rels::parse_rels(&out, &ParseOptions::LENIENT, &FUZZ_LIMITS).unwrap();
        assert_eq!(back.len(), list.len());
        for (a, b) in list.iter().zip(&back) {
            assert_eq!(write::strip_invalid(&a.id), b.id);
            assert_eq!(write::strip_invalid(&a.type_), b.type_);
            assert_eq!(write::strip_invalid(&a.target), b.target);
            assert_eq!(a.external, b.external);
        }
        if let Ok((kept, removed)) = rels::remove_rels(&xml, &opts, &FUZZ_LIMITS, |r| r.id.len() % 2 == 0) {
            assert!(kept.len() <= xml.len());
            assert!(removed.iter().all(|r| r.id.len() % 2 == 0));
        }
        if let Some(first) = list.first() {
            let _ = rels::set_rel_target(&xml, &opts, &FUZZ_LIMITS, &first.id, "media/x&y.png");
        }
    }
    let rel = Relationship::new("rId9", "t", &xml).external();
    for at in [
        Insert::BeforeFirstClose,
        Insert::AfterRootOpen,
        Insert::BeforeLastCloseTrimmed,
        Insert::EndOfRoot,
        Insert::BeforeFirst("Relationship"),
    ] {
        let _ = rels::add_rel(Some(&xml), &rel, at, &FUZZ_LIMITS);
        let _ = rels::add_rel(None, &rel, at, &FUZZ_LIMITS);
    }
    let _ = (rels::rels_part_for(&xml), rels::rels_part_for_legacy(&xml));
}

/// `[Content_Types].xml` reading and edits.
pub fn content_types(data: &[u8]) {
    let (sel, rest) = split1(data);
    let f = fields(rest, 2);
    let xml = text(f.first().copied().unwrap_or(&[]));
    let part = text(f.get(1).copied().unwrap_or(b"word/document.xml"));
    let opts = lenient_preset(sel);
    if let Ok(ct) = ContentTypes::parse(&xml, &opts, &FUZZ_LIMITS) {
        let _ = (ct.content_type_of_exact(&part), ct.override_of_exact(&part), ct.default_of(&part));
        let _ = (ct.content_type_of_part(&part), ct.override_of_part(&part));
    }
    for at in [
        Insert::BeforeFirstClose,
        Insert::AfterRootOpen,
        Insert::BeforeLastCloseTrimmed,
        Insert::EndOfRoot,
        Insert::BeforeFirst("Override"),
    ] {
        let _ = content_types::add_override(&xml, &part, "application/xml", at, &FUZZ_LIMITS);
        let _ = content_types::add_default(&xml, &part, "image/png", at, &FUZZ_LIMITS);
    }
    let _ = content_types::remove_override(&xml, &opts, &FUZZ_LIMITS, &part);
    let _ = content_types::remove_default(&xml, &opts, &FUZZ_LIMITS, &part);
    let _ = content_types::remove_overrides_of_part(&xml, &opts, &FUZZ_LIMITS, &part);
    // the generated elements read back
    let x = format!("<Types>{}{}</Types>", content_types::default_xml(&part, "a&b"), content_types::override_xml(&part, "c<d"));
    let ct = ContentTypes::parse(&x, &ParseOptions::LENIENT, &FUZZ_LIMITS).unwrap();
    assert_eq!(ct.defaults[0].extension, part);
    assert_eq!(ct.override_of_exact(&part), Some("c<d"));
    // the OPC rule finds it too, in any ASCII case, when the name is a part name written as is
    if part_name::resolve_relationship_target("", &format!("/{part}")).as_deref() == Some(part.as_str()) {
        assert_eq!(ct.override_of_part(&part), Some("c<d"));
        assert_eq!(ct.content_type_of_part(&part.to_ascii_uppercase()), Some("c<d"));
    }
}

// ───────────────────────────── lookup / text / resolver ─────────────────────────────

/// The caseless index against a model, through a sequence of edits.
pub fn caseless(data: &[u8]) {
    let (sel, rest) = split1(data);
    let policy = if sel.is_multiple_of(2) { CaseFold::None } else { CaseFold::Ascii };
    let fold = |s: &str| match policy {
        CaseFold::None => None,
        CaseFold::Ascii => Some(part_name::fold_ascii(s)),
    };
    let names: Vec<String> = rest.split(|&b| b == 0).take(24).map(text).collect();
    let options = OpenOptions { case_fold: policy, ..OpenOptions::default() };
    let mut pkg = Package::from_parts(names.iter().map(|n| (n.clone(), n.as_bytes().to_vec())), &FUZZ_LIMITS, &options);
    // the model: ordered unique names
    let mut model: Vec<String> = Vec::new();
    for n in &names {
        if !model.contains(n) {
            model.push(n.clone());
        }
    }
    let check = |pkg: &Package, model: &[String], queries: &[String]| {
        assert_eq!(pkg.part_names().collect::<Vec<_>>(), model.iter().map(String::as_str).collect::<Vec<_>>());
        for q in queries {
            let expected = if model.contains(q) {
                Some(q.as_str())
            } else {
                fold(q).and_then(|k| model.iter().find(|m| fold(m).as_deref() == Some(k.as_str()))).map(String::as_str)
            };
            assert_eq!(pkg.find_name(q), expected, "query {q:?}");
        }
    };
    let queries: Vec<String> = names.iter().flat_map(|n| [n.to_uppercase(), n.to_lowercase(), n.clone()]).collect();
    check(&pkg, &model, &queries);
    // edits driven by the selector: remove, retain, add
    if let Some(n) = names.get(usize::from(sel) % names.len().max(1)) {
        pkg.remove_part(n);
        model.retain(|m| m != n);
        check(&pkg, &model, &queries);
    }
    pkg.retain(|n| n.len() % 3 != 1);
    model.retain(|n| n.len() % 3 != 1);
    check(&pkg, &model, &queries);
    for n in names.iter().take(4) {
        let added = n.to_uppercase();
        pkg.set_part(added.clone(), Vec::new());
        if !model.contains(&added) {
            model.push(added);
        }
    }
    check(&pkg, &model, &queries);
    // D6: the OPC rule (resolver, then the exact name or the first ASCII-folded match),
    // whatever the package's policy
    for q in &queries {
        let expected = part_name::resolve_relationship_target("", q).and_then(|r| {
            if model.contains(&r) { Some(r) } else { model.iter().find(|m| part_name::fold_ascii(m) == part_name::fold_ascii(&r)).cloned() }
        });
        assert_eq!(pkg.resolve_target("", q), expected.as_deref(), "target {q:?}");
    }
}

/// Part text decoding (UTF-8 with BOM, UTF-16 with BOM).
pub fn part_text(data: &[u8]) {
    let utf8 = package::decode_text(data, false);
    let body = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    assert_eq!(utf8, String::from_utf8_lossy(body));
    let t = package::decode_text(data, true);
    match data {
        [0xFF, 0xFE, rest @ ..] | [0xFE, 0xFF, rest @ ..] => {
            let le = data[0] == 0xFF;
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
                .collect();
            let mut expected = String::from_utf16_lossy(&units);
            if rest.len() % 2 == 1 {
                expected.push('\u{FFFD}');
            }
            assert_eq!(t, expected);
        }
        _ => assert_eq!(t, utf8),
    }
    let _ = zip::decode_latin1(data);
}

/// The relationship resolver: what it returns is a clean part name built from the target
/// (no empty, `.` or `..` segment, no `\`, no control character from the target), and every
/// escaped segment resolves back to itself.
pub fn resolver(data: &[u8]) {
    let f = fields(data, 2);
    let source = text(f.first().copied().unwrap_or(&[]));
    let target = text(f.get(1).copied().unwrap_or(&[]));
    if let Some(name) = part_name::resolve_relationship_target(&source, &target) {
        assert!(!name.is_empty() && !name.starts_with('/'));
        for s in name.split('/') {
            assert!(!s.is_empty() && s != "." && s != "..", "{name:?}");
        }
    }
    // the folder of the source is taken as is (a zip name); every segment from the target is
    // checked after unescaping
    if let Some(name) = part_name::resolve_relationship_target("", &target) {
        assert!(!name.contains('\\') && !name.chars().any(char::is_control), "{name:?}");
    }
    // escape → resolve gives the segment back
    let seg = target.replace('/', "_");
    if !seg.is_empty() && seg != "." && seg != ".." && !seg.contains('\\') && !seg.chars().any(char::is_control) {
        assert_eq!(part_name::resolve_relationship_target("", &part_name::escape_segment(&seg)).as_deref(), Some(seg.as_str()));
        assert_eq!(
            part_name::resolve_relationship_target("word/document.xml", &part_name::escape_segment(&seg)),
            Some(format!("word/{seg}"))
        );
    }
    let _ = part_name::fold_ascii(&target);
    // D8: what `safe_relative_path` accepts is a plain relative path
    for name in [&source, &target] {
        if let Some(path) = part_name::safe_relative_path(name) {
            assert_eq!(&path, name);
            assert!(!path.starts_with('/') && !path.contains(['\\', ':', '<', '>', '"', '|', '?', '*']), "{path:?}");
            assert!(!path.chars().any(char::is_control), "{path:?}");
            for s in path.split('/') {
                assert!(!s.is_empty() && s != "." && s != ".." && !s.ends_with(['.', ' ']), "{path:?}");
            }
        }
    }
}
