//! `deck-edit.ts` part 2: shapes and their text (lines 504-1030 of the TS): insert (text box, preset
//! shape, picture), frame, fill, outline, order, delete (with its animations), text, character and
//! paragraph formatting, bullets, hyperlink and replace. Every function is string surgery on one
//! part, as the TS (`edit::xml`).

use rustc_hash::FxHashSet;
use uo_jsnum::math::{max as jmax, min as jmin, round as jround};
use uo_jsnum::text::{is_js_space, is_js_word, js_space_len};

use super::pkg::{E, EditError, add_rel, ensure_default, fmt, hex, next_part_name, num_or0, rel, relative_target};
use super::xml::{
    NamedRange, XmlRange, child_ranges, element_name, escape_attr, escape_text, find_all_elements, find_element, find_elements, find_first,
    find_from, get_attr, inner_xml, insert_child, js_trim, set_attr, set_attr_in_tag, set_inner, sl, sl_from, splice, unescape_xml,
};
use crate::package::PartStore;

/// Position and size in EMU (`Frame`).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub cx: f64,
    pub cy: f64,
}

const SHAPE_TAGS: [&str; 4] = ["p:sp", "p:pic", "p:cxnSp", "p:graphicFrame"];

fn js(w: &'static str) -> EditError {
    EditError::Js(w)
}

/// The text of a part that must exist (`pkg.getPartText(path)!`).
pub fn must_text(store: &PartStore, path: &str) -> E<String> {
    store.text(path).ok_or(js("missing part"))
}

/// The spans of the element of shape `id` in the slide (more than one inside an
/// `mc:AlternateContent`) (`shapeRanges`).
pub fn shape_ranges(xml: &str, id: &str) -> Vec<XmlRange> {
    let Some(tree) = find_first(xml, "p:spTree") else { return Vec::new() };
    let mut out = Vec::new();
    for tag in SHAPE_TAGS {
        for r in find_elements(xml, tag, tree.open_end, tree.end) {
            if let Some(c) = find_element(xml, "p:cNvPr", r.start, r.end)
                && get_attr(xml, &c, "id").as_deref() == Some(id)
            {
                out.push(r);
            }
        }
    }
    out.sort_by_key(|r| r.start);
    out
}

/// What leaves/enters/moves with the shape: it, or the `mc:AlternateContent` that wraps it.
fn outer_range(xml: &str, r: &XmlRange) -> E<XmlRange> {
    let tree = find_first(xml, "p:spTree").ok_or(js("p:spTree"))?;
    for ac in find_elements(xml, "mc:AlternateContent", tree.open_end, tree.end) {
        if ac.start < r.start && ac.end > r.end {
            return Ok(ac);
        }
    }
    Ok(*r)
}

/// Is the shape a direct child of the slide tree (not in a group)? (`isTopLevel`)
pub fn is_top_level(xml: &str, id: &str) -> E<bool> {
    let Some(r) = shape_ranges(xml, id).into_iter().next() else { return Ok(false) };
    let o = outer_range(xml, &r)?;
    let tree = find_first(xml, "p:spTree").ok_or(js("p:spTree"))?;
    Ok(child_ranges(xml, &tree).iter().any(|k| k.range.start == o.start))
}

/// Applies `f` to the text of each copy of the shape (back to front: the indices do not move).
fn edit_shape(store: &mut PartStore, slide: &str, id: &str, mut f: impl FnMut(String, XmlRange) -> E<String>) -> E<bool> {
    let Some(mut xml) = store.text(slide).filter(|x| !x.is_empty()) else { return Ok(false) };
    let rs = shape_ranges(&xml, id);
    if rs.is_empty() {
        return Ok(false);
    }
    for r in rs.iter().rev() {
        xml = f(xml, *r)?;
    }
    store.set_text(slide, &xml);
    Ok(true)
}

/// The `<p:cNvPr ... id="N"` matches of a text: (start of the match, start of the digits, end of the
/// digits, end of the match). `/<p:cNvPr\b[^>]*?\sid="(\d+)"/g`.
pub fn scan_cnvpr_ids(xml: &str) -> Vec<(usize, usize, usize, usize)> {
    let b = xml.as_bytes();
    let name = "<p:cNvPr";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find(name)).map(|p| from + p) {
        let after = p + name.len();
        if b.get(after).is_some_and(|&c| is_js_word(c)) {
            from = p + 1;
            continue;
        }
        let mut q = after;
        let mut found = None;
        while q < b.len() {
            let wl = js_space_len(b, q);
            if wl > 0 && xml[q + wl..].starts_with("id=\"") {
                let ds = q + wl + 4;
                let mut de = ds;
                while b.get(de).is_some_and(u8::is_ascii_digit) {
                    de += 1;
                }
                if de > ds && b.get(de) == Some(&b'"') {
                    found = Some((p, ds, de, de + 1));
                    break;
                }
            }
            if b[q] == b'>' {
                break;
            }
            q += xml[q..].chars().next().map_or(1, char::len_utf8);
        }
        match found {
            Some(m) => {
                from = m.3;
                out.push(m);
            }
            None => from = p + 1,
        }
    }
    out
}

/// The largest `id` of a `<p:cNvPr>` in the slide (the next one is +1) (`maxShapeId`).
pub fn max_shape_id(xml: &str) -> f64 {
    let mut max = 1.0;
    for (_, ds, de, _) in scan_cnvpr_ids(xml) {
        max = jmax(max, uo_jsnum::parse::to_number(&xml[ds..de]));
    }
    max
}

/// Inserts the XML of a shape at the end of the tree (on top of everything) (`appendShape`).
fn append_shape(xml: &str, shape: &str) -> E<String> {
    let tree = find_first(xml, "p:spTree").ok_or(js("p:spTree"))?;
    Ok(insert_child(xml, &tree, shape, &["p:extLst"]))
}

fn xfrm_xml(f: &Frame, tag: &str, attrs: &str) -> String {
    format!(
        "<{tag}{attrs}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></{tag}>",
        fmt(jround(f.x)),
        fmt(jround(f.y)),
        fmt(jmax(0.0, jround(f.cx))),
        fmt(jmax(0.0, jround(f.cy)))
    )
}

