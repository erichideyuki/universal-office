//! Shape properties: the property tables (`OfficeArtFOPT`, `SecondaryFOPT`, `TertiaryFOPT`,
//! [MS-ODRAW] 2.2.9) read into an ordered table, `readFopt` of the three TS readers.
//!
//! The table keeps the iteration order of the TS `Map`: a new id is appended, an id read again
//! is replaced in place. Where the three readers differ (how a boolean set read twice merges,
//! where complex data goes) the [`Flavor`] names the rule.

use std::collections::BTreeMap;

use super::Flavor;
use super::record::{Rec, sub};

/// One property (`OfficeArtFOPTE`): 14-bit id, 32-bit value, and the complex data when `fComplex`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prop<'a> {
    pub id: u16,
    pub value: u32,
    /// `fBid`: the value is a BLIP index (1-based in the BStore)
    pub bid: bool,
    /// `fComplex`: the bytes that follow the table (strings, `IMsoArray`s, blobs); possibly empty
    pub complex: Option<&'a [u8]>,
}

/// The complex properties that are `IMsoArray`s (`nElems`, `nElemsAlloc`, `cbElem` + data):
/// vertices, segments, connection sites, inscribe rectangles, adjust handles, dash style, the
/// fill shade colours, the wrap polygon and the gradient stops.
const MSO_ARRAY_PROPS: [u16; 13] = [0x145, 0x146, 0x151, 0x152, 0x153, 0x154, 0x155, 0x156, 0x157, 0x197, 0x1cf, 0x383, 0x3a0];

/// Above this many entries a map also keeps an index (a FOPT has at most 4,095 entries, so a
/// linear search per entry would be quadratic on hostile input).
const LINEAR_MAX: usize = 16;

/// An ordered map id → property with the semantics of a JS `Map`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PropMap<'a> {
    entries: Vec<Prop<'a>>,
    index: Option<BTreeMap<u16, usize>>,
}

impl<'a> PropMap<'a> {
    fn position(&self, id: u16) -> Option<usize> {
        match &self.index {
            Some(ix) => ix.get(&id).copied(),
            None => self.entries.iter().position(|p| p.id == id),
        }
    }

    fn get(&self, id: u16) -> Option<&Prop<'a>> {
        self.position(id).and_then(|i| self.entries.get(i))
    }

    fn set(&mut self, prop: Prop<'a>) {
        if let Some(slot) = self.position(prop.id).and_then(|i| self.entries.get_mut(i)) {
            *slot = prop;
            return;
        }
        let at = self.entries.len();
        self.entries.push(prop);
        match &mut self.index {
            Some(ix) => {
                ix.insert(prop.id, at);
            }
            None if self.entries.len() > LINEAR_MAX => {
                self.index = Some(self.entries.iter().enumerate().map(|(i, p)| (p.id, i)).collect());
            }
            None => {}
        }
    }
}

/// The properties of one shape, in the order the TS `Map` iterates them.
///
/// Word and Slides keep every property in one table (a complex property replaces a simple one of
/// the same id and back). Sheet keeps the complex ones apart (`complex: Map<id, bytes>` next to
/// `props: Map<id, number>`): with [`Flavor::Sheet`], [`read_fopt`] puts them in a second table
/// that only [`PropTable::get_complex`] and [`PropTable::iter_complex`] see.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PropTable<'a> {
    main: PropMap<'a>,
    split: Option<Box<PropMap<'a>>>,
}

impl<'a> PropTable<'a> {
    pub fn new() -> Self {
        PropTable::default()
    }

