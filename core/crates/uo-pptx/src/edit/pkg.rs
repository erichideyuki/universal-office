//! `deck-edit.ts` part 1: parts, relationships, content types and the slide list (lines 1-503 of the
//! TS): `readRelList`, `addRel`, `removeRels`, `nextPartName`, `slideEntries`, `listLayouts`,
//! `addSlide`, `duplicateSlide`, `deleteSlide`, `moveSlide` and the per-slide properties.
//!
//! The operations are functions over the part store ([`PartStore`], the `PptxPackage` of the TS): each
//! replaces whole parts, never bytes in place, and only the edited span of each part (`edit::xml`).
//! A `throw` of the TS (a `!` on a missing element, a missing slide) is an [`EditError`]; the
//! [`crate::edit::Editor`] restores the snapshot, so an `Err` leaves no partial change (the TS leaves what
//! it had written).

use std::sync::Arc;

use rustc_hash::FxHashSet;
use uo_jsnum::format::to_js_string;
use uo_jsnum::parse::to_number;
use uo_jsnum::text::{is_js_space, is_js_word};

use super::random::Xorshift32;
use super::xml::{
    XmlRange, child_ranges, escape_attr, find_element, find_elements, find_elements_all, find_first, find_from, get_attr, insert_child,
    set_attr, set_attr_in_tag, set_inner, sl, splice,
};
use crate::package::PartStore;

/// Why an edit operation failed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EditError {
    /// A part the operation needs is missing (`Pacote sem ppt/presentation.xml`).
    Missing(Box<str>),
    /// The slide does not exist (`slide N inexistente`).
    NoSuchSlide(i64),
    /// A JavaScript `TypeError`: a `!` on a missing element (the part is not what the operation expects).
    Js(&'static str),
    /// A declared refusal of hostile input.
    Limit(&'static str),
    /// The deck could not be read back (the session's re-read after an edit).
    Deck(Box<str>),
    /// The font embedding plan failed (a malformed font).
    Fonts(Box<str>),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Missing(p) => write!(f, "package without {p}"),
            EditError::NoSuchSlide(i) => write!(f, "slide {i} does not exist"),
            EditError::Js(w) => write!(f, "TypeError: {w}"),
            EditError::Limit(w) => write!(f, "limit exceeded: {w}"),
            EditError::Deck(m) => write!(f, "deck: {m}"),
            EditError::Fonts(m) => write!(f, "fonts: {m}"),
        }
    }
}

impl std::error::Error for EditError {}

pub type E<T> = Result<T, EditError>;

const NS_PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// `REL`.
pub mod rel {
    pub const SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
    pub const SLIDE_LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
    pub const SLIDE_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
    pub const NOTES_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
    pub const NOTES_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
    pub const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    pub const HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
    pub const PRES_PROPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps";
    pub const THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
    pub const CORE_PROPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
}

/// `CT`.
pub mod ct {
    pub const SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
    pub const NOTES_SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
    pub const NOTES_MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
    pub const THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
    pub const PRES_PROPS: &str = "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml";
    pub const CORE_PROPS: &str = "application/vnd.openxmlformats-package.core-properties+xml";
}

pub const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
pub const NS_DECL: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";
pub const PRES: &str = "ppt/presentation.xml";
const CT_PATH: &str = "[Content_Types].xml";

/// `Math.max(a, b)` of JS numbers (`NaN` wins).
fn jmax(a: f64, b: f64) -> f64 {
    uo_jsnum::math::max(a, b)
}

/// `Number(x) || 0`.
pub fn num_or0(s: &str) -> f64 {
    let n = to_number(s);
    if n.is_nan() { 0.0 } else { n }
}

/// `${n}` of a JS number.
pub fn fmt(n: f64) -> String {
    to_js_string(n)
}

/// `pkg.presentationXml()`.
pub fn presentation_xml(store: &PartStore) -> E<String> {
    store.text(PRES).ok_or_else(|| EditError::Missing(PRES.into()))
}

// ───────────────────────── parts, relations and types ─────────────────────────

/// `.rels` of a part; `""` = the package relations (`relsPathOf`).
pub fn rels_path_of(part: &str) -> String {
    if part.is_empty() {
        return "_rels/.rels".to_owned();
    }
    match part.rfind('/') {
        None => format!("_rels/{part}.rels"),
        Some(i) => format!("{}/_rels/{}.rels", &part[..i], &part[i + 1..]),
    }
}

/// A relationship (`Rel`); the target is RAW, as in the `.rels`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rel {
    pub id: String,
    pub type_: String,
    pub target: String,
    pub external: bool,
}

/// The relations of a part (`readRelList`).
pub fn read_rel_list(store: &PartStore, part: &str) -> Vec<Rel> {
    let Some(xml) = store.text(&rels_path_of(part)) else { return Vec::new() };
    find_elements_all(&xml, "Relationship")
        .iter()
        .map(|r| Rel {
            id: get_attr(&xml, r, "Id").unwrap_or_default(),
            type_: get_attr(&xml, r, "Type").unwrap_or_default(),
            target: get_attr(&xml, r, "Target").unwrap_or_default(),
            external: get_attr(&xml, r, "TargetMode").as_deref() == Some("External"),
        })
        .collect()
}

