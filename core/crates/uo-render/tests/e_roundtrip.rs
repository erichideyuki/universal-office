//! R-P2 for the encoder (frontier E): Rust page -> `encode_page_for_canvas` -> `decode_page` = the
//! input, field by field (every item kind, every optional field, hostile numbers), the resource
//! deltas (pulled out of order, across generations), the app-section hook and the errors.
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss, clippy::cast_possible_truncation)]

mod e_support;
mod g_support;

use e_support::full::full_page;
use e_support::*;
use uo_codec::{DecodeError, Limits};
use uo_render::encode::EncodeError;
use uo_render::encode::decode::{DecodeTables, decode_page};
use uo_render::types::{Item, RunFlags};
use uo_render::{
    AppSection, CanvasOptions, DeltaMark, FaceId, GlyphRunHead, ItemList, NoApp, OptF64, Page, Resources, Span, encode_page_for_canvas,
    encode_page_for_canvas_with,
};
use uo_testkit::generate::Rng;

fn debug_text() -> CanvasOptions {
    CanvasOptions { debug_text: true, ..CanvasOptions::default() }
}

#[test]
fn every_field_survives() {
    let fonts = fonts();
    for seed in 1..=60u32 {
        let mut r = Rng::new(seed);
        let full = full_page(&mut r, fonts.first(), seed % 2 == 0);
        let mut sent = DeltaMark::new(7);
        let mut buf = Vec::new();
        encode_page_for_canvas_with(&full.page, &full.res, &NoApp, &mut sent, &debug_text(), &mut buf).unwrap();
        let mut tables = DecodeTables::new();
        let got = decode_page(&buf, &mut tables, &Limits::default()).unwrap();
        assert_eq!(format!("{:?}", got.page), format!("{:?}", full.page), "seed {seed}: the page");
        let (a, b) = (&full.res, tables.resources());
        assert_eq!(b.symbols.len(), a.symbols.len(), "seed {seed}: symbols");
        for i in 0..a.symbols.len() {
            let id = uo_render::Sym::from_index(i).unwrap();
            assert_eq!(a.symbols.get(id), b.symbols.get(id));
        }
        assert_eq!(format!("{:?}", a.text_fx), format!("{:?}", b.text_fx), "seed {seed}: text effects");
        assert_eq!(format!("{:?}", a.page_fills), format!("{:?}", b.page_fills), "seed {seed}: page fills");
        assert_eq!(format!("{:?}", a.cust), format!("{:?}", b.cust), "seed {seed}: custom geometries");
        assert_eq!(a.faces.len(), b.faces.len());
        for (x, y) in a.faces.iter().zip(&b.faces) {
            assert_eq!(
                (x.key, x.family, x.bold, x.italic, x.synth_bold, x.synth_italic, x.stand_in),
                (y.key, y.family, y.bold, y.italic, y.synth_bold, y.synth_italic, y.stand_in)
            );
        }
        assert_eq!(a.images.len(), b.images.len());
        for (x, y) in a.images.iter().zip(&b.images) {
            assert_eq!(
                (x.ext, x.width_twips.to_bits(), x.height_twips.to_bits(), x.natural_px),
                (y.ext, y.width_twips.to_bits(), y.height_twips.to_bits(), y.natural_px)
            );
            assert_eq!(format!("{:?}", x.effects), format!("{:?}", y.effects));
        }
        assert!(sent.sent().iter().sum::<usize>() > 0);
    }
}

fn five_pages() -> (Resources, Vec<Page>) {
    let mut res = Resources::new();
    let colors: Vec<_> = ["000000", "FF0000", "00FF00", "0000FF", "123456"].iter().map(|c| res.symbols.intern_str(c).unwrap()).collect();
    let mut pages = Vec::new();
    for (k, c) in colors.iter().enumerate() {
        let mut list = ItemList::new();
        list.push_plain(Item::Rect { x: k as f64, y: 0.0, w: 10.0, h: 10.0, fill: *c, alpha: OptF64::NONE });
        pages.push(Page { w: 100.0, h: 100.0, list, background: None, background_fill: None, background_show: false });
    }
    (res, pages)
}

#[test]
fn pages_pulled_in_any_order_send_what_they_need() {
    let (res, pages) = five_pages();
    let mut sent = DeltaMark::new(1);
    let mut tables = DecodeTables::new();
    // (page pulled, symbols sent after it): page 3 brings 0..=3, page 4 the last one
    for (k, total) in [(3usize, 4usize), (0, 4), (4, 5), (1, 5), (2, 5)] {
        let mut buf = Vec::new();
        let page = pages.get(k).unwrap();
        encode_page_for_canvas(page, &res, &NoApp, &mut sent, &mut buf).unwrap();
        let got = decode_page(&buf, &mut tables, &Limits::default()).unwrap();
        assert_eq!(format!("{:?}", got.page), format!("{page:?}"), "page {k}");
        assert_eq!(sent.sent().first().copied(), Some(total), "after page {k}");
        assert_eq!(tables.resources().symbols.len(), total);
    }
}

#[test]
fn a_new_generation_restarts_the_tables() {
    let (res, pages) = five_pages();
    let mut tables = DecodeTables::new();
    let mut sent = DeltaMark::new(0);
    for generation in [1u32, 2, 2, 3] {
        // the mark of a generation is kept for its pages; a new layout starts a new one
        if sent.generation() != generation {
            sent = DeltaMark::new(generation);
        }
        let mut buf = Vec::new();
        encode_page_for_canvas(pages.get(2).unwrap(), &res, &NoApp, &mut sent, &mut buf).unwrap();
        let got = decode_page(&buf, &mut tables, &Limits::default()).unwrap();
        assert_eq!(format!("{:?}", got.page), format!("{:?}", pages.get(2).unwrap()));
        assert_eq!(tables.resources().symbols.len(), 3);
    }
}

