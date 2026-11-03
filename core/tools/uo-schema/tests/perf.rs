//! Throughput numbers for the module record (not a gate): encode and decode of the 100,000-glyph columns page and of a
//! typical text page. Run with
//! `cargo test -p uo-schema --release --test perf -- --ignored --nocapture`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod render_codec;

use std::hint::black_box;
use std::time::Instant;

use render_codec::generated::*;
use uo_codec::Limits;

fn page(glyphs: usize, runs: usize) -> Page {
    let mut items = Vec::new();
    for r in 0..runs {
        let mut cols = GlyphColumns::default();
        for i in 0..glyphs / runs {
            let v = u32::try_from(i + r * 1000).unwrap();
            cols.cp.push(0x20 + v % 0x5F);
            cols.x.push(f64::from(v) * 7.25);
            cols.gid.push(None);
        }
        items.push(Item::Glyphs(GlyphRun {
            face: 0,
            size_pt: 11.0,
            color: 0,
            y: f64::from(u32::try_from(r).unwrap()) * 240.0,
            glyphs: cols,
            scale_x: None,
            synth_bold: false,
            synth_italic: false,
            alpha: None,
            as_path: false,
            colr: false,
            fx: None,
            text: None,
            joins: None,
            remap: None,
            outlines: None,
            r#box: None,
        }));
    }
    Page { w: 12240.0, h: 15840.0, background: None, background_fill: None, background_show: false, items, app: Vec::new() }
}

fn delta() -> Delta {
    let mut d = Delta { generation: 1, ..Delta::default() };
    d.sym.items = vec![vec![0x30]];
    d.faces.items = vec![Face {
        key: 0,
        family: 0,
        bold: false,
        italic: false,
        synth_bold: false,
        synth_italic: false,
        stand_in: false,
        has_bytes: true,
        upem: 2048,
    }];
    d
}

fn run(label: &str, p: &Page, iters: u32) {
    let d = delta();
    let mut out = Vec::new();
    encode_page(&mut out, &d, p).unwrap();
    let wire = out.len();
    let glyphs: usize = p.items.iter().map(|i| if let Item::Glyphs(g) = i { g.glyphs.cp.len() } else { 0 }).sum();
    let t = Instant::now();
    for _ in 0..iters {
        let mut o = Vec::with_capacity(wire);
        encode_page(&mut o, black_box(&d), black_box(p)).unwrap();
        black_box(&o);
    }
    let enc = t.elapsed();
    let t = Instant::now();
    for _ in 0..iters {
        let mut tables = Tables::new();
        black_box(decode_page(black_box(&out), &mut tables, &Limits::default()).unwrap());
    }
    let dec = t.elapsed();
    let per = |d: std::time::Duration| d.as_secs_f64() * 1e6 / f64::from(iters);
    eprintln!(
        "{label}: {glyphs} glyphs, {wire} B on the wire; encode {:.1} us ({:.0} MB/s), decode {:.1} us ({:.0} MB/s)",
        per(enc),
        wire as f64 / per(enc),
        per(dec),
        wire as f64 / per(dec)
    );
}

#[test]
#[ignore = "numbers for the record"]
fn throughput() {
    run("text page (3,000 glyphs, 60 runs)", &page(3_000, 60), 2_000);
    run("100,000 glyphs, 1 run", &page(100_000, 1), 50);
}
