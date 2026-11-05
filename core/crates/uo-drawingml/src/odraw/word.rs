//! Word's drawings ([MS-DOC] 2.9.172 `OfficeArtContent`): the `DggContainer` with the BLIP
//! store, then `dgglbl` + `DgContainer` pairs (main document 0, headers 1) at `fcDggInfo` of the
//! table stream (`DocReader.loadDrawings` of `docx-core/src/doc/reader.ts`).

use std::collections::BTreeSet;

use super::OdrawLimits;
use super::bstore::Bse;
use super::record::{read_rec, rt, u8_at};
use super::shape::{Shape, ShapeMap, apply_masters, fsp, index_drawing_counted};

/// The drawings of a Word document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordDrawings<'a> {
    /// the BLIP store (`pib - 1`); the last `BStoreContainer` of the `DggContainer`
    pub bstore: Vec<Option<Bse<'a>>>,
    /// top-level shapes of the main document's drawing, by spid
    pub main: ShapeMap<'a>,
    /// top-level shapes of the headers' drawing, by spid
    pub hdr: ShapeMap<'a>,
    /// spids of master shapes (`hspMaster` of another shape): Word does not draw them
    pub masters: BTreeSet<u32>,
}

impl<'a> WordDrawings<'a> {
    /// The shape of a story (`DocReader.shape`): the headers' drawing first for a header story,
    /// then the other one.
    pub fn shape(&self, spid: u32, hdr: bool) -> Option<&Shape<'a>> {
        let (first, second) = if hdr { (&self.hdr, &self.main) } else { (&self.main, &self.hdr) };
        first.get(spid).or_else(|| second.get(spid))
    }

    /// The page background (`backgroundShape`): the first main shape with `fBackground`.
    pub fn background_shape(&self) -> Option<&Shape<'a>> {
        self.main.iter().find(|s| s.has_flag(fsp::BACKGROUND))
    }

    /// The BLIP store entry of a 1-based `pib` (`bse`).
    pub fn bse(&self, pib: u32) -> Option<&Bse<'a>> {
        let i = usize::try_from(pib.checked_sub(1)?).ok()?;
        self.bstore.get(i).and_then(Option::as_ref)
    }
}

/// Reads `OfficeArtContent` at `[fc, fc + lcb)` of the table stream (`loadDrawings`): nothing
/// when `lcb` is 0, the range leaves the stream or the first record is not a `DggContainer`.
/// At most `limits.max_word_drawings` drawings and `limits.max_shapes` shapes in all are read;
/// masters are applied to the main drawing, then to the headers'.
pub fn read_word_drawings<'a>(table: &'a [u8], fc: u32, lcb: u32, limits: &OdrawLimits) -> WordDrawings<'a> {
    let mut out = WordDrawings::default();
    let (fc, end) = (u64::from(fc), u64::from(fc) + u64::from(lcb));
    if lcb == 0 || end > table.len() as u64 {
        return out;
    }
    let Some(dgg) = read_rec(table, fc, end).filter(|r| r.rec_type == rt::DGG_CONTAINER) else { return out };
    for c in dgg.children() {
        if c.rec_type == rt::BSTORE_CONTAINER {
            out.bstore = super::bstore::read_bstore(&c);
        }
    }
    // OfficeArtWordDrawing: dgglbl (u8) + OfficeArtDgContainer, from the declared end of the Dgg
    let mut o = dgg.start + u64::from(dgg.len);
    let mut guard = 0;
    let mut left = limits.max_shapes;
    while o + 9 <= end && guard < limits.max_word_drawings {
        guard += 1;
        let lbl = u8_at(table, o);
        let Some(dg) = read_rec(table, o + 1, end).filter(|r| r.rec_type == rt::DG_CONTAINER) else { break };
        index_drawing_counted(&dg, if lbl == 1 { &mut out.hdr } else { &mut out.main }, limits, &mut left);
        o = dg.start + u64::from(dg.len);
    }
    apply_masters(&mut out.main, &mut out.masters, limits);
    apply_masters(&mut out.hdr, &mut out.masters, limits);
    out
}
