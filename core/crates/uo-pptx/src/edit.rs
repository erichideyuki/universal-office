//! The editing session of the presentation (feature `edit`, frontier B8): the 46 operations of
//! `deck-edit.ts` as string surgery over the part store ([`pkg`], [`shapes`], [`doc`], with the XML
//! scan primitives of `xml-edit.ts` in [`xml`]), the blank presentation ([`blank`]), the font
//! embedding of the save ([`fonts`]), snapshot undo (this module, `history.ts`) and the
//! [`Editor`] session that ties them to a [`crate::Deck`] ([`session`], the `edit()` / `undo()` /
//! `redo()` of `universal-slides.ts` without the DOM).
//!
//! Snapshot undo: a step keeps the package before and after (one `Arc` per part, shared: the editor
//! replaces whole parts, never bytes in place) and the UI state (slide index, selected shape). The
//! byte cap is new (memory gate M5, `uo-pptx.md` P6 and "B8"); the 100-step limit is the TS's.

use std::collections::VecDeque;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::package::{PartEntry, PartStore, PartsSnapshot};

pub mod blank;
pub mod doc;
pub mod fonts;
pub mod pkg;
pub mod random;
pub mod session;
pub mod shapes;
pub mod xml;

pub use pkg::EditError;
pub use random::Xorshift32;
pub use session::{Editor, Op, Outcome, Reread, SavedPackage, Target, Then};

/// The UI state saved with a step (`EditState`, `history.ts:13`).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EditState {
    pub index: u32,
    pub shape_id: Option<Box<str>>,
}

/// One undo step (`Step`, `history.ts:15`).
#[derive(Clone, Debug)]
pub struct Step {
    pub label: Box<str>,
    pub before: PartsSnapshot,
    pub after: PartsSnapshot,
    pub state_before: EditState,
    pub state_after: EditState,
}

/// `changedSince`: the package differs from the snapshot (by size, then by part identity: the `Arc`).
pub fn changed_since(store: &PartStore, snap: &PartsSnapshot) -> bool {
    if store.len() != snap.0.len() {
        return true;
    }
    let by_name: FxHashMap<&str, &Arc<[u8]>> = snap.0.iter().map(|e| (&*e.name, &e.bytes)).collect();
    store.entries().iter().any(|e| by_name.get(&*e.name).is_none_or(|b| !Arc::ptr_eq(b, &e.bytes)))
}

/// What changed since `snap` (the binding's sync of the shell's copy of the package): the parts that are new or were
/// replaced, in store order, and the names of the parts the store no longer has, in snapshot order.
pub fn changes_since<'a>(store: &'a PartStore, snap: &PartsSnapshot) -> (Vec<&'a PartEntry>, Vec<Box<str>>) {
    let by_name: FxHashMap<&str, &Arc<[u8]>> = snap.0.iter().map(|e| (&*e.name, &e.bytes)).collect();
    let changed = store.entries().iter().filter(|e| by_name.get(&*e.name).is_none_or(|b| !Arc::ptr_eq(b, &e.bytes))).collect();
    let gone = snap.0.iter().filter(|e| store.bytes(&e.name).is_none()).map(|e| e.name.clone()).collect();
    (changed, gone)
}

/// `restore`: gives the package back to the snapshot (only the parts that differ): the parts that are
/// not in it go, then the differing ones are set in snapshot order.
pub fn restore(store: &mut PartStore, snap: &PartsSnapshot) {
    let present: rustc_hash::FxHashSet<&str> = snap.0.iter().map(|e| &*e.name).collect();
    let gone: Vec<Box<str>> = store.names().filter(|n| !present.contains(n)).map(Box::from).collect();
    for n in gone {
        store.remove(&n);
    }
    for e in &snap.0 {
        if store.bytes(&e.name).is_none_or(|b| !Arc::ptr_eq(b, &e.bytes)) {
            store.set(&e.name, Arc::clone(&e.bytes));
        }
    }
}

/// `EditHistory` (`history.ts:41`) with the byte cap.
#[derive(Clone, Debug)]
pub struct EditHistory {
    steps: VecDeque<Step>,
    /// How many steps are applied (the rest can be redone).
    pos: usize,
    /// The position of the last save; `None` = the saved file left the history.
    saved_pos: Option<usize>,
    limit: usize,
    byte_cap: u64,
    /// Bytes retained by the distinct part versions of the snapshots in the history (by `Arc` address:
    /// length, number of snapshots holding it).
    retained: FxHashMap<usize, (u64, u32)>,
    retained_bytes: u64,
    /// Bytes of the live package at the last push (every part of it is in `retained`): the cap bounds
    /// what the history holds BEYOND the package, not the package itself.
    live_bytes: u64,
}

fn addr(a: &Arc<[u8]>) -> usize {
    Arc::as_ptr(a).cast::<u8>() as usize
}

