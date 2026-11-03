//! The `.odttf` obfuscation of embedded fonts (ECMA-376 17.8.1): the first 32 bytes XORed with the
//! reversed GUID of `w:fontKey`. Keys are processed as UTF-16 code units, as the TS does with its
//! strings (R-ODTTF): the length check, the two-unit slices and `parseInt` all count code units,
//! so a non-ASCII or astral key gives what the TS gives and never panics on a char boundary.
//!
//! Two rules are kept, as in the TS: [`obfuscate`] (the writer of `ooxml.ts`) does not check the
//! length of the key, [`deobfuscate`] (docx-core `deobfuscateFont`) returns the data unchanged when
//! fewer than 32 units remain after removing `{`, `}` and `-`.

use std::borrow::Cow;

use uo_jsnum::conv::to_uint8;
use uo_jsnum::parse::parse_int_utf16;

/// `fontKey.replace(/[{}-]/g, '')`, on code units.
fn hex_units(font_key: &[u16]) -> Vec<u16> {
    font_key.iter().copied().filter(|&u| !matches!(u, 0x7B | 0x7D | 0x2D)).collect()
}

/// The 16-byte key: byte `i` is `parseInt(hex.slice(2i, 2i + 2), 16)` stored in a `Uint8Array`
/// (`NaN` is 0, `-1` is 255, a slice cut short by the end of the key parses what it holds), then
/// reversed.
fn key_of(hex: &[u16]) -> [u8; 16] {
    let mut key = [0u8; 16];
    for (i, slot) in key.iter_mut().enumerate() {
        let start = (i * 2).min(hex.len());
        let end = (i * 2 + 2).min(hex.len());
        let pair = hex.get(start..end).unwrap_or(&[]);
        *slot = to_uint8(parse_int_utf16(pair, 16));
    }
    key.reverse();
    key
}

/// A copy of the data with its first `min(32, len)` bytes XORed with the key, cycling.
fn apply(data: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let mut out = data.to_vec();
    for (i, byte) in out.iter_mut().take(32).enumerate() {
        *byte ^= key[i % 16];
    }
    out
}

/// The data with its first 32 bytes obfuscated by the key (no length check on the key: as the
/// TS writer does). The operation is its own inverse.
pub fn obfuscate(data: &[u8], font_key: &str) -> Vec<u8> {
    let units: Vec<u16> = font_key.encode_utf16().collect();
    obfuscate_utf16(data, &units)
}

/// The data de-obfuscated; unchanged (borrowed) when fewer than 32 units of key remain after
/// removing `{`, `}`, `-`.
pub fn deobfuscate<'a>(data: &'a [u8], font_key: &str) -> Cow<'a, [u8]> {
    let units: Vec<u16> = font_key.encode_utf16().collect();
    deobfuscate_utf16(data, &units)
}

/// [`obfuscate`] with the key as UTF-16 code units (a JS string may hold lone surrogates).
pub fn obfuscate_utf16(data: &[u8], font_key: &[u16]) -> Vec<u8> {
    let hex = hex_units(font_key);
    apply(data, &key_of(&hex))
}

