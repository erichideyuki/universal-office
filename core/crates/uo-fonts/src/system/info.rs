//! The reading of a font file's `name` and `OS/2` tables for the system index: node-fonts.mjs
//! `infos`/`info`, as `SystemFonts.cs` ported them (the C# is the reference). Everything has the
//! semantics of a Node `Buffer`: a read outside the buffer throws, `subarray` clamps, `swap16`
//! throws on an odd length, and a `catch` gives `[]` for the file or `null` for a face. Here a
//! throw is [`Fail`].

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use uo_jsnum::text::trim;

/// A Node `RangeError` or an I/O error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fail;

/// The faces of a file as the index lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FaceInfo {
    pub(crate) family: String,
    /// the typographic family (name ID 16), when the font has one
    pub(crate) typo: Option<String>,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) weight: u16,
}

fn u16_at(b: &[u8], o: u64) -> Result<u16, Fail> {
    let o = usize::try_from(o).map_err(|_| Fail)?;
    let w = b.get(o..o.checked_add(2).ok_or(Fail)?).ok_or(Fail)?;
    Ok(u16::from_be_bytes([w[0], w[1]]))
}

fn u32_at(b: &[u8], o: u64) -> Result<u32, Fail> {
    let o = usize::try_from(o).map_err(|_| Fail)?;
    let w = b.get(o..o.checked_add(4).ok_or(Fail)?).ok_or(Fail)?;
    Ok(u32::from_be_bytes([w[0], w[1], w[2], w[3]]))
}

/// `buf.toString('latin1', start, end)` as bytes (clamped).
fn latin1_slice(b: &[u8], start: usize, end: usize) -> &[u8] {
    let end = end.min(b.len());
    if start >= end { &[] } else { &b[start..end] }
}

/// `readSync(fd, Buffer.alloc(len), 0, len, pos)` and `subarray(0, n)`: what could be read.
fn read_at(file: &mut File, pos: u64, len: usize) -> Result<Vec<u8>, Fail> {
    file.seek(SeekFrom::Start(pos)).map_err(|_| Fail)?;
    let mut out = vec![0u8; len];
    let mut n = 0;
    while n < len {
        let got = file.read(&mut out[n..]).map_err(|_| Fail)?;
        if got == 0 {
            break;
        }
        n += got;
    }
    out.truncate(n);
    Ok(out)
}

fn ascii_ci_at(s: &[u8], at: usize, lower: &[u8]) -> bool {
    s.get(at..at + lower.len()).is_some_and(|w| w.iter().zip(lower).all(|(c, l)| c.to_ascii_lowercase() == *l))
}

