//! The text of the `.doc` ([MS-DOC] 2.4.1, `text.ts`): the piece table (Clx → Pcdt → PlcPcd)
//! maps character positions (CP) to offsets of the `WordDocument` stream (FC). A piece is
//! "compressed" (one byte per character, cp1252 with the special mapping of 0x80-0x9F) or
//! UTF-16LE. The Clx starts with Prcs (grpprls the Prm of a piece refers to after a fast save).

use uo_jsnum::conv::js_index;

use super::bytes::ByteReader;
use super::fib::{Fib, fc};
use super::{DocError, DocLimitKind, DocLimits};

/// A piece of the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub cp_start: u32,
    pub cp_end: u32,
    /// Byte offset in `WordDocument` of the first character. A JS number in the TS: a compressed
    /// piece stores `fcVal / 2`, so an odd `fcVal` gives a fraction (`x.5`), kept as an `f64`
    /// (declaration 1 of D4: the fractional `fc` reads `bytes[1.5]`, undefined, which the text
    /// decoder turns into U+0000).
    pub fc: f64,
    pub compressed: bool,
    /// The Prm (u16): a single sprm (`fComplex` 0) or an index of a Prc (`fComplex` 1).
    pub prm: u16,
}

/// The piece table and the grpprls of the Prcs of the Clx (borrowed from the table stream).
#[derive(Debug, Clone, PartialEq)]
pub struct PieceTable<'a> {
    pub pieces: Vec<Piece>,
    /// the grpprls of the Prcs (index = `igrpprl`)
    pub prcs: Vec<&'a [u8]>,
}

/// The bytes 0x80-0x9F of a compressed piece ([MS-DOC] 2.4.1, the `FcCompressed` table); the
/// other bytes are the character of the same value.
fn cp1252_high(b: u8) -> Option<u16> {
    Some(match b {
        0x82 => 0x201a,
        0x83 => 0x0192,
        0x84 => 0x201e,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02c6,
        0x89 => 0x2030,
        0x8a => 0x0160,
        0x8b => 0x2039,
        0x8c => 0x0152,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201c,
        0x94 => 0x201d,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02dc,
        0x99 => 0x2122,
        0x9a => 0x0161,
        0x9b => 0x203a,
        0x9c => 0x0153,
        0x9f => 0x0178,
        _ => return None,
    })
}

/// `compressedChar`: a byte of a compressed piece as a character (UTF-16 unit).
#[must_use]
pub fn compressed_char(b: u8) -> u16 {
    cp1252_high(b).unwrap_or(u16::from(b))
}

/// `readPieceTable`.
///
/// # Errors
/// [`DocError::NoClx`], [`DocError::NoPcdt`], [`DocError::EmptyPieceTable`].
pub fn read_piece_table<'a>(fib: &Fib, table: &'a [u8]) -> Result<PieceTable<'a>, DocError> {
    let p = fib.fc_lcb(fc::Clx);
    let (fc0, lcb) = (u64::from(p.fc), u64::from(p.lcb));
    let r = ByteReader::new(table);
    if lcb == 0 || fc0 + lcb > r.len() {
        return Err(DocError::NoClx);
    }
    let end = fc0 + lcb;
    let mut off = fc0;
    let mut prcs = Vec::new();
    while off < end && r.u8(off) == 0x01 {
        let cb = u64::from(r.i16(off + 1).max(0).unsigned_abs());
        prcs.push(r.sub(off + 3, off + 3 + cb));
        off += 3 + cb;
    }
    if r.u8(off) != 0x02 {
        return Err(DocError::NoPcdt);
    }
    let lcb_plc = i64::from(r.u32(off + 1));
    let plc = off + 5;
    // Math.floor((lcbPlc - 4) / 12): the dividend may be negative
    let n = (lcb_plc - 4).div_euclid(12);
    if n <= 0 {
        return Err(DocError::EmptyPieceTable);
    }
    let n = n.unsigned_abs();
    let mut pieces = Vec::new();
    let mut i = 0u64;
    // a piece whose first CP is past the end of the table reads zeros: `cpEnd <= cpStart`, skipped;
    // so the walk stops there (the TS walks up to 357 million zero entries)
    while i < n && plc + i * 4 < r.len() {
        let cp_start = r.u32(plc + i * 4);
        let cp_end = r.u32(plc + (i + 1) * 4);
        let pcd = plc + (n + 1) * 4 + i * 8;
        let fc_raw = r.u32(pcd + 2);
        let compressed = fc_raw & 0x4000_0000 != 0;
        let fc_val = fc_raw & 0x3fff_ffff;
        i += 1;
        if cp_end <= cp_start {
            continue;
        }
        pieces.push(Piece {
            cp_start,
            cp_end,
            fc: if compressed { f64::from(fc_val) / 2.0 } else { f64::from(fc_val) },
            compressed,
            prm: r.u16(pcd + 6),
        });
    }
    Ok(PieceTable { pieces, prcs })
}

