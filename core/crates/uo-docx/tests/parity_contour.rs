//! Parity of the Word-only DrawingML code moved into uo-docx (uo-drawingml gate decision G3),
//! replayed against V8:
//!
//! - `wrap_contour` against `wrapContour` (`shared/drawingml/src/contour.ts`) over the 2,369
//!   generated cases of `tests/parity/contour.inputs.txt` (rect, ellipse, ring, triangle, box
//!   unions, noise, sparse pixels, frames, checkers, the dilation-reaches-the-grid family, hostile
//!   sizes from 0 to 100,000, truncated and over-long buffers, every preset shape at five sizes);
//!   the facets are the digest of the RGBA bytes (`in`: the Rust generator of each case builds the
//!   same bytes as the JS one), the digest of the exact bits of every vertex (`out`) and the
//!   vertex count (`len`);
//! - `shape_style_preset` / `find_shape_style` / the table against `SHAPE_STYLE_PRESETS`
//!   (`tests/parity/shape-styles.inputs.txt`): the table digest, the lookups `SHAPE_STYLE_PRESETS[n - 1]`
//!   over hostile numbers (as f64 bits) and the comparison loop of `shapeStyleIndex` over every
//!   entry and six variants of it.
//!
//! A truncated buffer is the one declared divergence (record, decision C2): the TS reads the
//! missing bytes as `undefined` (not 0: opaque), `wrap_contour` returns `BufferSize`; the replay
//! checks the error and then compares the TS result with the one on the buffer padded with 255.
//!
//! Recapture: `UO_TS_ROOT=<checkout of main> node core/crates/uo-docx/js/capture-contour.mjs`
//! (no `pow`, no libm call: `Math.hypot` of V8 is software, so one Windows capture is enough).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use uo_docx::drawing::{ContourError, SHAPE_STYLE_PRESETS, find_shape_style, shape_style_preset, wrap_contour};
use uo_drawingml::{Affine, DEFAULT_TOL, Limits, Mask, fill_polygons, flatten_path, preset_outline};
use uo_jsnum::conv::to_uint8;
use uo_jsnum::math::round;
use uo_testkit::{Capture, Replay, Stream};

fn xorshift(seed: u32) -> impl FnMut() -> u32 {
    let mut s = if seed == 0 { 1 } else { seed };
    move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    }
}

struct Buf {
    w: usize,
    h: usize,
    data: Vec<u8>,
}

impl Buf {
    fn new(w: usize, h: usize) -> Buf {
        Buf { w, h, data: vec![0; w * h * 4] }
    }

    fn set(&mut self, x: usize, y: usize, a: u8) {
        let i = (y * self.w + x) * 4;
        let t = x * 5 + y * 3;
        self.data[i] = u8::try_from(t & 255).unwrap();
        self.data[i + 1] = u8::try_from((t + 11) & 255).unwrap();
        self.data[i + 2] = u8::try_from((t + 22) & 255).unwrap();
        self.data[i + 3] = a;
    }

    fn each(&mut self, mut f: impl FnMut(usize, usize) -> u8) {
        for y in 0..self.h {
            for x in 0..self.w {
                let a = f(x, y);
                self.set(x, y, a);
            }
        }
    }
}

fn int(s: &str) -> i64 {
    s.parse::<i64>().unwrap()
}

fn alpha(s: &str) -> u8 {
    u8::try_from(int(s)).unwrap()
}

