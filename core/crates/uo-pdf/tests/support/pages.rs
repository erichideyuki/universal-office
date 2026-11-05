//! The spec of the painter capture (`capture_painter.mjs`) decoded into uo-render pages and
//! resources: the LayoutResult of the frozen TS in the shape of `tree.ts`, with faces and images
//! by index.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uo_drawingml::ImageEffects;
use uo_fonts::registry::AdvanceTable;
use uo_fonts::{Face, FontLimits};
use uo_render::types::*;
use uo_render::{
    CustId, FaceId, FaceRes, GlyphRunHead, ImageId, ImageRes, ItemList, LinkTarget, Navigation, OptF64, OutlineEntry, Page, PageLink,
    PdfLink, Resources, Sym,
};

use super::json::{Json, parse};

pub struct Loaded {
    pub pages: Vec<Page>,
    pub res: Resources,
    pub nav: Option<Navigation>,
}

/// Where the bytes of faces and images come from.
pub struct Sources {
    pub font_dir: PathBuf,
    /// the dump of a real-document capture: `fonts/<sha>.bin`, `media/<sha>.bin`
    pub dump: Option<PathBuf>,
}

impl Sources {
    pub fn open_fonts() -> Sources {
        Sources { font_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fonts/files"), dump: None }
    }

    fn font(&self, f: &Json) -> Arc<[u8]> {
        if let Some(file) = f.get("file") {
            let site = self.font_dir.join("../../../site/assets/fonts").join(file.str());
            let path = if self.font_dir.join(file.str()).exists() { self.font_dir.join(file.str()) } else { site };
            return Arc::from(std::fs::read(&path).unwrap_or_else(|e| panic!("font {}: {e}", path.display())));
        }
        let sha = f.get("sha").unwrap().str();
        let dump = self.dump.as_ref().expect("a dump directory for the faces of a real document");
        Arc::from(std::fs::read(dump.join("fonts").join(format!("{sha}.bin"))).unwrap_or_else(|e| panic!("face {sha}: {e}")))
    }

    fn image(&self, im: &Json) -> Vec<u8> {
        if let Some(b) = im.get("b64") {
            return b64(b.str());
        }
        let sha = im.get("sha").unwrap().str();
        let dump = self.dump.as_ref().expect("a dump directory for the images of a real document");
        std::fs::read(dump.join("media").join(format!("{sha}.bin"))).unwrap_or_else(|e| panic!("image {sha}: {e}"))
    }
}

fn b64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

pub fn load_text(text: &str, src: &Sources) -> Loaded {
    load(&parse(text), src)
}

pub fn load(spec: &Json, src: &Sources) -> Loaded {
    let mut b = Builder { res: Resources::new() };
    for f in spec.get("faces").unwrap().arr() {
        let bytes = src.font(f);
        let face = Face::parse(Arc::clone(&bytes), &FontLimits::NATIVE).expect("the face parses");
        let advances = f.get("adv").map(|a| {
            let mut t = AdvanceTable::new();
            if let Json::Obj(sizes) = a {
                for (size, chars) in sizes {
                    if let Json::Obj(chars) = chars {
                        for (ch, v) in chars {
                            let mut it = ch.chars();
                            if let (Some(c), None) = (it.next(), it.next()) {
                                t.insert(size, u32::from(c), v.num()).unwrap();
                            }
                        }
                    }
                }
            }
            Arc::new(t)
        });
        let no_bytes = f.get("bytes").is_some_and(|v| *v == Json::Bool(false));
        let fr = FaceRes {
            key: b.sym(f.get("key").unwrap().str()),
            family: b.sym(f.get("family").unwrap().str()),
            bold: f.get("bold").is_some_and(Json::bool),
            italic: f.get("italic").is_some_and(Json::bool),
            synth_bold: false,
            synth_italic: false,
            stand_in: false,
            face: Some(face),
            bytes: if no_bytes { None } else { Some(bytes) },
            advances,
        };
        b.res.add_face(fr).unwrap();
    }
    for im in spec.get("images").unwrap().arr() {
        let ext = b.sym(im.get("ext").map_or("", Json::str));
        let effects = im.get("fx").map(|fx| ImageEffects {
            gray: fx.get("gray").is_some_and(Json::bool),
            gain: fx.get("gain").map(Json::num),
            bright: fx.get("bright").map(Json::num),
            floor: fx.get("floor").is_some_and(Json::bool),
            bi_level: fx.get("biLevel").map(Json::num),
        });
        b.res
            .add_image(ImageRes {
                bytes: Arc::from(src.image(im)),
                ext,
                svg: None,
                effects,
                width_twips: 0.0,
                height_twips: 0.0,
                natural_px: None,
            })
            .unwrap();
    }
    let pages = spec.get("pages").unwrap().arr().iter().map(|p| b.page(p)).collect();
    let nav = spec.get("nav").filter(|n| !n.is_null()).map(|n| b.nav(n));
    Loaded { pages, res: b.res, nav }
}

struct Builder {
    res: Resources,
}

fn opt(j: &Json, k: &str) -> OptF64 {
    j.get(k).map_or(OptF64::NONE, |v| OptF64::some(v.num()))
}

fn flag(j: &Json, k: &str) -> bool {
    j.get(k).is_some_and(Json::bool)
}

fn f(j: &Json, k: &str) -> f64 {
    j.get(k).unwrap_or_else(|| panic!("missing {k} in {j:?}")).num()
}

impl Builder {
    fn sym(&mut self, s: &str) -> Sym {
        self.res.symbols.intern_str(s).unwrap()
    }

