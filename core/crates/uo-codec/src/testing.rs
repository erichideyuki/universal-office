//! A small deterministic generator for the generated `random_*` functions of the test-support
//! files (tests and fuzz targets; no dependency on a random crate).

/// SplitMix64.
#[derive(Debug, Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn u32(&mut self) -> u32 {
        u32::try_from(self.u64() >> 32).unwrap_or(0)
    }

    /// 0..n (0 when n is 0).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { u32::try_from(self.u64() % u64::from(n)).unwrap_or(0) }
    }

    pub fn bool(&mut self) -> bool {
        self.u64() & 1 == 1
    }

    pub fn u8(&mut self) -> u8 {
        u8::try_from(self.u64() >> 56).unwrap_or(0)
    }

    pub fn u16(&mut self) -> u16 {
        u16::try_from(self.u64() >> 48).unwrap_or(0)
    }

    /// Mostly small, sometimes at the edges of the varint ranges.
    pub fn u32_edge(&mut self) -> u32 {
        match self.below(8) {
            0 => 0,
            1 => 0x7F,
            2 => 0x80,
            3 => 0x3FFF,
            4 => 0x4000,
            5 => u32::MAX,
            6 => self.u32(),
            _ => self.below(300),
        }
    }

    pub fn i32_edge(&mut self) -> i32 {
        match self.below(8) {
            0 => 0,
            1 => -1,
            2 => i32::MIN,
            3 => i32::MAX,
            4 => 63,
            5 => -64,
            6 => self.u32().cast_signed(),
            _ => i32::try_from(self.below(600)).unwrap_or(0).saturating_sub(300),
        }
    }

    /// Finite values, the edges (`-0`, infinities, subnormals, the canonical NaN, a NaN with a payload that the encoder canonicalizes) and random bits.
    pub fn f64_edge(&mut self) -> f64 {
        match self.below(12) {
            0 => 0.0,
            1 => -0.0,
            2 => f64::INFINITY,
            3 => f64::NEG_INFINITY,
            4 => f64::NAN,
            5 => f64::from_bits(0x7FF8_0000_0000_0001),
            6 => f64::from_bits(1),
            7 => f64::MAX,
            8 => f64::MIN_POSITIVE,
            9 => f64::from(self.below(100_000)) / 20.0,
            _ => f64::from_bits(self.u64()),
        }
    }

    /// Random code units, lone surrogates included, at most `max` of them.
    pub fn utf16(&mut self, max: u32) -> Vec<u16> {
        let n = self.below(max.saturating_add(1));
        (0..n)
            .map(|_| match self.below(6) {
                0 => 0xD800 + u16::try_from(self.below(0x800)).unwrap_or(0),
                1 => 0,
                2 => 0x20 + u16::try_from(self.below(0x5F)).unwrap_or(0),
                _ => self.u16(),
            })
            .collect()
    }

    /// A string that differs from every other `index`: symbol tables are keyed by their strings in TypeScript.
    pub fn unique_utf16(&mut self, index: u32, max: u32) -> Vec<u16> {
        let mut v = vec![u16::try_from(index >> 16).unwrap_or(0), u16::try_from(index & 0xFFFF).unwrap_or(0)];
        v.extend(self.utf16(max));
        v
    }

    pub fn bytes(&mut self, max: u32) -> Vec<u8> {
        let n = self.below(max.saturating_add(1));
        (0..n).map(|_| self.u8()).collect()
    }
}
