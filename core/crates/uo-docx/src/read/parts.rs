//! Frontier R2 (D1a reader, record "Milestone 1 — D1a foundation"): `ooxml/parts.ts`:
//! `readStyleSheet`, `readNumbering`, `readTheme` (Word rules over `uo_drawingml::theme::read`),
//! `readNoteProps`, `readSettings`, `readFontTable`, `partByRelType` (uo-opc D6), `numberingImages`
//! and `readDocFormatting` (the declarations are in `core/docs/modules/uo-docx-d1a/R2.md`).
//!
//! The TS readers take the part text (`string | undefined`, the empty string meaning no part);
//! here they take the parsed part (`Option<&XmlDocument>`, parsed with the docx preset by
//! [`ReadCtx::xml_part_plain`]) and [`read_doc_formatting`] does the part lookup. The `*_text`
//! functions are the text entry points of the TS signatures (an empty text is no part).
//!
//! Key orders: every object of `parts.ts` is declared in the model in the order the TS assigns
//! its keys, except `AbstractNum` (`{ abstractNumId, levels }` first, then the optional strings)
//! and `StyleSheet.defaults` (assigned in data order); those two record the assignments with
//! `mark`. Dictionaries are [`JsRecord`]s (JS own-key order, `Record<number, T>` keys are
//! `String(n)`); a key `__proto__` is never an own key of a JS plain object (assigning it sets
//! the prototype or is ignored), so [`record_set`] drops it.

use uo_drawingml::theme::{self, FontSet};
use uo_jsnum::conv::truthy;
use uo_jsnum::format::to_js_string;
use uo_jsnum::parse::{parse_float, parse_int};
use uo_opc::rels::{self, Relationship};
use uo_opc::xml::write::{Escape, escape_into};
use uo_opc::xml::{Document as XmlDocument, Element, Node, ParseOptions, parse};

use crate::error::{DocxError, DocxLimitKind};
use crate::js::{JsNumber, JsObject, JsRecord, Str};
use crate::limits::DocxLimits;
use crate::model::drawing::{ShapeShadow, ShapeStyle};
use crate::model::formatting::{
    AbstractNum, DocFormatting, FontInfo, NoteProps, NumInstance, NumLevel, NumOverride, Numbering, PicBullet, Settings, StyleDef,
    StyleSheet, TblStylePart, Theme, ThemeFontLang, ThemeFontSet,
};
use crate::model::media::{ImageData, MediaRef};

use super::DOCX;
use super::ctx::ReadCtx;
use super::drawing::read_sp_pr;
use super::matchers::Cur;
use super::props::{measure_twips, on_off_val, read_ppr, read_rpr, read_tbl_pr, read_tc_pr, read_tr_pr};

/// `o[key] = value` on a plain object used as a dictionary; `__proto__` is never an own key
/// (see the module comment).
pub(crate) fn record_set<V>(rec: &mut JsRecord<V>, key: &str, value: V) {
    if key != "__proto__" {
        rec.insert(key, value);
    }
}

/// A JS string used as a condition (`if (s)`): `Some` when non-empty.
fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.is_empty())
}

fn num(x: f64) -> JsNumber {
    JsNumber::Num(x)
}

/// `x || 0`.
fn or0(x: f64) -> f64 {
    if truthy(x) { x } else { 0.0 }
}

/// `firstChild(el, name)?.attrs['w:val']`.
fn val_of<'d>(el: Element<'d>, name: &str) -> Option<&'d str> {
    el.first_child(name).and_then(|c| c.attr("w:val"))
}

fn boxed<T>(v: Option<T>) -> Option<Box<T>> {
    v.map(Box::new)
}

// ───────────────────────── text entry points and part lookup ─────────────────────────

/// A part as parsed XML, `None` for no part and for an empty one (`if (!xml) return default`).
pub(crate) fn part_doc(ctx: &ReadCtx<'_>, name: &str) -> Result<Option<XmlDocument>, DocxError> {
    if ctx.pkg.entry(name).is_none_or(|e| e.size <= 3)
        && let Some(text) = ctx.pkg.part_text(name)?
        && text.is_empty()
    {
        return Ok(None);
    }
    ctx.xml_part_plain(name)
}

