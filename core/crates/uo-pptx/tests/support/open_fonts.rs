//! The open fonts of the repository (the files the B3 captures read) as a `FaceSet`; no proprietary font.
#![allow(dead_code, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_fonts::{Face, FontLimits};
use uo_pptx::pdf::FaceSet;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Liberation Sans, Serif and Mono, Carlito and Caladea in the four variants; the fallback is Liberation Sans.
pub fn open_fonts() -> FaceSet {
    let mut set = FaceSet::new("liberation sans");
    let sets: [(&str, PathBuf, &str); 5] = [
        ("liberation sans", root().join("site/assets/fonts"), "liberationsans"),
        ("liberation serif", root().join("shared/fonts/files"), "liberationserif"),
        ("liberation mono", root().join("shared/fonts/files"), "liberationmono"),
        ("carlito", root().join("shared/fonts/files"), "carlito"),
        ("caladea", root().join("shared/fonts/files"), "caladea"),
    ];
    for (family, dir, stem) in sets {
        for (bold, italic, style) in [(false, false, "regular"), (true, false, "bold"), (false, true, "italic"), (true, true, "bolditalic")]
        {
            let bytes: Arc<[u8]> = std::fs::read(dir.join(format!("{stem}-{style}.ttf"))).unwrap().into();
            let face = Face::parse(bytes, &FontLimits::NATIVE).unwrap();
            set.insert(family, bold, italic, Arc::new(face));
        }
    }
    set
}
