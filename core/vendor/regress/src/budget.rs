//! uo-calc budget (vendored change, see `Cargo.toml`): a per-thread step budget for the backtracking executor. The
//! searches started after [`set_step_budget`] may dispatch at most that many instructions IN ALL (every start
//! position, backtrack and lookaround of every search: a search takes the remaining budget when it starts and gives
//! back what it did not use when its matcher is dropped); past it a search reports no further match and
//! [`budget_exhausted`] turns true until the next [`set_step_budget`]. The default is unlimited (upstream
//! behaviour).

use core::cell::Cell;

#[cfg(not(feature = "std"))]
compile_error!("the uo-calc step budget needs the `std` feature (a per-thread counter)");

std::thread_local! {
    static BUDGET: Cell<u64> = const { Cell::new(u64::MAX) };
    static EXHAUSTED: Cell<bool> = const { Cell::new(false) };
}

/// Sets the step budget of the searches this thread starts next and clears the exhausted flag.
pub fn set_step_budget(steps: u64) {
    BUDGET.with(|b| b.set(steps));
    EXHAUSTED.with(|e| e.set(false));
}

/// `true` when a search since the last [`set_step_budget`] ran out of steps (its result is incomplete).
pub fn budget_exhausted() -> bool {
    EXHAUSTED.with(Cell::get)
}

/// Takes the whole remaining budget for a new search (given back by [`give_back`]).
pub(crate) fn step_budget() -> u64 {
    BUDGET.with(|b| b.replace(0))
}

pub(crate) fn give_back(steps: u64) {
    BUDGET.with(|b| b.set(b.get().saturating_add(steps)));
}

pub(crate) fn exhaust() {
    EXHAUSTED.with(|e| e.set(true));
}
