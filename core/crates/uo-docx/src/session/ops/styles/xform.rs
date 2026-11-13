//! `styles.ts` (the editor): the transformations of the style sheet text: the list of styles,
//! `ensureDefined`, create, modify, rename, delete and the gallery switch; plus
//! `stylesPartPath`/`writeStylesPart`. `ds` of the TS (`DocStyles`) is the pair of default style
//! ids the transformations read ([`Defaults`]); the text goes in and out.

use std::collections::HashSet;

use uo_opc::Package;

use super::sheet::{
    BuiltinStyle, Ch, LatentMeta, NewStyle, StyleFormat, StyleHead, StyleIndex, StyleMeta, UiLang, builtin_display_name, builtin_xml,
    builtins, drop_style, new_style_xml, patch_style_ppr, patch_style_rpr, patch_style_xml, put_style, read_style_index,
    set_latent_q_format, style_id_from_name, style_kid, style_raw,
};
use crate::error::DocxError;

/// `ds.defaultParagraph()` and `ds.defaultCharacter()`.
#[derive(Debug, Clone)]
pub(crate) struct Defaults {
    pub paragraph: String,
    pub character: String,
}

impl Defaults {
    pub(crate) fn of(idx: &StyleIndex) -> Defaults {
        Defaults {
            paragraph: idx
                .styles
                .iter()
                .find(|s| s.ty == "paragraph" && s.is_default)
                .map_or_else(|| "Normal".to_owned(), |s| s.id.clone()),
            character: idx
                .styles
                .iter()
                .find(|s| s.ty == "character" && s.is_default)
                .map_or_else(|| "Fontepargpadro".to_owned(), |s| s.id.clone()),
        }
    }
}

/// `putStyle` that leaves the text alone when the part has no `</w:styles>` (the TS throws).
fn put(xml: &str, id: &str, style: &str) -> String {
    put_style(xml, id, style).unwrap_or_else(|| xml.to_owned())
}

/// `xml.match(/w:styleId="([^"]*)"/)![1]`.
pub(crate) fn id_of(xml: &str) -> String {
    let needle = "w:styleId=\"";
    xml.find(needle).and_then(|p| xml.get(p + needle.len()..)).and_then(|r| r.find('"').map(|c| r[..c].to_owned())).unwrap_or_default()
}

/// `matchAll(/<w:(?:basedOn|next|link) w:val="([^"]*)"\/>/g)`: (start, end, tag, value).
pub(crate) fn ref_tags(xml: &str, tags: &[&str]) -> Vec<(usize, usize, usize, String)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(p) = xml.get(at..).and_then(|r| r.find("<w:")) {
        let s = at + p;
        let mut hit = None;
        for (ti, t) in tags.iter().enumerate() {
            let head = format!("<{t} w:val=\"");
            if xml.get(s..).is_some_and(|r| r.starts_with(&head)) {
                let vs = s + head.len();
                if let Some(c) = xml.get(vs..).and_then(|r| r.find('"'))
                    && xml.get(vs + c..).is_some_and(|r| r.starts_with("\"/>"))
                {
                    hit = Some((s, vs + c + 3, ti, xml[vs..vs + c].to_owned()));
                    break;
                }
            }
        }
        match hit {
            Some(h) => {
                at = h.1;
                out.push(h);
            }
            None => at = s + 1,
        }
    }
    out
}

fn refs_in(xml: &str) -> Vec<String> {
    ref_tags(xml, &["w:basedOn", "w:next", "w:link"]).into_iter().map(|h| h.3).collect()
}

fn builtin_with_id(id: &str) -> Option<BuiltinStyle> {
    builtins().find(|x| id_of(x.xml) == id || x.twin.is_some_and(|t| id_of(t) == id))
}

fn lower(s: &str) -> String {
    uo_text::case::to_lowercase_str(s)
}

/// `resolveStyleId(xml, id)`: the id of the style IN THE DOCUMENT; a built-in id (pt-BR, as
/// `Ttulo2`) becomes the one of the style with the same `w:name` the file has (English
/// `Heading2`…).
pub(crate) fn resolve_style_id(xml: Option<&str>, id: &str) -> String {
    let Some(xml) = xml else { return id.to_owned() };
    let idx = read_style_index(xml);
    if idx.styles.iter().any(|s| s.id == id) {
        return id.to_owned();
    }
    let Some(b) = builtin_with_id(id) else { return id.to_owned() };
    let twin = b.twin.is_some_and(|t| id_of(t) == id);
    let bn = lower(b.name);
    let Some(para) = idx.styles.iter().find(|s| lower(&s.name) == bn) else { return id.to_owned() };
    if twin { para.link.clone().unwrap_or_else(|| id.to_owned()) } else { para.id.clone() }
}

