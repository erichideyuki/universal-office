//! The deck half of `read.ts`: `readPresentation`, `readShowSettings`, `themeFor`, `findRelTarget`
//! callers, `readSlideNotes`, `readSlide` (inheritance from layout and master, the text cascade,
//! background, legacy animations and transition), `placeholderXfrmMap`, `placeholderBodyPrMap`,
//! `buildTextStyleContext`, `collectPhStyles`. The shape reader itself is `uo_dml_render::read`.
//!
//! The TS reads the master and layout shape trees up to five times per slide; here each part is
//! read once per (part, rels) inside one `read_slide` (the passes are deterministic: the same
//! inputs give the same shapes), behaviour-neutral. Not here: `parseSlideTiming` and the bookmark
//! trigger fix-up of `readSlide` (frontier B6: `SlideModel::timing` stays `None`).

use std::collections::HashMap;
use std::sync::Arc;

use uo_dml_render::CssColor;
use uo_dml_render::model::*;
use uo_dml_render::read::{StyleFill, read_shapes, theme_style_fill};
use uo_dml_render::read_dml::{Rd, apply_tile, parse_grad_fill, parse_patt_fill, resolve_blip, usize_of};
use uo_dml_render::source::{Limits, PartSource, R, ReadError, RelMap, find_rel_target, int_attr, pint, read_rels, rels_path_for, ta};
use uo_dml_render::text_style::{
    PhMap, StyleLevels, TextStyleContext, TextStyleLevel, apply_shape_text_cascade, apply_text_styles, merge_style_levels,
    parse_tx_style_levels,
};
use uo_dml_render::theme::{Theme, apply_clr_map, clr_map_attrs, read_theme, solid_fill_color};
use uo_opc::xml::{Document, Element};

use crate::model::*;
use crate::timing::AnimTarget;

/// `phKey` (`read.ts:569`): the placeholder key, the TS string (`title#3`, `body`).
pub fn ph_key(ty: Option<&str>, idx: Option<&str>) -> String {
    // normalizes the title family: ctrTitle ~ title
    let t = if ty == Some("ctrTitle") { "title" } else { ty.unwrap_or("body") };
    match idx {
        Some(i) => format!("{t}#{i}"),
        None => t.to_string(),
    }
}

/// The master placeholder type a layout/slide placeholder inherits (`masterTypeOf`).
pub fn master_type_of(ty: &str) -> &str {
    if ty == "ctrTitle" {
        return "title";
    }
    if matches!(ty, "title" | "dt" | "ftr" | "sldNum") { ty } else { "body" }
}

/// `readPresentation` (`read.ts:75`): slide size and slide list of `ppt/presentation.xml`.
pub fn read_presentation(src: &dyn PartSource) -> R<Presentation> {
    let doc = src.xml("ppt/presentation.xml")?.ok_or_else(|| ReadError::Missing("ppt/presentation.xml".into()))?;
    let root = doc.root();
    let rels = read_rels(src, "ppt/_rels/presentation.xml.rels", "ppt/presentation.xml")?;
    let sz = root.first_child("p:sldSz");
    let slide_w = sz.and_then(|s| int_attr(s, "cx")).unwrap_or(9144000.0);
    let slide_h = sz.and_then(|s| int_attr(s, "cy")).unwrap_or(5143500.0);
    let mut slides = Vec::new();
    if let Some(lst) = root.first_child("p:sldIdLst") {
        for s in lst.children_named("p:sldId") {
            let r_id = s.attr("r:id").unwrap_or("");
            slides.push(SlideRef { id: s.attr("id").unwrap_or("").into(), r_id: r_id.into(), target: rels.get(r_id).map(Into::into) });
        }
    }
    let mut pres = Presentation { slide_w, slide_h, size_type: None, slides, show: None };
    if let Some(t) = sz.and_then(|s| ta(s, "type")) {
        pres.size_type = Some(t.into());
    }
    pres.show = read_show_settings(src)?;
    Ok(pres)
}

/// `readShowSettings` (`read.ts:100`): `<p:showPr>` of `presProps` (configure the show). An absent or
/// unreadable part is `None`.
fn read_show_settings(src: &dyn PartSource) -> R<Option<ShowSettings>> {
    let part = find_rel_target(src, "ppt/presentation.xml", "presProps")?.unwrap_or_else(|| "ppt/presProps.xml".to_string());
    let Some(text) = src.text(&part) else { return Ok(None) };
    let _ = text;
    let Ok(Some(doc)) = src.xml(&part) else { return Ok(None) };
    let Some(sp) = doc.root().first_child("p:showPr") else { return Ok(None) };
    let bool_of = |v: Option<&str>| v.map(|v| v == "1" || v == "true");
    let mut out = ShowSettings {
        loop_: bool_of(sp.attr("loop")),
        use_timings: bool_of(sp.attr("useTimings")),
        show_animation: bool_of(sp.attr("showAnimation")),
        ..ShowSettings::default()
    };
    if sp.first_child("p:kiosk").is_some() {
        out.kiosk = Some(true);
    }
    if let Some(rg) = sp.first_child("p:sldRg") {
        let st = pint(rg.attr("st").unwrap_or(""));
        let end = pint(rg.attr("end").unwrap_or(""));
        if st.is_finite() && end.is_finite() && st >= 1.0 && end >= st {
            out.range = Some((st - 1.0, end - 1.0));
        }
    }
    Ok(Some(out))
}

/// `slideNumberOf`'s map (`read.ts:57`): displayed number of each slide = position in the list +
/// `firstSlideNum`. A presentation that cannot be read gives an empty map.
fn slide_numbers(src: &dyn PartSource) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    let Ok(pres) = read_presentation(src) else { return map };
    let first = match src.xml("ppt/presentation.xml") {
        Ok(Some(d)) => {
            let v = pint(d.root().attr("firstSlideNum").unwrap_or("1"));
            if v == 0.0 || v.is_nan() { 1.0 } else { v }
        }
        _ => return map,
    };
    for (i, s) in pres.slides.iter().enumerate() {
        if let Some(t) = s.target.as_deref().filter(|t| !t.is_empty()) {
            map.insert(t.to_string(), first + i as f64);
        }
    }
    map
}

