//! What a recorded pass reads and writes between two page checkpoints (LA7): the body blocks it
//! reads besides the one being placed (lookahead, reference fields, list markers, late floats)
//! and the earliest page it writes. A checkpoint is usable to resume a relayout only when nothing
//! before it read the changed blocks and nothing after it wrote the pages before it ("sealed");
//! the previous run can be spliced in at a checkpoint only when nothing after it read them.

use std::cell::Cell;

/// The reads and writes of one interval between checkpoints. `min_read`/`max_read`: the body
/// blocks read (`None` = none); `min_touch`: the lowest page index written (`u32::MAX` = none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Epoch {
    pub reads: Option<(u32, u32)>,
    pub min_touch: u32,
}

impl Default for Epoch {
    fn default() -> Epoch {
        Epoch { reads: None, min_touch: u32::MAX }
    }
}

impl Epoch {
    /// Both intervals' reads and writes.
    #[must_use]
    pub fn merge(self, o: Epoch) -> Epoch {
        let reads = match (self.reads, o.reads) {
            (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
            (x, None) | (None, x) => x,
        };
        Epoch { reads, min_touch: self.min_touch.min(o.min_touch) }
    }
}

/// The trace of the running pass ([`crate::engine::Engine::trace`]). The reads come from `&self`
/// methods of the engine (`flow_block`, `flow_table`), hence the `Cell`s: plain counters, never
/// shared outside the engine that owns the trace.
#[derive(Clone, Debug)]
pub struct Trace {
    min_read: Cell<u32>,
    max_read: Cell<u32>,
    any_read: Cell<bool>,
    min_touch: u32,
    /// the lowest page written since the trace started (not reset by [`Trace::take_epoch`]): the
    /// pages a resumed run changed below its start
    pub lowest_touch: u32,
}

impl Default for Trace {
    fn default() -> Trace {
        Trace::new()
    }
}

impl Trace {
    #[must_use]
    pub fn new() -> Trace {
        Trace {
            min_read: Cell::new(u32::MAX),
            max_read: Cell::new(0),
            any_read: Cell::new(false),
            min_touch: u32::MAX,
            lowest_touch: u32::MAX,
        }
    }

    /// A read of body block `i`.
    pub fn read(&self, i: usize) {
        let i = u32::try_from(i).unwrap_or(u32::MAX);
        self.any_read.set(true);
        self.min_read.set(self.min_read.get().min(i));
        self.max_read.set(self.max_read.get().max(i));
    }

    /// A read that may reach any body block.
    pub fn read_all(&self) {
        self.any_read.set(true);
        self.min_read.set(0);
        self.max_read.set(u32::MAX);
    }

    /// A write to page `page`.
    pub fn touch(&mut self, page: u32) {
        self.min_touch = self.min_touch.min(page);
        self.lowest_touch = self.lowest_touch.min(page);
    }

    /// The interval since the last call, and a new one starts.
    pub fn take_epoch(&mut self) -> Epoch {
        let e = Epoch { reads: self.any_read.get().then(|| (self.min_read.get(), self.max_read.get())), min_touch: self.min_touch };
        self.min_read.set(u32::MAX);
        self.max_read.set(0);
        self.any_read.set(false);
        self.min_touch = u32::MAX;
        e
    }
}
