//! `deck-edit.ts` part 3: notes, document properties, "Set up show" and the shape clipboard (lines
//! 1031-1304 of the TS).

use std::sync::Arc;

use rustc_hash::FxHashSet;
use uo_jsnum::parse::to_number;
use uo_jsnum::text::is_js_word;

use super::blank::{notes_master, theme};
use super::pkg::{
    E, EditError, NS_DECL, PRES, XML_DECL, add_override, add_rel, content_type_of, ct, ensure_default, fmt, next_part_name, read_rel_list,
    rel, relative_target, remove_rels, resolve_target, set_rel_target,
};
use super::shapes::{Frame, is_top_level, max_shape_id, must_text, set_shape_text, shape_ranges};
use super::xml::{
    child_ranges, escape_text, find_all_elements, find_element, find_elements, find_first, get_attr, inner_xml, insert_child, js_trim,
    set_attr, set_inner, sl, splice, unescape_xml,
};
use crate::package::PartStore;

fn js(w: &'static str) -> EditError {
    EditError::Js(w)
}

// ───────────────────────── notes ─────────────────────────

/// The presentation has a notes master (a notes page needs it) (`hasNotesMaster`).
pub fn has_notes_master(store: &PartStore) -> bool {
    read_rel_list(store, PRES).iter().any(|r| r.type_ == rel::NOTES_MASTER && store.bytes(&resolve_target(PRES, &r.target)).is_some())
}

const AFTER_NOTES_MASTER_LIST: [&str; 13] = [
    "p:handoutMasterIdLst",
    "p:sldIdLst",
    "p:sldSz",
    "p:notesSz",
    "p:smartTags",
    "p:embeddedFontLst",
    "p:custShowLst",
    "p:photoAlbum",
    "p:custDataLst",
    "p:kinsoku",
    "p:defaultTextStyle",
    "p:modifyVerifier",
    "p:extLst",
];

/// The notes master of the presentation; without it, creates the Office default (with its own theme),
/// as PowerPoint does on the first note. Returns the path (`ensureNotesMaster`).
pub fn ensure_notes_master(store: &mut PartStore) -> E<String> {
    let existing = read_rel_list(store, PRES)
        .into_iter()
        .find(|r| r.type_ == rel::NOTES_MASTER && store.bytes(&resolve_target(PRES, &r.target)).is_some());
    if let Some(e) = existing {
        return Ok(resolve_target(PRES, &e.target));
    }
    let none = FxHashSet::default();
    let theme_path = next_part_name(store, "ppt/theme", "theme", "xml", &none);
    store.set_text(&theme_path, &theme("Office Theme"));
    add_override(store, &theme_path, ct::THEME)?;
    let master_path = next_part_name(store, "ppt/notesMasters", "notesMaster", "xml", &none);
    store.set_text(&master_path, &notes_master());
    add_rel(store, &master_path, rel::THEME, &relative_target(&master_path, &theme_path), false)?;
    add_override(store, &master_path, ct::NOTES_MASTER)?;
    // a broken reference (list pointing to an absent part) gives way to the new one
    let stale: FxHashSet<String> = read_rel_list(store, PRES).into_iter().filter(|r| r.type_ == rel::NOTES_MASTER).map(|r| r.id).collect();
    if !stale.is_empty() {
        remove_rels(store, PRES, &stale);
    }
    let r_id = add_rel(store, PRES, rel::NOTES_MASTER, &relative_target(PRES, &master_path), false)?;
    let mut xml = super::pkg::presentation_xml(store)?;
    let entry = format!("<p:notesMasterId r:id=\"{r_id}\"/>");
    match find_first(&xml, "p:notesMasterIdLst") {
        Some(lst) => xml = set_inner(&xml, &lst, &entry),
        None => {
            let root = find_first(&xml, "p:presentation").ok_or(js("p:presentation"))?;
            xml = insert_child(&xml, &root, &format!("<p:notesMasterIdLst>{entry}</p:notesMasterIdLst>"), &AFTER_NOTES_MASTER_LIST);
        }
    }
    if find_first(&xml, "p:notesSz").is_none() {
        let root = find_first(&xml, "p:presentation").ok_or(js("p:presentation"))?;
        xml = insert_child(&xml, &root, "<p:notesSz cx=\"6858000\" cy=\"9144000\"/>", &AFTER_NOTES_MASTER_LIST[4..]);
    }
    store.set_text(PRES, &xml);
    Ok(master_path)
}

