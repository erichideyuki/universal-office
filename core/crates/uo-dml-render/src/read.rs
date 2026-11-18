//! The shape reader (`read.ts` shape half, lines 732–2320 minus the deck functions): `<p:spTree>`
//! in document order (z-order) to [`Shape`]s: `p:sp`, `p:pic`, `p:cxnSp`, `p:graphicFrame` (charts,
//! tables, SmartArt drawings, OLE previews) and `p:grpSp` (flattened with the `chOff/chExt`
//! transform applied). The same reader serves spreadsheet drawings (`xdr:` prefix).
//!
//! Behaviour source: `slides/packages/slides-core/src/read.ts` (frozen 0.9.2). Gaps owned by later
//! frontiers: SmartArt regeneration and the fallback layout (B5a) return what the TS returns when
//! nothing is regenerated; `diagramStyler` goes with `diagram-fallback.ts` (B5a).

use uo_jsnum::math;
use uo_opc::xml::Element;

use crate::color::CssColor;
use crate::model::*;
use crate::read_dml::*;
use crate::read_text::read_tx_body;
use crate::source::*;
use crate::table_style::*;
use crate::text_style::BlipResolver;
use crate::theme::{Theme, apply_clr_map, color_from_dml, read_theme, solid_fill_color};

/// `mc:AlternateContent` and groups both recurse: one counter for the nesting.
fn deeper(rd: &Rd<'_>, depth: u32) -> R<u32> {
    if depth >= rd.limits.max_group_depth { Err(ReadError::Limit("group depth")) } else { Ok(depth + 1) }
}

/// `readShapes` (`read.ts:733`): walks the tree in DOCUMENT ORDER (z-order) and emits the shapes.
pub fn read_shapes(rd: &Rd<'_>, sp_tree: Element<'_>, theme: &Theme, rels: &RelMap) -> R<Vec<Shape>> {
    read_shapes_at(rd, sp_tree, theme, rels, 0)
}

fn read_shapes_at(rd: &Rd<'_>, sp_tree: Element<'_>, theme: &Theme, rels: &RelMap, depth: u32) -> R<Vec<Shape>> {
    let mut out: Vec<Shape> = Vec::new();
    for c in sp_tree.child_elements() {
        match c.name() {
            "p:sp" | "p:cxnSp" => out.push(read_shape(rd, c, theme, Some(rels))?),
            "p:pic" => out.push(read_pic(rd, c, theme, rels)),
            "p:graphicFrame" => out.extend(read_graphic_frame(rd, c, rels, theme)?),
            "p:grpSp" => out.extend(read_group(rd, c, theme, rels, deeper(rd, depth)?)?),
            "mc:AlternateContent" => {
                // alternate content (3D model, ink, slide zoom…): the Choice needs extensions we do
                // not draw — the Fallback is the image the application made
                if let Some(alt) = c.first_child("mc:Fallback").or_else(|| c.first_child("mc:Choice")) {
                    out.extend(read_shapes_at(rd, alt, theme, rels, deeper(rd, depth)?)?);
                }
            }
            _ => {}
        }
        if out.len() > usize::try_from(rd.limits.max_shapes).unwrap_or(usize::MAX) {
            return Err(ReadError::Limit("shapes per slide"));
        }
    }
    Ok(out)
}

