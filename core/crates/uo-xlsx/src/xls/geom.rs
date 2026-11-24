//! Geometry of the binary OfficeArt shapes ([MS-ODRAW]) → DrawingML (`odraw-geom.ts`): the msospt type → `<a:prstGeom>`
//! with the adjusts converted (the table is `uo_drawingml::odraw::msospt`, variant Sheet), a freeform
//! (pVertices/pSegmentInfo) → `<a:custGeom>`, the legacy arc and the classic WordArt warps.
//!
//! Where the TS reads past a record with a `DataView` it THROWS (the shape is then left out by the caller): [`Thrown`].

use std::f64::consts::PI;

use uo_drawingml::odraw::PropTable;
use uo_drawingml::odraw::msospt::{PresetVariant, preset_for};
use uo_jsnum::format::to_js_string;
use uo_jsnum::math::{ceil, cos, max, min, round, sin, tan};

/// A `RangeError` of the TS `DataView`: the shape being converted is dropped.
#[derive(Debug, Clone, Copy)]
pub struct Thrown;

/// `K = 100000 / 21600`.
const K: f64 = 100_000.0 / 21_600.0;

/// `Math.round(x)` printed as the TS template prints it.
pub fn rnd(x: f64) -> String {
    to_js_string(round(x))
}

/// `presetGeomXml`: `<a:prstGeom>` of the type + adjusts (w/h = the box's proportion); `None` = no preset. The flag is
/// the extra `flipV` of the mirrored presets.
pub fn preset_geom_xml(spt: u16, adjust: &[Option<f64>; 10], w: f64, h: f64) -> Option<(String, bool)> {
    let r = preset_for(spt, adjust, w, h, PresetVariant::Sheet)?;
    let mut av = String::new();
    for a in r.av() {
        av.push_str(&format!("<a:gd name=\"{}\" fmla=\"val {}\"/>", a.name, to_js_string(a.val)));
    }
    Some((format!("<a:prstGeom prst=\"{}\"><a:avLst>{av}</a:avLst></a:prstGeom>", r.prst), r.flip_v))
}

/// An `IMsoArray` (nElems, nElemsAlloc, cbElem + data): the count (limited to what the data holds) and the element size.
struct MsoArray<'a> {
    n: usize,
    size: usize,
    b: &'a [u8],
}

fn mso_array(b: Option<&[u8]>) -> Option<MsoArray<'_>> {
    let b = b.filter(|b| b.len() >= 6)?;
    let n = usize::from(u16::from_le_bytes([b[0], b[1]]));
    let mut size = usize::from(u16::from_le_bytes([b[4], b[5]]));
    if size == 0xfff0 {
        size = 4;
    }
    if size == 0 {
        return None;
    }
    Some(MsoArray { n: n.min((b.len() - 6) / size), size, b })
}

fn dv<const N: usize>(b: &[u8], o: usize) -> Result<[u8; N], Thrown> {
    let end = o.checked_add(N).ok_or(Thrown)?;
    <[u8; N]>::try_from(b.get(o..end).ok_or(Thrown)?).map_err(|_| Thrown)
}