/// Writes the notes of the slide (creates the notes page and, if missing, the notes master)
/// (`setSlideNotes`).
pub fn set_slide_notes(store: &mut PartStore, slide: &str, text: &str, lang: &str) -> E<bool> {
    let rel_ = read_rel_list(store, slide).into_iter().find(|r| r.type_ == rel::NOTES_SLIDE);
    let mut notes_path = rel_.as_ref().map(|r| resolve_target(slide, &r.target));
    if notes_path.as_deref().is_none_or(|p| store.bytes(p).is_none()) {
        let master_path = ensure_notes_master(store)?;
        let np = next_part_name(store, "ppt/notesSlides", "notesSlide", "xml", &FxHashSet::default());
        store.set_text(
            &np,
            &format!(
                "{XML_DECL}<p:notes {NS_DECL}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"
            ),
        );
        add_rel(store, &np, rel::NOTES_MASTER, &relative_target(&np, &master_path), false)?;
        add_rel(store, &np, rel::SLIDE, &relative_target(&np, slide), false)?;
        add_override(store, &np, ct::NOTES_SLIDE)?;
        match &rel_ {
            Some(r) => set_rel_target(store, slide, &r.id, &relative_target(slide, &np)),
            None => {
                add_rel(store, slide, rel::NOTES_SLIDE, &relative_target(slide, &np), false)?;
            }
        }
        notes_path = Some(np);
    }
    let notes_path = notes_path.ok_or(js("notes path"))?;
    let mut xml = must_text(store, &notes_path)?;
    let Some(tree) = find_first(&xml, "p:spTree") else { return Ok(false) };
    let is_body = |xml: &str, sp: &super::xml::XmlRange| {
        find_element(xml, "p:ph", sp.start, sp.end).is_some_and(|ph| get_attr(xml, &ph, "type").as_deref() == Some("body"))
    };
    let mut body = find_elements(&xml, "p:sp", tree.open_end, tree.end).into_iter().find(|sp| is_body(&xml, sp));
    if body.is_none() {
        let id = max_shape_id(&xml) + 1.0;
        let tree = find_first(&xml, "p:spTree").ok_or(js("p:spTree"))?;
        xml = insert_child(
            &xml,
            &tree,
            &format!(
                "<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"Notes Placeholder {}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p></p:txBody></p:sp>",
                fmt(id),
                fmt(id - 1.0)
            ),
            &["p:extLst"],
        );
        store.set_text(&notes_path, &xml);
        let t2 = find_first(&xml, "p:spTree").ok_or(js("p:spTree"))?;
        body = find_elements(&xml, "p:sp", t2.open_end, t2.end).pop();
    }
    let body = body.ok_or(js("notes body"))?;
    let id = find_element(&xml, "p:cNvPr", body.start, body.end).and_then(|c| get_attr(&xml, &c, "id")).unwrap_or_default();
    // the notes page is a "slide" for the text edit (the same `p:spTree` tree)
    set_shape_text(store, &notes_path, &id, text, lang)
}

// ───────────────────────── document properties ─────────────────────────

/// `DocProperties`: a field that is `Some` is written (`Some("")` removes the element).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DocProperties {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub author: Option<String>,
    pub keywords: Option<String>,
    pub description: Option<String>,
    pub last_modified_by: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
}

