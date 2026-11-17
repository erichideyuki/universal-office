//! Bounded no-panic tests of the `.ppt` converter (frontier B2, security, seconds): the 25 samples
//! (and their streams) under seeded mutations — random byte edits, 4-byte writes of hostile lengths
//! (`0`, `0x7FFFFFFF`, `0xFFFFFFFF`) at 4-aligned offsets, cuts, zero and 0xFF fills, block copies —
//! and synthetic streams whose containers nest 100,000 deep (the recursions have bounds: a stack
//! overflow would be a finding). Errors are expected; a panic, a hang or an unbounded allocation is a
//! finding. Debug builds (overflow checks on) are the point: `cargo test -p uo-pptx --features ppt`.
//! `UO_PPT_NOPANIC_ROUNDS=<n>` multiplies the mutants per sample for a long run.
#![cfg(feature = "ppt")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use uo_pptx::ppt::{ConvertOptions, convert_ppt_to_pptx_parts, ppt_to_pptx_parts, read_ppt_document};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { usize::try_from(self.next() % n as u64).unwrap_or(0) }
    }
}

fn samples() -> Vec<(String, Vec<u8>)> {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../slides/samples");
    let mut out = Vec::new();
    for sub in ["legacy", "playback"] {
        let mut names: Vec<_> = std::fs::read_dir(dir.join(sub))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ppt"))
            .collect();
        names.sort();
        for p in names {
            out.push((p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()));
        }
    }
    out
}

fn opts() -> ConvertOptions {
    ConvertOptions { today: Some((2026, 11, 17)), ..ConvertOptions::default() }
}