/// `parseXml(xml)` of a part text; `None` for the empty text.
pub fn parse_text(xml: &str) -> Result<Option<XmlDocument>, DocxError> {
    if xml.is_empty() {
        return Ok(None);
    }
    Ok(Some(parse(xml, &DOCX)?))
}

/// `readStyleSheet(xml)` over a text.
pub fn read_style_sheet_text(xml: &str) -> Result<StyleSheet, DocxError> {
    Ok(read_style_sheet(parse_text(xml)?.as_ref()))
}

// ───────────────────────── styles ─────────────────────────

fn tbl_style_part(tsp: Element<'_>) -> TblStylePart {
    TblStylePart {
        p_pr: boxed(read_ppr(tsp.first_child("w:pPr"))),
        r_pr: boxed(read_rpr(tsp.first_child("w:rPr"))),
        tbl_pr: boxed(read_tbl_pr(tsp.first_child("w:tblPr"))),
        tr_pr: boxed(read_tr_pr(tsp.first_child("w:trPr"))),
        tc_pr: boxed(read_tc_pr(tsp.first_child("w:tcPr"))),
        ..TblStylePart::default()
    }
}

/// `readStyleSheet` (parts.ts:60): the style definitions of `styles.xml`, unresolved.
#[must_use]
pub fn read_style_sheet(doc: Option<&XmlDocument>) -> StyleSheet {
    let mut sheet = StyleSheet::default();
    let Some(doc) = doc else { return sheet };
    let root = doc.root();
    if let Some(dd) = root.first_child("w:docDefaults") {
        let r_pr = read_rpr(dd.first_child("w:rPrDefault").unwrap_or(dd).first_child("w:rPr"));
        let p_pr = read_ppr(dd.first_child("w:pPrDefault").unwrap_or(dd).first_child("w:pPr"));
        if let Some(r) = r_pr {
            sheet.doc_defaults.r_pr = Some(Box::new(r));
        }
        if let Some(p) = p_pr {
            sheet.doc_defaults.p_pr = Some(Box::new(p));
        }
    }
    for st in root.children_named("w:style") {
        let id = st.attr("w:styleId");
        let type_ = st.attr("w:type").unwrap_or("paragraph");
        let Some(id) = non_empty(id) else { continue };
        if !matches!(type_, "paragraph" | "character" | "table" | "numbering") {
            continue;
        }
        let mut def = StyleDef { style_id: id.into(), r#type: type_.into(), ..StyleDef::default() };
        if let Some(name) = val_of(st, "w:name") {
            def.name = Some(name.into());
        }
        if let Some(v) = non_empty(val_of(st, "w:basedOn")) {
            def.based_on = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(st, "w:next")) {
            def.next = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(st, "w:link")) {
            def.link = Some(v.into());
        }
        if matches!(st.attr("w:default"), Some("1" | "true" | "on")) {
            def.is_default = Some(true);
            let slot = match type_ {
                "paragraph" => &mut sheet.defaults.paragraph,
                "character" => &mut sheet.defaults.character,
                "table" => &mut sheet.defaults.table,
                _ => &mut sheet.defaults.numbering,
            };
            if slot.is_none() {
                *slot = Some(id.into());
                sheet.defaults.mark(type_);
            }
        }
        def.p_pr = boxed(read_ppr(st.first_child("w:pPr")));
        def.r_pr = boxed(read_rpr(st.first_child("w:rPr")));
        def.tbl_pr = boxed(read_tbl_pr(st.first_child("w:tblPr")));
        def.tr_pr = boxed(read_tr_pr(st.first_child("w:trPr")));
        def.tc_pr = boxed(read_tc_pr(st.first_child("w:tcPr")));
        for tsp in st.children_named("w:tblStylePr") {
            let Some(t) = non_empty(tsp.attr("w:type")) else { continue };
            let part = tbl_style_part(tsp);
            record_set(def.tbl_style_pr.get_or_insert_with(JsRecord::new), t, part);
        }
        record_set(&mut sheet.styles, id, def);
    }
    sheet
}

// ───────────────────────── numbering ─────────────────────────

/// `vmlLenPt` (parts.ts:146), the regex `(?:^|;)\s*PROP:\s*([0-9.]+)\s*(pt|in|px|cm|mm|pc)?` (PROP
/// is the constant `width` or `height`): the first match as `(number text, unit)`.
#[must_use]
pub fn vml_len_match<'a>(style: &'a str, prop: &str) -> Option<(&'a str, Option<&'a str>)> {
    const UNITS: [&str; 6] = ["pt", "in", "px", "cm", "mm", "pc"];
    let try_at = |at: usize| -> Option<(&'a str, Option<&'a str>)> {
        let mut c = Cur::at(style, at);
        c.spaces();
        if !c.lit(prop) || !c.lit(":") {
            return None;
        }
        c.spaces();
        let number = c.take_while1(|ch| ch.is_ascii_digit() || ch == '.')?;
        c.spaces();
        let unit = UNITS.into_iter().find(|u| {
            let mut t = c;
            t.lit(u)
        });
        Some((number, unit))
    };
    if let Some(m) = try_at(0) {
        return Some(m);
    }
    for (i, b) in style.bytes().enumerate() {
        if b == b';'
            && let Some(m) = try_at(i + 1)
        {
            return Some(m);
        }
    }
    None
}