/// `themeFor` (`read.ts:122`): the theme of the master + clrMap (slide override → layout override → master).
pub fn theme_for(src: &dyn PartSource, part: &str) -> R<Theme> {
    // master → theme. Slides do not reference the theme directly; they go up to the master.
    let master_path = match find_rel_target(src, part, "slideMaster")? {
        Some(m) => Some(m),
        None => {
            let l = find_rel_target(src, part, "slideLayout")?.unwrap_or_default();
            find_rel_target(src, &l, "slideMaster")?
        }
    }
    .filter(|m| !m.is_empty());
    let theme_path = match &master_path {
        Some(m) => find_rel_target(src, m, "theme")?,
        None => find_rel_target(src, "ppt/presentation.xml", "theme")?,
    };
    let theme = read_theme(src, theme_path.as_deref().unwrap_or("ppt/theme/theme1.xml"))?;
    // clrMap: the master's; `<p:clrMapOvr><a:overrideClrMapping>` of the part itself (slide) and,
    // without it, the layout's replace it (`masterClrMapping` = inherits)
    let ovr = |p: Option<&str>| -> R<Option<uo_dml_render::theme::JsRecord<Box<str>>>> {
        let Some(p) = p.filter(|p| !p.is_empty()) else { return Ok(None) };
        let Some(doc) = src.xml(p)? else { return Ok(None) };
        let o = doc.root().first_child("p:clrMapOvr");
        Ok(clr_map_attrs(o.and_then(|o| o.first_child("a:overrideClrMapping"))))
    };
    let master_map = match master_path.as_deref() {
        Some(m) => match src.xml(m)? {
            Some(d) => clr_map_attrs(d.root().first_child("p:clrMap")),
            None => None,
        },
        None => None,
    };
    let layout_path: Option<String> = if Some(part) == master_path.as_deref() {
        None
    } else if part.contains("slideLayout") {
        Some(part.to_string())
    } else {
        find_rel_target(src, part, "slideLayout")?
    };
    let own = if Some(part) != layout_path.as_deref() { ovr(Some(part))? } else { None };
    let map = match own {
        Some(m) => Some(m),
        None => match ovr(layout_path.as_deref())? {
            Some(m) => Some(m),
            None => master_map,
        },
    };
    Ok(apply_clr_map(theme, map.as_ref()))
}

/// `readSlideNotes` (`read.ts:162`): the speaker notes of a slide (plain text, one paragraph per line):
/// the `body` placeholder of the notesSlide linked to the slide. No notes → ''.
pub fn read_slide_notes(src: &dyn PartSource, slide_path: &str) -> R<String> {
    let notes_path = find_rel_target(src, slide_path, "notesSlide")?;
    let doc = match notes_path.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => {
            if src.text(p).is_none_or(|t| t.is_empty()) {
                return Ok(String::new());
            }
            src.xml(p)?
        }
        None => None,
    };
    let Some(doc) = doc else { return Ok(String::new()) };
    let Some(tree) = doc.root().descend("p:spTree") else { return Ok(String::new()) };
    for sp in tree.children_named("p:sp") {
        let ph = sp.descend("p:nvPr").and_then(|n| n.first_child("p:ph"));
        if ph.and_then(|p| p.attr("type")) != Some("body") {
            continue;
        }
        let Some(body) = sp.first_child("p:txBody") else { continue };
        let paras: Vec<String> = body
            .children_named("a:p")
            .map(|p| {
                let mut s = String::new();
                for c in p.children() {
                    if let uo_opc::xml::Node::Element(e) = c {
                        match e.name() {
                            "a:br" => s.push('\n'),
                            "a:r" | "a:fld" => s.push_str(&e.first_child("a:t").unwrap_or(e).text_content()),
                            _ => {}
                        }
                    }
                }
                s
            })
            .collect();
        return Ok(uo_jsnum::text::trim(&paras.join("\n")).to_string());
    }
    Ok(String::new())
}

/// The fields of a layout/master placeholder shape the slide inherits from its `spPr`
/// (`spMap` of `readSlide`): the merge `{...prev, ...stripUndef(psh)}` is "defined fields override".
#[derive(Clone, Default)]
struct PhShapeProps {
    shadow: Option<OuterShadow>,
    fill: Option<Box<Fill>>,
    fill_alpha: Option<f64>,
    fill_image: Option<Box<ImageRef>>,
    fill_tile: Option<bool>,
    use_bg_fill: Option<bool>,
    line_color: Option<CssColor>,
    line_none: Option<bool>,
    line_width: Option<f64>,
    line_dash: Option<Box<str>>,
    geom: Option<Box<str>>,
    adj: Option<Box<[f64]>>,
    adj_names: Option<Box<[Box<str>]>>,
    cust_geom: Option<Box<CustGeom>>,
    glow: Option<Glow>,
    soft_edge: Option<f64>,
    reflection: Option<Reflection>,
    inner_shadow: Option<InnerShadow>,
}

impl PhShapeProps {
    fn of(s: &Shape) -> PhShapeProps {
        let e = s.extras.as_deref();
        PhShapeProps {
            shadow: e.and_then(|e| e.fx().shadow.clone()),
            fill: s.fill.clone(),
            fill_alpha: s.paint().fill_alpha,
            fill_image: s.paint().fill_image.clone(),
            fill_tile: s.paint().fill_tile,
            use_bg_fill: s.paint().use_bg_fill,
            line_color: s.line_color.clone(),
            line_none: s.line_none,
            line_width: s.line_width,
            line_dash: s.x().line_dash.clone(),
            geom: s.geom.clone(),
            adj: s.x().adj.clone(),
            adj_names: s.x().adj_names.clone(),
            cust_geom: s.cust_geom.clone(),
            glow: e.and_then(|e| e.fx().glow.clone()),
            soft_edge: e.and_then(|e| e.fx().soft_edge),
            reflection: e.and_then(|e| e.fx().reflection),
            inner_shadow: e.and_then(|e| e.fx().inner_shadow.clone()),
        }
    }

    /// `{...self, ...over}` without the undefined fields of `over`.
    fn over(&mut self, o: &PhShapeProps) {
        macro_rules! take {
            ($($f:ident),*) => { $(if o.$f.is_some() { self.$f = o.$f.clone(); })* };
        }
        take!(
            shadow,
            fill,
            fill_alpha,
            fill_image,
            fill_tile,
            use_bg_fill,
            line_color,
            line_none,
            line_width,
            line_dash,
            geom,
            adj,
            adj_names,
            cust_geom,
            glow,
            soft_edge,
            reflection,
            inner_shadow
        );
    }
}

/// The `bodyPr` properties a placeholder passes on (`placeholderBodyPrMap`, `read.ts:614`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct BodyPrProps {
    pub anchor: Option<Box<str>>,
    pub wrap: Option<Box<str>>,
    pub spc_first_last_para: Option<bool>,
    pub vert: Option<Box<str>>,
    pub ins_l: Option<f64>,
    pub ins_t: Option<f64>,
    pub ins_r: Option<f64>,
    pub ins_b: Option<f64>,
    pub num_col: Option<f64>,
    pub spc_col: Option<f64>,
    pub autofit: Option<Autofit>,
    pub font_scale: Option<f64>,
    pub ln_spc_reduction: Option<f64>,
}

impl BodyPrProps {
    /// `{...self, ...over}`.
    fn merged(&self, o: &BodyPrProps) -> BodyPrProps {
        macro_rules! pick {
            ($($f:ident),*) => { BodyPrProps { $($f: o.$f.clone().or_else(|| self.$f.clone())),* } };
        }
        pick!(anchor, wrap, spc_first_last_para, vert, ins_l, ins_t, ins_r, ins_b, num_col, spc_col, autofit, font_scale, ln_spc_reduction)
    }
}

