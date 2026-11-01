//! Parity tooling of the UniversalOffice Rust core (rust-port workflow, stage 4), shared by the
//! verification of every module (opc now; fonts, layout and the rest later).
//!
//! The method (from `experiments/rust-spike`): the frozen TS engine of `main` runs over a
//! corpus and its results are **captured**; the Rust module **replays** the same inputs and
//! must produce exactly the same results.
//!
//! - [`stream`]: hashed canonical streams. Both sides write the same typed items for the value
//!   under comparison and keep only the SHA-256 of the sequence, so captures stay small (no giant
//!   JSON, no V8 limits) and comparison is exact. Optional readable traces locate the first
//!   diverging item.
//! - [`capture`]: the capture file format (one line per input, `facet=value` fields), its
//!   reader and [`capture::Replay`], the exact comparator that reports mismatches, accepted
//!   differences (named decisions, never tolerances) and capture records never replayed.
//! - [`generate`]: seeded generators for differential corpora (the PRNG of the JS generators and a
//!   raw zip builder for archives no real writer produces).
//! - [`zipdir`]: an independent, strict listing of a zip central directory, to check what a
//!   writer under test produced (names, flags, methods, order).
//! - [`sha256`]: the digest.
//!
//! The Node side is `js/testkit.mjs` (same stream encoding, `CaptureWriter`, helpers to load
//! the frozen TS from `UO_TS_ROOT`). Captures need a built checkout of `main` and are a manual,
//! documented step; replays run in plain `cargo test` from the committed capture files.
//!
//! This crate has no dependencies, and in particular never depends on the crates it verifies.

pub mod capture;
pub mod generate;
pub mod sha256;
pub mod stream;
pub mod zipdir;

pub use capture::{Capture, Replay, Report};
pub use stream::{Digest, Stream};

use std::path::{Path, PathBuf};

/// The repository root seen from a crate of `core/crates/` (pass `env!("CARGO_MANIFEST_DIR")`).
pub fn repo_root(manifest_dir: &str) -> PathBuf {
    Path::new(manifest_dir).join("../../..")
}

/// True when the environment variable is set to `1` (opt-in tests that need Node and the
/// frozen TS checkout).
pub fn env_flag(name: &str) -> bool {
    std::env::var(name).as_deref() == Ok("1")
}
