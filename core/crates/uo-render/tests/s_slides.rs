//! R-S1/R-S2: the Slides display list types, the resources, the encoder/decoder stream, `validate`, the frame message,
//! and the proof that Write's schema (magic, hash) is untouched by the new ones.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_possible_truncation, clippy::cast_precision_loss)]

mod s_support;

use s_support::*;
use uo_render::slides::*;
use uo_testkit::generate::Rng;

type Mutation = Box<dyn Fn(&mut SlidesList)>;

fn encode_one(list: &SlidesList, res: &SlidesResources) -> Vec<u8> {
    let mut out = Vec::new();
    SlidesEncoder::new(1).encode(list, res, &mut out).unwrap();
    out
}

#[test]
fn sample_round_trips_with_exact_bytes_and_reports_its_size() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    let bytes = encode_one(&list, &res);
    let mut dec = SlidesDecoder::new();
    let back = dec.decode(&bytes).unwrap();
    assert_eq!(back, list);
    // canonical: decode then encode gives the input bytes
    assert_eq!(encode_one(&back, &res), bytes);
    // the tables the decoder holds are the producer's
    assert_eq!(dec.tables().lens(), res.lens());
    let tables = dec.tables();
    assert_eq!(tables.sym_units(tables.faces[0].family), utf16("Calibri").as_slice());
    let tables_bytes = {
        let mut empty = list.clone();
        empty.items.clear();
        empty.paths.clear();
        empty.stops.clear();
        empty.dashes.clear();
        empty.texts.clear();
        empty.gradients.clear();
        empty.patterns.clear();
        empty.runs.clear();
        empty.effects.clear();
        empty.slots.clear();
        encode_one(&empty, &res).len()
    };
    eprintln!(
        "SLIDES_SIZE: {} items, {} B wire total ({} B tables + header, {} B list), {:.1} B/item; size_of SItem = {}, TextRun = {}, SlidesList = {}",
        list.items.len(),
        bytes.len(),
        tables_bytes,
        bytes.len() - tables_bytes,
        (bytes.len() - tables_bytes) as f64 / list.items.len() as f64,
        std::mem::size_of::<SItem>(),
        std::mem::size_of::<TextRun>(),
        std::mem::size_of::<SlidesList>(),
    );
}

#[test]
fn second_message_carries_only_new_table_entries() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    let mut enc = SlidesEncoder::new(7);
    let mut dec = SlidesDecoder::new();
    let mut first = Vec::new();
    enc.encode(&list, &res, &mut first).unwrap();
    assert_eq!(enc.sent(), res.lens());
    dec.decode(&first).unwrap();
    // the same list again: no table entries, so a much smaller message
    let mut second = Vec::new();
    enc.encode(&list, &res, &mut second).unwrap();
    assert!(second.len() < first.len() - 150, "{} vs {}", second.len(), first.len());
    assert_eq!(dec.decode(&second).unwrap(), list);
    // a resize re-emits the list with one new colour and face
    let extra = res.sym_str("#ABCDEF").unwrap();
    let face = res.face(extra, false, true).unwrap();
    let mut resized = list.clone();
    resized.view.scale *= 2.0;
    resized.items.push(SItem::FillStyle(Paint::Css(extra)));
    resized.items.push(SItem::Font(FontRef { face, size_px: 9.0 }));
    let mut third = Vec::new();
    enc.encode(&resized, &res, &mut third).unwrap();
    assert_eq!(dec.decode(&third).unwrap(), resized);
    assert_eq!(dec.tables().lens(), res.lens());
    // a new generation: the decoder drops its tables, so the producer sends them all again
    let mut enc2 = SlidesEncoder::new(8);
    let mut fourth = Vec::new();
    enc2.encode(&resized, &res, &mut fourth).unwrap();
    assert_eq!(dec.decode(&fourth).unwrap(), resized);
    assert!(fourth.len() > third.len(), "the new generation resends every table entry");
}

