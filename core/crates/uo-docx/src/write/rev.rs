//! The XML of the revisions generated at save (docx-core `rev-xml.ts`): `w:id` above the ids the
//! package already uses, author, date, `w16du:dateUtc`, and the namespaces the part root then
//! needs. The TS module state (`nextId`, `usedUtc`) is [`RevState`], one per save (G7).

use uo_jsnum::format::to_js_string;
use uo_jsnum::text::{is_js_space, split_js_space};
use uo_opc::Package;

use super::matchers::{find_from, has_xmlns};
use crate::error::DocxError;
use crate::model::props::RevisionInfo;

pub const W16DU_NS: &str = "http://schemas.microsoft.com/office/word/2023/wordml/word16du";
const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// `rev-xml.ts`'s module state, per save.
#[derive(Debug, Clone, PartialEq)]
pub struct RevState {
    /// the next `w:id` (a JS number: printed with `String(n)`)
    pub next_id: f64,
    /// a `w16du:dateUtc` was written since the last [`RevState::take_used_date_utc`]
    pub used_utc: bool,
}

impl Default for RevState {
    fn default() -> Self {
        RevState { next_id: 1.0, used_utc: false }
    }
}

/// `esc` of rev-xml.ts: `& < > "`.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

impl RevState {
    /// `beginRevisionIds(...xmls)`: ids above the largest `w:id` of the texts.
    pub fn begin(&mut self, texts: &[&str]) {
        self.next_id = super::matchers::max_w_id(texts) + 1.0;
        self.used_utc = false;
    }

    /// `takeUsedDateUtc()`.
    pub fn take_used_date_utc(&mut self) -> bool {
        std::mem::take(&mut self.used_utc)
    }

    /// `revisionAttrs(r)`: ` w:id="…" w:author="…"` (+ `w:date`, `w16du:dateUtc`).
    pub fn attrs(&mut self, r: &RevisionInfo) -> String {
        self.attrs_of(&r.author, r.date.as_deref(), r.date_utc.as_deref())
    }

    /// `revisionAttrs` of any object with `author`, `date`, `dateUtc` (a `FormatChange`, a
    /// `ParaFormatChange`).
    pub fn attrs_of(&mut self, author: &str, date: Option<&str>, date_utc: Option<&str>) -> String {
        let id = self.next_id;
        self.next_id += 1.0;
        let mut a = format!(" w:id=\"{}\" w:author=\"{}\"", to_js_string(id), esc(author));
        if let Some(d) = date.filter(|d| !d.is_empty()) {
            a.push_str(&format!(" w:date=\"{}\"", esc(d)));
        }
        if let Some(d) = date_utc.filter(|d| !d.is_empty()) {
            a.push_str(&format!(" w16du:dateUtc=\"{}\"", esc(d)));
            self.used_utc = true;
        }
        a
    }

    /// `markRevisionXml(kind, r)`: `<w:ins …/>`, `<w:del …/>` or `w:moveTo`/`w:moveFrom`.
    pub fn mark_xml(&mut self, kind: RevKind, r: &RevisionInfo) -> String {
        format!("<{}{}/>", kind.tag(r), self.attrs(r))
    }

    /// `wrapRevision(kind, r, inner)`.
    pub fn wrap(&mut self, kind: RevKind, r: &RevisionInfo, inner: &str) -> String {
        let tag = kind.tag(r);
        format!("<{tag}{}>{inner}</{tag}>", self.attrs(r))
    }
}

/// `'ins' | 'del'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevKind {
    Ins,
    Del,
}

impl RevKind {
    fn tag(self, r: &RevisionInfo) -> &'static str {
        match (self, r.r#move == Some(true)) {
            (RevKind::Ins, true) => "w:moveTo",
            (RevKind::Del, true) => "w:moveFrom",
            (RevKind::Ins, false) => "w:ins",
            (RevKind::Del, false) => "w:del",
        }
    }
}

/// `ensureRevisionNamespaces(open, rootName)`: `xmlns:w16du`, `xmlns:mc` and `mc:Ignorable`
/// with `w16du` on the root's start tag.
#[must_use]
pub fn ensure_revision_namespaces(open: &str, root_name: &str) -> String {
    let Some(at) = find_from(open, &format!("<{root_name}"), 0) else { return open.to_owned() };
    // `open.indexOf('>', at)`: −1 slices one unit before the end (`slice(at, -1)`, `slice(-1)`)
    let (mut tag, tail) = match find_from(open, ">", at) {
        Some(end) => (open[at..end].to_owned(), open[end..].to_owned()),
        None => super::parts::split_last_unit(&open[at..]),
    };
    if !has_xmlns(&tag, "w16du") {
        tag.push_str(&format!(" xmlns:w16du=\"{W16DU_NS}\""));
    }
    if !has_xmlns(&tag, "mc") {
        tag.push_str(&format!(" xmlns:mc=\"{MC_NS}\""));
    }
    match find_ignorable(&tag) {
        None => tag.push_str(" mc:Ignorable=\"w16du\""),
        Some((s, e, v)) => {
            if !split_js_space(v).any(|x| x == "w16du") {
                let v = v.to_owned();
                let rep = format!(" mc:Ignorable=\"{}w16du\"", if v.is_empty() { String::new() } else { format!("{v} ") });
                tag.replace_range(s..e, &rep);
            }
        }
    }
    format!("{}{tag}{tail}", &open[..at])
}

