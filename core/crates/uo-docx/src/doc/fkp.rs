//! Direct formatting ([MS-DOC] 2.4.6, `fkp.ts`): `PlcBteChpx`/`PlcBtePapx` (table stream) point at
//! 512-byte FKP pages of `WordDocument`. Each FKP lists FC ranges (`rgfc`) and, per range, a
//! CHPX (a character grpprl) or a PAPX (`istd` + a paragraph grpprl). Everything becomes one flat
//! list of FC ranges, sorted, for binary search.

use std::collections::HashSet;

use super::bytes::ByteReader;
use super::fib::{Fib, fc};
use super::sprm::prls;

/// A range of FC with its grpprl (borrowed from the `WordDocument` or `Data` stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FcRun<'a> {
    pub fc_start: u32,
    pub fc_end: u32,
    /// the grpprl, without the `istd`
    pub grpprl: &'a [u8],
    /// PAPX: the paragraph style
    pub istd: u16,
}

/// A generic PLC: `n + 1` u32 CPs and `n` structures of `cbData` bytes.
#[derive(Debug, Clone)]
pub struct Plc<'a> {
    pub cps: Vec<u32>,
    pub n: u64,
    pub r: ByteReader<'a>,
    base: u64,
    cb_data: u64,
}

impl Plc<'_> {
    /// The offset (in the buffer) of the i-th structure.
    #[must_use]
    pub fn data_at(&self, i: u64) -> u64 {
        self.base + i * self.cb_data
    }
}

/// `readPlc`: `None` when `lcb` is 0 or the PLC does not fit in the buffer.
#[must_use]
pub fn read_plc(buf: &[u8], fc0: u32, lcb: u32, cb_data: u64) -> Option<Plc<'_>> {
    let (fc0, lcb) = (u64::from(fc0), u64::from(lcb));
    let r = ByteReader::new(buf);
    if lcb == 0 || fc0 + lcb > r.len() {
        return None;
    }
    // Math.floor((lcb - 4) / (4 + cbData)) and Math.floor(lcb / 4) - 1: floors of possibly negative numbers
    let n = if cb_data > 0 { (lcb as i64 - 4).div_euclid(4 + cb_data as i64) } else { (lcb / 4) as i64 - 1 };
    let n = u64::try_from(n).ok()?;
    let cps = (0..=n).map(|i| r.u32(fc0 + i * 4)).collect();
    let base = fc0 + (n + 1) * 4;
    Some(Plc { cps, n, r, base, cb_data })
}

fn sort_runs(runs: &mut [FcRun<'_>]) {
    // a stable sort by `fcStart`, as Array.prototype.sort with `a.fcStart - b.fcStart`
    runs.sort_by_key(|r| r.fc_start);
}

/// `readChpxRuns`: the character runs of every CHPX FKP, sorted by FC.
#[must_use]
pub fn read_chpx_runs<'a>(fib: &Fib, table: &[u8], word: &'a [u8]) -> Vec<FcRun<'a>> {
    let p = fib.fc_lcb(fc::PlcfBteChpx);
    let Some(plc) = read_plc(table, p.fc, p.lcb, 4) else { return Vec::new() };
    let w = ByteReader::new(word);
    let mut runs = Vec::new();
    let mut seen = HashSet::new();
    for i in 0..plc.n {
        let pn = plc.r.u32(plc.data_at(i)) & 0x3f_ffff;
        if !seen.insert(pn) {
            continue;
        }
        let page = u64::from(pn) * 512;
        if page + 512 > w.len() {
            continue;
        }
        let crun = u64::from(w.u8(page + 511));
        for k in 0..crun {
            let fc_start = w.u32(page + k * 4);
            let fc_end = w.u32(page + (k + 1) * 4);
            let b = u64::from(w.u8(page + (crun + 1) * 4 + k));
            let mut grpprl: &[u8] = &[];
            if b != 0 {
                let at = page + b * 2;
                let cb = u64::from(w.u8(at));
                grpprl = w.sub(at + 1, (at + 1 + cb).min(page + 511));
            }
            if fc_end > fc_start {
                runs.push(FcRun { fc_start, fc_end, grpprl, istd: 0 });
            }
        }
    }
    sort_runs(&mut runs);
    runs
}

/// `prcData`: `cbGrpprl` (i16) + the grpprl at an offset of the Data stream.
fn prc_data(data: Option<&[u8]>, off: u64) -> &[u8] {
    let Some(data) = data else { return &[] };
    if off + 2 > data.len() as u64 {
        return &[];
    }
    let r = ByteReader::new(data);
    let cb = u64::from(r.i16(off).max(0).unsigned_abs());
    r.sub(off + 2, off + 2 + cb)
}

