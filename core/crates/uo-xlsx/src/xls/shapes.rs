//! The OfficeArt shapes of the `.xls` sheet (rectangles, ellipses, lines, text boxes, freeforms, WordArt, the autoshapes of
//! Excel 2007+ saved in .xls, pictures and groups) → DrawingML objects (`<xdr:sp>`, `<xdr:cxnSp>`, `<xdr:pic>`,
//! `<xdr:grpSp>`), with the text of the TXO (font runs) in the `<xdr:txBody>` (`shapes.ts`).
//!
//! Properties of the FOPT ([MS-ODRAW] 2.3): geometry (0x0140–0x017F), fill (0x0180–0x01BF), line (0x01C0–0x01FF), shadow
//! (0x0200–), text margins/anchor (0x0081–0x0087), rotation (0x0004). OfficeArtCOLORREF colours: RGB, workbook palette index
//! (fSchemeIndex — as Excel writes: 0x08000040 = window text, 0x08000041 = window background) or system colour/the shape's
//! own colour (fSysIndex).
//!
//! Excel 2007+ writes in the metroBlob (0x03A9) the ORIGINAL DrawingML XML of the shape (`drs/shapexml.xml`); on opening the
//! .xls it uses it if the shape was not altered by an old version. Here: the anchor and the text of the binary must match
//! those of the XML — then it is used instead of the conversion.
//!
//! Where the TS throws ([`Thrown`]: a `DataView` past its data) the caller leaves the shape out.

use std::collections::HashMap;

use uo_drawingml::odraw::{PropTable, msospt};
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{atan2, hypot, max, min, rem, round};

use super::drawing::DrawState;
use super::geom::{Thrown, WARPS, arc_geom_xml, cust_geom_xml, preset_geom_xml, rnd, warp_adjust};
use super::globals::XlsGlobals;
use super::officeart::{OaRect, XlsShape};
use super::sheet::ObjInfo;
use super::styles_xml::{Role, font_at, icv_color};
use super::xmlw::xml_text_s;

/// What the conversion of a shape needs from the drawing around it.
pub struct Env<'e, 'm, 'a> {
    pub g: &'e XlsGlobals,
    /// The default FOPT of the document (DggContainer).
    pub defaults: &'e PropTable<'a>,
    /// OBJ linked to each shape (by `seq`).
    pub obj_of: &'e HashMap<usize, usize>,
    pub objs: &'e [ObjInfo],
    /// Counters, relationships and media; `None` for the fill of a chart (GELFRAME).
    pub st: Option<&'e mut DrawState<'m, 'a>>,
    /// Chart fill (GELFRAME): the gradient angle in Excel's convention.
    pub chart: bool,
}

/// The OBJ that do NOT become shapes: notes, form controls, filter/validation arrows.
const SKIP_OBJS: [u16; 11] = [7, 0x0b, 0x0c, 0x0d, 0x0e, 0x10, 0x11, 0x12, 0x13, 0x14, 0x19];

fn prop(s: &XlsShape<'_>, id: u16) -> Option<u32> {
    s.props.value(id)
}

/// `v | 0`.
fn sprop(s: &XlsShape<'_>, id: u16) -> Option<i32> {
    prop(s, id).map(|v| i32::from_le_bytes(v.to_le_bytes()))
}

/// 16.16 fixed with sign → number.
fn fixed(v: Option<u32>, dflt: f64) -> f64 {
    v.map_or(dflt, |v| f64::from(i32::from_le_bytes(v.to_le_bytes())) / 65536.0)
}

/// A boolean of a flag set (0x..BF/FF): bit `bit` = value, bit+16 = "fUse" (value specified). Without any fUse in the set
/// (old writers) the value is taken as it is.
fn bool_opt(props: &PropTable<'_>, id: u16, bit: u32) -> Option<bool> {
    let v = props.value(id)?;
    if v & (1 << (bit + 16)) != 0 {
        return Some(v & (1 << bit) != 0);
    }
    if v >> 16 == 0 {
        return Some(v & (1 << bit) != 0);
    }
    None
}

fn bool_prop(s: &XlsShape<'_>, id: u16, bit: u32, dflt: bool) -> bool {
    bool_opt(&s.props, id, bit).unwrap_or(dflt)
}

