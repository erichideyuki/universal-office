//! `DocxDocument`: a .docx opened for editing (C# `Editing/DocxDocument.cs`, `DocxStructure.cs`).
//!
//! The package is the uo-opc [`Package`] (untouched parts leave byte for byte); XML parts are
//! parsed on first use into the document's arena ([`Dom`]) and re-serialized on save only
//! when dirty. Part lookup is the C# `FindPart` (exact name, then `OrdinalIgnoreCase`);
//! relationship targets go through the uo-opc D6 rule (`part_name::resolve_relationship_target`,
//! the port of the C# `OpcPartName`).

use indexmap::IndexMap;
use uo_opc::Package;
use uo_opc::package::OpenOptions;
use uo_opc::part_name;

use super::dom::{Declaration, Dom, NS_CT, NS_PKG_RELS, NodeId, n};
use super::error::{ApiError, Limit, Result};
use super::net;
use super::ooxml::{content_types, rel_types};
use super::read;
use super::write;

/// Resource caps of a document opened from untrusted input (C# `DocxLimits`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocxLimits {
    /// uncompressed bytes of one part (C# `OpcPackageLimits.MaxPartBytes`, 256 MiB)
    pub max_part_bytes: u64,
    /// uncompressed bytes of all parts (C# `MaxTotalBytes`, 512 MiB)
    pub max_total_bytes: u64,
    /// `XmlReader.Depth` of any node of a part (root = 0), 256
    pub max_xml_depth: u32,
    /// complex fields per story, 100,000
    pub max_fields_per_story: u32,
    /// complex fields open at once in a story, 64
    pub max_field_nesting: u32,
    /// Rust-only caps (uo-opc): archive entries and elements of one XML part
    pub max_entries: u32,
    pub max_xml_elements: u32,
}

impl Default for DocxLimits {
    fn default() -> Self {
        DocxLimits {
            max_part_bytes: 256 << 20,
            max_total_bytes: 512 << 20,
            max_xml_depth: 256,
            max_fields_per_story: 100_000,
            max_field_nesting: 64,
            max_entries: 65_535,
            max_xml_elements: 20_000_000,
        }
    }
}

impl DocxLimits {
    fn package(&self) -> uo_opc::Limits {
        uo_opc::Limits {
            max_xml_part_bytes: self.max_part_bytes,
            max_binary_part_bytes: self.max_part_bytes,
            max_total_bytes: self.max_total_bytes,
            max_entries: self.max_entries,
            // the XML depth is checked by the strict parse itself (`XmlReader.Depth`)
            max_xml_depth: self.max_xml_depth.saturating_add(1).max(1),
            max_xml_elements: self.max_xml_elements,
        }
    }
}

/// A relationship of a part (C# `DocxRelationship`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    pub rel_type: String,
    pub target: String,
    pub external: bool,
}

#[derive(Debug, Clone)]
struct XmlPart {
    name: String,
    doc: NodeId,
}

/// The id source of the few random ids the C# draws (`RandomNumberGenerator`: `w14:paraId` and
/// `w:sdt` ids on a collision, every new `w:nsid`): xorshift64*, seeded per document from the
/// process's random hash keys, settable for tests ([`DocxDocument::set_seed`]).
#[derive(Debug, Clone)]
pub(crate) struct Ids {
    state: u64,
}

impl Ids {
    fn new() -> Self {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(0x9E37_79B9_7F4A_7C15);
        Ids { state: h.finish() | 1 }
    }

    /// `RandomNumberGenerator.GetInt32(lo, hi)`: uniform in `[lo, hi)`.
    pub(crate) fn next_in(&mut self, lo: i32, hi: i32) -> i32 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let span = u64::try_from(i64::from(hi) - i64::from(lo)).unwrap_or(1).max(1);
        let v = i64::from(lo) + i64::try_from(r % span).unwrap_or(0);
        i32::try_from(v).unwrap_or(lo)
    }
}

/// A .docx opened for editing (C# `DocxDocument`).
pub struct DocxDocument {
    pub(crate) pkg: Package,
    pub(crate) dom: Dom,
    /// parsed XML parts by `OrdinalIgnoreCase` key, in the order they were loaded
    xml: IndexMap<String, XmlPart>,
    structure_changed: bool,
    main_part: String,
    limits: DocxLimits,
    pub(crate) ids: Ids,
}

