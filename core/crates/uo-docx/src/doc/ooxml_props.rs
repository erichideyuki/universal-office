//! CHP/PAP/TAP/SEP to OOXML elements (`w:rPr`, `w:pPr`, borders, shading), in schema order
//! (`CT_RPr`, `CT_PPr`, `CT_TblPr`, `CT_TcPr`, `CT_SectPr`), `ooxml-props.ts`. Only what was
//! touched is emitted (the `touched` sets), as the Word does when it converts a `.doc`.

use std::borrow::Cow;

use super::fonts::DocFont;
use super::props::{AUTO, Brc, Chp, ChpKey, KeySet, Pap, PapKey, Shd, TabStop, ico_color};
use super::xmlw::{AttrVal, Attrs, el};

/// `hex`: a colour as the six hex digits OOXML uses, or `auto`.
#[must_use]
pub fn hex(c: i32) -> String {
    if c == AUTO { "auto".to_owned() } else { format!("{c:06X}") }
}

/// `brcXml`: a border element (`name` = `w:top`, `w:left`…); an absent border is `val="nil"`.
#[must_use]
pub fn brc_xml(name: &str, b: Option<&Brc>) -> String {
    let nil = |v: &'static str| el(name, &Attrs::of(&[("w:val", Some(AttrVal::Str(v)))]), "");
    let Some(b) = b else { return nil("nil") };
    let val = brc_type_name(b.brc_type).unwrap_or("single");
    if val == "none" || val == "nil" {
        return nil(if val == "none" && b.width == 0 { "nil" } else { val });
    }
    let color = hex(b.color);
    let a = Attrs::of(&[
        ("w:val", Some(AttrVal::Str(val))),
        ("w:sz", Some(b.width.into())),
        ("w:space", Some(b.space.into())),
        ("w:color", Some(AttrVal::Str(&color))),
        ("w:shadow", b.shadow.then_some(AttrVal::Bool(true))),
        ("w:frame", b.frame.then_some(AttrVal::Bool(true))),
    ]);
    el(name, &a, "")
}

/// `shdXml`: a shading element, or nothing.
#[must_use]
pub fn shd_xml(s: Option<&Shd>) -> String {
    let Some(s) = s else { return String::new() };
    let val = shd_pat_name(s.pat).unwrap_or("clear");
    if val == "nil" {
        return el("w:shd", &Attrs::of(&[("w:val", Some(AttrVal::Str("nil")))]), "");
    }
    let (color, fill) = (hex(s.fore), hex(s.back));
    el(
        "w:shd",
        &Attrs::of(&[("w:val", Some(AttrVal::Str(val))), ("w:color", Some(AttrVal::Str(&color))), ("w:fill", Some(AttrVal::Str(&fill)))]),
        "",
    )
}

const HIGHLIGHT: [&str; 17] = [
    "none",
    "black",
    "blue",
    "cyan",
    "green",
    "magenta",
    "red",
    "yellow",
    "white",
    "darkBlue",
    "darkCyan",
    "darkGreen",
    "darkMagenta",
    "darkRed",
    "darkYellow",
    "darkGray",
    "lightGray",
];

/// `lcidTag`: LCID to a BCP 47 language tag (the ones the Word uses).
#[must_use]
pub fn lcid_tag(lid: Option<u16>) -> Option<&'static str> {
    lcid_name(lid?)
}

/// What the property emitters read of the document: the font table and the `styleId` of an `istd`.
pub struct PropCtx<'a, 's> {
    pub fonts: &'a [DocFont],
    pub style_id: &'a dyn Fn(u16) -> Option<Cow<'s, str>>,
}

fn font_name<'a>(ctx: &PropCtx<'a, '_>, ftc: Option<u16>) -> Option<AttrVal<'a>> {
    let f = ctx.fonts.get(usize::from(ftc?))?;
    Some(AttrVal::Units(f.name.units()))
}

/// `onOff`: `<w:b/>`, `<w:b w:val="0"/>`, or nothing for `undefined`.
fn on_off(name: &str, v: Option<bool>) -> String {
    match v {
        None => String::new(),
        Some(true) => format!("<{name}/>"),
        Some(false) => format!("<{name} w:val=\"0\"/>"),
    }
}

pub(super) fn val_attr<'a>(v: Option<AttrVal<'a>>) -> Attrs<'a> {
    Attrs::of(&[("w:val", v)])
}

fn num<T: Into<i64>>(v: Option<T>) -> Option<AttrVal<'static>> {
    v.map(|v| AttrVal::Int(v.into()))
}

