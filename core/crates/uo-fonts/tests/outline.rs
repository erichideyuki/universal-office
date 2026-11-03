//! `glyph_outline`: simple and composite glyphs, `contoursToPath`, the depth and component bounds
//! of the TS, the visit and point limits, and the per-face cache.
#![allow(clippy::unwrap_used, clippy::panic)]

mod b_support;

use std::sync::Arc;

use b_support::*;
use uo_fonts::{Face, FontError, FontLimits, OutlineCmd};

type Pt = (i16, i16, bool);

const SQUARE: [Pt; 4] = [(0, 0, true), (100, 0, true), (100, 100, true), (0, 100, true)];

fn m(x: f64, y: f64) -> OutlineCmd {
    OutlineCmd::M { x, y }
}
fn l(x: f64, y: f64) -> OutlineCmd {
    OutlineCmd::L { x, y }
}
fn q(cx: f64, cy: f64, x: f64, y: f64) -> OutlineCmd {
    OutlineCmd::Q { cx, cy, x, y }
}
const Z: OutlineCmd = OutlineCmd::Z;

fn face_of(glyphs: &[Vec<u8>], long: bool, limits: &FontLimits) -> Face {
    Face::parse(Arc::from(font_with_glyphs(glyphs, long).build()), limits).unwrap()
}

fn outline_of(glyph: Vec<u8>) -> Option<Vec<OutlineCmd>> {
    let f = face_of(&[vec![], glyph], false, &FontLimits::DEFAULT);
    f.glyph_outline(1).map(|a| a.to_vec())
}

fn square() -> Vec<u8> {
    simple_glyph(&[&SQUARE])
}

#[test]
fn contours_become_move_line_quad_close() {
    assert_eq!(
        outline_of(square()),
        Some(vec![m(0.0, 0.0), l(100.0, 0.0), l(100.0, 100.0), l(0.0, 100.0), l(0.0, 0.0), Z]),
        "the TS closes with a line back to the start before Z"
    );
    let curve = simple_glyph(&[&[(0, 0, true), (50, 100, false), (100, 0, true)]]);
    assert_eq!(outline_of(curve), Some(vec![m(0.0, 0.0), q(50.0, 100.0, 100.0, 0.0), l(0.0, 0.0), Z]));
    let all_off = simple_glyph(&[&[(0, 0, false), (100, 0, false), (100, 100, false)]]);
    assert_eq!(
        outline_of(all_off),
        Some(vec![m(50.0, 0.0), q(100.0, 0.0, 100.0, 50.0), q(100.0, 100.0, 50.0, 50.0), q(0.0, 0.0, 50.0, 0.0), Z]),
        "no on-curve point: start at the midpoint of the first two, implied midpoints between off-curve points"
    );
    let skip_one = simple_glyph(&[&[(5, 5, true)], &[(0, 0, true), (10, 0, true)]]);
    assert_eq!(outline_of(skip_one), Some(vec![m(0.0, 0.0), l(10.0, 0.0), l(0.0, 0.0), Z]), "a contour of one point is skipped");
    let start_on_later = simple_glyph(&[&[(0, 0, false), (10, 0, true), (10, 10, false)]]);
    assert_eq!(
        outline_of(start_on_later),
        Some(vec![m(10.0, 0.0), q(10.0, 10.0, 5.0, 5.0), q(0.0, 0.0, 10.0, 0.0), Z]),
        "the first on-curve point starts the contour, the others wrap around it"
    );
}

#[test]
fn an_empty_glyph_is_some_and_a_missing_one_is_none() {
    let f = face_of(&[vec![], square(), vec![]], false, &FontLimits::DEFAULT);
    assert_eq!(f.glyph_outline(0).map(|a| a.len()), Some(0));
    assert_eq!(f.glyph_outline(2).map(|a| a.len()), Some(0));
    assert!(f.glyph_outline(1).is_some());
    assert_eq!(f.glyph_outline(3), None, "gid >= numGlyphs");
    assert_eq!(f.glyph_outline(u32::MAX), None);
    let plain = Face::parse(Arc::from(Font::basic().build()), &FontLimits::DEFAULT).unwrap();
    assert_eq!(plain.glyph_outline(0), None, "no glyf: no outlines");
    let no_loca = Font::basic().add(b"glyf", vec![0; 16]);
    let g = Face::parse(Arc::from(no_loca.build()), &FontLimits::DEFAULT).unwrap();
    assert_eq!(g.glyph_outline(0), None);
}

