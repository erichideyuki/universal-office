//! The style sheet and numbering version stores (U12, G26): `DocStyles`/`DocNumbering` of the
//! editor keep every XML version forever (`versions`, `v`), and undo steps hold only the index
//! (`sv`, `nv`). Here a version is collected when neither the current state nor any history step
//! refers to it, and the texts count against the history's byte cap.
//!
//! Semantics kept: `xml` = the current version's text; `dirty` = the current text differs from the
//! loaded one (`this.xml !== this.versions[0]`); `restore(v)` = `false` for the current or an
//! unknown version.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::state::VersionId;

/// One store (see the module comment).
#[derive(Debug, Clone, Default)]
pub struct VersionStore {
    /// the text loaded with the document (`versions[0]`)
    base: Option<Arc<str>>,
    texts: BTreeMap<VersionId, Option<Arc<str>>>,
    cur: VersionId,
    next: u32,
}

impl VersionStore {
    /// `new DocStyles(xml)`.
    #[must_use]
    pub fn new(xml: Option<Arc<str>>) -> VersionStore {
        let mut texts = BTreeMap::new();
        texts.insert(VersionId(0), xml.clone());
        VersionStore { base: xml, texts, cur: VersionId(0), next: 1 }
    }

    /// `xml`.
    #[must_use]
    pub fn xml(&self) -> Option<&Arc<str>> {
        self.texts.get(&self.cur).and_then(Option::as_ref)
    }

    /// `version`.
    #[must_use]
    pub fn version(&self) -> VersionId {
        self.cur
    }

    /// `dirty`: the current text is not the loaded one (by content).
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.xml().map(|s| &**s) != self.base.as_deref()
    }

    /// `set(xml)`: a new version, current.
    pub fn set(&mut self, xml: Option<Arc<str>>) -> VersionId {
        let id = VersionId(self.next);
        self.next = self.next.saturating_add(1);
        self.texts.insert(id, xml);
        self.cur = id;
        id
    }

    /// `restore(v)`: whether the current version changed.
    pub fn restore(&mut self, v: VersionId) -> bool {
        if v == self.cur || !self.texts.contains_key(&v) {
            return false;
        }
        self.cur = v;
        true
    }

    /// Drops every version but the current one, the loaded one and those in `live` (the history's
    /// steps).
    pub fn collect(&mut self, live: impl IntoIterator<Item = VersionId>) {
        let mut keep: Vec<VersionId> = live.into_iter().collect();
        keep.push(self.cur);
        keep.push(VersionId(0));
        self.texts.retain(|k, _| keep.contains(k));
    }

    /// The bytes of the kept texts (counted with the history, G26).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.texts.values().map(|t| t.as_ref().map_or(0, |s| s.len())).sum()
    }

    /// How many versions are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.texts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }
}
