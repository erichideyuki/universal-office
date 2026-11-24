//! Conditional formatting of BIFF8 → `<conditionalFormatting>` + the `<dxf>` of `styles.xml` (`cf.ts`):
//! CONDFMT + CF (Excel 97: cell value with operator, or formula; DXFN with font/border/fill), CONDFMT12 + CF12
//! (Excel 2007+: colour scales, data bars, icon sets, top/bottom N, above average, unique/duplicate; own priority and
//! the DXFN12 with full RGB/theme colours in the XFExtNoFRT) and CFEX (priority, stop-if-true and the DXFN12 of a CF
//! rule). The formulas go through the decompiler of the cells (references relative to the corner of the range).

use super::biff::Rd;
use super::formula::{FormulaContext, decompile};
use super::globals::XlsGlobals;
use super::styles_xml::{Role, fixed_num, icv_color};
use super::xmlw::{range_ref_s, xml_text_s};

/// A converted rule (the XML is written at the end, with the final priority).
#[derive(Clone, Debug)]
pub struct CondRuleOut {
    pub ty: String,
    /// Attributes besides type/dxfId/priority/stopIfTrue (with a leading space).
    pub attrs: String,
    /// Children (`<formula>`, `<colorScale>`…).
    pub body: String,
    pub dxf_id: Option<usize>,
    pub priority: u32,
    pub stop_if_true: bool,
}

/// The rules of a CONDFMT/CONDFMT12 with the sqref in A1.
#[derive(Clone, Debug)]
pub struct CondFormatOut {
    pub sqref: String,
    pub rules: Vec<CondRuleOut>,
    /// The rules in CF-record order, as indices into `rules` (the `icf` of the CFEX; `None` = not converted).
    pub by_icf: Vec<Option<usize>>,
}

/// `ruleXml`: the `<cfRule>` of a converted rule.
pub fn rule_xml(out: &mut String, r: &CondRuleOut) {
    out.push_str("<cfRule type=\"");
    out.push_str(&r.ty);
    out.push('"');
    if let Some(d) = r.dxf_id {
        out.push_str(&format!(" dxfId=\"{d}\""));
    }
    out.push_str(&format!(" priority=\"{}\"", r.priority));
    if r.stop_if_true {
        out.push_str(" stopIfTrue=\"1\"");
    }
    out.push_str(&r.attrs);
    out.push('>');
    out.push_str(&r.body);
    out.push_str("</cfRule>");
}

const OPERATORS: [&str; 9] =
    ["", "between", "notBetween", "equal", "notEqual", "greaterThan", "lessThan", "greaterThanOrEqual", "lessThanOrEqual"];
const BORDER: [&str; 14] = [
    "",
    "thin",
    "medium",
    "dashed",
    "dotted",
    "thick",
    "double",
    "hair",
    "mediumDashed",
    "dashDot",
    "mediumDashDot",
    "dashDotDot",
    "mediumDashDotDot",
    "slantDashDot",
];

/// `readCondFmt`: CONDFMT (off 0) / CONDFMT12 (off 12, after the FrtRefHeaderU): number of CFs, nID (15 bits after
/// fToughRecalc), refBound and sqref (SqRefU).
pub struct CondFmtHead {
    pub nid: u32,
    pub sqref: String,
    pub top: u32,
    pub left: u32,
}

pub fn read_cond_fmt(r: &Rd<'_>, off: usize) -> CondFmtHead {
    let nid = u32::from(r.u16(off + 2)) >> 1;
    let n = usize::from(r.u16(off + 12));
    let mut refs: Vec<String> = Vec::new();
    let mut i = 0;
    let mut o = off + 14;
    while i < n && o + 8 <= r.len() {
        let (r1, r2, c1, c2) = (u32::from(r.u16(o)), u32::from(r.u16(o + 2)), u32::from(r.u16(o + 4)), u32::from(r.u16(o + 6)));
        if r2 >= r1 && c2 >= c1 {
            refs.push(range_ref_s(r1, c1, r2, c2));
        }
        i += 1;
        o += 8;
    }
    CondFmtHead { nid, sqref: refs.join(" "), top: u32::from(r.u16(off + 4)), left: u32::from(r.u16(off + 8)) }
}

fn hex(c: &str) -> String {
    format!("FF{}", c.replace('#', "").to_uppercase())
}

