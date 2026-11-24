//! BIFF records (`biff.ts`, [MS-XLS] 2.1.4): a record is a 4-byte header (type, length) and its data; the CONTINUE
//! records that follow are joined, remembering where each began (SST/TXO strings re-read the options byte at each
//! boundary). Bounded little-endian reads, and the string readers of the format.

use std::borrow::Cow;

/// Record types used by the converter ([MS-XLS] 2.3).
pub mod rt {
    pub const FORMULA: u16 = 0x0006;
    pub const EOF: u16 = 0x000a;
    pub const CALCMODE: u16 = 0x000d;
    pub const NOTE: u16 = 0x001c;
    pub const EXTERNSHEET: u16 = 0x0017;
    pub const NAME: u16 = 0x0018;
    pub const SHRFMLA: u16 = 0x04bc;
    pub const ARRAY: u16 = 0x0221;
    pub const CONDFMT: u16 = 0x01b0;
    pub const CF: u16 = 0x01b1;
    pub const CONDFMT12: u16 = 0x0879;
    pub const CF12: u16 = 0x087a;
    pub const CFEX: u16 = 0x087b;
    pub const DV: u16 = 0x01be;
    pub const AUTOFILTERINFO: u16 = 0x009d;
    pub const SUPBOOK: u16 = 0x01ae;
    pub const AI: u16 = 0x1051;
    pub const DATEMODE: u16 = 0x0022;
    pub const FILEPASS: u16 = 0x002f;
    pub const FONT: u16 = 0x0031;
    pub const CONTINUE: u16 = 0x003c;
    pub const WINDOW1: u16 = 0x003d;
    pub const PANE: u16 = 0x0041;
    pub const CODEPAGE: u16 = 0x0042;
    pub const DEFCOLWIDTH: u16 = 0x0055;
    pub const OBJ: u16 = 0x005d;
    pub const COLINFO: u16 = 0x007d;
    pub const BOUNDSHEET: u16 = 0x0085;
    pub const PALETTE: u16 = 0x0092;
    pub const STANDARDWIDTH: u16 = 0x0099;
    pub const SCL: u16 = 0x00a0;
    pub const MULRK: u16 = 0x00bd;
    pub const MULBLANK: u16 = 0x00be;
    pub const RSTRING: u16 = 0x00d6;
    pub const XF: u16 = 0x00e0;
    pub const MERGECELLS: u16 = 0x00e5;
    pub const MSODRAWINGGROUP: u16 = 0x00eb;
    pub const MSODRAWING: u16 = 0x00ec;
    pub const LABELSST: u16 = 0x00fd;
    pub const SST: u16 = 0x00fc;
    pub const TXO: u16 = 0x01b6;
    pub const HLINK: u16 = 0x01b8;
    pub const BLANK: u16 = 0x0201;
    pub const NUMBER: u16 = 0x0203;
    pub const LABEL: u16 = 0x0204;
    pub const BOOLERR: u16 = 0x0205;
    pub const STRING: u16 = 0x0207;
    pub const ROW: u16 = 0x0208;
    pub const DEFAULTROWHEIGHT: u16 = 0x0225;
    pub const WINDOW2: u16 = 0x023e;
    pub const RK: u16 = 0x027e;
    pub const FORMAT: u16 = 0x041e;
    pub const HLINKTOOLTIP: u16 = 0x0800;
    pub const BOF: u16 = 0x0809;
    pub const XFEXT: u16 = 0x087d;
    pub const THEME: u16 = 0x0896;
    pub const EXTERNNAME: u16 = 0x0023;
}

/// BIFF2/3/4 BOFs (`OLD_BOFS`): not supported.
pub const OLD_BOFS: [u16; 3] = [0x0009, 0x0209, 0x0409];

/// Most bytes joined from CONTINUE records into one (`MAX_MERGED`: a hostile SST "without end").
const MAX_MERGED: usize = 64 * 1024 * 1024;

/// A BIFF record with the data of its CONTINUE records appended.
#[derive(Debug)]
pub struct Rec<'a> {
    pub ty: u16,
    /// Offset of the header in the stream.
    pub pos: usize,
    pub data: Cow<'a, [u8]>,
    /// Offsets (in `data`) where each joined CONTINUE begins; empty when there is none.
    pub conts: Vec<usize>,
}

impl Rec<'_> {
    pub fn rd(&self) -> Rd<'_> {
        Rd(&self.data)
    }
}

fn le16(b: &[u8], o: usize) -> usize {
    match (b.get(o), b.get(o + 1)) {
        (Some(&l), Some(&h)) => usize::from(l) | (usize::from(h) << 8),
        _ => 0,
    }
}

/// The records of `[start, end)` of the stream (`records`): stops at a truncated header; a record that does not fit
/// is cut at the end of the stream.
pub struct Records<'a> {
    buf: &'a [u8],
    off: usize,
    lim: usize,
}

