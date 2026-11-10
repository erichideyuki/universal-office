//! Memory accounting for the measured memory gate of the Rust port (rust-port workflow).
//!
//! [`CountingAllocator`] wraps the system allocator and counts, globally and **per thread**,
//! the bytes allocated, the bytes live and the peak of live bytes. Per-thread numbers let
//! tests that run in parallel measure their own operations; document operations run on one
//! thread. A reallocation counts as a new allocation of the new size (like an array that a
//! .NET `List` grows into).
//!
//! The helpers turn the gate into assertions:
//! - M1: [`assert_no_leak`] — live bytes return exactly after the operation's outputs drop;
//! - M2: [`soak`] + [`assert_flat`] — a long scenario keeps live bytes flat;
//! - M3: [`measure`] — bytes allocated and peak above the start, for the module budgets.
//!
//! With the `global` feature the counting allocator is installed for the whole artifact;
//! without it, [`is_counting`] is false and the helpers refuse to assert on zeros.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// The system allocator with byte counters.
#[derive(Debug, Default, Clone, Copy)]
pub struct CountingAllocator;

static TOTAL: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    // no destructors and const initializers: safe to touch from inside the allocator
    static T_TOTAL: Cell<u64> = const { Cell::new(0) };
    static T_LIVE: Cell<i64> = const { Cell::new(0) };
    static T_PEAK: Cell<i64> = const { Cell::new(0) };
}

#[inline]
fn on_alloc(size: usize) {
    let n = size as u64;
    TOTAL.fetch_add(n, Relaxed);
    LIVE.fetch_add(n, Relaxed);
    // try_with: during thread teardown the slots may be gone; then only the global counts
    let _ = T_TOTAL.try_with(|t| t.set(t.get() + n));
    let _ = T_LIVE.try_with(|l| {
        let v = l.get() + size as i64;
        l.set(v);
        let _ = T_PEAK.try_with(|p| {
            if v > p.get() {
                p.set(v);
            }
        });
    });
}

#[inline]
fn on_dealloc(size: usize) {
    LIVE.fetch_sub(size as u64, Relaxed);
    let _ = T_LIVE.try_with(|l| l.set(l.get() - size as i64));
}

// SAFETY: every method forwards to `System` with the same arguments and only adds counting
// that never allocates; the pointers and layouts are passed through unchanged.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ACTIVE.store(true, Relaxed);
        // SAFETY: same contract as GlobalAlloc::alloc, forwarded unchanged.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            on_alloc(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ACTIVE.store(true, Relaxed);
        // SAFETY: same contract as GlobalAlloc::alloc_zeroed, forwarded unchanged.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            on_alloc(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` was allocated by this allocator (i.e. by `System`) with `layout`.
        unsafe { System.dealloc(ptr, layout) };
        on_dealloc(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: same contract as GlobalAlloc::realloc, forwarded unchanged.
        let q = unsafe { System.realloc(ptr, layout, new_size) };
        if !q.is_null() {
            on_dealloc(layout.size());
            on_alloc(new_size);
        }
        q
    }
}

#[cfg(feature = "global")]
#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// The counting allocator is the one in use (feature `global` in this build).
pub fn is_counting() -> bool {
    // a Box forces at least one allocation through the global allocator
    drop(std::hint::black_box(Box::new(0u8)));
    ACTIVE.load(Relaxed)
}

/// Counters of the current thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadStats {
    /// bytes allocated since the thread started
    pub total: u64,
    /// bytes allocated and not freed by this thread (negative if it freed memory of others)
    pub live: i64,
    /// highest `live` since the last [`reset_peak`]
    pub peak: i64,
}

/// Counters of the current thread.
pub fn thread_stats() -> ThreadStats {
    ThreadStats {
        total: T_TOTAL.try_with(Cell::get).unwrap_or(0),
        live: T_LIVE.try_with(Cell::get).unwrap_or(0),
        peak: T_PEAK.try_with(Cell::get).unwrap_or(0),
    }
}

/// Global counters (all threads): (bytes allocated, bytes live).
pub fn global_stats() -> (u64, u64) {
    (TOTAL.load(Relaxed), LIVE.load(Relaxed))
}

/// Restarts the peak of the current thread at its live bytes.
pub fn reset_peak() {
    let live = T_LIVE.try_with(Cell::get).unwrap_or(0);
    let _ = T_PEAK.try_with(|p| p.set(live));
}

/// What an operation cost in memory (current thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// bytes allocated while it ran (M3 budget)
    pub allocated: u64,
    /// highest live bytes above the value at the start (M3 budget)
    pub peak_above_start: i64,
    /// live bytes at the end minus at the start (what it kept, outputs included)
    pub live_delta: i64,
}

