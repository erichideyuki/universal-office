use crate::DecodeError;

/// Most tables a schema may declare (the reader keeps one bound per table).
pub const MAX_TABLES: usize = 16;

/// Bounds on what a message may make the decoder allocate. The message size is the caller's to
/// limit; these cap what a small message can expand into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Nesting of recursive types.
    pub max_depth: u32,
    /// Elements of one list, columns value or table, and code units of one string.
    pub max_list_len: u32,
    /// Elements and code units of the whole message, all lists and strings together.
    pub max_total: u64,
    /// Bytes the decoded values may take in memory: a count of one-byte elements that are 200 bytes
    /// in Rust is bounded here, not by the message size. The tables of a stream are counted per message.
    pub max_alloc: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_depth: 64, max_list_len: 1 << 24, max_total: 1 << 26, max_alloc: 1 << 27 }
    }
}

/// The fixed part of a message header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub message: u32,
    pub generation: u32,
}

/// Bit `i` of a bitmap read with [`Reader::bitmap`].
#[must_use]
pub fn bit(bitmap: u64, i: u32) -> bool {
    bitmap.checked_shr(i).is_some_and(|v| v & 1 == 1)
}

/// Bit `i` of bit-packed booleans read with [`Reader::bits`] (false past the end).
#[must_use]
pub fn bit_at(bits: &[u8], i: usize) -> bool {
    bits.get(i / 8).is_some_and(|b| (b >> (i % 8)) & 1 == 1)
}

