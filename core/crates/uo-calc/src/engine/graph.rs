//! The formula index (`formulaCols`, `engine.ts:206`), the sheet order of formulas (`allFormulas`), and the
//! reverse dependency index (`rev`, `engine.ts:866-893`) as per-column interval lists bucketed by length.

use std::collections::HashMap;

use super::{Graph, NONE, Rec, fl};
use crate::key::Area;

/// The formulas of one column: the CURRENT formula of each row (an orphaned duplicate is not here), and the dirty
/// count of every formula of the column, orphans included (`dirtyCount`).
#[derive(Clone, Debug, Default)]
pub(crate) struct FCol {
    pub col: u32,
    pub dirty: u32,
    pub rows: Vec<u32>,
    pub fids: Vec<u32>,
}

/// One entry of the reverse index: rows `r1..=r2` of a column are read by formula `fid`.
type RevEnt = (u32, u32, u32);

/// Length classes: an interval of `len` rows is in class `⌈log2 len⌉` (0..=20).
const CLASSES: usize = 21;

#[derive(Clone, Debug, Default)]
pub(crate) struct ColRev {
    cls: [Vec<RevEnt>; CLASSES],
}

/// The reverse index of one sheet (`rev[sheet]`): narrow areas (< 64 columns) per column, wide ones in a list.
#[derive(Clone, Debug, Default)]
pub(crate) struct RevSheet {
    cols: HashMap<u32, Box<ColRev>>,
    wide: Vec<(Area, u32)>,
}

fn class_of(r1: u32, r2: u32) -> usize {
    let len = r2 - r1 + 1;
    (32 - (len - 1).leading_zeros()) as usize
}

impl RevSheet {
    pub(crate) fn insert(&mut self, d: Area, fid: u32, sorted: bool) {
        if d.c2 - d.c1 < 64 {
            let k = class_of(d.r1, d.r2);
            for c in d.c1..=d.c2 {
                let v = &mut self.cols.entry(c).or_default().cls[k];
                if sorted {
                    let i = v.partition_point(|e| e.0 <= d.r1);
                    v.insert(i, (d.r1, d.r2, fid));
                } else {
                    v.push((d.r1, d.r2, fid));
                }
            }
        } else {
            self.wide.push((d, fid));
        }
    }

    pub(crate) fn remove(&mut self, d: Area, fid: u32) {
        if d.c2 - d.c1 < 64 {
            let k = class_of(d.r1, d.r2);
            for c in d.c1..=d.c2 {
                let Some(cr) = self.cols.get_mut(&c) else { continue };
                let v = &mut cr.cls[k];
                let mut i = v.partition_point(|e| e.0 < d.r1);
                while i < v.len() && v[i].0 == d.r1 {
                    if v[i].1 == d.r2 && v[i].2 == fid {
                        v.remove(i);
                        break;
                    }
                    i += 1;
                }
            }
        } else if let Some(i) = self.wide.iter().position(|&(a, f)| a == d && f == fid) {
            self.wide.remove(i);
        }
    }

    pub(crate) fn sort(&mut self) {
        for cr in self.cols.values_mut() {
            for v in &mut cr.cls {
                v.sort_unstable();
            }
        }
    }

    /// Formulas whose static precedents overlap `a` (with repetitions; the caller dedupes).
    pub(crate) fn query(&self, a: Area, out: &mut Vec<u32>) {
        let mut col = |cr: &ColRev| {
            for (k, v) in cr.cls.iter().enumerate() {
                if v.is_empty() {
                    continue;
                }
                let span = 1u32 << k;
                let lo = a.r1.saturating_sub(span - 1);
                let mut i = v.partition_point(|e| e.0 < lo);
                while i < v.len() && v[i].0 <= a.r2 {
                    if v[i].1 >= a.r1 {
                        out.push(v[i].2);
                    }
                    i += 1;
                }
            }
        };
        let w = u64::from(a.c2 - a.c1) + 1;
        if w <= self.cols.len() as u64 {
            for c in a.c1..=a.c2 {
                if let Some(cr) = self.cols.get(&c) {
                    col(cr);
                }
            }
        } else {
            for (&c, cr) in &self.cols {
                if c >= a.c1 && c <= a.c2 {
                    col(cr);
                }
            }
        }
        for &(d, f) in &self.wide {
            if crate::a1::areas_overlap(d, a) {
                out.push(f);
            }
        }
    }

    fn heap_bytes(&self) -> usize {
        let cols: usize =
            self.cols.values().map(|cr| core::mem::size_of::<ColRev>() + cr.cls.iter().map(|v| v.capacity() * 12).sum::<usize>()).sum();
        cols + self.cols.capacity() * 16 + self.wide.capacity() * 24
    }
}

/// First index of `rows` (ascending) not below `r`: probed at `r - rows[0]` first (a column filled downwards has one
/// formula per row, so the probe hits), else a binary search.
#[inline]
pub(crate) fn lower(rows: &[u32], r: u32) -> usize {
    if let Some(&first) = rows.first() {
        let g = r.wrapping_sub(first) as usize;
        if g < rows.len() && rows[g] >= r && (g == 0 || rows[g - 1] < r) {
            return g;
        }
        if r <= first {
            return 0;
        }
    }
    rows.partition_point(|&x| x < r)
}

