//! Test-only export (feature `opc-memtest`) for the wasm memory gate of `uo-opc` (stage 4, M2):
//! one call runs the app-like round trip of the native soak on a byte buffer — open → read and
//! parse every XML part → edit → save → reopen → verify → read and parse again → drop — so the
//! Node test can sample live bytes and `memory.buffer.byteLength`. It is not the binding API of
//! `uo-opc` (that arrives with the format crates); the presets are reduced copies of the apps'.

use uo_opc::cfb::CfbFile;
use uo_opc::package::{CaseFold, OpenOptions, Package, SaveOptions};
use uo_opc::xml::raw::{Insert, insert_child};
use uo_opc::xml::{CanonicalPrefixes, CharRefPolicy, ContentPi, ParseOptions, parse};
use uo_opc::{Limits, limits};

const WORD: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "w"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://schemas.openxmlformats.org/markup-compatibility/2006", "mc"),
];
const SHEET_MAIN: &[&str] =
    &["http://schemas.openxmlformats.org/spreadsheetml/2006/main", "http://schemas.openxmlformats.org/package/2006/relationships"];
const DOCX: ParseOptions<'static> = ParseOptions {
    canonical: Some(CanonicalPrefixes { table: WORD, default_namespace: false }),
    max_depth: Some(1000),
    raw_capture: &["w:drawing"],
    positions: true,
    ..ParseOptions::LENIENT
};
const SHEET: ParseOptions<'static> = ParseOptions { strip_root_prefixes: SHEET_MAIN, normalize_eol: true, ..ParseOptions::LENIENT };
const SLIDES: ParseOptions<'static> =
    ParseOptions { bad_char_ref: CharRefPolicy::Replace, content_pi: ContentPi::Skip, ..ParseOptions::LENIENT };

/// (open options, preset, main part) of a kind: 0 docx, 1 xlsx, 2 pptx.
fn product(kind: u32) -> Option<(OpenOptions, ParseOptions<'static>, &'static str)> {
    match kind {
        0 => Some((OpenOptions::default(), DOCX, "word/document.xml")),
        1 => Some((OpenOptions { case_fold: CaseFold::Unicode, ..OpenOptions::default() }, SHEET, "xl/worksheets/sheet1.xml")),
        2 => Some((OpenOptions { skip_impossible_ratio: true, ..OpenOptions::default() }, SLIDES, "ppt/slides/slide1.xml")),
        _ => None,
    }
}

fn parse_all(pkg: &Package, preset: &ParseOptions<'_>) -> uo_opc::Result<usize> {
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let mut elements = 0;
    for name in &names {
        if limits::is_xml_like(name) {
            if let Some(text) = pkg.part_text(name)? {
                elements += parse(text.as_ref(), preset)?.element_count();
            }
        } else {
            pkg.part(name)?;
        }
    }
    Ok(elements)
}

fn zip_round_trip(bytes: Vec<u8>, kind: u32) -> uo_opc::Result<usize> {
    let Some((options, preset, main)) = product(kind) else { return Err(uo_opc::Error::Corrupt("unknown kind")) };
    let mut pkg = Package::open_owned(bytes, &Limits::DEFAULT, &options)?;
    parse_all(&pkg, &preset)?;
    let text = pkg.require(main).map(|b| String::from_utf8_lossy(b).into_owned())?;
    let root = parse(text.as_str(), &preset)?.root().name().to_owned();
    let edited = insert_child(&text, &root, "<uo:soak xmlns:uo=\"urn:uo\"/>", Insert::EndOfRoot)?.unwrap_or(text);
    pkg.set_part_text(main, edited);
    pkg.set_part_text("customXml/soak.xml", "<soak/>");
    let saved = pkg.save(&SaveOptions::default())?;
    drop(pkg);
    let again = Package::open_owned(saved, &Limits::DEFAULT, &options)?;
    again.verify()?;
    parse_all(&again, &preset)
}

fn cfb_read_all(bytes: &[u8]) -> uo_opc::Result<usize> {
    let f = CfbFile::open(bytes)?;
    let mut total = 0;
    for e in f.entries() {
        if let Some(s) = f.stream(&e.path)? {
            total += s.len();
        }
    }
    Ok(total)
}

/// Runs the round trip on a buffer from [`crate::uo_alloc_bytes`] (filled with `len` bytes),
/// which it takes back and releases. `kind`: 0 docx, 1 xlsx, 2 pptx, 3 legacy compound file
/// (open and read every stream). Returns the elements parsed (or bytes read for kind 3), or −1
/// when the input fails.
///
/// # Safety
/// `ptr`/`len` must be a buffer from `uo_alloc_bytes(len)` that JS filled completely; it is
/// consumed here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_memtest_round_trip(ptr: *mut u8, len: usize, kind: u32) -> f64 {
    if ptr.is_null() {
        return -1.0;
    }
    // SAFETY: the caller guarantees the buffer came from uo_alloc_bytes(len) and holds len
    // initialized bytes; it is taken back here exactly once.
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let r = if kind == 3 { cfb_read_all(&bytes) } else { zip_round_trip(bytes, kind) };
    r.map_or(-1.0, |n| n as f64)
}