    fn osym(&mut self, j: &Json, k: &str) -> Option<Sym> {
        j.get(k).map(|v| self.sym(v.str()))
    }

    fn ltrb(j: &Json) -> Ltrb {
        Ltrb { l: f(j, "l"), t: f(j, "t"), r: f(j, "r"), b: f(j, "b") }
    }

    fn page(&mut self, p: &Json) -> Page {
        let mut page = Page { w: f(p, "widthTw"), h: f(p, "heightTw"), ..Page::default() };
        page.background = self.osym(p, "background");
        if let Some(bf) = p.get("backgroundFill") {
            let fill = self.page_fill(bf);
            page.background_fill = Some(self.res.add_page_fill(fill).unwrap());
        }
        for it in p.get("items").unwrap().arr() {
            self.item(&mut page.list, it);
        }
        page
    }

    fn page_fill(&mut self, f_: &Json) -> PageFill {
        let kind = match f_.get("type").unwrap().str() {
            "gradient" => PageFillKind::Gradient,
            "gradientRadial" => PageFillKind::GradientRadial,
            "pattern" => PageFillKind::Pattern,
            "frame" => PageFillKind::Frame,
            "tile" => PageFillKind::Tile,
            other => panic!("page fill {other}"),
        };
        PageFill {
            kind,
            c1: self.sym(f_.get("c1").unwrap().str()),
            c2: self.sym(f_.get("c2").unwrap().str()),
            stops: f_.get("stops").map(|s| {
                s.arr().iter().map(|st| GradStop { pos: f(st, "pos"), color: self.sym(st.get("color").unwrap().str()) }).collect()
            }),
            angle: f(f_, "angle"),
            focus: f(f_, "focus"),
            focus_position: f_.get("focusPosition").map(|a| [a.arr()[0].num(), a.arr()[1].num()]),
            gradient_center: flag(f_, "gradientCenter"),
            image: f_.get("image").map(|i| ImageId::from_index(usize::try_from(i.uint()).unwrap()).unwrap()),
            image_px: f_.get("imagePx").map(|p| [f(p, "w"), f(p, "h"), f(p, "dpi")]),
            pattern: f_.get("pattern").map(Json::nums),
        }
    }

    fn stroke(&mut self, s: &Json) -> Stroke {
        Stroke { color: self.sym(s.get("color").unwrap().str()), width_pt: f(s, "widthPt"), dash: self.osym(s, "dash") }
    }

