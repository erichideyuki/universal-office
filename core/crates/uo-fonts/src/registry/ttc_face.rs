//! One face of a `.ttc` as a standalone sfnt, by **face index**: what the system font index
//! loads (`SystemFonts.cs` `TtcFace`, itself the port of `ttcFace` in node-fonts.mjs) and what the
//! registry does for a host that pushes a collection with a face number.
//!
//! This is not `uo_fonts::standalone_face` (which picks by PostScript name and refuses a table
//! outside the file). The rules are those of a Node `Buffer`: a read or write outside the buffer
//! fails, `copy` clamps to the sizes, and the padded length `(len + 3) & ~3` is computed in
//! `int32` (a declared length near 4 GiB wraps). Not in the TS or C#: the output is bounded by
//! [`FontLimits::max_output_bytes`], because the table lengths are declared by the file.

use crate::error::{FontError, LimitKind};
use crate::limits::FontLimits;
use crate::read::View;

/// `(len + 3) & ~3` of the JS (`ToInt32` of the sum: a length from 2^31 - 3 on goes negative).
fn pad4(len: u64) -> i64 {
    let wrapped = (len.wrapping_add(3) & 0xFFFF_FFFF) as u32;
    i64::from((wrapped as i32) & !3)
}

/// `buf.copy(target, targetStart, sourceStart, sourceEnd)` of Node 24 for in-range starts.
fn node_copy(source: &[u8], target: &mut [u8], target_start: u64, source_start: u64, source_end: u64) -> Result<(), FontError> {
    if source_start > source.len() as u64 {
        return Err(FontError::OutOfBounds);
    }
    if target_start >= target.len() as u64 || source_start >= source_end {
        return Ok(());
    }
    let count = (source_end - source_start).min(target.len() as u64 - target_start).min(source.len() as u64 - source_start);
    let (t, s, n) = (target_start as usize, source_start as usize, count as usize);
    let from = source.get(s..s + n).ok_or(FontError::OutOfBounds)?;
    target.get_mut(t..t + n).ok_or(FontError::OutOfBounds)?.copy_from_slice(from);
    Ok(())
}

fn write_u32(target: &mut [u8], value: u64, at: u64) -> Result<(), FontError> {
    let value = u32::try_from(value).map_err(|_| FontError::OutOfBounds)?;
    let at = usize::try_from(at).map_err(|_| FontError::OutOfBounds)?;
    target.get_mut(at..at + 4).ok_or(FontError::OutOfBounds)?.copy_from_slice(&value.to_be_bytes());
    Ok(())
}

