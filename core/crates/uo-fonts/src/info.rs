//! What the embedding formats need from a font file: the licence, the outline kind, the OS/2 and
//! head values they repeat and the names, read with the first-match `tableOf`.

use crate::error::Result;
use crate::limits::{FontLimits, WorkBudget};
use crate::read::{Tag, View};
use crate::sfnt::{TableRecord, table_of};

/// The names of the `name` table (platform 3, the en-US record first, else the first non-empty
/// one) as UTF-16 code units: lone surrogates survive, an odd length drops the last byte.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FaceNames {
    pub family: Vec<u16>,
    pub style: Vec<u16>,
    pub version: Vec<u16>,
    pub full: Vec<u16>,
}

/// OS/2 and head data the embedding formats repeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceInfo {
    /// PANOSE in upper-case hex: 20 characters, fewer when the table is cut by the end of the
    /// input (S10), `"0"` x 20 without a 42-byte OS/2
    pub panose: String,
    pub weight: u16,
    pub italic: bool,
    pub fs_type: u16,
    pub unicode_range: [u32; 4],
    pub code_page_range: [u32; 2],
    pub check_sum_adjustment: u32,
    pub names: FaceNames,
}

/// The licence allows embedding: OS/2 `fsType` 0x2 alone is restricted, 0x200 is bitmap-only; no
/// readable OS/2 allows (a table directory cut short or an `fsType` past the end included).
pub fn is_embeddable(bytes: &[u8]) -> bool {
    let view = View::new(bytes);
    let Ok(Some(os2)) = table_of(view, 0, Tag::OS2) else {
        return true;
    };
    if os2.length < 10 {
        return true;
    }
    let Ok(fs_type) = view.u16(u64::from(os2.offset) + 8) else {
        return true;
    };
    !((fs_type & 0x2) != 0 && (fs_type & 0xC) == 0) && (fs_type & 0x200) == 0
}

/// The font has `glyf` and `loca` (the only outlines the subsetter cuts); false when unreadable.
pub fn has_true_type_outlines(bytes: &[u8]) -> bool {
    let view = View::new(bytes);
    matches!(table_of(view, 0, Tag::GLYF), Ok(Some(_))) && matches!(table_of(view, 0, Tag::LOCA), Ok(Some(_)))
}

/// The OS/2, head and name values of a font; a malformed table is an `Err` that fails the
/// embedding plan, as the uncaught TS throw does. The name strings share one work budget of
/// [`FontLimits::DEFAULT`] (one unit per name record and per character read), so a table of
/// records that all point at one long string cannot take unbounded time (`Limit(TableWork)`).
pub fn face_info(bytes: &[u8]) -> Result<FaceInfo> {
    let view = View::new(bytes);
    let os2 = table_of(view, 0, Tag::OS2)?;
    let head = table_of(view, 0, Tag::HEAD)?;
    let u32_at = |t: Option<TableRecord>, o: u64| -> Result<u32> {
        match t {
            Some(t) if u64::from(t.length) >= o + 4 => Ok(view.u32(u64::from(t.offset) + o)?),
            _ => Ok(0),
        }
    };
    let panose = match os2 {
        Some(t) if t.length >= 42 => {
            let start = u64::from(t.offset) + 32;
            view.subarray(start, start + 10).iter().map(|b| format!("{b:02X}")).collect()
        }
        _ => "0".repeat(20),
    };
    let (weight, italic, fs_type) = match os2 {
        Some(t) => {
            let o = u64::from(t.offset);
            let weight = view.u16(o + 4)?;
            let fs_selection = view.u16(o + 62)?;
            let fs_type = view.u16(o + 8)?;
            (if weight == 0 { 400 } else { weight }, fs_selection & 1 == 1, fs_type)
        }
        None => (400, false, 0),
    };
    let unicode_range = [u32_at(os2, 42)?, u32_at(os2, 46)?, u32_at(os2, 50)?, u32_at(os2, 54)?];
    let code_page_range = match os2 {
        Some(t) if view.u16(u64::from(t.offset))? >= 1 => [u32_at(os2, 78)?, u32_at(os2, 82)?],
        _ => [0, 0],
    };
    let check_sum_adjustment = u32_at(head, 8)?;
    let mut budget = FontLimits::DEFAULT.work();
    let names = FaceNames {
        family: name_string(view, 1, &mut budget)?,
        style: name_string(view, 2, &mut budget)?,
        version: name_string(view, 5, &mut budget)?,
        full: name_string(view, 4, &mut budget)?,
    };
    Ok(FaceInfo { panose, weight, italic, fs_type, unicode_range, code_page_range, check_sum_adjustment, names })
}

