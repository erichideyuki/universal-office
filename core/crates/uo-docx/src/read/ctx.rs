//! The context of one read (architecture A5, A7): limits, id allocation, the media store, the
//! per-read replacements of the TS process-global counters (G7), the nesting guard (G20), the
//! source store, and the package access the readers use (XML parts with the docx presets,
//! relationships resolved with uo-opc D6 only).
//!
//! The TS `ReadContext` (`resolveImage`, `resolveLink`, `activeComments`, `onBookmark`,
//! `fieldStack`, `readBlocks`…) is the reader's own state: frontier R4c ports it as a struct
//! next to `readParagraph` that borrows this one; callbacks become methods or enum results, never
//! stored closures.

use uo_opc::Package;
use uo_opc::rels::Relationship;
use uo_opc::xml::Document as XmlDocument;

use crate::error::{DocxError, DocxLimitKind};
use crate::ids::{IdAlloc, MediaId};
use crate::limits::DocxLimits;
use crate::model::MediaStore;
use crate::model::para::SdtKey;
use crate::model::sources::{SourcePart, SourceStore, WrapId};

use super::{DOCX, DOCX_POS, ReadOptions};

/// The per-read values of the TS process-global counters (G7): the TS never resets them, so
/// their values depend on every document read before in the process; here they start at 0 per
/// read and the digest relabels both sides by first appearance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadCounters {
    /// `wrapIds` (fulldoc.ts `unwrapRaw`)
    pub wrap_ids: u32,
    /// `blockSdtKeys` (fulldoc.ts `markBlockSdt`)
    pub block_sdt_keys: u32,
}

/// Counts of one part, against [`DocxLimits`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartCounts {
    pub paragraphs: u32,
    pub runs: u32,
}

/// The context of one read (see the module comment).
pub struct ReadCtx<'p> {
    pub pkg: &'p Package,
    pub limits: DocxLimits,
    pub ids: IdAlloc,
    pub media: MediaStore,
    pub counters: ReadCounters,
    /// the source store (`{ sources: true }`)
    pub sources: Option<SourceStore>,
    /// stories open below the body (cells, textboxes, groups): see [`ReadCtx::enter_nested`]
    nesting: u32,
    /// the part being read and its counts
    part: Option<Box<str>>,
    counts: PartCounts,
}

impl<'p> ReadCtx<'p> {
    pub fn new(pkg: &'p Package, opts: &ReadOptions) -> ReadCtx<'p> {
        ReadCtx {
            pkg,
            limits: opts.limits.clone(),
            ids: IdAlloc::new(),
            media: MediaStore::new(),
            counters: ReadCounters::default(),
            sources: opts.sources.then(SourceStore::default),
            nesting: 0,
            part: None,
            counts: PartCounts::default(),
        }
    }

    /// Whether the read keeps sources.
    #[must_use]
    pub fn with_sources(&self) -> bool {
        self.sources.is_some()
    }

    /// A part parsed as XML with the docx-core preset (`parseXml(xml, { positions: !!store })`):
    /// `Ok(None)` when the part does not exist (`getPartText` gives `undefined`).
    pub fn xml_part(&self, name: &str) -> Result<Option<XmlDocument>, DocxError> {
        if !self.pkg.contains(name) {
            return Ok(None);
        }
        let opts = if self.with_sources() { &DOCX_POS } else { &DOCX };
        Ok(Some(self.pkg.xml_part(name, opts)?))
    }

    /// A part parsed as XML without positions (`parseXml(xml)`), for the parts the TS never
    /// reads with positions (styles, numbering, theme, settings, rels…).
    pub fn xml_part_plain(&self, name: &str) -> Result<Option<XmlDocument>, DocxError> {
        if !self.pkg.contains(name) {
            return Ok(None);
        }
        Ok(Some(self.pkg.xml_part(name, &DOCX)?))
    }

    /// The relationships of `rels_part` (`word/_rels/document.xml.rels`…), empty without one.
    pub fn rels(&self, rels_part: &str) -> Result<Vec<Relationship>, DocxError> {
        match self.xml_part_plain(rels_part)? {
            Some(doc) => Ok(uo_opc::rels::from_document(&doc)),
            None => Ok(Vec::new()),
        }
    }

    /// The part a relationship target names, resolved with uo-opc D6 (`Package::resolve_target`,
    /// the one rule of the library and its hosts; the TS `word/${target}` rules are replaced,
    /// accepted by name in uo-opc R1).
    #[must_use]
    pub fn resolve_target(&self, source_part: &str, target: &str) -> Option<&'p str> {
        self.pkg.resolve_target(source_part, target)
    }

