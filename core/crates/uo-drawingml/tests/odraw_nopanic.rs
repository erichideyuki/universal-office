//! No panic, no hang and the limits respected on hostile OfficeArt (overflow checks on in the
//! test profile): the corpus of the capture mutated byte by byte, with lengths rewritten, cut at
//! random points and read at random `fc`/`lcb`, plus random bytes, through every entry point and
//! the three flavors.
#![cfg(feature = "legacy")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod odraw_support;

use std::hint::black_box;
use std::time::{Duration, Instant};

use odraw_support::{docs, records};
use uo_drawingml::odraw::msospt::{PresetVariant, has_preset, is_connector_type, is_word_art_type, preset_for, raw_adjust};
use uo_drawingml::odraw::{
    BlipBudget, Flavor, OdrawLimits, PropTable, Shape, decode_blip, read_bstore, read_fbse, read_fopt, read_rec, read_sp_container,
    read_spgr_container, read_word_drawings, recs_in, rt,
};
use uo_testkit::generate::Rng;

fn count(s: &Shape<'_>) -> u64 {
    1 + s.children.iter().flatten().map(count).sum::<u64>()
}

/// Every entry point over one buffer.
fn exercise(b: &[u8], fc: u32, lcb: u32, limits: &OdrawLimits, rng: &mut Rng) {
    let t0 = Instant::now();
    let d = read_word_drawings(b, fc, lcb, limits);
    let shapes: u64 = d.main.iter().chain(d.hdr.iter()).map(count).sum();
    assert!(shapes <= u64::from(limits.max_shapes), "{shapes} shapes");
    let mut budget = BlipBudget { left: u64::from(rng.next_u32() % (1 << 20)) };
    for e in d.bstore.iter().flatten() {
        if let Some(r) = &e.embedded {
            for flavor in [Flavor::Word, Flavor::Slides, Flavor::Sheet] {
                let before = budget.left;
                if let Some(blip) = decode_blip(r, &mut budget, flavor, limits) {
                    assert!(before - budget.left <= limits.max_blip_bytes);
                    black_box(blip.kind.mime());
                }
            }
        }
        black_box(d.bse(e.size));
    }
    for s in d.main.iter() {
        black_box(raw_adjust(&s.props));
        black_box(preset_for(s.shape_type, &raw_adjust(&s.props), f64::from(s.flags), 1.0, PresetVariant::WordSlides));
        black_box(d.shape(s.spid, true));
    }
    black_box(d.background_shape());
    // every record of the buffer at a random offset, read as each kind
    let start = u64::from(rng.next_u32()) % (b.len() as u64 + 1);
    for r in recs_in(b, start, b.len() as u64).take(64) {
        for flavor in [Flavor::Word, Flavor::Slides, Flavor::Sheet] {
            let mut t = PropTable::new();
            read_fopt(&r, &mut t, flavor);
            assert!(t.len() <= 4095);
            black_box(t.get_complex(0x145));
            black_box(read_sp_container(&r, flavor));
            let g = read_spgr_container(&r, flavor, limits);
            assert!(count(&g) <= u64::from(limits.max_shapes) + 1);
            black_box(decode_blip(&r, &mut BlipBudget { left: 1 << 16 }, flavor, limits));
        }
        black_box(read_bstore(&r));
        black_box(read_fbse(&r));
        black_box(r.child(rt::FSP));
    }
    assert!(t0.elapsed() < Duration::from_secs(2), "slow input: {:?}", t0.elapsed());
}

#[test]
fn no_panic_on_mutated_corpus() {
    let docs = docs();
    let mut rng = Rng::new(0x0d4a_7e57);
    let small = OdrawLimits { max_blip_bytes: 1 << 16, max_group_depth: 8, max_word_drawings: 4, max_shapes: 300, max_master_props: 2000 };
    let rounds = if cfg!(debug_assertions) { 120 } else { 600 };
    for _ in 0..rounds {
        for d in &docs {
            let lo = usize::try_from(d.fc).unwrap().min(d.table.len());
            let hi = (lo + usize::try_from(d.lcb).unwrap()).min(d.table.len());
            let mut b = d.table.get(lo..hi).unwrap_or(&[]).to_vec();
            if b.is_empty() {
                continue;
            }
            let edits = 1 + rng.below(12);
            for _ in 0..edits {
                let at = rng.below(b.len());
                match rng.below(4) {
                    // a length field: rewrite four bytes
                    0 if at + 4 <= b.len() => {
                        let v = [0, 1, 7, 8, 0xffff_ffff, 0x7fff_ffff, rng.next_u32() % 4096][rng.below(7)];
                        b[at..at + 4].copy_from_slice(&u32::to_le_bytes(v));
                    }
                    1 => b[at] = 0x0f,
                    _ => b[at] = u8::try_from(rng.next_u32() & 0xff).unwrap(),
                }
            }
            if rng.one_in(5) {
                b.truncate(rng.below(b.len() + 1));
            }
            let lcb = u32::try_from(b.len()).unwrap();
            let limits = if rng.one_in(2) { &small } else { &OdrawLimits::WORD };
            exercise(&b, 0, lcb, limits, &mut rng);
            let fc = rng.next_u32() % (lcb + 1);
            exercise(&b, fc, rng.next_u32() % (lcb - fc + 2), limits, &mut rng);
        }
    }
    for _ in 0..2000 {
        let n = rng.below(400);
        let b = rng.bytes(n);
        exercise(&b, 0, u32::try_from(n).unwrap(), &small, &mut rng);
    }
    for b in records("blip/").iter().chain(records("fopt/").iter()) {
        let mut b = b.clone();
        for _ in 0..20 {
            let at = rng.below(b.len());
            b[at] = u8::try_from(rng.next_u32() & 0xff).unwrap();
            let r = read_rec(&b, 0, b.len() as u64).unwrap();
            for flavor in [Flavor::Word, Flavor::Slides, Flavor::Sheet] {
                black_box(decode_blip(&r, &mut BlipBudget { left: u64::from(rng.next_u32()) }, flavor, &small));
                read_fopt(&r, &mut PropTable::new(), flavor);
            }
        }
    }
}

#[test]
fn presets_on_any_number() {
    let specials = [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300, 5e-324, 1.0, -1.0];
    let mut rng = Rng::new(7);
    for spt in 0..=u16::MAX {
        let (w, h) = (specials[usize::from(spt) % specials.len()], specials[rng.below(specials.len())]);
        let adj: Vec<Option<f64>> = (0..rng.below(12)).map(|_| (!rng.one_in(3)).then(|| specials[rng.below(specials.len())])).collect();
        for v in [PresetVariant::WordSlides, PresetVariant::Sheet] {
            if let Some(r) = preset_for(spt, &adj, w, h, v) {
                assert!(r.av().count() <= 2);
                assert!(has_preset(spt) || (v == PresetVariant::WordSlides && is_word_art_type(spt)));
            }
        }
        assert!(!(is_word_art_type(spt) && is_connector_type(spt)));
    }
}