/// The rules of the header of `js/capture-contour.mjs`, with integer arithmetic only.
fn build(spec: &[&str]) -> Buf {
    let w = usize::try_from(int(spec[1])).unwrap();
    let h = usize::try_from(int(spec[2])).unwrap();
    let p = &spec[3..];
    let mut b = Buf::new(w, h);
    let wi = i64::try_from(w).unwrap();
    let hi = i64::try_from(h).unwrap();
    let at = |x: usize, y: usize| (i64::try_from(x).unwrap(), i64::try_from(y).unwrap());
    match spec[0] {
        "rect" => {
            let (x0, y0, x1, y1, a) = (int(p[0]), int(p[1]), int(p[2]), int(p[3]), alpha(p[4]));
            b.each(|x, y| {
                let (x, y) = at(x, y);
                if x >= x0 && x < x1 && y >= y0 && y < y1 { a } else { 0 }
            });
        }
        "ellipse" => {
            let (x0, y0, x1, y1, a) = (int(p[0]), int(p[1]), int(p[2]), int(p[3]), alpha(p[4]));
            let (aa, bb) = (x1 - x0, y1 - y0);
            b.each(|x, y| {
                let (x, y) = at(x, y);
                let (dx, dy) = (2 * x + 1 - (x0 + x1), 2 * y + 1 - (y0 + y1));
                if dx * dx * bb * bb + dy * dy * aa * aa <= aa * aa * bb * bb { a } else { 0 }
            });
        }
        "ring" => {
            let (x0, y0, x1, y1, k, a) = (int(p[0]), int(p[1]), int(p[2]), int(p[3]), int(p[4]), alpha(p[5]));
            let (aa, bb) = (x1 - x0, y1 - y0);
            b.each(|x, y| {
                let (x, y) = at(x, y);
                let (dx, dy) = (2 * x + 1 - (x0 + x1), 2 * y + 1 - (y0 + y1));
                let v = dx * dx * bb * bb + dy * dy * aa * aa;
                if v <= aa * aa * bb * bb && v * 64 > k * k * aa * aa * bb * bb { a } else { 0 }
            });
        }
        "tri" => {
            let v: Vec<i64> = p[..6].iter().map(|s| int(s)).collect();
            let a = alpha(p[6]);
            let cr = |ax: i64, ay: i64, bx: i64, by: i64, px: i64, py: i64| (bx - ax) * (py - ay) - (by - ay) * (px - ax);
            b.each(|x, y| {
                let (x, y) = at(x, y);
                let c0 = cr(v[0], v[1], v[2], v[3], x, y);
                let c1 = cr(v[2], v[3], v[4], v[5], x, y);
                let c2 = cr(v[4], v[5], v[0], v[1], x, y);
                if (c0 >= 0 && c1 >= 0 && c2 >= 0) || (c0 <= 0 && c1 <= 0 && c2 <= 0) { a } else { 0 }
            });
        }
        "rects" => {
            let a = alpha(p[0]);
            let boxes: Vec<Vec<i64>> = p[1].split(';').map(|bx| bx.split(',').map(int).collect()).collect();
            b.each(|x, y| {
                let (x, y) = at(x, y);
                if boxes.iter().any(|q| x >= q[0] && x < q[2] && y >= q[1] && y < q[3]) { a } else { 0 }
            });
        }
        "noise" => {
            let mut next = xorshift(u32::try_from(int(p[0])).unwrap());
            let zpm = u32::try_from(int(p[1])).unwrap();
            for y in 0..h {
                for x in 0..w {
                    let r = next();
                    let i = (y * w + x) * 4;
                    b.data[i] = u8::try_from((r >> 8) & 255).unwrap();
                    b.data[i + 1] = u8::try_from((r >> 16) & 255).unwrap();
                    b.data[i + 2] = u8::try_from((r >> 24) & 255).unwrap();
                    b.data[i + 3] = if r % 1000 < zpm { 0 } else { u8::try_from(1 + ((r >> 10) % 255)).unwrap() };
                }
            }
        }
        "sparse" => {
            let mut next = xorshift(u32::try_from(int(p[0])).unwrap());
            b.each(|_, _| 0);
            for _ in 0..int(p[1]) {
                let x = usize::try_from(next()).unwrap() % w;
                let y = usize::try_from(next()).unwrap() % h;
                b.set(x, y, 255);
            }
        }
        "frame" => {
            let (t, a) = (int(p[0]), alpha(p[1]));
            b.each(|x, y| {
                let (x, y) = at(x, y);
                if x < t || y < t || x >= wi - t || y >= hi - t { a } else { 0 }
            });
        }
        "checker" => {
            let (c, a) = (int(p[0]), alpha(p[1]));
            b.each(|x, y| {
                let (x, y) = at(x, y);
                if ((x / c) + (y / c)) & 1 == 1 { a } else { 0 }
            });
        }
        "full" => {
            let a = alpha(p[0]);
            b.each(|_, _| a);
        }
        "preset" => {
            let (name, ox, oy) = (p[0], int(p[1]), int(p[2]));
            b.each(|_, _| 0);
            let (bw, bh) = (wi - 2 * ox, hi - 2 * oy);
            if bw > 0 && bh > 0 {
                let limits = Limits::NATIVE;
                let o = preset_outline(Some(name), bw as f64, bh as f64, None);
                let polys = flatten_path(&o.subs, &Affine([1.0, 0.0, 0.0, 1.0, ox as f64, oy as f64]), DEFAULT_TOL, &limits).unwrap();
                let mut m = Mask::new(u32::try_from(w).unwrap(), u32::try_from(h).unwrap(), &limits).unwrap();
                fill_polygons(&mut m, &polys, false, &limits).unwrap();
                for (i, &c) in m.data().iter().enumerate() {
                    b.data[i * 4 + 3] = to_uint8(round(f64::from(c) * 255.0));
                }
            }
        }
        k => panic!("unknown kind {k}"),
    }
    b
}