fn i32_at(b: &[u8], o: usize) -> Result<i32, Thrown> {
    dv::<4>(b, o).map(i32::from_le_bytes)
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, Thrown> {
    dv::<4>(b, o).map(u32::from_le_bytes)
}

fn i16_at(b: &[u8], o: usize) -> Result<i16, Thrown> {
    dv::<2>(b, o).map(i16::from_le_bytes)
}

fn u16_at(b: &[u8], o: usize) -> Result<u16, Thrown> {
    dv::<2>(b, o).map(u16::from_le_bytes)
}

const LIMIT_PTS: usize = 100_000;

/// `custGeomXml`: a freeform geometry (pVertices 0x145 / pSegmentInfo 0x146 in the space geoLeft..geoBottom) →
/// `<a:custGeom>`; `Ok(None)` without vertices.
pub fn cust_geom_xml(props: &PropTable<'_>) -> Result<Option<String>, Thrown> {
    let Some(verts) = mso_array(props.get_complex(0x145)).filter(|v| v.n > 0) else { return Ok(None) };
    let mut pts: Vec<(i64, i64)> = Vec::new();
    for i in 0..verts.n.min(LIMIT_PTS) {
        let o = 6 + i * verts.size;
        if verts.size >= 8 {
            pts.push((i64::from(i32_at(verts.b, o)?), i64::from(i32_at(verts.b, o + 4)?)));
        } else {
            pts.push((i64::from(i16_at(verts.b, o)?), i64::from(i16_at(verts.b, o + 2)?)));
        }
    }
    let sp = |id: u16, d: i64| -> i64 { props.value(id).map_or(d, |v| i64::from(i32::from_le_bytes(v.to_le_bytes()))) };
    let (gl, gt, gr, gb) = (sp(0x140, 0), sp(0x141, 0), sp(0x142, 21600), sp(0x143, 21600));
    let w = (gr - gl).max(1);
    let h = (gb - gt).max(1);
    let pt = |p: Option<&(i64, i64)>| -> String {
        let (x, y) = p.copied().unwrap_or((0, 0));
        format!("<a:pt x=\"{}\" y=\"{}\"/>", x - gl, y - gt)
    };
    let segs = mso_array(props.get_complex(0x146));
    let mut paths: Vec<String> = Vec::new();
    let mut cur = String::new();
    let (mut no_fill, mut no_line) = (false, false);
    let end_path = |cur: &mut String, no_fill: &mut bool, no_line: &mut bool, paths: &mut Vec<String>| {
        if !cur.is_empty() {
            paths.push(format!(
                "<a:path w=\"{w}\" h=\"{h}\"{}{}>{cur}</a:path>",
                if *no_fill { " fill=\"none\"" } else { "" },
                if *no_line { " stroke=\"0\"" } else { "" }
            ));
        }
        cur.clear();
        *no_fill = false;
        *no_line = false;
    };
    let mut pi = 0usize;
    match segs.filter(|s| s.n > 0) {
        None => {
            // without segments: a polyline through the vertices (closed for shapePath 1/3)
            cur.push_str(&format!("<a:moveTo>{}</a:moveTo>", pt(pts.first())));
            for p in pts.iter().skip(1) {
                cur.push_str(&format!("<a:lnTo>{}</a:lnTo>", pt(Some(p))));
            }
            let shp = props.value(0x144).unwrap_or(1);
            if shp == 1 || shp == 3 {
                cur.push_str("<a:close/>");
            }
            end_path(&mut cur, &mut no_fill, &mut no_line, &mut paths);
        }
        Some(segs) => {
            // the segments consume vertices; corrupt counts stop when the points run out
            let mut i = 0usize;
            while i < segs.n && pi <= pts.len() {
                let o = 6 + i * segs.size;
                let s = if segs.size >= 4 { u32_at(segs.b, o)? } else { u32::from(u16_at(segs.b, o)?) };
                let (ty, count) = if segs.size >= 4 {
                    ((s >> 29) & 7, usize::try_from(s & 0x1fff_ffff).unwrap_or(usize::MAX))
                } else {
                    ((s >> 13) & 7, usize::try_from(s & 0x1fff).unwrap_or(0))
                };
                match ty {
                    0 => {
                        // lineTo
                        let mut k = 0usize;
                        while k < count.max(1) && pi < pts.len() {
                            cur.push_str(&format!("<a:lnTo>{}</a:lnTo>", pt(pts.get(pi))));
                            pi += 1;
                            k += 1;
                        }
                    }
                    1 => {
                        // curveTo (3 points per segment)
                        let mut k = 0usize;
                        while k < count.max(1) && pi + 2 < pts.len() {
                            cur.push_str(&format!(
                                "<a:cubicBezTo>{}{}{}</a:cubicBezTo>",
                                pt(pts.get(pi)),
                                pt(pts.get(pi + 1)),
                                pt(pts.get(pi + 2))
                            ));
                            pi += 3;
                            k += 1;
                        }
                    }
                    2 => {
                        cur.push_str(&format!("<a:moveTo>{}</a:moveTo>", pt(pts.get(pi))));
                        pi += 1;
                    }
                    3 => cur.push_str("<a:close/>"),
                    4 => end_path(&mut cur, &mut no_fill, &mut no_line, &mut paths),
                    5 => {
                        // escape
                        let code = (s >> 8) & 0x1f;
                        let n = usize::try_from(s & 0xff).unwrap_or(0);
                        if code == 10 {
                            no_fill = true;
                        } else if code == 11 {
                            no_line = true;
                        } else {
                            pi += n;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            end_path(&mut cur, &mut no_fill, &mut no_line, &mut paths);
        }
    }
    if paths.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"l\" t=\"t\" r=\"r\" b=\"b\"/><a:pathLst>{}</a:pathLst></a:custGeom>",
        paths.join("")
    )))
}

/// `arcGeomXml`: the legacy arc (msosptArc): the box is the ARC's + centre; adjust1/adjust2 = start/end angles (16.16
/// degrees, clockwise on screen, 270 = top; default 270 → 0). A custGeom with the outline (no fill) and the filled wedge.
pub fn arc_geom_xml(props: &PropTable<'_>) -> String {
    let deg = |id: u16, d: f64| -> f64 { props.value(id).map_or(d, |v| f64::from(i32::from_le_bytes(v.to_le_bytes())) / 65536.0) };
    let a0 = deg(0x147, 270.0);
    let mut a1 = deg(0x148, 0.0);
    while a1 <= a0 {
        a1 += 360.0;
    }
    while a1 - a0 > 360.0 {
        a1 -= 360.0;
    }
    let r = 21600.0;
    let pt = |a: f64| -> (f64, f64) { (r * cos((a * PI) / 180.0), r * sin((a * PI) / 180.0)) };
    let mut pts: Vec<(f64, f64)> = vec![(0.0, 0.0), pt(a0), pt(a1)];
    let mut k = ceil(a0 / 90.0) * 90.0;
    while k < a1 {
        pts.push(pt(k));
        k += 90.0;
    }
    let min_x = pts.iter().fold(f64::INFINITY, |m, p| min(m, p.0));
    let min_y = pts.iter().fold(f64::INFINITY, |m, p| min(m, p.1));
    let max_x = pts.iter().fold(f64::NEG_INFINITY, |m, p| max(m, p.0));
    let max_y = pts.iter().fold(f64::NEG_INFINITY, |m, p| max(m, p.1));
    let w = to_js_string(max(1.0, round(max_x - min_x)));
    let h = to_js_string(max(1.0, round(max_y - min_y)));
    let q = |p: (f64, f64)| -> String { format!("<a:pt x=\"{}\" y=\"{}\"/>", rnd(p.0 - min_x), rnd(p.1 - min_y)) };
    let n = max(1.0, ceil((a1 - a0) / 90.0));
    let step = (a1 - a0) / n;
    let kk = (4.0 / 3.0) * tan((step * PI) / 720.0);
    let mut arc = format!("<a:moveTo>{}</a:moveTo>", q(pt(a0)));
    let mut i = 0.0;
    while i < n {
        let s = ((a0 + i * step) * PI) / 180.0;
        let e = ((a0 + (i + 1.0) * step) * PI) / 180.0;
        let c1 = (r * (cos(s) - kk * sin(s)), r * (sin(s) + kk * cos(s)));
        let c2 = (r * (cos(e) + kk * sin(e)), r * (sin(e) - kk * cos(e)));
        arc.push_str(&format!("<a:cubicBezTo>{}{}{}</a:cubicBezTo>", q(c1), q(c2), q((r * cos(e), r * sin(e)))));
        i += 1.0;
    }
    let paths = format!(
        "<a:path w=\"{w}\" h=\"{h}\" fill=\"none\" extrusionOk=\"0\">{arc}</a:path><a:path w=\"{w}\" h=\"{h}\" stroke=\"0\" extrusionOk=\"0\">{arc}<a:lnTo>{}</a:lnTo><a:close/></a:path>",
        q((0.0, 0.0))
    );
    format!(
        "<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"l\" t=\"t\" r=\"r\" b=\"b\"/><a:pathLst>{paths}</a:pathLst></a:custGeom>"
    )
}

/// `WARPS`: the warps of the WordArt (msosptText* 136–175, the order of DrawingML).
pub const WARPS: [&str; 40] = [
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

const WARP_ANGLE: [&str; 8] =
    ["textArchUp", "textArchDown", "textCircle", "textButton", "textArchUpPour", "textArchDownPour", "textCirclePour", "textButtonPour"];

fn warp_bin_default(prst: &str) -> Option<f64> {
    match prst {
        "textWave1" | "textWave2" => Some(2809.0),
        "textDoubleWave1" | "textWave4" => Some(1404.0),
        "textInflateBottom" => Some(14706.0),
        _ => None,
    }
}

/// `warpAdjust`: the adjusts of the legacy WordArt → the avLst of the prstTxWarp (as the PowerPoint conversion does).
pub fn warp_adjust(prst: &str, raw: &[Option<f64>; 10], w: f64, h: f64) -> Vec<f64> {
    if WARP_ANGLE.contains(&prst) {
        let ang = |v: Option<f64>, dflt: f64| -> f64 {
            let Some(v) = v else { return dflt };
            let t = ((v / 65536.0) * PI) / 180.0;
            let mut d = (uo_jsnum::math::atan2(h * sin(t), w * cos(t)) * 180.0) / PI;
            if d < 0.0 {
                d += 360.0;
            }
            uo_jsnum::math::rem(round(d * 60000.0), 21_600_000.0)
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
            Some(v) => round(v * 2.0 * K),
        };
        return vec![a1, second];
    }
    let a0 = raw[0].or_else(|| warp_bin_default(prst));
    if prst.contains("Wave") {
        if a0.is_none() && raw[1].is_none() {
            return Vec::new();
        }
        return vec![round(a0.unwrap_or(0.0) * K), raw[1].map_or(0.0, |v| round((v - 10800.0) * K))];
    }
    match a0 {
        None => Vec::new(),
        Some(v) => vec![round(v * K)],
    }
}