impl DocProperties {
    fn fields(&self) -> [(&Option<String>, &'static str); 8] {
        [
            (&self.title, "dc:title"),
            (&self.subject, "dc:subject"),
            (&self.author, "dc:creator"),
            (&self.keywords, "cp:keywords"),
            (&self.description, "dc:description"),
            (&self.last_modified_by, "cp:lastModifiedBy"),
            (&self.created, "dcterms:created"),
            (&self.modified, "dcterms:modified"),
        ]
    }

    fn slots(&mut self) -> [&mut Option<String>; 8] {
        [
            &mut self.title,
            &mut self.subject,
            &mut self.author,
            &mut self.keywords,
            &mut self.description,
            &mut self.last_modified_by,
            &mut self.created,
            &mut self.modified,
        ]
    }
}

const CORE_TAGS: [&str; 8] =
    ["dc:title", "dc:subject", "dc:creator", "cp:keywords", "dc:description", "cp:lastModifiedBy", "dcterms:created", "dcterms:modified"];

fn core_path(store: &PartStore) -> Option<String> {
    read_rel_list(store, "")
        .into_iter()
        .find(|r| r.type_ == rel::CORE_PROPS)
        .map(|r| r.target.strip_prefix('/').unwrap_or(&r.target).to_owned())
}

/// The properties (`docProps/core.xml`) (`readProperties`).
pub fn read_properties(store: &PartStore) -> DocProperties {
    let mut out = DocProperties::default();
    let Some(xml) = core_path(store).and_then(|p| store.text(&p)).filter(|x| !x.is_empty()) else { return out };
    for (slot, tag) in out.slots().into_iter().zip(CORE_TAGS) {
        let v = find_first(&xml, tag).map_or_else(String::new, |el| js_trim(&unescape_xml(inner_xml(&xml, &el))).to_owned());
        if !v.is_empty() {
            *slot = Some(v);
        }
    }
    out
}

/// Writes the editable properties (title, subject, author, keywords, comments) (`writeProperties`).
pub fn write_properties(store: &mut PartStore, props: &DocProperties) -> E<()> {
    let path = match core_path(store) {
        Some(p) => p,
        None => {
            let p = "docProps/core.xml".to_owned();
            store.set_text(&p, &format!("{XML_DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"></cp:coreProperties>"));
            add_rel(store, "", rel::CORE_PROPS, &p, false)?;
            add_override(store, &p, ct::CORE_PROPS)?;
            p
        }
    };
    let mut xml = must_text(store, &path)?;
    for (slot, tag) in props.fields() {
        if tag == "dcterms:created" || tag == "dcterms:modified" {
            continue;
        }
        let Some(v) = slot else { continue };
        match find_first(&xml, tag) {
            Some(el) => xml = if v.is_empty() { splice(&xml, el.start, el.end, "") } else { set_inner(&xml, &el, &escape_text(v)) },
            None => {
                if !v.is_empty() {
                    let root = find_first(&xml, "cp:coreProperties").ok_or(js("cp:coreProperties"))?;
                    xml = insert_child(&xml, &root, &format!("<{tag}>{}</{tag}>", escape_text(v)), &[]);
                }
            }
        }
    }
    store.set_text(&path, &xml);
    Ok(())
}

// ───────────────────────── set up show ─────────────────────────

/// `ShowSettingsSpec`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ShowSettingsSpec {
    pub loop_: Option<bool>,
    pub use_timings: Option<bool>,
    pub show_animation: Option<bool>,
    pub kiosk: Option<bool>,
}

/// Writes "Set up show" (`<p:showPr>` of presProps) (`writeShowSettings`).
pub fn write_show_settings(store: &mut PartStore, s: &ShowSettingsSpec) -> E<()> {
    let rel_ = read_rel_list(store, PRES).into_iter().find(|r| r.type_ == rel::PRES_PROPS);
    let path = match &rel_ {
        Some(r) => resolve_target(PRES, &r.target),
        None => "ppt/presProps.xml".to_owned(),
    };
    if store.bytes(&path).is_none() {
        store.set_text(&path, &format!("{XML_DECL}<p:presentationPr {NS_DECL}><p:showPr showNarration=\"1\"><p:present/><p:sldAll/><p:penClr><a:prstClr val=\"red\"/></p:penClr></p:showPr></p:presentationPr>"));
        add_override(store, &path, ct::PRES_PROPS)?;
        if rel_.is_none() {
            add_rel(store, PRES, rel::PRES_PROPS, &relative_target(PRES, &path), false)?;
        }
    }
    let mut xml = must_text(store, &path)?;
    let mut sp = find_first(&xml, "p:showPr");
    if sp.is_none() {
        let root = find_first(&xml, "p:presentationPr").ok_or(js("p:presentationPr"))?;
        xml = insert_child(&xml, &root, "<p:showPr showNarration=\"1\"><p:present/><p:sldAll/></p:showPr>", &["p:clrMru", "p:extLst"]);
        sp = Some(find_first(&xml, "p:showPr").ok_or(js("p:showPr"))?);
    }
    let mut sp = sp.ok_or(js("p:showPr"))?;
    for (attr, v, def) in [("loop", s.loop_, false), ("useTimings", s.use_timings, true), ("showAnimation", s.show_animation, true)] {
        let Some(v) = v else { continue };
        xml = set_attr(
            &xml,
            &sp,
            attr,
            if v == def {
                None
            } else if v {
                Some("1")
            } else {
                Some("0")
            },
        );
        sp = find_first(&xml, "p:showPr").ok_or(js("p:showPr"))?;
    }
    if let Some(kiosk) = s.kiosk {
        if sp.self_closing {
            xml = set_inner(&xml, &sp, "");
            sp = find_first(&xml, "p:showPr").ok_or(js("p:showPr"))?;
        }
        for k in child_ranges(&xml, &sp).iter().rev() {
            if ["p:present", "p:browse", "p:kiosk"].contains(&k.name.as_str()) {
                xml = splice(&xml, k.range.start, k.range.end, "");
            }
        }
        sp = find_first(&xml, "p:showPr").ok_or(js("p:showPr"))?;
        xml = splice(&xml, sp.open_end, sp.open_end, if kiosk { "<p:kiosk/>" } else { "<p:present/>" });
    }
    store.set_text(&path, &xml);
    Ok(())
}

// ───────────────────────── clipboard (shapes) ─────────────────────────

/// A relation cited by a copied shape, with the bytes of its target for another presentation.
#[derive(Clone, Debug)]
pub struct ClipRel {
    pub id: String,
    pub type_: String,
    pub target: String,
    pub external: bool,
    pub bytes: Option<Arc<[u8]>>,
    pub content_type: Option<String>,
}

/// A copied shape (`ShapeClip`): its XML and the relations it cites. `source_id` identifies the
/// session it came from (pasting in the same session shares the parts).
#[derive(Clone, Debug)]
pub struct ShapeClip {
    pub xml: String,
    pub rels: Vec<ClipRel>,
    /// the text of the shape (also plain text for the system clipboard)
    pub text: String,
    pub source_id: u64,
    pub slide_path: String,
}

/// The `r:embed|link|id|pict|dm|lo|qs|cs="..."` attributes of a text: (start, end, kind, id).
fn rel_attrs(xml: &str) -> Vec<(usize, usize, &str, &str)> {
    const KINDS: [&str; 8] = ["embed", "link", "id", "pict", "dm", "lo", "qs", "cs"];
    let b = xml.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find("r:")).map(|p| from + p) {
        from = p + 1;
        if p > 0 && is_js_word(b[p - 1]) {
            continue;
        }
        let ks = p + 2;
        let mut ke = ks;
        while b.get(ke).is_some_and(u8::is_ascii_lowercase) {
            ke += 1;
        }
        let kind = &xml[ks..ke];
        if !KINDS.contains(&kind) || !xml[ke..].starts_with("=\"") {
            continue;
        }
        let vs = ke + 2;
        let Some(len) = b[vs..].iter().position(|&c| c == b'"') else { continue };
        if len == 0 {
            continue;
        }
        out.push((p, vs + len + 1, kind, &xml[vs..vs + len]));
        from = vs + len + 1;
    }
    out
}

fn portable_rel(type_: &str) -> bool {
    ["image", "media", "video", "audio"].iter().any(|k| type_.strip_suffix(k).is_some_and(|h| h.ends_with('/')))
}

/// Copies the shape (only the ones directly in the slide tree, not in groups) (`extractShape`).
pub fn extract_shape(store: &PartStore, source_id: u64, slide: &str, id: &str) -> E<Option<ShapeClip>> {
    let Some(xml) = store.text(slide).filter(|x| !x.is_empty()) else { return Ok(None) };
    if !is_top_level(&xml, id)? {
        return Ok(None);
    }
    let first = shape_ranges(&xml, id).into_iter().next().ok_or(js("shape"))?;
    let tree = find_first(&xml, "p:spTree").ok_or(js("p:spTree"))?;
    let o = find_elements(&xml, "mc:AlternateContent", tree.open_end, tree.end)
        .into_iter()
        .find(|ac| ac.start < first.start && ac.end > first.end)
        .unwrap_or(first);
    let shape_xml = sl(&xml, o.start, o.end).to_owned();
    let ids: FxHashSet<&str> = rel_attrs(&shape_xml).iter().map(|m| m.3).collect();
    let rels = read_rel_list(store, slide)
        .into_iter()
        .filter(|r| ids.contains(r.id.as_str()))
        .map(|r| {
            if r.external {
                return ClipRel { id: r.id, type_: r.type_, target: r.target, external: true, bytes: None, content_type: None };
            }
            let abs = resolve_target(slide, &r.target);
            ClipRel {
                id: r.id,
                type_: r.type_,
                bytes: store.bytes(&abs).cloned(),
                content_type: content_type_of(store, &abs),
                target: abs,
                external: false,
            }
        })
        .collect();
    let text = find_all_elements(&shape_xml, "a:p", 0, shape_xml.len())
        .iter()
        .map(|p| {
            find_elements(&shape_xml, "a:t", p.start, p.end).iter().map(|t| unescape_xml(inner_xml(&shape_xml, t))).collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(ShapeClip { xml: shape_xml, rels, text, source_id, slide_path: slide.to_owned() }))
}

/// `/<a:off\b[^>]*\/>/` first match: (start, end).
fn first_off(xml: &str) -> Option<(usize, usize)> {
    let b = xml.as_bytes();
    let name = "<a:off";
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find(name)).map(|p| from + p) {
        from = p + 1;
        let after = p + name.len();
        if b.get(after).is_some_and(|&c| is_js_word(c)) {
            continue;
        }
        let gt = b[after..].iter().position(|&c| c == b'>').map(|g| after + g);
        if let Some(gt) = gt
            && gt > after.saturating_sub(0)
            && b[gt - 1] == b'/'
        {
            return Some((p, gt + 1));
        }
    }
    None
}

/// `Number(/\s<attr>="(-?\d+)"/.exec(off)?.[1] ?? 0)`.
fn off_value(off: &str, attr: &str) -> f64 {
    let b = off.as_bytes();
    let lit = format!("{attr}=\"");
    let mut q = 0;
    while q < b.len() {
        let wl = uo_jsnum::text::js_space_len(b, q);
        if wl > 0 && off[q + wl..].starts_with(&lit) {
            let ds = q + wl + lit.len();
            let mut de = ds;
            if b.get(de) == Some(&b'-') {
                de += 1;
            }
            let digits_start = de;
            while b.get(de).is_some_and(u8::is_ascii_digit) {
                de += 1;
            }
            if de > digits_start && b.get(de) == Some(&b'"') {
                return to_number(&off[ds..de]);
            }
        }
        q += off[q..].chars().next().map_or(1, char::len_utf8);
    }
    0.0
}

/// Pastes the shape in the slide (on top of everything), with new ids and the relations rebuilt;
/// `dx`/`dy` (EMU) shift the position. Returns the new id, or `None` when the shape depends on parts
/// that cannot go to another presentation (chart, SmartArt, OLE object); the relations and parts
/// already added stay, as in the TS (`pasteShape`).
pub fn paste_shape(store: &mut PartStore, same: bool, slide: &str, clip: &ShapeClip, dx: f64, dy: f64) -> E<Option<String>> {
    let mut xml = clip.xml.clone();
    // mc:AlternateContent: the Choice uses prefixes declared in the root of the source slide (p14...);
    // the Fallback is plain PresentationML and holds in any slide
    if xml.starts_with("<mc:AlternateContent") {
        let fb = find_first(&xml, "mc:Fallback").or_else(|| find_first(&xml, "mc:Choice"));
        if let Some(fb) = fb {
            xml = js_trim(inner_xml(&xml, &fb)).to_owned();
        }
    }
    // relations: the same part (same package) or a copy of the bytes (image/media in another package)
    let mut remap: Vec<(String, String)> = Vec::new();
    for r in &clip.rels {
        if r.external {
            let id = add_rel(store, slide, &r.type_, &r.target, true)?;
            remap.push((r.id.clone(), id));
            continue;
        }
        if same && store.bytes(&r.target).is_some() {
            let target =
                if super::pkg::shared_rel(&r.type_) { r.target.clone() } else { super::pkg::copy_part_deep_new(store, &r.target)? };
            let id = add_rel(store, slide, &r.type_, &relative_target(slide, &target), false)?;
            remap.push((r.id.clone(), id));
            continue;
        }
        let Some(bytes) = r.bytes.clone().filter(|_| portable_rel(&r.type_)) else { return Ok(None) };
        let ext = {
            let e = &r.target[r.target.rfind('.').map_or(0, |i| i + 1)..];
            let e = uo_jsnum::text::to_lowercase(e);
            if e.is_empty() { "bin".to_owned() } else { e }
        };
        let part = next_part_name(store, "ppt/media", "image", &ext, &FxHashSet::default());
        store.set(&part, bytes);
        if let Some(ct_) = r.content_type.as_deref().filter(|c| !c.is_empty()) {
            ensure_default(store, &ext, ct_)?;
        }
        let id = add_rel(store, slide, &r.type_, &relative_target(slide, &part), false)?;
        remap.push((r.id.clone(), id));
    }
    // r:embed="..." -> the new ids
    {
        let attrs = rel_attrs(&xml);
        let mut out = String::new();
        let mut at = 0;
        for (s, e, kind, id) in attrs {
            if let Some((_, new)) = remap.iter().rev().find(|(old, _)| old == id) {
                out.push_str(&xml[at..s]);
                out.push_str(&format!("r:{kind}=\"{new}\""));
                at = e;
            }
        }
        out.push_str(&xml[at..]);
        xml = out;
    }
    // new ids (copies in the mc:AlternateContent keep the SAME id among themselves)
    let target = must_text(store, slide)?;
    let mut next = max_shape_id(&target) + 1.0;
    let mut ids: Vec<(String, f64)> = Vec::new();
    {
        let mut out = String::new();
        let mut at = 0;
        for (start, ds, de, end) in scan(&xml) {
            let old = &xml[ds..de];
            let new = match ids.iter().find(|(o, _)| o == old) {
                Some((_, n)) => *n,
                None => {
                    let n = next;
                    next += 1.0;
                    ids.push((old.to_owned(), n));
                    n
                }
            };
            out.push_str(&xml[at..ds]);
            out.push_str(&fmt(new));
            at = de;
            let _ = (start, end);
        }
        out.push_str(&xml[at..]);
        xml = out;
    }
    // shift: the first <a:off>/<p:xfrm> of each copy
    if (dx != 0.0 && !dx.is_nan() || dy != 0.0 && !dy.is_nan())
        && let Some((s, e)) = first_off(&xml)
    {
        let off = sl(&xml, s, e).to_owned();
        let x = off_value(&off, "x") + dx;
        let y = off_value(&off, "y") + dy;
        let new = format!("<a:off x=\"{}\" y=\"{}\"/>", fmt(uo_jsnum::math::round(x)), fmt(uo_jsnum::math::round(y)));
        xml = splice(&xml, s, e, &new);
    }
    let tree = find_first(&target, "p:spTree").ok_or(js("p:spTree"))?;
    store.set_text(slide, &insert_child(&target, &tree, &xml, &["p:extLst"]));
    Ok(Some(match ids.first() {
        Some((_, n)) => fmt(*n),
        None => fmt(next - 1.0),
    }))
}

fn scan(xml: &str) -> Vec<(usize, usize, usize, usize)> {
    super::shapes::scan_cnvpr_ids(xml)
}

/// The frame helper re-exported for the session (a centred rectangle).
pub fn centered(slide_w: f64, slide_h: f64, w: f64, h: f64) -> Frame {
    let cx = uo_jsnum::math::min(w, slide_w * 0.9);
    let cy = uo_jsnum::math::min(h, slide_h * 0.9);
    Frame { x: (slide_w - cx) / 2.0, y: (slide_h - cy) / 2.0, cx, cy }
}