impl std::fmt::Debug for DocxDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocxDocument").field("main_part", &self.main_part).field("parts", &self.pkg.len()).finish()
    }
}

impl DocxDocument {
    /// Opens a .docx (also .docm/.dotx; saved in the same type) with the default limits.
    pub fn open(bytes: &[u8]) -> Result<DocxDocument> {
        DocxDocument::open_with(bytes, DocxLimits::default())
    }

    /// Opens a .docx with the given limits. Every part is checked against the caps at open (the
    /// C# reads all entries at load).
    pub fn open_with(bytes: &[u8], limits: DocxLimits) -> Result<DocxDocument> {
        let opts = OpenOptions { utf16_text: true, ..OpenOptions::default() };
        let mut pkg = Package::open(bytes, &limits.package(), &opts).map_err(|e| match e {
            e @ uo_opc::Error::Limit { .. } => ApiError::from(e),
            other => ApiError::invalid_data(format!("not a valid OOXML package: {other}")),
        })?;
        pkg.verify().map_err(|e| match e {
            e @ uo_opc::Error::Limit { .. } => ApiError::from(e),
            other => ApiError::invalid_data(format!("not a valid OOXML package: {other}")),
        })?;
        // the C# skips directory entries
        let dirs: Vec<String> = pkg.part_names().filter(|p| p.ends_with('/')).map(str::to_owned).collect();
        for d in dirs {
            pkg.remove_part(&d);
        }
        let mut doc = DocxDocument {
            pkg,
            dom: Dom::new(),
            xml: IndexMap::new(),
            structure_changed: false,
            main_part: String::new(),
            limits,
            ids: Ids::new(),
        };
        doc.main_part =
            doc.resolve_main_part()?.ok_or_else(|| ApiError::invalid_data("the package has no main document part (officeDocument)"))?;
        let main = doc.main_part.clone();
        let root = doc.root_of(&main)?;
        if root.and_then(|r| doc.dom.element(r, n::body)).is_none() {
            return Err(ApiError::invalid_data("the main part is not a WordprocessingML document (w:document/w:body)"));
        }
        Ok(doc)
    }

    /// The limits the document was opened with.
    pub fn limits(&self) -> DocxLimits {
        self.limits
    }

    /// The main part (normally `word/document.xml`).
    pub fn main_part_path(&self) -> &str {
        &self.main_part
    }

    /// Seeds the random ids (tests; the C# draws them from the system RNG).
    pub fn set_seed(&mut self, seed: u64) {
        self.ids.state = seed | 1;
    }

    /// The arena (read access for views and tests).
    pub fn dom(&self) -> &Dom {
        &self.dom
    }

    pub fn dom_mut(&mut self) -> &mut Dom {
        &mut self.dom
    }

    /// Saves the package: dirty XML parts are re-serialized, the others leave as they came.
    /// Normalization and garbage collection run first when anything changed.
    pub fn save(&mut self) -> Result<Vec<u8>> {
        self.save_in_place()?;
        self.pkg.save(&uo_opc::package::SaveOptions::default()).map_err(ApiError::from)
    }