impl<'a> Records<'a> {
    pub fn new(buf: &'a [u8], start: usize, end: usize) -> Self {
        Records { buf, off: start, lim: end.min(buf.len()) }
    }

    /// The offset of the next record's header (just past the last one returned).
    pub fn offset(&self) -> usize {
        self.off
    }
}

impl<'a> Iterator for Records<'a> {
    type Item = Rec<'a>;

    fn next(&mut self) -> Option<Rec<'a>> {
        let (buf, lim, off) = (self.buf, self.lim, self.off);
        if off.checked_add(4)? > lim {
            return None;
        }
        let ty = u16::try_from(le16(buf, off)).unwrap_or(0);
        let len = le16(buf, off + 2);
        let d_start = off + 4;
        let d_end = lim.min(d_start + len);
        let after = d_start + len;
        let mut next = after;
        let mut conts: Vec<usize> = Vec::new();
        let mut total = d_end - d_start;
        while next + 4 <= lim && le16(buf, next) == usize::from(rt::CONTINUE) && ty != rt::CONTINUE {
            let cl = le16(buf, next + 2);
            conts.push(next);
            total += cl.min(lim - next - 4);
            next += 4 + cl;
            if total > MAX_MERGED {
                break;
            }
        }
        if conts.is_empty() {
            self.off = after;
            return Some(Rec { ty, pos: off, data: Cow::Borrowed(buf.get(d_start..d_end).unwrap_or(&[])), conts });
        }
        let mut data = vec![0u8; total];
        let first = d_end - d_start;
        if let (Some(dst), Some(src)) = (data.get_mut(..first), buf.get(d_start..d_end)) {
            dst.copy_from_slice(src);
        }
        let mut w = first;
        let mut marks = Vec::with_capacity(conts.len());
        for &c in &conts {
            let cl = le16(buf, c + 2);
            let s = c + 4;
            let e = lim.min(s + cl).min(s + (total - w));
            marks.push(w);
            if let (Some(dst), Some(src)) = (data.get_mut(w..w + (e - s)), buf.get(s..e)) {
                dst.copy_from_slice(src);
            }
            w += e - s;
        }
        self.off = next;
        if w != total {
            data.truncate(w);
        }
        Some(Rec { ty, pos: off, data: Cow::Owned(data), conts: marks })
    }
}

/// Little-endian reader with bounds over the data of a record (`Bytes`): out of range reads 0.
#[derive(Clone, Copy, Debug)]
pub struct Rd<'a>(pub &'a [u8]);

#[allow(clippy::len_without_is_empty)]
impl<'a> Rd<'a> {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn u8(&self, o: usize) -> u8 {
        self.0.get(o).copied().unwrap_or(0)
    }

    fn arr<const N: usize>(&self, o: usize) -> Option<[u8; N]> {
        let end = o.checked_add(N)?;
        <[u8; N]>::try_from(self.0.get(o..end)?).ok()
    }

    pub fn u16(&self, o: usize) -> u16 {
        self.arr::<2>(o).map_or(0, u16::from_le_bytes)
    }

    pub fn i16(&self, o: usize) -> i16 {
        self.arr::<2>(o).map_or(0, i16::from_le_bytes)
    }

    pub fn u32(&self, o: usize) -> u32 {
        self.arr::<4>(o).map_or(0, u32::from_le_bytes)
    }

    pub fn i32(&self, o: usize) -> i32 {
        self.arr::<4>(o).map_or(0, i32::from_le_bytes)
    }

    pub fn f64(&self, o: usize) -> f64 {
        self.arr::<8>(o).map_or(0.0, f64::from_le_bytes)
    }

    /// `sub(s, e)`: the bytes `[s, e)` clamped to the data.
    pub fn sub(&self, s: usize, e: usize) -> &'a [u8] {
        let e = e.min(self.0.len());
        self.0.get(s.min(e)..e).unwrap_or(&[])
    }
}

// ───────────────────────────── strings ─────────────────────────────

/// Windows-1252 0x80–0x9F (the rest coincides with Latin-1).
const CP1252_HIGH: [u16; 32] = [
    0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018,
    0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
];

/// Decoder of the 8-bit strings of BIFF5 (the codepage of the CODEPAGE record). Only the Windows-1252 family is
/// decoded as the TS decodes it (437, 850, 367, 32769, 1252); every other codepage reads as Windows-1252 here (the TS
/// uses the `TextDecoder` of that codepage: the C2 record lists it among the open items).
#[derive(Clone, Copy, Debug)]
pub struct Codepage;

impl Codepage {
    pub fn new(_cp: u16) -> Self {
        Codepage
    }

    pub fn decode(&self, b: &[u8]) -> String {
        b.iter()
            .map(|&c| {
                if (0x80..=0x9f).contains(&c) {
                    char::from_u32(u32::from(CP1252_HIGH[usize::from(c - 0x80)])).unwrap_or('\u{fffd}')
                } else {
                    char::from(c)
                }
            })
            .collect()
    }
}

