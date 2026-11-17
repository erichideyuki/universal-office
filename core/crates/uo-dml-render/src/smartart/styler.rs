//! `diagramStyler` (`read.ts:1575`): the style of a SmartArt label (`node1`, `sibTrans2D1`…): colour by
//! the colours definition (`colorsN.xml`: repeat/cycle list or span interpolated by the node's index)
//! and fill/outline by the quick style (`quickStyleN.xml`: the theme's fillRef/lnRef with that colour in
//! phClr). Without the part, the application's default (accent1_2).

use rustc_hash::FxHashMap;
use uo_jsnum::conv::to_uint8;
use uo_jsnum::format::to_js_string;
use uo_jsnum::math;
use uo_jsnum::parse::parse_int;

use super::xt::{X, to_xml};
use crate::color::CssColor;
use crate::model::{Fill, FillKind};
use crate::read::theme_style_fill;
use crate::read_dml::{group_fill, solid_fill};
use crate::source::parse_xml;
use crate::theme::{Theme, solid_fill_color};

/// The style resolved for a label (`DgmStyle`, `diagram-fallback.ts:21`).
#[derive(Clone, Debug, Default)]
pub struct DgmStyle {
    pub fill: Option<Fill>,
    pub fill_alpha: Option<f64>,
    pub line: Option<CssColor>,
    pub line_width: Option<f64>,
    pub text: Option<CssColor>,
}

/// The styler of one diagram (the closure `diagramStyler` returns).
pub struct Styler<'a> {
    cl: FxHashMap<String, &'a X>,
    qs: FxHashMap<String, &'a X>,
    theme: &'a Theme,
}

/// A throw of the TS styler (`parseXml` of the built fill reference failed): the fallback gives nothing.
#[derive(Debug)]
pub struct StyleThrow;

/// `diagramStyler(colors, quick, theme)`.
pub fn diagram_styler<'a>(colors: Option<&'a X>, quick: Option<&'a X>, theme: &'a Theme) -> Styler<'a> {
    let lbls = |root: Option<&'a X>| -> FxHashMap<String, &'a X> {
        let mut m: FxHashMap<String, &'a X> = FxHashMap::default();
        if let Some(r) = root {
            for e in r.kids_named("dgm:styleLbl") {
                m.insert(e.attr("name").unwrap_or("").to_string(), e);
            }
        }
        m
    };
    Styler { cl: lbls(colors), qs: lbls(quick), theme }
}

fn color_x(x: &X, theme: &Theme) -> Option<CssColor> {
    super::color_of_x(x, theme)
}

