//! Write's undo history (G5, U11: a module, not a crate, until Slides and Sheet converge). The
//! semantics are `universal-write.ts`'s (`snapshot`, `discardSnapshot`, `changed`, `popHistory`,
//! `setDirty`):
//!
//! - a step stores the state **before** it; undo pushes the **current** state on the other stack
//!   (so a change made without a snapshot lands in redo);
//! - a new step clears redo; at most 200 steps (the oldest is dropped, `shift()`);
//! - every step and every undo/redo makes the document dirty; undo never makes it clean again (the
//!   saved state can be any point of the stack); `discardSnapshot` of the step that took the
//!   document out of the saved state makes it clean again (`dirtiedAt`);
//! - plus (new, U19, G26): a **byte cap** over both stacks, counted as the bytes of each step that
//!   are not shared with the state after it (see [`StepCost`]); past it the oldest undo steps go,
//!   then the farthest redo steps.
//!
//! Generic over the step type so that the history tests run on small states; the session's step
//! is [`super::state::EditState`].

use std::collections::VecDeque;

/// The TS stack limit (`if (this.undoStack.length > 200) this.undoStack.shift()`).
pub const MAX_STEPS: usize = 200;

/// The byte cap of the history (U19): 64 MiB on wasm32, 256 MiB natively.
#[cfg(target_pointer_width = "32")]
pub const DEFAULT_MAX_BYTES: usize = 64 << 20;
/// The byte cap of the history (U19): 64 MiB on wasm32, 256 MiB natively.
#[cfg(not(target_pointer_width = "32"))]
pub const DEFAULT_MAX_BYTES: usize = 256 << 20;

/// The memory a step keeps alive on its own: the bytes it does not share with `next`, the state
/// that follows it (pointer comparison of the shared chunks and nodes; an estimate, used only for
/// the cap).
pub trait StepCost {
    fn unshared_bytes(&self, next: &Self) -> usize;
}

/// The caps of a history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryLimits {
    pub max_steps: usize,
    pub max_bytes: usize,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        HistoryLimits { max_steps: MAX_STEPS, max_bytes: DEFAULT_MAX_BYTES }
    }
}

/// What a history call changed in the dirty state (for the `uw-dirty-change` event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtyChange {
    /// `Some(new)` when the dirty flag changed
    pub dirty: Option<bool>,
}

/// One step on a stack. The state is boxed: the stacks then hold 24-byte entries, so their
/// growth (up to 256 entries) stays far below the per-keystroke budget (U19).
#[derive(Debug, Clone)]
struct Entry<S> {
    state: Box<S>,
    /// bytes not shared with the next state (0 until priced)
    cost: usize,
    /// priced against a neighbour that no longer changes
    fin: bool,
}

/// The two stacks and the dirty state of one document.
#[derive(Debug, Clone)]
pub struct History<S> {
    undo: VecDeque<Entry<S>>,
    redo: Vec<Entry<S>>,
    limits: HistoryLimits,
    dirty: bool,
    /// the undo depth at which the top step took the document out of its saved state (`-1`: no)
    dirtied_at: Option<usize>,
    /// steps taken since the document was installed (`editsSinceLoad`)
    edits_since_load: u32,
    bytes: usize,
    /// bytes kept with the history outside the steps (the version stores, G26)
    extra: usize,
}

impl<S> Default for History<S> {
    fn default() -> Self {
        History::new(HistoryLimits::default())
    }
}

impl<S> History<S> {
    #[must_use]
    pub fn new(limits: HistoryLimits) -> History<S> {
        History { undo: VecDeque::new(), redo: Vec::new(), limits, dirty: false, dirtied_at: None, edits_since_load: 0, bytes: 0, extra: 0 }
    }

    #[must_use]
    pub fn limits(&self) -> HistoryLimits {
        self.limits
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    #[must_use]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    #[must_use]
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[must_use]
    pub fn edits_since_load(&self) -> u32 {
        self.edits_since_load
    }

    /// The bytes counted against the cap (the steps and the extra bytes).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes.saturating_add(self.extra)
    }

    /// The bytes kept with the history outside its steps (the style and numbering version
    /// stores, G26): counted against the cap from the next pricing on.
    pub fn set_extra_bytes(&mut self, n: usize) {
        self.extra = n;
    }

    /// The step an undo would restore.
    #[must_use]
    pub fn peek_undo(&self) -> Option<&S> {
        self.undo.back().map(|e| &*e.state)
    }