#[test]
fn encoder_refuses_inconsistent_lists_and_keeps_out_and_mark() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    let lens = res.lens();
    let mutate: Vec<(&str, Mutation)> = vec![
        ("path span", Box::new(|l| l.items.push(SItem::AppendPath(Span { start: 5, len: 100 })))),
        ("path span overflow", Box::new(|l| l.items.push(SItem::AppendPath(Span { start: u32::MAX, len: u32::MAX })))),
        ("dash span", Box::new(|l| l.items.push(SItem::LineDash(Span { start: 0, len: 3 })))),
        ("run index", Box::new(|l| l.items.push(SItem::Text(99)))),
        ("effect index", Box::new(|l| l.items.push(SItem::Effect(1)))),
        ("slot index", Box::new(|l| l.items.push(SItem::SlotBegin(1)))),
        ("gradient index", Box::new(|l| l.items.push(SItem::FillStyle(Paint::Radial(1))))),
        ("pattern index", Box::new(|l| l.items.push(SItem::StrokeStyle(Paint::Pattern(1))))),
        ("symbol id", Box::new(move |l| l.items.push(SItem::ShadowColor(lens[0] as u32)))),
        ("face id", Box::new(move |l| l.items.push(SItem::Font(FontRef { face: lens[1] as u32, size_px: 1.0 })))),
        (
            "media id",
            Box::new(move |l| {
                l.items.push(SItem::DrawImage(DrawImageItem { src: ImageSrc::Media(lens[2] as u32), s: None, d: r4(0.0, 0.0, 1.0, 1.0) }))
            }),
        ),
        (
            "derived id",
            Box::new(move |l| {
                l.items.push(SItem::DrawImage(DrawImageItem { src: ImageSrc::Derived(lens[3] as u32), s: None, d: r4(0.0, 0.0, 1.0, 1.0) }))
            }),
        ),
        (
            "bitmap id",
            Box::new(move |l| {
                l.items.push(SItem::DrawImage(DrawImageItem { src: ImageSrc::Bitmap(lens[4] as u32), s: None, d: r4(0.0, 0.0, 1.0, 1.0) }))
            }),
        ),
        ("ctm register", Box::new(|l| l.items.push(SItem::CaptureCtm(CTM_REGISTERS)))),
        ("layer id", Box::new(|l| l.items.push(SItem::DrawLayer(MAX_LAYERS)))),
        (
            "layer fallback behind",
            Box::new(|l| {
                let at = u32::try_from(l.items.len()).unwrap();
                l.items.push(SItem::LayerBegin(LayerBeginItem { layer: 0, fallback_end: at }));
            }),
        ),
        (
            "layer fallback past end",
            Box::new(|l| {
                let at = u32::try_from(l.items.len()).unwrap();
                l.items.push(SItem::LayerBegin(LayerBeginItem { layer: 0, fallback_end: at + 5 }));
            }),
        ),
        ("text span", Box::new(|l| l.runs[0].text.len = 10_000)),
        ("run font", Box::new(move |l| l.runs[0].font.face = lens[1] as u32)),
        ("run underline colour", Box::new(move |l| l.runs[1].underline.as_mut().unwrap().color = lens[0] as u32)),
        ("gradient stop span", Box::new(|l| l.gradients[0].stops.len = 3)),
        ("stop colour", Box::new(move |l| l.stops[0].color = lens[0] as u32)),
        ("pattern bitmap", Box::new(move |l| l.patterns[0].src = PatternSrc::Bitmap(lens[4] as u32))),
        (
            "warp font",
            Box::new(move |l| {
                if let Effect::Warp(w) = &mut l.effects[0] {
                    w.lines[0].runs[0].font.face = lens[1] as u32;
                }
            }),
        ),
        (
            "bevel span",
            Box::new(|l| {
                l.effects.push(Effect::Bevel(BevelEffect {
                    path: Span { start: 0, len: 99 },
                    rule: FillRule::NonZero,
                    bx: r4(0.0, 0.0, 1.0, 1.0),
                    w: 1.0,
                    h: 1.0,
                    scale: 1.0,
                    prst: None,
                    rig: None,
                    mat: None,
                    dir: None,
                }));
            }),
        ),
        (
            "metafile operand",
            Box::new(move |l| {
                l.effects.push(Effect::MetafileRop(MetafileRopEffect { rop: 1, rect: r4(0.0, 0.0, 1.0, 1.0), operand: lens[4] as u32 }))
            }),
        ),
    ];
    for (name, f) in &mutate {
        let mut bad = list.clone();
        f(&mut bad);
        let mut enc = SlidesEncoder::new(1);
        let mut out = vec![0xAA, 0xBB];
        let e = enc.encode(&bad, &res, &mut out).unwrap_err();
        assert!(matches!(e, SlidesError::Invalid(_)), "{name}: {e:?}");
        assert_eq!(out, [0xAA, 0xBB], "{name}: output touched");
        assert_eq!(enc.sent(), [0; 5], "{name}: mark moved");
    }
    // the unmodified list still passes
    assert!(validate(&list, &lens).is_ok());
}