    /// What [`DocxDocument::save`] writes, as (name, bytes) in package order, without the zip
    /// (parity tests compare these).
    pub fn save_parts(&mut self) -> Result<Vec<(String, Vec<u8>)>> {
        self.save_in_place()?;
        let names: Vec<String> = self.pkg.part_names().map(str::to_owned).collect();
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let bytes = self.pkg.part(&name).map_err(ApiError::from)?.map(<[u8]>::to_vec).unwrap_or_default();
            out.push((name, bytes));
        }
        Ok(out)
    }

    fn save_in_place(&mut self) -> Result<()> {
        if self.structure_changed || self.xml.values().any(|p| self.dom.is_dirty(p.doc)) {
            super::structure::normalize_document(self)?;
            self.collect_garbage()?;
        }
        let parts: Vec<XmlPart> = self.xml.values().cloned().collect();
        for p in parts {
            if !self.dom.is_dirty(p.doc) {
                continue;
            }
            let bytes = write::serialize(&mut self.dom, p.doc)?;
            self.pkg.set_part(p.name.clone(), bytes);
            self.dom.set_dirty(p.doc, false);
        }
        self.structure_changed = false;
        Ok(())
    }

    /// An independent copy (the content of the current save).
    pub fn try_clone(&mut self) -> Result<DocxDocument> {
        let bytes = self.save()?;
        DocxDocument::open_with(&bytes, self.limits)
    }

    // ─────────────────────────────── parts ───────────────────────────────

    pub fn part_names(&self) -> Vec<String> {
        self.pkg.part_names().map(str::to_owned).collect()
    }

    pub fn has_part(&self, path: &str) -> bool {
        self.find_part(path).is_some()
    }

    /// The bytes of a part (a dirty XML part is serialized, as the C# does, which also
    /// ensures its root's namespace declarations).
    pub fn get_part_bytes(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(real) = self.find_part(path) else { return Ok(None) };
        if let Some(p) = self.xml.get(&net::fold_key(&real)).cloned()
            && self.dom.is_dirty(p.doc)
        {
            return write::serialize(&mut self.dom, p.doc).map(Some);
        }
        Ok(self.pkg.part(&real).map_err(ApiError::from)?.map(<[u8]>::to_vec))
    }

    /// C# `FindPart`: the exact name, else the first name equal under `OrdinalIgnoreCase`.
    pub fn find_part(&self, path: &str) -> Option<String> {
        if self.pkg.contains(path) {
            return Some(path.to_owned());
        }
        self.pkg.part_names().find(|p| net::eq_ignore_case(p, path)).map(str::to_owned)
    }

    /// The document node of an XML part, parsed once (C# `Xml`: missing is `InvalidData`).
    pub fn xml(&mut self, path: &str) -> Result<NodeId> {
        self.try_xml(path)?.ok_or_else(|| ApiError::invalid_data(format!("missing part: {path}")))
    }

    /// C# `TryXml`.
    pub fn try_xml(&mut self, path: &str) -> Result<Option<NodeId>> {
        let Some(real) = self.find_part(path) else { return Ok(None) };
        let key = net::fold_key(&real);
        if let Some(p) = self.xml.get(&key) {
            return Ok(Some(p.doc));
        }
        let doc = read::parse_part(&mut self.dom, &self.pkg, &real, self.limits.max_xml_depth)?;
        self.xml.insert(key, XmlPart { name: real, doc });
        Ok(Some(doc))
    }

    /// The root element of an XML part (`Xml(path).Root`), if the part exists.
    pub fn root_of(&mut self, path: &str) -> Result<Option<NodeId>> {
        Ok(self.try_xml(path)?.and_then(|d| self.dom.root(d)))
    }

    fn track(&mut self, path: &str, doc: NodeId) {
        self.dom.set_dirty(doc, false);
        self.xml.insert(net::fold_key(path), XmlPart { name: path.to_owned(), doc });
    }

    /// Creates (or replaces) an XML part with the content type (C# `SetXmlPart`): serialized
    /// right away, tracked clean.
    pub(crate) fn set_xml_part(&mut self, path: &str, doc: NodeId, content_type: Option<&str>) -> Result<()> {
        if self.dom.declaration(doc).is_none() {
            self.dom.set_declaration(
                doc,
                Some(Declaration { version: Some("1.0".into()), encoding: Some("UTF-8".into()), standalone: Some("yes".into()) }),
            );
        }
        let real = self.find_part(path).unwrap_or_else(|| path.to_owned());
        let bytes = write::serialize(&mut self.dom, doc)?;
        self.pkg.set_part(real.clone(), bytes);
        self.track(&real, doc);
        if let Some(ct) = content_type {
            self.set_content_type(&real, ct)?;
        }
        self.structure_changed = true;
        Ok(())
    }

    /// Creates (or replaces) a binary part (C# `SetBinaryPart`).
    pub(crate) fn set_binary_part(&mut self, path: &str, data: Vec<u8>, content_type: Option<&str>) -> Result<()> {
        let real = self.find_part(path).unwrap_or_else(|| path.to_owned());
        self.xml.shift_remove(&net::fold_key(&real));
        self.pkg.set_part(real.clone(), data);
        if let Some(ct) = content_type {
            self.set_content_type(&real, ct)?;
        }
        self.structure_changed = true;
        Ok(())
    }

    /// Removes the part, its `.rels` and its content-type override (C# `DeletePart`).
    pub(crate) fn delete_part(&mut self, path: &str) -> Result<()> {
        let Some(real) = self.find_part(path) else { return Ok(()) };
        self.xml.shift_remove(&net::fold_key(&real));
        self.pkg.remove_part(&real);
        let rels = rels_path(&real);
        if let Some(real_rels) = self.find_part(&rels) {
            self.xml.shift_remove(&net::fold_key(&real_rels));
            self.pkg.remove_part(&real_rels);
        }
        self.remove_content_type_override(&real)?;
        self.structure_changed = true;
        Ok(())
    }

    /// A free path: `word/media/image.png` → `image1.png`, `image2.png`… (C# `UniquePartPath`;
    /// the stem's trailing .NET `\d+` is dropped first).
    pub fn unique_part_path(&self, desired: &str) -> String {
        if self.find_part(desired).is_none() {
            return desired.to_owned();
        }
        let dir = desired.rfind('/').map_or("", |k| desired.get(..=k).unwrap_or(""));
        let file = desired.get(dir.len()..).unwrap_or("");
        let ext = extension(file);
        let stem = file.get(..file.len() - ext.len()).unwrap_or("");
        let base = strip_trailing_digits(stem);
        let mut i: u64 = 1;
        loop {
            let candidate = format!("{dir}{base}{i}{ext}");
            if self.find_part(&candidate).is_none() {
                return candidate;
            }
            i += 1;
        }
    }

    // ─────────────────────────────── relationships ───────────────────────────────

    /// The relationships of a part (`""` = the package), in file order.
    pub fn get_relationships(&mut self, part: &str) -> Result<Vec<Relationship>> {
        let Some(root) = self.root_of(&rels_path(part))? else { return Ok(Vec::new()) };
        let dom = &self.dom;
        Ok(dom
            .elements_named(root, n::Relationship)
            .into_iter()
            .map(|e| Relationship {
                id: dom.attr(e, n::Id).unwrap_or("").to_owned(),
                rel_type: dom.attr(e, n::Type).unwrap_or("").to_owned(),
                target: dom.attr(e, n::Target).unwrap_or("").to_owned(),
                external: dom.attr(e, n::TargetMode).is_some_and(|m| net::eq_ignore_case(m, "External")),
            })
            .collect())
    }

    pub fn get_relationship(&mut self, part: &str, id: &str) -> Result<Option<Relationship>> {
        Ok(self.get_relationships(part)?.into_iter().find(|r| r.id == id))
    }

    pub(crate) fn find_relationship(&mut self, part: &str, suffix: &str) -> Result<Option<Relationship>> {
        Ok(self.get_relationships(part)?.into_iter().find(|r| rel_types::is(&r.rel_type, suffix)))
    }

    /// The part an internal relationship points to (`None` if external or missing).
    pub fn resolve_target(&self, part: &str, rel: &Relationship) -> Option<String> {
        if rel.external {
            return None;
        }
        let name = part_name::resolve_relationship_target(part, &rel.target)?;
        self.find_part(&name)
    }

    pub(crate) fn find_related_part(&mut self, part: &str, suffix: &str) -> Result<Option<String>> {
        Ok(self.find_relationship(part, suffix)?.and_then(|r| self.resolve_target(part, &r)))
    }

    fn rels_document(&mut self, part: &str, create: bool) -> Result<Option<NodeId>> {
        let path = rels_path(part);
        if let Some(d) = self.try_xml(&path)? {
            return Ok(Some(d));
        }
        if !create {
            return Ok(None);
        }
        let doc = self.dom.new_document(Some(Declaration {
            version: Some("1.0".into()),
            encoding: Some("UTF-8".into()),
            standalone: Some("yes".into()),
        }));
        let root = self.dom.new_element(n::Relationships);
        self.dom.append(doc, root);
        self.set_xml_part(&path, doc, None)?;
        Ok(Some(doc))
    }

    /// Adds a relationship with a new id (`rIdN` above the existing ones).
    pub(crate) fn add_relationship(&mut self, part: &str, ty: &str, target: &str, external: bool) -> Result<String> {
        let doc = self.rels_document(part, true)?.ok_or_else(|| ApiError::invalid_operation("no relationships part"))?;
        let root = self.dom.root(doc).ok_or_else(|| ApiError::invalid_operation("empty relationships part"))?;
        let ids: Vec<String> =
            self.dom.elements_named(root, n::Relationship).into_iter().map(|e| self.dom.attr(e, n::Id).unwrap_or("").to_owned()).collect();
        let id = next_rel_id(ids.iter().map(String::as_str));
        self.add_relationship_with_id(part, &id, ty, target, external)?;
        Ok(id)
    }

    /// Adds a relationship with the given id.
    pub(crate) fn add_relationship_with_id(&mut self, part: &str, id: &str, ty: &str, target: &str, external: bool) -> Result<()> {
        let doc = self.rels_document(part, true)?.ok_or_else(|| ApiError::invalid_operation("no relationships part"))?;
        let root = self.dom.root(doc).ok_or_else(|| ApiError::invalid_operation("empty relationships part"))?;
        let el = self.dom.new_element_with(n::Relationship, &[(n::Id, id), (n::Type, ty), (n::Target, target)]);
        if external {
            self.dom.set_attr(el, n::TargetMode, Some("External"));
        }
        self.dom.append(root, el);
        Ok(())
    }

    pub(crate) fn remove_relationship(&mut self, part: &str, id: &str) -> Result<()> {
        let Some(doc) = self.rels_document(part, false)? else { return Ok(()) };
        let Some(root) = self.dom.root(doc) else { return Ok(()) };
        let doomed: Vec<NodeId> =
            self.dom.elements_named(root, n::Relationship).into_iter().filter(|&e| self.dom.attr(e, n::Id) == Some(id)).collect();
        for e in doomed {
            self.dom.remove_if_attached(e);
        }
        Ok(())
    }

    // ─────────────────────────────── content types ───────────────────────────────

    fn content_types_root(&mut self) -> Result<NodeId> {
        let doc = self.xml("[Content_Types].xml")?;
        self.dom.root(doc).ok_or_else(|| ApiError::invalid_data("[Content_Types].xml has no root"))
    }

    pub fn get_content_type(&mut self, part: &str) -> Result<Option<String>> {
        let real = self.find_part(part).unwrap_or_else(|| part.to_owned());
        let root = self.content_types_root()?;
        let dom = &self.dom;
        for o in dom.elements_named(root, n::Override) {
            if dom.attr(o, n::PartName).is_some_and(|p| net::eq_ignore_case(p.trim_start_matches('/'), &real)) {
                return Ok(dom.attr(o, n::ContentType).map(str::to_owned));
            }
        }
        let ext = extension(&real).trim_start_matches('.');
        for d in dom.elements_named(root, n::Default) {
            if dom.attr(d, n::Extension).is_some_and(|e| net::eq_ignore_case(e, ext)) {
                return Ok(dom.attr(d, n::ContentType).map(str::to_owned));
            }
        }
        Ok(None)
    }

    /// Makes the part resolve to the content type (an override only where the default does not).
    pub(crate) fn set_content_type(&mut self, part: &str, ct: &str) -> Result<()> {
        self.remove_content_type_override(part)?;
        if self.get_content_type(part)?.is_some_and(|c| net::eq_ignore_case(&c, ct)) {
            return Ok(());
        }
        let root = self.content_types_root()?;
        let ext = extension(part).trim_start_matches('.').to_owned();
        let defaults = self.dom.elements_named(root, n::Default);
        let has_default = defaults.iter().any(|&d| self.dom.attr(d, n::Extension).is_some_and(|e| net::eq_ignore_case(e, &ext)));
        if !has_default && !ext.is_empty() && !net::ends_with_ignore_case(ct, "+xml") {
            let el = self.dom.new_element_with(n::Default, &[(n::Extension, &ext), (n::ContentType, ct)]);
            match defaults.last() {
                Some(&last) => {
                    self.dom.add_after(last, &[el])?;
                }
                None => {
                    self.dom.prepend(root, &[el]);
                }
            }
            return Ok(());
        }
        let el = self.dom.new_element_with(n::Override, &[(n::PartName, &format!("/{part}")), (n::ContentType, ct)]);
        self.dom.append(root, el);
        Ok(())
    }

    fn remove_content_type_override(&mut self, part: &str) -> Result<()> {
        let root = self.content_types_root()?;
        let doomed: Vec<NodeId> = self
            .dom
            .elements_named(root, n::Override)
            .into_iter()
            .filter(|&o| self.dom.attr(o, n::PartName).is_some_and(|p| net::eq_ignore_case(p.trim_start_matches('/'), part)))
            .collect();
        for o in doomed {
            self.dom.remove_if_attached(o);
        }
        Ok(())
    }

    // ─────────────────────────────── shortcuts ───────────────────────────────

    /// The root of the main part.
    pub fn main_root(&mut self) -> Result<NodeId> {
        let main = self.main_part.clone();
        self.root_of(&main)?.ok_or_else(|| ApiError::invalid_data("the main part has no root"))
    }

    /// `w:body` of the main part.
    pub fn body(&mut self) -> Result<NodeId> {
        let root = self.main_root()?;
        self.dom.element(root, n::body).ok_or_else(|| ApiError::invalid_data("the main part has no w:body"))
    }

    /// The `w:sectPr` child of the body (the last section), if any.
    pub fn last_section_properties(&mut self) -> Result<Option<NodeId>> {
        let body = self.body()?;
        Ok(self.dom.elements_named(body, n::sectPr).last().copied())
    }

    /// A part related to the main part by type (styles, numbering, settings…).
    pub fn main_related_part(&mut self, suffix: &str) -> Result<Option<String>> {
        let main = self.main_part.clone();
        self.find_related_part(&main, suffix)
    }

    /// The root of a part related to the main part, created empty when `create`.
    pub(crate) fn main_related_root(
        &mut self,
        suffix: &str,
        create: bool,
        default_path: &str,
        content_type: &str,
        root_name: super::dom::Name,
    ) -> Result<Option<NodeId>> {
        if let Some(path) = self.main_related_part(suffix)? {
            let d = self.xml(&path)?;
            return Ok(self.dom.root(d));
        }
        if !create {
            return Ok(None);
        }
        let new_path = self.unique_part_path(default_path);
        let doc = self.dom.new_document(Some(Declaration {
            version: Some("1.0".into()),
            encoding: Some("UTF-8".into()),
            standalone: Some("yes".into()),
        }));
        let w_uri = self.dom.ns_uri(super::dom::NS_W).to_owned();
        let r_uri = self.dom.ns_uri(super::dom::NS_R).to_owned();
        let root = self.dom.new_element_with(root_name, &[(n::xmlns_w, &w_uri), (n::xmlns_r, &r_uri)]);
        self.dom.append(doc, root);
        self.set_xml_part(&new_path, doc, Some(content_type))?;
        let main = self.main_part.clone();
        let target = relative_target(&main, &new_path);
        self.add_relationship(&main, &format!("{}{suffix}", rel_types::OFFICE_DOCUMENT_BASE), &target, false)?;
        Ok(self.dom.root(doc))
    }

    /// The story parts: main, headers, footers, footnotes, endnotes (C# `StoryParts`).
    pub(crate) fn story_parts(&mut self, hf: bool, notes: bool) -> Result<Vec<String>> {
        let main = self.main_part.clone();
        let mut out = vec![main.clone()];
        for rel in self.get_relationships(&main)? {
            let is_hf = rel_types::is(&rel.rel_type, "header") || rel_types::is(&rel.rel_type, "footer");
            let is_notes = rel_types::is(&rel.rel_type, "footnotes") || rel_types::is(&rel.rel_type, "endnotes");
            if !(is_hf && hf) && !(is_notes && notes) {
                continue;
            }
            if let Some(target) = self.resolve_target(&main, &rel)
                && self.try_xml(&target)?.is_some()
            {
                out.push(target);
            }
        }
        Ok(out)
    }

    /// The roots of the story parts, distinct by `OrdinalIgnoreCase` (C# `Stories`).
    pub(crate) fn stories(&mut self, hf: bool, notes: bool) -> Result<Vec<(String, NodeId)>> {
        let parts = self.story_parts(hf, notes)?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for p in parts {
            if !seen.insert(net::fold_key(&p)) {
                continue;
            }
            if let Some(root) = self.root_of(&p)? {
                out.push((p, root));
            }
        }
        Ok(out)
    }

    /// Marks the package structure changed (normalization and collection at the next save).
    pub(crate) fn mark_changed(&mut self) {
        self.structure_changed = true;
    }

    // ─────────────────────────────── garbage collection ───────────────────────────────

    fn is_xml_part(&mut self, part: &str) -> Result<bool> {
        Ok(self.get_content_type(part)?.is_some_and(|ct| net::ends_with_ignore_case(&ct, "xml") || net::ends_with_ignore_case(&ct, "+xml")))
    }

    /// C# `CollectGarbage`: drops relationships nobody references any more (ids searched in
    /// every attribute of the part) and the parts no longer reachable from `_rels/.rels`.
    pub(crate) fn collect_garbage(&mut self) -> Result<()> {
        for part in self.part_names() {
            if net::ends_with_ignore_case(&part, ".rels") || part == "[Content_Types].xml" {
                continue;
            }
            let rels = self.get_relationships(&part)?;
            if !rels.iter().any(|r| rel_types::is_referenced(rel_types::last(&r.rel_type))) {
                continue;
            }
            if !self.is_xml_part(&part)? {
                continue;
            }
            let doc = match self.xml(&part) {
                Ok(d) => d,
                Err(ApiError { kind: super::error::ErrorKind::Xml, .. }) => continue,
                Err(e) => return Err(e),
            };
            let mut used = std::collections::HashSet::new();
            for el in self.dom.descendants(doc) {
                for a in self.dom.attrs(el) {
                    used.insert(a.value.clone());
                }
            }
            for rel in rels {
                if rel_types::is_referenced(rel_types::last(&rel.rel_type)) && !used.contains(&rel.id) {
                    self.remove_relationship(&part, &rel.id)?;
                }
            }
        }
        let mut reachable = std::collections::HashSet::new();
        let mut pending: Vec<String> = Vec::new();
        for rel in self.get_relationships("")? {
            if let Some(t) = self.resolve_target("", &rel)
                && reachable.insert(net::fold_key(&t))
            {
                pending.push(t);
            }
        }
        while let Some(part) = pending.pop() {
            for rel in self.get_relationships(&part)? {
                if let Some(t) = self.resolve_target(&part, &rel)
                    && reachable.insert(net::fold_key(&t))
                {
                    pending.push(t);
                }
            }
        }
        for part in self.part_names() {
            if part == "[Content_Types].xml" || net::ends_with_ignore_case(&part, ".rels") {
                continue;
            }
            if !reachable.contains(&net::fold_key(&part)) {
                self.delete_part(&part)?;
            }
        }
        for part in self.part_names() {
            if !net::ends_with_ignore_case(&part, ".rels") || part == "_rels/.rels" {
                continue;
            }
            let source = source_of_rels(&part)?;
            if self.find_part(&source).is_none() {
                self.xml.shift_remove(&net::fold_key(&part));
                self.pkg.remove_part(&part);
            }
        }
        Ok(())
    }

    fn resolve_main_part(&mut self) -> Result<Option<String>> {
        for rel in self.get_relationships("")? {
            if rel.external || !net::eq_ignore_case(&rel.rel_type, rel_types::OFFICE_DOCUMENT) {
                continue;
            }
            if let Some(t) = self.resolve_target("", &rel) {
                return Ok(Some(t));
            }
        }
        Ok(self.find_part("word/document.xml"))
    }

    /// The `[Content_Types].xml` root name check of the vocabulary (keeps `NS_CT`/`NS_PKG_RELS`
    /// referenced for the readers of this module).
    #[doc(hidden)]
    pub fn vocabulary_namespaces() -> (super::dom::Ns, super::dom::Ns) {
        (NS_CT, NS_PKG_RELS)
    }

    /// Bytes held by the parsed parts (memory tests).
    pub fn dom_bytes(&self) -> usize {
        self.dom.heap_bytes()
    }

    /// Fails with the field limits (helper of the field scans).
    pub(crate) fn field_limit(&self, limit: Limit, part: &str) -> ApiError {
        let max = match limit {
            Limit::FieldsPerStory => u64::from(self.limits.max_fields_per_story),
            _ => u64::from(self.limits.max_field_nesting),
        };
        ApiError::limit(limit, max, Some(part))
    }
}

