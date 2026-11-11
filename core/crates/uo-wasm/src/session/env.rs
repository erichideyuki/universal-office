//! The session's environment in the binding: the host's clock (local and UTC date parts, changed by
//! `ExecOp::SetClock`) and the TS `Math.random` sites as the testkit xorshift32 (the same generator
//! as uo-docx's `FixedEnv`, so a capture replay opened with the default clock and seed 1 equals the
//! native replay).
//!
//! The clock is shared between the handle and the session's `Box<dyn Env>` through one `Rc<Cell>` of a
//! `Copy` value (declared WS-D5: not a graph, one cell of plain data the handle writes and the session
//! reads; the module is single-threaded).

use std::cell::Cell;
use std::rc::Rc;

use uo_docx::session::{DateParts, Env, LocalNow};

/// The default clock: 2026-01-02 03:04 local and UTC (`FixedEnv::default()`).
#[must_use]
pub fn default_now() -> LocalNow {
    let t = DateParts { year: 2026, month: 1, day: 2, hour: 3, minute: 4 };
    LocalNow { local: t, utc: t }
}

/// The host environment of one session.
pub struct HostEnv {
    pub now: Rc<Cell<LocalNow>>,
    state: u32,
}

impl HostEnv {
    /// `seed` 0 is the testkit's fixed seed `0x9e3779b9` (as `FixedEnv::new`).
    #[must_use]
    pub fn new(now: Rc<Cell<LocalNow>>, seed: u32) -> HostEnv {
        HostEnv { now, state: if seed == 0 { 0x9e37_79b9 } else { seed } }
    }
}

impl Env for HostEnv {
    fn now(&self) -> LocalNow {
        self.now.get()
    }

    fn random_u32(&mut self) -> u32 {
        // uo-testkit `Rng` (xorshift32), as uo-docx `FixedEnv`
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_docx::session::FixedEnv;

    #[test]
    fn the_same_sequence_as_the_fixed_env() {
        let mut a = HostEnv::new(Rc::new(Cell::new(default_now())), 1);
        let mut b = FixedEnv::default();
        for _ in 0..100 {
            assert_eq!(a.random_u32(), b.random_u32());
        }
        assert_eq!(a.now(), b.now());
        let mut z = HostEnv::new(Rc::new(Cell::new(default_now())), 0);
        let mut f = FixedEnv::new(default_now(), 0);
        assert_eq!(z.random_u32(), f.random_u32());
    }
}