/// `vmlLenPt`: a VML style length in points.
#[must_use]
pub fn vml_len_pt(style: &str, prop: &str) -> Option<f64> {
    let (number, unit) = vml_len_match(style, prop)?;
    let v = parse_float(number);
    let k = match unit {
        Some("pt") => 1.0,
        Some("in") => 72.0,
        None | Some("px") => 0.75,
        Some("cm") => 72.0 / 2.54,
        Some("mm") => 72.0 / 25.4,
        Some("pc") => 12.0,
        Some(_) => 1.0,
    };
    v.is_finite().then_some(v * k)
}

fn read_level(el: Element<'_>) -> NumLevel {
    let ilvl = or0(parse_int(el.attr("w:ilvl").unwrap_or("0"), 10));
    let mut lvl = NumLevel { ilvl: num(ilvl), ..NumLevel::default() };
    if let Some(v) = val_of(el, "w:start") {
        lvl.start = Some(num(parse_int(v, 10)));
    }
    if let Some(fmt) = el.first_child("w:numFmt") {
        if let Some(v) = non_empty(fmt.attr("w:val")) {
            lvl.num_fmt = Some(v.into());
        }
        if let Some(v) = non_empty(fmt.attr("w:format")) {
            lvl.num_fmt_custom = Some(v.into());
        }
    }
    if let Some(v) = val_of(el, "w:lvlRestart") {
        lvl.lvl_restart = Some(num(parse_int(v, 10)));
    }
    if let Some(v) = non_empty(val_of(el, "w:pStyle")) {
        lvl.p_style = Some(v.into());
    }
    if let Some(e) = el.first_child("w:isLgl") {
        lvl.is_lgl = Some(on_off_val(e));
    }
    if let Some(v @ ("tab" | "space" | "nothing")) = val_of(el, "w:suff") {
        lvl.suff = Some(v.into());
    }
    if let Some(lt) = el.first_child("w:lvlText") {
        if let Some(v) = lt.attr("w:val") {
            lvl.lvl_text = Some(v.into());
        }
        if matches!(lt.attr("w:null"), Some("1" | "true")) {
            lvl.lvl_text_null = Some(true);
        }
    }
    if let Some(v) = val_of(el, "w:lvlPicBulletId") {
        lvl.lvl_pic_bullet_id = Some(num(parse_int(v, 10)));
    }
    if let Some(v) = non_empty(val_of(el, "w:lvlJc")) {
        lvl.lvl_jc = Some(v.into());
    }
    lvl.p_pr = boxed(read_ppr(el.first_child("w:pPr")));
    lvl.r_pr = boxed(read_rpr(el.first_child("w:rPr")));
    lvl
}

/// The pic-bullet reader of `readNumbering`: `find(pb, name)` is the first descendant `name`.
fn read_pic_bullet(pb: Element<'_>) -> PicBullet {
    let id = parse_int(pb.attr("w:numPicBulletId").unwrap_or("-1"), 10);
    let mut bullet = PicBullet { id: num(id), ..PicBullet::default() };
    let img = pb.descend("v:imagedata");
    let blip = pb.descend("a:blip");
    let rid = img.and_then(|e| e.attr("r:id")).or_else(|| blip.and_then(|e| e.attr("r:embed")));
    if let Some(r) = non_empty(rid) {
        bullet.rid = Some(r.into());
    }
    let style = pb.descend("v:shape").and_then(|e| e.attr("style")).unwrap_or("");
    if let Some(w) = vml_len_pt(style, "width") {
        bullet.width_pt = Some(num(w));
    }
    if let Some(h) = vml_len_pt(style, "height") {
        bullet.height_pt = Some(num(h));
    }
    bullet
}

