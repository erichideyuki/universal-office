//! No-panic loops of the writer layer (record §8): random documents built from hostile values
//! (NaN, infinities, subnormals, 1e300, `i64::MIN`, lone mismatches between handles of different
//! documents, garbage font and image bytes, tiny limits, failing sinks) must end in `Ok` with a
//! valid file or in an `Err`, never in a panic; and the stress shapes of the plan (500 pages,
//! 10,000 links, a 5,000-entry outline, a chain of 20,000 levels).
//!
//! `UO_PDF_FUZZ` sets the number of documents (default 3,000).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod support;

use std::sync::Arc;

use support::pdf_parse::parse_pdf;
use support::script::fonts;
use uo_pdf::{
    ChunkedSink, FontId, FontProgram, GlyphOptions, JpegImage, PathCmd, PdfDocument, PdfError, PdfImage, PdfLimits, PdfLink,
    PdfOutlineItem, PdfShading, PdfSink, PlacedGlyph, ShadingGeom, SoftMask, Stroke,
};
use uo_testkit::generate::Rng;

fn hostile(r: &mut Rng) -> f64 {
    match r.below(16) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => -0.0,
        4 => 5e-324,
        5 => 1e300,
        6 => -1e300,
        7 => 1e9,
        8 => f64::from(r.next_u32()) * 1e6,
        9 => f64::from_bits(u64::from(r.next_u32()) << 32 | u64::from(r.next_u32())),
        _ => f64::from(r.next_u32()) / 4_294_967_296.0 * 1000.0 - 200.0,
    }
}

struct Wild {
    upem: f64,
    seed: u32,
}

impl FontProgram for Wild {
    fn units_per_em(&self) -> f64 {
        self.upem
    }

    fn glyph_id(&self, cp: u32) -> u32 {
        cp.wrapping_mul(self.seed | 1) >> (self.seed % 24)
    }

    fn advance_width(&self, gid: u32) -> f64 {
        f64::from(gid % 3000) * if self.seed.is_multiple_of(5) { -1.0 } else { 1.0 }
    }

    fn font_bbox(&self) -> Option<[f64; 4]> {
        (self.seed.is_multiple_of(3)).then_some([f64::NAN, -1e300, 5.0, f64::INFINITY])
    }

    fn italic_angle(&self) -> Option<f64> {
        (self.seed.is_multiple_of(7)).then_some(f64::NAN)
    }
}

struct FailAfter {
    left: usize,
    sent: usize,
}

impl PdfSink for FailAfter {
    fn write(&mut self, bytes: &[u8]) -> Result<(), PdfError> {
        if bytes.len() > self.left {
            return Err(PdfError::Sink);
        }
        self.left -= bytes.len();
        self.sent += bytes.len();
        Ok(())
    }
}

fn blob(r: &mut Rng, max: usize) -> Vec<u8> {
    let n = r.below(max);
    r.bytes(n)
}

fn text(r: &mut Rng) -> String {
    const PIECES: [&str; 12] = ["a", "(", ")", "\\", " ", "\u{e9}", "\u{1F600}", "\u{0}", "\n", "http://", "\u{feff}", "<>"];
    (0..r.below(8)).map(|_| PIECES[r.below(PIECES.len())]).collect()
}

fn garbage_font(r: &mut Rng, real: &[u8]) -> Arc<[u8]> {
    match r.below(4) {
        0 => Arc::from(blob(r, 200)),
        1 => {
            let mut b = real.to_vec();
            for _ in 0..1 + r.below(8) {
                let at = r.below(b.len());
                b[at] = r.next_u32() as u8;
            }
            Arc::from(b)
        }
        2 => Arc::from(&real[..r.below(real.len())]),
        _ => Arc::from(real),
    }
}

