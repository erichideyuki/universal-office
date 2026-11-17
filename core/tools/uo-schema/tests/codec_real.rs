//! R-P2 on the real schemas: `core/schema/render.uos` (the per-page display list) and
//! `core/schema/write.uos` (the Write section carried as the page's `app` payload), and on the Slides schemas
//! `core/schema/slides.uos` (the Slides display list) and `core/schema/slots.uos` (its per-frame animation values).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

#[macro_use]
mod common;
#[deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
mod render_codec;
#[deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
mod slides_codec;
#[deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
mod slots_codec;
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

codec_suite! {
    slides_list,
    gen = crate::slides_codec::slides_generated,
    support = crate::slides_codec::slides_generated_support,
    msg = SlidesList,
    encode = encode_slides_list,
    decode = decode_slides_list,
    dump = dump_slides_list,
    ts = "crates/uo-wasm/js/gen/slides.ts",
    ts_support = "crates/uo-wasm/js/gen/slides.support.ts"
}

codec_suite! {
    slot_frame,
    gen = crate::slots_codec::slots_generated,
    support = crate::slots_codec::slots_generated_support,
    msg = SlotFrame,
    encode = encode_slot_frame,
    decode = decode_slot_frame,
    dump = dump_slot_frame,
    ts = "crates/uo-wasm/js/gen/slots.ts",
    ts_support = "crates/uo-wasm/js/gen/slots.support.ts"
}
