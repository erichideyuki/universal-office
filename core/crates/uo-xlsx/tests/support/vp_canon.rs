//! The canonical op stream of a viewport list: the list's items expanded to the canvas calls the painter issues
//! (`GlyphRun` -> `textAlign` + the per-glyph `fillText`s, `SheetFont` -> `font`, paths flushed at fill/stroke/clip), then the
//! same canonicalisation `js/capture-viewport.mjs` applies to the TS recording (redundant assignments dropped with the
//! save/restore stack). Floats are IEEE bits, colours lower case.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;

use uo_render::slides::{Align, Baseline, Cap, FillRule, ImageSrc, Join, Paint, PathOp, SItem, SlidesList, SlidesResources};

fn fh(x: f64) -> String {
    // -0 and +0 paint the same: the capture normalises the sign of zero (`Math.round(-0.3)` is -0 in the TS)
    let x = if x == 0.0 { 0.0 } else { x };
    if x.is_nan() { "nan".to_owned() } else { format!("{:016x}", x.to_bits()) }
}

fn json_units(u: &[u16]) -> String {
    let mut o = String::from("\"");
    let mut i = 0;
    while i < u.len() {
        let x = u[i];
        match x {
            0x22 => o.push_str("\\\""),
            0x5c => o.push_str("\\\\"),
            0x0a => o.push_str("\\n"),
            0x0d => o.push_str("\\r"),
            0x09 => o.push_str("\\t"),
            0x08 => o.push_str("\\b"),
            0x0c => o.push_str("\\f"),
            x if x < 0x20 => o.push_str(&format!("\\u{x:04x}")),
            0xD800..=0xDBFF if u.get(i + 1).is_some_and(|l| (0xDC00..0xE000).contains(l)) => {
                o.push(char::decode_utf16([x, u[i + 1]]).next().unwrap().unwrap());
                i += 1;
            }
            0xD800..=0xDFFF => o.push_str(&format!("\\u{x:04x}")),
            x => o.push(char::from_u32(u32::from(x)).unwrap()),
        }
        i += 1;
    }
    o.push('"');
    o
}

fn text(res: &SlidesResources, id: u32) -> String {
    String::from_utf16_lossy(res.sym_units(id))
}

fn path_op(p: &PathOp) -> String {
    match p {
        PathOp::MoveTo(a) => format!("M {} {}", fh(a.x), fh(a.y)),
        PathOp::LineTo(a) => format!("L {} {}", fh(a.x), fh(a.y)),
        PathOp::Arc(a) => format!("A {} {} {} {} {} {}", fh(a.cx), fh(a.cy), fh(a.r), fh(a.a0), fh(a.a1), u8::from(a.ccw)),
        PathOp::Rect(r) => format!("R {} {} {} {}", fh(r.x), fh(r.y), fh(r.w), fh(r.h)),
        PathOp::RoundRect(r) => format!("RR {} {} {} {} {}", fh(r.x), fh(r.y), fh(r.w), fh(r.h), fh(r.r)),
        PathOp::Close => "Z".to_owned(),
        other => format!("?{other:?}"),
    }
}

fn sha16(b: &[u8]) -> String {
    uo_testkit::stream::hex(&uo_testkit::sha256::sha256(b))[..16].to_owned()
}

fn paint_text(l: &SlidesList, res: &SlidesResources, p: &Paint) -> String {
    match p {
        Paint::Css(s) => text(res, *s).to_lowercase(),
        Paint::Linear(i) => {
            let g = &l.gradients[*i as usize];
            let st: Vec<String> = l.stops[g.stops.start as usize..(g.stops.start + g.stops.len) as usize]
                .iter()
                .map(|s| format!("{}={}", fh(s.offset), text(res, s.color).to_lowercase()))
                .collect();
            format!("L({},{},{},{};{})", fh(g.g0), fh(g.g1), fh(g.g2), fh(g.g3), st.join(","))
        }
        Paint::Pattern(i) => {
            let p = &l.patterns[*i as usize];
            let id = match p.src {
                uo_render::slides::PatternSrc::Bitmap(b) => b,
                uo_render::slides::PatternSrc::Image(_) => 0,
            };
            let bytes = res.bitmap_bytes(id).unwrap();
            let tr = p.transform.as_ref().map_or_else(|| "-".to_owned(), |m| format!("{},{}", fh(m.e), fh(m.f)));
            format!("PAT({}/{tr})", sha16(bytes))
        }
        _ => "?".to_owned(),
    }
}