fn build_one(seed: u32) -> (Result<usize, PdfError>, bool) {
    let mut r = Rng::new(seed);
    let f = fonts();
    let limits = if r.below(4) == 0 {
        PdfLimits {
            max_ops_per_page: r.below(40) as u64,
            max_content_bytes: r.below(3000) as u64,
            max_output_bytes: r.below(30_000) as u64 + 100,
            ..PdfLimits::NATIVE
        }
    } else {
        PdfLimits::NATIVE
    };
    let compress = r.below(2) == 0;
    let fail_at = if r.below(5) == 0 { r.below(4000) } else { usize::MAX };
    let mut d = PdfDocument::with_limits(FailAfter { left: fail_at, sent: 0 }, compress, limits);
    let mut other = PdfDocument::new(Vec::new(), false);
    let stray_font: FontId = other.embed_font("x", Arc::from(vec![1u8]), Wild { upem: 1000.0, seed: 1 });
    let mut fonts_made: Vec<FontId> = Vec::new();
    let mut images: Vec<PdfImage> = Vec::new();
    let mut shadings: Vec<PdfShading> = Vec::new();
    let mut ok = true;
    macro_rules! step {
        ($e:expr) => {
            match $e {
                Ok(v) => Some(v),
                Err(_) => {
                    ok = false;
                    None
                }
            }
        };
    }
    for _ in 0..1 + r.below(12) {
        match r.below(12) {
            0 | 1 => {
                let which = r.below(f.bytes.len());
                let bytes = garbage_font(&mut r, &f.bytes[which]);
                let id = if r.below(3) == 0 {
                    d.embed_font(&text(&mut r), bytes, Wild { upem: hostile(&mut r), seed: r.next_u32() })
                } else {
                    d.embed_font(&text(&mut r), bytes, f.faces[which].clone())
                };
                fonts_made.push(id);
            }
            2 => {
                let (w, h) = (r.below(40) as u32, r.below(40) as u32);
                let len = if r.below(3) == 0 { r.below(5000) } else { (w * h * 4) as usize + r.below(7) };
                if let Some(i) = step!(d.embed_rgba(&r.bytes(len), w, h)) {
                    images.push(i);
                }
            }
            3 => {
                let bytes = blob(&mut r, 300);
                let decode: Vec<f64> = (0..r.below(9)).map(|_| hostile(&mut r)).collect();
                let mask = blob(&mut r, 100);
                let jpeg = JpegImage {
                    bytes: &bytes,
                    width_px: r.next_u32(),
                    height_px: r.next_u32(),
                    components: r.below(7) as u32,
                    adobe_inverted: r.below(2) == 0,
                    decode: &decode,
                    smask: (r.below(2) == 0).then(|| SoftMask { alpha: &mask, width: r.next_u32(), height: r.next_u32() }),
                };
                if let Some(i) = step!(d.embed_jpeg(&jpeg)) {
                    images.push(i);
                }
            }
            4 => {
                let geom = if r.below(2) == 0 {
                    ShadingGeom::Axial { x0: hostile(&mut r), y0: hostile(&mut r), x1: hostile(&mut r), y1: hostile(&mut r) }
                } else {
                    ShadingGeom::Radial { x0: hostile(&mut r), y0: hostile(&mut r), r: hostile(&mut r) }
                };
                let samples = blob(&mut r, 400);
                let h = hostile(&mut r);
                if let Some(s) = step!(d.add_shading(&geom, &samples, h)) {
                    shadings.push(s);
                }
            }
            5 | 6 => {
                d.add_outline(PdfOutlineItem {
                    title: text(&mut r),
                    level: [0, 1, 2, 3, 9, u32::MAX][r.below(6)],
                    page_index: [i64::MIN, -1, 0, 1, 3, i64::MAX][r.below(6)],
                    top_pt: hostile(&mut r),
                });
            }
            _ => {
                let (w, h) = (hostile(&mut r), hostile(&mut r));
                let mut p = d.add_page(w, h);
                for _ in 0..r.below(30) {
                    let col = [hostile(&mut r), hostile(&mut r), hostile(&mut r)];
                    let pick_font = |r: &mut Rng, fonts_made: &[FontId]| -> FontId {
                        if fonts_made.is_empty() || r.below(8) == 0 { stray_font } else { fonts_made[r.below(fonts_made.len())] }
                    };
                    match r.below(16) {
                        0 => p.fill_rect(hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r), &col),
                        1 => {
                            let dash: Vec<f64> = (0..r.below(4)).map(|_| hostile(&mut r)).collect();
                            p.line(hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r), &col, hostile(&mut r), &dash);
                        }
                        2 | 3 => {
                            let font = pick_font(&mut r, &fonts_made);
                            let n = r.below(20);
                            let mut placed = Vec::new();
                            for _ in 0..n {
                                let cp = [0x41, 0xe9, 0x1F600, 0xf0b7, 0x10ffff, r.next_u32() % 0x30000][r.below(6)];
                                let gid = p.encode(font, cp).map_or(r.next_u32(), |e| e.gid);
                                placed.push(PlacedGlyph {
                                    gid,
                                    code_point: cp,
                                    x_pt: hostile(&mut r),
                                    baseline_from_top_pt: hostile(&mut r),
                                });
                            }
                            let opts = GlyphOptions {
                                scale_x: (r.below(2) == 0).then(|| hostile(&mut r)),
                                synth_bold: r.below(2) == 0,
                                synth_italic: r.below(2) == 0,
                                runs: r.below(2) == 0,
                            };
                            p.glyphs(font, hostile(&mut r), &col, &placed, &opts);
                        }
                        4 | 5 => {
                            let cmds: Vec<PathCmd> = (0..r.below(8))
                                .map(|_| match r.below(4) {
                                    0 => PathCmd::M { x: hostile(&mut r), y: hostile(&mut r) },
                                    1 => PathCmd::L { x: hostile(&mut r), y: hostile(&mut r) },
                                    2 => PathCmd::C {
                                        x1: hostile(&mut r),
                                        y1: hostile(&mut r),
                                        x2: hostile(&mut r),
                                        y2: hostile(&mut r),
                                        x: hostile(&mut r),
                                        y: hostile(&mut r),
                                    },
                                    _ => PathCmd::Z,
                                })
                                .collect();
                            let dash = [hostile(&mut r)];
                            let stroke = Stroke { color: col, width_pt: hostile(&mut r), dash: if r.below(2) == 0 { &dash } else { &[] } };
                            let fill = (r.below(2) == 0).then_some(col);
                            p.path(&cmds, fill.as_ref(), (r.below(2) == 0).then_some(&stroke), r.below(2) == 0);
                            if r.below(3) == 0 {
                                p.push_clip_path(&cmds, r.below(2) == 0);
                                p.pop_clip();
                            }
                            if let Some(s) = shadings.first() {
                                p.fill_shaded(&cmds, *s, r.below(2) == 0);
                            }
                        }
                        6 => {
                            p.push_clip_rect(hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r));
                            p.pop_clip();
                        }
                        7 | 8 => {
                            if let Some(img) = images.get(r.below(images.len().max(1))).copied() {
                                match r.below(3) {
                                    0 => p.draw_image(img, hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r)),
                                    1 => p.draw_image_matrix(
                                        img,
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        hostile(&mut r),
                                    ),
                                    _ => p.draw_image_transformed(
                                        img,
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        hostile(&mut r),
                                        r.below(2) == 0,
                                        r.below(2) == 0,
                                    ),
                                }
                            }
                        }
                        9 => {
                            p.push_transform(
                                hostile(&mut r),
                                hostile(&mut r),
                                hostile(&mut r),
                                hostile(&mut r),
                                hostile(&mut r),
                                hostile(&mut r),
                            );
                            p.pop_clip();
                        }
                        10 | 11 => {
                            let (fa, sa) = ((r.below(2) == 0).then(|| hostile(&mut r)), (r.below(2) == 0).then(|| hostile(&mut r)));
                            p.with_alpha(fa, sa, |p| p.fill_rect(1.0, 1.0, 2.0, 2.0, &[0.0; 3]));
                        }
                        12 | 13 => {
                            let uris = ["https://a.b/c", "javascript:x", "mailto:\u{0}x", "", "  HTTP://\u{e9}\\("];
                            p.add_link(&PdfLink {
                                x_pt: hostile(&mut r),
                                y_top_pt: hostile(&mut r),
                                w_pt: hostile(&mut r),
                                h_pt: hostile(&mut r),
                                uri: (r.below(2) == 0).then(|| uris[r.below(uris.len())].to_owned()),
                                page_index: (r.below(2) == 0).then(|| [i64::MIN, -3, 0, 1, 7, i64::MAX][r.below(6)]),
                                dest_top_pt: (r.below(2) == 0).then(|| hostile(&mut r)),
                            });
                        }
                        _ => {
                            if let Ok(img) = p.document().embed_rgba(&r.bytes(16), 2, 2) {
                                images.push(img);
                            }
                        }
                    }
                }
                if r.below(5) == 0 {
                    drop(p);
                } else if p.finish().is_err() {
                    ok = false;
                }
            }
        }
    }
    let built = d.build().map(|s| s.sent);
    (built, ok)
}

