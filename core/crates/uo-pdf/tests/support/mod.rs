//! Shared by the tests of uo-pdf: the op-script interpreter, the test parser, the scalar inputs.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing, clippy::expect_used)]

pub mod hostile;
pub mod inputs;
pub mod json;
pub mod pages;
pub mod pdf_parse;
pub mod script;