/// `pkg.resolveRelTarget(fromPart, target)`.
pub fn resolve_target(from: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_owned();
    }
    let mut dir: &str = match from.rfind('/') {
        Some(i) => &from[..i],
        None => "",
    };
    let mut t = target;
    while let Some(rest) = t.strip_prefix("../") {
        t = rest;
        dir = match dir.rfind('/') {
            Some(i) => &dir[..i],
            None => "",
        };
    }
    if dir.is_empty() { t.to_owned() } else { format!("{dir}/{t}") }
}

/// The target relative to `from` (`relativeTarget`).
pub fn relative_target(from: &str, to: &str) -> String {
    let a: Vec<&str> = {
        let mut v: Vec<&str> = from.split('/').collect();
        v.pop();
        v
    };
    let b: Vec<&str> = to.split('/').collect();
    let mut i = 0;
    while i < a.len() && i + 1 < b.len() && a[i] == b[i] {
        i += 1;
    }
    let mut out: Vec<&str> = vec![".."; a.len() - i];
    out.extend_from_slice(&b[i..]);
    out.join("/")
}

fn next_rel_id(rels: &[Rel]) -> String {
    let mut max = 0.0;
    for r in rels {
        if let Some(d) = r.id.strip_prefix("rId")
            && !d.is_empty()
            && d.bytes().all(|c| c.is_ascii_digit())
        {
            max = jmax(max, to_number(d));
        }
    }
    format!("rId{}", fmt(max + 1.0))
}

/// Appends a relationship to the part (creates the `.rels` if needed); returns the Id (`addRel`).
pub fn add_rel(store: &mut PartStore, part: &str, type_: &str, target: &str, external: bool) -> E<String> {
    let path = rels_path_of(part);
    let xml = store.text(&path).unwrap_or_else(|| format!("{XML_DECL}<Relationships xmlns=\"{NS_PKG_REL}\"></Relationships>"));
    let id = next_rel_id(&read_rel_list(store, part));
    let el = format!(
        "<Relationship Id=\"{id}\" Type=\"{type_}\" Target=\"{}\"{}/>",
        escape_attr(target),
        if external { " TargetMode=\"External\"" } else { "" }
    );
    let root = find_first(&xml, "Relationships").ok_or(EditError::Js("Relationships"))?;
    store.set_text(&path, &insert_child(&xml, &root, &el, &[]));
    Ok(id)
}

/// Removes relationships by Id (`removeRels`).
pub fn remove_rels(store: &mut PartStore, part: &str, ids: &FxHashSet<String>) {
    let path = rels_path_of(part);
    let Some(mut xml) = store.text(&path) else { return };
    for r in find_elements_all(&xml, "Relationship").iter().rev() {
        if ids.contains(&get_attr(&xml, r, "Id").unwrap_or_default()) {
            xml = splice(&xml, r.start, r.end, "");
        }
    }
    store.set_text(&path, &xml);
}

/// Changes the target of a relationship (`setRelTarget`).
pub fn set_rel_target(store: &mut PartStore, part: &str, id: &str, target: &str) {
    let path = rels_path_of(part);
    let Some(xml) = store.text(&path) else { return };
    let r = find_elements_all(&xml, "Relationship").into_iter().find(|x| get_attr(&xml, x, "Id").as_deref() == Some(id));
    if let Some(r) = r {
        store.set_text(&path, &set_attr(&xml, &r, "Target", Some(target)));
    }
}

fn override_of(store: &PartStore, part: &str) -> Option<String> {
    let xml = store.text(CT_PATH)?;
    let want = format!("/{part}");
    let o = find_elements_all(&xml, "Override").into_iter().find(|r| get_attr(&xml, r, "PartName").as_deref() == Some(&want))?;
    get_attr(&xml, &o, "ContentType")
}

/// `contentTypeOf`: the Override, else the Default of the extension.
pub fn content_type_of(store: &PartStore, part: &str) -> Option<String> {
    let xml = store.text(CT_PATH)?;
    let want = format!("/{part}");
    if let Some(o) = find_elements_all(&xml, "Override").into_iter().find(|r| get_attr(&xml, r, "PartName").as_deref() == Some(&want)) {
        return get_attr(&xml, &o, "ContentType");
    }
    let ext = js_lower(&part[part.rfind('.').map_or(0, |i| i + 1)..]);
    // `part.slice(part.lastIndexOf('.') + 1)`: no dot -> the whole name
    let d =
        find_elements_all(&xml, "Default").into_iter().find(|r| js_lower(&get_attr(&xml, r, "Extension").unwrap_or_default()) == ext)?;
    get_attr(&xml, &d, "ContentType")
}

