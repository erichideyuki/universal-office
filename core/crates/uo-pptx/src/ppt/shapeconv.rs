//! OfficeArt shapes → elements of the spTree (`<p:sp>`, `<p:pic>`, `<p:cxnSp>`, `<p:grpSp>`),
//! with the FOPT properties translated to DrawingML: position (anchor + rotation/flip), geometry
//! (preset/custom), fill, line, shadow, text and placeholder (`ppt/shapeconv.ts`).
//!
//! Everything is written explicitly; with round trip, the appearance PowerPoint 2007+ saved in the
//! metroBlob (shapexml) overrides the binary's.

use std::rc::Rc;

use uo_drawingml::odraw::msospt::{self, PresetVariant};
use uo_drawingml::odraw::{Prop, PropTable, fsp};
use uo_drawingml::preset_patterns;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{abs, atan2, ceil, cos, hypot, max, max_n, min, min_n, round, sin, tan};
use uo_jsnum::parse::parse_int;

use super::context::{PartCtx, PartHost, default_scheme_at};
use super::error::PptError;
use super::metafile::recolor_metafile;
use super::oa::{OaShape, Rect, bool_prop, bool_prop_opt, fixed, prop, sprop};
use super::rd::{Rec, RecRead, children, i16_at, i32_at, read_rec, rt, u16_at, u32_at, utf16, utf16_units};
use super::reader::PptDocument;
use super::roundtrip::{XNode, XmlElement, metro_diagram, metro_text_body, metro_visual, parse_xml, serialize_xml};
use super::text::{TextBlock, read_client_textbox, read_style_text_prop9};
use super::textconv::{TextHost, emu, hex3, paragraphs_xml};
use super::xmlw::{V, XML_DECL, b, el, esc_attr, esc_text, n, no, ns, nz, s, so};

type R = Result<String, PptError>;

// ───────────────────────────── properties ─────────────────────────────

/// The string (UTF-16, without the terminator) of a complex property.
fn prop_string(p: Option<&Prop<'_>>) -> Option<String> {
    let c = p?.complex?;
    if c.len() < 2 {
        return None;
    }
    let mut units = utf16_units(c, 0, c.len() as u64);
    if let Some(z) = units.iter().position(|&u| u == 0) {
        units.truncate(z);
    }
    if units.is_empty() { None } else { Some(String::from_utf16_lossy(&units)) }
}

/// An `IMsoArray` (nElems, nElemsAlloc, cbElem + data) → raw elements.
struct MsoArray<'p> {
    n: u64,
    size: u64,
    data: &'p [u8],
    off: u64,
}

fn mso_array<'p>(p: Option<&Prop<'p>>) -> Option<MsoArray<'p>> {
    let c = p?.complex?;
    if c.len() < 6 {
        return None;
    }
    let nn = u64::from(u16_at(c, 0));
    let mut size = u64::from(u16_at(c, 4));
    if size == 0xfff0 {
        size = 4;
    }
    if size == 0 {
        return None;
    }
    let cap = (c.len() as u64 - 6) / size;
    Some(MsoArray { n: nn.min(cap), size, data: c, off: 6 })
}

// ───────────────────────────── colours ─────────────────────────────

/// System colours (GetSysColor) with the default values of Windows.
const SYS_COLORS: [&str; 25] = [
    "C8C8C8", "000000", "99B4D1", "BFCDDB", "F0F0F0", "FFFFFF", "646464", "000000", "000000", "000000", "B4B4B4", "F4F7FC", "ABABAB",
    "3399FF", "FFFFFF", "F0F0F0", "A0A0A0", "6D6D6D", "000000", "434E54", "FFFFFF", "696969", "E3E3E3", "000000", "FFFFE1",
];

fn hex_byte(s: &str, i: usize) -> f64 {
    s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).map_or(f64::NAN, f64::from)
}

/// `OfficeArtCOLORREF` → 'RRGGBB'. High byte: 0x08 = index in the scheme (red byte), 0x10 =
/// system/special colour (0xF0 = the fill colour, 0xF2 = the line's… + a lighten/darken function
/// in the 2nd byte and its parameter in the 3rd).
fn oa_color(value: u32, props: &PropTable<'_>, pc: &PartCtx<'_, '_>, depth: u32) -> String {
    let flags = (value >> 24) & 0xff;
    let r = value & 0xff;
    let g = (value >> 8) & 0xff;
    let bb = (value >> 16) & 0xff;
    if flags & 0x08 != 0 {
        let i = usize::try_from(r).unwrap_or(0);
        return pc.scheme.get(i).cloned().or_else(|| default_scheme_at(i).map(str::to_owned)).unwrap_or_else(|| "000000".to_owned());
    }
    if flags & 0x10 != 0 {
        let idx = value & 0xff;
        let func = (value >> 8) & 0x0f;
        let invert = ((value >> 8) & 0x20) != 0;
        let param = f64::from((value >> 16) & 0xff);
        let sub = |id: u16, d: u32| -> String {
            if depth > 2 { "000000".to_owned() } else { oa_color(prop(props, id).unwrap_or(d), props, pc, depth + 1) }
        };
        let base = match idx {
            0xf0 => sub(0x181, 0xff_ffff),
            0xf1 => {
                if bool_prop(props, 0x1ff, 3, true) {
                    sub(0x1c0, 0)
                } else {
                    sub(0x181, 0xff_ffff)
                }
            }
            0xf2 => sub(0x1c0, 0),
            0xf3 => sub(0x201, 0x80_8080),
            0xf5 => sub(0x183, 0xff_ffff),
            0xf6 => sub(0x1c2, 0xff_ffff),
            0xf7 => {
                if bool_prop(props, 0x1bf, 4, true) {
                    sub(0x181, 0xff_ffff)
                } else {
                    sub(0x1c0, 0)
                }
            }
            _ => SYS_COLORS.get(usize::try_from(idx).unwrap_or(usize::MAX)).map_or_else(|| "000000".to_owned(), |c| (*c).to_owned()),
        };
        let (mut cr, mut cg, mut cb) = (hex_byte(&base, 0), hex_byte(&base, 2), hex_byte(&base, 4));
        let f = |c: f64| -> f64 {
            match func {
                // darken/lighten in sRGB (the application's conversion writes <a:gamma/><a:shade|tint/><a:invGamma/>:
                // the gamma takes the shade/tint out of linear space — it draws like that, darker)
                1 => (c * param) / 255.0,
                2 => 255.0 - ((255.0 - c) * param) / 255.0,
                3 => c + param,
                4 => c - param,
                5 => param - c,
                6 => {
                    if c < param {
                        0.0
                    } else {
                        255.0
                    }
                }
                _ => c,
            }
        };
        cr = f(cr);
        cg = f(cg);
        cb = f(cb);
        if invert {
            cr = 255.0 - cr;
            cg = 255.0 - cg;
            cb = 255.0 - cb;
        }
        let clamp = |c: f64| max(0.0, min(255.0, round(c)));
        return hex3(clamp(cr), clamp(cg), clamp(cb));
    }
    hex3(f64::from(r), f64::from(g), f64::from(bb))
}

fn srgb(val: &str, alpha: Option<f64>) -> String {
    match alpha {
        Some(a) if a < 1.0 => el("a:srgbClr", vec![("val", s(val))], &el("a:alpha", vec![("val", n(round(a * 100_000.0)))], "")),
        _ => el("a:srgbClr", vec![("val", s(val))], ""),
    }
}

// ───────────────────────────── media ─────────────────────────────

/// Makes sure the media part of the BLIP `pib` (1-based) exists; gives its path.
fn media_for(pc: &mut PartCtx<'_, '_>, pib: u32) -> Option<String> {
    let key = i64::from(pib);
    if let Some(hit) = pc.conv.media.get(&key).filter(|p| !p.is_empty()) {
        return Some(hit.clone());
    }
    let doc = pc.conv.doc;
    let blip = doc.blip(pib)?;
    let ext = blip.kind.ext();
    let path = format!("ppt/media/image{}.{ext}", pc.conv.media.len() + 1);
    pc.conv.media.set(key, path.clone());
    pc.conv.parts.set(path.clone(), blip.bytes.to_vec());
    pc.conv.media_exts.set(ext.to_owned(), ());
    Some(path)
}

/// The relation to `ppt/media/…` as the part's rels write it: `../media/…`.
fn media_rel(pc: &mut PartCtx<'_, '_>, path: &str) -> String {
    pc.rels.add("image", &format!("../media/{}", path.strip_prefix("ppt/media/").unwrap_or(path)), false)
}

/// Relation of an image of the current part → rId (`None` without an image).
fn image_rel(pc: &mut PartCtx<'_, '_>, pib: u32) -> Option<String> {
    let path = media_for(pc, pib)?;
    Some(media_rel(pc, &path))
}

/// Pattern fill of the binary: the fillBlip is a 1 bpp 8×8 BMP. Matches the bitmap (the LIGHT
/// palette pixel = foreground/fillColor — checked in the reference application: white background
/// with thin lines) with the predefined DrawingML patterns, in any phase (cyclic shift); no match
/// → `None`.
fn pattern_prst(pc: &PartCtx<'_, '_>, pib: u32) -> Option<&'static str> {
    let blip = pc.conv.doc.blip(pib)?;
    let bytes: &[u8] = &blip.bytes;
    if bytes.len() < 62 || bytes[0] != 0x42 || bytes[1] != 0x4d {
        return None;
    }
    let le32 = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let off = u64::from(le32(10));
    let hdr = u64::from(le32(14));
    let w = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let h = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let bpp = u16::from_le_bytes([bytes[28], bytes[29]]);
    if w != 8 || h.unsigned_abs() != 8 || bpp != 1 {
        return None;
    }
    let at = |o: u64| usize::try_from(o).ok().and_then(|i| bytes.get(i)).copied().unwrap_or(0);
    let pal: Vec<u32> = (0..2u64)
        .map(|i| {
            let o = 14 + hdr + i * 4;
            u32::from(at(o)) + u32::from(at(o + 1)) + u32::from(at(o + 2))
        })
        .collect();
    let light_bit = pal.get(1).copied().unwrap_or(0) > pal.first().copied().unwrap_or(0);
    let mut rows = [0u8; 8];
    for (r, slot) in rows.iter_mut().enumerate() {
        let rr = u64::try_from(r).unwrap_or(0);
        let v = at(off + (if h > 0 { 7 - rr } else { rr }) * 4);
        *slot = if light_bit { v } else { !v };
    }
    let rot = |v: u8, k: u32| -> u8 { u8::try_from(((u32::from(v) << k) | (u32::from(v) >> (8 - k))) & 0xff).unwrap_or(0) };
    for dy in 0..8usize {
        for dx in 0..8u32 {
            let mut cand = [0u8; 8];
            for (r, slot) in cand.iter_mut().enumerate() {
                *slot = rot(rows.get((r + dy) % 8).copied().unwrap_or(0), dx);
            }
            for (k, pat) in preset_patterns() {
                if *pat == cand {
                    return Some(k);
                }
            }
        }
    }
    None
}

/// Image embedded in the complex data of the property (an OfficeArtBlip record) → rId.
fn complex_image_rel(pc: &mut PartCtx<'_, '_>, complex: &[u8]) -> Option<String> {
    let mut key = format!("{}:", complex.len());
    for v in complex.iter().take(64) {
        key.push_str(&format!("{v:x}"));
    }
    let path = match pc.conv.complex_media.get(&key).filter(|p| !p.is_empty()) {
        Some(p) => p.clone(),
        None => {
            let rec = read_rec(complex, 0, complex.len() as u64)?;
            let blip = super::reader::decode_unlimited(&rec)?;
            let ext = blip.kind.ext();
            let path = format!("ppt/media/image{}c.{ext}", pc.conv.media.len() + pc.conv.complex_media.len() + 1);
            pc.conv.complex_media.set(key, path.clone());
            pc.conv.parts.set(path.clone(), blip.bytes.to_vec());
            pc.conv.media_exts.set(ext.to_owned(), ());
            path
        }
    };
    Some(media_rel(pc, &path))
}

