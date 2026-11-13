//! The XPS writer of the display list (milestone 2, wave 7, frontier A1, decision D-A1-2: "XPS from the display list",
//! written in Rust; record `core/docs/modules/uo-bindings.md` "Wave 7 — A1"). The same pages and resources the PDF
//! painter reads ([`crate::render_pdf`]) become an XPS package (OPC zip): one `FixedPage` per page, the faces' bytes as
//! font parts and the pictures as image parts. The package layout is the editor's former TS writer's (`xps.ts`: the
//! `FixedDocumentSequence`, `/Documents/1/Pages/<n>.fpage`, per-page relationships to the fonts and images it uses).
//!
//! What a page holds (XPS units: 1/96 inch; twips / 15):
//! - glyph runs: `Glyphs` with the face embedded, each glyph's index in that face and its advance as the layout placed
//!   it (`Indices`), the run's colour, `Opacity` (its alpha), the synthetic bold/italic (`StyleSimulations`) and the
//!   character scale (`RenderTransform`); a run of white space only, or with a face whose bytes the layout does not
//!   have, is not written;
//! - rectangles (a 6-digit hex fill only, as the canvas painter), lines (with their dashes), paths (fill and stroke),
//!   shapes (the preset geometry of `uo_render::geom`, solid or gradient fill, outline, the outer shadow, a picture fill
//!   stretched over the frame), pictures (`ImageBrush`: crop, rotation, flips, the colour adjustments of the pixels),
//!   the page colour and the page fill (gradient, pattern, texture: a tiled `ImageBrush`);
//! - transforms and clips as nested `Canvas` elements (`RenderTransform`, `Clip`), alphas as `Opacity`.
//!
//! Not written (the XPS has no counterpart the editor needs, or they are rasters the PDF makes): the text effects (w14:
//! the run is written plain), WordArt's outlined text (written as glyphs), the shape and picture effects (glow, soft
//! edges, bevel, reflection), a picture's preset outline and frame, pattern fills of shapes, images the decoders do not
//! read (EMF/WMF/SVG without a raster).

use std::fmt::Write as _;

use rustc_hash::FxHashMap;
use uo_drawingml::{apply_image_effects, apply_picture_adjust};
use uo_image::{ImageLimits, decode_raster, jpeg_info};
use uo_jsnum::conv::{js_index, truthy};
use uo_jsnum::math;
use uo_jsnum::text::is_js_space_unit;
use uo_opc::zip::{WriteOptions, ZipWriter};
use uo_render::geom::{self, LineUnit};
use uo_render::paint::fill::{GradBox, gradient_geometry};
use uo_render::paint::page_fill::{gradient_tile_rgba, page_fill_tile_tw, pattern_rgba};
use uo_render::paint::picture::AdjustParts;
use uo_render::paint::{GradGeom, PaintLimits};
use uo_render::types::{ImageItem, Item, Ltrb, PageFill, PageFillKind, PathCmd as RPath, RunFlags, ShapeItem, Stroke as RStroke};
use uo_render::{FaceId, ImageId, ItemList, NO_GID, OptF64, Page, Resources, Sym};

use crate::color::hex_color_units;
use crate::error::{PdfError, Result};
use crate::num::fmt;
use crate::render::is_hex6;

/// Twips per XPS unit (1/96 inch).
const TW: f64 = 15.0;
/// XPS units per point.
const PT: f64 = 96.0 / 72.0;

/// What an XPS render takes besides the pages.
#[derive(Clone, Debug)]
pub struct XpsOptions {
    /// the limits of the raster helpers (page-fill tiles)
    pub paint: PaintLimits,
    /// the limits of the image decoders
    pub image: ImageLimits,
    /// the `xml:lang` of the pages
    pub lang: String,
}

impl Default for XpsOptions {
    fn default() -> Self {
        XpsOptions { paint: PaintLimits::DEFAULT, image: ImageLimits::DEFAULT, lang: "pt-BR".to_owned() }
    }
}