#[test]
fn a_loca_with_end_not_after_start_is_an_empty_glyph() {
    let mut font = font_with_glyphs(&[square(), square()], false);
    for t in &mut font.tables {
        if &t.tag == b"loca" {
            let n = t.data.len();
            t.data[n - 2..].copy_from_slice(&0u16.to_be_bytes());
        }
    }
    let f = Face::parse(Arc::from(font.build()), &FontLimits::DEFAULT).unwrap();
    assert_eq!(f.glyph_outline(1).map(|a| a.len()), Some(0));
    assert!(f.glyph_outline(0).is_some());
}

#[test]
fn short_and_long_loca_agree() {
    let glyphs = vec![vec![], square(), simple_glyph(&[&[(0, 0, true), (10, 0, false), (20, 0, true)]])];
    let short = face_of(&glyphs, false, &FontLimits::DEFAULT);
    let long = face_of(&glyphs, true, &FontLimits::DEFAULT);
    for gid in 0..3 {
        assert_eq!(short.glyph_outline(gid), long.glyph_outline(gid), "gid {gid}");
    }
}

#[test]
fn coordinates_accumulate_deltas_in_every_encoding() {
    let mut w = uo_testkit::generate::sfnt::BeWriter::new();
    w.i16(1).i16(0).i16(0).i16(0).i16(0).u16(3).u16(0);
    w.u8(0x01 | 0x02 | 0x04 | 0x10 | 0x20);
    w.u8(0x01 | 0x08 | 0x02 | 0x04 | 0x10 | 0x20);
    w.u8(2);
    w.u8(7).u8(3).u8(3).u8(3);
    w.u8(8).u8(1).u8(1).u8(1);
    let g = w.into_bytes();
    assert_eq!(
        outline_of(g),
        Some(vec![m(7.0, 8.0), l(10.0, 9.0), l(13.0, 10.0), l(16.0, 11.0), l(7.0, 8.0), Z]),
        "short vectors, the repeat flag and the 'same' bits"
    );
    let neg = {
        let mut w = uo_testkit::generate::sfnt::BeWriter::new();
        w.i16(1).i16(0).i16(0).i16(0).i16(0).u16(1).u16(0);
        w.u8(0x01 | 0x02 | 0x04).u8(0x01 | 0x02 | 0x04).u8(5).u8(6).u8(7).u8(8);
        w.into_bytes()
    };
    assert_eq!(
        outline_of(neg),
        Some(vec![m(-5.0, -7.0), l(-11.0, -15.0), l(-5.0, -7.0), Z]),
        "a short vector without the positive bit is negative"
    );
}

#[test]
fn composites_transform_in_the_ts_order() {
    let base = square();
    let tri = simple_glyph(&[&[(100, 200, true), (0, 0, true)]]);
    let f = face_of(
        &[
            vec![],
            tri,
            composite(&[Comp { scale: Scale::Matrix(0x4000, 0x2000, 0x1000, 0x7FFF), ..comp(1, 10, 20) }]),
            composite(&[Comp { scale: Scale::Uniform(0x2000), ..comp(1, -5, 7) }]),
            composite(&[Comp { scale: Scale::Xy(0x2000, 0x1000), ..comp(1, 0, 0) }]),
            composite(&[Comp { words: false, ..comp(1, -3, 4) }]),
            composite(&[Comp { xy: false, ..comp(1, 99, 99) }]),
            base,
        ],
        false,
        &FontLimits::DEFAULT,
    );
    let first = |gid: u32| f.glyph_outline(gid).unwrap()[0];
    let (am, bm, cm, dm) = (1.0, 0.5, 0.25, f64::from(0x7FFF_i16) / 16384.0);
    assert_eq!(first(2), m(am * 100.0 + cm * 200.0 + 10.0, bm * 100.0 + dm * 200.0 + 20.0));
    assert_eq!(first(3), m(0.5 * 100.0 + 0.0 * 200.0 + -5.0, 0.0 * 100.0 + 0.5 * 200.0 + 7.0));
    assert_eq!(first(4), m(0.5 * 100.0, 0.25 * 200.0));
    assert_eq!(first(5), m(100.0 - 3.0, 200.0 + 4.0));
    assert_eq!(first(6), m(100.0, 200.0), "without ARGS_ARE_XY_VALUES the offsets are 0");
}

