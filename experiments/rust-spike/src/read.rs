//! Leitura big-endian com limites, como o DataView do JS: fora do buffer é erro
//! (o RangeError que o TS captura nos mesmos pontos).

#[derive(Debug, Clone, Copy)]
pub struct OutOfRange;

pub type R<T> = Result<T, OutOfRange>;

#[derive(Clone, Copy)]
pub struct View<'a>(pub &'a [u8]);

impl<'a> View<'a> {
    #[inline]
    pub fn u8(&self, o: usize) -> R<u8> {
        self.0.get(o).copied().ok_or(OutOfRange)
    }
    #[inline]
    pub fn i8(&self, o: usize) -> R<i8> {
        self.u8(o).map(|b| b as i8)
    }
    #[inline]
    pub fn u16(&self, o: usize) -> R<u16> {
        match self.0.get(o..o + 2) {
            Some(b) => Ok(u16::from_be_bytes([b[0], b[1]])),
            None => Err(OutOfRange),
        }
    }
    #[inline]
    pub fn i16(&self, o: usize) -> R<i16> {
        self.u16(o).map(|v| v as i16)
    }
    #[inline]
    pub fn u32(&self, o: usize) -> R<u32> {
        match self.0.get(o..o + 4) {
            Some(b) => Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            None => Err(OutOfRange),
        }
    }
    #[inline]
    pub fn tag(&self, o: usize) -> R<[u8; 4]> {
        match self.0.get(o..o + 4) {
            Some(b) => Ok([b[0], b[1], b[2], b[3]]),
            None => Err(OutOfRange),
        }
    }
}

/// Math.round do JS (meio arredonda para +infinito).
#[inline]
pub fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}
