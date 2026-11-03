//! Frontier R1 (D1a reader, record "Milestone 1 — D1a foundation"): `text-effects.ts` (read
//! side): `readColor`, `readFill`, `readTextEffects`, `elementXml` (the declarations are in
//! `core/docs/modules/uo-docx-d1a/R1.md`).
//!
//! `readTextEffects(rPr, raw = elementXml)` has a second parameter nobody passes: here it is
//! always [`element_xml`]. `elementXml` is recursive in the TS and a loop here.

use uo_jsnum::parse;
use uo_opc::xml::{Element, Node};

use crate::js::{JsNumber, JsObject, Str};
use crate::model::effects::{
    FxColor, FxFill, FxGlow, FxGradStop, FxLin, FxOutline, FxPath, FxReflection, FxShadow, OpenTypeProps, TextEffects,
};

/// `W14(a, k)`: the attribute `w14:k`.
fn w14<'a>(el: Element<'a>, k: &str) -> Option<&'a str> {
    el.attr(&format!("w14:{k}"))
}

/// The `num` helper: `parseInt(v ?? '', 10)` when finite, else `d`.
fn num(v: Option<&str>, d: f64) -> JsNumber {
    let n = parse::parse_int(v.unwrap_or(""), 10);
    JsNumber::Num(if n.is_finite() { n } else { d })
}

/// `readColor`: the `w14:srgbClr` (else `w14:schemeClr`) child of `el` with its modifiers.
#[must_use]
pub fn read_color(el: Option<Element<'_>>) -> Option<FxColor> {
    let el = el?;
    let c = el.first_child("w14:srgbClr").or_else(|| el.first_child("w14:schemeClr"))?;
    let mut out = FxColor::default();
    if c.name() == "w14:srgbClr" {
        out.rgb = Some(w14(c, "val").unwrap_or("000000").to_uppercase().into());
    } else {
        out.scheme = Some(w14(c, "val").unwrap_or("tx1").into());
    }
    let mods: Vec<(Str, JsNumber)> =
        c.child_elements().map(|m| (uo_jsnum::text::utf16_slice(m.name(), 4, i64::MAX).as_ref().into(), num(w14(m, "val"), 0.0))).collect();
    if !mods.is_empty() {
        out.mods = Some(mods);
    }
    Some(out)
}