/// `rPrInner`: the content of `w:rPr` (without the outer element). `touched` decides what is
/// written; `with_style` includes `w:rStyle`.
#[must_use]
pub fn r_pr_inner(chp: &Chp, touched: &KeySet<ChpKey>, ctx: &PropCtx<'_, '_>, with_style: bool) -> String {
    let t = |k: ChpKey| touched.has(k);
    let mut x = String::new();
    if with_style
        && let Some(istd) = chp.istd
        && istd != 10
        && t(ChpKey::Istd)
        && let Some(id) = (ctx.style_id)(istd)
        && !id.is_empty()
    {
        x += &el("w:rStyle", &val_attr(Some(AttrVal::Str(&id))), "");
    }
    if t(ChpKey::FtcAscii) || t(ChpKey::FtcOther) || t(ChpKey::FtcFE) || t(ChpKey::FtcBi) || t(ChpKey::IdctHint) {
        let hint = if t(ChpKey::IdctHint) {
            match chp.idct_hint {
                Some(1) => Some(AttrVal::Str("eastAsia")),
                Some(2) => Some(AttrVal::Str("cs")),
                _ => None,
            }
        } else {
            None
        };
        x += &el(
            "w:rFonts",
            &Attrs::of(&[
                ("w:ascii", if t(ChpKey::FtcAscii) { font_name(ctx, chp.ftc_ascii) } else { None }),
                ("w:eastAsia", if t(ChpKey::FtcFE) { font_name(ctx, chp.ftc_fe) } else { None }),
                ("w:hAnsi", if t(ChpKey::FtcOther) { font_name(ctx, chp.ftc_other) } else { None }),
                ("w:cs", if t(ChpKey::FtcBi) { font_name(ctx, chp.ftc_bi) } else { None }),
                ("w:hint", hint),
            ]),
            "",
        );
    }
    for (k, name, v) in [
        (ChpKey::Bold, "w:b", chp.bold),
        (ChpKey::BoldBi, "w:bCs", chp.bold_bi),
        (ChpKey::Italic, "w:i", chp.italic),
        (ChpKey::ItalicBi, "w:iCs", chp.italic_bi),
        (ChpKey::Caps, "w:caps", chp.caps),
        (ChpKey::SmallCaps, "w:smallCaps", chp.small_caps),
        (ChpKey::Strike, "w:strike", chp.strike),
        (ChpKey::Dstrike, "w:dstrike", chp.dstrike),
        (ChpKey::Outline, "w:outline", chp.outline),
        (ChpKey::Shadow, "w:shadow", chp.shadow),
        (ChpKey::Emboss, "w:emboss", chp.emboss),
        (ChpKey::Imprint, "w:imprint", chp.imprint),
        (ChpKey::NoProof, "w:noProof", chp.no_proof),
        (ChpKey::Vanish, "w:vanish", chp.vanish),
        (ChpKey::WebHidden, "w:webHidden", chp.web_hidden),
    ] {
        if t(k) {
            x += &on_off(name, v);
        }
    }
    if t(ChpKey::Cv) || t(ChpKey::Ico) {
        let c = match chp.cv {
            Some(cv) if t(ChpKey::Cv) => cv,
            _ => ico_color(chp.ico.unwrap_or(0)),
        };
        x += &el("w:color", &val_attr(Some(AttrVal::Str(&hex(c)))), "");
    }
    if t(ChpKey::DxaSpace) {
        x += &el("w:spacing", &val_attr(num(chp.dxa_space)), "");
    }
    if t(ChpKey::CharScale)
        && let Some(v) = chp.char_scale
    {
        x += &el("w:w", &val_attr(num(Some(v))), "");
    }
    if t(ChpKey::HpsKern) {
        x += &el("w:kern", &val_attr(num(chp.hps_kern)), "");
    }
    if t(ChpKey::HpsPos) {
        x += &el("w:position", &val_attr(num(chp.hps_pos)), "");
    }
    if t(ChpKey::Hps) {
        x += &el("w:sz", &val_attr(num(chp.hps)), "");
    }
    if t(ChpKey::HpsBi) {
        x += &el("w:szCs", &val_attr(num(chp.hps_bi)), "");
    }
    if t(ChpKey::Highlight) {
        let h = HIGHLIGHT.get(usize::from(chp.highlight.unwrap_or(0))).copied().unwrap_or("none");
        x += &el("w:highlight", &val_attr(Some(AttrVal::Str(h))), "");
    }
    if t(ChpKey::Kul) {
        let color = if t(ChpKey::CvUl) { chp.cv_ul.map(hex) } else { None };
        let mut a = Attrs::new();
        a.set("w:val", Some(AttrVal::Str(kul_name(chp.kul.unwrap_or(0)).unwrap_or("single"))));
        if let Some(c) = &color {
            a.set("w:color", Some(AttrVal::Str(c)));
        }
        x += &el("w:u", &a, "");
    }
    if t(ChpKey::Brc) {
        x += &brc_xml("w:bdr", chp.brc.as_ref());
    }
    if t(ChpKey::Shd) {
        x += &shd_xml(chp.shd.as_ref());
    }
    if t(ChpKey::Iss) {
        let v = match chp.iss {
            Some(1) => "superscript",
            Some(2) => "subscript",
            _ => "baseline",
        };
        x += &el("w:vertAlign", &val_attr(Some(AttrVal::Str(v))), "");
    }
    if t(ChpKey::Rtl) {
        x += &on_off("w:rtl", chp.rtl);
    }
    if t(ChpKey::Cs) {
        x += &on_off("w:cs", chp.cs);
    }
    if t(ChpKey::Kcd) {
        let v = ["none", "dot", "comma", "circle", "underDot"].get(usize::from(chp.kcd.unwrap_or(0))).copied().unwrap_or("none");
        x += &el("w:em", &val_attr(Some(AttrVal::Str(v))), "");
    }
    if t(ChpKey::Lid) || t(ChpKey::LidFE) || t(ChpKey::LidBi) {
        let val = if t(ChpKey::Lid) { lcid_tag(chp.lid) } else { None };
        let east = if t(ChpKey::LidFE) { lcid_tag(chp.lid_fe) } else { None };
        let bidi = if t(ChpKey::LidBi) { lcid_tag(chp.lid_bi) } else { None };
        if val.is_some() || east.is_some() || bidi.is_some() {
            x += &el(
                "w:lang",
                &Attrs::of(&[("w:val", val.map(AttrVal::Str)), ("w:eastAsia", east.map(AttrVal::Str)), ("w:bidi", bidi.map(AttrVal::Str))]),
                "",
            );
        }
    }
    if t(ChpKey::SpecVanish) {
        x += &on_off("w:specVanish", chp.spec_vanish);
    }
    x
}

// ───────────────────────────── paragraph ─────────────────────────────

const JC: [&str; 10] =
    ["left", "center", "right", "both", "distribute", "mediumKashida", "both", "highKashida", "lowKashida", "thaiDistribute"];