/// The XPS package of the pages.
///
/// # Errors
/// [`PdfError::Invalid`] when the zip writer refuses an entry (a part over the zip limits).
pub fn render_xps(pages: &[Page], res: &Resources, o: &XpsOptions) -> Result<Vec<u8>> {
    let mut w = Xps {
        res,
        opts: o,
        fonts: FxHashMap::default(),
        font_parts: Vec::new(),
        images: FxHashMap::default(),
        image_parts: Vec::new(),
        rasters: FxHashMap::default(),
    };
    let mut fpages: Vec<(String, Vec<String>)> = Vec::with_capacity(pages.len());
    for lp in pages {
        fpages.push(w.page(lp));
    }
    let mut z = ZipWriter::new(WriteOptions::default());
    let add = |z: &mut ZipWriter, name: &str, data: &[u8]| z.add(name, data).map_err(|_| PdfError::Invalid("an XPS part the zip refuses"));
    add(&mut z, "[Content_Types].xml", content_types(fpages.len(), !w.image_parts.is_empty()).as_bytes())?;
    add(&mut z, "_rels/.rels", ROOT_RELS.as_bytes())?;
    add(&mut z, "FixedDocumentSequence.fdseq", FDSEQ.as_bytes())?;
    let mut fdoc =
        String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?><FixedDocument xmlns=\"http://schemas.microsoft.com/xps/2005/06\">");
    for i in 1..=fpages.len() {
        let _ = write!(fdoc, "<PageContent Source=\"/Documents/1/Pages/{i}.fpage\"/>");
    }
    fdoc.push_str("</FixedDocument>");
    add(&mut z, "Documents/1/FixedDocument.fdoc", fdoc.as_bytes())?;
    for (i, (xml, used)) in fpages.iter().enumerate() {
        let n = i + 1;
        add(&mut z, &format!("Documents/1/Pages/{n}.fpage"), xml.as_bytes())?;
        let mut rels = String::from(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        );
        for (k, target) in used.iter().enumerate() {
            let _ = write!(
                rels,
                "<Relationship Id=\"R{}\" Type=\"http://schemas.microsoft.com/xps/2005/06/required-resource\" Target=\"{target}\"/>",
                k + 1
            );
        }
        rels.push_str("</Relationships>");
        add(&mut z, &format!("Documents/1/Pages/_rels/{n}.fpage.rels"), rels.as_bytes())?;
    }
    for (name, bytes) in &w.font_parts {
        add(&mut z, name.trim_start_matches('/'), bytes)?;
    }
    for (name, bytes) in &w.image_parts {
        add(&mut z, name.trim_start_matches('/'), bytes)?;
    }
    Ok(z.finish())
}

const FDSEQ: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?><FixedDocumentSequence xmlns=\"http://schemas.microsoft.com/xps/2005/06\"><DocumentReference Source=\"/Documents/1/FixedDocument.fdoc\"/></FixedDocumentSequence>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.microsoft.com/xps/2005/06/fixedrepresentation\" Target=\"/FixedDocumentSequence.fdseq\"/></Relationships>";

fn content_types(pages: usize, images: bool) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"fdseq\" ContentType=\"application/vnd.ms-package.xps-fixeddocumentsequence+xml\"/>\
<Default Extension=\"fdoc\" ContentType=\"application/vnd.ms-package.xps-fixeddocument+xml\"/>\
<Default Extension=\"fpage\" ContentType=\"application/vnd.ms-package.xps-fixedpage+xml\"/>\
<Default Extension=\"ttf\" ContentType=\"application/vnd.ms-opentype\"/>",
    );
    if images {
        s.push_str("<Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>");
    }
    s.push_str(
        "<Override PartName=\"/FixedDocumentSequence.fdseq\" ContentType=\"application/vnd.ms-package.xps-fixeddocumentsequence+xml\"/>\
<Override PartName=\"/Documents/1/FixedDocument.fdoc\" ContentType=\"application/vnd.ms-package.xps-fixeddocument+xml\"/>",
    );
    for i in 1..=pages {
        let _ =
            write!(s, "<Override PartName=\"/Documents/1/Pages/{i}.fpage\" ContentType=\"application/vnd.ms-package.xps-fixedpage+xml\"/>");
    }
    s.push_str("</Types>");
    s
}

/// An image part: its name and its size in XPS units at the image's own resolution (the `Viewbox`).
#[derive(Clone)]
struct ImagePart {
    name: String,
    w: f64,
    h: f64,
}

/// The state of one render: the parts made so far, by face key and by image.
struct Xps<'r> {
    res: &'r Resources,
    opts: &'r XpsOptions,
    /// face key → font part name (`None`: no bytes)
    fonts: FxHashMap<Sym, Option<String>>,
    font_parts: Vec<(String, std::sync::Arc<[u8]>)>,
    /// (image, the adjustment key) → image part (`None`: undecodable)
    images: FxHashMap<(ImageId, String), Option<ImagePart>>,
    image_parts: Vec<(String, Vec<u8>)>,
    /// the page-fill tiles, by their key
    rasters: FxHashMap<String, ImagePart>,
}

/// A page being written: the XAML and the parts it uses.
struct PageOut {
    xml: String,
    used: Vec<String>,
    /// the `Canvas` elements open (transforms and clips)
    depth: usize,
}

impl PageOut {
    fn use_part(&mut self, name: &str) {
        if !self.used.iter().any(|u| u == name) {
            self.used.push(name.to_owned());
        }
    }
}