    /// The property (`props.get(id)`).
    pub fn get(&self, id: u16) -> Option<&Prop<'a>> {
        self.main.get(id)
    }

    /// Its value (`props.get(id)?.value`).
    pub fn value(&self, id: u16) -> Option<u32> {
        self.get(id).map(|p| p.value)
    }

    /// `props.has(id)`.
    pub fn contains(&self, id: u16) -> bool {
        self.main.position(id).is_some()
    }

    /// The complex data of the id: from the table (Word, Slides) or from the separate table of
    /// [`Flavor::Sheet`].
    pub fn get_complex(&self, id: u16) -> Option<&'a [u8]> {
        self.main.get(id).and_then(|p| p.complex).or_else(|| self.split.as_ref().and_then(|m| m.get(id)).and_then(|p| p.complex))
    }

    /// Sets a property (`props.set`): replaced in place when the id is there, appended otherwise.
    pub fn set(&mut self, prop: Prop<'a>) {
        self.main.set(prop);
    }

    /// The properties in `Map` order.
    pub fn iter(&self) -> impl Iterator<Item = &Prop<'a>> {
        self.main.entries.iter()
    }

    /// The complex properties kept apart by [`Flavor::Sheet`], in `Map` order (empty otherwise).
    pub fn iter_complex(&self) -> impl Iterator<Item = &Prop<'a>> {
        self.split.iter().flat_map(|m| m.entries.iter())
    }

    pub fn len(&self) -> usize {
        self.main.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.main.entries.is_empty()
    }
}

/// Is the id a boolean set ([MS-ODRAW] 2.3.*.BooleanProperties: the last id of each group,
/// `0x3F`, `0x7F`, ..., `0x1BF`, `0x1FF`...)? `isBooleanProp`.
pub fn is_boolean_prop(id: u16) -> bool {
    id & 0x3f == 0x3f
}

/// Overlays `over` on `base` only in the bits whose `fUse*` (bit + 16) is set in `over`, the
/// `fUse*` bits included (`mergeBooleans`).
pub fn merge_booleans(base: u32, over: u32) -> u32 {
    let use_ = over >> 16;
    let mask = use_ | (use_ << 16);
    (base & !mask) | (over & mask)
}

/// The per-bit loop of Slides and Sheet: for each bit with `fUse` in `value`, the bit comes from
/// `value` and its `fUse` is set. The same bits as [`merge_booleans`]; the readers differ in when
/// they call it.
fn merge_use_bits(prev: u32, value: u32) -> u32 {
    let use_ = value >> 16;
    let mut v = prev;
    for b in 0..16 {
        let bit = 1u32 << b;
        if use_ & bit != 0 {
            let fuse = 1u32 << (b + 16);
            v = (v & !bit & !fuse) | (value & bit) | fuse;
        }
    }
    v
}

