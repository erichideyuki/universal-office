//! End-to-end parity of the server path (record uo-bindings.md, "Parity"): Rust read → layout →
//! navigation → PDF of the 68 committed layout fixture documents (uo-docx-layout
//! `tests/captures/f7a..f8/docs`, open fonts) equals the frozen TS read → `layoutDocument` →
//! `pdfNavigation` → `renderPdf` **byte for byte** (`compress: false`).
//!
//! The TS side is captured by uo-pdf's harness (`tests/parity_support/capture_painter.mjs --docs
//! <dir> --open-fonts`, the node-fonts provider over the open fonts of the repository, every pattern
//! set loaded, `now` 2026-01-15T12:00Z); the captures are `tests/parity/server-<family>.cap`. The Rust
//! side runs the real [`Converter`] with the `oracle` policy (the port of node-fonts) over the same
//! two folders, and the `server-substitute` policy that ships — over these documents the two answer
//! the same faces, so the shipped policy is compared too.
//!
//!   UO_TS_ROOT=<built d0f45bc5> node crates/uo-pdf/tests/parity_support/capture_painter.mjs \
//!     --docs crates/uo-docx-layout/tests/captures/<family>/docs --open-fonts --dump <tmp> \
//!     --out crates/uo-ffi/tests/parity/server-<family>.cap

#![cfg(not(target_arch = "wasm32"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::{Path, PathBuf};

use uo_docx_layout::LayoutOptions;
use uo_docx_layout::options::{Clock, TimeZoneRule};
use uo_ffi::pipeline::Trust;
use uo_ffi::server::{Converter, ConverterOptions, PdfJob};
use uo_fonts::policy::PolicyName;

const FAMILIES: [&str; 5] = ["f7a", "f7b", "f7c", "f7d", "f8"];

/// Every n-th document of the fixtures (`UO_SERVER_PARITY_EVERY`, default 7: about ten documents, the debug
/// test job); CI's release step and `UO_BLESS` run all 68.
fn every() -> usize {
    if std::env::var_os("UO_BLESS").is_some() {
        return 1;
    }
    std::env::var("UO_SERVER_PARITY_EVERY").ok().and_then(|s| s.parse().ok()).filter(|&n| n > 0).unwrap_or(7)
}

/// The documents of the five captures: `(family, key, raw)`, every n-th.
fn documents() -> Vec<(&'static str, String, String)> {
    let mut all = Vec::new();
    for fam in FAMILIES {
        for (key, raw) in raws(&core().join(format!("crates/uo-ffi/tests/parity/server-{fam}.cap"))) {
            all.push((fam, key, raw));
        }
    }
    assert_eq!(all.len(), 68, "the five fixture families");
    all.into_iter().step_by(every()).collect()
}

fn core() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn open_font_dirs() -> Vec<PathBuf> {
    let repo = core().join("..");
    vec![repo.join("shared/fonts/files"), repo.join("site/assets/fonts")]
}

pub fn ts_options() -> LayoutOptions {
    LayoutOptions {
        clock: Clock { now_ms: 1_768_478_400_000.0, tz: TimeZoneRule::Fixed { offset_min: 0 } },
        hyphenation_loaded: uo_fonts::hyphen::HYPHENATION_PATTERN_IDS.iter().map(|s| Box::<str>::from(*s)).collect(),
        ..LayoutOptions::default()
    }
    // wave 7c (W): every Word-over-TS switch on (ts_empty_theme, ts_pageref_without_chapter and the six of wave 7c):
    // replays against the frozen TS captures
    .ts_strict()
}

/// `key → raw` of a capture (`raw` = `length:sha256` prefix of 32 hex digits).
fn raws(path: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with('@') && !l.is_empty())
        .filter_map(|l| {
            let mut f = l.split('\t');
            let key = f.next()?.to_owned();
            let raw = f.find_map(|x| x.strip_prefix("raw="))?.to_owned();
            Some((key, raw))
        })
        .collect()
}

fn digest(pdf: &[u8]) -> String {
    let h = uo_testkit::stream::hex(&uo_testkit::sha256::sha256(pdf));
    format!("{}:{}", pdf.len(), &h[..32])
}