/// `#RRGGBB` (or `#AARRGGBB` with an alpha under 1) of components 0..1.
fn argb(c: [f64; 3]) -> String {
    let b = |v: f64| uo_jsnum::conv::to_uint8_clamp(math::round(if v.is_nan() { 0.0 } else { v } * 255.0));
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

/// The colour of a display-list colour string (`hexColor`: a bad hex reads as 0).
fn color_of(units: &[u16]) -> String {
    argb(hex_color_units(units))
}

/// ` Opacity="a"` for an alpha under 1 (nothing otherwise).
fn opacity(a: OptF64) -> String {
    match a.get() {
        Some(v) if v < 1.0 => format!(" Opacity=\"{}\"", fmt(math::max(0.0, v))),
        _ => String::new(),
    }
}

/// XML text of an attribute value.
fn esc(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
}

/// The abbreviated path geometry of commands in the producer's unit divided by `div` (`F 1`: non-zero; `F 0`: even-odd).
fn path_data(cmds: &[RPath], div: f64, even_odd: bool) -> String {
    let mut d = String::from(if even_odd { "F 0" } else { "F 1" });
    let p = |d: &mut String, x: f64, y: f64| {
        let _ = write!(d, " {},{}", fmt(x / div), fmt(y / div));
    };
    for c in cmds {
        match *c {
            RPath::M { x, y } => {
                d.push_str(" M");
                p(&mut d, x, y);
            }
            RPath::L { x, y } => {
                d.push_str(" L");
                p(&mut d, x, y);
            }
            RPath::C { x1, y1, x2, y2, x, y } => {
                d.push_str(" C");
                p(&mut d, x1, y1);
                p(&mut d, x2, y2);
                p(&mut d, x, y);
            }
            RPath::Z => d.push_str(" Z"),
        }
    }
    d
}

fn rect_data(x: f64, y: f64, w: f64, h: f64) -> String {
    format!("M {},{} L {},{} {},{} {},{} Z", fmt(x), fmt(y), fmt(x + w), fmt(y), fmt(x + w), fmt(y + h), fmt(x), fmt(y + h))
}

/// The stroke attributes of a display-list stroke (points → XPS units; the dash in multiples of the width).
fn stroke_attrs(res: &Resources, s: &RStroke, require_hex6: bool) -> Option<String> {
    let c = res.sym(s.color);
    if require_hex6 && !is_hex6(c) {
        return None;
    }
    let w = math::max(0.25, s.width_pt);
    let mut a = format!(" Stroke=\"{}\" StrokeThickness=\"{}\"", color_of(c), fmt(w * PT));
    if s.dash.is_some_and(|d| !res.sym(d).is_empty()) {
        a.push_str(" StrokeDashArray=\"3 2\"");
    }
    Some(a)
}

/// A rotation (degrees, clockwise) and flips about a centre, as an XPS matrix.
fn rot_flip_matrix(cx: f64, cy: f64, rot: f64, flip_h: bool, flip_v: bool) -> String {
    let r = rot * std::f64::consts::PI / 180.0;
    let (c, s) = (math::cos(r), math::sin(r));
    let (sx, sy) = (if flip_h { -1.0 } else { 1.0 }, if flip_v { -1.0 } else { 1.0 });
    let (m11, m12, m21, m22) = (c * sx, s * sx, -s * sy, c * sy);
    let ox = cx - (m11 * cx + m21 * cy);
    let oy = cy - (m12 * cx + m22 * cy);
    format!("{},{},{},{},{},{}", fmt(m11), fmt(m12), fmt(m21), fmt(m22), fmt(ox), fmt(oy))
}

/// The resolution (dots per inch) a JPEG's JFIF header or a PNG's `pHYs` gives; 96 when absent.
fn image_dpi(data: &[u8]) -> (f64, f64) {
    let be16 = |p: usize| {
        data.get(p..p + 2).map(|b| f64::from(u16::from_be_bytes([b.first().copied().unwrap_or(0), b.get(1).copied().unwrap_or(0)])))
    };
    // JFIF: FF D8 FF E0 len "JFIF\0" ver(2) units xd yd
    if data.get(..4) == Some(&[0xff, 0xd8, 0xff, 0xe0][..]) && data.get(6..11) == Some(&b"JFIF\0"[..]) {
        let units = data.get(13).copied().unwrap_or(0);
        if let (Some(xd), Some(yd)) = (be16(14), be16(16))
            && xd > 0.0
            && yd > 0.0
        {
            match units {
                1 => return (xd, yd),
                2 => return (xd * 2.54, yd * 2.54),
                _ => {}
            }
        }
    }
    // PNG: the pHYs chunk (pixels per metre, unit 1)
    if data.get(..8) == Some(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..]) {
        let mut p = 8usize;
        while let Some(len) = data.get(p..p + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes) {
            let ty = data.get(p + 4..p + 8);
            if ty == Some(&b"pHYs"[..]) {
                let rd = |q: usize| data.get(q..q + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes);
                if let (Some(x), Some(y), Some(1)) = (rd(p + 8), rd(p + 12), data.get(p + 16).copied())
                    && x > 0
                    && y > 0
                {
                    return (f64::from(x) * 0.0254, f64::from(y) * 0.0254);
                }
                break;
            }
            if ty == Some(&b"IDAT"[..]) || ty == Some(&b"IEND"[..]) {
                break;
            }
            let Some(next) = usize::try_from(len).ok().and_then(|l| p.checked_add(12)?.checked_add(l)) else { break };
            p = next;
        }
    }
    (96.0, 96.0)
}