/// The CPs of the text the reader decodes (the constructor of `DocReader`): the sum of the story
/// counts of the FIB (each clamped at 0), or one past it when the last piece reaches that far.
#[must_use]
pub fn total_cp(fib: &Fib, pt: &PieceTable<'_>) -> u64 {
    let counts = [fib.ccp_text, fib.ccp_ftn, fib.ccp_hdd, fib.ccp_mcr, fib.ccp_atn, fib.ccp_edn, fib.ccp_txbx, fib.ccp_hdr_txbx];
    let at: u64 = counts.iter().map(|&v| u64::from(v.max(0).unsigned_abs())).sum();
    let last_piece_cp = pt.pieces.last().map_or(0, |p| u64::from(p.cp_end));
    at.max(last_piece_cp.min(at + 1))
}

/// `decodeText`: the whole text (every story) as UTF-16 units indexed by CP. Gaps of the piece
/// table are filled with a space, the end with a paragraph mark. Overlapping pieces push their
/// characters again, as the TS does (the text can be longer than `total_cp`).
///
/// # Errors
/// [`DocError::Limit`] when the text would pass `limits.max_text_units` (the TS builds it up to
/// the V8 string limit and then throws a RangeError).
pub fn decode_text(pt: &PieceTable<'_>, word: &[u8], total_cp: u64, limits: &DocLimits) -> Result<Vec<u16>, DocError> {
    let r = ByteReader::new(word);
    let cap = limits.max_text_units;
    let too_long = || DocError::Limit { kind: DocLimitKind::TextUnits, max: cap };
    let mut out: Vec<u16> = Vec::new();
    let mut cp: u64 = 0;
    for p in &pt.pieces {
        let (cp_start, cp_end) = (u64::from(p.cp_start), u64::from(p.cp_end));
        if cp_start >= total_cp {
            break;
        }
        // gaps in the piece table (corrupt): filled with a space
        if cp < cp_start {
            let gap = cp_start - cp;
            if out.len() as u64 + gap > cap {
                return Err(too_long());
            }
            out.resize(out.len() + usize::try_from(gap).map_err(|_| too_long())?, 0x20);
            cp = cp_start;
        }
        let end = cp_end.min(total_cp);
        if end > cp_start {
            let count = end - cp_start;
            if out.len() as u64 + count > cap {
                return Err(too_long());
            }
            out.reserve(usize::try_from(count).map_err(|_| too_long())?);
            if p.compressed {
                // an integer fc reads the bytes in place, a fractional one (an odd compressed fc)
                // reads `bytes[x.5]`, undefined, which `String.fromCharCode` makes U+0000
                match js_index(p.fc, usize::MAX) {
                    Some(base) => {
                        let base = base as u64;
                        for i in 0..count {
                            out.push(compressed_char(r.u8(base + i)));
                        }
                    }
                    None => out.resize(out.len() + usize::try_from(count).map_err(|_| too_long())?, 0),
                }
            } else {
                let base = js_index(p.fc, usize::MAX).map_or(0, |b| b as u64);
                for i in 0..count {
                    out.push(r.u16(base + i * 2));
                }
            }
        }
        cp = cp.max(end);
    }
    if cp < total_cp {
        let rest = total_cp - cp;
        if out.len() as u64 + rest > cap {
            return Err(too_long());
        }
        out.resize(out.len() + usize::try_from(rest).map_err(|_| too_long())?, 0x0d);
    }
    Ok(out)
}

