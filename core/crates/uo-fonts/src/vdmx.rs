//! Vertical device metrics: the `yMax` and `yMin` of a pixel size.

use rustc_hash::FxHashMap;

use crate::read::View;
use crate::sfnt::TableRecord;

/// The `yMax` and `yMin` of one `ppem`, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VdmxEntry {
    pub y_max: i16,
    pub y_min: i16,
}

/// The records of the chosen ratio by pixel size (`Map.set`: the last record of a size wins).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VdmxTable {
    pub entries: FxHashMap<u16, VdmxEntry>,
}

/// The first ratio group that serves a square device (the charset byte is ignored): the ratio
/// `x == 0` (all ratios) or `y0 <= x <= y1`. Its records are read whole before they are kept, so
/// any read past the end of the input anywhere in that group is `None` for the whole table (the
/// TS `catch`); a table with no such ratio is `None` too.
pub(crate) fn parse(view: View<'_>, table: TableRecord) -> Option<VdmxTable> {
    parse_inner(view, u64::from(table.offset))
}

fn parse_inner(view: View<'_>, o: u64) -> Option<VdmxTable> {
    let n_ratios = u64::from(view.u16(o + 4).ok()?);
    for i in 0..n_ratios {
        let r = o + 6 + i * 4;
        let x = view.u8(r + 1).ok()?;
        let y0 = view.u8(r + 2).ok()?;
        let y1 = view.u8(r + 3).ok()?;
        if !(x == 0 || (y0 <= x && x <= y1)) {
            continue;
        }
        let g = o + u64::from(view.u16(o + 6 + n_ratios * 4 + i * 2).ok()?);
        let records = u64::from(view.u16(g).ok()?);
        let mut entries: FxHashMap<u16, VdmxEntry> = FxHashMap::default();
        for k in 0..records {
            let e = g + 4 + k * 6;
            let ppem = view.u16(e).ok()?;
            let y_max = view.i16(e + 2).ok()?;
            let y_min = view.i16(e + 4).ok()?;
            entries.insert(ppem, VdmxEntry { y_max, y_min });
        }
        return Some(VdmxTable { entries });
    }
    None
}