/// `jcVal`: `w:jc` of a justification; `para_jc80` makes 3-5 `both` (the `Jc80` of paragraphs).
#[must_use]
pub fn jc_val(jc: Option<u8>, para_jc80: bool) -> &'static str {
    if para_jc80
        && let Some(j) = jc
        && (3..=5).contains(&j)
    {
        return "both";
    }
    JC.get(usize::from(jc.unwrap_or(0))).copied().unwrap_or("left")
}

const TAB_JC: [&str; 8] = ["left", "center", "right", "decimal", "bar", "clear", "num", "left"];
const TAB_TLC: [Option<&str>; 8] = [None, Some("dot"), Some("hyphen"), Some("underscore"), Some("heavy"), Some("middleDot"), None, None];

/// `tabsXml`: the tab stops that differ from the base (the style or the parent): clears those that
/// are gone and adds the new ones.
#[must_use]
pub fn tabs_xml(base: Option<&[TabStop]>, final_tabs: Option<&[TabStop]>, cleared: &[i16]) -> String {
    let b = base.unwrap_or(&[]);
    let f = final_tabs.unwrap_or(&[]);
    let same = |x: &TabStop, y: &TabStop| x.pos == y.pos && x.jc == y.jc && x.tlc == y.tlc;
    let mut clear_pos: Vec<i16> = Vec::new();
    for x in b {
        if !f.iter().any(|y| same(x, y)) && !clear_pos.contains(&x.pos) {
            clear_pos.push(x.pos);
        }
    }
    // explicit removals of the level itself (the Word keeps them without a matching tab in the style)
    for &p in cleared {
        if !f.iter().any(|y| y.pos == p) && !clear_pos.contains(&p) {
            clear_pos.push(p);
        }
    }
    let adds: Vec<&TabStop> = f.iter().filter(|y| !b.iter().any(|x| same(x, y))).collect();
    if clear_pos.is_empty() && adds.is_empty() {
        return String::new();
    }
    let mut x = String::new();
    clear_pos.sort_unstable();
    for c in clear_pos {
        if !adds.iter().any(|a| a.pos == c) {
            x += &el("w:tab", &Attrs::of(&[("w:val", Some(AttrVal::Str("clear"))), ("w:pos", Some(c.into()))]), "");
        }
    }
    for a in adds {
        let jc = TAB_JC.get(usize::from(a.jc)).copied().unwrap_or("left");
        let leader = TAB_TLC.get(usize::from(a.tlc)).copied().flatten();
        x += &el(
            "w:tab",
            &Attrs::of(&[("w:val", Some(AttrVal::Str(jc))), ("w:leader", leader.map(AttrVal::Str)), ("w:pos", Some(a.pos.into()))]),
            "",
        );
    }
    if x.is_empty() { x } else { format!("<w:tabs>{x}</w:tabs>") }
}

/// `PPrOpts`: what `pPrInner` takes besides the properties.
pub struct PPrOpts<'a> {
    /// the tab stops of the level below (style/parent) for the diff
    pub base_tabs: Option<&'a [TabStop]>,
    /// `numId` for an `ilfo`
    pub num_id: Option<&'a dyn Fn(u32) -> i64>,
    /// extra content at the end: the mark's `w:rPr` content
    pub r_pr: Option<&'a str>,
    /// extra content at the end: the `w:sectPr`
    pub sect_pr: Option<&'a str>,
    /// `withStyle !== false`
    pub with_style: bool,
}

impl Default for PPrOpts<'_> {
    fn default() -> Self {
        PPrOpts { base_tabs: None, num_id: None, r_pr: None, sect_pr: None, with_style: true }
    }
}

