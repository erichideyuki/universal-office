//! File Information Block ([MS-DOC] 2.5, `fib.ts`): the index of the `.doc`. It starts the
//! `WordDocument` stream and points (`fc`/`lcb`) at the structures of the table stream
//! (`0Table`/`1Table`): `FibBase` (32 bytes), `csw` + `FibRgW97`, `cslw` + `FibRgLw97`,
//! `cbRgFcLcb` + `FibRgFcLcb` (the pairs), `cswNew` + `FibRgCswNew`. The sizes come from the
//! counters, not from assumptions.

use super::DocError;
use super::bytes::ByteReader;

/// Indexes of the `fc`/`lcb` pairs of `FibRgFcLcb97` (and the 2000+ extensions): the `FC` object
/// of the TS.
#[allow(non_upper_case_globals, dead_code)]
pub mod fc {
    pub const StshfOrig: usize = 0;
    pub const Stshf: usize = 1;
    pub const PlcffndRef: usize = 2;
    pub const PlcffndTxt: usize = 3;
    pub const PlcfandRef: usize = 4;
    pub const PlcfandTxt: usize = 5;
    pub const PlcfSed: usize = 6;
    pub const PlcPad: usize = 7;
    pub const PlcfPhe: usize = 8;
    pub const SttbfGlsy: usize = 9;
    pub const PlcfGlsy: usize = 10;
    pub const PlcfHdd: usize = 11;
    pub const PlcfBteChpx: usize = 12;
    pub const PlcfBtePapx: usize = 13;
    pub const PlcfSea: usize = 14;
    pub const SttbfFfn: usize = 15;
    pub const PlcfFldMom: usize = 16;
    pub const PlcfFldHdr: usize = 17;
    pub const PlcfFldFtn: usize = 18;
    pub const PlcfFldAtn: usize = 19;
    pub const PlcfFldMcr: usize = 20;
    pub const SttbfBkmk: usize = 21;
    pub const PlcfBkf: usize = 22;
    pub const PlcfBkl: usize = 23;
    pub const Cmds: usize = 24;
    pub const Unused1: usize = 25;
    pub const SttbfMcr: usize = 26;
    pub const PrDrvr: usize = 27;
    pub const PrEnvPort: usize = 28;
    pub const PrEnvLand: usize = 29;
    pub const Wss: usize = 30;
    pub const Dop: usize = 31;
    pub const SttbfAssoc: usize = 32;
    pub const Clx: usize = 33;
    pub const PlcfPgdFtn: usize = 34;
    pub const AutosaveSource: usize = 35;
    pub const GrpXstAtnOwners: usize = 36;
    pub const SttbfAtnBkmk: usize = 37;
    pub const Unused2: usize = 38;
    pub const Unused3: usize = 39;
    pub const PlcSpaMom: usize = 40;
    pub const PlcSpaHdr: usize = 41;
    pub const PlcfAtnBkf: usize = 42;
    pub const PlcfAtnBkl: usize = 43;
    pub const Pms: usize = 44;
    pub const FormFldSttbs: usize = 45;
    pub const PlcfendRef: usize = 46;
    pub const PlcfendTxt: usize = 47;
    pub const PlcfFldEdn: usize = 48;
    pub const Unused4: usize = 49;
    pub const DggInfo: usize = 50;
    pub const SttbfRMark: usize = 51;
    pub const SttbfCaption: usize = 52;
    pub const SttbfAutoCaption: usize = 53;
    pub const PlcfWkb: usize = 54;
    pub const PlcfSpl: usize = 55;
    pub const PlcftxbxTxt: usize = 56;
    pub const PlcfFldTxbx: usize = 57;
    pub const PlcfHdrtxbxTxt: usize = 58;
    pub const PlcffldHdrTxbx: usize = 59;
    pub const StwUser: usize = 60;
    pub const SttbTtmbd: usize = 61;
    pub const CookieData: usize = 62;
    pub const PgdMotherOldOld: usize = 63;
    pub const BkdMotherOldOld: usize = 64;
    pub const PgdFtnOldOld: usize = 65;
    pub const BkdFtnOldOld: usize = 66;
    pub const PgdEdnOldOld: usize = 67;
    pub const BkdEdnOldOld: usize = 68;
    pub const SttbfIntlFld: usize = 69;
    pub const RouteSlip: usize = 70;
    pub const SttbSavedBy: usize = 71;
    pub const SttbFnm: usize = 72;
    pub const PlfLst: usize = 73;
    pub const PlfLfo: usize = 74;
    pub const PlcfTxbxBkd: usize = 75;
    pub const PlcfTxbxHdrBkd: usize = 76;
    pub const DocUndoWord9: usize = 77;
    pub const RgbUse: usize = 78;
    pub const Usp: usize = 79;
    pub const Uskf: usize = 80;
    pub const PlcupcRgbUse: usize = 81;
    pub const PlcupcUsp: usize = 82;
    pub const SttbGlsyStyle: usize = 83;
    pub const Plgosl: usize = 84;
    pub const Plcocx: usize = 85;
    pub const PlcfBteLvc: usize = 86;
    pub const FtModified: usize = 87;
    pub const PlcfLvcPre10: usize = 88;
    pub const PlcfAsumy: usize = 89;
    pub const PlcfGram: usize = 90;
    pub const SttbListNames: usize = 91;
    pub const SttbfUssr: usize = 92;
}

