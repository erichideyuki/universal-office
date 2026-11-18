//! SVG pictures: a picture whose only data is an SVG (`asvg:svgBlip` with no raster in `r:embed`; PowerPoint 2016+
//! writes the PNG beside the SVG and the reader takes the PNG then, other producers sometimes write only the SVG).
//! The TS viewer has the browser decode it; here the static subset that Office icons and the usual producers write is
//! played as vector content into a form XObject of the picture's natural size (y down, like a metafile recipe), which
//! `drawImage` places as any picture.
//!
//! Drawn: `svg` (`viewBox`, `preserveAspectRatio`, nested viewports), `g`, `a`, `switch` (its first child), `use`
//! (elements and `symbol`s), `path` (the whole path grammar, arcs as cubics), `rect` (`rx`/`ry`), `circle`, `ellipse`,
//! `line`, `polyline`, `polygon`; presentation attributes, `style` attributes and `<style>` sheets (type, class, id and
//! universal selectors, by specificity); fill and stroke (colour, `currentColor`, opacity, rule, width, caps, joins,
//! miter limit, dashes), `opacity` (a transparency group), `transform`, `visibility`, `display`; `clip-path` (the shapes
//! of a `clipPath`, both units); linear and radial gradients (`href` inheritance, units, `gradientTransform`, stop
//! opacity as a soft mask). Counted and not drawn (`PdfReport::missing`): `svg:text`, `svg:image`, `svg:pattern` (a
//! pattern paint: the fallback colour or nothing), `svg:marker`; drawn without the effect: `svg:filter`, `svg:mask`,
//! `svg:spread` (reflect/repeat drawn as pad), `gradient:stroke` (the stops' average colour), `svg:effect` (an
//! `fxImage` colour effect on the picture); `svg:limit` when a picture exceeds the segment or content budget (what was
//! written stays). A file that does not parse as XML with an `svg` root is `image:unsupported`, as before.
//!
//! The raster interpreter has no SVG player: a soft-edge or reflection layer holding an SVG picture is drawn as vector
//! (`raster:fallback`, like a JPEG), and the canvas shadow of an SVG picture is the shadow of its box (`svg:shadow`).
//!
//! Natural size: `width`/`height` in absolute units, else the `viewBox` size, else 300 x 150 (one of them with a
//! `viewBox`: the other from its ratio). The viewBox is fitted into that box by `preserveAspectRatio`, and `drawImage`
//! stretches the box into the destination (a browser fits an SVG without intrinsic size into the destination itself:
//! the two differ only when the destination's ratio differs from the viewBox's).

use std::fmt::Write as _;

use rustc_hash::FxHashMap;
use uo_jsnum::math::{abs, atan2, cos, hypot, max, min, sin, sqrt, tan};
use uo_jsnum::parse::parse_float;
use uo_opc::xml::{self, ContentPi, Document, Element, ElementId, ParseOptions};

use super::css::{Rgba, parse_css};
use super::geom::{Mat, n, nm};
use super::interp::Interp;
use super::{ImgHandle, PicSrc};

/// Nesting of elements and `use` references followed.
const MAX_DEPTH: u32 = 64;
/// Path segments of one picture.
const MAX_SEGS: usize = 1_000_000;
/// Content bytes of one picture (all its forms).
const MAX_CONTENT: usize = 64 << 20;
/// Elements of one SVG document.
const MAX_ELEMENTS: u32 = 200_000;
/// The arc of a cubic piece of an elliptical arc (a quarter turn).
const QUARTER: f64 = std::f64::consts::PI / 2.0;
/// The cubic of a quarter ellipse.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// Whether the bytes are an SVG document (the start of the file, after a byte order mark and white space, is markup,
/// and an `svg` element starts within the first 4 KB). No raster format starts with `<`.
pub(super) fn is_svg(data: &[u8]) -> bool {
    let head = data.get(..data.len().min(4096)).unwrap_or_default();
    let head = head.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(head);
    let start = head.iter().position(|b| !ws(*b));
    if start.and_then(|i| head.get(i)) != Some(&b'<') {
        return false;
    }
    let lower: Vec<u8> = head.iter().map(u8::to_ascii_lowercase).collect();
    lower.windows(4).any(|w| w == b"<svg" || w == b":svg")
}

/// SVG white space (XML S, and the form feed CSS allows).
fn ws(c: u8) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c)
}

/// `uo_jsnum::text::trim` as a method.
trait JsTrim {
    fn jtrim(&self) -> &str;
}

impl JsTrim for str {
    fn jtrim(&self) -> &str {
        uo_jsnum::text::trim(self)
    }
}

// ------------------------------------------------------------------------------------------------ paths

#[derive(Clone, Copy, Debug)]
enum Seg {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Z,
}

#[derive(Default)]
struct Path {
    segs: Vec<Seg>,
    cur: (f64, f64),
    start: (f64, f64),
}

impl Path {
    fn move_to(&mut self, x: f64, y: f64) {
        self.segs.push(Seg::M(x, y));
        self.cur = (x, y);
        self.start = (x, y);
    }
    fn line_to(&mut self, x: f64, y: f64) {
        self.segs.push(Seg::L(x, y));
        self.cur = (x, y);
    }
    fn cubic(&mut self, c: [f64; 6]) {
        let [a, b, cc, d, e, f] = c;
        self.segs.push(Seg::C(a, b, cc, d, e, f));
        self.cur = (e, f);
    }
    fn close(&mut self) {
        self.segs.push(Seg::Z);
        self.cur = self.start;
    }

    /// The SVG elliptical arc from the current point to `(x, y)` (implementation notes F.6.5/F.6.6) as cubics of at
    /// most a quarter turn.
    #[allow(clippy::too_many_arguments, clippy::similar_names)]
    fn arc(&mut self, rx: f64, ry: f64, rot_deg: f64, large: bool, sweep: bool, x: f64, y: f64) {
        let (x1, y1) = self.cur;
        if x1 == x && y1 == y {
            return;
        }
        let (mut rx, mut ry) = (abs(rx), abs(ry));
        if rx == 0.0 || ry == 0.0 {
            self.line_to(x, y);
            return;
        }
        let phi = rot_deg * std::f64::consts::PI / 180.0;
        let (sp, cp) = (sin(phi), cos(phi));
        let (dx, dy) = ((x1 - x) / 2.0, (y1 - y) / 2.0);
        let x1p = cp * dx + sp * dy;
        let y1p = -sp * dx + cp * dy;
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = sqrt(lambda);
            rx *= s;
            ry *= s;
        }
        let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
        let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
        let mut k = if den == 0.0 { 0.0 } else { sqrt(max(0.0, num / den)) };
        if large == sweep {
            k = -k;
        }
        let cxp = k * rx * y1p / ry;
        let cyp = -k * ry * x1p / rx;
        let cx = cp * cxp - sp * cyp + (x1 + x) / 2.0;
        let cy = sp * cxp + cp * cyp + (y1 + y) / 2.0;
        let ang = |ux: f64, uy: f64, vx: f64, vy: f64| atan2(ux * vy - uy * vx, ux * vx + uy * vy);
        let t1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
        let mut dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
        if !sweep && dt > 0.0 {
            dt -= 2.0 * std::f64::consts::PI;
        } else if sweep && dt < 0.0 {
            dt += 2.0 * std::f64::consts::PI;
        }
        if !(dt.is_finite() && t1.is_finite() && cx.is_finite() && cy.is_finite()) {
            self.line_to(x, y);
            return;
        }
        let mut pieces = 1u32;
        while f64::from(pieces) * QUARTER < abs(dt) - 1e-9 && pieces < 8 {
            pieces += 1;
        }
        let step = dt / f64::from(pieces);
        let h = 4.0 / 3.0 * tan(step / 4.0);
        let pt = |t: f64| {
            let (ct, st) = (cos(t), sin(t));
            let (ex, ey) = (rx * ct, ry * st);
            (cp * ex - sp * ey + cx, sp * ex + cp * ey + cy)
        };
        let der = |t: f64| {
            let (ct, st) = (cos(t), sin(t));
            let (ex, ey) = (-rx * st, ry * ct);
            (cp * ex - sp * ey, sp * ex + cp * ey)
        };
        let mut t = t1;
        for i in 0..pieces {
            let t2 = if i + 1 == pieces { t1 + dt } else { t + step };
            let (p0, d0) = (pt(t), der(t));
            let (p3, d3) = (if i + 1 == pieces { (x, y) } else { pt(t2) }, der(t2));
            self.cubic([p0.0 + h * d0.0, p0.1 + h * d0.1, p3.0 - h * d3.0, p3.1 - h * d3.1, p3.0, p3.1]);
            t = t2;
        }
    }

    /// An axis-aligned ellipse as four cubics, clockwise from the right (as the browsers start a `circle`).
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) {
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        self.move_to(cx + rx, cy);
        self.cubic([cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry]);
        self.cubic([cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy]);
        self.cubic([cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry]);
        self.cubic([cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy]);
        self.close();
    }

    /// The bounding box of the points (control points included: exact for lines, rectangles and ellipses).
    fn bbox(&self) -> Option<[f64; 4]> {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        let mut add = |x: f64, y: f64| {
            b = [min(b[0], x), min(b[1], y), max(b[2], x), max(b[3], y)];
        };
        for s in &self.segs {
            match *s {
                Seg::M(x, y) | Seg::L(x, y) => add(x, y),
                Seg::C(a, bb, c, d, e, f) => {
                    add(a, bb);
                    add(c, d);
                    add(e, f);
                }
                Seg::Z => {}
            }
        }
        b.iter().all(|v| v.is_finite()).then_some(b)
    }

    /// The path operators, each point through `m` (`None`: as they are).
    fn emit(&self, m: Option<Mat>, out: &mut String) {
        let p = |x: f64, y: f64| m.map_or((x, y), |m| m.apply(x, y));
        for s in &self.segs {
            match *s {
                Seg::M(x, y) => {
                    let (x, y) = p(x, y);
                    let _ = writeln!(out, "{} {} m", n(x), n(y));
                }
                Seg::L(x, y) => {
                    let (x, y) = p(x, y);
                    let _ = writeln!(out, "{} {} l", n(x), n(y));
                }
                Seg::C(a, b, c, d, e, f) => {
                    let ((a, b), (c, d), (e, f)) = (p(a, b), p(c, d), p(e, f));
                    let _ = writeln!(out, "{} {} {} {} {} {} c", n(a), n(b), n(c), n(d), n(e), n(f));
                }
                Seg::Z => out.push_str("h\n"),
            }
        }
    }
}