fn rounds() -> usize {
    std::env::var("UO_PPT_NOPANIC_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(1)
}

/// One mutant of `b`.
fn mutate(rng: &mut Rng, b: &[u8]) -> Vec<u8> {
    let mut m = b.to_vec();
    if m.is_empty() {
        return m;
    }
    match rng.below(8) {
        0 => {
            for _ in 0..1 + rng.below(16) {
                let i = rng.below(m.len());
                m[i] = u8::try_from(rng.next() & 0xff).unwrap();
            }
        }
        1 | 2 => {
            // hostile 32-bit values at aligned offsets (lengths, counts, offsets of the records)
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(m.len() / 4) * 4;
                let v: u32 = [0, 0x7fff_ffff, 0xffff_ffff, 0x8000_0000, 1, 0x0001_0000][rng.below(6)];
                if let Some(s) = m.get_mut(i..i + 4) {
                    s.copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        3 => m.truncate(rng.below(m.len())),
        4 => {
            let (s, l) = (rng.below(m.len()), 1 + rng.below(4096));
            for x in m.iter_mut().skip(s).take(l) {
                *x = 0;
            }
        }
        5 => {
            let (s, l) = (rng.below(m.len()), 1 + rng.below(4096));
            for x in m.iter_mut().skip(s).take(l) {
                *x = 0xff;
            }
        }
        6 => {
            let (s, d, l) = (rng.below(m.len()), rng.below(m.len()), 1 + rng.below(8192));
            let chunk: Vec<u8> = m.iter().skip(s).take(l).copied().collect();
            for (k, x) in chunk.into_iter().enumerate() {
                if let Some(slot) = m.get_mut(d + k) {
                    *slot = x;
                }
            }
        }
        _ => {
            // flip single bits
            for _ in 0..1 + rng.below(32) {
                let i = rng.below(m.len());
                m[i] ^= 1 << rng.below(8);
            }
        }
    }
    m
}

#[test]
fn mutated_files_never_panic() {
    let t = Instant::now();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut ok, mut err) = (0usize, 0usize);
    for (_, bytes) in samples() {
        for _ in 0..4 * rounds() {
            let m = mutate(&mut rng, &bytes);
            match convert_ppt_to_pptx_parts(&m, &opts()) {
                Ok(p) => {
                    ok += 1;
                    black_box(p.len());
                }
                Err(e) => {
                    err += 1;
                    black_box(e.message());
                }
            }
        }
    }
    eprintln!("whole files: {ok} converted, {err} refused, {:?}", t.elapsed());
}

/// The streams of a sample, mutated one by one and put through the reader and the writer: the
/// bytes the record parsers see are the mutated ones (a flip in the whole file mostly lands in the
/// FAT or in a stream the converter never reads).
#[test]
fn mutated_streams_never_panic() {
    let t = Instant::now();
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let (mut ok, mut err) = (0usize, 0usize);
    for (_, bytes) in samples() {
        let cfb = uo_opc::cfb::CfbFile::open(&bytes).unwrap();
        let doc = cfb.stream("PowerPoint Document").unwrap().unwrap();
        let pics = cfb.stream("Pictures").unwrap();
        let cu = cfb.stream("Current User").unwrap();
        for _ in 0..12 * rounds() {
            let md = mutate(&mut rng, &doc);
            let mp = pics.as_ref().map(|p| if rng.below(3) == 0 { mutate(&mut rng, p) } else { p.clone() });
            let mc = cu.as_ref().map(|c| if rng.below(4) == 0 { mutate(&mut rng, c) } else { c.clone() });
            match read_ppt_document(&md, mp.as_deref(), mc.as_deref()) {
                Ok(d) => match ppt_to_pptx_parts(&d, &opts(), (2026, 11, 17)) {
                    Ok(p) => {
                        ok += 1;
                        black_box(p.len());
                    }
                    Err(e) => {
                        err += 1;
                        black_box(e.message());
                    }
                },
                Err(e) => {
                    err += 1;
                    black_box(e.message());
                }
            }
        }
    }
    eprintln!("streams: {ok} converted, {err} refused, {:?}", t.elapsed());
}

// ───────────────────────── synthetic streams ─────────────────────────

fn header(ver: u8, inst: u16, ty: u16, len: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(8);
    h.extend_from_slice(&(u16::from(ver) | (inst << 4)).to_le_bytes());
    h.extend_from_slice(&ty.to_le_bytes());
    h.extend_from_slice(&len.to_le_bytes());
    h
}

fn rec(ver: u8, inst: u16, ty: u16, body: &[u8]) -> Vec<u8> {
    let mut v = header(ver, inst, ty, u32::try_from(body.len()).unwrap());
    v.extend_from_slice(body);
    v
}

/// `depth` containers of type `ty` nested, `leaf` inside: built outermost first in one pass.
fn nested(ty: u16, depth: usize, leaf: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(depth * 8 + leaf.len());
    for i in 0..depth {
        let inner = (depth - i - 1) * 8 + leaf.len();
        out.extend(header(0xf, 0, ty, u32::try_from(inner).unwrap()));
    }
    out.extend_from_slice(leaf);
    out
}

/// A "PowerPoint Document" stream with one slide: `slide_children` go inside the SlideContainer.
fn synth(doc_extra: &[u8], slide_children: &[u8]) -> Vec<u8> {
    let slide_atom = rec(0, 0, 0x03ef, &[0; 24]);
    let slide = rec(0xf, 0, 0x03ee, &[slide_atom, slide_children.to_vec()].concat());
    let mut persist_atom = vec![0u8; 20];
    persist_atom[..4].copy_from_slice(&2u32.to_le_bytes());
    persist_atom[12..16].copy_from_slice(&256u32.to_le_bytes());
    let list = rec(0xf, 0, 0x0ff0, &rec(0, 0, 0x03f3, &persist_atom));
    let doc_atom = rec(0, 0, 0x03e9, &[0; 48]);
    let document = rec(0xf, 0, 0x03e8, &[doc_atom, doc_extra.to_vec(), list].concat());
    let mut stream = document.clone();
    let slide_off = stream.len();
    stream.extend_from_slice(&slide);
    let pd_off = stream.len();
    // PersistDirectoryAtom: (id 1, count 2): the document at 0 and the slide
    let mut pd = Vec::new();
    pd.extend_from_slice(&((2u32 << 20) | 1).to_le_bytes());
    pd.extend_from_slice(&0u32.to_le_bytes());
    pd.extend_from_slice(&u32::try_from(slide_off).unwrap().to_le_bytes());
    stream.extend(rec(0, 0, 0x1772, &pd));
    let mut ue = vec![0u8; 28];
    ue[12..16].copy_from_slice(&u32::try_from(pd_off).unwrap().to_le_bytes());
    ue[16..20].copy_from_slice(&1u32.to_le_bytes());
    stream.extend(rec(0, 0, 0x0ff5, &ue));
    stream
}

fn run_synth(doc_extra: &[u8], slide_children: &[u8]) -> (bool, usize) {
    let stream = synth(doc_extra, slide_children);
    match read_ppt_document(&stream, None, None) {
        Ok(d) => {
            let n = d.slides.len();
            let ok = ppt_to_pptx_parts(&d, &opts(), (2026, 11, 17)).is_ok();
            (ok, n)
        }
        Err(e) => {
            black_box(e.message());
            (false, 0)
        }
    }
}

#[test]
fn a_clean_synthetic_slide_converts() {
    let (ok, slides) = run_synth(&[], &[]);
    assert!(ok);
    assert_eq!(slides, 1);
}

#[test]
fn deeply_nested_tag_lists_are_an_error_not_a_stack_overflow() {
    let (ok, _) = run_synth(&nested(0x07d0, 100_000, &[]), &[]);
    assert!(!ok);
    let (ok, _) = run_synth(&[], &nested(0x1388, 100_000, &[]));
    assert!(!ok);
}

#[test]
fn deeply_nested_drawings_and_animations_stay_bounded() {
    // a drawing: Drawing → DgContainer → SpgrContainer nested 100,000 deep
    let spgr = nested(0xf003, 100_000, &[]);
    let dg = rec(0xf, 0, 0xf002, &spgr);
    let drawing = rec(0xf, 0, 0x040c, &dg);
    let (_, slides) = run_synth(&[], &drawing);
    assert_eq!(slides, 1);
    // timing: a "___PPT10" tag whose ExtTimeNodeContainer nests 100,000 deep
    let name = rec(0, 0, 0x0fba, &"___PPT10\0".encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>());
    let tree = nested(0xf144, 100_000, &[]);
    let blob = rec(0, 0, 0x138b, &tree);
    let tag = rec(0xf, 0, 0x138a, &[name, blob].concat());
    let tags = rec(0xf, 0, 0x1388, &tag);
    let (_, slides) = run_synth(&[], &tags);
    assert_eq!(slides, 1);
}

#[test]
fn hostile_property_tables_and_anchors_stay_bounded() {
    // a shape container with a FOPT of 4,095 entries all complex, a huge fill and a client anchor
    let mut fopt = Vec::new();
    for i in 0..4095u32 {
        fopt.extend_from_slice(&(0x8000u16 | u16::try_from(0x180 + (i % 64)).unwrap()).to_le_bytes());
        fopt.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    }
    let fsp = rec(2, 1, 0xf00a, &[0u8; 8]);
    let opt = rec(3, 4095, 0xf00b, &fopt);
    let anchor = rec(0, 0, 0xf010, &[0xff; 16]);
    let sp = rec(0xf, 0, 0xf004, &[fsp, opt, anchor].concat());
    let spgr = rec(0xf, 0, 0xf003, &sp.repeat(2000));
    let dg = rec(0xf, 0, 0xf002, &spgr);
    let drawing = rec(0xf, 0, 0x040c, &dg);
    let (_, slides) = run_synth(&[], &drawing);
    assert_eq!(slides, 1);
}
