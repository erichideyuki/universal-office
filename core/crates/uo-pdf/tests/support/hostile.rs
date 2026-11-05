//! Hostile and large display lists for the no-panic, memory and stress tests of the painter.

use std::path::Path;
use std::sync::Arc;

use uo_fonts::{Face, FontLimits};
use uo_render::types::*;
use uo_render::{
    CustId, FaceId, FaceRes, GlyphRunHead, ImageId, ImageRes, ItemList, LinkTarget, Navigation, OptF64, OutlineEntry, Page, PageFillId,
    PageLink, PdfLink, Resources, Sym, TextFxId,
};
use uo_testkit::generate::Rng;

pub const FONT_FILES: [&str; 4] = ["carlito-regular.ttf", "carlito-bold.ttf", "liberationserif-italic.ttf", "liberationmono-regular.ttf"];

pub fn font_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/files")
}

pub fn hostile(r: &mut Rng) -> f64 {
    match r.below(16) {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => -0.0,
        4 => 5e-324,
        5 => 1e300,
        6 => -1e300,
        7 => 1e9,
        8 => f64::from(r.next_u32()) * 1e6,
        9 => f64::from_bits(u64::from(r.next_u32()) << 32 | u64::from(r.next_u32())),
        _ => f64::from(r.next_u32()) / 4_294_967_296.0 * 12000.0 - 300.0,
    }
}

/// A coordinate: hostile one time in `n`, plain otherwise.
fn num(r: &mut Rng, n: usize) -> f64 {
    if r.one_in(n) { hostile(r) } else { f64::from(r.next_u32()) / 4_294_967_296.0 * 9000.0 }
}

pub fn opt(r: &mut Rng) -> OptF64 {
    match r.below(4) {
        0 => OptF64::NONE,
        1 => OptF64::some(hostile(r)),
        _ => OptF64::some(f64::from(r.next_u32()) / 4_294_967_296.0),
    }
}

pub fn png_rgba(w: u32, h: u32) -> Vec<u8> {
    png_seeded(w, h, 0)
}

/// A PNG of stored deflate blocks, its pixels depending on `seed`.
pub fn png_seeded(w: u32, h: u32, seed: u32) -> Vec<u8> {
    // the stored (uncompressed) deflate blocks of a zlib stream, so no compressor is needed
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&[
                (x.wrapping_mul(9).wrapping_add(seed)) as u8,
                (y.wrapping_mul(7) ^ seed.rotate_left(5)) as u8,
                (x ^ y ^ seed) as u8,
                if (x + y) % 5 == 0 { 0 } else { 255 },
            ]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + u32::from(v)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let chunk = |ty: &[u8; 4], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(ty);
        out.extend_from_slice(data);
        let mut crc = !0u32;
        for &v in ty.iter().chain(data) {
            crc ^= u32::from(v);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        out.extend_from_slice(&(!crc).to_be_bytes());
        out
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &z));
    out.extend(chunk(b"IEND", &[]));
    out
}

pub fn jpeg_shell(w: u16, h: u16, comps: u8) -> Vec<u8> {
    let mut v = vec![0xff, 0xd8, 0xff, 0xc0, 0, 8 + 3 * comps, 8];
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&w.to_be_bytes());
    v.push(comps);
    v.extend(std::iter::repeat_n(0u8, usize::from(comps) * 3));
    v.extend_from_slice(&[0xff, 0xda, 0, 8, 1, 1, 0, 0, 0x3f, 0, 1, 2, 3, 0xff, 0xd9]);
    v
}

/// The symbols the hostile items pick from, and the ids of the resources.
pub struct World {
    pub res: Resources,
    pub syms: Vec<Sym>,
    pub n_faces: usize,
    pub n_images: usize,
}