fn hex2(n: u32) -> String {
    format!("{:02X}", n & 0xff)
}

/// An extension colour of the DXFN12: the element writer (`tag` → `<tag rgb|theme … tint/>`).
enum ExtColor {
    Rgb(String, String),
    Theme(u32, String),
}

impl ExtColor {
    fn render(&self, tag: &str) -> String {
        match self {
            ExtColor::Rgb(v, t) => format!("<{tag} rgb=\"{v}\"{t}/>"),
            ExtColor::Theme(v, t) => format!("<{tag} theme=\"{v}\"{t}/>"),
        }
    }
}

/// Full colours of the XFExtNoFRT of the DXFN12: extType → colour.
fn xf_ext_colors(r: &Rd<'_>, off: usize, end: usize) -> Vec<(u16, ExtColor)> {
    let mut out: Vec<(u16, ExtColor)> = Vec::new();
    // reserved1, reserved2 (0xFFFF), reserved3, cexts, rgExt (ExtProp: extType u16, cb u16, data)
    if off + 8 > end {
        return out;
    }
    let n = usize::from(r.u16(off + 6));
    let mut p = off + 8;
    let mut i = 0;
    while i < n && p + 4 <= end {
        let ty = r.u16(p);
        let cb = usize::from(r.u16(p + 2));
        if cb < 4 {
            break;
        }
        if ((4..=5).contains(&ty) || (7..=11).contains(&ty) || ty == 13) && p + 12 <= end {
            // FullColorExt: xclrType u16, nTintShade i16, xclrValue u32
            let ct = r.u16(p + 4);
            let tint = f64::from(r.i16(p + 6)) / 32767.0;
            let v = r.u32(p + 8);
            let t = if tint != 0.0 { format!(" tint=\"{}\"", fixed_num(tint, 6)) } else { String::new() };
            let col = if ct == 2 {
                Some(ExtColor::Rgb(format!("FF{}{}{}", hex2(v), hex2(v >> 8), hex2(v >> 16)), t))
            } else if ct == 3 {
                Some(ExtColor::Theme(v, t))
            } else {
                None
            };
            if let Some(c) = col {
                out.retain(|(k, _)| *k != ty);
                out.push((ty, c));
            }
        }
        p += cb;
        i += 1;
    }
    out
}

fn ext_get(v: &[(u16, ExtColor)], ty: u16) -> Option<&ExtColor> {
    v.iter().find(|(k, _)| *k == ty).map(|(_, c)| c)
}

