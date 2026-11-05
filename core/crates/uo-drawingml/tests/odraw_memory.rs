//! Memory gate of `odraw` (M1, M2, M3; M5: the module has no cache): every call returns the live
//! bytes to their value before it with the outputs dropped, a soak over the whole corpus is flat,
//! and the cost of the reads and of a metafile inflate is recorded (printed with `--nocapture`;
//! the numbers are in the module record).
#![cfg(feature = "legacy")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod odraw_support;

use std::hint::black_box;

use odraw_support::{docs, records};
use uo_alloc::{assert_flat, assert_no_leak, measure, soak};
use uo_drawingml::odraw::msospt::{PresetVariant, preset_for};
use uo_drawingml::odraw::{
    BlipBudget, Flavor, OdrawLimits, PropTable, decode_blip, read_fopt, read_rec, read_spgr_container, read_word_drawings, recs_in, rt,
};

const L: OdrawLimits = OdrawLimits::WORD;

/// What D4 does with a document's drawings: read them and decode every BLIP of the store.
fn read_all(d: &odraw_support::Doc) -> usize {
    let dr = read_word_drawings(&d.table, d.fc, d.lcb, &L);
    let mut budget = BlipBudget::document();
    let mut n = dr.main.len() + dr.hdr.len();
    for e in dr.bstore.iter().flatten() {
        let blip = match &e.embedded {
            Some(r) => decode_blip(r, &mut budget, Flavor::Word, &L),
            None => d.word.as_deref().and_then(|w| {
                let rec = read_rec(w, u64::from(e.fo_delay), w.len() as u64)?;
                decode_blip(&rec, &mut budget, Flavor::Word, &L)
            }),
        };
        n += blip.map_or(0, |b| b.bytes.len());
    }
    n
}

/// A metafile BLIP (EMF, one UID) whose data is `payload` deflated with zlib.
fn emf_blip(payload: &[u8]) -> Vec<u8> {
    let z = uo_opc::flate::zlib(payload, 6);
    let mut body = vec![0u8; 16];
    body.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    body.extend_from_slice(&[0; 24]);
    body.extend_from_slice(&u32::try_from(z.len()).unwrap().to_le_bytes());
    body.extend_from_slice(&[0, 0xfe]);
    body.extend_from_slice(&z);
    let mut rec = Vec::new();
    rec.extend_from_slice(&((0x3d4u16 << 4).to_le_bytes()));
    rec.extend_from_slice(&rt::BLIP_EMF.to_le_bytes());
    rec.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    rec.extend_from_slice(&body);
    rec
}

#[test]
fn m1_every_operation() {
    let docs = docs();
    for d in &docs {
        assert_no_leak(|| black_box(read_all(d)));
        assert_no_leak(|| black_box(read_word_drawings(&d.table, d.fc, d.lcb, &L)));
    }
    for b in records("blip/") {
        let rec = read_rec(&b, 0, b.len() as u64).unwrap();
        for flavor in [Flavor::Word, Flavor::Slides, Flavor::Sheet] {
            for left in [BlipBudget::document().left, 0, 100] {
                assert_no_leak(|| black_box(decode_blip(&rec, &mut BlipBudget { left }, flavor, &L)));
            }
        }
    }
    for b in records("fopt/") {
        for flavor in [Flavor::Word, Flavor::Slides, Flavor::Sheet] {
            assert_no_leak(|| {
                let mut t = PropTable::new();
                for c in recs_in(&b, 0, b.len() as u64) {
                    read_fopt(&c, &mut t, flavor);
                }
                black_box(t)
            });
        }
    }
    let adj = [Some(5400.0), None, Some(-1.0)];
    for spt in 0..=256u16 {
        assert_no_leak(|| black_box(preset_for(spt, &adj, 3.0, 2.0, PresetVariant::WordSlides)));
        assert_no_leak(|| black_box(preset_for(spt, &adj, 0.0, f64::NAN, PresetVariant::Sheet)));
    }
    uo_drawingml::clear_caches();
}

#[test]
fn m1_inflate_cap_and_budget() {
    let payload = vec![7u8; 3 << 20];
    let blip = emf_blip(&payload);
    let rec = read_rec(&blip, 0, blip.len() as u64).unwrap();
    // under the cap: the whole picture
    let u = assert_no_leak(|| {
        let b = decode_blip(&rec, &mut BlipBudget::document(), Flavor::Word, &L).unwrap();
        assert_eq!(b.bytes.len(), payload.len());
        b
    });
    eprintln!("odraw M3: 3 MiB EMF inflate: allocated {} B, peak {} B", u.allocated, u.peak_above_start);
    assert!(u.peak_above_start < 3 * (3 << 20), "peak {}", u.peak_above_start);
    // over a 1 MiB cap and over a small budget: nothing, nothing kept, budget untouched
    let small = OdrawLimits { max_blip_bytes: 1 << 20, ..L };
    let u = assert_no_leak(|| assert!(decode_blip(&rec, &mut BlipBudget::document(), Flavor::Word, &small).is_none()));
    eprintln!("odraw M3: refused at a 1 MiB cap: allocated {} B, peak {} B", u.allocated, u.peak_above_start);
    assert!(u.peak_above_start < 3 * (1 << 20), "peak {}", u.peak_above_start);
    let mut budget = BlipBudget { left: 1000 };
    assert_no_leak(|| assert!(decode_blip(&rec, &mut budget, Flavor::Word, &L).is_none()));
    assert_eq!(budget.left, 1000);
    // the budget shrinks by what was inflated
    let mut budget = BlipBudget { left: 4 << 20 };
    assert!(decode_blip(&rec, &mut budget, Flavor::Word, &L).is_some());
    assert_eq!(budget.left, (4 << 20) - (3 << 20));
    assert!(decode_blip(&rec, &mut budget, Flavor::Word, &L).is_none());
}