    /// The step a redo would restore.
    #[must_use]
    pub fn peek_redo(&self) -> Option<&S> {
        self.redo.last().map(|e| &*e.state)
    }

    /// Every state the history holds (the version stores collect against them, U12).
    pub fn states(&self) -> impl Iterator<Item = &S> + '_ {
        self.undo.iter().chain(self.redo.iter()).map(|e| &*e.state)
    }

    /// The install (`installFullDoc`): both stacks emptied, no edits, clean.
    pub fn reset(&mut self) -> DirtyChange {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
        self.edits_since_load = 0;
        self.set_dirty(false)
    }

    /// `setDirty(dirty)`: the host marks the document saved (or changed).
    pub fn set_dirty(&mut self, dirty: bool) -> DirtyChange {
        self.dirtied_at = None;
        if self.dirty == dirty {
            return DirtyChange::default();
        }
        self.dirty = dirty;
        DirtyChange { dirty: Some(dirty) }
    }

    /// `snapshot()`: `before` (the state before the step) goes on the undo stack; redo is
    /// cleared; the document is dirty.
    pub fn push(&mut self, before: S) -> DirtyChange {
        self.edits_since_load = self.edits_since_load.saturating_add(1);
        self.undo.push_back(Entry { state: Box::new(before), cost: 0, fin: false });
        if self.undo.len() > self.limits.max_steps
            && let Some(e) = self.undo.pop_front()
        {
            self.bytes = self.bytes.saturating_sub(e.cost);
        }
        for e in self.redo.drain(..) {
            self.bytes = self.bytes.saturating_sub(e.cost);
        }
        let was_clean = !self.dirty;
        let change = self.set_dirty(true);
        self.dirtied_at = was_clean.then_some(self.undo.len());
        change
    }

    /// `discardSnapshot()`: the step just pushed changed nothing; if it was the one that took the
    /// document out of its saved state, the document is clean again. Returns the state.
    pub fn discard(&mut self) -> (Option<S>, DirtyChange) {
        let top = self.undo.len();
        let e = self.undo.pop_back();
        if let Some(e) = &e {
            self.bytes = self.bytes.saturating_sub(e.cost);
        }
        let change = if self.dirtied_at == Some(top) { self.set_dirty(false) } else { DirtyChange::default() };
        (e.map(|e| *e.state), change)
    }

    /// `oneUndo`: the steps above `len` are dropped, the first of them stays (`undoStack.splice(mark
    /// + 1)`; D2b-F7).
    pub fn collapse_to(&mut self, len: usize) {
        while self.undo.len() > len {
            if let Some(e) = self.undo.pop_back() {
                self.bytes = self.bytes.saturating_sub(e.cost);
            }
        }
    }

    /// `editsSinceLoad++` outside a step (`toggleTrackChanges`).
    pub fn note_edit(&mut self) {
        self.edits_since_load = self.edits_since_load.saturating_add(1);
    }

    /// `changed()`: the document changed outside a step (dirty, no step).
    pub fn changed(&mut self) -> DirtyChange {
        self.set_dirty(true)
    }

    /// `popHistory(undo, redo)` (undo): the step to restore, after `current` went on the redo
    /// stack; `None` (nothing changes) with an empty undo stack.
    pub fn undo(&mut self, current: S) -> Option<(S, DirtyChange)> {
        let e = self.undo.pop_back()?;
        let change = self.set_dirty(true);
        self.bytes = self.bytes.saturating_sub(e.cost);
        self.redo.push(Entry { state: Box::new(current), cost: 0, fin: false });
        Some((*e.state, change))
    }

    /// `popHistory(redo, undo)` (redo).
    pub fn redo(&mut self, current: S) -> Option<(S, DirtyChange)> {
        let e = self.redo.pop()?;
        let change = self.set_dirty(true);
        self.bytes = self.bytes.saturating_sub(e.cost);
        self.undo.push_back(Entry { state: Box::new(current), cost: 0, fin: false });
        Some((*e.state, change))
    }
}