/// The shapes of one master/layout part, read once per (part, rels) inside a `read_slide`.
struct PartShapes {
    part: String,
    empty_rels: bool,
    shapes: Vec<Shape>,
}

#[derive(Default)]
struct ShapeMemo {
    items: Vec<PartShapes>,
}

impl ShapeMemo {
    /// The shapes of `part`'s `<p:spTree>` read with the part's own rels (or none): an absent part or
    /// tree gives no shapes.
    fn get(&mut self, rd: &Rd<'_>, theme: &Theme, part: &str, empty_rels: bool) -> R<&[Shape]> {
        let pos = match self.items.iter().position(|p| p.part == part && p.empty_rels == empty_rels) {
            Some(p) => p,
            None => {
                let shapes = Self::read(rd, theme, part, empty_rels)?;
                self.items.push(PartShapes { part: part.to_string(), empty_rels, shapes });
                self.items.len() - 1
            }
        };
        Ok(self.items.get(pos).map_or(&[][..], |p| &p.shapes[..]))
    }

    fn read(rd: &Rd<'_>, theme: &Theme, part: &str, empty_rels: bool) -> R<Vec<Shape>> {
        let Some(doc) = rd.src.xml(part)? else { return Ok(Vec::new()) };
        let Some(tree) = doc.root().descend("p:spTree") else { return Ok(Vec::new()) };
        let rels = if empty_rels { RelMap::new() } else { read_rels(rd.src, &rels_path_for(part), part)? };
        read_shapes(rd, tree, theme, &rels)
    }
}

/// The date/time fields of a shape (its text and its table cells; groups are flat in the model) with the clock's date in the run's
/// language (`uo_dml_render::datefield`).
fn datetime_fields(sh: &mut Shape, clock: &uo_dml_render::datefield::FieldClock) {
    use uo_dml_render::datefield::{format_datetime_field, is_datetime_field};
    let fix = |tb: &mut uo_dml_render::model::TextBody| {
        for p in &mut tb.paragraphs {
            for r in &mut p.runs {
                let Some(kind) = r.extras.as_ref().and_then(|e| e.field.as_deref()).filter(|k| is_datetime_field(k)) else { continue };
                if let Some(t) = format_datetime_field(kind, r.lang.as_deref(), clock) {
                    r.text = t.into();
                }
            }
        }
    };
    if let Some(tb) = sh.tx_body.as_deref_mut() {
        fix(tb);
    }
    if let Some(t) = sh.extras.as_deref_mut().and_then(|e| e.table.as_deref_mut()) {
        for row in &mut t.rows {
            for cell in &mut row.cells {
                if let Some(tb) = cell.tx_body.as_deref_mut() {
                    fix(tb);
                }
            }
        }
    }
}

fn has_ph(s: &Shape) -> bool {
    s.ph_type.is_some() || s.ph_idx.is_some()
}

/// `readSlide` (`read.ts:197`): reads a slide, inheriting placeholder geometry from the layout/master.
pub fn read_slide(src: &dyn PartSource, slide_path: &str, limits: Limits) -> R<SlideModel> {
    read_slide_with(src, slide_path, limits, &crate::limits::Limits::default())
}