    /// The bytes of a media part, stored once in the document's media (G24). `Ok(None)` when the
    /// part does not exist.
    pub fn media_part(&mut self, name: &str) -> Result<Option<MediaId>, DocxError> {
        match self.pkg.part(name)? {
            Some(bytes) => Ok(Some(self.media.add(bytes))),
            None => Ok(None),
        }
    }

    /// `++wrapIds`.
    pub fn next_wrap_id(&mut self) -> WrapId {
        self.counters.wrap_ids = self.counters.wrap_ids.saturating_add(1);
        WrapId(self.counters.wrap_ids)
    }

    /// `` `s${++blockSdtKeys}` ``.
    pub fn next_block_sdt_key(&mut self) -> SdtKey {
        self.counters.block_sdt_keys = self.counters.block_sdt_keys.saturating_add(1);
        SdtKey(format!("s{}", self.counters.block_sdt_keys).into())
    }

    /// Enters a nested story (cell, textbox, group content). `false` when the content is past
    /// `max_nesting`: the caller reads it with [`super::plain::plain_paragraphs`] instead (G20)
    /// and does not call [`ReadCtx::leave_nested`].
    #[must_use]
    pub fn enter_nested(&mut self) -> bool {
        if self.nesting >= self.limits.max_nesting {
            return false;
        }
        self.nesting += 1;
        true
    }

    /// Leaves a story entered with [`ReadCtx::enter_nested`].
    pub fn leave_nested(&mut self) {
        self.nesting = self.nesting.saturating_sub(1);
    }

    /// The current nesting depth.
    #[must_use]
    pub fn nesting(&self) -> u32 {
        self.nesting
    }

    /// Starts counting a new part.
    pub fn begin_part(&mut self, name: &str) {
        self.part = Some(name.into());
        self.counts = PartCounts::default();
    }

    /// Counts a paragraph of the current part against `max_paragraphs_per_part`.
    pub fn count_paragraph(&mut self) -> Result<(), DocxError> {
        self.counts.paragraphs = self.counts.paragraphs.saturating_add(1);
        if self.counts.paragraphs > self.limits.max_paragraphs_per_part {
            return Err(self.limit(DocxLimitKind::ParagraphsPerPart, self.limits.max_paragraphs_per_part));
        }
        Ok(())
    }

    /// Counts a run of the current part against `max_runs_per_part`.
    pub fn count_run(&mut self) -> Result<(), DocxError> {
        self.counts.runs = self.counts.runs.saturating_add(1);
        if self.counts.runs > self.limits.max_runs_per_part {
            return Err(self.limit(DocxLimitKind::RunsPerPart, self.limits.max_runs_per_part));
        }
        Ok(())
    }

    /// A Word-level limit error naming the current part.
    #[must_use]
    pub fn limit(&self, kind: DocxLimitKind, max: u32) -> DocxError {
        DocxError::Limit { kind, max: u64::from(max), part: self.part.clone() }
    }

    /// Registers the text of a part read with sources (spans point into it); returns its index.
    pub fn source_part(&mut self, path: &str, doc: &XmlDocument) -> Option<u32> {
        let store = self.sources.as_mut()?;
        let text: std::sync::Arc<str> = std::sync::Arc::from(&**doc.source());
        store.parts.push(SourcePart { path: path.into(), text });
        u32::try_from(store.parts.len() - 1).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_and_nesting() {
        let pkg = Package::new_empty(&uo_opc::Limits::DEFAULT, &uo_opc::package::OpenOptions::default());
        let opts = ReadOptions { limits: DocxLimits { max_nesting: 2, ..DocxLimits::DEFAULT }, ..ReadOptions::default() };
        let mut ctx = ReadCtx::new(&pkg, &opts);
        assert_eq!(ctx.next_block_sdt_key().0.as_ref(), "s1");
        assert_eq!(ctx.next_block_sdt_key().0.as_ref(), "s2");
        assert_eq!(ctx.next_wrap_id(), WrapId(1));
        assert!(ctx.enter_nested());
        assert!(ctx.enter_nested());
        assert!(!ctx.enter_nested());
        ctx.leave_nested();
        assert!(ctx.enter_nested());
        assert!(ctx.xml_part("word/document.xml").unwrap().is_none());
    }
}
