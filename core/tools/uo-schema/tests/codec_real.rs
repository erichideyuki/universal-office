//! R-P2 on the real schemas: `core/schema/render.uos` (the per-page display list) and
//! `core/schema/write.uos` (the Write section carried as the page's `app` payload).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

#[macro_use]
mod common;
#[deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
mod render_codec;
#[deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
mod write_codec;

codec_suite! {
    render_page,
    gen = crate::render_codec::generated,
    support = crate::render_codec::generated_support,
    msg = Page,
    encode = encode_page,
    decode = decode_page,
    dump = dump_page,
    ts = "crates/uo-wasm/js/gen/render.ts",
    ts_support = "crates/uo-wasm/js/gen/render.support.ts"
}

codec_suite! {
    write_page,
    gen = crate::write_codec::generated,
    support = crate::write_codec::generated_support,
    msg = WritePage,
    encode = encode_write_page,
    decode = decode_write_page,
    dump = dump_write_page,
    ts = "crates/uo-wasm/js/gen/write.ts",
    ts_support = "crates/uo-wasm/js/gen/write.support.ts"
}