struct Section(Vec<u8>);
impl AppSection for Section {
    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0);
    }
}

#[test]
fn the_app_section_travels_opaque() {
    let (res, pages) = five_pages();
    let bytes: Vec<u8> = (0..=255u8).chain([0, 0, 1]).collect();
    let mut buf = Vec::new();
    encode_page_for_canvas(pages.first().unwrap(), &res, &Section(bytes.clone()), &mut DeltaMark::new(1), &mut buf).unwrap();
    let got = decode_page(&buf, &mut DecodeTables::new(), &Limits::default()).unwrap();
    assert_eq!(got.app, bytes);
    let mut buf = Vec::new();
    encode_page_for_canvas(pages.first().unwrap(), &res, &NoApp, &mut DeltaMark::new(1), &mut buf).unwrap();
    assert!(decode_page(&buf, &mut DecodeTables::new(), &Limits::default()).unwrap().app.is_empty());
}

fn one_face_resources() -> (Resources, uo_render::Sym) {
    let mut res = Resources::new();
    let color = res.symbols.intern_str("000000").unwrap();
    let key = res.symbols.intern_str("k").unwrap();
    res.add_face(uo_render::FaceRes {
        key,
        family: key,
        bold: false,
        italic: false,
        synth_bold: false,
        synth_italic: false,
        stand_in: false,
        face: None,
        bytes: None,
        advances: None,
    })
    .unwrap();
    (res, color)
}

fn head(color: uo_render::Sym, text: &[u16]) -> GlyphRunHead<'_> {
    GlyphRunHead {
        face: FaceId::from_index(0).unwrap(),
        color,
        size_pt: 10.0,
        y: 0.0,
        text,
        scale_x: OptF64::NONE,
        alpha: OptF64::NONE,
        fx: None,
        flags: RunFlags::default(),
    }
}

#[test]
fn glyph_text_is_sent_only_on_request() {
    let (res, color) = one_face_resources();
    let mut list = ItemList::new();
    let text: Vec<u16> = "ab".encode_utf16().collect();
    list.push_glyphs(head(color, &text), [(97, 0.0, None), (98, 5.0, None)]).unwrap();
    let page = page_of(list);
    for want in [false, true] {
        let mut buf = Vec::new();
        let opts = CanvasOptions { debug_text: want, ..CanvasOptions::default() };
        encode_page_for_canvas_with(&page, &res, &NoApp, &mut DeltaMark::new(1), &opts, &mut buf).unwrap();
        let got = decode_page(&buf, &mut DecodeTables::new(), &Limits::default()).unwrap();
        assert_eq!(got.page.list.text.is_empty(), !want);
    }
}

#[test]
fn errors_leave_the_buffer_and_the_mark_alone() {
    let (res, pages) = five_pages();
    // an id of another layout's table
    let mut other = Resources::new();
    for i in 0..9 {
        other.symbols.intern_str(&format!("x{i}")).unwrap();
    }
    let far = other.symbols.intern_str("far").unwrap();
    let mut list = ItemList::new();
    list.push_plain(Item::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, fill: far, alpha: OptF64::NONE });
    let page = page_of(list);
    let mut sent = DeltaMark::new(1);
    let mut buf = vec![9u8, 9, 9];
    assert_eq!(encode_page_for_canvas(&page, &res, &NoApp, &mut sent, &mut buf), Err(EncodeError::BadId("sym")));
    assert_eq!(buf, vec![9, 9, 9]);
    assert_eq!(sent.sent(), [0; 6]);
    // a span past the glyph columns
    let mut list = ItemList::new();
    list.push_plain(Item::Glyphs {
        face: FaceId::from_index(0).unwrap(),
        color: uo_render::Sym::from_index(0).unwrap(),
        size_pt: 1.0,
        y: 0.0,
        glyphs: Span { start: 4, len: 2 },
        run: 0,
        flags: RunFlags::default(),
    });
    let r = encode_page_for_canvas(&page_of(list), &res, &NoApp, &mut sent, &mut buf);
    assert!(matches!(r, Err(EncodeError::BadSpan)));
    assert_eq!(buf, vec![9, 9, 9]);
    // the same state still encodes after the failures
    let mut ok = Vec::new();
    encode_page_for_canvas(pages.first().unwrap(), &res, &NoApp, &mut sent, &mut ok).unwrap();
    // a stale decoder: another schema hash
    let mut bad = ok.clone();
    if let Some(b) = bad.get_mut(6) {
        *b ^= 0xff;
    }
    let mut tables = DecodeTables::new();
    assert!(matches!(decode_page(&bad, &mut tables, &Limits::default()), Err(DecodeError::VersionMismatch { .. })));
    assert!(tables.resources().symbols.is_empty());
}

#[test]
fn a_face_without_a_font_has_no_glyph_hints() {
    let (res, color) = one_face_resources();
    let mut list = ItemList::new();
    list.push_glyphs(head(color, &[]), [(97, 0.0, Some(4)), (0xf041, 5.0, None)]).unwrap();
    let mut buf = Vec::new();
    encode_page_for_canvas(&page_of(list), &res, &NoApp, &mut DeltaMark::new(1), &mut buf).unwrap();
    let mut t = uo_render::codec::Tables::default();
    let msg = uo_render::codec::decode_page(&buf, &mut t, &Limits::default()).unwrap();
    let uo_render::codec::Item::Glyphs(g) = msg.items.first().unwrap() else { panic!("not a run") };
    assert!(g.joins.is_none() && g.remap.is_none() && g.outlines.is_none());
}