pub fn world() -> World {
    let mut res = Resources::new();
    for f in FONT_FILES {
        let bytes: Arc<[u8]> = Arc::from(std::fs::read(font_dir().join(f)).unwrap());
        let face = Face::parse(Arc::clone(&bytes), &FontLimits::NATIVE).unwrap();
        let key = res.symbols.intern_str(f).unwrap();
        let family = res.symbols.intern_str("Some Family").unwrap();
        res.add_face(FaceRes {
            key,
            family,
            bold: f.contains("bold"),
            italic: f.contains("italic"),
            synth_bold: false,
            synth_italic: false,
            stand_in: false,
            face: Some(face),
            bytes: Some(bytes),
            advances: None,
        })
        .unwrap();
    }
    // a face without bytes, one without a parsed font
    let key = res.symbols.intern_str("nobytes").unwrap();
    let face = Face::parse(Arc::from(std::fs::read(font_dir().join(FONT_FILES[0])).unwrap()), &FontLimits::NATIVE).unwrap();
    res.add_face(FaceRes {
        key,
        family: key,
        bold: false,
        italic: false,
        synth_bold: false,
        synth_italic: false,
        stand_in: false,
        face: Some(face),
        bytes: None,
        advances: None,
    })
    .unwrap();
    let key = res.symbols.intern_str("noface").unwrap();
    res.add_face(FaceRes {
        key,
        family: key,
        bold: false,
        italic: false,
        synth_bold: false,
        synth_italic: false,
        stand_in: false,
        face: None,
        bytes: Some(Arc::from(vec![0u8; 16])),
        advances: None,
    })
    .unwrap();
    let n_faces = res.faces.len();
    let ext = res.symbols.intern_str("png").unwrap();
    let mut images: Vec<Vec<u8>> = vec![
        png_rgba(9, 7),
        png_rgba(40, 30),
        jpeg_shell(50, 40, 3),
        jpeg_shell(8, 8, 1),
        jpeg_shell(8, 8, 4),
        vec![1, 2, 3, 4, 5],
        Vec::new(),
    ];
    // a PNG that declares 60,000 x 60,000 pixels and holds no data
    let mut huge = png_rgba(2, 2);
    huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
    huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
    images.push(huge);
    for b in images {
        res.add_image(ImageRes {
            bytes: Arc::from(b),
            ext,
            svg: None,
            effects: None,
            width_twips: 0.0,
            height_twips: 0.0,
            natural_px: None,
        })
        .unwrap();
    }
    let n_images = res.images.len();
    let names = [
        "FF0000",
        "00FF00",
        "0000FF",
        "auto",
        "#12ab34",
        "zz",
        "",
        "dash",
        "pct50",
        "pct5",
        "circle",
        "rect",
        "shape",
        "ellipse",
        "roundRect",
        "custom",
        "line",
        "straightConnector1",
        "star5",
        "unknown",
        "tl",
        "ctr",
        "b",
        "sysDash",
        "dot",
        "lgDashDot",
        "000000",
        "\u{1F600}",
        "FFFFFF",
    ];
    let syms: Vec<Sym> = names.iter().map(|n| res.symbols.intern_str(n).unwrap()).collect();
    let red = syms[0];
    // text effects of every kind, page fills of every kind, a custom geometry
    let solid = ResolvedFill::Solid { hex: red, alpha: 0.5 };
    let grad = ResolvedFill::Grad {
        stops: vec![FxStop { pos: 0.0, hex: red, alpha: 1.0 }, FxStop { pos: 1.0, hex: syms[2], alpha: 0.2 }],
        angle_deg: 30.0,
        scaled: true,
        path: None,
        fill_to: None,
    };
    let fx_all = |fill: Option<ResolvedFill>| ResolvedTextFx {
        fill,
        outline: Some(FxOutline { width_pt: 1.5, fill: solid.clone(), dash: Some(syms[23]), cap: LineCap::Butt, join: LineJoin::Miter }),
        shadow: Some(FxShadow {
            blur_pt: 2.0,
            dist_pt: 3.0,
            dir_deg: 45.0,
            sx: 1.0,
            sy: 1.0,
            kx_deg: 0.0,
            ky_deg: 0.0,
            algn: syms[22],
            hex: syms[26],
            alpha: 0.5,
        }),
        glow: Some(FxGlow { rad_pt: 4.0, hex: red, alpha: 0.5 }),
        reflection: Some(FxReflection {
            blur_pt: 0.5,
            st_a: 0.5,
            st_pos: 0.0,
            end_a: 0.0,
            end_pos: 0.9,
            dist_pt: 1.0,
            dir_deg: 90.0,
            algn: syms[22],
        }),
    };
    for fill in [None, Some(solid.clone()), Some(grad.clone()), Some(ResolvedFill::None)] {
        res.add_text_fx(fx_all(fill)).unwrap();
    }
    let kinds = [PageFillKind::Gradient, PageFillKind::GradientRadial, PageFillKind::Pattern, PageFillKind::Frame, PageFillKind::Tile];
    for (i, kind) in kinds.into_iter().enumerate() {
        res.add_page_fill(PageFill {
            kind,
            c1: red,
            c2: syms[2],
            stops: None,
            angle: 45.0,
            focus: 50.0,
            focus_position: None,
            gradient_center: i == 1,
            image: ImageId::from_index(i % 2),
            image_px: Some([40.0, 30.0, 96.0]),
            pattern: Some(vec![f64::from(0x55u8), f64::from(0xAAu8), 1.0, 2.0, 4.0, 8.0, 16.0, 32.0]),
        })
        .unwrap();
    }
    res.add_cust(Arc::from(vec![CustPath {
        w: 100.0,
        h: 100.0,
        fill: true,
        stroke: true,
        cmds: vec![
            CustCmd::M { x: 0.0, y: 0.0 },
            CustCmd::L { x: 100.0, y: 0.0 },
            CustCmd::C { pts: vec![[100.0, 50.0], [50.0, 100.0], [0.0, 100.0]] },
            CustCmd::A { w_r: 50.0, h_r: 50.0, st: 0.0, sw: 90.0 },
            CustCmd::Z,
        ],
    }]))
    .unwrap();
    World { res, syms, n_faces, n_images }
}

