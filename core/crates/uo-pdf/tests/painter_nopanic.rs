//! No-panic loops of the painter (record "Milestone 1 — frontier P", §8): random display lists built from
//! hostile values (NaN, infinities, subnormals, 1e300, ids of resources that do not exist, faces
//! without bytes or without a parsed font, garbage and declared-huge images, crops over 100 %, tiny
//! tiles, tiny limits, failing sinks, the cancellation flag) must end in `Ok` with a valid file or in
//! an `Err`, never in a panic or a hang.
//!
//! `UO_PDF_FUZZ` sets the number of documents (default 200).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use support::hostile::{hostile_nav, hostile_page, text_pages, world};
use support::pdf_parse::parse_pdf;
use uo_image::ImageLimits;
use uo_pdf::{PdfError, PdfLimits, PdfOptions, PdfSink, render_pdf, render_pdf_to};
use uo_render::paint::PaintLimits;
use uo_render::types::{Item, PageFill, PageFillKind};
use uo_render::{ImageId, ItemList, Page, PageFillId};
use uo_testkit::generate::Rng;

struct FailAfter(usize);

impl PdfSink for FailAfter {
    fn write(&mut self, bytes: &[u8]) -> Result<(), PdfError> {
        if bytes.len() > self.0 {
            return Err(PdfError::Sink);
        }
        self.0 -= bytes.len();
        Ok(())
    }
}

