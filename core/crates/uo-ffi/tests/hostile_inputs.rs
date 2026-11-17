//! Hostile documents of the milestone-1 security review through the C ABI (record uo-bindings.md,
//! "Milestone-1 gate fixes"): nested tables and text boxes converted from a 1 MiB caller thread (the
//! layout of a nested table level uses about 19 KB of release stack; the conversion runs on its own
//! thread of `CONVERSION_STACK_BYTES`), and inflation bombs refused with LIMIT_EXCEEDED by the
//! untrusted converter (256 MiB per part, 512 MiB in total, the frozen C# `OpcPackageLimits`).
//!
//! The committed fixtures in `tests/hostile/` are the generators' output (`UO_BLESS=1` rewrites them).
//! The two bomb tests inflate up to 512 MiB: release builds only (the CI `server-pdf` job).

#![cfg(not(target_arch = "wasm32"))] // native only, as the crate
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test harness

use std::path::PathBuf;
use std::sync::Mutex;

use uo_ffi::pipeline::{UoErrorInfo, code, stage, uo_last_error, uo_last_error_clear};
use uo_ffi::{
    JOB_COMPRESS, UoBuf, UoConverter, UoConverterOptions, UoPdfJob, uo_buf_free, uo_buf_len, uo_buf_ptr, uo_converter_free,
    uo_converter_new, uo_docx_to_pdf,
};
use uo_opc::flate;
use uo_testkit::generate::{RawZipEntry, RawZipOptions, build_zip};

const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#;
const CT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const P: &str = "<w:p><w:r><w:t>x</w:t></w:r></w:p>";

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn core() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hostile_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/hostile")
}

fn open_dirs() -> Vec<PathBuf> {
    let repo = core().join("..");
    vec![repo.join("shared/fonts/files"), repo.join("site/assets/fonts")]
}

// ───────────────────────────── generators ─────────────────────────────

fn document(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {NS}><w:body>{body}<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"#
    )
}

fn docx(body: &str) -> Vec<u8> {
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#;
    let mut z = uo_opc::zip::ZipWriter::new(uo_opc::zip::WriteOptions::default());
    for (name, data) in
        [("[Content_Types].xml", CT), ("_rels/.rels", RELS), ("word/document.xml", &document(body)), ("word/_rels/document.xml.rels", rels)]
    {
        z.add(name, data.as_bytes()).unwrap();
    }
    z.finish()
}

fn tbl(inner: &str) -> String {
    format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr><w:tblGrid><w:gridCol w:w="2000"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="0" w:type="auto"/></w:tcPr>{inner}{P}</w:tc></w:tr></w:tbl>"#
    )
}

fn vml_box(inner: &str) -> String {
    format!(
        r##"<w:p><w:r><w:pict><v:shape style="position:absolute;width:300pt;height:200pt" type="#_x0000_t202"><v:textbox><w:txbxContent>{inner}{P}</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"##
    )
}

fn nested_tables(n: usize) -> Vec<u8> {
    let mut s = P.to_owned();
    for _ in 0..n {
        s = tbl(&s);
    }
    docx(&format!("{s}{P}"))
}

fn nested_vml_boxes(n: usize) -> Vec<u8> {
    let mut s = P.to_owned();
    for _ in 0..n {
        s = vml_box(&s);
    }
    docx(&format!("{s}{P}"))
}

fn mixed(n: usize) -> Vec<u8> {
    let mut s = P.to_owned();
    for i in 0..n {
        s = if i % 2 == 0 { tbl(&s) } else { vml_box(&s) };
    }
    docx(&format!("{s}{P}"))
}

