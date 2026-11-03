//! The interpreter of the op scripts of `tests/parity_support/capture_writer.mjs` over the Rust
//! API: the same scripts the frozen TS ran, so the two files can be compared.

use std::sync::{Arc, OnceLock};

use uo_fonts::{Face, FontLimits};
use uo_pdf::{
    FontId, FontProgram, GlyphOptions, JpegImage, PathCmd, PdfDocument, PdfError, PdfImage, PdfLink, PdfOutlineItem, PdfShading,
    PlacedGlyph, ShadingGeom, SoftMask, Stroke,
};
use uo_testkit::generate::Rng;

use super::json::Json;

pub const FONT_FILES: [&str; 5] =
    ["carlito-regular.ttf", "carlito-bold.ttf", "liberationserif-italic.ttf", "liberationmono-regular.ttf", "caladea-bold.ttf"];

pub struct Fonts {
    pub bytes: Vec<Arc<[u8]>>,
    pub faces: Vec<Face>,
}

pub fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let dir = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR")).join("shared/fonts/files");
        let bytes: Vec<Arc<[u8]>> = FONT_FILES.iter().map(|f| Arc::from(std::fs::read(dir.join(f)).unwrap())).collect();
        let faces = bytes.iter().map(|b| Face::parse(Arc::clone(b), &FontLimits::DEFAULT).unwrap()).collect();
        Fonts { bytes, faces }
    })
}

/// The three programs of the scripts: the face as the painter gives it, the same with the symbol
/// fallback of `fontFor`, and a custom one with every optional field of the TS `FontProgram`.
enum Kind {
    Face,
    Sym,
    Custom { upem: f64, asc: Option<f64>, desc: Option<f64>, bbox: Option<[f64; 4]>, italic: Option<f64> },
}

struct Prog {
    face: Face,
    kind: Kind,
}

impl FontProgram for Prog {
    fn units_per_em(&self) -> f64 {
        match &self.kind {
            Kind::Custom { upem, .. } => *upem,
            _ => f64::from(self.face.metrics().units_per_em),
        }
    }

    fn glyph_id(&self, cp: u32) -> u32 {
        let g = self.face.glyph_id(cp);
        match self.kind {
            Kind::Sym if g == 0 && (0xf000..=0xf0ff).contains(&cp) => self.face.glyph_id(cp - 0xf000),
            _ => g,
        }
    }

    fn advance_width(&self, gid: u32) -> f64 {
        f64::from(self.face.advance_width(gid))
    }

    fn font_bbox(&self) -> Option<[f64; 4]> {
        match &self.kind {
            Kind::Custom { bbox, .. } => *bbox,
            _ => None,
        }
    }

    fn ascender(&self) -> Option<f64> {
        match &self.kind {
            Kind::Custom { asc, .. } => *asc,
            _ => Some(f64::from(self.face.metrics().ascender)),
        }
    }

    fn descender(&self) -> Option<f64> {
        match &self.kind {
            Kind::Custom { desc, .. } => *desc,
            _ => Some(f64::from(self.face.metrics().descender)),
        }
    }

    fn italic_angle(&self) -> Option<f64> {
        match &self.kind {
            Kind::Custom { italic, .. } => *italic,
            _ => None,
        }
    }
}

fn noise(seed: u32, len: usize) -> Vec<u8> {
    let mut r = Rng::new(seed);
    (0..len).map(|_| (r.next_u32() & 255) as u8).collect()
}

pub fn rgba_pixels(seed: u32, w: u32, h: u32, mode: u32) -> Vec<u8> {
    let mut r = Rng::new(seed);
    let extra = if mode == 5 { 2 } else { 0 };
    let n = (w * h + extra) as usize;
    let mut out = vec![0u8; n * 4 + if mode == 5 { 3 } else { 0 }];
    let flat = mode == 2 || mode == 3;
    let alpha = mode == 1 || mode == 3 || mode == 5;
    for i in 0..n {
        for j in 0..3 {
            out[i * 4 + j] = if flat { (r.below(4) * 60) as u8 } else { (r.next_u32() & 255) as u8 };
        }
        out[i * 4 + 3] = if alpha && r.below(3) != 0 { if r.below(2) == 0 { 255 } else { r.below(256) as u8 } } else { 255 };
    }
    if mode == 5 {
        out[n * 4] = 1;
        out[n * 4 + 1] = 2;
        out[n * 4 + 2] = 3;
    }
    out
}