/// `pPrInner`: the content of `w:pPr`.
#[must_use]
pub fn p_pr_inner(pap: &Pap, touched: &KeySet<PapKey>, ctx: &PropCtx<'_, '_>, o: &PPrOpts<'_>) -> String {
    let t = |k: PapKey| touched.has(k);
    let mut x = String::new();
    if o.with_style
        && pap.istd != 0
        && let Some(id) = (ctx.style_id)(pap.istd)
        && !id.is_empty()
    {
        x += &el("w:pStyle", &val_attr(Some(AttrVal::Str(&id))), "");
    }
    if t(PapKey::KeepFollow) {
        x += &on_off("w:keepNext", pap.keep_follow);
    }
    if t(PapKey::Keep) {
        x += &on_off("w:keepLines", pap.keep);
    }
    if t(PapKey::PageBreakBefore) {
        x += &on_off("w:pageBreakBefore", pap.page_break_before);
    }
    x += &frame_pr_xml(pap, touched);
    if t(PapKey::WidowControl) {
        x += &on_off("w:widowControl", pap.widow_control);
    }
    if t(PapKey::Ilfo) || t(PapKey::Ilvl) {
        let ilfo = i64::from(pap.ilfo.unwrap_or(0));
        let num_id: i64 = if ilfo == 0 || ilfo == -2047 {
            0
        } else {
            let a = u32::try_from(ilfo.abs()).unwrap_or(0);
            match o.num_id {
                Some(f) => f(a),
                None => i64::from(a),
            }
        };
        let mut n = String::new();
        if t(PapKey::Ilvl) || num_id != 0 {
            n += &el("w:ilvl", &val_attr(Some(AttrVal::Int(i64::from(pap.ilvl.unwrap_or(0)).min(8)))), "");
        }
        n += &el("w:numId", &val_attr(Some(AttrVal::Int(num_id))), "");
        x += &format!("<w:numPr>{n}</w:numPr>");
    }
    if t(PapKey::NoLineNumb) {
        x += &on_off("w:suppressLineNumbers", pap.no_line_numb);
    }
    let mut bdr = String::new();
    for (k, name, b) in [
        (PapKey::BrcTop, "w:top", &pap.brc_top),
        (PapKey::BrcLeft, "w:left", &pap.brc_left),
        (PapKey::BrcBottom, "w:bottom", &pap.brc_bottom),
        (PapKey::BrcRight, "w:right", &pap.brc_right),
        (PapKey::BrcBetween, "w:between", &pap.brc_between),
        (PapKey::BrcBar, "w:bar", &pap.brc_bar),
    ] {
        if t(k) {
            bdr += &brc_xml(name, b.as_ref());
        }
    }
    if !bdr.is_empty() {
        x += &format!("<w:pBdr>{bdr}</w:pBdr>");
    }
    if t(PapKey::Shd) {
        x += &shd_xml(pap.shd.as_ref());
    }
    if t(PapKey::Tabs) {
        x += &tabs_xml(o.base_tabs, pap.tabs.as_deref(), pap.tabs_cleared.as_deref().unwrap_or(&[]));
    }
    for (k, name, v) in [
        (PapKey::NoAutoHyph, "w:suppressAutoHyphens", pap.no_auto_hyph),
        (PapKey::Kinsoku, "w:kinsoku", pap.kinsoku),
        (PapKey::WordWrap, "w:wordWrap", pap.word_wrap),
        (PapKey::OverflowPunct, "w:overflowPunct", pap.overflow_punct),
        (PapKey::TopLinePunct, "w:topLinePunct", pap.top_line_punct),
        (PapKey::AutoSpaceDE, "w:autoSpaceDE", pap.auto_space_de),
        (PapKey::AutoSpaceDN, "w:autoSpaceDN", pap.auto_space_dn),
        (PapKey::Bidi, "w:bidi", pap.bidi),
        (PapKey::AdjustRight, "w:adjustRightInd", pap.adjust_right),
        (PapKey::UsePgsuSettings, "w:snapToGrid", pap.use_pgsu_settings),
    ] {
        if t(k) {
            x += &on_off(name, v);
        }
    }
    if t(PapKey::DyaBefore)
        || t(PapKey::DyaAfter)
        || t(PapKey::LineSpacing)
        || t(PapKey::BeforeAuto)
        || t(PapKey::AfterAuto)
        || t(PapKey::DylBefore)
        || t(PapKey::DylAfter)
    {
        let ls = pap.line_spacing.filter(|_| t(PapKey::LineSpacing));
        let auto = |k: PapKey, v: Option<bool>| if t(k) { Some(AttrVal::Str(if v == Some(true) { "1" } else { "0" })) } else { None };
        let on = |k: PapKey| t(k);
        x += &el(
            "w:spacing",
            &Attrs::of(&[
                ("w:before", if on(PapKey::DyaBefore) { num(pap.dya_before) } else { None }),
                ("w:beforeLines", if on(PapKey::DylBefore) { num(pap.dyl_before) } else { None }),
                ("w:beforeAutospacing", auto(PapKey::BeforeAuto, pap.before_auto)),
                ("w:after", if on(PapKey::DyaAfter) { num(pap.dya_after) } else { None }),
                ("w:afterLines", if on(PapKey::DylAfter) { num(pap.dyl_after) } else { None }),
                ("w:afterAutospacing", auto(PapKey::AfterAuto, pap.after_auto)),
                ("w:line", ls.map(|l| AttrVal::Int(i64::from(l.line).abs()))),
                (
                    "w:lineRule",
                    ls.map(|l| {
                        AttrVal::Str(if l.mult {
                            "auto"
                        } else if l.line < 0 {
                            "exact"
                        } else {
                            "atLeast"
                        })
                    }),
                ),
            ]),
            "",
        );
    }
    if t(PapKey::DxaLeft) || t(PapKey::DxaRight) || t(PapKey::DxaLeft1) || t(PapKey::DxcLeft) || t(PapKey::DxcRight) || t(PapKey::DxcLeft1)
    {
        let mut a = Attrs::new();
        a.set("w:left", if t(PapKey::DxaLeft) { pap.dxa_left.map(AttrVal::Int) } else { None });
        a.set("w:leftChars", if t(PapKey::DxcLeft) { num(pap.dxc_left) } else { None });
        a.set("w:right", if t(PapKey::DxaRight) { num(pap.dxa_right) } else { None });
        a.set("w:rightChars", if t(PapKey::DxcRight) { num(pap.dxc_right) } else { None });
        if t(PapKey::DxaLeft1)
            && let Some(l1) = pap.dxa_left1
        {
            if l1 < 0 {
                a.set("w:hanging", Some(AttrVal::Int(-i64::from(l1))));
            } else {
                a.set("w:firstLine", Some(AttrVal::Int(i64::from(l1))));
            }
        }
        if t(PapKey::DxcLeft1)
            && let Some(c1) = pap.dxc_left1
        {
            if c1 < 0 {
                a.set("w:hangingChars", Some(AttrVal::Int(-i64::from(c1))));
            } else {
                a.set("w:firstLineChars", Some(AttrVal::Int(i64::from(c1))));
            }
        }
        x += &el("w:ind", &a, "");
    }
    if t(PapKey::ContextualSpacing) {
        x += &on_off("w:contextualSpacing", pap.contextual_spacing);
    }
    if t(PapKey::MirrorIndents) {
        x += &on_off("w:mirrorIndents", pap.mirror_indents);
    }
    if t(PapKey::Jc) {
        x += &el("w:jc", &val_attr(Some(AttrVal::Str(jc_val(pap.jc, false)))), "");
    }
    if t(PapKey::WAlignFont) {
        let v = ["top", "center", "baseline", "bottom", "auto"].get(usize::from(pap.w_align_font.unwrap_or(4))).copied().unwrap_or("auto");
        x += &el("w:textAlignment", &val_attr(Some(AttrVal::Str(v))), "");
    }
    if t(PapKey::OutLvl)
        && let Some(l) = pap.out_lvl
        && l <= 9
    {
        x += &el("w:outlineLvl", &val_attr(Some(AttrVal::Int(i64::from(l)))), "");
    }
    if let Some(r) = o.r_pr
        && !r.is_empty()
    {
        x += &format!("<w:rPr>{r}</w:rPr>");
    }
    if let Some(s) = o.sect_pr {
        x += s;
    }
    x
}