/// The `.rels` path of a part (`""` = the package root).
pub fn rels_path(part: &str) -> String {
    if part.is_empty() {
        return "_rels/.rels".to_owned();
    }
    match part.rfind('/') {
        Some(k) => format!("{}_rels/{}.rels", part.get(..=k).unwrap_or(""), part.get(k + 1..).unwrap_or("")),
        None => format!("_rels/{part}.rels"),
    }
}

/// C# `SourceOfRels`: a `.rels` name without `_rels/` throws there (`relsPath[..-1]`).
fn source_of_rels(rels: &str) -> Result<String> {
    let folded = rels.to_ascii_lowercase();
    let idx =
        folded.rfind("_rels/").ok_or_else(|| ApiError::new(super::error::ErrorKind::ArgumentOutOfRange, "a .rels part outside _rels/"))?;
    let dir = rels.get(..idx).unwrap_or("");
    let file = rels.get(idx + 6..).unwrap_or("");
    let stem = file.get(..file.len().saturating_sub(5)).unwrap_or("");
    Ok(format!("{dir}{stem}"))
}

/// `Path.GetExtension` of a file name: from the last `.` (after the last separator), or "".
pub fn extension(file: &str) -> &str {
    let name_start = file.rfind(['/', '\\']).map_or(0, |k| k + 1);
    match file.rfind('.') {
        Some(k) if k >= name_start && k + 1 < file.len() => file.get(k..).unwrap_or(""),
        _ => "",
    }
}