/// Reads the wire format from untrusted bytes. Every read is bounds-checked and returns
/// [`DecodeError`]; nothing panics.
#[derive(Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    limits: Limits,
    depth: u32,
    total: u64,
    alloc: u64,
    bounds: [u32; MAX_TABLES],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8], limits: &Limits) -> Self {
        Self { buf, pos: 0, limits: *limits, depth: 0, total: 0, alloc: 0, bounds: [0; MAX_TABLES] }
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// The message must end here.
    pub fn end(&self) -> Result<(), DecodeError> {
        if self.pos == self.buf.len() { Ok(()) } else { Err(DecodeError::TrailingBytes) }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        let b = *self.buf.get(self.pos).ok_or(DecodeError::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::BadBool),
        }
    }

    /// LEB128 `u32`: at most 5 bytes, nothing over `u32::MAX`, shortest form only.
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        let mut value: u64 = 0;
        for i in 0..5u32 {
            let b = self.u8()?;
            value |= u64::from(b & 0x7F) << (7 * i);
            if b & 0x80 == 0 {
                if i > 0 && b == 0 {
                    return Err(DecodeError::BadVarint);
                }
                return u32::try_from(value).map_err(|_| DecodeError::BadVarint);
            }
        }
        Err(DecodeError::BadVarint)
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        u16::try_from(self.u32()?).map_err(|_| DecodeError::BadVarint)
    }

    pub fn i32(&mut self) -> Result<i32, DecodeError> {
        let z = self.u32()?;
        Ok(((z >> 1) ^ (z & 1).wrapping_neg()).cast_signed())
    }

    /// 8 raw bytes; a NaN other than the canonical one is refused.
    pub fn f64(&mut self) -> Result<f64, DecodeError> {
        let s = self.take(8)?;
        let mut b = [0u8; 8];
        b.copy_from_slice(s);
        let v = f64::from_le_bytes(b);
        if v.is_nan() && v.to_bits() != 0x7FF8_0000_0000_0000 {
            return Err(DecodeError::NonCanonicalNaN);
        }
        Ok(v)
    }

    fn charge(&mut self, n: u64) -> Result<(), DecodeError> {
        self.total = self.total.saturating_add(n);
        if self.total > self.limits.max_total { Err(DecodeError::Limit("total elements")) } else { Ok(()) }
    }

    /// The count of a list or a table whose elements take at least `min_bits` bits each: it must
    /// fit in the bytes that remain (so nothing is allocated for a count the message cannot back),
    /// in `max_list_len`, and in the message's total.
    pub fn list_len(&mut self, min_bits: u64) -> Result<usize, DecodeError> {
        let n = self.u32()?;
        if n > self.limits.max_list_len {
            return Err(DecodeError::Limit("list length"));
        }
        let have = u64::try_from(self.remaining()).unwrap_or(u64::MAX).saturating_mul(8);
        if u64::from(n).saturating_mul(min_bits) > have {
            return Err(DecodeError::Truncated);
        }
        self.charge(u64::from(n))?;
        usize::try_from(n).map_err(|_| DecodeError::Limit("list length"))
    }

    /// Counts `n` values of `elem_bytes` bytes each against [`Limits::max_alloc`]; call it before allocating them.
    pub fn charge_alloc(&mut self, n: usize, elem_bytes: usize) -> Result<(), DecodeError> {
        let bytes = u64::try_from(n).unwrap_or(u64::MAX).saturating_mul(u64::try_from(elem_bytes).unwrap_or(u64::MAX));
        self.alloc = self.alloc.saturating_add(bytes);
        if self.alloc > self.limits.max_alloc { Err(DecodeError::Limit("allocation")) } else { Ok(()) }
    }

    /// [`Reader::list_len`] for a list of `T`: the allocation of the elements is counted too.
    pub fn list_of<T>(&mut self, min_bits: u64) -> Result<usize, DecodeError> {
        let n = self.list_len(min_bits)?;
        self.charge_alloc(n, std::mem::size_of::<T>())?;
        Ok(n)
    }

    /// A string of UTF-16 code units (lone surrogates kept).
    pub fn utf16(&mut self) -> Result<Vec<u16>, DecodeError> {
        let n = self.list_len(16)?;
        self.charge_alloc(n, 2)?;
        let bytes = self.take(n.checked_mul(2).ok_or(DecodeError::Truncated)?)?;
        Ok(bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)])).collect())
    }

    /// An inline byte string.
    pub fn bytes(&mut self) -> Result<Vec<u8>, DecodeError> {
        let n = self.list_len(8)?;
        self.charge_alloc(n, 1)?;
        Ok(self.take(n)?.to_vec())
    }

    /// The bitmap of a struct of `nbits` (at most 64) bit fields; the padding must be zero.
    pub fn bitmap(&mut self, nbits: u32) -> Result<u64, DecodeError> {
        let bytes = self.bits(usize::try_from(nbits.min(64)).unwrap_or(64))?;
        let mut all = [0u8; 8];
        for (dst, src) in all.iter_mut().zip(bytes) {
            *dst = *src;
        }
        Ok(u64::from_le_bytes(all))
    }

    /// `n` bit-packed booleans: `ceil(n/8)` bytes whose padding bits are zero. Read them with
    /// [`bit_at`].
    pub fn bits(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let s = self.take(n.div_ceil(8))?;
        if !n.is_multiple_of(8) && s.last().copied().unwrap_or(0) >> (n % 8) != 0 {
            return Err(DecodeError::NonZeroPadding);
        }
        Ok(s)
    }

    /// A `list<bool>`: the count, then the bits.
    pub fn bit_list(&mut self) -> Result<Vec<bool>, DecodeError> {
        let n = self.list_len(1)?;
        self.charge_alloc(n, 1)?;
        let bits = self.bits(n)?;
        Ok((0..n).map(|i| bit_at(bits, i)).collect())
    }

    /// Enters a recursive type.
    pub fn enter(&mut self) -> Result<(), DecodeError> {
        self.depth += 1;
        if self.depth > self.limits.max_depth { Err(DecodeError::Limit("depth")) } else { Ok(()) }
    }

    /// Leaves a recursive type.
    pub fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// The header: magic and schema hash are checked here, the message id and generation returned.
    pub fn header(&mut self, magic: &[u8; 4], hash: &[u8; 8]) -> Result<Header, DecodeError> {
        if self.take(4)? != magic {
            return Err(DecodeError::BadMagic);
        }
        let found = self.take(8)?;
        if found != hash {
            let mut f = [0u8; 8];
            f.copy_from_slice(found);
            return Err(DecodeError::VersionMismatch { expected: *hash, found: f });
        }
        Ok(Header { message: self.u32()?, generation: self.u32()? })
    }

    /// `from` and `count` of one table's entries in a message. `from` must be the table's current
    /// length, `min_bits` the least an entry takes. Sets the bound of ids into this table.
    pub fn table_header(&mut self, table: usize, name: &'static str, len: usize, min_bits: u64) -> Result<usize, DecodeError> {
        let from = self.u32()?;
        let expected = u32::try_from(len).map_err(|_| DecodeError::Limit("table length"))?;
        if from != expected {
            return Err(DecodeError::TableGap { table: name, expected, found: from });
        }
        let count = self.list_len(min_bits)?;
        let bound = u32::try_from(count).ok().and_then(|c| expected.checked_add(c)).ok_or(DecodeError::Limit("table length"))?;
        if let Some(slot) = self.bounds.get_mut(table) {
            *slot = bound;
        }
        Ok(count)
    }

    /// An id into table `table` (`name` for the error): below the bound set by `table_header`.
    pub fn ref_in(&mut self, table: usize, name: &'static str) -> Result<u32, DecodeError> {
        let index = self.u32()?;
        let bound = self.bounds.get(table).copied().unwrap_or(0);
        if index < bound { Ok(index) } else { Err(DecodeError::BadRef { table: name, index, bound }) }
    }
}