fn js_lower(s: &str) -> String {
    uo_jsnum::text::to_lowercase(s)
}

/// `addOverride`.
pub fn add_override(store: &mut PartStore, part: &str, type_: &str) -> E<()> {
    let Some(xml) = store.text(CT_PATH) else { return Ok(()) };
    let want = format!("/{part}");
    if find_elements_all(&xml, "Override").iter().any(|r| get_attr(&xml, r, "PartName").as_deref() == Some(&want)) {
        return Ok(());
    }
    let root = find_first(&xml, "Types").ok_or(EditError::Js("Types"))?;
    let el = format!("<Override PartName=\"/{}\" ContentType=\"{}\"/>", escape_attr(part), escape_attr(type_));
    store.set_text(CT_PATH, &insert_child(&xml, &root, &el, &[]));
    Ok(())
}

fn remove_override(store: &mut PartStore, part: &str) {
    let Some(xml) = store.text(CT_PATH) else { return };
    let want = format!("/{part}");
    let o = find_elements_all(&xml, "Override").into_iter().find(|r| get_attr(&xml, r, "PartName").as_deref() == Some(&want));
    if let Some(o) = o {
        store.set_text(CT_PATH, &splice(&xml, o.start, o.end, ""));
    }
}

/// `ensureDefault`: a `Default` for the extension, before the first `Override`.
pub fn ensure_default(store: &mut PartStore, ext: &str, type_: &str) -> E<()> {
    let Some(xml) = store.text(CT_PATH) else { return Ok(()) };
    if find_elements_all(&xml, "Default").iter().any(|r| js_lower(&get_attr(&xml, r, "Extension").unwrap_or_default()) == ext) {
        return Ok(());
    }
    let root = find_first(&xml, "Types").ok_or(EditError::Js("Types"))?;
    let first = find_from(&xml, "Override", root.open_end);
    let el = format!("<Default Extension=\"{ext}\" ContentType=\"{type_}\"/>");
    let out = match first {
        Some(f) => splice(&xml, f.start, f.start, &el),
        None => insert_child(&xml, &root, &el, &[]),
    };
    store.set_text(CT_PATH, &out);
    Ok(())
}

/// The number of `^dir/base(\d+)\.ext$` (flag `i`) in a part name, if it matches. `base` is taken
/// literally (the TS builds a `RegExp` from it unescaped; every caller passes a plain name).
fn numbered_part(name: &str, dir: &str, base: &str, ext: &str) -> Option<f64> {
    use uo_text::case::regexp_canonicalize as canon;
    let n: Vec<u16> = name.encode_utf16().collect();
    let eq = |a: u16, b: u16| a == b || canon(a) == canon(b);
    let mut i = 0;
    for u in dir.encode_utf16().chain(std::iter::once(u16::from(b'/'))).chain(base.encode_utf16()) {
        if !eq(*n.get(i)?, u) {
            return None;
        }
        i += 1;
    }
    let ds = i;
    while n.get(i).is_some_and(|&u| (u16::from(b'0')..=u16::from(b'9')).contains(&u)) {
        i += 1;
    }
    if i == ds {
        return None;
    }
    let digits: String = n[ds..i].iter().filter_map(|&u| char::from_u32(u32::from(u))).collect();
    if *n.get(i)? != u16::from(b'.') {
        return None;
    }
    i += 1;
    let e: Vec<u16> = ext.encode_utf16().collect();
    if n.len() != i + e.len() || !n[i..].iter().zip(&e).all(|(&a, &b)| eq(a, b)) {
        return None;
    }
    Some(to_number(&digits))
}

/// The next free name `dir/base{N}.ext` (N = largest existing + 1) (`nextPartName`).
pub fn next_part_name(store: &PartStore, dir: &str, base: &str, ext: &str, taken: &FxHashSet<String>) -> String {
    let mut max = 0.0;
    for n in store.names().chain(taken.iter().map(String::as_str)) {
        if let Some(v) = numbered_part(n, dir, base, ext) {
            max = jmax(max, v);
        }
    }
    format!("{dir}/{base}{}.{ext}", fmt(max + 1.0))
}

/// Removes the part, its `.rels` and the type Override (`removePartFully`).
pub fn remove_part_fully(store: &mut PartStore, part: &str) {
    store.remove(part);
    store.remove(&rels_path_of(part));
    remove_override(store, part);
}

// ───────────────────────── the slide list ─────────────────────────

/// `SlideEntry`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlideEntry {
    pub id: String,
    pub r_id: String,
    pub path: String,
}

/// The slides in presentation order (only those with a part) (`slideEntries`).
pub fn slide_entries(store: &PartStore) -> E<Vec<SlideEntry>> {
    let xml = presentation_xml(store)?;
    let rels = read_rel_list(store, PRES);
    let Some(lst) = find_first(&xml, "p:sldIdLst") else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for s in find_elements(&xml, "p:sldId", lst.open_end, lst.end) {
        let r_id = get_attr(&xml, &s, "r:id").unwrap_or_default();
        let Some(rel) = rels.iter().find(|r| r.id == r_id) else { continue };
        out.push(SlideEntry { id: get_attr(&xml, &s, "id").unwrap_or_default(), r_id, path: resolve_target(PRES, &rel.target) });
    }
    Ok(out)
}