/// DXFN (+ XFExtNoFRT up to `ext_end`, if any) → `<dxf>` (font, border, fill); and the end offset.
fn dxf_xml(g: &XlsGlobals, r: &Rd<'_>, off: usize, ext_end: Option<usize>) -> (String, usize) {
    let fl = r.u32(off);
    let fl2 = r.u16(off + 4);
    let mut p = off + 6;
    let mut font_color: Option<String> = None;
    let mut fill_icv: Option<u32> = None;
    let mut font = String::new();
    let mut border = String::new();
    if fl & (1 << 25) != 0 {
        // DXFNum (user or ifmt)
        p += if fl2 & 1 != 0 { usize::from(r.u16(p)).max(2) } else { 2 };
    }
    if fl & (1 << 26) != 0 {
        let ts = r.u32(p + 68);
        let bls = r.u16(p + 72);
        let icv = r.u32(p + 80);
        let ts_ninch = r.u32(p + 88);
        let bls_ninch = r.u32(p + 100);
        if bls_ninch == 0 && bls >= 600 && bls != 0xffff {
            font.push_str("<b/>");
        }
        if ts_ninch & 2 == 0 && ts & 2 != 0 {
            font.push_str("<i/>");
        }
        if ts_ninch & 0x80 == 0 && ts & 0x80 != 0 {
            font.push_str("<strike/>");
        }
        if icv != 0xffff_ffff
            && let Some(c) = icv_color(icv & 0xffff, &g.palette, Role::Font)
        {
            font_color = Some(format!("<color rgb=\"{}\"/>", hex(&c)));
        }
        p += 118;
    }
    if fl & (1 << 27) != 0 {
        p += 8; // DXFALC
    }
    if fl & (1 << 28) != 0 {
        let a = r.u32(p);
        let b = r.u32(p + 4);
        let edge = |tag: &str, dg: u32, icv: u32, ninch: bool| -> String {
            let st = BORDER.get(usize::try_from(dg).unwrap_or(usize::MAX)).copied().unwrap_or("");
            if ninch || dg == 0 || st.is_empty() {
                return String::new();
            }
            let c = icv_color(icv, &g.palette, Role::Border).unwrap_or_else(|| "#000000".to_owned());
            format!("<{tag} style=\"{st}\"><color rgb=\"{}\"/></{tag}>", hex(&c))
        };
        border.push_str(&edge("left", a & 0xf, (a >> 16) & 0x7f, fl & (1 << 10) != 0));
        border.push_str(&edge("right", (a >> 4) & 0xf, (a >> 23) & 0x7f, fl & (1 << 11) != 0));
        border.push_str(&edge("top", (a >> 8) & 0xf, b & 0x7f, fl & (1 << 12) != 0));
        border.push_str(&edge("bottom", (a >> 12) & 0xf, (b >> 7) & 0x7f, fl & (1 << 13) != 0));
        p += 8;
    }
    let mut has_fill = false;
    if fl & (1 << 29) != 0 {
        let v = u32::from(r.u16(p + 2));
        let fg = v & 0x7f;
        let bg = (v >> 7) & 0x7f;
        // the solid fill of the rule: the visible colour is the BACKGROUND one (bgColor in the dxf, as in the .xlsx)
        fill_icv = if fl & (1 << 18) == 0 {
            Some(bg)
        } else if fl & (1 << 17) == 0 {
            Some(fg)
        } else {
            None
        };
        has_fill = fill_icv.is_some();
        p += 4;
    }
    if fl & (1 << 30) != 0 {
        p += 2;
    }
    // Excel 2007+: full colours (RGB/theme) in the XFExtNoFRT after the DXFN
    let ext = match ext_end {
        Some(e) if e > p => xf_ext_colors(r, p, e),
        _ => Vec::new(),
    };
    if let Some(fc) = ext_get(&ext, 13) {
        font_color = Some(fc.render("color"));
    }
    let mut fill = String::new();
    let xfill = ext_get(&ext, 5).or_else(|| ext_get(&ext, 4));
    if has_fill && xfill.is_some() {
        if let Some(x) = xfill {
            fill = format!("<fill><patternFill>{}</patternFill></fill>", x.render("bgColor"));
        }
    } else if has_fill
        && let Some(icv) = fill_icv
        && let Some(c) = icv_color(icv, &g.palette, Role::Fg)
    {
        fill = format!("<fill><patternFill><bgColor rgb=\"{}\"/></patternFill></fill>", hex(&c));
    }
    if let Some(fc) = font_color
        && !fc.is_empty()
    {
        font.push_str(&fc);
    }
    let xml = format!(
        "<dxf>{}{fill}{}</dxf>",
        if font.is_empty() { String::new() } else { format!("<font>{font}</font>") },
        if border.is_empty() { String::new() } else { format!("<border>{border}</border>") }
    );
    (xml, ext_end.map_or(p, |e| p.max(e)))
}

/// DXFN12 at `off` (cbDxf u32; 0 = 2 reserved bytes) → the dxfId (or `None`) and the end.
fn dxfn12(g: &XlsGlobals, r: &Rd<'_>, off: usize, dxfs: &mut Vec<String>) -> (Option<usize>, usize) {
    let cb = usize::try_from(r.u32(off)).unwrap_or(usize::MAX);
    if cb == 0 {
        return (None, off + 6);
    }
    let end = off.saturating_add(4).saturating_add(cb);
    let (xml, _) = dxf_xml(g, r, off + 4, Some(end));
    dxfs.push(xml);
    (Some(dxfs.len() - 1), end)
}

/// `readCf`: CF → rule (`cellIs` or `expression`); the dxf goes to the workbook's `dxfs`.
pub fn read_cf(g: &XlsGlobals, r: &Rd<'_>, dxfs: &mut Vec<String>, ctx: &FormulaContext<'_>, priority: u32) -> Option<CondRuleOut> {
    let ct = r.u8(0);
    let cp = r.u8(1);
    let cce1 = usize::from(r.u16(2));
    let cce2 = usize::from(r.u16(4));
    let (xml, end) = dxf_xml(g, r, 6, None);
    let f1 = if cce1 > 0 { decompile(r, end, cce1, ctx) } else { None };
    let f2 = if cce2 > 0 { decompile(r, end + cce1, cce2, ctx) } else { None };
    dxfs.push(xml);
    basic_rule(ct, cp, f1.as_deref(), f2.as_deref(), Some(dxfs.len() - 1), priority)
}