fn sym(w: &World, r: &mut Rng) -> Sym {
    w.syms[r.below(w.syms.len())]
}

fn face_id(w: &World, r: &mut Rng) -> FaceId {
    // mostly valid, sometimes a face that does not exist
    FaceId::from_index(if r.one_in(15) { w.n_faces + r.below(5) } else { r.below(w.n_faces) }).unwrap()
}

fn image_id(w: &World, r: &mut Rng) -> ImageId {
    ImageId::from_index(if r.one_in(15) { w.n_images + r.below(5) } else { r.below(w.n_images) }).unwrap()
}

fn unit(r: &mut Rng, lo: f64, hi: f64, hostile_numbers: bool) -> f64 {
    if hostile_numbers && r.one_in(6) { hostile(r) } else { lo + f64::from(r.next_u32()) / 4_294_967_296.0 * (hi - lo) }
}

fn look(w: &World, r: &mut Rng, hn: bool) -> PictureLook {
    let shadow = PicShadow {
        kind: if r.one_in(3) { ShadowKind::Inner } else { ShadowKind::Outer },
        color: sym(w, r),
        alpha: unit(r, 0.0, 1.0, hn),
        blur_pt: unit(r, 0.0, 8.0, hn),
        dist_pt: unit(r, 0.0, 8.0, hn),
        dir_deg: unit(r, 0.0, 360.0, hn),
        sx: opt(r),
        sy: opt(r),
        kx_deg: opt(r),
        ky_deg: opt(r),
        algn: if r.one_in(2) { Some(w.syms[20 + r.below(3)]) } else { None },
        rot_with_shape: r.one_in(2),
    };
    PictureLook {
        geom: if r.one_in(2) {
            Some(PresetSpec { prst: sym(w, r), adj: if r.one_in(3) { Some(vec![(w.syms[7], unit(r, 0.0, 100_000.0, hn))]) } else { None } })
        } else {
            None
        },
        line: if r.one_in(2) {
            Some(PicLine {
                color: sym(w, r),
                alpha: unit(r, 0.0, 1.0, hn),
                width_pt: unit(r, 0.0, 6.0, hn),
                dash: if r.one_in(2) { Some(sym(w, r)) } else { None },
                cmpd: None,
                cap: None,
                join: None,
            })
        } else {
            None
        },
        shadow: if r.one_in(2) { Some(shadow) } else { None },
        glow: if r.one_in(3) {
            Some(PicGlow { color: sym(w, r), alpha: unit(r, 0.0, 1.0, hn), rad_pt: unit(r, 0.0, 12.0, hn) })
        } else {
            None
        },
        soft_edge_pt: if r.one_in(3) { OptF64::some(unit(r, 0.0, 10.0, hn)) } else { OptF64::NONE },
        bevel: if r.one_in(4) {
            Some(PicBevel {
                w_pt: unit(r, 0.0, 8.0, hn),
                h_pt: unit(r, 0.0, 8.0, hn),
                prst: if r.one_in(2) { Some(sym(w, r)) } else { None },
                rig: None,
                dir: None,
                material: None,
            })
        } else {
            None
        },
        reflection: if r.one_in(3) {
            Some(PicReflection {
                blur_pt: unit(r, 0.0, 2.0, hn),
                st_a: unit(r, 0.0, 1.0, hn),
                st_pos: unit(r, 0.0, 1.0, hn),
                end_a: unit(r, 0.0, 1.0, hn),
                end_pos: unit(r, 0.0, 1.0, hn),
                dist_pt: unit(r, 0.0, 6.0, hn),
                dir_deg: 90.0,
                sy: -1.0,
                algn: None,
                rot_with_shape: false,
            })
        } else {
            None
        },
        alpha: opt(r),
        duotone: if r.one_in(5) { Some([sym(w, r), sym(w, r)]) } else { None },
        clr_change: if r.one_in(5) { Some(ClrChange { from: sym(w, r), to: sym(w, r), to_alpha: unit(r, 0.0, 1.0, hn) }) } else { None },
    }
}

