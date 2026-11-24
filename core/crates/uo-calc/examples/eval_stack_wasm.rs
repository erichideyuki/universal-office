//! wasm32 stack probe of the A2 recursion (decision A2-2): a scenario of `stack_common` nested `levels` deep is
//! parsed (setup), then printed or evaluated, one phase per call, so that `js/wasm-stack.mjs` can paint the shadow
//! stack below [`eval_stack_probe`] before each call and find the lowest byte it touched.
//!
//! `cargo build -p uo-calc --example eval_stack_wasm --target wasm32-unknown-unknown --release`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "stack_common/mod.rs"]
mod stack_common;

use std::cell::RefCell;
use std::hint::black_box;

thread_local! {
    static CASE: RefCell<Option<stack_common::Case>> = const { RefCell::new(None) };
}

/// The address of a local of this export: the shadow stack pointer at a call from JS, give or take this frame.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn eval_stack_probe() -> usize {
    let local = 0u8;
    black_box(&local) as *const u8 as usize
}

/// Phase `0` parses `KINDS[kind]` nested `levels` deep (1 = parsed, 0 = refused), `1` prints it (the text length),
/// `2` evaluates it (the result code of `stack_common::eval`), `3` drops it.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn eval_stack_phase(kind: u32, levels: u32, phase: u32) -> f64 {
    let levels = black_box(levels as usize);
    match phase {
        0 => {
            let Some(&k) = stack_common::KINDS.get(kind as usize) else { return -1.0 };
            let c = stack_common::case(k, levels);
            let ok = c.root.is_some();
            CASE.with(|x| *x.borrow_mut() = Some(c));
            f64::from(u8::from(ok))
        }
        1 => CASE.with(|x| x.borrow().as_ref().map_or(-1.0, |c| stack_common::print(c) as f64)),
        2 => CASE.with(|x| x.borrow_mut().as_mut().map_or(-1.0, stack_common::eval)),
        _ => {
            let c = CASE.with(|x| x.borrow_mut().take());
            drop(black_box(c));
            0.0
        }
    }
}

fn main() {}
