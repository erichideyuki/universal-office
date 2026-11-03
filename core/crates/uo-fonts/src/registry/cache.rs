//! The face cache: parsed [`Face`]s deduplicated by content (length and SHA-256), least recently
//! used out first over `bytes.len() + heap_bytes()` (LQ8). One per process or wasm instance; every
//! method takes `&self` and is safe to call from several threads (the server converts documents in
//! parallel).
//!
//! A face is **pinned** while a [`CachedFace`] handle exists: the answer of a provider, a
//! registered font, a `Layout`. Only the faces nobody holds are counted against
//! [`FontBudget::cache_bytes`] and evicted. Pinning is the `Arc` count of the entry, checked under
//! the cache lock: a handle can only be cloned from a handle, so the count of an entry the cache
//! is the only holder of cannot rise while the lock is held. What a document may pin is the
//! [`FaceLedger`](super::FaceLedger).
//!
//! The budget is applied when the cache is used (every insertion) and by [`FaceCache::trim_now`];
//! the face an insertion just made is pinned by the handle it returns, so the cache can hold one
//! face more than its budget until the next call, and a face whose last handle dropped is
//! counted again from the next call on. `trim_now` after a layout makes it exact.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::budget::FontBudget;
use super::digest::sha256;
use crate::error::FontError;
use crate::face::Face;
use crate::limits::FontLimits;

/// Unpinned faces kept at most, whatever their size (a cap on the entries as M5 asks of every
/// cache; the bytes budget alone would let thousands of tiny faces pile up).
pub const MAX_UNPINNED_FACES: usize = 4096;

/// Aliases (names a source gave to faces) remembered at most; past it they are forgotten and the
/// next ask reads the source again.
pub const MAX_ALIASES: usize = MAX_UNPINNED_FACES * 4;

/// The content of a font file: length and SHA-256. Equal keys are the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentKey {
    len: u64,
    digest: [u8; 32],
}

impl ContentKey {
    pub fn of(bytes: &[u8]) -> ContentKey {
        ContentKey { len: bytes.len() as u64, digest: sha256(bytes) }
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The first 16 bytes of the digest as lower-case hex (what the parity captures print).
    pub fn short_hex(&self) -> String {
        self.digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
    }
}

struct Entry {
    key: ContentKey,
    face: Face,
}

/// A pinned face: the parsed [`Face`] and the guarantee it stays in the cache while this handle
/// (or a clone of it) lives.
#[derive(Clone)]
pub struct CachedFace(Arc<Entry>);

impl CachedFace {
    pub fn face(&self) -> &Face {
        &self.0.face
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        self.0.face.bytes()
    }

    pub fn content_key(&self) -> ContentKey {
        self.0.key
    }

    /// `bytes.len() + heap_bytes()` now: what the budgets count for this face.
    pub fn cost(&self) -> u64 {
        face_cost(&self.0.face)
    }

    /// True when both handles are the same cache entry (the same file content).
    pub fn same_entry(&self, other: &CachedFace) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

fn face_cost(face: &Face) -> u64 {
    (face.bytes().len() as u64).saturating_add(face.heap_bytes() as u64)
}

struct Slot {
    entry: Arc<Entry>,
    used: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<ContentKey, Slot>,
    aliases: HashMap<String, ContentKey>,
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

/// What the cache holds, from [`FaceCache::stats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub entries: usize,
    pub pinned_entries: usize,
    /// cost of the faces some handle holds
    pub pinned_bytes: u64,
    /// cost of the faces nobody holds (what `cache_bytes` bounds)
    pub cached_bytes: u64,
    /// names of a source (path and face) remembered for the faces cached
    pub aliases: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

/// See the module documentation.
pub struct FaceCache {
    state: Mutex<State>,
    limits: FontLimits,
    cache_bytes: u64,
}

impl FaceCache {
    pub fn new(budget: FontBudget, limits: FontLimits) -> FaceCache {
        FaceCache { state: Mutex::new(State::default()), limits, cache_bytes: budget.cache_bytes }
    }

    /// The process-wide cache with the desktop budget of the target ([`FontBudget::DESKTOP`]) and
    /// the default limits: what the shared system index and the default registries use.
    pub fn shared() -> Arc<FaceCache> {
        use std::sync::OnceLock;
        static SHARED: OnceLock<Arc<FaceCache>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| Arc::new(FaceCache::new(FontBudget::DESKTOP, FontLimits::DEFAULT))))
    }

    pub fn limits(&self) -> &FontLimits {
        &self.limits
    }

