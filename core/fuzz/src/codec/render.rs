//! The display list's codec (`core/schema/render.uos`), compiled into the fuzz crate from the uo-render tree.
#![allow(dead_code, unused_imports)]

#[path = "../../../crates/uo-render/src/codec/generated.rs"]
pub mod generated;
#[path = "../../../crates/uo-render/src/codec/generated_support.rs"]
pub mod generated_support;