/// `ensureDefined(ds, xml, id, used)`: a latent built-in is defined (with its `basedOn` chain and
/// its Char twin); `used` drops the `semiHidden` (Word does that on use).
pub(crate) fn ensure_defined(xml: &str, id: &str, used: bool) -> String {
    ensure_defined_at(xml, id, used, 0)
}

fn ensure_defined_at(xml: &str, id: &str, used: bool, depth: u32) -> String {
    let idx = read_style_index(xml);
    let has = |sid: &str| idx.styles.iter().find(|s| s.id == sid);
    if let Some(cur) = has(id) {
        if used && cur.semi_hidden && cur.unhide_when_used {
            let Some(raw) = style_raw(xml, id) else { return xml.to_owned() };
            let mut out = put(xml, id, &patch_style_xml(&raw, &StyleHead { semi_hidden: Some(false), ..StyleHead::default() }));
            if let Some(tw) = cur.link.as_deref().and_then(has)
                && tw.semi_hidden
                && let Some(traw) = style_raw(&out, &tw.id)
            {
                let tid = tw.id.clone();
                out = put(&out, &tid, &patch_style_xml(&traw, &StyleHead { semi_hidden: Some(false), ..StyleHead::default() }));
            }
            return out;
        }
        return xml.to_owned();
    }
    let Some(b) = builtin_with_id(id) else { return xml.to_owned() };
    if depth > 32 {
        return xml.to_owned();
    }
    // the ids the document uses for the same built-ins (English `Heading1` × pt-BR `Ttulo1`)
    let mut refs: Vec<(String, String)> = Vec::new();
    let mut out = xml.to_owned();
    let (bid, tid) = (id_of(b.xml), b.twin.map(id_of));
    let mut all = refs_in(b.xml);
    if let Some(t) = b.twin {
        all.extend(refs_in(t));
    }
    for r in all {
        if r == bid || tid.as_deref() == Some(r.as_str()) {
            continue;
        }
        let rb = builtin_with_id(&r);
        let cur_idx = read_style_index(&out);
        let named = rb.and_then(|rb| {
            let want = if id_of(rb.xml) == r { lower(rb.name) } else { lower(&format!("{} Char", rb.pt)) };
            cur_idx.styles.iter().find(|s| lower(&s.name) == want)
        });
        if let Some(n) = named {
            if n.id != r {
                set_ref(&mut refs, &r, &n.id);
            }
            continue;
        }
        if cur_idx.styles.iter().any(|s| s.id == r) {
            continue;
        }
        if r == "Fontepargpadro" || r == "Normal" {
            let ty = if r == "Normal" { "paragraph" } else { "character" };
            if let Some(d) = cur_idx.styles.iter().find(|s| s.is_default && s.ty == ty) {
                set_ref(&mut refs, &r, &d.id);
                continue;
            }
        }
        out = ensure_defined_at(&out, &r, false, depth + 1);
    }
    out = put(&out, &bid, &builtin_xml(b.xml, None, used, &refs));
    if let Some(t) = b.twin
        && let Some(tid) = &tid
        && !read_style_index(&out).styles.iter().any(|s| s.id == *tid)
    {
        out = put(&out, tid, &builtin_xml(t, None, used, &refs));
    }
    out
}

/// `refs[from] = to` (a JS object: a repeated key keeps its place).
fn set_ref(refs: &mut Vec<(String, String)>, from: &str, to: &str) {
    match refs.iter_mut().find(|(f, _)| f == from) {
        Some(e) => e.1 = to.to_owned(),
        None => refs.push((from.to_owned(), to.to_owned())),
    }
}

/// `CreateStyleOptions`.
#[derive(Debug, Clone, Default)]
pub(crate) struct CreateStyle {
    pub name: String,
    /// `paragraph` | `character` | `linked`
    pub ty: String,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub quick: Option<bool>,
    pub auto_update: bool,
    pub format: StyleFormat,
}