/// A hidden shape (fHidden of the group set 0x03BF)?
pub fn is_hidden_shape(s: &XlsShape<'_>) -> bool {
    bool_prop(s, 0x3bf, 1, false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Group,
    Chart,
    Pic,
    Shape,
}

/// The kind of object to generate for the shape (`None` = left out).
pub fn shape_kind(s: &XlsShape<'_>, obj: Option<&ObjInfo>) -> Option<ShapeKind> {
    if s.group {
        return Some(ShapeKind::Group);
    }
    if obj.is_some_and(|o| o.ot == 5) {
        return Some(ShapeKind::Chart);
    }
    if obj.is_some_and(|o| SKIP_OBJS.contains(&o.ot)) {
        return None;
    }
    if prop(s, 0x104).is_some() {
        return Some(ShapeKind::Pic);
    }
    if s.ty == 75 || s.ty == 201 {
        return None; // frame without picture / control
    }
    if msospt::has_preset(s.ty) || msospt::is_word_art_type(s.ty) || s.props.get_complex(0x145).is_some() {
        return Some(ShapeKind::Shape);
    }
    None
}

// ───────────────────────────── colours ─────────────────────────────

/// GetSysColor with the Windows defaults.
const SYS_COLORS: [&str; 25] = [
    "C8C8C8", "000000", "99B4D1", "BFCDDB", "F0F0F0", "FFFFFF", "646464", "000000", "000000", "000000", "B4B4B4", "F4F7FC", "ABABAB",
    "3399FF", "FFFFFF", "F0F0F0", "A0A0A0", "6D6D6D", "000000", "434E54", "FFFFFF", "696969", "E3E3E3", "000000", "FFFFE1",
];

fn hex2(n: f64) -> String {
    let c = max(0.0, min(255.0, round(n)));
    format!("{:02X}", u8::try_from(uo_jsnum::conv::to_int32(c)).unwrap_or(0))
}

/// `oaColor`: OfficeArtCOLORREF → `RRGGBB`. High byte: 0x08 = workbook palette index (fSchemeIndex, as Excel writes), 0x10 =
/// system/special colour (0xF0 = the fill colour, 0xF2 = the line's… with the lighten/darken function in the 2nd byte and
/// the parameter in the 3rd), else RGB.
pub fn oa_color(env: &Env<'_, '_, '_>, value: u32, s: &XlsShape<'_>, depth: u32) -> String {
    let flags = (value >> 24) & 0xff;
    let (r, g, b) = (value & 0xff, (value >> 8) & 0xff, (value >> 16) & 0xff);
    if flags & 0x08 != 0 || flags & 0x01 != 0 {
        return icv_color(value & 0xffff, &env.g.palette, Role::Fg)
            .unwrap_or_else(|| "#000000".to_owned())
            .get(1..)
            .unwrap_or("")
            .to_uppercase();
    }
    if flags & 0x10 != 0 {
        let idx = value & 0xff;
        let fnc = (value >> 8) & 0x0f;
        let invert = (value >> 8) & 0x20 != 0;
        let param = f64::from((value >> 16) & 0xff);
        let sub = |id: u16, d: u32| -> String {
            if depth > 2 {
                "000000".to_owned()
            } else {
                oa_color(env, prop(s, id).or_else(|| env.defaults.value(id)).unwrap_or(d), s, depth + 1)
            }
        };
        let base = match idx {
            0xf0 => sub(0x181, 0xff_ffff),
            0xf1 => {
                if bool_prop(s, 0x1ff, 3, true) {
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
                if bool_prop(s, 0x1bf, 4, true) {
                    sub(0x181, 0xff_ffff)
                } else {
                    sub(0x1c0, 0)
                }
            }
            _ => SYS_COLORS.get(usize::try_from(idx).unwrap_or(usize::MAX)).copied().unwrap_or("000000").to_owned(),
        };
        let comp = |i: usize| -> f64 { f64::from(u32::from_str_radix(base.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0)) };
        let f = |c: f64| -> f64 {
            match fnc {
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
        let mut c3 = [f(comp(0)), f(comp(2)), f(comp(4))];
        if invert {
            c3 = [255.0 - c3[0], 255.0 - c3[1], 255.0 - c3[2]];
        }
        return format!("{}{}{}", hex2(c3[0]), hex2(c3[1]), hex2(c3[2]));
    }
    format!("{}{}{}", hex2(f64::from(r)), hex2(f64::from(g)), hex2(f64::from(b)))
}

fn srgb(val: &str, alpha: Option<f64>) -> String {
    match alpha {
        Some(a) if a < 1.0 => format!("<a:srgbClr val=\"{val}\"><a:alpha val=\"{}\"/></a:srgbClr>", rnd(max(0.0, a) * 100_000.0)),
        _ => format!("<a:srgbClr val=\"{val}\"/>"),
    }
}

/// The property of the shape or, absent, the default of the document.
fn with_default(env: &Env<'_, '_, '_>, s: &XlsShape<'_>, id: u16) -> Option<u32> {
    prop(s, id).or_else(|| env.defaults.value(id))
}

// ───────────────────────────── fill ─────────────────────────────

/// The IMsoArray of the gradient colours (fillShadeColors: colour + position 16.16).
fn shade_colors(b: Option<&[u8]>) -> Option<Vec<(u32, f64)>> {
    let b = b.filter(|b| b.len() >= 6)?;
    let n = usize::from(u16::from_le_bytes([b[0], b[1]]));
    let size = u16::from_le_bytes([b[4], b[5]]);
    if size != 8 || n < 2 || 6 + n * 8 > b.len() {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let mut out: Vec<(u32, f64)> =
        (0..n).map(|i| (u32_at(6 + i * 8), f64::from(i32::from_le_bytes(u32_at(10 + i * 8).to_le_bytes())) / 65536.0)).collect();
    out.sort_by(|x, y| x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
    Some(out)
}

#[derive(Clone, PartialEq)]
struct Stop {
    pos: f64,
    color: String,
    alpha: Option<f64>,
}

fn rev(l: &[Stop]) -> Vec<Stop> {
    l.iter().map(|x| Stop { pos: 1.0 - x.pos, ..x.clone() }).rev().collect()
}

/// The stops of the gradient along the vector (focus: 0 = colour → background, ±100 = reversed, between = mirrored).
fn gradient_stops(env: &Env<'_, '_, '_>, s: &XlsShape<'_>, fill: &str, back: &str, fill_a: f64, back_a: f64, flip_mirror: bool) -> String {
    let arr = shade_colors(s.props.get_complex(0x197));
    let list: Vec<Stop> = match arr {
        Some(a) => a.iter().map(|&(c, p)| Stop { pos: p, color: oa_color(env, c, s, 0), alpha: None }).collect(),
        None => vec![
            Stop { pos: 0.0, color: fill.to_owned(), alpha: Some(fill_a) },
            Stop { pos: 1.0, color: back.to_owned(), alpha: Some(back_a) },
        ],
    };
    let focus = sprop(s, 0x18c).unwrap_or(0);
    let mut stops: Vec<Stop> = if focus == 0 {
        list
    } else if focus >= 100 || focus <= -100 {
        rev(&list)
    } else {
        let f = f64::from(focus.abs()) / 100.0;
        let fill_at_ends = (focus > 0) != flip_mirror;
        let a = if fill_at_ends { list } else { rev(&list) };
        let mut v: Vec<Stop> = a.iter().map(|x| Stop { pos: x.pos * f, ..x.clone() }).collect();
        v.extend(rev(&a).iter().map(|x| Stop { pos: f + x.pos * (1.0 - f), ..x.clone() }));
        v
    };
    stops.sort_by(|x, y| x.pos.partial_cmp(&y.pos).unwrap_or(std::cmp::Ordering::Equal));
    let kept: Vec<&Stop> = stops
        .iter()
        .enumerate()
        .filter(|&(i, st)| i == 0 || st.pos != stops[i - 1].pos || st.color != stops[i - 1].color || st.alpha != stops[i - 1].alpha)
        .map(|(_, st)| st)
        .collect();
    let mut out = String::from("<a:gsLst>");
    for st in kept {
        out.push_str(&format!("<a:gs pos=\"{}\">{}</a:gs>", rnd(max(0.0, min(1.0, st.pos)) * 100_000.0), srgb(&st.color, st.alpha)));
    }
    out.push_str("</a:gsLst>");
    out
}

/// The predefined patterns of DrawingML (8×8 bitmap; copy of slides-core/src/patterns.ts), in the order of the TS table.
const PRESET_PATTERNS: [(&str, &str); 54] = [
    ("pct5", "8000000008000000"),
    ("pct10", "8000080080000800"),
    ("pct20", "8800220088002200"),
    ("pct25", "8822882288228822"),
    ("pct30", "aa44aa11aa44aa11"),
    ("pct40", "aa55aa51aa55aa15"),
    ("pct50", "aa55aa55aa55aa55"),
    ("pct60", "ee55bb55ee55bb55"),
    ("pct70", "77dd77dd77dd77dd"),
    ("pct75", "77ffddff77ffddff"),
    ("pct80", "effffeffeffffeff"),
    ("pct90", "fffffff7ffffff7f"),
    ("horz", "ff00000000000000"),
    ("vert", "8080808080808080"),
    ("ltHorz", "ff000000ff000000"),
    ("ltVert", "8888888888888888"),
    ("dkHorz", "ffff0000ffff0000"),
    ("dkVert", "cccccccccccccccc"),
    ("narHorz", "ff00ff00ff00ff00"),
    ("narVert", "5555555555555555"),
    ("dashHorz", "f00000000f000000"),
    ("dashVert", "8080808008080808"),
    ("cross", "ff80808080808080"),
    ("dnDiag", "8040201008040201"),
    ("upDiag", "0102040810204080"),
    ("ltDnDiag", "8844221188442211"),
    ("ltUpDiag", "1122448811224488"),
    ("dkDnDiag", "cc663399cc663399"),
    ("dkUpDiag", "3366cc993366cc99"),
    ("wdDnDiag", "c1e070381c0e0783"),
    ("wdUpDiag", "83070e1c3870e0c1"),
    ("dashDnDiag", "0000884422110000"),
    ("dashUpDiag", "0000112244880000"),
    ("diagCross", "8142241818244281"),
    ("smCheck", "9966669999666699"),
    ("lgCheck", "f0f0f0f00f0f0f0f"),
    ("smGrid", "ff888888ff888888"),
    ("lgGrid", "ff80808080808080"),
    ("dotGrid", "aa00800080008000"),
    ("smConfetti", "8008400210012004"),
    ("lgConfetti", "b130031bd8c00c8d"),
    ("horzBrick", "ff808080ff080808"),
    ("diagBrick", "0102040818244281"),
    ("solidDmnd", "10387cfe7c381000"),
    ("openDmnd", "8244281028448201"),
    ("dotDmnd", "8000220008002200"),
    ("plaid", "aa55aa55f0f0f0f0"),
    ("sphere", "77898f8f7798f8f8"),
    ("weave", "8854224588142251"),
    ("divot", "0010081000800180"),
    ("shingle", "038448300c020101"),
    ("wave", "001825c0001825c0"),
    ("trellis", "ff66ff99ff66ff99"),
    ("zigZag", "8142241881422418"),
];

/// A PATTERN fill: the fillBlip is a 1-bpp 8×8 BMP — matches the bitmap (light pixel = foreground colour) with the
/// predefined patterns in any phase.
fn pattern_prst(b: Option<&[u8]>) -> Option<&'static str> {
    let b = b.filter(|b| b.len() >= 62 && b[0] == 0x42 && b[1] == 0x4d)?;
    let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let off = usize::try_from(u32_at(10)).unwrap_or(usize::MAX);
    let hdr = usize::try_from(u32_at(14)).unwrap_or(usize::MAX);
    let w = i32::from_le_bytes(u32_at(18).to_le_bytes());
    let h = i32::from_le_bytes(u32_at(22).to_le_bytes());
    let bpp = u16::from_le_bytes([b[28], b[29]]);
    if w != 8 || h.unsigned_abs() != 8 || bpp != 1 {
        return None;
    }
    let at = |o: usize| -> u32 { b.get(o).copied().map_or(0, u32::from) };
    let pal: Vec<u32> = (0..2usize)
        .map(|i| {
            let o = 14usize.saturating_add(hdr).saturating_add(i * 4);
            at(o) + at(o.saturating_add(1)) + at(o.saturating_add(2))
        })
        .collect();
    let light_bit = pal[1] > pal[0];
    let rows: Vec<u32> = (0..8usize)
        .map(|r| {
            let rr = if h > 0 { 7 - r } else { r };
            let v = at(off.saturating_add(rr * 4));
            if light_bit { v } else { !v & 0xff }
        })
        .collect();
    let rot = |v: u32, n: u32| -> u32 { ((v << n) | (v >> (8 - n))) & 0xff };
    for dy in 0..8usize {
        for dx in 0..8u32 {
            let hexs: String = (0..8usize).map(|r| format!("{:02x}", rot(rows[(r + dy) % 8], dx))).collect();
            if let Some((k, _)) = PRESET_PATTERNS.iter().find(|(_, p)| *p == hexs) {
                return Some(k);
            }
        }
    }
    None
}

/// Fill → `<a:noFill/>`, solid, gradient, pattern or picture.
fn fill_xml(env: &mut Env<'_, '_, '_>, s: &XlsShape<'_>, on: bool) -> String {
    if !on {
        return "<a:noFill/>".to_owned();
    }
    let ty = prop(s, 0x180).unwrap_or(0);
    let color = oa_color(env, with_default(env, s, 0x181).unwrap_or(0xff_ffff), s, 0);
    let opacity = fixed(prop(s, 0x182), 1.0);
    let back = oa_color(env, with_default(env, s, 0x183).unwrap_or(0xff_ffff), s, 0);
    let back_opacity = fixed(prop(s, 0x184), 1.0);
    let solid = format!("<a:solidFill>{}</a:solidFill>", srgb(&color, Some(opacity)));
    match ty {
        2 | 3 => {
            let pib = prop(s, 0x186).filter(|&p| p != 0);
            let rid = pib.and_then(|p| env.st.as_deref_mut().and_then(|st| st.image_rel(p)));
            let Some(rid) = rid else { return solid };
            format!(
                "<a:blipFill dpi=\"0\" rotWithShape=\"1\"><a:blip r:embed=\"{rid}\"/><a:srcRect/>{}</a:blipFill>",
                if ty == 2 {
                    "<a:tile tx=\"0\" ty=\"0\" sx=\"100000\" sy=\"100000\" flip=\"none\" algn=\"tl\"/>"
                } else {
                    "<a:stretch><a:fillRect/></a:stretch>"
                }
            )
        }
        4..=8 => {
            if ty == 4 || ty == 7 || ty == 8 {
                // [MS-ODRAW] fillAngle θ → ang = 90° − θ, θ absent/0 being −180° (measured on Slides); in the GELFRAME of the
                // Excel charts, ang = 270° − θ (the gradient of the 2007 theme — dark at the bottom — has θ = 180° and focus
                // 100: measured on the oracle, poi-WithTwoCharts/poi-48180)
                let raw = if prop(s, 0x18b).is_none() { 0.0 } else { fixed(prop(s, 0x18b), 0.0) };
                let theta = if raw == 0.0 { -180.0 } else { raw };
                let ang =
                    if env.chart { rem(rem(270.0 - raw, 360.0) + 360.0, 360.0) } else { rem(rem(90.0 - theta, 360.0) + 360.0, 360.0) };
                return format!(
                    "<a:gradFill flip=\"none\" rotWithShape=\"1\">{}<a:lin ang=\"{}\" scaled=\"{}\"/></a:gradFill>",
                    gradient_stops(env, s, &color, &back, opacity, back_opacity, raw != 0.0),
                    to_js_string(rem(round(ang * 60000.0), 21_600_000.0)),
                    i32::from(ty == 7)
                );
            }
            // radial (shadeCenter/shadeShape): the background at the focus (fillTo*), the fill colour at the edge
            let l = fixed(prop(s, 0x18d), 0.0);
            let t = fixed(prop(s, 0x18e), 0.0);
            let r = fixed(prop(s, 0x18f), 0.0);
            let b = fixed(prop(s, 0x190), 0.0);
            format!(
                "<a:gradFill flip=\"none\" rotWithShape=\"1\">{}<a:path path=\"{}\"><a:fillToRect l=\"{}\" t=\"{}\" r=\"{}\" b=\"{}\"/></a:path></a:gradFill>",
                gradient_stops(env, s, &back, &color, back_opacity, opacity, false),
                if ty == 6 { "shape" } else { "rect" },
                rnd(l * 100_000.0),
                rnd(t * 100_000.0),
                rnd((1.0 - r) * 100_000.0),
                rnd((1.0 - b) * 100_000.0)
            )
        }
        1 => {
            // pattern (8×8 bitmap) → predefined pattFill; without a match, the foreground colour
            let pib = prop(s, 0x186).filter(|&p| p != 0);
            let prst = pib.and_then(|p| {
                let bytes = env.st.as_deref_mut().and_then(|st| st.blip_bytes(p));
                pattern_prst(bytes.as_deref())
            });
            match prst {
                Some(p) => format!(
                    "<a:pattFill prst=\"{p}\"><a:fgClr>{}</a:fgClr><a:bgClr>{}</a:bgClr></a:pattFill>",
                    srgb(&color, None),
                    srgb(&back, None)
                ),
                None => solid,
            }
        }
        9 => "<a:noFill/>".to_owned(), // background (msofillBackground): the cell shows through
        _ => solid,
    }
}

/// `foptFillXml`: the fill from loose FOPT properties (the GELFRAME of the charts): solid, gradient (fillShadeColors/angle/
/// focus) or pattern; no pictures. `None` = fFilled off.
pub fn foptfill_xml(g: &XlsGlobals, props: &PropTable<'_>) -> Option<String> {
    let s = XlsShape {
        seq: 0,
        spid: 0,
        ty: 1,
        flags: 0,
        props: props.clone(),
        anchor: None,
        child_anchor: None,
        group_rect: None,
        client_data_at: None,
        group: false,
        children: Vec::new(),
    };
    let defaults = PropTable::new();
    let none: HashMap<usize, usize> = HashMap::new();
    let mut env = Env { g, defaults: &defaults, obj_of: &none, objs: &[], st: None, chart: true };
    if !bool_prop(&s, 0x1bf, 4, true) {
        return None;
    }
    Some(fill_xml(&mut env, &s, true))
}

// ───────────────────────────── line, shadow ─────────────────────────────

const DASH: [&str; 11] =
    ["solid", "sysDash", "sysDot", "sysDashDot", "sysDashDotDot", "dot", "dash", "lgDash", "dashDot", "lgDashDot", "lgDashDotDot"];
const ARROW: [&str; 6] = ["none", "triangle", "stealth", "diamond", "oval", "arrow"];
const ARROW_SZ: [&str; 3] = ["sm", "med", "lg"];

fn arrow_end(tag: &str, s: &XlsShape<'_>, type_id: u16, w_id: u16, len_id: u16) -> String {
    let t = prop(s, type_id).unwrap_or(0);
    if t == 0 {
        return String::new();
    }
    let size = |id: u16| -> String {
        match prop(s, id) {
            Some(v) if v != 1 => ARROW_SZ
                .get(usize::try_from(v).unwrap_or(usize::MAX))
                .map_or_else(String::new, |z| format!(" {}=\"{z}\"", if id == w_id { "w" } else { "len" })),
            _ => String::new(),
        }
    };
    format!(
        "<{tag} type=\"{}\"{}{}/>",
        ARROW.get(usize::try_from(t).unwrap_or(usize::MAX)).copied().unwrap_or("triangle"),
        size(w_id),
        size(len_id)
    )
}

/// Outline → `<a:ln>` (Excel's default: black 0.75 pt).
fn line_xml(env: &Env<'_, '_, '_>, s: &XlsShape<'_>, on: bool) -> String {
    if !on {
        return "<a:ln><a:noFill/></a:ln>".to_owned();
    }
    let w = max(0.0, min(f64::from(prop(s, 0x1cb).unwrap_or(9525)), 20.0 * 12700.0));
    let color = oa_color(env, with_default(env, s, 0x1c0).unwrap_or(0), s, 0);
    let alpha = fixed(prop(s, 0x1c1), 1.0);
    let dash = prop(s, 0x1ce).unwrap_or(0);
    let cap = prop(s, 0x1d7);
    let cmpd = prop(s, 0x1cd);
    let join = prop(s, 0x1d6);
    let mut kids = format!(
        "<a:solidFill>{}</a:solidFill><a:prstDash val=\"{}\"/>",
        srgb(&color, Some(alpha)),
        DASH.get(usize::try_from(dash).unwrap_or(usize::MAX)).copied().unwrap_or("solid")
    );
    kids.push_str(match join {
        Some(0) => "<a:bevel/>",
        Some(1) => "<a:miter lim=\"800000\"/>",
        _ => "<a:round/>",
    });
    kids.push_str(&arrow_end("a:headEnd", s, 0x1d0, 0x1d2, 0x1d3));
    kids.push_str(&arrow_end("a:tailEnd", s, 0x1d1, 0x1d4, 0x1d5));
    let cap_a = match cap {
        Some(0) => " cap=\"rnd\"",
        Some(1) => " cap=\"sq\"",
        _ => "",
    };
    let cmpd_a = match cmpd {
        Some(c) if c != 0 => format!(
            " cmpd=\"{}\"",
            ["sng", "dbl", "thickThin", "thinThick", "tri"].get(usize::try_from(c).unwrap_or(usize::MAX)).copied().unwrap_or("sng")
        ),
        _ => String::new(),
    };
    format!("<a:ln w=\"{}\"{cap_a}{cmpd_a}>{kids}</a:ln>", to_js_string(w))
}

/// Shadow (fShadow) → `<a:effectLst><a:outerShdw>`.
fn shadow_xml(env: &Env<'_, '_, '_>, s: &XlsShape<'_>) -> String {
    if !bool_prop(s, 0x23f, 1, false) {
        return String::new();
    }
    let color = oa_color(env, prop(s, 0x201).unwrap_or(0x80_8080), s, 0);
    let alpha = fixed(prop(s, 0x204), 1.0);
    let ox = f64::from(sprop(s, 0x205).unwrap_or(25400));
    let oy = f64::from(sprop(s, 0x206).unwrap_or(25400));
    let dist = rnd(hypot(ox, oy));
    let mut dir = round((atan2(oy, ox) * 180.0 / std::f64::consts::PI) * 60000.0);
    if dir < 0.0 {
        dir += 21_600_000.0;
    }
    let blur = prop(s, 0x21c).unwrap_or(0);
    format!(
        "<a:effectLst><a:outerShdw{} dist=\"{dist}\" dir=\"{}\" algn=\"ctr\" rotWithShape=\"0\">{}</a:outerShdw></a:effectLst>",
        if blur != 0 { format!(" blurRad=\"{blur}\"") } else { String::new() },
        to_js_string(dir),
        srgb(&color, Some(alpha))
    )
}

// ───────────────────────────── geometry, position ─────────────────────────────

/// A box in the parent space: off/ext (the top-level shape: the anchor rules).
#[derive(Clone, Copy)]
struct BoxF {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn rotation_deg(s: &XlsShape<'_>) -> f64 {
    let r = fixed(prop(s, 0x0004), 0.0);
    rem(rem(r, 360.0) + 360.0, 360.0)
}

/// The box of a shape INSIDE a group: the ChildAnchor; those rotated between 45° and 135° (and 225°–315°) store the box already
/// "lying" (width/height swapped around the centre) — at the top level the renderer undoes that.
fn child_box(s: &XlsShape<'_>) -> Option<BoxF> {
    let a: OaRect = s.child_anchor?;
    let (l, t, r, b) = (f64::from(a.l), f64::from(a.t), f64::from(a.r), f64::from(a.b));
    let mut x = min(l, r);
    let mut y = min(t, b);
    let mut w = (r - l).abs();
    let mut h = (b - t).abs();
    let rot = rotation_deg(s);
    if (45.0..135.0).contains(&rot) || (225.0..315.0).contains(&rot) {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        std::mem::swap(&mut w, &mut h);
        x = cx - w / 2.0;
        y = cy - h / 2.0;
    }
    Some(BoxF { x, y, w, h })
}

fn xfrm_xml(s: &XlsShape<'_>, bx: Option<BoxF>, extra_flip_v: bool, inner: &str) -> String {
    let rot = rem(round(rotation_deg(s) * 60000.0), 21_600_000.0);
    let flip_h = s.flags & 0x40 != 0;
    let flip_v = (s.flags & 0x80 != 0) != extra_flip_v;
    let b = bx.unwrap_or(BoxF { x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    format!(
        "<a:xfrm{}{}{}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/>{inner}</a:xfrm>",
        if rot != 0.0 { format!(" rot=\"{}\"", to_js_string(rot)) } else { String::new() },
        if flip_h { " flipH=\"1\"" } else { "" },
        if flip_v { " flipV=\"1\"" } else { "" },
        rnd(b.x),
        rnd(b.y),
        rnd(b.w),
        rnd(b.h)
    )
}

/// Geometry: arc, freeform or preset (with the adjusts); gives the extra flipV of the mirrored presets.
fn geometry_xml(s: &XlsShape<'_>, w: f64, h: f64) -> Result<(String, bool), Thrown> {
    if s.ty == 19 {
        return Ok((arc_geom_xml(&s.props), false));
    }
    if (!msospt::has_preset(s.ty) || s.ty == 0)
        && let Some(cg) = cust_geom_xml(&s.props)?
    {
        return Ok((cg, false));
    }
    Ok(preset_geom_xml(s.ty, &msospt::raw_adjust(&s.props), w, h)
        .unwrap_or_else(|| ("<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_owned(), false)))
}

// ───────────────────────────── text ─────────────────────────────

fn h_align(v: u16) -> &'static str {
    match v {
        2 => "ctr",
        3 => "r",
        4 => "just",
        7 => "dist",
        _ => "l",
    }
}

fn v_anchor(v: u16) -> &'static str {
    match v {
        2 => "ctr",
        3 => "b",
        4 => "just",
        7 => "dist",
        _ => "t",
    }
}

/// The text of the TXO → `<xdr:txBody>` (paragraphs by line break, runs by the fonts).
fn tx_body(env: &Env<'_, '_, '_>, s: &XlsShape<'_>, obj: Option<&ObjInfo>) -> String {
    let Some(obj) = obj else { return String::new() };
    let Some(text) = obj.text.as_ref().filter(|t| !t.text.is_empty()) else { return String::new() };
    let g = env.g;
    let al = obj.txo_align.unwrap_or(0);
    let algn = h_align((al >> 1) & 7);
    let anchor = v_anchor((al >> 4) & 7);
    // fAutoTextMargin (textBool bit 3): automatic margins — the ones Excel writes converting the legacy text box to DrawingML
    // (0.03" on the left, 0.025" on top, 0 on the other sides)
    let auto = bool_prop(s, 0xbf, 3, false);
    let ins = |pid: u16, def: i32, auto_v: i32| -> i32 { if auto { auto_v } else { sprop(s, pid).unwrap_or(def) } };
    let rot = obj.txo_rot.unwrap_or(0);
    let vert = match rot {
        1 => " vert=\"wordArtVert\"",
        2 => " vert=\"vert270\"",
        3 => " vert=\"vert\"",
        _ => "",
    };
    let wrap = if prop(s, 0x85).unwrap_or(0) == 2 { "none" } else { "square" };
    let body = format!(
        "<a:bodyPr vertOverflow=\"clip\" wrap=\"{wrap}\" lIns=\"{}\" tIns=\"{}\" rIns=\"{}\" bIns=\"{}\" anchor=\"{anchor}\"{vert}/>",
        ins(0x81, 91440, 27432),
        ins(0x82, 45720, 22860),
        ins(0x83, 91440, 0),
        ins(0x84, 45720, 0)
    );
    // runs: [ich, ifnt] sorted; before the 1st run, the font 0
    let mut runs: Vec<(u16, u16)> = text.runs.iter().map(|r| (r.ich, r.ifnt)).collect();
    runs.sort_by_key(|r| r.0);
    if runs.is_empty() || runs[0].0 > 0 {
        runs.insert(0, (0, 0));
    }
    let rpr = |ifnt: u16, tag: &str| -> String {
        let f = font_at(g, ifnt);
        let color = icv_color(u32::from(f.color), &g.palette, Role::Font).unwrap_or_else(|| "#000000".to_owned());
        let u = if f.underline == 2 || f.underline == 0x22 {
            " u=\"dbl\""
        } else if f.underline != 0 {
            " u=\"sng\""
        } else {
            ""
        };
        let base = match f.script {
            1 => " baseline=\"30000\"",
            2 => " baseline=\"-25000\"",
            _ => "",
        };
        let name = xml_text_s(&f.name);
        format!(
            "<{tag} lang=\"pt-BR\" sz=\"{}\" b=\"{}\" i=\"{}\"{u}{}{base}><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:latin typeface=\"{name}\"/><a:cs typeface=\"{name}\"/></{tag}>",
            rnd((f64::from(f.height) / 20.0) * 100.0),
            i32::from(f.weight >= 600),
            i32::from(f.italic),
            if f.strike { " strike=\"sngStrike\"" } else { "" },
            color.replace('#', "").to_uppercase()
        )
    };
    // `text.text.replace(/\r\n?/g, '\n')` over the UTF-16 units: the run offsets index THIS string (a TS quirk kept)
    let mut full: Vec<u16> = Vec::with_capacity(text.text.len());
    let units: Vec<u16> = text.text.encode_utf16().collect();
    let mut i = 0;
    while i < units.len() {
        if units[i] == 13 {
            full.push(10);
            if units.get(i + 1) == Some(&10) {
                i += 1;
            }
        } else {
            full.push(units[i]);
        }
        i += 1;
    }
    let mut paras: Vec<(String, u16)> = Vec::new();
    let mut cur = String::new();
    let mut cur_font = runs[0].1;
    for (ri, r) in runs.iter().enumerate() {
        let end = if ri + 1 < runs.len() { usize::from(runs[ri + 1].0) } else { full.len() };
        let a = usize::from(r.0).min(full.len());
        let b = end.min(full.len()).max(a);
        let seg = &full[a..b];
        for (k, line) in seg.split(|&u| u == 10).enumerate() {
            if k > 0 {
                paras.push((std::mem::take(&mut cur), cur_font));
            }
            cur_font = r.1;
            if !line.is_empty() {
                cur.push_str(&format!("<a:r>{}<a:t>{}</a:t></a:r>", rpr(r.1, "a:rPr"), xml_text_s(&String::from_utf16_lossy(line))));
            }
        }
    }
    paras.push((cur, cur_font));
    // an empty paragraph takes the height of the font at the point (endParaRPr)
    let mut p_xml = String::new();
    for (x, ifnt) in &paras {
        p_xml.push_str(&format!(
            "<a:p><a:pPr algn=\"{algn}\"/>{x}{}</a:p>",
            if x.is_empty() { rpr(*ifnt, "a:endParaRPr") } else { String::new() }
        ));
    }
    format!("<xdr:txBody>{body}<a:lstStyle/>{p_xml}</xdr:txBody>")
}

/// The UTF-16 string of a complex property (without the terminator).
fn prop_string(b: Option<&[u8]>) -> Option<String> {
    let b = b.filter(|b| b.len() >= 2)?;
    let mut units: Vec<u16> = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        let c = u16::from_le_bytes([b[i], b[i + 1]]);
        if c == 0 {
            break;
        }
        units.push(c);
        i += 2;
    }
    if units.is_empty() { None } else { Some(String::from_utf16_lossy(&units)) }
}

/// The classic WordArt (gtextUNICODE 0xC0, gtextFont 0xC5, gtextSize 0xC3) → text with prstTxWarp; the LETTERS carry the
/// fill, the outline and the shadow of the shape (as the Office conversion does).
fn word_art_body(env: &mut Env<'_, '_, '_>, s: &XlsShape<'_>, w: f64, h: f64) -> Option<String> {
    let text = prop_string(s.props.get_complex(0xc0))?;
    let font = prop_string(s.props.get_complex(0xc5));
    let bold = bool_prop(s, 0xff, 5, false);
    let italic = bool_prop(s, 0xff, 4, false);
    let lines = super::chart::split_lines(&text);
    let warp: Option<&str> = if (136..=175).contains(&s.ty) { WARPS.get(usize::from(s.ty) - 136).copied() } else { None };
    let sz_pt = if warp.is_some() {
        let v = fixed(prop(s, 0xc3), 36.0);
        if v == 0.0 || v.is_nan() { 36.0 } else { v }
    } else {
        let longest = lines.iter().map(|l| l.encode_utf16().count()).max().unwrap_or(0).max(1) as f64;
        max(4.0, min(min(400.0, (h / 12700.0 / lines.len() as f64) * 0.75), (w / 12700.0) / (longest * if bold { 0.66 } else { 0.6 })))
    };
    let fx = fill_xml(env, s, bool_prop(s, 0x1bf, 4, true));
    let letter_fill = if fx.starts_with("<a:solidFill") || fx.starts_with("<a:gradFill") || fx.starts_with("<a:pattFill") {
        fx
    } else {
        format!("<a:solidFill>{}</a:solidFill>", srgb(&oa_color(env, with_default(env, s, 0x181).unwrap_or(0xff_ffff), s, 0), None))
    };
    let r_pr = format!(
        "<a:rPr lang=\"pt-BR\" sz=\"{}\" b=\"{}\" i=\"{}\">{}{letter_fill}{}{}</a:rPr>",
        rnd(sz_pt * 100.0),
        i32::from(bold),
        i32::from(italic),
        line_xml(env, s, bool_prop(s, 0x1ff, 3, true)),
        shadow_xml(env, s),
        font.as_deref().map_or_else(String::new, |f| format!("<a:latin typeface=\"{}\"/>", xml_text_s(f)))
    );
    let paras: String =
        lines.iter().map(|l| format!("<a:p><a:pPr algn=\"ctr\"/><a:r>{r_pr}<a:t>{}</a:t></a:r></a:p>", xml_text_s(l))).collect();
    let av = match warp {
        Some(wp) => warp_adjust(wp, &msospt::raw_adjust(&s.props), w, h),
        None => Vec::new(),
    };
    let wx = match warp {
        Some(wp) => {
            let gds: String = av
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    format!(
                        "<a:gd name=\"{}\" fmla=\"val {}\"/>",
                        if av.len() > 1 { format!("adj{}", i + 1) } else { "adj".to_owned() },
                        to_js_string(*v)
                    )
                })
                .collect();
            format!("<a:prstTxWarp prst=\"{wp}\"><a:avLst>{gds}</a:avLst></a:prstTxWarp>")
        }
        None => String::new(),
    };
    let vert = if warp.is_some() && bool_prop(s, 0xff, 13, false) { " vert=\"wordArtVert\"" } else { "" };
    Some(format!(
        "<xdr:txBody><a:bodyPr wrap=\"none\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"ctr\" rtlCol=\"0\"{vert}{}>{wx}<a:noAutofit/></a:bodyPr><a:lstStyle/>{paras}</xdr:txBody>",
        if warp.is_some() { " fromWordArt=\"1\"" } else { "" }
    ))
}

// ───────────────────────────── metroBlob (Excel 2007+) ─────────────────────────────

/// Cap of the parts of the metroBlob zip (the XML of a shape: KB).
const MAX_METRO_PART: u64 = 4 * 1024 * 1024;

fn is_xml_name(s: &str) -> bool {
    // /^[A-Za-z_][\w.:-]*$/
    let mut it = s.chars();
    it.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && it.all(|c| c.is_ascii_alphanumeric() || c == '_' || matches!(c, '.' | ':' | '-'))
}

fn serialize_xml(e: uo_opc::xml::Element<'_>, out: &mut String, drop_links: bool) {
    let name = e.name();
    if !is_xml_name(name) {
        return;
    }
    out.push('<');
    out.push_str(name);
    for a in e.attrs() {
        if is_xml_name(a.name) {
            out.push(' ');
            out.push_str(a.name);
            out.push_str("=\"");
            super::xmlw::xml_text(out, a.value);
            out.push('"');
        }
    }
    let kids: Vec<uo_opc::xml::Node<'_>> = e
        .children()
        .filter(|n| match n {
            uo_opc::xml::Node::Element(c) => !(drop_links && matches!(c.name(), "a:hlinkClick" | "a:hlinkHover")),
            uo_opc::xml::Node::Text(_) | uo_opc::xml::Node::CData(_) => true,
            _ => false,
        })
        .collect();
    if kids.is_empty() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for k in kids {
        match k {
            uo_opc::xml::Node::Element(c) => serialize_xml(c, out, drop_links),
            uo_opc::xml::Node::Text(t) | uo_opc::xml::Node::CData(t) => super::xmlw::xml_text(out, t),
            _ => {}
        }
    }
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

/// The text of every `a:t` of the tree.
fn all_text(e: uo_opc::xml::Element<'_>, out: &mut String) {
    for c in e.children() {
        let uo_opc::xml::Node::Element(c) = c else { continue };
        if c.name() == "a:t" {
            for n in c.children() {
                if let Some(t) = n.as_text() {
                    out.push_str(t);
                }
            }
        } else {
            all_text(c, out);
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `/\br:(id|embed|link|pict)=/`.
fn has_r_attr(xml: &str) -> bool {
    let mut from = 0;
    while let Some(k) = xml[from..].find("r:") {
        let at = from + k;
        let prev_ok = xml[..at].chars().next_back().is_none_or(|c| !is_word(c));
        let rest = &xml[at + 2..];
        if prev_ok && ["id=", "embed=", "link=", "pict="].iter().any(|p| rest.starts_with(p)) {
            return true;
        }
        from = at + 2;
    }
    false
}

/// `xml.replace(/(<xdr:cNvPr\b[^>]*?\bid=")\d+(")/, `$1${id}$2`)`.
fn replace_cnvpr_id(xml: &str, id: u32) -> String {
    let tag = "<xdr:cNvPr";
    let mut from = 0;
    while let Some(k) = xml[from..].find(tag) {
        let start = from + k;
        let after = start + tag.len();
        from = after;
        if xml[after..].chars().next().is_some_and(is_word) {
            continue; // `\b` after the name
        }
        // lazy `[^>]*?` then `\bid="`
        let mut j = after;
        while j <= xml.len() {
            if xml[j..].starts_with("id=\"") && xml[..j].chars().next_back().is_none_or(|c| !is_word(c)) {
                let ds = j + 4;
                let digits = xml[ds..].bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 && xml[ds + digits..].starts_with('"') {
                    return format!("{}{id}{}", &xml[..ds], &xml[ds + digits..]);
                }
            }
            match xml[j..].chars().next() {
                Some('>') | None => break,
                Some(c) => j += c.len_utf8(),
            }
        }
    }
    xml.to_owned()
}

/// The original DrawingML object of the metroBlob (Excel 2007+), validated against the binary: the same cell anchor
/// (downrev.xml) and the same TXO text. Objects with pictures from the inner zip keep the conversion of the binary.
fn metro_object(s: &XlsShape<'_>, obj: Option<&ObjInfo>, id: u32, budget: Option<&mut u64>) -> Option<String> {
    let blob = s.props.get_complex(0x3a9)?;
    if blob.len() < 4 || blob[0] != 0x50 || blob[1] != 0x4b {
        return None;
    }
    let a = s.anchor?;
    let left = budget.as_ref().map_or(u64::MAX, |b| **b);
    if left == 0 {
        return None;
    }
    let parts = super::theme_xml::unzip_filtered(blob, 4096, MAX_METRO_PART, &mut |e| {
        matches!(e.name.as_str(), "drs/shapexml.xml" | "drs/connectorxml.xml" | "drs/downrev.xml")
            && e.size <= MAX_METRO_PART
            && e.size <= left
            && e.size <= (1u64 << 20).max(e.compressed_size.saturating_mul(1100))
    })?;
    if let Some(b) = budget {
        *b = b.saturating_sub(parts.iter().map(|(_, v)| v.len() as u64).sum());
    }
    let get = |n: &str| parts.iter().find(|(k, _)| k == n).map(|(_, v)| v);
    let main = get("drs/shapexml.xml").or_else(|| get("drs/connectorxml.xml"))?;
    let down = get("drs/downrev.xml")?;
    let opts = uo_opc::Limits::DEFAULT.bounded(&crate::read::SHEET_XML);
    let root_doc = uo_opc::xml::parse(uo_jsnum::text::decode_utf8(main).into_owned(), &opts).ok()?;
    let dr_doc = uo_opc::xml::parse(uo_jsnum::text::decode_utf8(down).into_owned(), &opts).ok()?;
    let dr = dr_doc.root();
    // the anchor written with the XML = the binary's (a shape not moved by an old version)
    let xa = dr.first_child("a:xlAnchor");
    let from = xa.and_then(|x| x.first_child("a:from"));
    let to = xa.and_then(|x| x.first_child("a:to"));
    let (from, to) = (from?, to?);
    let n = |e: uo_opc::xml::Element<'_>, k: &str| -> f64 { e.attr(k).map_or(f64::NAN, uo_jsnum::parse::to_number) };
    if n(from, "col") != f64::from(a.c1)
        || n(from, "row") != f64::from(a.r1)
        || n(to, "col") != f64::from(a.c2)
        || n(to, "row") != f64::from(a.r2)
    {
        return None;
    }
    let root = root_doc.root();
    let anchor_el = root.child_elements().find(|c| c.name().ends_with("Anchor"))?;
    let o = anchor_el.child_elements().find(|c| matches!(c.name(), "xdr:sp" | "xdr:cxnSp" | "xdr:grpSp" | "xdr:pic"))?;
    // text: the binary's (TXO) must be the same (without line breaks)
    let norm = |t: &str| -> String { t.chars().filter(|&c| !uo_jsnum::text::is_js_space(c)).collect() };
    let mut mt = String::new();
    all_text(o, &mut mt);
    if norm(&mt) != norm(obj.and_then(|o| o.text.as_ref()).map_or("", |t| t.text.as_str())) {
        return None;
    }
    let mut xml = String::new();
    serialize_xml(o, &mut xml, true);
    if has_r_attr(&xml) {
        return None;
    }
    // unique id in the drawing
    Some(replace_cnvpr_id(&xml, id))
}

// ───────────────────────────── objects ─────────────────────────────

/// Picture adjustments → children of `<a:blip>` (as the Office conversion): transparent colour (0x107), brightness (0x109,
/// ±32768 = ±100%) and contrast (0x108, 16.16) → `<a:lum>`; grayscale / black and white (0x13F).
fn blip_adjust_xml(env: &Env<'_, '_, '_>, s: &XlsShape<'_>) -> String {
    let mut out = String::new();
    if let Some(tr) = prop(s, 0x107) {
        let c = oa_color(env, tr, s, 0);
        out.push_str(&format!(
            "<a:clrChange><a:clrFrom>{}</a:clrFrom><a:clrTo><a:srgbClr val=\"{c}\"><a:alpha val=\"0\"/></a:srgbClr></a:clrTo></a:clrChange>",
            srgb(&c, None)
        ));
    }
    let br = prop(s, 0x109);
    let ct = prop(s, 0x108);
    let bright = match br {
        Some(b) => round(f64::from(i32::from_le_bytes(b.to_le_bytes())) / 32768.0 * 100.0) * 1000.0,
        None => 0.0,
    };
    let mut contrast = 0.0;
    if let Some(c) = ct {
        let c = f64::from(c) / 65536.0;
        contrast = round(if c < 1.0 { c - 1.0 } else { 1.0 - 1.0 / c } * 100.0) * 1000.0;
    }
    if bright != 0.0 || contrast != 0.0 {
        out.push_str(&format!(
            "<a:lum{}{}/>",
            if bright != 0.0 { format!(" bright=\"{}\"", to_js_string(bright)) } else { String::new() },
            if contrast != 0.0 { format!(" contrast=\"{}\"", to_js_string(contrast)) } else { String::new() }
        ));
    }
    if bool_prop(s, 0x13f, 2, false) {
        out.push_str("<a:grayscl/>");
    } else if bool_prop(s, 0x13f, 1, false) {
        out.push_str("<a:biLevel thresh=\"50000\"/>");
    }
    out
}

/// Crop (16.16, fraction of the picture; negative = margin) → `a:srcRect`.
fn src_rect_xml(s: &XlsShape<'_>) -> String {
    let v = |id: u16| -> f64 { round(fixed(prop(s, id), 0.0) * 100_000.0) };
    let (t, b, l, r) = (v(0x100), v(0x101), v(0x102), v(0x103));
    if t == 0.0 && b == 0.0 && l == 0.0 && r == 0.0 {
        return String::new();
    }
    let attr = |n: &str, x: f64| -> String { if x != 0.0 { format!(" {n}=\"{}\"", to_js_string(x)) } else { String::new() } };
    format!("<a:srcRect{}{}{}{}/>", attr("l", l), attr("t", t), attr("r", r), attr("b", b))
}

fn shape_name(s: &XlsShape<'_>, fallback: &str) -> String {
    prop_string(s.props.get_complex(0x380)).unwrap_or_else(|| fallback.to_owned())
}

/// The DrawingML object of the shape (without the anchor). `size` = the size in EMU of the box at the top level (the proportion
/// for the preset adjusts); inside a group the box comes from the ChildAnchor. `""` = nothing to draw.
pub fn object_xml(env: &mut Env<'_, '_, '_>, s: &XlsShape<'_>, size: (f64, f64), in_group: bool) -> Result<String, Thrown> {
    if is_hidden_shape(s) {
        return Ok(String::new());
    }
    let obj_idx = env.obj_of.get(&s.seq).copied();
    let obj = obj_idx.and_then(|i| env.objs.get(i));
    let Some(kind) = shape_kind(s, obj) else { return Ok(String::new()) };
    let bx = if in_group { child_box(s) } else { None };
    if in_group && bx.is_none() {
        return Ok(String::new());
    }
    let w = bx.map_or(size.0, |b| b.w);
    let h = bx.map_or(size.1, |b| b.h);
    let id = env.st.as_deref_mut().map_or(0, DrawState::next_id);
    if !in_group && let Some(m) = metro_object(s, obj, id, env.st.as_deref_mut().map(DrawState::metro_budget)) {
        return Ok(m);
    }
    match kind {
        ShapeKind::Group => group_xml(env, s, bx, id),
        ShapeKind::Chart => {
            let rid = if in_group && obj.is_some() {
                let range = obj.and_then(|o| o.chart);
                env.st.as_deref_mut().and_then(|st| st.chart_rel(range))
            } else {
                None
            };
            let (Some(rid), Some(b)) = (rid, bx) else { return Ok(String::new()) };
            Ok(format!(
                "<xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{id}\" name=\"{}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>\
                 <xdr:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></xdr:xfrm>\
                 <a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">\
                 <c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame>",
                xml_text_s(&shape_name(s, &format!("Gráfico {}", id.saturating_sub(1)))),
                rnd(b.x),
                rnd(b.y),
                rnd(b.w),
                rnd(b.h)
            ))
        }
        ShapeKind::Pic => {
            let pib = prop(s, 0x104).unwrap_or(0);
            let rid = env.st.as_deref_mut().and_then(|st| st.image_rel(pib));
            let Some(rid) = rid else { return Ok(String::new()) };
            let (geom, flip) = geometry_xml(s, w, h)?;
            // picture: no outline except the explicit fLine
            let ln = if bool_opt(&s.props, 0x1ff, 3).is_some_and(|v| v) { line_xml(env, s, true) } else { String::new() };
            Ok(format!(
                "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{id}\" name=\"{}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr>\
                 <xdr:blipFill><a:blip r:embed=\"{rid}\">{}</a:blip>{}<a:stretch><a:fillRect/></a:stretch></xdr:blipFill>\
                 <xdr:spPr>{}{geom}{ln}{}</xdr:spPr></xdr:pic>",
                xml_text_s(&shape_name(s, &format!("Imagem {}", id.saturating_sub(1)))),
                blip_adjust_xml(env, s),
                src_rect_xml(s),
                xfrm_xml(s, bx, flip, ""),
                shadow_xml(env, s)
            ))
        }
        ShapeKind::Shape => sp_xml(env, s, obj_idx, bx, w, h, id),
    }
}

fn sp_xml(
    env: &mut Env<'_, '_, '_>,
    s: &XlsShape<'_>,
    obj_idx: Option<usize>,
    bx: Option<BoxF>,
    w: f64,
    h: f64,
    id: u32,
) -> Result<String, Thrown> {
    let obj: Option<&ObjInfo> = obj_idx.and_then(|i| env.objs.get(i));
    let conn = msospt::is_connector_type(s.ty);
    let is_line = s.ty == 20 || conn || obj.is_some_and(|o| o.ot == 1);
    let is_text = s.ty == 202 || obj.is_some_and(|o| o.ot == 6);
    let word_art = if msospt::is_word_art_type(s.ty) { word_art_body(env, s, w, h) } else { None };
    let has_wa = word_art.is_some();
    let (geom, flip) =
        if word_art.is_some() { ("<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_owned(), false) } else { geometry_xml(s, w, h)? };
    let fill = if word_art.is_some() || is_line { "<a:noFill/>".to_owned() } else { fill_xml(env, s, bool_prop(s, 0x1bf, 4, true)) };
    let ln = if word_art.is_some() { "<a:ln><a:noFill/></a:ln>".to_owned() } else { line_xml(env, s, bool_prop(s, 0x1ff, 3, true)) };
    let effects = if word_art.is_some() { String::new() } else { shadow_xml(env, s) };
    let n1 = id.saturating_sub(1);
    let name = shape_name(
        s,
        &if is_text {
            format!("Caixa de Texto {n1}")
        } else if is_line {
            format!("Conector {n1}")
        } else {
            format!("Forma {n1}")
        },
    );
    let sp_pr = format!("<xdr:spPr>{}{geom}{fill}{ln}{effects}</xdr:spPr>", xfrm_xml(s, bx, flip, ""));
    let obj: Option<&ObjInfo> = obj_idx.and_then(|i| env.objs.get(i));
    if conn {
        return Ok(format!(
            "<xdr:cxnSp macro=\"\"><xdr:nvCxnSpPr><xdr:cNvPr id=\"{id}\" name=\"{}\"/><xdr:cNvCxnSpPr/></xdr:nvCxnSpPr>{sp_pr}</xdr:cxnSp>",
            xml_text_s(&name)
        ));
    }
    let body = match word_art {
        Some(b) => b,
        None => tx_body(env, s, obj),
    };
    Ok(format!(
        "<xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"{id}\" name=\"{}\"/><xdr:cNvSpPr{}/></xdr:nvSpPr>{sp_pr}{body}</xdr:sp>",
        xml_text_s(&name),
        if is_text || has_wa { " txBox=\"1\"" } else { "" }
    ))
}

/// Group: off/ext = the box in the parent (top level: the anchor rules); chOff/chExt = FSPGR.
fn group_xml(env: &mut Env<'_, '_, '_>, s: &XlsShape<'_>, bx: Option<BoxF>, id: u32) -> Result<String, Thrown> {
    let mut kids = String::new();
    for c in &s.children {
        kids.push_str(&object_xml(env, c, (0.0, 0.0), true)?);
    }
    if kids.is_empty() {
        return Ok(String::new());
    }
    let gr = s.group_rect.unwrap_or(OaRect { l: 0, t: 0, r: 1, b: 1 });
    let (gl, gt, gr_r, gr_b) = (f64::from(gr.l), f64::from(gr.t), f64::from(gr.r), f64::from(gr.b));
    let ch = format!(
        "<a:chOff x=\"{}\" y=\"{}\"/><a:chExt cx=\"{}\" cy=\"{}\"/>",
        to_js_string(min(gl, gr_r)),
        to_js_string(min(gt, gr_b)),
        to_js_string((gr_r - gl).abs()),
        to_js_string((gr_b - gt).abs())
    );
    let b = bx.unwrap_or(BoxF { x: min(gl, gr_r), y: min(gt, gr_b), w: (gr_r - gl).abs(), h: (gr_b - gt).abs() });
    Ok(format!(
        "<xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id=\"{id}\" name=\"{}\"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr><xdr:grpSpPr>{}</xdr:grpSpPr>{kids}</xdr:grpSp>",
        xml_text_s(&shape_name(s, &format!("Grupo {}", id.saturating_sub(1)))),
        xfrm_xml(s, Some(b), false, &ch)
    ))
}