#[derive(Default)]
struct State {
    fonts: Vec<FontId>,
    images: Vec<PdfImage>,
    shadings: Vec<PdfShading>,
}

fn color(v: &Json) -> [f64; 3] {
    let a = v.nums();
    [a[0], a[1], a[2]]
}

fn cmds(list: &Json) -> Vec<PathCmd> {
    list.arr()
        .iter()
        .map(|c| {
            let a = c.arr();
            match a[0].str() {
                "Z" => PathCmd::Z,
                "M" => PathCmd::M { x: a[1].num(), y: a[2].num() },
                "L" => PathCmd::L { x: a[1].num(), y: a[2].num() },
                "C" => PathCmd::C { x1: a[1].num(), y1: a[2].num(), x2: a[3].num(), y2: a[4].num(), x: a[5].num(), y: a[6].num() },
                other => panic!("path op {other}"),
            }
        })
        .collect()
}

/// A resource op, at the document level or in the middle of a page; false for any other op.
fn resource(doc: &mut PdfDocument<Vec<u8>>, st: &mut State, op: &[Json]) -> Result<bool, PdfError> {
    match op[0].str() {
        "font" => {
            let file = op[1].uint() as usize;
            let f = fonts();
            let face = f.faces[file].clone();
            let p = op[3].arr();
            let kind = match p[0].str() {
                "face" => Kind::Face,
                "sym" => Kind::Sym,
                _ => {
                    let opt = |v: &Json| if v.is_null() { None } else { Some(v.num()) };
                    let bbox = if p[4].is_null() {
                        None
                    } else {
                        let b = p[4].nums();
                        Some([b[0], b[1], b[2], b[3]])
                    };
                    Kind::Custom { upem: p[1].num(), asc: opt(&p[2]), desc: opt(&p[3]), bbox, italic: opt(&p[5]) }
                }
            };
            st.fonts.push(doc.embed_font(op[2].str(), Arc::clone(&f.bytes[file]), Prog { face, kind }));
            Ok(true)
        }
        "jpeg" => {
            let bytes = noise(op[1].uint(), op[2].uint() as usize);
            let decode = if op[7].is_null() { Vec::new() } else { op[7].nums() };
            let mask = if op[8].is_null() {
                None
            } else {
                let m = op[8].arr();
                Some((noise(m[0].uint(), m[1].uint() as usize), m[2].uint(), m[3].uint()))
            };
            let jpeg = JpegImage {
                bytes: &bytes,
                width_px: op[3].uint(),
                height_px: op[4].uint(),
                components: op[5].uint(),
                adobe_inverted: op[6].uint() == 1,
                decode: &decode,
                smask: mask.as_ref().map(|(alpha, width, height)| SoftMask { alpha, width: *width, height: *height }),
            };
            st.images.push(doc.embed_jpeg(&jpeg)?);
            Ok(true)
        }
        "rgba" => {
            let (w, h) = (op[2].uint(), op[3].uint());
            st.images.push(doc.embed_rgba(&rgba_pixels(op[1].uint(), w, h, op[4].uint()), w, h)?);
            Ok(true)
        }
        "shading" => {
            let g = op[2].nums();
            let geom = if op[1].uint() == 0 {
                ShadingGeom::Axial { x0: g[0], y0: g[1], x1: g[2], y1: g[3] }
            } else {
                ShadingGeom::Radial { x0: g[0], y0: g[1], r: g[2] }
            };
            let samples = noise(op[4].uint(), op[3].uint() as usize);
            st.shadings.push(doc.add_shading(&geom, &samples, op[5].num())?);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn page_ops(page: &mut uo_pdf::PageWriter<'_, Vec<u8>>, st: &mut State, ops: &[Json]) -> Result<(), PdfError> {
    for op in ops {
        let a = op.arr();
        if resource(page.document(), st, a)? {
            continue;
        }
        match a[0].str() {
            "rect" => page.fill_rect(a[1].num(), a[2].num(), a[3].num(), a[4].num(), &color(&a[5])),
            "line" => page.line(a[1].num(), a[2].num(), a[3].num(), a[4].num(), &color(&a[5]), a[6].num(), &a[7].nums()),
            "glyphs" => {
                let font = st.fonts[a[1].uint() as usize];
                let mut placed = Vec::new();
                for g in a[4].arr() {
                    let g = g.arr();
                    let cp = g[0].uint();
                    let gid = page.encode(font, cp).unwrap().gid;
                    placed.push(PlacedGlyph { gid, code_point: cp, x_pt: g[1].num(), baseline_from_top_pt: g[2].num() });
                }
                let o = &a[5];
                let flag = |k: &str| o.get(k).is_some_and(Json::bool);
                let opts = GlyphOptions {
                    scale_x: o.get("scaleX").map(Json::num),
                    synth_bold: flag("synthBold"),
                    synth_italic: flag("synthItalic"),
                    runs: flag("runs"),
                };
                page.glyphs(font, a[2].num(), &color(&a[3]), &placed, &opts);
            }
            "path" => {
                let fill = if a[2].is_null() { None } else { Some(color(&a[2])) };
                let stroke_parts = if a[3].is_null() {
                    None
                } else {
                    let s = a[3].arr();
                    Some((color(&s[0]), s[1].num(), s[2].nums()))
                };
                let stroke = stroke_parts.as_ref().map(|(c, w, d)| Stroke { color: *c, width_pt: *w, dash: d });
                page.path(&cmds(&a[1]), fill.as_ref(), stroke.as_ref(), a[4].bool());
            }
            "shaded" => page.fill_shaded(&cmds(&a[1]), st.shadings[a[2].uint() as usize], a[3].bool()),
            "clipRect" => page.push_clip_rect(a[1].num(), a[2].num(), a[3].num(), a[4].num()),
            "pop" => page.pop_clip(),
            "clipPath" => page.push_clip_path(&cmds(&a[1]), a[2].bool()),
            "imgMatrix" => {
                let n = &a[2..];
                page.draw_image_matrix(
                    st.images[a[1].uint() as usize],
                    n[0].num(),
                    n[1].num(),
                    n[2].num(),
                    n[3].num(),
                    n[4].num(),
                    n[5].num(),
                );
            }
            "xform" => page.push_transform(a[1].num(), a[2].num(), a[3].num(), a[4].num(), a[5].num(), a[6].num()),
            "imgT" => page.draw_image_transformed(
                st.images[a[1].uint() as usize],
                a[2].num(),
                a[3].num(),
                a[4].num(),
                a[5].num(),
                a[6].num(),
                a[7].bool(),
                a[8].bool(),
            ),
            "img" => page.draw_image(st.images[a[1].uint() as usize], a[2].num(), a[3].num(), a[4].num(), a[5].num()),
            "link" => {
                let l = &a[1];
                let n = |k: &str| l.get(k).unwrap().num();
                page.add_link(&PdfLink {
                    x_pt: n("x"),
                    y_top_pt: n("y"),
                    w_pt: n("w"),
                    h_pt: n("h"),
                    uri: l.get("uri").map(|u| u.str().to_owned()),
                    page_index: l.get("pageIndex").map(Json::int),
                    dest_top_pt: l.get("destTop").map(Json::num),
                });
            }
            "alpha" => {
                let opt = |v: &Json| if v.is_null() { None } else { Some(v.num()) };
                let mut result = Ok(());
                page.with_alpha(opt(&a[1]), opt(&a[2]), |p| result = page_ops(p, st, a[3].arr()));
                result?;
            }
            other => panic!("page op {other}"),
        }
    }
    Ok(())
}

/// Runs a script into a file; the error is the TS `throw` (a code point past U+10FFFF in a ToUnicode map).
pub fn run_script(script: &Json, compress: bool) -> Result<Vec<u8>, PdfError> {
    let mut doc = PdfDocument::new(Vec::new(), compress);
    let mut st = State::default();
    for op in script.arr() {
        let a = op.arr();
        if resource(&mut doc, &mut st, a)? {
            continue;
        }
        match a[0].str() {
            "page" => {
                let mut page = doc.add_page(a[1].num(), a[2].num());
                page_ops(&mut page, &mut st, a[3].arr())?;
                page.finish()?;
            }
            "outline" => doc.add_outline(PdfOutlineItem {
                title: a[1].str().to_owned(),
                level: a[2].int().max(0) as u32,
                page_index: a[3].int(),
                top_pt: a[4].num(),
            }),
            other => panic!("op {other}"),
        }
    }
    doc.build()
}
