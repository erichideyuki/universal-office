//! wasm32 stack probe of nested stories (G20): a text box nested `levels` deep is read, digested
//! and dropped, one phase per call, so that `js/wasm-stack.mjs` can paint the shadow stack below
//! [`nested_stack_probe`] before each call and find the lowest byte it touched.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../tests/support/nested_stories.rs"]
mod nested_stories;

use std::cell::RefCell;
use std::hint::black_box;

use nested_stories::{NestedRead, Nesting};
use uo_docx::model::{MediaStore, Paragraph};

thread_local! {
    static READ: RefCell<Option<(Vec<Paragraph>, MediaStore)>> = const { RefCell::new(None) };
}

/// The address of a local of this export: the shadow stack pointer at a call from JS, give or
/// take this frame. JS paints below it.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nested_stack_probe() -> usize {
    let local = 0u8;
    black_box(&local) as *const u8 as usize
}

/// One phase over `levels` nested text boxes (`mode` bit 0: positions kept; bit 1: the text boxes
/// read by the block reader of the full document; `mode >> 2`: the index of the kind in
/// `Nesting::ALL`): 0 reads and keeps the result (1 when the read succeeded), 1 digests it (the
/// count of items written), 2 drops it.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nested_stack_phase(levels: u32, mode: u32, phase: u32) -> f64 {
    let levels = black_box(levels as usize);
    match phase {
        0 => {
            let Some(&kind) = Nesting::ALL.get((mode >> 2) as usize) else { return -1.0 };
            let read = nested_stories::read_nested(levels, NestedRead { kind, sources: mode & 1 != 0, blocks: mode & 2 != 0 });
            let ok = read.is_some();
            READ.with(|r| *r.borrow_mut() = read);
            f64::from(u8::from(ok))
        }
        1 => READ.with(|r| r.borrow().as_ref().map_or(-1.0, |(paras, media)| nested_stories::digest(paras, media) as f64)),
        _ => {
            let read = READ.with(|r| r.borrow_mut().take());
            drop(black_box(read));
            0.0
        }
    }
}
