//! The workbook package as the shared DrawingML reader sees it (`SheetDrawings.dml = pkg`, drawings.ts): parts by name with
//! the Sheet's case-insensitive fallback (`XlsxPackage.getPart`), the parsed-XML cache of the reader, the media table of the
//! pictures (`MediaId` → part and bytes, created on first use and kept for the viewer's life: the list's media ids refer to it).

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_dml_render::{MediaId, PartSource, R, ReadError, SlidesRules};
use uo_opc::Package;
use uo_opc::xml::Document;

/// Parsed parts kept (the drawing parts, their rels, the theme): least recently used dropped first (M5).
pub const MAX_XML_CACHE: usize = 64;

/// The caches behind a [`BookSource`] (the viewer owns them).
#[derive(Default)]
pub struct SrcCache {
    xml: Vec<(Box<str>, Option<Arc<Document>>)>,
    media: Vec<(Box<str>, Arc<[u8]>)>,
    media_index: FxHashMap<Box<str>, u32>,
}

impl SrcCache {
    /// Empties the parse cache (the media table stays: the resources refer to its ids).
    pub fn clear_xml(&mut self) {
        self.xml = Vec::new();
    }

    /// Heap bytes held by the media table (the picture bytes are shared with no one else).
    #[must_use]
    pub fn media_bytes(&self) -> usize {
        self.media.iter().map(|(_, b)| b.len()).sum()
    }

    /// Entries of the media table.
    #[must_use]
    pub fn media_len(&self) -> usize {
        self.media.len()
    }

    /// The part and bytes of a media id.
    #[must_use]
    pub fn media(&self, id: MediaId) -> Option<(&str, &Arc<[u8]>)> {
        self.media.get(id.0 as usize).map(|(p, b)| (&**p, b))
    }
}

/// The workbook package as a [`PartSource`].
pub struct BookSource<'a> {
    pub pkg: &'a Package,
    pub cache: &'a RefCell<SrcCache>,
    pub rules: SlidesRules,
}

impl BookSource<'_> {
    fn bytes(&self, name: &str) -> Option<&[u8]> {
        self.pkg.find(name).ok().flatten()
    }
}

impl PartSource for BookSource<'_> {
    fn part(&self, name: &str) -> Option<Arc<[u8]>> {
        self.bytes(name).map(Arc::from)
    }

    fn xml(&self, name: &str) -> R<Option<Arc<Document>>> {
        {
            let mut c = self.cache.borrow_mut();
            if let Some(i) = c.xml.iter().position(|(k, _)| &**k == name) {
                let e = c.xml.remove(i);
                let d = e.1.clone();
                c.xml.push(e);
                return Ok(d);
            }
        }
        let doc = match self.bytes(name) {
            None => None,
            Some(b) => {
                let text = uo_jsnum::text::decode_utf8(b).into_owned();
                Some(Arc::new(uo_dml_render::source::parse_xml(text).map_err(|e| match e {
                    ReadError::Xml(m) => ReadError::Xml(format!("{name}: {m}")),
                    other => other,
                })?))
            }
        };
        let mut c = self.cache.borrow_mut();
        if c.xml.len() >= MAX_XML_CACHE {
            c.xml.remove(0);
        }
        c.xml.push((name.into(), doc.clone()));
        Ok(doc)
    }

    fn media(&self, part: &str) -> Option<MediaId> {
        if let Some(&i) = self.cache.borrow().media_index.get(part) {
            return Some(MediaId(i));
        }
        let b = self.bytes(part)?;
        let mut c = self.cache.borrow_mut();
        let i = u32::try_from(c.media.len()).ok()?;
        c.media.push((part.into(), Arc::from(b)));
        c.media_index.insert(part.into(), i);
        Some(MediaId(i))
    }

    fn media_entry(&self, id: MediaId) -> Option<(Box<str>, Arc<[u8]>)> {
        self.cache.borrow().media.get(id.0 as usize).map(|(p, b)| (p.clone(), Arc::clone(b)))
    }

    fn rules(&self) -> SlidesRules {
        self.rules
    }
}