/// "Recolor picture" of PowerPoint 97: the RecolorInfoAtom (0x0FE7) of the ClientData changes
/// colours of the METAFILE. Entries of 44 bytes after 12 of header (the number of colours at +2);
/// each: bit 0 = replaced, the new colour (16 bits/channel), the index, the original colour. An
/// index < 8 = a colour of the slide's SCHEME — the recorded one is only the picture of when it was
/// chosen. → rId of a recoloured copy of the media, or `None`.
fn recolor_rel(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, pib: u32) -> Result<Option<String>, PptError> {
    let Some(cd) = &sh.client_data else { return Ok(None) };
    let Some(rec) = children(cd).into_iter().find(|c| c.rec_type == 0x0fe7) else { return Ok(None) };
    let d = super::rd::sub(rec.data, rec.start, rec.end);
    if d.len() < 12 {
        return Ok(None);
    }
    let byte = |o: usize| u32::from(d.get(o).copied().unwrap_or(0));
    let u16d = |o: usize| byte(o) | (byte(o + 1) << 8);
    let mut map: Vec<(u32, u32)> = Vec::new();
    let n_g = usize::try_from(u16d(2)).unwrap_or(0);
    let mut k = 0usize;
    while k < n_g && 12 + k * 44 + 18 <= d.len() {
        let o = 12 + k * 44;
        k += 1;
        if u16d(o) & 1 == 0 {
            continue;
        }
        let idx = usize::try_from(u16d(o + 8)).unwrap_or(usize::MAX);
        let from = byte(o + 13) | (byte(o + 15) << 8) | (byte(o + 17) << 16);
        let mut to = byte(o + 3) | (byte(o + 5) << 8) | (byte(o + 7) << 16);
        let sch = if idx < 8 { pc.scheme.get(idx).cloned().or_else(|| default_scheme_at(idx).map(str::to_owned)) } else { None };
        if let Some(sch) = sch.filter(|x| !x.is_empty()) {
            let v = parse_int(&sch, 16);
            let v = if v.is_nan() { 0 } else { uo_jsnum::conv::to_int32(v) };
            let v = u32::from_ne_bytes(v.to_ne_bytes());
            to = ((v >> 16) & 0xff) | (v & 0xff00) | ((v & 0xff) << 16);
        }
        if to != from {
            match map.iter_mut().find(|(f, _)| *f == from) {
                Some(slot) => slot.1 = to,
                None => map.push((from, to)),
            }
        }
    }
    let doc = pc.conv.doc;
    let blip = if map.is_empty() { None } else { doc.blip(pib) };
    let Some(blip) = blip else { return Ok(None) };
    let ext = blip.kind.ext();
    if ext != "emf" && ext != "wmf" {
        return Ok(None);
    }
    let list: Vec<String> = map.iter().map(|(a, b2)| format!("{a},{b2}")).collect();
    let key = format!("{pib}:{}", list.join(";"));
    let path = match pc.conv.recolor_media.get(&key).filter(|p| !p.is_empty()) {
        Some(p) => p.clone(),
        None => {
            let Some(bytes) = recolor_metafile(&blip.bytes, &map)? else { return Ok(None) };
            let path = format!("ppt/media/image{}r.{ext}", pc.conv.recolor_media.len() + 1);
            pc.conv.recolor_media.set(key, path.clone());
            pc.conv.parts.set(path.clone(), bytes);
            pc.conv.media_exts.set(ext.to_owned(), ());
            path
        }
    };
    Ok(Some(media_rel(pc, &path)))
}

// ───────────────────────────── fill / line / shadow ─────────────────────────────

#[derive(Clone)]
struct Stop {
    pos: f64,
    color: String,
    alpha: Option<f64>,
}

/// Gradient stops along the vector. The colours go from the fill to the background (or the
/// explicit list fillShadeColors, fill → background) and the focus decides the order — matched to
/// PowerPoint's own conversion (~13 thousand gradients of the corpus): focus 0 = fill →
/// background, ±100 = inverted, in between = mirrored; the mirror puts the fill at the ends with
/// positive focus and in the middle with negative — and the other way round when the fillAngle was
/// written (≠ 0), which `flip_mirror` indicates.
fn gradient_stops(
    props: &PropTable<'_>,
    pc: &PartCtx<'_, '_>,
    fill: &str,
    back: &str,
    fill_a: Option<f64>,
    back_a: Option<f64>,
    flip_mirror: bool,
) -> String {
    let arr = mso_array(props.get(0x197));
    let mut list: Vec<Stop>;
    match arr {
        Some(a) if a.size == 8 && a.n >= 2 => {
            list = Vec::new();
            for i in 0..a.n {
                let o = a.off + i * 8;
                list.push(Stop {
                    color: oa_color(u32_at(a.data, o), props, pc, 0),
                    pos: fixed(Some(u32_at(a.data, o + 4)), 0.0),
                    alpha: None,
                });
            }
            list.sort_by(|x, y| x.pos.partial_cmp(&y.pos).unwrap_or(std::cmp::Ordering::Equal));
        }
        _ => {
            list = vec![Stop { pos: 0.0, color: fill.to_owned(), alpha: fill_a }, Stop { pos: 1.0, color: back.to_owned(), alpha: back_a }];
        }
    }
    let focus = sprop(props, 0x18c).unwrap_or(0);
    let rev = |l: &[Stop]| -> Vec<Stop> { l.iter().map(|x| Stop { pos: 1.0 - x.pos, ..x.clone() }).rev().collect() };
    let mut stops: Vec<Stop>;
    if focus == 0 {
        stops = list;
    } else if focus >= 100 || focus <= -100 {
        stops = rev(&list);
    } else {
        let ff = f64::from(focus.abs()) / 100.0;
        // fillAtEnds: fill at the ends (f…b…f); otherwise in the middle (b…f…b)
        let fill_at_ends = (focus > 0) != flip_mirror;
        let a = if fill_at_ends { list } else { rev(&list) }; // stretch [0, F]
        let mut first: Vec<Stop> = a.iter().map(|x| Stop { pos: x.pos * ff, ..x.clone() }).collect();
        first.extend(rev(&a).into_iter().map(|x| Stop { pos: ff + x.pos * (1.0 - ff), ..x }));
        stops = first;
    }
    stops.sort_by(|x, y| x.pos.partial_cmp(&y.pos).unwrap_or(std::cmp::Ordering::Equal));
    // 2-colour mirror: PowerPoint writes f/b/b/f, but in the .ppt it draws the "2 colours with
    // focus" gradient (sigma curve in each half) — which in the .pptx only comes out with 3 stops
    // of equal ends (measured, syn-gradmode2); 4 stops = sRGB
    let kept: Vec<&Stop> = stops
        .iter()
        .enumerate()
        .filter(|(i, st)| {
            *i == 0 || {
                let p = &stops[*i - 1];
                st.pos != p.pos || st.color != p.color || st.alpha != p.alpha
            }
        })
        .map(|(_, st)| st)
        .collect();
    let body: String = kept
        .iter()
        .map(|st| el("a:gs", vec![("pos", n(round(max(0.0, min(1.0, st.pos)) * 100_000.0)))], &srgb(&st.color, st.alpha)))
        .collect();
    el("a:gsLst", vec![], &body)
}

/// Fill of the shape → DrawingML XML (`<a:noFill/>`, solid, grad, blip).
fn fill_xml(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, is_background: bool) -> String {
    let props = &sh.props;
    // placeholder and text box (msosptTextBox, 202) without fFilled in the chain (shape →
    // master) come out WITHOUT fill in the reference application, despite the format's "filled"
    // default (measured in the corpus); the same for the line
    if !is_background && !fill_on(sh, pc) {
        return "<a:noFill/>".to_owned();
    }
    let ty = prop(props, 0x180).unwrap_or(0);
    let color = oa_color(prop(props, 0x181).unwrap_or(0xff_ffff), props, pc, 0);
    let opacity = fixed(prop(props, 0x182), 1.0);
    let back = oa_color(prop(props, 0x183).unwrap_or(0xff_ffff), props, pc, 0);
    let back_opacity = fixed(prop(props, 0x184), 1.0);
    match ty {
        2 | 3 => {
            let pib = prop(props, 0x186);
            // COMPLEX fillBlip (OpenOffice writes the image inside the shape, without a BStore)
            let complex = props.get(0x186).and_then(|p| p.complex);
            let rid = if let Some(c) = complex {
                complex_image_rel(pc, c)
            } else {
                match pib {
                    Some(p) if p != 0 => image_rel(pc, p),
                    _ => None,
                }
            };
            let Some(rid) = rid else { return el("a:solidFill", vec![], &srgb(&color, Some(opacity))) };
            // fRecolorFillAsPicture: the image in two colours (dark = fill, light = background) — the
            // application converts it to duotone; in the slide BACKGROUND the default fill is the
            // scheme's background colour (bg1 in the application's conversion) (only without
            // fillBackColor and without fillBlipFlags written: in the 13 backgrounds of the corpus the
            // application matches exactly like that)
            let recolor = bool_prop(props, 0x1bf, 6, false) && prop(props, 0x183).is_none() && prop(props, 0x188).is_none();
            let duo = if recolor {
                let dflt = pc.conv.doc.drawing_defaults.value(0x181);
                let c1 = oa_color(
                    prop(props, 0x181).unwrap_or(if is_background { 0x0800_0000 } else { dflt.unwrap_or(0xff_ffff) }),
                    props,
                    pc,
                    0,
                );
                let c2 = oa_color(prop(props, 0x183).unwrap_or(0xff_ffff), props, pc, 0);
                el("a:duotone", vec![], &(srgb(&c1, None) + &srgb(&c2, None)))
            } else {
                String::new()
            };
            // table cell: the tile starts at the origin of the TABLE (negative offset of the cell)
            let (tx, ty2) = match (pc.tile_origin, sh.anchor) {
                (Some((ox, oy)), Some(a)) => (emu(ox - f64::from(a.l.min(a.r))), emu(oy - f64::from(a.t.min(a.b)))),
                _ => (0.0, 0.0),
            };
            let tile = if ty == 2 {
                format!(
                    "<a:tile tx=\"{}\" ty=\"{}\" sx=\"100000\" sy=\"100000\" flip=\"none\" algn=\"tl\"/>",
                    to_js_string(tx),
                    to_js_string(ty2)
                )
            } else {
                "<a:stretch><a:fillRect/></a:stretch>".to_owned()
            };
            el(
                "a:blipFill",
                vec![("dpi", n(0)), ("rotWithShape", n(1))],
                &(el("a:blip", vec![("r:embed", s(rid))], &duo) + "<a:srcRect/>" + &tile),
            )
        }
        4..=8 => {
            if ty == 4 || ty == 7 || ty == 8 {
                // [MS-ODRAW] fillAngle θ → DrawingML ang = 90° − θ, with θ missing/0 worth −180°
                // (matched to PowerPoint's own conversion in ~13 thousand gradients of
                // shapes/backgrounds of the corpus)
                let raw = if prop(props, 0x18b).is_none() { 0.0 } else { fixed(prop(props, 0x18b), 0.0) };
                let theta = if raw == 0.0 { -180.0 } else { raw };
                let ang = (((90.0 - theta) % 360.0) + 360.0) % 360.0;
                let gs = gradient_stops(props, pc, &color, &back, Some(opacity), Some(back_opacity), raw != 0.0);
                return el(
                    "a:gradFill",
                    vec![("flip", s("none")), ("rotWithShape", n(1))],
                    &(gs + &el("a:lin", vec![("ang", n(round(ang * 60000.0) % 21_600_000.0)), ("scaled", n(u32::from(ty == 7)))], "")),
                );
            }
            // radial (shadeCenter/shadeShape): background colour in the focus (fillTo*), fill colour at the edge
            let gs = gradient_stops(props, pc, &back, &color, Some(back_opacity), Some(opacity), false);
            let l = fixed(prop(props, 0x18d), 0.0);
            let t = fixed(prop(props, 0x18e), 0.0);
            let r = fixed(prop(props, 0x18f), 0.0);
            let bb = fixed(prop(props, 0x190), 0.0);
            el(
                "a:gradFill",
                vec![("flip", s("none")), ("rotWithShape", n(1))],
                &(gs + &el(
                    "a:path",
                    vec![("path", s(if ty == 6 { "shape" } else { "rect" }))],
                    &el(
                        "a:fillToRect",
                        vec![
                            ("l", n(round(l * 100_000.0))),
                            ("t", n(round(t * 100_000.0))),
                            ("r", n(round((1.0 - r) * 100_000.0))),
                            ("b", n(round((1.0 - bb) * 100_000.0))),
                        ],
                        "",
                    ),
                )),
            )
        }
        9 => match &pc.bg_color {
            Some(c) if !c.is_empty() => el("a:solidFill", vec![], &srgb(c, None)),
            _ => "<a:noFill/>".to_owned(),
        },
        1 => {
            // pattern (8×8 bitmap) → predefined pattFill; without a match, the foreground colour
            let pib = prop(props, 0x186);
            let prst = match pib {
                Some(p) if p != 0 => pattern_prst(pc, p),
                _ => None,
            };
            if let Some(prst) = prst {
                return el(
                    "a:pattFill",
                    vec![("prst", s(prst))],
                    &(el("a:fgClr", vec![], &srgb(&color, None)) + &el("a:bgClr", vec![], &srgb(&back, None))),
                );
            }
            el("a:solidFill", vec![], &srgb(&color, Some(opacity)))
        }
        _ => el("a:solidFill", vec![], &srgb(&color, Some(opacity))),
    }
}