/// `/needle/i.test(s)` for an ASCII lower-case needle.
fn contains_ascii_ci(s: &[u8], lower: &[u8]) -> bool {
    (0..=s.len().saturating_sub(lower.len())).any(|i| ascii_ci_at(s, i, lower))
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `/\bword\b/i.test(s)` for an ASCII lower-case word of word characters.
fn contains_word_ascii_ci(s: &[u8], lower: &[u8]) -> bool {
    (0..=s.len().saturating_sub(lower.len()))
        .any(|i| ascii_ci_at(s, i, lower) && (i == 0 || !is_word(s[i - 1])) && s.get(i + lower.len()).is_none_or(|n| !is_word(*n)))
}

/// The faces of a file: one for a `.ttf`/`.otf`, all (at most 64) of a `.ttc`. Any failure outside
/// a face's own reading discards the whole file (`[]`), the faces read before it included.
pub(crate) fn infos(path: &Path) -> Vec<(FaceInfo, u32)> {
    infos_inner(path).unwrap_or_default()
}

fn infos_inner(path: &Path) -> Result<Vec<(FaceInfo, u32)>, Fail> {
    let mut file = File::open(path).map_err(|_| Fail)?;
    let head = read_at(&mut file, 0, 16)?;
    if head.len() < 12 {
        return Ok(Vec::new());
    }
    if latin1_slice(&head, 0, 4) != b"ttcf" {
        return Ok(info(&mut file, 0).map(|i| vec![(i, 0)]).unwrap_or_default());
    }
    let n = u64::from(u32_at(&head, 8)?).min(64);
    let offs = read_at(&mut file, 12, (n * 4) as usize)?;
    let mut out = Vec::new();
    for k in 0..n {
        let base = u64::from(u32_at(&offs, k * 4)?);
        if let Some(i) = info(&mut file, base) {
            out.push((i, k as u32));
        }
    }
    Ok(out)
}

fn info(file: &mut File, base: u64) -> Option<FaceInfo> {
    info_inner(file, base).ok().flatten()
}

fn info_inner(file: &mut File, base: u64) -> Result<Option<FaceInfo>, Fail> {
    let off = read_at(file, base, 12)?;
    let num = u64::from(u16_at(&off, 4)?);
    if num == 0 || num > 512 {
        return Ok(None);
    }
    let dir = read_at(file, base + 12, (num * 16) as usize)?;
    let mut tables: HashMap<[u8; 4], (u64, u64)> = HashMap::new();
    for i in 0..num {
        let at = (i * 16) as usize;
        let tag_bytes = latin1_slice(&dir, at, at + 4);
        let (off, len) = (u64::from(u32_at(&dir, i * 16 + 8)?), u64::from(u32_at(&dir, i * 16 + 12)?));
        let mut tag = [0u8; 4];
        tag[..tag_bytes.len()].copy_from_slice(tag_bytes);
        tables.insert(tag, (off, len));
    }
    let Some(&(name_off, name_len)) = tables.get(b"name") else { return Ok(None) };
    let name = read_at(file, name_off, name_len.min(1 << 20) as usize)?;
    let count = u64::from(u16_at(&name, 2)?);
    let strings = u64::from(u16_at(&name, 4)?);

    let get = |id: u16| -> Result<Option<String>, Fail> {
        let mut any: Option<String> = None;
        for i in 0..count {
            let r = 6 + i * 12;
            if u16_at(&name, r)? != 3 || u16_at(&name, r + 6)? != id {
                continue;
            }
            let len = u64::from(u16_at(&name, r + 8)?);
            let o = strings + u64::from(u16_at(&name, r + 10)?);
            let s0 = o.min(name.len() as u64);
            let s1 = (o + len).min(name.len() as u64);
            let cnt = s1.saturating_sub(s0) as usize;
            if !cnt.is_multiple_of(2) {
                return Err(Fail);
            }
            let units: Vec<u16> = (0..cnt / 2)
                .map(|k| {
                    let at = s0 as usize + 2 * k;
                    u16::from_be_bytes([name[at], name[at + 1]])
                })
                .collect();
            let s = String::from_utf16_lossy(&units);
            if u16_at(&name, r + 4)? == 0x409 {
                return Ok(Some(s));
            }
            any.get_or_insert(s);
        }
        Ok(any)
    };

    let Some(family) = get(1)?.map(|f| trim(&f).to_owned()) else { return Ok(None) };
    if family.is_empty() || family.starts_with('@') {
        return Ok(None);
    }
    let sub = get(2)?.unwrap_or_else(|| "Regular".to_owned());
    let mut bold = contains_word_ascii_ci(sub.as_bytes(), b"bold");
    let mut italic = contains_ascii_ci(sub.as_bytes(), b"italic") || contains_ascii_ci(sub.as_bytes(), b"oblique");
    let mut weight: u16 = 400;
    if let Some(&(os2_off, _)) = tables.get(b"OS/2") {
        let os2 = read_at(file, os2_off, 64)?;
        if os2.len() >= 64 {
            let fs = u16_at(&os2, 62)?;
            let w = u16_at(&os2, 4)?;
            weight = if w != 0 { w } else { 400 };
            italic = (fs & 1) == 1 || italic;
            bold = (fs & 0x20) != 0 || bold;
        }
    }
    let typo = get(16)?.map(|t| trim(&t).to_owned());
    Ok(Some(FaceInfo { family, typo, bold, italic, weight }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_regexes_of_the_js() {
        assert!(contains_word_ascii_ci(b"Bold Italic", b"bold"));
        assert!(!contains_word_ascii_ci(b"SemiBold", b"bold"), "no word boundary inside SemiBold");
        assert!(contains_word_ascii_ci(b"x bold-y", b"bold"));
        assert!(!contains_word_ascii_ci(b"Bolder", b"bold"));
        assert!(!contains_word_ascii_ci(b"bold_x", b"bold"), "underscore is a word character");
        assert!(contains_word_ascii_ci("\u{e9}bold".as_bytes(), b"bold"), "a non-ASCII neighbour is not a word character");
        assert!(contains_ascii_ci(b"Semi-ITALIC", b"italic"));
        assert!(contains_ascii_ci(b"Oblique", b"oblique"));
        assert!(!contains_ascii_ci(b"ital", b"italic"));
        assert!(!contains_ascii_ci(b"", b"italic"));
    }
}
