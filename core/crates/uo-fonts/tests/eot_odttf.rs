//! `sfnt_to_eot` and the odttf obfuscation: no-panic loops over mutated fonts and random keys
//! (UTF-16 code units, surrogates included), an independent model of the key parsing, round trips
//! and the memory gate. The comparison with the TS is `subset_parity.rs` (EOT) and
//! `frontier_d_parity.rs` (odttf).
#![allow(clippy::unwrap_used, clippy::panic)]

mod frontier_d_support;

use std::borrow::Cow;

use frontier_d_support::{TestFont, mutate};
use uo_alloc::assert_no_leak;
use uo_fonts::odttf::{deobfuscate_utf16, obfuscate_utf16};
use uo_fonts::{face_info, sfnt_to_eot};
use uo_testkit::generate::Rng;

/// What the TS computes for byte `i` of the key: `parseInt(hex.slice(2i, 2i + 2), 16)` stored in a
/// `Uint8Array`, for a slice of at most two units. Written from the language rules, not from the
/// port: whitespace, an optional `+`, hex digits, anything else stops the number.
fn model_byte(pair: &[u16]) -> u8 {
    let space =
        |u: u16| matches!(u, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF);
    let digit = |u: u16| -> Option<u32> { char::from_u32(u32::from(u)).and_then(|c| c.to_digit(16)).filter(|_| u < 0x80) };
    let mut i = 0;
    while pair.get(i).is_some_and(|&u| space(u)) {
        i += 1;
    }
    if pair.get(i) == Some(&0x2B) {
        i += 1;
    }
    let mut value: Option<u32> = None;
    while let Some(d) = pair.get(i).copied().and_then(digit) {
        value = Some(value.unwrap_or(0) * 16 + d);
        i += 1;
    }
    u8::try_from(value.unwrap_or(0) & 0xFF).unwrap()
}

fn model_key(font_key: &[u16]) -> [u8; 16] {
    let hex: Vec<u16> = font_key.iter().copied().filter(|u| !matches!(u, 0x7B | 0x7D | 0x2D)).collect();
    let mut key = [0u8; 16];
    for (i, slot) in key.iter_mut().enumerate() {
        let from = (2 * i).min(hex.len());
        let to = (2 * i + 2).min(hex.len());
        *slot = model_byte(&hex[from..to]);
    }
    key.reverse();
    key
}

fn random_key(rng: &mut Rng) -> Vec<u16> {
    let pool: Vec<u16> = "0123456789abcdefABCDEF"
        .encode_utf16()
        .chain([0x78, 0x58, 0x2B, 0x2D, 0x7B, 0x7D, 0x20, 0x09, 0xA0, 0xFEFF, 0x67, 0xE9, 0xD800, 0xDC00, 0xD83D, 0xDE00])
        .collect();
    let len = [0usize, 1, 5, 16, 31, 32, 33, 40, 64][rng.below(9)];
    (0..len).map(|_| pool[rng.below(pool.len())]).collect()
}

#[test]
fn the_key_bytes_match_an_independent_model_of_parse_int_over_random_keys() {
    let mut rng = Rng::new(0x0D77_F001);
    let data: Vec<u8> = (0..48u8).collect();
    for _ in 0..30_000 {
        let key = random_key(&mut rng);
        let out = obfuscate_utf16(&data, &key);
        let model = model_key(&key);
        for (i, (&o, &d)) in out.iter().zip(&data).enumerate() {
            let expected = if i < 32 { d ^ model[i % 16] } else { d };
            assert_eq!(o, expected, "unit {i} of key {key:04x?}");
        }
        let hex_len = key.iter().filter(|u| !matches!(u, 0x7B | 0x7D | 0x2D)).count();
        match deobfuscate_utf16(&data, &key) {
            Cow::Borrowed(b) => assert!(hex_len < 32 && b == data.as_slice()),
            Cow::Owned(v) => {
                assert!(hex_len >= 32);
                assert_eq!(v, out);
            }
        }
    }
}

#[test]
fn obfuscating_twice_is_the_identity_and_lengths_never_change() {
    let mut rng = Rng::new(0x0D77_F002);
    for len in 0..80usize {
        let data: Vec<u8> = (0..len).map(|i| (i * 13 + 1) as u8).collect();
        for _ in 0..40 {
            let key = random_key(&mut rng);
            let once = obfuscate_utf16(&data, &key);
            assert_eq!(once.len(), len);
            assert_eq!(obfuscate_utf16(&once, &key), data);
            assert_eq!(&once[len.min(32)..], &data[len.min(32)..]);
        }
    }
}

#[test]
fn the_eot_of_mutated_fonts_never_panics_and_keeps_its_size_fields() {
    let bases = [TestFont::default().build(), TestFont { long_loca: true, italic: true, weight: 700, ..TestFont::default() }.build()];
    let mut rng = Rng::new(0xE07_0001);
    let (mut ok, mut failed) = (0, 0);
    for i in 0..3000usize {
        let base = &bases[i % 2];
        let count = 1 + rng.below(6);
        let font = mutate(base, &mut rng, 0, base.len(), count);
        match sfnt_to_eot(&font, i % 3 == 0) {
            Ok(eot) => {
                ok += 1;
                assert_eq!(u32::from_le_bytes([eot[0], eot[1], eot[2], eot[3]]) as usize, eot.len());
                assert_eq!(u32::from_le_bytes([eot[4], eot[5], eot[6], eot[7]]) as usize, font.len());
                assert_eq!(&eot[eot.len() - font.len()..], font.as_slice());
                assert!(face_info(&font).is_ok());
            }
            Err(_) => {
                failed += 1;
                assert!(face_info(&font).is_err());
            }
        }
    }
    assert!(ok > 1000 && failed > 20, "{ok} ok, {failed} failed");
}

#[test]
fn m1_eot_and_odttf_leak_nothing() {
    let font = TestFont::default().build();
    let units: Vec<u16> = "{00112233-4455-6677-8899-AABBCCDDEEFF}".encode_utf16().collect();
    assert_no_leak(|| sfnt_to_eot(&font, true).unwrap().len());
    assert_no_leak(|| sfnt_to_eot(&font, false).unwrap().len());
    assert_no_leak(|| sfnt_to_eot(&font[..40], false).is_err());
    assert_no_leak(|| sfnt_to_eot(&[], false).is_err());
    assert_no_leak(|| obfuscate_utf16(&font, &units).len());
    assert_no_leak(|| deobfuscate_utf16(&font, &units).len());
    assert_no_leak(|| deobfuscate_utf16(&font, &units[..10]).len());
}