#[test]
fn decoder_validates_and_rolls_back_the_tables() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    let good = encode_one(&list, &res);
    // a message whose list is inconsistent (written around the encoder's check) refuses to decode
    let mut bad = list.clone();
    bad.items.push(SItem::Text(40));
    let delta = {
        // the same table entries the producer would send
        let mut d = uo_render::codec::slides_generated::Tables::new();
        let mut dec = SlidesDecoder::new();
        dec.decode(&good).unwrap();
        d.clone_from(dec.tables());
        d.delta_since(&[0; 5])
    };
    let mut raw = Vec::new();
    uo_render::codec::slides_generated::encode_slides_list(&mut raw, &delta, &bad).unwrap();
    // fresh decoder, same generation as an earlier good message: tables are as they were afterwards
    let mut dec = SlidesDecoder::new();
    assert_eq!(dec.decode(&good).unwrap(), list);
    let before = dec.tables().clone();
    let mut other_gen = raw.clone();
    // a message of the same generation (1): delta `from` is 0, the decoder's tables are not empty: TableGap, tables as before
    assert!(matches!(dec.decode(&raw), Err(SlidesError::Decode(_))));
    assert_eq!(dec.tables(), &before);
    // a new generation that decodes but does not validate: the old tables come back
    other_gen.splice(13..14, [9u8]);
    let e = dec.decode(&other_gen).unwrap_err();
    assert!(matches!(e, SlidesError::Invalid("run index")), "{e:?}");
    assert_eq!(dec.tables(), &before);
    // and a fresh decoder on the raw message of generation 1 reports Invalid and keeps nothing
    let mut fresh = SlidesDecoder::new();
    assert!(matches!(fresh.decode(&raw), Err(SlidesError::Invalid("run index"))));
    assert_eq!(fresh.tables().lens(), [0; 5]);
}

#[test]
fn every_prefix_and_single_byte_flip_of_the_sample_is_refused_or_canonical() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    let good = encode_one(&list, &res);
    let mut dec = SlidesDecoder::new();
    for cut in 0..good.len() {
        assert!(dec.decode(good.get(..cut).unwrap()).is_err(), "prefix {cut}");
        assert_eq!(dec.tables().lens(), [0; 5]);
    }
    let mut canonical = 0;
    let mut refused = 0;
    for at in 0..good.len() {
        for flip in [0x01u8, 0x80, 0xFF] {
            let mut m = good.clone();
            m[at] ^= flip;
            match SlidesDecoder::new().decode(&m) {
                Err(_) => refused += 1,
                Ok(l) => {
                    // whatever decodes re-encodes to exactly the changed bytes (the tables are in `m`)
                    let mut d2 = SlidesDecoder::new();
                    d2.decode(&m).unwrap();
                    let delta = d2.tables().delta_since(&[0; 5]);
                    let mut again = Vec::new();
                    uo_render::codec::slides_generated::encode_slides_list(&mut again, &delta, &l).unwrap();
                    assert_eq!(again, m, "flip at {at}");
                    canonical += 1;
                }
            }
        }
    }
    eprintln!("SLIDES_FLIPS: {} messages: {refused} refused, {canonical} canonical", refused + canonical);
}

#[test]
fn random_hostile_lists_encode_or_are_refused_and_decode_back() {
    let res = small_resources();
    let lens = res.lens();
    let mut r = Rng::new(0x51D3);
    let (mut ok, mut refused) = (0, 0);
    for _ in 0..3000 {
        let list = hostile_list(&mut r, lens);
        let mut out = Vec::new();
        match SlidesEncoder::new(1).encode(&list, &res, &mut out) {
            Err(SlidesError::Invalid(_)) => {
                assert!(out.is_empty());
                refused += 1;
            }
            Err(e) => panic!("{e:?}"),
            Ok(()) => {
                ok += 1;
                let back = SlidesDecoder::new().decode(&out).unwrap();
                // NaN payloads are canonicalized by the wire: compare through the bytes
                assert_eq!(encode_one(&back, &res), out);
            }
        }
    }
    eprintln!("SLIDES_HOSTILE: 3000 lists, {ok} encoded and decoded, {refused} refused by validate");
    assert!(ok > 300 && refused > 300);
}