/// `readFill`: `w14:noFill`, `w14:solidFill` or `w14:gradFill` below `el`.
#[must_use]
pub fn read_fill(el: Option<Element<'_>>) -> Option<FxFill> {
    let el = el?;
    if el.first_child("w14:noFill").is_some() {
        return Some(FxFill { r#type: "none".into(), ..FxFill::default() });
    }
    if let Some(solid) = el.first_child("w14:solidFill") {
        let color = read_color(Some(solid))?;
        return Some(FxFill { r#type: "solid".into(), color: Some(color), ..FxFill::default() });
    }
    let g = el.first_child("w14:gradFill")?;
    let list = g.first_child("w14:gsLst").unwrap_or(g);
    let stops: Vec<FxGradStop> = list
        .children_named("w14:gs")
        .filter_map(|s| read_color(Some(s)).map(|color| FxGradStop { pos: num(w14(s, "pos"), 0.0), color, ..FxGradStop::default() }))
        .collect();
    let lin = g.first_child("w14:lin").map(|lin| {
        let scaled = w14(lin, "scaled");
        FxLin { ang: num(w14(lin, "ang"), 0.0), scaled: scaled == Some("1") || scaled == Some("true"), ..FxLin::default() }
    });
    let path = g.first_child("w14:path").map(|path| {
        let fr = path.first_child("w14:fillToRect");
        let side = |k: &str| num(fr.and_then(|f| w14(f, k)), 0.0);
        FxPath {
            kind: w14(path, "path").unwrap_or("circle").into(),
            l: side("l"),
            t: side("t"),
            r: side("r"),
            b: side("b"),
            ..FxPath::default()
        }
    });
    Some(FxFill { r#type: "grad".into(), stops: Some(stops), lin, path, ..FxFill::default() })
}

/// `readTextEffects`: the w14 effects and OpenType typography of a `w:rPr`; `None` for a group
/// without any key.
#[must_use]
pub fn read_text_effects(rpr: Element<'_>) -> (Option<TextEffects>, Option<OpenTypeProps>) {
    let mut fx = TextEffects::default();
    let mut ot = OpenTypeProps::default();
    let (mut any_fx, mut any_ot) = (false, false);
    macro_rules! fx_set {
        ($f:ident, $k:literal, $v:expr) => {{
            let v = $v;
            fx.$f = Some(v);
            fx.mark($k);
            any_fx = true;
        }};
    }
    macro_rules! ot_set {
        ($f:ident, $k:literal, $v:expr) => {{
            let v = $v;
            ot.$f = Some(v);
            ot.mark($k);
            any_ot = true;
        }};
    }
    for el in rpr.child_elements() {
        match el.name() {
            "w14:glow" => {
                if let Some(color) = read_color(Some(el)) {
                    fx_set!(glow, "glow", Box::new(FxGlow { rad: num(w14(el, "rad"), 0.0), color, ..FxGlow::default() }));
                }
            }
            "w14:shadow" => {
                if let Some(color) = read_color(Some(el)) {
                    let sh = FxShadow {
                        blur_rad: num(w14(el, "blurRad"), 0.0),
                        dist: num(w14(el, "dist"), 0.0),
                        dir: num(w14(el, "dir"), 0.0),
                        sx: num(w14(el, "sx"), 100_000.0),
                        sy: num(w14(el, "sy"), 100_000.0),
                        kx: num(w14(el, "kx"), 0.0),
                        ky: num(w14(el, "ky"), 0.0),
                        algn: w14(el, "algn").unwrap_or("b").into(),
                        color,
                        ..FxShadow::default()
                    };
                    fx_set!(shadow, "shadow", Box::new(sh));
                }
            }
            "w14:reflection" => {
                let r = FxReflection {
                    blur_rad: num(w14(el, "blurRad"), 0.0),
                    st_a: num(w14(el, "stA"), 100_000.0),
                    st_pos: num(w14(el, "stPos"), 0.0),
                    end_a: num(w14(el, "endA"), 0.0),
                    end_pos: num(w14(el, "endPos"), 100_000.0),
                    dist: num(w14(el, "dist"), 0.0),
                    dir: num(w14(el, "dir"), 0.0),
                    fade_dir: num(w14(el, "fadeDir"), 5_400_000.0),
                    sx: num(w14(el, "sx"), 100_000.0),
                    sy: num(w14(el, "sy"), 100_000.0),
                    kx: num(w14(el, "kx"), 0.0),
                    ky: num(w14(el, "ky"), 0.0),
                    algn: w14(el, "algn").unwrap_or("b").into(),
                    ..FxReflection::default()
                };
                fx_set!(reflection, "reflection", Box::new(r));
            }
            "w14:textOutline" => {
                let mut o = FxOutline::default();
                if w14(el, "w").is_some() {
                    o.w = Some(num(w14(el, "w"), 0.0));
                }
                if let Some(v) = w14(el, "cap").filter(|v| !v.is_empty()) {
                    o.cap = Some(v.into());
                }
                if let Some(v) = w14(el, "cmpd").filter(|v| !v.is_empty()) {
                    o.cmpd = Some(v.into());
                }
                if let Some(v) = w14(el, "algn").filter(|v| !v.is_empty()) {
                    o.algn = Some(v.into());
                }
                if let Some(f) = read_fill(Some(el)) {
                    o.fill = Some(Box::new(f));
                }
                if let Some(d) = el.first_child("w14:prstDash") {
                    o.dash = Some(w14(d, "val").unwrap_or("solid").into());
                }
                if el.first_child("w14:round").is_some() {
                    o.join = Some("round".into());
                } else if el.first_child("w14:bevel").is_some() {
                    o.join = Some("bevel".into());
                } else if let Some(m) = el.first_child("w14:miter") {
                    o.join = Some("miter".into());
                    if w14(m, "lim").is_some_and(|v| !v.is_empty()) {
                        o.miter_lim = Some(num(w14(m, "lim"), 0.0));
                    }
                }
                fx_set!(outline, "outline", Box::new(o));
            }
            "w14:textFill" => {
                if let Some(f) = read_fill(Some(el)) {
                    fx_set!(fill, "fill", Box::new(f));
                }
            }
            "w14:scene3d" => fx_set!(scene3d, "scene3d", element_xml(el).into()),
            "w14:props3d" => fx_set!(props3d, "props3d", element_xml(el).into()),
            "w14:ligatures" => ot_set!(ligatures, "ligatures", w14(el, "val").unwrap_or("standard").into()),
            "w14:numForm" => ot_set!(num_form, "numForm", w14(el, "val").unwrap_or("default").into()),
            "w14:numSpacing" => ot_set!(num_spacing, "numSpacing", w14(el, "val").unwrap_or("default").into()),
            "w14:stylisticSets" => {
                let sets: Vec<JsNumber> = el
                    .children_named("w14:styleSet")
                    .map(|s| num(w14(s, "id"), 0.0))
                    .filter(|n| matches!(n, JsNumber::Num(x) if *x >= 1.0 && *x <= 20.0))
                    .collect();
                ot_set!(stylistic_sets, "stylisticSets", sets);
            }
            "w14:cntxtAlts" => {
                let v = w14(el, "val");
                ot_set!(cntxt_alts, "cntxtAlts", v != Some("0") && v != Some("false"));
            }
            _ => {}
        }
    }
    (any_fx.then_some(fx), any_ot.then_some(ot))
}

/// `escX`: `&`, `<`, `>` and `"` as entities (the four chained global replaces of the TS are one
/// pass: no replacement text holds a character a later replace looks for, as `&` goes first).
#[must_use]
pub fn esc_x(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    escape(t, &mut out);
    out
}

fn escape(t: &str, out: &mut String) {
    for c in t.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// One element being written by [`element_xml`].
struct Frame<'a> {
    el: Element<'a>,
    children: std::vec::IntoIter<Node<'a>>,
    /// the `>` of the start tag is out (a child wrote something)
    open: bool,
}

/// `elementXml`: an element read back as XML (the 3D parts kept raw): the captured source when
/// the element has one, else its name, attributes (JS object order) and children, text escaped,
/// `<x/>` when nothing was written for the children. A loop with its own stack: no recursion on
/// deep trees.
#[must_use]
pub fn element_xml(el: Element<'_>) -> String {
    let mut out = String::new();
    let mut stack: Vec<Frame<'_>> = Vec::new();
    enter(el, &mut out, &mut stack);
    while let Some(top) = stack.last_mut() {
        match top.children.next() {
            Some(Node::Element(c)) => {
                if !top.open {
                    top.open = true;
                    out.push('>');
                }
                enter(c, &mut out, &mut stack);
            }
            Some(Node::Text(t) | Node::CData(t)) => {
                if !t.is_empty() && !top.open {
                    top.open = true;
                    out.push('>');
                }
                escape(t, &mut out);
            }
            Some(_) => {}
            None => {
                if let Some(done) = stack.pop() {
                    if done.open {
                        out.push_str("</");
                        out.push_str(done.el.name());
                        out.push('>');
                    } else {
                        out.push_str("/>");
                    }
                }
            }
        }
    }
    out
}

/// Starts `el`: its raw source, or its start tag without the closing `>` and a frame.
fn enter<'a>(el: Element<'a>, out: &mut String, stack: &mut Vec<Frame<'a>>) {
    if let Some(raw) = el.raw().filter(|r| !r.is_empty()) {
        out.push_str(raw);
        if let Some(parent) = stack.last_mut() {
            parent.open = true;
        }
        return;
    }
    out.push('<');
    out.push_str(el.name());
    for a in el.attrs() {
        out.push(' ');
        out.push_str(a.name);
        out.push_str("=\"");
        escape(a.value, out);
        out.push('"');
    }
    let children: Vec<Node<'a>> = el.children().collect();
    stack.push(Frame { el, children: children.into_iter(), open: false });
}
