//! The answer caches of the policies: identity per key that does not pin memory.
//!
//! A provider that remembers every answer keeps every face it ever resolved alive, which defeats
//! the face budget. The slots here hold the answers **weakly**: while a layout (or anything else)
//! holds an answer, asking again for the same key gives that same answer, so the identity is the
//! one the TS provider's cache would give; once every holder dropped it, the slot is dead and the
//! next ask resolves again with a new identity, which no one can observe. "No such face" is
//! remembered (the TS and C# caches keep their `null`s) until the cap is reached.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use super::answer::FontAnswer;

/// Slots kept at most (M5); past it the dead slots go, and if that is not enough the cache starts
/// again. Within one layout call a document would have to ask for this many distinct families
/// for the identity of a repeated request to be lost.
pub const MAX_ANSWER_SLOTS: usize = 8192;

enum Slot {
    Missing,
    Live(Weak<FontAnswer>),
}

/// What resolving a key produced.
pub(crate) enum Resolved {
    Answer(Arc<FontAnswer>),
    /// no such face: remembered
    Missing,
    /// the face exists but the budget refused it: answered `None` now, asked again next time
    Refused,
}

pub(crate) struct AnswerCache<K> {
    slots: Mutex<HashMap<K, Slot>>,
}

impl<K: Hash + Eq + Clone> AnswerCache<K> {
    pub(crate) fn new() -> AnswerCache<K> {
        AnswerCache { slots: Mutex::new(HashMap::new()) }
    }

    /// The live answer of the key, or the result of `resolve` (run outside the lock: it reads
    /// files). When two threads resolve the same key at once, the first answer inserted wins and
    /// both get it.
    pub(crate) fn get_or_resolve(&self, key: &K, resolve: impl FnOnce() -> Resolved) -> Option<Arc<FontAnswer>> {
        {
            let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            match slots.get(key) {
                Some(Slot::Missing) => return None,
                Some(Slot::Live(w)) => {
                    if let Some(answer) = w.upgrade() {
                        return Some(answer);
                    }
                }
                None => {}
            }
        }
        let resolved = resolve();
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(Slot::Live(w)) = slots.get(key)
            && let Some(first) = w.upgrade()
        {
            return Some(first);
        }
        if slots.len() >= MAX_ANSWER_SLOTS {
            slots.retain(|_, s| matches!(s, Slot::Live(w) if w.strong_count() > 0));
            if slots.len() >= MAX_ANSWER_SLOTS {
                slots.clear();
            }
        }
        match resolved {
            Resolved::Answer(answer) => {
                slots.insert(key.clone(), Slot::Live(Arc::downgrade(&answer)));
                Some(answer)
            }
            Resolved::Missing => {
                slots.insert(key.clone(), Slot::Missing);
                None
            }
            Resolved::Refused => None,
        }
    }

    /// Slots held (answers still alive and remembered misses).
    pub(crate) fn len(&self) -> usize {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}
