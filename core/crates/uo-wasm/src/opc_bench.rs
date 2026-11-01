//! Bench-only exports (feature `opc-bench`) of the uo-opc benchmark (rust-port stage 5,
//! `tools/bench/opc/wasm.mjs`): the scenarios of `uo-opc/tests/bench_support` on byte buffers
//! from [`crate::uo_alloc_bytes`], which stay owned by JS (they are only read here). Each export
//! returns a checksum; the memory counts of the last call are read with [`uo_opc_bench_cost`].
//! Never in the shipped module.

use std::cell::Cell;

use uo_opc::Limits;
use uo_opc::package::Package;

use crate::bench_support::{self, Cost, Totals, frames, kind_of};
use crate::memory_support::open_options;

thread_local! {
    static LAST: Cell<Cost> = const { Cell::new(Cost { allocated: 0, peak: 0, kept: 0 }) };
}

/// # Safety
/// `ptr`/`len` must be a live buffer of `len` initialized bytes.
unsafe fn input<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() {
        return &[];
    }
    // SAFETY: the caller guarantees the buffer is live and initialized for `len` bytes.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

fn done(t: Totals, c: Cost) -> f64 {
    LAST.with(|l| l.set(c));
    (t.parts + t.elements + t.bytes) as f64
}

/// Memory of the last call: 0 allocated bytes, 1 highest peak of one operation, 2 kept bytes.
#[unsafe(no_mangle)]
pub extern "C" fn uo_opc_bench_cost(field: u32) -> f64 {
    let c = LAST.with(Cell::get);
    match field {
        0 => c.allocated as f64,
        1 => c.peak as f64,
        _ => c.kept as f64,
    }
}

/// (a) every package of a framed corpus (u32 length + bytes each), opened, read and parsed.
///
/// # Safety
/// `ptr`/`len` must be a live buffer of `len` initialized bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_bench_corpus(ptr: *const u8, len: usize, kind: u32) -> f64 {
    // SAFETY: forwarded from the caller.
    let files = frames(unsafe { input(ptr, len) });
    let (t, c) = bench_support::corpus(&files, kind_of(kind));
    done(t, c)
}

/// (b) one package opened, read and parsed.
///
/// # Safety
/// `ptr`/`len` must be a live buffer of `len` initialized bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_bench_package(ptr: *const u8, len: usize, kind: u32) -> f64 {
    // SAFETY: forwarded from the caller.
    let (t, c) = bench_support::package(unsafe { input(ptr, len) }, kind_of(kind));
    done(t, c)
}

/// (c) the entry `name` (UTF-8 at `name_ptr`) of an archive inflated.
///
/// # Safety
/// Both buffers must be live and initialized for their lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_bench_inflate(ptr: *const u8, len: usize, name_ptr: *const u8, name_len: usize) -> f64 {
    // SAFETY: forwarded from the caller.
    let (archive, name) = unsafe { (input(ptr, len), input(name_ptr, name_len)) };
    let (n, c) = bench_support::inflate_entry(archive, &String::from_utf8_lossy(name));
    done(Totals { bytes: n, ..Totals::default() }, c)
}

/// Opens a package for [`uo_opc_bench_save`] (outside its timing); free with
/// [`uo_opc_bench_package_free`].
///
/// # Safety
/// `ptr`/`len` must be a live buffer of `len` initialized bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_bench_open(ptr: *const u8, len: usize, kind: u32) -> *mut Package {
    // SAFETY: forwarded from the caller.
    match Package::open(unsafe { input(ptr, len) }, &Limits::DEFAULT, &open_options(kind_of(kind))) {
        Ok(p) => Box::into_raw(Box::new(p)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `pkg` must be null or a live handle from [`uo_opc_bench_open`], released once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_bench_package_free(pkg: *mut Package) {
    if !pkg.is_null() {
        // SAFETY: the caller guarantees `pkg` came from uo_opc_bench_open and is released once.
        drop(unsafe { Box::from_raw(pkg) });
    }
}

/// (d) the edit of the main part `name` and the save; returns the archive size.
///
/// # Safety
/// `pkg` must be a live handle; `name_ptr`/`name_len` a live buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uo_opc_bench_save(pkg: *mut Package, name_ptr: *const u8, name_len: usize) -> f64 {
    // SAFETY: the caller guarantees `pkg` is null or a live handle and the name buffer is live.
    let (Some(pkg), name) = (unsafe { pkg.as_mut() }, unsafe { input(name_ptr, name_len) }) else { return -1.0 };
    let (n, c) = bench_support::edit_and_save(pkg, &String::from_utf8_lossy(name));
    done(Totals { bytes: n, ..Totals::default() }, c)
}