/// How many pairs of `FibRgFcLcb` the port keeps: the TS names indexes 0 to 92 only, so the
/// pairs past 92 are never read (`fcLcb` of a larger index would be real data in the TS; no
/// caller asks).
pub const FC_LCB_COUNT: usize = 93;

/// An `fc`/`lcb` pair: where a structure starts in the table stream and its size in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FcLcb {
    pub fc: u32,
    pub lcb: u32,
}

/// The FIB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fib {
    pub w_ident: u16,
    /// `nFib`, replaced by `nFibNew` when the FIB has one and it is not 0
    pub n_fib: u16,
    pub lid: u16,
    pub f_dot: bool,
    pub f_complex: bool,
    pub f_encrypted: bool,
    pub f_which_tbl_stm: bool,
    pub f_obfuscated: bool,
    pub f_ext_char: bool,
    pub f_far_east: bool,
    pub lid_fe: u16,
    pub ccp_text: i32,
    pub ccp_ftn: i32,
    pub ccp_hdd: i32,
    pub ccp_mcr: i32,
    pub ccp_atn: i32,
    pub ccp_edn: i32,
    pub ccp_txbx: i32,
    pub ccp_hdr_txbx: i32,
    pairs: [FcLcb; FC_LCB_COUNT],
}

impl Fib {
    /// `fcLcb(index)`: the pair, or zeros when absent (past `cbRgFcLcb`, or past the 93 kept).
    #[must_use]
    pub fn fc_lcb(&self, index: usize) -> FcLcb {
        self.pairs.get(index).copied().unwrap_or_default()
    }
}

