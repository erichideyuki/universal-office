//! The editing session's history (`history.ts`), feature `edit`, frontier B8: snapshot undo of the
//! byte parts. A step keeps the package before and after (one `Arc` per part, shared: the editor
//! replaces whole parts, never bytes in place) and the UI state (slide index, selected shape). The
//! byte cap is new (memory gate M5, `uo-pptx.md` P6); the 100-step limit is the TS's.

use std::collections::VecDeque;

use crate::package::PartsSnapshot;

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

/// `EditHistory` (`history.ts:41`).
#[derive(Clone, Debug)]
pub struct EditHistory {
    steps: VecDeque<Step>,
    /// How many steps are applied (the rest can be redone).
    pos: usize,
    /// The position of the last save; `None` = the saved file left the history.
    saved_pos: Option<usize>,
    limit: usize,
    byte_cap: u64,
}

impl EditHistory {
    /// An empty history with the TS step limit and a byte cap.
    pub fn new(limit: usize, byte_cap: u64) -> EditHistory {
        EditHistory { steps: VecDeque::new(), pos: 0, saved_pos: Some(0), limit, byte_cap }
    }

    pub fn can_undo(&self) -> bool {
        self.pos > 0
    }

    pub fn can_redo(&self) -> bool {
        self.pos < self.steps.len()
    }

    /// `dirty`: the applied position differs from the saved one.
    pub fn dirty(&self) -> bool {
        self.saved_pos != Some(self.pos)
    }

    /// The step limit (100 in the TS) and the byte cap.
    pub fn caps(&self) -> (usize, u64) {
        (self.limit, self.byte_cap)
    }
}

impl Default for EditHistory {
    fn default() -> Self {
        EditHistory::new(100, 64 << 20)
    }
}