/// Runs `f` and returns its result with the memory it used. The result is still alive, so
/// `live_delta` includes it.
pub fn measure<R>(f: impl FnOnce() -> R) -> (R, Usage) {
    let s0 = thread_stats();
    reset_peak();
    let r = f();
    let s1 = thread_stats();
    let usage = Usage { allocated: s1.total - s0.total, peak_above_start: s1.peak - s0.live, live_delta: s1.live - s0.live };
    (r, usage)
}

/// Why a memory assertion failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryCheck {
    /// the counting allocator is not installed: enable the `global` feature in the test build
    NotCounting,
    /// bytes still live after the operation and its outputs were dropped (M1)
    Leak { bytes: i64, usage: Usage },
    /// the soak series is not flat after warm-up (M2)
    Growth { min: i64, max: i64, tolerance: i64, series: Vec<i64> },
}

/// M1: runs `op` (which must drop everything it creates, or return it to be dropped here)
/// and checks that live bytes return exactly to their starting value.
pub fn check_no_leak<R>(op: impl FnOnce() -> R) -> Result<Usage, MemoryCheck> {
    if !is_counting() {
        return Err(MemoryCheck::NotCounting);
    }
    let s0 = thread_stats();
    reset_peak();
    drop(op());
    let s1 = thread_stats();
    let usage = Usage { allocated: s1.total - s0.total, peak_above_start: s1.peak - s0.live, live_delta: s1.live - s0.live };
    if usage.live_delta != 0 {
        return Err(MemoryCheck::Leak { bytes: usage.live_delta, usage });
    }
    Ok(usage)
}

/// M1 as a test assertion: panics with the numbers when [`check_no_leak`] fails.
#[allow(clippy::panic)]
pub fn assert_no_leak<R>(op: impl FnOnce() -> R) -> Usage {
    match check_no_leak(op) {
        Ok(u) => u,
        Err(e) => panic!("memory gate M1 failed: {e:?}"),
    }
}

/// M1 over ALL threads: like [`check_no_leak`] but compares the global live bytes, so work done on
/// other threads (a conversion that runs on its own worker) is counted. Returns the bytes allocated
/// meanwhile. Valid only while no other thread allocates: the callers hold a serial lock, as the
/// .NET M4 tests do.
pub fn check_no_leak_global<R>(op: impl FnOnce() -> R) -> Result<u64, MemoryCheck> {
    if !is_counting() {
        return Err(MemoryCheck::NotCounting);
    }
    let (t0, l0) = global_stats();
    drop(op());
    let (t1, l1) = global_stats();
    let delta = i64::try_from(l1).unwrap_or(i64::MAX) - i64::try_from(l0).unwrap_or(i64::MAX);
    let allocated = t1.saturating_sub(t0);
    if delta != 0 {
        return Err(MemoryCheck::Leak { bytes: delta, usage: Usage { allocated, peak_above_start: 0, live_delta: delta } });
    }
    Ok(allocated)
}

/// [`check_no_leak_global`] as a test assertion; returns the bytes allocated meanwhile.
#[allow(clippy::panic)]
pub fn assert_no_leak_global<R>(op: impl FnOnce() -> R) -> u64 {
    match check_no_leak_global(op) {
        Ok(u) => u,
        Err(e) => panic!("memory gate M1 (global) failed: {e:?}"),
    }
}

/// M2: runs `step(i)` `iterations` times and samples the live bytes of the current thread
/// every `sample_every` iterations (and once before the first).
pub fn soak(iterations: usize, sample_every: usize, mut step: impl FnMut(usize)) -> Vec<i64> {
    let every = sample_every.max(1);
    // the series is allocated BEFORE the first sample: growing it would count as the
    // scenario's memory
    let mut series = Vec::with_capacity(iterations / every + 1);
    series.push(thread_stats().live);
    for i in 0..iterations {
        step(i);
        if (i + 1) % every == 0 {
            series.push(thread_stats().live);
        }
    }
    series
}

/// M2: the series is flat after `warmup` samples: max − min within `tolerance` bytes.
pub fn check_flat(series: &[i64], warmup: usize, tolerance: i64) -> Result<(), MemoryCheck> {
    if !is_counting() {
        return Err(MemoryCheck::NotCounting);
    }
    let rest = series.get(warmup..).unwrap_or(&[]);
    let (min, max) = rest.iter().fold((i64::MAX, i64::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    if rest.is_empty() || max - min <= tolerance {
        return Ok(());
    }
    Err(MemoryCheck::Growth { min, max, tolerance, series: series.to_vec() })
}

/// M2 as a test assertion.
#[allow(clippy::panic)]
pub fn assert_flat(series: &[i64], warmup: usize, tolerance: i64) {
    if let Err(e) = check_flat(series, warmup, tolerance) {
        panic!("memory gate M2 failed: {e:?}");
    }
}