/// `readNumbering` (parts.ts:178): abstract numberings, instances and picture bullets (without
/// the images, which [`read_doc_formatting`] attaches). An abstract numbering with more than
/// `limits.max_numbering_levels` `w:lvl` is `DocxError::Limit` (`NumberingLevels`).
pub fn read_numbering(doc: Option<&XmlDocument>, limits: &DocxLimits) -> Result<Numbering, DocxError> {
    let mut out = Numbering::default();
    let Some(doc) = doc else { return Ok(out) };
    let root = doc.root();
    for pb in root.children_named("w:numPicBullet") {
        let bullet = read_pic_bullet(pb);
        let key = to_js_string(bullet.id.to_number());
        record_set(&mut out.pic_bullets, &key, bullet);
    }
    for an in root.children_named("w:abstractNum") {
        let id = parse_int(an.attr("w:abstractNumId").unwrap_or("-1"), 10);
        let mut a = AbstractNum { abstract_num_id: num(id), ..AbstractNum::default() };
        a.mark("abstractNumId");
        a.mark("levels");
        if let Some(v) = non_empty(val_of(an, "w:multiLevelType")) {
            a.multi_level_type = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(an, "w:styleLink")) {
            a.style_link = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(an, "w:numStyleLink")) {
            a.num_style_link = Some(v.into());
        }
        let mut count = 0u32;
        for lvl in an.children_named("w:lvl") {
            count = count.saturating_add(1);
            if count > limits.max_numbering_levels {
                return Err(DocxError::Limit {
                    kind: DocxLimitKind::NumberingLevels,
                    max: u64::from(limits.max_numbering_levels),
                    part: None,
                });
            }
            let l = read_level(lvl);
            let key = to_js_string(l.ilvl.to_number());
            record_set(&mut a.levels, &key, l);
        }
        record_set(&mut out.abstract_nums, &to_js_string(id), a);
    }
    for n in root.children_named("w:num") {
        let num_id = parse_int(n.attr("w:numId").unwrap_or("-1"), 10);
        let abs = parse_int(val_of(n, "w:abstractNumId").unwrap_or("-1"), 10);
        let mut inst = NumInstance { num_id: num(num_id), abstract_num_id: num(abs), ..NumInstance::default() };
        for ov in n.children_named("w:lvlOverride") {
            let ilvl = or0(parse_int(ov.attr("w:ilvl").unwrap_or("0"), 10));
            let mut o = NumOverride::default();
            if let Some(v) = val_of(ov, "w:startOverride") {
                o.start_override = Some(num(parse_int(v, 10)));
            }
            o.lvl = ov.first_child("w:lvl").map(|l| Box::new(read_level(l)));
            record_set(&mut inst.overrides, &to_js_string(ilvl), o);
        }
        record_set(&mut out.nums, &to_js_string(num_id), inst);
    }
    Ok(out)
}

/// `readNumbering(xml)` over a text.
pub fn read_numbering_text(xml: &str, limits: &DocxLimits) -> Result<Numbering, DocxError> {
    read_numbering(parse_text(xml)?.as_ref(), limits)
}

// ───────────────────────── theme ─────────────────────────