/// The frame (`w:framePr`), as the Word writes it (measured in the Word): only when there is a
/// position, a size, a wrap or a drop cap (`sprmPPc` alone is not enough); `w:wrap` always;
/// `w:vAnchor` omitted when it is "margin"; `w:hRule` only when exact.
fn frame_pr_xml(pap: &Pap, touched: &KeySet<PapKey>) -> String {
    let truthy = |k: PapKey, v: i64| touched.has(k) && v != 0;
    let any = truthy(PapKey::DxaAbs, i64::from(pap.dxa_abs.unwrap_or(0)))
        || truthy(PapKey::DyaAbs, i64::from(pap.dya_abs.unwrap_or(0)))
        || truthy(PapKey::DxaWidth, i64::from(pap.dxa_width.unwrap_or(0)))
        || truthy(PapKey::Wr, i64::from(pap.wr.unwrap_or(0)))
        || truthy(PapKey::WHeightAbs, i64::from(pap.w_height_abs.unwrap_or(0)))
        || truthy(PapKey::Dcs, i64::from(pap.dcs.unwrap_or(0)));
    if !any {
        return String::new();
    }
    let pc = pap.pc.unwrap_or(0);
    // 0 margin, 1 page, 2 text
    let pc_vert = (pc >> 4) & 3;
    // 0 column, 1 margin, 2 page
    let pc_horz = (pc >> 6) & 3;
    let mut drop_cap = None;
    let mut lines = None;
    if let Some(dcs) = pap.dcs.filter(|&d| d != 0) {
        let fdct = dcs & 7;
        if fdct != 0 {
            drop_cap = Some(if fdct == 1 { "drop" } else { "margin" });
            lines = Some(i64::from((dcs >> 3) & 0x1f));
        }
    }
    let w = pap.dxa_width.filter(|&v| v != 0).map(|v| AttrVal::Int(i64::from(v)));
    let mut h = None;
    let mut h_rule = None;
    if let Some(wh) = pap.w_height_abs.filter(|&v| v != 0) {
        h = Some(AttrVal::Int(i64::from(wh & 0x7fff)));
        if wh & 0x8000 == 0 {
            h_rule = Some("exact");
        }
    }
    let h_space = pap.dxa_from_text.filter(|&v| v != 0).map(|v| AttrVal::Int(i64::from(v)));
    let v_space = pap.dya_from_text.filter(|&v| v != 0).map(|v| AttrVal::Int(i64::from(v)));
    let wrap = ["auto", "notBeside", "around", "none", "tight", "through"].get(usize::from(pap.wr.unwrap_or(0))).copied().unwrap_or("auto");
    let v_anchor = ["margin", "page", "text"].get(usize::from(pc_vert)).copied().filter(|&v| v != "margin");
    let h_anchor = ["text", "margin", "page"].get(usize::from(pc_horz)).copied().unwrap_or("text");
    let xa = i64::from(pap.dxa_abs.unwrap_or(0));
    let x_align = match xa {
        -4 => Some("center"),
        -8 => Some("right"),
        -12 => Some("inside"),
        -16 => Some("outside"),
        _ => None,
    };
    // the Word writes the value of the XAS_plusOne as is in the .docx (measured: dyaAbs 1 gives w:y=1)
    let x_pos = if x_align.is_none() && xa != 0 { Some(AttrVal::Int(xa)) } else { None };
    let ya = i64::from(pap.dya_abs.unwrap_or(0));
    let y_align = match ya {
        -4 => Some("top"),
        -8 => Some("center"),
        -12 => Some("bottom"),
        -16 => Some("inside"),
        -20 => Some("outside"),
        _ => None,
    };
    let y_pos = if y_align.is_none() && ya != 0 { Some(AttrVal::Int(ya)) } else { None };
    // the attribute order of the Word: x before xAlign, y before yAlign
    let a = Attrs::of(&[
        ("w:dropCap", drop_cap.map(AttrVal::Str)),
        ("w:lines", lines.map(AttrVal::Int)),
        ("w:w", w),
        ("w:h", h),
        ("w:hRule", h_rule.map(AttrVal::Str)),
        ("w:hSpace", h_space),
        ("w:vSpace", v_space),
        ("w:wrap", Some(AttrVal::Str(wrap))),
        ("w:vAnchor", v_anchor.map(AttrVal::Str)),
        ("w:hAnchor", Some(AttrVal::Str(h_anchor))),
        ("w:x", x_pos),
        ("w:xAlign", x_align.map(AttrVal::Str)),
        ("w:y", y_pos),
        ("w:yAlign", y_align.map(AttrVal::Str)),
        ("w:anchorLock", pap.frame_locked.filter(|&l| l).map(|_| AttrVal::Str("1"))),
    ]);
    el("w:framePr", &a, "")
}

/// `numFmtOf`: `w:numFmt` of an `nfc`.
#[must_use]
pub fn num_fmt_of(nfc: u8) -> &'static str {
    nfc_name(nfc).unwrap_or("decimal")
}

/// `BRC_TYPE` of the TS (a lookup by number; absent keys are `undefined`).
fn brc_type_name(v: u8) -> Option<&'static str> {
    Some(match v {
        0 => "none",
        1 => "single",
        2 => "thick",
        3 => "double",
        5 => "single",
        6 => "dotted",
        7 => "dashed",
        8 => "dotDash",
        9 => "dotDotDash",
        10 => "triple",
        11 => "thinThickSmallGap",
        12 => "thickThinSmallGap",
        13 => "thinThickThinSmallGap",
        14 => "thinThickMediumGap",
        15 => "thickThinMediumGap",
        16 => "thinThickThinMediumGap",
        17 => "thinThickLargeGap",
        18 => "thickThinLargeGap",
        19 => "thinThickThinLargeGap",
        20 => "wave",
        21 => "doubleWave",
        22 => "dashSmallGap",
        23 => "dashDotStroked",
        24 => "threeDEmboss",
        25 => "threeDEngrave",
        26 => "outset",
        27 => "inset",
        255 => "nil",
        _ => return None,
    })
}