const SHAPE_STYLE: &str = "<p:style><a:lnRef idx=\"2\"><a:schemeClr val=\"accent1\"><a:shade val=\"50000\"/></a:schemeClr></a:lnRef><a:fillRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:fillRef><a:effectRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:effectRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"lt1\"/></a:fontRef></p:style>";

fn paragraphs_xml(lines: &[&str], lang: &str) -> String {
    lines
        .iter()
        .map(|l| {
            if l.is_empty() {
                format!("<a:p><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p>")
            } else {
                format!("<a:p><a:r><a:rPr lang=\"{lang}\" dirty=\"0\"/><a:t>{}</a:t></a:r></a:p>", escape_text(l))
            }
        })
        .collect()
}

/// New text box; returns the shape id (`insertTextBox`).
pub fn insert_text_box(store: &mut PartStore, slide: &str, frame: &Frame, text: &str, lang: &str) -> E<String> {
    let xml = must_text(store, slide)?;
    let id = max_shape_id(&xml) + 1.0;
    let lines: Vec<&str> = text.split('\n').collect();
    let sp = format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"TextBox {n}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>{xf}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" rtlCol=\"0\"><a:spAutoFit/></a:bodyPr><a:lstStyle/>{paras}</p:txBody></p:sp>",
        id = fmt(id),
        n = fmt(id - 1.0),
        xf = xfrm_xml(frame, "a:xfrm", ""),
        paras = paragraphs_xml(&lines, lang)
    );
    store.set_text(slide, &append_shape(&xml, &sp)?);
    Ok(fmt(id))
}

/// New preset shape with the theme style; returns the id (`insertShape`).
pub fn insert_shape(store: &mut PartStore, slide: &str, prst: &str, frame: &Frame, lang: &str) -> E<String> {
    let xml = must_text(store, slide)?;
    let id = max_shape_id(&xml) + 1.0;
    let (ids, n) = (fmt(id), fmt(id - 1.0));
    let sp = if prst == "line" {
        format!(
            "<p:cxnSp><p:nvCxnSpPr><p:cNvPr id=\"{ids}\" name=\"Straight Connector {n}\"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>{}<a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom></p:spPr><p:style><a:lnRef idx=\"1\"><a:schemeClr val=\"accent1\"/></a:lnRef><a:fillRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:fillRef><a:effectRef idx=\"0\"><a:schemeClr val=\"accent1\"/></a:effectRef><a:fontRef idx=\"minor\"><a:schemeClr val=\"tx1\"/></a:fontRef></p:style></p:cxnSp>",
            xfrm_xml(frame, "a:xfrm", "")
        )
    } else {
        format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{ids}\" name=\"{} {n}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"{}\"><a:avLst/></a:prstGeom></p:spPr>{SHAPE_STYLE}<p:txBody><a:bodyPr rtlCol=\"0\" anchor=\"ctr\"/><a:lstStyle/><a:p><a:pPr algn=\"ctr\"/><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p></p:txBody></p:sp>",
            escape_attr(prst),
            xfrm_xml(frame, "a:xfrm", ""),
            escape_attr(prst)
        )
    };
    store.set_text(slide, &append_shape(&xml, &sp)?);
    Ok(ids)
}

fn image_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpeg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => "png",
    }
}

/// New picture (the part goes to `ppt/media`); returns `(id, part)` (`insertImage`).
pub fn insert_image(
    store: &mut PartStore,
    slide: &str,
    bytes: std::sync::Arc<[u8]>,
    mime: &str,
    frame: &Frame,
    name: &str,
) -> E<(String, String)> {
    let ext = image_ext(mime);
    let part = next_part_name(store, "ppt/media", "image", ext, &FxHashSet::default());
    store.set(&part, bytes);
    ensure_default(store, ext, mime)?;
    let r_id = add_rel(store, slide, rel::IMAGE, &relative_target(slide, &part), false)?;
    let xml = must_text(store, slide)?;
    let id = max_shape_id(&xml) + 1.0;
    let pic = format!(
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{ids}\" name=\"{} {n}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{r_id}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        escape_attr(name),
        xfrm_xml(frame, "a:xfrm", ""),
        ids = fmt(id),
        n = fmt(id - 1.0)
    );
    store.set_text(slide, &append_shape(&xml, &pic)?);
    Ok((fmt(id), part))
}

/// `tag.replace(/\s*\/?>$/, '')`.
fn strip_tag_end(tag: &str) -> &str {
    if !tag.ends_with('>') {
        return tag;
    }
    let mut cut = tag.len() - 1;
    if tag[..cut].ends_with('/') {
        cut -= 1;
    }
    tag[..cut].trim_end_matches(is_js_space)
}

/// Position/size of the shape (creates the `xfrm` of a placeholder that inherited the geometry)
/// (`setShapeFrame`).
pub fn set_shape_frame(store: &mut PartStore, slide: &str, id: &str, f: &Frame) -> E<bool> {
    edit_shape(store, slide, id, |xml, r| {
        if element_name(&xml, &r) == "p:graphicFrame" {
            if let Some(x) = find_element(&xml, "p:xfrm", r.start, r.end) {
                return Ok(splice(&xml, x.start, x.end, &xfrm_xml(f, "p:xfrm", "")));
            }
            let nv = find_element(&xml, "p:nvGraphicFramePr", r.start, r.end).ok_or(js("p:nvGraphicFramePr"))?;
            return Ok(splice(&xml, nv.end, nv.end, &xfrm_xml(f, "p:xfrm", "")));
        }
        let Some(sp) = find_element(&xml, "p:spPr", r.start, r.end) else { return Ok(xml) };
        if let Some(x) = find_element(&xml, "a:xfrm", sp.open_end, sp.end) {
            // keeps rot/flipH/flipV of the tag
            let open = strip_tag_end(sl(&xml, x.start, x.open_end));
            let open = open.strip_prefix("<a:xfrm").unwrap_or(open);
            return Ok(splice(&xml, x.start, x.end, &xfrm_xml(f, "a:xfrm", open)));
        }
        Ok(if sp.self_closing {
            set_inner(&xml, &sp, &xfrm_xml(f, "a:xfrm", ""))
        } else {
            splice(&xml, sp.open_end, sp.open_end, &xfrm_xml(f, "a:xfrm", ""))
        })
    })
}