const DASH: [&str; 11] =
    ["solid", "sysDash", "sysDot", "sysDashDot", "sysDashDotDot", "dot", "dash", "lgDash", "dashDot", "lgDashDot", "lgDashDotDot"];
const ARROW: [&str; 6] = ["none", "triangle", "stealth", "diamond", "oval", "arrow"];
const ARROW_SZ: [&str; 3] = ["sm", "med", "lg"];

fn arrow_end(tag: &str, props: &PropTable<'_>, type_id: u16, w_id: u16, len_id: u16) -> String {
    let t = prop(props, type_id).unwrap_or(0);
    if t == 0 {
        return String::new();
    }
    let w = prop(props, w_id);
    let len = prop(props, len_id);
    let sz = |v: Option<u32>| -> Option<V> {
        match v {
            Some(x) if x != 1 => usize::try_from(x).ok().and_then(|i| ARROW_SZ.get(i)).and_then(|y| s(*y)),
            _ => None,
        }
    };
    let ty = usize::try_from(t).ok().and_then(|i| ARROW.get(i)).copied().unwrap_or("triangle");
    el(tag, vec![("type", s(ty)), ("w", sz(w)), ("len", sz(len))], "")
}

/// fFilled with the DEFAULT measured in the reference application (the application's own
/// .ppt→.pptx conversion of ~240 real decks): without fFilled in the shape (nor in the master) a
/// placeholder is empty; with a fillColor DIFFERENT from the document's default (FOPT of the Dgg)
/// the shape is filled and with the colour equal to the default, empty; without a colour, a text
/// box follows the document's default and an autoshape is empty.
fn fill_on(sh: &OaShape<'_>, pc: &PartCtx<'_, '_>) -> bool {
    if let Some(own) = bool_prop_opt(&sh.props, 0x1bf, 4) {
        return own;
    }
    if placeholder_of(sh).is_some() {
        return false;
    }
    let fc = prop(&sh.props, 0x181);
    let defs = &pc.conv.doc.drawing_defaults;
    let dfc = defs.value(0x181);
    // own colour ≠ the document's default and ≠ the "fill" colour of the scheme (0x08000004):
    // 99.5% in the ~3.6 thousand pictures and 99.0% in the ~35 thousand autoshapes converted by the
    // application
    if sh.shape_type == 75 {
        return fc.is_some() && fc != dfc && fc != Some(0x0800_0004);
    }
    if fc.is_some() {
        // in the MASTER, the colour equal to the default follows the document's default fFilled
        // (99.5% in ~2.4 thousand master/layout shapes converted by the application)
        if pc.is_master && fc == dfc {
            return bool_prop_opt(defs, 0x1bf, 4).unwrap_or(true);
        }
        return fc != dfc && fc != Some(0x0800_0004);
    }
    if sh.shape_type == 202 {
        return bool_prop_opt(defs, 0x1bf, 4).unwrap_or(true);
    }
    false
}

/// Default of fLine (the same measurement): lines/connectors with an outline; text box and
/// placeholder without; autoshape with an outline if the colour differs from the document's default
/// or the width was given, otherwise the document's default.
fn line_default(sh: &OaShape<'_>, pc: &PartCtx<'_, '_>) -> bool {
    if placeholder_of(sh).is_some() || sh.shape_type == 202 {
        return false;
    }
    if sh.shape_type == 20 || msospt::is_connector_type(sh.shape_type) {
        return true;
    }
    let defs = &pc.conv.doc.drawing_defaults;
    let lc = prop(&sh.props, 0x1c0);
    if (lc.is_some() && lc != defs.value(0x1c0)) || prop(&sh.props, 0x1cb).is_some() {
        return true;
    }
    bool_prop_opt(defs, 0x1ff, 3).unwrap_or(true)
}

/// Outline → `<a:ln>`.
fn line_xml(sh: &OaShape<'_>, pc: &PartCtx<'_, '_>, dflt_on: bool) -> String {
    let props = &sh.props;
    if !bool_prop(props, 0x1ff, 3, dflt_on) {
        return "<a:ln><a:noFill/></a:ln>".to_owned();
    }
    let w = prop(props, 0x1cb).unwrap_or(9525);
    let color = oa_color(prop(props, 0x1c0).unwrap_or(0), props, pc, 0);
    let alpha = fixed(prop(props, 0x1c1), 1.0);
    let dash = prop(props, 0x1ce).unwrap_or(0);
    let cap = prop(props, 0x1d7);
    let cmpd = prop(props, 0x1cd);
    let join = prop(props, 0x1d6);
    let mut kids = el("a:solidFill", vec![], &srgb(&color, Some(alpha)));
    kids.push_str(&el(
        "a:prstDash",
        vec![("val", s(usize::try_from(dash).ok().and_then(|i| DASH.get(i)).copied().unwrap_or("solid")))],
        "",
    ));
    match join {
        Some(0) => kids.push_str("<a:bevel/>"),
        Some(1) => kids.push_str("<a:miter lim=\"800000\"/>"),
        _ => kids.push_str("<a:round/>"),
    }
    kids.push_str(&arrow_end("a:headEnd", props, 0x1d0, 0x1d2, 0x1d3));
    kids.push_str(&arrow_end("a:tailEnd", props, 0x1d1, 0x1d4, 0x1d5));
    let cmpd_v = match cmpd {
        Some(c) if c != 0 => {
            usize::try_from(c).ok().and_then(|i| ["sng", "dbl", "thickThin", "thinThick", "tri"].get(i)).and_then(|x| s(*x))
        }
        _ => None,
    };
    el(
        "a:ln",
        vec![
            ("w", n(w)),
            (
                "cap",
                match cap {
                    Some(0) => s("rnd"),
                    Some(1) => s("sq"),
                    _ => None,
                },
            ),
            ("cmpd", cmpd_v),
        ],
        &kids,
    )
}

/// Legacy 3D (f3D, 0x2BF bit 3) → `<a:scene3d>` + `<a:sp3d>`, like the application's own
/// conversion (oracle/ppt2pptx): depth = c3DExtrudeBackward (0x285; default 36 pt) − 27000 EMU of
/// the two "angle" bevels of 13500; rotation lat = −rotX (0x2C1), lon = rotY (0x2C0); oblique camera
/// without a Z viewpoint (0x2CB) and perspective with it, in the direction of the skew angle (0x2D0,
/// default −135° = up and right; amount 0x2D1 = 0 → front); the extrusion colour = 0x287.
fn three_d_xml(sh: &OaShape<'_>, pc: &PartCtx<'_, '_>, word_art: bool) -> String {
    let props = &sh.props;
    if !bool_prop(props, 0x2bf, 3, false) {
        return String::new();
    }
    let depth = f64::from(sprop(props, 0x285).unwrap_or(457_200));
    let ext = max(0.0, depth - 27000.0);
    let deg = |id: u16| fixed(prop(props, id), 0.0);
    let norm = |d: f64| round((((d % 360.0) + 360.0) % 360.0) * 60000.0) % 21_600_000.0;
    let lat = norm(-deg(0x2c1));
    let lon = norm(deg(0x2c0));
    let ang = if prop(props, 0x2d0).is_some() { deg(0x2d0) } else { -135.0 };
    let amount = if prop(props, 0x2d1).is_some() { deg(0x2d1) } else { 50.0 };
    let persp = props.contains(0x2cb);
    let a = ((ang + 180.0) * std::f64::consts::PI) / 180.0;
    let dx = cos(a);
    let dy = -sin(a);
    let dir = if amount == 0.0 {
        "Front".to_owned()
    } else {
        let v = format!(
            "{}{}",
            if dy < -0.38 {
                "Top"
            } else if dy > 0.38 {
                "Bottom"
            } else {
                ""
            },
            if dx < -0.38 {
                "Left"
            } else if dx > 0.38 {
                "Right"
            } else {
                ""
            }
        );
        if v.is_empty() { "Front".to_owned() } else { v }
    };
    let cam = format!("{}{dir}", if persp { "legacyPerspective" } else { "legacyOblique" });
    let rot = if lat != 0.0 || lon != 0.0 { el("a:rot", vec![("lat", n(lat)), ("lon", n(lon)), ("rev", n(0))], "") } else { String::new() };
    let ec = prop(props, 0x287);
    let clr = ec.map_or(String::new(), |e| srgb(&oa_color(e, props, pc, 0), None));
    let bev =
        if word_art { "" } else { "<a:bevelT w=\"13500\" h=\"13500\" prst=\"angle\"/><a:bevelB w=\"13500\" h=\"13500\" prst=\"angle\"/>" };
    let ext_clr = if clr.is_empty() { String::new() } else { el("a:extrusionClr", vec![], &clr) + &el("a:contourClr", vec![], &clr) };
    el("a:scene3d", vec![], &(el("a:camera", vec![("prst", s(cam))], &rot) + "<a:lightRig rig=\"legacyFlat3\" dir=\"b\"/>"))
        + &el("a:sp3d", vec![("extrusionH", n(ext)), ("prstMaterial", s("legacyMatte"))], &(bev.to_owned() + &ext_clr))
}

/// Shadow (fShadow) → `<a:effectLst><a:outerShdw>`.
fn shadow_xml(sh: &OaShape<'_>, pc: &PartCtx<'_, '_>) -> String {
    let props = &sh.props;
    if !bool_prop(props, 0x23f, 1, false) {
        return String::new();
    }
    let color = oa_color(prop(props, 0x201).unwrap_or(0x80_8080), props, pc, 0);
    let alpha = fixed(prop(props, 0x204), 1.0);
    let ox = f64::from(sprop(props, 0x205).unwrap_or(25400));
    let oy = f64::from(sprop(props, 0x206).unwrap_or(25400));
    let dist = round(hypot(ox, oy));
    let mut dir = round(((atan2(oy, ox) * 180.0) / std::f64::consts::PI) * 60000.0);
    if dir < 0.0 {
        dir += 21_600_000.0;
    }
    let blur = prop(props, 0x21c).unwrap_or(0);
    // origin (shadowOriginX/Y: −0.5..0.5) → alignment
    let oxr = fixed(prop(props, 0x210), 0.0);
    let oyr = fixed(prop(props, 0x211), 0.0);
    let algn = format!(
        "{}{}",
        if oyr < -0.25 {
            "t"
        } else if oyr > 0.25 {
            "b"
        } else {
            ""
        },
        if oxr < -0.25 {
            "l"
        } else if oxr > 0.25 {
            "r"
        } else {
            ""
        }
    );
    let sx = prop(props, 0x209);
    let sy = prop(props, 0x20c);
    let scale = |v: Option<u32>| -> Option<V> {
        match v {
            Some(x) if x != 0x10000 => n(round(fixed(Some(x), 0.0) * 100_000.0)),
            _ => None,
        }
    };
    el(
        "a:effectLst",
        vec![],
        &el(
            "a:outerShdw",
            vec![
                ("blurRad", if blur != 0 { n(blur) } else { None }),
                ("dist", n(dist)),
                ("dir", n(dir)),
                ("sx", scale(sx)),
                ("sy", scale(sy)),
                ("algn", s(if algn.is_empty() { "ctr".to_owned() } else { algn })),
                ("rotWithShape", n(0)),
            ],
            &srgb(&color, Some(alpha)),
        ),
    )
}

// ───────────────────────────── geometry ─────────────────────────────

/// A position: anchor + rotation/flip (`Xf`), in EMU.
#[derive(Debug, Clone, Copy)]
struct Xf {
    x: f64,
    y: f64,
    cx: f64,
    cy: f64,
    rot: f64,
    flip_h: bool,
    flip_v: bool,
}