const PRES_AFTER_SLD_IDS: [&str; 11] = [
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

/// Puts the slide `path` at position `at` of the list (rel + sldId) (`insertSlideEntry`).
fn insert_slide_entry(store: &mut PartStore, at: i64, path: &str) -> E<()> {
    let r_id = add_rel(store, PRES, rel::SLIDE, &relative_target(PRES, path), false)?;
    let mut xml = presentation_xml(store)?;
    let mut max_id = 255.0;
    for s in find_elements_all(&xml, "p:sldId") {
        max_id = jmax(max_id, num_or0(&get_attr(&xml, &s, "id").unwrap_or_default()));
    }
    let el = format!("<p:sldId id=\"{}\" r:id=\"{r_id}\"/>", fmt(max_id + 1.0));
    let mut lst = find_first(&xml, "p:sldIdLst");
    if lst.is_none() {
        let root = find_first(&xml, "p:presentation").ok_or(EditError::Js("p:presentation"))?;
        xml = insert_child(&xml, &root, "<p:sldIdLst></p:sldIdLst>", &PRES_AFTER_SLD_IDS);
        lst = Some(find_first(&xml, "p:sldIdLst").ok_or(EditError::Js("p:sldIdLst"))?);
    }
    let lst = lst.ok_or(EditError::Js("p:sldIdLst"))?;
    let items = find_elements(&xml, "p:sldId", lst.open_end, lst.end);
    let len = i64::try_from(items.len()).unwrap_or(i64::MAX);
    let pos: Option<usize> = if at < len {
        items.get(usize::try_from(at.max(0)).unwrap_or(0)).map(|i| i.start)
    } else if lst.self_closing {
        None
    } else {
        Some(lst.end.saturating_sub("</p:sldIdLst>".len()))
    };
    xml = match pos {
        None => set_inner(&xml, &lst, &el),
        Some(p) => splice(&xml, p, p, &el),
    };
    store.set_text(PRES, &xml);
    Ok(())
}

// ───────────────────────── layouts ─────────────────────────

/// `LayoutInfo`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LayoutInfo {
    pub path: String,
    pub name: String,
    pub type_: String,
    pub master: String,
}

/// The layouts of all the masters, in `sldLayoutIdLst` order (`listLayouts`).
pub fn list_layouts(store: &PartStore) -> E<Vec<LayoutInfo>> {
    let mut out = Vec::new();
    let pxml = presentation_xml(store)?;
    let prels = read_rel_list(store, PRES);
    let masters: Vec<String> = match find_first(&pxml, "p:sldMasterIdLst") {
        Some(mlst) => find_elements(&pxml, "p:sldMasterId", mlst.open_end, mlst.end)
            .iter()
            .filter_map(|m| {
                let id = get_attr(&pxml, m, "r:id");
                prels.iter().find(|r| Some(r.id.as_str()) == id.as_deref())
            })
            .map(|r| resolve_target(PRES, &r.target))
            .collect(),
        None => Vec::new(),
    };
    for master in masters {
        let Some(mxml) = store.text(&master) else { continue };
        let mrels = read_rel_list(store, &master);
        let ids: Vec<Option<String>> = match find_first(&mxml, "p:sldLayoutIdLst") {
            Some(llst) => {
                find_elements(&mxml, "p:sldLayoutId", llst.open_end, llst.end).iter().map(|l| get_attr(&mxml, l, "r:id")).collect()
            }
            None => Vec::new(),
        };
        for id in ids {
            let Some(rel) = mrels.iter().find(|r| Some(r.id.as_str()) == id.as_deref()) else { continue };
            let path = resolve_target(&master, &rel.target);
            let Some(lxml) = store.text(&path) else { continue };
            let root = find_first(&lxml, "p:sldLayout");
            let csld = find_first(&lxml, "p:cSld");
            let name = csld.and_then(|c| get_attr(&lxml, &c, "name")).filter(|s| !s.is_empty()).unwrap_or_else(|| {
                let file = path.rsplit('/').next().unwrap_or("");
                file.strip_suffix(".xml").unwrap_or(file).to_owned()
            });
            let type_ = root.and_then(|r| get_attr(&lxml, &r, "type")).filter(|s| !s.is_empty()).unwrap_or_else(|| "cust".to_owned());
            out.push(LayoutInfo { path, master: master.clone(), name, type_ });
        }
    }
    Ok(out)
}

/// The layout of a slide (by its `.rels`) (`layoutOf`).
pub fn layout_of(store: &PartStore, slide_path: &str) -> Option<String> {
    read_rel_list(store, slide_path).into_iter().find(|r| r.type_ == rel::SLIDE_LAYOUT).map(|r| resolve_target(slide_path, &r.target))
}

