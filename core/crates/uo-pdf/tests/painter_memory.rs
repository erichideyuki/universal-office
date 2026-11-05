//! The memory gate of the painter, measured with `uo-alloc` (record "Milestone 1 — frontier P"):
//!
//! - M1: `render_pdf` returns the live bytes exactly to their starting value once its output is dropped,
//!   over the specs of the parity capture (both modes), on every limit and error path, and over the
//!   hostile pages of the no-panic corpus;
//! - M2: a soak of `render_pdf` over the specs keeps the live bytes flat, at 0 B above the start;
//! - M3: the working set of a render. With a sink that drops what it receives, the peak does not grow
//!   with the number of images (they are decoded, embedded and forgotten one at a time) nor with the
//!   size of the file; page contents wait compressed until the catalog (record PQ4).
//! - M5: there is no cross-call cache (`clear_caches` is a no-op); the per-call maps are bounded by the
//!   input and returned with it (covered by M1).
//!
//! `cargo test --release -p uo-pdf --test painter_memory -- --nocapture --test-threads=1 --include-ignored` prints
//! the numbers of the module record. `UO_PDF_SOAK` overrides the 200 rounds.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use support::hostile::{World, hostile_nav, hostile_page, png_seeded, text_pages, world};
use support::pages::{Loaded, Sources, load_text};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_pdf::{ChunkedSink, PdfError, PdfLimits, PdfOptions, clear_caches, render_pdf, render_pdf_to};
use uo_render::paint::PaintLimits;
use uo_render::types::{ImageItem, Item, PictureLook, PresetSpec, ShadowKind};
use uo_render::{ImageId, ImageRes, ItemList, OptF64, Page, Resources};
use uo_testkit::Capture;
use uo_testkit::generate::Rng;

fn specs(n: usize) -> Vec<Loaded> {
    let cap = Capture::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity/painter.cap")).unwrap();
    cap.records().iter().take(n).map(|r| load_text(cap.get(&r.key, "spec").unwrap(), &Sources::open_fonts())).collect()
}

/// Renders every spec once, so the outline caches of the faces are warm (they belong to the faces, not to a render).
fn warm(list: &[Loaded]) {
    for s in list {
        for compress in [false, true] {
            render_pdf(&s.pages, &s.res, s.nav.as_ref(), &PdfOptions { compress, ..PdfOptions::default() }).unwrap();
        }
    }
}

#[test]
fn m1_every_spec_in_both_modes() {
    clear_caches();
    let list = specs(24);
    warm(&list);
    for (i, s) in list.iter().enumerate() {
        for compress in [false, true] {
            assert_no_leak(|| {
                render_pdf(&s.pages, &s.res, s.nav.as_ref(), &PdfOptions { compress, ..PdfOptions::default() })
                    .map(|v| v.len())
                    .map_err(|e| (i, e))
            });
        }
    }
    println!("M1 over {} specs x 2 modes: live bytes back to the start every time", list.len());
}