fn font_text(res: &SlidesResources, f: &uo_render::slides::SheetFontItem) -> String {
    if let Some(c) = f.css {
        return text(res, c);
    }
    let fr = f.font.as_ref().unwrap();
    let face = &res.face_entries()[fr.face as usize];
    let fam = text(res, face.family);
    let name = fam.split('|').next().unwrap_or("");
    format!("{}{} {} {}", if f.italic { "italic " } else { "" }, if face.bold { "700" } else { "400" }, fr.size_px, name)
}

/// The raw events (before the state canonicalisation).
pub fn expand(l: &SlidesList, res: &SlidesResources) -> Vec<String> {
    let mut ev: Vec<String> = Vec::new();
    let set = |ev: &mut Vec<String>, k: &str, v: String| ev.push(format!("S {k} {v}"));
    for it in &l.items {
        match it {
            SItem::Save => ev.push("save".into()),
            SItem::Restore => ev.push("restore".into()),
            SItem::Translate(p) => ev.push(format!("translate {} {}", fh(p.x), fh(p.y))),
            SItem::Scale(p) => ev.push(format!("scale {} {}", fh(p.x), fh(p.y))),
            SItem::Rotate(a) => ev.push(format!("rotate {}", fh(*a))),
            SItem::SetTransform(m) => {
                ev.push(format!("setTransform {} {} {} {} {} {}", fh(m.a), fh(m.b), fh(m.c), fh(m.d), fh(m.e), fh(m.f)));
            }
            SItem::ClearRect(r) => ev.push(format!("clearRect {} {} {} {}", fh(r.x), fh(r.y), fh(r.w), fh(r.h))),
            SItem::FillRect(r) => ev.push(format!("fillRect {} {} {} {}", fh(r.x), fh(r.y), fh(r.w), fh(r.h))),
            SItem::StrokeRect(r) => ev.push(format!("strokeRect {} {} {} {}", fh(r.x), fh(r.y), fh(r.w), fh(r.h))),
            SItem::FillStyle(p) => set(&mut ev, "fillStyle", paint_text(l, res, p)),
            SItem::StrokeStyle(p) => set(&mut ev, "strokeStyle", paint_text(l, res, p)),
            SItem::LineWidth(w) => set(&mut ev, "lineWidth", fh(*w)),
            SItem::LineJoin(j) => set(
                &mut ev,
                "lineJoin",
                match j {
                    Join::Round => "round",
                    Join::Bevel => "bevel",
                    Join::Miter => "miter",
                }
                .into(),
            ),
            SItem::LineCap(c) => set(
                &mut ev,
                "lineCap",
                match c {
                    Cap::Butt => "butt",
                    Cap::Round => "round",
                    Cap::Square => "square",
                }
                .into(),
            ),
            SItem::LineDash(s) => {
                let d: Vec<String> = l.dashes[s.start as usize..(s.start + s.len) as usize].iter().map(|x| fh(*x)).collect();
                set(&mut ev, "lineDash", format!("[{}]", d.join(",")));
            }
            SItem::TextAlign(a) => set(
                &mut ev,
                "textAlign",
                match a {
                    Align::Left => "left",
                    Align::Right => "right",
                    Align::Center => "center",
                    Align::Start => "start",
                    Align::End => "end",
                }
                .into(),
            ),
            SItem::TextBaseline(b) => set(
                &mut ev,
                "textBaseline",
                match b {
                    Baseline::Alphabetic => "alphabetic",
                    Baseline::Top => "top",
                    Baseline::Hanging => "hanging",
                    Baseline::Middle => "middle",
                    Baseline::Ideographic => "ideographic",
                    Baseline::Bottom => "bottom",
                }
                .into(),
            ),
            SItem::SheetFont(f) => set(&mut ev, "font", font_text(res, f)),
            SItem::BeginPath => ev.push("beginPath".into()),
            SItem::AppendPath(s) => {
                for p in &l.paths[s.start as usize..(s.start + s.len) as usize] {
                    ev.push(path_op(p));
                }
            }
            SItem::Fill(r) => ev.push(format!("fill {}", if *r == FillRule::NonZero { "nonzero" } else { "evenodd" })),
            SItem::Clip(r) => ev.push(format!("clip {}", if *r == FillRule::NonZero { "nonzero" } else { "evenodd" })),
            SItem::Stroke => ev.push("stroke".into()),
            SItem::FillText(t) => {
                let u = &l.texts[t.text.start as usize..(t.text.start + t.text.len) as usize];
                ev.push(format!("fillText {} {} {}", json_units(u), fh(t.x), fh(t.y)));
            }
            SItem::DrawImage(d) => {
                let ImageSrc::Bitmap(b) = d.src else { panic!("image source") };
                ev.push(format!(
                    "drawImage {} {} {} {} {}",
                    sha16(res.bitmap_bytes(b).unwrap()),
                    fh(d.d.x),
                    fh(d.d.y),
                    fh(d.d.w),
                    fh(d.d.h)
                ));
            }
            SItem::GlyphRun(g) => {
                set(&mut ev, "textAlign", "left".into());
                let units = &l.texts[g.text.start as usize..(g.text.start + g.text.len) as usize];
                let adv = &l.advances[g.advances.start as usize..(g.advances.start + g.advances.len) as usize];
                if g.mirrored {
                    ev.push("save".into());
                    ev.push(format!("translate {} {}", fh(g.x + g.width), fh(0.0)));
                    ev.push(format!("scale {} {}", fh(-1.0), fh(1.0)));
                    glyphs(&mut ev, units, adv, 0.0, g.y, f64::NEG_INFINITY, f64::INFINITY, g.complex);
                    ev.push("restore".into());
                } else {
                    glyphs(&mut ev, units, adv, g.x, g.y, g.min_x, g.max_x, g.complex);
                }
            }
            other => panic!("item not in the Sheet subset: {other:?}"),
        }
    }
    ev
}

