//! C# `DocxImporter`: imports the body of one .docx into another with the TARGET's styles
//! (DocIO's `ImportContent(UseDestinationStyles)`): a style of the same name uses the target's,
//! missing ones come in (with a free id); numbering, pictures and other related parts, fonts
//! of the font table (embedded ones too), footnotes/endnotes, bookmarks and drawing,
//! paragraph and control ids are copied without colliding with the target's.
//!
//! The source and target are two documents (two arenas): source nodes are copied into the
//! target arena ([`Dom::import`]) and processed there. The C# recursion (styles ↔ numbering ↔
//! notes ↔ parts) is bounded here (`MAX_DEPTH`): the C# overflows its stack (a process crash)
//! a few thousand levels down; this port stops with an error at 256 (accepted by name).

use std::collections::{HashMap, HashSet};

use super::document::{DocxDocument, relative_target};
use super::dom::{Dom, NodeId, n};
use super::error::{ApiError, Result};
use super::net;
use super::ooxml::{self, content_types, order};
use super::paragraph_text::remove_if_empty;
use super::{numbering, pictures, review, sections, styles};

const FONT_ATTRIBUTES: [super::dom::Name; 4] = [n::w_ascii, n::w_hAnsi, n::w_eastAsia, n::w_cs];
const MAX_DEPTH: u32 = 256;

pub(crate) type SectionMapper<'m> = &'m mut dyn FnMut(&mut Dom, NodeId) -> NodeId;

pub(crate) struct Importer<'a> {
    target: &'a mut DocxDocument,
    source: &'a mut DocxDocument,
    source_styles: Option<NodeId>,
    source_numbering: Option<NodeId>,
    style_map: HashMap<String, Option<String>>,
    num_map: HashMap<String, String>,
    abstract_map: HashMap<String, String>,
    pic_bullet_map: HashMap<String, String>,
    rel_map: HashMap<(String, String, String), String>,
    part_map: HashMap<String, String>,
    note_map: HashMap<(String, String), String>,
    fonts: HashSet<String>,
    bookmark_ids: HashMap<String, String>,
    bookmark_names: HashMap<String, String>,
    target_bookmark_names: HashSet<String>,
    target_para_ids: HashSet<String>,
    target_sdt_ids: HashSet<String>,
    next_bookmark_id: i32,
    next_doc_pr_id: i32,
    depth: u32,
}

