//! Native stack of nested stories: `nested_stack_native <levels>... [sources] [blocks] [kind]`
//! reads a text box nested `levels` deep (G20; `blocks`: through the block reader of the full
//! document; `kind`: `vml` (default), `dml`, `dml-group`, `vml-group`, `alternate`, `mixed`) and prints the bytes of stack the read used. The stack below the
//! caller is painted with a pattern (twice, two patterns), the read runs, and the lowest byte it
//! changed is found (the wasm32 probe does the same over the shadow stack). Painting goes down one
//! byte at a time, so the guard page of a Windows thread stack moves as it would for a call.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../tests/support/nested_stories.rs"]
mod nested_stories;

use std::hint::black_box;

use nested_stories::{NestedRead, Nesting};

/// Bytes painted below the caller (the read must use less).
const SPAN: usize = 6 << 20;
/// Room left for the frames of `paint`/`scan` themselves.
const GAP: usize = 4096;

/// The address of a local of this frame: the stack pointer of the caller, give or take a frame.
#[inline(never)]
fn here() -> usize {
    let local = 0u8;
    black_box(&local) as *const u8 as usize
}

#[inline(never)]
fn paint(top: usize, pattern: u8) {
    for i in 0..SPAN {
        // SAFETY: the bytes are in this thread's stack (spawned with SPAN + 2 MiB), below every
        // live frame (GAP under the caller's), written top-down so that each page is touched
        // after the one above it.
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
    assert!(addr > bottom, "the read used more than the painted stack");
    top - addr
}

/// The most stack `f` used, over two patterns.
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

/// The stack of the read, of the digest and of the drop of `levels` nested text boxes.
fn measure(levels: usize, how: NestedRead) -> [usize; 3] {
    let mut reads = Vec::with_capacity(2);
    let read = painted(|| reads.push(black_box(nested_stories::read_nested(black_box(levels), how)).unwrap()));
    let digest = painted(|| {
        let (paras, media) = &reads[0];
        black_box(nested_stories::digest(black_box(paras), media));
    });
    let dropped = painted(|| drop(black_box(reads.pop())));
    [read, digest, dropped]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kind = match args.iter().find(|a| a.parse::<usize>().is_err() && *a != "sources" && *a != "blocks").map(String::as_str) {
        None | Some("vml") => Nesting::Vml,
        Some("dml") => Nesting::Dml,
        Some("dml-group") => Nesting::DmlGroup,
        Some("vml-group") => Nesting::VmlGroup,
        Some("alternate") => Nesting::Alternate,
        Some("mixed") => Nesting::Mixed,
        Some(other) => panic!("unknown kind {other}"),
    };
    let how = NestedRead { kind, sources: args.iter().any(|a| a == "sources"), blocks: args.iter().any(|a| a == "blocks") };
    let levels: Vec<usize> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let out = std::thread::Builder::new()
        .stack_size(SPAN + (2 << 20))
        .spawn(move || levels.iter().map(|&n| (n, measure(n, how))).collect::<Vec<_>>())
        .unwrap()
        .join()
        .unwrap();
    println!("{how:?}");
    for (n, [read, digest, dropped]) in out {
        println!("levels {n:>3}: read {read:>8} B, digest {digest:>8} B, drop {dropped:>8} B");
    }
}