/// `n` runs of "ab " each with a 78.7 pt `w14:glow` of its own colour on one page: the runs do not
/// merge, so the PDF has a layer per run, every one under the per-call caps (600 runs at 200 dpi:
/// 44 s of wasm PDF, 18 MB; at the Word's 50 dpi, `fx_downscale`, see pdf_cancel_effects.rs).
fn glow_dense(n: usize) -> Vec<u8> {
    let runs: String = (0..n)
        .map(|i| {
            let c = (i as u32 + 1).wrapping_mul(2_654_435_761) & 0x00FF_FFFF;
            format!(r#"<w:r><w:rPr><w14:glow w14:rad="1000000"><w14:srgbClr w14:val="{c:06X}"/></w14:glow></w:rPr><w:t xml:space="preserve">ab </w:t></w:r>"#)
        })
        .collect();
    docx(&format!("<w:p>{runs}</w:p>"))
}

fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("nest_tables_64.docx", nested_tables(64)),
        ("nest_tables_200.docx", nested_tables(200)),
        ("nest_vml_64.docx", nested_vml_boxes(64)),
        ("nest_mixed_64.docx", mixed(64)),
        ("glow_dense_100.docx", glow_dense(100)),
        ("glow_dense_600.docx", glow_dense(600)),
    ]
}

/// The committed hostile fixture `name` (also used by the pdf_cancel_effects test).
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(hostile_dir().join(name)).unwrap()
}

#[test]
fn committed_hostile_fixtures_are_the_generator_output() {
    let bless = std::env::var("UO_BLESS").is_ok_and(|v| v == "1");
    if bless {
        std::fs::create_dir_all(hostile_dir()).unwrap();
    }
    for (name, bytes) in fixtures() {
        let path = hostile_dir().join(name);
        if bless {
            std::fs::write(&path, &bytes).unwrap();
        } else {
            let committed = std::fs::read(&path).unwrap_or_else(|_| panic!("{name} missing: run with UO_BLESS=1"));
            assert!(committed == bytes, "{name} differs from the generator: run with UO_BLESS=1");
        }
    }
}

// ───────────────────────────── the C ABI ─────────────────────────────

fn converter(trusted: bool) -> *mut UoConverter {
    let joined = open_dirs().iter().map(|d| d.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
    let o = UoConverterOptions {
        size: u32::try_from(size_of::<UoConverterOptions>()).unwrap(),
        flags: if trusted { uo_ffi::CONVERTER_TRUSTED } else { 0 },
        font_dirs: joined.as_ptr(),
        font_dirs_len: joined.len(),
    };
    let mut c = std::ptr::null_mut();
    // SAFETY: valid options for the call, writable output.
    assert_eq!(unsafe { uo_converter_new(&raw const o, &raw mut c) }, code::OK);
    c
}

fn job() -> UoPdfJob {
    UoPdfJob {
        size: u32::try_from(size_of::<UoPdfJob>()).unwrap(),
        flags: JOB_COMPRESS,
        now_ms: 1_768_478_400_000.0,
        tz_offset_min: 0,
        revisions: 0,
        max_pages: 0,
        reserved: 0,
        lang: std::ptr::null(),
        lang_len: 0,
        hyph_langs: std::ptr::null(),
        hyph_langs_len: 0,
    }
}

/// `Ok(pdf)` or `Err(info)`.
fn convert(c: *const UoConverter, bytes: &[u8]) -> Result<Vec<u8>, UoErrorInfo> {
    let mut out: *mut UoBuf = std::ptr::null_mut();
    let j = job();
    // SAFETY: live converter, readable input, valid job, writable output; the buffer is freed here.
    unsafe {
        let s = uo_docx_to_pdf(c, bytes.as_ptr(), bytes.len(), &raw const j, std::ptr::null(), &raw mut out);
        if s != code::OK {
            assert!(out.is_null());
            let mut info = UoErrorInfo::default();
            assert_eq!(uo_last_error(&raw mut info), s);
            uo_last_error_clear();
            return Err(info);
        }
        let v = std::slice::from_raw_parts(uo_buf_ptr(out), uo_buf_len(out)).to_vec();
        uo_buf_free(out);
        Ok(v)
    }
}

#[test]
fn nested_documents_convert_on_a_1_mib_caller_thread() {
    let _serial = serial();
    let names = ["nest_tables_64.docx", "nest_vml_64.docx", "nest_mixed_64.docx", "nest_tables_200.docx"];
    let docs: Vec<(&str, Vec<u8>)> = names.iter().map(|n| (*n, fixture(n))).collect();
    // the Windows main thread has 1 MiB: before the worker thread, 56 nested tables aborted the process
    let h = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let c = converter(false);
            let mut report = Vec::new();
            for (name, bytes) in &docs {
                let t = std::time::Instant::now();
                match convert(c, bytes) {
                    Ok(pdf) => {
                        assert!(pdf.starts_with(b"%PDF-"), "{name}");
                        report.push(format!("{name}: ok, {} B pdf in {:?}", pdf.len(), t.elapsed()));
                    }
                    Err(info) => panic!("{name}: code {} stage {}", info.code, info.stage),
                }
            }
            // SAFETY: a live converter, freed once.
            unsafe { uo_converter_free(c) };
            report
        })
        .unwrap();
    for line in h.join().unwrap() {
        println!("1 MiB caller thread: {line}");
    }
}