impl<'a> Importer<'a> {
    pub(crate) fn new(target: &'a mut DocxDocument, source: &'a mut DocxDocument) -> Result<Importer<'a>> {
        let source_styles = styles::root(source, false)?;
        let source_numbering = numbering::root(source, false)?;
        let roots: Vec<NodeId> = target.stories(true, true)?.into_iter().map(|(_, r)| r).collect();
        let dom = target.dom();
        let mut names = HashSet::new();
        let mut max_bm = 0i32;
        let mut paras = HashSet::new();
        let mut sdts = HashSet::new();
        for &r in &roots {
            for b in dom.descendants_named(r, n::bookmarkStart) {
                names.insert(net::fold_key(dom.attr(b, n::name).unwrap_or("")));
                max_bm = max_bm.max(dom.attr(b, n::id).and_then(net::parse_int).unwrap_or(0));
            }
            for e in dom.descendants(r) {
                for a in dom.attrs(e) {
                    if a.name == n::w14_paraId {
                        paras.insert(net::fold_key(&a.value));
                    }
                }
            }
            for p in dom.descendants_named(r, n::sdtPr) {
                sdts.insert(dom.element(p, n::id).and_then(|i| dom.attr(i, n::val)).unwrap_or("").to_owned());
            }
        }
        let next_doc_pr_id = pictures::next_doc_pr_id(target)?;
        Ok(Importer {
            target,
            source,
            source_styles,
            source_numbering,
            style_map: HashMap::new(),
            num_map: HashMap::new(),
            abstract_map: HashMap::new(),
            pic_bullet_map: HashMap::new(),
            rel_map: HashMap::new(),
            part_map: HashMap::new(),
            note_map: HashMap::new(),
            fonts: HashSet::new(),
            bookmark_ids: HashMap::new(),
            bookmark_names: HashMap::new(),
            target_bookmark_names: names,
            target_para_ids: paras,
            target_sdt_ids: sdts,
            next_bookmark_id: max_bm.wrapping_add(1),
            next_doc_pr_id,
            depth: 0,
        })
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(ApiError::invalid_operation("style, numbering or note references nested too deep"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Copies the source body blocks to the end of the target's last section. Without a mapper
    /// the section breaks leave; with one, each source break (in document order, already in
    /// the target arena) is replaced by what it returns.
    pub(crate) fn import_body(mut self, mut section_break: Option<SectionMapper<'_>>) -> Result<Vec<NodeId>> {
        let sbody = self.source.body()?;
        let src_blocks: Vec<NodeId> = self.source.dom().elements(sbody).filter(|&e| !self.source.dom().is(e, n::sectPr)).collect();
        let mut blocks = Vec::with_capacity(src_blocks.len());
        for b in src_blocks {
            if let Some(c) = self.target.dom_mut().import(self.source.dom(), b) {
                blocks.push(c);
            }
        }
        let source_main = self.source.main_part_path().to_owned();
        let target_main = self.target.main_part_path().to_owned();
        for &block in &blocks {
            let source_break = if section_break.is_some() { sections::break_of(self.target.dom(), block) } else { None };
            let dom = self.target.dom_mut();
            for s in dom.descendants_and_self(block).into_iter().filter(|&e| dom.is(e, n::sectPr)).collect::<Vec<_>>() {
                dom.remove(s)?;
            }
            self.process(block, &source_main, &target_main)?;
            if let (Some(sb), Some(mapper)) = (source_break, section_break.as_mut()) {
                let dom = self.target.dom_mut();
                let mapped = mapper(dom, sb);
                let ppr = ooxml::ensure_ppr(dom, block);
                ooxml::set(dom, ppr, mapped, order::PPR);
            }
        }
        match self.target.last_section_properties()? {
            Some(anchor) => {
                self.target.dom_mut().add_before(anchor, &blocks)?;
            }
            None => {
                let body = self.target.body()?;
                self.target.dom_mut().append_all(body, &blocks);
            }
        }
        self.import_fonts()?;
        self.target.mark_changed();
        Ok(blocks)
    }

    /// Adapts a copied subtree from `source_part` to live in `target_part`.
    fn process(&mut self, root: NodeId, source_part: &str, target_part: &str) -> Result<()> {
        let all = self.target.dom().descendants_and_self(root);
        for &e in &all {
            let dom = self.target.dom_mut();
            if dom.is_any(e, &[n::commentRangeStart, n::commentRangeEnd, n::commentReference]) {
                let parent = dom.parent(e);
                dom.remove(e)?;
                if let Some(p) = parent {
                    remove_if_empty(dom, p);
                }
            }
        }
        for e in self.target.dom().descendants_and_self(root) {
            let dom = self.target.dom();
            if dom.is_any(e, &[n::pStyle, n::rStyle, n::tblStyle]) {
                let Some(id) = dom.attr(e, n::val).map(str::to_owned) else { continue };
                match self.import_style(&id)? {
                    None => self.target.dom_mut().remove(e)?,
                    Some(m) => self.target.dom_mut().set_attr(e, n::val, Some(&m)),
                }
            } else if dom.is(e, n::numId) && dom.parent(e).is_some_and(|p| dom.is(p, n::numPr)) {
                if let Some(id) = dom.attr(e, n::val).map(str::to_owned)
                    && id != "0"
                {
                    let m = self.import_num(&id)?;
                    self.target.dom_mut().set_attr(e, n::val, Some(&m));
                }
            } else if dom.is(e, n::rFonts) {
                self.add_fonts(e);
            } else if dom.is_any(e, &[n::footnoteReference, n::endnoteReference]) {
                let kind = if dom.is(e, n::footnoteReference) { "footnotes" } else { "endnotes" };
                if let Some(id) = dom.attr(e, n::id).map(str::to_owned) {
                    let m = self.import_note(kind, &id)?;
                    self.target.dom_mut().set_attr(e, n::id, Some(&m));
                }
            } else if dom.is_any(e, &[n::bookmarkStart, n::bookmarkEnd]) {
                self.renumber_bookmark(e);
            } else if dom.is(e, n::wp_docPr) {
                let v = self.next_doc_pr_id;
                self.next_doc_pr_id = v.wrapping_add(1);
                self.target.dom_mut().set_attr(e, n::a_id, Some(&v.to_string()));
            } else if dom.is(e, n::sdtPr)
                && let Some(sdt_id) = dom.element(e, n::id)
            {
                let v = dom.attr(sdt_id, n::val).unwrap_or("").to_owned();
                if !self.target_sdt_ids.insert(v) {
                    let id = loop {
                        let c = self.target.ids.next_in(1, i32::MAX).to_string();
                        if self.target_sdt_ids.insert(c.clone()) {
                            break c;
                        }
                    };
                    self.target.dom_mut().set_attr(sdt_id, n::val, Some(&id));
                }
            }
            let dom = self.target.dom();
            if let Some(para) = dom.attr(e, n::w14_paraId).map(str::to_owned)
                && !self.target_para_ids.insert(net::fold_key(&para))
            {
                let id = loop {
                    let c = format!("{:08X}", self.target.ids.next_in(1, 0x7FFF_FFFF));
                    if self.target_para_ids.insert(net::fold_key(&c)) {
                        break c;
                    }
                };
                self.target.dom_mut().set_attr(e, n::w14_paraId, Some(&id));
            }
            let dom = self.target.dom();
            let rel_attrs: Vec<(super::dom::Name, String)> = dom
                .attrs(e)
                .iter()
                .filter(|a| dom.name_ns(a.name) == super::dom::NS_R || a.name == n::o_relid)
                .map(|a| (a.name, a.value.clone()))
                .collect();
            for (name, value) in rel_attrs {
                let mapped = self.import_relationship(source_part, target_part, &value)?;
                self.target.dom_mut().set_attr(e, name, mapped.as_deref());
            }
        }
        self.rename_bookmark_references(root)
    }

    fn add_fonts(&mut self, rfonts: NodeId) {
        let dom = self.target.dom();
        for a in FONT_ATTRIBUTES {
            if let Some(f) = dom.attr(rfonts, a)
                && !f.is_empty()
            {
                self.fonts.insert(net::fold_key(f));
            }
        }
    }

    // ── styles ──

    fn import_style(&mut self, source_id: &str) -> Result<Option<String>> {
        if let Some(m) = self.style_map.get(source_id) {
            return Ok(m.clone());
        }
        self.enter()?;
        let r = self.import_style_inner(source_id);
        self.leave();
        r
    }

    fn import_style_inner(&mut self, source_id: &str) -> Result<Option<String>> {
        let style = styles::by_id(self.source.dom(), self.source_styles, source_id);
        let target_root = styles::root(self.target, style.is_some())?;
        let Some(style) = style else {
            let dom = self.target.dom();
            let mapped = if styles::by_id(dom, target_root, source_id).is_some() {
                Some(source_id.to_owned())
            } else {
                styles::built_in_name(source_id).and_then(|b| styles::by_name(dom, target_root, &b, None)).and_then(|s| styles::id(dom, s))
            };
            self.style_map.insert(source_id.to_owned(), mapped.clone());
            return Ok(mapped);
        };
        let sdom = self.source.dom();
        let name = styles::name(sdom, style).unwrap_or_else(|| source_id.to_owned());
        let ty = styles::type_of(sdom, style);
        if let Some(existing) = styles::by_name(self.target.dom(), target_root, &name, Some(&ty)) {
            let mapped = styles::id(self.target.dom(), existing);
            self.style_map.insert(source_id.to_owned(), mapped.clone());
            return Ok(mapped);
        }
        let target_root = target_root.ok_or_else(|| ApiError::invalid_operation("no styles part"))?;
        let new_id = styles::unique_id(self.target.dom(), target_root, source_id);
        self.style_map.insert(source_id.to_owned(), Some(new_id.clone()));
        let copy =
            self.target.dom_mut().import(self.source.dom(), style).ok_or_else(|| ApiError::invalid_operation("style copy failed"))?;
        let dom = self.target.dom_mut();
        dom.set_attr(copy, n::styleId, Some(&new_id));
        dom.set_attr(copy, n::default, None);
        let links: Vec<NodeId> = dom.elements(copy).filter(|&e| dom.is_any(e, &[n::basedOn, n::next, n::link])).collect();
        for link in links {
            let v = self.target.dom().attr(link, n::val).unwrap_or("").to_owned();
            match self.import_style(&v)? {
                None => self.target.dom_mut().remove(link)?,
                Some(l) => self.target.dom_mut().set_attr(link, n::val, Some(&l)),
            }
        }
        let dom = self.target.dom();
        let nums: Vec<NodeId> =
            dom.descendants_named(copy, n::numId).into_iter().filter(|&x| dom.parent(x).is_some_and(|p| dom.is(p, n::numPr))).collect();
        for x in nums {
            if let Some(id) = self.target.dom().attr(x, n::val).map(str::to_owned)
                && id != "0"
            {
                let m = self.import_num(&id)?;
                self.target.dom_mut().set_attr(x, n::val, Some(&m));
            }
        }
        for f in self.target.dom().descendants_named(copy, n::rFonts) {
            self.add_fonts(f);
        }
        self.target.dom_mut().append(target_root, copy);
        Ok(Some(new_id))
    }

    // ── numbering ──

    fn import_num(&mut self, source_num_id: &str) -> Result<String> {
        if let Some(m) = self.num_map.get(source_num_id) {
            return Ok(m.clone());
        }
        self.enter()?;
        let r = self.import_num_inner(source_num_id);
        self.leave();
        r
    }

    fn import_num_inner(&mut self, source_num_id: &str) -> Result<String> {
        let Some(num) = numbering::num(self.source.dom(), self.source_numbering, source_num_id) else {
            return Ok(source_num_id.to_owned());
        };
        let target_root = numbering::root(self.target, true)?.ok_or_else(|| ApiError::invalid_operation("no numbering part"))?;
        let new_id = numbering::next_num_id(self.target.dom(), target_root);
        self.num_map.insert(source_num_id.to_owned(), new_id.clone());
        let copy =
            self.target.dom_mut().import(self.source.dom(), num).ok_or_else(|| ApiError::invalid_operation("numbering copy failed"))?;
        self.target.dom_mut().set_attr(copy, n::numId, Some(&new_id));
        let aref = self.target.dom().element(copy, n::abstractNumId);
        if let Some(a) = aref
            && let Some(aid) = self.target.dom().attr(a, n::val).map(str::to_owned)
        {
            let m = self.import_abstract(&aid)?;
            self.target.dom_mut().set_attr(a, n::val, Some(&m));
        }
        for ps in self.target.dom().descendants_named(copy, n::pStyle) {
            let v = self.target.dom().attr(ps, n::val).unwrap_or("").to_owned();
            match self.import_style(&v)? {
                None => self.target.dom_mut().remove(ps)?,
                Some(s) => self.target.dom_mut().set_attr(ps, n::val, Some(&s)),
            }
        }
        numbering::insert_num(self.target.dom_mut(), target_root, copy)?;
        Ok(new_id)
    }

    fn import_abstract(&mut self, source_abstract_id: &str) -> Result<String> {
        if let Some(m) = self.abstract_map.get(source_abstract_id) {
            return Ok(m.clone());
        }
        self.enter()?;
        let r = self.import_abstract_inner(source_abstract_id);
        self.leave();
        r
    }

    fn import_abstract_inner(&mut self, source_abstract_id: &str) -> Result<String> {
        let an = numbering::abstract_num(self.source.dom(), self.source_numbering, source_abstract_id);
        let target_root = numbering::root(self.target, true)?.ok_or_else(|| ApiError::invalid_operation("no numbering part"))?;
        let Some(an) = an else { return Ok(source_abstract_id.to_owned()) };
        let new_id = numbering::next_abstract_id(self.target.dom(), target_root);
        self.abstract_map.insert(source_abstract_id.to_owned(), new_id.clone());
        let copy =
            self.target.dom_mut().import(self.source.dom(), an).ok_or_else(|| ApiError::invalid_operation("numbering copy failed"))?;
        self.target.dom_mut().set_attr(copy, n::abstractNumId, Some(&new_id));
        if let Some(nsid) = self.target.dom().element(copy, n::nsid) {
            let v = format!("{:08X}", self.target.ids.next_in(1, i32::MAX));
            self.target.dom_mut().set_attr(nsid, n::val, Some(&v));
        }
        for e in self.target.dom().descendants(copy) {
            let dom = self.target.dom();
            if dom.is_any(e, &[n::pStyle, n::numStyleLink, n::styleLink]) {
                let v = dom.attr(e, n::val).unwrap_or("").to_owned();
                match self.import_style(&v)? {
                    None => self.target.dom_mut().remove(e)?,
                    Some(s) => self.target.dom_mut().set_attr(e, n::val, Some(&s)),
                }
            } else if dom.is(e, n::lvlPicBulletId) {
                if let Some(id) = dom.attr(e, n::val).map(str::to_owned) {
                    let m = self.import_pic_bullet(&id)?;
                    self.target.dom_mut().set_attr(e, n::val, Some(&m));
                }
            } else if dom.is(e, n::rFonts) {
                self.add_fonts(e);
            }
        }
        numbering::insert_abstract(self.target.dom_mut(), target_root, copy)?;
        Ok(new_id)
    }

    fn import_pic_bullet(&mut self, source_id: &str) -> Result<String> {
        if let Some(m) = self.pic_bullet_map.get(source_id) {
            return Ok(m.clone());
        }
        let sdom = self.source.dom();
        let bullet = self.source_numbering.and_then(|r| {
            sdom.elements_named(r, n::numPicBullet).into_iter().find(|&b| sdom.attr(b, n::numPicBulletId) == Some(source_id))
        });
        let Some(bullet) = bullet else { return Ok(source_id.to_owned()) };
        let target_root = numbering::root(self.target, true)?.ok_or_else(|| ApiError::invalid_operation("no numbering part"))?;
        let dom = self.target.dom();
        let max = dom
            .elements_named(target_root, n::numPicBullet)
            .into_iter()
            .map(|b| dom.attr(b, n::numPicBulletId).and_then(net::parse_int).unwrap_or(-1))
            .max()
            .unwrap_or(-1);
        let new_id = max.wrapping_add(1).to_string();
        self.pic_bullet_map.insert(source_id.to_owned(), new_id.clone());
        let copy =
            self.target.dom_mut().import(self.source.dom(), bullet).ok_or_else(|| ApiError::invalid_operation("numbering copy failed"))?;
        self.target.dom_mut().set_attr(copy, n::numPicBulletId, Some(&new_id));
        let source_part = self.source.main_related_part("numbering")?.unwrap_or_default();
        let target_part = self.target.main_related_part("numbering")?.unwrap_or_default();
        for e in self.target.dom().descendants_and_self(copy) {
            let dom = self.target.dom();
            let rel_attrs: Vec<(super::dom::Name, String)> = dom
                .attrs(e)
                .iter()
                .filter(|a| dom.name_ns(a.name) == super::dom::NS_R || a.name == n::o_relid)
                .map(|a| (a.name, a.value.clone()))
                .collect();
            for (name, value) in rel_attrs {
                let m = self.import_relationship(&source_part, &target_part, &value)?;
                self.target.dom_mut().set_attr(e, name, m.as_deref());
            }
        }
        numbering::insert_pic_bullet(self.target.dom_mut(), target_root, copy)?;
        Ok(new_id)
    }

    // ── relationships and parts ──

    fn import_relationship(&mut self, source_part: &str, target_part: &str, source_id: &str) -> Result<Option<String>> {
        let key = (source_part.to_owned(), target_part.to_owned(), source_id.to_owned());
        if let Some(m) = self.rel_map.get(&key) {
            return Ok(Some(m.clone()));
        }
        let Some(rel) = self.source.get_relationship(source_part, source_id)? else { return Ok(None) };
        let id = if rel.external {
            self.target.add_relationship(target_part, &rel.rel_type, &rel.target, true)?
        } else {
            let Some(st) = self.source.resolve_target(source_part, &rel) else { return Ok(None) };
            let copied = self.copy_part(&st)?;
            self.target.add_relationship(target_part, &rel.rel_type, &relative_target(target_part, &copied), false)?
        };
        self.rel_map.insert(key, id.clone());
        Ok(Some(id))
    }

    /// Copies the part (and, depth first, its parts) to a free path of the target. Iterative
    /// (explicit frames), in the C# recursion's order: a child is copied with all of its own
    /// parts before the parent's relationship to it is added.
    fn copy_part(&mut self, source_path: &str) -> Result<String> {
        enum Frame {
            Copy { source: String, target: String, rels: Vec<super::document::Relationship>, next: usize },
            AddRel { part: String, id: String, ty: String, child_target: String },
        }
        if let Some(c) = self.part_map.get(&net::fold_key(source_path)) {
            return Ok(c.clone());
        }
        let root_target = self.start_copy(source_path)?;
        let rels = self.source.get_relationships(source_path)?;
        let mut stack = vec![Frame::Copy { source: source_path.to_owned(), target: root_target.clone(), rels, next: 0 }];
        while let Some(top) = stack.pop() {
            let (src, tgt, rels, next) = match top {
                Frame::AddRel { part, id, ty, child_target } => {
                    let target = relative_target(&part, &child_target);
                    self.target.add_relationship_with_id(&part, &id, &ty, &target, false)?;
                    continue;
                }
                Frame::Copy { source, target, rels, next } => (source, target, rels, next),
            };
            let Some(rel) = rels.get(next).cloned() else { continue };
            stack.push(Frame::Copy { source: src.clone(), target: tgt.clone(), rels, next: next + 1 });
            if rel.external {
                self.target.add_relationship_with_id(&tgt, &rel.id, &rel.rel_type, &rel.target, true)?;
                continue;
            }
            let Some(child) = self.source.resolve_target(&src, &rel) else { continue };
            if let Some(done) = self.part_map.get(&net::fold_key(&child)).cloned() {
                self.target.add_relationship_with_id(&tgt, &rel.id, &rel.rel_type, &relative_target(&tgt, &done), false)?;
                continue;
            }
            let child_target = self.start_copy(&child)?;
            let child_rels = self.source.get_relationships(&child)?;
            stack.push(Frame::AddRel { part: tgt, id: rel.id, ty: rel.rel_type, child_target: child_target.clone() });
            stack.push(Frame::Copy { source: child, target: child_target, rels: child_rels, next: 0 });
            if stack.len() > 140_000 {
                return Err(ApiError::invalid_operation("relationship chain too long"));
            }
        }
        Ok(root_target)
    }

    fn start_copy(&mut self, source_path: &str) -> Result<String> {
        let target_path = self.target.unique_part_path(source_path);
        self.part_map.insert(net::fold_key(source_path), target_path.clone());
        let bytes = self.source.get_part_bytes(source_path)?.unwrap_or_default();
        let ct = self.source.get_content_type(source_path)?.unwrap_or_else(|| "application/octet-stream".to_owned());
        self.target.set_binary_part(&target_path, bytes, Some(&ct))?;
        Ok(target_path)
    }

    // ── notes ──

    fn import_note(&mut self, kind: &str, source_id: &str) -> Result<String> {
        let key = (kind.to_owned(), source_id.to_owned());
        if let Some(m) = self.note_map.get(&key) {
            return Ok(m.clone());
        }
        self.enter()?;
        let r = self.import_note_inner(kind, source_id);
        self.leave();
        r
    }

    fn import_note_inner(&mut self, kind: &str, source_id: &str) -> Result<String> {
        let (note_name, root_name) = if kind == "footnotes" { (n::footnote, n::footnotes) } else { (n::endnote, n::endnotes) };
        let source_part = self.source.main_related_part(kind)?;
        let note = match &source_part {
            Some(p) => self.source.root_of(p)?.and_then(|r| {
                let sdom = self.source.dom();
                sdom.elements_named(r, note_name).into_iter().find(|&x| sdom.attr(x, n::id) == Some(source_id))
            }),
            None => None,
        };
        let (Some(note), Some(source_part)) = (note, source_part) else { return Ok(source_id.to_owned()) };
        let target_root = self.notes_root(kind, root_name, note_name)?;
        let target_part = self.target.main_related_part(kind)?.unwrap_or_default();
        let dom = self.target.dom();
        let max = dom
            .elements_named(target_root, note_name)
            .into_iter()
            .map(|x| dom.attr(x, n::id).and_then(net::parse_int).unwrap_or(0))
            .max()
            .unwrap_or(0);
        let new_id = max.wrapping_add(1).to_string();
        self.note_map.insert((kind.to_owned(), source_id.to_owned()), new_id.clone());
        let copy = self.target.dom_mut().import(self.source.dom(), note).ok_or_else(|| ApiError::invalid_operation("note copy failed"))?;
        self.target.dom_mut().set_attr(copy, n::id, Some(&new_id));
        self.process(copy, &source_part, &target_part)?;
        self.target.dom_mut().append(target_root, copy);
        Ok(new_id)
    }

    fn notes_root(&mut self, kind: &str, root_name: super::dom::Name, note_name: super::dom::Name) -> Result<NodeId> {
        if let Some(existing) = self.target.main_related_part(kind)? {
            let d = self.target.xml(&existing)?;
            return self.target.dom().root(d).ok_or_else(|| ApiError::invalid_data("notes part without root"));
        }
        let ct = if kind == "footnotes" { content_types::FOOTNOTES } else { content_types::ENDNOTES };
        let root = self
            .target
            .main_related_root(kind, true, &format!("word/{kind}.xml"), ct, root_name)?
            .ok_or_else(|| ApiError::invalid_operation("no notes part"))?;
        let dom = self.target.dom_mut();
        for (ty, id, inner) in [("separator", "-1", n::separator), ("continuationSeparator", "0", n::continuationSeparator)] {
            let x = dom.new_element(inner);
            let r = dom.new_element(n::r);
            dom.append(r, x);
            let p = dom.new_element(n::p);
            dom.append(p, r);
            let note = dom.new_element_with(note_name, &[(n::type_, ty), (n::id, id)]);
            dom.append(note, p);
            dom.append(root, note);
        }
        Ok(root)
    }

    // ── bookmarks ──

    fn renumber_bookmark(&mut self, e: NodeId) {
        let dom = self.target.dom();
        let Some(id) = dom.attr(e, n::id).map(str::to_owned) else { return };
        let new_id = match self.bookmark_ids.get(&id) {
            Some(v) => v.clone(),
            None => {
                let v = self.next_bookmark_id.to_string();
                self.next_bookmark_id = self.next_bookmark_id.wrapping_add(1);
                self.bookmark_ids.insert(id, v.clone());
                v
            }
        };
        self.target.dom_mut().set_attr(e, n::id, Some(&new_id));
        let dom = self.target.dom();
        if !dom.is(e, n::bookmarkStart) {
            return;
        }
        let Some(name) = dom.attr(e, n::name).map(str::to_owned) else { return };
        if self.target_bookmark_names.insert(net::fold_key(&name)) {
            return;
        }
        let mut k: i32 = 1;
        let renamed = loop {
            let c = format!("{name}_{k}");
            k = k.wrapping_add(1);
            if !self.target_bookmark_names.contains(&net::fold_key(&c)) {
                break c;
            }
        };
        self.target_bookmark_names.insert(net::fold_key(&renamed));
        self.bookmark_names.insert(name, renamed.clone());
        self.target.dom_mut().set_attr(e, n::name, Some(&renamed));
    }

    /// Hyperlink anchors and field references (REF, PAGEREF, HYPERLINK \l) to the new names.
    fn rename_bookmark_references(&mut self, root: NodeId) -> Result<()> {
        if self.bookmark_names.is_empty() {
            return Ok(());
        }
        let dom = self.target.dom_mut();
        for h in dom.descendants_and_self(root).into_iter().filter(|&e| dom.is(e, n::hyperlink)).collect::<Vec<_>>() {
            if let Some(a) = dom.attr(h, n::anchor)
                && let Some(r) = self.bookmark_names.get(a).cloned()
            {
                dom.set_attr(h, n::anchor, Some(&r));
            }
        }
        for instr in dom.descendants_and_self(root).into_iter().filter(|&e| dom.is(e, n::instrText)).collect::<Vec<_>>() {
            let value = dom.value(instr);
            let mut parts: Vec<String> = value.split(' ').map(str::to_owned).collect();
            let mut changed = false;
            for part in parts.iter_mut() {
                let token = part.trim_matches('"').to_owned();
                if let Some(r) = self.bookmark_names.get(&token) {
                    if token.is_empty() {
                        return Err(ApiError::argument("String cannot be of zero length. (Parameter 'oldValue')"));
                    }
                    *part = part.replace(&token, r);
                    changed = true;
                }
            }
            if changed {
                dom.set_value(instr, &parts.join(" "));
            }
        }
        Ok(())
    }

    // ── fonts ──

    fn import_fonts(&mut self) -> Result<()> {
        let Some(source_part) = self.source.main_related_part("fontTable")? else { return Ok(()) };
        let Some(source_root) = self.source.root_of(&source_part)? else { return Ok(()) };
        if self.fonts.is_empty() {
            return Ok(());
        }
        let sdom = self.source.dom();
        let missing: Vec<NodeId> = sdom
            .elements_named(source_root, n::font)
            .into_iter()
            .filter(|&f| self.fonts.contains(&net::fold_key(sdom.attr(f, n::name).unwrap_or(""))))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let target_root = self
            .target
            .main_related_root("fontTable", true, "word/fontTable.xml", content_types::FONT_TABLE, n::fonts)?
            .ok_or_else(|| ApiError::invalid_operation("no font table"))?;
        let target_part = self.target.main_related_part("fontTable")?.unwrap_or_default();
        let dom = self.target.dom();
        let mut present: HashSet<String> =
            dom.elements_named(target_root, n::font).into_iter().map(|f| net::fold_key(dom.attr(f, n::name).unwrap_or(""))).collect();
        let mut embedded = false;
        for font in missing {
            if !present.insert(net::fold_key(self.source.dom().attr(font, n::name).unwrap_or(""))) {
                continue;
            }
            let copy =
                self.target.dom_mut().import(self.source.dom(), font).ok_or_else(|| ApiError::invalid_operation("font copy failed"))?;
            let dom = self.target.dom();
            let embeds: Vec<NodeId> = dom.elements(copy).filter(|&e| dom.local_of(e).starts_with("embed")).collect();
            for embed in embeds {
                let rid = self.target.dom().attr(embed, n::r_id).map(str::to_owned);
                let mapped = match rid {
                    Some(r) => self.import_relationship(&source_part, &target_part, &r)?,
                    None => None,
                };
                match mapped {
                    None => self.target.dom_mut().remove(embed)?,
                    Some(m) => {
                        self.target.dom_mut().set_attr(embed, n::r_id, Some(&m));
                        embedded = true;
                    }
                }
            }
            self.target.dom_mut().append(target_root, copy);
        }
        if embedded
            && let Some(settings) = review::settings_root(self.target)?
            && self.target.dom().element(settings, n::embedTrueTypeFonts).is_none()
        {
            let dom = self.target.dom_mut();
            let e = dom.new_element(n::embedTrueTypeFonts);
            ooxml::set(dom, settings, e, order::SETTINGS);
        }
        Ok(())
    }
}
