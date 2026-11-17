//! Frontier T: the item size budget (RA1.3), the optional number, the symbol table, the dense
//! ids and the TS `shiftItem` arithmetic of `ItemList::push_shifted` (table.ts:1392).

use std::mem::size_of;

use uo_render::types::{Item, PathCmd, RunFlags, Stroke};
use uo_render::{FaceId, GlyphRunHead, ItemList, OptF64, Sym, SymbolTable};

#[test]
fn an_item_is_at_most_48_bytes() {
    assert!(size_of::<Item>() <= 48, "Item is {} bytes", size_of::<Item>());
    assert_eq!(size_of::<OptF64>(), 8);
    assert_eq!(size_of::<Option<Sym>>(), 4);
    assert_eq!(size_of::<Option<Stroke>>(), 16);
}

#[test]
fn opt_f64_keeps_absent_apart_from_every_value() {
    for v in [
        0.0,
        -0.0,
        f64::NAN,
        -f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1e300,
        5e-324,
        f64::from_bits(0x7ff4_0000_0000_0001),
        f64::from_bits(u64::MAX),
        f64::from_bits(0x7ff8_0000_0000_0001),
        f64::MAX,
    ] {
        let o = OptF64::some(v);
        assert!(o.is_some());
        let got = o.get().unwrap();
        if v.is_nan() {
            assert_eq!(got.to_bits(), 0x7ff8_0000_0000_0000, "NaN is canonical");
        } else {
            assert_eq!(got.to_bits(), v.to_bits());
        }
    }
    assert_eq!(OptF64::NONE.get(), None);
    assert_ne!(OptF64::some(0.0), OptF64::NONE);
    assert_eq!(OptF64::from(Some(f64::NAN)), OptF64::some(f64::NAN));
    assert_eq!(OptF64::default().get(), None);
    assert_eq!(OptF64::from(None).or(7.0), 7.0);
}

#[test]
fn symbols_are_dense_in_first_use_order_and_keep_lone_surrogates() {
    let mut t = SymbolTable::new();
    let a = t.intern_str("ffffff").unwrap();
    let lone = [0xd800u16, 0x41];
    let b = t.intern(&lone).unwrap();
    assert_eq!(t.intern_str("ffffff"), Some(a));
    assert_eq!((a.index(), b.index()), (0, 1));
    assert_eq!(t.get(b), &lone);
    assert_eq!(t.since(1).count(), 1);
    assert_eq!(Sym::from_raw(0), None);
}

#[test]
fn push_shifted_is_the_ts_shift_item() {
    let mut t = SymbolTable::new();
    let c = t.intern_str("000000").unwrap();
    let mut src = ItemList::new();
    let head = GlyphRunHead {
        face: FaceId::from_index(0).unwrap(),
        color: c,
        size_pt: 11.0,
        y: 100.0,
        text: &[0x61, 0x62],
        scale_x: OptF64::NONE,
        alpha: OptF64::some(0.5),
        fx: None,
        flags: RunFlags::default().with(RunFlags::SYNTH_BOLD, true),
    };
    src.push_glyphs(head, [(0x61, 10.0, None), (0x62, 20.5, Some(7))]).unwrap();
    src.push_plain(Item::Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0, fill: c, alpha: OptF64::NONE });
    src.push_plain(Item::ClipPush { x: OptF64::NONE, y: OptF64::NONE, w: OptF64::some(5.0), h: OptF64::NONE });
    src.push_plain(Item::ClipPush { x: OptF64::NONE, y: OptF64::some(1.0), w: OptF64::NONE, h: OptF64::NONE });
    src.push_plain(Item::XformPush { m: Some(Box::new([2.0, 0.0, 0.0, 3.0, 10.0, 20.0])) });
    src.push_path(
        &[PathCmd::M { x: 0.0, y: 0.0 }, PathCmd::C { x1: 1.0, y1: 1.0, x2: 2.0, y2: 2.0, x: 3.0, y: 3.0 }, PathCmd::Z],
        None,
        None,
        true,
    )
    .unwrap();

    let mut dst = ItemList::new();
    dst.extend_shifted(&src, 5.0, 100.0).unwrap();
    assert_eq!(dst.len(), src.len());
    let Item::Glyphs { y, glyphs, run, flags, .. } = &dst.items[0] else { panic!("glyphs") };
    assert_eq!(*y, 105.0);
    assert!(flags.has(RunFlags::SYNTH_BOLD));
    let g: Vec<_> = dst.glyphs_of(*glyphs).collect();
    assert_eq!(g, vec![(0x61, 110.0, None), (0x62, 120.5, Some(7))]);
    let side = dst.run(*run).unwrap();
    assert_eq!(dst.text_of(side.text), &[0x61, 0x62]);
    assert_eq!(side.alpha.get(), Some(0.5));
    let Item::Rect { x, y, .. } = dst.items[1] else { panic!("rect") };
    assert_eq!((x, y), (101.0, 7.0));
    // a clip without y stays as it is; with y, x becomes (x ?? 0) + dx
    let Item::ClipPush { x, y, w, .. } = dst.items[2] else { panic!("clip") };
    assert_eq!((x.get(), y.get(), w.get()), (None, None, Some(5.0)));
    let Item::ClipPush { x, y, .. } = dst.items[3] else { panic!("clip") };
    assert_eq!((x.get(), y.get()), (Some(100.0), Some(6.0)));
    // M'(p + d) = M(p) + d
    let Item::XformPush { m: Some(m) } = &dst.items[4] else { panic!("xform") };
    assert_eq!(**m, [2.0, 0.0, 0.0, 3.0, 10.0 + 100.0 - 2.0 * 100.0, 20.0 + 5.0 - 3.0 * 5.0]);
    let Item::Path { cmds, even_odd, .. } = dst.items[5] else { panic!("path") };
    assert!(even_odd);
    assert!(matches!(dst.cmds_of(cmds)[1], PathCmd::C { x1: 101.0, y1: 6.0, x: 103.0, y: 8.0, .. }));

    // dx = 0 or NaN: the glyph x stay (the TS `dx ? … : it.glyphs`)
    let mut nan = ItemList::new();
    nan.push_shifted(&src, 0, 0.0, f64::NAN).unwrap();
    let Item::Glyphs { glyphs, .. } = nan.items[0] else { panic!("glyphs") };
    assert_eq!(nan.glyphs_of(glyphs).map(|g| g.1).collect::<Vec<_>>(), vec![10.0, 20.5]);
}