fn shape(w: &World, r: &mut Rng) -> ShapeItem {
    let hostile_frame = r.one_in(8);
    let (x, y, sw, sh) = if hostile_frame {
        (hostile(r), hostile(r), hostile(r), hostile(r))
    } else {
        (num(r, 50), num(r, 50), 200.0 + num(r, 50).abs() % 5000.0, 200.0 + num(r, 50).abs() % 4000.0)
    };
    let stops = (0..1 + r.below(4)).map(|i| GradStop { pos: i as f64 / 3.0, color: sym(w, r) }).collect();
    let fx_look = look(w, r, false);
    ShapeItem {
        x,
        y,
        w: sw,
        h: sh,
        geom: ShapeGeom {
            geom: sym(w, r),
            adj: if r.one_in(3) { Some(vec![(w.syms[7], hostile(r))]) } else { None },
            cust: if r.one_in(3) { CustId::from_index(r.below(2)) } else { None },
            flip_h: r.one_in(3),
            flip_v: r.one_in(3),
            rot: opt(r),
            fill_alpha: opt(r),
            line_alpha: opt(r),
            eval: GeomEval::Compact,
        },
        fill: if r.one_in(2) { Some(sym(w, r)) } else { None },
        stroke: if r.one_in(2) {
            Some(Stroke { color: sym(w, r), width_pt: num(r, 6) % 8.0, dash: if r.one_in(2) { Some(sym(w, r)) } else { None } })
        } else {
            None
        },
        line: if r.one_in(4) { Some([num(r, 6), num(r, 6), num(r, 6), num(r, 6)]) } else { None },
        grad: if r.one_in(3) {
            Some(GradFill {
                stops,
                angle_deg: num(r, 6),
                scaled: r.one_in(2),
                rot_with_shape: r.one_in(2),
                path: if r.one_in(2) { Some(w.syms[10 + r.below(3)]) } else { None },
                fill_to: if r.one_in(2) { Some(Ltrb { l: num(r, 8), t: 0.3, r: 0.3, b: num(r, 8) }) } else { None },
            })
        } else {
            None
        },
        shadow: if r.one_in(4) {
            Some(ShapeShadow { dx: num(r, 6), dy: num(r, 6), color: sym(w, r), alpha: opt(r), blur: opt(r) })
        } else {
            None
        },
        pattern: if r.one_in(4) { Some(Pattern { prst: sym(w, r), fg: sym(w, r), bg: sym(w, r) }) } else { None },
        blip: if r.one_in(4) {
            Some(Blip {
                image: image_id(w, r),
                tile: if r.one_in(2) {
                    Some(BlipTile {
                        tx: num(r, 6),
                        ty: num(r, 6),
                        sx: if r.one_in(3) { 1e-9 } else { num(r, 6) },
                        sy: num(r, 6),
                        algn: sym(w, r),
                        flip: sym(w, r),
                    })
                } else {
                    None
                },
                alpha: opt(r),
            })
        } else {
            None
        },
        fx: if r.one_in(4) {
            Some(ShapeFx {
                shadow: fx_look.shadow,
                glow: fx_look.glow,
                soft_edge_pt: OptF64::some(f64::from(r.next_u32() % 8)),
                reflection: fx_look.reflection,
            })
        } else {
            None
        },
    }
}

