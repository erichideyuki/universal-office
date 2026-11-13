//! The adapter check of the F0 harness (record "Milestone 1 — F0", like uo-render R-P2c): the
//! raw P1 stream the TS walker wrote for a frozen-TS layout is decoded back into this crate's
//! `Layout` (uo-render pages and resources + the Write section), and `canon_layout` must write
//! the same stream again. It proves that the Rust types hold every field the walker emits and that
//! `src/canon.rs` follows `tools/lib/canon.mjs` item by item, before any stage is ported.
//!
//! Faces and image payloads come back by SHA-256 from the blob index (the repository's open
//! fonts, the installed fonts a transcript names, the `<key>.media/` payloads of the dump).

use std::sync::Arc;

use uo_docx_layout::keys::{LineSeq, TbKey};
use uo_docx_layout::model::{BlockPath, HfPartId, NoteKind, RunRef, RunSub, StoryRef};
use uo_docx_layout::output::{
    GlyphMeta, HfSlot, HfWhich, Layer, Layout, LayoutPage, LineBox, LineGeom, LineParaFormat, LineRef, LineStory, ObjectFrame, PlacedCell,
    PlacedObject, RefPath, TabAlign, TabLeader, TabStop,
};
use uo_render::types::{
    Blip, BlipTile, ClrChange, CustCmd, CustPath, FxGlow, FxOutline, FxReflection, FxShadow, FxStop, GeomEval, GradFill, GradStop,
    ImageItem, Item, LineCap, LineDash, LineJoin, Ltrb, PageFill, PageFillKind, PathCmd, Pattern, PicBevel, PicGlow, PicLine,
    PicReflection, PicShadow, PictureLook, PresetSpec, ResolvedFill, ResolvedTextFx, RunFlags, ShadowKind, ShapeFx, ShapeGeom, ShapeItem,
    ShapeShadow, Stroke,
};
use uo_render::{FaceId, FaceRes, GlyphRunHead, OptF64, Span, Sym};

use super::FontIndex;

/// The decoded layout and the names of the placed objects' runs (`PlacedObject.run` holds an
/// index into `run_names` in its path).
pub struct Decoded {
    pub layout: Layout,
    pub run_names: Vec<Vec<u16>>,
}

struct R<'b> {
    b: &'b [u8],
    at: usize,
    fonts: &'b FontIndex,
    layout: Layout,
    run_names: Vec<Vec<u16>>,
    faces: u32,
    images: u32,
    /// (F6) the stream's face and image labels when decoding into shared resources
    share: Option<&'b mut Share>,
    face_ids: Vec<FaceId>,
    image_ids: Vec<uo_render::ImageId>,
}

/// Resources shared by many decoded streams (F6: every facet value of a document decodes into
/// one resource table): faces and images deduplicated by their descriptor, `lineBox.line` labels
/// offset per stream so lines of different streams stay distinct.
#[derive(Default)]
pub struct Share {
    pub res: uo_render::Resources,
    faces: std::collections::HashMap<Vec<u8>, FaceId>,
    images: std::collections::HashMap<Vec<u8>, uo_render::ImageId>,
    line_base: u32,
    line_max: u32,
}

type Res<T> = Result<T, String>;