fn basic_rule(ct: u8, cp: u8, f1: Option<&str>, f2: Option<&str>, dxf_id: Option<usize>, priority: u32) -> Option<CondRuleOut> {
    let rule =
        |ty: &str, attrs: String, body: String| CondRuleOut { ty: ty.to_owned(), attrs, body, dxf_id, priority, stop_if_true: false };
    if ct == 1 {
        let op = OPERATORS.get(usize::from(cp)).copied().filter(|o| !o.is_empty())?;
        let f1 = f1?;
        let two = cp == 1 || cp == 2;
        if two && f2.is_none() {
            return None;
        }
        let mut body = format!("<formula>{}</formula>", xml_text_s(f1));
        if two {
            body.push_str(&format!("<formula>{}</formula>", xml_text_s(f2.unwrap_or(""))));
        }
        return Some(rule("cellIs", format!(" operator=\"{op}\""), body));
    }
    if ct == 2
        && let Some(f1) = f1
    {
        return Some(rule("expression", String::new(), format!("<formula>{}</formula>", xml_text_s(f1))));
    }
    None
}

/// The rule by the template (icfTemplate) when it defines the OOXML type: average, unique/duplicate.
fn template_rule(tpl: u16, r: &Rd<'_>, parm: usize, dxf_id: Option<usize>, priority: u32) -> Option<CondRuleOut> {
    let rule =
        |ty: &str, attrs: String| CondRuleOut { ty: ty.to_owned(), attrs, body: String::new(), dxf_id, priority, stop_if_true: false };
    match tpl {
        0x07 => Some(rule("uniqueValues", String::new())),
        0x1b => Some(rule("duplicateValues", String::new())),
        0x19 | 0x1a | 0x1d | 0x1e => {
            // CFExAveragesTemplateParams: iParam u16 = number of standard deviations
            let sd = r.u16(parm);
            let below = tpl == 0x1a || tpl == 0x1e;
            let eq = tpl == 0x1d || tpl == 0x1e;
            let mut a = String::new();
            if below {
                a.push_str(" aboveAverage=\"0\"");
            }
            if eq {
                a.push_str(" equalAverage=\"1\"");
            }
            if sd != 0 {
                a.push_str(&format!(" stdDev=\"{sd}\""));
            }
            Some(rule("aboveAverage", a))
        }
        _ => None,
    }
}

const ICON_SETS: [&str; 17] = [
    "3Arrows",
    "3ArrowsGray",
    "3Flags",
    "3TrafficLights1",
    "3TrafficLights2",
    "3Signs",
    "3Symbols",
    "3Symbols2",
    "4Arrows",
    "4ArrowsGray",
    "4RedToBlack",
    "4Rating",
    "4TrafficLights",
    "5Arrows",
    "5ArrowsGray",
    "5Rating",
    "5Quarters",
];

fn cfvo_type(t: u8) -> &'static str {
    match t {
        2 => "min",
        3 => "max",
        4 => "percent",
        5 => "percentile",
        7 => "formula",
        _ => "num", // 1 and anything else
    }
}

/// CFVO at `off` → the attributes of the `<cfvo>` (not closed, for `gte`) and the end.
fn cfvo(r: &Rd<'_>, off: usize, ctx: &FormulaContext<'_>) -> (String, usize) {
    let t = r.u8(off);
    let cce = usize::from(r.u16(off + 1));
    let mut p = off + 3;
    let mut val: Option<String> = None;
    if cce > 0 {
        val = decompile(r, p, cce, ctx);
        p += cce;
    } else if t != 2 && t != 3 {
        let x = r.f64(p);
        let s = uo_jsnum::format::to_precision(x, 15).unwrap_or_default();
        val = Some(uo_jsnum::format::to_js_string(uo_jsnum::parse::to_number(&s)));
        p += 8;
    }
    let ty = cfvo_type(t);
    let mut attrs = format!("type=\"{ty}\"");
    if let Some(v) = val
        && ty != "min"
        && ty != "max"
    {
        attrs.push_str(&format!(" val=\"{}\"", xml_text_s(&v)));
    }
    (attrs, p)
}