impl Graph {
    /// The current formula of a cell (`st.get(r, c)?.f`).
    #[inline]
    pub(crate) fn current(&self, s: u32, r: u32, c: u32) -> Option<u32> {
        let sg = self.sheets.get(s as usize)?;
        let slot = *sg.col_slot.get(c as usize)?;
        if slot == NONE {
            return None;
        }
        let fc = &sg.fcols[slot as usize];
        let i = lower(&fc.rows, r);
        (fc.rows.get(i) == Some(&r)).then(|| fc.fids[i])
    }

    fn fcol_mut(&mut self, s: u32, c: u32) -> &mut FCol {
        let sg = &mut self.sheets[s as usize];
        if sg.col_slot.len() <= c as usize {
            sg.col_slot.resize(c as usize + 1, NONE);
        }
        if sg.col_slot[c as usize] == NONE {
            sg.col_slot[c as usize] = sg.fcols.len() as u32;
            sg.fcols.push(FCol { col: c, ..FCol::default() });
        }
        let slot = sg.col_slot[c as usize] as usize;
        &mut sg.fcols[slot]
    }

    /// The current formula of a cell stops being current (`st.set`/`st.delete` replaced its `CellRec`): it stays in
    /// `formulas` as an orphan.
    pub(crate) fn unset_current(&mut self, s: u32, r: u32, c: u32) {
        let Some(sg) = self.sheets.get_mut(s as usize) else { return };
        let Some(&slot) = sg.col_slot.get(c as usize) else { return };
        if slot == NONE {
            return;
        }
        let fc = &mut sg.fcols[slot as usize];
        if let Ok(i) = fc.rows.binary_search(&r) {
            fc.rows.remove(i);
            fc.fids.remove(i);
        }
    }

    /// `addFormula` + `st.set(r, c, { f })`: a new record, current at its cell.
    pub(crate) fn add_formula(&mut self, s: u32, r: u32, c: u32, node: u32, flags: u16) -> u32 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let rec = Rec { sheet: s, row: r, col: c as u16, flags: flags | fl::LIVE, node, scan: u32::MAX, seen: 0, seq, tpl: NONE };
        let fid = match self.free.pop() {
            Some(i) => {
                self.recs[i as usize] = rec;
                i
            }
            None => {
                self.recs.push(rec);
                self.recs.len() as u32 - 1
            }
        };
        self.live += 1;
        if flags & fl::VOLATILE != 0 {
            self.volatile_count += 1;
        }
        let fc = self.fcol_mut(s, c);
        match fc.rows.binary_search(&r) {
            Ok(i) => fc.fids[i] = fid,
            Err(i) => {
                fc.rows.insert(i, r);
                fc.fids.insert(i, fid);
            }
        }
        self.note_row(s, r);
        self.ordered_ok = false;
        fid
    }

    /// Frees a record (`removeFormula` after the caller removed its reverse entries).
    pub(crate) fn free_formula(&mut self, fid: u32) {
        let r = self.recs[fid as usize];
        if !r.has(fl::LIVE) {
            return;
        }
        self.set_dirty(fid, false);
        self.remove_master(fid);
        if self.current(r.sheet, r.row, u32::from(r.col)) == Some(fid) {
            self.unset_current(r.sheet, r.row, u32::from(r.col));
        }
        if r.has(fl::CIRC)
            && let Some(i) = self.circular.iter().position(|&x| x == fid)
        {
            self.circular.remove(i);
        }
        if r.has(fl::VOLATILE) {
            self.volatile_count -= 1;
        }
        self.areas.remove(&fid);
        self.cached.remove(&fid);
        self.recs[fid as usize].flags = 0;
        self.free.push(fid);
        self.live -= 1;
        self.ordered_ok = false;
    }

    /// The sort key of `allFormulas` (sheet, row, col, then load order).
    pub(crate) fn order_key(&self, fid: u32) -> (u32, u32, u16, u32) {
        let r = &self.recs[fid as usize];
        (r.sheet, r.row, r.col, r.seq)
    }

    /// `allFormulas()`: every live formula in sheet order.
    pub(crate) fn ensure_ordered(&mut self) {
        if self.ordered_ok {
            return;
        }
        let mut v = core::mem::take(&mut self.ordered);
        v.clear();
        v.extend((0..self.recs.len() as u32).filter(|&i| self.recs[i as usize].has(fl::LIVE)));
        v.sort_unstable_by_key(|&f| self.order_key(f));
        self.ordered = v;
        self.ordered_ok = true;
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        let sheets: usize = self
            .sheets
            .iter()
            .map(|s| {
                s.col_slot.capacity() * 4
                    + s.fcols.capacity() * core::mem::size_of::<FCol>()
                    + s.fcols.iter().map(|f| f.rows.capacity() * 4 + f.fids.capacity() * 4).sum::<usize>()
                    + s.owners.capacity() * 13
            })
            .sum();
        let rev = self.rev.as_ref().map_or(0, |r| r.iter().map(RevSheet::heap_bytes).sum());
        sheets
            + rev
            + self.recs.capacity() * core::mem::size_of::<Rec>()
            + self.free.capacity() * 4
            + self.masters.capacity() * 4
            + self.areas.capacity() * 28
            + self.ordered.capacity() * 4
            + self.dirty_list.capacity() * 4
            + self.tpl.capacity() * 16
            + self.tpls.capacity() * 8
            + self.tpl_refs.capacity() * core::mem::size_of::<super::TplRef>()
            + self.analysis.capacity() * 12
            + self.consed.capacity() * 32
            + self.consed.values().map(|v| v.capacity() * 8).sum::<usize>()
    }
}