impl Styler<'_> {
    /// `pick(lst, idx, cnt)`. `Err` = the TS throws (a negative index reads `undefined.slice`).
    fn pick(&self, lst: Option<&X>, idx: f64, cnt: f64) -> Result<Option<CssColor>, StyleThrow> {
        let Some(lst) = lst else { return Ok(None) };
        let cs: Vec<CssColor> = lst.kids().filter_map(|c| color_x(c, self.theme)).filter(|c| !c.as_str().is_empty()).collect();
        if cs.is_empty() {
            return Ok(None);
        }
        let meth = lst.attr("meth").unwrap_or("span");
        let len = cs.len();
        let lf = usize_f(len);
        if meth != "span" || len == 1 {
            return Ok(uo_jsnum::conv::js_index(math::rem(idx, lf), len).and_then(|i| cs.get(i).cloned()));
        }
        let t = if cnt > 1.0 { math::min(1.0, idx / (cnt - 1.0)) * (lf - 1.0) } else { 0.0 };
        let i_f = math::min(lf - 2.0, math::floor(t));
        let u = t - i_f;
        let i = uo_jsnum::conv::js_index(i_f, len).ok_or(StyleThrow)?;
        let a = cs.get(i).ok_or(StyleThrow)?;
        let b = cs.get(i + 1).ok_or(StyleThrow)?;
        let ch = |k: usize| -> String {
            let sl = |c: &CssColor| parse_int(c.as_str().get(k..k + 2).unwrap_or(""), 16);
            let v = math::round(sl(a) * (1.0 - u) + sl(b) * u);
            if v.is_nan() { "NaN".to_string() } else { format!("{:02x}", to_uint8(v)) }
        };
        Ok(Some(CssColor::new(&format!("#{}{}{}", ch(1), ch(3), ch(5)))))
    }

    /// `styleOf(lbl, idx, cnt)`.
    fn style_of(&self, lbl: &str, idx: f64, cnt: f64) -> Result<Option<DgmStyle>, StyleThrow> {
        let Some(c) = self.cl.get(lbl).copied() else { return Ok(None) };
        let q = self.qs.get(lbl).copied();
        let fill_c = self.pick(c.first("dgm:fillClrLst"), idx, cnt)?;
        let line_c = self.pick(c.first("dgm:linClrLst"), idx, cnt)?;
        let style = q.and_then(|q| q.first("dgm:style"));
        let font_ref = style.and_then(|s| s.first("a:fontRef"));
        let text = self
            .pick(c.first("dgm:txFillClrLst"), idx, cnt)?
            .or_else(|| font_ref.and_then(|f| f.kids().next()).and_then(|k| color_x(k, self.theme)));
        let fill_ref = style.and_then(|s| s.first("a:fillRef"));
        let ln_ref = style.and_then(|s| s.first("a:lnRef"));
        let mut fill: Fill = match &fill_c {
            Some(c) => solid_fill(c.clone()),
            None => Fill::of_kind(FillKind::None),
        };
        if let (Some(fr), Some(fc)) = (fill_ref, &fill_c) {
            let idx_f = parse_int(fr.attr("idx").unwrap_or("0"), 10);
            fill = if idx_f > 0.0 {
                let txt = format!(
                    "<a:fillRef idx=\"{}\"><a:srgbClr val=\"{}\"/></a:fillRef>",
                    to_js_string(idx_f),
                    fc.as_str().get(1..).unwrap_or("")
                );
                let doc = parse_xml(txt).map_err(|_| StyleThrow)?;
                let sf = theme_style_fill(doc.root(), self.theme, None).map_err(|_| StyleThrow)?;
                sf.and_then(|s| s.fill).unwrap_or(fill)
            } else {
                Fill::of_kind(FillKind::None)
            };
        }
        let ln_idx = match ln_ref {
            Some(l) => parse_int(l.attr("idx").unwrap_or("0"), 10),
            None => 1.0,
        };
        let line_width: Option<f64> = if ln_idx > 0.0 && line_c.is_some() {
            Some(
                self.theme
                    .line_widths
                    .as_ref()
                    .and_then(|w| uo_jsnum::conv::js_index(ln_idx - 1.0, w.len()).and_then(|i| w.get(i).copied()))
                    .unwrap_or(9525.0),
            )
        } else {
            None
        };
        let truthy = |v: f64| v != 0.0 && !v.is_nan();
        Ok(Some(DgmStyle {
            fill: Some(fill),
            fill_alpha: None,
            line: if line_width.is_some_and(truthy) { line_c } else { None },
            line_width,
            text,
        }))
    }

    /// The closure of `diagramStyler`: the label's style, with the user's formatting of the point
    /// (`dgm:spPr`) over it.
    pub fn style(&self, lbl: &str, idx: f64, cnt: f64, sp_pr: Option<&X>) -> Result<Option<DgmStyle>, StyleThrow> {
        let base = self.style_of(lbl, idx, cnt)?;
        let Some(sp_pr) = sp_pr else { return Ok(base) };
        let mut out = base.unwrap_or_default();
        let theme = self.theme;
        if let Ok(doc) = parse_xml(to_xml(sp_pr))
            && let Some((fill, alpha)) = group_fill(doc.root(), theme)
        {
            out.fill = Some(fill);
            if let Some(a) = alpha {
                out.fill_alpha = Some(a);
            }
        }
        if let Some(ln) = sp_pr.first("a:ln") {
            if ln.first("a:noFill").is_some() {
                out.line = None;
                out.line_width = None;
            } else {
                let lc = parse_xml(to_xml(ln)).ok().and_then(|d| solid_fill_color(d.root(), theme));
                if let Some(c) = &lc {
                    out.line = Some(c.clone());
                }
                if let Some(w) = ln.attr("w").filter(|w| !w.is_empty()) {
                    out.line_width = Some(parse_int(w, 10));
                } else if lc.is_some() && out.line_width.is_none() {
                    out.line_width = Some(9525.0);
                }
            }
        }
        Ok(Some(out))
    }
}

fn usize_f(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}