impl R<'_> {
    fn take(&mut self, n: usize) -> Res<&[u8]> {
        let s = self.b.get(self.at..self.at + n).ok_or_else(|| format!("truncated at {}", self.at))?;
        self.at += n;
        Ok(s)
    }
    fn tag(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }
    fn expect(&mut self, t: u8) -> Res<()> {
        let at = self.at;
        let g = self.tag()?;
        if g == t { Ok(()) } else { Err(format!("at {at}: tag {:?}, expected {:?}", g as char, t as char)) }
    }
    fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| "u32")?))
    }
    fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| "u64")?))
    }
    fn num(&mut self) -> Res<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn bool(&mut self) -> Res<bool> {
        Ok(self.tag()? != 0)
    }
    fn bytes(&mut self) -> Res<Vec<u8>> {
        let n = usize::try_from(self.u64()?).map_err(|_| "len")?;
        Ok(self.take(n)?.to_vec())
    }
    fn digest(&mut self) -> Res<(u64, String)> {
        let n = self.u64()?;
        let d = self.take(32)?;
        Ok((n, uo_testkit::stream::hex(d)))
    }
    fn u16s(&mut self) -> Res<Vec<u16>> {
        let b = self.bytes()?;
        Ok(b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
    }
    fn string(&mut self) -> Res<String> {
        Ok(String::from_utf16_lossy(&self.u16s()?))
    }
    fn sym(&mut self) -> Res<Sym> {
        let u = self.u16s()?;
        self.layout.resources.symbols.intern(&u).ok_or_else(|| "symbols".into())
    }
    fn opt_num(&mut self) -> Res<OptF64> {
        Ok(if self.bool()? { OptF64::some(self.num()?) } else { OptF64::NONE })
    }
    fn opt_f(&mut self) -> Res<Option<f64>> {
        Ok(self.opt_num()?.get())
    }
    fn opt_sym(&mut self) -> Res<Option<Sym>> {
        Ok(if self.bool()? { Some(self.sym()?) } else { None })
    }
    fn opt<T>(&mut self, f: impl FnOnce(&mut Self) -> Res<T>) -> Res<Option<T>> {
        if self.bool()? { Ok(Some(f(self)?)) } else { Ok(None) }
    }
    fn ltrb(&mut self) -> Res<Ltrb> {
        Ok(Ltrb { l: self.num()?, t: self.num()?, r: self.num()?, b: self.num()? })
    }

    fn face(&mut self) -> Res<FaceId> {
        if self.share.is_some() {
            return self.face_shared();
        }
        let label = self.u32()?;
        if label < self.faces {
            return FaceId::from_index(label as usize).ok_or_else(|| "face".into());
        }
        if label != self.faces {
            return Err(format!("face label {label} out of order"));
        }
        self.faces += 1;
        let key = self.sym()?;
        let family = self.sym()?;
        let (bold, italic, synth_bold, synth_italic, stand_in) = (self.bool()?, self.bool()?, self.bool()?, self.bool()?, self.bool()?);
        let bytes = match self.opt(|r| r.digest())? {
            Some((_, sha)) => Some(self.fonts.get(&sha).ok_or_else(|| format!("font {sha} not in the index"))?),
            None => None,
        };
        let face = bytes.as_ref().and_then(|b| uo_fonts::Face::parse(b.clone(), &uo_fonts::FontLimits::default()).ok());
        self.layout
            .resources
            .add_face(FaceRes { key, family, bold, italic, synth_bold, synth_italic, stand_in, face, bytes, advances: None })
            .ok_or_else(|| "faces".into())
    }

    /// A face label of a stream decoded into [`Share`]: the descriptor's face if seen before.
    fn face_shared(&mut self) -> Res<FaceId> {
        let label = self.u32()?;
        if let Some(id) = self.face_ids.get(label as usize) {
            return Ok(*id);
        }
        if label as usize != self.face_ids.len() {
            return Err(format!("face label {label} out of order"));
        }
        let at0 = self.at;
        let key = self.sym()?;
        let family = self.sym()?;
        let (bold, italic, synth_bold, synth_italic, stand_in) = (self.bool()?, self.bool()?, self.bool()?, self.bool()?, self.bool()?);
        let digest = self.opt(|r| r.digest())?;
        let desc = self.b.get(at0..self.at).ok_or("face")?.to_vec();
        let known = self.share.as_ref().and_then(|s| s.faces.get(&desc).copied());
        let id = match known {
            Some(id) => id,
            None => {
                let bytes = match digest {
                    Some((_, sha)) => Some(self.fonts.get(&sha).ok_or_else(|| format!("font {sha} not in the index"))?),
                    None => None,
                };
                let face = bytes.as_ref().and_then(|b| uo_fonts::Face::parse(b.clone(), &uo_fonts::FontLimits::default()).ok());
                let id = self
                    .layout
                    .resources
                    .add_face(FaceRes { key, family, bold, italic, synth_bold, synth_italic, stand_in, face, bytes, advances: None })
                    .ok_or("faces")?;
                if let Some(s) = self.share.as_mut() {
                    s.faces.insert(desc, id);
                }
                id
            }
        };
        self.face_ids.push(id);
        Ok(id)
    }

    /// An image label of a stream decoded into [`Share`].
    fn image_shared(&mut self) -> Res<uo_render::ImageId> {
        let label = self.u32()?;
        if let Some(id) = self.image_ids.get(label as usize) {
            return Ok(*id);
        }
        if label as usize != self.image_ids.len() {
            return Err(format!("image label {label} out of order"));
        }
        let at0 = self.at;
        let ext = self.sym()?;
        let (_, sha) = self.digest()?;
        let svg = self.opt(|r| r.digest())?;
        let (width_twips, height_twips) = (self.num()?, self.num()?);
        let effects = self.opt(|r| {
            Ok(uo_drawingml::ImageEffects { gray: r.bool()?, gain: r.opt_f()?, bright: r.opt_f()?, floor: r.bool()?, bi_level: r.opt_f()? })
        })?;
        let desc = self.b.get(at0..self.at).ok_or("image")?.to_vec();
        let known = self.share.as_ref().and_then(|s| s.images.get(&desc).copied());
        let id = match known {
            Some(id) => id,
            None => {
                let bytes = self.fonts.get(&sha).ok_or_else(|| format!("image payload {sha} not dumped"))?;
                let svg = match svg {
                    Some((_, h)) => Some(self.fonts.get(&h).ok_or_else(|| format!("svg payload {h} not dumped"))?),
                    None => None,
                };
                let id = self
                    .layout
                    .resources
                    .add_image(uo_render::ImageRes { bytes, ext, svg, effects, width_twips, height_twips, natural_px: None })
                    .ok_or("images")?;
                if let Some(s) = self.share.as_mut() {
                    s.images.insert(desc, id);
                }
                id
            }
        };
        self.image_ids.push(id);
        Ok(id)
    }

    fn image(&mut self) -> Res<uo_render::ImageId> {
        if self.share.is_some() {
            return self.image_shared();
        }
        let label = self.u32()?;
        if label < self.images {
            return uo_render::ImageId::from_index(label as usize).ok_or_else(|| "image".into());
        }
        if label != self.images {
            return Err(format!("image label {label} out of order"));
        }
        self.images += 1;
        let ext = self.sym()?;
        let (_, sha) = self.digest()?;
        let bytes = self.fonts.get(&sha).ok_or_else(|| format!("image payload {sha} not dumped"))?;
        let svg = match self.opt(|r| r.digest())? {
            Some((_, h)) => Some(self.fonts.get(&h).ok_or_else(|| format!("svg payload {h} not dumped"))?),
            None => None,
        };
        let (width_twips, height_twips) = (self.num()?, self.num()?);
        let effects = self.opt(|r| {
            Ok(uo_drawingml::ImageEffects { gray: r.bool()?, gain: r.opt_f()?, bright: r.opt_f()?, floor: r.bool()?, bi_level: r.opt_f()? })
        })?;
        self.layout
            .resources
            .add_image(uo_render::ImageRes { bytes, ext, svg, effects, width_twips, height_twips, natural_px: None })
            .ok_or_else(|| "images".into())
    }

    fn adj(&mut self) -> Res<Vec<(Sym, f64)>> {
        let n = self.u32()?;
        (0..n).map(|_| Ok((self.sym()?, self.num()?))).collect()
    }

    fn fill(&mut self) -> Res<ResolvedFill> {
        Ok(match self.tag()? {
            b'n' => ResolvedFill::None,
            b's' => ResolvedFill::Solid { hex: self.sym()?, alpha: self.num()? },
            b'g' => {
                let n = self.u32()?;
                let stops =
                    (0..n).map(|_| Ok(FxStop { pos: self.num()?, hex: self.sym()?, alpha: self.num()? })).collect::<Res<Vec<_>>>()?;
                ResolvedFill::Grad {
                    stops,
                    angle_deg: self.num()?,
                    scaled: self.bool()?,
                    path: self.opt_sym()?,
                    fill_to: self.opt(|r| r.ltrb())?,
                }
            }
            t => return Err(format!("fill tag {t}")),
        })
    }

    fn text_fx(&mut self) -> Res<ResolvedTextFx> {
        let fill = self.opt(|r| r.fill())?;
        let outline = self.opt(|r| {
            let width_pt = r.num()?;
            let fill = r.fill()?;
            let dash = r.opt_sym()?;
            let cap = match r.string()?.as_str() {
                "round" => LineCap::Round,
                "square" => LineCap::Square,
                _ => LineCap::Butt,
            };
            let join = match r.string()?.as_str() {
                "round" => LineJoin::Round,
                "bevel" => LineJoin::Bevel,
                _ => LineJoin::Miter,
            };
            Ok(FxOutline { width_pt, fill, dash, cap, join })
        })?;
        let shadow = self.opt(|r| {
            Ok(FxShadow {
                blur_pt: r.num()?,
                dist_pt: r.num()?,
                dir_deg: r.num()?,
                sx: r.num()?,
                sy: r.num()?,
                kx_deg: r.num()?,
                ky_deg: r.num()?,
                algn: r.sym()?,
                hex: r.sym()?,
                alpha: r.num()?,
            })
        })?;
        let glow = self.opt(|r| Ok(FxGlow { rad_pt: r.num()?, hex: r.sym()?, alpha: r.num()? }))?;
        let reflection = self.opt(|r| {
            Ok(FxReflection {
                blur_pt: r.num()?,
                st_a: r.num()?,
                st_pos: r.num()?,
                end_a: r.num()?,
                end_pos: r.num()?,
                dist_pt: r.num()?,
                dir_deg: r.num()?,
                algn: r.sym()?,
            })
        })?;
        Ok(ResolvedTextFx { fill, outline, shadow, glow, reflection })
    }

    fn shadow(&mut self) -> Res<PicShadow> {
        let kind = if self.string()? == "inner" { ShadowKind::Inner } else { ShadowKind::Outer };
        Ok(PicShadow {
            kind,
            color: self.sym()?,
            alpha: self.num()?,
            blur_pt: self.num()?,
            dist_pt: self.num()?,
            dir_deg: self.num()?,
            sx: self.opt_num()?,
            sy: self.opt_num()?,
            kx_deg: self.opt_num()?,
            ky_deg: self.opt_num()?,
            algn: self.opt_sym()?,
            rot_with_shape: self.bool()?,
        })
    }

    fn glow(&mut self) -> Res<PicGlow> {
        Ok(PicGlow { color: self.sym()?, alpha: self.num()?, rad_pt: self.num()? })
    }

    fn reflection(&mut self) -> Res<PicReflection> {
        Ok(PicReflection {
            blur_pt: self.num()?,
            st_a: self.num()?,
            st_pos: self.num()?,
            end_a: self.num()?,
            end_pos: self.num()?,
            dist_pt: self.num()?,
            dir_deg: self.num()?,
            sy: self.num()?,
            algn: self.opt_sym()?,
            rot_with_shape: self.bool()?,
        })
    }

    fn pic(&mut self) -> Res<PictureLook> {
        Ok(PictureLook {
            geom: self.opt(|r| Ok(PresetSpec { prst: r.sym()?, adj: r.opt(|r| r.adj())? }))?,
            line: self.opt(|r| {
                Ok(PicLine {
                    color: r.sym()?,
                    alpha: r.num()?,
                    width_pt: r.num()?,
                    dash: r.opt_sym()?,
                    cmpd: r.opt_sym()?,
                    cap: r.opt_sym()?,
                    join: r.opt_sym()?,
                })
            })?,
            shadow: self.opt(|r| r.shadow())?,
            glow: self.opt(|r| r.glow())?,
            soft_edge_pt: self.opt_num()?,
            bevel: self.opt(|r| {
                Ok(PicBevel {
                    w_pt: r.num()?,
                    h_pt: r.num()?,
                    prst: r.opt_sym()?,
                    rig: r.opt_sym()?,
                    dir: r.opt_sym()?,
                    material: r.opt_sym()?,
                })
            })?,
            reflection: self.opt(|r| r.reflection())?,
            alpha: self.opt_num()?,
            duotone: self.opt(|r| Ok([r.sym()?, r.sym()?]))?,
            clr_change: self.opt(|r| Ok(ClrChange { from: r.sym()?, to: r.sym()?, to_alpha: r.num()? }))?,
        })
    }

    fn stroke(&mut self) -> Res<Stroke> {
        Ok(Stroke { color: self.sym()?, width_pt: self.num()?, dash: self.opt_sym()? })
    }

    fn cust(&mut self) -> Res<uo_render::CustId> {
        let n = self.u32()?;
        let mut paths = Vec::new();
        for _ in 0..n {
            let (w, h, fill, stroke) = (self.num()?, self.num()?, self.bool()?, self.bool()?);
            let k = self.u32()?;
            let mut cmds = Vec::new();
            for _ in 0..k {
                cmds.push(match self.tag()? {
                    b'M' => CustCmd::M { x: self.num()?, y: self.num()? },
                    b'L' => CustCmd::L { x: self.num()?, y: self.num()? },
                    t @ (b'C' | b'Q') => {
                        let m = self.u32()?;
                        let pts = (0..m).map(|_| Ok([self.num()?, self.num()?])).collect::<Res<Vec<_>>>()?;
                        if t == b'C' { CustCmd::C { pts } } else { CustCmd::Q { pts } }
                    }
                    b'A' => CustCmd::A { w_r: self.num()?, h_r: self.num()?, st: self.num()?, sw: self.num()? },
                    b'Z' => CustCmd::Z,
                    t => return Err(format!("cust tag {t}")),
                });
            }
            paths.push(CustPath { w, h, fill, stroke, cmds });
        }
        self.layout.resources.add_cust(Arc::from(paths)).ok_or_else(|| "cust".into())
    }

    fn shape(&mut self) -> Res<ShapeItem> {
        let (x, y, w, h) = (self.num()?, self.num()?, self.num()?, self.num()?);
        let g = self.sym()?;
        let adj = self.opt(|r| r.adj())?;
        let cust = self.opt(|r| r.cust())?;
        let geom = ShapeGeom {
            geom: g,
            adj,
            cust,
            flip_h: self.bool()?,
            flip_v: self.bool()?,
            rot: self.opt_num()?,
            fill_alpha: self.opt_num()?,
            line_alpha: self.opt_num()?,
            eval: GeomEval::Compact,
        };
        Ok(ShapeItem {
            x,
            y,
            w,
            h,
            geom,
            fill: self.opt_sym()?,
            stroke: self.opt(|r| r.stroke())?,
            line: self.opt(|r| Ok([r.num()?, r.num()?, r.num()?, r.num()?]))?,
            grad: self.opt(|r| {
                let n = r.u32()?;
                let stops = (0..n).map(|_| Ok(GradStop { pos: r.num()?, color: r.sym()? })).collect::<Res<Vec<_>>>()?;
                Ok(GradFill {
                    stops,
                    angle_deg: r.num()?,
                    scaled: r.bool()?,
                    rot_with_shape: r.bool()?,
                    path: r.opt_sym()?,
                    fill_to: r.opt(|r| r.ltrb())?,
                })
            })?,
            shadow: self
                .opt(|r| Ok(ShapeShadow { dx: r.num()?, dy: r.num()?, color: r.sym()?, alpha: r.opt_num()?, blur: r.opt_num()? }))?,
            pattern: self.opt(|r| Ok(Pattern { prst: r.sym()?, fg: r.sym()?, bg: r.sym()? }))?,
            blip: self.opt(|r| {
                let image = r.image()?;
                let tile =
                    r.opt(|r| Ok(BlipTile { tx: r.num()?, ty: r.num()?, sx: r.num()?, sy: r.num()?, algn: r.sym()?, flip: r.sym()? }))?;
                Ok(Blip { image, tile, alpha: r.opt_num()? })
            })?,
            fx: self.opt(|r| {
                Ok(ShapeFx {
                    shadow: r.opt(|r| r.shadow())?,
                    glow: r.opt(|r| r.glow())?,
                    soft_edge_pt: r.opt_num()?,
                    reflection: r.opt(|r| r.reflection())?,
                })
            })?,
        })
    }

    fn line_ref(s: &str) -> Res<LineRef> {
        let (story, rest) = if let Some(r) = s.strip_prefix("txbx") {
            (LineStory::Textbox(TbKey(0)), r)
        } else if let Some(r) = s.strip_prefix("preview") {
            (LineStory::Preview, r)
        } else if let Some(r) = s.strip_prefix("hdr:").or_else(|| s.strip_prefix("ftr:")) {
            let which = if s.starts_with("hdr:") { HfWhich::Header } else { HfWhich::Footer };
            let (slot, r) = r.split_once('/').map_or((r, ""), |(a, b)| (a, b));
            let slot = match slot {
                "first" => HfSlot::First,
                "even" => HfSlot::Even,
                _ => HfSlot::Default,
            };
            return Ok(LineRef { story: LineStory::HeaderFooter { which, slot, part: HfPartId(0) }, path: R::path(r)? });
        } else if let Some(r) = s.strip_prefix("fn").or_else(|| s.strip_prefix("en")) {
            let kind = if s.starts_with("fn") { NoteKind::Footnote } else { NoteKind::Endnote };
            let (id, r) = r.split_once('/').map_or((r, ""), |(a, b)| (a, b));
            let id = uo_jsnum::parse::to_number(id);
            return Ok(LineRef { story: LineStory::Note(kind, uo_docx_layout::model::NumKey::new(id)), path: R::path(r)? });
        } else {
            (LineStory::Body, s)
        };
        Ok(LineRef { story, path: R::path(rest.trim_start_matches('/'))? })
    }

    fn path(s: &str) -> Res<RefPath> {
        if s.is_empty() {
            return Ok(RefPath::default());
        }
        let segs = s
            .split('/')
            .map(|g| g.get(1..).and_then(|n| n.parse().ok()).ok_or_else(|| format!("ref segment {g}")))
            .collect::<Res<Vec<u32>>>()?;
        Ok(RefPath(segs.into()))
    }

    fn line(&mut self, p: &mut LayoutPage) -> Res<LineGeom> {
        self.expect(b'n')?;
        let at = R::line_ref(&self.string()?)?;
        let (start, end) = (self.num()?, self.num()?);
        let (x, y, height, baseline, width) = (self.num()?, self.num()?, self.num()?, self.num()?, self.num()?);
        let chars = self.opt(|r| {
            let n = r.u32()?;
            let s0 = u32::try_from(p.chars.o.len()).map_err(|_| "chars")?;
            for _ in 0..n {
                let (o, x, w) = (r.num()?, r.num()?, r.num()?);
                p.chars.o.push(int(o)?);
                p.chars.x.push(x);
                p.chars.w.push(w);
            }
            Ok(Span { start: s0, len: n })
        })?;
        let hyph = self.opt_f()?.map(int).transpose()?;
        let rev = self.opt(|r| Ok((r.num()?, r.num()?)))?;
        let frame = self.opt(|r| Ok((r.num()?, r.num()?)))?;
        let pfmt = self.opt(|r| {
            let (left, right, first_line) = (r.num()?, r.num()?, r.num()?);
            let n = r.u32()?;
            let tabs = (0..n)
                .map(|_| {
                    let val = match r.string()?.as_str() {
                        "left" => TabAlign::Left,
                        "center" => TabAlign::Center,
                        "right" => TabAlign::Right,
                        "decimal" => TabAlign::Decimal,
                        "bar" => TabAlign::Bar,
                        "clear" => TabAlign::Clear,
                        "num" => TabAlign::Num,
                        v => return Err(format!("tab {v}")),
                    };
                    let pos = r.num()?;
                    let leader = r.opt(|r| {
                        Ok(match r.string()?.as_str() {
                            "none" => TabLeader::None,
                            "dot" => TabLeader::Dot,
                            "hyphen" => TabLeader::Hyphen,
                            "underscore" => TabLeader::Underscore,
                            "heavy" => TabLeader::Heavy,
                            "middleDot" => TabLeader::MiddleDot,
                            v => return Err(format!("leader {v}")),
                        })
                    })?;
                    Ok(TabStop { val, pos, leader })
                })
                .collect::<Res<Vec<_>>>()?;
            p.pfmts.push(LineParaFormat { left, right, first_line, tabs: tabs.into() });
            u32::try_from(p.pfmts.len() - 1).map_err(|_| "pfmts".to_string())
        })?;
        Ok(LineGeom { at, start: int(start)?, end: int(end)?, x, y, height, baseline, desc: 0.0, width, chars, hyph, rev, frame, pfmt })
    }

    fn item(&mut self, p: &mut LayoutPage) -> Res<()> {
        let i = u32::try_from(p.page.list.items.len()).map_err(|_| "items")?;
        match self.tag()? {
            b'g' => {
                let face = self.face()?;
                let size_pt = self.num()?;
                let color = self.sym()?;
                let y = self.num()?;
                let n = self.u32()?;
                let glyphs =
                    (0..n).map(|_| Ok((int(self.num()?)?, self.num()?, self.opt_f()?.map(int).transpose()?))).collect::<Res<Vec<_>>>()?;
                let scale_x = self.opt_num()?;
                let text = self.u16s()?;
                let (sb, si) = (self.bool()?, self.bool()?);
                let alpha = self.opt_num()?;
                let (as_path, colr) = (self.bool()?, self.bool()?);
                let fx = match self.opt(|r| r.text_fx())? {
                    Some(fx) => Some(self.layout.resources.add_text_fx(fx).ok_or("fx")?),
                    None => None,
                };
                let line_box = self.opt(|r| Ok(LineBox { asc: r.num()?, desc: r.num()?, lead: r.num()?, line: r.line_seq()? }))?;
                let line_num = self.bool()?;
                let flags = RunFlags::default()
                    .with(RunFlags::SYNTH_BOLD, sb)
                    .with(RunFlags::SYNTH_ITALIC, si)
                    .with(RunFlags::AS_PATH, as_path)
                    .with(RunFlags::COLR, colr);
                p.page
                    .list
                    .push_glyphs(GlyphRunHead { face, color, size_pt, y, text: &text, scale_x, alpha, fx, flags }, glyphs)
                    .map_err(|e| e.to_string())?;
                if line_box.is_some() || line_num {
                    p.item_meta.push((i, GlyphMeta { line_box, line_num }));
                }
            }
            b'r' => {
                let it = Item::Rect {
                    x: self.num()?,
                    y: self.num()?,
                    w: self.num()?,
                    h: self.num()?,
                    fill: self.sym()?,
                    alpha: self.opt_num()?,
                };
                p.page.list.push_plain(it);
            }
            b'l' => {
                let (x1, y1, x2, y2) = (self.num()?, self.num()?, self.num()?, self.num()?);
                let (color, width_pt) = (self.sym()?, self.num()?);
                let dash = self.opt(|r| r.string())?.map(|d| LineDash::from_name(&d).ok_or(format!("dash {d}"))).transpose()?;
                p.page.list.push_plain(Item::Line { x1, y1, x2, y2, color, width_pt, dash });
            }
            b'i' => {
                let (x, y, w, h) = (self.num()?, self.num()?, self.num()?, self.num()?);
                let image = self.image()?;
                let crop = self.opt(|r| r.ltrb())?;
                let rot = self.opt_num()?;
                let (flip_h, flip_v) = (self.bool()?, self.bool()?);
                let pic = self.opt(|r| r.pic())?.map(Box::new);
                p.page.list.push_plain(Item::Image(Box::new(ImageItem { x, y, w, h, image, crop, rot, flip_h, flip_v, pic })));
            }
            b's' => {
                let sh = self.shape()?;
                p.page.list.push_plain(Item::Shape(Box::new(sh)));
            }
            b'p' => {
                let n = self.u32()?;
                let cmds = (0..n)
                    .map(|_| {
                        Ok(match self.tag()? {
                            b'M' => PathCmd::M { x: self.num()?, y: self.num()? },
                            b'L' => PathCmd::L { x: self.num()?, y: self.num()? },
                            b'C' => PathCmd::C {
                                x1: self.num()?,
                                y1: self.num()?,
                                x2: self.num()?,
                                y2: self.num()?,
                                x: self.num()?,
                                y: self.num()?,
                            },
                            b'Z' => PathCmd::Z,
                            t => return Err(format!("cmd {t}")),
                        })
                    })
                    .collect::<Res<Vec<_>>>()?;
                let fill = self.opt_sym()?;
                let stroke = self.opt(|r| r.stroke())?;
                let even_odd = self.bool()?;
                p.page.list.push_path(&cmds, fill, stroke, even_odd).map_err(|e| e.to_string())?;
            }
            b'c' => {
                let it = if self.bool()? {
                    Item::ClipPush { x: self.opt_num()?, y: self.opt_num()?, w: self.opt_num()?, h: self.opt_num()? }
                } else {
                    Item::ClipPop
                };
                p.page.list.push_plain(it);
                if let Some(src) = self.opt(|r| r.image())? {
                    p.clip_src.push((i, src));
                }
            }
            b'x' => {
                let push = self.bool()?;
                let m = self.opt(|r| Ok(Box::new([r.num()?, r.num()?, r.num()?, r.num()?, r.num()?, r.num()?])))?;
                p.page.list.push_plain(if push { Item::XformPush { m } } else { Item::XformPop });
            }
            t => return Err(format!("item tag {:?}", t as char)),
        }
        Ok(())
    }

    fn page(&mut self) -> Res<LayoutPage> {
        self.expect(b'P')?;
        let mut p = LayoutPage { ..LayoutPage::default() };
        p.page.w = self.num()?;
        p.page.h = self.num()?;
        p.section = int(self.num()?)?;
        p.number = self.num()?;
        p.hf_items = self.opt_f()?.map(int).transpose()?;
        p.page.background = self.opt_sym()?;
        p.page.background_show = self.bool()?;
        if let Some(fill) = self.opt(|r| r.page_fill())? {
            p.page.background_fill = Some(self.layout.resources.add_page_fill(fill).ok_or("fills")?);
        }
        for _ in 0..self.u32()? {
            self.item(&mut p)?;
        }
        for _ in 0..self.u32()? {
            let l = self.line(&mut p)?;
            p.lines.push(l);
        }
        if self.bool()? {
            let mut os = Vec::new();
            for _ in 0..self.u32()? {
                os.push(self.object(&mut p)?);
            }
            p.objects = Some(os);
        }
        if self.bool()? {
            let mut cs = Vec::new();
            for _ in 0..self.u32()? {
                self.expect(b'k')?;
                let at = R::line_ref(&self.string()?)?;
                cs.push(PlacedCell { at, x: self.num()?, y: self.num()?, w: self.num()?, h: self.num()?, span: self.num()? });
            }
            p.cells = Some(cs);
        }
        Ok(p)
    }

    fn object(&mut self, p: &mut LayoutPage) -> Res<PlacedObject> {
        self.expect(b'o')?;
        let run = match self.opt(|r| r.u16s())? {
            Some(name) => {
                self.run_names.push(name);
                Some(RunRef {
                    story: StoryRef::Body,
                    path: BlockPath::root(u32::try_from(self.run_names.len() - 1).map_err(|_| "runs")?),
                    run: u32::MAX,
                    sub: RunSub::None,
                })
            }
            None => None,
        };
        let (x, y, w, h) = (self.num()?, self.num()?, self.num()?, self.num()?);
        let layer = if self.string()? == "behind" { Layer::Behind } else { Layer::Front };
        let hf = self.bool()?;
        let frame = self.opt(|r| {
            Ok(ObjectFrame {
                col_x: r.num()?,
                col_w: r.num()?,
                para_top: r.num()?,
                page_w: r.num()?,
                page_h: r.num()?,
                mar_l: r.num()?,
                mar_r: r.num()?,
                mar_t: r.num()?,
                mar_b: r.num()?,
                body_top: r.num()?,
            })
        })?;
        let content = self.opt(|r| {
            let n = r.u32()?;
            let s0 = u32::try_from(p.content_lines.len()).map_err(|_| "lines")?;
            for _ in 0..n {
                let l = r.line(p)?;
                p.content_lines.push(l);
            }
            let xf = [r.num()?, r.num()?, r.num()?, r.num()?, r.num()?, r.num()?];
            Ok((s0..s0 + n, xf))
        })?;
        Ok(PlacedObject { run, drawing: None, x, y, w, h, layer, hf, frame, content })
    }

    fn page_fill(&mut self) -> Res<PageFill> {
        let kind = match self.string()?.as_str() {
            "gradient" => PageFillKind::Gradient,
            "gradientRadial" => PageFillKind::GradientRadial,
            "pattern" => PageFillKind::Pattern,
            "frame" => PageFillKind::Frame,
            "tile" => PageFillKind::Tile,
            k => return Err(format!("page fill {k}")),
        };
        let (c1, c2) = (self.sym()?, self.sym()?);
        let stops = self.opt(|r| {
            let n = r.u32()?;
            (0..n).map(|_| Ok(GradStop { pos: r.num()?, color: r.sym()? })).collect::<Res<Vec<_>>>()
        })?;
        Ok(PageFill {
            kind,
            c1,
            c2,
            stops,
            angle: self.num()?,
            focus: self.num()?,
            focus_position: self.opt(|r| Ok([r.num()?, r.num()?]))?,
            gradient_center: self.bool()?,
            image: self.opt(|r| r.image())?,
            image_px: self.opt(|r| Ok([r.num()?, r.num()?, r.num()?]))?,
            pattern: self.opt(|r| {
                let n = r.u32()?;
                (0..n).map(|_| r.num()).collect::<Res<Vec<_>>>()
            })?,
        })
    }
}