/// Reads the properties of a FOPT (or Secondary/Tertiary FOPT) into `into` (`readFopt`).
///
/// `rec.inst` entries of 6 bytes, then the complex data in entry order. For an `IMsoArray` some
/// writers (LibreOffice) declare the size without the 6 header bytes: when the declared size is
/// exactly the header's size minus 6 and the header's size fits, the header's size is used.
/// Complex data never reads past the record.
///
/// A boolean set read again (FOPT then TertiaryFOPT of the same shape):
/// - [`Flavor::Word`]: when the new property is not complex, its value becomes
///   `merge_booleans(prev, new)` (with no `fUse` bit that is the previous value), and the new
///   property (its `bid`) replaces the previous one;
/// - [`Flavor::Slides`] and [`Flavor::Sheet`]: only when neither is complex and the new value has
///   an `fUse` bit; the previous property keeps its `bid` and takes the merged value. Without an
///   `fUse` bit the new value replaces the old one.
///
/// [`Flavor::Sheet`] keeps complex properties in the separate table of [`PropTable`].
pub fn read_fopt<'a>(rec: &Rec<'a>, into: &mut PropTable<'a>, flavor: Flavor) {
    let n = u64::from(rec.inst);
    let mut complex_off = rec.start + n * 6;
    for i in 0..n {
        let o = rec.start + i * 6;
        if o + 6 > rec.end {
            break;
        }
        let opid = rec.u16(o);
        let value = rec.u32(o + 2);
        let mut prop = Prop { id: opid & 0x3fff, value, bid: opid & 0x4000 != 0, complex: None };
        if opid & 0x8000 != 0 {
            let mut size = u64::from(value);
            if MSO_ARRAY_PROPS.contains(&prop.id) && complex_off + 6 <= rec.end {
                let mut cb = u64::from(rec.u16(complex_off + 4));
                if cb == 0xfff0 {
                    cb = 4;
                }
                let arr_size = 6 + u64::from(rec.u16(complex_off)) * cb;
                if u64::from(value) == arr_size - 6 && complex_off + arr_size <= rec.end {
                    size = arr_size;
                }
            }
            let len = if rec.end > complex_off { size.min(rec.end - complex_off) } else { 0 };
            prop.complex = Some(sub(rec.data, complex_off, complex_off + len));
            complex_off += len;
        }
        match flavor {
            Flavor::Word => {
                if prop.complex.is_none()
                    && is_boolean_prop(prop.id)
                    && let Some(prev) = into.main.get(prop.id)
                {
                    prop.value = merge_booleans(prev.value, value);
                }
                into.main.set(prop);
            }
            Flavor::Slides | Flavor::Sheet => {
                if flavor == Flavor::Sheet && prop.complex.is_some() {
                    into.split.get_or_insert_with(Box::default).set(prop);
                    continue;
                }
                if let Some(prev) = into.main.get(prop.id).copied()
                    && is_boolean_prop(prop.id)
                    && prev.complex.is_none()
                    && prop.complex.is_none()
                    && value >> 16 != 0
                {
                    into.main.set(Prop { value: merge_use_bits(prev.value, value), ..prev });
                    continue;
                }
                into.main.set(prop);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::record::read_rec;
    use super::*;

    fn fopt(entries: &[(u16, u32)], complex: &[u8]) -> Vec<u8> {
        let n = u16::try_from(entries.len()).unwrap();
        let len = u32::try_from(entries.len() * 6 + complex.len()).unwrap();
        let mut b = Vec::new();
        b.extend_from_slice(&((n << 4) | 3).to_le_bytes());
        b.extend_from_slice(&0xf00bu16.to_le_bytes());
        b.extend_from_slice(&len.to_le_bytes());
        for &(op, v) in entries {
            b.extend_from_slice(&op.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(complex);
        b
    }

    #[test]
    fn merge_booleans_keeps_bits_without_fuse() {
        assert_eq!(merge_booleans(0x0010_0010, 0x0008_0000), 0x0018_0010);
        assert_eq!(merge_booleans(0x0010_0010, 0x0010_0000), 0x0010_0000);
        assert_eq!(merge_booleans(0xffff_ffff, 0), 0xffff_ffff);
        for (a, b) in [(0x1234_5678, 0x8765_4321), (0, 0xffff_ffff), (0xdead_beef, 0x00f0_000f)] {
            if b >> 16 != 0 {
                assert_eq!(merge_booleans(a, b), merge_use_bits(a, b));
            }
        }
    }

    #[test]
    fn array_size_without_header_and_order() {
        // 0x145 complex, declared 8 = 2 elements of 4 bytes without the 6-byte header
        let mut cx = vec![2, 0, 2, 0, 4, 0];
        cx.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let b = fopt(&[(0x8145, 8), (0x0181, 0xff), (0x0181, 0xee)], &cx);
        let rec = read_rec(&b, 0, b.len() as u64).unwrap();
        let mut t = PropTable::new();
        read_fopt(&rec, &mut t, Flavor::Word);
        assert_eq!(t.get_complex(0x145).map(<[u8]>::len), Some(14));
        assert_eq!(t.iter().map(|p| (p.id, p.value)).collect::<Vec<_>>(), [(0x145, 8), (0x181, 0xee)]);
        let mut s = PropTable::new();
        read_fopt(&rec, &mut s, Flavor::Sheet);
        assert_eq!(s.len(), 1);
        assert_eq!(s.get_complex(0x145).map(<[u8]>::len), Some(14));
    }

    #[test]
    fn many_entries_use_the_index() {
        let entries: Vec<(u16, u32)> = (0..100u16).map(|i| (i, u32::from(i))).chain([(5, 99)]).collect();
        let b = fopt(&entries, &[]);
        let rec = read_rec(&b, 0, b.len() as u64).unwrap();
        let mut t = PropTable::new();
        read_fopt(&rec, &mut t, Flavor::Word);
        assert_eq!(t.len(), 100);
        assert_eq!(t.value(5), Some(99));
        assert_eq!(t.iter().nth(5).map(|p| p.value), Some(99));
    }
}