/// Rotation (degrees, normalised 0..360) of the FOPT 0x0004 (16.16).
fn rotation_deg(sh: &OaShape<'_>) -> f64 {
    let r = fixed(prop(&sh.props, 0x0004), 0.0);
    ((r % 360.0) + 360.0) % 360.0
}

/// Anchor → xfrm. In the binary, shapes rotated between 45° and 135° (and 225°–315°) keep the
/// anchor already "lying" (width/height swapped around the centre).
fn anchor_xf(sh: &OaShape<'_>) -> Option<Xf> {
    let a: Rect = sh.anchor?;
    let (l, t, r, bt) = (f64::from(a.l), f64::from(a.t), f64::from(a.r), f64::from(a.b));
    let mut x = min(l, r);
    let mut y = min(t, bt);
    let mut w = abs(r - l);
    let mut h = abs(bt - t);
    let rot = rotation_deg(sh);
    if (45.0..135.0).contains(&rot) || (225.0..315.0).contains(&rot) {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        std::mem::swap(&mut w, &mut h);
        x = cx - w / 2.0;
        y = cy - h / 2.0;
    }
    Some(Xf { x: emu(x), y: emu(y), cx: emu(w), cy: emu(h), rot, flip_h: sh.flags & fsp::FLIP_H != 0, flip_v: sh.flags & fsp::FLIP_V != 0 })
}

fn xfrm_xml(xf: &Xf, tag: &str, inner: &str) -> String {
    let rot = round(xf.rot * 60000.0) % 21_600_000.0;
    el(
        tag,
        vec![("rot", nz(rot)), ("flipH", if xf.flip_h { b(true) } else { None }), ("flipV", if xf.flip_v { b(true) } else { None })],
        &(el("a:off", vec![("x", n(xf.x)), ("y", n(xf.y))], "") + &el("a:ext", vec![("cx", n(xf.cx)), ("cy", n(xf.cy))], "") + inner),
    )
}

/// Free geometry (pVertices/pSegmentInfo) → `<a:custGeom>`.
fn cust_geom_xml(sh: &OaShape<'_>) -> Option<String> {
    let props = &sh.props;
    let verts = mso_array(props.get(0x145))?;
    if verts.n == 0 {
        return None;
    }
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for i in 0..verts.n {
        let o = verts.off + i * verts.size;
        if verts.size >= 8 {
            pts.push((f64::from(i32_at(verts.data, o)), f64::from(i32_at(verts.data, o + 4))));
        } else {
            pts.push((f64::from(i16_at(verts.data, o)), f64::from(i16_at(verts.data, o + 2))));
        }
    }
    let gl = f64::from(sprop(props, 0x140).unwrap_or(0));
    let gt = f64::from(sprop(props, 0x141).unwrap_or(0));
    let gr = f64::from(sprop(props, 0x142).unwrap_or(21600));
    let gb = f64::from(sprop(props, 0x143).unwrap_or(21600));
    let w = max(1.0, gr - gl);
    let h = max(1.0, gb - gt);
    let pt = |p: Option<&(f64, f64)>| -> String {
        let (px, py) = p.copied().unwrap_or((0.0, 0.0));
        el("a:pt", vec![("x", n(round(px - gl))), ("y", n(round(py - gt)))], "")
    };
    let segs = mso_array(props.get(0x146));
    let mut paths: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut no_fill = false;
    let mut no_line = false;
    let mut end_path = |cur: &mut String, no_fill: &mut bool, no_line: &mut bool| {
        if !cur.is_empty() {
            paths.push(el(
                "a:path",
                vec![
                    ("w", n(w)),
                    ("h", n(h)),
                    ("fill", if *no_fill { s("none") } else { None }),
                    ("stroke", if *no_line { s("0") } else { None }),
                ],
                cur,
            ));
        }
        cur.clear();
        *no_fill = false;
        *no_line = false;
    };
    let mut pi: usize = 0;
    match segs {
        Some(sg) if sg.n != 0 => {
            // segments consume vertices; corrupt counts stop when the points run out
            let mut i = 0u64;
            while i < sg.n && pi <= pts.len() {
                let o = sg.off + i * sg.size;
                i += 1;
                let sv = if sg.size >= 4 { u32_at(sg.data, o) } else { u32::from(u16_at(sg.data, o)) };
                let (ty, count) = if sg.size >= 4 { ((sv >> 29) & 7, sv & 0x1fff_ffff) } else { ((sv >> 13) & 7, sv & 0x1fff) };
                match ty {
                    0 => {
                        // lineTo
                        let mut k = 0u32;
                        while k < count.max(1) && pi < pts.len() {
                            cur.push_str(&el("a:lnTo", vec![], &pt(pts.get(pi))));
                            pi += 1;
                            k += 1;
                        }
                    }
                    1 => {
                        // curveTo (3 points per segment)
                        let mut k = 0u32;
                        while k < count.max(1) && pi + 2 < pts.len() {
                            cur.push_str(&el("a:cubicBezTo", vec![], &(pt(pts.get(pi)) + &pt(pts.get(pi + 1)) + &pt(pts.get(pi + 2)))));
                            pi += 3;
                            k += 1;
                        }
                    }
                    2 => {
                        cur.push_str(&el("a:moveTo", vec![], &pt(pts.get(pi))));
                        pi += 1;
                    }
                    3 => cur.push_str("<a:close/>"),
                    4 => end_path(&mut cur, &mut no_fill, &mut no_line),
                    5 => {
                        // escape
                        let code = (sv >> 8) & 0x1f;
                        let nn = sv & 0xff;
                        if code == 10 {
                            no_fill = true;
                        } else if code == 11 {
                            no_line = true;
                        } else {
                            pi += usize::try_from(nn).unwrap_or(0);
                        }
                    }
                    _ => {}
                }
            }
            end_path(&mut cur, &mut no_fill, &mut no_line);
        }
        _ => {
            // no segments: polyline through the vertices (closed for shapePath 1/3)
            cur.push_str(&el("a:moveTo", vec![], &pt(pts.first())));
            pi = 1;
            while pi < pts.len() {
                cur.push_str(&el("a:lnTo", vec![], &pt(pts.get(pi))));
                pi += 1;
            }
            let sp = prop(props, 0x144).unwrap_or(1);
            if sp == 1 || sp == 3 {
                cur.push_str("<a:close/>");
            }
            end_path(&mut cur, &mut no_fill, &mut no_line);
        }
    }
    if paths.is_empty() {
        return None;
    }
    Some(el(
        "a:custGeom",
        vec![],
        &("<a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"l\" t=\"t\" r=\"r\" b=\"b\"/>".to_owned()
            + &el("a:pathLst", vec![], &paths.concat())),
    ))
}

/// Legacy arc (msosptArc): the box is the ARC's + centre (not the whole ellipse's); adjust1/adjust2 =
/// start/end angles (16.16 degrees, clockwise on screen, 270 = top; default 270 → 0). Like the
/// application's own conversion (oracle/ppt2pptx): a custGeom in radius units of 21600 with the
/// outline path (without fill) and the SLICE (arc + centre) that fills.
fn arc_geom_xml(sh: &OaShape<'_>) -> String {
    let props = &sh.props;
    let deg = |v: Option<u32>, d: f64| -> f64 { v.map_or(d, |x| f64::from(i32::from_ne_bytes(x.to_ne_bytes())) / 65536.0) };
    let a0 = deg(props.value(0x147), 270.0);
    let mut a1 = deg(props.value(0x148), 0.0);
    while a1 <= a0 {
        a1 += 360.0;
    }
    while a1 - a0 > 360.0 {
        a1 -= 360.0;
    }
    let pi = std::f64::consts::PI;
    let rr = 21600.0;
    let p = |a: f64| -> (f64, f64) { (rr * cos((a * pi) / 180.0), rr * sin((a * pi) / 180.0)) };
    // box: centre + ends + extremes (0/90/180/270) crossed by the arc
    let mut pts: Vec<(f64, f64)> = vec![(0.0, 0.0), p(a0), p(a1)];
    let mut k = ceil(a0 / 90.0) * 90.0;
    while k < a1 {
        pts.push(p(k));
        k += 90.0;
    }
    let xs: Vec<f64> = pts.iter().map(|q| q.0).collect();
    let ys: Vec<f64> = pts.iter().map(|q| q.1).collect();
    let min_x = min_n(&xs);
    let min_y = min_n(&ys);
    let w = max(1.0, round(max_n(&xs) - min_x));
    let h = max(1.0, round(max_n(&ys) - min_y));
    let q = |pt: (f64, f64)| -> String {
        format!("<a:pt x=\"{}\" y=\"{}\"/>", to_js_string(round(pt.0 - min_x)), to_js_string(round(pt.1 - min_y)))
    };
    // arc in cubics of up to 90°
    let nn = max(1.0, ceil((a1 - a0) / 90.0));
    let step = (a1 - a0) / nn;
    let kk = (4.0 / 3.0) * tan((step * pi) / 720.0);
    let mut arc = format!("<a:moveTo>{}</a:moveTo>", q(p(a0)));
    let mut i = 0.0;
    while i < nn {
        let st = ((a0 + i * step) * pi) / 180.0;
        let e = ((a0 + (i + 1.0) * step) * pi) / 180.0;
        let c1 = (rr * (cos(st) - kk * sin(st)), rr * (sin(st) + kk * cos(st)));
        let c2 = (rr * (cos(e) + kk * sin(e)), rr * (sin(e) - kk * cos(e)));
        arc.push_str(&format!("<a:cubicBezTo>{}{}{}</a:cubicBezTo>", q(c1), q(c2), q((rr * cos(e), rr * sin(e)))));
        i += 1.0;
    }
    let paths = el("a:path", vec![("w", n(w)), ("h", n(h)), ("fill", s("none")), ("extrusionOk", n(0))], &arc)
        + &el(
            "a:path",
            vec![("w", n(w)), ("h", n(h)), ("stroke", n(0)), ("extrusionOk", n(0))],
            &format!("{arc}<a:lnTo>{}</a:lnTo><a:close/>", q((0.0, 0.0))),
        );
    el(
        "a:custGeom",
        vec![],
        &("<a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"l\" t=\"t\" r=\"r\" b=\"b\"/>".to_owned()
            + &el("a:pathLst", vec![], &paths)),
    )
}

/// Geometry of the shape: preset (with avLst) or custGeom; gives the extra flipV.
fn geometry_xml(sh: &OaShape<'_>, xf: Option<&Xf>) -> (String, bool) {
    if sh.shape_type == 19 {
        return (arc_geom_xml(sh), false);
    }
    if (sh.shape_type == 0 || msospt::preset_for(sh.shape_type, &[], 1.0, 1.0, PresetVariant::WordSlides).is_none())
        && let Some(cg) = cust_geom_xml(sh)
    {
        return (cg, false);
    }
    let cx = xf.map_or(0.0, |x| x.cx);
    let cy = xf.map_or(0.0, |x| x.cy);
    let raw = msospt::raw_adjust(&sh.props);
    let w = if cx == 0.0 || cx.is_nan() { 1.0 } else { cx };
    let h = if cy == 0.0 || cy.is_nan() { 1.0 } else { cy };
    match msospt::preset_for(sh.shape_type, &raw, w, h, PresetVariant::WordSlides) {
        Some(pr) => {
            let av: String =
                pr.av().map(|g| el("a:gd", vec![("name", s(g.name)), ("fmla", s(format!("val {}", to_js_string(g.val))))], "")).collect();
            (el("a:prstGeom", vec![("prst", s(pr.prst))], &el("a:avLst", vec![], &av)), pr.flip_v)
        }
        None => (el("a:prstGeom", vec![("prst", s("rect"))], &el("a:avLst", vec![], "")), false),
    }
}

// ───────────────────────────── text ─────────────────────────────

