//! OfficeArt ([MS-ODRAW]) of the `.xls` (`officeart.ts`): the MSODRAWINGGROUP holds the image store (BStore → FBSE with the
//! embedded BLIP) and the concatenated MSODRAWING of each sheet holds the tree of shapes (DgContainer → SpgrContainer →
//! SpContainer with FSP, FOPT, cell anchor and ClientData — which links each shape, by order, to the OBJ record that follows).
//!
//! The records, the properties (Flavor::Sheet), the BLIP store and the BLIP decoding are `uo_drawingml::odraw`; this module
//! is the tree of the sheet drawing.

use std::collections::HashMap;

use uo_drawingml::odraw::{Blip, BlipBudget, Bse, Flavor, OdrawLimits, PropTable, Rec, decode_blip, read_bstore, read_fopt, recs_in, rt};

/// The cell anchor of Excel (OfficeArtClientAnchorSheet).
#[derive(Clone, Copy, Debug)]
pub struct CellAnchor {
    pub c1: u32,
    pub dx1: u32,
    pub r1: u32,
    pub dy1: u32,
    pub c2: u32,
    pub dx2: u32,
    pub r2: u32,
    pub dy2: u32,
}

/// A rectangle (FSPGR / ChildAnchor): coordinates of the group space.
#[derive(Clone, Copy, Debug)]
pub struct OaRect {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

/// A shape (or group) of the sheet drawing.
pub struct XlsShape<'a> {
    /// A number unique in the drawing, in document order (the key of the OBJ link).
    pub seq: usize,
    pub spid: u32,
    /// The msospt type (FSP.recInstance): 75 = picture frame, 201 = chart/control…
    pub ty: u16,
    /// FSP flags (fGroup 0x1, fChild 0x2, fPatriarch 0x4, fDeleted 0x8, fFlipH 0x40, fFlipV 0x80…).
    pub flags: u32,
    /// The properties of the FOPTs (simple ones in the table, complex ones apart).
    pub props: PropTable<'a>,
    /// The cell anchor (top-level shapes).
    pub anchor: Option<CellAnchor>,
    /// The anchor in the space of the parent group (shapes inside a group).
    pub child_anchor: Option<OaRect>,
    /// The coordinate space of the children (FSPGR) — groups only.
    pub group_rect: Option<OaRect>,
    /// The offset of the ClientData in the concatenated drawing (links to the OBJ).
    pub client_data_at: Option<u64>,
    pub group: bool,
    pub children: Vec<XlsShape<'a>>,
}

/// Deepest group nesting followed (`MAX_GROUP_DEPTH`).
const MAX_GROUP_DEPTH: u32 = 32;

fn u16_at(d: &[u8], o: u64) -> u32 {
    usize::try_from(o).ok().and_then(|o| Some(u32::from(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?])))).unwrap_or(0)
}

fn u32_at(d: &[u8], o: u64) -> u32 {
    usize::try_from(o)
        .ok()
        .and_then(|o| Some(u32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?])))
        .unwrap_or(0)
}

fn i32_at(d: &[u8], o: u64) -> i32 {
    i32::from_le_bytes(u32_at(d, o).to_le_bytes())
}

fn rect(d: &[u8], o: u64) -> OaRect {
    OaRect { l: i32_at(d, o), t: i32_at(d, o + 4), r: i32_at(d, o + 8), b: i32_at(d, o + 12) }
}

/// The state of a read of one drawing: the sequence and the shape budget.
struct Reader {
    seq: usize,
    left: u32,
}

/// `readSp`: SpContainer → shape (`None` = deleted, fDeleted 0x8).
fn read_sp<'a>(rd: &mut Reader, rec: &Rec<'a>) -> Option<XlsShape<'a>> {
    let d = rec.data;
    let seq = rd.seq;
    rd.seq += 1;
    let mut sh = XlsShape {
        seq,
        spid: 0,
        ty: 0,
        flags: 0,
        props: PropTable::new(),
        anchor: None,
        child_anchor: None,
        group_rect: None,
        client_data_at: None,
        group: false,
        children: Vec::new(),
    };
    for c in recs_in(d, rec.start, rec.end) {
        match c.rec_type {
            rt::FSP => {
                sh.ty = c.inst;
                sh.spid = u32_at(d, c.start);
                sh.flags = u32_at(d, c.start + 4);
            }
            rt::FSPGR => {
                if c.end - c.start >= 16 {
                    sh.group_rect = Some(rect(d, c.start));
                }
            }
            rt::FOPT | rt::SECONDARY_FOPT | rt::TERTIARY_FOPT => read_fopt(&c, &mut sh.props, Flavor::Sheet),
            rt::CLIENT_ANCHOR => {
                if c.end - c.start >= 18 {
                    let o = c.start;
                    sh.anchor = Some(CellAnchor {
                        c1: u16_at(d, o + 2),
                        dx1: u16_at(d, o + 4),
                        r1: u16_at(d, o + 6),
                        dy1: u16_at(d, o + 8),
                        c2: u16_at(d, o + 10),
                        dx2: u16_at(d, o + 12),
                        r2: u16_at(d, o + 14),
                        dy2: u16_at(d, o + 16),
                    });
                }
            }
            rt::CHILD_ANCHOR => {
                if c.end - c.start >= 16 {
                    sh.child_anchor = Some(rect(d, c.start));
                }
            }
            rt::CLIENT_DATA => sh.client_data_at = Some(c.hdr),
            _ => {}
        }
    }
    if sh.flags & 0x8 != 0 { None } else { Some(sh) }
}

