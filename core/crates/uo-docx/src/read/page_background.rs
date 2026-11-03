//! Frontier R3 (D1a reader, record "Milestone 1 — D1a foundation"): docx-core
//! `page-background.ts`, read side: `readPageBackground` (`w:background` with its
//! `v:background/v:fill` effect). The writer half of that file is D1b.

use uo_jsnum::parse::{parse_float, parse_int};
use uo_jsnum::text::{split_js_space, trim};
use uo_opc::xml::Element;

use super::drawing::vml_color;
use crate::js::JsNumber;
use crate::model::background::{FillStop, PageBackground, PageFill};
use crate::model::media::ImageData;

/// `pct`: a VML percentage (`50%`, `49152f`, `.5`) in percent. `Some(NaN)` is a value (the TS
/// returns `parseFloat('%')` for a bad percentage string).
fn pct(v: Option<&str>) -> Option<f64> {
    let v = v.filter(|s| !s.is_empty())?;
    let s = trim(v);
    if s.ends_with('%') {
        return Some(parse_float(s));
    }
    if s.ends_with('f') {
        return Some((parse_int(s, 10) / 65536.0) * 100.0);
    }
    let x = parse_float(s);
    x.is_finite().then_some(x * 100.0)
}

/// `frac`: a VML fraction (`.5`, `49152f`); NaN when not a number.
fn frac(v: &str) -> f64 {
    if v.ends_with('f') { parse_int(v, 10) / 65536.0 } else { parse_float(v) }
}

/// `readPageBackground(bg, resolveImage)`: the `w:background` element read, with the effect's
/// image resolved by its `r:id` through `resolve_image` (a call-time argument, not stored: R5
/// passes the relationship lookup). `None` for a background without any key.
pub fn read_page_background(bg: Element<'_>, resolve_image: &mut dyn FnMut(&str) -> Option<ImageData>) -> Option<PageBackground> {
    let mut out = PageBackground::default();
    if let Some(v) = bg.attr("w:color").filter(|v| !v.is_empty()) {
        out.color = Some(v.into());
    }
    if let Some(v) = bg.attr("w:themeColor").filter(|v| !v.is_empty()) {
        out.theme_color = Some(v.into());
    }
    if let Some(v) = bg.attr("w:themeTint").filter(|v| !v.is_empty()) {
        out.theme_tint = Some(v.into());
    }
    if let Some(v) = bg.attr("w:themeShade").filter(|v| !v.is_empty()) {
        out.theme_shade = Some(v.into());
    }
    let vbg = bg.first_child("v:background");
    let vf = vbg.and_then(|b| b.first_child("v:fill"));
    let kind = vf.and_then(|f| f.attr("type"));
    if let (Some(vbg), Some(vf), Some(kind)) = (vbg, vf, kind)
        && matches!(kind, "gradient" | "gradientRadial" | "pattern" | "frame" | "tile")
    {
        let mut f = PageFill { r#type: kind.into(), ..PageFill::default() };
        if let Some(c1) = vml_color(vbg.attr("fillcolor")).filter(|c| !c.is_empty()) {
            f.color = Some(c1.into());
        }
        if let Some(c2) = vf.attr("color2").filter(|c| !c.is_empty()) {
            f.color2 = Some(c2.into());
        }
        if let Some(angle) = vf.attr("angle").filter(|a| !a.is_empty()) {
            f.angle = Some(JsNumber::Num(parse_float(angle)));
        }
        if let Some(fo) = pct(vf.attr("focus")) {
            f.focus = Some(JsNumber::Num(fo));
        }
        if let Some(fp) = vf.attr("focusposition").filter(|p| !p.is_empty()) {
            let mut it = fp.split(',');
            let x = it.next().unwrap_or("");
            let y = it.next().unwrap_or("");
            let one = |s: &str| {
                let t = trim(s);
                if t.is_empty() { 0.0 } else { frac(t) }
            };
            f.focus_position = Some((JsNumber::Num(one(x)), JsNumber::Num(one(y))));
        }
        if vf.first_child("o:fill").and_then(|o| o.attr("type")) == Some("gradientCenter") {
            f.gradient_center = Some(true);
        }
        if let Some(colors) = vf.attr("colors").filter(|c| !c.is_empty()) {
            let stops: Vec<FillStop> = colors
                .split(';')
                .map(|s| split_js_space(trim(s)).collect::<Vec<_>>())
                .filter(|p| p.len() >= 2)
                .map(|p| FillStop {
                    pos: JsNumber::Num(frac(p.first().copied().unwrap_or(""))),
                    color: vml_color(p.get(1).copied()).unwrap_or_else(|| "000000".to_string()).into(),
                    ..FillStop::default()
                })
                .collect();
            if !stops.is_empty() {
                f.colors = Some(stops);
            }
        }
        if let Some(m) = vf.attr("method").filter(|m| !m.is_empty()) {
            f.method = Some(m.into());
        }
        if let Some(t) = vf.attr("o:title") {
            f.title = Some(t.into());
        }
        if let Some(rid) = vf.attr("r:id").filter(|r| !r.is_empty())
            && let Some(img) = resolve_image(rid)
        {
            f.image = Some(img);
        }
        if vbg.attr("o:targetscreensize").is_some_and(|t| !t.is_empty()) {
            f.target_screen = Some(true);
        }
        out.fill = Some(Box::new(f));
    }
    let any =
        out.color.is_some() || out.theme_color.is_some() || out.theme_tint.is_some() || out.theme_shade.is_some() || out.fill.is_some();
    any.then_some(out)
}