    pub fn cache_bytes(&self) -> u64 {
        self.cache_bytes
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cached face of these bytes, parsing them when the content is new. Equal content shares
    /// one entry: the buffer passed is dropped in favour of the cached one (one copy of the bytes).
    /// A parse failure is not cached.
    pub fn get_or_parse(&self, bytes: Arc<[u8]>) -> Result<CachedFace, FontError> {
        let key = ContentKey::of(&bytes);
        if let Some(hit) = self.hit(&key) {
            return Ok(hit);
        }
        let face = Face::parse(bytes, &self.limits)?;
        let mut state = self.lock();
        state.clock += 1;
        let clock = state.clock;
        let found = state.entries.get_mut(&key).map(|slot| {
            slot.used = clock;
            Arc::clone(&slot.entry)
        });
        if let Some(entry) = found {
            state.hits += 1;
            return Ok(CachedFace(entry));
        }
        state.misses += 1;
        let entry = Arc::new(Entry { key, face });
        state.entries.insert(key, Slot { entry: Arc::clone(&entry), used: clock });
        self.trim(&mut state);
        Ok(CachedFace(entry))
    }

    /// Like [`FaceCache::get_or_parse`], for a source that names its files (`alias`: a path and a
    /// face index): a known alias whose content is still cached answers without reading the
    /// source again. `load` returns the bytes, or `None` when the source cannot be read.
    pub fn get_or_load(&self, alias: &str, load: impl FnOnce() -> Option<Vec<u8>>) -> Option<CachedFace> {
        {
            let mut state = self.lock();
            if let Some(key) = state.aliases.get(alias).copied() {
                state.clock += 1;
                let clock = state.clock;
                let found = state.entries.get_mut(&key).map(|slot| {
                    slot.used = clock;
                    Arc::clone(&slot.entry)
                });
                if let Some(entry) = found {
                    state.hits += 1;
                    return Some(CachedFace(entry));
                }
            }
        }
        let bytes = load()?;
        let cached = self.get_or_parse(Arc::from(bytes)).ok()?;
        let mut state = self.lock();
        if state.aliases.len() >= MAX_ALIASES {
            state.aliases.clear();
        }
        state.aliases.insert(alias.to_owned(), cached.content_key());
        Some(cached)
    }

    fn hit(&self, key: &ContentKey) -> Option<CachedFace> {
        let mut state = self.lock();
        state.clock += 1;
        let clock = state.clock;
        let slot = state.entries.get_mut(key)?;
        slot.used = clock;
        let entry = Arc::clone(&slot.entry);
        state.hits += 1;
        Some(CachedFace(entry))
    }

    /// Evicts the unpinned faces, least recently used first, until what is left fits the budget
    /// and the entry cap.
    fn trim(&self, state: &mut State) {
        let mut unpinned: Vec<(u64, ContentKey, u64)> = state
            .entries
            .iter()
            .filter(|(_, slot)| Arc::strong_count(&slot.entry) == 1)
            .map(|(key, slot)| (slot.used, *key, face_cost(&slot.entry.face)))
            .collect();
        let mut total: u64 = unpinned.iter().map(|u| u.2).sum();
        if total <= self.cache_bytes && unpinned.len() <= MAX_UNPINNED_FACES {
            return;
        }
        unpinned.sort_unstable_by_key(|u| u.0);
        let mut count = unpinned.len();
        for (_, key, cost) in unpinned {
            if total <= self.cache_bytes && count <= MAX_UNPINNED_FACES {
                break;
            }
            state.entries.remove(&key);
            state.evictions += 1;
            total = total.saturating_sub(cost);
            count -= 1;
        }
    }

    /// Applies the budget again (the lazy parts of a face grow after it was cached, and a face
    /// whose last handle dropped becomes evictable): the registry calls it after a layout.
    pub fn trim_now(&self) {
        let mut state = self.lock();
        self.trim(&mut state);
    }

    /// Drops every face nobody holds, and the aliases. Pinned faces stay (they are the callers').
    pub fn clear(&self) {
        let mut state = self.lock();
        state.entries.retain(|_, slot| Arc::strong_count(&slot.entry) > 1);
        state.entries.shrink_to_fit();
        state.aliases.clear();
        state.aliases.shrink_to_fit();
    }

    pub fn stats(&self) -> CacheStats {
        let state = self.lock();
        let mut out = CacheStats {
            aliases: state.aliases.len(),
            hits: state.hits,
            misses: state.misses,
            evictions: state.evictions,
            ..CacheStats::default()
        };
        for slot in state.entries.values() {
            let cost = face_cost(&slot.entry.face);
            out.entries += 1;
            if Arc::strong_count(&slot.entry) > 1 {
                out.pinned_entries += 1;
                out.pinned_bytes = out.pinned_bytes.saturating_add(cost);
            } else {
                out.cached_bytes = out.cached_bytes.saturating_add(cost);
            }
        }
        out
    }
}