/// `pieceAt`: the index of the piece that contains the CP (binary search), `None` outside.
#[must_use]
pub fn piece_at(pieces: &[Piece], cp: i64) -> Option<usize> {
    let (mut lo, mut hi) = (0i64, pieces.len() as i64 - 1);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let p = &pieces[usize::try_from(mid).ok()?];
        if cp < i64::from(p.cp_start) {
            hi = mid - 1;
        } else if cp >= i64::from(p.cp_end) {
            lo = mid + 1;
        } else {
            return usize::try_from(mid).ok();
        }
    }
    None
}

/// `fcOfCp`: the FC (byte of `WordDocument`) of the character at the CP; `None` is the `-1` of
/// the TS.
#[must_use]
pub fn fc_of_cp(pieces: &[Piece], cp: i64) -> Option<f64> {
    let p = &pieces[piece_at(pieces, cp)?];
    let width = if p.compressed { 1.0 } else { 2.0 };
    Some(p.fc + (cp - i64::from(p.cp_start)) as f64 * width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::fib::read_fib;

    /// A FIB whose Clx pair is (`fc`, `lcb`).
    fn fib_with_clx(fc: u32, lcb: u32) -> Fib {
        let mut w = vec![0u8; 0x200];
        w[0..2].copy_from_slice(&0xa5ecu16.to_le_bytes());
        w[32..34].copy_from_slice(&14u16.to_le_bytes());
        let off = 34 + 28;
        w[off..off + 2].copy_from_slice(&22u16.to_le_bytes());
        let off = off + 2 + 88;
        w[off..off + 2].copy_from_slice(&93u16.to_le_bytes());
        let pair = off + 2 + 33 * 8;
        w[pair..pair + 4].copy_from_slice(&fc.to_le_bytes());
        w[pair + 4..pair + 8].copy_from_slice(&lcb.to_le_bytes());
        read_fib(&w).unwrap()
    }

    /// A Clx: Pcdt with the given (cpStart, cpEnd, fcRaw) pieces.
    fn clx(prcs: &[&[u8]], pieces: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut c = Vec::new();
        for p in prcs {
            c.push(1);
            c.extend((p.len() as i16).to_le_bytes());
            c.extend(*p);
        }
        c.push(2);
        let n = pieces.len() as u32;
        c.extend((4 + 12 * n).to_le_bytes());
        for p in pieces {
            c.extend(p.0.to_le_bytes());
        }
        c.extend(pieces.last().map_or(0, |p| p.1).to_le_bytes());
        for p in pieces {
            c.extend([0, 0]);
            c.extend(p.2.to_le_bytes());
            c.extend([0x34, 0x12]);
        }
        c
    }

    #[test]
    fn reads_pieces_and_prcs() {
        let c = clx(&[&[9, 9, 9]], &[(0, 3, 0x4000_0010), (3, 5, 0x20), (5, 5, 0)]);
        let fib = fib_with_clx(0, c.len() as u32);
        let pt = read_piece_table(&fib, &c).unwrap();
        assert_eq!(pt.prcs, vec![&[9u8, 9, 9][..]]);
        assert_eq!(pt.pieces.len(), 2);
        assert_eq!(pt.pieces[0], Piece { cp_start: 0, cp_end: 3, fc: 8.0, compressed: true, prm: 0x1234 });
        assert_eq!(pt.pieces[1].fc, 32.0);
        assert!(!pt.pieces[1].compressed);
    }

    #[test]
    fn errors_by_class() {
        let fib = fib_with_clx(0, 0);
        assert_eq!(read_piece_table(&fib, &[0; 16]).unwrap_err(), DocError::NoClx);
        let fib = fib_with_clx(10, 16);
        assert_eq!(read_piece_table(&fib, &[0; 16]).unwrap_err(), DocError::NoClx);
        let fib = fib_with_clx(0, 4);
        assert_eq!(read_piece_table(&fib, &[7, 0, 0, 0]).unwrap_err(), DocError::NoPcdt);
        let fib = fib_with_clx(0, 5);
        assert_eq!(read_piece_table(&fib, &[2, 4, 0, 0, 0]).unwrap_err(), DocError::EmptyPieceTable);
    }

    #[test]
    fn decodes_compressed_utf16_gaps_and_the_odd_fc() {
        let mut word = vec![0u8; 64];
        word[16..19].copy_from_slice(&[0x41, 0x93, 0x8d]);
        word[32..36].copy_from_slice(&[0x42, 0, 0x43, 0]);
        let pt = PieceTable {
            pieces: vec![
                Piece { cp_start: 0, cp_end: 3, fc: 16.0, compressed: true, prm: 0 },
                Piece { cp_start: 5, cp_end: 7, fc: 32.0, compressed: false, prm: 0 },
                Piece { cp_start: 7, cp_end: 9, fc: 8.5, compressed: true, prm: 0 },
            ],
            prcs: Vec::new(),
        };
        let t = decode_text(&pt, &word, 11, &DocLimits::DEFAULT).unwrap();
        assert_eq!(t, vec![0x41, 0x201c, 0x8d, 0x20, 0x20, 0x42, 0x43, 0, 0, 0x0d, 0x0d]);
    }

    #[test]
    fn overlapping_pieces_push_again_and_the_limit_stops_them() {
        let word = vec![0x41u8; 16];
        let piece = Piece { cp_start: 0, cp_end: 4, fc: 0.0, compressed: true, prm: 0 };
        let pt = PieceTable { pieces: vec![piece, piece], prcs: Vec::new() };
        assert_eq!(decode_text(&pt, &word, 4, &DocLimits::DEFAULT).unwrap().len(), 8);
        let tight = DocLimits { max_text_units: 7, ..DocLimits::DEFAULT };
        assert_eq!(decode_text(&pt, &word, 4, &tight).unwrap_err(), DocError::Limit { kind: DocLimitKind::TextUnits, max: 7 });
        assert!(decode_text(&PieceTable { pieces: Vec::new(), prcs: Vec::new() }, &word, 1 << 40, &DocLimits::DEFAULT).is_err());
    }

    #[test]
    fn total_cp_is_the_story_counts_or_one_more() {
        let mut w = vec![0u8; 0x200];
        w[0..2].copy_from_slice(&0xa5ecu16.to_le_bytes());
        w[32..34].copy_from_slice(&14u16.to_le_bytes());
        w[62..64].copy_from_slice(&22u16.to_le_bytes());
        for (i, v) in [10i32, 5, -7, 0, 0, 0, 0, 3].iter().enumerate() {
            w[64 + (3 + i) * 4..68 + (3 + i) * 4].copy_from_slice(&v.to_le_bytes());
        }
        let fib = read_fib(&w).unwrap();
        let piece = |end| Piece { cp_start: 0, cp_end: end, fc: 0.0, compressed: true, prm: 0 };
        let pt = |end| PieceTable { pieces: vec![piece(end)], prcs: Vec::new() };
        assert_eq!(total_cp(&fib, &pt(100)), 19);
        assert_eq!(total_cp(&fib, &pt(18)), 18);
        assert_eq!(total_cp(&fib, &PieceTable { pieces: Vec::new(), prcs: Vec::new() }), 18);
    }

    #[test]
    fn binary_search_and_fc_of_cp() {
        let pieces = vec![
            Piece { cp_start: 0, cp_end: 3, fc: 8.0, compressed: true, prm: 0 },
            Piece { cp_start: 3, cp_end: 6, fc: 100.0, compressed: false, prm: 0 },
        ];
        assert_eq!(piece_at(&pieces, 0), Some(0));
        assert_eq!(piece_at(&pieces, 3), Some(1));
        assert_eq!(piece_at(&pieces, 6), None);
        assert_eq!(piece_at(&pieces, -1), None);
        assert_eq!(fc_of_cp(&pieces, 2), Some(10.0));
        assert_eq!(fc_of_cp(&pieces, 4), Some(102.0));
        assert_eq!(fc_of_cp(&pieces, 9), None);
        assert_eq!(compressed_char(0x80), 0x80);
        assert_eq!(compressed_char(0x9f), 0x178);
    }
}
