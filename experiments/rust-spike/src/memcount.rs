//! Alocador que conta bytes (o equivalente dos contadores do GC no TS e no C#): total alocado,
//! vivo agora e pico do vivo. Realocação conta o tamanho novo inteiro, como o crescimento de
//! uma List no .NET (array novo).
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

pub struct Counting;

static TOTAL: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

#[inline]
fn grow(n: usize) {
    TOTAL.fetch_add(n, Relaxed);
    let live = LIVE.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(live, Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            LIVE.fetch_sub(l.size(), Relaxed);
            grow(new);
        }
        q
    }
}

/// (total alocado, vivo, pico) em bytes.
pub fn stats() -> (usize, usize, usize) {
    (TOTAL.load(Relaxed), LIVE.load(Relaxed), PEAK.load(Relaxed))
}

/// Recomeça o pico a partir do vivo de agora.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Relaxed), Relaxed);
}