#[test]
fn a_composite_of_several_components_appends_their_contours_in_order() {
    let f = face_of(
        &[vec![], square(), simple_glyph(&[&[(0, 0, true), (7, 0, true)]]), composite(&[comp(1, 0, 0), comp(2, 1, 1), comp(1, 200, 0)])],
        false,
        &FontLimits::DEFAULT,
    );
    let path = f.glyph_outline(3).unwrap();
    let moves: Vec<OutlineCmd> = path.iter().copied().filter(|c| matches!(c, OutlineCmd::M { .. })).collect();
    assert_eq!(moves, vec![m(0.0, 0.0), m(1.0, 1.0), m(200.0, 0.0)]);
    assert_eq!(path.iter().filter(|c| **c == Z).count(), 3);
}

#[test]
fn at_most_64_components_are_read() {
    let comps: Vec<Comp> = (0..70).map(|i| comp(1, i, 0)).collect();
    let f = face_of(&[vec![], square(), composite(&comps)], false, &FontLimits::DEFAULT);
    let path = f.glyph_outline(2).unwrap();
    assert_eq!(path.iter().filter(|c| **c == Z).count(), 64);
}

fn chain(depth_of_leaf: u16) -> Face {
    let mut glyphs = vec![vec![]];
    for i in 0..depth_of_leaf {
        glyphs.push(composite(&[comp(i + 2, 0, 0)]));
    }
    glyphs.push(square());
    face_of(&glyphs, false, &FontLimits::DEFAULT)
}

#[test]
fn components_deeper_than_eight_are_skipped() {
    let ok = chain(8);
    assert_eq!(ok.glyph_outline(1).unwrap().iter().filter(|c| **c == Z).count(), 1, "the leaf at depth 8 is read");
    let deep = chain(9);
    assert_eq!(deep.glyph_outline(1).map(|a| a.len()), Some(0), "the leaf at depth 9 is null: the composite is empty, not None");
    let shallow = chain(1);
    assert_eq!(shallow.glyph_outline(1).unwrap().iter().filter(|c| **c == Z).count(), 1);
}

#[test]
fn a_component_that_does_not_exist_is_skipped() {
    let f = face_of(&[vec![], composite(&[comp(77, 0, 0), comp(2, 0, 0)]), square()], false, &FontLimits::DEFAULT);
    assert_eq!(f.glyph_outline(1).unwrap().iter().filter(|c| **c == Z).count(), 1);
}

#[test]
fn a_self_referencing_composite_ends_by_depth_or_hits_the_visit_limit() {
    let two = face_of(&[vec![], composite(&[comp(1, 0, 0), comp(1, 0, 0)]), composite(&[comp(1, 0, 0)])], false, &FontLimits::DEFAULT);
    assert_eq!(two.glyph_outline(1).map(|a| a.len()), Some(0), "2^10 visits: the depth bound ends it, as in the TS");
    assert_eq!(two.glyph_outline(2).map(|a| a.len()), Some(0));
    let five: Vec<Comp> = (0..5).map(|_| comp(1, 0, 0)).collect();
    assert_eq!(face_of(&[vec![], composite(&five)], false, &FontLimits::DEFAULT).glyph_outline(1), None, "5^9 visits");
    let wide: Vec<Comp> = (0..64).map(|_| comp(1, 0, 0)).collect();
    let g = face_of(&[vec![], composite(&wide)], false, &FontLimits::DEFAULT);
    assert_eq!(g.glyph_outline(1), None, "64 components that are the glyph itself: 64^9 visits in the TS");
}

#[test]
fn the_visit_and_point_limits_are_counted_per_call() {
    let glyphs = [vec![], square(), composite(&[comp(1, 0, 0), comp(1, 5, 0)])];
    let with = |visits: u32, points: u32| {
        face_of(&glyphs, false, &FontLimits { max_outline_work: visits, max_outline_points: points, ..FontLimits::DEFAULT })
    };
    assert!(with(3, 100).glyph_outline(2).is_some());
    assert_eq!(with(2, 100).glyph_outline(2), None, "root + 2 components = 3 visits");
    assert!(with(1, 100).glyph_outline(1).is_some());
    assert_eq!(with(0, 100).glyph_outline(1), None);
    assert!(with(9, 8).glyph_outline(2).is_some(), "8 points in all");
    assert_eq!(with(9, 7).glyph_outline(2), None);
    assert!(with(9, 4).glyph_outline(1).is_some());
    assert_eq!(with(9, 3).glyph_outline(1), None);
}