/// `^(.*?)(\d+)$` on a stem (.NET `\d` = Unicode decimal digits, `$` also before a final LF):
/// the stem without its trailing digits, or the stem.
fn strip_trailing_digits(stem: &str) -> &str {
    let body = stem.strip_suffix('\n').unwrap_or(stem);
    let trimmed = body.trim_end_matches(is_unicode_decimal);
    if trimmed.len() == body.len() { stem } else { trimmed }
}

fn is_unicode_decimal(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(net::is_decimal_digit_unit)
}

/// `NextRelId`: `rId` + one above the largest `rId<digits>` (NumberStyles.None), skipping taken ids.
pub fn next_rel_id<'a>(existing: impl Iterator<Item = &'a str>) -> String {
    let mut max: i64 = 0;
    let mut set = std::collections::HashSet::new();
    for id in existing {
        set.insert(id.to_owned());
        if let Some(rest) = id.strip_prefix("rId")
            && let Some(v) = net::parse_int_digits(rest)
        {
            max = max.max(i64::from(v));
        }
    }
    loop {
        max += 1;
        let c = format!("rId{max}");
        if !set.contains(&c) {
            return c;
        }
    }
}

/// `RelativeTarget(from, to)`: the target of `to` relative to the folder of `from`, segments
/// escaped (`OpcPartName.EscapeSegment`).
pub fn relative_target(from: &str, to: &str) -> String {
    let f: Vec<&str> = from.split('/').collect();
    let from_dirs = f.get(..f.len().saturating_sub(1)).unwrap_or(&[]);
    let t: Vec<&str> = to.split('/').collect();
    let mut common = 0;
    while common < from_dirs.len()
        && common + 1 < t.len()
        && from_dirs.get(common).zip(t.get(common)).is_some_and(|(a, b)| net::eq_ignore_case(a, b))
    {
        common += 1;
    }
    let mut s = String::new();
    for _ in common..from_dirs.len() {
        s.push_str("../");
    }
    let rest: Vec<String> = t.get(common..).unwrap_or(&[]).iter().map(|seg| part_name::escape_segment(seg)).collect();
    s.push_str(&rest.join("/"));
    s
}

/// The vocabulary name of `[Content_Types].xml` and `.rels` roots (kept for completeness).
pub(crate) fn _content_types_vocab() -> (&'static str, &'static str) {
    (content_types::MAIN_DOCUMENT, rel_types::OFFICE_DOCUMENT)
}
