//! The memory gate of the writer layer, measured with `uo-alloc`.
//!
//! - M1: every public operation, on success and on each error path, returns live bytes exactly to
//!   their starting value once its outputs are dropped (also over every script of the parity
//!   capture, in both modes);
//! - M2: 1,000 documents (the scripts of the capture, in turn) keep the live bytes flat, at 0 B
//!   above the start;
//! - streaming (record PQ4): with a sink that drops what it receives, the peak of a render does
//!   not grow with the size of the file (images are written as they are made); with a `Vec` sink it
//!   does; page contents are held compressed until `build` and are what the peak grows with.
//!
//! `cargo test --release -p uo-pdf --test writer_memory -- --nocapture --test-threads=1` prints the
//! numbers of the module record. `UO_PDF_SOAK` overrides the 1,000 rounds; the 12 Mpx image test is
//! `#[ignore]`d (`-- --ignored`).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::path::Path;
use std::sync::Arc;

use support::json::{Json, parse};
use support::script::{fonts, run_script};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_pdf::{
    ChunkedSink, GlyphOptions, JpegImage, PathCmd, PdfDocument, PdfError, PdfLimits, PdfLink, PdfOutlineItem, PdfSink, PdfWriter,
    PlacedGlyph, ShadingGeom, SoftMask, Stroke, clear_caches, fmt, hex_color, safe_pdf_uri, utf16be_hex,
};
use uo_testkit::Capture;
use uo_testkit::generate::Rng;

fn scripts() -> Vec<Json> {
    let cap = Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/writer.cap")).unwrap();
    cap.records().iter().filter(|r| r.key.starts_with("s0")).map(|r| parse(cap.get(&r.key, "script").unwrap())).collect()
}

const RED: [f64; 3] = [1.0, 0.0, 0.0];

#[test]
fn m1_scalars() {
    clear_caches();
    assert_no_leak(|| fmt(123.4567));
    assert_no_leak(|| hex_color("#FF8800"));
    assert_no_leak(|| safe_pdf_uri(Some("https://a b/(c)")));
    assert_no_leak(|| safe_pdf_uri(Some("javascript:x")));
    assert_no_leak(|| utf16be_hex("title \u{1F600}"));
}

#[test]
fn m1_writer_and_sinks() {
    assert_no_leak(|| {
        let mut w = PdfWriter::new(Vec::new(), true);
        let (a, b, c) = (w.alloc().unwrap(), w.alloc().unwrap(), w.alloc().unwrap());
        w.define_text(c, "<< /C 1 >>").unwrap();
        w.define_text(b, "<< /B 1 >>").unwrap();
        w.define_text(b, "<< /B 2 >>").unwrap();
        w.define_text(a, "<< /A 1 >>").unwrap();
        w.add_stream("/X 1", &[b'x'; 5000], true).unwrap();
        w.add_stream("/Y 1", &Rng::new(3).bytes(5000), true).unwrap();
        w.build(a).unwrap()
    });
    // errors
    assert_no_leak(|| {
        let mut w = PdfWriter::new(Vec::new(), true);
        let a = w.alloc().unwrap();
        w.alloc().unwrap();
        w.define_text(a, "<< >>").unwrap();
        (w.define_text(a, "<< >>").err(), w.define_text(77, "<< >>").err(), w.build(a).err())
    });
    assert_no_leak(|| {
        let limits = PdfLimits { max_output_bytes: 500, ..PdfLimits::NATIVE };
        let mut w = PdfWriter::with_limits(Vec::new(), false, &limits);
        let (_a, b) = (w.alloc().unwrap(), w.alloc().unwrap());
        (w.define(b, &[0; 900]).err(), w.add_stream("/Z 1", &[0; 900], true).err())
    });
    struct Broken;
    impl PdfSink for Broken {
        fn write(&mut self, _: &[u8]) -> Result<(), PdfError> {
            Err(PdfError::Sink)
        }
    }
    assert_no_leak(|| {
        let mut w = PdfWriter::new(Broken, true);
        let a = w.alloc().unwrap();
        w.define_text(a, "<< >>").err()
    });
    // the chunked sink
    assert_no_leak(|| {
        let mut count = 0usize;
        let mut d = PdfDocument::new(
            ChunkedSink::new(4096, |c: Vec<u8>| {
                count += c.len();
                Ok(())
            }),
            true,
        );
        for i in 0..6u8 {
            d.embed_rgba(&Rng::new(u32::from(i) + 1).bytes(64 * 64 * 4), 64, 64).unwrap();
        }
        let _ = d.build().unwrap().finish().unwrap();
        count
    });
}