/// `<a:bodyPr>` from the text properties of the FOPT.
fn body_pr_xml(sh: &OaShape<'_>) -> String {
    let props = &sh.props;
    let (anchor, anchor_ctr) = match prop(props, 0x87).unwrap_or(0) {
        1 => ("ctr", false),
        2 => ("b", false),
        3 => ("t", true),
        4 => ("ctr", true),
        5 => ("b", true),
        7 => ("b", false),
        8 => ("t", true),
        9 => ("b", true),
        _ => ("t", false),
    };
    let flow = prop(props, 0x88).unwrap_or(0);
    let vert = if flow == 1 || flow == 3 || flow == 5 {
        s("vert")
    } else if flow == 2 {
        s("vert270")
    } else {
        None
    };
    let fit = bool_prop(props, 0xbf, 1, false);
    el(
        "a:bodyPr",
        vec![
            ("vert", vert),
            ("wrap", s(if prop(props, 0x85).unwrap_or(0) == 2 { "none" } else { "square" })),
            ("lIns", n(sprop(props, 0x81).unwrap_or(91440))),
            ("tIns", n(sprop(props, 0x82).unwrap_or(45720))),
            ("rIns", n(sprop(props, 0x83).unwrap_or(91440))),
            ("bIns", n(sprop(props, 0x84).unwrap_or(45720))),
            ("anchor", s(anchor)),
            ("anchorCtr", if anchor_ctr { b(true) } else { None }),
            ("rtlCol", n(0)),
        ],
        if fit { "<a:spAutoFit/>" } else { "<a:noAutofit/>" },
    )
}

/// The text block of the shape (ClientTextbox or the slide's outline) + the PP9 extension.
fn text_block_of(sh: &OaShape<'_>, pc: &PartCtx<'_, '_>) -> Option<TextBlock> {
    let ct = read_client_textbox(sh.client_textbox.as_ref()?);
    let outl = ct.outline_ref.and_then(|r| pc.outline.get(usize::try_from(r).ok()?)).map(|b| (**b).clone());
    let block = match ct.block {
        Some(bk) => bk,
        None => match (outl, ct.ruler) {
            (Some(o), Some(r)) => TextBlock { ruler: Some(Rc::new(r)), ..o },
            (Some(o), None) => o,
            (None, _) => return None,
        },
    };
    // StyleTextProp9Atom in the tag "___PPT9" of the ClientData; the outline text uses the
    // document's OutlineTextProps9 (slideId + text type)
    let pf9 = sh.client_data.as_ref().and_then(pp9_of).map(Rc::new).or_else(|| match (ct.outline_ref, pc.slide_id) {
        (Some(_), Some(sid)) => pc.conv.doc.outline_pf9.get(&format!("{sid}:{}", block.text_type)).cloned(),
        _ => None,
    });
    Some(match pf9 {
        Some(p) => TextBlock { pf9: Some(p), ..block },
        None => block,
    })
}

fn pp9_of(client_data: &Rec<'_>) -> Option<Vec<super::text::TextPF>> {
    for c in children(client_data) {
        if c.rec_type != rt::PROG_TAGS {
            continue;
        }
        for t in children(&c) {
            if t.rec_type != rt::PROG_BINARY_TAG {
                continue;
            }
            let kids = children(&t);
            let name = kids.iter().find(|k| k.rec_type == rt::CSTRING);
            let blob = kids.iter().find(|k| k.rec_type == rt::BINARY_TAG_DATA_BLOB);
            let (Some(name), Some(blob)) = (name, blob) else { continue };
            if utf16(name.data, name.start, name.end) != "___PPT9" {
                continue;
            }
            let mut o = blob.start;
            while o + 8 <= blob.end {
                let Some(rec) = read_rec(blob.data, o, blob.end) else { break };
                if rec.rec_type == rt::STYLE_TEXT_PROP9_ATOM {
                    return Some(read_style_text_prop9(&rec));
                }
                o = rec.start + u64::from(rec.len);
            }
        }
    }
    None
}

// ───────────────────────────── placeholders ─────────────────────────────

/// PlaceholderEnum → ST_PlaceholderType (`None` = no `<p:ph>`).
fn ph_type(id: u8) -> Option<&'static str> {
    Some(match id {
        1 => "title",
        2 => "body",
        3 => "ctrTitle",
        4 => "subTitle",
        5 => "sldImg",
        6 => "body",
        7 => "dt",
        8 => "sldNum",
        9 => "ftr",
        10 => "hdr",
        11 => "sldImg",
        12 => "body",
        13 => "title",
        14 => "body",
        15 => "ctrTitle",
        16 => "subTitle",
        17 => "title",
        18 => "body",
        19 => "obj",
        20 => "chart",
        21 => "tbl",
        22 => "clipArt",
        23 => "dgm",
        24 => "media",
        25 => "obj",
        26 => "pic",
        _ => return None,
    })
}

struct PhInfo {
    type_: &'static str,
    idx: Option<i32>,
    vert: bool,
}

/// OEPlaceholderAtom (position, placementId, size) → type/idx of the `<p:ph>`.
fn placeholder_of(sh: &OaShape<'_>) -> Option<PhInfo> {
    let cd = sh.client_data.as_ref()?;
    for c in children(cd) {
        if c.rec_type != rt::PLACEHOLDER_ATOM {
            continue;
        }
        let position = c.i32(c.start);
        let id = c.u8(c.start + 4);
        let type_ = ph_type(id)?;
        let idx = if type_ != "title" && type_ != "ctrTitle" && position >= 0 { Some(position) } else { None };
        return Some(PhInfo { type_, idx, vert: id == 17 || id == 18 || id == 25 });
    }
    None
}

/// The placeholder type (ST_PlaceholderType) of the shape, if it is a placeholder.
#[must_use]
pub fn placeholder_type(sh: &OaShape<'_>) -> Option<&'static str> {
    placeholder_of(sh).map(|p| p.type_)
}

fn ph_xml(ph: Option<&PhInfo>) -> String {
    let Some(ph) = ph else { return "<p:nvPr/>".to_owned() };
    el(
        "p:nvPr",
        vec![],
        &el(
            "p:ph",
            vec![
                ("type", if ph.type_ == "obj" { None } else { s(ph.type_) }),
                ("orient", if ph.vert { s("vert") } else { None }),
                ("idx", no(ph.idx.map(f64::from))),
            ],
            "",
        ),
    )
}

// ───────────────────────────── shapes ─────────────────────────────

fn shape_name(sh: &OaShape<'_>, fallback: &str) -> String {
    prop_string(sh.props.get(0x380)).unwrap_or_else(|| format!("{fallback} {}", sh.spid))
}

fn shape_id(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>) -> f64 {
    let mut id = if sh.spid > 0 { i64::from(sh.spid) } else { 2 };
    while pc.used_ids.contains(&id) {
        id += 1;
    }
    pc.used_ids.insert(id);
    if id == i64::from(sh.spid) {
        pc.spids.insert(sh.spid);
    }
    // original OOXML id (round trip): the animations kept as XML use that id
    let orig = metro_c_nv_pr_id(sh, pc);
    #[allow(clippy::cast_precision_loss)]
    let idf = id as f64;
    if let Some(o) = orig {
        pc.ooxml_ids.entry(o.to_bits()).or_insert(idf);
    }
    idf
}

/// `/shapeId="(\d+)"/` first match.
fn find_shape_id(text: &str) -> Option<String> {
    let key = "shapeId=\"";
    let mut from = 0usize;
    while let Some(k) = text.get(from..).and_then(|r| r.find(key)) {
        let ds = from + k + key.len();
        let nd = text.get(ds..).map_or(0, |r| r.bytes().take_while(u8::is_ascii_digit).count());
        if nd > 0 && text.as_bytes().get(ds + nd) == Some(&b'"') {
            return text.get(ds..ds + nd).map(str::to_owned);
        }
        from = ds;
    }
    None
}

/// Original OOXML id of the shape (round trip): `drs/downrev.xml` (`<a:downRevStg shapeId>` —
/// charts included, which have no shapexml) or the `<p:cNvPr id>` of the shapexml.
fn metro_c_nv_pr_id(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>) -> Option<f64> {
    let blob = sh.props.get(0x3a9).and_then(|p| p.complex);
    if let Some(dr) = pc.conv.rt.zip_part_text(blob, "downrev.xml")
        && let Some(m) = find_shape_id(&dr)
    {
        return Some(parse_int(&m, 10));
    }
    let root = pc.conv.rt.metro_shape(blob)?;
    fn find(x: &XmlElement, depth: u32) -> Option<String> {
        if x.name == "p:cNvPr" {
            return x.attr("id").map(str::to_owned);
        }
        if depth > 3 {
            return None;
        }
        for c in &x.children {
            if let XNode::El(e) = c
                && let Some(v) = find(e, depth + 1)
            {
                return Some(v);
            }
        }
        None
    }
    let v = find(&root, 0);
    let nn = v.map_or(f64::NAN, |x| parse_int(&x, 10));
    if nn.is_finite() { Some(nn) } else { None }
}

// ───────────────────────────── SmartArt (round trip) ─────────────────────────────

const DGM_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

/// Relation type → name of the diagram part.
fn dgm_kind(ty: &str) -> Option<&'static str> {
    if let Some(rest) = ty.strip_prefix(DGM_REL) {
        return match rest {
            "diagramData" => Some("data"),
            "diagramLayout" => Some("layout"),
            "diagramQuickStyle" => Some("quickStyle"),
            "diagramColors" => Some("colors"),
            _ => None,
        };
    }
    if ty == "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing" { Some("drawing") } else { None }
}

fn dgm_ct(kind: &str) -> &'static str {
    match kind {
        "data" => "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml",
        "layout" => "application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml",
        "quickStyle" => "application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml",
        "colors" => "application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml",
        _ => "application/vnd.ms-office.drawingml.diagramDrawing+xml",
    }
}

struct ZipRel {
    id: String,
    type_: String,
    target: String,
    external: bool,
    raw: String,
}

/// ` KEY="([^"]*)"` first match in the tag.
fn attr_in(raw: &str, key: &str) -> Option<String> {
    let pat = format!(" {key}=\"");
    let k = raw.find(&pat)?;
    let vs = k + pat.len();
    let e = raw.get(vs..)?.find('"')?;
    raw.get(vs..vs + e).map(str::to_owned)
}

/// Relations of a part of the inner zip (`folder/_rels/name.rels`).
fn zip_rels(parts: &super::roundtrip::ZipParts, path: &str) -> Vec<ZipRel> {
    let i = path.rfind('/').unwrap_or(0);
    let name = format!("{}/_rels/{}.rels", path.get(..i).unwrap_or(""), path.get(i + 1..).unwrap_or(""));
    let Some(bytes) = parts.get(&name) else { return Vec::new() };
    let text = uo_jsnum::text::decode_utf8(bytes).into_owned();
    let mut out = Vec::new();
    let open = "<Relationship ";
    let mut from = 0usize;
    while let Some(k) = text.get(from..).and_then(|r| r.find(open)) {
        let start = from + k;
        let Some(e) = text.get(start + open.len()..).and_then(|r| r.find('>')) else { break };
        let end = start + open.len() + e + 1;
        from = end;
        let raw = text.get(start..end).unwrap_or("");
        // the tag is copied RAW to the package: it only passes as a well-formed element
        let closed = if raw.ends_with("/>") { raw.to_owned() } else { format!("{}/>", raw.strip_suffix('>').unwrap_or(raw)) };
        if raw.get(1..).is_some_and(|r| r.contains('<')) {
            continue;
        }
        if parse_xml(&closed).is_err() {
            continue;
        }
        let (id, ty, target) = (attr_in(raw, "Id"), attr_in(raw, "Type"), attr_in(raw, "Target"));
        if let (Some(id), Some(ty), Some(target)) = (id, ty, target)
            && !id.is_empty()
            && !ty.is_empty()
            && !target.is_empty()
        {
            out.push(ZipRel {
                id,
                type_: ty,
                target,
                external: attr_in(raw, "TargetMode").as_deref() == Some("External"),
                raw: raw.to_owned(),
            });
        }
    }
    out
}

/// `target` relative to the folder of `from` (paths of the inner zip).
fn zip_resolve(from: &str, target: &str) -> String {
    let mut segs: Vec<&str> = from.split('/').collect();
    segs.pop();
    for sg in target.split('/') {
        if sg == ".." {
            segs.pop();
        } else if !sg.is_empty() && sg != "." {
            segs.push(sg);
        }
    }
    segs.join("/")
}