/// The numbers and flags of path data, points lists and transforms.
struct Lex<'a> {
    s: &'a [u8],
    p: usize,
}

impl Lex<'_> {
    fn skip(&mut self) {
        while let Some(c) = self.s.get(self.p) {
            if ws(*c) || *c == b',' {
                self.p += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.p).copied()
    }

    /// A number (`-1.5e-3`, `.5`, `1.` — `1.5.5` is two numbers); `None` where none starts.
    fn num(&mut self) -> Option<f64> {
        self.skip();
        let st = self.p;
        let mut i = self.p;
        let at = |i: usize| self.s.get(i).copied();
        if matches!(at(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let mut digits = false;
        while at(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
            digits = true;
        }
        if at(i) == Some(b'.') {
            i += 1;
            while at(i).is_some_and(|c| c.is_ascii_digit()) {
                i += 1;
                digits = true;
            }
        }
        if !digits {
            return None;
        }
        if matches!(at(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(at(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if at(j).is_some_and(|c| c.is_ascii_digit()) {
                while at(j).is_some_and(|c| c.is_ascii_digit()) {
                    j += 1;
                }
                i = j;
            }
        }
        let v = parse_float(std::str::from_utf8(self.s.get(st..i)?).ok()?);
        self.p = i;
        v.is_finite().then_some(v)
    }

    /// An arc flag: one `0` or `1` (no separator needed after it).
    fn flag(&mut self) -> Option<bool> {
        let c = self.peek()?;
        let v = match c {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.p += 1;
        Some(v)
    }
}

/// Path data (`d`): drawn up to the first error, as the browsers do.
#[allow(clippy::too_many_lines)]
fn parse_path(d: &str) -> Path {
    let mut p = Path::default();
    let mut lx = Lex { s: d.as_bytes(), p: 0 };
    let mut cmd = 0u8;
    let mut last_ctrl: Option<(f64, f64)> = None;
    let mut last_q: Option<(f64, f64)> = None;
    let mut started = false;
    while let Some(c) = lx.peek() {
        if c.is_ascii_alphabetic() {
            cmd = c;
            lx.p += 1;
        } else if cmd == 0 {
            break;
        } else if cmd == b'M' {
            cmd = b'L';
        } else if cmd == b'm' {
            cmd = b'l';
        } else if cmd == b'Z' || cmd == b'z' {
            break;
        }
        if !started && !matches!(cmd, b'M' | b'm') {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { p.cur } else { (0.0, 0.0) };
        let (mut ctrl, mut qctrl) = (None, None);
        let ok = (|| -> Option<()> {
            match cmd.to_ascii_uppercase() {
                b'M' => {
                    let (x, y) = (lx.num()?, lx.num()?);
                    p.move_to(ox + x, oy + y);
                    started = true;
                }
                b'L' => {
                    let (x, y) = (lx.num()?, lx.num()?);
                    p.line_to(ox + x, oy + y);
                }
                b'H' => {
                    let x = lx.num()?;
                    let y = p.cur.1;
                    p.line_to(if rel { p.cur.0 + x } else { x }, y);
                }
                b'V' => {
                    let y = lx.num()?;
                    let x = p.cur.0;
                    p.line_to(x, if rel { p.cur.1 + y } else { y });
                }
                b'C' => {
                    let v = [lx.num()?, lx.num()?, lx.num()?, lx.num()?, lx.num()?, lx.num()?];
                    let c = [ox + v[0], oy + v[1], ox + v[2], oy + v[3], ox + v[4], oy + v[5]];
                    ctrl = Some((c[2], c[3]));
                    p.cubic(c);
                }
                b'S' => {
                    let v = [lx.num()?, lx.num()?, lx.num()?, lx.num()?];
                    let (cx, cy) = p.cur;
                    let (r1x, r1y) = last_ctrl.map_or((cx, cy), |(lx0, ly0)| (2.0 * cx - lx0, 2.0 * cy - ly0));
                    let c = [r1x, r1y, ox + v[0], oy + v[1], ox + v[2], oy + v[3]];
                    ctrl = Some((c[2], c[3]));
                    p.cubic(c);
                }
                b'Q' => {
                    let v = [lx.num()?, lx.num()?, lx.num()?, lx.num()?];
                    let (qx, qy, x, y) = (ox + v[0], oy + v[1], ox + v[2], oy + v[3]);
                    let (cx, cy) = p.cur;
                    qctrl = Some((qx, qy));
                    p.cubic(quad(cx, cy, qx, qy, x, y));
                }
                b'T' => {
                    let (x, y) = (ox + lx.num()?, oy + lx.num()?);
                    let (cx, cy) = p.cur;
                    let (qx, qy) = last_q.map_or((cx, cy), |(a, b)| (2.0 * cx - a, 2.0 * cy - b));
                    qctrl = Some((qx, qy));
                    p.cubic(quad(cx, cy, qx, qy, x, y));
                }
                b'A' => {
                    let (rx, ry, rot) = (lx.num()?, lx.num()?, lx.num()?);
                    let (large, sweep) = (lx.flag()?, lx.flag()?);
                    let (x, y) = (ox + lx.num()?, oy + lx.num()?);
                    p.arc(rx, ry, rot, large, sweep, x, y);
                }
                b'Z' => p.close(),
                _ => return None,
            }
            Some(())
        })();
        if ok.is_none() {
            break;
        }
        last_ctrl = ctrl;
        last_q = qctrl;
        if p.segs.len() > MAX_SEGS {
            break;
        }
    }
    p
}

fn quad(x0: f64, y0: f64, qx: f64, qy: f64, x: f64, y: f64) -> [f64; 6] {
    [x0 + 2.0 / 3.0 * (qx - x0), y0 + 2.0 / 3.0 * (qy - y0), x + 2.0 / 3.0 * (qx - x), y + 2.0 / 3.0 * (qy - y), x, y]
}

/// A `transform` list (`matrix`, `translate`, `scale`, `rotate`, `skewX`, `skewY`): the matrix that takes the
/// element's coordinates to its parent's. Stops at the first error with what was read (the browsers drop the whole
/// attribute: an invalid list is the identity).
fn parse_transform(s: &str) -> Mat {
    let mut acc = Mat::IDENTITY;
    let b = s.as_bytes();
    let mut lx = Lex { s: b, p: 0 };
    loop {
        lx.skip();
        let st = lx.p;
        while lx.s.get(lx.p).is_some_and(u8::is_ascii_alphabetic) {
            lx.p += 1;
        }
        let Some(name) = b.get(st..lx.p) else { return Mat::IDENTITY };
        if name.is_empty() {
            return if lx.p >= b.len() { acc } else { Mat::IDENTITY };
        }
        if lx.peek() != Some(b'(') {
            return Mat::IDENTITY;
        }
        lx.p += 1;
        let mut args: Vec<f64> = Vec::new();
        while let Some(v) = lx.num() {
            args.push(v);
            if args.len() > 6 {
                return Mat::IDENTITY;
            }
        }
        if lx.peek() != Some(b')') {
            return Mat::IDENTITY;
        }
        lx.p += 1;
        let a = |i: usize| args.get(i).copied();
        let deg = |v: f64| v * std::f64::consts::PI / 180.0;
        let t = match (name, args.len()) {
            (b"matrix", 6) => Mat([args[0], args[1], args[2], args[3], args[4], args[5]]),
            (b"translate", 1 | 2) => Mat::translate(a(0).unwrap_or(0.0), a(1).unwrap_or(0.0)),
            (b"scale", 1 | 2) => {
                let sx = a(0).unwrap_or(1.0);
                Mat::scale(sx, a(1).unwrap_or(sx))
            }
            (b"rotate", 1) => Mat::rotate(deg(a(0).unwrap_or(0.0))),
            (b"rotate", 3) => {
                let (cx, cy) = (a(1).unwrap_or(0.0), a(2).unwrap_or(0.0));
                Mat::translate(-cx, -cy).then(Mat::rotate(deg(a(0).unwrap_or(0.0)))).then(Mat::translate(cx, cy))
            }
            (b"skewX", 1) => Mat([1.0, 0.0, tan(deg(a(0).unwrap_or(0.0))), 1.0, 0.0, 0.0]),
            (b"skewY", 1) => Mat([1.0, tan(deg(a(0).unwrap_or(0.0))), 0.0, 1.0, 0.0, 0.0]),
            _ => return Mat::IDENTITY,
        };
        // `A B`: a point of the element goes through B, then A
        acc = t.then(acc);
    }
}

fn points(s: &str) -> Vec<(f64, f64)> {
    let mut lx = Lex { s: s.as_bytes(), p: 0 };
    let mut out = Vec::new();
    while let (Some(x), Some(y)) = (lx.num(), lx.num()) {
        out.push((x, y));
    }
    out
}

// ------------------------------------------------------------------------------------------------ values

/// A length in user units: `px` and plain numbers 1, `pt`, `pc`, `mm`, `cm`, `in`, `em` (16 px), `ex` (8 px), `%` of
/// `pct_of`. `None` when it does not read.
fn length(s: &str, pct_of: f64) -> Option<f64> {
    let t = s.jtrim();
    let mut lx = Lex { s: t.as_bytes(), p: 0 };
    let v = lx.num()?;
    let unit = t.get(lx.p..)?.jtrim();
    let k = match unit {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        "em" => 16.0,
        "ex" => 8.0,
        "%" => pct_of / 100.0,
        _ => return None,
    };
    Some(v * k)
}

/// The CSS colour names `parse_css` does not know (it reads the ones of the display list).
const NAMED: &str = "aliceblue f0f8ff antiquewhite faebd7 aquamarine 7fffd4 azure f0ffff beige f5f5dc bisque ffe4c4 \
blanchedalmond ffebcd blueviolet 8a2be2 brown a52a2a burlywood deb887 cadetblue 5f9ea0 chartreuse 7fff00 chocolate d2691e \
coral ff7f50 cornflowerblue 6495ed cornsilk fff8dc crimson dc143c darkblue 00008b darkcyan 008b8b darkgoldenrod b8860b \
darkgray a9a9a9 darkgrey a9a9a9 darkgreen 006400 darkkhaki bdb76b darkmagenta 8b008b darkolivegreen 556b2f darkorange ff8c00 \
darkorchid 9932cc darkred 8b0000 darksalmon e9967a darkseagreen 8fbc8f darkslateblue 483d8b darkslategray 2f4f4f \
darkslategrey 2f4f4f darkturquoise 00ced1 darkviolet 9400d3 deeppink ff1493 deepskyblue 00bfff dimgray 696969 dimgrey 696969 \
dodgerblue 1e90ff firebrick b22222 floralwhite fffaf0 forestgreen 228b22 gainsboro dcdcdc ghostwhite f8f8ff gold ffd700 \
goldenrod daa520 greenyellow adff2f honeydew f0fff0 hotpink ff69b4 indianred cd5c5c indigo 4b0082 ivory fffff0 khaki f0e68c \
lavender e6e6fa lavenderblush fff0f5 lawngreen 7cfc00 lemonchiffon fffacd lightblue add8e6 lightcoral f08080 lightcyan e0ffff \
lightgoldenrodyellow fafad2 lightgray d3d3d3 lightgrey d3d3d3 lightgreen 90ee90 lightpink ffb6c1 lightsalmon ffa07a \
lightseagreen 20b2aa lightskyblue 87cefa lightslategray 778899 lightslategrey 778899 lightsteelblue b0c4de lightyellow ffffe0 \
limegreen 32cd32 linen faf0e6 mediumaquamarine 66cdaa mediumblue 0000cd mediumorchid ba55d3 mediumpurple 9370db \
mediumseagreen 3cb371 mediumslateblue 7b68ee mediumspringgreen 00fa9a mediumturquoise 48d1cc mediumvioletred c71585 \
midnightblue 191970 mintcream f5fffa mistyrose ffe4e1 moccasin ffe4b5 navajowhite ffdead oldlace fdf5e6 olivedrab 6b8e23 \
orangered ff4500 orchid da70d6 palegoldenrod eee8aa palegreen 98fb98 paleturquoise afeeee palevioletred db7093 papayawhip ffefd5 \
peachpuff ffdab9 peru cd853f pink ffc0cb plum dda0dd powderblue b0e0e6 rebeccapurple 663399 rosybrown bc8f8f royalblue 4169e1 \
saddlebrown 8b4513 salmon fa8072 sandybrown f4a460 seagreen 2e8b57 seashell fff5ee sienna a0522d skyblue 87ceeb slateblue 6a5acd \
slategray 708090 slategrey 708090 snow fffafa springgreen 00ff7f steelblue 4682b4 tan d2b48c thistle d8bfd8 tomato ff6347 \
turquoise 40e0d0 violet ee82ee wheat f5deb3 whitesmoke f5f5f5 yellowgreen 9acd32";

fn colour(s: &str) -> Option<Rgba> {
    if let Some(c) = parse_css(s) {
        return Some(c);
    }
    let want = s.jtrim().to_ascii_lowercase();
    let mut it = NAMED.split_ascii_whitespace();
    while let (Some(name), Some(hex)) = (it.next(), it.next()) {
        if name == want {
            return parse_css(&format!("#{hex}"));
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
enum Paint {
    None,
    Colour(Rgba),
    Current,
    /// `url(#id)` with the fallback after it
    Url(String, Option<Box<Paint>>),
}

fn paint(s: &str) -> Option<Paint> {
    let t = s.jtrim();
    if t == "none" {
        return Some(Paint::None);
    }
    if t.eq_ignore_ascii_case("currentcolor") {
        return Some(Paint::Current);
    }
    if let Some(rest) = t.strip_prefix("url(") {
        let (inner, after) = rest.split_once(')')?;
        let id = inner.jtrim().trim_matches(['"', '\'']).jtrim();
        let id = id.strip_prefix('#')?.to_owned();
        let fb = after.jtrim();
        let fallback = if fb.is_empty() { None } else { paint(fb).map(Box::new) };
        return Some(Paint::Url(id, fallback));
    }
    colour(t).map(Paint::Colour)
}

fn url_id(s: &str) -> Option<&str> {
    let rest = s.jtrim().strip_prefix("url(")?;
    let (inner, _) = rest.split_once(')')?;
    inner.jtrim().trim_matches(['"', '\'']).jtrim().strip_prefix('#')
}

fn unit(s: &str) -> Option<f64> {
    let t = s.jtrim();
    let v = if let Some(p) = t.strip_suffix('%') { parse_float(p) / 100.0 } else { parse_float(t) };
    v.is_finite().then(|| max(0.0, min(1.0, v)))
}

/// The inherited properties (and the few that are not inherited, reset per element by the caller).
#[derive(Clone, Debug)]
struct Style {
    fill: Paint,
    fill_op: f64,
    fill_eo: bool,
    stroke: Paint,
    stroke_op: f64,
    width: f64,
    cap: u8,
    join: u8,
    miter: f64,
    dash: Vec<f64>,
    dash_off: f64,
    color: Rgba,
    visible: bool,
    clip_eo: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            fill: Paint::Colour(Rgba::BLACK),
            fill_op: 1.0,
            fill_eo: false,
            stroke: Paint::None,
            stroke_op: 1.0,
            width: 1.0,
            cap: 0,
            join: 0,
            miter: 4.0,
            dash: Vec::new(),
            dash_off: 0.0,
            color: Rgba::BLACK,
            visible: true,
            clip_eo: false,
        }
    }
}

/// The non-inherited properties of one element.
#[derive(Default)]
struct Own {
    opacity: f64,
    display_none: bool,
    clip: Option<String>,
    filter: bool,
    mask: bool,
    marker: bool,
}

/// One rule of a `<style>` sheet: its simple selector, specificity, order and declarations.
struct Rule {
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
    spec: (u32, u32, u32),
    order: usize,
    decls: Vec<(String, String)>,
}

fn declarations(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let v = v.jtrim().trim_end_matches("!important").jtrim();
            Some((k.jtrim().to_ascii_lowercase(), v.to_owned()))
        })
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        .collect()
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(rest.get(..i).unwrap_or_default());
        rest = match rest.get(i + 2..).and_then(|r| r.find("*/").map(|j| (r, j))) {
            Some((r, j)) => r.get(j + 2..).unwrap_or_default(),
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// The rules of a sheet with simple selectors (`tag`, `.class`, `#id`, `tag.class`, `*`); combinators, attribute
/// selectors, pseudo-classes and at-rules are not matched.
fn sheet(css: &str, rules: &mut Vec<Rule>) {
    let css = strip_comments(css);
    for block in css.split('}') {
        let Some((sel, body)) = block.split_once('{') else { continue };
        if sel.jtrim().starts_with('@') {
            continue;
        }
        let decls = declarations(body);
        if decls.is_empty() {
            continue;
        }
        for one in sel.split(',') {
            let one = one.jtrim();
            if one.is_empty() || one.contains([' ', '>', '+', '~', '[', ':']) {
                continue;
            }
            let mut tag = None;
            let mut classes = Vec::new();
            let mut id = None;
            let mut cur = String::new();
            let mut kind = b't';
            let mut flush = |kind: u8, cur: &mut String| {
                if !cur.is_empty() {
                    match kind {
                        b'.' => classes.push(std::mem::take(cur)),
                        b'#' => id = Some(std::mem::take(cur)),
                        _ => {
                            if cur != "*" {
                                tag = Some(std::mem::take(cur));
                            }
                            cur.clear();
                        }
                    }
                }
            };
            for ch in one.chars() {
                if ch == '.' || ch == '#' {
                    flush(kind, &mut cur);
                    kind = if ch == '.' { b'.' } else { b'#' };
                } else {
                    cur.push(ch);
                }
            }
            flush(kind, &mut cur);
            let spec = (u32::from(id.is_some()), u32::try_from(classes.len()).unwrap_or(u32::MAX), u32::from(tag.is_some()));
            let order = rules.len();
            rules.push(Rule { tag, classes, id, spec, order, decls: decls.clone() });
        }
    }
}

const PRESENTATION: &[&str] = &[
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-opacity",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "color",
    "visibility",
    "display",
    "opacity",
    "clip-path",
    "clip-rule",
    "filter",
    "mask",
    "marker-start",
    "marker-mid",
    "marker-end",
    "marker",
    "stop-color",
    "stop-opacity",
];

// ------------------------------------------------------------------------------------------------ the player

/// The parsed picture and what playing it needs.
struct Svg<'d> {
    doc: &'d Document,
    ids: FxHashMap<&'d str, ElementId>,
    rules: Vec<Rule>,
    /// the root viewport (percentages)
    vw: f64,
    vh: f64,
    /// the picture's box (the form's)
    w: f64,
    h: f64,
    segs: usize,
    bytes: usize,
    over: bool,
    out: String,
    /// gradient and soft-mask shadings by (gradient element, bbox key)
    shadings: FxHashMap<String, Option<String>>,
}

impl<'d> Svg<'d> {
    fn el(&self, id: &str) -> Option<Element<'d>> {
        self.ids.get(id).and_then(|i| self.doc.element(*i))
    }

    /// The declarations of an element in cascade order: presentation attributes, sheet rules (by specificity, then
    /// order), the `style` attribute.
    fn cascade(&self, e: Element<'d>) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for a in e.attrs() {
            let k = a.name.rsplit(':').next().unwrap_or(a.name);
            if PRESENTATION.contains(&k) {
                out.push((k.to_owned(), a.value.jtrim().to_owned()));
            }
        }
        if !self.rules.is_empty() {
            let tag = e.local_name();
            let classes: Vec<&str> = e.attr("class").map(|c| c.split_ascii_whitespace().collect()).unwrap_or_default();
            let id = e.attr("id");
            let mut hits: Vec<&Rule> = self
                .rules
                .iter()
                .filter(|r| {
                    r.tag.as_deref().is_none_or(|t| t == tag)
                        && r.id.as_deref().is_none_or(|i| Some(i) == id)
                        && r.classes.iter().all(|c| classes.contains(&c.as_str()))
                })
                .collect();
            hits.sort_by_key(|r| (r.spec, r.order));
            for r in hits {
                out.extend(r.decls.iter().cloned());
            }
        }
        if let Some(s) = e.attr("style") {
            out.extend(declarations(s));
        }
        out
    }

    /// The style of an element from its parent's and its declarations, and its own non-inherited properties.
    fn style(&self, parent: &Style, decls: &[(String, String)]) -> (Style, Own) {
        let mut s = parent.clone();
        let mut own = Own { opacity: 1.0, ..Own::default() };
        let diag = hypot(self.vw, self.vh) / std::f64::consts::SQRT_2;
        for (k, v) in decls {
            let v = v.as_str();
            if v == "inherit" {
                continue;
            }
            match k.as_str() {
                "fill" => {
                    if let Some(p) = paint(v) {
                        s.fill = p;
                    }
                }
                "stroke" => {
                    if let Some(p) = paint(v) {
                        s.stroke = p;
                    }
                }
                "fill-opacity" => s.fill_op = unit(v).unwrap_or(s.fill_op),
                "stroke-opacity" => s.stroke_op = unit(v).unwrap_or(s.stroke_op),
                "opacity" => own.opacity = unit(v).unwrap_or(1.0),
                "fill-rule" => s.fill_eo = v == "evenodd",
                "clip-rule" => s.clip_eo = v == "evenodd",
                "stroke-width" => {
                    if let Some(w) = length(v, diag).filter(|w| *w >= 0.0) {
                        s.width = w;
                    }
                }
                "stroke-linecap" => {
                    s.cap = match v {
                        "round" => 1,
                        "square" => 2,
                        _ => 0,
                    }
                }
                "stroke-linejoin" => {
                    s.join = match v {
                        "round" => 1,
                        "bevel" => 2,
                        _ => 0,
                    }
                }
                "stroke-miterlimit" => {
                    let m = parse_float(v);
                    if m.is_finite() && m >= 1.0 {
                        s.miter = m;
                    }
                }
                "stroke-dasharray" => {
                    if v == "none" {
                        s.dash.clear();
                    } else {
                        let d: Vec<Option<f64>> = v.split([',', ' ']).filter(|x| !x.jtrim().is_empty()).map(|x| length(x, diag)).collect();
                        if d.iter().all(|x| x.is_some_and(|x| x >= 0.0)) {
                            let mut d: Vec<f64> = d.into_iter().flatten().collect();
                            if d.len() % 2 == 1 {
                                d.extend_from_within(..);
                            }
                            s.dash = if d.iter().all(|x| *x == 0.0) { Vec::new() } else { d };
                        }
                    }
                }
                "stroke-dashoffset" => s.dash_off = length(v, diag).unwrap_or(s.dash_off),
                "color" => {
                    if let Some(c) = colour(v) {
                        s.color = c;
                    }
                }
                "visibility" => s.visible = v == "visible",
                "display" => own.display_none = v == "none",
                "clip-path" => own.clip = url_id(v).map(str::to_owned),
                "filter" => own.filter = v != "none",
                "mask" => own.mask = v != "none",
                "marker-start" | "marker-mid" | "marker-end" | "marker" => own.marker |= v != "none",
                _ => {}
            }
        }
        (s, own)
    }

    fn put(&mut self, s: &str) {
        self.bytes += s.len();
        if self.bytes > MAX_CONTENT {
            self.over = true;
            return;
        }
        self.out.push_str(s);
    }
}

/// `preserveAspectRatio` of a viewport `vw x vh` at `(x, y)` showing the `viewBox` `vb`: the matrix from the viewBox to
/// the viewport.
fn view_box(vb: [f64; 4], par: Option<&str>, x: f64, y: f64, vw: f64, vh: f64) -> Mat {
    let par = par.unwrap_or("xMidYMid meet").jtrim();
    let mut words = par.split_ascii_whitespace();
    let mut align = words.next().unwrap_or("xMidYMid");
    if align == "defer" {
        align = words.next().unwrap_or("xMidYMid");
    }
    let slice = words.next() == Some("slice");
    let (sx, sy) = (vw / vb[2], vh / vb[3]);
    if align == "none" {
        return Mat([sx, 0.0, 0.0, sy, x - vb[0] * sx, y - vb[1] * sy]);
    }
    let s = if slice { max(sx, sy) } else { min(sx, sy) };
    let f = |a: &str| {
        if a.contains("Mid") {
            0.5
        } else if a.contains("Max") {
            1.0
        } else {
            0.0
        }
    };
    let (ax, ay) = match align.find('Y') {
        Some(i) => (f(align.get(..i).unwrap_or_default()), f(align.get(i..).unwrap_or_default())),
        None => (0.5, 0.5),
    };
    let tx = x - vb[0] * s + (vw - vb[2] * s) * ax;
    let ty = y - vb[1] * s + (vh - vb[3] * s) * ay;
    Mat([s, 0.0, 0.0, s, tx, ty])
}

fn view_box_of(e: Element<'_>) -> Option<[f64; 4]> {
    let mut lx = Lex { s: e.attr("viewBox")?.as_bytes(), p: 0 };
    let vb = [lx.num()?, lx.num()?, lx.num()?, lx.num()?];
    (vb[2] > 0.0 && vb[3] > 0.0).then_some(vb)
}

fn cm(m: Mat) -> String {
    let [a, b, c, d, e, f] = m.0;
    format!("{} {} {} {} {} {} cm\n", nm(a), nm(b), nm(c), nm(d), nm(e), nm(f))
}

fn rgb(c: Rgba) -> String {
    let b = c.bytes();
    format!("{} {} {}", nm(f64::from(b[0]) / 255.0), nm(f64::from(b[1]) / 255.0), nm(f64::from(b[2]) / 255.0))
}

impl Interp<'_, '_> {
    /// An SVG picture as a form of its natural size; `None` (counted `image:unsupported`) when it does not parse.
    pub(super) fn svg_picture(&mut self, data: &[u8], fx: bool) -> Option<ImgHandle> {
        let text = String::from_utf8_lossy(data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data)).into_owned();
        let opts =
            ParseOptions { content_pi: ContentPi::Skip, max_depth: Some(256), max_elements: Some(MAX_ELEMENTS), ..ParseOptions::LENIENT };
        let Ok(doc) = xml::parse(text, &opts) else {
            self.count("image:unsupported");
            return None;
        };
        let root = doc.root();
        if root.local_name() != "svg" {
            self.count("image:unsupported");
            return None;
        }
        if fx {
            self.count("svg:effect");
        }
        // the natural size: width/height, else the viewBox's, else the replaced-element default 300 x 150
        let vb = view_box_of(root);
        let abs_len = |k: &str| root.attr(k).filter(|v| !v.jtrim().ends_with('%')).and_then(|v| length(v, 0.0)).filter(|v| *v > 0.0);
        let (mut w, mut h) = (abs_len("width"), abs_len("height"));
        let ratio = vb.map(|v| v[2] / v[3]);
        match (w, h, ratio) {
            (Some(_), Some(_), _) => {}
            (Some(x), None, Some(r)) => h = Some(x / r),
            (None, Some(y), Some(r)) => w = Some(y * r),
            (None, None, Some(_)) => {
                let v = vb.unwrap_or([0.0, 0.0, 300.0, 150.0]);
                (w, h) = (Some(v[2]), Some(v[3]));
            }
            _ => {}
        }
        let (w, h) = (w.unwrap_or(300.0), h.unwrap_or(150.0));
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            self.count("image:unsupported");
            return None;
        }
        let base = vb.map_or(Mat::IDENTITY, |v| view_box(v, root.attr("preserveAspectRatio"), 0.0, 0.0, w, h));
        let (vw, vh) = vb.map_or((w, h), |v| (v[2], v[3]));
        let mut ids = FxHashMap::default();
        let mut rules = Vec::new();
        for e in std::iter::once(root).chain(root.descendants()) {
            if let Some(id) = e.attr("id") {
                ids.entry(id).or_insert(e.id());
            }
            if e.local_name() == "style" {
                sheet(&e.text_content(), &mut rules);
            }
        }
        let mut st =
            Svg { doc: &doc, ids, rules, vw, vh, w, h, segs: 0, bytes: 0, over: false, out: String::new(), shadings: FxHashMap::default() };
        self.svg_children(&mut st, root, &Style::default(), base, 0, true);
        if st.over {
            self.count("svg:limit");
        }
        let content = std::mem::take(&mut st.out).into_bytes();
        let doc = self.w.doc.as_mut()?;
        let r = doc.add_form([0.0, 0.0, w, h], content, Some("DeviceRGB"));
        let form = self.w.note(r)?;
        self.w.report.svgs += 1;
        Some(ImgHandle { src: PicSrc::Form(form), w, h })
    }

    /// An element as its own node (`is_root`: the root `svg`, whose viewport is the form).
    fn svg_children(&mut self, st: &mut Svg<'_>, e: Element<'_>, parent: &Style, m: Mat, depth: u32, is_root: bool) {
        let decls = st.cascade(e);
        let (style, own) = st.style(parent, &decls);
        if own.display_none {
            return;
        }
        self.svg_group(st, e, &style, &own, m, depth, is_root);
    }

    /// One element: its transform, clip and opacity group around what it draws.
    #[allow(clippy::too_many_arguments)]
    fn svg_group(&mut self, st: &mut Svg<'_>, e: Element<'_>, style: &Style, own: &Own, m: Mat, depth: u32, is_root: bool) {
        if depth > MAX_DEPTH || st.over || self.halt {
            if depth > MAX_DEPTH {
                st.over = true;
            }
            return;
        }
        let name = e.local_name();
        let m = match e.attr("transform") {
            Some(t) if !is_root && name != "svg" => parse_transform(t).then(m),
            _ => m,
        };
        if !m.0.iter().all(|v| v.is_finite()) || m.det() == 0.0 {
            return;
        }
        if own.filter {
            self.count("svg:filter");
        }
        if own.mask {
            self.count("svg:mask");
        }
        let clip = own.clip.as_deref().and_then(|id| st.el(id)).filter(|c| c.local_name() == "clipPath");
        if let Some(c) = clip {
            let mut path = String::new();
            let eo = self.svg_clip_path(st, c, style, m, depth, &mut path, e);
            if path.is_empty() {
                // a clip with nothing in it hides the element
                return;
            }
            st.put(&format!("q\n{path}{} n\n", if eo { "W*" } else { "W" }));
        }
        let group = own.opacity < 1.0;
        let saved = if group { Some(std::mem::take(&mut st.out)) } else { None };
        if own.opacity > 0.0 {
            self.svg_content(st, e, style, own, m, depth, is_root);
        }
        if let Some(outer) = saved {
            let inner = std::mem::replace(&mut st.out, outer);
            if own.opacity > 0.0 && !inner.is_empty() {
                let form = self.w.doc.as_mut().map(|d| d.add_form([0.0, 0.0, st.w, st.h], inner.into_bytes(), Some("DeviceRGB")));
                if let Some(form) = form.and_then(|r| self.w.note(r)) {
                    let a = uo_jsnum::format::Js(own.opacity);
                    let gs = self.w.doc.as_mut().map(|d| d.add_ext_gstate(&format!("/BM /Normal /ca {a} /CA {a} /SMask /None")));
                    if let Some(gs) = gs {
                        st.put(&format!("q\n/GX{gs} gs\n/Fm{form} Do\nQ\n"));
                    }
                }
            }
        }
        if clip.is_some() {
            st.put("Q\n");
        }
    }

    /// What an element draws, under its matrix.
    #[allow(clippy::too_many_arguments)]
    fn svg_content(&mut self, st: &mut Svg<'_>, e: Element<'_>, style: &Style, own: &Own, m: Mat, depth: u32, is_root: bool) {
        let name = e.local_name();
        match name {
            "svg" if !is_root => {
                // a nested viewport: x, y, width, height and its viewBox, clipped to the viewport
                let len = |k: &str, of: f64, d: f64| e.attr(k).and_then(|v| length(v, of)).unwrap_or(d);
                let (x, y) = (len("x", st.vw, 0.0), len("y", st.vh, 0.0));
                let (w, h) = (len("width", st.vw, st.vw), len("height", st.vh, st.vh));
                if w <= 0.0 || h <= 0.0 {
                    return;
                }
                let inner = view_box_of(e).map_or(Mat::translate(x, y), |vb| view_box(vb, e.attr("preserveAspectRatio"), x, y, w, h));
                let mut p = Path::default();
                p.move_to(x, y);
                p.line_to(x + w, y);
                p.line_to(x + w, y + h);
                p.line_to(x, y + h);
                p.close();
                let mut clip = String::new();
                p.emit(Some(m), &mut clip);
                st.put(&format!("q\n{clip}W n\n"));
                self.svg_kids(st, e, style, inner.then(m), depth);
                st.put("Q\n");
            }
            "svg" | "g" | "a" => self.svg_kids(st, e, style, m, depth),
            "switch" => {
                if let Some(k) =
                    e.child_elements().find(|k| !k.attrs().any(|a| a.name.starts_with("required") || a.name == "systemLanguage"))
                {
                    self.svg_children(st, k, style, m, depth + 1, false);
                }
            }
            "use" => {
                let href = e.attr("href").or_else(|| e.attr("xlink:href")).and_then(|h| h.jtrim().strip_prefix('#'));
                let Some(target) = href.and_then(|id| st.el(id)) else { return };
                let len = |k: &str, of: f64| e.attr(k).and_then(|v| length(v, of)).unwrap_or(0.0);
                let m = Mat::translate(len("x", st.vw), len("y", st.vh)).then(m);
                if target.local_name() == "symbol" {
                    let decls = st.cascade(target);
                    let (s2, o2) = st.style(style, &decls);
                    if o2.display_none {
                        return;
                    }
                    let w = e.attr("width").and_then(|v| length(v, st.vw)).unwrap_or(st.vw);
                    let h = e.attr("height").and_then(|v| length(v, st.vh)).unwrap_or(st.vh);
                    let inner =
                        view_box_of(target).map_or(Mat::IDENTITY, |vb| view_box(vb, target.attr("preserveAspectRatio"), 0.0, 0.0, w, h));
                    self.svg_kids(st, target, &s2, inner.then(m), depth + 1);
                } else if !is_ancestor(target, e) {
                    self.svg_children(st, target, style, m, depth + 1, false);
                }
            }
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => {
                if own.marker {
                    self.count("svg:marker");
                }
                let Some(p) = shape(e, st.vw, st.vh) else { return };
                st.segs += p.segs.len();
                if st.segs > MAX_SEGS {
                    st.over = true;
                    return;
                }
                if style.visible {
                    self.svg_leaf(st, &p, style, m);
                }
            }
            "text" => self.count("svg:text"),
            "image" => self.count("svg:image"),
            // definitions and metadata draw nothing where they are
            _ => {}
        }
    }

    fn svg_kids(&mut self, st: &mut Svg<'_>, e: Element<'_>, style: &Style, m: Mat, depth: u32) {
        for k in e.child_elements() {
            if st.over || self.halt {
                return;
            }
            if matches!(
                k.local_name(),
                "defs"
                    | "clipPath"
                    | "linearGradient"
                    | "radialGradient"
                    | "symbol"
                    | "style"
                    | "title"
                    | "desc"
                    | "metadata"
                    | "mask"
                    | "marker"
                    | "pattern"
                    | "filter"
                    | "script"
            ) {
                continue;
            }
            self.svg_children(st, k, style, m, depth + 1, false);
        }
    }

    /// The clip path of `clipPath` element `c` for the element `user` under `m`: its shapes (through their own
    /// transforms, the clip's `transform` and its units) written in form space; whether its rule is even-odd.
    #[allow(clippy::too_many_arguments)]
    fn svg_clip_path(
        &mut self,
        st: &Svg<'_>,
        c: Element<'_>,
        style: &Style,
        m: Mat,
        depth: u32,
        out: &mut String,
        user: Element<'_>,
    ) -> bool {
        let mut cm_ = c.attr("transform").map_or(Mat::IDENTITY, parse_transform);
        if c.attr("clipPathUnits") == Some("objectBoundingBox") {
            let Some(b) = element_bbox(st, user, depth) else { return false };
            cm_ = cm_.then(Mat([b[2] - b[0], 0.0, 0.0, b[3] - b[1], b[0], b[1]]));
        }
        let m = cm_.then(m);
        let mut eo = false;
        for k in c.child_elements() {
            let decls = st.cascade(k);
            let (s, own) = st.style(style, &decls);
            if own.display_none || !s.visible {
                continue;
            }
            let km = k.attr("transform").map_or(Mat::IDENTITY, parse_transform).then(m);
            let p = match k.local_name() {
                "use" => {
                    let href = k.attr("href").or_else(|| k.attr("xlink:href")).and_then(|h| h.jtrim().strip_prefix('#'));
                    href.and_then(|id| st.el(id)).and_then(|t| shape(t, st.vw, st.vh))
                }
                _ => shape(k, st.vw, st.vh),
            };
            if let Some(p) = p {
                eo |= s.clip_eo;
                p.emit(Some(km), out);
            }
        }
        eo
    }

    /// Fill and stroke of one shape under `m`.
    fn svg_leaf(&mut self, st: &mut Svg<'_>, p: &Path, style: &Style, m: Mat) {
        if p.segs.is_empty() {
            return;
        }
        let mut body = String::new();
        p.emit(None, &mut body);
        let bbox = p.bbox();
        let mut out = String::new();
        out.push_str("q\n");
        out.push_str(&cm(m));
        // fill
        match self.svg_paint(st, &style.fill, style, bbox) {
            Resolved::Colour(c) => {
                let a = c.3 * style.fill_op;
                if a > 0.0 {
                    out.push_str("q\n");
                    if a < 1.0 {
                        out.push_str(&self.svg_alpha(a));
                    }
                    let _ = writeln!(out, "{} rg\n{body}{}\nQ", rgb(c), if style.fill_eo { "f*" } else { "f" });
                }
            }
            Resolved::Gradient(g) => {
                let _ = write!(out, "q\n{body}{} n\n", if style.fill_eo { "W*" } else { "W" });
                out.push_str(&self.svg_gradient_paint(st, &g, style.fill_op, bbox));
                out.push_str("Q\n");
            }
            Resolved::None => {}
        }
        // stroke
        if style.width > 0.0 {
            let c = match self.svg_paint(st, &style.stroke, style, bbox) {
                Resolved::Colour(c) => Some(c),
                Resolved::Gradient(g) => {
                    self.count("gradient:stroke");
                    Some(g.average())
                }
                Resolved::None => None,
            };
            if let Some(c) = c {
                let a = c.3 * style.stroke_op;
                if a > 0.0 {
                    out.push_str("q\n");
                    if a < 1.0 {
                        out.push_str(&self.svg_alpha(a));
                    }
                    let dash: Vec<String> = style.dash.iter().map(|d| n(*d)).collect();
                    let _ = writeln!(
                        out,
                        "{} RG\n{} w\n{} J\n{} j\n{} M\n[{}] {} d\n{body}S\nQ",
                        rgb(c),
                        n(style.width),
                        style.cap,
                        style.join,
                        n(style.miter),
                        dash.join(" "),
                        n(style.dash_off)
                    );
                }
            }
        }
        out.push_str("Q\n");
        st.put(&out);
    }

    fn svg_alpha(&mut self, a: f64) -> String {
        let av = uo_jsnum::format::Js(round_permille(a) / 1000.0);
        match self.w.doc.as_mut() {
            Some(d) => format!("/GX{} gs\n", d.add_ext_gstate(&format!("/BM /Normal /ca {av} /CA {av} /SMask /None"))),
            None => String::new(),
        }
    }

    /// A paint resolved for one shape: a colour, a gradient (its geometry for the shape's box) or nothing.
    fn svg_paint(&mut self, st: &Svg<'_>, p: &Paint, style: &Style, bbox: Option<[f64; 4]>) -> Resolved {
        match p {
            Paint::None => Resolved::None,
            Paint::Colour(c) => Resolved::Colour(*c),
            Paint::Current => Resolved::Colour(style.color),
            Paint::Url(id, fallback) => {
                let target = st.el(id);
                match target.map(Element::local_name) {
                    Some("linearGradient" | "radialGradient") => {
                        if let Some(t) = target
                            && let Some(g) = self.svg_gradient(st, t, style, bbox)
                        {
                            return g;
                        }
                        Resolved::None
                    }
                    other => {
                        if other == Some("pattern") {
                            self.count("svg:pattern");
                        }
                        match fallback.as_deref() {
                            Some(f) if !matches!(f, Paint::Url(..)) => self.svg_paint(st, f, style, bbox),
                            _ => Resolved::None,
                        }
                    }
                }
            }
        }
    }

    /// A gradient element (with its `href` chain) for a shape's box.
    fn svg_gradient(&mut self, st: &Svg<'_>, g: Element<'_>, style: &Style, bbox: Option<[f64; 4]>) -> Option<Resolved> {
        let mut chain = vec![g];
        while chain.len() < 16 {
            let last = *chain.last()?;
            let Some(next) =
                last.attr("href").or_else(|| last.attr("xlink:href")).and_then(|h| h.jtrim().strip_prefix('#')).and_then(|id| st.el(id))
            else {
                break;
            };
            if chain.iter().any(|c| c.id() == next.id()) {
                break;
            }
            chain.push(next);
        }
        let attr = |k: &str| chain.iter().find_map(|c| c.attr(k));
        let radial = g.local_name() == "radialGradient";
        let bbox_units = attr("gradientUnits") != Some("userSpaceOnUse");
        if attr("spreadMethod").is_some_and(|s| s != "pad") {
            self.count("svg:spread");
        }
        // the stops of the first element of the chain that has any
        let stop_el = chain.iter().find(|c| c.child_elements().any(|k| k.local_name() == "stop"))?;
        let mut stops: Vec<(f64, Rgba)> = Vec::new();
        for s in stop_el.child_elements().filter(|k| k.local_name() == "stop") {
            let decls = st.cascade(s);
            let mut colour_v = Rgba::BLACK;
            let mut op = 1.0;
            for (k, v) in &decls {
                match k.as_str() {
                    "stop-color" => {
                        if v.eq_ignore_ascii_case("currentcolor") {
                            colour_v = style.color;
                        } else if let Some(c) = colour(v) {
                            colour_v = c;
                        }
                    }
                    "stop-opacity" => op = unit(v).unwrap_or(op),
                    _ => {}
                }
            }
            let prev = stops.last().map_or(0.0, |s| s.0);
            let off = max(prev, s.attr("offset").and_then(unit).unwrap_or(0.0));
            stops.push((off, Rgba(colour_v.0, colour_v.1, colour_v.2, colour_v.3 * op)));
        }
        match stops.len() {
            0 => return Some(Resolved::None),
            1 => return stops.first().map(|s| Resolved::Colour(s.1)),
            _ => {}
        }
        let (rw, rh) = if bbox_units { (1.0, 1.0) } else { (st.vw, st.vh) };
        let rd = if bbox_units { 1.0 } else { hypot(st.vw, st.vh) / std::f64::consts::SQRT_2 };
        let val = |k: &str, of: f64, d: f64| -> f64 {
            match attr(k) {
                Some(v) if bbox_units && !v.jtrim().ends_with('%') => {
                    let x = parse_float(v.jtrim());
                    if x.is_finite() { x } else { d }
                }
                Some(v) => length(v, of).unwrap_or(d),
                None => d,
            }
        };
        let coords = if radial {
            let cx = val("cx", rw, 0.5 * rw);
            let cy = val("cy", rh, 0.5 * rh);
            let r = val("r", rd, 0.5 * rd);
            let fx = val("fx", rw, cx);
            let fy = val("fy", rh, cy);
            let fr = val("fr", rd, 0.0);
            if r <= 0.0 {
                return stops.last().map(|s| Resolved::Colour(s.1));
            }
            vec![fx, fy, fr, cx, cy, r]
        } else {
            vec![val("x1", rw, 0.0), val("y1", rh, 0.0), val("x2", rw, rw), val("y2", rh, 0.0)]
        };
        let mut gm = attr("gradientTransform").map_or(Mat::IDENTITY, parse_transform);
        if bbox_units {
            let b = bbox?;
            let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
            if bw <= 0.0 || bh <= 0.0 {
                return Some(Resolved::None);
            }
            gm = gm.then(Mat([bw, 0.0, 0.0, bh, b[0], b[1]]));
        }
        let key = format!("{}|{radial}|{coords:?}", g.attr("id").unwrap_or_default());
        Some(Resolved::Gradient(Grad { key, radial, coords, stops, gm }))
    }

    /// The content that paints a gradient inside the clip in force: the alpha (constant, or a luminosity soft mask of
    /// the stop opacities over the shape's box), the gradient matrix and the shading.
    fn svg_gradient_paint(&mut self, st: &mut Svg<'_>, g: &Grad, fill_op: f64, bbox: Option<[f64; 4]>) -> String {
        let Some(ramp) = Self::ramp(&g.stops) else { return String::new() };
        let Some(sh) = self.svg_shading(st, &format!("{}|rgb", g.key), g.radial, &g.coords, false, &ramp.rgb) else { return String::new() };
        let mut out = String::new();
        match ramp.const_alpha {
            Some(a) => {
                let a = a * fill_op;
                if a <= 0.0 {
                    return String::new();
                }
                if a < 1.0 {
                    out.push_str(&self.svg_alpha(a));
                }
            }
            None => {
                let mask = (|| -> Option<String> {
                    let b = bbox?;
                    let msh = self.svg_shading(st, &format!("{}|alpha", g.key), g.radial, &g.coords, true, &ramp.alpha)?;
                    let content = format!("{}/{msh} sh\n", cm(g.gm)).into_bytes();
                    let doc = self.w.doc.as_mut()?;
                    let r = doc.add_form([b[0] - 1.0, b[1] - 1.0, b[2] + 1.0, b[3] + 1.0], content, Some("DeviceGray"));
                    let idx = self.w.note(r)?;
                    let obj = self.w.doc.as_ref()?.form_object(idx)?;
                    let a = uo_jsnum::format::Js(max(0.0, min(1.0, fill_op)));
                    let gs = self
                        .w
                        .doc
                        .as_mut()?
                        .add_ext_gstate(&format!("/BM /Normal /ca {a} /CA {a} /SMask << /Type /Mask /S /Luminosity /G {obj} 0 R >>"));
                    Some(format!("/GX{gs} gs\n"))
                })();
                match mask {
                    Some(m) => out.push_str(&m),
                    None => {
                        if fill_op < 1.0 {
                            out.push_str(&self.svg_alpha(fill_op));
                        }
                    }
                }
            }
        }
        let _ = writeln!(out, "{}/{sh} sh", cm(g.gm));
        out
    }

    fn svg_shading(&mut self, st: &mut Svg<'_>, key: &str, radial: bool, coords: &[f64], gray: bool, samples: &[u8]) -> Option<String> {
        if let Some(v) = st.shadings.get(key) {
            return v.clone();
        }
        let v = self.w.doc.as_mut().and_then(|d| d.add_user_shading(radial, coords, gray, samples).ok()).map(|s| s.resource_name());
        st.shadings.insert(key.to_owned(), v.clone());
        v
    }
}

fn round_permille(a: f64) -> f64 {
    uo_jsnum::math::round(max(0.0, min(1.0, a)) * 1000.0)
}

struct Grad {
    key: String,
    radial: bool,
    coords: Vec<f64>,
    stops: Vec<(f64, Rgba)>,
    /// gradient space to the shape's user space
    gm: Mat,
}

impl Grad {
    fn average(&self) -> Rgba {
        let k = self.stops.len().max(1) as f64;
        let s = self.stops.iter().fold((0.0, 0.0, 0.0, 0.0), |a, (_, c)| (a.0 + c.0, a.1 + c.1, a.2 + c.2, a.3 + c.3));
        Rgba(s.0 / k, s.1 / k, s.2 / k, s.3 / k)
    }
}

enum Resolved {
    None,
    Colour(Rgba),
    Gradient(Grad),
}

fn is_ancestor(a: Element<'_>, e: Element<'_>) -> bool {
    a.id() == e.id() || a.descendants().any(|d| d.id() == e.id())
}

/// The bounding box of an element's shapes in its own user space (a group: its children through their transforms).
fn element_bbox(st: &Svg<'_>, e: Element<'_>, depth: u32) -> Option<[f64; 4]> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(p) = shape(e, st.vw, st.vh) {
        return p.bbox();
    }
    let mut b: Option<[f64; 4]> = None;
    for k in e.child_elements() {
        let Some(kb) = element_bbox(st, k, depth + 1) else { continue };
        let t = k.attr("transform").map_or(Mat::IDENTITY, parse_transform);
        for (x, y) in [(kb[0], kb[1]), (kb[2], kb[1]), (kb[0], kb[3]), (kb[2], kb[3])] {
            let (x, y) = t.apply(x, y);
            b = Some(match b {
                Some(o) => [min(o[0], x), min(o[1], y), max(o[2], x), max(o[3], y)],
                None => [x, y, x, y],
            });
        }
    }
    b
}

/// The path of a basic shape or `path` element in its user space; `None` for other elements and shapes that
/// render nothing (zero or negative sizes).
fn shape(e: Element<'_>, vw: f64, vh: f64) -> Option<Path> {
    let diag = hypot(vw, vh) / std::f64::consts::SQRT_2;
    let len = |k: &str, of: f64| e.attr(k).and_then(|v| length(v, of));
    let mut p = Path::default();
    match e.local_name() {
        "path" => {
            p = parse_path(e.attr("d")?);
            if p.segs.is_empty() {
                return None;
            }
        }
        "rect" => {
            let (x, y) = (len("x", vw).unwrap_or(0.0), len("y", vh).unwrap_or(0.0));
            let (w, h) = (len("width", vw)?, len("height", vh)?);
            if !(w > 0.0 && h > 0.0) {
                return None;
            }
            let (rx0, ry0) = (len("rx", vw).filter(|v| *v >= 0.0), len("ry", vh).filter(|v| *v >= 0.0));
            let (rx, ry) = match (rx0, ry0) {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) => (a, a),
                (None, Some(b)) => (b, b),
                (None, None) => (0.0, 0.0),
            };
            let (rx, ry) = (min(rx, w / 2.0), min(ry, h / 2.0));
            if rx > 0.0 && ry > 0.0 {
                let (kx, ky) = (rx * (1.0 - KAPPA), ry * (1.0 - KAPPA));
                p.move_to(x + rx, y);
                p.line_to(x + w - rx, y);
                p.cubic([x + w - kx, y, x + w, y + ky, x + w, y + ry]);
                p.line_to(x + w, y + h - ry);
                p.cubic([x + w, y + h - ky, x + w - kx, y + h, x + w - rx, y + h]);
                p.line_to(x + rx, y + h);
                p.cubic([x + kx, y + h, x, y + h - ky, x, y + h - ry]);
                p.line_to(x, y + ry);
                p.cubic([x, y + ky, x + kx, y, x + rx, y]);
                p.close();
            } else {
                p.move_to(x, y);
                p.line_to(x + w, y);
                p.line_to(x + w, y + h);
                p.line_to(x, y + h);
                p.close();
            }
        }
        "circle" => {
            let r = len("r", diag)?;
            if r <= 0.0 {
                return None;
            }
            p.ellipse(len("cx", vw).unwrap_or(0.0), len("cy", vh).unwrap_or(0.0), r, r);
        }
        "ellipse" => {
            let (rx0, ry0) = (len("rx", vw), len("ry", vh));
            let (rx, ry) = (rx0.or(ry0)?, ry0.or(rx0)?);
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            p.ellipse(len("cx", vw).unwrap_or(0.0), len("cy", vh).unwrap_or(0.0), rx, ry);
        }
        "line" => {
            p.move_to(len("x1", vw).unwrap_or(0.0), len("y1", vh).unwrap_or(0.0));
            p.line_to(len("x2", vw).unwrap_or(0.0), len("y2", vh).unwrap_or(0.0));
        }
        "polyline" | "polygon" => {
            let pts = points(e.attr("points")?);
            let (first, rest) = pts.split_first()?;
            p.move_to(first.0, first.1);
            for (x, y) in rest {
                p.line_to(*x, *y);
            }
            if e.local_name() == "polygon" {
                p.close();
            }
        }
        _ => return None,
    }
    Some(p)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn sniff() {
        assert!(is_svg(b"<svg viewBox='0 0 1 1'/>"));
        assert!(is_svg(b"\xef\xbb\xbf  <?xml version='1.0'?>\n<!-- x --><svg/>"));
        assert!(!is_svg(b"\x89PNG\r\n"));
        assert!(!is_svg(b"<html><body/></html>"));
    }

    #[test]
    fn path_grammar() {
        let p = parse_path("M10-20l5.5.5h1v-2 z m1 1 C1 2 3 4 5 6s1 1 2 2Q1 1 2 2t3 3 A5 5 0 0110 10");
        assert!(p.segs.len() > 10);
        // the implicit lineto after a moveto and the compact arc flags
        let p = parse_path("M0 0 10 0 10 10a5 5 0 1 1-10 0");
        assert!(matches!(p.segs[1], Seg::L(x, y) if x == 10.0 && y == 0.0));
        assert!(matches!(p.segs.last(), Some(Seg::C(_, _, _, _, x, y)) if abs(*x) < 1e-9 && abs(*y - 10.0) < 1e-9));
        // an error ends the path where it is
        assert_eq!(parse_path("M0 0 L 5 x 6 6").segs.len(), 1);
        assert!(parse_path("L 1 1").segs.is_empty());
    }

    #[test]
    fn transforms() {
        let m = parse_transform("translate(10,20) scale(2)");
        assert_eq!(m.apply(1.0, 1.0), (12.0, 22.0));
        let m = parse_transform("rotate(90 5 5)");
        let (x, y) = m.apply(10.0, 5.0);
        assert!(abs(x - 5.0) < 1e-9 && abs(y - 10.0) < 1e-9);
        assert_eq!(parse_transform("scale(2) bogus(1)"), Mat::IDENTITY);
    }

    #[test]
    fn aspect() {
        let m = view_box([0.0, 0.0, 100.0, 50.0], None, 0.0, 0.0, 100.0, 100.0);
        assert_eq!(m.apply(0.0, 0.0), (0.0, 25.0));
        let m = view_box([0.0, 0.0, 100.0, 50.0], Some("none"), 0.0, 0.0, 100.0, 100.0);
        assert_eq!(m.apply(100.0, 50.0), (100.0, 100.0));
    }

    #[test]
    fn colours_and_sheets() {
        assert_eq!(colour("cornflowerblue"), Some(Rgba(100.0, 149.0, 237.0, 1.0)));
        assert_eq!(paint("url(#g) red"), Some(Paint::Url("g".into(), Some(Box::new(Paint::Colour(Rgba(255.0, 0.0, 0.0, 1.0)))))));
        let mut rules = Vec::new();
        sheet(".a{fill:#f00} /* c */ circle.a, #x { stroke: blue } g > p {fill:red}", &mut rules);
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[1].spec, (0, 1, 1));
        assert_eq!(rules[2].spec, (1, 0, 0));
    }
}
