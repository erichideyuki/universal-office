//! The generator's own test schema (`core/tools/uo-schema/tests/schemas/codec_test.uos`), compiled into the fuzz crate.
#![allow(dead_code, unused_imports)]

#[path = "../../../tools/uo-schema/tests/fixtures/codec_test.rs"]
pub mod codec_test;
#[path = "../../../tools/uo-schema/tests/fixtures/codec_test_support.rs"]
pub mod codec_test_support;
