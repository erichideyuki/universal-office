//! The generated codecs of the two Slides schemas (`core/schema/slides.uos`, `core/schema/slots.uos`), compiled into the fuzz
//! crate from the uo-render tree for the generated-message targets (the validated decoder is `uo_render::slides`, a direct
//! dependency).
#![allow(dead_code, unused_imports)]

#[path = "../../crates/uo-render/src/codec/slides_generated.rs"]
pub mod slides_generated;
#[path = "../../crates/uo-render/src/codec/slides_generated_support.rs"]
pub mod slides_generated_support;
#[path = "../../crates/uo-render/src/codec/slots_generated.rs"]
pub mod slots_generated;
#[path = "../../crates/uo-render/src/codec/slots_generated_support.rs"]
pub mod slots_generated_support;