fn default_char_of(xml: &str) -> String {
    read_style_index(xml)
        .styles
        .iter()
        .find(|s| s.ty == "character" && s.is_default)
        .map_or_else(|| "Fontepargpadro".to_owned(), |s| s.id.clone())
}

/// `createStyleXml(ds, xml, o)`: the style (and its Char twin when linked); the text and the id.
pub(crate) fn create_style_xml(ds: &Defaults, xml: &str, o: &CreateStyle) -> (String, String) {
    let ty = if o.ty.is_empty() { "linked" } else { o.ty.as_str() };
    let mut taken: HashSet<String> = read_style_index(xml).styles.iter().map(|s| s.id.clone()).collect();
    let id = style_id_from_name(&o.name, &taken);
    taken.insert(id.clone());
    let f = &o.format;
    let r_pr = if f.has_run_changes() { patch_style_rpr(None, f) } else { String::new() };
    let p_pr = if ty != "character" && f.has_para_changes() { patch_style_ppr(None, f) } else { String::new() };
    let mut out = xml.to_owned();
    if ty == "character" {
        let base = o.based_on.clone().unwrap_or_else(|| ds.character.clone());
        out = ensure_defined(&out, &base, false);
        let s = new_style_xml(&NewStyle {
            ty: "character",
            custom: true,
            id: id.clone(),
            name: o.name.clone(),
            based_on: Some(base),
            q_format: o.quick.unwrap_or(true),
            r_pr,
            ..NewStyle::default()
        });
        return (put(&out, &id, &s), id);
    }
    let base = o.based_on.clone().unwrap_or_else(|| ds.paragraph.clone());
    out = ensure_defined(&out, &base, false);
    let twin_id = (ty == "linked").then(|| style_id_from_name(&format!("{id}Char"), &taken));
    let s = new_style_xml(&NewStyle {
        ty: "paragraph",
        custom: true,
        id: id.clone(),
        name: o.name.clone(),
        based_on: Some(base.clone()),
        next: o.next.clone().filter(|n| *n != id),
        link: twin_id.clone(),
        auto_redefine: o.auto_update,
        q_format: o.quick.unwrap_or(true),
        p_pr,
        r_pr: r_pr.clone(),
        ..NewStyle::default()
    });
    out = put(&out, &id, &s);
    if let Some(tw) = twin_id {
        let base_twin = read_style_index(&out).styles.iter().find(|s| s.id == base).and_then(|s| s.link.clone());
        let t = new_style_xml(&NewStyle {
            ty: "character",
            custom: true,
            id: tw.clone(),
            name: format!("{} Char", o.name),
            based_on: Some(base_twin.unwrap_or_else(|| default_char_of(&out))),
            link: Some(id.clone()),
            r_pr,
            ..NewStyle::default()
        });
        out = put(&out, &tw, &t);
    }
    (out, id)
}

/// `ModifyStyleOptions`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModifyStyle {
    pub name: Option<String>,
    pub based_on: Option<String>,
    /// `Null` = the next is the style itself
    pub next: Ch<String>,
    pub quick: Option<bool>,
    pub auto_update: Option<bool>,
    pub format: StyleFormat,
}

/// `displayName(m, lang)` (aliases appended).
pub(crate) fn display_name(m: &StyleMeta, lang: UiLang) -> String {
    let base = if m.custom { m.name.clone() } else { builtin_display_name(&m.name, lang).unwrap_or_else(|| m.name.clone()) };
    match m.aliases.as_deref().filter(|a| !a.is_empty()) {
        Some(a) => format!("{base},{a}"),
        None => base,
    }
}