/// CFColor (16 bytes: xclrType u32, xclrValue u32, numTint Xnum) → `<color …/>`.
fn cf_color(g: &XlsGlobals, r: &Rd<'_>, off: usize) -> String {
    let t = r.u32(off);
    let v = r.u32(off + 4);
    let tint = r.f64(off + 8);
    let ta = if tint != 0.0 { format!(" tint=\"{}\"", fixed_num(tint, 6)) } else { String::new() };
    if t == 2 {
        return format!("<color rgb=\"FF{}{}{}\"{ta}/>", hex2(v), hex2(v >> 8), hex2(v >> 16));
    }
    if t == 3 {
        return format!("<color theme=\"{v}\"{ta}/>");
    }
    if t == 1
        && let Some(c) = icv_color(v & 0xffff, &g.palette, Role::Fg)
    {
        return format!("<color rgb=\"{}\"{ta}/>", hex(&c));
    }
    "<color rgb=\"FF000000\"/>".to_owned()
}

/// `readCf12`: CF12 (Excel 2007+) → rule. Layout ([MS-XLS] 2.4.43): FrtRefHeader (12), ct, cp, cce1, cce2, DXFN12,
/// rgce1, rgce2, fmlaActive (cce + rgce), flags (bit 1 fStopIfTrue), ipriority, icfTemplate, cbTemplateParm (16) +
/// rgbTemplateParms, rgbCT (CFGradient/CFDatabar/CFFilter/CFMultistate).
pub fn read_cf12(g: &XlsGlobals, r: &Rd<'_>, dxfs: &mut Vec<String>, ctx: &FormulaContext<'_>) -> Option<CondRuleOut> {
    let ct = r.u8(12);
    let cp = r.u8(13);
    let cce1 = usize::from(r.u16(14));
    let cce2 = usize::from(r.u16(16));
    let (dxf_id, d_end) = dxfn12(g, r, 18, dxfs);
    let mut p = d_end;
    let f1 = if cce1 > 0 { decompile(r, p, cce1, ctx) } else { None };
    p += cce1;
    let f2 = if cce2 > 0 { decompile(r, p, cce2, ctx) } else { None };
    p += cce2;
    p += 2 + usize::from(r.u16(p)); // fmlaActive
    let stop = r.u8(p) & 2 != 0;
    let priority = u32::from(r.u16(p + 1));
    let tpl = r.u16(p + 3);
    let parm = p + 6;
    p = parm + usize::from(r.u8(p + 5));
    let mut rule: Option<CondRuleOut> = None;
    let mk = |ty: &str, attrs: String, body: String, dxf: Option<usize>| CondRuleOut {
        ty: ty.to_owned(),
        attrs,
        body,
        dxf_id: dxf,
        priority,
        stop_if_true: false,
    };
    match ct {
        1 | 2 => {
            rule = template_rule(tpl, r, parm, dxf_id, priority)
                .or_else(|| basic_rule(ct, cp, f1.as_deref(), f2.as_deref(), dxf_id, priority));
        }
        3 => {
            // CFGradient: unused (2), reserved (1), cInterpCurve, cGradientCurve, flags, rgInterp, rgCurve
            let n = usize::from(r.u8(p + 3));
            let m = usize::from(r.u8(p + 4));
            let mut q = p + 6;
            let mut cfvos = String::new();
            for _ in 0..n {
                let (attrs, end) = cfvo(r, q, ctx);
                cfvos.push_str(&format!("<cfvo {attrs}/>"));
                q = end + 8; // + numDomain
            }
            let mut colors = String::new();
            for _ in 0..m {
                colors.push_str(&cf_color(g, r, q + 8)); // numGrange + CFColor
                q += 24;
            }
            if n >= 2 && n == m {
                rule = Some(mk("colorScale", String::new(), format!("<colorScale>{cfvos}{colors}</colorScale>"), None));
            }
        }
        4 => {
            // CFDatabar: unused (2), reserved (1), flags, iPercentMin, iPercentMax, CFColor, cfvo x2
            // flags: Excel writes 0 in the bars that SHOW the value (oracle, poi-NewStyleConditionalFormattings) —
            // bit 0 = only the bar, as in POI ([MS-XLS] says fRightToLeft/fShowValue)
            let fl = r.u8(p + 3);
            let min = r.u8(p + 4);
            let max = r.u8(p + 5);
            let color = cf_color(g, r, p + 6);
            let (a1, e1) = cfvo(r, p + 22, ctx);
            let (a2, _) = cfvo(r, e1, ctx);
            let mut attrs = String::new();
            if min != 10 {
                attrs.push_str(&format!(" minLength=\"{min}\""));
            }
            if max != 90 {
                attrs.push_str(&format!(" maxLength=\"{max}\""));
            }
            if fl & 1 != 0 {
                attrs.push_str(" showValue=\"0\"");
            }
            rule = Some(mk("dataBar", String::new(), format!("<dataBar{attrs}><cfvo {a1}/><cfvo {a2}/>{color}</dataBar>"), None));
        }
        5 => {
            // CFFilter: cbFilter u16, reserved u8, flags (fTop, fPercent), iParam u16
            let fl = r.u8(p + 3);
            let rank = r.u16(p + 4);
            let mut attrs = String::new();
            if fl & 2 != 0 {
                attrs.push_str(" percent=\"1\"");
            }
            if fl & 1 == 0 {
                attrs.push_str(" bottom=\"1\"");
            }
            attrs.push_str(&format!(" rank=\"{rank}\""));
            rule = Some(mk("top10", attrs, String::new(), dxf_id));
        }
        6 => {
            // CFMultistate: unused (2), reserved (1), cStates, iIconSet, flags (fIconOnly, -, fReverse), rgStates
            let n = usize::from(r.u8(p + 3));
            let set = ICON_SETS.get(usize::from(r.u8(p + 4))).copied();
            let fl = r.u8(p + 5);
            if let Some(set) = set {
                let mut q = p + 6;
                let mut cfvos = String::new();
                for _ in 0..n {
                    let (attrs, end) = cfvo(r, q, ctx);
                    let eq = r.u8(end);
                    cfvos.push_str(&format!("<cfvo {attrs}{}/>", if eq != 0 { "" } else { " gte=\"0\"" }));
                    q = end + 5;
                }
                let mut a = String::new();
                if fl & 1 != 0 {
                    a.push_str(" showValue=\"0\"");
                }
                if fl & 4 != 0 {
                    a.push_str(" reverse=\"1\"");
                }
                rule = Some(mk("iconSet", String::new(), format!("<iconSet iconSet=\"{set}\"{a}>{cfvos}</iconSet>"), None));
            }
        }
        _ => {}
    }
    if let Some(r) = rule.as_mut()
        && stop
    {
        r.stop_if_true = true;
    }
    rule
}