/// `expandHugePapx`: `sprmPHugePapx` (the real grpprl lives in the Data stream); only the first
/// `Prl` of the grpprl is looked at.
#[must_use]
pub fn expand_huge_papx<'a>(grpprl: &'a [u8], data: Option<&'a [u8]>) -> &'a [u8] {
    if let Some(p) = prls(grpprl).next()
        && p.sprm == 0x6646
        && data.is_some()
    {
        let huge = prc_data(data, u64::from(p.r.u32(p.op)));
        return if huge.is_empty() { grpprl } else { huge };
    }
    grpprl
}

/// `readPapxRuns`: the paragraph runs of every PAPX FKP, sorted by FC.
#[must_use]
pub fn read_papx_runs<'a>(fib: &Fib, table: &[u8], word: &'a [u8], data: Option<&'a [u8]>) -> Vec<FcRun<'a>> {
    let p = fib.fc_lcb(fc::PlcfBtePapx);
    let Some(plc) = read_plc(table, p.fc, p.lcb, 4) else { return Vec::new() };
    let w = ByteReader::new(word);
    let mut runs = Vec::new();
    let mut seen = HashSet::new();
    for i in 0..plc.n {
        let pn = plc.r.u32(plc.data_at(i)) & 0x3f_ffff;
        if !seen.insert(pn) {
            continue;
        }
        let page = u64::from(pn) * 512;
        if page + 512 > w.len() {
            continue;
        }
        let cpara = u64::from(w.u8(page + 511));
        for k in 0..cpara {
            let fc_start = w.u32(page + k * 4);
            let fc_end = w.u32(page + (k + 1) * 4);
            let b_offset = u64::from(w.u8(page + (cpara + 1) * 4 + k * 13));
            let mut istd = 0;
            let mut grpprl: &[u8] = &[];
            if b_offset != 0 {
                let at = page + b_offset * 2;
                let mut cb = u64::from(w.u8(at));
                let mut start = at + 1;
                let size;
                if cb == 0 {
                    cb = u64::from(w.u8(at + 1));
                    start = at + 2;
                    size = cb * 2;
                } else {
                    size = cb * 2 - 1;
                }
                let end = (start + size).min(page + 511);
                istd = w.u16(start);
                grpprl = expand_huge_papx(w.sub(start + 2, end), data);
            }
            if fc_end > fc_start {
                runs.push(FcRun { fc_start, fc_end, grpprl, istd });
            }
        }
    }
    sort_runs(&mut runs);
    runs
}

/// `runAt`: the run that contains the FC (binary search); `fc` is a JS number (a fractional FC of
/// an odd compressed piece compares as is).
#[must_use]
pub fn run_at<'a, 'r>(runs: &'r [FcRun<'a>], fc: f64) -> Option<&'r FcRun<'a>> {
    let (mut lo, mut hi) = (0i64, runs.len() as i64 - 1);
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        let r = &runs[usize::try_from(mid).ok()?];
        if fc < f64::from(r.fc_start) {
            hi = mid - 1;
        } else if fc >= f64::from(r.fc_end) {
            lo = mid + 1;
        } else {
            return Some(r);
        }
    }
    None
}

