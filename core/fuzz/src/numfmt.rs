//! Fuzz target of `uo-numfmt` (record `core/docs/modules/uo-numfmt.md`): a format code and a value.
//!
//! Input: byte 0 = flags (bit 0 PT_BR passed explicitly, bit 1 date1904, bit 2 showZeros off, bit 3
//! column width given, bit 4 the value is the text of the code itself, bits 5-7 the budget of
//! `format_general`), bytes 1..9 = the value as `f64` bits, the rest = the format code as UTF-16LE
//! code units (lone surrogates reachable). Beyond "no panic" it checks the invariants the TS
//! guarantees: parts and text agree, an overflow gives `"#"`, at most one fill survives, the parse
//! cache does not change a result, and `decimal_digits` returns digits only.

use uo_numfmt::{
    FmtPart, FormatOptions, Value, cache_len, clear_caches, decimal_digits, format_general, format_value, format_value_parts, pt_br,
    serial_to_date_ms, utc_fields,
};

/// Seeds: (flags, value, code).
pub fn numfmt_seeds() -> Vec<Vec<u8>> {
    let mk = |flags: u8, v: f64, code: &str| {
        let mut b = vec![flags];
        b.extend_from_slice(&v.to_bits().to_le_bytes());
        for u in code.encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        b
    };
    let mut out = Vec::new();
    for (code, v) in [
        ("#,##0.00", 1234567.891),
        ("0.00E+00", 0.000123),
        ("# ?/?", 3.4159),
        ("dd/mm/yyyy hh:mm:ss", 43101.123),
        ("[h]:mm:ss.000", -0.5),
        ("[Red][<=100]0.0;[Blue]-0;\"z\"", 5.0),
        ("_-* #,##0.00_-;\\-* #,##0.00_-;_-* \"-\"??_-;_-@_-", 0.0),
        ("[$-F800]", 44927.0),
        ("@", 1.0),
        ("0%", 0.256),
        ("#\\:? ?=/=#", 23.75),
    ] {
        out.push(mk(0, v, code));
        out.push(mk(0b0000_1110, v, code));
    }
    out
}

fn units_of(data: &[u8]) -> Vec<u16> {
    data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

pub fn numfmt_format(data: &[u8]) {
    let (Some(&flags), Some(rest)) = (data.first(), data.get(1..)) else { return };
    let (value_bytes, code_bytes) = rest.split_at(rest.len().min(8));
    let mut vb = [0u8; 8];
    vb[..value_bytes.len()].copy_from_slice(value_bytes);
    let v = f64::from_bits(u64::from_le_bytes(vb));
    let code = units_of(code_bytes);
    let opts = FormatOptions {
        locale: (flags & 1 != 0).then(pt_br),
        date1904: flags & 2 != 0,
        show_zeros: flags & 4 == 0,
        col_width_chars: (flags & 8 != 0).then_some(f64::from(code.len() as u32 % 40)),
    };
    let value = if flags & 16 != 0 { Value::Str(&code) } else { Value::Num(v) };
    let parts = format_value_parts(value, Some(&code), &opts);
    let r = format_value(value, Some(&code), &opts);
    if parts.overflow {
        assert_eq!(r.text, vec![u16::from(b'#')], "an overflow is shown as #");
        assert!(parts.parts.is_empty());
    }
    assert!(parts.parts.iter().filter(|p| matches!(p, FmtPart::Fill(_))).count() <= 1, "only the last fill survives");
    assert!(
        parts.parts.windows(2).all(|w| !matches!(w, [FmtPart::Text(_), FmtPart::Text(_)])) || flags & 16 != 0,
        "adjacent texts are merged"
    );
    assert_eq!(parts.color, r.color);
    // the cache never changes a result
    clear_caches();
    let cold = format_value(value, Some(&code), &opts);
    assert_eq!(cold, r, "cached and cold results agree");
    assert!(cache_len() <= 1);
    // the other entry points
    let n_frac = usize::from(flags >> 5) * 5;
    let (i, f) = decimal_digits(v.abs(), n_frac);
    assert!(i.bytes().chain(f.bytes()).all(|b| b.is_ascii_digit()) && f.len() == n_frac, "digits only, n_frac of them");
    let _ = format_general(v, pt_br(), i32::from(flags >> 5) + 2);
    let t = serial_to_date_ms(v);
    if t.is_finite() {
        assert!(utc_fields(t).is_some());
    }
}