const TEXT_PH: [&str; 6] = ["", "title", "ctrTitle", "subTitle", "body", "obj"];
const SKIP_PH: [&str; 5] = ["dt", "ftr", "sldNum", "hdr", "sldImg"];

/// `tag.replace(/\s*\/?>$/, '/>')`.
fn self_close(tag: &str) -> String {
    if !tag.ends_with('>') {
        return tag.to_owned();
    }
    let mut cut = tag.len() - 1;
    if tag[..cut].ends_with('/') {
        cut -= 1;
    }
    let head = tag[..cut].trim_end_matches(is_js_space);
    format!("{head}/>")
}

/// The XML of the new slide: the layout placeholders, empty (`slideFromLayout`).
fn slide_from_layout(store: &PartStore, layout_path: &str, lang: &str) -> String {
    let lxml = store.text(layout_path).unwrap_or_default();
    let mut shapes = String::new();
    let mut id = 2u64;
    if let Some(tree) = find_first(&lxml, "p:spTree") {
        for sp in find_elements(&lxml, "p:sp", tree.open_end, tree.end) {
            let Some(ph) = find_element(&lxml, "p:ph", sp.start, sp.end) else { continue };
            let type_ = get_attr(&lxml, &ph, "type").unwrap_or_default();
            if SKIP_PH.contains(&type_.as_str()) {
                continue;
            }
            let cnv = find_element(&lxml, "p:cNvPr", sp.start, sp.end);
            let name = cnv
                .and_then(|c| get_attr(&lxml, &c, "name"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("Placeholder {}", id - 1));
            let ph_tag = set_attr_in_tag(&self_close(sl(&lxml, ph.start, ph.open_end)), "hasCustomPrompt", None);
            let text = if TEXT_PH.contains(&type_.as_str()) {
                format!("<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p></p:txBody>")
            } else {
                String::new()
            };
            shapes.push_str(&format!(
                "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr>{ph_tag}</p:nvPr></p:nvSpPr><p:spPr/>{text}</p:sp>",
                escape_attr(&name)
            ));
            id += 1;
        }
    }
    format!(
        "{XML_DECL}<p:sld {NS_DECL}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    )
}

/// New slide from the layout at position `at`; returns the part path (`addSlide`).
pub fn add_slide(store: &mut PartStore, at: i64, layout_path: &str, lang: &str) -> E<String> {
    let path = next_part_name(store, "ppt/slides", "slide", "xml", &FxHashSet::default());
    let xml = slide_from_layout(store, layout_path, lang);
    store.set_text(&path, &xml);
    add_rel(store, &path, rel::SLIDE_LAYOUT, &relative_target(&path, layout_path), false)?;
    add_override(store, &path, ct::SLIDE)?;
    insert_slide_entry(store, at, &path)?;
    Ok(path)
}

/// Relations whose target may be SHARED between the original and the copy (the rest is copied).
pub fn shared_rel(type_: &str) -> bool {
    ["image", "media", "video", "audio", "hyperlink", "slideLayout", "slideMaster", "slide", "notesMaster", "tags", "font"]
        .iter()
        .any(|k| type_.strip_suffix(k).is_some_and(|h| h.ends_with('/')))
}

/// Budget of a deep copy: nesting depth and number of parts (the TS recurses without a bound).
struct CopyCx {
    taken: FxHashSet<String>,
    depth: u32,
    parts: u32,
}

const COPY_MAX_DEPTH: u32 = 64;
const COPY_MAX_PARTS: u32 = 20_000;

/// `copyPartDeep(pkg, part, new Set())`: a deep copy with a fresh set of taken names.
pub fn copy_part_deep_new(store: &mut PartStore, part: &str) -> E<String> {
    let mut cx = CopyCx { taken: FxHashSet::default(), depth: 0, parts: 0 };
    copy_part_deep(store, part, &mut cx)
}

/// `/^(.*?)(\d*)\.(\w+)$/` on a file name: (base without the trailing digits, extension); `None`
/// when it does not match (a line terminator, no `.ext`).
fn split_file(file: &str) -> Option<(&str, &str)> {
    if file.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let dot = file.rfind('.')?;
    let ext = &file[dot + 1..];
    if ext.is_empty() || !ext.bytes().all(is_js_word) {
        return None;
    }
    Some((file[..dot].trim_end_matches(|c: char| c.is_ascii_digit()), ext))
}

/// Copies a part and (recursively) the ones only it uses; returns the new path (`copyPartDeep`).
fn copy_part_deep(store: &mut PartStore, part: &str, cx: &mut CopyCx) -> E<String> {
    if cx.depth >= COPY_MAX_DEPTH || cx.parts >= COPY_MAX_PARTS {
        return Err(EditError::Limit("deep copy of parts"));
    }
    cx.parts += 1;
    // `part.slice(0, part.lastIndexOf('/'))`: no slash drops the last character
    let (dir, file) = match part.rfind('/') {
        Some(i) => (&part[..i], &part[i + 1..]),
        None => {
            let mut d = part.to_owned();
            d.pop();
            return copy_part_deep_in(store, part, &d, part, cx);
        }
    };
    copy_part_deep_in(store, part, dir, file, cx)
}

fn copy_part_deep_in(store: &mut PartStore, part: &str, dir: &str, file: &str, cx: &mut CopyCx) -> E<String> {
    let (base, ext) = match split_file(file) {
        Some((b, e)) => (if b.is_empty() { "part" } else { b }, e),
        None => ("part", "xml"),
    };
    let copy = next_part_name(store, dir, base, ext, &cx.taken);
    cx.taken.insert(copy.clone());
    if let Some(data) = store.bytes(part).cloned() {
        store.set(&copy, data);
    }
    if let Some(ov) = override_of(store, part) {
        add_override(store, &copy, &ov)?;
    } else {
        let a = content_type_of(store, part);
        if a != content_type_of(store, &copy) {
            add_override(store, &copy, &a.ok_or(EditError::Js("content type"))?)?;
        }
    }
    if read_rel_list(store, part).is_empty() {
        return Ok(copy);
    }
    let rels_xml = store.text(&rels_path_of(part)).ok_or(EditError::Js("rels"))?;
    let mut out = rels_xml.clone();
    for r in find_elements_all(&rels_xml, "Relationship").iter().rev() {
        let type_ = get_attr(&rels_xml, r, "Type").unwrap_or_default();
        let external = get_attr(&rels_xml, r, "TargetMode").as_deref() == Some("External");
        let target = get_attr(&rels_xml, r, "Target").unwrap_or_default();
        let mut t = target.clone();
        if !external && !shared_rel(&type_) {
            let abs = resolve_target(part, &target);
            if store.bytes(&abs).is_some() {
                cx.depth += 1;
                let c = copy_part_deep(store, &abs, cx);
                cx.depth -= 1;
                t = relative_target(&copy, &c?);
            }
        } else if !external {
            t = relative_target(&copy, &resolve_target(part, &target));
        }
        if t != target {
            out = splice(&out, r.start, r.end, &set_attr_in_tag(sl(&rels_xml, r.start, r.end), "Target", Some(&t)));
        }
    }
    store.set_text(&rels_path_of(&copy), &out);
    Ok(copy)
}

/// A new creation id (`p14:creationId`): the copy must not pass for the original (`freshCreationId`).
/// `Math.random()` is `rng`, consumed only when the id is found.
fn fresh_creation_id(xml: &str, rng: &mut Xorshift32) -> String {
    let b = xml.as_bytes();
    let name = "<p14:creationId";
    let mut from = 0;
    while let Some(p) = xml.get(from..).and_then(|s| s.find(name)).map(|p| from + p) {
        let after = p + name.len();
        from = p + 1;
        if b.get(after).is_some_and(|&c| is_js_word(c)) {
            continue;
        }
        let gt = b[after..].iter().position(|&c| c == b'>').map_or(b.len(), |g| after + g);
        // the rightmost `\bval="\d+"` inside the tag
        let mut q = gt;
        while q > after {
            q -= 1;
            if !xml.is_char_boundary(q) || !xml[q..].starts_with("val=\"") {
                continue;
            }
            if q > 0 && is_js_word(b[q - 1]) {
                continue;
            }
            let ds = q + 5;
            let mut de = ds;
            while b.get(de).is_some_and(u8::is_ascii_digit) {
                de += 1;
            }
            if de > ds && b.get(de) == Some(&b'"') {
                let v = (rng.next_f64() * f64::from(0xffff_fffeu32)).floor() + 1.0;
                return format!("{}{}{}", &xml[..ds], fmt(v), &xml[de..]);
            }
        }
    }
    xml.to_owned()
}

/// Duplicates the slide `index` (with its notes) right after it; returns the new path (`duplicateSlide`).
pub fn duplicate_slide(store: &mut PartStore, index: i64, rng: &mut Xorshift32) -> E<String> {
    let entries = slide_entries(store)?;
    let e = usize::try_from(index).ok().and_then(|i| entries.get(i)).ok_or(EditError::NoSuchSlide(index))?.clone();
    let mut cx = CopyCx { taken: FxHashSet::default(), depth: 0, parts: 0 };
    let copy = copy_part_deep(store, &e.path, &mut cx)?;
    let text = store.text(&copy).ok_or(EditError::Js("slide copy"))?;
    store.set_text(&copy, &fresh_creation_id(&text, rng));
    // the notes page was copied along: it now points to the copy
    let notes = read_rel_list(store, &copy).into_iter().find(|r| r.type_ == rel::NOTES_SLIDE);
    let notes_path = notes.map(|n| resolve_target(&copy, &n.target));
    if let Some(np) = notes_path {
        let orig_notes =
            read_rel_list(store, &e.path).into_iter().find(|r| r.type_ == rel::NOTES_SLIDE).map(|r| r.target).unwrap_or_default();
        if np != resolve_target(&e.path, &orig_notes) {
            let t = store.text(&np).unwrap_or_default();
            store.set_text(&np, &fresh_creation_id(&t, rng));
            if let Some(back) = read_rel_list(store, &np).into_iter().find(|x| x.type_ == rel::SLIDE) {
                set_rel_target(store, &np, &back.id, &relative_target(&np, &copy));
            }
        }
    }
    insert_slide_entry(store, index + 1, &copy)?;
    Ok(copy)
}

/// Deletes the slide `index`: it leaves the list, the part and its notes leave the package (`deleteSlide`).
pub fn delete_slide(store: &mut PartStore, index: i64) -> E<()> {
    let entries = slide_entries(store)?;
    let Some(e) = usize::try_from(index).ok().and_then(|i| entries.get(i)).cloned() else { return Ok(()) };
    let mut xml = presentation_xml(store)?;
    let lst = find_first(&xml, "p:sldIdLst").ok_or(EditError::Js("p:sldIdLst"))?;
    let item =
        find_elements(&xml, "p:sldId", lst.open_end, lst.end).into_iter().find(|s| get_attr(&xml, s, "r:id").as_deref() == Some(&e.r_id));
    if let Some(item) = item {
        xml = splice(&xml, item.start, item.end, "");
    }
    // custom shows that cite the slide (`<p:sld r:id>` exists only in custShowLst)
    for s in find_elements_all(&xml, "p:sld").iter().rev() {
        if get_attr(&xml, s, "r:id").as_deref() == Some(&e.r_id) {
            xml = splice(&xml, s.start, s.end, "");
        }
    }
    store.set_text(PRES, &xml);
    remove_rels(store, PRES, &std::iter::once(e.r_id.clone()).collect());
    // the notes of the slide
    if let Some(notes) = read_rel_list(store, &e.path).into_iter().find(|r| r.type_ == rel::NOTES_SLIDE) {
        let np = resolve_target(&e.path, &notes.target);
        if store.bytes(&np).is_some() {
            remove_part_fully(store, &np);
        }
    }
    // links of other slides to this one: the relation and the hyperlink go together
    for other in &entries {
        if other.path == e.path {
            continue;
        }
        let dangling: Vec<Rel> = read_rel_list(store, &other.path)
            .into_iter()
            .filter(|r| !r.external && r.type_ == rel::SLIDE && resolve_target(&other.path, &r.target) == e.path)
            .collect();
        if dangling.is_empty() {
            continue;
        }
        let ids: FxHashSet<String> = dangling.iter().map(|r| r.id.clone()).collect();
        let mut sx = store.text(&other.path).unwrap_or_default();
        for tag in ["a:hlinkClick", "a:hlinkMouseOver"] {
            for h in find_elements_all(&sx, tag).iter().rev() {
                if ids.contains(&get_attr(&sx, h, "r:id").unwrap_or_default()) {
                    sx = splice(&sx, h.start, h.end, "");
                }
            }
        }
        store.set_text(&other.path, &sx);
        remove_rels(store, &other.path, &ids);
    }
    remove_part_fully(store, &e.path);
    Ok(())
}

/// Moves the slide from `from` to `to` (list indices) (`moveSlide`).
pub fn move_slide(store: &mut PartStore, from: i64, to: i64) -> E<()> {
    let mut xml = presentation_xml(store)?;
    let Some(lst) = find_first(&xml, "p:sldIdLst") else { return Ok(()) };
    let items = find_elements(&xml, "p:sldId", lst.open_end, lst.end);
    let n = i64::try_from(items.len()).unwrap_or(i64::MAX);
    if from < 0 || from >= n || to < 0 || to >= n || from == to {
        return Ok(());
    }
    let mut texts: Vec<String> = items.iter().map(|r| sl(&xml, r.start, r.end).to_owned()).collect();
    let moved = texts.remove(usize::try_from(from).unwrap_or(0));
    texts.insert(usize::try_from(to).unwrap_or(0).min(texts.len()), moved);
    let (first, last) = (items.first().map_or(0, |r| r.start), items.last().map_or(0, |r| r.end));
    xml = splice(&xml, first, last, &texts.concat());
    store.set_text(PRES, &xml);
    Ok(())
}

// ───────────────────────── slide properties ─────────────────────────

fn root_range(xml: &str) -> Option<XmlRange> {
    find_first(xml, "p:sld")
}

/// `<p:sld show="0">`: hidden in the show (`setSlideHidden`).
pub fn set_slide_hidden(store: &mut PartStore, slide_path: &str, hidden: bool) {
    let Some(xml) = store.text(slide_path) else { return };
    let Some(root) = root_range(&xml) else { return };
    if xml.is_empty() {
        return;
    }
    store.set_text(slide_path, &set_attr(&xml, &root, "show", if hidden { Some("0") } else { None }));
}

/// Changes the layout of the slide (`setSlideLayout`).
pub fn set_slide_layout(store: &mut PartStore, slide_path: &str, layout_path: &str) -> E<()> {
    let target = relative_target(slide_path, layout_path);
    match read_rel_list(store, slide_path).into_iter().find(|r| r.type_ == rel::SLIDE_LAYOUT) {
        Some(r) => set_rel_target(store, slide_path, &r.id, &target),
        None => {
            add_rel(store, slide_path, rel::SLIDE_LAYOUT, &target, false)?;
        }
    }
    Ok(())
}

/// `hex`: `c.replace(/^#/, '').toUpperCase().slice(0, 6).padStart(6, '0')`.
pub fn hex(c: &str) -> String {
    let t = c.strip_prefix('#').unwrap_or(c);
    let up = uo_text::case::to_uppercase_str(t);
    let mut units: Vec<u16> = up.encode_utf16().take(6).collect();
    while units.len() < 6 {
        units.insert(0, u16::from(b'0'));
    }
    String::from_utf16_lossy(&units)
}

/// Solid background of the slide (`None` = inherit again) (`setSlideBackground`).
pub fn set_slide_background(store: &mut PartStore, slide_path: &str, color: Option<&str>) {
    let Some(mut xml) = store.text(slide_path).filter(|x| !x.is_empty()) else { return };
    let Some(csld) = find_first(&xml, "p:cSld") else { return };
    if let Some(bg) = find_element(&xml, "p:bg", csld.open_end, csld.end) {
        xml = splice(&xml, bg.start, bg.end, "");
    }
    if let Some(color) = color.filter(|c| !c.is_empty())
        && let Some(c) = find_first(&xml, "p:cSld")
    {
        xml = splice(
            &xml,
            c.open_end,
            c.open_end,
            &format!("<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>", hex(color)),
        );
    }
    store.set_text(slide_path, &xml);
}

/// `TransitionSpec`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TransitionSpec {
    pub type_: String,
    pub dir: Option<String>,
    /// `fast` | `med` | `slow`
    pub speed: Option<String>,
    pub advance_on_click: Option<bool>,
    pub advance_after_ms: Option<f64>,
}

fn transition_xml(t: &TransitionSpec) -> String {
    let mut attrs = String::new();
    if let Some(s) = t.speed.as_deref().filter(|s| !s.is_empty() && *s != "fast") {
        attrs.push_str(&format!(" spd=\"{s}\""));
    }
    if t.advance_on_click == Some(false) {
        attrs.push_str(" advClick=\"0\"");
    }
    if let Some(ms) = t.advance_after_ms.filter(|m| *m >= 0.0) {
        attrs.push_str(&format!(" advTm=\"{}\"", fmt(uo_jsnum::math::round(ms))));
    }
    let mut effect = String::new();
    if t.type_ != "none" {
        let dir = t.dir.as_deref().filter(|d| !d.is_empty());
        let ty = t.type_.as_str();
        let ea = if matches!(ty, "push" | "wipe" | "cover" | "pull") && dir.is_some() {
            format!(" dir=\"{}\"", dir.unwrap_or(""))
        } else if ty == "split" {
            format!(" orient=\"vert\" dir=\"{}\"", if t.dir.as_deref() == Some("in") { "in" } else { "out" })
        } else if matches!(ty, "blinds" | "checker" | "comb" | "randomBar") && dir.is_some() {
            format!(" dir=\"{}\"", dir.unwrap_or(""))
        } else if ty == "wheel" {
            " spokes=\"1\"".to_owned()
        } else {
            String::new()
        };
        effect = format!("<p:{ty}{ea}/>");
    }
    format!("<p:transition{attrs}>{effect}</p:transition>")
}

/// Writes (or removes, with `None`) the entrance transition of the slide (`setSlideTransition`).
pub fn set_slide_transition(store: &mut PartStore, slide_path: &str, t: Option<&TransitionSpec>) -> E<()> {
    let Some(mut xml) = store.text(slide_path).filter(|x| !x.is_empty()) else { return Ok(()) };
    let Some(root) = root_range(&xml) else { return Ok(()) };
    // removes the current one: `<p:transition>` direct or inside an mc:AlternateContent (p14/p15)
    for k in child_ranges(&xml, &root).iter().rev() {
        if k.name == "p:transition" || (k.name == "mc:AlternateContent" && sl(&xml, k.range.start, k.range.end).contains("<p:transition")) {
            xml = splice(&xml, k.range.start, k.range.end, "");
        }
    }
    if let Some(t) = t {
        let root = root_range(&xml).ok_or(EditError::Js("p:sld"))?;
        xml = insert_child(&xml, &root, &transition_xml(t), &["p:timing", "p:extLst"]);
    }
    store.set_text(slide_path, &xml);
    Ok(())
}

/// A shared handle for the part bytes of the media inserted by the editor.
pub type Bytes = Arc<[u8]>;