#[test]
fn the_server_pdf_equals_the_ts_render_pdf() {
    let conv = Converter::new(&ConverterOptions { system_fonts: false, font_dirs: open_font_dirs(), trust: Trust::Trusted });
    let job = PdfJob { layout: ts_options(), compress: false };
    let (mut docs, mut oracle_eq, mut server_eq, mut bytes) = (0, 0, 0, 0usize);
    let mut bad = Vec::new();
    for (fam, key, raw) in documents() {
        let docx = std::fs::read(core().join(format!("crates/uo-docx-layout/tests/captures/{fam}/docs/{key}.docx"))).unwrap();
        let oracle = conv.convert(docx.clone(), &job, PolicyName::Oracle, None, Vec::new()).unwrap();
        let server = conv.docx_to_pdf(docx, &job, None).unwrap();
        docs += 1;
        bytes += oracle.len();
        if digest(&oracle) == raw {
            oracle_eq += 1;
        } else {
            bad.push(format!("{fam}/{key} oracle {} vs TS {raw}", digest(&oracle)));
        }
        if digest(&server) == raw {
            server_eq += 1;
        } else {
            bad.push(format!("{fam}/{key} server-substitute {} vs TS {raw}", digest(&server)));
        }
    }
    println!("server parity: {docs} documents, {bytes} bytes; oracle policy {oracle_eq} equal, server-substitute {server_eq} equal");
    assert_eq!(docs, 68usize.div_ceil(every()));
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
}

/// The compressed server PDFs (the shipped options) as `family/key\tlength:sha256-prefix`: committed in
/// `tests/parity/server-native.tsv`, read by the .NET facade tests (the facade's bytes equal the native
/// ones). `UO_BLESS=1` rewrites the file.
#[test]
fn the_compressed_server_pdfs_are_the_committed_ones() {
    let conv = Converter::new(&ConverterOptions { system_fonts: false, font_dirs: open_font_dirs(), trust: Trust::Untrusted });
    let job = PdfJob { layout: ts_options(), compress: true };
    let mut out = String::new();
    for (fam, key, _) in documents() {
        let docx = std::fs::read(core().join(format!("crates/uo-docx-layout/tests/captures/{fam}/docs/{key}.docx"))).unwrap();
        let pdf = conv.docx_to_pdf(docx, &job, None).unwrap();
        out.push_str(&format!("{fam}/{key}\t{}\n", digest(&pdf)));
    }
    let path = core().join("crates/uo-ffi/tests/parity/server-native.tsv");
    if std::env::var_os("UO_BLESS").is_some() {
        std::fs::write(&path, &out).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    for line in out.lines() {
        assert!(committed.lines().any(|c| c == line), "{line} is not in {} (UO_BLESS=1 rewrites it)", path.display());
    }
}

/// Opt-in (local, installed fonts — the oracle documents name Office fonts, so nothing of it is
/// committed): `UO_SERVER_ORACLE_DIR=<dir>` holds `<group>.cap` files that capture_painter.mjs wrote
/// for `write/packages/docx-layout/test/oracle/<group>` **without** `--open-fonts` (node-fonts over the
/// machine's fonts); the Rust oracle policy over the machine's index must give the same bytes. Writes
/// `<dir>/native.tsv` (the compressed server-substitute PDFs) for the .NET facade's opt-in test.
#[test]
fn the_oracle_documents_with_installed_fonts_equal_the_ts() {
    let Some(dir) = std::env::var_os("UO_SERVER_ORACLE_DIR").map(PathBuf::from) else {
        println!("SKIPPED: set UO_SERVER_ORACLE_DIR (see the doc comment)");
        return;
    };
    let docs_root = core().join("../write/packages/docx-layout/test/oracle");
    let conv = Converter::new(&ConverterOptions { system_fonts: true, font_dirs: Vec::new(), trust: Trust::Trusted });
    let raw_job = PdfJob { layout: ts_options(), compress: false };
    let shipped = PdfJob { layout: ts_options(), compress: true };
    let server = Converter::new(&ConverterOptions { system_fonts: true, font_dirs: Vec::new(), trust: Trust::Untrusted });
    let mut caps: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "cap")).collect();
    caps.sort();
    let (mut n, mut eq, mut pages_bytes) = (0, 0, 0usize);
    let mut bad = Vec::new();
    let mut native = String::new();
    for cap in caps {
        let group = cap.file_stem().unwrap().to_string_lossy().into_owned();
        for (key, raw) in raws(&cap) {
            let docx = std::fs::read(docs_root.join(&group).join(format!("{key}.docx"))).unwrap();
            let pdf = conv.convert(docx.clone(), &raw_job, PolicyName::Oracle, None, Vec::new()).unwrap();
            n += 1;
            pages_bytes += pdf.len();
            if digest(&pdf) == raw {
                eq += 1;
            } else {
                bad.push(format!("{group}/{key}: {} vs TS {raw}", digest(&pdf)));
            }
            native.push_str(&format!("{group}/{key}\t{}\n", digest(&server.docx_to_pdf(docx, &shipped, None).unwrap())));
        }
    }
    std::fs::write(dir.join("native.tsv"), native).unwrap();
    println!("oracle documents: {n}, {eq} equal to the TS renderPdf ({pages_bytes} bytes uncompressed)");
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
}