#[test]
fn m1_limits_and_error_paths() {
    let w = world();
    let pages = text_pages(&w, 4, 30, 6);
    let mut r = Rng::new(3);
    let hostile: Vec<Page> = (0..30).map(|_| hostile_page(&w, &mut r, true)).collect();
    // the faces' outline caches
    render_pdf(&pages, &w.res, None, &PdfOptions::default()).unwrap();
    for p in &hostile {
        let _ = render_pdf(std::slice::from_ref(p), &w.res, None, &PdfOptions::default());
    }
    // each limit of the writer
    for (name, limits) in [
        ("ops", PdfLimits { max_ops_per_page: 10, ..PdfLimits::NATIVE }),
        ("content", PdfLimits { max_content_bytes: 200, ..PdfLimits::NATIVE }),
        ("output", PdfLimits { max_output_bytes: 2000, ..PdfLimits::NATIVE }),
    ] {
        let o = PdfOptions { limits, ..PdfOptions::default() };
        assert_no_leak(|| {
            let e = render_pdf(&pages, &w.res, None, &o).err();
            assert!(matches!(e, Some(PdfError::Limit(_))), "{name}: {e:?}");
        });
    }
    // the cancellation flag, raised before and a sink that fails
    let flag = Arc::new(AtomicBool::new(true));
    let o = PdfOptions { cancel: Some(flag), ..PdfOptions::default() };
    assert_no_leak(|| assert_eq!(render_pdf(&pages, &w.res, None, &o).err(), Some(PdfError::Cancelled)));
    struct Broken;
    impl uo_pdf::PdfSink for Broken {
        fn write(&mut self, _: &[u8]) -> Result<(), PdfError> {
            Err(PdfError::Sink)
        }
    }
    assert_no_leak(|| assert_eq!(render_pdf_to(&pages, &w.res, None, &PdfOptions::default(), Broken).err(), Some(PdfError::Sink)));
    // tight helper limits: the pictures that cannot be made are left out
    let o = PdfOptions { paint: PaintLimits { max_raster_px: 100, max_tiles: 10, ..PaintLimits::DEFAULT }, ..PdfOptions::default() };
    for p in &hostile {
        let _ = render_pdf(std::slice::from_ref(p), &w.res, None, &o);
    }
    for (i, p) in hostile.iter().enumerate() {
        let nav = hostile_nav(&mut Rng::new(u32::try_from(i).unwrap() + 1), 1);
        assert_no_leak(|| render_pdf(std::slice::from_ref(p), &w.res, Some(&nav), &PdfOptions::default()).map(|v| v.len()).ok());
    }
    println!(
        "M1 on the limits (ops, content, output), the cancellation flag, a failing sink, tight helper limits and {} hostile pages: no leak",
        hostile.len()
    );
}

#[test]
fn m2_soak_is_flat() {
    let list = specs(20);
    warm(&list);
    let rounds: usize = std::env::var("UO_PDF_SOAK").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let series = soak(rounds, 20, |i| {
        let s = &list[i % list.len()];
        let bytes = render_pdf(&s.pages, &s.res, s.nav.as_ref(), &PdfOptions { compress: i % 2 == 0, ..PdfOptions::default() }).unwrap();
        std::hint::black_box(bytes.len());
    });
    println!("M2 soak, {rounds} renders over {} specs: live bytes {series:?}", list.len());
    assert_flat(&series, 1, 0);
}

fn page_with_images(first: usize, n: usize) -> Page {
    let mut list = ItemList::new();
    for k in 0..n {
        list.push_plain(Item::Image(Box::new(ImageItem {
            x: 1000.0 + (k % 8) as f64 * 150.0,
            y: 1000.0 + (k / 8) as f64 * 150.0,
            w: 140.0,
            h: 140.0,
            image: ImageId::from_index(first + k).unwrap(),
            crop: None,
            rot: OptF64::NONE,
            flip_h: false,
            flip_v: false,
            pic: None,
        })));
    }
    Page { w: 12240.0, h: 15840.0, list, ..Page::default() }
}

fn image_world(n: usize, side: u32) -> (Resources, Vec<Page>) {
    let mut res = Resources::new();
    let ext = res.symbols.intern_str("png").unwrap();
    for i in 0..n {
        res.add_image(ImageRes {
            bytes: Arc::from(png_seeded(side, side, u32::try_from(i).unwrap() * 7 + 1)),
            ext,
            svg: None,
            effects: None,
            width_twips: 0.0,
            height_twips: 0.0,
            natural_px: None,
        })
        .unwrap();
    }
    let pages = (0..n.div_ceil(64)).map(|p| page_with_images(p * 64, (n - p * 64).min(64))).collect();
    (res, pages)
}

/// Bytes allocated and peak above the start of a render into a sink that drops the file.
fn dropping(pages: &[Page], res: &Resources, compress: bool) -> (usize, u64, u64) {
    let (len, u) = measure(|| {
        let n = std::cell::Cell::new(0usize);
        let sink = ChunkedSink::new(1 << 20, |c: Vec<u8>| {
            n.set(n.get() + c.len());
            Ok(())
        });
        let sink = render_pdf_to(pages, res, None, &PdfOptions { compress, ..PdfOptions::default() }, sink).unwrap();
        let _ = sink.finish().unwrap();
        n.get()
    });
    (len, u.peak_above_start.max(0) as u64, u.allocated)
}