impl EditHistory {
    /// An empty history with the step limit (100 in the TS) and a byte cap.
    pub fn new(limit: usize, byte_cap: u64) -> EditHistory {
        EditHistory {
            steps: VecDeque::new(),
            pos: 0,
            saved_pos: Some(0),
            limit,
            byte_cap,
            retained: FxHashMap::default(),
            retained_bytes: 0,
            live_bytes: 0,
        }
    }

    fn hold(&mut self, s: &PartsSnapshot) {
        for e in &s.0 {
            let slot = self.retained.entry(addr(&e.bytes)).or_insert_with(|| {
                self.retained_bytes += e.bytes.len() as u64;
                (e.bytes.len() as u64, 0)
            });
            slot.1 += 1;
        }
    }

    fn release(&mut self, s: &PartsSnapshot) {
        for e in &s.0 {
            let key = addr(&e.bytes);
            if let Some(slot) = self.retained.get_mut(&key) {
                slot.1 -= 1;
                if slot.1 == 0 {
                    self.retained_bytes -= slot.0;
                    self.retained.remove(&key);
                }
            }
        }
    }

    fn drop_oldest(&mut self) {
        if let Some(s) = self.steps.pop_front() {
            self.release(&s.before);
            self.release(&s.after);
        }
        self.pos = self.pos.saturating_sub(1);
        self.saved_pos = match self.saved_pos {
            Some(p) if p > 0 => Some(p - 1),
            _ => None,
        };
    }

    /// `push`: truncates the redo tail, adds the step, enforces the step limit and the byte cap (the
    /// oldest steps go; the newest one is always kept).
    pub fn push(&mut self, label: &str, before: PartsSnapshot, after: PartsSnapshot, state_before: EditState, state_after: EditState) {
        while self.steps.len() > self.pos {
            if let Some(s) = self.steps.pop_back() {
                self.release(&s.before);
                self.release(&s.after);
            }
        }
        if self.saved_pos.is_some_and(|p| p > self.pos) {
            self.saved_pos = None;
        }
        self.hold(&before);
        self.hold(&after);
        let mut seen = rustc_hash::FxHashSet::default();
        self.live_bytes = after.0.iter().filter(|e| seen.insert(addr(&e.bytes))).map(|e| e.bytes.len() as u64).sum();
        self.steps.push_back(Step { label: label.into(), before, after, state_before, state_after });
        self.pos += 1;
        while self.steps.len() > self.limit && !self.steps.is_empty() {
            self.drop_oldest();
        }
        while self.retained_bytes.saturating_sub(self.live_bytes) > self.byte_cap && self.steps.len() > 1 {
            self.drop_oldest();
        }
    }

    pub fn can_undo(&self) -> bool {
        self.pos > 0
    }

    pub fn can_redo(&self) -> bool {
        self.pos < self.steps.len()
    }

    /// The label of the step `undo` would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.pos.checked_sub(1).and_then(|i| self.steps.get(i)).map(|s| &*s.label)
    }

    /// The label of the step `redo` would apply.
    pub fn redo_label(&self) -> Option<&str> {
        self.steps.get(self.pos).map(|s| &*s.label)
    }

    /// Undo: the snapshot and the UI state from before the step.
    pub fn undo(&mut self) -> Option<(PartsSnapshot, EditState)> {
        if !self.can_undo() {
            return None;
        }
        self.pos -= 1;
        let s = self.steps.get(self.pos)?;
        Some((s.before.clone(), s.state_before.clone()))
    }

    /// Redo: the snapshot and the UI state after the step.
    pub fn redo(&mut self) -> Option<(PartsSnapshot, EditState)> {
        let s = self.steps.get(self.pos)?;
        let out = (s.after.clone(), s.state_after.clone());
        self.pos += 1;
        Some(out)
    }

    /// Empties the history (a new/opened document).
    pub fn clear(&mut self) {
        self.steps.clear();
        self.retained.clear();
        self.retained_bytes = 0;
        self.live_bytes = 0;
        self.pos = 0;
        self.saved_pos = Some(0);
    }

    /// `markSaved`.
    pub fn mark_saved(&mut self) {
        self.saved_pos = Some(self.pos);
    }

    /// `dirty`: the applied position differs from the saved one.
    pub fn dirty(&self) -> bool {
        self.saved_pos != Some(self.pos)
    }

    /// The step limit (100 in the TS) and the byte cap.
    pub fn caps(&self) -> (usize, u64) {
        (self.limit, self.byte_cap)
    }

    /// Steps in the history.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// No steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Bytes retained by the distinct part versions of the history, the live package included.
    pub fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    /// What the history holds beyond the live package (the M5 measure the byte cap bounds).
    pub fn extra_bytes(&self) -> u64 {
        self.retained_bytes.saturating_sub(self.live_bytes)
    }
}

impl Default for EditHistory {
    fn default() -> Self {
        EditHistory::new(100, 64 << 20)
    }
}
