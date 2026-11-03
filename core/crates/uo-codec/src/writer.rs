use crate::EncodeError;
use crate::canonical_f64_bits;

/// Appends the wire format to a caller-owned buffer. Nothing else is allocated. A value that
/// cannot be expressed (a count over `u32::MAX`, columns of different lengths) is remembered, the
/// rest of the encode is a no-op and [`Writer::finish`] reports it and removes the partial bytes.
#[derive(Debug)]
pub struct Writer<'a> {
    out: &'a mut Vec<u8>,
    start: usize,
    error: Option<EncodeError>,
}

impl<'a> Writer<'a> {
    pub fn new(out: &'a mut Vec<u8>) -> Self {
        let start = out.len();
        Self { out, start, error: None }
    }

    /// Records the first error; the rest of the encode is wasted work, not a panic.
    pub fn fail(&mut self, error: EncodeError) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    /// Ends the message: the error of the first failed write, with the partial bytes removed.
    pub fn finish(self) -> Result<(), EncodeError> {
        match self.error {
            None => Ok(()),
            Some(e) => {
                self.out.truncate(self.start);
                Err(e)
            }
        }
    }

    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.out.push(u8::from(v));
    }

    /// LEB128, shortest form.
    pub fn u32(&mut self, v: u32) {
        let mut v = v;
        loop {
            let low = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.out.push(low);
                return;
            }
            self.out.push(low | 0x80);
        }
    }

    pub fn u16(&mut self, v: u16) {
        self.u32(u32::from(v));
    }

    /// Zigzag, then LEB128.
    pub fn i32(&mut self, v: i32) {
        let z = ((v << 1) ^ (v >> 31)).cast_unsigned();
        self.u32(z);
    }

    /// 8 raw bytes; NaN is canonicalized.
    pub fn f64(&mut self, v: f64) {
        self.out.extend_from_slice(&canonical_f64_bits(v).to_le_bytes());
    }

    /// A count, a length or a table size.
    pub fn len(&mut self, n: usize) {
        match u32::try_from(n) {
            Ok(n) => self.u32(n),
            Err(_) => self.fail(EncodeError::TooLarge),
        }
    }

    /// UTF-16 code units: the count, then 2 bytes each.
    pub fn utf16(&mut self, units: &[u16]) {
        self.len(units.len());
        self.out.reserve(units.len().saturating_mul(2));
        for u in units {
            self.out.extend_from_slice(&u.to_le_bytes());
        }
    }

    /// An inline byte string: the length, then the bytes.
    pub fn bytes(&mut self, bytes: &[u8]) {
        self.len(bytes.len());
        self.out.extend_from_slice(bytes);
    }

    /// The bitmap of a struct: bit `i` of the result is `bits[i]`, `ceil(n/8)` bytes (at most 64 bits).
    pub fn bitmap(&mut self, bits: &[bool]) {
        let mut acc: u64 = 0;
        for (i, b) in bits.iter().take(64).enumerate() {
            acc |= u64::from(*b) << i;
        }
        let bytes = bits.len().min(64).div_ceil(8);
        let all = acc.to_le_bytes();
        self.out.extend_from_slice(all.get(..bytes).unwrap_or(&all));
    }

    /// Bit-packed booleans without a count (the caller wrote it or it is implied): `ceil(n/8)` bytes.
    pub fn bits(&mut self, bits: impl Iterator<Item = bool>) {
        let mut byte = 0u8;
        let mut n = 0usize;
        for b in bits {
            byte |= u8::from(b) << (n & 7);
            n += 1;
            if n & 7 == 0 {
                self.out.push(byte);
                byte = 0;
            }
        }
        if n & 7 != 0 {
            self.out.push(byte);
        }
    }

    /// A `list<bool>`: the count, then the bits.
    pub fn bit_list(&mut self, items: &[bool]) {
        self.len(items.len());
        self.bits(items.iter().copied());
    }

    /// The message header: magic, schema hash, message id and generation.
    pub fn header(&mut self, magic: &[u8; 4], hash: &[u8; 8], message: u32, generation: u32) {
        self.out.extend_from_slice(magic);
        self.out.extend_from_slice(hash);
        self.u32(message);
        self.u32(generation);
    }
}