/// A page of random items, hostile numbers included.
pub fn hostile_page(w: &World, r: &mut Rng, with_pictures: bool) -> Page {
    let mut list = ItemList::new();
    for _ in 0..r.below(24) {
        match r.below(16) {
            0..=3 => {
                let n = r.below(20);
                let cps: Vec<(u32, f64, Option<u32>)> = (0..n)
                    .map(|_| {
                        (
                            if r.one_in(10) { r.next_u32() } else { 0x20 + r.next_u32() % 0x2000 },
                            num(r, 12),
                            if r.one_in(8) { Some(r.next_u32() % 400) } else { None },
                        )
                    })
                    .collect();
                let mut flags = RunFlags::default();
                for f in [RunFlags::SYNTH_BOLD, RunFlags::SYNTH_ITALIC, RunFlags::AS_PATH, RunFlags::COLR] {
                    flags = flags.with(f, r.one_in(4));
                }
                let head = GlyphRunHead {
                    face: face_id(w, r),
                    color: sym(w, r),
                    size_pt: if r.one_in(10) { hostile(r) } else { 6.0 + f64::from(r.next_u32() % 40) },
                    y: num(r, 10),
                    text: &[],
                    scale_x: opt(r),
                    alpha: opt(r),
                    fx: if r.one_in(4) { TextFxId::from_index(r.below(6)) } else { None },
                    flags,
                };
                list.push_glyphs(head, cps).unwrap();
            }
            4 => list.push_plain(Item::Rect { x: num(r, 6), y: num(r, 6), w: num(r, 6), h: num(r, 6), fill: sym(w, r), alpha: opt(r) }),
            5 => {
                let dashes = [
                    None,
                    Some(LineDash::Dotted),
                    Some(LineDash::Dashed),
                    Some(LineDash::DashDot),
                    Some(LineDash::BdrDash),
                    Some(LineDash::BdrDashSmall),
                ];
                list.push_plain(Item::Line {
                    x1: num(r, 6),
                    y1: num(r, 6),
                    x2: num(r, 6),
                    y2: num(r, 6),
                    color: sym(w, r),
                    width_pt: num(r, 8),
                    dash: dashes[r.below(dashes.len())],
                });
            }
            6 | 7 if with_pictures => {
                let pic = if r.one_in(2) { Some(Box::new(look(w, r, true))) } else { None };
                list.push_plain(Item::Image(Box::new(ImageItem {
                    x: num(r, 10),
                    y: num(r, 10),
                    w: if r.one_in(10) { hostile(r) } else { 200.0 + num(r, 30).abs() % 5000.0 },
                    h: if r.one_in(10) { hostile(r) } else { 200.0 + num(r, 30).abs() % 5000.0 },
                    image: image_id(w, r),
                    crop: if r.one_in(3) {
                        Some(Ltrb { l: num(r, 4) % 2.0, t: num(r, 4) % 2.0, r: num(r, 4) % 2.0, b: num(r, 4) % 2.0 })
                    } else {
                        None
                    },
                    rot: opt(r),
                    flip_h: r.one_in(3),
                    flip_v: r.one_in(3),
                    pic,
                })));
            }
            8..=10 if with_pictures => list.push_plain(Item::Shape(Box::new(shape(w, r)))),
            11 => {
                let cmds: Vec<PathCmd> = (0..r.below(8))
                    .map(|_| match r.below(4) {
                        0 => PathCmd::M { x: num(r, 6), y: num(r, 6) },
                        1 => PathCmd::L { x: num(r, 6), y: num(r, 6) },
                        2 => PathCmd::C { x1: num(r, 6), y1: num(r, 6), x2: num(r, 6), y2: num(r, 6), x: num(r, 6), y: num(r, 6) },
                        _ => PathCmd::Z,
                    })
                    .collect();
                list.push_path(
                    &cmds,
                    if r.one_in(2) { Some(sym(w, r)) } else { None },
                    if r.one_in(2) { Some(Stroke { color: sym(w, r), width_pt: num(r, 6), dash: Some(sym(w, r)) }) } else { None },
                    r.one_in(2),
                )
                .unwrap();
            }
            12 => list.push_plain(Item::ClipPush { x: opt(r), y: opt(r), w: opt(r), h: opt(r) }),
            13 => list.push_plain(Item::ClipPop),
            14 => list.push_plain(Item::XformPush {
                m: if r.one_in(4) { None } else { Some(Box::new([num(r, 4), num(r, 4), num(r, 4), num(r, 4), num(r, 4), num(r, 4)])) },
            }),
            _ => list.push_plain(Item::XformPop),
        }
    }
    Page {
        w: if r.one_in(30) { hostile(r) } else { 4000.0 + f64::from(r.next_u32() % 12000) },
        h: if r.one_in(30) { hostile(r) } else { 4000.0 + f64::from(r.next_u32() % 12000) },
        list,
        background: if r.one_in(4) { Some(sym(w, r)) } else { None },
        background_fill: if r.one_in(4) { PageFillId::from_index(r.below(7)) } else { None },
        background_show: r.one_in(2),
    }
}

