//! B5b metafiles, PICT, TIFF and EOT: native checks over the committed capture of the frozen TS (`uo-pptx/tests/captures/meta-b5b.*`,
//! 1,267 rasterizations of 873 synthetic files; the exact call-stream parity is `uo-pptx/js/compare-meta.mjs`, which replays the Rust
//! recipes with `uo-wasm/js/meta-player.ts` and compares every line), M1 and M5 of the memory gate, the two-phase measurement protocol,
//! and bounded no-panic over damaged files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use uo_alloc::assert_no_leak;
use uo_dml_render::metafile::{Decoded, Measure, Metrics, Need, Outcome, Raster, rasterize_special};
use uo_testkit::capture::Capture;

#[path = "support/meta_stub.rs"]
mod stub;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../uo-pptx/tests/captures/");

/// The measurer of the capture: the recorder's stub, no compressed bitmap decodable (Node).
struct Stub;

impl Measure for Stub {
    fn metrics(&mut self, font: &[u16], text: &[u16]) -> Option<Metrics> {
        Some(stub::stub_metrics(font, text))
    }
    fn decoded(&mut self, _key: &str) -> Decoded {
        Decoded::Failed
    }
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    let b = std::fs::read(format!("{DIR}meta-b5b.inputs")).unwrap();
    let mut out = Vec::new();
    let mut p = 0;
    let u32at = |p: usize| u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]) as usize;
    while p < b.len() {
        let kl = u32at(p);
        let key = String::from_utf8(b[p + 4..p + 4 + kl].to_vec()).unwrap();
        p += 4 + kl;
        let n = u32at(p);
        out.push((key, b[p + 4..p + 4 + n].to_vec()));
        p += 4 + n;
    }
    out
}