#[test]
fn m3_reads() {
    let docs = docs();
    let mut worst = (0.0f64, String::new());
    for d in &docs {
        let (_, u) = measure(|| black_box(read_all(d)));
        let input = u64::from(d.lcb).max(1);
        if d.key.starts_with("doc/") {
            eprintln!("odraw M3: {} (lcb {}): allocated {} B, peak {} B", d.key, d.lcb, u.allocated, u.peak_above_start);
        }
        let ratio = u.allocated as f64 / input as f64;
        if ratio > worst.0 && d.lcb > 64 {
            worst = (ratio, d.key.clone());
        }
    }
    eprintln!("odraw M3: worst allocated/lcb {:.1} ({})", worst.0, worst.1);
}

#[test]
fn m2_soak() {
    let docs = docs();
    let blips = records("blip/");
    let series = soak(200, 10, |i| {
        for d in &docs {
            black_box(read_all(d));
        }
        let b = &blips[i % blips.len()];
        let rec = read_rec(b, 0, b.len() as u64).unwrap();
        black_box(decode_blip(&rec, &mut BlipBudget::document(), Flavor::Word, &L));
    });
    assert_flat(&series, 1, 0);
    eprintln!("odraw M2: 200 rounds over {} documents, live bytes {:?}", docs.len(), series.first());
}

#[test]
fn deep_nesting_is_bounded() {
    // 100,000 nested SpgrContainers, each declaring the rest as its data
    let depth = 100_000u32;
    let mut bytes = Vec::with_capacity(depth as usize * 8);
    for i in 0..depth {
        bytes.extend_from_slice(&0x000fu16.to_le_bytes());
        bytes.extend_from_slice(&rt::SPGR_CONTAINER.to_le_bytes());
        bytes.extend_from_slice(&((depth - i - 1) * 8).to_le_bytes());
    }
    let rec = read_rec(&bytes, 0, bytes.len() as u64).unwrap();
    let g = read_spgr_container(&rec, Flavor::Word, &L);
    let mut levels = 0;
    let mut at = &g;
    while let Some(c) = at.children.as_ref().and_then(|c| c.first()) {
        levels += 1;
        at = c;
    }
    assert_eq!(levels, L.max_group_depth);
}

#[test]
fn shape_and_master_limits() {
    // a DgContainer with 10 loose SpContainers of the same master-less shape: max_shapes 4
    let sp = |spid: u32, props: &[(u16, u32)]| {
        let mut fsp = vec![0x20, 0x00, 0x0a, 0xf0, 8, 0, 0, 0];
        fsp.extend_from_slice(&spid.to_le_bytes());
        fsp.extend_from_slice(&0u32.to_le_bytes());
        let n = u16::try_from(props.len()).unwrap();
        let mut fopt = Vec::new();
        fopt.extend_from_slice(&((n << 4) | 3).to_le_bytes());
        fopt.extend_from_slice(&rt::FOPT.to_le_bytes());
        fopt.extend_from_slice(&(u32::from(n) * 6).to_le_bytes());
        for &(id, v) in props {
            fopt.extend_from_slice(&id.to_le_bytes());
            fopt.extend_from_slice(&v.to_le_bytes());
        }
        let body = [fsp, fopt].concat();
        let mut r = vec![0x0f, 0x00, 0x04, 0xf0];
        r.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
        r.extend_from_slice(&body);
        r
    };
    let master: Vec<(u16, u32)> = (0..100u16).map(|i| (0x100 + i, u32::from(i))).collect();
    let mut kids = sp(1, &master);
    for i in 2..12 {
        kids.extend_from_slice(&sp(i, &[(0x301, 1)]));
    }
    let mut dg = vec![0x0f, 0x00, 0x02, 0xf0];
    dg.extend_from_slice(&u32::try_from(kids.len()).unwrap().to_le_bytes());
    dg.extend_from_slice(&kids);
    let rec = read_rec(&dg, 0, dg.len() as u64).unwrap();
    let mut map = uo_drawingml::odraw::ShapeMap::new();
    uo_drawingml::odraw::index_drawing(&rec, &mut map, &OdrawLimits { max_shapes: 4, ..L });
    assert_eq!(map.len(), 4);
    let mut map = uo_drawingml::odraw::ShapeMap::new();
    uo_drawingml::odraw::index_drawing(&rec, &mut map, &L);
    assert_eq!(map.len(), 11);
    let mut masters = std::collections::BTreeSet::new();
    // 250 inherited properties allowed: two shapes inherit their 100, the third would pass it
    uo_drawingml::odraw::apply_masters(&mut map, &mut masters, &OdrawLimits { max_master_props: 250, ..L });
    let inherited: Vec<usize> = map.iter().skip(1).map(|s| s.props.len()).collect();
    assert_eq!(inherited, [101, 101, 1, 1, 1, 1, 1, 1, 1, 1]);
    assert_eq!(masters.into_iter().collect::<Vec<_>>(), [1]);
}
