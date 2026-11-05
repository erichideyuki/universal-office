//! Lists ([MS-DOC] 2.9: `PlfLst`/`LSTF`/`LVL` + `PlfLfo`/`LFO`/`LFOData`/`LFOLVL`, `lists.ts`).
//! `PlfLst` is `cLst` (i16) then `LSTF[cLst]`; right after it, in the table stream, come the LVLs
//! of each list (9, or 1 if `fSimpleList`). Each LVL is `LVLF` (28 bytes), `grpprlPapx`,
//! `grpprlChpx` and `xst` (the number text with level markers). `PlfLfo` is `lfoMac` (u32), the
//! `LFO[lfoMac]` and the `LFOData[lfoMac]` (with the `LFOLVL`s of override). `sprmPIlfo` refers
//! to an LFO 1-based: the `numId` of OOXML.

use super::bytes::ByteReader;
use super::fib::{FcLcb, Fib, fc};
use crate::text::Utf16;

/// A list level (`Lvl`); the grpprls borrow the table stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lvl<'a> {
    pub start: i32,
    pub nfc: u8,
    pub jc: u8,
    pub legal: bool,
    pub no_restart: bool,
    pub ilvl_restart_lim: u8,
    /// 0 tab, 1 space, 2 nothing
    pub follow: u8,
    /// the number text: the codes 0-8 are the marker of level N
    pub xst: Utf16,
    /// the 1-based positions of the markers in `xst`
    pub rgbxch_nums: Vec<u8>,
    pub grpprl_papx: &'a [u8],
    pub grpprl_chpx: &'a [u8],
    pub tentative: bool,
}

/// A list (`Lst`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lst<'a> {
    pub lsid: i32,
    pub tplc: i32,
    pub simple: bool,
    pub hybrid: bool,
    /// the `istd` of the style linked to each level (0xFFF = none)
    pub rgistd_para: [u16; 9],
    pub levels: Vec<Lvl<'a>>,
}

/// An override of one level (`LfoLvl`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfoLvl<'a> {
    pub ilvl: u8,
    pub start_at: Option<i32>,
    pub lvl: Option<Lvl<'a>>,
}

/// A list override (`Lfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lfo<'a> {
    pub lsid: i32,
    pub overrides: Vec<LfoLvl<'a>>,
}

/// The lists of a document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lists<'a> {
    pub lsts: Vec<Lst<'a>>,
    pub lfos: Vec<Lfo<'a>>,
}

/// `readLvl`: the level at `off` and the offset after it; `end` bounds the grpprls.
pub(super) fn read_lvl<'a>(r: ByteReader<'a>, off: u64, end: u64) -> (Lvl<'a>, u64) {
    let flags = r.u8(off + 5);
    let mut nums = Vec::new();
    for i in 0..9 {
        let v = r.u8(off + 6 + i);
        if v == 0 {
            break;
        }
        nums.push(v);
    }
    let cb_chpx = u64::from(r.u8(off + 24));
    let cb_papx = u64::from(r.u8(off + 25));
    let mut o = off + 28;
    let grpprl_papx = r.sub(o, (o + cb_papx).min(end));
    o += cb_papx;
    let grpprl_chpx = r.sub(o, (o + cb_chpx).min(end));
    o += cb_chpx;
    let cch = u64::from(r.u16(o));
    let xst = r.utf16(o + 2, cch.min(0x100));
    o += 2 + cch * 2;
    (
        Lvl {
            start: r.i32(off),
            nfc: r.u8(off + 4),
            jc: flags & 3,
            legal: flags & 4 != 0,
            no_restart: flags & 8 != 0,
            tentative: flags & 0x80 != 0,
            rgbxch_nums: nums,
            follow: r.u8(off + 15),
            ilvl_restart_lim: r.u8(off + 26),
            grpprl_papx,
            grpprl_chpx,
            xst: Utf16::from_units(xst),
        },
        o,
    )
}

/// `readLists`.
#[must_use]
pub fn read_lists<'a>(fib: &Fib, table: &'a [u8]) -> Lists<'a> {
    read_lists_at(fib.fc_lcb(fc::PlfLst), fib.fc_lcb(fc::PlfLfo), table)
}