/// The `a:spPr` the TS builds around style-list entries (`readSpPr({ name: 'a:spPr', attrs: {},
/// children: kids })`): the same elements serialised under a new root and parsed again, because
/// `read_sp_pr` takes a parsed element. Elements named `?x` (processing instructions the lenient
/// reader keeps as elements) are left out: no reader looks them up by name.
fn synthetic_sp_pr(kids: &[Element<'_>]) -> Result<XmlDocument, DocxError> {
    fn open(out: &mut String, el: Element<'_>) {
        out.push('<');
        out.push_str(el.name());
        for a in el.attrs() {
            out.push(' ');
            out.push_str(a.name);
            out.push_str("=\"");
            escape_into(out, a.value, Escape::ATTR);
            out.push('"');
        }
        out.push('>');
    }
    type Kids<'d> = Box<dyn Iterator<Item = Node<'d>> + 'd>;
    let mut out = String::from("<a:spPr>");
    for &root in kids {
        if root.name().starts_with('?') {
            continue;
        }
        open(&mut out, root);
        let mut stack: Vec<(Element<'_>, Kids<'_>)> = vec![(root, Box::new(root.children()))];
        while let Some((el, it)) = stack.last_mut() {
            let el = *el;
            match it.next() {
                Some(Node::Element(c)) => {
                    if !c.name().starts_with('?') {
                        open(&mut out, c);
                        stack.push((c, Box::new(c.children())));
                    }
                }
                Some(Node::Text(t) | Node::CData(t)) => escape_into(&mut out, t, Escape::TEXT),
                Some(_) => {}
                None => {
                    out.push_str("</");
                    out.push_str(el.name());
                    out.push('>');
                    stack.pop();
                }
            }
        }
    }
    out.push_str("</a:spPr>");
    Ok(parse(out, &ParseOptions::LENIENT)?)
}

/// `readSpPr(spPr-of(kids))`.
fn sp_pr_of(kids: &[Element<'_>]) -> Result<ShapeStyle, DocxError> {
    let doc = synthetic_sp_pr(kids)?;
    Ok(read_sp_pr(Some(doc.root()), None))
}

fn word_font_set(src: Option<&FontSet>) -> ThemeFontSet {
    let mut set = ThemeFontSet::default();
    let Some(src) = src else { return set };
    if let Some(v) = non_empty(src.latin.as_deref()) {
        set.latin = Some(v.into());
    }
    if let Some(v) = non_empty(src.ea.as_deref()) {
        set.ea = Some(v.into());
    }
    if let Some(v) = non_empty(src.cs.as_deref()) {
        set.cs = Some(v.into());
    }
    for f in &src.fonts {
        if let (Some(script), Some(face)) = (non_empty(f.script.as_deref()), non_empty(f.typeface.as_deref())) {
            record_set(&mut set.scripts, script, Str::from(face));
        }
    }
    set
}

/// `readTheme` (parts.ts:260): Word's rules over the as-written view of the theme part.
pub fn read_theme(doc: Option<&XmlDocument>) -> Result<Theme, DocxError> {
    let mut theme = Theme::default();
    let Some(doc) = doc else { return Ok(theme) };
    let view = theme::read(doc);
    theme.major = word_font_set(view.major.as_ref());
    theme.minor = word_font_set(view.minor.as_ref());
    if let Some(ws) = &view.line_widths {
        theme.line_widths_pt = Some(ws.iter().map(|w| num(or0(parse_int(w.as_deref().unwrap_or("0"), 10)) / 12700.0)).collect());
    }
    let element = |id| doc.element(id);
    if let Some(ids) = &view.fill_styles {
        let mut styles = Vec::with_capacity(ids.len());
        for el in ids.iter().filter_map(|&id| element(id)) {
            styles.push(sp_pr_of(&[el])?);
        }
        theme.fill_styles = Some(styles);
    }
    if let Some(ids) = &view.bg_fill_styles {
        let mut styles = Vec::with_capacity(ids.len());
        for el in ids.iter().filter_map(|&id| element(id)) {
            styles.push(sp_pr_of(&[el])?);
        }
        theme.bg_fill_styles = Some(styles);
    }
    if let Some(ids) = &view.effect_styles {
        let mut styles: Vec<Option<ShapeShadow>> = Vec::with_capacity(ids.len());
        for el in ids.iter().filter_map(|&id| element(id)) {
            let kids: Vec<Element<'_>> = el.children_named("a:effectLst").collect();
            styles.push(sp_pr_of(&kids)?.shadow.map(|s| *s));
        }
        theme.effect_styles = Some(styles);
    }
    for e in view.color_scheme.iter().flatten() {
        let key = e.name.strip_prefix("a:").unwrap_or(&e.name);
        let srgb = e.first("a:srgbClr").and_then(|c| c.val.as_deref());
        let sys = e.first("a:sysClr");
        let by_name = || match sys.and_then(|s| s.val.as_deref()) {
            Some("window") => Some("FFFFFF"),
            Some("windowText") => Some("000000"),
            _ => None,
        };
        let v = srgb.or_else(|| sys.and_then(|s| s.last_clr.as_deref())).or_else(by_name);
        if let Some(v) = non_empty(v) {
            record_set(&mut theme.colors, key, Str::from(v.to_uppercase()));
        }
    }
    Ok(theme)
}

/// `readTheme(xml)` over a text.
pub fn read_theme_text(xml: &str) -> Result<Theme, DocxError> {
    read_theme(parse_text(xml)?.as_ref())
}

// ───────────────────────── settings ─────────────────────────

/// `readNoteProps` (parts.ts:322): `w:footnotePr` / `w:endnotePr`; `None` without any key.
#[must_use]
pub fn read_note_props(el: Option<Element<'_>>) -> Option<NoteProps> {
    let el = el?;
    let mut out = NoteProps::default();
    if let Some(v) = non_empty(val_of(el, "w:pos")) {
        out.pos = Some(v.into());
    }
    if let Some(v) = non_empty(val_of(el, "w:numFmt")) {
        out.num_fmt = Some(v.into());
    }
    let start = parse_int(val_of(el, "w:numStart").unwrap_or(""), 10);
    if start.is_finite() {
        out.num_start = Some(num(start));
    }
    if let Some(v @ ("continuous" | "eachSect" | "eachPage")) = val_of(el, "w:numRestart") {
        out.num_restart = Some(v.into());
    }
    let any = out.pos.is_some() || out.num_fmt.is_some() || out.num_start.is_some() || out.num_restart.is_some();
    any.then_some(out)
}

const ROOT_COMPAT_FLAGS: [&str; 11] = [
    "w:doNotUseHTMLParagraphAutoSpacing",
    "w:doNotExpandShiftReturn",
    "w:ulTrailSpace",
    "w:spaceForUL",
    "w:balanceSingleByteDoubleByteWidth",
    "w:adjustLineHeightInTable",
    "w:doNotSuppressParagraphBorders",
    "w:noSpaceRaiseLower",
    "w:suppressSpBfAfterPgBrk",
    "w:suppressTopSpacing",
    "w:doNotUseIndentAsNumberingTabStop",
];

/// `readSettings` (parts.ts:365): `settings.xml`.
#[must_use]
pub fn read_settings(doc: Option<&XmlDocument>) -> Settings {
    let mut s =
        Settings { default_tab_stop: num(720.0), compatibility_mode: num(if doc.is_some() { 12.0 } else { 15.0 }), ..Settings::default() };
    let Some(doc) = doc else { return s };
    let root = doc.root();
    let on = |n: &str| root.first_child(n).is_some_and(on_off_val);
    if let Some(dts) = measure_twips(val_of(root, "w:defaultTabStop"))
        && dts > 0.0
    {
        s.default_tab_stop = num(dts);
    }
    s.even_and_odd_headers = on("w:evenAndOddHeaders");
    s.mirror_margins = on("w:mirrorMargins");
    s.auto_hyphenation = on("w:autoHyphenation");
    s.do_not_hyphenate_caps = on("w:doNotHyphenateCaps");
    s.display_background_shape = on("w:displayBackgroundShape");
    s.update_fields = on("w:updateFields");
    if on("w:bordersDoNotSurroundHeader") {
        s.borders_do_not_surround_header = Some(true);
    }
    if on("w:bordersDoNotSurroundFooter") {
        s.borders_do_not_surround_footer = Some(true);
    }
    s.footnote_pr = read_note_props(root.first_child("w:footnotePr"));
    s.endnote_pr = read_note_props(root.first_child("w:endnotePr"));
    if let Some(hz) = measure_twips(val_of(root, "w:hyphenationZone")) {
        s.hyphenation_zone = Some(num(hz));
    }
    if let Some(v) = val_of(root, "w:consecutiveHyphenLimit") {
        s.consecutive_hyphen_limit = Some(num(parse_int(v, 10)));
    }
    if let Some(v) = non_empty(val_of(root, "w:characterSpacingControl")) {
        s.character_spacing_control = Some(v.into());
    }
    if let Some(tfl) = root.first_child("w:themeFontLang") {
        let mut l = ThemeFontLang::default();
        if let Some(v) = non_empty(tfl.attr("w:val")) {
            l.val = Some(v.into());
        }
        if let Some(v) = non_empty(tfl.attr("w:eastAsia")) {
            l.east_asia = Some(v.into());
        }
        if let Some(v) = non_empty(tfl.attr("w:bidi")) {
            l.bidi = Some(v.into());
        }
        s.theme_font_lang = Some(l);
    }
    for c in root.child_elements() {
        if ROOT_COMPAT_FLAGS.contains(&c.name()) {
            record_set(&mut s.compat, c.name().strip_prefix("w:").unwrap_or(c.name()), on_off_val(c));
        }
    }
    if let Some(compat) = root.first_child("w:compat") {
        for c in compat.child_elements() {
            if c.name() == "w:compatSetting" {
                if c.attr("w:name") == Some("compatibilityMode") && c.attr("w:uri") == Some("http://schemas.microsoft.com/office/word") {
                    let n = parse_int(c.attr("w:val").unwrap_or(""), 10);
                    if n.is_finite() {
                        s.compatibility_mode = num(n);
                    }
                }
                continue;
            }
            record_set(&mut s.compat, c.name().strip_prefix("w:").unwrap_or(c.name()), on_off_val(c));
        }
    }
    s
}

/// `readSettings(xml)` over a text.
pub fn read_settings_text(xml: &str) -> Result<Settings, DocxError> {
    Ok(read_settings(parse_text(xml)?.as_ref()))
}

// ───────────────────────── font table ─────────────────────────

/// `readFontTable` (parts.ts:458): `w:font` entries by name.
#[must_use]
pub fn read_font_table(doc: Option<&XmlDocument>) -> JsRecord<FontInfo> {
    let mut out = JsRecord::new();
    let Some(doc) = doc else { return out };
    for f in doc.root().children_named("w:font") {
        let Some(name) = non_empty(f.attr("w:name")) else { continue };
        let mut info = FontInfo { name: name.into(), ..FontInfo::default() };
        if let Some(v) = non_empty(val_of(f, "w:altName")) {
            info.alt_name = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(f, "w:panose1")) {
            info.panose1 = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(f, "w:charset")) {
            info.charset = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(f, "w:family")) {
            info.family = Some(v.into());
        }
        if let Some(v) = non_empty(val_of(f, "w:pitch")) {
            info.pitch = Some(v.into());
        }
        if let Some(sig) = f.first_child("w:sig") {
            let mut rec = JsRecord::new();
            for a in sig.attrs() {
                record_set(&mut rec, a.name.strip_prefix("w:").unwrap_or(a.name), Str::from(a.value));
            }
            info.sig = Some(rec);
        }
        record_set(&mut out, name, info);
    }
    out
}

/// `readFontTable(xml)` over a text.
pub fn read_font_table_text(xml: &str) -> Result<JsRecord<FontInfo>, DocxError> {
    Ok(read_font_table(parse_text(xml)?.as_ref()))
}

// ───────────────────────── the aggregate ─────────────────────────

/// `partByRelType` (parts.ts:497): the part of the first internal relationship of `rels` (the
/// relationships of `word/document.xml`) whose type ends with `/<type_>`, resolved with uo-opc D6
/// (the TS `word/${target}` rules are replaced). `Some(fallback)` when no relationship matches;
/// `None` when the first match names no part (the TS returns the path, the part is then missing
/// and the caller reads the default).
#[must_use]
pub fn part_by_rel_type(ctx: &ReadCtx<'_>, rels: &[Relationship], type_: &str, fallback: &str) -> Option<String> {
    let suffix = format!("/{type_}");
    for rel in rels {
        if rel.type_.ends_with(&suffix) && rel.has_target && !rel.target.is_empty() && !rel.external {
            return ctx.resolve_target("word/document.xml", &rel.target).map(String::from);
        }
    }
    Some(fallback.to_string())
}

/// `/\.([a-z0-9]+)$/i.exec(target)?.[1]`: the extension of a relationship target as written
/// (the text after the last `.`, which must be non-empty ASCII letters and digits).
#[must_use]
pub fn target_ext_group(target: &str) -> Option<&str> {
    target.rfind('.').map(|i| &target[i + 1..]).filter(|e| !e.is_empty() && e.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The image extension of a relationship target: `(/\.([a-z0-9]+)$/i.exec(target)?.[1] ?? 'png')
/// .toLowerCase()`.
#[must_use]
pub fn target_ext(target: &str) -> String {
    target_ext_group(target).map_or_else(|| "png".to_string(), str::to_ascii_lowercase)
}

/// `numberingImages` (parts.ts:517): the image relationships of the numbering part, as `(Id,
/// part, extension)` in relationship order, for the relationships whose part exists. The bytes
/// are stored (G24) only for the images a picture bullet uses.
pub fn numbering_images(ctx: &ReadCtx<'_>, part: &str) -> Result<Vec<(String, String, String)>, DocxError> {
    let mut out = Vec::new();
    let Some(doc) = part_doc(ctx, &rels::rels_part_for(part))? else { return Ok(out) };
    for rel in rels::from_document(&doc) {
        if rel.id.is_empty() || rel.target.is_empty() || rel.external || !rel.type_.ends_with("/image") {
            continue;
        }
        let Some(path) = ctx.resolve_target(part, &rel.target) else { continue };
        if ctx.pkg.part(path)?.is_none() {
            continue;
        }
        out.push((rel.id, path.to_string(), target_ext(&rel.target)));
    }
    Ok(out)
}

/// `readDocFormatting` (parts.ts:539): styles, numbering (with its picture bullets' images),
/// theme, settings and font table of the package.
pub fn read_doc_formatting(ctx: &mut ReadCtx<'_>) -> Result<DocFormatting, DocxError> {
    read_doc_formatting_facts(ctx).map(|(f, _)| f)
}

/// [`read_doc_formatting`] and what it saw of the settings part (wave 7c, W, item 2: [`crate::model::ReadFacts`]).
pub fn read_doc_formatting_facts(ctx: &mut ReadCtx<'_>) -> Result<(DocFormatting, crate::model::ReadFacts), DocxError> {
    let rels = match part_doc(ctx, "word/_rels/document.xml.rels")? {
        Some(d) => rels::from_document(&d),
        None => Vec::new(),
    };
    let load = |ctx: &ReadCtx<'_>, type_: &str, fallback: &str| -> Result<(Option<String>, Option<XmlDocument>), DocxError> {
        let path = part_by_rel_type(ctx, &rels, type_, fallback);
        let doc = match &path {
            Some(p) => part_doc(ctx, p)?,
            None => None,
        };
        Ok((path, doc))
    };
    let (numbering_path, numbering_doc) = load(ctx, "numbering", "word/numbering.xml")?;
    let mut numbering = read_numbering(numbering_doc.as_ref(), &ctx.limits)
        .map_err(|e| e.in_part(numbering_path.as_deref().unwrap_or("word/numbering.xml")))?;
    if !numbering.pic_bullets.is_empty()
        && let Some(path) = &numbering_path
    {
        let images = numbering_images(ctx, path)?;
        let keys: Vec<String> = numbering.pic_bullets.keys().map(String::from).collect();
        for key in keys {
            let Some(rid) = numbering.pic_bullets.get(&key).and_then(|b| non_empty(b.rid.as_deref())).map(String::from) else { continue };
            let Some((_, image_path, ext)) = images.iter().rev().find(|(id, _, _)| *id == rid) else { continue };
            let Some(data) = ctx.media_part(image_path)? else { continue };
            if let Some(b) = numbering.pic_bullets.get_mut(&key) {
                b.image = Some(ImageData { data: MediaRef(data), ext: ext.as_str().into(), ..ImageData::default() });
            }
        }
    }
    let (_, styles) = load(ctx, "styles", "word/styles.xml")?;
    let (_, theme_doc) = load(ctx, "theme", "word/theme/theme1.xml")?;
    let (_, settings) = load(ctx, "settings", "word/settings.xml")?;
    let (_, fonts) = load(ctx, "fontTable", "word/fontTable.xml")?;
    let facts = crate::model::ReadFacts {
        settings_absent: settings.is_none(),
        default_tab_absent: settings.as_ref().is_none_or(|d| d.root().first_child("w:defaultTabStop").is_none()),
    };
    let f = DocFormatting {
        styles: read_style_sheet(styles.as_ref()),
        numbering,
        theme: read_theme(theme_doc.as_ref())?,
        settings: read_settings(settings.as_ref()),
        font_table: read_font_table(fonts.as_ref()),
        ..DocFormatting::default()
    };
    Ok((f, facts))
}