#[test]
fn random_hostile_pages_never_panic() {
    let n: usize = std::env::var("UO_PDF_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let w = world();
    let (mut ok, mut limit, mut sink, mut invalid, mut cancelled) = (0, 0, 0, 0, 0);
    for i in 0..n {
        let mut r = Rng::new(0x9e37_79b9u32.wrapping_mul(u32::try_from(i).unwrap() + 1));
        let pages: Vec<Page> = (0..1 + r.below(3)).map(|_| hostile_page(&w, &mut r, true)).collect();
        let nav = if r.one_in(2) { Some(hostile_nav(&mut r, pages.len())) } else { None };
        // (a blur radius of thousands of points on a 8 Mpx layer is bounded by the kernel limit but takes seconds: 17 s measured
        // at the default 4,096; the CI loop uses 64)
        let mut o = PdfOptions { compress: r.one_in(2), ..PdfOptions::default() };
        o.paint.draw.max_kernel_radius = 64;
        if r.one_in(4) {
            o.limits = PdfLimits {
                max_ops_per_page: r.below(60) as u64,
                max_content_bytes: r.below(5000) as u64,
                max_output_bytes: r.below(60_000) as u64 + 100,
                ..PdfLimits::NATIVE
            };
        }
        if r.one_in(5) {
            o.paint = PaintLimits {
                max_raster_px: r.below(5000) as u64,
                max_tiles: r.below(50) as u64,
                max_samples: 300,
                draw: o.paint.draw,
                ..PaintLimits::DEFAULT
            };
            o.image = ImageLimits { max_px: r.below(200) as u64, ..ImageLimits::NATIVE };
        }
        if r.one_in(12) {
            let flag = Arc::new(AtomicBool::new(false));
            flag.store(true, Ordering::Relaxed);
            o.cancel = Some(flag);
        }
        let started = std::time::Instant::now();
        let result = if r.one_in(6) {
            render_pdf_to(&pages, &w.res, nav.as_ref(), &o, FailAfter(r.below(4000))).map(|_| Vec::new())
        } else {
            render_pdf(&pages, &w.res, nav.as_ref(), &o)
        };
        if started.elapsed().as_millis() > 1500 {
            println!("document {i} took {:?}", started.elapsed());
            if std::env::var("UO_PDF_BISECT").is_ok() {
                for pg in &pages {
                    for (k, it) in pg.list.items.iter().enumerate() {
                        let mut one = Page { w: pg.w, h: pg.h, ..Page::default() };
                        one.list.glyphs = pg.list.glyphs.clone();
                        one.list.runs = pg.list.runs.clone();
                        one.list.path_cmds = pg.list.path_cmds.clone();
                        one.list.items.push(it.clone());
                        let t0 = std::time::Instant::now();
                        let _ = render_pdf(&[one], &w.res, None, &PdfOptions::default());
                        if t0.elapsed().as_millis() > 500 {
                            println!("  item {k} {:?}: {:?}", t0.elapsed(), it);
                        }
                    }
                }
            }
        }
        match result {
            Ok(bytes) => {
                ok += 1;
                if !bytes.is_empty() {
                    parse_pdf(&bytes).unwrap_or_else(|e| panic!("document {i}: the file is not valid: {e}"));
                }
            }
            Err(PdfError::Limit(_)) => limit += 1,
            Err(PdfError::Sink) => sink += 1,
            Err(PdfError::Cancelled) => cancelled += 1,
            Err(PdfError::Invalid(_)) => invalid += 1,
        }
    }
    println!(
        "{n} hostile documents: {ok} written (the files parse), {limit} stopped by a limit, {sink} by the sink, {cancelled} cancelled, {invalid} refused as invalid"
    );
}

/// A page-fill tile of a thousandth of a point over a page of 1000 pt: millions of draws in the TS
/// (it hangs), the operations limit here.
#[test]
fn a_tiny_page_fill_tile_hits_the_operations_limit() {
    let mut w = world();
    let fill = PageFill {
        kind: PageFillKind::Tile,
        c1: w.syms[0],
        c2: w.syms[1],
        stops: None,
        angle: 0.0,
        focus: 0.0,
        focus_position: None,
        gradient_center: false,
        image: ImageId::from_index(0),
        // 1/1000 of a pixel at 96 dpi
        image_px: Some([0.001, 0.001, 96.0]),
        pattern: None,
    };
    let id = w.res.add_page_fill(fill).unwrap();
    let pages =
        vec![Page { w: 20000.0, h: 20000.0, list: ItemList::new(), background: None, background_fill: Some(id), background_show: true }];
    let o = PdfOptions { limits: PdfLimits { max_ops_per_page: 100_000, ..PdfLimits::NATIVE }, ..PdfOptions::default() };
    assert_eq!(render_pdf(&pages, &w.res, None, &o).err(), Some(PdfError::Limit(uo_pdf::LimitKind::Ops)));
    // the same tile through the default limits ends too (2 million draws)
    let t = std::time::Instant::now();
    assert_eq!(render_pdf(&pages, &w.res, None, &PdfOptions::default()).err(), Some(PdfError::Limit(uo_pdf::LimitKind::Ops)));
    println!("2,000,000 tile draws up to the operations limit: {:?}", t.elapsed());
    let _ = PageFillId::from_index(0);
}

#[test]
fn the_cancellation_flag_stops_the_render() {
    let w = world();
    let pages = text_pages(&w, 30, 40, 8);
    let flag = Arc::new(AtomicBool::new(false));
    let o = PdfOptions { cancel: Some(Arc::clone(&flag)), ..PdfOptions::default() };
    let ok = render_pdf(&pages, &w.res, None, &o).unwrap();
    parse_pdf(&ok).unwrap();
    flag.store(true, Ordering::Relaxed);
    assert_eq!(render_pdf(&pages, &w.res, None, &o).err(), Some(PdfError::Cancelled));
    // raised from another thread in the middle of a long render: the render ends with `Cancelled`
    // (or, on a very fast machine, finishes first)
    let flag2 = Arc::new(AtomicBool::new(false));
    let o2 = PdfOptions { cancel: Some(Arc::clone(&flag2)), ..PdfOptions::default() };
    let many = text_pages(&w, 400, 40, 8);
    let f = Arc::clone(&flag2);
    let h = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        f.store(true, Ordering::Relaxed);
    });
    let t = std::time::Instant::now();
    let r = render_pdf(&many, &w.res, None, &o2);
    h.join().unwrap();
    println!("cancelled after 30 ms: {:?} in {:?}", r.as_ref().map(Vec::len), t.elapsed());
    assert!(matches!(r, Err(PdfError::Cancelled) | Ok(_)));
}

#[test]
fn missing_resources_and_empty_pages_are_tolerated() {
    let w = world();
    // no pages, a page with no items, an outline with no pages
    let nav = uo_render::Navigation {
        links: vec![],
        outline: vec![uo_render::OutlineEntry { title: [0x41u16].into(), level: 1, page_index: 9, top_pt: 0.0 }],
    };
    let none = render_pdf(&[], &w.res, Some(&nav), &PdfOptions::default()).unwrap();
    parse_pdf(&none).unwrap();
    let empty = Page { w: 100.0, h: 100.0, ..Page::default() };
    parse_pdf(&render_pdf(&[empty], &uo_render::Resources::new(), None, &PdfOptions::default()).unwrap()).unwrap();
    // items that name resources an empty table does not have
    let mut r = Rng::new(5);
    let pages: Vec<Page> = (0..20).map(|_| hostile_page(&w, &mut r, true)).collect();
    let bare = uo_render::Resources::new();
    // (symbols of another table read as empty strings)
    for p in &pages {
        let _ = render_pdf(std::slice::from_ref(p), &bare, None, &PdfOptions::default());
    }
    let _ = Item::ClipPop;
}