#[test]
fn contour_ranges_come_from_the_end_points_as_written() {
    let points = [(0, 0, true), (10, 0, true)];
    let decreasing = simple_glyph_ends(&[1, 0, 1], &points);
    assert_eq!(outline_of(decreasing), Some(vec![m(0.0, 0.0), l(10.0, 0.0), l(0.0, 0.0), Z]), "1 then 0 then 1: 2 points, none, 1 point");
    let past = simple_glyph_ends(&[5, 1], &points);
    assert_eq!(outline_of(past), None, "an end past the last point has no coordinates (G4)");
    let none = simple_glyph_ends(&[], &[]);
    assert_eq!(outline_of(none).map(|p| p.len()), Some(0), "no contours: an empty outline");
}

#[test]
fn emitted_points_count_against_the_limit_even_when_ends_repeat_points() {
    let points = [(0, 0, true), (10, 0, true), (20, 0, true), (30, 0, true)];
    let glyph = simple_glyph_ends(&[3, 0, 3], &points);
    let f = |max| face_of(&[vec![], glyph.clone()], false, &FontLimits { max_outline_points: max, ..FontLimits::DEFAULT });
    // the empty contour counts one (stage 4, S4-4)
    assert!(f(8).glyph_outline(1).is_some(), "4 + 1 + 3 points");
    assert_eq!(f(7).glyph_outline(1), None);
    let ends: Vec<u16> = (0..2999).map(|i| if i % 2 == 0 { 3 } else { 0 }).collect();
    let bomb = simple_glyph_ends(&ends, &points);
    let g = |max| face_of(&[vec![], bomb.clone()], false, &FontLimits { max_outline_points: max, ..FontLimits::DEFAULT });
    assert_eq!(g(100).glyph_outline(1), None, "4 decoded points, about 4,500 emitted");
    assert!(g(100_000).glyph_outline(1).is_some());
}

#[test]
fn a_read_past_the_end_of_a_glyph_is_none() {
    let mut font = font_with_glyphs(&[vec![], square()], false);
    let glyf = font.table(b"glyf").clone();
    font.tables.retain(|t| &t.tag != b"glyf");
    let mut cut = glyf;
    cut.data.truncate(30);
    font.tables.push(cut);
    let f = Face::parse(Arc::from(font.build()), &FontLimits::DEFAULT).unwrap();
    assert_eq!(f.glyph_outline(1), None);
}

#[test]
fn the_cache_returns_the_same_outline_and_is_cleared_when_full() {
    let glyphs: Vec<Vec<u8>> = (0..12i16).map(|i| simple_glyph(&[&[(i, 0, true), (i + 5, 0, true)]])).collect();
    let f = face_of(&glyphs, false, &FontLimits { outline_cache_entries: 4, ..FontLimits::DEFAULT });
    let first = f.glyph_outline(3).unwrap();
    assert!(Arc::ptr_eq(&first, &f.glyph_outline(3).unwrap()), "a hit returns the same allocation");
    for gid in 0..12 {
        let a = f.glyph_outline(gid).unwrap();
        assert_eq!(a[0], m(f64::from(gid as i16), 0.0));
    }
    let again = f.glyph_outline(11).unwrap();
    assert!(Arc::ptr_eq(&again, &f.glyph_outline(11).unwrap()));
    let old = f.glyph_outline(3).unwrap();
    assert!(!Arc::ptr_eq(&first, &old), "gid 3 was evicted when the cache filled and cleared");
    assert_eq!(first, old);
    let off = face_of(&glyphs, false, &FontLimits { outline_cache_entries: 0, ..FontLimits::DEFAULT });
    let (x, y) = (off.glyph_outline(1).unwrap(), off.glyph_outline(1).unwrap());
    assert!(!Arc::ptr_eq(&x, &y));
    assert_eq!(x, y);
}

#[test]
fn outlines_are_the_same_from_every_clone_and_thread() {
    let glyphs = vec![vec![], square()];
    let f = face_of(&glyphs, false, &FontLimits::DEFAULT);
    let want = f.glyph_outline(1).unwrap();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let g = f.clone();
            std::thread::spawn(move || g.glyph_outline(1).unwrap())
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), want);
    }
}

#[test]
fn a_face_that_fails_to_parse_has_no_outlines() {
    let err = Face::parse(Arc::from(vec![0u8; 3]), &FontLimits::DEFAULT).err();
    assert_eq!(err, Some(FontError::OutOfBounds));
}