/// A deflated zero-filled entry, compressed once and reused.
struct Bomb {
    comp: Vec<u8>,
    crc: u32,
    size: u32,
}

impl Bomb {
    fn of(mib: u32) -> Bomb {
        let zeros = vec![0u8; (mib as usize) << 20];
        Bomb { comp: flate::deflate_raw(&zeros, 1), crc: flate::crc32(&zeros), size: mib << 20 }
    }

    fn entry(&self, name: &str) -> RawZipEntry {
        RawZipEntry { crc32: self.crc, ..RawZipEntry::deflated(name, self.comp.clone(), self.size) }
    }
}

fn small(name: &str, text: &str) -> RawZipEntry {
    RawZipEntry { crc32: flate::crc32(text.as_bytes()), ..RawZipEntry::stored(name, text.as_bytes()) }
}

/// A document with `n` inline pictures `rIdI<i>` → `word/media/i<i>.png`, each part a bomb entry.
fn picture_bomb(bomb: &Bomb, n: usize) -> Vec<u8> {
    let mut body = String::new();
    let mut rels = String::new();
    for i in 1..=n {
        body.push_str(&format!(
            r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="{i}" name="p{i}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{i}" name="p{i}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdI{i}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"/></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rIdI{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/i{i}.png"/>"#
        ));
    }
    let doc_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let mut entries = vec![
        small("[Content_Types].xml", CT),
        small("_rels/.rels", RELS),
        small("word/document.xml", &document(&body)),
        small("word/_rels/document.xml.rels", &doc_rels),
    ];
    entries.extend((1..=n).map(|i| bomb.entry(&format!("word/media/i{i}.png"))));
    build_zip(&entries, &RawZipOptions::default())
}

#[cfg_attr(debug_assertions, ignore = "inflates up to 512 MiB: release only (CI server-pdf job)")]
#[test]
fn a_document_inflating_past_512_mib_is_limit_exceeded_untrusted() {
    let _serial = serial();
    // three parts of 200 MiB: each under the 256 MiB part cap, 600 MiB together over the 512 MiB total
    let bomb = Bomb::of(200);
    let bytes = picture_bomb(&bomb, 3);
    println!("bomb: {} B document inflates to {} MiB", bytes.len(), 3 * 200);
    drop(bomb);
    let c = converter(false);
    let info = convert(c, &bytes).expect_err("must be refused");
    // SAFETY: a live converter, freed once.
    unsafe { uo_converter_free(c) };
    assert_eq!((info.code, info.stage, info.max), (code::LIMIT_EXCEEDED, stage::READ, 512 << 20));
}

#[cfg_attr(debug_assertions, ignore = "inflates up to 512 MiB: release only (CI server-pdf job)")]
#[test]
fn a_part_over_256_mib_is_limit_exceeded_untrusted() {
    let _serial = serial();
    let bomb = Bomb::of(257);
    let bytes = picture_bomb(&bomb, 1);
    println!("bomb: {} B document, one part of 257 MiB", bytes.len());
    drop(bomb);
    let c = converter(false);
    let info = convert(c, &bytes).expect_err("must be refused");
    // SAFETY: a live converter, freed once.
    unsafe { uo_converter_free(c) };
    assert_eq!((info.code, info.stage, info.max), (code::LIMIT_EXCEEDED, stage::READ, 256 << 20));
}