#[test]
fn m1_document_operations() {
    let f = fonts();
    for compress in [false, true] {
        // every method, with fonts, images, shadings, links, outline and opacity
        assert_no_leak(|| {
            let mut d = PdfDocument::new(Vec::new(), compress);
            let font = d.embed_font("Carlito Test", Arc::clone(&f.bytes[0]), f.faces[0].clone());
            let program = uo_fonts::FontProgram::parse(Arc::clone(&f.bytes[1]), &uo_fonts::FontLimits::DEFAULT).unwrap();
            let font2 = d.embed_font("Program", Arc::clone(&f.bytes[1]), program);
            let img = d.embed_rgba(&Rng::new(9).bytes(40 * 40 * 4), 40, 40).unwrap();
            let opaque = d.embed_rgba(&[200; 16], 2, 2).unwrap();
            let jpeg = d.embed_jpeg(&JpegImage::new(&[1, 2, 3], 3, 1)).unwrap();
            let masked = d
                .embed_jpeg(&JpegImage {
                    decode: &[1.0, 0.0],
                    smask: Some(SoftMask { alpha: &[5; 30], width: 5, height: 6 }),
                    ..JpegImage::new(&[1], 1, 1)
                })
                .unwrap();
            let sh = d.add_shading(&ShadingGeom::Axial { x0: 0.0, y0: 0.0, x1: 5.0, y1: 5.0 }, &Rng::new(5).bytes(300), 800.0).unwrap();
            d.add_outline(PdfOutlineItem { title: "One".into(), level: 1, page_index: 0, top_pt: 5.0 });
            d.add_outline(PdfOutlineItem { title: "Two \u{1F600}".into(), level: 2, page_index: 1, top_pt: 9.0 });
            for pg in 0..3 {
                let mut p = d.add_page(612.0, 792.0);
                let glyphs: Vec<PlacedGlyph> = "Hello, memory gate"
                    .chars()
                    .enumerate()
                    .map(|(i, ch)| {
                        let cp = u32::from(ch);
                        PlacedGlyph {
                            gid: p.encode(font, cp).unwrap().gid,
                            code_point: cp,
                            x_pt: 72.0 + 7.0 * i as f64,
                            baseline_from_top_pt: 100.0,
                        }
                    })
                    .collect();
                p.glyphs(font, 12.0, &RED, &glyphs, &GlyphOptions { runs: true, ..Default::default() });
                p.glyphs(font, 12.0, &RED, &glyphs, &GlyphOptions::default());
                p.encode(font2, 0x41);
                p.fill_rect(1.0, 2.0, 3.0, 4.0, &RED);
                p.line(0.0, 0.0, 9.0, 9.0, &RED, 1.0, &[2.0]);
                p.path(
                    &[PathCmd::M { x: 0.0, y: 0.0 }, PathCmd::L { x: 5.0, y: 5.0 }, PathCmd::Z],
                    Some(&RED),
                    Some(&Stroke { color: RED, width_pt: 1.0, dash: &[] }),
                    true,
                );
                p.fill_shaded(&[PathCmd::M { x: 0.0, y: 0.0 }, PathCmd::L { x: 5.0, y: 5.0 }, PathCmd::Z], sh, false);
                p.push_clip_rect(0.0, 0.0, 10.0, 10.0);
                p.draw_image(img, 1.0, 1.0, 20.0, 20.0);
                p.pop_clip();
                p.draw_image_matrix(opaque, 1.0, 0.0, 0.0, 1.0, 5.0, 5.0);
                p.draw_image_transformed(jpeg, 10.0, 10.0, 20.0, 20.0, 33.0, true, true);
                p.draw_image_transformed(masked, 10.0, 10.0, 20.0, 20.0, 0.0, false, false);
                p.push_transform(1.0, 0.0, 0.0, 1.0, 5.0, 5.0);
                p.pop_clip();
                p.with_alpha(Some(0.5), Some(0.25), |p| p.fill_rect(0.0, 0.0, 9.0, 9.0, &RED));
                p.add_link(&PdfLink {
                    x_pt: 1.0,
                    y_top_pt: 1.0,
                    w_pt: 10.0,
                    h_pt: 10.0,
                    uri: Some("https://example.com".into()),
                    ..PdfLink::default()
                });
                p.add_link(&PdfLink { page_index: Some(pg), ..PdfLink::default() });
                if pg == 2 {
                    drop(p);
                } else {
                    p.finish().unwrap();
                }
            }
            d.build().unwrap()
        });
    }
}