/// [`read_slide`] with the presentation-layer limits (`max_time_depth`/`max_time_nodes` bound the time tree).
pub fn read_slide_with(src: &dyn PartSource, slide_path: &str, limits: Limits, time_limits: &crate::limits::Limits) -> R<SlideModel> {
    let rd = Rd { src, limits };
    let doc = src.xml(slide_path)?.ok_or_else(|| ReadError::Missing(slide_path.into()))?;
    let root = doc.root();
    let theme = theme_for(src, slide_path)?;
    // the slide's .rels is needed to resolve `r:embed` of `<p:pic>` and the bg image
    let slide_rels = read_rels(src, &rels_path_for(slide_path), slide_path)?;

    let c_sld = root.first_child("p:cSld");
    let sp_tree = c_sld.and_then(|c| c.first_child("p:spTree"));
    let mut shapes: Vec<Shape> = match sp_tree {
        Some(t) => read_shapes(&rd, t, &theme, &slide_rels)?,
        None => Vec::new(),
    };

    // INHERITED decorations: shapes WITHOUT a placeholder of the master and the layout are drawn under
    // the slide's own (a Slidesgo-style template background lives here). `showMasterSp="0"` on the
    // layout turns the master's off.
    let layout_path = find_rel_target(src, slide_path, "slideLayout")?.filter(|p| !p.is_empty());
    let master_path = match &layout_path {
        Some(l) => find_rel_target(src, l, "slideMaster")?.filter(|p| !p.is_empty()),
        None => None,
    };
    let mut memo = ShapeMemo::default();
    let mut inherited: Vec<Shape> = Vec::new();
    // on the SLIDE, showMasterSp="0" = "hide background graphics": neither master nor layout
    if let Some(lp) = &layout_path
        && root.attr("showMasterSp") != Some("0")
        && let Some(lroot_doc) = src.xml(lp)?
    {
        let layout_show_master_sp = lroot_doc.root().attr("showMasterSp") != Some("0");
        for (part, enabled) in [(master_path.as_deref(), layout_show_master_sp), (Some(lp.as_str()), true)] {
            let Some(part) = part.filter(|_| enabled) else { continue };
            for sh in memo.get(&rd, &theme, part, false)? {
                if has_ph(sh) {
                    continue; // placeholder ≠ decoration
                }
                let mut sh = sh.clone();
                sh.inherited = Some(true);
                inherited.push(sh);
            }
        }
    }
    if !inherited.is_empty() {
        inherited.append(&mut shapes);
        shapes = inherited;
    }
    // slide number: the application shows the CURRENT number (the field's cache is the one of the
    // recording — in a master/layout it is always the same, e.g. "1")
    if let Some(num) = slide_numbers(src).get(slide_path).copied() {
        let text = uo_jsnum::format::to_js_string(num);
        for sh in &mut shapes {
            let Some(tb) = sh.tx_body.as_deref_mut() else { continue };
            for p in &mut tb.paragraphs {
                for r in &mut p.runs {
                    if r.extras.as_ref().and_then(|e| e.field.as_deref()) == Some("slidenum") {
                        r.text = text.as_str().into();
                    }
                }
            }
        }
    }

    // date/time fields: PowerPoint shows the CURRENT date in the field's format and language (Slides wave 5; the TS the cache)
    if !src.rules().ts_date_locale
        && let Some(clock) = src.field_clock()
    {
        for sh in &mut shapes {
            datetime_fields(sh, &clock);
        }
    }

    // inherits xfrm of placeholders without their own geometry
    let ph_map = placeholder_xfrm_map(&rd, &theme, &mut memo, layout_path.as_deref(), master_path.as_deref())?;
    for sh in &mut shapes {
        if sh.xfrm.is_none()
            && let Some(t) = sh.ph_type.as_deref()
        {
            let x = ph_map
                .get(&ph_key(Some(t), sh.ph_idx.as_deref()))
                .or_else(|| ph_map.get(&ph_key(Some(t), None)))
                .or_else(|| ph_map.get(&format!("idx:{}", sh.ph_idx.as_deref().unwrap_or(""))));
            if let Some(x) = x {
                sh.xfrm = Some(*x);
            }
        }
    }

    // spPr inherited from the placeholder: the layout/master SHADOW applies to the slide (template
    // titles carry outerShdw on the layout placeholder) and also the rest of the spPr: fill, outline,
    // geometry and effects of the layout/master placeholder hold on the slide when the shape has none
    let mut sp_map: PhMap<PhShapeProps> = PhMap::default();
    for part in [master_path.as_deref(), layout_path.as_deref()] {
        let Some(part) = part else { continue };
        let is_layout = Some(part) == layout_path.as_deref();
        for psh in memo.get(&rd, &theme, part, false)? {
            if !has_ph(psh) {
                continue;
            }
            let props = PhShapeProps::of(psh);
            let mut merge = |k: &str| {
                let merged = match sp_map.get(k) {
                    Some(prev) => {
                        let mut m = prev.clone();
                        m.over(&props);
                        m
                    }
                    None => props.clone(),
                };
                sp_map.set(k, merged);
            };
            merge(&ph_key(psh.ph_type.as_deref(), psh.ph_idx.as_deref()));
            merge(&ph_key(psh.ph_type.as_deref(), None));
            if let (Some(idx), true) = (psh.ph_idx.as_deref(), is_layout) {
                merge(&format!("idx:{idx}"));
            }
        }
    }
    for sh in &mut shapes {
        if sh.inherited == Some(true) || !has_ph(sh) {
            continue;
        }
        let inh = sp_map
            .get(&ph_key(sh.ph_type.as_deref(), sh.ph_idx.as_deref()))
            .or_else(|| sp_map.get(&ph_key(sh.ph_type.as_deref(), None)))
            .or_else(|| sp_map.get(&format!("idx:{}", sh.ph_idx.as_deref().unwrap_or(""))));
        let Some(inh) = inh else { continue };
        if sh.extras.as_ref().is_none_or(|e| e.fx().shadow.is_none()) && inh.shadow.is_some() {
            sh.extras_mut().fx_mut().shadow = inh.shadow.clone();
        }
        if sh.fill.is_none() && sh.paint().fill_image.is_none() && sh.grp_fill.is_none() && sh.paint().use_bg_fill.is_none() {
            if inh.fill.is_some() {
                sh.fill = inh.fill.clone();
            }
            if inh.fill_alpha.is_some() && sh.paint().fill_alpha.is_none() {
                sh.paint_mut().fill_alpha = inh.fill_alpha;
            }
            if inh.fill_image.is_some() {
                sh.paint_mut().fill_image = inh.fill_image.clone();
                if inh.fill_tile == Some(true) {
                    sh.paint_mut().fill_tile = Some(true);
                }
            }
            if inh.use_bg_fill == Some(true) {
                sh.paint_mut().use_bg_fill = Some(true);
            }
        }
        if sh.line_color.is_none() && sh.line_none != Some(true) {
            if inh.line_color.as_ref().is_some_and(|c| !c.as_str().is_empty()) {
                sh.line_color = inh.line_color.clone();
            }
            if inh.line_none == Some(true) {
                sh.line_none = Some(true);
            }
            if sh.line_width.is_none() && inh.line_width.is_some() {
                sh.line_width = inh.line_width;
            }
            if sh.x().line_dash.is_none() && inh.line_dash.is_some() {
                sh.x_mut().line_dash = inh.line_dash.clone();
            }
        }
        if sh.geom.is_none() && sh.cust_geom.is_none() && inh.geom.as_deref().is_some_and(|g| !g.is_empty()) {
            sh.geom = inh.geom.clone();
            if inh.adj.is_some() {
                sh.x_mut().adj = inh.adj.clone();
                if inh.adj_names.is_some() {
                    sh.x_mut().adj_names = inh.adj_names.clone();
                }
            }
        } else if sh.geom.is_none() && sh.cust_geom.is_none() && inh.cust_geom.is_some() {
            sh.cust_geom = inh.cust_geom.clone();
        }
        if sh.extras.as_ref().is_none_or(|e| e.fx().glow.is_none()) && inh.glow.is_some() {
            sh.extras_mut().fx_mut().glow = inh.glow.clone();
        }
        if sh.extras.as_ref().is_none_or(|e| e.fx().soft_edge.is_none()) && inh.soft_edge.is_some() {
            sh.extras_mut().fx_mut().soft_edge = inh.soft_edge;
        }
        if sh.extras.as_ref().is_none_or(|e| e.fx().reflection.is_none()) && inh.reflection.is_some() {
            sh.extras_mut().fx_mut().reflection = inh.reflection;
        }
        if sh.extras.as_ref().is_none_or(|e| e.fx().inner_shadow.is_none()) && inh.inner_shadow.is_some() {
            sh.extras_mut().fx_mut().inner_shadow = inh.inner_shadow.clone();
        }
    }

    // bodyPr inherited from the placeholder (vertical anchor/insets/autofit/columns)
    let body_pr_map = placeholder_body_pr_map(src, layout_path.as_deref(), master_path.as_deref())?;
    for sh in &mut shapes {
        if sh.tx_body.is_none() || !has_ph(sh) {
            continue;
        }
        // matches by idx in the layout before the type (the type falls to the master)
        let inh = body_pr_map
            .get(&ph_key(sh.ph_type.as_deref(), sh.ph_idx.as_deref()))
            .or_else(|| sh.ph_idx.as_deref().and_then(|i| body_pr_map.get(&format!("idx:{i}"))))
            .or_else(|| body_pr_map.get(&ph_key(sh.ph_type.as_deref(), None)))
            .or_else(|| body_pr_map.get(&ph_key(Some(master_type_of(sh.ph_type.as_deref().unwrap_or("body"))), None)));
        let Some(inh) = inh else { continue };
        let Some(tb) = sh.tx_body.as_deref_mut() else { continue };
        if tb.anchor.is_none() && inh.anchor.is_some() {
            tb.anchor = inh.anchor.clone();
        }
        if tb.wrap.is_none() && inh.wrap.is_some() {
            tb.wrap = inh.wrap.clone();
        }
        if tb.x().spc_first_last_para.is_none() && inh.spc_first_last_para.is_some() {
            tb.x_mut().spc_first_last_para = inh.spc_first_last_para;
        }
        if tb.x().vert.is_none() && inh.vert.is_some() {
            tb.x_mut().vert = inh.vert.clone();
        }
        if tb.ins_l.is_none() && inh.ins_l.is_some() {
            tb.ins_l = inh.ins_l;
        }
        if tb.ins_t.is_none() && inh.ins_t.is_some() {
            tb.ins_t = inh.ins_t;
        }
        if tb.ins_r.is_none() && inh.ins_r.is_some() {
            tb.ins_r = inh.ins_r;
        }
        if tb.ins_b.is_none() && inh.ins_b.is_some() {
            tb.ins_b = inh.ins_b;
        }
        if tb.x().num_col.is_none() && inh.num_col.is_some() {
            tb.x_mut().num_col = inh.num_col;
        }
        if tb.x().spc_col.is_none() && inh.spc_col.is_some() {
            tb.x_mut().spc_col = inh.spc_col;
        }
        if tb.autofit.is_none() && inh.autofit.is_some() {
            tb.autofit = inh.autofit;
        }
        if tb.x().font_scale.is_none() && inh.font_scale.is_some() && tb.autofit == Some(Autofit::Norm) {
            tb.x_mut().font_scale = inh.font_scale;
        }
        if tb.x().ln_spc_reduction.is_none() && inh.ln_spc_reduction.is_some() {
            tb.x_mut().ln_spc_reduction = inh.ln_spc_reduction;
        }
    }

    // text-style cascade (spec 03 A/B): defaultTextStyle → master txStyles → fontRef of `<p:style>` →
    // placeholder lstStyle (master/layout) → the txBody's own lstStyle. Fills defaults in Run/Paragraph.
    let style_ctx = build_text_style_context(&rd, &theme, &mut memo, layout_path.as_deref(), master_path.as_deref())?;
    for sh in &mut shapes {
        if sh.tx_body.is_none() {
            continue;
        }
        let keys: Vec<String> = if has_ph(sh) {
            vec![
                ph_key(sh.ph_type.as_deref(), sh.ph_idx.as_deref()),
                ph_key(sh.ph_type.as_deref(), None),
                format!("idx:{}", sh.ph_idx.as_deref().unwrap_or("")),
            ]
        } else {
            Vec::new()
        };
        let m_type = match sh.ph_type.as_deref() {
            None | Some("obj" | "subTitle") => "body",
            Some(t) => t,
        };
        let master_keys: Vec<String> = if keys.is_empty() { Vec::new() } else { vec![ph_key(Some(m_type), None)] };
        apply_shape_text_cascade(sh, &style_ctx, &theme, &keys, &master_keys);
        // no layer defined the colour: the application default is tx1 (already through the clrMap — white in a dark theme)
        if let Some(tx1) = theme.colors.get("tx1").filter(|c| !c.as_str().is_empty())
            && let Some(tb) = sh.tx_body.as_deref_mut()
        {
            for p in &mut tb.paragraphs {
                for r in &mut p.runs {
                    if r.color.is_none() {
                        r.color = Some(tx1.clone());
                    }
                }
            }
        }
    }
    // table cells: defaultTextStyle + otherStyle of the master + the table STYLE's tcTxStyle
    // (colour/bold per part, e.g. white/bold header) on top
    for sh in &mut shapes {
        let Some(table) = sh.extras.as_deref_mut().and_then(|e| e.table.as_deref_mut()) else { continue };
        let base = merge_style_levels(&[&style_ctx.default_style, &style_ctx.other_style]);
        for row in &mut table.rows {
            for cell in &mut row.cells {
                let (tx_color, tx_bold) = (cell.tx_color.clone(), cell.tx_bold);
                let Some(tb) = cell.tx_body.as_deref_mut() else { continue };
                let mut overlay = TextStyleLevel::default();
                if tx_color.is_some() {
                    overlay.r_mut().color = tx_color;
                }
                if tx_bold == Some(true) {
                    overlay.r_mut().bold = Some(true);
                }
                if overlay.r().color.is_some() || overlay.r().bold.is_some() {
                    let ov: Box<StyleLevels> = Box::new(core::array::from_fn(|_| overlay.clone()));
                    let levels = merge_style_levels(&[&base, &ov]);
                    apply_text_styles(tb, &levels, &theme);
                } else {
                    apply_text_styles(tb, &base, &theme);
                }
            }
        }
    }

    let mut slide = SlideModel::default();
    // background: the SLIDE's wins; without it, inherits from the LAYOUT and finally the MASTER
    // (each resolves blips with ITS OWN rels).
    let mut bg = BgState::default();
    let mut bg_done = match c_sld {
        Some(c) => apply_bg(src, &theme, &mut bg, c, &slide_rels)?,
        None => false,
    };
    if !bg_done
        && let Some(lp) = &layout_path
        && let Some(ld) = src.xml(lp)?
    {
        let rels = read_rels(src, &rels_path_for(lp), lp)?;
        bg_done = apply_bg(src, &theme, &mut bg, ld.root(), &rels)?;
    }
    if !bg_done
        && let Some(mp) = &master_path
        && let Some(md) = src.xml(mp)?
    {
        let rels = read_rels(src, &rels_path_for(mp), mp)?;
        apply_bg(src, &theme, &mut bg, md.root(), &rels)?;
    }
    slide.bg = bg.bg;
    slide.bg_image = bg.image;
    slide.bg_tile = bg.tile.then_some(true);
    // no <p:bg> at any level: the background is bg1 of the EFFECTIVE colour map (a layout clrMapOvr
    // turning bg1→dk1 leaves the slide black in the application)
    if slide.bg.is_none()
        && slide.bg_image.is_none()
        && let Some(bg1) = theme.colors.get("bg1").filter(|c| !c.as_str().is_empty())
    {
        slide.bg = Some(solid(bg1.clone()));
    }
    // shadeToTitle: the focus of the path gradient is the TITLE's rectangle (the slide's; else the master's)
    if bg.shade_to_title
        && slide.bg.as_ref().is_some_and(|b| b.kind == FillKind::Gradient && b.path.as_deref().is_some_and(|p| !p.is_empty()))
    {
        let t = shapes
            .iter()
            .find(|s| matches!(s.ph_type.as_deref(), Some("title" | "ctrTitle")) && s.xfrm.is_some())
            .and_then(|s| s.xfrm)
            .or_else(|| ph_map.get(&ph_key(Some("title"), None)).copied())
            .or_else(|| ph_map.get(&ph_key(Some("ctrTitle"), None)).copied());
        let sz = match src.xml("ppt/presentation.xml")? {
            Some(d) => d.root().first_child("p:sldSz").map(|s| (s.attr("cx").map(str::to_string), s.attr("cy").map(str::to_string))),
            None => return Err(ReadError::Missing("ppt/presentation.xml".into())),
        };
        let (cx, cy) = sz.unwrap_or((None, None));
        let w = pint(cx.as_deref().unwrap_or("9144000"));
        let h = pint(cy.as_deref().unwrap_or("5143500"));
        if let Some(t) = t
            && w != 0.0
            && !w.is_nan()
            && h != 0.0
            && !h.is_nan()
            && let Some(b) = slide.bg.as_mut()
        {
            b.fill_to = Some(Inset { l: t.x / w, t: t.y / h, r: 1.0 - (t.x + t.cx) / w, b: 1.0 - (t.y + t.cy) / h });
        }
    }
    // shapes with useBgFill (also those inherited from master/layout) get the resolved background
    for sh in &mut shapes {
        if sh.paint().use_bg_fill != Some(true) {
            continue;
        }
        if let Some(b) = &slide.bg {
            sh.fill = Some(Box::new(b.clone()));
        } else if let Some(img) = &slide.bg_image {
            sh.paint_mut().fill_image = Some(Box::new(img.clone()));
            if slide.bg_tile == Some(true) {
                sh.paint_mut().fill_tile = Some(true);
            }
        } else {
            sh.fill = Some(Box::new(solid(CssColor::new("#FFFFFF"))));
        }
    }
    // animations (<p:timing> "cascade fade" of the showcase)
    if let Some(timing) = root.first_child("p:timing") {
        let anims = parse_timing(timing);
        if !anims.is_empty() {
            slide.animations = Some(anims);
        }
    }
    // transition between slides (<p:transition>)
    if let Some(t) = root.first_child("p:transition") {
        slide.transition = Some(parse_transition(t));
    }
    // full time model (automatic playback): the entrance transition in `mc:AlternateContent` + the whole
    // `<p:timing>` tree (B6)
    {
        let units = |t: Option<&AnimTarget>, k: &str| count_text_units(&shapes, t, k);
        let color = |el: Element<'_>| uo_dml_render::theme::color_from_dml(Some(el), &theme);
        let has_bg = |spid: &str| has_background(&shapes, spid);
        let inp = crate::timing_parse::TimingInputs { units: &units, color: Some(&color), has_background: Some(&has_bg) };
        let mut tm = crate::timing_parse::parse_slide_timing(root, &inp, time_limits);
        // media bookmarks: the position in the file comes from the shape (p14:bmkLst)
        for bt in tm.bookmark_triggers.iter_mut().flatten() {
            let media = shapes
                .iter()
                .find(|s| s.inherited != Some(true) && *s.id == *bt.spid)
                .and_then(|s| s.extras.as_deref())
                .and_then(|x| x.media.as_ref());
            if let Some(b) = media.and_then(|m| m.bookmarks.as_deref()).and_then(|bs| bs.iter().find(|x| *x.name == *bt.bookmark)) {
                bt.time_ms = Some(b.time_ms);
            }
            if let Some((st, _)) = media.and_then(|m| m.trim)
                && uo_jsnum::conv::truthy(st)
            {
                bt.trim_start_ms = Some(st);
            }
        }
        tm.shrink_to_fit();
        slide.timing = Some(Arc::new(tm));
    }
    slide.shapes = shapes;
    slide.theme = Some(src.share_theme(theme));
    slide.shrink_to_fit();
    Ok(slide)
}