/// `SHD_PAT` of the TS (a lookup by number; absent keys are `undefined`).
fn shd_pat_name(v: u16) -> Option<&'static str> {
    Some(match v {
        0 => "clear",
        1 => "solid",
        2 => "pct5",
        3 => "pct10",
        4 => "pct20",
        5 => "pct25",
        6 => "pct30",
        7 => "pct40",
        8 => "pct50",
        9 => "pct60",
        10 => "pct70",
        11 => "pct75",
        12 => "pct80",
        13 => "pct90",
        14 => "horzStripe",
        15 => "vertStripe",
        16 => "reverseDiagStripe",
        17 => "diagStripe",
        18 => "horzCross",
        19 => "diagCross",
        20 => "thinHorzStripe",
        21 => "thinVertStripe",
        22 => "thinReverseDiagStripe",
        23 => "thinDiagStripe",
        24 => "thinHorzCross",
        25 => "thinDiagCross",
        37 => "pct12",
        38 => "pct15",
        43 => "pct35",
        44 => "pct37",
        46 => "pct45",
        49 => "pct55",
        51 => "pct62",
        52 => "pct65",
        57 => "pct85",
        58 => "pct87",
        60 => "pct95",
        0xffff => "nil",
        _ => return None,
    })
}

/// `KUL` of the TS (a lookup by number; absent keys are `undefined`).
fn kul_name(v: u8) -> Option<&'static str> {
    Some(match v {
        0 => "none",
        1 => "single",
        2 => "words",
        3 => "double",
        4 => "dotted",
        6 => "thick",
        7 => "dash",
        9 => "dotDash",
        10 => "dotDotDash",
        11 => "wave",
        20 => "dottedHeavy",
        23 => "dashedHeavy",
        25 => "dashDotHeavy",
        26 => "dashDotDotHeavy",
        27 => "wavyHeavy",
        39 => "dashLong",
        43 => "wavyDouble",
        55 => "dashLongHeavy",
        _ => return None,
    })
}

/// `LCID` of the TS (a lookup by number; absent keys are `undefined`).
fn lcid_name(v: u16) -> Option<&'static str> {
    Some(match v {
        0 => "x-none",
        0x0401 => "ar-SA",
        0x0402 => "bg-BG",
        0x0403 => "ca-ES",
        0x0404 => "zh-TW",
        0x0405 => "cs-CZ",
        0x0406 => "da-DK",
        0x0407 => "de-DE",
        0x0408 => "el-GR",
        0x0409 => "en-US",
        0x040a => "es-ES_tradnl",
        0x040b => "fi-FI",
        0x040c => "fr-FR",
        0x040d => "he-IL",
        0x040e => "hu-HU",
        0x040f => "is-IS",
        0x0410 => "it-IT",
        0x0411 => "ja-JP",
        0x0412 => "ko-KR",
        0x0413 => "nl-NL",
        0x0414 => "nb-NO",
        0x0415 => "pl-PL",
        0x0416 => "pt-BR",
        0x0417 => "rm-CH",
        0x0418 => "ro-RO",
        0x0419 => "ru-RU",
        0x041a => "hr-HR",
        0x041b => "sk-SK",
        0x041c => "sq-AL",
        0x041d => "sv-SE",
        0x041e => "th-TH",
        0x041f => "tr-TR",
        0x0420 => "ur-PK",
        0x0421 => "id-ID",
        0x0422 => "uk-UA",
        0x0423 => "be-BY",
        0x0424 => "sl-SI",
        0x0425 => "et-EE",
        0x0426 => "lv-LV",
        0x0427 => "lt-LT",
        0x0429 => "fa-IR",
        0x042a => "vi-VN",
        0x042d => "eu-ES",
        0x0436 => "af-ZA",
        0x0439 => "hi-IN",
        0x043e => "ms-MY",
        0x0456 => "gl-ES",
        0x0804 => "zh-CN",
        0x0807 => "de-CH",
        0x0809 => "en-GB",
        0x080a => "es-MX",
        0x080c => "fr-BE",
        0x0810 => "it-CH",
        0x0813 => "nl-BE",
        0x0816 => "pt-PT",
        0x0c04 => "zh-HK",
        0x0c07 => "de-AT",
        0x0c09 => "en-AU",
        0x0c0a => "es-ES",
        0x0c0c => "fr-CA",
        0x1009 => "en-CA",
        0x100c => "fr-CH",
        0x1409 => "en-NZ",
        0x1809 => "en-IE",
        0x1c09 => "en-ZA",
        0x2009 => "en-JM",
        0x200a => "es-VE",
        0x240a => "es-CO",
        0x280a => "es-PE",
        0x2c0a => "es-AR",
        0x300a => "es-EC",
        0x340a => "es-CL",
        0x380a => "es-UY",
        0x3c0a => "es-PY",
        0x400a => "es-BO",
        0x4009 => "en-IN",
        0x540a => "es-US",
        0x0c01 => "ar-EG",
        _ => return None,
    })
}

