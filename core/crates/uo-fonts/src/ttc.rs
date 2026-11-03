//! Extracting one face of a `.ttc` collection as a standalone sfnt.

use std::borrow::Cow;

use crate::error::{FontError, LimitKind, Result};
use crate::info::postscript_name;
use crate::limits::FontLimits;
use crate::read::{Tag, View};

/// The bytes of a face as a standalone sfnt: a plain font file comes back as is; of a collection,
/// the face whose PostScript name is `want_postscript` (`Some("")` is `None`), else face 0.
///
/// The TS reads in this order: `numFonts` and the first face offset, the name scan (each face's
/// directory and name records, compared as UTF-16 code units), then the chosen face's directory
/// with the check that every table lies inside the file (`TableOutsideFile`); the output is the
/// offset table (12 bytes, clamped to the file), the directory with the new offsets and the tables
/// padded to 4 bytes. Over the limits, `Limit(TableWork)` for the name scan (R-BUDGET) and
/// `Limit(OutputBytes)` when the output would pass [`FontLimits::max_output_bytes`] (a directory
/// that points every table at the whole file multiplies the input), both before any output is
/// allocated.
pub fn standalone_face<'a>(bytes: &'a [u8], want_postscript: Option<&str>, limits: &FontLimits) -> Result<Cow<'a, [u8]>> {
    let view = View::new(bytes);
    if bytes.len() < 12 || view.tag(0)? != Tag::TTCF {
        return Ok(Cow::Borrowed(bytes));
    }
    let num_fonts = u64::from(view.u32(8)?);
    let mut base = u64::from(view.u32(12)?);
    if let Some(want) = want_postscript.filter(|w| !w.is_empty()) {
        let want: Vec<u16> = want.encode_utf16().collect();
        let mut budget = limits.work();
        for i in 0..num_fonts {
            budget.tick()?;
            let b = u64::from(view.u32(12 + i * 4)?);
            if postscript_name(view, b, &mut budget)?.is_some_and(|name| name == want) {
                base = b;
                break;
            }
        }
    }
    let num = u64::from(view.u16(base + 4)?);
    let mut records: Vec<(u64, u64, u64)> = Vec::new();
    for i in 0..num {
        let o = base + 12 + i * 16;
        let off = u64::from(view.u32(o + 8)?);
        let len = u64::from(view.u32(o + 12)?);
        if off + len > view.len() {
            return Err(FontError::TableOutsideFile);
        }
        records.push((o, off, len));
    }
    let header_len = 12 + num * 16;
    let total = records.iter().fold(header_len, |n, &(_, _, len)| n + padded(len));
    if total > limits.max_output_bytes {
        return Err(FontError::Limit(LimitKind::OutputBytes));
    }
    let total = usize::try_from(total).map_err(|_| FontError::Limit(LimitKind::OutputBytes))?;
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(total).map_err(|_| FontError::Limit(LimitKind::OutputBytes))?;
    let head = view.subarray(base, base + 12);
    out.extend_from_slice(head);
    out.resize(12, 0);
    let mut pos = header_len;
    for &(o, _, len) in &records {
        out.extend_from_slice(view.subarray(o, o + 8));
        out.extend_from_slice(&u32::try_from(pos).map_err(|_| FontError::Limit(LimitKind::OutputBytes))?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(len).map_err(|_| FontError::Limit(LimitKind::OutputBytes))?.to_be_bytes());
        pos += padded(len);
    }
    for &(_, off, len) in &records {
        out.extend_from_slice(view.subarray(off, off + len));
        let pad = padded(len) - len;
        out.resize(out.len() + usize::try_from(pad).map_err(|_| FontError::Limit(LimitKind::OutputBytes))?, 0);
    }
    Ok(Cow::Owned(out))
}

/// `(len + 3) & ~3`.
fn padded(len: u64) -> u64 {
    (len + 3) & !3
}
