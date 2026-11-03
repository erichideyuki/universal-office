//! JavaScript numerics of the UniversalOffice Rust core, shared by every engine crate of Write,
//! Sheet and Slides: the JS number model as V8 13.6.233.17 (Node 24.15.0) computes it, so the
//! Rust engines give the bits the frozen TS engines gave.
//!
//! - [`math`]: `Math.*` and `**`: the fdlibm functions of V8 (not the platform libm, which
//!   differs in bits from one OS to the next), `hypot`, `round`, `sign`, `max`/`min`, `imul`,
//!   `clz32`, `fround`.
//! - [`conv`]: ToInt32 and friends, typed-array element stores, `js_index`, NaN
//!   canonicalization, truthiness.
//! - [`text`]: the JS white space set, `trim`, `split(/\s+/)`, UTF-16 length and slice.
//! - [`parse`]: `Number(s)` and `parseInt`.
//!
//! This crate is the only place of the workspace where the `f64` methods that differ from JS
//! (`sin`, `ln`, `powf`, `round`, `max`, `signum`, `to_radians`, ...) may be called: they are
//! banned workspace-wide by `core/clippy.toml`, and the replacements are here. Plan and
//! decisions: `core/docs/modules/uo-jsnum.md`.
//!
//! Nothing here allocates except the strings that are returned, keeps state, or panics.
//! Not `no_std`: `core` has no `f64::sqrt`, `floor`, `ceil` or `trunc` yet.
#![forbid(unsafe_code)]

pub mod conv;
mod fdlibm;
pub mod math;
pub mod parse;
pub mod text;

/// Nothing in this crate caches across calls; kept for the memory-gate convention (M1).
pub fn clear_caches() {}
