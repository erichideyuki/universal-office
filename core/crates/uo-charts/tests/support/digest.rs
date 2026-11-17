//! The digests of the capture facets, as the JS side writes them: SHA-1 of the op-stream text (`lines:sha1[0..16]`, the
//! `draw` form of `opstream.mjs`), SHA-256 prefixes of texts and of sorted key sets, and the JS string length (UTF-16 units).

use uo_testkit::sha256::sha256;

/// SHA-1 (RFC 3174), for the op-stream digests (the JS recorder uses `createHash('sha1')`).
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&(u64::try_from(data.len()).unwrap_or(0).wrapping_mul(8)).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, c) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The JS `string.length`: UTF-16 code units.
pub fn js_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `lines:sha1[0..16]` of a draw-form stream (the lines, each followed by `\n`).
pub fn lines_digest(lines: &[String]) -> String {
    let mut text = String::new();
    for l in lines {
        text.push_str(l);
        text.push('\n');
    }
    format!("{}:{}", lines.len(), &hex(&sha1(text.as_bytes()))[..16])
}

/// `len:sha256[0..32]` of a text (`textDigest` of `canon-json.mjs`; the length is in UTF-8 bytes).
pub fn text_digest(text: &str) -> String {
    format!("{}:{}", text.len(), &hex(&sha256(text.as_bytes()))[..32])
}

/// `len:sha256[0..32]` of the XML text as the `input` facet writes it (the length is the JS `.length`).
pub fn input_digest(xml: &str) -> String {
    format!("{}:{}", js_len(xml), &hex(&sha256(xml.as_bytes()))[..32])
}

/// `n:sha256[0..16]` of the sorted distinct keys (the `ask.*` facets): sorted by UTF-16 code units.
pub fn set_digest<I: IntoIterator<Item = String>>(keys: I) -> String {
    let mut k: Vec<String> = keys.into_iter().collect();
    k.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    format!("{}:{}", k.len(), &hex(&sha256(k.join("\n").as_bytes()))[..16])
}

/// `n:sha256[0..16]` of the lines joined by `\n`, in order (the `host`, `hostx` and `lay.*` facets).
pub fn ordered_digest(lines: &[String]) -> String {
    format!("{}:{}", lines.len(), &hex(&sha256(lines.join("\n").as_bytes()))[..16])
}
