//! Image helpers of the paste (D2b-F8): `imageSize` (universal-write.ts:703) and the `atob` of
//! `decodeDataUrl` (paste-html.ts:127). `atob` is the WHATWG "forgiving-base64 decode"; the only
//! other base64 decoder of the crate (`odf::reader::decode_base64`, feature `odf`) is a lenient
//! reader of ODF data, not this algorithm, so this is not a copy of it (record F8.md).

/// `imageSize(b)`: width × height (px) of a PNG, GIF or JPEG from the header bytes.
#[must_use]
pub fn image_size(b: &[u8]) -> Option<(f64, f64)> {
    let at = |i: usize| -> u32 { u32::from(b.get(i).copied().unwrap_or(0)) };
    if b.len() > 24 && at(0) == 0x89 && at(1) == 0x50 {
        // `(b16 << 24 | …) >>> 0`: the bits as an unsigned 32-bit number
        let w = (at(16) << 24) | (at(17) << 16) | (at(18) << 8) | at(19);
        let h = (at(20) << 24) | (at(21) << 16) | (at(22) << 8) | at(23);
        return Some((f64::from(w), f64::from(h)));
    }
    if b.len() > 10 && at(0) == 0x47 && at(1) == 0x49 {
        return Some((f64::from(at(6) | (at(7) << 8)), f64::from(at(8) | (at(9) << 8))));
    }
    if b.first() == Some(&0xff) && b.get(1) == Some(&0xd8) {
        let mut i = 2usize;
        while i + 9 < b.len() {
            if at(i) != 0xff {
                i += 1;
                continue;
            }
            let m = at(i + 1);
            let len = (at(i + 2) << 8) | at(i + 3);
            if (0xc0..=0xcf).contains(&m) && m != 0xc4 && m != 0xc8 && m != 0xcc {
                return Some((f64::from((at(i + 7) << 8) | at(i + 8)), f64::from((at(i + 5) << 8) | at(i + 6))));
            }
            i += 2 + len as usize;
        }
    }
    None
}

fn sextet(u: u16) -> Option<u32> {
    let c = u8::try_from(u).ok()?;
    Some(u32::from(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    }))
}

/// `atob(s)` (forgiving-base64 decode, after the white space was removed): `None` where `atob`
/// throws.
#[must_use]
pub fn atob(s: &[u16]) -> Option<Vec<u8>> {
    let ascii_ws = |u: &u16| matches!(*u, 9 | 10 | 12 | 13 | 32);
    let data: Vec<u16> = s.iter().copied().filter(|u| !ascii_ws(u)).collect();
    let mut n = data.len();
    let eq = u16::from(b'=');
    if n.is_multiple_of(4) {
        if n >= 2 && data.get(n - 1) == Some(&eq) && data.get(n - 2) == Some(&eq) {
            n -= 2;
        } else if n >= 1 && data.get(n - 1) == Some(&eq) {
            n -= 1;
        }
    }
    if n % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(n * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for u in data.get(..n).unwrap_or(&[]) {
        buf = (buf << 6) | sextet(*u)?;
        bits += 6;
        if bits == 24 {
            out.extend_from_slice(&[(buf >> 16) as u8, (buf >> 8) as u8, buf as u8]);
            buf = 0;
            bits = 0;
        }
    }
    match bits {
        12 => out.push((buf >> 4) as u8),
        18 => out.extend_from_slice(&[(buf >> 10) as u8, (buf >> 2) as u8]),
        _ => {}
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn atob_rules() {
        assert_eq!(atob(&u("aGk=")), Some(b"hi".to_vec()));
        assert_eq!(atob(&u("aGk")), Some(b"hi".to_vec()));
        assert_eq!(atob(&u("a")), None);
        assert_eq!(atob(&u("a=b=")), None);
        assert_eq!(atob(&u("")), Some(Vec::new()));
    }
}