#[allow(clippy::too_many_arguments)]
fn glyphs(ev: &mut Vec<String>, units: &[u16], adv: &[i32], x: f64, y: f64, min_x: f64, max_x: f64, complex: bool) {
    if complex {
        ev.push(format!("fillText {} {} {}", json_units(units), fh(x), fh(y)));
        return;
    }
    let mut cx = x;
    let mut k = 0;
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        let two = (0xD800..0xDC00).contains(&u) && units.get(i + 1).is_some_and(|l| (0xDC00..0xE000).contains(l));
        let n = if two { 2 } else { 1 };
        let w = f64::from(adv.get(k).copied().unwrap_or(0));
        k += 1;
        if cx > max_x {
            break;
        }
        if cx + w >= min_x && u != 0x20 {
            ev.push(format!("fillText {} {} {}", json_units(&units[i..i + n]), fh(cx), fh(y)));
        }
        cx += w;
        i += n;
    }
}

/// `canon()` of `capture-viewport.mjs`.
pub fn canonicalize(ev: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut state: HashMap<String, String> = HashMap::new();
    let mut stack: Vec<HashMap<String, String>> = Vec::new();
    let mut path: Vec<String> = Vec::new();
    for e in ev {
        if e == "save" {
            stack.push(state.clone());
            out.push(e.clone());
        } else if e == "restore" {
            state = stack.pop().unwrap_or_default();
            out.push(e.clone());
        } else if e == "beginPath" {
            path.clear();
        } else if ["M ", "L ", "A ", "R ", "RR "].iter().any(|p| e.starts_with(p)) || e == "Z" {
            path.push(e.clone());
        } else if let Some(rest) = e.strip_prefix("S ") {
            let (k, v) = rest.split_once(' ').unwrap();
            if state.get(k).is_some_and(|c| c == v) {
                continue;
            }
            state.insert(k.to_owned(), v.to_owned());
            out.push(e.clone());
        } else if e.starts_with("fill ") || e.starts_with("clip ") {
            out.push(format!("{} {}", e, path.join(",")));
        } else if e == "stroke" {
            out.push(format!("stroke {}", path.join(",")));
        } else {
            out.push(e.clone());
        }
    }
    out
}

/// The canonical lines of a list.
pub fn canon(l: &SlidesList, res: &SlidesResources) -> Vec<String> {
    canonicalize(&expand(l, res))
}