#[test]
fn m3_streaming_images_and_pages() {
    // images: 256x256 noise, 2 to 128 of them (the file grows to 33 MB; the peak stays the working set of one image)
    println!("M3 images (256x256 noise, chunked sink that drops the file):");
    for n in [2usize, 8, 32, 128] {
        let (res, pages) = image_world(n, 256);
        let (len, peak, alloc) = dropping(&pages, &res, true);
        println!(
            "  {n:>3} images: file {:>6.1} MB, peak {:>6.2} MB, allocated {:>7.1} MB (input PNG bytes {:.1} MB)",
            len as f64 / 1e6,
            peak as f64 / 1e6,
            alloc as f64 / 1e6,
            (n * 256 * 256 * 4) as f64 / 1e6
        );
    }
    // text: 10/100/500 pages of 40 lines of 8 words
    let w = world();
    println!("M3 text pages (40 lines x 8 words, 4 faces):");
    for n in [10usize, 100, 500] {
        let pages = text_pages(&w, n, 40, 8);
        render_pdf(&pages[..1], &w.res, None, &PdfOptions::default()).unwrap();
        let (len, peak, alloc) = dropping(&pages, &w.res, true);
        let (_, vpeak) = {
            let (l, u) = measure(|| render_pdf(&pages, &w.res, None, &PdfOptions::default()).unwrap().len());
            (l, u.peak_above_start.max(0) as u64)
        };
        println!(
            "  {n:>3} pages: file {:>6.2} MB, peak {:>5.2} MB (dropping sink), {:>5.2} MB (Vec sink), allocated {:>6.1} MB",
            len as f64 / 1e6,
            peak as f64 / 1e6,
            vpeak as f64 / 1e6,
            alloc as f64 / 1e6
        );
    }
}

/// The picture with every effect at the size where the layer is capped at 8 Mpx.
#[test]
fn m3_picture_effects() {
    let mut w = world();
    let sym = |w: &mut World, s: &str| w.res.symbols.intern_str(s).unwrap();
    let (blue, rect) = (sym(&mut w, "0000FF"), sym(&mut w, "roundRect"));
    let shadow = uo_render::types::PicShadow {
        kind: ShadowKind::Outer,
        color: blue,
        alpha: 0.5,
        blur_pt: 6.0,
        dist_pt: 5.0,
        dir_deg: 45.0,
        sx: OptF64::NONE,
        sy: OptF64::NONE,
        kx_deg: OptF64::NONE,
        ky_deg: OptF64::NONE,
        algn: None,
        rot_with_shape: false,
    };
    let look = PictureLook {
        geom: Some(PresetSpec { prst: rect, adj: None }),
        shadow: Some(shadow),
        glow: Some(uo_render::types::PicGlow { color: blue, alpha: 0.4, rad_pt: 10.0 }),
        soft_edge_pt: OptF64::some(4.0),
        ..PictureLook::default()
    };
    let mut list = ItemList::new();
    list.push_plain(Item::Image(Box::new(ImageItem {
        x: 1000.0,
        y: 1000.0,
        w: 9000.0,
        h: 6000.0,
        image: ImageId::from_index(1).unwrap(),
        crop: None,
        rot: OptF64::NONE,
        flip_h: false,
        flip_v: false,
        pic: Some(Box::new(look)),
    })));
    let pages = vec![Page { w: 12240.0, h: 15840.0, list, ..Page::default() }];
    render_pdf(&pages, &w.res, None, &PdfOptions::default()).unwrap();
    let (len, u) = measure(|| render_pdf(&pages, &w.res, None, &PdfOptions::default()).unwrap().len());
    println!(
        "M3 picture 450x300 pt with shadow, glow, soft edges (layers capped at 8 Mpx): file {:.2} MB, peak {:.1} MB, allocated {:.1} MB",
        len as f64 / 1e6,
        u.peak_above_start as f64 / 1e6,
        u.allocated as f64 / 1e6
    );
    assert!(u.peak_above_start < 600 << 20);
}

/// A 12 Mpx image (4000x3000): the working set of one decode + embed.
#[test]
#[ignore = "builds a 48 MB PNG; run with --include-ignored"]
fn m3_one_big_image() {
    let (res, pages) = image_world(1, 3464);
    let (len, peak, alloc) = dropping(&pages, &res, true);
    println!(
        "M3 one 12 Mpx image: file {:.1} MB, peak {:.1} MB ({:.2} N), allocated {:.1} MB",
        len as f64 / 1e6,
        peak as f64 / 1e6,
        peak as f64 / (3464.0 * 3464.0),
        alloc as f64 / 1e6
    );
}
