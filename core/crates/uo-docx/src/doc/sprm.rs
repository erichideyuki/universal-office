//! Single Property Modifiers ([MS-DOC] 2.2.5.1, `sprm.ts`): a `Prl` is a `sprm` (u16) plus an
//! operand. The `sprm` encodes `ispmd` (9 bits), `fSpec` (1), `sgc` (3: 1 paragraph, 2 character,
//! 3 picture, 4 section, 5 table) and `spra` (3: the operand size): 0 and 1 are one byte, 2, 4 and
//! 5 two, 3 four, 7 three, 6 variable (the first byte is the size; exceptions: `sprmTDefTable`
//! with a u16 size "plus one" and `sprmPChgTabs` with `cb = 255`).

use super::bytes::ByteReader;

/// One property modifier: the operand starts at `op` of `r` (for the variable ones, AFTER the
/// size byte or bytes) and is `len` bytes long.
#[derive(Debug, Clone, Copy)]
pub struct Prl<'a> {
    pub sprm: u16,
    pub op: u64,
    pub len: u64,
    pub r: ByteReader<'a>,
}

/// `sgcOf`.
#[must_use]
pub fn sgc_of(sprm: u16) -> u16 {
    (sprm >> 10) & 7
}

impl Prl<'_> {
    #[must_use]
    pub fn u8(&self) -> u8 {
        self.r.u8(self.op)
    }

    #[must_use]
    pub fn u16(&self) -> u16 {
        self.r.u16(self.op)
    }

    #[must_use]
    pub fn i16(&self) -> i16 {
        self.r.i16(self.op)
    }

    #[must_use]
    pub fn u32(&self) -> u32 {
        self.r.u32(self.op)
    }

    #[must_use]
    pub fn i32(&self) -> i32 {
        self.r.i32(self.op)
    }

    /// The operand bytes (`p.r.sub(p.op, p.op + p.len)`).
    #[must_use]
    pub fn operand(&self) -> &[u8] {
        self.r.sub(self.op, self.op + self.len)
    }
}

/// The `Prl`s of a grpprl, in order; stops at the first one that overflows (`prls`).
#[must_use]
pub fn prls(grpprl: &[u8]) -> Prls<'_> {
    Prls { r: ByteReader::new(grpprl), off: 0, end: grpprl.len() as u64 }
}

/// Iterator over the `Prl`s of a grpprl.
#[derive(Debug, Clone)]
pub struct Prls<'a> {
    r: ByteReader<'a>,
    off: u64,
    end: u64,
}

impl<'a> Iterator for Prls<'a> {
    type Item = Prl<'a>;

    fn next(&mut self) -> Option<Prl<'a>> {
        let r = self.r;
        if self.off + 2 > self.end {
            return None;
        }
        let sprm = r.u16(self.off);
        let off = self.off + 2;
        let spra = sprm >> 13;
        let mut op = off;
        let len: u64 = match spra {
            0 | 1 => 1,
            2 | 4 | 5 => 2,
            3 => 4,
            7 => 3,
            _ => {
                if sprm == 0xd608 || sprm == 0xd606 {
                    // TDefTableOperand: cb (u16) = the remaining bytes + 1
                    let cb = r.u16(off);
                    op = off + 2;
                    u64::from(cb.saturating_sub(1))
                } else if sprm == 0xc615 {
                    let cb = r.u8(off);
                    op = off + 1;
                    if cb == 255 {
                        // the long PChgTabsOperand: the size comes from the counts
                        let c_del = u64::from(r.u8(op));
                        let c_add = u64::from(r.u8(op + 1 + c_del * 4));
                        1 + c_del * 4 + 1 + c_add * 3
                    } else {
                        u64::from(cb)
                    }
                } else {
                    let l = u64::from(r.u8(off));
                    op = off + 1;
                    l
                }
            }
        };
        if op + len > self.end {
            // `break`: the walk is over for good
            self.off = self.end;
            return None;
        }
        self.off = op + len;
        Some(Prl { sprm, op, len, r })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_the_operand_classes() {
        // sprmCFBold (0x0835, spra 0): 1 byte; sprmCHps (0x4a43, spra 2): 2 bytes;
        // sprmPDyaLine (0x6412, spra 3): 4 bytes; sprmCCv (0x6870): 4; spra 7: 3 bytes
        let g = [0x35, 0x08, 0x01, 0x43, 0x4a, 0x18, 0x00, 0x12, 0x64, 1, 0, 0, 0, 0x01, 0xe0, 9, 8, 7];
        let v: Vec<_> = prls(&g).map(|p| (p.sprm, p.op, p.len)).collect();
        assert_eq!(v, vec![(0x0835, 2, 1), (0x4a43, 5, 2), (0x6412, 9, 4), (0xe001, 15, 3)]);
    }

    #[test]
    fn variable_operands_and_the_exceptions() {
        // spra 6 with a size byte: 0xc60d (sprmPChgTabsPapx)
        let g = [0x0d, 0xc6, 0x02, 0xaa, 0xbb];
        let p = prls(&g).next().unwrap();
        assert_eq!((p.op, p.len), (3, 2));
        // sprmTDefTable: the u16 size is the remaining bytes plus one
        let g = [0x08, 0xd6, 0x04, 0x00, 1, 2, 3];
        let p = prls(&g).next().unwrap();
        assert_eq!((p.op, p.len), (4, 3));
        // sprmPChgTabs with cb = 255: sizes from the counts: cDel = 1, cAdd = 2
        let mut g = vec![0x15, 0xc6, 0xff, 1, 0, 0, 0, 0, 2];
        g.extend([0; 2 + 6]);
        let p = prls(&g).next().unwrap();
        assert_eq!((p.op, p.len), (3, 1 + 4 + 1 + 6));
    }

    #[test]
    fn stops_at_the_first_overflow() {
        let g = [0x35, 0x08, 0x01, 0x43, 0x4a, 0x18];
        assert_eq!(prls(&g).count(), 1);
        let g = [0x35, 0x08, 0x01, 0x0d, 0xc6, 0xff, 0x35, 0x08, 0x01];
        assert_eq!(prls(&g).count(), 1);
        assert_eq!(prls(&[]).count(), 0);
        assert_eq!(prls(&[0x35]).count(), 0);
    }

    #[test]
    fn sgc() {
        assert_eq!(sgc_of(0x0835), 2);
        assert_eq!(sgc_of(0x4600), 1);
        assert_eq!(sgc_of(0xd608), 5);
        assert_eq!(sgc_of(0xb017), 4);
    }
}