/// `nameString`: the platform-3 record of the name id, en-US (0x409) first, else the first
/// non-empty one; UTF-16BE code units, an odd length drops the last byte. Every record is read
/// until the en-US one is found, so a read past the end anywhere fails the call.
fn name_string(view: View<'_>, id: u16, budget: &mut WorkBudget) -> Result<Vec<u16>> {
    let Some(name) = table_of(view, 0, Tag::NAME)? else {
        return Ok(Vec::new());
    };
    let off = u64::from(name.offset);
    let count = view.u16(off + 2)?;
    let str_off = off + u64::from(view.u16(off + 4)?);
    let mut any: Vec<u16> = Vec::new();
    for i in 0..u64::from(count) {
        budget.tick()?;
        let r = off + 6 + i * 12;
        if view.u16(r)? != 3 || view.u16(r + 6)? != id {
            continue;
        }
        let len = u64::from(view.u16(r + 8)?);
        let o = str_off + u64::from(view.u16(r + 10)?);
        let mut s: Vec<u16> = Vec::new();
        let mut k = 0;
        while k + 1 < len {
            budget.tick()?;
            s.push(view.u16(o + k)?);
            k += 2;
        }
        if view.u16(r + 4)? == 0x409 {
            return Ok(s);
        }
        if any.is_empty() {
            any = s;
        }
    }
    Ok(any)
}

/// `postscriptName`: the name id 6 of the face that starts at `base`, platforms 3 and 0 as UTF-16
/// code units, the others as Latin-1 bytes; the first non-empty one. The directory scan, every
/// name record and every character count against `budget` (R-BUDGET).
pub(crate) fn postscript_name(view: View<'_>, base: u64, budget: &mut WorkBudget) -> Result<Option<Vec<u16>>> {
    let num = u64::from(view.u16(base + 4)?);
    let mut name: Option<TableRecord> = None;
    for i in 0..num {
        budget.tick()?;
        let p = base + 12 + i * 16;
        if view.tag(p)? == Tag::NAME {
            name = Some(TableRecord { tag: Tag::NAME, offset: view.u32(p + 8)?, length: view.u32(p + 12)? });
            break;
        }
    }
    let Some(name) = name else {
        return Ok(None);
    };
    let off = u64::from(name.offset);
    let count = view.u16(off + 2)?;
    let str_off = off + u64::from(view.u16(off + 4)?);
    for i in 0..u64::from(count) {
        budget.tick()?;
        let r = off + 6 + i * 12;
        if view.u16(r + 6)? != 6 {
            continue;
        }
        let platform = view.u16(r)?;
        let len = u64::from(view.u16(r + 8)?);
        let o = str_off + u64::from(view.u16(r + 10)?);
        let mut s: Vec<u16> = Vec::new();
        if platform == 3 || platform == 0 {
            let mut k = 0;
            while k + 1 < len {
                budget.tick()?;
                s.push(view.u16(o + k)?);
                k += 2;
            }
        } else {
            for k in 0..len {
                budget.tick()?;
                s.push(u16::from(view.u8(o + k)?));
            }
        }
        if !s.is_empty() {
            return Ok(Some(s));
        }
    }
    Ok(None)
}
