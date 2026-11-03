//! Media payloads of a document (images, OLE data, embedded layers): stored once per distinct
//! content (full byte equality, G24), referenced by [`MediaId`]. The TS model holds a
//! `Uint8Array` per field; the digest writes the bytes' SHA-256, so sharing is not observable.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::ids::MediaId;
use crate::js::{JsValue, JsonWriter, Str};
use crate::js_object;

/// The media of one document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaStore {
    items: Vec<Arc<[u8]>>,
    by_hash: HashMap<u64, Vec<MediaId>>,
}

fn content_hash(b: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

impl MediaStore {
    #[must_use]
    pub fn new() -> MediaStore {
        MediaStore::default()
    }

    /// Stores `bytes`, or returns the id of an equal payload already stored.
    pub fn add(&mut self, bytes: &[u8]) -> MediaId {
        let h = content_hash(bytes);
        if let Some(ids) = self.by_hash.get(&h)
            && let Some(&id) = ids.iter().find(|id| self.items.get(id.index()).is_some_and(|b| **b == *bytes))
        {
            return id;
        }
        let id = MediaId(u32::try_from(self.items.len()).unwrap_or(u32::MAX));
        self.items.push(Arc::from(bytes));
        self.by_hash.entry(h).or_default().push(id);
        id
    }

    /// The bytes of a payload.
    #[must_use]
    pub fn get(&self, id: MediaId) -> Option<&[u8]> {
        self.items.get(id.index()).map(|b| &**b)
    }

    /// The shared bytes (for the layout's resources, uo-render RQ3).
    #[must_use]
    pub fn get_arc(&self, id: MediaId) -> Option<&Arc<[u8]>> {
        self.items.get(id.index())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Bytes held (M3).
    #[must_use]
    pub fn payload_bytes(&self) -> usize {
        self.items.iter().map(|b| b.len()).sum()
    }
}

/// A `Uint8Array` field of the model: a reference to the document's media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MediaRef(pub MediaId);

impl JsValue for MediaRef {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let bytes = w.media().and_then(|m| m.get(self.0)).unwrap_or(&[]);
        w.bytes(bytes);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        let bytes = w.media().and_then(|m| m.get(self.0)).unwrap_or(&[]);
        w.bytes(bytes);
        true
    }
}

js_object! {
    /// `{ data: Uint8Array; ext: string }` (blip fills, pattern images, page fills, picture
    /// bullets, image layers).
    pub struct ImageData {
        req data: MediaRef = "data",
        req ext: Str = "ext",
    }
}
