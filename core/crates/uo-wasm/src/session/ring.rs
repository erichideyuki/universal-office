//! The Office Clipboard of the wasm instance (G7): one [`ClipboardRing`] shared by every session of
//! the module (the TS keeps one per page), lent to a command for the duration of the call.

use std::cell::RefCell;

use uo_docx::session::ClipboardRing;

thread_local! {
    static RING: RefCell<ClipboardRing> = RefCell::new(ClipboardRing::default());
}

/// Runs `f` with the instance's ring. A call from inside `f` (no export calls another) gets a fresh
/// ring instead of panicking.
pub fn with_ring<R>(f: impl FnOnce(&mut ClipboardRing) -> R) -> R {
    RING.with(|r| match r.try_borrow_mut() {
        Ok(mut ring) => f(&mut ring),
        Err(_) => f(&mut ClipboardRing::default()),
    })
}

/// Empties the Office Clipboard of the instance (the memory tests take their baseline after it).
#[unsafe(no_mangle)]
pub extern "C" fn uo_session_clipboard_clear() {
    with_ring(ClipboardRing::clear);
}