/// `modifyStyleXml(ds, xml, id, o)`: the text and the ids renamed. `None` when the rename finds
/// the name in use (the TS throws).
pub(crate) fn modify_style_xml(xml: &str, id: &str, o: &ModifyStyle) -> Option<(String, Vec<(String, String)>)> {
    let mut out = ensure_defined(xml, id, false);
    let idx = read_style_index(&out);
    let Some(meta) = idx.styles.iter().find(|s| s.id == id).cloned() else { return Some((xml.to_owned(), Vec::new())) };
    let Some(raw0) = style_raw(&out, id) else { return Some((xml.to_owned(), Vec::new())) };
    let f = &o.format;
    let mut head = StyleHead::default();
    if let Some(b) = &o.based_on
        && Some(b.as_str()) != meta.based_on.as_deref()
        && b != id
    {
        out = ensure_defined(&out, b, false);
        head.based_on = Ch::Val(b.clone());
    }
    match &o.next {
        Ch::Keep => {}
        Ch::Null => head.next = Ch::Null,
        Ch::Val(n) => head.next = if n == id { Ch::Null } else { Ch::Val(n.clone()) },
    }
    if let Some(a) = o.auto_update {
        head.auto_redefine = Some(a);
    }
    if let Some(q) = o.quick {
        head.q_format = Some(q);
    }
    if f.has_run_changes() {
        head.r_pr = Ch::Val(patch_style_rpr(style_kid(&raw0, "w:rPr").as_deref(), f));
    }
    if meta.ty == "paragraph" && f.has_para_changes() {
        head.p_pr = Ch::Val(patch_style_ppr(style_kid(&raw0, "w:pPr").as_deref(), f));
    }
    let cur = style_raw(&out, id)?;
    out = put(&out, id, &patch_style_xml(&cur, &head));
    if let Some(q) = o.quick
        && !meta.custom
    {
        out = set_latent_q_format(&out, &meta.name, q);
    }
    // the Char twin of a linked style: the same rPr (Word updates both)
    let twin = meta.link.as_deref().and_then(|l| read_style_index(&out).styles.into_iter().find(|s| s.id == l && s.ty == "character"));
    if let Some(tw) = twin {
        let mut th = StyleHead::default();
        let mut any = false;
        if !matches!(head.r_pr, Ch::Keep) {
            th.r_pr = head.r_pr.clone();
            any = true;
        }
        if let Ch::Val(b) = &head.based_on {
            let l = read_style_index(&out).styles.iter().find(|s| s.id == *b).and_then(|s| s.link.clone());
            th.based_on = Ch::Val(l.unwrap_or_else(|| default_char_of(&out)));
            any = true;
        }
        if any && let Some(traw) = style_raw(&out, &tw.id) {
            out = put(&out, &tw.id, &patch_style_xml(&traw, &th));
        }
    }
    let mut renamed = Vec::new();
    if let Some(name) = &o.name {
        let tname = uo_jsnum::text::trim(name);
        let same = [UiLang::Pt, UiLang::En, UiLang::Es].iter().any(|l| display_name(&meta, *l) == *name) || *name == meta.name;
        if !tname.is_empty() && !same {
            let (x, r) = rename_style_xml(&out, id, tname)?;
            out = x;
            renamed = r;
        }
    }
    Some((out, renamed))
}

/// `renameStyleXml(ds, xml, id, name)`: a custom style changes name AND id (the document
/// follows); a built-in gets `w:aliases`. `None` = the name is in use (the TS throws).
pub(crate) fn rename_style_xml(xml: &str, id: &str, name: &str) -> Option<(String, Vec<(String, String)>)> {
    let mut out = ensure_defined(xml, id, false);
    let idx = read_style_index(&out);
    let Some(meta) = idx.styles.iter().find(|s| s.id == id).cloned() else { return Some((xml.to_owned(), Vec::new())) };
    if name.is_empty() {
        return Some((xml.to_owned(), Vec::new()));
    }
    let twin = meta.link.as_deref().and_then(|l| idx.styles.iter().find(|s| s.id == l && s.ty == "character" && s.custom)).cloned();
    if !meta.custom {
        let raw = style_raw(&out, id)?;
        out = put(&out, id, &patch_style_xml(&raw, &StyleHead { aliases: Ch::Val(name.to_owned()), ..StyleHead::default() }));
        if let Some(tw) = twin
            && let Some(traw) = style_raw(&out, &tw.id)
        {
            out =
                put(&out, &tw.id, &patch_style_xml(&traw, &StyleHead { aliases: Ch::Val(format!("{name} Char")), ..StyleHead::default() }));
        }
        return Some((out, Vec::new()));
    }
    let lname = lower(name);
    if idx.styles.iter().any(|s| s.id != id && lower(&s.name) == lname) {
        return None;
    }
    let mut taken: HashSet<String> =
        idx.styles.iter().filter(|s| s.id != id && Some(&s.id) != twin.as_ref().map(|t| &t.id)).map(|s| s.id.clone()).collect();
    let new_id = style_id_from_name(name, &taken);
    taken.insert(new_id.clone());
    let mut renamed: Vec<(String, String)> = Vec::new();
    if new_id != id {
        renamed.push((id.to_owned(), new_id.clone()));
    }
    let new_twin = twin.as_ref().map(|_| style_id_from_name(&format!("{new_id}Char"), &taken));
    if let (Some(tw), Some(nt)) = (&twin, &new_twin)
        && *nt != tw.id
    {
        renamed.push((tw.id.clone(), nt.clone()));
    }
    let raw = style_raw(&out, id)?;
    let head = StyleHead {
        style_id: Some(new_id.clone()),
        name: Some(name.to_owned()),
        link: new_twin.clone().map_or(Ch::Keep, Ch::Val),
        ..StyleHead::default()
    };
    out = put(&out, id, &patch_style_xml(&raw, &head));
    if let (Some(tw), Some(nt)) = (&twin, &new_twin)
        && let Some(traw) = style_raw(&out, &tw.id)
    {
        let h = StyleHead {
            style_id: Some(nt.clone()),
            name: Some(format!("{name} Char")),
            link: Ch::Val(new_id.clone()),
            ..StyleHead::default()
        };
        out = put(&out, &tw.id, &patch_style_xml(&traw, &h));
    }
    Some((retarget(&out, &renamed), renamed))
}