#[test]
fn wrap_contour_matches_v8() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/contour.capture")).unwrap();
    let inputs = std::fs::read_to_string(Path::new(manifest).join("tests/parity/contour.inputs.txt")).unwrap();
    let mut replay = Replay::new(&cap);
    let (mut nulls, mut polys, mut vertices, mut truncated, mut cases) = (0, 0, 0, 0, 0);
    for (i, line) in inputs.lines().enumerate() {
        let all: Vec<&str> = line.split('\t').collect();
        let spec: Vec<&str> = all.iter().copied().filter(|f| !f.starts_with("trunc=") && !f.starts_with("extra=")).collect();
        let (w, h) = (u32::try_from(int(spec[1])).unwrap(), u32::try_from(int(spec[2])).unwrap());
        let mut data = build(&spec).data;
        let full_len = data.len();
        let mut cut = false;
        for m in all.iter().filter(|f| f.starts_with("trunc=") || f.starts_with("extra=")) {
            let (k, v) = m.split_once('=').unwrap();
            let v = usize::try_from(int(v)).unwrap();
            if k == "trunc" {
                data.truncate(v);
                cut = true;
            } else {
                data.resize(data.len() + v, 255);
            }
        }
        let key = i.to_string();
        replay.check(&key, "in", &Stream::new().digest_of(&data).digest().short());
        let given = if cut && w > 0 && h > 0 {
            truncated += 1;
            assert_eq!(wrap_contour(&data, w, h), Err(ContourError::BufferSize), "case {i}: a short buffer is refused");
            data.resize(full_len, 255);
            data
        } else {
            data
        };
        match wrap_contour(&given, w, h).unwrap_or_else(|e| panic!("case {i}: {e:?}")) {
            None => {
                nulls += 1;
                replay.check(&key, "out", "null");
                replay.check(&key, "len", "0");
            }
            Some(pts) => {
                polys += 1;
                vertices += pts.len();
                let mut s = Stream::new();
                s.u64(pts.len() as u64);
                for [x, y] in &pts {
                    assert!(x.fract() == 0.0 && y.fract() == 0.0 && *x >= 0.0 && *y >= 0.0 && *x <= 21600.0 && *y <= 21600.0, "case {i}");
                    assert!(x.is_sign_positive() && y.is_sign_positive(), "case {i}: -0");
                    s.f64(*x).f64(*y);
                }
                replay.check(&key, "out", &s.finish().short());
                replay.check(&key, "len", &pts.len().to_string());
                assert_eq!(pts.first(), pts.last(), "case {i}: the polygon is closed");
            }
        }
        cases += 1;
    }
    let corpus = cap.meta("corpus").unwrap();
    assert!(corpus.starts_with(&format!("cases={cases} null={nulls} polygons={polys} vertices={vertices} ")), "corpus line {corpus}");
    let report = replay.finish();
    println!(
        "contour parity: {} ({cases} cases, {nulls} null, {polys} polygons, {vertices} vertices, {truncated} truncated)",
        report.summary()
    );
    report.assert_clean();
    assert!(polys > 1500 && nulls > 300 && truncated >= 70);
}