#[test]
fn random_hostile_documents_never_panic() {
    let n: usize = std::env::var("UO_PDF_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let (mut ok, mut err_limit, mut err_sink, mut err_invalid) = (0, 0, 0, 0);
    for i in 0..n {
        match build_one(0x9e37_79b9u32.wrapping_mul(i as u32 + 1)).0 {
            Ok(_) => ok += 1,
            Err(PdfError::Limit(_)) => err_limit += 1,
            Err(PdfError::Sink) => err_sink += 1,
            Err(PdfError::Invalid(_)) => err_invalid += 1,
        }
    }
    println!(
        "{n} hostile documents: {ok} written, {err_limit} stopped by a limit, {err_sink} by the sink, {err_invalid} refused as invalid"
    );
    assert!(ok > n / 10 && err_limit + err_sink + err_invalid > 0, "the loop must exercise both outcomes");
}

#[test]
fn every_file_the_loop_writes_is_valid() {
    let n: usize = std::env::var("UO_PDF_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(3000) / 3;
    let mut checked = 0;
    for i in 0..n {
        let seed = 0x85eb_ca6bu32.wrapping_mul(i as u32 + 1);
        let Some(bytes) = build_to_vec(seed) else { continue };
        parse_pdf(&bytes).unwrap_or_else(|e| panic!("seed {seed:#x}: invalid file: {e}"));
        checked += 1;
    }
    println!("{checked} files written by the loop parsed with a valid xref and trailer");
    assert!(checked > 0);
}

/// `build_one` with a collecting sink and no failure injection.
fn build_to_vec(seed: u32) -> Option<Vec<u8>> {
    let mut r = Rng::new(seed);
    let f = fonts();
    let mut d = PdfDocument::new(Vec::new(), r.below(2) == 0);
    for _ in 0..1 + r.below(4) {
        let which = r.below(f.bytes.len());
        let font = d.embed_font("Face", garbage_font(&mut r, &f.bytes[which]), f.faces[which].clone());
        let img = d.embed_rgba(&r.bytes(16 * 16 * 4), 16, 16).ok();
        let mut p = d.add_page(hostile(&mut r), hostile(&mut r));
        let glyphs: Vec<PlacedGlyph> = (0..r.below(30))
            .map(|_| {
                let cp = 0x20 + r.below(200) as u32;
                PlacedGlyph {
                    gid: p.encode(font, cp).map_or(0, |e| e.gid),
                    code_point: cp,
                    x_pt: hostile(&mut r),
                    baseline_from_top_pt: hostile(&mut r),
                }
            })
            .collect();
        p.glyphs(font, hostile(&mut r), &[0.0; 3], &glyphs, &GlyphOptions { runs: r.below(2) == 0, ..Default::default() });
        if let Some(i) = img {
            p.draw_image(i, hostile(&mut r), hostile(&mut r), hostile(&mut r), hostile(&mut r));
        }
        p.add_link(&PdfLink {
            x_pt: 1.0,
            y_top_pt: 2.0,
            w_pt: 3.0,
            h_pt: 4.0,
            page_index: Some(r.next_u32() as i64 - 2_000_000_000),
            ..PdfLink::default()
        });
        p.finish().ok()?;
        d.add_outline(PdfOutlineItem {
            title: "t".into(),
            level: r.below(4) as u32,
            page_index: r.below(5) as i64 - 1,
            top_pt: hostile(&mut r),
        });
    }
    d.build().ok()
}

// ---- the stress shapes of the plan --------------------------------------------------------------

#[test]
fn five_hundred_pages_ten_thousand_links_and_a_big_outline() {
    let f = fonts();
    let mut d = PdfDocument::new(Vec::new(), true);
    let font = d.embed_font("Carlito", Arc::clone(&f.bytes[0]), f.faces[0].clone());
    for pg in 0..500 {
        let mut p = d.add_page(612.0, 792.0);
        let glyphs: Vec<PlacedGlyph> = (0..80)
            .map(|i| {
                let cp = 0x41 + (i % 26) as u32;
                PlacedGlyph {
                    gid: p.encode(font, cp).unwrap().gid,
                    code_point: cp,
                    x_pt: 72.0 + 6.5 * f64::from(i),
                    baseline_from_top_pt: 100.0 + f64::from(pg % 40),
                }
            })
            .collect();
        p.glyphs(font, 11.0, &[0.0; 3], &glyphs, &GlyphOptions { runs: true, ..Default::default() });
        for k in 0..20 {
            p.add_link(&PdfLink {
                x_pt: 10.0,
                y_top_pt: f64::from(k),
                w_pt: 5.0,
                h_pt: 5.0,
                page_index: Some((pg * 7 + k) as i64 % 500),
                ..PdfLink::default()
            });
        }
        p.finish().unwrap();
    }
    for i in 0..5000usize {
        d.add_outline(PdfOutlineItem {
            title: format!("Heading {i}"),
            level: 1 + (i % 9) as u32,
            page_index: (i % 500) as i64,
            top_pt: 100.0,
        });
    }
    let out = d.build().unwrap();
    let parsed = parse_pdf(&out).unwrap();
    println!("500 pages, 10,000 links, 5,000 outline entries over 9 levels: {} objects, {} bytes", parsed.count - 1, out.len());
}

#[test]
fn a_chain_of_twenty_thousand_levels_does_not_recurse() {
    let mut d = PdfDocument::new(Vec::new(), false);
    d.add_page(10.0, 10.0).finish().unwrap();
    for i in 0..20_000u32 {
        d.add_outline(PdfOutlineItem { title: "x".into(), level: i + 1, page_index: 0, top_pt: 0.0 });
    }
    let out = d.build().unwrap();
    parse_pdf(&out).unwrap();
    assert!(String::from_utf8_lossy(&out).contains("/Count -19999 "));
}

#[test]
fn a_chunked_sink_that_fails_stops_the_document_with_a_sink_error() {
    let mut d = PdfDocument::new(ChunkedSink::new(1024, |_c: Vec<u8>| Err(PdfError::Sink)), false);
    let r = (0..40).try_for_each(|_| d.embed_rgba(&[7; 4096], 32, 32).map(|_| ()));
    assert_eq!(r, Err(PdfError::Sink));
}