impl R<'_> {
    /// `lineBox.line`: the stream's label, offset into [`Share`]'s range when shared.
    fn line_seq(&mut self) -> Res<LineSeq> {
        let l = self.u32()?;
        Ok(match self.share.as_mut() {
            Some(s) => {
                s.line_max = s.line_max.max(l.saturating_add(1));
                LineSeq(s.line_base.saturating_add(l))
            }
            None => LineSeq(l),
        })
    }
}

fn int(v: f64) -> Res<u32> {
    if v >= 0.0 && v <= f64::from(u32::MAX) && v.fract() == 0.0 { Ok(v as u32) } else { Err(format!("not a u32: {v}")) }
}

/// Decodes a raw P1 stream (`<key>.p1.bin`) into a `Layout`.
pub fn decode_p1(bytes: &[u8], fonts: &FontIndex) -> Result<Decoded, String> {
    let mut r = R {
        b: bytes,
        at: 0,
        fonts,
        layout: Layout::default(),
        run_names: Vec::new(),
        faces: 0,
        images: 0,
        share: None,
        face_ids: Vec::new(),
        image_ids: Vec::new(),
    };
    r.expect(b'L')?;
    let v = r.u32()?;
    if v != uo_docx_layout::canon::CANON_VERSION {
        return Err(format!("canon version {v}"));
    }
    for _ in 0..r.u32()? {
        let p = r.page()?;
        r.layout.pages.push(p);
    }
    for _ in 0..r.u32()? {
        let w = r.string()?;
        r.layout.warnings.insert(w.into());
    }
    if r.at != bytes.len() {
        return Err(format!("{} bytes left", bytes.len() - r.at));
    }
    Ok(Decoded { layout: r.layout, run_names: r.run_names })
}