#[test]
fn shape_styles_match_v8() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let cap = Capture::read(Path::new(manifest).join("tests/parity/shape-styles.capture")).unwrap();
    let inputs = std::fs::read_to_string(Path::new(manifest).join("tests/parity/shape-styles.inputs.txt")).unwrap();
    let mut replay = Replay::new(&cap);
    let mut s = Stream::new();
    s.u64(SHAPE_STYLE_PRESETS.len() as u64);
    for p in SHAPE_STYLE_PRESETS {
        s.str(p.sp_pr).str(p.style);
    }
    replay.check("table", "digest", &s.finish().short());
    replay.check("table", "len", &SHAPE_STYLE_PRESETS.len().to_string());
    let un = |h: &str| -> String {
        if h == "-" {
            return String::new();
        }
        let bytes: Vec<u8> = (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect();
        String::from_utf8(bytes).unwrap()
    };
    let (mut picked, mut found, mut lines) = (0, 0, 0);
    for (i, line) in inputs.lines().enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        let key = i.to_string();
        if f[0] == "n" {
            let n = f64::from_bits(u64::from_str_radix(f[1], 16).unwrap());
            match shape_style_preset(n) {
                None => {
                    replay.check(&key, "pick", "undef");
                }
                Some(e) => {
                    picked += 1;
                    let mut s = Stream::new();
                    s.str(e.sp_pr).str(e.style);
                    replay.check(&key, "pick", &s.finish().short());
                }
            }
        } else {
            let (p, st) = if f[0] == "find" {
                let (i, v) = (usize::try_from(int(f[1])).unwrap(), int(f[2]));
                let (a, b) = (&SHAPE_STYLE_PRESETS[i], &SHAPE_STYLE_PRESETS[(i + 1) % SHAPE_STYLE_PRESETS.len()]);
                match v {
                    0 => (a.sp_pr.to_owned(), a.style.to_owned()),
                    1 => (a.style.to_owned(), a.sp_pr.to_owned()),
                    2 => (a.sp_pr.to_owned(), b.style.to_owned()),
                    3 => (b.sp_pr.to_owned(), a.style.to_owned()),
                    4 => (format!("{} ", a.sp_pr), a.style.to_owned()),
                    5 => (a.sp_pr.to_owned(), a.style.replacen("<wps:style>", "<wps:style >", 1)),
                    _ => panic!("variant {v}"),
                }
            } else {
                assert_eq!(f[0], "findx");
                (un(f[1]), un(f[2]))
            };
            let n = find_shape_style(&p, &st);
            if n.is_some() {
                found += 1;
            }
            replay.check(&key, "find", &n.unwrap_or(0).to_string());
        }
        lines += 1;
    }
    assert_eq!(cap.meta("corpus").unwrap(), format!("lines={lines} picked={picked} found={found}"));
    let report = replay.finish();
    println!("shape styles parity: {} ({lines} lines, {picked} picked, {found} found)", report.summary());
    report.assert_clean();
    assert!(picked >= 80 && found >= 200);
}