fn solid(c: CssColor) -> Fill {
    let mut f = Fill::of_kind(FillKind::Solid);
    f.color = Some(c);
    f
}

#[derive(Default)]
struct BgState {
    bg: Option<Fill>,
    image: Option<ImageRef>,
    tile: bool,
    shade_to_title: bool,
}

/// `applyBg` (`read.ts:336`): the first `<p:bg>` under `part_root`.
fn apply_bg(src: &dyn PartSource, theme: &Theme, st: &mut BgState, part_root: Element<'_>, rels: &RelMap) -> R<bool> {
    let Some(bg_el) = part_root.descend("p:bg") else { return Ok(false) };
    let Some(bg_pr) = bg_el.first_child("p:bgPr") else {
        // <p:bgRef idx="1001…"><a:schemeClr val="bg2"/>: bgFillStyleLst style of the theme
        // (solid/gradient/image) with phClr = the reference's colour
        let sf = match bg_el.first_child("p:bgRef") {
            Some(r) => theme_style_fill(r, theme, Some(src))?,
            None => None,
        };
        let Some(StyleFill { fill, image, tile }) = sf else { return Ok(false) };
        if let Some(f) = fill.filter(|f| f.kind != FillKind::None) {
            st.bg = Some(f);
        }
        if let Some(img) = image {
            st.image = Some(img);
            if tile {
                st.tile = true;
            }
        }
        return Ok(st.bg.is_some() || st.image.is_some());
    };
    if let Some(c) = solid_fill_color(bg_pr, theme) {
        st.bg = Some(solid(c));
    }
    if matches!(bg_pr.attr("shadeToTitle"), Some("1" | "true")) {
        st.shade_to_title = true;
    }
    if st.bg.is_none()
        && let Some(pf) = bg_pr.first_child("a:pattFill")
    {
        st.bg = Some(parse_patt_fill(pf, theme));
    }
    if st.bg.is_none()
        && let Some(grad) = bg_pr.first_child("a:gradFill")
        && let Some(g) = parse_grad_fill(grad, theme)
    {
        st.bg = Some(g);
    }
    if let Some(blip) = bg_pr.descend("a:blip")
        && let Some(mut img) = resolve_blip(blip, src, rels, Some(theme))
    {
        if apply_tile(&mut img, bg_pr.first_child("a:blipFill")) {
            st.tile = true;
        }
        st.image = Some(img);
    }
    Ok(st.bg.is_some() || st.image.is_some())
}

