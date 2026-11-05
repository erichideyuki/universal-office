// The uo-jsnum `Math.*` of V8 13.6 (Node 24.15) as a wasm module, for the Chromium harness of
// the D2b families (G17, record "D2a — session (architecture)": the harness page replaces
// `Math.sin/cos/tan/atan2/pow/hypot/exp/log` with these exports, proven equal to Node 24.15 in the
// uo-jsnum record). Built by `d2b-math-override.mjs` (rustc, no crate of its own):
//
//   rustc --edition 2024 --target wasm32-unknown-unknown --crate-type cdylib -C opt-level=3 \
//     -C panic=abort -L dependency=<deps> --extern uo_jsnum=<libuo_jsnum.rlib> d2b-math-override.rs
use uo_jsnum::math;

macro_rules! export1 {
    ($($name:ident),*) => { $(
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(x: f64) -> f64 { math::$name(x) }
    )* };
}
macro_rules! export2 {
    ($($name:ident),*) => { $(
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(x: f64, y: f64) -> f64 { math::$name(x, y) }
    )* };
}

export1!(sin, cos, tan, exp, log);
export2!(atan2, pow, hypot);

#[unsafe(no_mangle)]
pub extern "C" fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    math::hypot3(a, b, c)
}