const FILL_TAGS: [&str; 6] = ["a:noFill", "a:solidFill", "a:gradFill", "a:blipFill", "a:pattFill", "a:grpFill"];

/// Replaces the DIRECT fill of an element (spPr, ln or rPr), keeping the schema order.
fn replace_fill(mut xml: String, el: &XmlRange, fill: &str, before: &[&str]) -> E<String> {
    let name = element_name(&xml, el);
    for k in child_ranges(&xml, el).iter().rev() {
        if FILL_TAGS.contains(&k.name.as_str()) {
            xml = splice(&xml, k.range.start, k.range.end, "");
        }
    }
    // the span of the element shrank: find it again before inserting
    let again = find_from(&xml, &name, el.start).ok_or(js("element"))?;
    Ok(insert_child(&xml, &again, fill, before))
}

fn solid(color: &str) -> String {
    format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", hex(color))
}

/// `color ? x : y` of a JS string (an empty string is falsy).
pub fn truthy(c: Option<&str>) -> Option<&str> {
    c.filter(|s| !s.is_empty())
}

const SP_PR_AFTER_FILL: [&str; 6] = ["a:ln", "a:effectLst", "a:effectDag", "a:scene3d", "a:sp3d", "a:extLst"];
const LN_AFTER_FILL: [&str; 8] = ["a:prstDash", "a:custDash", "a:round", "a:bevel", "a:miter", "a:headEnd", "a:tailEnd", "a:extLst"];

/// Fill of the shape (`None` = no fill) (`setShapeFill`).
pub fn set_shape_fill(store: &mut PartStore, slide: &str, id: &str, color: Option<&str>) -> E<bool> {
    edit_shape(store, slide, id, |mut xml, r| {
        let Some(mut sp) = find_element(&xml, "p:spPr", r.start, r.end) else { return Ok(xml) };
        if sp.self_closing {
            xml = set_inner(&xml, &sp, "");
            sp = find_from(&xml, "p:spPr", r.start).ok_or(js("p:spPr"))?;
        }
        let fill = truthy(color).map_or_else(|| "<a:noFill/>".to_owned(), solid);
        replace_fill(xml, &sp, &fill, &SP_PR_AFTER_FILL)
    })
}

/// Outline of the shape (`None` = no outline) (`setShapeOutline`).
pub fn set_shape_outline(store: &mut PartStore, slide: &str, id: &str, color: Option<&str>) -> E<bool> {
    edit_shape(store, slide, id, |mut xml, r| {
        let Some(mut sp) = find_element(&xml, "p:spPr", r.start, r.end) else { return Ok(xml) };
        if sp.self_closing {
            xml = set_inner(&xml, &sp, "");
            sp = find_from(&xml, "p:spPr", r.start).ok_or(js("p:spPr"))?;
        }
        let mut ln = find_element(&xml, "a:ln", sp.open_end, sp.end);
        match ln {
            None => {
                let el = if truthy(color).is_some() { "<a:ln w=\"12700\"></a:ln>" } else { "<a:ln></a:ln>" };
                xml = insert_child(&xml, &sp, el, &["a:effectLst", "a:effectDag", "a:scene3d", "a:sp3d", "a:extLst"]);
                sp = find_from(&xml, "p:spPr", r.start).ok_or(js("p:spPr"))?;
                ln = Some(find_element(&xml, "a:ln", sp.open_end, sp.end).ok_or(js("a:ln"))?);
            }
            Some(l) if l.self_closing => {
                xml = set_inner(&xml, &l, "");
                ln = Some(find_from(&xml, "a:ln", sp.open_end).ok_or(js("a:ln"))?);
            }
            Some(_) => {}
        }
        let ln = ln.ok_or(js("a:ln"))?;
        let fill = truthy(color).map_or_else(|| "<a:noFill/>".to_owned(), solid);
        replace_fill(xml, &ln, &fill, &LN_AFTER_FILL)
    })
}

/// `ArrangeMode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrangeMode {
    Front,
    Back,
    Forward,
    Backward,
}

/// Order of the shape among the siblings of the slide tree (a shape in a group does not move)
/// (`arrangeShape`).
pub fn arrange_shape(store: &mut PartStore, slide: &str, id: &str, mode: ArrangeMode) -> E<bool> {
    let Some(xml) = store.text(slide).filter(|x| !x.is_empty()) else { return Ok(false) };
    let Some(r) = shape_ranges(&xml, id).into_iter().next() else { return Ok(false) };
    let o = outer_range(&xml, &r)?;
    let tree = find_first(&xml, "p:spTree").ok_or(js("p:spTree"))?;
    let kids: Vec<NamedRange> =
        child_ranges(&xml, &tree).into_iter().filter(|k| !["p:nvGrpSpPr", "p:grpSpPr", "p:extLst"].contains(&k.name.as_str())).collect();
    let Some(i) = kids.iter().position(|k| k.range.start == o.start) else { return Ok(false) };
    let last = kids.len() - 1;
    let j = match mode {
        ArrangeMode::Front => last,
        ArrangeMode::Back => 0,
        ArrangeMode::Forward => last.min(i + 1),
        ArrangeMode::Backward => i.saturating_sub(1),
    };
    if i == j {
        return Ok(false);
    }
    let mut texts: Vec<String> = kids.iter().map(|k| sl(&xml, k.range.start, k.range.end).to_owned()).collect();
    let moved = texts.remove(i);
    texts.insert(j, moved);
    // the sequence of siblings is contiguous in the text (only white space between them, if any)
    let out = splice(&xml, kids[0].range.start, kids[last].range.end, &texts.concat());
    store.set_text(slide, &out);
    Ok(true)
}

/// `tag` + word boundary at `at`: `\b` after a name ending in a word character.
fn boundary_after(b: &[u8], at: usize) -> bool {
    !b.get(at).is_some_and(|&c| is_js_word(c))
}

