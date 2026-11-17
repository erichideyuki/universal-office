//! The `RecCanvas` self-test: the line DSL of `js/gen-rec-selftest.mjs` run through the [`Canvas`] trait. The expected draw-form
//! lines in `tests/captures/rec-selftest.txt` were printed by the frozen recorder.

use uo_charts::emit::{Canvas, CanvasFont, CanvasPaint, CanvasStop, LineCap, LineJoin, TextAlign, TextBaseline};

use super::digest::{hex, sha1};
use super::json;
use super::rec::RecCanvas;

pub struct Case {
    pub name: String,
    pub calls: Vec<String>,
    pub expected: Vec<String>,
}

pub fn parse_cases(text: &str) -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("#case ") {
            out.push(Case { name: n.to_owned(), calls: Vec::new(), expected: Vec::new() });
        } else if let Some(c) = line.strip_prefix("> ")
            && let Some(k) = out.last_mut()
        {
            k.calls.push(c.to_owned());
        } else if let Some(e) = line.strip_prefix("< ")
            && let Some(k) = out.last_mut()
        {
            k.expected.push(e.to_owned());
        }
    }
    out
}

/// Whitespace-separated tokens; a JSON string is one token (decoded).
fn tokens(line: &str) -> Vec<String> {
    let b = line.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b' ' {
            i += 1;
        } else if b[i] == b'"' {
            let mut j = i + 1;
            while b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let s = json::parse(&line[i..=j]).ok().and_then(|j| j.str().map(str::to_owned)).unwrap_or_default();
            out.push(s);
            i = j + 1;
        } else {
            let j = line[i..].find(' ').map_or(line.len(), |k| i + k);
            out.push(line[i..j].to_owned());
            i = j;
        }
    }
    out
}

fn num(s: &str) -> f64 {
    uo_jsnum::parse::to_number(s)
}

/// The media sources the recorder printed for the self-test images (`img(sha1[0..12] of "media<i>")`).
pub fn media() -> Vec<String> {
    (0..2).map(|i| format!("img({})", &hex(&sha1(format!("media{i}").as_bytes()))[..12])).collect()
}

/// Runs the DSL calls of a case on a fresh [`RecCanvas`] (the Slides stack) and returns its lines.
pub fn run(case: &Case) -> Vec<String> {
    let mut c = RecCanvas::new(RecCanvas::ppt_stack, media());
    for call in &case.calls {
        let t = tokens(call);
        let op = t[0].as_str();
        let a: Vec<f64> = t[1..].iter().map(|x| num(x)).collect();
        let s = |i: usize| t[1 + i].as_str();
        match op {
            "save" => c.save(),
            "restore" => c.restore(),
            "translate" => c.translate(a[0], a[1]),
            "rotate" => c.rotate(a[0]),
            "alphaMul" => c.global_alpha_mul(a[0]),
            "beginPath" => c.begin_path(),
            "closePath" => c.close_path(),
            "fill" => c.fill(),
            "stroke" => c.stroke(),
            "clip" => c.clip(),
            "moveTo" => c.move_to(a[0], a[1]),
            "lineTo" => c.line_to(a[0], a[1]),
            "bezier" => c.bezier_curve_to(a[0], a[1], a[2], a[3], a[4], a[5]),
            "rect" => c.rect(a[0], a[1], a[2], a[3]),
            "roundRect" => c.round_rect(a[0], a[1], a[2], a[3], a[4]),
            "arc" => c.arc(a[0], a[1], a[2], a[3], a[4], a[5] == 1.0),
            "ellipse" => c.ellipse(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7] == 1.0),
            "fillRect" => c.fill_rect(a[0], a[1], a[2], a[3]),
            "strokeRect" => c.stroke_rect(a[0], a[1], a[2], a[3]),
            "fillText" => c.fill_text(s(0), a[1], a[2]),
            "drawImage" => c.draw_image(a[0] as u32, a[1], a[2], a[3], a[4]),
            "fillCss" => c.fill_style(&CanvasPaint::Css(s(0))),
            "fillLin" | "fillRad" => {
                let head = if op == "fillLin" { 4 } else { 6 };
                let stops: Vec<CanvasStop<'_>> = (head..a.len()).step_by(2).map(|i| CanvasStop { offset: a[i], color: s(i + 1) }).collect();
                if op == "fillLin" {
                    c.fill_style(&CanvasPaint::Linear { x0: a[0], y0: a[1], x1: a[2], y1: a[3], stops: &stops });
                } else {
                    c.fill_style(&CanvasPaint::Radial { x0: a[0], y0: a[1], r0: a[2], x1: a[3], y1: a[4], r1: a[5], stops: &stops });
                }
            }
            "fillStretch" => c.fill_style(&CanvasPaint::Stretched { media: a[0] as u32, transform: [a[1], a[2], a[3], a[4], a[5], a[6]] }),
            "strokeCss" => c.stroke_style(s(0)),
            "lineWidth" => c.line_width(a[0]),
            "dash" => c.line_dash(&if s(0) == "-" { Vec::new() } else { s(0).split(',').map(num).collect::<Vec<_>>() }),
            "cap" => c.line_cap(match s(0) {
                "round" => LineCap::Round,
                "square" => LineCap::Square,
                _ => LineCap::Butt,
            }),
            "join" => c.line_join(match s(0) {
                "round" => LineJoin::Round,
                "bevel" => LineJoin::Bevel,
                _ => LineJoin::Miter,
            }),
            "shadow" => {
                c.shadow_color(s(0));
                c.shadow_blur(a[1]);
                c.shadow_offset_x(a[2]);
                c.shadow_offset_y(a[3]);
            }
            "font" => c.font(&CanvasFont { family: s(0), bold: a[1] == 1.0, italic: a[2] == 1.0, size_px: a[3] }),
            "align" => c.text_align(match s(0) {
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                _ => TextAlign::Left,
            }),
            "baseline" => c.text_baseline(if s(0) == "middle" { TextBaseline::Middle } else { TextBaseline::Alphabetic }),
            other => panic!("unknown DSL op {other}"),
        }
    }
    c.into_lines()
}