/// `String.prototype.replace` with a string replacement: expands `$$`, `$&`, `` $` `` and `$'`
/// (a regex with no groups: `$1`.. and `$<` stay literal).
fn js_replace_expand(repl: &str, matched: &str, before: &str, after: &str) -> String {
    let mut out = String::with_capacity(repl.len());
    let mut it = repl.chars().peekable();
    while let Some(c) = it.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match it.peek().copied() {
            Some('$') => {
                out.push('$');
                it.next();
            }
            Some('&') => {
                out.push_str(matched);
                it.next();
            }
            Some('`') => {
                out.push_str(before);
                it.next();
            }
            Some('\'') => {
                out.push_str(after);
                it.next();
            }
            _ => out.push('$'),
        }
    }
    out
}

/// `raw.replace(/ Target="[^"]*"/, ' Target="<new>"')` (the first match, JS replacement rules).
fn replace_target(raw: &str, new_target: &str) -> String {
    let key = " Target=\"";
    let Some(k) = raw.find(key) else { return raw.to_owned() };
    let vs = k + key.len();
    let Some(e) = raw.get(vs..).and_then(|r| r.find('"')) else { return raw.to_owned() };
    let end = vs + e + 1;
    let matched = raw.get(k..end).unwrap_or("");
    let repl = format!(" Target=\"{new_target}\"");
    format!(
        "{}{}{}",
        raw.get(..k).unwrap_or(""),
        js_replace_expand(&repl, matched, raw.get(..k).unwrap_or(""), raw.get(end..).unwrap_or("")),
        raw.get(end..).unwrap_or("")
    )
}

/// Copies a part of the diagram to the package (`dst`) with its relations (pictures of SmartArt
/// with a photo): the internal targets go to ppt/media and the relation ids are kept (the part's
/// XML references them).
fn copy_diagram_part(pc: &mut PartCtx<'_, '_>, parts: &super::roundtrip::ZipParts, src: &str, dst: &str, xml: &str, k: u32) {
    let conv = &mut *pc.conv;
    conv.parts.set(dst.to_owned(), xml.as_bytes().to_vec());
    let rels = zip_rels(parts, src);
    if rels.is_empty() {
        return;
    }
    let items: Vec<String> = rels
        .iter()
        .map(|r| {
            if r.external {
                return r.raw.clone();
            }
            let from = zip_resolve(src, &r.target);
            let Some(bytes) = parts.get(&from) else { return r.raw.clone() };
            let base = from.get(from.rfind('/').map_or(0, |i| i + 1)..).unwrap_or("").to_owned();
            let ext = if base.contains('.') {
                base.get(base.rfind('.').map_or(0, |i| i + 1)..).unwrap_or("").to_lowercase()
            } else {
                "bin".to_owned()
            };
            let media = format!("ppt/media/dgm{k}_{base}");
            conv.parts.set(media, bytes.to_vec());
            if ext != "xml" {
                conv.media_exts.set(ext, ());
            }
            replace_target(&r.raw, &format!("../media/dgm{k}_{}", esc_attr(&base)))
        })
        .collect();
    let i = dst.rfind('/').unwrap_or(0);
    let body: String =
        items.iter().map(|r| if r.ends_with("/>") { r.clone() } else { format!("{}/>", r.strip_suffix('>').unwrap_or(r)) }).collect();
    conv.parts.set(
        format!("{}/_rels/{}.rels", dst.get(..i).unwrap_or(""), dst.get(i + 1..).unwrap_or("")),
        format!("{XML_DECL}<Relationships xmlns=\"{}\">{body}</Relationships>", ns::REL).into_bytes(),
    );
}

/// `xml.replace(/(<dsp:dataModelExt [^>]*relId=")([^"]+)"/, …)` with the new id of the relation.
fn replace_data_model_rel(xml: &str, map: &std::collections::HashMap<String, String>) -> String {
    let open = "<dsp:dataModelExt ";
    let mut from = 0usize;
    while let Some(k) = xml.get(from..).and_then(|r| r.find(open)) {
        let start = from + k;
        let span_from = start + open.len();
        let span_end = xml.get(span_from..).and_then(|r| r.find('>')).map_or(xml.len(), |e| span_from + e);
        let span = xml.get(span_from..span_end).unwrap_or("");
        // greedy `[^>]*`: the LAST `relId="` of the span first
        let mut cands: Vec<usize> = span.match_indices("relId=\"").map(|(i, _)| i).collect();
        cands.reverse();
        for ci in cands {
            let vs = span_from + ci + 7;
            let rest = xml.get(vs..).unwrap_or("");
            let nq = rest.find('"');
            if let Some(nq) = nq
                && nq > 0
            {
                let id = rest.get(..nq).unwrap_or("");
                let new = map.get(id).map_or(id, String::as_str);
                return format!("{}{new}{}", xml.get(..vs).unwrap_or(""), xml.get(vs + nq..).unwrap_or(""));
            }
        }
        from = span_from;
    }
    xml.to_owned()
}

/// SmartArt of the round trip: the .ppt keeps the diagram as an IMAGE (the 97-2003 fallback) and,
/// in the metroBlob, the original graphicFrame with the diagram parts. Rebuilt as a real
/// `<p:graphicFrame>` — as PowerPoint does when it opens the .ppt (including the animation by
/// parts: `<a:dgm id>` = modelId of the drawing's shapes). It only applies if the binary's image
/// still matches the original frame + effect margins (shape moved in an old application = the image
/// stays, which is what the binary shows).
fn diagram_frame_xml(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, xf: &Xf, c_nv_pr: &str) -> Option<String> {
    let dg = metro_diagram(sh.props.get(0x3a9).and_then(|p| p.complex))?;
    let fxfrm = dg.frame.first_child("p:xfrm")?;
    let off = fxfrm.first_child("a:off")?;
    let ext = fxfrm.first_child("a:ext")?;
    let num = |e: &XmlElement, k: &str| -> f64 {
        let v = parse_int(e.attr(k).unwrap_or("0"), 10);
        if v.is_nan() { 0.0 } else { v }
    };
    let (fx, fy, fcx, fcy) = (num(off, "x"), num(off, "y"), num(ext, "cx"), num(ext, "cy"));
    let m = dg.effect.unwrap_or([0.0; 4]);
    let tol = 3200.0;
    if abs(fx + m[0] * fcx - xf.x) > tol
        || abs(fy + m[1] * fcy - xf.y) > tol
        || abs(fx + fcx + m[2] * fcx - (xf.x + xf.cx)) > tol
        || abs(fy + fcy + m[3] * fcy - (xf.y + xf.cy)) > tol
    {
        return None;
    }
    // parts: at least data + drawing (the ready layout that we render)
    struct Found {
        id: String,
        type_: String,
        kind: &'static str,
        src: String,
    }
    let found: Vec<Found> = zip_rels(&dg.parts, "drs/e2oDoc.xml")
        .into_iter()
        .filter(|r| !r.external && dgm_kind(&r.type_).is_some())
        .map(|r| Found {
            kind: dgm_kind(&r.type_).unwrap_or("data"),
            src: zip_resolve("drs/e2oDoc.xml", &r.target),
            id: r.id,
            type_: r.type_,
        })
        .filter(|p| dg.parts.get(&p.src).is_some())
        .collect();
    if !found.iter().any(|p| p.kind == "data") || !found.iter().any(|p| p.kind == "drawing") {
        return None;
    }
    pc.conv.diagrams += 1;
    let k = pc.conv.diagrams;
    let mut new_id: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for p in &found {
        let rid = pc.rels.add(&p.type_, &format!("../diagrams/{}{k}.xml", p.kind), false);
        new_id.insert(p.id.clone(), rid);
    }
    for p in &found {
        let dst = format!("ppt/diagrams/{}{k}.xml", p.kind);
        let mut xml = uo_jsnum::text::decode_utf8(dg.parts.get(&p.src).unwrap_or(&[])).into_owned();
        // data → drawing: `<dsp:dataModelExt relId>` points at the relation of the OWNER part (the slide)
        if p.kind == "data" {
            xml = replace_data_model_rel(&xml, &new_id);
        }
        copy_diagram_part(pc, &dg.parts, &p.src, &dst, &xml, k);
        let ct = dg.types.get(&p.src).cloned().unwrap_or_else(|| dgm_ct(p.kind).to_owned());
        pc.conv.overrides.set(dst, ct);
    }
    let graphic = dg.frame.first_child("a:graphic").unwrap_or(&dg.frame);
    let gdata = graphic.first_child("a:graphicData").unwrap_or(&dg.frame);
    let ids = gdata.first_child("dgm:relIds");
    let rid = |a: &str| -> Option<V> {
        ids.and_then(|i| i.attr(a)).filter(|v| !v.is_empty()).and_then(|v| new_id.get(v)).and_then(|v| s(v.clone()))
    };
    let rel_ids = el(
        "dgm:relIds",
        vec![
            ("xmlns:dgm", s("http://schemas.openxmlformats.org/drawingml/2006/diagram")),
            ("r:dm", rid("r:dm")),
            ("r:lo", rid("r:lo")),
            ("r:qs", rid("r:qs")),
            ("r:cs", rid("r:cs")),
        ],
        "",
    );
    Some(el(
        "p:graphicFrame",
        vec![],
        &(el("p:nvGraphicFramePr", vec![], &format!("{c_nv_pr}<p:cNvGraphicFramePr/><p:nvPr/>"))
            + &serialize_xml(fxfrm)
            + &el(
                "a:graphic",
                vec![],
                &el("a:graphicData", vec![("uri", s("http://schemas.openxmlformats.org/drawingml/2006/diagram"))], &rel_ids),
            )),
    ))
}

/// Most bytes an embedded chart's storage may inflate to.
const MAX_OLE_STORAGE: usize = 64 << 20;

/// The storage of an `ExOleObjStg` (compressed when its instance is 1: the size, then zlib). PowerPoint writes the zlib
/// stream without its end (the last block unterminated, no Adler-32): the data inflated up to the declared size is kept.
fn ole_storage(rec: &Rec<'_>) -> Option<Vec<u8>> {
    let d = super::rd::sub(rec.data, rec.start, rec.end);
    if rec.inst != 1 {
        return Some(d.to_vec());
    }
    let declared = usize::try_from(u32::from_le_bytes([*d.first()?, *d.get(1)?, *d.get(2)?, *d.get(3)?])).ok()?;
    if declared == 0 || declared > MAX_OLE_STORAGE {
        return None;
    }
    let z = d.get(4..)?;
    match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(z, declared) {
        Ok(v) => Some(v),
        Err(e) if e.output.len() == declared => Some(e.output),
        Err(_) => None,
    }
}

/// An embedded `Excel.Chart` OLE object that PowerPoint converts back (ClientData `ExObjRefAtom` → `ExOleEmbedContainer` →
/// `ExOleObjStg`) as a
/// `p:graphicFrame` with a chart part converted from the BIFF8 chart of its `Workbook` stream (`uo_charts::biff`), in the
/// shape's frame; `None` = not a chart (the picture stays).
fn ole_chart_xml(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, xf: &Xf, c_nv_pr: &str) -> Option<String> {
    // only a chart that PowerPoint 2007+ downgraded: the round-trip blob's `drs/downrev.xml` says `convChart="1"` (measured:
    // pb-graficos-partes.ppt opens as msoChart; pb-graficos.ppt, the same OLE storages without that blob, stays an
    // msoEmbeddedOLEObject drawn from its picture)
    if !pc.conv.use_round_trip {
        return None;
    }
    let blob = sh.props.get(0x3a9).and_then(|p| p.complex);
    let downrev = pc.conv.rt.zip_part_text(blob, "drs/downrev.xml")?;
    if !downrev.contains("convChart=\"1\"") {
        return None;
    }
    let cd = sh.client_data.as_ref()?;
    let r = children(cd).into_iter().find(|c| c.rec_type == rt::EX_OBJ_REF_ATOM)?;
    let ole = pc.conv.doc.ole.get(&r.u32(r.start))?;
    if !ole.prog_id.as_deref().is_some_and(|p| p.starts_with("Excel.Chart")) {
        return None;
    }
    let storage = ole_storage(ole.storage.as_ref()?)?;
    let cfb = uo_opc::cfb::CfbFile::open(&storage).ok()?;
    let wb = cfb.stream("Workbook").ok().flatten()?;
    let xml = uo_charts::biff::chart_space_from_workbook(&wb)?;
    pc.conv.charts += 1;
    let k = pc.conv.charts;
    let dst = format!("ppt/charts/chart{k}.xml");
    pc.conv.parts.set(dst.clone(), xml.into_bytes());
    pc.conv.overrides.set(dst, "application/vnd.openxmlformats-officedocument.drawingml.chart+xml".to_owned());
    let rid = pc.rels.add("chart", &format!("../charts/chart{k}.xml"), false);
    Some(el(
        "p:graphicFrame",
        vec![],
        &(el("p:nvGraphicFramePr", vec![], &format!("{c_nv_pr}<p:cNvGraphicFramePr/><p:nvPr/>"))
            + &xfrm_xml(&Xf { rot: 0.0, flip_h: false, flip_v: false, ..*xf }, "p:xfrm", "")
            + &el(
                "a:graphic",
                vec![],
                &el(
                    "a:graphicData",
                    vec![("uri", s("http://schemas.openxmlformats.org/drawingml/2006/chart"))],
                    &el(
                        "c:chart",
                        vec![
                            ("xmlns:c", s("http://schemas.openxmlformats.org/drawingml/2006/chart")),
                            ("xmlns:r", s("http://schemas.openxmlformats.org/officeDocument/2006/relationships")),
                            ("r:id", s(rid)),
                        ],
                        "",
                    ),
                ),
            )),
    ))
}