    fn adj(&mut self, j: &Json) -> Option<Vec<(Sym, f64)>> {
        j.get("adj").map(|a| match a {
            Json::Obj(o) => o.iter().map(|(k, v)| (self.sym(k), v.num())).collect(),
            _ => panic!("adj"),
        })
    }

    fn item(&mut self, list: &mut ItemList, it: &Json) {
        match it.get("t").unwrap().str() {
            "glyphs" => {
                let fx = it.get("fx").map(|fx| {
                    let r = self.text_fx(fx);
                    self.res.add_text_fx(r).unwrap()
                });
                let mut flags = RunFlags::default();
                flags = flags.with(RunFlags::SYNTH_BOLD, flag(it, "synthBold"));
                flags = flags.with(RunFlags::SYNTH_ITALIC, flag(it, "synthItalic"));
                flags = flags.with(RunFlags::AS_PATH, flag(it, "asPath"));
                flags = flags.with(RunFlags::COLR, flag(it, "colr"));
                let head = GlyphRunHead {
                    face: FaceId::from_index(usize::try_from(it.get("face").unwrap().uint()).unwrap()).unwrap(),
                    color: self.sym(it.get("color").unwrap().str()),
                    size_pt: f(it, "sizePt"),
                    y: f(it, "y"),
                    text: &[],
                    scale_x: opt(it, "scaleX"),
                    alpha: opt(it, "alpha"),
                    fx,
                    flags,
                };
                let glyphs = it.get("glyphs").unwrap().arr().iter().map(|g| {
                    let g = g.arr();
                    (g[0].uint(), g[1].num(), g.get(2).map(Json::uint))
                });
                list.push_glyphs(head, glyphs).unwrap();
            }
            "rect" => {
                let fill = self.sym(it.get("fill").unwrap().str());
                list.push_plain(Item::Rect { x: f(it, "x"), y: f(it, "y"), w: f(it, "w"), h: f(it, "h"), fill, alpha: opt(it, "alpha") });
            }
            "line" => {
                let color = self.sym(it.get("color").unwrap().str());
                let dash = it.get("dash").map(|d| LineDash::from_name(d.str()).expect("dash"));
                list.push_plain(Item::Line {
                    x1: f(it, "x1"),
                    y1: f(it, "y1"),
                    x2: f(it, "x2"),
                    y2: f(it, "y2"),
                    color,
                    width_pt: f(it, "widthPt"),
                    dash,
                });
            }
            "image" => {
                let pic = it.get("pic").map(|p| Box::new(self.look(p)));
                list.push_plain(Item::Image(Box::new(ImageItem {
                    x: f(it, "x"),
                    y: f(it, "y"),
                    w: f(it, "w"),
                    h: f(it, "h"),
                    image: ImageId::from_index(usize::try_from(it.get("image").unwrap().uint()).unwrap()).unwrap(),
                    crop: it.get("crop").map(Self::ltrb),
                    rot: opt(it, "rot"),
                    flip_h: flag(it, "flipH"),
                    flip_v: flag(it, "flipV"),
                    pic,
                })));
            }
            "shape" => {
                let sh = self.shape(it);
                list.push_plain(Item::Shape(Box::new(sh)));
            }
            "path" => {
                let cmds: Vec<PathCmd> = it
                    .get("cmds")
                    .unwrap()
                    .arr()
                    .iter()
                    .map(|c| match c.get("op").unwrap().str() {
                        "M" => PathCmd::M { x: f(c, "x"), y: f(c, "y") },
                        "L" => PathCmd::L { x: f(c, "x"), y: f(c, "y") },
                        "C" => PathCmd::C { x1: f(c, "x1"), y1: f(c, "y1"), x2: f(c, "x2"), y2: f(c, "y2"), x: f(c, "x"), y: f(c, "y") },
                        _ => PathCmd::Z,
                    })
                    .collect();
                let fill = self.osym(it, "fill");
                let stroke = it.get("stroke").map(|s| self.stroke(s));
                list.push_path(&cmds, fill, stroke, flag(it, "evenOdd")).unwrap();
            }
            "clip" => {
                if it.get("op").unwrap().str() == "push" {
                    list.push_plain(Item::ClipPush { x: opt(it, "x"), y: opt(it, "y"), w: opt(it, "w"), h: opt(it, "h") });
                } else {
                    list.push_plain(Item::ClipPop);
                }
            }
            "xform" => {
                if it.get("op").unwrap().str() == "push" {
                    let m = it.get("m").map(|m| {
                        let v = m.nums();
                        Box::new([v[0], v[1], v[2], v[3], v[4], v[5]])
                    });
                    list.push_plain(Item::XformPush { m });
                } else {
                    list.push_plain(Item::XformPop);
                }
            }
            other => panic!("item {other}"),
        }
    }