/// A PNG of RGBA pixels (8 bits, no interlace, filter 0 on every row, zlib at the default level).
fn png_rgba(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let row = usize::try_from(w).unwrap_or(0).saturating_mul(4);
    let rows = usize::try_from(h).unwrap_or(0);
    let mut raw = Vec::with_capacity(row.saturating_add(1).saturating_mul(rows));
    for r in rgba.chunks(row.max(1)).take(rows) {
        raw.push(0);
        raw.extend_from_slice(r);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let chunk = |out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
        let mut body = Vec::with_capacity(data.len() + 4);
        body.extend_from_slice(ty);
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&uo_opc::flate::crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &uo_opc::flate::zlib(&raw, 6));
    chunk(&mut out, b"IEND", &[]);
    out
}

impl Xps<'_> {
    /// One `FixedPage` and the parts it names.
    fn page(&mut self, lp: &Page) -> (String, Vec<String>) {
        let (w, h) = (lp.w / TW, lp.h / TW);
        let mut p = PageOut { xml: String::with_capacity(4096), used: Vec::new(), depth: 0 };
        let _ = write!(
            p.xml,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><FixedPage xmlns=\"http://schemas.microsoft.com/xps/2005/06\" Width=\"{}\" Height=\"{}\" xml:lang=\"",
            fmt(w),
            fmt(h)
        );
        esc(&mut p.xml, &self.opts.lang);
        p.xml.push_str("\">");
        if let Some(bg) = lp.background.map(|s| self.res.sym(s)).filter(|b| !b.is_empty()) {
            let _ = write!(p.xml, "<Path Fill=\"{}\" Data=\"{}\"/>", color_of(bg), rect_data(0.0, 0.0, w, h));
        }
        if let Some(f) = lp.background_fill.and_then(|id| self.res.page_fill(id)) {
            self.page_fill(&mut p, lp, f);
        }
        for it in &lp.list.items {
            self.item(&mut p, &lp.list, it);
        }
        for _ in 0..p.depth {
            p.xml.push_str("</Canvas>");
        }
        p.xml.push_str("</FixedPage>");
        (p.xml, p.used)
    }

    fn item(&mut self, p: &mut PageOut, list: &ItemList, it: &Item) {
        let res = self.res;
        match it {
            Item::Glyphs { face, color, size_pt, y, glyphs, run, flags } => {
                let Some(r) = glyphs.range() else { return };
                let side = usize::try_from(*run).ok().and_then(|i| list.runs.get(i)).copied().unwrap_or_default();
                let cps = list.glyphs.cp.get(r.clone()).unwrap_or(&[]);
                let xs = list.glyphs.x.get(r.clone()).unwrap_or(&[]);
                let gids = if list.glyphs.gid.is_empty() { &[][..] } else { list.glyphs.gid.get(r).unwrap_or(&[]) };
                self.glyphs(p, *face, res.sym(*color), *size_pt, *y, cps, xs, gids, side.scale_x, side.alpha, *flags);
            }
            Item::Rect { x, y, w, h, fill, alpha } => {
                // a fill that is not 6-digit hex is not painted (the canvas painter's rule)
                let f = res.sym(*fill);
                if !is_hex6(f) {
                    return;
                }
                let _ = write!(
                    p.xml,
                    "<Path Fill=\"{}\"{} Data=\"{}\"/>",
                    color_of(f),
                    opacity(*alpha),
                    rect_data(x / TW, y / TW, w / TW, h / TW)
                );
            }
            Item::Line { x1, y1, x2, y2, color, width_pt, dash } => {
                let w = *width_pt;
                let dash: Vec<f64> = match dash {
                    Some(uo_render::types::LineDash::Dotted) => vec![w, w * 2.0],
                    Some(uo_render::types::LineDash::Dashed) => vec![w * 4.0, w * 3.0],
                    Some(uo_render::types::LineDash::DashDot) => vec![w * 4.0, w * 2.0, w, w * 2.0],
                    Some(uo_render::types::LineDash::DashDotDot) => vec![w * 4.0, w * 2.0, w, w * 2.0, w, w * 2.0],
                    Some(d) => d.border_dashes(w).unwrap_or_default(),
                    None => Vec::new(),
                };
                let _ = write!(p.xml, "<Path Stroke=\"{}\" StrokeThickness=\"{}\"", color_of(res.sym(*color)), fmt(w * PT));
                if !dash.is_empty() && w > 0.0 {
                    p.xml.push_str(" StrokeDashArray=\"");
                    for (i, d) in dash.iter().enumerate() {
                        if i > 0 {
                            p.xml.push(' ');
                        }
                        p.xml.push_str(&fmt(d / w));
                    }
                    p.xml.push('"');
                }
                let _ = write!(p.xml, " Data=\"M {},{} L {},{}\"/>", fmt(x1 / TW), fmt(y1 / TW), fmt(x2 / TW), fmt(y2 / TW));
            }
            Item::Image(im) => self.image(p, im),
            Item::Shape(sh) => self.shape(p, sh),
            Item::Path { cmds, fill, stroke, even_odd } => {
                let fill = fill.map(|f| res.sym(f)).filter(|f| is_hex6(f)).map(color_of);
                let stroke = stroke.as_ref().and_then(|s| stroke_attrs(res, s, false));
                if fill.is_none() && stroke.is_none() {
                    return;
                }
                p.xml.push_str("<Path");
                if let Some(f) = fill {
                    let _ = write!(p.xml, " Fill=\"{f}\"");
                }
                if let Some(s) = stroke {
                    p.xml.push_str(&s);
                }
                let _ = write!(p.xml, " Data=\"{}\"/>", path_data(list.cmds_of(*cmds), TW, *even_odd));
            }
            Item::XformPush { m: Some(m) } => {
                let [a, b, c, d, e, f] = **m;
                let _ = write!(
                    p.xml,
                    "<Canvas RenderTransform=\"{},{},{},{},{},{}\">",
                    fmt(a),
                    fmt(b),
                    fmt(c),
                    fmt(d),
                    fmt(e / TW),
                    fmt(f / TW)
                );
                p.depth += 1;
            }
            // the PDF painter's pop for these three (`pop_clip`): a Canvas closed when one is open
            Item::XformPush { m: None } | Item::XformPop | Item::ClipPop => {
                if p.depth > 0 {
                    p.depth -= 1;
                    p.xml.push_str("</Canvas>");
                }
            }
            Item::ClipPush { x, y, w, h } => {
                let _ = write!(p.xml, "<Canvas Clip=\"{}\">", rect_data(x.or(0.0) / TW, y.or(0.0) / TW, w.or(0.0) / TW, h.or(0.0) / TW));
                p.depth += 1;
            }
        }
    }

    // ---- glyph runs ---------------------------------------------------------------------

    /// The font part of a face (embedded once per face key); `None` without bytes.
    fn font_part(&mut self, face: FaceId) -> Option<String> {
        let fr = self.res.face(face)?;
        if let Some(n) = self.fonts.get(&fr.key) {
            return n.clone();
        }
        let made = fr.bytes.as_ref().map(|b| {
            let name = format!("/Resources/Fonts/F{}.ttf", self.font_parts.len() + 1);
            self.font_parts.push((name.clone(), std::sync::Arc::clone(b)));
            name
        });
        self.fonts.insert(fr.key, made.clone());
        made
    }

    #[allow(clippy::too_many_arguments, reason = "the columns of one glyph run")]
    fn glyphs(
        &mut self,
        p: &mut PageOut,
        face: FaceId,
        color: &[u16],
        size_pt: f64,
        y: f64,
        cps: &[u32],
        xs: &[f64],
        gids: &[u32],
        scale_x: OptF64,
        alpha: OptF64,
        flags: RunFlags,
    ) {
        if cps.is_empty() || size_pt <= 0.0 {
            return;
        }
        // a run of white space only is not written (nothing to see)
        let blank =
            cps.iter().all(|&c| u16::try_from(c).is_ok_and(is_js_space_unit) || c == 0xad || c == 0xfffc || c == 0x200b || c < 0x20);
        if blank {
            return;
        }
        let Some(font) = self.font_part(face) else { return };
        let sf = self.res.face(face).and_then(|f| f.face.as_ref());
        let gid_of = |i: usize, cp: u32| -> Option<u32> {
            if let Some(g) = gids.get(i).copied().filter(|&g| g != NO_GID) {
                return Some(g);
            }
            let f = sf?;
            let g = f.glyph_id(cp);
            // the symbol-font fallback of the PDF (`SymbolFallback`): an F0xx code the cmap lacks
            Some(if g == 0 && (0xf000..=0xf0ff).contains(&cp) && f.is_symbol_font() { f.glyph_id(cp - 0xf000) } else { g })
        };
        let sx = scale_x.get().filter(|&s| truthy(s) && s > 0.0).unwrap_or(1.0);
        let em = size_pt * PT;
        let fill = color_of(color);
        let sim = match (flags.has(RunFlags::SYNTH_BOLD), flags.has(RunFlags::SYNTH_ITALIC)) {
            (true, true) => " StyleSimulations=\"BoldItalicSimulation\"",
            (true, false) => " StyleSimulations=\"BoldSimulation\"",
            (false, true) => " StyleSimulations=\"ItalicSimulation\"",
            (false, false) => "",
        };
        let op = opacity(alpha);
        let forward = xs.windows(2).all(|w| matches!(w, [a, b] if b >= a));
        // a run whose glyphs go back is written one element per glyph (each at its own x)
        let groups: Vec<(usize, usize)> = if forward { vec![(0, cps.len())] } else { (0..cps.len()).map(|i| (i, i + 1)).collect() };
        for (a, b) in groups {
            let (Some(cs), Some(gx)) = (cps.get(a..b), xs.get(a..b)) else { continue };
            let Some(&x0) = gx.first() else { continue };
            let mut text = String::new();
            let mut idx = String::new();
            for (k, (&cp, &x)) in cs.iter().zip(gx).enumerate() {
                // a control character (not allowed in XML) is written as a space; a lone surrogate as U+FFFD
                let ch = match char::from_u32(cp) {
                    Some(c) if c.is_control() => ' ',
                    Some(c) => c,
                    None => '\u{fffd}',
                };
                text.push(ch);
                if k > 0 {
                    idx.push(';');
                }
                if ch.len_utf16() > 1 {
                    idx.push_str("(2:1)");
                }
                if let Some(g) = gid_of(a + k, cp) {
                    idx.push_str(&uo_jsnum::format::to_js_string(f64::from(g)));
                }
                if let Some(&nx) = gx.get(k + 1) {
                    // the advance to the next glyph as the layout placed it, in 1/100 em (before the character scale)
                    idx.push(',');
                    idx.push_str(&fmt(((nx - x) / 20.0 / size_pt) * 100.0 / sx));
                }
            }
            let ox = x0 / TW;
            let oy = y / TW;
            let _ = write!(
                p.xml,
                "<Glyphs OriginX=\"{}\" OriginY=\"{}\" FontRenderingEmSize=\"{}\" FontUri=\"{font}\" Fill=\"{fill}\"{op}{sim}",
                fmt(ox),
                fmt(oy),
                fmt(em)
            );
            if (sx - 1.0).abs() > f64::EPSILON {
                let _ = write!(p.xml, " RenderTransform=\"{},0,0,1,{},0\"", fmt(sx), fmt(ox * (1.0 - sx)));
            }
            let _ = write!(p.xml, " Indices=\"{idx}\" UnicodeString=\"");
            // a string that starts with "{" is escaped by "{}" (XPS markup compatibility)
            if text.starts_with('{') {
                p.xml.push_str("{}");
            }
            esc(&mut p.xml, &text);
            p.xml.push_str("\"/>");
        }
        p.use_part(&font);
    }

    // ---- images -------------------------------------------------------------------------

    /// The image part of an image (with the picture's pixel adjustments when it has them).
    fn image_part(&mut self, p: &mut PageOut, id: ImageId, it: Option<&ImageItem>) -> Option<ImagePart> {
        let need = it.and_then(|i| i.pic.as_deref()).is_some_and(|l| l.alpha.is_some() || l.duotone.is_some() || l.clr_change.is_some());
        let key = (
            id,
            if need {
                it.map_or_else(String::new, |i| format!("{:?}", i.pic.as_deref().map(|l| (l.alpha, l.duotone, l.clr_change))))
            } else {
                String::new()
            },
        );
        if let Some(h) = self.images.get(&key) {
            if let Some(h) = h {
                p.use_part(&h.name);
            }
            return h.clone();
        }
        let made = self.make_image(id, if need { it } else { None });
        self.images.insert(key, made.clone());
        if let Some(h) = &made {
            p.use_part(&h.name);
        }
        made
    }

    fn make_image(&mut self, id: ImageId, adjust: Option<&ImageItem>) -> Option<ImagePart> {
        let ir = self.res.image(id)?;
        let data: &[u8] = &ir.bytes;
        let n = self.image_parts.len() + 1;
        let jp = jpeg_info(data);
        let is_png = data.get(..8) == Some(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..]);
        let plain = ir.effects.is_none() && adjust.is_none();
        // a JPEG goes as it is (no JPEG decoder: its colour adjustments are not applied); a plain PNG too
        if let Some(j) = &jp {
            let (dx, dy) = image_dpi(data);
            let name = format!("/Resources/Images/I{n}.jpg");
            self.image_parts.push((name.clone(), data.to_vec()));
            return Some(ImagePart { name, w: f64::from(j.width) * 96.0 / dx, h: f64::from(j.height) * 96.0 / dy });
        }
        if is_png && plain {
            let r = uo_image::image_pixel_size(data)?;
            let (dx, dy) = image_dpi(data);
            let name = format!("/Resources/Images/I{n}.png");
            self.image_parts.push((name.clone(), data.to_vec()));
            return Some(ImagePart { name, w: f64::from(r.width) * 96.0 / dx, h: f64::from(r.height) * 96.0 / dy });
        }
        let mut r = decode_raster(data, &self.opts.image).ok()?;
        if let Some(fx) = &ir.effects {
            apply_image_effects(&mut r.data, fx);
        }
        if let Some(pic) = adjust.and_then(|i| i.pic.as_ref()) {
            apply_picture_adjust(&mut r.data, &AdjustParts::of(self.res, pic).spec());
        }
        let name = format!("/Resources/Images/I{n}.png");
        self.image_parts.push((name.clone(), png_rgba(&r.data, r.width, r.height)));
        Some(ImagePart { name, w: f64::from(r.width), h: f64::from(r.height) })
    }

    /// An RGBA raster as an image part (the page-fill tiles).
    fn raster_part(&mut self, p: &mut PageOut, key: String, rgba: &[u8], w: u32, h: u32) -> ImagePart {
        if let Some(h) = self.rasters.get(&key) {
            let h = h.clone();
            p.use_part(&h.name);
            return h;
        }
        let name = format!("/Resources/Images/I{}.png", self.image_parts.len() + 1);
        self.image_parts.push((name.clone(), png_rgba(rgba, w, h)));
        let part = ImagePart { name, w: f64::from(w), h: f64::from(h) };
        self.rasters.insert(key, part.clone());
        p.use_part(&part.name);
        part
    }

    /// `<ImageBrush>` of a part over the box `(x, y, w, h)` (XPS units).
    fn brush(part: &ImagePart, x: f64, y: f64, w: f64, h: f64, tile: bool) -> String {
        format!(
            "<ImageBrush ImageSource=\"{}\" Viewbox=\"0,0,{},{}\" ViewboxUnits=\"Absolute\" Viewport=\"{},{},{},{}\" ViewportUnits=\"Absolute\"{}/>",
            part.name,
            fmt(part.w),
            fmt(part.h),
            fmt(x),
            fmt(y),
            fmt(w),
            fmt(h),
            if tile { " TileMode=\"Tile\"" } else { "" }
        )
    }

    fn image(&mut self, p: &mut PageOut, it: &ImageItem) {
        let Some(part) = self.image_part(p, it.image, Some(it)) else { return };
        let (x, y, w, h) = (it.x / TW, it.y / TW, it.w / TW, it.h / TW);
        let rot = it.rot.or(0.0);
        let turned = truthy(rot) || it.flip_h || it.flip_v;
        if turned {
            let _ = write!(p.xml, "<Canvas RenderTransform=\"{}\">", rot_flip_matrix(x + w / 2.0, y + h / 2.0, rot, it.flip_h, it.flip_v));
        }
        // the crop: the picture drawn larger, clipped by its frame
        let (mut ix, mut iy, mut iw, mut ih) = (x, y, w, h);
        if let Some(Ltrb { l, t, r, b }) = it.crop.filter(|c| truthy(c.l) || truthy(c.t) || truthy(c.r) || truthy(c.b)) {
            let fw = w / math::max(0.01, 1.0 - l - r);
            let fh = h / math::max(0.01, 1.0 - t - b);
            ix = x - l * fw;
            iy = y - t * fh;
            iw = fw;
            ih = fh;
        }
        // a picture's transparency is in its pixels (the adjustments of `image_part`)
        let _ = write!(
            p.xml,
            "<Path Data=\"{}\"><Path.Fill>{}</Path.Fill></Path>",
            rect_data(x, y, w, h),
            Self::brush(&part, ix, iy, iw, ih, false)
        );
        if turned {
            p.xml.push_str("</Canvas>");
        }
    }

    // ---- shapes -------------------------------------------------------------------------

    fn shape(&mut self, p: &mut PageOut, it: &ShapeItem) {
        let res = self.res;
        let st = &it.geom;
        let fill = it.fill.map(|f| res.sym(f)).filter(|f| is_hex6(f)).map(color_of);
        let stroke = it.stroke.as_ref().and_then(|s| stroke_attrs(res, s, true));
        if fill.is_none() && stroke.is_none() && it.grad.is_none() && it.blip.is_none() {
            return;
        }
        // the parts in points, then to XPS units (the PDF's geometry with its unit)
        let (x, y, w, h) = (it.x / 20.0, it.y / 20.0, it.w / 20.0, it.h / 20.0);
        let parts = geom::shape_parts(LineUnit::PdfPoints, st, res, [x, y, w, h], it.line);
        let k = 1.0 / PT;
        // outer shadow: a shifted copy in the shadow colour (no blur), as the PDF
        if let Some(sh) = &it.shadow {
            let (dx, dy) = (sh.dx / TW, sh.dy / TW);
            let col = color_of(res.sym(sh.color));
            let _ = write!(p.xml, "<Canvas RenderTransform=\"1,0,0,1,{},{}\"{}>", fmt(dx), fmt(dy), opacity(sh.alpha));
            for part in &parts {
                let f = part.fill && (fill.is_some() || it.grad.is_some());
                let s = part.stroke && stroke.is_some();
                if !f && !s {
                    continue;
                }
                p.xml.push_str("<Path");
                if f {
                    let _ = write!(p.xml, " Fill=\"{col}\"");
                }
                if s && let Some(sk) = &it.stroke {
                    // the outline in the shadow colour
                    let _ = write!(p.xml, " Stroke=\"{col}\" StrokeThickness=\"{}\"", fmt(math::max(0.25, sk.width_pt) * PT));
                }
                let _ = write!(p.xml, " Data=\"{}\"/>", path_data(&part.cmds, k, part.even_odd));
            }
            p.xml.push_str("</Canvas>");
        }
        // the fill: a picture stretched over the frame, a gradient, or the colour
        let fill_attr = if let Some(blip) = &it.blip {
            self.image_part(p, blip.image, None)
                .map(|part| (Self::brush(&part, x * PT, y * PT, w * PT, h * PT, false), opacity(blip.alpha)))
        } else if let Some(g) = &it.grad {
            let all: Vec<RPath> = parts.iter().flat_map(|q| q.cmds.iter().copied()).collect();
            let pb = geom::path_bounds(&all);
            let gg = gradient_geometry(g, &res.symbols, GradBox { x, y, w, h, rot: st.rot.or(0.0) }, pb);
            let mut stops = String::new();
            for s in &g.stops {
                let _ = write!(
                    stops,
                    "<GradientStop Color=\"{}\" Offset=\"{}\"/>",
                    color_of(res.sym(s.color)),
                    fmt(math::max(0.0, math::min(1.0, s.pos)))
                );
            }
            let b = match gg {
                GradGeom::Axial { x0, y0, x1, y1 } => format!(
                    "<LinearGradientBrush MappingMode=\"Absolute\" StartPoint=\"{},{}\" EndPoint=\"{},{}\"><LinearGradientBrush.GradientStops>{stops}</LinearGradientBrush.GradientStops></LinearGradientBrush>",
                    fmt(x0 * PT),
                    fmt(y0 * PT),
                    fmt(x1 * PT),
                    fmt(y1 * PT)
                ),
                GradGeom::Radial { x0, y0, r } => format!(
                    "<RadialGradientBrush MappingMode=\"Absolute\" Center=\"{0},{1}\" GradientOrigin=\"{0},{1}\" RadiusX=\"{2}\" RadiusY=\"{2}\"><RadialGradientBrush.GradientStops>{stops}</RadialGradientBrush.GradientStops></RadialGradientBrush>",
                    fmt(x0 * PT),
                    fmt(y0 * PT),
                    fmt(r * PT)
                ),
            };
            Some((b, opacity(st.fill_alpha)))
        } else {
            None
        };
        for part in &parts {
            let data = path_data(&part.cmds, k, part.even_odd);
            if part.fill {
                match (&fill_attr, &fill) {
                    (Some((brush, op)), _) => {
                        let _ = write!(p.xml, "<Path{op} Data=\"{data}\"><Path.Fill>{brush}</Path.Fill></Path>");
                    }
                    (None, Some(f)) => {
                        let _ = write!(p.xml, "<Path Fill=\"{f}\"{} Data=\"{data}\"/>", opacity(st.fill_alpha));
                    }
                    _ => {}
                }
            }
            if part.stroke
                && let Some(s) = &stroke
            {
                let _ = write!(p.xml, "<Path{s}{} Data=\"{data}\"/>", opacity(st.line_alpha));
            }
        }
    }

    // ---- page fill ----------------------------------------------------------------------

    /// The page-fill effect (gradient, pattern, texture) as a tiled `ImageBrush` over the page, the PDF's tiles.
    fn page_fill(&mut self, p: &mut PageOut, lp: &Page, f: &PageFill) {
        let Some((tile_w, tile_h)) = page_fill_tile_tw(f, 2.4, 20.0, true) else { return };
        let (tw, th) = (tile_w / TW, tile_h / TW);
        let syms = &self.res.symbols;
        let part = match f.kind {
            PageFillKind::Gradient | PageFillKind::GradientRadial => {
                let pw = math::max(2.0, math::round(tw));
                let ph = math::max(2.0, math::round(th));
                let (Some(pwu), Some(phu)) = (js_index(pw, 1 << 20), js_index(ph, 1 << 20)) else { return };
                let Ok(rgba) = gradient_tile_rgba(f, syms, pwu, phu, true, &self.opts.paint) else { return };
                let (Ok(wu), Ok(hu)) = (u32::try_from(pwu), u32::try_from(phu)) else { return };
                let key = format!("g|{f:?}|{wu}x{hu}");
                self.raster_part(p, key, &rgba, wu, hu)
            }
            PageFillKind::Pattern => {
                let base = pattern_rgba(f, syms);
                let key = format!("p|{f:?}");
                self.raster_part(p, key, &base, 8, 8)
            }
            PageFillKind::Frame | PageFillKind::Tile => {
                let Some(part) = f.image.and_then(|id| self.image_part(p, id, None)) else { return };
                part
            }
        };
        if tw <= 0.0 || th <= 0.0 {
            return;
        }
        let (w, h) = (lp.w / TW, lp.h / TW);
        let _ = write!(
            p.xml,
            "<Path Data=\"{}\"><Path.Fill>{}</Path.Fill></Path>",
            rect_data(0.0, 0.0, w, h),
            Self::brush(&part, 0.0, 0.0, tw, th, true)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_well_formed() {
        let px = [255u8, 0, 0, 255, 0, 255, 0, 128];
        let png = png_rgba(&px, 2, 1);
        assert_eq!(png.get(..8), Some(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..]));
        let back = uo_image::decode_png(&png, &ImageLimits::DEFAULT).unwrap();
        assert_eq!((back.width, back.height), (2, 1));
        assert_eq!(back.data, px.to_vec());
        assert_eq!(image_dpi(&png), (96.0, 96.0));
    }

    #[test]
    fn rotation_matrix_keeps_the_centre() {
        let m = rot_flip_matrix(10.0, 20.0, 90.0, false, false);
        assert_eq!(m, "0,1,-1,0,30,10");
    }

    #[test]
    fn empty_document_is_a_package() {
        let res = Resources::default();
        let page =
            Page { w: 12240.0, h: 15840.0, list: ItemList::default(), background: None, background_fill: None, background_show: false };
        let z = render_xps(std::slice::from_ref(&page), &res, &XpsOptions::default()).unwrap();
        assert_eq!(z.get(..2), Some(&b"PK"[..]));
    }
}