/// `runIndexFrom`: the index of the first run with `fcEnd > fc`.
#[must_use]
pub fn run_index_from(runs: &[FcRun<'_>], fc: f64) -> usize {
    let (mut lo, mut hi) = (0usize, runs.len());
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if f64::from(runs[mid].fc_end) <= fc {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plc_counts() {
        // lcb 4 + 2 × (4 + 4): two entries
        let mut buf = vec![0u8; 24];
        buf[0..4].copy_from_slice(&10u32.to_le_bytes());
        buf[4..8].copy_from_slice(&20u32.to_le_bytes());
        buf[8..12].copy_from_slice(&30u32.to_le_bytes());
        buf[12..16].copy_from_slice(&7u32.to_le_bytes());
        let p = read_plc(&buf, 0, 20, 4).unwrap();
        assert_eq!((p.n, p.cps.clone()), (2, vec![10, 20, 30]));
        assert_eq!(p.r.u32(p.data_at(0)), 7);
        assert!(read_plc(&buf, 0, 0, 4).is_none());
        assert!(read_plc(&buf, 10, 20, 4).is_none());
        // a PLC of CPs only (cbData 0): lcb / 4 - 1 entries
        let q = read_plc(&buf, 0, 12, 0).unwrap();
        assert_eq!(q.n, 2);
        // too short for one entry: a negative count is no PLC
        assert!(read_plc(&buf, 0, 3, 4).is_none());
        assert_eq!(read_plc(&buf, 0, 4, 4).unwrap().n, 0);
    }

    /// A word stream with one CHPX page (page 1) and one PAPX page (page 2).
    fn word_with_pages() -> Vec<u8> {
        let mut w = vec![0u8; 512 * 3];
        // CHPX page: crun = 2: ranges [10,20), [20,30); offsets b = 100, 0
        let page = 512;
        w[page..page + 4].copy_from_slice(&10u32.to_le_bytes());
        w[page + 4..page + 8].copy_from_slice(&20u32.to_le_bytes());
        w[page + 8..page + 12].copy_from_slice(&30u32.to_le_bytes());
        w[page + 12] = 100;
        w[page + 13] = 0;
        w[page + 511] = 2;
        // the grpprl at b*2 = 200: cb 3, bytes 1 2 3
        w[page + 200] = 3;
        w[page + 201..page + 204].copy_from_slice(&[1, 2, 3]);
        // PAPX page: cpara = 1: [40, 50); BX at (cpara+1)*4 = 8: bOffset 60; at 120: cb 2 → size 3, istd 5, grpprl 9 9
        let page = 1024;
        w[page..page + 4].copy_from_slice(&40u32.to_le_bytes());
        w[page + 4..page + 8].copy_from_slice(&50u32.to_le_bytes());
        w[page + 8] = 60;
        w[page + 511] = 1;
        w[page + 120] = 2;
        w[page + 121..page + 123].copy_from_slice(&5u16.to_le_bytes());
        w[page + 123] = 9;
        w
    }

    fn plc_table(page: u32) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend(0u32.to_le_bytes());
        t.extend(100u32.to_le_bytes());
        t.extend(page.to_le_bytes());
        t
    }

    fn fib_with(chpx: (u32, u32), papx: (u32, u32)) -> Fib {
        let mut w = vec![0u8; 0x300];
        w[0..2].copy_from_slice(&0xa5ecu16.to_le_bytes());
        w[32..34].copy_from_slice(&14u16.to_le_bytes());
        w[62..64].copy_from_slice(&22u16.to_le_bytes());
        w[152..154].copy_from_slice(&93u16.to_le_bytes());
        let set = |w: &mut Vec<u8>, i: usize, fc: u32, lcb: u32| {
            w[154 + i * 8..158 + i * 8].copy_from_slice(&fc.to_le_bytes());
            w[158 + i * 8..162 + i * 8].copy_from_slice(&lcb.to_le_bytes());
        };
        set(&mut w, fc::PlcfBteChpx, chpx.0, chpx.1);
        set(&mut w, fc::PlcfBtePapx, papx.0, papx.1);
        crate::doc::fib::read_fib(&w).unwrap()
    }

    #[test]
    fn chpx_and_papx_runs() {
        let word = word_with_pages();
        let mut table = plc_table(1);
        let papx_at = table.len();
        table.extend(plc_table(2));
        let fib = fib_with((0, 12), (papx_at as u32, 12));
        let c = read_chpx_runs(&fib, &table, &word);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].fc_start, c[0].fc_end, c[0].grpprl), (10, 20, &[1u8, 2, 3][..]));
        assert_eq!((c[1].fc_start, c[1].grpprl.len()), (20, 0));
        let p = read_papx_runs(&fib, &table, &word, None);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].fc_start, p[0].fc_end, p[0].istd, p[0].grpprl), (40, 50, 5, &[9u8][..]));
        assert!(run_at(&c, 10.0).is_some());
        assert!(run_at(&c, 29.5).is_some());
        assert!(run_at(&c, 30.0).is_none());
        assert!(run_at(&c, 9.5).is_none());
        assert_eq!(run_index_from(&c, 20.0), 1);
        assert_eq!(run_index_from(&c, 0.0), 0);
        assert_eq!(run_index_from(&c, 99.0), 2);
    }

    #[test]
    fn a_page_past_the_stream_is_skipped_and_pages_are_read_once() {
        let word = word_with_pages();
        let mut table = Vec::new();
        for v in [0u32, 100, 200, 300] {
            table.extend(v.to_le_bytes());
        }
        table.extend(1u32.to_le_bytes());
        table.extend(1u32.to_le_bytes());
        table.extend(99u32.to_le_bytes());
        let fib = fib_with((0, 28), (0, 0));
        assert_eq!(read_chpx_runs(&fib, &table, &word).len(), 2);
    }

    #[test]
    fn huge_papx_comes_from_the_data_stream() {
        let mut data = vec![0u8; 16];
        data[4..6].copy_from_slice(&3i16.to_le_bytes());
        data[6..9].copy_from_slice(&[7, 8, 9]);
        let mut g = vec![0x46, 0x66];
        g.extend(4u32.to_le_bytes());
        assert_eq!(expand_huge_papx(&g, Some(&data)), &[7, 8, 9]);
        assert_eq!(expand_huge_papx(&g, None), &g[..]);
        // an empty or out-of-range PrcData keeps the grpprl
        let mut g2 = vec![0x46, 0x66];
        g2.extend(100u32.to_le_bytes());
        assert_eq!(expand_huge_papx(&g2, Some(&data)), &g2[..]);
        // only the first Prl counts
        let g3 = [0x35, 0x08, 1, 0x46, 0x66, 4, 0, 0, 0];
        assert_eq!(expand_huge_papx(&g3, Some(&data)), &g3[..]);
    }
}