    fn fill(&mut self, j: &Json) -> ResolvedFill {
        match j.get("kind").unwrap().str() {
            "none" => ResolvedFill::None,
            "solid" => ResolvedFill::Solid { hex: self.sym(j.get("hex").unwrap().str()), alpha: f(j, "alpha") },
            _ => ResolvedFill::Grad {
                stops: j
                    .get("stops")
                    .unwrap()
                    .arr()
                    .iter()
                    .map(|s| FxStop { pos: f(s, "pos"), hex: self.sym(s.get("hex").unwrap().str()), alpha: f(s, "alpha") })
                    .collect(),
                angle_deg: f(j, "angleDeg"),
                scaled: flag(j, "scaled"),
                path: self.osym(j, "path"),
                fill_to: j.get("fillTo").map(Self::ltrb),
            },
        }
    }

    fn text_fx(&mut self, j: &Json) -> ResolvedTextFx {
        let cap = |s: &str| match s {
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => LineCap::Butt,
        };
        let join = |s: &str| match s {
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
        ResolvedTextFx {
            fill: j.get("fill").map(|x| self.fill(x)),
            outline: j.get("outline").map(|o| FxOutline {
                width_pt: f(o, "widthPt"),
                fill: self.fill(o.get("fill").unwrap()),
                dash: self.osym(o, "dash"),
                cap: cap(o.get("cap").map_or("butt", Json::str)),
                join: join(o.get("join").map_or("miter", Json::str)),
            }),
            shadow: j.get("shadow").map(|s| FxShadow {
                blur_pt: f(s, "blurPt"),
                dist_pt: f(s, "distPt"),
                dir_deg: f(s, "dirDeg"),
                sx: f(s, "sx"),
                sy: f(s, "sy"),
                kx_deg: f(s, "kxDeg"),
                ky_deg: f(s, "kyDeg"),
                algn: self.sym(s.get("algn").unwrap().str()),
                hex: self.sym(s.get("hex").unwrap().str()),
                alpha: f(s, "alpha"),
            }),
            glow: j.get("glow").map(|g| FxGlow { rad_pt: f(g, "radPt"), hex: self.sym(g.get("hex").unwrap().str()), alpha: f(g, "alpha") }),
            reflection: j.get("reflection").map(|r| FxReflection {
                blur_pt: f(r, "blurPt"),
                st_a: f(r, "stA"),
                st_pos: f(r, "stPos"),
                end_a: f(r, "endA"),
                end_pos: f(r, "endPos"),
                dist_pt: f(r, "distPt"),
                dir_deg: f(r, "dirDeg"),
                algn: self.sym(r.get("algn").unwrap().str()),
            }),
        }
    }

    fn pic_shadow(&mut self, s: &Json) -> PicShadow {
        PicShadow {
            kind: if s.get("kind").unwrap().str() == "inner" { ShadowKind::Inner } else { ShadowKind::Outer },
            color: self.sym(s.get("color").unwrap().str()),
            alpha: f(s, "alpha"),
            blur_pt: f(s, "blurPt"),
            dist_pt: f(s, "distPt"),
            dir_deg: f(s, "dirDeg"),
            sx: opt(s, "sx"),
            sy: opt(s, "sy"),
            kx_deg: opt(s, "kxDeg"),
            ky_deg: opt(s, "kyDeg"),
            algn: self.osym(s, "algn"),
            rot_with_shape: flag(s, "rotWithShape"),
        }
    }

    fn pic_glow(&mut self, g: &Json) -> PicGlow {
        PicGlow { color: self.sym(g.get("color").unwrap().str()), alpha: f(g, "alpha"), rad_pt: f(g, "radPt") }
    }

    fn pic_reflection(&mut self, r: &Json) -> PicReflection {
        PicReflection {
            blur_pt: f(r, "blurPt"),
            st_a: f(r, "stA"),
            st_pos: f(r, "stPos"),
            end_a: f(r, "endA"),
            end_pos: f(r, "endPos"),
            dist_pt: f(r, "distPt"),
            dir_deg: f(r, "dirDeg"),
            sy: f(r, "sy"),
            algn: self.osym(r, "algn"),
            rot_with_shape: flag(r, "rotWithShape"),
        }
    }

    fn look(&mut self, p: &Json) -> PictureLook {
        PictureLook {
            geom: p.get("geom").map(|g| PresetSpec { prst: self.sym(g.get("prst").unwrap().str()), adj: self.adj(g) }),
            line: p.get("line").map(|l| PicLine {
                color: self.sym(l.get("color").unwrap().str()),
                alpha: f(l, "alpha"),
                width_pt: f(l, "widthPt"),
                dash: self.osym(l, "dash"),
                cmpd: None,
                cap: None,
                join: None,
            }),
            shadow: p.get("shadow").map(|s| self.pic_shadow(s)),
            glow: p.get("glow").map(|g| self.pic_glow(g)),
            soft_edge_pt: opt(p, "softEdgePt"),
            bevel: p.get("bevel").map(|b| PicBevel {
                w_pt: f(b, "wPt"),
                h_pt: f(b, "hPt"),
                prst: self.osym(b, "prst"),
                rig: self.osym(b, "rig"),
                dir: self.osym(b, "dir"),
                material: self.osym(b, "material"),
            }),
            reflection: p.get("reflection").map(|r| self.pic_reflection(r)),
            alpha: opt(p, "alpha"),
            duotone: p.get("duotone").map(|d| [self.sym(d.arr()[0].str()), self.sym(d.arr()[1].str())]),
            clr_change: p.get("clrChange").map(|c| ClrChange {
                from: self.sym(c.get("from").unwrap().str()),
                to: self.sym(c.get("to").unwrap().str()),
                to_alpha: f(c, "toAlpha"),
            }),
        }
    }

    fn shape(&mut self, it: &Json) -> ShapeItem {
        let st = it.get("style").unwrap();
        let cust: Option<CustId> = st.get("cust").map(|c| {
            let paths: Vec<CustPath> = c
                .arr()
                .iter()
                .map(|p| CustPath {
                    w: f(p, "w"),
                    h: f(p, "h"),
                    fill: flag(p, "fill"),
                    stroke: flag(p, "stroke"),
                    cmds: p
                        .get("cmds")
                        .unwrap()
                        .arr()
                        .iter()
                        .map(|c| match c.get("op").unwrap().str() {
                            "M" => CustCmd::M { x: f(c, "x"), y: f(c, "y") },
                            "L" => CustCmd::L { x: f(c, "x"), y: f(c, "y") },
                            "C" => CustCmd::C {
                                pts: c.get("pts").unwrap().arr().iter().map(|q| [q.arr()[0].num(), q.arr()[1].num()]).collect(),
                            },
                            "Q" => CustCmd::Q {
                                pts: c.get("pts").unwrap().arr().iter().map(|q| [q.arr()[0].num(), q.arr()[1].num()]).collect(),
                            },
                            "A" => CustCmd::A { w_r: f(c, "wR"), h_r: f(c, "hR"), st: f(c, "st"), sw: f(c, "sw") },
                            _ => CustCmd::Z,
                        })
                        .collect(),
                })
                .collect();
            self.res.add_cust(Arc::from(paths)).unwrap()
        });
        let geom = ShapeGeom {
            geom: self.sym(st.get("geom").unwrap().str()),
            adj: self.adj(st),
            cust,
            flip_h: flag(st, "flipH"),
            flip_v: flag(st, "flipV"),
            rot: opt(st, "rot"),
            fill_alpha: opt(st, "fillAlpha"),
            line_alpha: st.get("line").map_or(OptF64::NONE, |l| opt(l, "alpha")),
            eval: GeomEval::Compact,
        };
        ShapeItem {
            x: f(it, "x"),
            y: f(it, "y"),
            w: f(it, "w"),
            h: f(it, "h"),
            geom,
            fill: self.osym(it, "fill"),
            stroke: it.get("stroke").map(|s| self.stroke(s)),
            line: it.get("line").map(|l| [f(l, "x1"), f(l, "y1"), f(l, "x2"), f(l, "y2")]),
            grad: it.get("grad").map(|g| GradFill {
                stops: g
                    .get("stops")
                    .unwrap()
                    .arr()
                    .iter()
                    .map(|s| GradStop { pos: f(s, "pos"), color: self.sym(s.get("color").unwrap().str()) })
                    .collect(),
                angle_deg: f(g, "angleDeg"),
                scaled: flag(g, "scaled"),
                rot_with_shape: g.get("rotWithShape").is_none_or(|v| *v != Json::Bool(false)),
                path: self.osym(g, "path"),
                fill_to: g.get("fillTo").map(Self::ltrb),
            }),
            shadow: it.get("shadow").map(|s| ShapeShadow {
                dx: f(s, "dx"),
                dy: f(s, "dy"),
                color: self.sym(s.get("color").unwrap().str()),
                alpha: opt(s, "alpha"),
                blur: opt(s, "blur"),
            }),
            pattern: it.get("pattern").map(|p| Pattern {
                prst: self.sym(p.get("prst").unwrap().str()),
                fg: self.sym(p.get("fg").unwrap().str()),
                bg: self.sym(p.get("bg").unwrap().str()),
            }),
            blip: it.get("blip").map(|b| Blip {
                image: ImageId::from_index(usize::try_from(b.get("image").unwrap().uint()).unwrap()).unwrap(),
                tile: b.get("tile").map(|t| BlipTile {
                    tx: f(t, "tx"),
                    ty: f(t, "ty"),
                    sx: f(t, "sx"),
                    sy: f(t, "sy"),
                    algn: self.sym(t.get("algn").unwrap().str()),
                    flip: self.sym(t.get("flip").unwrap().str()),
                }),
                alpha: opt(b, "alpha"),
            }),
            fx: it.get("fx").map(|x| ShapeFx {
                shadow: x.get("shadow").map(|s| self.pic_shadow(s)),
                glow: x.get("glow").map(|g| self.pic_glow(g)),
                soft_edge_pt: opt(x, "softEdgePt"),
                reflection: x.get("reflection").map(|r| self.pic_reflection(r)),
            }),
        }
    }

    fn nav(&mut self, n: &Json) -> Navigation {
        let links = n
            .get("links")
            .unwrap()
            .arr()
            .iter()
            .map(|l| {
                let k = l.get("link").unwrap();
                let target = match k.get("uri") {
                    Some(u) => LinkTarget::Uri(u.str().encode_utf16().collect::<Vec<u16>>().into()),
                    None => LinkTarget::Dest { page_index: k.get("pageIndex").unwrap().uint(), dest_top_pt: f(k, "destTopPt") },
                };
                PageLink {
                    page: l.get("page").unwrap().uint(),
                    link: PdfLink { x_pt: f(k, "xPt"), y_top_pt: f(k, "yTopPt"), w_pt: f(k, "wPt"), h_pt: f(k, "hPt"), target },
                }
            })
            .collect();
        let outline = n
            .get("outline")
            .unwrap()
            .arr()
            .iter()
            .map(|o| OutlineEntry {
                title: o.get("title").unwrap().str().encode_utf16().collect::<Vec<u16>>().into(),
                level: o.get("level").unwrap().uint(),
                page_index: o.get("pageIndex").unwrap().uint(),
                top_pt: f(o, "topPt"),
            })
            .collect();
        Navigation { links, outline }
    }
}