/// `new RegExp('<p:spTgt\\b[^>]*\\sspid="ID"').test(s)`.
fn has_target(s: &str, id: &str) -> bool {
    let b = s.as_bytes();
    let lit = format!("spid=\"{id}\"");
    let name = "<p:spTgt";
    let mut from = 0;
    while let Some(p) = s.get(from..).and_then(|x| x.find(name)).map(|p| from + p) {
        from = p + 1;
        let after = p + name.len();
        if !boundary_after(b, after) {
            continue;
        }
        let gt = b[after..].iter().position(|&c| c == b'>').map_or(b.len(), |g| after + g);
        let mut q = after;
        while q < gt {
            let wl = js_space_len(b, q);
            if wl > 0 && s.get(q + wl..).is_some_and(|r| r.starts_with(&lit)) {
                return true;
            }
            q += s[q..].chars().next().map_or(1, char::len_utf8);
        }
    }
    false
}

/// `new RegExp('\\sspid="ID"').test(s)`.
fn has_spid(s: &str, id: &str) -> bool {
    let b = s.as_bytes();
    let lit = format!("spid=\"{id}\"");
    let mut q = 0;
    while q < b.len() {
        let wl = js_space_len(b, q);
        if wl > 0 && s.get(q + wl..).is_some_and(|r| r.starts_with(&lit)) {
            return true;
        }
        q += s[q..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// `/^<p:par>\s*<p:cTn\b[^>]*\spresetClass=/.test(s)`.
fn par_has_preset_class(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("<p:par>") else { return false };
    let b = rest.as_bytes();
    let mut i = 0;
    while let n @ 1.. = js_space_len(b, i) {
        i += n;
    }
    let Some(r2) = rest.get(i..).and_then(|r| r.strip_prefix("<p:cTn")) else { return false };
    let b2 = r2.as_bytes();
    if !boundary_after(b2, 0) {
        return false;
    }
    let gt = b2.iter().position(|&c| c == b'>').unwrap_or(b2.len());
    let mut q = 0;
    while q < gt {
        let wl = js_space_len(b2, q);
        if wl > 0 && r2.get(q + wl..).is_some_and(|r| r.starts_with("presetClass=")) {
            return true;
        }
        q += r2[q..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Removes, one at a time (the innermost first), the first `tag` that satisfies `pred`.
fn drop_where(t: &mut String, tag: &str, pred: &dyn Fn(&str) -> bool) {
    loop {
        let mut all = find_all_elements(t, tag, 0, t.len());
        all.sort_by_key(|r| r.end - r.start); // stable: ties stay in document order
        let hit = all.iter().find(|r| pred(sl(t, r.start, r.end))).copied();
        match hit {
            Some(r) => *t = splice(t, r.start, r.end, ""),
            None => return,
        }
    }
}

/// Removes from `<p:timing>` the animations that target the shape (`dropAnimationsOf`). The id is
/// matched literally (the TS interpolates it into a `RegExp`; ids are decimal).
fn drop_animations_of(xml: &str, id: &str) -> String {
    let Some(timing) = find_first(xml, "p:timing") else { return xml.to_owned() };
    let mut t = sl(xml, timing.start, timing.end).to_owned();
    if !has_target(&t, id) {
        return xml.to_owned();
    }
    drop_where(&mut t, "p:par", &|s| par_has_preset_class(s) && has_target(s, id));
    drop_where(&mut t, "p:video", &|s| has_target(s, id));
    drop_where(&mut t, "p:audio", &|s| has_target(s, id));
    drop_where(&mut t, "p:seq", &|s| !s.contains("nodeType=\"mainSeq\"") && has_target(s, id));
    for tag in ["p:bldP", "p:bldDgm", "p:bldOleChart", "p:bldGraphic"] {
        drop_where(&mut t, tag, &|s| has_spid(s, id));
    }
    let any_effect = |s: &str| {
        s.contains("presetClass=")
            || s.contains("<p:video")
            || s.contains("<p:audio")
            || s.contains("<p:cmd")
            || s.contains("nodeType=\"mainSeq\"")
            || s.contains("nodeType=\"tmRoot\"")
    };
    drop_where(&mut t, "p:par", &|s| !any_effect(s));
    if !(t.contains("presetClass=") || t.contains("<p:video") || t.contains("<p:audio")) {
        t.clear();
    }
    splice(xml, timing.start, timing.end, &t)
}

/// Deletes the shape (and its animations) (`deleteShape`).
pub fn delete_shape(store: &mut PartStore, slide: &str, id: &str) -> E<bool> {
    let Some(mut xml) = store.text(slide).filter(|x| !x.is_empty()) else { return Ok(false) };
    let rs = shape_ranges(&xml, id);
    if rs.is_empty() {
        return Ok(false);
    }
    let mut outs: Vec<XmlRange> = Vec::new();
    for r in &rs {
        let o = outer_range(&xml, r)?;
        if !outs.iter().any(|x| x.start == o.start) {
            outs.push(o);
        }
    }
    outs.sort_by_key(|o| std::cmp::Reverse(o.start));
    for o in outs {
        xml = splice(&xml, o.start, o.end, "");
    }
    xml = drop_animations_of(&xml, id);
    store.set_text(slide, &xml);
    Ok(true)
}

// ───────────────────────── text of the shape ─────────────────────────

/// Plain text of an `<a:p>` (runs, fields, line breaks as `\u{b}`) (`paragraphText`).
fn paragraph_text(xml: &str, p: &XmlRange) -> String {
    let mut s = String::new();
    for k in child_ranges(xml, p) {
        if k.name == "a:r" || k.name == "a:fld" {
            if let Some(t) = find_element(xml, "a:t", k.range.start, k.range.end) {
                s.push_str(&unescape_xml(inner_xml(xml, &t)));
            }
        } else if k.name == "a:br" {
            s.push('\u{b}');
        }
    }
    s
}

/// `rPr` without the hyperlink (`stripLink`): `/<a:hlink(Click|MouseOver)\b[^>]*?(\/>|>[\s\S]*?<\/a:hlink\1>)/g` removed.
fn strip_link(r_pr: &str) -> String {
    let b = r_pr.as_bytes();
    let mut out = String::new();
    let mut at = 0;
    let mut from = 0;
    while let Some(p) = r_pr.get(from..).and_then(|x| x.find("<a:hlink")).map(|p| from + p) {
        let after_hlink = p + "<a:hlink".len();
        let kind = if r_pr[after_hlink..].starts_with("Click") {
            "Click"
        } else if r_pr[after_hlink..].starts_with("MouseOver") {
            "MouseOver"
        } else {
            from = p + 1;
            continue;
        };
        let name_end = after_hlink + kind.len();
        if !boundary_after(b, name_end) {
            from = p + 1;
            continue;
        }
        // lazy `[^>]*?` then `/>` or `>...</a:hlinkKIND>`
        let close = format!("</a:hlink{kind}>");
        let mut q = name_end;
        let mut end = None;
        while q < b.len() {
            if b[q..].starts_with(b"/>") {
                end = Some(q + 2);
                break;
            }
            if b[q] == b'>' {
                end = r_pr[q + 1..].find(&close).map(|c| q + 1 + c + close.len());
                break;
            }
            q += r_pr[q..].chars().next().map_or(1, char::len_utf8);
        }
        match end {
            Some(e) => {
                out.push_str(&r_pr[at..p]);
                at = e;
                from = e;
            }
            None => from = p + 1,
        }
    }
    out.push_str(&r_pr[at..]);
    out
}

/// `s.replace(/^<from/, '<to')`.
fn swap_prefix(s: &str, from: &str, to: &str) -> String {
    match s.strip_prefix(from) {
        Some(rest) => format!("{to}{rest}"),
        None => s.to_owned(),
    }
}

/// `s.replace(/<\/from>$/, '</to>')`.
fn swap_suffix(s: &str, from: &str, to: &str) -> String {
    match s.strip_suffix(from) {
        Some(rest) => format!("{rest}{to}"),
        None => s.to_owned(),
    }
}

/// Replaces the text of the shape, one line per paragraph. A paragraph whose text did not change stays
/// AS IT WAS (runs and formatting); a changed one keeps the `pPr` and the `rPr` of the paragraph with
/// the same index (or the last) (`setShapeText`).
pub fn set_shape_text(store: &mut PartStore, slide: &str, id: &str, text: &str, lang: &str) -> E<bool> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    edit_shape(store, slide, id, |mut xml, r| {
        let mut body = find_element(&xml, "p:txBody", r.start, r.end);
        if body.is_none() {
            if element_name(&xml, &r) != "p:sp" {
                return Ok(xml);
            }
            xml = insert_child(&xml, &r, "<p:txBody><a:bodyPr/><a:lstStyle/></p:txBody>", &["p:extLst"]);
            body = Some(find_from(&xml, "p:txBody", r.start).ok_or(js("p:txBody"))?);
        }
        let body = body.ok_or(js("p:txBody"))?;
        let kids = child_ranges(&xml, &body);
        let paras: Vec<&NamedRange> = kids.iter().filter(|k| k.name == "a:p").collect();
        struct OldPara {
            xml: String,
            text: String,
            range: XmlRange,
        }
        let old: Vec<OldPara> = paras
            .iter()
            .map(|p| OldPara {
                xml: sl(&xml, p.range.start, p.range.end).to_owned(),
                text: paragraph_text(&xml, &p.range).replace('\u{b}', "\n"),
                range: p.range,
            })
            .collect();
        let mut out: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let same = old.get(i);
            if let Some(s) = same
                && s.text == *line
            {
                out.push(s.xml.clone());
                continue;
            }
            let r#ref = same.or(old.last());
            let mut p_pr = String::new();
            let mut r_pr = format!("<a:rPr lang=\"{lang}\" dirty=\"0\"/>");
            let mut end = format!("<a:endParaRPr lang=\"{lang}\" dirty=\"0\"/>");
            if let Some(rf) = r#ref {
                if let Some(pp) = find_element(&xml, "a:pPr", rf.range.start, rf.range.end) {
                    p_pr = sl(&xml, pp.start, pp.end).to_owned();
                }
                let run = find_element(&xml, "a:r", rf.range.start, rf.range.end)
                    .or_else(|| find_element(&xml, "a:fld", rf.range.start, rf.range.end));
                let rp = run.and_then(|run| find_element(&xml, "a:rPr", run.start, run.end));
                let ep = find_element(&xml, "a:endParaRPr", rf.range.start, rf.range.end);
                if let Some(rp) = rp {
                    r_pr = strip_link(sl(&xml, rp.start, rp.end));
                } else if let Some(ep) = ep {
                    r_pr = swap_suffix(&swap_prefix(sl(&xml, ep.start, ep.end), "<a:endParaRPr", "<a:rPr"), "</a:endParaRPr>", "</a:rPr>");
                }
                if let Some(ep) = ep {
                    end = sl(&xml, ep.start, ep.end).to_owned();
                } else if rp.is_some() {
                    end = swap_suffix(&swap_prefix(&r_pr, "<a:rPr", "<a:endParaRPr"), "</a:rPr>", "</a:endParaRPr>");
                }
            }
            let runs = if line.is_empty() { String::new() } else { format!("<a:r>{r_pr}<a:t>{}</a:t></a:r>", escape_text(line)) };
            out.push(format!("<a:p>{p_pr}{runs}{end}</a:p>"));
        }
        // the new content takes the place of the old paragraphs (bodyPr/lstStyle and extLst stay)
        if let (Some(first), Some(last)) = (paras.first(), paras.last()) {
            return Ok(splice(&xml, first.range.start, last.range.end, &out.concat()));
        }
        Ok(insert_child(&xml, &body, &out.concat(), &["a:extLst"]))
    })
}

/// `RunFormat`: character formatting applied to the WHOLE text of the shape.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RunFormat {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// points
    pub size: Option<f64>,
    pub font: Option<String>,
    /// `#RRGGBB`
    pub color: Option<String>,
}

const RPR_ORDER: [&str; 23] = [
    "a:ln",
    "a:noFill",
    "a:solidFill",
    "a:gradFill",
    "a:blipFill",
    "a:pattFill",
    "a:grpFill",
    "a:effectLst",
    "a:effectDag",
    "a:highlight",
    "a:uLnTx",
    "a:uLn",
    "a:uFillTx",
    "a:uFill",
    "a:latin",
    "a:ea",
    "a:cs",
    "a:sym",
    "a:hlinkClick",
    "a:hlinkMouseOver",
    "a:rtl",
    "a:extLst",
    "",
];

fn after(tag: &str) -> &'static [&'static str] {
    let order = &RPR_ORDER[..22];
    match order.iter().position(|t| *t == tag) {
        Some(i) => &order[i + 1..],
        None => order,
    }
}

fn whole(x: &str) -> XmlRange {
    XmlRange { start: 0, end: x.len(), open_end: x.len(), self_closing: false }
}

/// Re-finds the element after an edit: `findElement(x, elementName(x, r))!`.
fn refind(x: &str, r: &XmlRange) -> E<XmlRange> {
    find_first(x, &element_name(x, r)).ok_or(js("element"))
}

/// `formatRPr`: applies the formatting to ONE rPr (the text of the element); returns the new text.
fn format_r_pr(r_pr: &str, f: &RunFormat) -> E<String> {
    let mut x = r_pr.to_owned();
    let mut r = find_first(&x, &element_name(&x, &whole(&x))).ok_or(js("rPr"))?;
    macro_rules! set {
        ($attr:expr, $v:expr) => {{
            x = set_attr(&x, &r, $attr, $v);
            r = refind(&x, &r)?;
        }};
    }
    if let Some(b) = f.bold {
        set!("b", Some(if b { "1" } else { "0" }));
    }
    if let Some(b) = f.italic {
        set!("i", Some(if b { "1" } else { "0" }));
    }
    if let Some(b) = f.underline {
        set!("u", Some(if b { "sng" } else { "none" }));
    }
    if let Some(b) = f.strike {
        set!("strike", Some(if b { "sngStrike" } else { "noStrike" }));
    }
    if let Some(sz) = f.size {
        let v = fmt(jround(jmax(1.0, jmin(4000.0, sz)) * 100.0));
        set!("sz", Some(&v));
    }
    if let Some(color) = &f.color {
        if r.self_closing {
            x = set_inner(&x, &r, "");
            r = refind(&x, &r)?;
        }
        x = replace_fill(x, &r, &solid(color), after("a:grpFill"))?;
        r = refind(&x, &r)?;
    }
    if let Some(font) = &f.font {
        if r.self_closing {
            x = set_inner(&x, &r, "");
            r = refind(&x, &r)?;
        }
        for tag in ["a:latin", "a:cs"] {
            let ex = child_ranges(&x, &r).into_iter().find(|k| k.name == tag);
            let el = format!("<{tag} typeface=\"{}\"/>", escape_attr(font));
            x = match ex {
                Some(ex) => splice(&x, ex.range.start, ex.range.end, &el),
                None => insert_child(&x, &r, &el, after(tag)),
            };
            r = refind(&x, &r)?;
        }
    }
    Ok(x)
}

/// Each rPr/endParaRPr of the shape paragraphs (runs without an rPr get one) (`eachRunProps`).
fn each_run_props(mut xml: String, body: &XmlRange, lang: &str, f: &mut dyn FnMut(&str) -> E<String>) -> E<String> {
    let paras: Vec<NamedRange> = child_ranges(&xml, body).into_iter().filter(|k| k.name == "a:p").rev().collect();
    for p in paras {
        let kids: Vec<NamedRange> = child_ranges(&xml, &p.range).into_iter().rev().collect();
        for k in &kids {
            if k.name == "a:r" || k.name == "a:fld" || k.name == "a:br" {
                let rp = child_ranges(&xml, &k.range).into_iter().find(|c| c.name == "a:rPr");
                if let Some(rp) = rp {
                    let new = f(sl(&xml, rp.range.start, rp.range.end))?;
                    xml = splice(&xml, rp.range.start, rp.range.end, &new);
                } else {
                    let new = f(&format!("<a:rPr lang=\"{lang}\" dirty=\"0\"/>"))?;
                    if k.range.self_closing {
                        xml = set_inner(&xml, &k.range, &new);
                        continue;
                    }
                    xml = splice(&xml, k.range.open_end, k.range.open_end, &new);
                }
            } else if k.name == "a:endParaRPr" {
                let new = f(sl(&xml, k.range.start, k.range.end))?;
                xml = splice(&xml, k.range.start, k.range.end, &new);
            }
        }
        // an empty paragraph without endParaRPr gets one (the size applies to text typed later)
        if !kids.iter().any(|k| k.name == "a:endParaRPr" || k.name == "a:r") {
            let pr = find_from(&xml, "a:p", p.range.start).ok_or(js("a:p"))?;
            let new = f(&format!("<a:endParaRPr lang=\"{lang}\" dirty=\"0\"/>"))?;
            xml = insert_child(&xml, &pr, &new, &[]);
        }
    }
    Ok(xml)
}

/// Character formatting on ALL the text of the shape (`formatShapeText`).
pub fn format_shape_text(store: &mut PartStore, slide: &str, id: &str, f: &RunFormat, lang: &str) -> E<bool> {
    edit_shape(store, slide, id, |xml, r| match find_element(&xml, "p:txBody", r.start, r.end) {
        Some(body) => each_run_props(xml, &body, lang, &mut |rp| format_r_pr(rp, f)),
        None => Ok(xml),
    })
}

/// `/\ssz="(\d+)"/.exec(s)?.[1]`.
fn find_sz(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut q = 0;
    while q < b.len() {
        let wl = js_space_len(b, q);
        if wl > 0 && s[q + wl..].starts_with("sz=\"") {
            let ds = q + wl + 4;
            let mut de = ds;
            while b.get(de).is_some_and(u8::is_ascii_digit) {
                de += 1;
            }
            if de > ds && b.get(de) == Some(&b'"') {
                return Some(&s[ds..de]);
            }
        }
        q += s[q..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// Text size of the shape scaled: `sz = sizes(current)` per run (`scaleShapeText`).
pub fn scale_shape_text(store: &mut PartStore, slide: &str, id: &str, sizes: &mut dyn FnMut(Option<f64>) -> f64, lang: &str) -> E<bool> {
    edit_shape(store, slide, id, |xml, r| match find_element(&xml, "p:txBody", r.start, r.end) {
        Some(body) => each_run_props(xml, &body, lang, &mut |rp| {
            let head = match rp.find('>') {
                Some(i) => &rp[..=i],
                None => "",
            };
            let cur = find_sz(head).map(|d| uo_jsnum::parse::to_number(d) / 100.0);
            format_r_pr(rp, &RunFormat { size: Some(sizes(cur)), ..RunFormat::default() })
        }),
        None => Ok(xml),
    })
}

/// Removes the direct character formatting (bold, italic, size, colour, font...) (`clearShapeFormatting`).
pub fn clear_shape_formatting(store: &mut PartStore, slide: &str, id: &str, lang: &str) -> E<bool> {
    const DROP: [&str; 14] = [
        "a:noFill",
        "a:solidFill",
        "a:gradFill",
        "a:blipFill",
        "a:pattFill",
        "a:grpFill",
        "a:latin",
        "a:ea",
        "a:cs",
        "a:ln",
        "a:effectLst",
        "a:highlight",
        "a:uLn",
        "a:uFill",
    ];
    edit_shape(store, slide, id, |xml, r| match find_element(&xml, "p:txBody", r.start, r.end) {
        Some(body) => each_run_props(xml, &body, lang, &mut |rp| {
            let mut x = rp.to_owned();
            let mut el = find_first(&x, &element_name(&x, &whole(&x))).ok_or(js("rPr"))?;
            for a in ["b", "i", "u", "strike", "sz", "spc", "cap", "baseline", "kern"] {
                x = set_attr(&x, &el, a, None);
                el = refind(&x, &el)?;
            }
            for k in child_ranges(&x, &el).iter().rev() {
                if DROP.contains(&k.name.as_str()) {
                    x = splice(&x, k.range.start, k.range.end, "");
                }
            }
            Ok(x)
        }),
        None => Ok(xml),
    })
}

/// Each `<a:pPr>` of the shape paragraphs (creates what is missing) (`eachParaProps`).
fn each_para_props(mut xml: String, body: &XmlRange, f: &mut dyn FnMut(&str) -> E<String>) -> E<String> {
    let paras: Vec<NamedRange> = child_ranges(&xml, body).into_iter().filter(|k| k.name == "a:p").rev().collect();
    for p in paras {
        let pp = child_ranges(&xml, &p.range).into_iter().find(|k| k.name == "a:pPr");
        if let Some(pp) = pp {
            let new = f(sl(&xml, pp.range.start, pp.range.end))?;
            xml = splice(&xml, pp.range.start, pp.range.end, &new);
        } else if p.range.self_closing {
            let new = f("<a:pPr/>")?;
            xml = set_inner(&xml, &p.range, &new);
        } else {
            let new = f("<a:pPr/>")?;
            xml = splice(&xml, p.range.open_end, p.range.open_end, &new);
        }
    }
    Ok(xml)
}

/// Paragraph alignment (`l` | `ctr` | `r` | `just`) (`alignShapeText`).
pub fn align_shape_text(store: &mut PartStore, slide: &str, id: &str, algn: &str) -> E<bool> {
    edit_shape(store, slide, id, |xml, r| match find_element(&xml, "p:txBody", r.start, r.end) {
        Some(body) => each_para_props(xml, &body, &mut |pp| {
            let cut = pp.find('>').map_or(0, |i| i + 1);
            Ok(format!("{}{}", set_attr_in_tag(&pp[..cut], "algn", Some(algn)), &pp[cut..]))
        }),
        None => Ok(xml),
    })
}

/// `kind` of bullets (`setShapeBullets`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BulletKind {
    Bullet,
    Number,
    None,
}

const BULLET_TAGS: [&str; 6] = ["a:buFontTx", "a:buFont", "a:buNone", "a:buAutoNum", "a:buChar", "a:buBlip"];
const PPR_AFTER_BULLET: [&str; 3] = ["a:tabLst", "a:defRPr", "a:extLst"];

/// Bullets of the paragraphs: with bullet, numbered or none.
pub fn set_shape_bullets(store: &mut PartStore, slide: &str, id: &str, kind: BulletKind) -> E<bool> {
    edit_shape(store, slide, id, |xml, r| {
        let Some(body) = find_element(&xml, "p:txBody", r.start, r.end) else { return Ok(xml) };
        each_para_props(xml, &body, &mut |pp| {
            let mut x = pp.to_owned();
            let mut el = find_first(&x, "a:pPr").ok_or(js("a:pPr"))?;
            if el.self_closing {
                x = set_inner(&x, &el, "");
                el = find_first(&x, "a:pPr").ok_or(js("a:pPr"))?;
            }
            for k in child_ranges(&x, &el).iter().rev() {
                if BULLET_TAGS.contains(&k.name.as_str()) {
                    x = splice(&x, k.range.start, k.range.end, "");
                }
            }
            el = find_first(&x, "a:pPr").ok_or(js("a:pPr"))?;
            let lvl = get_attr(&x, &el, "lvl").map_or(0.0, |s| num_or0(&s));
            let lvl = if lvl.is_nan() { 0.0 } else { lvl };
            if kind == BulletKind::None {
                x = insert_child(&x, &el, "<a:buNone/>", &PPR_AFTER_BULLET);
                el = find_first(&x, "a:pPr").ok_or(js("a:pPr"))?;
                x = set_attr(&x, &el, "indent", Some("0"));
                el = find_first(&x, "a:pPr").ok_or(js("a:pPr"))?;
                return Ok(set_attr(&x, &el, "marL", Some(&fmt(lvl * 457_200.0))));
            }
            let bu = if kind == BulletKind::Bullet {
                "<a:buFont typeface=\"Arial\" panose=\"020B0604020202020204\" pitchFamily=\"34\" charset=\"0\"/><a:buChar char=\"\u{2022}\"/>"
            } else {
                "<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"arabicPeriod\"/>"
            };
            x = insert_child(&x, &el, bu, &PPR_AFTER_BULLET);
            el = find_first(&x, "a:pPr").ok_or(js("a:pPr"))?;
            // bullet indent (a text box has marL=0: the bullet would cover the text)
            let mar_l = get_attr(&x, &el, "marL").map_or(0.0, |s| uo_jsnum::parse::to_number(&s));
            if mar_l.is_nan() || mar_l <= 0.0 {
                x = set_attr(&x, &el, "marL", Some(&fmt(lvl * 457_200.0 + 342_900.0)));
                el = find_first(&x, "a:pPr").ok_or(js("a:pPr"))?;
                x = set_attr(&x, &el, "indent", Some("-342900"));
            }
            Ok(x)
        })
    })
}

/// Hyperlink on all the text of the shape (`None` removes) (`setShapeLink`).
pub fn set_shape_link(store: &mut PartStore, slide: &str, id: &str, url: Option<&str>, lang: &str) -> E<bool> {
    let Some(xml0) = store.text(slide).filter(|x| !x.is_empty()) else { return Ok(false) };
    if shape_ranges(&xml0, id).is_empty() {
        return Ok(false);
    }
    let url = truthy(url);
    let r_id = match url {
        Some(u) => add_rel(store, slide, rel::HYPERLINK, u, true)?,
        None => String::new(),
    };
    edit_shape(store, slide, id, |xml, r| {
        let Some(body) = find_element(&xml, "p:txBody", r.start, r.end) else { return Ok(xml) };
        each_run_props(xml, &body, lang, &mut |rp| {
            if rp.starts_with("<a:endParaRPr") {
                return Ok(rp.to_owned());
            }
            let mut x = strip_link(rp);
            if url.is_none() {
                return Ok(x);
            }
            let mut el = find_first(&x, "a:rPr").ok_or(js("a:rPr"))?;
            if el.self_closing {
                x = set_inner(&x, &el, "");
                el = find_first(&x, "a:rPr").ok_or(js("a:rPr"))?;
            }
            Ok(insert_child(&x, &el, &format!("<a:hlinkClick r:id=\"{r_id}\"/>"), after("a:hlinkClick")))
        })
    })
}

/// The `i`-flag canonical form of a UTF-16 unit (the `RegExp` `Canonicalize`).
fn canon(u: u16) -> u16 {
    uo_text::case::regexp_canonicalize(u)
}

/// Non-overlapping matches of `query` in `text` (UTF-16 units, `gi` or `g` flags of an escaped pattern).
fn find_matches(text: &[u16], query: &[u16], match_case: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let n = query.len();
    let mut i = 0;
    while n > 0 && i + n <= text.len() {
        let hit = (0..n).all(|k| if match_case { text[i + k] == query[k] } else { canon(text[i + k]) == canon(query[k]) });
        if hit {
            out.push((i, i + n));
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

/// Replaces `query` by `repl` in the text of the shape, INSIDE each `<a:t>` (what spans runs of
/// different formatting does not match). Returns the number of replacements (`replaceInShape`).
pub fn replace_in_shape(store: &mut PartStore, slide: &str, id: &str, query: &str, repl: &str, match_case: bool) -> E<f64> {
    if query.is_empty() {
        return Ok(0.0);
    }
    let q: Vec<u16> = query.encode_utf16().collect();
    let mut count = 0.0;
    edit_shape(store, slide, id, |mut xml, r| {
        let Some(body) = find_element(&xml, "p:txBody", r.start, r.end) else { return Ok(xml) };
        for t in find_elements(&xml, "a:t", body.open_end, body.end).iter().rev() {
            let text = unescape_xml(inner_xml(&xml, t));
            let units: Vec<u16> = text.encode_utf16().collect();
            let ms = find_matches(&units, &q, match_case);
            if ms.is_empty() {
                continue;
            }
            let mut next: Vec<u16> = Vec::new();
            let mut at = 0;
            for (s, e) in &ms {
                next.extend_from_slice(&units[at..*s]);
                next.extend(repl.encode_utf16());
                at = *e;
                count += 1.0;
            }
            next.extend_from_slice(&units[at..]);
            let next = String::from_utf16_lossy(&next);
            if next != text {
                xml = set_inner(&xml, t, &escape_text(&next));
            }
        }
        Ok(xml)
    })?;
    Ok(count)
}

/// `str.trim()` re-export for the notes/properties code.
pub fn trim(s: &str) -> &str {
    js_trim(s)
}

/// `xml.slice(a)` re-export.
pub fn tail(s: &str, a: usize) -> &str {
    sl_from(s, a)
}

/// `PptxManipulate.SetRunText` (the C# `UniversalOffice.Pptx`, `Manipulate.cs`): replaces the text of the
/// `run_index`-th `<a:t>` (0-based, document order) of the slide part with minimal surgery: only the content
/// of the target `<a:t>` changes (a self-closing `<a:t/>` counts as an empty run and becomes `<a:t>text</a:t>`);
/// the rest of the XML and every other part flow byte for byte. Errors where the C# throws: a missing part,
/// fewer runs than the index, a malformed `<a:t`.
pub fn set_run_text(store: &mut PartStore, slide: &str, run_index: usize, new_text: &str) -> E<()> {
    let xml = store.text(slide).ok_or_else(|| EditError::Missing(slide.into()))?;
    let b = xml.as_bytes();
    let mut seen = 0usize;
    let mut i = 0usize;
    loop {
        let Some(open) = xml.get(i..).and_then(|s| s.find("<a:t")).map(|p| i + p) else {
            return Err(EditError::Js("the slide has fewer <a:t> runs than the index"));
        };
        let after_name = open + 4;
        // exactly `<a:t>` or `<a:t ...>` (not `<a:tab>`): the next character is `>`, a space or `/`
        let next = b.get(after_name).copied().unwrap_or(0);
        if next != b'>' && next != b' ' && next != b'/' {
            i = after_name;
            continue;
        }
        let tag_end = open + xml[open..].find('>').ok_or(EditError::Js("malformed slide (<a:t without >)"))?;
        if b.get(tag_end - 1) == Some(&b'/') {
            if seen == run_index {
                let out = format!("{}<a:t>{}</a:t>{}", &xml[..open], escape_text(new_text), &xml[tag_end + 1..]);
                store.set_text(slide, &out);
                return Ok(());
            }
            seen += 1;
            i = tag_end + 1;
            continue;
        }
        let close = tag_end + xml[tag_end..].find("</a:t>").ok_or(EditError::Js("malformed slide (</a:t> missing)"))?;
        if seen == run_index {
            let out = format!("{}{}{}", &xml[..=tag_end], escape_text(new_text), &xml[close..]);
            store.set_text(slide, &out);
            return Ok(());
        }
        seen += 1;
        i = close + 6;
    }
}
