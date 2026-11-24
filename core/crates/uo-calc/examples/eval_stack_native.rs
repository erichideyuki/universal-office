//! Native stack of the A2 recursion (decision A2-2): `eval_stack_native [levels...]` parses, prints and evaluates
//! each scenario of `stack_common` nested `levels` deep and prints the bytes of stack each phase used. The stack
//! below the caller is painted with two patterns, the phase runs, and the lowest byte it changed is found (the
//! wasm32 probe does the same over the shadow stack). Run in release: `cargo run -p uo-calc --release --example
//! eval_stack_native`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stdout)]

#[path = "stack_common/mod.rs"]
mod stack_common;

use std::hint::black_box;

/// Bytes painted below the caller.
const SPAN: usize = 24 << 20;
/// Room for the frames of `paint`/`scan`.
const GAP: usize = 4096;

#[inline(never)]
fn here() -> usize {
    let local = 0u8;
    black_box(&local) as *const u8 as usize
}

#[inline(never)]
fn paint(top: usize, pattern: u8) {
    for i in 0..SPAN {
        // SAFETY: the bytes are in this thread's stack (spawned with SPAN + 8 MiB), below every live frame (GAP
        // under the caller's), written top-down so that each page is touched after the one above it.
        unsafe { std::ptr::write_volatile((top - GAP - i) as *mut u8, pattern) };
    }
}

#[inline(never)]
fn lowest_touched(top: usize, pattern: u8) -> usize {
    let bottom = top - GAP - (SPAN - 1);
    let mut addr = bottom;
    // SAFETY: the same bytes `paint` wrote.
    while addr < top - GAP && unsafe { std::ptr::read_volatile(addr as *const u8) } == pattern {
        addr += 1;
    }
    assert!(addr > bottom, "the phase used more than the painted stack");
    top - addr
}

fn painted(mut f: impl FnMut()) -> usize {
    let mut used = 0;
    for pattern in [0xa5, 0x5a] {
        let top = here();
        paint(top, pattern);
        f();
        used = used.max(lowest_touched(top, pattern));
    }
    used
}

fn main() {
    let levels: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let levels = if levels.is_empty() { vec![1, 64, 256, 511, 512, 513, 100_000] } else { levels };
    std::thread::Builder::new()
        .stack_size(SPAN + (8 << 20))
        .spawn(move || {
            for kind in stack_common::KINDS {
                for &n in &levels {
                    let mut c = stack_common::case(kind, n);
                    let mut len = 0;
                    let p = painted(|| len = black_box(stack_common::print(&c)));
                    let mut r = 0.0;
                    let e = painted(|| r = black_box(stack_common::eval(&mut c)));
                    let per = if n > 0 { format!("{:.0} B/level", e as f64 / n as f64) } else { String::new() };
                    println!("{kind:6} {n:5}: parsed {} print {p:8} B  eval {e:8} B {per:>14}  result {r}", c.root.is_some());
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