/// [`deobfuscate`] with the key as UTF-16 code units.
pub fn deobfuscate_utf16<'a>(data: &'a [u8], font_key: &[u16]) -> Cow<'a, [u8]> {
    let hex = hex_units(font_key);
    if hex.len() < 32 {
        return Cow::Borrowed(data);
    }
    Cow::Owned(apply(data, &key_of(&hex)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_alloc::assert_no_leak;

    const KEY: &str = "{00112233-4455-6677-8899-AABBCCDDEEFF}";

    fn sample(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + 3) as u8).collect()
    }

    #[test]
    fn the_key_is_the_reversed_guid_bytes() {
        let units: Vec<u16> = "00112233445566778899AABBCCDDEEFF".encode_utf16().collect();
        let key = key_of(&units);
        assert_eq!(key, [0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00]);
    }

    #[test]
    fn xor_of_the_first_32_bytes_only_and_an_involution() {
        let data = sample(100);
        let out = obfuscate(&data, KEY);
        assert_eq!(out.len(), 100);
        assert_eq!(&out[32..], &data[32..]);
        assert_ne!(&out[..32], &data[..32]);
        assert_eq!(out[0], data[0] ^ 0xFF);
        assert_eq!(out[16], data[16] ^ 0xFF);
        assert_eq!(out[31], data[31], "the last key byte is zero");
        assert_eq!(deobfuscate(&out, KEY).as_ref(), data.as_slice());
        assert_eq!(obfuscate(&out, KEY), data);
    }

    #[test]
    fn short_data_is_xored_over_its_length() {
        let data = sample(5);
        let out = obfuscate(&data, KEY);
        assert_eq!(out, data.iter().zip([0xFFu8, 0xEE, 0xDD, 0xCC, 0xBB]).map(|(a, b)| a ^ b).collect::<Vec<_>>());
        assert!(obfuscate(&[], KEY).is_empty());
        assert!(deobfuscate(&[], KEY).is_empty());
    }

    #[test]
    fn deobfuscate_returns_the_input_when_fewer_than_32_units_remain() {
        let data = sample(64);
        for key in ["", "{}", "{00112233-4455-6677-8899-AABBCCDDEEF}", "0011223344556677889900112233445"] {
            assert!(matches!(deobfuscate(&data, key), Cow::Borrowed(b) if b == data.as_slice()), "{key}");
        }
        assert!(matches!(deobfuscate(&data, KEY), Cow::Owned(_)));
        assert!(matches!(deobfuscate(&data, "00112233445566778899AABBCCDDEEFF"), Cow::Owned(_)));
    }

    #[test]
    fn obfuscate_does_not_check_the_length_of_the_key() {
        let data = sample(40);
        let out = obfuscate(&data, "AABB");
        let mut key = [0u8; 16];
        key[15] = 0xAA;
        key[14] = 0xBB;
        let expected: Vec<u8> = data.iter().enumerate().map(|(i, b)| if i < 32 { b ^ key[i % 16] } else { *b }).collect();
        assert_eq!(out, expected);
        assert_eq!(obfuscate(&data, ""), data);
    }

    #[test]
    fn keys_parse_as_js_parse_int_does() {
        let data = vec![0u8; 32];
        let stored = |key: &str| -> Vec<u8> {
            let mut k = obfuscate(&data, key);
            k.truncate(16);
            k.reverse();
            k
        };
        let rest = "0".repeat(28);
        assert_eq!(stored(&format!("1g{}", "0".repeat(30)))[0], 1);
        assert_eq!(stored(&format!("{{-1}}{rest}"))[0], 16, "the dash is removed before parsing, so a sign never reaches parseInt");
        assert_eq!(stored(&format!("0x{rest}"))[0], 0);
        assert_eq!(stored(&format!("zz{}", "0".repeat(30)))[0], 0);
        assert_eq!(stored(&format!(" 5{rest}"))[0], 5);
        assert_eq!(stored(&format!("+f{rest}"))[0], 15);
        assert_eq!(stored(&format!("ff{}", "0".repeat(30)))[0], 255);
    }

    #[test]
    fn non_ascii_and_astral_keys_count_utf16_units() {
        let data = sample(40);
        let key = format!("{{\u{e9}\u{1F600}{}", "A".repeat(40));
        let out = obfuscate(&data, &key);
        assert_eq!(out.len(), 40);
        let units: Vec<u16> = key.encode_utf16().collect();
        assert_eq!(obfuscate_utf16(&data, &units), out);
        assert_eq!(deobfuscate(&out, &key).as_ref(), data.as_slice());
        let short = "\u{1F600}".repeat(15);
        assert!(matches!(deobfuscate(&data, &short), Cow::Borrowed(_)));
        assert!(matches!(deobfuscate(&data, &"\u{1F600}".repeat(16)), Cow::Owned(_)));
    }

    #[test]
    fn a_surrogate_in_a_slice_parses_nothing_and_gives_zero() {
        let mut units: Vec<u16> = vec![0xD800];
        units.extend("A".encode_utf16().cycle().take(40));
        let data = vec![0u8; 32];
        let out = obfuscate_utf16(&data, &units);
        let mut key = out[..16].to_vec();
        key.reverse();
        assert_eq!(key[0], 0);
        assert_eq!(key[1], 0xAA);
        assert!(matches!(deobfuscate_utf16(&data, &units), Cow::Owned(_)));
    }

    #[test]
    fn m1_no_leak() {
        let data = sample(100);
        let units: Vec<u16> = KEY.encode_utf16().collect();
        assert_no_leak(|| obfuscate(&data, KEY));
        assert_no_leak(|| deobfuscate(&data, KEY).len());
        assert_no_leak(|| deobfuscate(&data, "short").len());
        assert_no_leak(|| obfuscate_utf16(&data, &units));
        assert_no_leak(|| deobfuscate_utf16(&data, &units).len());
        assert_no_leak(|| obfuscate(&[], ""));
    }
}