/// `utf16(b)`: `TextDecoder('utf-16le').decode` (a lone surrogate becomes U+FFFD, a leading BOM is removed).
pub fn utf16(b: &[u8]) -> String {
    let b = if b.len() & 1 == 1 { b.get(..b.len() - 1).unwrap_or(&[]) } else { b };
    let mut units = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).peekable();
    if units.peek() == Some(&0xfeff) {
        units.next();
    }
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{fffd}')).collect()
}

/// `latin1(b)`: BIFF8 "compressed" characters (the low byte of the UTF-16 unit).
pub fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

/// `readChars`: `cch` characters from `off`, the options byte already consumed (`high` = fHighByte). At each CONTINUE
/// boundary inside the characters a new options byte starts the continuation. Returns the text and the next offset.
pub fn read_chars(r: &Rd<'_>, off: usize, cch: usize, mut high: bool, conts: &[usize]) -> (String, usize) {
    let mut text = String::new();
    let mut left = cch;
    let mut p = off;
    let mut ci = 0usize;
    // a boundary EXACTLY at the start of the characters also carries the options byte
    while ci < conts.len() && conts.get(ci).is_some_and(|&c| c < p) {
        ci += 1;
    }
    while left > 0 && p < r.len() {
        let bound = conts.get(ci).copied().unwrap_or(r.len());
        let span = i64::try_from(bound).unwrap_or(i64::MAX) - i64::try_from(p).unwrap_or(0);
        let avail = if high { span >> 1 } else { span };
        let n = i64::try_from(left).unwrap_or(i64::MAX).min(avail);
        let mut n_used = 0usize;
        if n > 0 {
            n_used = usize::try_from(n).unwrap_or(0);
            let bytes = r.sub(p, p + if high { n_used * 2 } else { n_used });
            if high {
                text.push_str(&utf16(bytes));
            } else {
                text.push_str(&latin1(bytes));
            }
            p += if high { n_used * 2 } else { n_used };
            left -= n_used;
        }
        if left > 0 {
            if p >= r.len() {
                break;
            }
            if p >= bound && ci < conts.len() {
                // a new CONTINUE: one options byte (only fHighByte matters)
                high = r.u8(p) & 1 != 0;
                p += 1;
                ci += 1;
            } else if n_used == 0 {
                // a UTF-16 character split at the boundary (malformed file): skip the byte
                p += 1;
            }
        }
    }
    (text, p)
}

/// `readXLString`: BIFF8 `XLUnicodeString` (cch + options + characters) or BIFF5 (cch + bytes in the codepage).
/// `cch_size` is 1 for `ShortXLUnicodeString`.
pub fn read_xl_string(r: &Rd<'_>, off: usize, biff8: bool, cp: &Codepage, cch_size: usize, conts: &[usize]) -> (String, usize) {
    let cch = if cch_size == 1 { usize::from(r.u8(off)) } else { usize::from(r.u16(off)) };
    let mut p = off + cch_size;
    if !biff8 {
        let bytes = r.sub(p, p + cch);
        return (cp.decode(bytes), p + cch);
    }
    let flags = r.u8(p);
    p += 1;
    read_chars(r, p, cch, flags & 1 != 0, conts)
}

/// A text-format run: from the character `ich` on, the font `ifnt` applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatRun {
    pub ich: u16,
    pub ifnt: u16,
}

/// A string with format runs (SST, RSTRING, TXO).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichString {
    pub text: String,
    pub runs: Vec<FormatRun>,
}

/// `readRichExtString`: an SST item (`XLUnicodeRichExtendedString`): cch, options (fHighByte, fExtSt, fRichSt), [cRun],
/// [cbExtRst], characters, runs (4 bytes each), the phonetic block (skipped).
pub fn read_rich_ext_string(r: &Rd<'_>, off: usize, conts: &[usize]) -> (RichString, usize) {
    let cch = usize::from(r.u16(off));
    let flags = r.u8(off + 2);
    let mut p = off + 3;
    let mut c_run = 0usize;
    let mut cb_ext = 0i32;
    if flags & 0x08 != 0 {
        c_run = usize::from(r.u16(p));
        p += 2;
    }
    if flags & 0x04 != 0 {
        cb_ext = r.i32(p);
        p += 4;
    }
    let (text, next) = read_chars(r, p, cch, flags & 1 != 0, conts);
    p = next;
    let mut s = RichString { text, runs: Vec::new() };
    if c_run > 0 {
        let mut i = 0;
        while i < c_run && p + 4 <= r.len() {
            s.runs.push(FormatRun { ich: r.u16(p), ifnt: r.u16(p + 2) });
            i += 1;
            p += 4;
        }
        if c_run > s.runs.len() {
            p += (c_run - s.runs.len()) * 4;
        }
    }
    if cb_ext > 0 {
        p += usize::try_from(cb_ext).unwrap_or(0);
    }
    (s, p)
}