/// `retarget(xml, renamed)`: `basedOn`/`next`/`link` follow the renamed ids.
pub(crate) fn retarget(xml: &str, renamed: &[(String, String)]) -> String {
    if renamed.is_empty() {
        return xml.to_owned();
    }
    let tags = ["w:basedOn", "w:next", "w:link"];
    let mut out = String::with_capacity(xml.len());
    let mut at = 0;
    for (s, e, ti, v) in ref_tags(xml, &tags) {
        out.push_str(&xml[at..s]);
        match renamed.iter().find(|(f, _)| *f == v) {
            Some((_, to)) => out.push_str(&format!("<{} w:val=\"{to}\"/>", tags[ti])),
            None => out.push_str(&xml[s..e]),
        }
        at = e;
    }
    out.push_str(&xml[at..]);
    out
}

/// `deleteStyleXml(xml, id)`: deletes a CUSTOM style (and its twin); what was based on it goes to
/// its base; a `next` pointing at it goes. `None` for a built-in or the default (Word does not).
pub(crate) fn delete_style_xml(xml: &str, id: &str) -> Option<(String, Vec<String>)> {
    let idx = read_style_index(xml);
    let meta = idx.styles.iter().find(|s| s.id == id)?;
    if !meta.custom || meta.is_default {
        return None;
    }
    let twin = meta.link.as_deref().and_then(|l| idx.styles.iter().find(|s| s.id == l && s.custom));
    let mut removed = vec![id.to_owned()];
    if let Some(t) = twin {
        removed.push(t.id.clone());
    }
    let mut out = xml.to_owned();
    for r in &removed {
        out = drop_style(&out, r);
    }
    let gone = |v: &str| removed.iter().any(|r| r == v);
    // `basedOn` of whatever pointed at a removed style
    let mut s1 = String::with_capacity(out.len());
    let mut at = 0;
    for (s, e, _, v) in ref_tags(&out, &["w:basedOn"]) {
        s1.push_str(&out[at..s]);
        if gone(&v) {
            if v == id
                && let Some(b) = &meta.based_on
            {
                s1.push_str(&format!("<w:basedOn w:val=\"{b}\"/>"));
            } else if let Some(t) = twin.filter(|t| t.id == v)
                && let Some(b) = &t.based_on
            {
                s1.push_str(&format!("<w:basedOn w:val=\"{b}\"/>"));
            }
        } else {
            s1.push_str(&out[s..e]);
        }
        at = e;
    }
    s1.push_str(&out[at..]);
    // `next`/`link` pointing at a removed style
    let mut s2 = String::with_capacity(s1.len());
    let mut at = 0;
    for (s, e, _, v) in ref_tags(&s1, &["w:next", "w:link"]) {
        s2.push_str(&s1[at..s]);
        if !gone(&v) {
            s2.push_str(&s1[s..e]);
        }
        at = e;
    }
    s2.push_str(&s1[at..]);
    Some((s2, removed))
}