/// Shapes of a container that is NOT PresentationML — a sheet drawing (`xl/drawings/drawingN.xml`:
/// `xdr:sp/pic/cxnSp/grpSp/graphicFrame`): the SAME reader as the slide's shapes, the container's
/// prefix renamed to `p:`. Each element becomes the list of shapes it draws (group flattened), in
/// the coordinates of its own `a:xfrm`; the caller positions it (anchor). Hidden shapes
/// (`cNvPr hidden="1"`) are left out, groups included. Pictures/charts resolve through the rels of
/// `owner_part`. An element that fails to read gives an empty list (the TS `try/catch`).
pub fn read_drawing_elements(
    src: &dyn PartSource,
    limits: Limits,
    owner_part: &str,
    elements: &[ElementRef],
    theme: &Theme,
    prefix: ShapePrefix,
) -> R<Vec<Vec<Shape>>> {
    let rd = Rd { src, limits };
    let rels = rels_of(src, owner_part)?;
    let from = match prefix {
        ShapePrefix::P => "p:",
        ShapePrefix::Xdr => "xdr:",
    };
    let mut all = Vec::with_capacity(elements.len());
    for er in elements {
        let Some(el) = er.doc.element(er.element) else {
            all.push(Vec::new());
            continue;
        };
        let text = rename_prefix_text(el, from, "p:", true, Some("p:spTree"));
        let shapes = match parse_xml(text) {
            Ok(doc) => read_shapes(&rd, doc.root(), theme, &rels).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        all.push(shapes);
    }
    Ok(all)
}

/// `renamePrefix` (`read.ts:1161`) as XML text: the element subtree with `from…` element names renamed
/// to `to…`; with `prune`, hidden shapes (`pruneHidden`) are left out. `wrap` surrounds it with an
/// element (the TS builds `{ name: 'p:spTree', attrs: {}, children: [el] }`).
pub fn rename_prefix_text(el: Element<'_>, from: &str, to: &str, prune: bool, wrap: Option<&str>) -> String {
    use uo_opc::xml::Node;
    use uo_opc::xml::write::XmlWriter;
    enum Step<'d> {
        Open(Element<'d>),
        Close,
        Text(&'d str),
    }
    let mut w = XmlWriter::new();
    if let Some(wr) = wrap {
        w.start(wr);
    }
    let hidden = |e: Element<'_>| -> bool {
        let nv = e.child_elements().find(|k| {
            let n = k.name();
            n.strip_prefix("p:nv")
                .is_some_and(|r| r.strip_suffix("Pr").is_some_and(|m| m.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')))
        });
        let h = nv.and_then(|n| n.first_child("p:cNvPr")).and_then(|c| c.attr("hidden"));
        matches!(h, Some("1" | "true"))
    };
    let renamed = |e: Element<'_>| -> String {
        let n = e.name();
        match n.strip_prefix(from) {
            Some(rest) => format!("{to}{rest}"),
            None => n.to_string(),
        }
    };
    // the element is pruned in the TS only as a child of the wrapper tree or a pruned container
    let is_hidden_after_rename = |e: Element<'_>| -> bool {
        // pruneHidden tests the children AFTER the rename: nv*Pr / cNvPr are `p:` names there
        let nv = e.child_elements().find(|k| {
            let kn = match k.name().strip_prefix(from) {
                Some(rest) => format!("{to}{rest}"),
                None => k.name().to_string(),
            };
            kn.strip_prefix("p:nv")
                .is_some_and(|r| r.strip_suffix("Pr").is_some_and(|m| m.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')))
        });
        let h = nv.and_then(|n| n.child_elements().find(|c| renamed(*c) == "p:cNvPr")).and_then(|c| c.attr("hidden"));
        matches!(h, Some("1" | "true"))
    };
    let _ = hidden;
    let container =
        |e: Element<'_>| -> bool { matches!(renamed(e).as_str(), "p:grpSp" | "mc:AlternateContent" | "mc:Choice" | "mc:Fallback") };
    let mut stack: Vec<Step<'_>> = Vec::new();
    // the top element is a child of the wrapper: pruned too
    if !(prune && is_hidden_after_rename(el)) {
        stack.push(Step::Open(el));
    }
    while let Some(step) = stack.pop() {
        match step {
            Step::Close => {
                w.end();
            }
            Step::Text(t) => {
                w.text(t);
            }
            Step::Open(e) => {
                w.start(&renamed(e));
                for a in e.attrs() {
                    w.attr(a.name, a.value);
                }
                stack.push(Step::Close);
                // children in reverse so they pop in order
                let prune_kids = prune && container(e);
                let mut kids: Vec<Step<'_>> = Vec::new();
                for n in e.children() {
                    match n {
                        Node::Element(k) => {
                            if prune_kids && is_hidden_after_rename(k) {
                                continue;
                            }
                            kids.push(Step::Open(k));
                        }
                        Node::Text(t) | Node::CData(t) => kids.push(Step::Text(t)),
                        _ => {}
                    }
                }
                stack.extend(kids.into_iter().rev());
            }
        }
    }
    w.finish()
}

fn base_shape(c_nv_pr: Option<Element<'_>>) -> Shape {
    Shape {
        id: c_nv_pr.and_then(|c| c.attr("id")).unwrap_or("").into(),
        name: c_nv_pr.and_then(|c| c.attr("name")).unwrap_or("").into(),
        extras: c_nv_pr.and_then(|c| ta(c, "descr")).map(|d| Box::new(ShapeExtras { descr: Some(d.into()), ..ShapeExtras::default() })),
        ..Shape::default()
    }
}

/// `readGroup` (`read.ts:861`): `<p:grpSp>` FLATTENING the children's coordinates into the parent's
/// space. The transform comes from `<p:grpSpPr><a:xfrm>`:
/// `x' = grpX + (x − chOffX) · (grpCx / chExtX)` (same for y / width / height). Nested groups resolve
/// by recursion. The group's rotation propagates (adds `rot` to the children and turns their centre
/// around the group's); flipH/flipV mirror the position inside the rect and toggle the child's flip.
fn read_group(rd: &Rd<'_>, grp: Element<'_>, theme: &Theme, rels: &RelMap, depth: u32) -> R<Vec<Shape>> {
    let mut children = read_shapes_at(rd, grp, theme, rels, depth)?;
    // the group's id: target of animations applied to the whole group
    let gid = grp.first_child("p:nvGrpSpPr").and_then(|n| n.first_child("p:cNvPr")).and_then(|c| c.attr("id"));
    if let Some(gid) = gid {
        for sh in &mut children {
            let ex = sh.extras_mut();
            let mut ids: Vec<Box<str>> = vec![gid.into()];
            if let Some(old) = ex.group_ids.take() {
                ids.extend(old.into_vec());
            }
            ex.group_ids = Some(ids.into_boxed_slice());
        }
    }
    let grp_sp_pr = grp.first_child("p:grpSpPr");
    // the GROUP's fill: children with <a:grpFill/> inherit (nested ones without their own fill
    // go up to the group that has one)
    let g_fill = grp_sp_pr.and_then(|g| group_fill(g, theme));
    if let Some((fill, alpha)) = &g_fill {
        for sh in &mut children {
            if sh.grp_fill != Some(true) {
                continue;
            }
            sh.fill = Some(Box::new(fill.clone()));
            if alpha.is_some() {
                sh.paint_mut().fill_alpha = *alpha;
            }
            sh.grp_fill = None;
        }
    }
    let Some(xfrm_el) = grp_sp_pr.and_then(|g| g.first_child("a:xfrm")) else { return Ok(children) };
    let off = xfrm_el.first_child("a:off");
    let ext = xfrm_el.first_child("a:ext");
    let ch_off = xfrm_el.first_child("a:chOff");
    let ch_ext = xfrm_el.first_child("a:chExt");
    let (gx, gy) = (int_or(off, "x", 0.0), int_or(off, "y", 0.0));
    let (gcx, gcy) = (int_or(ext, "cx", 0.0), int_or(ext, "cy", 0.0));
    // chOff/chExt absent = child space identical to the group's rect (identity)
    let (cox, coy) = (int_or(ch_off, "x", gx), int_or(ch_off, "y", gy));
    let (cex, cey) = (int_or(ch_ext, "cx", gcx), int_or(ch_ext, "cy", gcy));
    let sx = if cex != 0.0 { gcx / cex } else { 1.0 };
    let sy = if cey != 0.0 { gcy / cey } else { 1.0 };
    let rot_raw = ta(xfrm_el, "rot").map_or(0.0, pint);
    let flip_h = xfrm_el.attr("flipH") == Some("1");
    let flip_v = xfrm_el.attr("flipV") == Some("1");
    let (g_center_x, g_center_y) = (gx + gcx / 2.0, gy + gcy / 2.0);
    for sh in &mut children {
        let Some(x) = sh.xfrm.as_mut() else { continue };
        x.x = gx + (x.x - cox) * sx;
        x.y = gy + (x.y - coy) * sy;
        x.cx *= sx;
        x.cy *= sy;
        // F·R(θ) = R(−θ)·F: mirroring the group negates the child's local rotation
        // (COM oracle: rect at 30° in a group with flipH → rot = 330° + flipH)
        if flip_h {
            x.x = 2.0 * g_center_x - (x.x + x.cx);
            x.flip_h = if x.flip_h == Some(true) { None } else { Some(true) };
            if let Some(r) = x.rot.filter(|r| *r != 0.0 && !r.is_nan()) {
                x.rot = Some(-r);
            }
        }
        if flip_v {
            x.y = 2.0 * g_center_y - (x.y + x.cy);
            x.flip_v = if x.flip_v == Some(true) { None } else { Some(true) };
            if let Some(r) = x.rot.filter(|r| *r != 0.0 && !r.is_nan()) {
                x.rot = Some(-r);
            }
        }
        if rot_raw != 0.0 && !rot_raw.is_nan() {
            // turn the child's CENTRE around the group's centre; rot accumulates
            let a = (rot_raw / 60000.0) * std::f64::consts::PI / 180.0;
            let dx = (x.x + x.cx / 2.0) - g_center_x;
            let dy = (x.y + x.cy / 2.0) - g_center_y;
            let ncx = g_center_x + dx * math::cos(a) - dy * math::sin(a);
            let ncy = g_center_y + dx * math::sin(a) + dy * math::cos(a);
            x.x = ncx - x.cx / 2.0;
            x.y = ncy - x.cy / 2.0;
            x.rot = nonzero(x.rot.unwrap_or(0.0) + rot_raw);
        }
        // line width does NOT scale with the group (the application draws the absolute width)
    }
    Ok(children)
}

/// The fill (or image) of a theme style (`themeStyleFill`'s result).
#[derive(Default)]
pub struct StyleFill {
    pub fill: Option<Fill>,
    pub image: Option<ImageRef>,
    pub tile: bool,
}

/// `themeStyleFill` (`read.ts:1650`): the theme fill style referenced by `<a:fillRef idx>` (1…) or
/// `<p:bgRef idx>` (1001…), with `phClr` = the reference's child colour. idx 0 = nothing. Images of
/// the style resolve through the theme's own rels.
pub fn theme_style_fill(ref_el: Element<'_>, theme: &Theme, src: Option<&dyn PartSource>) -> R<Option<StyleFill>> {
    let idx = pint(ref_el.attr("idx").unwrap_or("0"));
    if idx.is_nan() || idx <= 0.0 {
        return Ok(None);
    }
    let Some(styles) = theme.styles.as_ref() else { return Ok(None) };
    let el = if idx >= 1001.0 {
        usize_of(idx - 1001.0).and_then(|i| styles.bg_fill_style(i))
    } else {
        usize_of(idx - 1.0).and_then(|i| styles.fill_style(i))
    };
    let Some(el) = el else { return Ok(None) };
    let ph = color_from_dml(ref_el.child_elements().next(), theme);
    let owned;
    let t: &Theme = match &ph {
        Some(p) => {
            let mut c = theme.clone();
            c.colors.set("phClr", p.clone());
            owned = c;
            &owned
        }
        None => theme,
    };
    let ph_solid = |ph: &Option<CssColor>| ph.clone().map(|c| StyleFill { fill: Some(solid_fill(c)), ..StyleFill::default() });
    Ok(match el.name() {
        "a:noFill" => Some(StyleFill { fill: Some(Fill::of_kind(FillKind::None)), ..StyleFill::default() }),
        "a:solidFill" => {
            color_from_dml(el.child_elements().next(), t).map(|c| StyleFill { fill: Some(solid_fill(c)), ..StyleFill::default() })
        }
        "a:gradFill" => parse_grad_fill(el, t).map(|g| StyleFill { fill: Some(g), ..StyleFill::default() }),
        "a:pattFill" => Some(StyleFill { fill: Some(parse_patt_fill(el, t)), ..StyleFill::default() }),
        "a:blipFill" => {
            let blip = el.first_child("a:blip");
            let (Some(blip), Some(src), Some(part)) = (blip, src, theme.part.as_deref()) else { return Ok(ph_solid(&ph)) };
            let rels = rels_of(src, part)?;
            match resolve_blip(blip, src, &rels, Some(t)) {
                None => ph_solid(&ph),
                Some(mut img) => {
                    let tile = apply_tile(&mut img, Some(el));
                    Some(StyleFill { fill: None, image: Some(img), tile })
                }
            }
        }
        _ => None,
    })
}

/// `cellPaintOf` (`read.ts:1056`): the fill of a table / cell / style part: `el` is the container with
/// EG_FillProperties (`<a:tcPr>`, `<a:tblPr>`, `<a:fill>`) or an `<a:fillRef>` (a theme style with phClr).
fn cell_paint_of(el: Element<'_>, theme: &Theme, src: &dyn PartSource, rels: &RelMap) -> R<PaintOutcome> {
    let paint = |fill: Option<Fill>, alpha: Option<f64>, image: Option<ImageRef>, tile: bool| {
        PaintOutcome::Paint(CellPaint { fill, alpha, image: image.map(Box::new), tile: tile.then_some(true) })
    };
    if el.name() == "a:fillRef" {
        if let Some(st) = theme_style_fill(el, theme, Some(src))? {
            if st.image.is_some() {
                return Ok(paint(None, None, st.image, st.tile));
            }
            if let Some(f) = st.fill {
                return Ok(if f.kind == FillKind::None { PaintOutcome::Null } else { paint(Some(f), None, None, false) });
            }
        }
        return Ok(match color_from_dml(el.child_elements().next(), theme) {
            Some(c) => paint(Some(solid_fill(c)), None, None, false),
            None => PaintOutcome::Undefined,
        });
    }
    if el.first_child("a:noFill").is_some() {
        return Ok(PaintOutcome::Null);
    }
    if let Some(sf) = el.first_child("a:solidFill") {
        let Some(color) = color_from_dml(sf.child_elements().next(), theme) else { return Ok(PaintOutcome::Undefined) };
        let a = fill_alpha_val(sf).map(|a| pint(a) / 100000.0);
        return Ok(paint(Some(solid_fill(color)), a, None, false));
    }
    if let Some(gf) = el.first_child("a:gradFill") {
        return Ok(match parse_grad_fill(gf, theme) {
            Some(g) => paint(Some(g), None, None, false),
            None => PaintOutcome::Undefined,
        });
    }
    if let Some(pf) = el.first_child("a:pattFill") {
        return Ok(paint(Some(parse_patt_fill(pf, theme)), None, None, false));
    }
    let bf = el.first_child("a:blipFill");
    if let Some(blip) = bf.and_then(|b| b.first_child("a:blip"))
        && let Some(mut img) = resolve_blip(blip, src, rels, Some(theme))
    {
        let tile = apply_tile(&mut img, bf);
        return Ok(paint(None, None, Some(img), tile));
    }
    Ok(PaintOutcome::Undefined)
}

/// `readGraphicFrame` (`read.ts:926`): the wrapper of charts/tables/diagrams. Returns the shapes it
/// draws (one, or the SmartArt's).
fn read_graphic_frame(rd: &Rd<'_>, gf: Element<'_>, rels: &RelMap, theme: &Theme) -> R<Vec<Shape>> {
    let src = rd.src;
    let nv_pr = gf.first_child("p:nvGraphicFramePr");
    let c_nv_pr = nv_pr.and_then(|n| n.first_child("p:cNvPr"));
    let mut shape = base_shape(c_nv_pr);
    if let Some(x) = gf.first_child("p:xfrm").or_else(|| gf.first_child("a:xfrm")) {
        shape.xfrm = Some(parse_xfrm(x));
    }
    let data = gf.first_child("a:graphic").and_then(|g| g.first_child("a:graphicData"));
    let Some(data) = data else { return Ok(vec![shape]) };
    // chart
    if let Some(chart_el) = data.first_child("c:chart") {
        let rid = chart_el.attr("r:id").or_else(|| chart_el.attr("id"));
        if let Some(rid) = rid.filter(|r| !r.is_empty())
            && let Some(target) = rels.get(rid).filter(|t| !t.is_empty())
            && let Some(xml) = src.text(target).filter(|x| !x.is_empty())
        {
            let images = chart_images_for(src, target, &xml)?;
            shape.extras_mut().chart = Some(Box::new(ChartRef {
                part: target.into(),
                theme: src.share_theme(chart_theme_for(src, target, theme)?),
                images,
                format: Some(chart_file_format(src)),
            }));
        }
    }
    // SmartArt: the modern pptx embeds a pre-rendered DRAWING of the diagram (ppt/diagrams/drawingN.xml,
    // prefix dsp:) — ordinary shapes we rescale to the frame's xfrm. r:dm resolves the data part;
    // the drawing comes from its rels.
    if let Some(rel_ids) = data.first_child("dgm:relIds")
        && let Some(frame) = shape.xfrm
    {
        let dm_id = rel_ids.attr("r:dm");
        let data_path = dm_id.and_then(|d| rels.get(d)).filter(|p| !p.is_empty());
        if let Some(data_path) = data_path {
            let data_rels = rels_of(src, data_path)?;
            let mut drawing_path: Option<String> = data_rels.values().find(|t| t.contains("drawing")).map(str::to_string);
            if drawing_path.is_none() {
                let guess = drawing_guess(data_path);
                if src.text(&guess).is_some() {
                    drawing_path = Some(guess);
                }
            }
            // without a cached drawing (or an empty one) the application redoes the layout from the definition
            let mut shapes = match &drawing_path {
                Some(dp) => read_diagram_shapes(rd, dp, theme, &frame)?,
                None => Vec::new(),
            };
            if shapes.is_empty() {
                // B5a: `regenerateDiagram` redoes the drawing from the layout definition
                let part = |id: Option<&str>| -> Option<crate::smartart::xt::X> {
                    let p = id.and_then(|i| rels.get(i))?;
                    let doc = src.xml(p).ok().flatten()?;
                    crate::smartart::xt::from_element(doc.root())
                };
                let tree = crate::smartart::regenerate_diagram(
                    part(dm_id).as_ref(),
                    part(rel_ids.attr("r:lo")).as_ref(),
                    part(rel_ids.attr("r:qs")).as_ref(),
                    part(rel_ids.attr("r:cs")).as_ref(),
                    frame.cx,
                    frame.cy,
                    theme,
                );
                if let Some(tree) = tree
                    && let Ok(doc) = parse_xml(crate::smartart::xt::to_xml(&tree))
                {
                    shapes = read_diagram_shapes_in(rd, data_path, theme, &frame, doc.root())?;
                }
            }
            if !shapes.is_empty() {
                // animations aim at the FRAME (the SmartArt's spid); by parts, `<a:dgm id>` names the
                // drawing's shape (dgmId)
                if !shape.id.is_empty() {
                    for s in &mut shapes {
                        push_group_id(s, &shape.id);
                    }
                }
                return Ok(shapes);
            }
            if drawing_path.is_none() {
                // no pre-computed drawing (Office 2007): approximate layout from the data model
                // (`diagramFallbackShapes`, B5a)
                let lo_xml = rel_ids.attr("r:lo").and_then(|i| rels.get(i)).and_then(|p| src.text(p));
                let uid = lo_xml.as_deref().and_then(unique_id_of).unwrap_or("");
                if let Some(data_text) = src.text(data_path) {
                    let part = |id: Option<&str>| -> Option<crate::smartart::xt::X> {
                        let p = id.and_then(|i| rels.get(i))?;
                        let doc = src.xml(p).ok().flatten()?;
                        crate::smartart::xt::from_element(doc.root())
                    };
                    let cs = part(rel_ids.attr("r:cs"));
                    let qs = part(rel_ids.attr("r:qs"));
                    let styler = crate::smartart::diagram_styler(cs.as_ref(), qs.as_ref(), theme);
                    // the TS wraps both in a try: any throw leaves no shapes
                    shapes = match parse_xml(data_text).ok().and_then(|d| crate::smartart::xt::from_element(d.root())) {
                        Some(dx) => crate::smartart::diagram_fallback_shapes(&dx, uid, &frame, theme, Some(&styler)).unwrap_or_default(),
                        None => Vec::new(),
                    };
                    if !shapes.is_empty() {
                        if !shape.id.is_empty() {
                            for s in &mut shapes {
                                push_group_id(s, &shape.id);
                            }
                        }
                        return Ok(shapes);
                    }
                }
            }
        }
    }
    // OLE object (spreadsheet, document, equation, presentation…): the application draws the stored
    // PREVIEW — the `<p:pic>` inside oleObj (2010+) or the `v:imagedata` of the shape with the same
    // spid in the slide's VML drawing (2007)
    let ole = data.first_child("p:oleObj").or_else(|| data.descend("p:oleObj"));
    if let Some(ole) = ole {
        // mc:Choice without preview (only <p:embed/>): the <p:pic> of mc:Fallback counts (lo-tdf143222)
        let pic = ole.descend("p:pic").or_else(|| data.descend("p:pic"));
        let img = match pic {
            Some(pic) => read_pic(rd, pic, theme, rels).paint_mut().image.take().map(|b| *b),
            None => ole_preview_from_vml(ole.attr("spid"), src, rels, theme, &mut shape)?,
        };
        if let Some(img) = img {
            shape.paint_mut().image = Some(Box::new(img));
        }
    }
    // table
    if let Some(tbl_el) = data.first_child("a:tbl") {
        let paint = |el: Element<'_>| cell_paint_of(el, theme, src, rels);
        let mut table = read_table(rd, tbl_el, theme, &paint, rels)?;
        // `<a:tblPr>` style (only the application's predefined ones; none = plain grid) + the TABLE's
        // fill and shadow (tblPr, or the style's tblBg)
        let (pr, style_id) = read_tbl_pr(tbl_el);
        let tbl_pr = tbl_el.first_child("a:tblPr");
        if let Some(t) = tbl_pr {
            match paint(t)? {
                PaintOutcome::Paint(bg) => table.bg_paint = Some(Box::new(bg)),
                PaintOutcome::Null | PaintOutcome::Undefined => {}
            }
        }
        let style = read_table_style(style_id.as_deref(), theme, &paint)?;
        apply_table_style(&mut table, style.as_ref(), &pr);
        let fx = read_effects(tbl_pr, style.as_ref().and_then(|s| s.effect_el()), theme);
        if let Some(sh) = fx.shadow {
            table.shadow = Some(sh);
        }
        shape.extras_mut().table = Some(Box::new(table));
    }
    Ok(vec![shape])
}

fn push_group_id(s: &mut Shape, id: &str) {
    let ex = s.extras_mut();
    let mut ids: Vec<Box<str>> = ex.group_ids.take().map(|b| b.into_vec()).unwrap_or_default();
    ids.push(id.into());
    ex.group_ids = Some(ids.into_boxed_slice());
}

/// `dataPath.replace(/data(\d+)\.xml$/, 'drawing$1.xml')`.
fn drawing_guess(data_path: &str) -> String {
    if let Some(stem) = data_path.strip_suffix(".xml") {
        let digits = stem.bytes().rev().take_while(u8::is_ascii_digit).count();
        if digits > 0
            && let Some(split) = stem.len().checked_sub(digits)
            && let (Some(rest), Some(nums)) = (stem.get(..split), stem.get(split..))
            && let Some(head) = rest.strip_suffix("data")
        {
            return format!("{head}drawing{nums}.xml");
        }
    }
    data_path.to_string()
}

/// `chartThemeFor` (`read.ts:1094`): the presentation's theme or the chart's own `themeOverride`
/// (colours/fonts from when it was pasted from another presentation/workbook), with the same clrMap.
fn chart_theme_for(src: &dyn PartSource, chart_path: &str, theme: &Theme) -> R<Theme> {
    let rels = rels_of(src, chart_path)?;
    let ov = rels.values().find(|t| is_theme_override(t));
    let Some(ov) = ov else { return Ok(theme.clone()) };
    let o = read_theme(src, ov)?;
    if o.scheme.as_ref().is_none_or(|s| s.is_empty()) {
        return Ok(theme.clone());
    }
    let mut merged = Theme { colors: o.colors, scheme: o.scheme, ..theme.clone() };
    if let Some(of) = o.fonts {
        let base = theme.fonts.clone().unwrap_or_default();
        merged.fonts = Some(crate::theme::ThemeFonts { major: of.major.or(base.major), minor: of.minor.or(base.minor) });
    }
    Ok(match &theme.clr_map {
        Some(m) => apply_clr_map(merged, Some(m)),
        None => merged,
    })
}

/// `/themeOverride\d*\.xml$/i`.
fn is_theme_override(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    let Some(stem) = l.strip_suffix(".xml") else { return false };
    let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    stem.ends_with("themeoverride")
}

/// `chartImagesFor` (`read.ts:1112`): the images the chart's XML references by `r:embed`, resolved
/// through the part's rels.
fn chart_images_for(src: &dyn PartSource, chart_path: &str, xml: &str) -> R<Option<Box<[ChartImage]>>> {
    if !xml.contains("embed=") {
        return Ok(None);
    }
    let rels = rels_of(src, chart_path)?;
    let mut out: Vec<ChartImage> = Vec::new();
    for (rid, part) in rels.iter() {
        let mime = mime_from_part(part);
        if !mime.starts_with("image/") || !xml.contains(&format!("embed=\"{rid}\"")) {
            continue;
        }
        if src.part(part).is_some()
            && let Some(media) = src.media(part)
        {
            out.push(ChartImage { r_id: rid.into(), image: ImageRef { media, mime: mime.into(), fx: None, tile: None, fill_rect: None } });
        }
    }
    Ok((!out.is_empty()).then(|| out.into_boxed_slice()))
}

/// `chartFileFormat` (`read.ts:1135`): a deck saved by Office 2007 (`docProps/app.xml`: Application
/// "Microsoft…" and AppVersion 12.x): PowerPoint reads its charts with the old meaning of absent
/// elements and boolean without val (measured, syn-a-mso07 × syn-a-mso16).
pub fn chart_file_format(src: &dyn PartSource) -> ChartFormat {
    let app = src.text("docProps/app.xml").unwrap_or_default();
    let tag = |t: &str| -> String {
        let open = format!("<{t}>");
        match app.find(&open) {
            Some(i) => {
                let rest = app.get(i + open.len()..).unwrap_or("");
                // `([^<]*)<`: the text must be closed by a `<`
                match rest.find('<') {
                    Some(e) => uo_jsnum::text::trim(rest.get(..e).unwrap_or("")).to_string(),
                    None => String::new(),
                }
            }
            None => String::new(),
        }
    };
    let name = tag("Application");
    let ver = tag("AppVersion");
    let microsoft = name.get(..9).is_some_and(|p| p.eq_ignore_ascii_case("Microsoft"));
    if microsoft && ver.starts_with("12.") { ChartFormat::Mso2007 } else { ChartFormat::Modern }
}

/// `olePreviewFromVml` (`read.ts:1000`): the preview of a PowerPoint 2007 OLE object (without
/// `<p:pic>`): the VML shape with id = spid in the slide's `vmlDrawing` holds `<v:imagedata
/// o:relid>` (EMF/WMF/PNG). Legacy VML is not always well-formed XML — it is searched as text.
fn ole_preview_from_vml(spid: Option<&str>, src: &dyn PartSource, rels: &RelMap, theme: &Theme, shape: &mut Shape) -> R<Option<ImageRef>> {
    let Some(spid) = spid.filter(|s| !s.is_empty()) else { return Ok(None) };
    let clean: String = spid.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    for target in rels.values() {
        if !is_vml_drawing(target) {
            continue;
        }
        let Some(vml) = src.text(target).filter(|v| !v.is_empty()) else { continue };
        let Some(at) = find_vml_shape(&vml, &clean) else { continue };
        let tail = vml.get(at..).unwrap_or("");
        let block = match tail.find("</v:shape>") {
            Some(e) => tail.get(..e).unwrap_or(tail),
            None => tail,
        };
        let Some(relid) = imagedata_relid(block) else { continue };
        // fill/outline of the VML frame (filled="t" fillcolor…, stroked="t" strokecolor…): the
        // application paints behind the preview (gdx-320163: equation with a yellow background and border)
        let tag = tag_of(block, "<v:shape").unwrap_or("");
        let attr = |n: &str| -> Option<String> { vml_attr(tag, n) };
        let fc = if attr("filled").as_deref() == Some("t") { vml_color(attr("fillcolor").as_deref()) } else { None };
        if let Some(fc) = fc
            && shape.fill.is_none()
        {
            shape.fill = Some(Box::new(solid_fill(CssColor::new(&fc))));
        }
        let sc =
            if attr("stroked").as_deref() == Some("t") { vml_color(Some(attr("strokecolor").as_deref().unwrap_or("black"))) } else { None };
        if let Some(sc) = sc
            && shape.line_color.is_none()
        {
            shape.line_color = Some(CssColor::new(&sc));
            let sw = attr("strokeweight").and_then(|w| w.strip_suffix("pt").map(str::to_string)).filter(|w| is_dec_num(w));
            shape.line_width = Some(math::round(sw.map_or(0.75, |w| uo_jsnum::parse::parse_float(&w)) * 12700.0));
        }
        let vrels = rels_of(src, target)?;
        let mut vr = RelMap::new();
        for (k, v) in vrels.iter() {
            vr.set(k, v);
        }
        // the synthetic blip: `{ name: 'a:blip', attrs: { 'r:embed': relid } }`
        if let Some(part) = vr.get(&relid)
            && src.part(part).is_some()
            && let Some(media) = src.media(part)
        {
            let mut r = ImageRef { media, mime: mime_from_part(part).into(), fx: None, tile: None, fill_rect: None };
            r.fx = None;
            let _ = theme;
            return Ok(Some(r));
        }
    }
    Ok(None)
}

/// `/vmlDrawing\d*\.vml$/i`.
fn is_vml_drawing(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    let Some(stem) = l.strip_suffix(".vml") else { return false };
    stem.trim_end_matches(|c: char| c.is_ascii_digit()).ends_with("vmldrawing")
}

/// `/^[\d.]+$/` (the JS `[\d.]+`).
fn is_dec_num(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The text of the start tag (`<v:shape …>`) beginning at the first `<v:shape` of `s`.
fn tag_of<'a>(s: &'a str, open: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = s.get(from..).and_then(|r| r.find(open)) {
        let at = from + i;
        let after = at + open.len();
        // `\b` after the name: a word character cannot follow
        if !s.as_bytes().get(after).is_some_and(|b| is_word(*b)) {
            let end = s.get(after..).and_then(|r| r.find('>')).map(|e| after + e + 1)?;
            return s.get(at..end);
        }
        from = at + 1;
    }
    None
}

/// `vml.search(/<v:shape\b[^>]*\b(?:id|o:spid)="SPID"/)`.
fn find_vml_shape(vml: &str, spid: &str) -> Option<usize> {
    let b = vml.as_bytes();
    let mut from = 0;
    let needles = [format!("id=\"{spid}\""), format!("o:spid=\"{spid}\"")];
    while let Some(i) = vml.get(from..).and_then(|r| r.find("<v:shape")) {
        let at = from + i;
        let after = at + "<v:shape".len();
        let word_next = b.get(after).is_some_and(|c| is_word(*c));
        if !word_next {
            let tag_end = vml.get(after..).and_then(|r| r.find('>')).map_or(vml.len(), |e| after + e);
            let region = vml.get(after..tag_end).unwrap_or("");
            for n in &needles {
                let mut f = 0;
                while let Some(j) = region.get(f..).and_then(|r| r.find(n.as_str())) {
                    let q = f + j;
                    // `\b` before id/o:spid: the previous character is not a word character
                    let prev = if q == 0 { b.get(after.wrapping_sub(1)).copied() } else { region.as_bytes().get(q - 1).copied() };
                    if !prev.is_some_and(is_word) {
                        return Some(at);
                    }
                    f = q + 1;
                }
            }
        }
        from = at + 1;
    }
    None
}

/// `/<v:imagedata\b[^>]*\b(?:o:relid|r:id)="([^"]+)"/.exec(block)?.[1]`.
fn imagedata_relid(block: &str) -> Option<String> {
    let b = block.as_bytes();
    let mut from = 0;
    while let Some(i) = block.get(from..).and_then(|r| r.find("<v:imagedata")) {
        let at = from + i;
        let after = at + "<v:imagedata".len();
        if !b.get(after).is_some_and(|c| is_word(*c)) {
            let tag_end = block.get(after..).and_then(|r| r.find('>')).map_or(block.len(), |e| after + e);
            let region = block.get(after..tag_end).unwrap_or("");
            // leftmost attribute match with a word boundary before
            let mut best: Option<(usize, String)> = None;
            for n in ["o:relid=\"", "r:id=\""] {
                let mut f = 0;
                while let Some(j) = region.get(f..).and_then(|r| r.find(n)) {
                    let q = f + j;
                    let prev = if q == 0 { b.get(after.wrapping_sub(1)).copied() } else { region.as_bytes().get(q - 1).copied() };
                    if !prev.is_some_and(is_word) {
                        let vs = q + n.len();
                        if let Some(e) = region.get(vs..).and_then(|r| r.find('"'))
                            && e > 0
                        {
                            let v = region.get(vs..vs + e).unwrap_or("").to_string();
                            if best.as_ref().is_none_or(|(p, _)| q < *p) {
                                best = Some((q, v));
                            }
                        }
                        break;
                    }
                    f = q + 1;
                }
            }
            if let Some((_, v)) = best {
                return Some(v);
            }
        }
        from = at + 1;
    }
    None
}

/// `new RegExp(`\\b${n}="([^"]*)"`).exec(tag)?.[1]`.
fn vml_attr(tag: &str, n: &str) -> Option<String> {
    let pat = format!("{n}=\"");
    let b = tag.as_bytes();
    let mut f = 0;
    while let Some(j) = tag.get(f..).and_then(|r| r.find(&pat)) {
        let q = f + j;
        let prev = if q == 0 { None } else { b.get(q - 1).copied() };
        if !prev.is_some_and(is_word) {
            let vs = q + pat.len();
            let e = tag.get(vs..).and_then(|r| r.find('"'))?;
            return Some(tag.get(vs..vs + e)?.to_string());
        }
        f = q + 1;
    }
    None
}

/// `vmlColor` (`read.ts:1033`): VML colour ("#e4e838", "#3cf", "black [3]", "white"…) → `#RRGGBB`.
fn vml_color(v: Option<&str>) -> Option<String> {
    let v = v.filter(|v| !v.is_empty())?;
    // `.replace(/\s*\[\d+\]\s*$/, '').trim().toLowerCase()`
    let mut s = v.trim_end_matches(uo_jsnum::text::is_js_space);
    if let Some(stripped) = s.strip_suffix(']') {
        let up_to = stripped.trim_end_matches(|c: char| c.is_ascii_digit());
        if up_to.len() < stripped.len()
            && let Some(before) = up_to.strip_suffix('[')
        {
            s = before.trim_end_matches(uo_jsnum::text::is_js_space);
        }
    }
    let c = uo_jsnum::text::to_lowercase(uo_jsnum::text::trim(s));
    let b = c.as_bytes();
    if b.len() == 7 && b.first() == Some(&b'#') && b.iter().skip(1).all(u8::is_ascii_hexdigit) {
        return Some(format!("#{}", c.get(1..)?.to_ascii_uppercase()));
    }
    if b.len() == 4 && b.first() == Some(&b'#') && b.iter().skip(1).all(u8::is_ascii_hexdigit) {
        let mut o = String::from("#");
        for ch in c.chars().skip(1) {
            o.push(ch);
            o.push(ch);
        }
        return Some(o.to_ascii_uppercase());
    }
    Some(
        match c.as_str() {
            "black" => "#000000",
            "white" => "#FFFFFF",
            "red" => "#FF0000",
            "green" => "#008000",
            "blue" => "#0000FF",
            "yellow" => "#FFFF00",
            "gray" => "#808080",
            "silver" => "#C0C0C0",
            "navy" => "#000080",
            "maroon" => "#800000",
            "olive" => "#808000",
            "purple" => "#800080",
            "teal" => "#008080",
            "aqua" => "#00FFFF",
            "fuchsia" => "#FF00FF",
            "lime" => "#00FF00",
            _ => return None,
        }
        .to_string(),
    )
}

/// `readDiagramShapes` (`read.ts:1169`): the pre-rendered drawing of a SmartArt (`<dsp:spTree>` with
/// ordinary shapes), RESCALED to fit the graphicFrame's xfrm.
fn read_diagram_shapes(rd: &Rd<'_>, drawing_path: &str, theme: &Theme, frame: &Xfrm) -> R<Vec<Shape>> {
    let src = rd.src;
    let Some(text) = src.text(drawing_path).filter(|t| !t.is_empty()) else { return Ok(Vec::new()) };
    let Ok(doc) = parse_xml(text) else { return Ok(Vec::new()) };
    read_diagram_shapes_in(rd, drawing_path, theme, frame, doc.root())
}

/// `readDiagramShapes` over a given root: the regenerated `dsp:spTree` (its relationships are the data
/// part's: `drawing_path` names the part whose rels the shapes' images resolve through).
fn read_diagram_shapes_in(rd: &Rd<'_>, drawing_path: &str, theme: &Theme, frame: &Xfrm, root: Element<'_>) -> R<Vec<Shape>> {
    let src = rd.src;
    let sp_tree = if root.name() == "dsp:spTree" { Some(root) } else { root.descend("dsp:spTree") };
    let Some(sp_tree) = sp_tree else { return Ok(Vec::new()) };
    let mut out: Vec<Shape> = Vec::new();
    // the drawing has its own rels (images of the shapes)
    let drels = rels_of(src, drawing_path)?;
    for c in sp_tree.children_named("dsp:sp") {
        let renamed = parse_xml(rename_prefix_text(c, "dsp:", "p:", false, None))?;
        let mut sh = read_shape(rd, renamed.root(), theme, Some(&drels))?;
        if let Some(m) = ta(c, "modelId") {
            sh.extras_mut().dgm_id = Some(m.into());
        }
        let Some(sx) = sh.xfrm else { continue };
        let dgm_id = sh.extras.as_ref().and_then(|e| e.dgm_id.clone());
        // <dsp:txXfrm>: the TEXT has its own box; its rot is RELATIVE to the shape's (shape at 90° +
        // text at −90° = text horizontal; 180° + 180° = upright)
        let tx_el = c.first_child("dsp:txXfrm");
        let mut text_shape: Option<Shape> = None;
        if let Some(tx_el) = tx_el
            && sh.tx_body.is_some()
        {
            let mut tf = parse_xfrm(tx_el);
            let net = math::rem(math::rem(sx.rot.unwrap_or(0.0) + tf.rot.unwrap_or(0.0), 21600000.0) + 21600000.0, 21600000.0);
            tf.rot = if net != 0.0 && !net.is_nan() { Some(net) } else { None };
            let mut t = Shape {
                id: format!("{}t", sh.id).into(),
                name: sh.name.clone(),
                xfrm: Some(tf),
                geom: Some("rect".into()),
                fill: Some(Box::new(Fill::of_kind(FillKind::None))),
                line_none: Some(true),
                tx_body: sh.tx_body.take(),
                style_font: StyleFont::boxed(
                    sh.sf().color.clone().filter(|c| !c.as_str().is_empty()),
                    sh.sf().idx.clone().filter(|c| !c.is_empty()),
                ),
                ..Shape::default()
            };
            let ex = t.extras_mut();
            ex.dgm_text_of = Some(sh.id.clone());
            if let Some(d) = dgm_id.filter(|d| !d.is_empty()) {
                ex.dgm_id = Some(d);
            }
            text_shape = Some(t);
        }
        out.push(sh);
        if let Some(t) = text_shape {
            out.push(t);
        }
    }
    if out.is_empty() {
        return Ok(out);
    }
    // the drawing's coordinates are those of its OWN frame (origin at its corner): just shift.
    // Rescale only if the set (ROTATED boxes) does not fit — frame resized by another generator
    // without redoing the drawing
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for sh in &out {
        let Some(f) = sh.xfrm else { continue };
        let th = ((f.rot.unwrap_or(0.0)) / 60000.0) * std::f64::consts::PI / 180.0;
        let (co, si) = (math::abs(math::cos(th)), math::abs(math::sin(th)));
        let (w, h) = (f.cx * co + f.cy * si, f.cx * si + f.cy * co);
        let (cx, cy) = (f.x + f.cx / 2.0, f.y + f.cy / 2.0);
        min_x = math::min(min_x, cx - w / 2.0);
        min_y = math::min(min_y, cy - h / 2.0);
        max_x = math::max(max_x, cx + w / 2.0);
        max_y = math::max(max_y, cy + h / 2.0);
    }
    let bw = nz1(max_x - min_x);
    let bh = nz1(max_y - min_y);
    let fits = min_x >= -0.05 * frame.cx && min_y >= -0.05 * frame.cy && max_x <= frame.cx * 1.05 && max_y <= frame.cy * 1.05;
    let s = if fits { 1.0 } else { math::min(frame.cx / bw, frame.cy / bh) };
    let ox = if fits { frame.x } else { frame.x + (frame.cx - bw * s) / 2.0 - min_x * s };
    let oy = if fits { frame.y } else { frame.y + (frame.cy - bh * s) / 2.0 - min_y * s };
    for sh in &mut out {
        if let Some(f) = sh.xfrm.as_mut() {
            f.x = ox + f.x * s;
            f.y = oy + f.y * s;
            f.cx *= s;
            f.cy *= s;
        }
    }
    Ok(out)
}

/// `/uniqueId="([^"]*)"/.exec(text)?.[1]`.
fn unique_id_of(text: &str) -> Option<&str> {
    let key = "uniqueId=\"";
    let mut from = 0usize;
    while let Some(i) = text.get(from..).and_then(|t| t.find(key)) {
        let start = from + i + key.len();
        if let Some(end) = text.get(start..).and_then(|t| t.find('"')) {
            return text.get(start..start + end);
        }
        from = start;
    }
    None
}

/// `a || 1`.
fn nz1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

/// `readTable` (`read.ts:1230`): `<a:tbl>` → [`Table`] model (grid, rows, cells; styles come after).
fn read_table(rd: &Rd<'_>, tbl: Element<'_>, theme: &Theme, paint: PaintResolver<'_>, rels: &RelMap) -> R<Table> {
    // PowerPoint links the runs of a cell too (`!ts_links`); the TS reads cells without the slide's relationships
    let cell_rels = if theme.rules.ts_links { None } else { Some(rels) };
    let grid_el = tbl.first_child("a:tblGrid");
    let mut col_widths: Vec<f64> = Vec::new();
    if let Some(g) = grid_el {
        for gc in g.children_named("a:gridCol") {
            col_widths.push(ta(gc, "w").map_or(0.0, pint));
        }
    }
    let n_rows = tbl.children_named("a:tr").count();
    if col_widths.len().saturating_mul(n_rows) > usize::try_from(rd.limits.max_table_cells).unwrap_or(usize::MAX) {
        return Err(ReadError::Limit("table cells"));
    }
    let mut rows: Vec<TableRow> = Vec::new();
    for tr in tbl.children_named("a:tr") {
        let height_emu = ta(tr, "h").map_or(0.0, pint);
        let mut cells: Vec<TableCell> = Vec::new();
        for tc in tr.children_named("a:tc") {
            let mut cell = TableCell::default();
            if let Some(g) = ta(tc, "gridSpan") {
                cell.grid_span = Some(pint(g));
            }
            if let Some(g) = ta(tc, "rowSpan") {
                cell.row_span = Some(pint(g));
            }
            if tc.attr("hMerge") == Some("1") {
                cell.h_merge = Some(true);
            }
            if tc.attr("vMerge") == Some("1") {
                cell.v_merge = Some(true);
            }
            if let Some(tx) = tc.first_child("a:txBody") {
                cell.tx_body = Some(Box::new(read_tx_body(tx, theme, cell_rels, None)));
            }
            if let Some(tc_pr) = tc.first_child("a:tcPr") {
                cell.anchor = ta(tc_pr, "anchor").map(Into::into);
                cell.vert = ta(tc_pr, "vert").filter(|v| *v != "horz").map(Into::into);
                // <a:noFill/> straight in tcPr: no fill (beats the table style)
                if tc_pr.first_child("a:noFill").is_some() {
                    cell.no_fill = Some(true);
                }
                for (k, slot) in
                    [("marL", &mut cell.mar_l), ("marR", &mut cell.mar_r), ("marT", &mut cell.mar_t), ("marB", &mut cell.mar_b)]
                {
                    if let Some(v) = tc_pr.attr(k).filter(|v| is_int_text(v)) {
                        *slot = Some(pint(v));
                    }
                }
                // fill: solid (with alpha), gradient/pattern or image (tiled from the table)
                match paint(tc_pr)? {
                    PaintOutcome::Paint(pt) => {
                        if let Some(f) = pt.fill.as_ref().filter(|f| f.kind == FillKind::Solid && f.color.is_some()) {
                            cell.fill = f.color.clone();
                            if pt.alpha.is_some() {
                                cell.fill_alpha = pt.alpha;
                            }
                        } else if let Some(f) = pt.fill.filter(|f| f.kind != FillKind::None) {
                            cell.fill_paint = Some(Box::new(f));
                        } else if let Some(img) = pt.image {
                            cell.fill_image = Some(img);
                            if pt.tile == Some(true) {
                                cell.fill_tile = Some(true);
                            }
                        }
                    }
                    PaintOutcome::Null | PaintOutcome::Undefined => {}
                }
                // borders PER SIDE (the reference application treats each side independently)
                for (key, name) in [(0usize, "a:lnL"), (1, "a:lnR"), (2, "a:lnT"), (3, "a:lnB")] {
                    let Some(ln) = tc_pr.first_child(name) else { continue };
                    if let Some(tl) = cell_line(ln, theme) {
                        let lines = cell.lines.get_or_insert_with(Default::default);
                        let v = match tl {
                            LineOrNull::Null => CellLine::NoFill,
                            LineOrNull::Line(l) => CellLine::Line(l),
                        };
                        side_slot(lines, key, v);
                    }
                    // side with explicit <a:noFill/>: no border (beats the style)
                    if ln.first_child("a:noFill").is_some() {
                        let bn = cell.border_none.get_or_insert_with(Default::default);
                        side_slot(bn, key, true);
                        continue;
                    }
                    let fill_el = ln.first_child("a:solidFill");
                    let Some(color) = fill_el.and_then(|f| color_from_dml(f.child_elements().next(), theme)) else { continue };
                    let width = ta(ln, "w").map_or(12700.0, pint);
                    let mut edge = CellBorder { color, width, dash: None };
                    if let Some(d) = ln.first_child("a:prstDash").and_then(|d| ta(d, "val")).filter(|d| *d != "solid") {
                        edge.dash = Some(d.into());
                    }
                    let borders = cell.borders.get_or_insert_with(Default::default);
                    side_slot(borders, key, edge);
                }
            }
            cells.push(cell);
        }
        rows.push(TableRow { height_emu, cells });
    }
    // merge only holds over covered cells that EXIST (hMerge/vMerge); a row with fewer <a:tc> than the
    // grid gets empty cells (measured, poi-LIBRE_OFFICE-100610-0 #8: the application draws "Vermelho"
    // in one column and two new cells with the grid)
    let n_c = col_widths.len();
    for r in 0..rows.len() {
        let n_cells = rows.get(r).map_or(0, |row| row.cells.len());
        for c in 0..n_cells {
            let (is_merged, gs, rs) = match rows.get(r).and_then(|row| row.cells.get(c)) {
                Some(cell) => (cell.h_merge == Some(true) || cell.v_merge == Some(true), cell.grid_span, cell.row_span),
                None => continue,
            };
            if is_merged {
                continue;
            }
            if gs.unwrap_or(1.0) > 1.0 {
                let gs = gs.unwrap_or(1.0);
                let mut k = 1.0_f64;
                while k < gs && rows.get(r).and_then(|row| row.cells.get(c + usize_of(k)?)).is_some_and(|x| x.h_merge == Some(true)) {
                    k += 1.0;
                }
                if let Some(cell) = rows.get_mut(r).and_then(|row| row.cells.get_mut(c)) {
                    cell.grid_span = if k > 1.0 { Some(k) } else { None };
                }
            }
            if rs.unwrap_or(1.0) > 1.0 {
                let rs = rs.unwrap_or(1.0);
                let mut k = 1.0_f64;
                while k < rs
                    && rows.get(r + usize_of(k).unwrap_or(0)).and_then(|row| row.cells.get(c)).is_some_and(|x| x.v_merge == Some(true))
                {
                    k += 1.0;
                }
                if let Some(cell) = rows.get_mut(r).and_then(|row| row.cells.get_mut(c)) {
                    cell.row_span = if k > 1.0 { Some(k) } else { None };
                }
            }
        }
        if let Some(row) = rows.get_mut(r) {
            while row.cells.len() < n_c {
                row.cells.push(TableCell::default());
            }
        }
    }
    Ok(Table { col_widths_emu: col_widths, rows: rows.into_iter().collect(), ..Table::default() })
}

fn side_slot<T>(s: &mut Sides<T>, key: usize, v: T) {
    match key {
        0 => s.l = Some(v),
        1 => s.r = Some(v),
        2 => s.t = Some(v),
        _ => s.b = Some(v),
    }
}

/// `readShape` (`read.ts:1313`): `<p:sp>` or `<p:cxnSp>`.
pub fn read_shape(rd: &Rd<'_>, sp: Element<'_>, theme: &Theme, rels: Option<&RelMap>) -> R<Shape> {
    let src = rd.src;
    // <p:sp> uses p:nvSpPr; <p:cxnSp> (connector) uses p:nvCxnSpPr — same structure
    let nv_sp_pr = sp.first_child("p:nvSpPr").or_else(|| sp.first_child("p:nvCxnSpPr"));
    let c_nv_pr = nv_sp_pr.and_then(|n| n.first_child("p:cNvPr"));
    let c_nv_sp_pr = nv_sp_pr.and_then(|n| n.first_child("p:cNvSpPr"));
    let nv_pr = nv_sp_pr.and_then(|n| n.first_child("p:nvPr"));
    let ph = nv_pr.and_then(|n| n.first_child("p:ph"));
    let sp_pr = sp.first_child("p:spPr");

    let mut shape = base_shape(c_nv_pr);
    if c_nv_sp_pr.and_then(|c| c.attr("txBox")) == Some("1") {
        shape.is_text_box = Some(true);
    }
    if let Some(ph) = ph {
        shape.ph_type = ta(ph, "type").map(Into::into);
        shape.ph_idx = ta(ph, "idx").map(Into::into);
    }
    // placeholders without an explicit type are "body" by default
    if ph.is_some() && shape.ph_type.is_none() {
        shape.ph_type = Some("body".into());
    }
    let blip_cx = rels;

    if let Some(sp_pr) = sp_pr {
        if let Some(x) = sp_pr.first_child("a:xfrm") {
            shape.xfrm = Some(parse_xfrm(x));
        }
        let geom = sp_pr.first_child("a:prstGeom");
        if let Some(p) = geom.and_then(|g| ta(g, "prst")) {
            shape.geom = Some(p.into());
        }
        // adjust values (thin frame, corners, arc angles…)
        if let Some(av) = geom.and_then(|g| g.first_child("a:avLst")) {
            read_adj(av, &mut shape);
        }
        // custom geometry (freeform): <a:custGeom><a:pathLst><a:path w h>…
        if let Some(cust) = sp_pr.first_child("a:custGeom")
            && let Some(cg) = parse_cust_geom(cust, shape.xfrm.map(|x| x.cx), shape.xfrm.map(|x| x.cy))
        {
            shape.cust_geom = Some(Box::new(cg));
        }
        if sp_pr.first_child("a:noFill").is_some() {
            shape.fill = Some(Box::new(Fill::of_kind(FillKind::None)));
        } else if sp_pr.first_child("a:grpFill").is_some() {
            shape.grp_fill = Some(true); // inherits from the group (readGroup)
        } else if let Some(c) = solid_fill_color(sp_pr, theme) {
            shape.fill = Some(Box::new(solid_fill(c)));
        }
        if shape.fill.is_none()
            && let Some(pf) = sp_pr.first_child("a:pattFill")
        {
            shape.fill = Some(Box::new(parse_patt_fill(pf, theme)));
        }
        // linear gradient (spec 03 G): <a:gradFill><a:gsLst>… + <a:lin ang>
        if shape.fill.is_none()
            && let Some(grad) = sp_pr.first_child("a:gradFill")
            && let Some(g) = parse_grad_fill(grad, theme)
        {
            shape.fill = Some(Box::new(g));
            if grad.attr("rotWithShape") == Some("0") {
                shape.paint_mut().fill_no_rot = Some(true);
            }
        }
        // blipFill of a shape (stretch): image as the geometry's fill
        if let Some(rels) = blip_cx {
            let bf = sp_pr.first_child("a:blipFill");
            if let Some(blip) = bf.and_then(|b| b.first_child("a:blip"))
                && let Some(mut img) = resolve_blip(blip, src, rels, Some(theme))
            {
                if apply_tile(&mut img, bf) {
                    shape.paint_mut().fill_tile = Some(true);
                }
                shape.paint_mut().fill_image = Some(Box::new(img));
                if bf.and_then(|b| b.attr("rotWithShape")) == Some("0") {
                    shape.paint_mut().fill_no_rot = Some(true);
                }
            }
        }
        // alpha of the fill: <a:solidFill><a:srgbClr><a:alpha val=60000/></a:srgbClr></a:solidFill>
        if let Some(sf) = sp_pr.first_child("a:solidFill")
            && let Some(a) = fill_alpha_val(sf).filter(|a| !a.is_empty())
        {
            shape.paint_mut().fill_alpha = Some(pint(a) / 100000.0);
        }
        if let Some(ln) = sp_pr.first_child("a:ln") {
            if ln.first_child("a:noFill").is_some() {
                shape.line_none = Some(true); // EXPLICITLY no outline
            }
            if let Some(lc) = solid_fill_color(ln, theme) {
                shape.line_color = Some(lc);
            }
            if let Some(la) = ln.first_child("a:solidFill").and_then(fill_alpha_val) {
                shape.x_mut().line_alpha = Some(math::max(0.0, math::min(1.0, pint(la) / 100000.0)));
            }
            if let Some(w) = ta(ln, "w") {
                shape.line_width = Some(pint(w));
            }
            if let Some(d) = ln.first_child("a:prstDash").and_then(|d| ta(d, "val")) {
                shape.x_mut().line_dash = Some(d.into());
            }
            // line arrows (headEnd/tailEnd; type=none omits)
            if let Some(h) = ln.first_child("a:headEnd").and_then(|e| ta(e, "type")).filter(|t| *t != "none") {
                shape.extras_mut().head_arrow = Some(h.into());
            }
            if let Some(t) = ln.first_child("a:tailEnd").and_then(|e| ta(e, "type")).filter(|t| *t != "none") {
                shape.extras_mut().tail_arrow = Some(t.into());
            }
        }
    }
    // <p:sp useBgFill="1">: the fill is the slide background (applied in readSlide)
    if matches!(sp.attr("useBgFill"), Some("1" | "true")) {
        shape.paint_mut().use_bg_fill = Some(true);
        shape.fill = None;
    }
    // effects (outer/inner shadow, glow, soft edges, reflection): the spPr's own or, without its
    // `<a:effectLst>`, the theme style by the effectRef
    let style_el = sp.first_child("p:style");
    let fx = read_effects(sp_pr, style_el, theme);
    shape.set_effects(fx);
    read_bevel(sp_pr, style_el, theme, &mut shape);
    // rotated 3D camera (the shape's 3D scene): the projection becomes an affine transform
    let cam_rot =
        sp_pr.and_then(|s| s.first_child("a:scene3d")).and_then(|s| s.first_child("a:camera")).and_then(|c| c.first_child("a:rot"));
    if let Some(cr) = cam_rot
        && (ta(cr, "lat").is_some() || ta(cr, "lon").is_some() || ta(cr, "rev").is_some())
    {
        let deg = |k: &str| pint(cr.attr(k).unwrap_or("0")) / 60000.0;
        shape.extras_mut().fx_mut().cam_rot = Some(CamRot { lat: deg("lat"), lon: deg("lon"), rev: deg("rev") });
    }

    // <p:style> (spec 03 B): thematic defaults when spPr/run does not specify.
    //   fillRef → fill colour; lnRef → line colour; fontRef → default text colour/font (cascade).
    if let Some(style_el) = style_el {
        let ref_color = |name: &str| -> Option<CssColor> {
            let r = style_el.first_child(name)?;
            if r.attr("idx") == Some("0") {
                return None; // idx=0 = no style
            }
            color_from_dml(r.child_elements().next(), theme)
        };
        if shape.fill.is_none() && shape.paint().fill_image.is_none() && shape.grp_fill.is_none() && shape.paint().use_bg_fill.is_none() {
            // fillStyleLst style of the theme (idx 1 solid, 2/3 gradients in 2007–2013 themes) with
            // phClr = the reference's colour; without a theme, the plain colour
            let r = style_el.first_child("a:fillRef");
            let st = match r {
                Some(r) => theme_style_fill(r, theme, Some(src))?,
                None => None,
            };
            match st {
                Some(StyleFill { fill: Some(f), .. }) => shape.fill = Some(Box::new(f)),
                Some(StyleFill { image: Some(img), tile, .. }) => {
                    shape.paint_mut().fill_image = Some(Box::new(img));
                    if tile {
                        shape.paint_mut().fill_tile = Some(true);
                    }
                }
                _ => {
                    if let Some(fc) = ref_color("a:fillRef") {
                        shape.fill = Some(Box::new(solid_fill(fc)));
                    }
                }
            }
        }
        // gradient of the spPr WITHOUT <a:lin>/<a:path>: the direction comes from the theme style
        // gradient the fillRef points at (measured, syn-gradnolin2: fillRef 2/3 with lin 90° →
        // vertical; fillRef 1 solid → the horizontal default)
        let gf_el = sp.first_child("p:spPr").and_then(|s| s.first_child("a:gradFill"));
        if shape.fill.as_ref().is_some_and(|f| f.kind == FillKind::Gradient)
            && let Some(gf_el) = gf_el
            && gf_el.first_child("a:lin").is_none()
            && gf_el.first_child("a:path").is_none()
        {
            let fr = style_el.first_child("a:fillRef");
            let idx = fr.map_or(0.0, |f| pint(f.attr("idx").unwrap_or("0")));
            let st_el =
                if (1.0..1000.0).contains(&idx) { theme.styles.as_ref().and_then(|s| s.fill_style(usize_of(idx - 1.0)?)) } else { None };
            let lin = st_el.filter(|s| s.name() == "a:gradFill").and_then(|s| s.first_child("a:lin"));
            if let Some(ang) = lin.and_then(|l| l.attr("ang"))
                && let Some(f) = shape.fill.as_mut()
            {
                f.angle_deg = Some(pint(ang) / 60000.0);
                if lin.and_then(|l| l.attr("scaled")) == Some("1") {
                    f.scaled = Some(true);
                }
            }
        }
        if shape.line_color.is_none() && shape.line_none != Some(true) {
            let lc = ref_color("a:lnRef");
            if let Some(lc) = lc {
                shape.line_color = Some(lc);
                // width comes from the theme's lnStyleLst by the lnRef idx
                if shape.line_width.is_none() {
                    let ln_ref = style_el.first_child("a:lnRef");
                    let idx = ln_ref.and_then(|l| l.attr("idx")).map_or(0.0, pint);
                    let w = if idx >= 1.0 { usize_of(idx - 1.0).and_then(|i| theme.line_widths.as_ref()?.get(i).copied()) } else { None };
                    if let Some(w) = w {
                        shape.line_width = Some(w);
                    }
                }
            }
        }
        if let Some(font_ref) = style_el.first_child("a:fontRef") {
            if let Some(fc) = color_from_dml(font_ref.child_elements().next(), theme) {
                shape.sf_mut().color = Some(fc);
            }
            if let Some(i) = ta(font_ref, "idx") {
                shape.sf_mut().idx = Some(i.into());
            }
        }
    }

    if let Some(tx) = sp.first_child("p:txBody") {
        let blip_fn;
        let blip: Option<BlipResolver<'_>> = match rels {
            Some(r) => {
                blip_fn = move |b: Element<'_>| resolve_blip(b, src, r, Some(theme));
                Some(&blip_fn)
            }
            None => None,
        };
        shape.tx_body = Some(Box::new(read_tx_body(tx, theme, rels, blip)));
    }
    Ok(shape)
}

/// The `<a:avLst><a:gd fmla="val N">` adjust values of a preset geometry: `adj` and `adjNames`.
fn read_adj(av: Element<'_>, shape: &mut Shape) {
    let mut adj: Vec<f64> = Vec::new();
    let mut names: Vec<Box<str>> = Vec::new();
    for gd in av.children_named("a:gd") {
        if let Some(m) = val_after(gd.attr("fmla").unwrap_or("")) {
            adj.push(pint(m));
            names.push(gd.attr("name").unwrap_or("").into());
        }
    }
    if !adj.is_empty() {
        shape.x_mut().adj = Some(adj.into_boxed_slice());
        shape.x_mut().adj_names = Some(names.into_boxed_slice());
    }
}

/// `readPic` (`read.ts:1963`): `<p:pic>` (embedded image) → Shape with the image resolved through the rels.
pub fn read_pic(rd: &Rd<'_>, pic: Element<'_>, theme: &Theme, rels: &RelMap) -> Shape {
    let src = rd.src;
    let nv_pic_pr = pic.first_child("p:nvPicPr");
    let c_nv_pr = nv_pic_pr.and_then(|n| n.first_child("p:cNvPr"));
    // <p:blipFill> inside mc:AlternateContent (PowerPoint for Mac: Choice "ma" with a PDF, Fallback
    // with a PNG) — the Fallback counts, which is what the Windows application draws
    let alt = pic.first_child("mc:AlternateContent");
    let blip_fill = pic.first_child("p:blipFill").or_else(|| {
        let a = alt?;
        let holder = a.first_child("mc:Fallback").or_else(|| a.first_child("mc:Choice")).unwrap_or(a);
        holder.first_child("p:blipFill")
    });
    let blip = blip_fill.and_then(|b| b.first_child("a:blip"));
    let src_rect = blip_fill.and_then(|b| b.first_child("a:srcRect"));
    let sp_pr = pic.first_child("p:spPr");

    let mut shape = base_shape(c_nv_pr);
    // image INSIDE a placeholder (`<p:ph type="pic" idx>`, modern templates): without its own xfrm
    // the geometry comes from the layout/master placeholder
    let ph = nv_pic_pr.and_then(|n| n.first_child("p:nvPr")).and_then(|n| n.first_child("p:ph"));
    if let Some(ph) = ph {
        shape.ph_type = Some(ph.attr("type").unwrap_or("body").into());
        shape.ph_idx = ta(ph, "idx").map(Into::into);
    }
    if let Some(m) = nv_pic_pr.and_then(|n| read_pic_media(n, src, rels)) {
        shape.extras_mut().media = Some(Box::new(m));
    }
    if let Some(blip) = blip {
        if let Some(img) = resolve_blip(blip, src, rels, Some(theme)) {
            shape.paint_mut().image = Some(Box::new(img));
        }
        // <a:alphaModFix amt="55000"/> → 55% opaque
        if let Some(a) = blip.first_child("a:alphaModFix").and_then(|e| ta(e, "amt")) {
            shape.paint_mut().alpha = Some(math::max(0.0, math::min(1.0, pint(a) / 100000.0)));
        }
    }
    if let Some(sr) = src_rect {
        // values in thousandths of a percent; absence = 0. Normalized to 0..1.
        let num = |k: &str| ta(sr, k).map_or(0.0, pint) / 100000.0;
        shape.paint_mut().src_rect = Some(Inset { l: num("l"), t: num("t"), r: num("r"), b: num("b") });
    }
    if let Some(sp_pr) = sp_pr {
        if let Some(x) = sp_pr.first_child("a:xfrm") {
            shape.xfrm = Some(parse_xfrm(x));
        }
        let geom = sp_pr.first_child("a:prstGeom");
        if let Some(p) = geom.and_then(|g| ta(g, "prst")) {
            shape.geom = Some(p.into());
        }
        // preset adjustments (arrow-shaped crop, circle…) and CUSTOM geometry (image cropped to a
        // triangle — web-gh-hta-animated-carousel)
        if let Some(pav) = geom.and_then(|g| g.first_child("a:avLst")) {
            read_adj(pav, &mut shape);
        }
        if let Some(pcust) = sp_pr.first_child("a:custGeom")
            && let Some(cg) = parse_cust_geom(pcust, shape.xfrm.map(|x| x.cx), shape.xfrm.map(|x| x.cy))
        {
            shape.cust_geom = Some(Box::new(cg));
        }
        // fill BEHIND the image (transparent OLE/WMF previews with a white background)
        let sf = sp_pr.first_child("a:solidFill");
        let sc = sf.and_then(|_| solid_fill_color(sp_pr, theme));
        if let Some(sc) = sc {
            shape.fill = Some(Box::new(solid_fill(sc)));
            if let Some(a) = sf.and_then(fill_alpha_val).filter(|a| !a.is_empty()) {
                shape.paint_mut().fill_alpha = Some(pint(a) / 100000.0);
            }
        } else if let Some(g) = sp_pr.first_child("a:gradFill").and_then(|g| parse_grad_fill(g, theme)) {
            shape.fill = Some(Box::new(g));
        }
        if let Some(ln) = sp_pr.first_child("a:ln") {
            if let Some(lc) = solid_fill_color(ln, theme) {
                shape.line_color = Some(lc);
            }
            if let Some(w) = ta(ln, "w") {
                shape.line_width = Some(pint(w));
            }
        }
    }
    let fx = read_effects(sp_pr, pic.first_child("p:style"), theme);
    shape.set_effects(fx);
    shape
}
