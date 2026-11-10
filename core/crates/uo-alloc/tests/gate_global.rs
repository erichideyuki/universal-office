//! The global M1 helper counts work done on other threads. Own binary: no other test may allocate
//! while it measures. Run with `cargo test -p uo-alloc --features global`.
#![cfg(feature = "global")]

use std::hint::black_box;

use uo_alloc::{MemoryCheck, assert_no_leak_global, check_no_leak_global};

#[test]
fn the_global_check_sees_other_threads() {
    // balanced work on another thread passes
    let allocated = assert_no_leak_global(|| {
        std::thread::spawn(|| black_box(vec![0u8; 8192]).len()).join().unwrap_or(0)
    });
    assert!(allocated >= 8192, "the worker's vector was counted: {allocated}");
    // a 512-byte leak made on another thread is caught with its size
    let mut leaked = 0usize;
    let r = check_no_leak_global(|| {
        leaked = std::thread::spawn(|| Box::into_raw(black_box(Box::new([0u8; 512]))) as usize).join().unwrap_or(0);
    });
    match r {
        Err(MemoryCheck::Leak { bytes, .. }) => assert_eq!(bytes, 512),
        other => panic!("expected a leak of 512 bytes, got {other:?}"),
    }
    // SAFETY: `leaked` came from Box::into_raw on the worker and was not freed.
    drop(unsafe { Box::from_raw(leaked as *mut [u8; 512]) });
}