/// `/\smc:Ignorable="([^"]*)"/.exec(tag)`: the span of the match and the value.
fn find_ignorable(tag: &str) -> Option<(usize, usize, &str)> {
    const NAME: &str = "mc:Ignorable=\"";
    let mut from = 0;
    while let Some(i) = find_from(tag, NAME, from) {
        from = i + 1;
        let Some((s, c)) = tag[..i].char_indices().next_back() else { continue };
        if !is_js_space(c) {
            continue;
        }
        let v = &tag[i + NAME.len()..];
        if let Some(q) = v.find('"') {
            return Some((s, i + NAME.len() + q + 1, &v[..q]));
        }
    }
    None
}

const BEFORE_TRACK: &[&str] = &[
    "w:writeProtection",
    "w:view",
    "w:zoom",
    "w:removePersonalInformation",
    "w:removeDateAndTime",
    "w:doNotDisplayPageBoundaries",
    "w:displayBackgroundShape",
    "w:printPostScriptOverText",
    "w:printFractionalCharacterWidth",
    "w:printFormsData",
    "w:embedTrueTypeFonts",
    "w:embedSystemFonts",
    "w:saveSubsetFonts",
    "w:saveFormsData",
    "w:mirrorMargins",
    "w:alignBordersAndEdges",
    "w:bordersDoNotSurroundHeader",
    "w:bordersDoNotSurroundFooter",
    "w:gutterAtTop",
    "w:hideSpellingErrors",
    "w:hideGrammaticalErrors",
    "w:activeWritingStyle",
    "w:proofState",
    "w:formsDesign",
    "w:attachedTemplate",
    "w:linkStyles",
    "w:stylePaneFormatFilter",
    "w:stylePaneSortMethod",
    "w:documentType",
    "w:mailMerge",
    "w:revisionView",
];

/// `readTrackRevisions` over a settings text: `/<w:trackRevisions\b([^>]*?)\/?>/` and not
/// `/w:val="(?:false|0|off)"/` in the attributes.
#[must_use]
pub fn track_revisions_on(xml: &str) -> bool {
    const OPEN: &str = "<w:trackRevisions";
    let mut from = 0;
    while let Some(i) = find_from(xml, OPEN, from) {
        from = i + 1;
        if !super::matchers::boundary_at(xml, i + OPEN.len()) {
            continue;
        }
        let Some(gt) = find_from(xml, ">", i + OPEN.len()) else { return false };
        // the lazy group stops before a `/` right before the `>`
        let mut attrs = &xml[i + OPEN.len()..gt];
        if let Some(a) = attrs.strip_suffix('/') {
            attrs = a;
        }
        return !["w:val=\"false\"", "w:val=\"0\"", "w:val=\"off\""].iter().any(|n| attrs.contains(n));
    }
    false
}

/// `writeTrackRevisions(pkg, on)`, the settings part found through the document's relationships
/// (`partByRelType`, D6). Errors are the package's.
pub fn write_track_revisions(pkg: &mut Package, settings_path: Option<&str>, on: bool) -> Result<(), DocxError> {
    let current = match settings_path {
        Some(p) => pkg.part_text(p)?.map(std::borrow::Cow::into_owned),
        None => None,
    };
    if track_revisions_on(current.as_deref().unwrap_or("")) == on {
        return Ok(());
    }
    let mut path = settings_path.unwrap_or("").to_owned();
    let mut xml = current.filter(|x| !x.is_empty());
    if xml.is_none() {
        if !on {
            return Ok(());
        }
        path = "word/settings.xml".to_owned();
        xml = Some(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"></w:settings>"
                .to_owned(),
        );
        let rels_path = "word/_rels/document.xml.rels";
        let rels = pkg.part_text(rels_path)?.map_or_else(|| super::parts::EMPTY_RELS.to_owned(), std::borrow::Cow::into_owned);
        let rel = "<Relationship Id=\"rIdWnSettings\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>";
        pkg.set_part_text(rels_path, super::parts::insert_before_first(&rels, "</Relationships>", rel));
        if let Some(ct) = pkg.part_text(super::parts::CONTENT_TYPES)?.map(std::borrow::Cow::into_owned)
            && !ct.is_empty()
            && !ct.contains("/word/settings.xml")
        {
            let ov = "<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>";
            pkg.set_part_text(super::parts::CONTENT_TYPES, super::parts::insert_before_first(&ct, "</Types>", ov));
        }
    }
    let xml = xml.unwrap_or_default();
    let mut xml = remove_track_revisions(&xml);
    if on {
        let doc = uo_opc::xml::parse(xml.as_str(), &pkg.limits().bounded(&crate::read::DOCX_POS)).map_err(DocxError::from)?;
        let root = doc.root();
        let pos = root.positions().ok_or(DocxError::Corrupt("settings root without positions"))?;
        if pos.content_start == pos.end {
            let open = &xml[pos.start..pos.end];
            let open = super::lossless::self_close_to_open_tail(open);
            xml = format!("{}{open}<w:trackRevisions/></w:settings>{}", &xml[..pos.start], &xml[pos.end..]);
        } else {
            let mut at = pos.content_start;
            for k in root.child_elements() {
                if BEFORE_TRACK.contains(&k.name())
                    && let Some(kp) = k.positions()
                {
                    at = at.max(kp.end);
                }
            }
            xml.insert_str(at, "<w:trackRevisions/>");
        }
    }
    pkg.set_part_text(path, xml);
    Ok(())
}

/// `xml.replace(/<w:trackRevisions\b[^>]*?(?:\/>|>[\s\S]*?<\/w:trackRevisions>)/g, '')`.
fn remove_track_revisions(xml: &str) -> String {
    super::matchers::remove_elements_lazy(xml, "w:trackRevisions")
}