/// `placeholderXfrmMap` (`read.ts:545`): xfrm of the layout's placeholders (and the master's as fallback).
fn placeholder_xfrm_map(
    rd: &Rd<'_>,
    theme: &Theme,
    memo: &mut ShapeMemo,
    layout_path: Option<&str>,
    master_path: Option<&str>,
) -> R<PhMap<Xfrm>> {
    let mut map: PhMap<Xfrm> = PhMap::default();
    // master first (lower priority), layout after (overwrites)
    for part in [master_path, layout_path] {
        let Some(part) = part else { continue };
        // master/layout need no rels: no real `<p:pic>` here (only placeholder geometry), so an empty map
        for sh in memo.get(rd, theme, part, true)? {
            if !has_ph(sh) {
                continue;
            }
            let Some(x) = sh.xfrm else { continue };
            map.set(&ph_key(sh.ph_type.as_deref(), sh.ph_idx.as_deref()), x);
            map.set(&ph_key(sh.ph_type.as_deref(), None), x);
            // idx without a type only in the LAYOUT (the master's idx is another numbering)
            if let (Some(idx), true) = (sh.ph_idx.as_deref(), Some(part) == layout_path) {
                map.set(&format!("idx:{idx}"), x);
            }
        }
    }
    Ok(map)
}