/// `readGroup`: SpgrContainer → group (the first SpContainer describes the group itself).
fn read_group<'a>(rd: &mut Reader, rec: &Rec<'a>, depth: u32) -> Option<XlsShape<'a>> {
    let mut kids = rec.children();
    let first = kids.next()?;
    if first.rec_type != rt::SP_CONTAINER {
        return None;
    }
    let mut g = read_sp(rd, &first)?;
    g.group = true;
    for k in kids {
        if rd.left == 0 {
            break;
        }
        let s = if k.rec_type == rt::SP_CONTAINER {
            rd.left -= 1;
            read_sp(rd, &k)
        } else if k.rec_type == rt::SPGR_CONTAINER && depth < MAX_GROUP_DEPTH {
            read_group(rd, &k, depth + 1)
        } else {
            None
        };
        if let Some(s) = s {
            g.children.push(s);
        }
    }
    Some(g)
}

/// Shapes one drawing may hold (the TS has no bound; the cost of a shape is a property table).
const MAX_SHAPES: u32 = 1 << 16;

/// `readShapes`: the shapes of the DgContainer (the concatenated drawing of the sheet): the top-level ones in drawing
/// order, the groups with their children.
pub fn read_shapes(dg: &[u8]) -> Vec<XlsShape<'_>> {
    let mut rd = Reader { seq: 0, left: MAX_SHAPES };
    let mut out = Vec::new();
    for t in recs_in(dg, 0, dg.len() as u64) {
        if t.rec_type != rt::DG_CONTAINER {
            continue;
        }
        for c in t.children() {
            if c.rec_type != rt::SPGR_CONTAINER {
                continue;
            }
            for (i, k) in c.children().enumerate() {
                if rd.left == 0 {
                    return out;
                }
                if i == 0 && k.rec_type == rt::SP_CONTAINER {
                    continue; // the patriarch group itself
                }
                let s = if k.rec_type == rt::SP_CONTAINER {
                    rd.left -= 1;
                    read_sp(&mut rd, &k)
                } else if k.rec_type == rt::SPGR_CONTAINER {
                    read_group(&mut rd, &k, 0)
                } else {
                    None
                };
                if let Some(s) = s {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// `flattenShapes`: every shape of the tree (groups and children) in document order.
pub fn flatten<'s, 'a>(shapes: &'s [XlsShape<'a>], out: &mut Vec<&'s XlsShape<'a>>) {
    for s in shapes {
        out.push(s);
        flatten(&s.children, out);
    }
}

/// The default properties of the drawing (the FOPT of the DggContainer in the MSODRAWINGGROUP — Excel writes fillColor
/// 0x08000041 = window background and lineColor 0x08000040 = window text).
pub fn read_drawing_defaults(dgg: Option<&[u8]>) -> PropTable<'_> {
    let mut out = PropTable::new();
    let Some(d) = dgg else { return out };
    for top in recs_in(d, 0, d.len() as u64) {
        if top.rec_type != rt::DGG_CONTAINER {
            continue;
        }
        for c in top.children() {
            if c.rec_type == rt::FOPT {
                read_fopt(&c, &mut out, Flavor::Sheet);
            }
        }
    }
    out
}

/// The image store of the MSODRAWINGGROUP: index pib (1-based) → image. Decoded on demand (only the images in use).
pub struct BlipStore<'a> {
    entries: Vec<Option<Bse<'a>>>,
    cache: HashMap<u32, Option<Blip<'a>>>,
    budget: BlipBudget,
}

impl<'a> BlipStore<'a> {
    pub fn new(dgg: Option<&'a [u8]>) -> Self {
        let mut entries = Vec::new();
        if let Some(d) = dgg {
            for top in recs_in(d, 0, d.len() as u64) {
                if top.rec_type != rt::DGG_CONTAINER {
                    continue;
                }
                for c in top.children() {
                    if c.rec_type == rt::BSTORE_CONTAINER {
                        entries.extend(read_bstore(&c));
                    }
                }
            }
        }
        BlipStore { entries, cache: HashMap::new(), budget: BlipBudget { left: 512 << 20 } }
    }

    /// The image of the index pib (1-based), if any.
    pub fn get(&mut self, pib: u32) -> Option<&Blip<'a>> {
        if !self.cache.contains_key(&pib) {
            let rec: Option<Rec<'a>> = usize::try_from(pib)
                .ok()
                .and_then(|p| p.checked_sub(1))
                .and_then(|i| self.entries.get(i))
                .and_then(|e| e.as_ref())
                .and_then(|b| b.embedded);
            let blip = rec.and_then(|r| decode_blip(&r, &mut self.budget, Flavor::Sheet, &OdrawLimits::SHEET));
            self.cache.insert(pib, blip);
        }
        self.cache.get(&pib).and_then(Option::as_ref)
    }
}
