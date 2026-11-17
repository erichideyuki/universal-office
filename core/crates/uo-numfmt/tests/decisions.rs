//! The decisions of the port that are outside the TS semantics (record, "Decisions"), each pinned by a
//! test: where the TS throws or leaks a JS object, and the guards against allocations the TS refuses.
#![allow(clippy::unwrap_used, clippy::panic)]

use uo_numfmt::{
    FmtPart, FormatLocale, FormatOptions, MAX_FILL_REPS, MAX_GENERAL_BUDGET, Value, format_general, format_value, format_value_parts, pt_br,
};

fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[test]
fn proto_keys_are_not_colours() {
    // TS: `COLOR_NAMES['constructor']` is a function and `['__proto__']` an object (truthy): they come out as the
    // "colour" of the result. The text is the same; Rust has no colour for them.
    for c in ["[constructor]0", "[__proto__]0", "[Constructor]0"] {
        let r = format_value(Value::Num(5.0), Some(&u(c)), &FormatOptions::default());
        assert_eq!(r.text_lossy(), "5");
        assert_eq!(r.color, None);
    }
    let r = format_value(Value::Num(5.0), Some(&u("[Red][constructor]0")), &FormatOptions::default());
    assert_eq!(r.color.as_deref(), Some("#FF0000"));
}

#[test]
fn empty_ampm_name_gives_nothing_where_the_ts_throws() {
    let loc = FormatLocale { ampm: [String::new(), String::new()], ..pt_br().clone() };
    let opts = FormatOptions { locale: Some(&loc), ..FormatOptions::default() };
    let r = format_value(Value::Num(0.5), Some(&u("hh a/p")), &opts);
    assert_eq!(r.text_lossy(), "12 ");
    let r = format_value(Value::Num(0.5), Some(&u("hh AM/PM")), &opts);
    assert_eq!(r.text_lossy(), "12 ");
}

#[test]
fn fill_width_is_bounded() {
    let code = u("0*x");
    let n = |w: f64| {
        format_value(Value::Num(1.0), Some(&code), &FormatOptions { col_width_chars: Some(w), ..FormatOptions::default() }).text.len()
    };
    assert_eq!(n(10.0), 10); // "1" + 9 x
    assert_eq!(n(10.9), 10); // repeat(9.9) = 9
    assert_eq!(n(1.0), 1); // Math.max(0, 1 - 1)
    assert_eq!(n(0.0), 2); // no width: once
    assert_eq!(n(f64::NAN), 2);
    // TS: RangeError (infinite count or past the V8 string limit)
    assert_eq!(n(f64::INFINITY), 1 + MAX_FILL_REPS);
    assert_eq!(n(1e18), 1 + MAX_FILL_REPS);
}

#[test]
fn general_budget_is_clamped_without_changing_the_text() {
    let x = [123456.789, 1e-300, 1.7976931348623157e308, 5e-324, 0.1 + 0.2];
    for v in x {
        assert_eq!(format_general(v, pt_br(), MAX_GENERAL_BUDGET), format_general(v, pt_br(), 400), "{v}");
        assert_eq!(format_general(v, pt_br(), i32::MAX), format_general(v, pt_br(), 500), "{v}");
    }
}

#[test]
fn code_unit_indexing_of_astral_characters() {
    // `_` and `*` take ONE code unit (the high surrogate); the low half is the next literal, and in
    // `format_value` the pair is whole again (the fill is inserted before the low half).
    let parts = format_value_parts(Value::Num(1.0), Some(&u("0_😀")), &FormatOptions::default()).parts;
    assert_eq!(parts, vec![FmtPart::Text(u("1")), FmtPart::Skip(0xD83D), FmtPart::Text(vec![0xDE00])]);
    let r = format_value(Value::Num(1.0), Some(&u("0*😀")), &FormatOptions::default());
    assert_eq!(r.text, u("1😀"));
}