/// Decodes a raw P1 stream into pages whose faces, images and symbols live in `share.res` (F6: the
/// facet values of one document). The returned layout's own resources are empty.
pub fn decode_p1_shared(bytes: &[u8], fonts: &FontIndex, share: &mut Share) -> Result<Decoded, String> {
    let res = std::mem::take(&mut share.res);
    share.line_max = 0;
    let layout = Layout { resources: res, ..Layout::default() };
    let mut r = R {
        b: bytes,
        at: 0,
        fonts,
        layout,
        run_names: Vec::new(),
        faces: 0,
        images: 0,
        share: Some(share),
        face_ids: Vec::new(),
        image_ids: Vec::new(),
    };
    let out = decode_body(&mut r, bytes.len());
    let mut layout = r.layout;
    let run_names = r.run_names;
    let share = r.share.take().ok_or("share")?;
    share.res = std::mem::take(&mut layout.resources);
    share.line_base = share.line_base.saturating_add(share.line_max);
    out?;
    Ok(Decoded { layout, run_names })
}

fn decode_body(r: &mut R<'_>, len: usize) -> Result<(), String> {
    r.expect(b'L')?;
    let v = r.u32()?;
    if v != uo_docx_layout::canon::CANON_VERSION {
        return Err(format!("canon version {v}"));
    }
    for _ in 0..r.u32()? {
        let p = r.page()?;
        r.layout.pages.push(p);
    }
    for _ in 0..r.u32()? {
        let w = r.string()?;
        r.layout.warnings.insert(w.into());
    }
    if r.at != len {
        return Err(format!("{} bytes left", len - r.at));
    }
    Ok(())
}