/// The face `index` of the collection `buf` as a standalone font: the offset table, the directory
/// with the tables moved right after it (each padded to 4 bytes) and the table bytes. Any read
/// outside `buf` is `OutOfBounds`; a result over `limits.max_output_bytes` is
/// `Limit(OutputBytes)`.
pub fn ttc_face_by_index(buf: &[u8], index: u32, limits: &FontLimits) -> Result<Vec<u8>, FontError> {
    let view = View::new(buf);
    let base = u64::from(view.u32(12 + 4 * u64::from(index))?);
    let num = u64::from(view.u16(base + 4)?);
    let mut records: Vec<(u64, u64, u64)> = Vec::new();
    for i in 0..num {
        let o = base + 12 + i * 16;
        records.push((o, u64::from(view.u32(o + 8)?), u64::from(view.u32(o + 12)?)));
    }
    let header_len = 12 + num * 16;
    let mut total = header_len as i64;
    for &(_, _, len) in &records {
        total += pad4(len);
    }
    if total < 0 {
        return Err(FontError::OutOfBounds);
    }
    if total as u64 > limits.max_output_bytes {
        return Err(FontError::Limit(LimitKind::OutputBytes));
    }
    let mut out = vec![0u8; total as usize];
    node_copy(buf, &mut out, 0, base, base + 12)?;
    let mut pos = header_len;
    for (i, &(o, off, len)) in records.iter().enumerate() {
        let d = 12 + i as u64 * 16;
        node_copy(buf, &mut out, d, o, o + 8)?;
        write_u32(&mut out, pos, d + 8)?;
        write_u32(&mut out, len, d + 12)?;
        node_copy(buf, &mut out, pos, off, off + len)?;
        pos = (pos as i64 + pad4(len)) as u64;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uo_testkit::generate::sfnt::{SfntOptions, SfntTable, build_sfnt, build_ttc};

    #[test]
    fn pad4_wraps_like_int32() {
        assert_eq!(pad4(0), 0);
        assert_eq!(pad4(1), 4);
        assert_eq!(pad4(4), 4);
        assert_eq!(pad4(5), 8);
        assert_eq!(pad4(0x7FFF_FFFC), 0x7FFF_FFFC);
        assert_eq!(pad4(0x7FFF_FFFD), -0x8000_0000);
        assert_eq!(pad4(0xFFFF_FFFF), 0);
    }

    #[test]
    fn extracts_each_face_and_matches_a_standalone_build() {
        let a = vec![SfntTable::new(b"head", vec![1u8; 10]), SfntTable::new(b"name", vec![2u8; 7])];
        let b = vec![SfntTable::new(b"head", vec![3u8; 6]), SfntTable::new(b"cmap", vec![4u8; 9])];
        let ttc = build_ttc(&[(a.clone(), SfntOptions::default()), (b.clone(), SfntOptions::default())], 0x0001_0000);
        let limits = FontLimits::NATIVE;
        for (i, tables) in [a, b].iter().enumerate() {
            let face = ttc_face_by_index(&ttc, i as u32, &limits).unwrap();
            let standalone = build_sfnt(tables, &SfntOptions::default());
            let (fv, sv) = (View::new(&face), View::new(&standalone));
            assert_eq!(fv.u16(4).unwrap(), sv.u16(4).unwrap(), "same table count");
            for (k, t) in tables.iter().enumerate() {
                let r = 12 + k as u64 * 16;
                assert_eq!(fv.tag(r).unwrap().0, t.tag);
                let (o, l) = (fv.u32(r + 8).unwrap() as usize, fv.u32(r + 12).unwrap() as usize);
                assert_eq!(&face[o..o + l], &t.data[..]);
            }
        }
    }

    #[test]
    fn failures_follow_the_buffer_rules() {
        let a = vec![SfntTable::new(b"head", vec![1u8; 10])];
        let ttc = build_ttc(&[(a, SfntOptions::default())], 0x0001_0000);
        let limits = FontLimits::NATIVE;
        assert_eq!(ttc_face_by_index(&ttc, 1, &limits), Err(FontError::OutOfBounds), "face 1 does not exist: offset read at 16");
        assert_eq!(ttc_face_by_index(&ttc[..13], 0, &limits), Err(FontError::OutOfBounds));
        assert_eq!(ttc_face_by_index(&[], 0, &limits), Err(FontError::OutOfBounds));
        // a table that points past the end of the file is clamped by `copy`, not an error
        let mut cut = ttc.clone();
        cut.truncate(cut.len() - 6);
        assert!(ttc_face_by_index(&cut, 0, &limits).is_ok());
        // declared lengths from the file: over the output limit nothing is allocated
        let lying = build_ttc(&[(vec![SfntTable::new(b"head", vec![1u8; 10]).length(0x7000_0000)], SfntOptions::default())], 0x0001_0000);
        let small = FontLimits { max_output_bytes: 64, ..FontLimits::NATIVE };
        assert_eq!(ttc_face_by_index(&lying, 0, &small), Err(FontError::Limit(LimitKind::OutputBytes)));
        // a length that wraps negative in int32 is the JS failure, not an allocation
        let wrapped = build_ttc(&[(vec![SfntTable::new(b"head", vec![1u8; 10]).length(0xFFFF_FFF0)], SfntOptions::default())], 0x0001_0000);
        assert!(ttc_face_by_index(&wrapped, 0, &limits).is_err());
    }
}
