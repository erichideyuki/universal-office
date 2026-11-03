//! The dictionary of the two LZW decoders of `images.ts` (GIF `lzw`, TIFF `tiffLzw`).
//!
//! The TS keeps each entry as an array copied from its predecessor and never stops adding
//! entries, so a hostile stream makes it quadratic in time and memory. Here an entry is a
//! (prefix, last byte) pair and the dictionary stops growing at 4,096 entries: a code has at
//! most 12 bits, so no later entry can be addressed and the output is the same.

const CAP: usize = 4096;
const NONE: u16 = u16::MAX;

#[derive(Clone, Copy)]
struct Entry {
    prefix: u16,
    suffix: u8,
    len: u16,
}

/// The decoder state shared by the GIF and TIFF loops: the dictionary, the previous entry (its
/// bytes and, when it is in the dictionary, its index) and the entry being decoded.
pub(crate) struct Lzw {
    dict: Vec<Entry>,
    clear: usize,
    prev: Vec<u8>,
    prev_idx: Option<usize>,
    have_prev: bool,
    cur: Vec<u8>,
    cur_idx: Option<usize>,
}

impl Lzw {
    /// A dictionary of `clear` one-byte entries (their value modulo 256, as the typed array the
    /// TS stores them in) and the two empty entries of the clear and end codes.
    pub(crate) fn new(clear: usize) -> Lzw {
        let mut lz = Lzw { dict: Vec::new(), clear, prev: Vec::new(), prev_idx: None, have_prev: false, cur: Vec::new(), cur_idx: None };
        lz.reset();
        lz
    }

    /// The clear code: the initial dictionary and no previous entry.
    pub(crate) fn reset(&mut self) {
        self.dict.clear();
        for i in 0..self.clear {
            self.dict.push(Entry { prefix: NONE, suffix: i as u8, len: 1 });
        }
        for _ in 0..2 {
            self.dict.push(Entry { prefix: NONE, suffix: 0, len: 0 });
        }
        self.have_prev = false;
        self.prev_idx = None;
    }

    /// `dict.length` of the TS (capped at 4,096, which changes no comparison the decoders make).
    pub(crate) fn len(&self) -> usize {
        self.dict.len()
    }

    /// The entry of a data code: the dictionary entry, or for a code beyond the dictionary the
    /// previous entry plus its first byte (whatever the code); `None` when there is no previous
    /// entry (the TS `break`).
    pub(crate) fn entry(&mut self, code: usize) -> Option<&[u8]> {
        if let Some(e) = self.dict.get(code).copied() {
            let len = usize::from(e.len);
            self.cur.clear();
            self.cur.resize(len, 0);
            let (mut e, mut j) = (e, len);
            while j > 0 {
                j -= 1;
                if let Some(slot) = self.cur.get_mut(j) {
                    *slot = e.suffix;
                }
                if e.prefix == NONE {
                    break;
                }
                match self.dict.get(usize::from(e.prefix)) {
                    Some(p) => e = *p,
                    None => break,
                }
            }
            self.cur_idx = Some(code);
        } else if self.have_prev {
            self.cur.clear();
            self.cur.extend_from_slice(&self.prev);
            self.cur.push(self.prev.first().copied().unwrap_or(0));
            self.cur_idx = None;
        } else {
            return None;
        }
        Some(&self.cur)
    }

    /// After the entry was used: adds `previous + first byte of this entry` to the dictionary
    /// and makes this entry the previous one.
    pub(crate) fn advance(&mut self) {
        if self.have_prev && self.dict.len() < CAP {
            let prefix = self.prev_idx.and_then(|i| u16::try_from(i).ok()).unwrap_or(NONE);
            if self.cur_idx.is_none() {
                self.cur_idx = Some(self.dict.len());
            }
            self.dict.push(Entry {
                prefix,
                suffix: self.cur.first().copied().unwrap_or(0),
                len: u16::try_from(self.prev.len() + 1).unwrap_or(u16::MAX),
            });
        }
        std::mem::swap(&mut self.prev, &mut self.cur);
        self.prev_idx = self.cur_idx;
        self.have_prev = true;
    }
}