/// `readLists` for the pairs `PlfLst` and `PlfLfo`.
#[must_use]
pub fn read_lists_at<'a>(lst: FcLcb, lfo: FcLcb, table: &'a [u8]) -> Lists<'a> {
    let r = ByteReader::new(table);
    let mut lsts: Vec<Lst<'a>> = Vec::new();
    let mut lfos: Vec<Lfo<'a>> = Vec::new();
    let (lst_fc, lst_lcb) = (u64::from(lst.fc), u64::from(lst.lcb));
    if lst_lcb != 0 && lst_fc + lst_lcb <= r.len() {
        let c_lst = r.i16(lst_fc);
        let mut o = lst_fc + 2;
        for _ in 0..c_lst.max(0) {
            let mut rgistd = [0u16; 9];
            for (k, v) in rgistd.iter_mut().enumerate() {
                *v = r.u16(o + 8 + k as u64 * 2);
            }
            let flags = r.u8(o + 26);
            lsts.push(Lst {
                lsid: r.i32(o),
                tplc: r.i32(o + 4),
                rgistd_para: rgistd,
                simple: flags & 1 != 0,
                hybrid: flags & 0x10 != 0,
                levels: Vec::new(),
            });
            o += 28;
        }
        // the LVLs follow the PlfLst (outside the declared lcb)
        let mut lo = lst_fc + lst_lcb;
        for l in &mut lsts {
            let n = if l.simple { 1 } else { 9 };
            let mut k = 0;
            while k < n && lo + 28 <= r.len() {
                let (lvl, next) = read_lvl(r, lo, r.len());
                l.levels.push(lvl);
                lo = next;
                k += 1;
            }
        }
    }
    let (lfo_fc, lfo_lcb) = (u64::from(lfo.fc), u64::from(lfo.lcb));
    if lfo_lcb != 0 && lfo_fc + lfo_lcb <= r.len() {
        let end = lfo_fc + lfo_lcb;
        let mac = u64::from(r.u32(lfo_fc));
        let mut o = lfo_fc + 4;
        let mut counts: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < mac && o + 16 <= end {
            lfos.push(Lfo { lsid: r.i32(o), overrides: Vec::new() });
            counts.push(r.u8(o + 12));
            o += 16;
            i += 1;
        }
        // LFOData: cp (u32) + LFOLVL[clfolvl]
        for (i, lfo) in lfos.iter_mut().enumerate() {
            if o + 4 > end {
                break;
            }
            o += 4;
            for _ in 0..counts[i] {
                let i_start_at = r.i32(o);
                let bits = r.u32(o + 4);
                o += 8;
                let mut ov = LfoLvl { ilvl: (bits & 0xf) as u8, start_at: None, lvl: None };
                let f_start_at = bits & 0x10 != 0;
                let f_formatting = bits & 0x20 != 0;
                if f_start_at && !f_formatting {
                    ov.start_at = Some(i_start_at);
                }
                if f_formatting {
                    let (lvl, next) = read_lvl(r, o, end);
                    if f_start_at {
                        ov.start_at = Some(lvl.start);
                    }
                    ov.lvl = Some(lvl);
                    o = next;
                }
                lfo.overrides.push(ov);
            }
        }
    }
    Lists { lsts, lfos }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An LVL: start, nfc, flags, markers, follow, the two grpprls and the text.
    fn lvl(start: i32, nfc: u8, flags: u8, nums: &[u8], papx: &[u8], chpx: &[u8], text: &[u16]) -> Vec<u8> {
        let mut b = vec![0u8; 28];
        b[0..4].copy_from_slice(&start.to_le_bytes());
        b[4] = nfc;
        b[5] = flags;
        b[6..6 + nums.len()].copy_from_slice(nums);
        b[15] = 1;
        b[24] = u8::try_from(chpx.len()).unwrap();
        b[25] = u8::try_from(papx.len()).unwrap();
        b[26] = 3;
        b.extend(papx);
        b.extend(chpx);
        b.extend(u16::try_from(text.len()).unwrap().to_le_bytes());
        for u in text {
            b.extend(u.to_le_bytes());
        }
        b
    }

    fn lstf(lsid: i32, simple: bool) -> Vec<u8> {
        let mut b = vec![0u8; 28];
        b[0..4].copy_from_slice(&lsid.to_le_bytes());
        b[4..8].copy_from_slice(&7i32.to_le_bytes());
        for k in 0..9 {
            b[8 + k * 2..10 + k * 2].copy_from_slice(&0x0fffu16.to_le_bytes());
        }
        b[8..10].copy_from_slice(&3u16.to_le_bytes());
        b[26] = if simple { 1 } else { 0x10 };
        b
    }

    #[test]
    fn reads_lists_levels_and_overrides() {
        let mut t = Vec::new();
        t.extend(2i16.to_le_bytes());
        t.extend(lstf(100, true));
        t.extend(lstf(200, false));
        let lst_lcb = t.len() as u32;
        t.extend(lvl(1, 0, 0x85, &[1], &[0x35, 0x08, 1], &[], &[0, 0x2e]));
        for k in 0..9 {
            t.extend(lvl(k, 23, 0, &[], &[], &[0x35, 0x08, 1], &[0xf0b7]));
        }
        let lfo_fc = t.len() as u32;
        t.extend(2u32.to_le_bytes());
        for (lsid, n) in [(100i32, 1u8), (200, 2)] {
            t.extend(lsid.to_le_bytes());
            t.extend([0u8; 8]);
            t.push(n);
            t.extend([0u8; 3]);
        }
        // LFOData 1: one override with a start-at only; LFOData 2: a formatted override with start, and a plain one
        t.extend([0u8; 4]);
        t.extend(5i32.to_le_bytes());
        t.extend((0x10u32 | 2).to_le_bytes());
        t.extend([0u8; 4]);
        t.extend(0i32.to_le_bytes());
        t.extend((0x30u32 | 1).to_le_bytes());
        t.extend(lvl(9, 1, 0, &[1], &[], &[], &[0, 0x29]));
        t.extend(0i32.to_le_bytes());
        t.extend(4u32.to_le_bytes());
        let lfo_lcb = t.len() as u32 - lfo_fc;
        let l = read_lists_at(FcLcb { fc: 0, lcb: lst_lcb }, FcLcb { fc: lfo_fc, lcb: lfo_lcb }, &t);
        assert_eq!(l.lsts.len(), 2);
        assert_eq!((l.lsts[0].lsid, l.lsts[0].tplc, l.lsts[0].simple, l.lsts[0].levels.len()), (100, 7, true, 1));
        assert_eq!((l.lsts[1].hybrid, l.lsts[1].levels.len(), l.lsts[1].rgistd_para[0], l.lsts[1].rgistd_para[1]), (true, 9, 3, 0xfff));
        let a = &l.lsts[0].levels[0];
        assert_eq!((a.start, a.nfc, a.jc, a.legal, a.tentative, a.follow, a.ilvl_restart_lim), (1, 0, 1, true, true, 1, 3));
        assert_eq!((a.rgbxch_nums.clone(), a.grpprl_papx, a.grpprl_chpx.len()), (vec![1], &[0x35u8, 0x08, 1][..], 0));
        assert!(a.xst.eq_str("\u{0}."));
        assert_eq!(l.lfos.len(), 2);
        assert_eq!(l.lfos[0].overrides, vec![LfoLvl { ilvl: 2, start_at: Some(5), lvl: None }]);
        let o = &l.lfos[1].overrides;
        assert_eq!((o.len(), o[0].ilvl, o[0].start_at, o[0].lvl.as_ref().map(|v| v.start)), (2, 1, Some(9), Some(9)));
        assert_eq!((o[1].ilvl, o[1].start_at, o[1].lvl.is_none()), (4, None, true));
    }

    #[test]
    fn absent_or_out_of_range_tables_are_empty() {
        let t = [0u8; 16];
        assert_eq!(read_lists_at(FcLcb::default(), FcLcb::default(), &t), Lists::default());
        assert_eq!(read_lists_at(FcLcb { fc: 8, lcb: 100 }, FcLcb { fc: 0, lcb: 100 }, &t), Lists::default());
        // a negative count makes no list; a count past the table makes zero lists
        let mut t = vec![0u8; 64];
        t[0..2].copy_from_slice(&(-3i16).to_le_bytes());
        assert!(read_lists_at(FcLcb { fc: 0, lcb: 2 }, FcLcb::default(), &t).lsts.is_empty());
    }

    #[test]
    fn counts_far_past_the_table_stay_bounded() {
        let mut t = vec![0u8; 256];
        t[0..2].copy_from_slice(&i16::MAX.to_le_bytes());
        let l = read_lists_at(FcLcb { fc: 0, lcb: 4 }, FcLcb::default(), &t);
        assert_eq!(l.lsts.len(), 32767);
        // each list of zeros reads 9 levels while there are 28 bytes left
        assert!(l.lsts.iter().map(|l| l.levels.len()).sum::<usize>() <= 256 / 28 + 9);
        let mut t = vec![0u8; 64];
        t[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        let l = read_lists_at(FcLcb::default(), FcLcb { fc: 0, lcb: 64 }, &t);
        assert_eq!(l.lfos.len(), 3);
    }
}