/// Is it a hidden shape (fHidden in the set 0x3BF)?
fn is_hidden(sh: &OaShape<'_>) -> bool {
    bool_prop(&sh.props, 0x3bf, 1, false)
}

/// Options of [`shape_xml`] (`ShapeOpts`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ShapeOpts {
    pub skip_placeholders: bool,
    pub in_group: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisKind {
    Shape,
    Pic,
    Line,
}

struct Visual {
    geom: String,
    fill: String,
    line: String,
    effects: String,
}

/// Visual parts of the spPr (geometry, fill, line, effects), from the binary and, with round trip,
/// overridden by the original shapexml.
fn visual_parts(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, xf: &mut Xf, kind: VisKind, in_group: bool) -> Visual {
    let (geom_xml, geom_flip) = geometry_xml(sh, Some(xf));
    let mut flip_v = geom_flip;
    let fill = if kind == VisKind::Line { "<a:noFill/>".to_owned() } else { fill_xml(sh, pc, false) };
    let line = line_xml(sh, pc, kind != VisKind::Pic && line_default(sh, pc));
    let effects = shadow_xml(sh, pc) + &(if kind == VisKind::Shape { three_d_xml(sh, pc, false) } else { String::new() });
    let mut parts = Visual { geom: geom_xml, fill, line, effects };
    let mv = if pc.conv.use_round_trip {
        let root = pc.conv.rt.metro_shape(sh.props.get(0x3a9).and_then(|p| p.complex));
        metro_visual(root.as_deref())
    } else {
        None
    };
    if let Some(mv) = mv {
        if let Some(g) = mv.geometry.filter(|g| !g.is_empty()) {
            parts.geom = g;
            flip_v = false;
        }
        if let Some(f) = mv.fill.filter(|f| !f.is_empty())
            && kind != VisKind::Pic
        {
            parts.fill = f;
        }
        if let Some(l) = mv.line.filter(|l| !l.is_empty()) {
            parts.line = l;
        }
        if let Some(e) = mv.effects {
            parts.effects = e;
        }
        // exact position in EMU (the binary rounds to 1/576") when it matches the anchor
        let tol = 1600.0;
        if let Some(m) = mv.xfrm
            && !in_group
            && abs(m.x - xf.x) <= tol
            && abs(m.y - xf.y) <= tol
            && abs(m.cx - xf.cx) <= tol
            && abs(m.cy - xf.cy) <= tol
            && abs(((((m.rot - xf.rot) % 360.0) + 540.0) % 360.0) - 180.0) < 1.0
        {
            *xf = Xf { x: m.x, y: m.y, cx: m.cx, cy: m.cy, rot: m.rot, flip_h: m.flip_h, flip_v: m.flip_v };
            flip_v = false;
        }
    }
    if flip_v {
        xf.flip_v = !xf.flip_v;
    }
    parts
}

/// Picture adjustments of the binary → children of `<a:blip>`, like the application's conversion
/// (measured in ~60 pictures of the corpus): transparent colour (0x107) → clrChange to alpha 0;
/// brightness (0x109, ±32768 = ±100%) and contrast (0x108, 16.16: < 1 → (c−1), > 1 → 1 − 1/c) →
/// `<a:lum>`; grayscale / black and white (0x13F bits 2/1) → grayscl / biLevel.
fn blip_adjust_xml(sh: &OaShape<'_>, pc: &PartCtx<'_, '_>) -> String {
    let props = &sh.props;
    let mut out = String::new();
    if let Some(tr) = prop(props, 0x107) {
        let c = oa_color(tr, props, pc, 0);
        out.push_str(&el(
            "a:clrChange",
            vec![],
            &(el("a:clrFrom", vec![], &srgb(&c, None))
                + &el("a:clrTo", vec![], &el("a:srgbClr", vec![("val", s(c.replace('#', "").to_uppercase()))], "<a:alpha val=\"0\"/>"))),
        ));
    }
    let br = prop(props, 0x109);
    let ct = prop(props, 0x108);
    let bright = br.map_or(0.0, |v| round((f64::from(i32::from_ne_bytes(v.to_ne_bytes())) / 32768.0) * 100.0) * 1000.0);
    let mut contrast = 0.0;
    if let Some(c) = ct {
        let c = f64::from(c) / 65536.0;
        contrast = round((if c < 1.0 { c - 1.0 } else { 1.0 - 1.0 / c }) * 100.0) * 1000.0;
    }
    if truthy(bright) || truthy(contrast) {
        out.push_str(&el("a:lum", vec![("bright", nz(bright)), ("contrast", nz(contrast))], ""));
    }
    if bool_prop(props, 0x13f, 2, false) {
        out.push_str("<a:grayscl/>");
    } else if bool_prop(props, 0x13f, 1, false) {
        out.push_str("<a:biLevel thresh=\"50000\"/>");
    }
    out
}

