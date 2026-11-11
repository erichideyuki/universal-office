//! R-P2 on the session schema (`core/schema/session.uos`, milestone 2: the session in wasm): every message of
//! the session protocol through the suite of codec_real.rs (Rust round trip, TS decode/encode, canonical form).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

#[macro_use]
mod common;
// the session codec is included by uo-wasm under #[allow(clippy::all)]: a list of a unit-only enum emits a
// redundant .copied() there (a generator wart outside this frontier; render.uos and write.uos have no such list)
#[allow(clippy::all)]
#[deny(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::indexing_slicing)]
mod session_codec;

codec_suite! {
    session_exec,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = Exec,
    encode = encode_exec,
    decode = decode_exec,
    dump = dump_exec,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_complete,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = Complete,
    encode = encode_complete,
    decode = decode_complete,
    dump = dump_complete,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_exec_result,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = ExecResult,
    encode = encode_exec_result,
    decode = decode_exec_result,
    dump = dump_exec_result,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_query,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = Query,
    encode = encode_query,
    decode = decode_query,
    dump = dump_query,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_query_result,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = QueryResult,
    encode = encode_query_result,
    decode = decode_query_result,
    dump = dump_query_result,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_session_open,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = SessionOpen,
    encode = encode_session_open,
    decode = decode_session_open,
    dump = dump_session_open,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_font_requests,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = FontRequests,
    encode = encode_font_requests,
    decode = decode_font_requests,
    dump = dump_font_requests,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_font_faces,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = FontFaces,
    encode = encode_font_faces,
    decode = decode_font_faces,
    dump = dump_font_faces,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_save_result,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = SaveResult,
    encode = encode_save_result,
    decode = decode_save_result,
    dump = dump_save_result,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_caret,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = Caret,
    encode = encode_caret,
    decode = decode_caret,
    dump = dump_caret,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_hit,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = Hit,
    encode = encode_hit,
    decode = decode_hit,
    dump = dump_hit,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}

codec_suite! {
    session_sel_rects,
    gen = crate::session_codec::generated,
    support = crate::session_codec::generated_support,
    msg = SelRects,
    encode = encode_sel_rects,
    decode = decode_sel_rects,
    dump = dump_sel_rects,
    ts = "crates/uo-wasm/js/gen/session.ts",
    ts_support = "crates/uo-wasm/js/gen/session.support.ts"
}