/// `NFC` of the TS (a lookup by number; absent keys are `undefined`).
fn nfc_name(v: u8) -> Option<&'static str> {
    Some(match v {
        0 => "decimal",
        1 => "upperRoman",
        2 => "lowerRoman",
        3 => "upperLetter",
        4 => "lowerLetter",
        5 => "ordinal",
        6 => "cardinalText",
        7 => "ordinalText",
        8 => "hex",
        9 => "chicago",
        10 => "ideographDigital",
        11 => "japaneseCounting",
        12 => "aiueo",
        13 => "iroha",
        14 => "decimalFullWidth",
        15 => "decimalHalfWidth",
        16 => "japaneseLegal",
        17 => "japaneseDigitalTenThousand",
        18 => "decimalEnclosedCircle",
        19 => "decimalFullWidth2",
        20 => "aiueoFullWidth",
        21 => "irohaFullWidth",
        22 => "decimalZero",
        23 => "bullet",
        24 => "ganada",
        25 => "chosung",
        26 => "decimalEnclosedFullstop",
        27 => "decimalEnclosedParen",
        28 => "decimalEnclosedCircleChinese",
        29 => "ideographEnclosedCircle",
        30 => "ideographTraditional",
        31 => "ideographZodiac",
        32 => "ideographZodiacTraditional",
        33 => "taiwaneseCounting",
        34 => "ideographLegalTraditional",
        35 => "taiwaneseCountingThousand",
        36 => "taiwaneseDigital",
        37 => "chineseCounting",
        38 => "chineseLegalSimplified",
        39 => "chineseCountingThousand",
        41 => "koreanDigital",
        42 => "koreanCounting",
        43 => "koreanLegal",
        44 => "koreanDigital2",
        45 => "hebrew1",
        46 => "arabicAlpha",
        47 => "hebrew2",
        48 => "arabicAbjad",
        49 => "hindiVowels",
        50 => "hindiConsonants",
        51 => "hindiNumbers",
        52 => "thaiLetters",
        53 => "thaiNumbers",
        54 => "thaiCounting",
        55 => "vietnameseCounting",
        56 => "numberInDash",
        57 => "russianLower",
        58 => "russianUpper",
        255 => "none",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Utf16;

    fn font(name: &str) -> DocFont {
        DocFont {
            name: Utf16::from(name),
            alt_name: None,
            prq: 0,
            true_type: false,
            ff: 0,
            weight: 0,
            charset: 0,
            panose: Vec::new(),
            sig: [0; 6],
        }
    }

    fn sid(istd: u16) -> Option<Cow<'static, str>> {
        (istd == 7).then_some(Cow::Borrowed("Heading7"))
    }

    fn keys<K: Copy + PartialEq>(ks: &[K]) -> KeySet<K> {
        let mut s = KeySet::new();
        for &k in ks {
            s.add(k);
        }
        s
    }

    #[test]
    fn colours_borders_and_shadings() {
        assert_eq!((hex(AUTO).as_str(), hex(0xff).as_str(), hex(0x123456).as_str()), ("auto", "0000FF", "123456"));
        assert_eq!(brc_xml("w:top", None), "<w:top w:val=\"nil\"/>");
        let b = Brc { color: 0xff0000, width: 4, brc_type: 1, space: 1, shadow: true, frame: false, from_new: false };
        assert_eq!(brc_xml("w:top", Some(&b)), "<w:top w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"FF0000\" w:shadow=\"1\"/>");
        // 0 is none (nil with no width), 0xFF is nil, an unknown type is single
        assert_eq!(brc_xml("w:left", Some(&Brc { brc_type: 0, width: 0, ..b })), "<w:left w:val=\"nil\"/>");
        assert_eq!(brc_xml("w:left", Some(&Brc { brc_type: 0, width: 3, ..b })), "<w:left w:val=\"none\"/>");
        assert_eq!(brc_xml("w:left", Some(&Brc { brc_type: 0xff, ..b })), "<w:left w:val=\"nil\"/>");
        assert!(brc_xml("w:left", Some(&Brc { brc_type: 4, ..b })).starts_with("<w:left w:val=\"single\""));
        assert_eq!(shd_xml(None), "");
        assert_eq!(
            shd_xml(Some(&Shd { fore: AUTO, back: 0xffff00, pat: 1 })),
            "<w:shd w:val=\"solid\" w:color=\"auto\" w:fill=\"FFFF00\"/>"
        );
        assert_eq!(shd_xml(Some(&Shd { fore: 0, back: 0, pat: 0xffff })), "<w:shd w:val=\"nil\"/>");
        assert_eq!(shd_xml(Some(&Shd { fore: 0, back: 0, pat: 0x30 })), "<w:shd w:val=\"clear\" w:color=\"000000\" w:fill=\"000000\"/>");
        assert_eq!(
            (lcid_tag(Some(0x0416)), lcid_tag(Some(0x0c0a)), lcid_tag(Some(0x9999)), lcid_tag(None)),
            (Some("pt-BR"), Some("es-ES"), None, None)
        );
        assert_eq!((num_fmt_of(0), num_fmt_of(23), num_fmt_of(255), num_fmt_of(40)), ("decimal", "bullet", "none", "decimal"));
        assert_eq!(
            (jc_val(Some(3), false), jc_val(Some(4), true), jc_val(Some(9), false), jc_val(Some(10), false), jc_val(None, false)),
            ("both", "both", "thaiDistribute", "left", "left")
        );
    }

    #[test]
    fn run_properties_in_schema_order() {
        let fonts = [font("Arial"), font("Ágata & <Co>")];
        let ctx = PropCtx { fonts: &fonts, style_id: &sid };
        let chp = Chp {
            istd: Some(7),
            ftc_ascii: Some(1),
            ftc_other: Some(0),
            bold: Some(true),
            italic: Some(false),
            hps: Some(24),
            cv: Some(0xff0000),
            kul: Some(1),
            cv_ul: Some(0x00ff00),
            iss: Some(2),
            lid: Some(0x0416),
            ..Chp::default()
        };
        let t = keys(&[
            ChpKey::Lid,
            ChpKey::Iss,
            ChpKey::Kul,
            ChpKey::CvUl,
            ChpKey::Hps,
            ChpKey::Cv,
            ChpKey::Italic,
            ChpKey::Bold,
            ChpKey::FtcOther,
            ChpKey::FtcAscii,
            ChpKey::Istd,
        ]);
        let x = r_pr_inner(&chp, &t, &ctx, true);
        assert_eq!(
            x,
            "<w:rStyle w:val=\"Heading7\"/><w:rFonts w:ascii=\"Ágata &amp; &lt;Co&gt;\" w:hAnsi=\"Arial\"/><w:b/><w:i w:val=\"0\"/><w:color w:val=\"FF0000\"/>\
             <w:sz w:val=\"24\"/><w:u w:val=\"single\" w:color=\"00FF00\"/><w:vertAlign w:val=\"subscript\"/><w:lang w:val=\"pt-BR\"/>"
        );
        assert!(!r_pr_inner(&chp, &t, &ctx, false).contains("rStyle"));
        // nothing touched, nothing written; a touched ico without cv uses its colour; an unknown language is not written
        assert_eq!(r_pr_inner(&chp, &KeySet::new(), &ctx, true), "");
        let c2 = Chp { ico: Some(6), lid: Some(0x9999), ..Chp::default() };
        assert_eq!(r_pr_inner(&c2, &keys(&[ChpKey::Ico, ChpKey::Lid]), &ctx, true), "<w:color w:val=\"FF0000\"/>");
    }

    #[test]
    fn tab_stops_diff() {
        let t = |pos, jc, tlc| TabStop { pos, jc, tlc };
        let base = [t(720, 0, 0)];
        let fin = [t(1440, 1, 1)];
        assert_eq!(
            tabs_xml(Some(&base), Some(&fin), &[]),
            "<w:tabs><w:tab w:val=\"clear\" w:pos=\"720\"/><w:tab w:val=\"center\" w:leader=\"dot\" w:pos=\"1440\"/></w:tabs>"
        );
        assert_eq!(tabs_xml(Some(&base), Some(&base), &[]), "");
        // an explicit clear of a position that is not in the final list, and none of one that is added back
        assert_eq!(
            tabs_xml(None, Some(&fin), &[500, 1440]),
            "<w:tabs><w:tab w:val=\"clear\" w:pos=\"500\"/><w:tab w:val=\"center\" w:leader=\"dot\" w:pos=\"1440\"/></w:tabs>"
        );
    }

    #[test]
    fn paragraph_properties_in_schema_order() {
        let fonts: [DocFont; 0] = [];
        let ctx = PropCtx { fonts: &fonts, style_id: &sid };
        let pap = Pap {
            istd: 7,
            jc: Some(3),
            dya_before: Some(120),
            line_spacing: Some(super::super::props::LineSpacing { line: -240, mult: false }),
            dxa_left: Some(720),
            dxa_left1: Some(-360),
            ilfo: Some(3),
            ilvl: Some(12),
            keep: Some(true),
            ..Pap::default()
        };
        let t = keys(&[
            PapKey::Jc,
            PapKey::DxaLeft1,
            PapKey::DxaLeft,
            PapKey::LineSpacing,
            PapKey::DyaBefore,
            PapKey::Ilvl,
            PapKey::Ilfo,
            PapKey::Keep,
        ]);
        let numid = |n: u32| i64::from(n) + 10;
        let x = p_pr_inner(
            &pap,
            &t,
            &ctx,
            &PPrOpts { num_id: Some(&numid), r_pr: Some("<w:b/>"), sect_pr: Some("<w:sectPr/>"), ..PPrOpts::default() },
        );
        assert_eq!(
            x,
            "<w:pStyle w:val=\"Heading7\"/><w:keepLines/><w:numPr><w:ilvl w:val=\"8\"/><w:numId w:val=\"13\"/></w:numPr>\
             <w:spacing w:before=\"120\" w:line=\"240\" w:lineRule=\"exact\"/><w:ind w:left=\"720\" w:hanging=\"360\"/><w:jc w:val=\"both\"/><w:rPr><w:b/></w:rPr><w:sectPr/>"
        );
        // ilfo 0 and -2047 are "no list"; without a numId function the number is the absolute value
        let none = Pap { ilfo: Some(-2047), ..Pap::default() };
        assert!(p_pr_inner(&none, &keys(&[PapKey::Ilfo]), &ctx, &PPrOpts::default()).contains("<w:numId w:val=\"0\"/>"));
        let neg = Pap { ilfo: Some(-4), ..Pap::default() };
        assert!(
            p_pr_inner(&neg, &keys(&[PapKey::Ilfo]), &ctx, &PPrOpts::default()).contains("<w:ilvl w:val=\"0\"/><w:numId w:val=\"4\"/>")
        );
    }

    #[test]
    fn frames_follow_the_word() {
        let fonts: [DocFont; 0] = [];
        let ctx = PropCtx { fonts: &fonts, style_id: &sid };
        let pap = Pap {
            dxa_abs: Some(-8),
            dya_abs: Some(240),
            dxa_width: Some(2000),
            pc: Some(0x40 | 0x10),
            wr: Some(2),
            w_height_abs: Some(600),
            frame_locked: Some(true),
            ..Pap::default()
        };
        let t = keys(&[PapKey::DxaAbs, PapKey::DyaAbs, PapKey::DxaWidth, PapKey::Wr, PapKey::WHeightAbs]);
        assert_eq!(
            p_pr_inner(&pap, &t, &ctx, &PPrOpts::default()),
            "<w:framePr w:w=\"2000\" w:h=\"600\" w:hRule=\"exact\" w:wrap=\"around\" w:vAnchor=\"page\" w:hAnchor=\"margin\" w:xAlign=\"right\" w:y=\"240\" w:anchorLock=\"1\"/>"
        );
        // only a touched, non-zero position, size, wrap or drop cap makes a frame
        let only_pc = Pap { pc: Some(0x50), ..Pap::default() };
        assert_eq!(p_pr_inner(&only_pc, &keys(&[PapKey::Pc]), &ctx, &PPrOpts::default()), "");
    }
}
