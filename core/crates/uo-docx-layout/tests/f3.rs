//! Frontier F3 unit tests (record "Wave 1 — F3"): the metafile limits (LQ14: truncated with a
//! warning), the TS throws kept as errors (LQ9), a no-panic loop over mutated EMF/WMF files, the
//! small helpers of `drawing.rs` against values worked out from the TS, and the M1 memory gate of
//! every F3 entry point.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod f3_support;

use std::hint::black_box;
use std::sync::Arc;

use f3_support::{J, Media, parse, unhex};
use uo_alloc::assert_no_leak;
use uo_docx::js::{JsNumber, Str};
use uo_docx::model::background::{PageBackground, PageFill};
use uo_docx::model::color::DrawingColor;
use uo_docx::model::drawing::ShapeStyle;
use uo_docx::model::formatting::Theme;
use uo_docx::model::run::{DrawingGroup, GroupItem, Run};
use uo_docx_layout::drawing::{
    ResWriter, group_child, image_pixel_info, resolve_color_str, resolve_color2, resolve_page_fill, shape_extras, shape_geom, subst_ph_clr,
    vml_color,
};
use uo_docx_layout::error::{LResult, LayoutError};
use uo_docx_layout::items::PassItems;
use uo_docx_layout::metafile::{
    FaceLookup, MetafileFrame, MetafileLimits, TRUNCATED, metafile_kind, paint_metafile, render_emf, render_wmf,
};
use uo_render::{FaceId, FaceRes, ImageId, Item, Resources};

/// Every family answered with one open face (Liberation Serif), as a resolver whose provider has
/// only that file.
struct OneFace {
    face: uo_fonts::Face,
    bytes: Arc<[u8]>,
    id: Option<FaceId>,
    warnings: Vec<String>,
}

impl OneFace {
    fn new() -> OneFace {
        let bytes: Arc<[u8]> = std::fs::read(common::repo().join("shared/fonts/files/liberationserif-regular.ttf")).unwrap().into();
        let face = uo_fonts::Face::parse(bytes.clone(), &uo_fonts::FontLimits::default()).unwrap();
        OneFace { face, bytes, id: None, warnings: Vec::new() }
    }
}

impl FaceLookup for OneFace {
    fn face(&mut self, res: &mut Resources, _family: &[u16], _bold: bool, _italic: bool) -> LResult<FaceId> {
        if let Some(id) = self.id {
            return Ok(id);
        }
        let s = res.symbols.intern_str("Liberation Serif").unwrap();
        let id = res
            .add_face(FaceRes {
                key: s,
                family: s,
                bold: false,
                italic: false,
                synth_bold: false,
                synth_italic: false,
                stand_in: false,
                face: Some(self.face.clone()),
                bytes: Some(self.bytes.clone()),
                advances: None,
            })
            .unwrap();
        self.id = Some(id);
        Ok(id)
    }
    fn warn(&mut self, msg: &str) {
        self.warnings.push(msg.to_owned());
    }
}

/// The generated metafiles of the committed capture (`tools/capture-f3.mjs --seed 1`).
fn generated() -> Vec<(bool, Vec<u8>)> {
    let text = std::fs::read_to_string(common::manifest().join("tests/captures/f3/metafile-gen.f3.jsonl")).unwrap();
    text.lines()
        .filter_map(|l| {
            let o = parse(l).ok()?;
            let emf = match o.get("f").and_then(J::str)? {
                "emf" => true,
                "wmf" => false,
                _ => return None,
            };
            Some((emf, unhex(o.get("hex")?.str()?)))
        })
        .collect()
}

const DEST: MetafileFrame = MetafileFrame { x: 100.0, y: 200.0, w: 3000.0, h: 2000.0 };

fn play(emf: bool, b: &[u8], limits: MetafileLimits, faces: &mut OneFace) -> (LResult<()>, PassItems, Resources) {
    let mut res = Resources::new();
    let mut origins = Vec::new();
    let mut out = PassItems::new();
    let r = {
        let mut w = ResWriter::new(&mut res, &mut origins);
        if emf { render_emf(b, DEST, limits, faces, &mut w, &mut out) } else { render_wmf(b, DEST, limits, faces, &mut w, &mut out) }
    };
    assert_eq!(out.list.items.len(), out.side.len(), "side data parallel to the items");
    assert_eq!(origins.len(), res.images.len(), "an origin per image");
    (r, out, res)
}