fn truthy(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

/// One shape (or group) → XML of the spTree. `skip_placeholders` omits placeholders (inherited decorations).
pub fn shape_xml(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, opts: ShapeOpts) -> R {
    if sh.has_flag(fsp::DELETED) {
        return Ok(String::new());
    }
    if is_hidden(sh) {
        return Ok(String::new());
    }
    if sh.children.is_some() {
        return group_xml(sh, pc, opts);
    }
    let ph = placeholder_of(sh);
    if ph.is_some() && opts.skip_placeholders {
        return Ok(String::new());
    }
    let Some(mut xf) = anchor_xf(sh) else { return Ok(String::new()) };
    let pib = prop(&sh.props, 0x104).filter(|&p| p != 0);
    let name = shape_name(sh, if pib.is_some() { "Imagem" } else { "Forma" });
    let descr = prop_string(sh.props.get(0x381));
    let id = shape_id(sh, pc);
    let c_nv_pr = el("p:cNvPr", vec![("id", n(id)), ("name", s(name)), ("descr", so(descr.as_deref()))], "");
    let in_group = opts.in_group;

    // round-trip SmartArt (the binary's image is only the 97-2003 fallback)
    if pc.conv.use_round_trip
        && sh.props.contains(0x3a9)
        && let Some(dgm) = diagram_frame_xml(sh, pc, &xf, &c_nv_pr)
    {
        return Ok(dgm);
    }

    // an embedded Excel chart: PowerPoint opens it as a native chart (the picture is only its 97-2003 fallback)
    if !pc.conv.ts_ppt_charts
        && let Some(frame) = ole_chart_xml(sh, pc, &xf, &c_nv_pr)
    {
        return Ok(frame);
    }

    // picture (PictureFrame or any shape with a pib)
    if let Some(pib) = pib {
        let rid = match recolor_rel(sh, pc, pib)? {
            Some(r) => Some(r),
            None => image_rel(pc, pib),
        };
        if let Some(rid) = rid.filter(|r| !r.is_empty()) {
            let v = visual_parts(sh, pc, &mut xf, VisKind::Pic, in_group);
            let crop = |pid: u16| round(fixed(prop(&sh.props, pid), 0.0) * 100_000.0);
            let (t, bb, l, r) = (crop(0x100), crop(0x101), crop(0x102), crop(0x103));
            let src_rect = if truthy(t) || truthy(bb) || truthy(l) || truthy(r) {
                el("a:srcRect", vec![("l", nz(l)), ("t", nz(t)), ("r", nz(r)), ("b", nz(bb))], "")
            } else {
                String::new()
            };
            let blip_fill = el(
                "p:blipFill",
                vec![],
                &(el("a:blip", vec![("r:embed", s(rid))], &blip_adjust_xml(sh, pc)) + &src_rect + "<a:stretch><a:fillRect/></a:stretch>"),
            );
            let sp_pr = el("p:spPr", vec![], &(xfrm_xml(&xf, "a:xfrm", "") + &v.geom + &v.fill + &v.line + &v.effects));
            return Ok(el(
                "p:pic",
                vec![],
                &(el(
                    "p:nvPicPr",
                    vec![],
                    &(c_nv_pr + &el("p:cNvPicPr", vec![], "<a:picLocks noChangeAspect=\"1\"/>") + &ph_xml(ph.as_ref())),
                ) + &blip_fill
                    + &sp_pr),
            ));
        }
    }

    // classic WordArt (gtext): the fill belongs to the letters, not to a box
    let word_art = if msospt::is_word_art_type(sh.shape_type) { word_art_xml(sh, pc, &xf) } else { None };
    if let Some(wa) = word_art {
        return Ok(el(
            "p:sp",
            vec![],
            &(el("p:nvSpPr", vec![], &(c_nv_pr + "<p:cNvSpPr txBox=\"1\"/>" + &ph_xml(ph.as_ref())))
                + &el(
                    "p:spPr",
                    vec![],
                    &(xfrm_xml(&xf, "a:xfrm", "") + "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln>"),
                )
                + &wa),
        ));
    }

    let block = text_block_of(sh, pc);
    if let Some(bk) = &block {
        pc.texts.insert(sh.spid, Rc::clone(&bk.text));
    }
    let is_conn = msospt::is_connector_type(sh.shape_type);
    let is_line = sh.shape_type == 20 || is_conn;
    let v = visual_parts(sh, pc, &mut xf, if is_line { VisKind::Line } else { VisKind::Shape }, in_group);
    let sp_pr = el("p:spPr", vec![], &(xfrm_xml(&xf, "a:xfrm", "") + &v.geom + &v.fill + &v.line + &v.effects));
    if is_conn {
        return Ok(el("p:cxnSp", vec![], &(el("p:nvCxnSpPr", vec![], &(c_nv_pr + "<p:cNvCxnSpPr/>" + &ph_xml(ph.as_ref()))) + &sp_pr)));
    }
    let mut tx_body = String::new();
    if let Some(bk) = &block {
        #[allow(clippy::cast_precision_loss)]
        let len = bk.text.len() as f64;
        pc.conv.text_budget -= len;
        if pc.conv.text_budget < 0.0 {
            return Err(PptError::Internal("texto convertido excede o limite (blocos repetidos demais)".to_owned()));
        }
        // round trip: formatting of the shapexml + characters of the binary (only outside
        // placeholders — those inherit from the original layout, which we do not reproduce)
        let metro_tx = if pc.conv.use_round_trip && ph.is_none() {
            let root = pc.conv.rt.metro_shape(sh.props.get(0x3a9).and_then(|p| p.complex));
            metro_text_body(root.as_deref(), &bk.text)
        } else {
            None
        };
        tx_body = match metro_tx {
            Some(t) => t,
            None => {
                let paras = {
                    let mut host = PartHost { conv: &mut *pc.conv, rels: &mut pc.rels };
                    let mut h: Option<&mut dyn TextHost> = Some(&mut host);
                    paragraphs_xml(bk, &pc.text, &mut h)
                };
                el("p:txBody", vec![], &(body_pr_xml(sh) + "<a:lstStyle/>" + &paras))
            }
        };
    }
    let c_nv_sp_pr = if sh.shape_type == 202 && ph.is_none() { "<p:cNvSpPr txBox=\"1\"/>" } else { "<p:cNvSpPr/>" };
    Ok(el("p:sp", vec![], &(el("p:nvSpPr", vec![], &(c_nv_pr + c_nv_sp_pr + &ph_xml(ph.as_ref()))) + &sp_pr + &tx_body)))
}

const WARPS: [&str; 40] = [
    "textPlain",
    "textStop",
    "textTriangle",
    "textTriangleInverted",
    "textChevron",
    "textChevronInverted",
    "textRingInside",
    "textRingOutside",
    "textArchUp",
    "textArchDown",
    "textCircle",
    "textButton",
    "textArchUpPour",
    "textArchDownPour",
    "textCirclePour",
    "textButtonPour",
    "textCurveUp",
    "textCurveDown",
    "textCascadeUp",
    "textCascadeDown",
    "textWave1",
    "textWave2",
    "textDoubleWave1",
    "textWave4",
    "textInflate",
    "textDeflate",
    "textInflateBottom",
    "textDeflateBottom",
    "textInflateTop",
    "textDeflateTop",
    "textDeflateInflate",
    "textDeflateInflateDeflate",
    "textFadeRight",
    "textFadeLeft",
    "textFadeUp",
    "textFadeDown",
    "textSlantUp",
    "textSlantDown",
    "textCanUp",
    "textCanDown",
];

/// Presets whose 1st adjust is an ANGLE (16.16 degrees in the binary).
fn warp_angle(prst: &str) -> bool {
    matches!(
        prst,
        "textArchUp"
            | "textArchDown"
            | "textCircle"
            | "textButton"
            | "textArchUpPour"
            | "textArchDownPour"
            | "textCirclePour"
            | "textButtonPour"
    )
}

/// Defaults of the binary that differ from DrawingML's (the application writes the converted value).
fn warp_bin_default(prst: &str) -> Option<f64> {
    match prst {
        "textWave1" | "textWave2" => Some(2809.0),
        "textDoubleWave1" | "textWave4" => Some(1404.0),
        "textInflateBottom" => Some(14706.0),
        _ => None,
    }
}

/// Adjusts of the legacy WordArt → avLst of the prstTxWarp, like the application's own conversion
/// (oracle/ppt2pptx, 60 shapes): linear fraction × 100000/21600; angles 16.16 carried by the box
/// proportion (θ' = atan2(h·sin θ, w·cos θ)); the 2nd adjust of the "pour" (inner radius) ×
/// 200000/21600; the displacement of the waves around 10800.
fn warp_adjust(prst: &str, raw: &[Option<f64>; 10], w: f64, h: f64) -> Vec<f64> {
    let kk = 100_000.0 / 21600.0;
    if warp_angle(prst) {
        let ang = |v: Option<f64>, dflt: f64| -> f64 {
            let Some(v) = v else { return dflt };
            let t = ((v / 65536.0) * std::f64::consts::PI) / 180.0;
            let mut d = (atan2(h * sin(t), w * cos(t)) * 180.0) / std::f64::consts::PI;
            if d < 0.0 {
                d += 360.0;
            }
            round(d * 60000.0) % 21_600_000.0
        };
        let a1 = ang(raw[0], if prst.contains("Down") { 0.0 } else { 10_800_000.0 });
        if !prst.ends_with("Pour") {
            return if raw[0].is_none() { Vec::new() } else { vec![a1] };
        }
        if raw[0].is_none() && raw[1].is_none() {
            return Vec::new();
        }
        let second = match raw[1] {
            None => {
                if prst == "textArchDownPour" {
                    25000.0
                } else {
                    50000.0
                }
            }
            Some(v) => round(v * 2.0 * kk),
        };
        return vec![a1, second];
    }
    let a0 = raw[0].or_else(|| warp_bin_default(prst));
    if prst.contains("Wave") {
        if a0.is_none() && raw[1].is_none() {
            return Vec::new();
        }
        return vec![
            round(a0.unwrap_or(0.0) * kk),
            match raw[1] {
                None => 0.0,
                Some(v) => round((v - 10800.0) * kk),
            },
        ];
    }
    match a0 {
        None => Vec::new(),
        Some(a) => vec![round(a * kk)],
    }
}

/// Text of the classic WordArt (gtextUNICODE/gtextFont/flags) → `<p:txBody>`: centred, without
/// wrapping, in the fill colour. WordArt stretches the text to the box — approximated by a font size
/// that fills the height.
fn word_art_xml(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, xf: &Xf) -> Option<String> {
    let props = &sh.props;
    let text = prop_string(props.get(0xc0))?;
    let font = prop_string(props.get(0xc5));
    let bold = bool_prop(props, 0xff, 5, false);
    let italic = bool_prop(props, 0xff, 4, false);
    let color = oa_color(prop(props, 0x181).unwrap_or(0xff_ffff), props, pc, 0);
    let lines = split_lines(&text);
    // msosptText* (136–175) ↔ prstTxWarp in the SAME order of DrawingML
    let warp = if (136..=175).contains(&sh.shape_type) { WARPS.get(usize::from(sh.shape_type) - 136).copied() } else { None };
    let sz_pt = if warp.is_some() {
        // like the application's conversion: gtextSize (16.16 pt; missing = 36) — the warp stretches the text to the shape
        let v = fixed(prop(props, 0xc3), 36.0);
        if truthy(v) { v } else { 36.0 }
    } else {
        // without a warp: fits the height (lines) and the width (longest line, ~0.6 em per glyph)
        let longest = max(1.0, max_n(&lines.iter().map(|l| len_f(uo_jsnum::text::utf16_len(l))).collect::<Vec<f64>>()));
        let by_h = (xf.cy / 12700.0 / len_f(lines.len())) * 0.75;
        let by_w = (xf.cx / 12700.0) / (longest * if bold { 0.66 } else { 0.6 });
        max(4.0, uo_jsnum::math::min_n(&[400.0, by_h, by_w]))
    };
    // spacing (gtextSpacing, 16.16; 1 = normal) → spc in 1/100 pt, as the application writes
    // (1.2 at 28 pt → 560; 0.8 at 36 pt → −360)
    let sp = if prop(props, 0xc4).is_some() { fixed(prop(props, 0xc4), 0.0) } else { 1.0 };
    let spc = if warp.is_some() && sp != 1.0 { n(round((sp - 1.0) * sz_pt * if sp > 1.0 { 100.0 } else { 50.0 })) } else { None };
    // the LETTERS carry the outline, the fill (solid/gradient/pattern) and the shadow of the shape
    // (like the application's own conversion: <a:ln> + pattFill + outerShdw in the rPr)
    let fx = fill_xml(sh, pc, false);
    let letter_fill = if fx.starts_with("<a:solidFill") || fx.starts_with("<a:gradFill") || fx.starts_with("<a:pattFill") {
        fx
    } else {
        el("a:solidFill", vec![], &srgb(&color, None))
    };
    let font_xml = match &font {
        Some(f) if !f.is_empty() => el("a:latin", vec![("typeface", s(f.clone()))], ""),
        _ => String::new(),
    };
    let r_pr = el(
        "a:rPr",
        vec![("sz", n(round(sz_pt * 100.0))), ("b", n(u32::from(bold))), ("i", n(u32::from(italic))), ("spc", spc)],
        &(line_xml(sh, pc, true) + &letter_fill + &shadow_xml(sh, pc) + &font_xml),
    );
    let paras: String = lines
        .iter()
        .map(|l| format!("<a:p><a:pPr algn=\"ctr\"><a:buNone/></a:pPr><a:r>{r_pr}<a:t>{}</a:t></a:r></a:p>", esc_text(l)))
        .collect();
    let av = match warp {
        Some(w) => warp_adjust(w, &msospt::raw_adjust(props), xf.cx, xf.cy),
        None => Vec::new(),
    };
    let w_xml = match warp {
        Some(w) => {
            let gds: String = av
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let name = if av.len() > 1 { format!("adj{}", i + 1) } else { "adj".to_owned() };
                    el("a:gd", vec![("name", s(name)), ("fmla", s(format!("val {}", to_js_string(*v))))], "")
                })
                .collect();
            el("a:prstTxWarp", vec![("prst", s(w))], &el("a:avLst", vec![], &gds))
        }
        None => String::new(),
    };
    // vertical text (gtextFVertical, bit 13): stacked letters — the application writes vert="wordArtVert"
    let vert = if warp.is_some() && bool_prop(props, 0xff, 13, false) { " vert=\"wordArtVert\"" } else { "" };
    Some(el(
        "p:txBody",
        vec![],
        &(format!(
            "<a:bodyPr wrap=\"none\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"ctr\" rtlCol=\"0\"{vert}{}>{w_xml}<a:noAutofit/></a:bodyPr><a:lstStyle/>",
            if warp.is_some() { " fromWordArt=\"1\"" } else { "" }
        ) + &paras),
    ))
}

fn len_f(v: usize) -> f64 {
    f64::from(u32::try_from(v).unwrap_or(u32::MAX))
}

/// `text.split(/\r\n|\r|\n/)`.
fn split_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            '\n' => out.push(std::mem::take(&mut cur)),
            other => cur.push(other),
        }
    }
    out.push(cur);
    out
}

/// Group: off/ext = the group's anchor; chOff/chExt = FSPGR (same scale).
fn group_xml(sh: &OaShape<'_>, pc: &mut PartCtx<'_, '_>, opts: ShapeOpts) -> R {
    let g: Option<Rect> = sh.group_rect;
    // a group that is a TABLE (tableProperties 0x39F fIsTable / row heights 0x3A0)
    let is_table = (prop(&sh.props, 0x39f).unwrap_or(0) & 1) != 0 || sh.props.contains(0x3a0);
    let prev_tile = pc.tile_origin;
    if is_table && let Some(g) = g {
        pc.tile_origin = Some((f64::from(g.l.min(g.r)), f64::from(g.t.min(g.b))));
    }
    let doc: &PptDocument<'_> = pc.conv.doc;
    let mut kids = String::new();
    for c in doc.shapes.kids(sh) {
        let x = shape_xml(c, pc, ShapeOpts { in_group: true, ..opts });
        match x {
            Ok(v) => kids.push_str(&v),
            Err(e) => {
                pc.tile_origin = prev_tile;
                return Err(e);
            }
        }
    }
    pc.tile_origin = prev_tile;
    if kids.is_empty() {
        return Ok(String::new());
    }
    let xf = anchor_xf(sh);
    let id = shape_id(sh, pc);
    let name = shape_name(sh, "Grupo");
    let mut xfrm = "<a:xfrm/>".to_owned();
    if let (Some(xf), Some(g)) = (xf, g) {
        let (gl, gt, gr, gb) = (f64::from(g.l), f64::from(g.t), f64::from(g.r), f64::from(g.b));
        xfrm = xfrm_xml(
            &xf,
            "a:xfrm",
            &(el("a:chOff", vec![("x", n(emu(min(gl, gr)))), ("y", n(emu(min(gt, gb))))], "")
                + &el("a:chExt", vec![("cx", n(emu(abs(gr - gl)))), ("cy", n(emu(abs(gb - gt))))], "")),
        );
    }
    Ok(el(
        "p:grpSp",
        vec![],
        &(el("p:nvGrpSpPr", vec![], &(el("p:cNvPr", vec![("id", n(id)), ("name", s(name))], "") + "<p:cNvGrpSpPr/><p:nvPr/>"))
            + &el("p:grpSpPr", vec![], &xfrm)
            + &kids),
    ))
}

/// Background (shape fBackground) → `<p:bg>`.
pub fn background_xml(bg: Option<&OaShape<'_>>, pc: &mut PartCtx<'_, '_>) -> String {
    let Some(bg) = bg else { return String::new() };
    let fill = fill_xml(bg, pc, true);
    if fill == "<a:noFill/>" {
        return String::new();
    }
    // background "from the title" (msofillShadeShape in the BACKGROUND): the application writes
    // shadeToTitle (2 of 2 decks of PowerPoint's own conversion)
    let to_title = prop(&bg.props, 0x180).unwrap_or(0) == 6;
    el("p:bg", vec![], &el("p:bgPr", if to_title { vec![("shadeToTitle", n(1))] } else { vec![] }, &(fill + "<a:effectLst/>")))
}

/// Effective solid colour of the background (for fillType "background" of the shapes).
pub fn background_color(bg: Option<&OaShape<'_>>, pc: &PartCtx<'_, '_>) -> Option<String> {
    let bg = bg?;
    let ty = prop(&bg.props, 0x180).unwrap_or(0);
    if ty == 0 { Some(oa_color(prop(&bg.props, 0x181).unwrap_or(0xff_ffff), &bg.props, pc, 0)) } else { None }
}