#[test]
fn m1_embedding_variants() {
    assert_no_leak(|| {
        let mut d = PdfDocument::new(Vec::new(), true);
        d.embed_rgba(&[1, 2, 3, 255, 4, 5, 6, 255], 2, 1).unwrap();
        d.embed_rgba(&[1, 2, 3, 0, 4, 5, 6, 255], 2, 1).unwrap();
        d.embed_rgba(&[1, 2, 3, 0, 4, 5, 6, 255, 7, 8], 2, 1).unwrap();
        (d.embed_rgba(&[0; 8], 5, 5).err(), d.embed_rgba(&[], u32::MAX, u32::MAX).err())
    });
    assert_no_leak(|| {
        let mut d = PdfDocument::new(Vec::new(), false);
        d.embed_jpeg(&JpegImage { components: 1, ..JpegImage::new(&[0xff, 0xd8], 1, 1) }).unwrap();
        d.embed_jpeg(&JpegImage { components: 4, adobe_inverted: true, ..JpegImage::new(&[0xff, 0xd8], 1, 1) }).unwrap();
        d.add_shading(&ShadingGeom::Radial { x0: 1.0, y0: 1.0, r: 5.0 }, &[], 10.0).unwrap();
    });
}

#[test]
fn m1_limits_and_error_paths() {
    // a page over each limit, then the failing build
    assert_no_leak(|| {
        let limits = PdfLimits { max_ops_per_page: 4, ..PdfLimits::NATIVE };
        let mut d = PdfDocument::with_limits(Vec::new(), true, limits);
        let mut p = d.add_page(10.0, 10.0);
        for _ in 0..20 {
            p.fill_rect(0.0, 0.0, 1.0, 1.0, &RED);
        }
        let e = (p.failure(), p.finish());
        (e, d.build().err())
    });
    assert_no_leak(|| {
        let limits = PdfLimits { max_content_bytes: 64, ..PdfLimits::NATIVE };
        let mut d = PdfDocument::with_limits(Vec::new(), true, limits);
        let mut p = d.add_page(10.0, 10.0);
        p.path(&[PathCmd::M { x: 1.0, y: 1.0 }; 50], Some(&RED), None, false);
        drop(p);
        d.build().err()
    });
    // a code point past U+10FFFF: the build fails after the fonts were half written
    assert_no_leak(|| {
        let f = fonts();
        let mut d = PdfDocument::new(Vec::new(), true);
        let font = d.embed_font("F", Arc::clone(&f.bytes[0]), f.faces[0].clone());
        d.font_mut(font).unwrap().encode(0x11_0000);
        d.add_page(10.0, 10.0).finish().unwrap();
        d.build().err()
    });
    // the output limit in the middle of a document
    assert_no_leak(|| {
        let limits = PdfLimits { max_output_bytes: 3000, ..PdfLimits::NATIVE };
        let mut d = PdfDocument::with_limits(Vec::new(), false, limits);
        let mut last = None;
        for _ in 0..10 {
            last = d.embed_rgba(&[9; 400], 10, 10).err().or(last);
        }
        (last, d.build().err())
    });
}