#[test]
fn resources_dedupe_and_check_their_arguments() {
    let mut res = SlidesResources::new();
    let a = res.sym_str("a").unwrap();
    assert_eq!(res.sym_str("a"), Some(a));
    assert_eq!(res.sym(&[0xD800]), res.sym(&[0xD800]));
    assert_eq!(res.sym_units(a), [u16::from(b'a')]);
    let f = res.face(a, true, false).unwrap();
    assert_eq!(res.face(a, true, false), Some(f));
    assert_ne!(res.face(a, false, false), Some(f));
    assert_eq!(res.face(99, false, false), None);
    let m = Media { part: a, ext: a, byte_len: 1, has_svg: false, natural: None };
    let id = res.media(m.clone()).unwrap();
    assert_eq!(res.media(Media { byte_len: 2, ..m }), Some(id));
    assert_eq!(res.media(Media { part: 77, ext: a, byte_len: 1, has_svg: false, natural: None }), None);
    let fx = ImageFx { duotone: None, gray: true, bi_level: None, bright: None, contrast: None, clr_change: None };
    let d = res.derived(id, fx.clone()).unwrap();
    assert_eq!(res.derived(id, fx.clone()), Some(d));
    assert_eq!(res.derived(5, fx), None);
    let bmp = res.bitmap(2, 2, vec![0; 16].into()).unwrap();
    assert_eq!(res.bitmap_bytes(bmp).map(|b| b.len()), Some(16));
    assert_eq!(res.bitmap(2, 2, vec![0; 15].into()), None);
    assert_eq!(res.bitmap(u32::MAX, u32::MAX, vec![].into()), None);
    assert!(res.bitmap_bytes(9).is_none());
}

#[test]
fn slot_frames_round_trip_and_check_against_the_list() {
    let mut res = SlidesResources::new();
    let list = sample_list(&mut res);
    let filter = 0u32;
    let state = PartState {
        visible: true,
        opacity: 0.5,
        x: 0.5,
        y: 0.5,
        w: 0.3,
        h: 0.1,
        rot: 0.0,
        sx: 1.0,
        sy: 1.0,
        mx: 0.0,
        my: 0.0,
        filters: vec![AnimFilter { filter, out: true, p: 0.25 }],
    };
    let frame = SlotFrame {
        frame: 3,
        values: vec![SlotValue::State(state.clone())],
        replacements: vec![Replacement { shape: 2, list: encode_one(&list, &res), values: vec![] }],
    };
    assert!(check_frame(&frame, &list).is_ok());
    let mut bytes = Vec::new();
    let delta = SlotDelta { generation: 1, sym: uo_codec::TableDelta { from: 0, items: vec![utf16("wipe(down)")] } };
    encode_slot_frame(&mut bytes, &delta, &frame).unwrap();
    let mut tables = SlotTables::new();
    let back = decode_slot_frame(&bytes, &mut tables, &uo_codec::Limits::default()).unwrap();
    assert_eq!(back, frame);
    assert_eq!(tables.sym_units(0), utf16("wipe(down)").as_slice());
    eprintln!("SLOTFRAME_SIZE: {} B (one animated part, one re-emitted list of {} B)", bytes.len(), frame.replacements[0].list.len());
    // positional: the wrong number of values, a replacement for an unknown shape
    let short = SlotFrame { values: vec![], ..frame.clone() };
    assert!(check_frame(&short, &list).is_err());
    let stray = SlotFrame { replacements: vec![Replacement { shape: 9, list: vec![], values: vec![] }], ..frame };
    assert!(check_frame(&stray, &list).is_err());
    // other values
    for v in [
        SlotValue::Static,
        SlotValue::Hidden,
        SlotValue::Shape(ShapeState { op: 0.3, all: Some(state) }),
        SlotValue::Shape(ShapeState { op: 1.0, all: None }),
    ] {
        let f = SlotFrame { frame: 0, values: vec![v], replacements: vec![] };
        let mut b = Vec::new();
        encode_slot_frame(&mut b, &SlotDelta { generation: 1, sym: uo_codec::TableDelta { from: 0, items: vec![utf16("f")] } }, &f)
            .unwrap();
        assert_eq!(decode_slot_frame(&b, &mut SlotTables::new(), &uo_codec::Limits::default()).unwrap(), f);
    }
}

/// Write's schema is a separate one: its magic and hash are pinned (they were `0836c72ae39d3a59` before the Slides schemas
/// existed), and the Slides schemas have their own magic and hash.
#[test]
fn write_schema_identity_is_unchanged_and_slides_has_its_own() {
    assert_eq!(uo_render::codec::MAGIC, *b"UORP");
    assert_eq!(uo_render::codec::SCHEMA_HASH, [0x08, 0x36, 0xc7, 0x2a, 0xe3, 0x9d, 0x3a, 0x59]);
    assert_eq!(MAGIC, *b"UORS");
    assert_ne!(SCHEMA_HASH, uo_render::codec::SCHEMA_HASH);
    assert_eq!(uo_render::codec::slots_generated::MAGIC, *b"UORF");
    // each decoder refuses the other's bytes at the magic
    let mut res = SlidesResources::new();
    let bytes = encode_one(&sample_list(&mut res), &res);
    let mut tables = uo_render::codec::Tables::new();
    assert!(uo_render::codec::decode_page(&bytes, &mut tables, &uo_codec::Limits::default()).is_err());
}
