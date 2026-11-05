//! The per-document face budget (uo-fonts gate-2 D4, LQ8(b)): what one document may pin.
//!
//! The cache bounds the faces nobody holds; a document that registers or asks for dozens of
//! embedded fonts pins them all, so the pinned side has its own bound. A [`FaceLedger`] belongs
//! to one document (a layout run, a conversion, the registry of a view). A face is charged once
//! however many answers use it (one copy of the bytes), by its content, and the charge ends when
//! the last [`Ticket`] for it drops. Past the budget [`FaceLedger::charge`] refuses: the caller
//! resolves the face as a miss of the provider (the layout chain continues to its fallbacks and
//! warns `"font budget"`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use super::cache::ContentKey;

struct Inner {
    ledger: Arc<FaceLedger>,
    key: ContentKey,
    cost: u64,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let mut state = self.ledger.lock();
        state.used = state.used.saturating_sub(self.cost);
        if state.tickets.get(&self.key).is_some_and(|w| w.strong_count() == 0) {
            state.tickets.remove(&self.key);
        }
    }
}

/// The charge of one face to a ledger; clones share it.
#[derive(Clone)]
pub struct Ticket(Arc<Inner>);

impl Ticket {
    pub fn cost(&self) -> u64 {
        self.0.cost
    }
}

#[derive(Default)]
struct State {
    used: u64,
    tickets: HashMap<ContentKey, Weak<Inner>>,
}

/// See the module documentation.
pub struct FaceLedger {
    budget: u64,
    state: Mutex<State>,
    /// charges refused so far (the signal of the layout's `"font budget"` warning; added by
    /// uo-docx-layout F8)
    refused: AtomicU64,
}

impl FaceLedger {
    /// A ledger that allows `budget` bytes pinned.
    pub fn new(budget: u64) -> Arc<FaceLedger> {
        Arc::new(FaceLedger { budget, state: Mutex::new(State::default()), refused: AtomicU64::new(0) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// Charges refused since the ledger was made (a face that would have passed the budget).
    pub fn refused(&self) -> u64 {
        self.refused.load(Ordering::Relaxed)
    }

    /// Bytes charged now.
    pub fn used(&self) -> u64 {
        self.lock().used
    }

    /// The ticket of the face with this content: the existing one when the face is already
    /// charged (no second charge), else a new one of `cost` bytes, or `None` when the charge
    /// would pass the budget. A face costing more than the whole budget is always refused.
    pub fn charge(self: &Arc<Self>, key: ContentKey, cost: u64) -> Option<Ticket> {
        let mut state = self.lock();
        if let Some(live) = state.tickets.get(&key).and_then(Weak::upgrade) {
            return Some(Ticket(live));
        }
        if state.used.saturating_add(cost) > self.budget {
            self.refused.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        state.used += cost;
        let inner = Arc::new(Inner { ledger: Arc::clone(self), key, cost });
        state.tickets.insert(key, Arc::downgrade(&inner));
        Some(Ticket(inner))
    }
}