/// SHA-1 (the digest of the Node capture: `createHash('sha1')`).
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h = [h[0].wrapping_add(a), h[1].wrapping_add(b), h[2].wrapping_add(c), h[3].wrapping_add(d), h[4].wrapping_add(e)];
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn hex16(d: &[u8]) -> String {
    sha1(d).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[test]
fn sizes_eot_and_tiff_pixels_match_the_ts_capture() {
    let cap = Capture::read(format!("{DIR}meta-b5b.capture")).unwrap();
    let files: std::collections::HashMap<String, Vec<u8>> = inputs().into_iter().collect();
    let (mut checked, mut eot, mut tiff, mut none) = (0, 0, 0, 0);
    for rec in cap.records() {
        let bytes = files.get(&rec.key).unwrap();
        for (facet, value) in &rec.facets {
            let disp = facet.strip_prefix("d.").unwrap();
            let display = if disp == "-" {
                None
            } else {
                let (w, h) = disp.split_once('x').unwrap();
                Some((uo_jsnum::parse::to_number(w), uo_jsnum::parse::to_number(h)))
            };
            // `result:lines:sha[:tiffSha]` (the `=` of the result is written as `:`)
            let fields: Vec<&str> = value.split(':').collect();
            let result = fields[0];
            if rec.key.starts_with("eot") {
                let got = uo_fonts::eot::read(bytes);
                match result.strip_prefix("eot ") {
                    Some(r) => {
                        let (len, sha) = r.split_once(' ').unwrap();
                        let f = got.unwrap_or_else(|| panic!("{}: the TS reads it", rec.key));
                        assert_eq!((f.len().to_string(), hex16(&f)), (len.to_string(), sha.to_string()), "{}", rec.key);
                        eot += 1;
                    }
                    None => assert!(got.is_none(), "{}", rec.key),
                }
                checked += 1;
                continue;
            }
            match rasterize_special(bytes, display, &mut Stub) {
                Outcome::Done(None) => {
                    assert_eq!(result, "none", "{} {facet}", rec.key);
                    none += 1;
                }
                Outcome::Done(Some(Raster::Recipe(r))) => {
                    assert_eq!(
                        result.split(' ').take(2).collect::<Vec<_>>().join(" "),
                        format!("canvas {}x{}", r.w, r.h),
                        "{} {facet}",
                        rec.key
                    );
                    if rec.key.starts_with("tiff") {
                        // the recipe of a TIFF is one `putImageData`: its blob is the decoded RGBA the TS digested
                        let img = uo_image::decode_tiff_slides(bytes).unwrap().unwrap();
                        assert_eq!(fields.last().copied().unwrap(), hex16(&img.data), "{}", rec.key);
                        tiff += 1;
                    }
                }
                Outcome::Need(n) => panic!("{} {facet}: unanswered needs {n:?}", rec.key),
            }
            checked += 1;
        }
    }
    assert!(checked >= 1_267 && eot >= 100 && tiff >= 100 && none > 0, "{checked} {eot} {tiff} {none}");
}

/// The recipes themselves are pinned by their SHA-1 (a regression gate: the TS parity is the Node comparison).
#[test]
fn recipes_are_deterministic() {
    for (key, bytes) in inputs().iter().filter(|(k, _)| !k.starts_with("eot")).take(120) {
        let a = rasterize_special(bytes, None, &mut Stub);
        let b = rasterize_special(bytes, None, &mut Stub);
        match (a, b) {
            (Outcome::Done(Some(Raster::Recipe(x))), Outcome::Done(Some(Raster::Recipe(y)))) => assert_eq!(x.bytes, y.bytes, "{key}"),
            (Outcome::Done(None), Outcome::Done(None)) => {}
            other => panic!("{key}: {other:?}"),
        }
    }
}

/// An EMF whose text asks the host to measure: a missing answer is a need, and the second pass with the answers is the first with the stub.
#[test]
fn a_missing_measurement_is_a_need() {
    struct Unknown;
    impl Measure for Unknown {
        fn metrics(&mut self, _: &[u16], _: &[u16]) -> Option<Metrics> {
            None
        }
        fn decoded(&mut self, _: &str) -> Decoded {
            Decoded::Unknown
        }
    }
    let files = inputs();
    let mut saw_text = false;
    let mut saw_decode = false;
    for (key, bytes) in files.iter().filter(|(k, _)| k.starts_with("emf/") || k.starts_with("emfplus/")).take(260) {
        match rasterize_special(bytes, None, &mut Unknown) {
            Outcome::Need(needs) => {
                assert!(!needs.is_empty(), "{key}");
                for n in &needs {
                    match n {
                        Need::Text { .. } => saw_text = true,
                        Need::Decode { key: k, data } => {
                            assert!(k.contains(':') && !data.is_empty());
                            saw_decode = true;
                        }
                    }
                }
            }
            Outcome::Done(_) => {}
        }
    }
    assert!(saw_text && saw_decode, "text {saw_text} decode {saw_decode}");
}

/// A decodable compressed bitmap makes the metafile draw it: the recipe carries the compressed bytes as a blob.
#[test]
fn a_decoded_bitmap_is_drawn_from_its_blob() {
    struct Ready;
    impl Measure for Ready {
        fn metrics(&mut self, f: &[u16], t: &[u16]) -> Option<Metrics> {
            Some(stub::stub_metrics(f, t))
        }
        fn decoded(&mut self, _: &str) -> Decoded {
            Decoded::Ready { w: 20.0, h: 10.0 }
        }
    }
    let mut changed = 0;
    for (_, bytes) in inputs().iter().filter(|(k, _)| k.starts_with("emfplus/")).take(70) {
        let (Outcome::Done(Some(Raster::Recipe(a))), Outcome::Done(Some(Raster::Recipe(b)))) =
            (rasterize_special(bytes, None, &mut Stub), rasterize_special(bytes, None, &mut Ready))
        else {
            continue;
        };
        if a.bytes != b.bytes {
            changed += 1;
        }
    }
    assert!(changed > 0, "no EMF+ of the set draws a compressed bitmap");
}

/// The prepare list the viewer decodes at open (`compressed_bitmaps`, the TS `prepareMetafile`) is exactly the set of bitmaps a
/// rasterization asks for (`Need::Decode`), so no paint of a warmed deck waits on a decode (`PENDING`).
#[test]
fn the_prepare_list_is_what_the_rasterization_asks_for() {
    use uo_dml_render::metafile::{compressed_bitmaps, emfplus::bitmap_key};
    struct Ask;
    impl Measure for Ask {
        fn metrics(&mut self, f: &[u16], t: &[u16]) -> Option<Metrics> {
            Some(stub::stub_metrics(f, t))
        }
        fn decoded(&mut self, _: &str) -> Decoded {
            Decoded::Unknown
        }
    }
    let (mut files, mut blobs) = (0, 0);
    for (key, bytes) in inputs() {
        let mut want: Vec<String> = compressed_bitmaps(&bytes).iter().map(|b| bitmap_key(b)).collect();
        let mut asked: Vec<String> = match rasterize_special(&bytes, None, &mut Ask) {
            Outcome::Need(n) => n.into_iter().filter_map(|n| if let Need::Decode { key, .. } = n { Some(key) } else { None }).collect(),
            Outcome::Done(_) => Vec::new(),
        };
        want.sort();
        want.dedup();
        asked.sort();
        asked.dedup();
        assert_eq!(want, asked, "{key}");
        if !want.is_empty() {
            files += 1;
            blobs += want.len();
        }
    }
    assert!(files > 10 && blobs >= files, "only {files} files with {blobs} compressed bitmaps");
}

// ─────────────────────────── M1, M2 ───────────────────────────

#[test]
fn m1_rasterization_leaves_nothing_behind() {
    let files = inputs();
    let sample: Vec<&Vec<u8>> =
        files.iter().filter(|(k, _)| !k.starts_with("eot") && !k.contains("-mut")).step_by(9).map(|(_, b)| b).collect();
    // warm the lazily built tables
    for b in &sample {
        let _ = rasterize_special(b, Some((480.0, 300.0)), &mut Stub);
    }
    assert_no_leak(|| {
        for b in &sample {
            let r = rasterize_special(b, Some((480.0, 300.0)), &mut Stub);
            drop(r);
        }
    });
    let eots: Vec<&Vec<u8>> = files.iter().filter(|(k, _)| k.starts_with("eot")).map(|(_, b)| b).collect();
    assert_no_leak(|| {
        for b in &eots {
            drop(uo_fonts::eot::read(b));
        }
    });
}

#[test]
fn m2_a_soak_of_rasterizations_is_flat() {
    let files = inputs();
    let sample: Vec<&Vec<u8>> =
        files.iter().filter(|(k, _)| k.starts_with("emfplus/") || k.starts_with("wmf/")).step_by(5).map(|(_, b)| b).collect();
    let series = uo_alloc::soak(60, 10, |i| {
        let b = sample[i % sample.len()];
        drop(rasterize_special(b, None, &mut Stub));
    });
    uo_alloc::assert_flat(&series, 1, 0);
}

// ─────────────────────────── M5: the caps ───────────────────────────

/// A record-count bomb: an EMF of 300,000 rectangles stops at the op cap, quickly, and says so.
#[test]
fn m5_a_record_bomb_stops_at_the_op_cap() {
    let mut f = vec![0u8; 88];
    f[0] = 1;
    f[8..12].copy_from_slice(&0i32.to_le_bytes());
    // header: bounds, frame 0..21000 x 0..29700, ' EMF', size, device 1920x1080 on 508x286 mm
    let put = |f: &mut Vec<u8>, o: usize, v: i32| f[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut f, 4, 88);
    put(&mut f, 16, 794);
    put(&mut f, 20, 1123);
    put(&mut f, 32, 21000);
    put(&mut f, 36, 29700);
    f[40..44].copy_from_slice(&0x464d_4520u32.to_le_bytes());
    put(&mut f, 72, 1920);
    put(&mut f, 76, 1080);
    put(&mut f, 80, 508);
    put(&mut f, 84, 286);
    for i in 0..300_000i32 {
        let mut r = Vec::new();
        for v in [43, 24, i % 700, i % 900, 100 + i % 700, 100 + i % 900] {
            r.extend_from_slice(&v.to_le_bytes());
        }
        // RECTANGLE: type 43, size 24, l t r b
        let mut rec = Vec::new();
        rec.extend_from_slice(&43u32.to_le_bytes());
        rec.extend_from_slice(&24u32.to_le_bytes());
        for v in [i % 700, i % 900, 100 + i % 700, 100 + i % 900] {
            rec.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(&rec);
        let _ = r;
    }
    let t = Instant::now();
    let Outcome::Done(Some(Raster::Recipe(r))) = rasterize_special(&f, None, &mut Stub) else { panic!("not a recipe") };
    assert!(r.truncated, "the op cap was not reached");
    assert!(t.elapsed() < Duration::from_secs(20), "{:?}", t.elapsed());
    assert!(r.bytes.len() < 400 << 20);
}

/// The raster cache of the emitter (256 pictures, 64 MiB) is the Slides side; here the sizes of a raster are bounded: a metafile
/// declaring a huge frame, or a huge display, rasterizes on a canvas of at most 2,400 px a side.
#[test]
fn m5_the_canvas_size_is_capped() {
    let files = inputs();
    let (_, emf) = files.iter().find(|(k, _)| k == "emf/0000").unwrap();
    for display in [(1.0e9, 1.0e9), (1.0e-9, 1.0e-9), (f64::MAX, 1.0), (0.0, 5.0), (f64::NAN, f64::NAN), (-5.0, 5.0)] {
        match rasterize_special(emf, Some(display), &mut Stub) {
            Outcome::Done(Some(Raster::Recipe(r))) => {
                assert!(r.w <= 2401 && r.h <= 2401 && r.w >= 1 && r.h >= 1, "{display:?} {}x{}", r.w, r.h)
            }
            Outcome::Done(None) => {}
            Outcome::Need(_) => panic!("needs"),
        }
    }
}

// ─────────────────────────── bounded no-panic ───────────────────────────

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn damaged(b: &[u8], r: &mut Rng) -> Vec<u8> {
    let mut v = b.to_vec();
    match r.below(5) {
        0 => v.truncate(r.below(v.len().max(1))),
        1 => {
            for _ in 0..1 + r.below(8) {
                let i = r.below(v.len().max(1));
                if let Some(x) = v.get_mut(i) {
                    *x ^= 1 << r.below(8);
                }
            }
        }
        2 => {
            for _ in 0..1 + r.below(4) {
                let i = r.below(v.len().max(1));
                if let Some(x) = v.get_mut(i) {
                    *x = 0xff;
                }
            }
        }
        3 => {
            // a hostile 32-bit field near the start (sizes, counts)
            let i = r.below(v.len().clamp(4, 120)) & !3;
            if let Some(s) = v.get_mut(i..i + 4) {
                s.copy_from_slice(&[0xff, 0xff, 0xff, 0x7f]);
            }
        }
        _ => {
            let i = r.below(v.len().max(1));
            v.splice(i..i, std::iter::repeat_n(0u8, r.below(64)));
        }
    }
    v
}

#[test]
fn damaged_files_never_panic_and_stay_bounded() {
    let files = inputs();
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let t0 = Instant::now();
    let rounds: usize = if std::env::var_os("UO_DML_LONG").is_some() { 20_000 } else { 1_500 };
    let mut slowest = Duration::ZERO;
    for n in 0..rounds {
        let (key, bytes) = &files[r.below(files.len())];
        let d = damaged(bytes, &mut r);
        let t = Instant::now();
        if key.starts_with("eot") {
            let _ = uo_fonts::eot::read(&d);
        } else {
            let display = if n % 3 == 0 { Some((r.below(3000) as f64, r.below(2000) as f64)) } else { None };
            match rasterize_special(&d, display, &mut Stub) {
                Outcome::Done(Some(Raster::Recipe(rc))) => {
                    assert!(rc.w >= 1 && rc.h >= 1);
                    assert!(rc.bytes.len() < 300 << 20, "{key}: {}", rc.bytes.len());
                }
                Outcome::Done(None) | Outcome::Need(_) => {}
            }
        }
        slowest = slowest.max(t.elapsed());
    }
    assert!(slowest < Duration::from_secs(10), "slowest {slowest:?}");
    eprintln!("{rounds} damaged rasterizations in {:.1} s (slowest {slowest:?})", t0.elapsed().as_secs_f64());
}

/// Hostile fixed inputs: huge declared sizes with tiny data.
#[test]
fn hostile_declared_sizes_are_refused_or_bounded() {
    // a TIFF declaring 60,000 x 60,000
    let mut t = vec![0x49, 0x49, 42, 0, 8, 0, 0, 0, 3, 0];
    for (tag, val) in [(256u16, 60_000u32), (257, 60_000), (277, 1)] {
        t.extend_from_slice(&tag.to_le_bytes());
        t.extend_from_slice(&4u16.to_le_bytes());
        t.extend_from_slice(&1u32.to_le_bytes());
        t.extend_from_slice(&val.to_le_bytes());
    }
    t.extend_from_slice(&[0, 0, 0, 0]);
    assert!(matches!(rasterize_special(&t, None, &mut Stub), Outcome::Done(None)));
    // a PICT whose bitmap is 16,384 x 16,384
    let mut p = vec![0u8; 512];
    p.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x01, 0x2c, 0x01, 0x2c, 0x00, 0x11, 0x02, 0xff, 0x0c, 0x00]);
    p.extend_from_slice(&[0; 24]);
    p.extend_from_slice(&[0x00, 0x90, 0x00, 0x00, 0, 0, 0x40, 0x00, 0x40, 0x00]);
    p.extend_from_slice(&[0; 40]);
    let _ = rasterize_special(&p, None, &mut Stub);
    // an EOT whose MTX block declares 16 MiB from 40 bytes
    let mut e = vec![0u8; 82];
    let total = 82 + 40;
    e[0..4].copy_from_slice(&(total as u32).to_le_bytes());
    e[4..8].copy_from_slice(&40u32.to_le_bytes());
    e[12..16].copy_from_slice(&4u32.to_le_bytes());
    e[34] = 0x4c;
    e[35] = 0x50;
    e.extend_from_slice(&[3, 0, 0, 0, 0, 0, 0, 0, 0, 20]);
    e.extend_from_slice(&[0xff; 30]);
    assert!(uo_fonts::eot::read(&e).is_none());
}