/// A minimal EMF: header (108 bytes) + the given records + EOF.
fn emf_of(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut b = vec![0u8; 108];
    let put = |b: &mut Vec<u8>, o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut b, 0, 1);
    put(&mut b, 4, 108);
    put(&mut b, 32, 1000);
    put(&mut b, 36, 1000);
    put(&mut b, 40, 0x464d_4520);
    put(&mut b, 72, 1000);
    put(&mut b, 76, 1000);
    put(&mut b, 80, 10);
    put(&mut b, 84, 10);
    for (t, body) in records {
        b.extend_from_slice(&t.to_le_bytes());
        b.extend_from_slice(&u32::try_from(8 + body.len()).unwrap().to_le_bytes());
        b.extend_from_slice(body);
    }
    b.extend_from_slice(&[14, 0, 0, 0, 20, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 20, 0, 0, 0]);
    b
}

fn le(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[test]
fn emf_rectangle_maps_the_frame_into_dest() {
    // frame 0..1000 hundredths of mm, 1000 px per 10 mm: 1 px per hundredth; RECTANGLE 0,0,500,500
    let b = emf_of(&[(43, le(&[0, 0, 500, 500]))]);
    assert_eq!(metafile_kind(&b), Some(uo_docx_layout::metafile::MetafileKind::Emf));
    let mut f = OneFace::new();
    let (r, out, res) = play(true, &b, MetafileLimits::default(), &mut f);
    r.unwrap();
    let [Item::Path { cmds, fill, stroke, even_odd }] = &out.list.items[..] else { panic!("{:?}", out.list.items) };
    let c = out.list.cmds_of(*cmds);
    assert!(matches!(c[0], uo_render::types::PathCmd::M { x, y } if x == 100.0 && y == 200.0));
    assert!(matches!(c[2], uo_render::types::PathCmd::L { x, y } if x == 1600.0 && y == 1200.0));
    assert_eq!(res.sym(fill.unwrap()), "FFFFFF".encode_utf16().collect::<Vec<_>>());
    assert_eq!(res.sym(stroke.unwrap().color), "000000".encode_utf16().collect::<Vec<_>>());
    assert!(*even_odd);
}

#[test]
fn ts_throws_are_errors() {
    let mut f = OneFace::new();
    // a short EMF: the header read at 84 throws in the TS
    let mut short = emf_of(&[]);
    short.truncate(60);
    assert_eq!(play(true, &short, MetafileLimits::default(), &mut f).0, Err(LayoutError::Internal("metafile: DataView read past the end")));
    // POLYGON16 declaring more points than the file holds
    let mut body = le(&[0, 0, 0, 0, 1000]);
    body.extend_from_slice(&[0; 8]);
    let b = emf_of(&[(86, body)]);
    assert!(matches!(play(true, &b, MetafileLimits::default(), &mut f).0, Err(LayoutError::Internal(_))));
    // POLYBEZIER with no point (`pts[0]![0]` throws a TypeError)
    let b = emf_of(&[(85, le(&[0, 0, 0, 0, 0]))]);
    assert!(matches!(play(true, &b, MetafileLimits::default(), &mut f).0, Err(LayoutError::Internal(_))));
}

#[test]
fn limits_truncate_with_a_warning() {
    let rects: Vec<(u32, Vec<u8>)> = (0..50).map(|k| (43, le(&[k, k, k + 10, k + 10]))).collect();
    let b = emf_of(&rects);
    let mut f = OneFace::new();
    let (r, out, _) = play(true, &b, MetafileLimits { records: 1000, items: 7, payload: 1 << 20 }, &mut f);
    r.unwrap();
    assert_eq!(out.list.items.len(), 7);
    assert_eq!(f.warnings, vec![TRUNCATED.to_owned()]);
    let mut f = OneFace::new();
    let (r, out, _) = play(true, &b, MetafileLimits { records: 10, items: 1000, payload: 1 << 20 }, &mut f);
    r.unwrap();
    assert_eq!(out.list.items.len(), 9, "the header is the first record");
    assert_eq!(f.warnings.len(), 1);
    let mut f = OneFace::new();
    let (r, out, _) = play(true, &b, MetafileLimits { records: 1000, items: 1000, payload: 23 }, &mut f);
    r.unwrap();
    assert_eq!(out.list.items.len(), 4, "5 commands a rectangle");
    assert_eq!(f.warnings.len(), 1);
    // within the limits: no warning
    let mut f = OneFace::new();
    let (r, out, _) = play(true, &b, MetafileLimits::default(), &mut f);
    r.unwrap();
    assert_eq!(out.list.items.len(), 50);
    assert!(f.warnings.is_empty());
}

#[test]
fn paint_metafile_wraps_the_items_in_a_clip_with_its_source() {
    let b = emf_of(&[(43, le(&[0, 0, 500, 500]))]);
    let mut res = Resources::new();
    let mut origins = Vec::new();
    let mut out = PassItems::new();
    let src = ImageId::from_index(0).unwrap();
    let mut f = OneFace::new();
    let drew =
        paint_metafile(&b, src, DEST, MetafileLimits::default(), &mut f, &mut ResWriter::new(&mut res, &mut origins), &mut out).unwrap();
    assert!(drew);
    assert!(matches!(out.list.items[..], [Item::ClipPush { .. }, Item::Path { .. }, Item::ClipPop]));
    assert_eq!(out.side[0].clip_src, Some(src));
    let png = [0x89u8, 0x50, 0x4e, 0x47];
    assert!(
        !paint_metafile(&png, src, DEST, MetafileLimits::default(), &mut f, &mut ResWriter::new(&mut res, &mut origins), &mut out).unwrap()
    );
}

/// No panic, and termination, on mutated files (security plan §7; the fuzz targets `emf_records`
/// and `wmf_records` run the same body).
#[test]
fn mutated_metafiles_never_panic() {
    let seeds = generated();
    assert!(seeds.len() > 100);
    let mut rng = uo_testkit::generate::Rng::new(0xf3);
    let mut f = OneFace::new();
    let (mut ok, mut err) = (0usize, 0usize);
    let rounds = if cfg!(debug_assertions) { 3 } else { 20 };
    for _ in 0..rounds {
        for (emf, b) in &seeds {
            let mut m = b.clone();
            for _ in 0..1 + rng.below(6) {
                if m.is_empty() {
                    break;
                }
                let i = rng.below(m.len());
                match rng.below(5) {
                    0 => m[i] = rng.next_u32() as u8,
                    1 => m[i] ^= 0x80,
                    // a huge count or size
                    2 if i + 4 <= m.len() => m[i..i + 4].copy_from_slice(&rng.next_u32().to_le_bytes()),
                    3 => m.truncate(i.max(1)),
                    _ => {
                        let n = rng.below(16);
                        let extra = rng.bytes(n);
                        m.splice(i..i, extra);
                    }
                }
            }
            let limits = if rng.one_in(4) { MetafileLimits { records: 50, items: 20, payload: 500 } } else { MetafileLimits::default() };
            match play(*emf, &m, limits, &mut f).0 {
                Ok(()) => ok += 1,
                Err(_) => err += 1,
            }
        }
    }
    eprintln!("mutated metafiles: {ok} drawn, {err} errors (where the TS throws)");
    assert!(ok > 0 && err > 0);
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[test]
fn colour_helpers_follow_the_ts() {
    let c1 = units("336699");
    assert_eq!(resolve_color2(Some("fill darken(128)"), &c1), units("19334C"));
    assert_eq!(resolve_color2(Some("  Fill  LIGHTEN(128) "), &c1), units("99B3CC"));
    assert_eq!(resolve_color2(Some("fill"), &c1), c1);
    assert_eq!(resolve_color2(Some("fill darken(x)"), &c1), units("FFFFFF"));
    assert_eq!(resolve_color2(Some("fill darken(9999)"), &units("zz")), units("NANNANNAN"));
    assert_eq!(resolve_color2(Some("#abc"), &c1), units("AABBCC"));
    assert_eq!(resolve_color2(Some("red [3]"), &c1), units("FF0000"));
    assert_eq!(resolve_color2(None, &c1), units("FFFFFF"));
    assert_eq!(vml_color(Some(" #12345678 ")), Some(units("123456")));
    assert_eq!(vml_color(Some("#1234")), None);
    assert_eq!(vml_color(Some("Silver")), Some(units("C0C0C0")));
    let mut theme = Theme::default();
    theme.colors.insert("accent1", Str::from("4472C4"));
    assert_eq!(resolve_color_str("@accent1", &theme).as_deref(), Some("4472C4"));
    assert_eq!(resolve_color_str("@tx1", &theme), None);
    assert_eq!(resolve_color_str("ff0000|", &theme).as_deref(), Some("FF0000"));
    assert_eq!(resolve_color_str("FF0000|lumMod=50000", &theme).as_deref(), Some("800000"));
    assert_eq!(resolve_color_str("FF0000|nope=5,lumMod=undefined", &theme).as_deref(), Some("FF0000"));
    // substPhClr: the reference's modifiers, then the style's
    let ph = f3_support::color("@phClr|shade=50000");
    let r = subst_ph_clr(&ph, &f3_support::color("@accent1|lumMod=75000"));
    assert_eq!(r.0.to_ts_string(), "@accent1|lumMod=75000,shade=50000");
    assert_eq!(subst_ph_clr(&f3_support::color("@phClr"), &f3_support::color("00FF00")).0.to_ts_string(), "00FF00");
    assert_eq!(subst_ph_clr(&f3_support::color("123456"), &f3_support::color("00FF00")).0.to_ts_string(), "123456");
}

#[test]
fn image_pixel_info_reads_the_headers() {
    let mut png = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 1, 0, 0, 0, 0, 32];
    png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    png.extend_from_slice(&[0, 0, 0, 9, b'p', b'H', b'Y', b's', 0, 0, 0x2e, 0x23, 0, 0, 0x2e, 0x23, 1, 0, 0, 0, 0]);
    assert_eq!(image_pixel_info(&png), Some([256.0, 32.0, 300.0]));
    let gif = [b'G', b'I', b'F', b'8', b'9', b'a', 10, 0, 7, 0, 0, 0, 0];
    assert_eq!(image_pixel_info(&gif), Some([10.0, 7.0, 96.0]));
    let mut bmp = vec![0u8; 54];
    bmp[0] = b'B';
    bmp[1] = b'M';
    bmp[18] = 5;
    bmp[22..26].copy_from_slice(&(-3i32).to_le_bytes());
    bmp[38..42].copy_from_slice(&3780u32.to_le_bytes());
    assert_eq!(image_pixel_info(&bmp), Some([5.0, 3.0, 96.0]));
    assert_eq!(image_pixel_info(&[1, 2, 3]), None);
}

#[test]
fn group_child_rotates_about_the_group_centre() {
    let item = GroupItem { x: 0.0.into(), y: 0.0.into(), w: 100.0.into(), h: 50.0.into(), run: Run::default(), ..Default::default() };
    let gr = DrawingGroup { width_twips: 200.0.into(), height_twips: 100.0.into(), ..Default::default() };
    let c = group_child(&gr, &item, 1000.0, 2000.0, 400.0, 200.0);
    assert!(matches!(c.run, std::borrow::Cow::Borrowed(_)));
    assert_eq!((c.x, c.y, c.w, c.h), (1000.0, 2000.0, 200.0, 100.0));
    let gr = DrawingGroup { rot: Some(JsNumber::Num(180.0)), flip_h: Some(true), ..gr };
    let mut run = Run::default();
    run.ext_mut().shape = Some(Box::new(uo_docx::model::run::DrawingShape {
        style: ShapeStyle { rot: Some(30.0.into()), ..Default::default() },
        ..Default::default()
    }));
    let item = GroupItem { run, ..item };
    let c = group_child(&gr, &item, 1000.0, 2000.0, 400.0, 200.0);
    let style = &c.run.ext().unwrap().shape.as_ref().unwrap().style;
    // an odd flip reverses the child's rotation: (-30 + 180) % 360
    assert_eq!(style.rot, Some(JsNumber::Num(150.0)));
    assert_eq!(style.flip_h, Some(true));
    assert!(matches!(c.run, std::borrow::Cow::Owned(_)));
}

#[test]
fn m1_every_entry_point_returns_its_memory() {
    let seeds = generated();
    let (emf, wmf) = (
        seeds.iter().find(|(e, b)| *e && b.len() > 400).unwrap().1.clone(),
        seeds.iter().find(|(e, b)| !*e && b.len() > 100).unwrap().1.clone(),
    );
    let mut f = OneFace::new();
    let mut res = Resources::new();
    // the face is registered once; the measured calls reuse it
    let _ = f.face(&mut res, &[], false, false);
    let run = |emf_: bool, b: &[u8], f: &mut OneFace, res: &Resources| {
        let mut res = res.clone();
        let mut origins = Vec::new();
        let mut out = PassItems::new();
        let r = {
            let mut w = ResWriter::new(&mut res, &mut origins);
            if emf_ {
                render_emf(b, DEST, MetafileLimits::default(), f, &mut w, &mut out)
            } else {
                render_wmf(b, DEST, MetafileLimits::default(), f, &mut w, &mut out)
            }
        };
        black_box((r.is_ok(), out, res, origins));
    };
    let u = assert_no_leak(|| run(true, &emf, &mut f, &res));
    eprintln!("M1 render_emf ({} B file): {} B allocated, {} B peak, 0 B left", emf.len(), u.allocated, u.peak_above_start);
    let u = assert_no_leak(|| run(false, &wmf, &mut f, &res));
    eprintln!("M1 render_wmf ({} B file): {} B allocated, {} B peak, 0 B left", wmf.len(), u.allocated, u.peak_above_start);
    // the drawing values over the committed .NET dumps' first inputs
    let text = std::fs::read_to_string(common::manifest().join("tests/captures/f3/pictures.f3.jsonl")).unwrap();
    let mut media = Media::default();
    let mut theme = Theme::default();
    let mut shapes = Vec::new();
    for l in text.lines() {
        let o = parse(l).unwrap();
        match o.get("f").and_then(J::str).unwrap_or("") {
            "media" => {
                media.by_sha.insert(f3_support::rs(&o, "sha").into(), unhex(o.get("hex").and_then(J::str).unwrap()));
            }
            "theme" => theme = f3_support::theme(o.get("theme").unwrap(), &mut media),
            "shape" => shapes.push(f3_support::shape_style(o.get("s").unwrap(), &mut media)),
            _ => {}
        }
    }
    let s = shapes.first().cloned().unwrap_or_default();
    assert_no_leak(|| {
        let mut res = Resources::new();
        let mut origins = Vec::new();
        let mut w = ResWriter::new(&mut res, &mut origins);
        black_box((shape_geom(&s, &mut w).is_ok(), shape_extras(&s, &theme, &mut w).map(|e| e.grad).is_ok()));
        black_box((w.res.symbols.len(), w.origins.len()));
    });
    let bg = PageBackground {
        fill: Some(Box::new(PageFill {
            r#type: "gradient".into(),
            color: Some("ff0000".into()),
            color2: Some("fill lighten(51)".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_no_leak(|| {
        let mut res = Resources::new();
        let mut origins = Vec::new();
        let r = resolve_page_fill(&bg, Some("FFFFFF"), &media.store, &mut ResWriter::new(&mut res, &mut origins), &mut |_| {});
        black_box((r.map(|p| p.is_some()).is_ok(), res, origins));
    });
    let dc = DrawingColor::plain("@accent1");
    assert_no_leak(|| black_box(subst_ph_clr(&dc, &dc)));
    assert_no_leak(|| black_box(resolve_color2(Some("fill darken(3)"), &units("102030"))));
}