pub fn hostile_nav(r: &mut Rng, pages: usize) -> Navigation {
    let links = (0..r.below(10))
        .map(|_| PageLink {
            page: u32::try_from(r.below(pages + 2)).unwrap(),
            link: PdfLink {
                x_pt: hostile(r),
                y_top_pt: hostile(r),
                w_pt: hostile(r),
                h_pt: hostile(r),
                target: if r.one_in(2) {
                    LinkTarget::Uri("http://x/(y) z".encode_utf16().collect::<Vec<_>>().into())
                } else {
                    LinkTarget::Dest { page_index: r.next_u32(), dest_top_pt: hostile(r) }
                },
            },
        })
        .collect();
    let outline = (0..r.below(8))
        .map(|_| OutlineEntry {
            title: [0xd800u16, 0x41, 0x20].into(),
            level: r.next_u32() % 12,
            page_index: r.next_u32() % 5,
            top_pt: hostile(r),
        })
        .collect();
    Navigation { links, outline }
}

/// `n` pages of text: `lines` lines of `words` words each, in the four open faces (the stress of the plan).
pub fn text_pages(w: &World, n: usize, lines: usize, words: usize) -> Vec<Page> {
    let mut r = Rng::new(7);
    (0..n)
        .map(|_| {
            let mut list = ItemList::new();
            for l in 0..lines {
                let mut x = 1440.0;
                for _ in 0..words {
                    let cps: Vec<(u32, f64, Option<u32>)> = (0..2 + r.below(7))
                        .map(|k| {
                            let cp = 0x61 + r.next_u32() % 26;
                            (cp, x + k as f64 * 105.0, None)
                        })
                        .collect();
                    x += (cps.len() as f64) * 105.0 + 230.0;
                    let head = GlyphRunHead {
                        face: FaceId::from_index(r.below(4)).unwrap(),
                        color: w.syms[0],
                        size_pt: 11.0,
                        y: 1440.0 + l as f64 * 280.0,
                        text: &[],
                        scale_x: OptF64::NONE,
                        alpha: OptF64::NONE,
                        fx: None,
                        flags: RunFlags::default(),
                    };
                    list.push_glyphs(head, cps).unwrap();
                }
            }
            Page { w: 12240.0, h: 15840.0, list, background: None, background_fill: None, background_show: false }
        })
        .collect()
}