/// `applyCfEx`: CFEX (fIsCF12 = 0) extends the rule `icf` of the CONDFMT `nID`: priority, stop-if-true, the DXFN12
/// (full colours) and the template (average, unique/duplicate become their own OOXML type).
pub fn apply_cf_ex(g: &XlsGlobals, r: &Rd<'_>, dxfs: &mut Vec<String>, cond_formats: &mut [CondFormatOut], by_nid: &[(u32, usize)]) {
    if r.u32(12) != 0 {
        return; // extends a CF12 (which carries everything)
    }
    let nid = u32::from(r.u16(16));
    // the Map keeps the last CONDFMT of an nID
    let Some(&(_, ci)) = by_nid.iter().rev().find(|(k, _)| *k == nid) else { return };
    let icf = usize::from(r.u16(18));
    let Some(cf) = cond_formats.get_mut(ci) else { return };
    let Some(idx) = cf.by_icf.get(icf).copied().flatten() else { return };
    let tpl = u16::from(r.u8(21));
    let fl = r.u8(24);
    let Some(rule) = cf.rules.get_mut(idx) else { return };
    rule.priority = u32::from(r.u16(22));
    if fl & 1 == 0 {
        rule.ty.clear(); // fActive = 0: the rule is off
        return;
    }
    if fl & 2 != 0 {
        rule.stop_if_true = true;
    }
    let mut p = 26usize;
    if r.u8(25) != 0 {
        let (id, end) = dxfn12(g, r, p, dxfs);
        if id.is_some() {
            rule.dxf_id = id;
        }
        p = end;
    }
    if let Some(t) = template_rule(tpl, r, p + 1, rule.dxf_id, rule.priority) {
        rule.ty = t.ty;
        rule.attrs = t.attrs;
        rule.body = t.body;
    }
}