/// `setQuickXml(ds, xml, id, on)`: shows or hides a style in the gallery (a latent one only
/// changes its `lsdException`).
pub(crate) fn set_quick_xml(xml: &str, id: &str, on: bool) -> String {
    let idx = read_style_index(xml);
    let Some(meta) = idx.styles.iter().find(|s| s.id == id) else {
        let Some(b) = builtins().find(|x| id_of(x.xml) == id) else { return xml.to_owned() };
        if !on {
            return set_latent_q_format(xml, b.name, false);
        }
        let out = ensure_defined(xml, id, false);
        return if out == xml { xml.to_owned() } else { set_quick_xml(&out, id, true) };
    };
    let Some(raw) = style_raw(xml, id) else { return xml.to_owned() };
    let mut out = put(xml, id, &patch_style_xml(&raw, &StyleHead { q_format: Some(on), ..StyleHead::default() }));
    if !meta.custom {
        out = set_latent_q_format(&out, &meta.name, on);
    }
    out
}

// ───────────────────────── the list of styles ─────────────────────────

/// `StyleKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StyleKind {
    Paragraph,
    Character,
    Linked,
}

/// A style as the interface shows it (`StyleInfo`, without the use flag).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct StyleInfo {
    pub id: String,
    pub name: String,
    pub raw_name: String,
    pub kind: StyleKind,
    pub built_in: bool,
    pub defined: bool,
    pub quick: bool,
    pub hidden: bool,
    pub priority: f64,
}

/// `latentOf(ds, name)`: the document's `lsdException`, else the blank document's (parsed at most once
/// per call and only when a latent style is missing from the document's own list: no cache that
/// outlives the call).
fn latent_of<'a>(idx: &'a StyleIndex, blank: &'a std::cell::OnceCell<Vec<LatentMeta>>, name: &str) -> Option<&'a LatentMeta> {
    let k = lower(name);
    idx.latent.iter().find(|l| lower(&l.name) == k).or_else(|| {
        blank.get_or_init(|| read_style_index(crate::read::blank::BLANK_STYLES_XML).latent).iter().find(|l| lower(&l.name) == k)
    })
}

/// `listStyles(ds, lang, inUse, 'all')`: defined paragraph/character styles (not the Char twins
/// of linked ones nor the default character style) plus the latent built-ins, in definition order
/// (the TS sorts by priority then by ICU collation; no caller of this port depends on the
/// order).
pub(crate) fn list_styles_all(idx: &StyleIndex, lang: UiLang) -> Vec<StyleInfo> {
    let mut out = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    for m in &idx.styles {
        if m.ty != "paragraph" && m.ty != "character" {
            continue;
        }
        names.insert(lower(&m.name));
        // the Char twin of a linked paragraph style
        if m.ty == "character" && m.link.as_deref().and_then(|l| idx.styles.iter().find(|s| s.id == l)).is_some_and(|s| s.ty == "paragraph")
        {
            continue;
        }
        if m.ty == "character" && m.is_default {
            continue;
        }
        let kind = if m.ty == "character" {
            StyleKind::Character
        } else if m.link.as_deref().and_then(|l| idx.styles.iter().find(|s| s.id == l)).is_some_and(|s| s.ty == "character") {
            StyleKind::Linked
        } else {
            StyleKind::Paragraph
        };
        out.push(StyleInfo {
            id: m.id.clone(),
            name: display_name(m, lang),
            raw_name: m.name.clone(),
            kind,
            built_in: !m.custom,
            defined: true,
            quick: m.q_format,
            hidden: m.semi_hidden || m.hidden,
            priority: m.ui_priority,
        });
    }
    let taken: HashSet<&str> = idx.styles.iter().map(|s| s.id.as_str()).collect();
    let blank = std::cell::OnceCell::new();
    for b in builtins() {
        if names.contains(&lower(b.name)) {
            continue;
        }
        let id = id_of(b.xml);
        if taken.contains(id.as_str()) {
            continue;
        }
        let l = latent_of(idx, &blank, b.name);
        let is_char = b.xml.contains("w:type=\"character\"");
        let kind = if is_char {
            StyleKind::Character
        } else if b.twin.is_some_and(|t| t.contains("w:styleId=\"")) {
            StyleKind::Linked
        } else {
            StyleKind::Paragraph
        };
        out.push(StyleInfo {
            id,
            name: builtin_display_name(b.name, lang).unwrap_or_else(|| b.name.to_owned()),
            raw_name: b.name.to_owned(),
            kind,
            built_in: true,
            defined: false,
            quick: l.is_some_and(|l| l.q_format),
            hidden: l.is_some_and(|l| l.semi_hidden),
            priority: l.map_or(99.0, |l| l.ui_priority),
        });
    }
    out
}

