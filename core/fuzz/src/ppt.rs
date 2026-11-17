//! The `ppt_convert` target of the `.ppt` converter (`uo-pptx` frontier B2, record
//! `core/docs/modules/uo-pptx.md`, "B2 — .ppt converter"): a whole `.ppt` file through the converter
//! (the Compound File reader, the persist directory and the edit chain, the records, the OfficeArt
//! drawings, the text cascade, the animations, the round-trip packages, the media) and, when the file
//! opens, through the reader and the writer apart from the container.
//!
//! Input: a selector byte (bit 0: without the round-trip layer), then the file.
//!
//! Invariants beyond "no panic": a conversion that succeeds starts with `[Content_Types].xml` and
//! has `ppt/presentation.xml`, its part names are unique, and converting the same bytes again gives
//! the same parts (the GUID counter and every cache are per conversion).

use std::collections::HashSet;
use std::hint::black_box;
use std::path::Path;

use uo_pptx::ppt::{ConvertOptions, convert_ppt_to_pptx_parts, ppt_to_pptx_parts, read_ppt_document};

pub fn ppt_convert(data: &[u8]) {
    let (sel, file) = match data.split_first() {
        Some((&s, rest)) => (s, rest),
        None => (0, &[][..]),
    };
    let opts = ConvertOptions { use_round_trip: sel & 1 == 0, today: Some((2026, 11, 17)) };
    match convert_ppt_to_pptx_parts(file, &opts) {
        Ok(parts) => {
            assert_eq!(parts.first().map(|(n, _)| n.as_str()), Some("[Content_Types].xml"));
            assert!(parts.iter().any(|(n, _)| n == "ppt/presentation.xml"));
            let names: HashSet<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names.len(), parts.len());
            let again = convert_ppt_to_pptx_parts(file, &opts).expect("a conversion that worked works again");
            assert!(again == parts, "the conversion is not deterministic");
        }
        Err(e) => {
            black_box(e.message());
        }
    }
    // the reader and the writer without the container: the streams as the file holds them
    let Ok(cfb) = uo_opc::cfb::CfbFile::open(file) else { return };
    let Ok(Some(doc)) = cfb.stream("PowerPoint Document") else { return };
    let pics = cfb.stream("Pictures").ok().flatten();
    let cu = cfb.stream("Current User").ok().flatten();
    if let Ok(d) = read_ppt_document(&doc, pics.as_deref(), cu.as_deref())
        && let Ok(p) = ppt_to_pptx_parts(&d, &opts, (2026, 11, 17))
    {
        black_box(p.len());
    }
}

/// Seeds: the 25 tracked `.ppt` samples (the 5 smallest also with the round-trip layer off).
pub fn ppt_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let mut files: Vec<Vec<u8>> = Vec::new();
    for sub in ["legacy", "playback"] {
        let Ok(rd) = std::fs::read_dir(repo.join("slides/samples").join(sub)) else { continue };
        let mut names: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "ppt")).collect();
        names.sort();
        for p in names {
            if let Ok(b) = std::fs::read(&p) {
                files.push(b);
            }
        }
    }
    for b in &files {
        out.push(("ppt_convert", [&[0u8][..], b].concat()));
    }
    files.sort_by_key(Vec::len);
    for b in files.iter().take(5) {
        out.push(("ppt_convert", [&[1u8][..], b].concat()));
    }
    out
}