/// `readFib`.
///
/// # Errors
/// [`DocError::FibTruncated`] under 0x9A bytes, [`DocError::FibSignature`] for an unknown
/// `wIdent`.
pub fn read_fib(word: &[u8]) -> Result<Fib, DocError> {
    if word.len() < 0x9a {
        return Err(DocError::FibTruncated);
    }
    let r = ByteReader::new(word);
    let w_ident = r.u16(0);
    if w_ident != 0xa5ec && w_ident != 0xa5dc {
        return Err(DocError::FibSignature { ident: w_ident });
    }
    let mut n_fib = r.u16(2);
    let lid = r.u16(6);
    let flags = r.u16(0x0a);
    // the variable counters
    let mut off: u64 = 32;
    let csw = u64::from(r.u16(off));
    off += 2;
    let rg_w = off;
    off += csw * 2;
    let cslw = u64::from(r.u16(off));
    off += 2;
    let rg_lw = off;
    off += cslw * 4;
    let cb_rg_fc_lcb = u64::from(r.u16(off));
    off += 2;
    let rg_fc_lcb = off;
    off += cb_rg_fc_lcb * 8;
    let csw_new = if off + 2 <= r.len() { r.u16(off) } else { 0 };
    off += 2;
    if csw_new > 0 && off + 2 <= r.len() {
        let n_fib_new = r.u16(off);
        if n_fib_new != 0 {
            n_fib = n_fib_new;
        }
    }
    let lw = |i: u64| -> i32 { if i < cslw { r.i32(rg_lw + i * 4) } else { 0 } };
    let mut pairs = [FcLcb::default(); FC_LCB_COUNT];
    for (i, p) in pairs.iter_mut().enumerate() {
        let i = i as u64;
        if i < cb_rg_fc_lcb {
            *p = FcLcb { fc: r.u32(rg_fc_lcb + i * 8), lcb: r.u32(rg_fc_lcb + i * 8 + 4) };
        }
    }
    Ok(Fib {
        w_ident,
        n_fib,
        lid,
        f_dot: flags & 0x0001 != 0,
        f_complex: flags & 0x0004 != 0,
        f_encrypted: flags & 0x0100 != 0,
        f_which_tbl_stm: flags & 0x0200 != 0,
        f_ext_char: flags & 0x1000 != 0,
        f_far_east: flags & 0x4000 != 0,
        f_obfuscated: flags & 0x8000 != 0,
        lid_fe: if csw > 13 { r.u16(rg_w + 26) } else { 0 },
        ccp_text: lw(3),
        ccp_ftn: lw(4),
        ccp_hdd: lw(5),
        ccp_mcr: lw(6),
        ccp_atn: lw(7),
        ccp_edn: lw(8),
        ccp_txbx: lw(9),
        ccp_hdr_txbx: lw(10),
        pairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(csw: u16, cslw: u16, cb: u16, ident: u16, n_fib: u16) -> Vec<u8> {
        let mut w = vec![0u8; 0x200];
        w[0..2].copy_from_slice(&ident.to_le_bytes());
        w[2..4].copy_from_slice(&n_fib.to_le_bytes());
        w[6..8].copy_from_slice(&0x0416u16.to_le_bytes());
        w[32..34].copy_from_slice(&csw.to_le_bytes());
        let mut off = 34 + usize::from(csw) * 2;
        w[off..off + 2].copy_from_slice(&cslw.to_le_bytes());
        off += 2;
        for i in 0..usize::from(cslw).min(11) {
            w[off + i * 4..off + i * 4 + 4].copy_from_slice(&(100 + i as i32).to_le_bytes());
        }
        off += usize::from(cslw) * 4;
        w[off..off + 2].copy_from_slice(&cb.to_le_bytes());
        off += 2;
        for i in 0..usize::from(cb).min(3) {
            w[off + i * 8..off + i * 8 + 4].copy_from_slice(&(1000 + i as u32).to_le_bytes());
            w[off + i * 8 + 4..off + i * 8 + 8].copy_from_slice(&(2000 + i as u32).to_le_bytes());
        }
        w
    }

    #[test]
    fn reads_the_counters_and_pairs() {
        let w = word(14, 22, 3, 0xa5ec, 0x00c1);
        let f = read_fib(&w).unwrap();
        assert_eq!((f.w_ident, f.n_fib, f.lid), (0xa5ec, 0xc1, 0x0416));
        assert_eq!((f.ccp_text, f.ccp_ftn, f.ccp_hdr_txbx), (103, 104, 110));
        assert_eq!(f.fc_lcb(1), FcLcb { fc: 1001, lcb: 2001 });
        assert_eq!(f.fc_lcb(3), FcLcb::default());
        assert_eq!(f.fc_lcb(500), FcLcb::default());
    }

    #[test]
    fn errors_by_class() {
        assert_eq!(read_fib(&[0; 0x99]).unwrap_err(), DocError::FibTruncated);
        let w = word(14, 22, 3, 0x1234, 0xc1);
        assert_eq!(read_fib(&w).unwrap_err(), DocError::FibSignature { ident: 0x1234 });
    }

    #[test]
    fn n_fib_new_replaces_n_fib_only_when_not_zero() {
        let mut w = word(14, 22, 2, 0xa5ec, 0x00c1);
        let off = 34 + 28 + 2 + 88 + 2 + 16;
        w[off..off + 2].copy_from_slice(&1u16.to_le_bytes());
        w[off + 2..off + 4].copy_from_slice(&0x0112u16.to_le_bytes());
        assert_eq!(read_fib(&w).unwrap().n_fib, 0x0112);
        w[off + 2..off + 4].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(read_fib(&w).unwrap().n_fib, 0x00c1);
    }

    #[test]
    fn huge_counters_read_zeros_past_the_end() {
        let mut w = word(14, 22, 3, 0xa5dc, 0x00c1);
        w[32..34].copy_from_slice(&0xffffu16.to_le_bytes());
        let f = read_fib(&w).unwrap();
        assert_eq!(f.fc_lcb(1), FcLcb::default());
        assert_eq!(f.ccp_text, 0);
    }
}