/// `placeholderBodyPrMap` (`read.ts:614`): phKey → bodyPr properties of the layout/master placeholders
/// (anchor, insets, autofit, wrap, columns). The slide inherits what it does not define.
fn placeholder_body_pr_map(src: &dyn PartSource, layout_path: Option<&str>, master_path: Option<&str>) -> R<PhMap<BodyPrProps>> {
    let mut map: PhMap<BodyPrProps> = PhMap::default();
    let mut master_by_type: PhMap<BodyPrProps> = PhMap::default();
    for part in [master_path, layout_path] {
        let Some(part) = part else { continue };
        let Some(doc) = src.xml(part)? else { continue };
        let Some(tree) = doc.root().descend("p:spTree") else { continue };
        for sp in tree.children_named("p:sp") {
            let nv = sp.first_child("p:nvSpPr");
            let ph = nv.and_then(|n| n.first_child("p:nvPr").unwrap_or(n).first_child("p:ph"));
            let Some(ph) = ph else { continue };
            let Some(body_pr) = sp.first_child("p:txBody").and_then(|t| t.first_child("a:bodyPr")) else { continue };
            let mut props = BodyPrProps {
                anchor: ta(body_pr, "anchor").map(Into::into),
                wrap: ta(body_pr, "wrap").map(Into::into),
                spc_first_last_para: body_pr.attr("spcFirstLastPara").map(|v| v == "1"),
                vert: ta(body_pr, "vert").map(Into::into), // vertical text of the layout ("Vertical Title and Text")
                ins_l: int_attr(body_pr, "lIns"),
                ins_t: int_attr(body_pr, "tIns"),
                ins_r: int_attr(body_pr, "rIns"),
                ins_b: int_attr(body_pr, "bIns"),
                num_col: int_attr(body_pr, "numCol"),
                spc_col: int_attr(body_pr, "spcCol"),
                ..BodyPrProps::default()
            };
            let na = body_pr.first_child("a:normAutofit");
            if na.is_some() {
                props.autofit = Some(Autofit::Norm);
            } else if body_pr.first_child("a:noAutofit").is_some() {
                props.autofit = Some(Autofit::None);
            } else if body_pr.first_child("a:spAutoFit").is_some() {
                props.autofit = Some(Autofit::Shape);
            }
            if let Some(na) = na {
                props.font_scale = int_attr(na, "fontScale").map(|v| v / 100000.0);
                props.ln_spc_reduction = int_attr(na, "lnSpcReduction").map(|v| v / 100000.0);
            }
            let ty = ph.attr("type").unwrap_or("body");
            let idx = ph.attr("idx");
            if Some(part) == master_path && Some(part) != layout_path {
                // master: by TYPE (the layout's placeholder inherits the master's of the same type)
                master_by_type.set(ty, props.clone());
                map.set(&ph_key(Some(ty), None), props);
                continue;
            }
            // layout: the slide's placeholder matches the layout's by idx; the layout's MERGES over the
            // master's of the corresponding type (even with an empty <a:bodyPr/> — before, it fell to the
            // type and inherited the anchor of ANOTHER layout placeholder)
            let merged = match master_by_type.get(master_type_of(ty)) {
                Some(m) => m.merged(&props),
                None => props,
            };
            map.set(&ph_key(Some(ty), idx), merged.clone());
            match idx {
                Some(i) => map.set(&format!("idx:{i}"), merged),
                None => map.set(&ph_key(Some(ty), None), merged),
            }
        }
    }
    Ok(map)
}

/// `buildTextStyleContext` (`read.ts:676`): the cascade context of a slide: `<p:defaultTextStyle>` of
/// presentation.xml, the master's `<p:txStyles>` and the `<a:lstStyle>` of the master's and layout's
/// placeholders (by phKey).
fn build_text_style_context(
    rd: &Rd<'_>,
    theme: &Theme,
    memo: &mut ShapeMemo,
    layout_path: Option<&str>,
    master_path: Option<&str>,
) -> R<TextStyleContext> {
    let src = rd.src;
    let mut ctx = TextStyleContext::default();
    if let Some(pres) = src.xml("ppt/presentation.xml")?
        && let Some(dts) = pres.root().first_child("p:defaultTextStyle")
    {
        ctx.default_style = parse_tx_style_levels(Some(dts), theme, None);
    }
    if let Some(mp) = master_path
        && let Some(doc) = src.xml(mp)?
    {
        let root = doc.root();
        let tx = root.first_child("p:txStyles");
        let m_rels = read_rels(src, &rels_path_for(mp), mp)?;
        let m_blip = |b: Element<'_>| resolve_blip(b, src, &m_rels, Some(theme));
        if let Some(tx) = tx {
            ctx.title_style = parse_tx_style_levels(tx.first_child("p:titleStyle"), theme, Some(&m_blip));
            ctx.body_style = parse_tx_style_levels(tx.first_child("p:bodyStyle"), theme, Some(&m_blip));
            ctx.other_style = parse_tx_style_levels(tx.first_child("p:otherStyle"), theme, Some(&m_blip));
        }
        collect_ph_styles(rd, theme, memo, mp, &mut ctx.master_ph)?;
    }
    if let Some(lp) = layout_path
        && src.xml(lp)?.is_some()
    {
        collect_ph_styles(rd, theme, memo, lp, &mut ctx.layout_ph)?;
    }
    Ok(ctx)
}

/// `collectPhStyles` (`read.ts:716`): the lstStyle (already parsed in readTxBody) of a part's placeholders.
fn collect_ph_styles(rd: &Rd<'_>, theme: &Theme, memo: &mut ShapeMemo, part: &str, map: &mut PhMap<Arc<StyleLevels>>) -> R<()> {
    for sh in memo.get(rd, theme, part, false)? {
        if !has_ph(sh) {
            continue;
        }
        let Some(ls) = sh.tx_body.as_deref().and_then(|t| t.lst_style.as_deref()) else { continue };
        let ls: Arc<StyleLevels> = Arc::new(ls.clone());
        map.set(&ph_key(sh.ph_type.as_deref(), sh.ph_idx.as_deref()), Arc::clone(&ls));
        if !map.has(&ph_key(sh.ph_type.as_deref(), None)) {
            map.set(&ph_key(sh.ph_type.as_deref(), None), Arc::clone(&ls));
        }
        if let Some(idx) = sh.ph_idx.as_deref()
            && !map.has(&format!("idx:{idx}"))
        {
            map.set(&format!("idx:{idx}"), ls);
        }
    }
    Ok(())
}

/// `parseTransition` (`read.ts:466`): `<p:transition spd=med><p:fade/></p:transition>` etc.: `spd`
/// → canonical duration; the type is the first known child.
fn parse_transition(el: Element<'_>) -> SlideTransition {
    let spd = el.attr("spd").unwrap_or("med");
    let dur_ms = match spd {
        "fast" => 300.0,
        "slow" => 1000.0,
        _ => 500.0,
    };
    for c in el.child_elements() {
        let (kind, has_dir) = match c.name() {
            "p:fade" => ("fade", false),
            "p:push" => ("push", true),
            "p:cover" => ("cover", true),
            "p:wipe" => ("wipe", true),
            "p:dissolve" => ("dissolve", false),
            "p:blinds" => ("blinds", true),
            "p:split" => ("split", true),
            "p:circle" => ("circle", false),
            _ => continue,
        };
        return SlideTransition { kind: kind.into(), dur_ms, dir: if has_dir { c.attr("dir").map(Into::into) } else { None } };
    }
    SlideTransition { kind: "fade".into(), dur_ms, dir: None }
}

/// `readCondDelay` (`read.ts:520`): the first `<p:cond delay>` of `<p:stCondLst>` (ms).
fn read_cond_delay(c_tn: Element<'_>) -> f64 {
    let d = c_tn.first_child("p:stCondLst").and_then(|s| s.first_child("p:cond")).and_then(|c| ta(c, "delay"));
    match d {
        Some(d) if d != "indefinite" => {
            let v = pint(d);
            if v.is_nan() { 0.0 } else { v }
        }
        _ => 0.0,
    }
}