// ───────────────────────── the part ─────────────────────────

const REL_NS_EMPTY: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>";

/// `attr="…"` in the text of one tag: the value.
fn tag_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let p = tag.find(&needle)?;
    let vs = p + needle.len();
    let c = tag.get(vs..)?.find('"')?;
    tag.get(vs..vs + c)
}

/// `stylesPartPath(pkg)`: the part the document relationship `…/styles` names.
fn styles_part_path(pkg: &Package) -> String {
    let rels = pkg.part_text("word/_rels/document.xml.rels").ok().flatten().map(|t| t.into_owned()).unwrap_or_default();
    let mut at = 0;
    while let Some(s) = super::super::lists::rx::next_open(&rels, "Relationship", at) {
        let Some(gt) = rels.get(s..).and_then(|r| r.find('>')).map(|p| s + p) else { break };
        let tag = &rels[s..=gt];
        at = gt + 1;
        // `Type="[^"]*\/styles"`
        let is_styles = tag_attr(tag, "Type").is_some_and(|t| t.ends_with("/styles")) && tag.contains("Type=\"");
        if !is_styles || tag.contains("TargetMode=\"External\"") {
            continue;
        }
        if let Some(t) = tag_attr(tag, "Target").filter(|t| !t.is_empty()) {
            return match t.strip_prefix('/') {
                Some(rest) => rest.to_owned(),
                None => format!("word/{}", t.strip_prefix("./").unwrap_or(t)),
            };
        }
    }
    "word/styles.xml".to_owned()
}

/// The text of the styles part the document relationship names (`None`: no such part). Wave 7c (S): the save compares
/// it with the style sheet (`save_parts.rs`).
pub(crate) fn styles_part_text(pkg: &Package) -> Result<Option<String>, DocxError> {
    Ok(pkg.part_text(&styles_part_path(pkg))?.map(std::borrow::Cow::into_owned))
}

/// `writeStylesPart(pkg, xml)`: the part, and for a new one its relationship and content type.
pub(crate) fn write_styles_part(pkg: &mut Package, xml: &str) -> Result<(), DocxError> {
    let path = styles_part_path(pkg);
    let existed = pkg.part_text(&path)?.is_some();
    pkg.set_part_text(path, xml.to_owned());
    if existed {
        return Ok(());
    }
    let rels_path = "word/_rels/document.xml.rels";
    let mut rels = pkg.part_text(rels_path)?.map_or_else(|| REL_NS_EMPTY.to_owned(), |t| t.into_owned());
    let has_styles_rel = {
        let mut f = false;
        let mut from = 0;
        while let Some(p) = rels.get(from..).and_then(|r| r.find("Type=\"")) {
            let vs = from + p + 6;
            if let Some(c) = rels.get(vs..).and_then(|r| r.find('"'))
                && rels[vs..vs + c].ends_with("/styles")
            {
                f = true;
                break;
            }
            from = vs;
        }
        f
    };
    if !has_styles_rel {
        let mut ids: HashSet<String> = HashSet::new();
        let mut from = 0;
        while let Some(p) = rels.get(from..).and_then(|r| r.find("Id=\"")) {
            let vs = from + p + 4;
            if let Some(c) = rels.get(vs..).and_then(|r| r.find('"')) {
                ids.insert(rels[vs..vs + c].to_owned());
            }
            from = vs;
        }
        let mut n = 1u64;
        while ids.contains(&format!("rId{n}")) {
            n += 1;
        }
        if let Some(p) = rels.find("</Relationships>") {
            rels.insert_str(
                p,
                &format!("<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>"),
            );
        }
        pkg.set_part_text(rels_path, rels);
    }
    if let Some(ct) = pkg.part_text("[Content_Types].xml")?.map(|t| t.into_owned())
        && !ct.contains("/word/styles.xml")
        && let Some(p) = ct.find("</Types>")
    {
        let mut ct = ct;
        ct.insert_str(p, "<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>");
        pkg.set_part_text("[Content_Types].xml", ct);
    }
    Ok(())
}
