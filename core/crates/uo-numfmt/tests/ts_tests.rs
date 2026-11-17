//! The assertions of `shared/charts/src/numfmt.test.ts`, transliterated one by one (expected values
//! copied from the TS test, independent of the capture).
// 3.14159 and 3.14159265 are the literals of the TS test, not pi
#![allow(clippy::unwrap_used, clippy::panic, clippy::approx_constant)]

use uo_numfmt::{
    FmtPart, FormatOptions, Value, builtin_format_code, decimal_digits, format_general, format_value, format_value_parts, pt_br,
};

fn u(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn t(v: f64, code: &str) -> String {
    format_value(Value::Num(v), Some(&u(code)), &FormatOptions::default()).text_lossy()
}

fn t1904(v: f64, code: &str, date1904: bool) -> String {
    format_value(Value::Num(v), Some(&u(code)), &FormatOptions { date1904, ..FormatOptions::default() }).text_lossy()
}

fn parts(v: f64, code: &str) -> uo_numfmt::FormatParts {
    format_value_parts(Value::Num(v), Some(&u(code)), &FormatOptions::default())
}

fn text(s: &str) -> FmtPart {
    FmtPart::Text(u(s))
}

fn g(x: f64, budget: i32) -> String {
    format_general(x, pt_br(), budget)
}

#[test]
fn parts_for_those_who_know_the_width() {
    let r = parts(1234.5, "_(* #,##0.00_);_(* (#,##0.00);_(* \"-\"??_);_(@_)");
    assert_eq!(
        r.parts,
        vec![FmtPart::Skip(u16::from(b'(')), FmtPart::Fill(u16::from(b' ')), text("1.234,50"), FmtPart::Skip(u16::from(b')'))]
    );
    assert_eq!(parts(5.0, "*x0*y").parts, vec![text("5"), FmtPart::Fill(u16::from(b'y'))]);
    // zero of the accounting section "-"?? keeps the spaces of the ? (fill without width = once)
    assert_eq!(t(0.0, "_-* #,##0.00_-;-* #,##0.00_-;_-* \"-\"??_-;_-@_-"), "  -   ");
    // General stays a part
    assert_eq!(parts(7.26126896, "General").parts, vec![FmtPart::General(7.26126896)]);
    assert_eq!(parts(5.0, "General_)").parts, vec![FmtPart::General(5.0), FmtPart::Skip(u16::from(b')'))]);
    // no applicable section -> overflow
    assert!(parts(25.0, "[<10]\"a\"0;[<20]\"b\"0").overflow);
    assert!(parts(-1.0, "dd/mm/yyyy").overflow);
    assert!(parts(3_000_000.0, "dd/mm/yyyy").overflow);
}

#[test]
fn show_zeros_off() {
    let no_zeros = FormatOptions { show_zeros: false, ..FormatOptions::default() };
    let p = |v: f64, c: &str| format_value_parts(Value::Num(v), Some(&u(c)), &no_zeros);
    assert!(p(0.0, "General").parts.is_empty());
    assert!(p(0.0, "0.00").parts.is_empty());
    assert_eq!(format_value(Value::Num(0.0), Some(&u("0;-0;\"zero\"")), &no_zeros).text_lossy(), "zero");
    assert_eq!(format_value(Value::Num(-0.0001), Some(&u("0.00")), &no_zeros).text_lossy(), "0,00");
}

#[test]
fn general_with_a_character_budget() {
    assert_eq!(g(7.26126896, 3), "7,3");
    assert_eq!(g(7.26126896, 2), "7");
    assert_eq!(g(1234567.891, 7), "1234568");
    assert_eq!(g(1234567.891, 6), "1E+06");
    assert_eq!(g(12345678901.0, 10), "1,2346E+10");
    assert_eq!(g(0.000123456, 5), "1E-04");
    assert_eq!(g(0.000123456, 6), "0,0001");
    assert_eq!(g(99.999999, 8), "100");
}

#[test]
fn decimal_digits_15_significant_half_away() {
    let d = |x, n| decimal_digits(x, n);
    assert_eq!(d(0.005, 2), (String::new(), "01".to_owned()));
    assert_eq!(d(2.5, 0), ("3".to_owned(), String::new()));
    assert_eq!(d(347.99232604148267, 15), ("347".to_owned(), "992326041483000".to_owned()));
    assert_eq!(d(9.995, 2), ("10".to_owned(), "00".to_owned()));
}

#[test]
fn negative_in_a_literal_only_section_and_elapsed() {
    assert_eq!(t(-5.0, "\"x\""), "-x");
    assert_eq!(t(-0.5, "x"), "-x");
    assert_eq!(t(-5.0, "\"\""), "-");
    assert_eq!(t(-5.0, "-"), "--");
    assert_eq!(t(5.0, "\"x\""), "x");
    assert_eq!(t(-5.0, "\"x\";\"y\""), "y");
    assert_eq!(t(-5.0, "x;"), "");
    assert_eq!(t(-5.0, "[h]"), "-120");
    assert_eq!(t(-0.5, "[mm]"), "-720");
    assert_eq!(t(-1.25, "[s]"), "-108000");
    assert_eq!(t(-1.25, "[h] \"x\""), "-30 x");
    assert!(parts(-1.25, "[h]:mm").overflow);
    assert!(parts(-0.5, "h:mm").overflow);
}

#[test]
fn scientific_and_fractions() {
    let table: &[(&str, f64, &str)] = &[
        ("#,#E+#", 123456.789, "12E+4"),
        ("#,#E+#", 0.0000123456789, "1.235E-8"),
        ("#,#E+#", 0.0, "0.000E+0"),
        ("#,#E-#", 0.0, "0.000E0"),
        ("#,##E+0", 123456.789, "12E+4"),
        ("#,#|E+|#", 123456.789, "12|E|+4"),
        ("0E+\"x\"0", 123456.789, "1Ex+5"),
        ("0E+\"x\"0", 0.0, "0Ex+0"),
        ("# #/#", 0.75, "3/4"),
        ("# #/#", 2.0, "2"),
        ("# #/#", -0.75, "-3/4"),
        ("# 0/#", 0.0, "0/1"),
        ("# 0/#", 2.0, "2 0/1"),
        ("# #?/#", 0.0, "0    "),
        ("# #/0?", 2.0, "2 "),
        ("# #/?0", 2.0, "2     "),
        ("# #/000", 0.75, "3/004"),
        ("# #/?00", 0.6875, "11/106"),
        ("# #/??", 0.75, "3/4 "),
        ("# ?0#/000", 0.0, "0  00/001"),
        ("# ?0#/000", 23.75, "23  03/004"),
        ("#\\:? ?=/=#", 23.75, "2:3 3=/=4"),
        ("#\\:? ?=/=#", 0.75, ":  3=/=4"),
        ("#_#/#", 3.75, "15 /4"),
        ("0/\\x#", 12.5, "25/x2"),
        ("0/\"x\"10", 3.75, "38/x10"),
        ("# ?/?", 0.1, "0    "),
        ("# ?/?", 0.3, " 2/7"),
        ("# ?/??", 3.14159265, "3 1/7 "),
        ("# ?/???", 0.123, " 23/187"),
        ("# ?/???", 0.333, " 1/3  "),
    ];
    for &(f, v, want) in table {
        assert_eq!(t(v, f), want, "{f} with {v}");
    }
}

#[test]
fn elapsed_time_larger_unit_register_and_1904() {
    assert_eq!(t(3.14159, "[h] h"), "75 75");
    assert_eq!(t(3.14159, "[s] ss"), "271433 271433");
    assert_eq!(t(0.123456, "[h] s"), "2 47");
    assert_eq!(t(1.75, "[m] mm:ss"), "2520 01:00");
    assert_eq!(t(3.14159, "s:m\" @ hour \"[hh]"), "53:23 @ hour 1293");
    assert_eq!(t(0.123456, "[s] m"), "47 67821");
    assert_eq!(t(0.123456, "[h]:mm h"), "2:57 1130");
    assert_eq!(t(0.123456, "ss.00 [h]"), "46,60 1448");
    assert!(parts(1.0, "h [m]").overflow);
    assert!(parts(1.0, "[h] [m]").overflow);
    assert_eq!(t(-0.5, "[h] \"x\" h"), "-12 x 12");
    assert!(parts(-0.5, "[h]:mm").overflow);
    assert_eq!(t1904(-0.08546296296296296, "dd/mm/yyyy", true), "-01/01/1904");
    assert_eq!(t1904(-0.08546296296296296, "s [h]", true), "-4 108");
}

#[test]
fn builtin_ids_honour_the_locale() {
    assert_eq!(builtin_format_code(14, pt_br()), Some("dd/mm/yyyy"));
    assert_eq!(builtin_format_code(0, pt_br()), Some("General"));
    assert_eq!(builtin_format_code(18, pt_br()), Some("h:mm AM/PM"));
    assert_eq!(builtin_format_code(99, pt_br()), None);
}

/// The oracle cases quoted in the comments of numfmt.ts (formatGeneral) and a few from the file header.
#[test]
fn documented_examples() {
    assert_eq!(g(0.666_666_666_666_666_6, 11), "0,666666667");
    assert_eq!(g(1234567.89, 11), "1234567,89");
    assert_eq!(g(123_456_789_012.0, 11), "1,23457E+11");
    assert_eq!(g(999_999_999_999.0, 11), "1E+12");
    assert_eq!(g(1e-10, 11), "1E-10");
    assert_eq!(t(62.0, "#\\ ###\\ ##0"), "  62");
    assert_eq!(t(209_496.0, "# ### ##0"), " 209 496");
}