/// `parseTiming` (`read.ts:475`): the sequence of entrance effects of the mainSeq (legacy list). Each
/// inner `par` of the mainSeq is a `SlideAnimEntry`; the delay is the SUM of the `stCondLst@delay` of the
/// ancestors' `cTn`.
fn parse_timing(timing: Element<'_>) -> Vec<SlideAnimEntry> {
    let mut out = Vec::new();
    // finds the seq cTn (mainSeq) going down
    let main_seq = timing.descendants().find(|e| e.name() == "p:cTn" && e.attr("nodeType") == Some("mainSeq"));
    let Some(main_seq) = main_seq else { return out };
    let Some(child_list) = main_seq.first_child("p:childTnLst") else { return out };
    // each `par` child here is a "step" of the sequence (the delay accumulates)
    let mut acc = 0.0;
    for par in child_list.children_named("p:par") {
        let Some(c_tn) = par.first_child("p:cTn") else { continue };
        acc += read_cond_delay(c_tn);
        // goes down to the cTn presetClass=entr
        let Some(inner) = c_tn.first_child("p:childTnLst") else { continue };
        for ipar in inner.children_named("p:par") {
            let Some(icn) = ipar.first_child("p:cTn") else { continue };
            if icn.attr("presetClass") != Some("entr") {
                continue;
            }
            let local_delay = read_cond_delay(icn);
            let click_gated = icn.attr("nodeType") == Some("clickEffect");
            // looks for `animEffect filter=fade` (or only `set` for appear)
            let Some(grand) = icn.first_child("p:childTnLst") else { continue };
            let mut kind = LegacyAnimType::Appear;
            let mut dur_ms = 500.0;
            let mut spid: Option<&str> = None;
            for k in grand.child_elements() {
                if k.name() == "p:animEffect" {
                    if k.attr("filter").is_some_and(|f| f.contains("fade")) {
                        kind = LegacyAnimType::Fade;
                    }
                    let c_bhvr = k.first_child("p:cBhvr");
                    let inner_c_tn = c_bhvr.and_then(|c| c.first_child("p:cTn"));
                    if let Some(d) = inner_c_tn.and_then(|c| ta(c, "dur")) {
                        dur_ms = pint(d);
                    }
                    let sp =
                        c_bhvr.and_then(|c| c.first_child("p:tgtEl")).and_then(|t| t.first_child("p:spTgt")).and_then(|s| ta(s, "spid"));
                    if sp.is_some() {
                        spid = sp;
                    }
                } else if k.name() == "p:set" && spid.is_none_or(str::is_empty) {
                    // fallback for the spid when it only has `set` (appear effect)
                    let sp = k
                        .first_child("p:cBhvr")
                        .and_then(|c| c.first_child("p:tgtEl"))
                        .and_then(|t| t.first_child("p:spTgt"))
                        .and_then(|s| ta(s, "spid"));
                    if sp.is_some() {
                        spid = sp;
                    }
                }
            }
            let Some(spid) = spid.filter(|s| !s.is_empty()) else { continue };
            out.push(SlideAnimEntry { spid: spid.into(), delay_ms: acc + local_delay, dur_ms, kind, click_gated: Some(click_gated) });
        }
    }
    out
}

/// `hasBackground` (`read.ts:405`): visible background (fill, image or outline) of the shape with this
/// id — in the per-letter/per-word effects the application animates it as the 1st unit (measured with
/// the oracle). Input of `parseSlideTiming` (frontier B6).
pub fn has_background(shapes: &[Shape], spid: &str) -> bool {
    let Some(sh) = shapes.iter().find(|s| s.inherited != Some(true) && &*s.id == spid) else { return false };
    sh.paint().fill_image.is_some()
        || sh.fill.as_ref().is_some_and(|f| f.kind != FillKind::None)
        || (sh.line_color.as_ref().is_some_and(|c| !c.as_str().is_empty()) && sh.line_none != Some(true))
}

/// `countTextUnits` (`read.ts:418`): words/letters of the target of a text animation (`<p:iterate>`);
/// `kind` is `el`, `wd` or `lt`. Input of `parseSlideTiming` (frontier B6).
pub fn count_text_units(shapes: &[Shape], target: Option<&AnimTarget>, kind: &str) -> f64 {
    let Some(target) = target else { return 1.0 };
    if kind == "el" {
        return 1.0;
    }
    let sh = shapes.iter().find(|s| s.inherited != Some(true) && *s.id == *target.spid);
    let mut paras: &[Paragraph] = sh.and_then(|s| s.tx_body.as_deref()).map_or(&[], |t| &t.paragraphs[..]);
    if let Some((a, b)) = target.p_range {
        paras = js_slice(paras, a, b + 1.0);
    }
    let mut text = paras.iter().map(|p| p.runs.iter().map(|r| &*r.text).collect::<String>()).collect::<Vec<_>>().join("\n");
    if let Some((a, b)) = target.char_range {
        text = uo_jsnum::text::utf16_slice(&text, js_to_i64(a), js_to_i64(b)).into_owned();
    }
    if kind == "wd" {
        let n = uo_jsnum::text::split_js_space(&text).filter(|s| !s.is_empty()).count();
        return uo_jsnum::math::max(1.0, n as f64);
    }
    // letters: the application does NOT count spaces/breaks as a unit (measured via COM)
    let n: usize = text.chars().filter(|c| !uo_jsnum::text::is_js_space(*c)).map(char::len_utf16).sum();
    uo_jsnum::math::max(1.0, n as f64)
}

/// `ToIntegerOrInfinity` of a slice bound, bounded to a range no string reaches.
fn js_to_i64(x: f64) -> i64 {
    if x.is_nan() {
        return 0;
    }
    let t = uo_jsnum::math::max(-9.0e15, uo_jsnum::math::min(9.0e15, uo_jsnum::math::trunc(x)));
    #[expect(clippy::cast_possible_truncation, reason = "|t| <= 9e15 < 2^53: exact")]
    let v = t as i64;
    v
}

/// `array.slice(a, b)` with JS argument conversion (negative counts from the end).
fn js_slice<T>(v: &[T], a: f64, b: f64) -> &[T] {
    let len = v.len() as f64;
    let rel = |x: f64| -> f64 {
        let t = if x.is_nan() { 0.0 } else { uo_jsnum::math::trunc(x) };
        if t < 0.0 { uo_jsnum::math::max(len + t, 0.0) } else { uo_jsnum::math::min(t, len) }
    };
    let (s, e) = (rel(a), rel(b));
    if s >= e {
        return &[];
    }
    match (usize_of(s), usize_of(e)) {
        (Some(s), Some(e)) => v.get(s..e).unwrap_or(&[]),
        _ => &[],
    }
}

/// A parsed document of the deck (for the callers that hold one).
pub type DocRef = Arc<Document>;
