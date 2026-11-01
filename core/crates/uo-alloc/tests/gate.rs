//! The gate helpers must catch what they claim to catch: a leak, a growing soak, and count
//! the bytes of an operation. Run with `cargo test -p uo-alloc --features global`.
#![cfg(feature = "global")]

use std::hint::black_box;

use uo_alloc::{MemoryCheck, assert_flat, assert_no_leak, check_flat, check_no_leak, is_counting, measure, soak};

#[test]
fn counting_is_installed() {
    assert!(is_counting());
}

#[test]
fn no_leak_passes_when_everything_is_dropped() {
    let usage = assert_no_leak(|| {
        let v: Vec<u64> = black_box((0..1000).collect());
        v.iter().sum::<u64>()
    });
    assert!(usage.allocated >= 8000, "the vector was counted: {usage:?}");
    assert!(usage.peak_above_start >= 8000);
}

#[test]
fn outputs_returned_by_the_operation_are_dropped_and_not_a_leak() {
    assert_no_leak(|| black_box(vec![0u8; 4096]));
}

#[test]
fn a_leak_is_caught_with_its_size() {
    let mut leaked: *mut [u8; 512] = std::ptr::null_mut();
    let r = check_no_leak(|| leaked = black_box(Box::into_raw(black_box(Box::new([0u8; 512])))));
    match r {
        Err(MemoryCheck::Leak { bytes, .. }) => assert_eq!(bytes, 512),
        other => panic!("expected a leak of 512 bytes, got {other:?}"),
    }
    // give it back, so Miri's own leak check (M6) stays clean
    // SAFETY: `leaked` came from Box::into_raw above and was not freed.
    drop(unsafe { Box::from_raw(leaked) });
}

#[test]
fn measure_reports_what_stays_alive() {
    let (kept, usage) = measure(|| black_box(vec![0u8; 2048]));
    assert_eq!(usage.live_delta, 2048);
    drop(kept);
}

#[test]
fn a_flat_soak_passes() {
    let series = soak(500, 50, |i| {
        let v = black_box(vec![i as u8; 1024]);
        std::hint::black_box(&v);
    });
    assert_eq!(series.len(), 11);
    assert_flat(&series, 1, 0);
}

#[test]
fn a_growing_soak_is_caught() {
    let mut kept = Vec::new();
    let series = soak(500, 50, |i| kept.push(vec![i as u8; 100]));
    assert!(matches!(check_flat(&series, 1, 1024), Err(MemoryCheck::Growth { .. })));
    drop(kept);
}
