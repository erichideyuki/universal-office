//! R-P4 slice for the canvas hints of frontier E: the frozen `paintItems` (recorded by
//! `tools/capture-e.mjs`) against the hints of `encode_page_for_canvas` replayed through the shell
//! transcription (`e_support::shell`). Glyph runs with and without `'fontKerning' in ctx`, fx run
//! boxes and gradient geometries, shapes (parts, gradient geometry, 64 samples) with and without
//! effects.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod e_support;
mod g_support;

use e_support::*;
use g_support::*;
use uo_codec::Limits;
use uo_render::codec as w;
use uo_render::{DeltaMark, ItemList, NoApp, Resources, encode_page_for_canvas};
use uo_testkit::generate::Rng;
use uo_testkit::{Capture, PlatformSpec, Replay, Stream};

const CAPTURE: &str = include_str!("captures/e_hints.capture");

/// Encodes a one-item page and decodes the message back (one stream per call site).
struct Pipe {
    sent: DeltaMark,
    tables: w::Tables,
}

impl Pipe {
    fn new() -> Pipe {
        Pipe { sent: DeltaMark::new(1), tables: w::Tables::default() }
    }

    fn run(&mut self, list: ItemList, res: &Resources) -> w::Item {
        let page = page_of(list);
        let mut buf = Vec::new();
        encode_page_for_canvas(&page, res, &NoApp, &mut self.sent, &mut buf).unwrap();
        let msg = w::decode_page(&buf, &mut self.tables, &Limits::default()).unwrap();
        msg.items.into_iter().next().unwrap()
    }
}

fn stats(log: &[Op], strings: &mut usize, batched: &mut usize, quads: &mut usize) {
    for o in log {
        if o.name == "fillText" {
            *strings += 1;
            if matches!(o.args.first(), Some(Arg::S(c)) if c.len() > 1) {
                *batched += 1;
            }
        }
        if o.name == "quadraticCurveTo" {
            *quads += 1;
        }
    }
}

/// The capture was taken with Node v24.15.0 (V8 13.6) on win32-x64. Unlike frontier G's it needs no
/// glibc environment: V8 runs its own `ieee754` `pow`/`sin`/`cos` on every OS (G's pow-dependent
/// facets came out equal on win32 and linux).
fn capture_e(text: &str) -> Capture {
    let cap = Capture::parse(text).unwrap();
    let spec = PlatformSpec {
        v8_prefix: Some("13.6.233.17".into()),
        arch: Some("x64".into()),
        os: None,
        glibc_prefix: None,
        tunables_contain: None,
        flags: Some("none".into()),
    };
    cap.require_platform(&spec).unwrap();
    assert_eq!(cap.platform().unwrap().node, "v24.15.0");
    cap
}

#[test]
fn canvas_hints_parity() {
    let cap = capture_e(CAPTURE);
    let mut rep = Replay::new(&cap);
    let fonts = fonts();

    // glyph runs: batches, remap, gid outlines
    for kerning in [true, false] {
        let mut r = Rng::new(7001);
        let mut res = resources_with_fonts(&fonts);
        let mut pipe = Pipe::new();
        let (mut strings, mut batched, mut quads) = (0, 0, 0);
        let mut s = Stream::new();
        for _ in 0..1500 {
            let case = gen_glyph_run(&mut r, &fonts, 14);
            let mut list = ItemList::new();
            case.push(&mut list, &mut res, None);
            let item = pipe.run(list, &res);
            let log = shell::paint_items(&item, &pipe.tables, kerning, false);
            stats(&log, &mut strings, &mut batched, &mut quads);
            log_out(&mut s, &log);
        }
        let key = if kerning { "glyphs.kern" } else { "glyphs.nokern" };
        let got = format!("{}:{}", s.items(), s.finish().short());
        rep.check(key, "v", &got);
        rep.check(key, "stats", &format!("strings={strings};batched={batched};quads={quads}"));
        if kerning {
            assert!(batched > 500, "the corpus must batch glyphs: {batched}");
            assert!(quads > 1000, "the corpus must draw gid outlines: {quads}");
        }
    }

    // fx runs: box and the gradient geometries
    {
        let mut r = Rng::new(7002);
        let mut res = resources_with_fonts(&fonts);
        let mut pipe = Pipe::new();
        let mut s = Stream::new();
        let mut grads = 0;
        for _ in 0..800 {
            let case = gen_glyph_run(&mut r, &fonts, 8);
            let fx = gen_fx(&mut r, &mut res.symbols);
            let fill_grad = matches!(fx.fill, Some(uo_render::types::ResolvedFill::Grad { .. }));
            let outline_grad = matches!(fx.outline.as_ref().map(|o| &o.fill), Some(uo_render::types::ResolvedFill::Grad { .. }));
            let id = res.add_text_fx(fx).unwrap();
            let mut list = ItemList::new();
            case.push(&mut list, &mut res, Some(id));
            let w::Item::Glyphs(g) = pipe.run(list, &res) else { panic!("not a run") };
            let b = g.r#box.expect("an fx run has a box");
            for v in [b.x, b.y, b.w, b.h] {
                f(&mut s, v);
            }
            for (geom, expect) in [(&g.fx_fill_geom, fill_grad), (&g.fx_outline_geom, outline_grad)] {
                assert_eq!(geom.is_some(), expect, "the geometry is there exactly for a gradient");
                match geom {
                    None => {
                        s.tag(b'N');
                    }
                    Some(w::GradGeom::Axial(a)) => {
                        grads += 1;
                        s.tag(b'A');
                        for v in [a.x0, a.y0, a.x1, a.y1] {
                            f(&mut s, v);
                        }
                    }
                    Some(w::GradGeom::Radial(q)) => {
                        grads += 1;
                        s.tag(b'R');
                        for v in [q.x0, q.y0, q.r] {
                            f(&mut s, v);
                        }
                    }
                }
            }
        }
        rep.check("fxrun", "v", &format!("{}:{}", s.items(), s.finish().short()));
        rep.check("fxrun", "stats", &format!("grads={grads}"));
    }

    // shapes
    {
        let mut r = Rng::new(7003);
        let mut res = resources_with_fonts(&fonts);
        let mut pipe = Pipe::new();
        let mut s = Stream::new();
        let mut sf = Stream::new();
        let (mut painted, mut grads, mut fxs, mut begins) = (0, 0, 0, 0);
        for _ in 0..2500 {
            let case = gen_shape(&mut r, &mut res);
            let mut list = ItemList::new();
            list.push_plain(item_shape(case.item));
            let item = pipe.run(list, &res);
            let log = shell::paint_items(&item, &pipe.tables, true, case.effects);
            if log.len() > 2 {
                painted += 1;
            }
            if log.iter().any(|o| o.name == "createLinearGradient" || o.name == "createRadialGradient") {
                grads += 1;
            }
            begins += shell::count(&log, "beginPath");
            if case.effects {
                fxs += 1;
                log_out(&mut sf, &log);
            } else {
                log_out(&mut s, &log);
            }
        }
        rep.check("shapes", "v", &format!("{}:{}", s.items(), s.finish().short()));
        rep.check("shapes", "stats", &format!("painted={painted};grads={grads};fx={fxs};begin={begins}"));
        rep.check("shapes.fx", "v", &format!("{}:{}", sf.items(), sf.finish().short()));
        assert!(grads > 400 && fxs > 300, "coverage: grads {grads}, fx {fxs}");
    }

    let report = rep.finish();
    eprintln!("e_hints: {}", report.summary());
    report.assert_clean();
}
