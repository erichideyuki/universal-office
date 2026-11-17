//! Support of the chart parity harness (frontier C-0): the capture readers, the digests, `RecCanvas`, the stage plug points
//! and the replay driver. Shared by `tests/parity.rs`; see `core/docs/modules/uo-charts.md` "C-0 — harness".
#![allow(dead_code)]

pub mod data;
pub mod diff_cases;
pub mod digest;
pub mod facets;
pub mod host;
pub mod json;
pub mod layout_json;
pub mod rec;
pub mod selftest;
pub mod stages;