#[test]
fn m1_every_script_of_the_parity_capture() {
    let list = scripts();
    fonts();
    for (i, script) in list.iter().enumerate() {
        for compress in [false, true] {
            // the first run warms nothing the writer keeps; every later one must give everything back
            assert_no_leak(|| run_script(script, compress).map(|v| v.len()).map_err(|e| (i, e)));
        }
    }
    println!("M1 over {} scripts x 2 modes: live bytes back to the start every time", list.len());
}

#[test]
fn m2_soak_documents_are_flat() {
    let list = scripts();
    fonts();
    let rounds: usize = std::env::var("UO_PDF_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
    let series = soak(rounds, 50, |i| {
        let script = &list[i % list.len()];
        drop(run_script(script, i % 2 == 0));
    });
    println!(
        "M2 soak, {rounds} documents, live bytes every 50: first {}, last {}, min {}, max {}",
        series[0],
        series[series.len() - 1],
        series.iter().min().unwrap(),
        series.iter().max().unwrap()
    );
    assert_flat(&series, 0, 0);
    assert_eq!(series[0], series[series.len() - 1]);
}

// ---- streaming ------------------------------------------------------------------------------------

/// Renders `n` noisy images of `side` x `side` pixels (about 2 x side^2 x 3 bytes in the file at
/// most), through a sink that drops what it receives, and returns (peak above start, bytes sent).
fn render_images_dropping(n: usize, side: u32) -> (i64, u64) {
    let px: Vec<u8> = Rng::new(11).bytes((side * side * 4) as usize);
    let mut sent = 0u64;
    let (_, usage) = measure(|| {
        let mut d = PdfDocument::new(
            ChunkedSink::new(1 << 20, |c: Vec<u8>| {
                sent += c.len() as u64;
                Ok(())
            }),
            true,
        );
        for _ in 0..n {
            d.embed_rgba(&px, side, side).unwrap();
        }
        let _ = d.build().unwrap().finish().unwrap();
    });
    (usage.peak_above_start, sent)
}

fn render_images_vec(n: usize, side: u32) -> (i64, u64) {
    let px: Vec<u8> = Rng::new(11).bytes((side * side * 4) as usize);
    let (out, usage) = measure(|| {
        let mut d = PdfDocument::new(Vec::new(), true);
        for _ in 0..n {
            d.embed_rgba(&px, side, side).unwrap();
        }
        d.build().unwrap()
    });
    (usage.peak_above_start, out.len() as u64)
}

#[test]
fn streaming_peak_is_independent_of_the_size_of_the_file() {
    let side = 256;
    let mut dropping = Vec::new();
    let mut vec = Vec::new();
    for n in [2usize, 8, 32, 128] {
        dropping.push((n, render_images_dropping(n, side)));
        vec.push((n, render_images_vec(n, side)));
    }
    for ((n, (peak, sent)), (_, (vpeak, vlen))) in dropping.iter().zip(&vec) {
        println!(
            "{n:>4} images of {side}x{side}: file {vlen:>9} B | chunked sink: peak {peak:>8} B (sent {sent}) | Vec sink: peak {vpeak:>9} B"
        );
    }
    let one_image = i64::from(side * side * 9);
    let (first, last) = (dropping[0].1.0, dropping[3].1.0);
    assert!(last <= first + (64 << 10), "peak with 128 images ({last}) vs 2 ({first}): it must not grow with the file");
    assert!(last < 6 * one_image + (4 << 20), "peak {last} over the working set of one image");
    let (vfirst, vlast) = (vec[0].1.0, vec[3].1.0);
    assert!(vlast > vfirst * 8, "the control: a Vec sink holds the whole file ({vfirst} -> {vlast})");
    assert!(dropping.iter().zip(&vec).all(|((_, (_, sent)), (_, (_, len)))| sent == len), "the same bytes either way");
}

/// A page of text-like content: `ops` rectangles; a document of `pages` of them.
fn render_pages(pages: usize, ops: usize, compress: bool) -> (i64, u64) {
    let mut sent = 0u64;
    let (_, usage) = measure(|| {
        let mut d = PdfDocument::new(
            ChunkedSink::new(1 << 20, |c: Vec<u8>| {
                sent += c.len() as u64;
                Ok(())
            }),
            compress,
        );
        let mut rng = Rng::new(5);
        for _ in 0..pages {
            let mut p = d.add_page(612.0, 792.0);
            for _ in 0..ops {
                p.fill_rect(rng.below(600) as f64, rng.below(780) as f64, rng.below(90) as f64 + 0.25, rng.below(30) as f64, &RED);
            }
            p.finish().unwrap();
        }
        let _ = d.build().unwrap().finish().unwrap();
    });
    (usage.peak_above_start, sent)
}

#[test]
fn page_contents_are_held_compressed_until_build() {
    println!("pages of 400 rectangles (about 12 KB of content, with a chunked sink that drops the file):");
    let mut rows = Vec::new();
    for pages in [10usize, 100, 500] {
        let (peak, sent) = render_pages(pages, 400, true);
        let (peak_raw, sent_raw) = render_pages(pages, 400, false);
        println!("{pages:>4} pages: file {sent:>9} B, peak {peak:>9} B | compress off: file {sent_raw:>9} B, peak {peak_raw:>9} B");
        rows.push((pages, peak, sent, peak_raw, sent_raw));
    }
    for (pages, peak, sent, peak_raw, sent_raw) in rows {
        // what is held until `build` is the compressed contents (and the page objects): at most the file, plus the working set of
        // one page and the compressor state
        assert!(peak <= sent as i64 + (2 << 20), "{pages} pages: peak {peak} vs file {sent}");
        assert!(peak_raw <= sent_raw as i64 + (2 << 20), "{pages} pages, stored: peak {peak_raw} vs file {sent_raw}");
    }
}

#[test]
#[ignore = "12 Mpx image: about 150 MB and a while in a debug build (run with --release -- --ignored --nocapture)"]
fn a_12_megapixel_image_peak() {
    let (w, h) = (4000u32, 3000u32);
    let px = Rng::new(3).bytes((w * h * 4) as usize);
    for (label, alpha) in [("opaque", false), ("with alpha", true)] {
        let mut data = px.clone();
        if alpha {
            data.iter_mut().skip(3).step_by(4).for_each(|a| *a = 128);
        } else {
            data.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
        }
        let mut sent = 0u64;
        let (_, usage) = measure(|| {
            let mut d = PdfDocument::new(
                ChunkedSink::new(1 << 20, |c: Vec<u8>| {
                    sent += c.len() as u64;
                    Ok(())
                }),
                true,
            );
            d.embed_rgba(&data, w, h).unwrap();
            let _ = d.build().unwrap().finish().unwrap();
        });
        let n = i64::from(w * h);
        println!(
            "12 Mpx {label}: input {} MB, peak above start {} MB ({:.2} N), allocated {} MB, file {} MB",
            n * 4 / 1_000_000,
            usage.peak_above_start / 1_000_000,
            usage.peak_above_start as f64 / n as f64,
            usage.allocated / 1_000_000,
            sent / 1_000_000
        );
        assert!(usage.peak_above_start <= n * 9 + (8 << 20), "{label}: peak over 9N + 8 MiB");
    }
}