impl<S: StepCost> History<S> {
    /// Prices the steps whose next state is now known and applies the byte cap: called after
    /// every step, undo and redo, with the live state. An entry that has a neighbour towards the
    /// live state is priced once against it (final); the top of each stack is re-priced against
    /// `live` on every call.
    pub fn price(&mut self, live: &S) {
        let n = self.undo.len();
        for i in 0..n {
            let fin = i + 1 < n;
            if self.undo.get(i).is_some_and(|e| e.fin) && fin {
                continue;
            }
            let cost = match (self.undo.get(i), self.undo.get(i + 1)) {
                (Some(e), Some(next)) => e.state.unshared_bytes(&next.state),
                (Some(e), None) => e.state.unshared_bytes(live),
                _ => continue,
            };
            if let Some(e) = self.undo.get_mut(i) {
                e.cost = cost;
                e.fin = fin;
            }
        }
        let m = self.redo.len();
        for i in 0..m {
            // redo[m - 1] follows the live state; redo[i] follows redo[i + 1]
            let fin = i + 1 < m;
            if self.redo.get(i).is_some_and(|e| e.fin) && fin {
                continue;
            }
            let cost = match (self.redo.get(i), self.redo.get(i + 1)) {
                (Some(e), Some(prev)) => e.state.unshared_bytes(&prev.state),
                (Some(e), None) => e.state.unshared_bytes(live),
                _ => continue,
            };
            if let Some(e) = self.redo.get_mut(i) {
                e.cost = cost;
                e.fin = fin;
            }
        }
        self.bytes = self.undo.iter().chain(self.redo.iter()).map(|e| e.cost).sum();
        self.enforce_bytes();
    }

    fn enforce_bytes(&mut self) {
        while self.bytes.saturating_add(self.extra) > self.limits.max_bytes && self.undo.len() + self.redo.len() > 1 {
            let e = if self.undo.len() > 1 || self.redo.is_empty() {
                let e = self.undo.pop_front();
                // the dirtying step keeps being recognised as the top (declared D2a-H1)
                if e.is_some() {
                    self.dirtied_at = self.dirtied_at.and_then(|d| d.checked_sub(1)).filter(|d| *d > 0);
                }
                e
            } else if self.redo.is_empty() {
                None
            } else {
                Some(self.redo.remove(0))
            };
            match e {
                Some(e) => self.bytes = self.bytes.saturating_sub(e.cost),
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct S(u32, usize);

    impl StepCost for S {
        fn unshared_bytes(&self, _next: &Self) -> usize {
            self.1
        }
    }

    #[test]
    fn write_semantics() {
        let mut h: History<S> = History::default();
        assert_eq!(h.push(S(0, 1)).dirty, Some(true));
        h.push(S(1, 1));
        let (s, _) = h.undo(S(2, 1)).unwrap();
        assert_eq!(s, S(1, 1));
        assert!(h.is_dirty());
        assert_eq!(h.set_dirty(false).dirty, Some(false));
        // undo never makes the document clean again
        let (s, c) = h.undo(S(1, 1)).unwrap();
        assert_eq!(s, S(0, 1));
        assert_eq!(c.dirty, Some(true));
        assert_eq!(h.redo_len(), 2);
        let (s, _) = h.redo(S(0, 1)).unwrap();
        assert_eq!(s, S(1, 1));
        // a new step clears redo
        h.push(S(9, 1));
        assert_eq!(h.redo_len(), 0);
    }

    #[test]
    fn discard_of_the_dirtying_step_makes_clean() {
        let mut h: History<S> = History::default();
        h.push(S(0, 1));
        let (_, c) = h.discard();
        assert_eq!(c.dirty, Some(false));
        h.push(S(0, 1));
        h.push(S(1, 1));
        let (_, c) = h.discard();
        assert_eq!(c.dirty, None);
        assert!(h.is_dirty());
    }

    #[test]
    fn step_cap() {
        let mut h: History<S> = History::default();
        for i in 0..250 {
            h.push(S(i, 1));
        }
        assert_eq!(h.undo_len(), MAX_STEPS);
        let (s, _) = h.undo(S(999, 1)).unwrap();
        assert_eq!(s.0, 249);
    }

    #[test]
    fn byte_cap_drops_the_oldest() {
        let mut h: History<S> = History::new(HistoryLimits { max_steps: 200, max_bytes: 100 });
        for i in 0..50 {
            h.push(S(i, 10));
            h.price(&S(1000, 10));
        }
        assert!(h.bytes() <= 100);
        assert_eq!(h.undo_len(), 10);
        let (s, _) = h.undo(S(1000, 10)).unwrap();
        assert_eq!(s.0, 49);
    }
}
