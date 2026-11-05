//! The shape tree: `SpContainer` (one shape, or the header of a group), `SpgrContainer` (a group
//! and its children), the Word drawing index by `spid` with the draw order, and the Word
//! inheritance from master shapes (`readSpContainer`, `readSpgrContainer`, `indexDrawing`,
//! `applyMasters` of `docx-core/src/doc/officeart.ts`).
//!
//! The record parts are kept raw where the apps read them differently (the client anchor: a
//! rectangle in Slides, a cell anchor in Sheet; nothing in Word, whose anchors come from the
//! `PlcSpa`), so the tree is the same for the three apps.

use std::collections::{BTreeMap, BTreeSet};

use super::fopt::{PropTable, is_boolean_prop, merge_booleans, read_fopt};
use super::record::{Rec, i32_at, rt};
use super::{Flavor, OdrawLimits};

/// `FSP.grfPersistent` flags ([MS-ODRAW] 2.2.40).
pub mod fsp {
    pub const GROUP: u32 = 0x1;
    pub const CHILD: u32 = 0x2;
    pub const PATRIARCH: u32 = 0x4;
    pub const DELETED: u32 = 0x8;
    pub const OLE_SHAPE: u32 = 0x10;
    pub const HAVE_MASTER: u32 = 0x20;
    pub const FLIP_H: u32 = 0x40;
    pub const FLIP_V: u32 = 0x80;
    pub const CONNECTOR: u32 = 0x100;
    pub const HAVE_ANCHOR: u32 = 0x200;
    pub const BACKGROUND: u32 = 0x400;
    pub const HAVE_SPT: u32 = 0x800;
}

/// The id of `hspMaster` (the spid of the master shape).
pub const PROP_HSP_MASTER: u16 = 0x301;

/// A rectangle of four `i32` (`FSPGR`, `ChildAnchor`, the Slides `RectStruct`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RectI32 {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl RectI32 {
    /// `readRect`: `l, t, r, b` at `off`, read in the whole buffer (past the record they are the
    /// bytes that follow, past the buffer 0).
    pub fn read(data: &[u8], off: u64) -> RectI32 {
        RectI32 { l: i32_at(data, off), t: i32_at(data, off + 4), r: i32_at(data, off + 8), b: i32_at(data, off + 12) }
    }
}

/// Which anchor record came last in the shape (Slides keeps one `anchor` with its kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    Client,
    Child,
}

/// A shape or a group of the drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape<'a> {
    pub spid: u32,
    /// the msospt type (`FSP.recInstance`)
    pub shape_type: u16,
    /// the `FSP` flags ([`fsp`])
    pub flags: u32,
    pub props: PropTable<'a>,
    /// the last `FSPGR` (the coordinate space of a group's children)
    pub fspgr: Option<Rec<'a>>,
    /// the last `ChildAnchor` (the anchor inside the parent group)
    pub child_anchor: Option<Rec<'a>>,
    /// the last `ClientAnchor` (Slides: rectangle; Sheet: cell anchor; Word: unused)
    pub client_anchor: Option<Rec<'a>>,
    /// which of the two anchors came last
    pub last_anchor: Option<AnchorKind>,
    /// the last `ClientData` (Slides: PPT records; Sheet: links the shape to its OBJ)
    pub client_data: Option<Rec<'a>>,
    /// the last `ClientTextbox` (Slides: the text records)
    pub client_textbox: Option<Rec<'a>>,
    /// a group's children in draw order; `None` for a leaf shape
    pub children: Option<Vec<Shape<'a>>>,
    /// Word: position in the drawing's draw order (top-level shapes of [`index_drawing`])
    pub z: Option<u32>,
}

impl<'a> Shape<'a> {
    fn empty(flags: u32) -> Shape<'a> {
        Shape {
            spid: 0,
            shape_type: 0,
            flags,
            props: PropTable::new(),
            fspgr: None,
            child_anchor: None,
            client_anchor: None,
            last_anchor: None,
            client_data: None,
            client_textbox: None,
            children: None,
            z: None,
        }
    }

    /// The group rectangle as Word and Slides read it (`readRect` at the `FSPGR` data, whatever
    /// its length).
    pub fn group_rect(&self) -> Option<RectI32> {
        self.fspgr.map(|r| RectI32::read(r.data, r.start))
    }

    /// The child anchor as Word and Slides read it (`readRect` at the `ChildAnchor` data).
    pub fn child_rect(&self) -> Option<RectI32> {
        self.child_anchor.map(|r| RectI32::read(r.data, r.start))
    }

    /// Is the flag set?
    pub fn has_flag(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

/// `SpContainer` → shape (`readSpContainer`): the last `FSP` gives type, spid and flags; the
/// FOPTs are read in order into the shape's properties with the flavor's rules.
pub fn read_sp_container<'a>(rec: &Rec<'a>, flavor: Flavor) -> Shape<'a> {
    let mut shape = Shape::empty(0);
    for c in rec.children() {
        match c.rec_type {
            rt::FSP => {
                shape.shape_type = c.inst;
                shape.spid = c.u32(c.start);
                shape.flags = c.u32(c.start + 4);
            }
            rt::FSPGR => shape.fspgr = Some(c),
            rt::FOPT | rt::SECONDARY_FOPT | rt::TERTIARY_FOPT => read_fopt(&c, &mut shape.props, flavor),
            rt::CHILD_ANCHOR => {
                shape.child_anchor = Some(c);
                shape.last_anchor = Some(AnchorKind::Child);
            }
            rt::CLIENT_ANCHOR => {
                shape.client_anchor = Some(c);
                shape.last_anchor = Some(AnchorKind::Client);
            }
            rt::CLIENT_DATA => shape.client_data = Some(c),
            rt::CLIENT_TEXTBOX => shape.client_textbox = Some(c),
            _ => {}
        }
    }
    shape
}

/// `SpgrContainer` → group with its children (`readSpgrContainer`, Word and Slides): the first
/// `SpContainer` describes the group itself (without one the group is an empty shape with
/// `fGroup`); deleted shapes and groups are dropped; groups nested deeper than
/// `limits.max_group_depth` are dropped. The recursion depth is that limit; at most
/// `limits.max_shapes` shapes are read.
pub fn read_spgr_container<'a>(rec: &Rec<'a>, flavor: Flavor, limits: &OdrawLimits) -> Shape<'a> {
    let mut left = limits.max_shapes;
    read_spgr(rec, flavor, limits, 0, &mut left)
}

/// One shape container read against the shape budget of the call.
fn take_sp<'a>(rec: &Rec<'a>, flavor: Flavor, left: &mut u32) -> Option<Shape<'a>> {
    *left = left.checked_sub(1)?;
    Some(read_sp_container(rec, flavor))
}

fn read_spgr<'a>(rec: &Rec<'a>, flavor: Flavor, limits: &OdrawLimits, depth: u32, left: &mut u32) -> Shape<'a> {
    let mut kids = rec.children();
    let first = kids.clone().next();
    let mut group = match first {
        Some(f) if f.rec_type == rt::SP_CONTAINER => {
            kids.next();
            take_sp(&f, flavor, left).unwrap_or_else(|| Shape::empty(0))
        }
        _ => Shape::empty(fsp::GROUP),
    };
    let mut children = Vec::new();
    for c in kids {
        if *left == 0 {
            break;
        }
        if c.rec_type == rt::SP_CONTAINER {
            if let Some(s) = take_sp(&c, flavor, left).filter(|s| !s.has_flag(fsp::DELETED)) {
                children.push(s);
            }
        } else if c.rec_type == rt::SPGR_CONTAINER && depth < limits.max_group_depth {
            let g = read_spgr(&c, flavor, limits, depth + 1, left);
            if !g.has_flag(fsp::DELETED) {
                children.push(g);
            }
        }
    }
    group.children = Some(children);
    group
}

/// The top-level shapes of Word's drawings by spid, in the insertion order of the TS `Map`:
/// a spid seen again replaces the shape in place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShapeMap<'a> {
    shapes: Vec<Shape<'a>>,
    by_spid: BTreeMap<u32, usize>,
}

impl<'a> ShapeMap<'a> {
    pub fn new() -> Self {
        ShapeMap::default()
    }

    /// `into.size`.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// `into.get(spid)`.
    pub fn get(&self, spid: u32) -> Option<&Shape<'a>> {
        self.by_spid.get(&spid).and_then(|&i| self.shapes.get(i))
    }

    /// `into.set(spid, shape)`.
    pub fn set(&mut self, shape: Shape<'a>) {
        match self.by_spid.get(&shape.spid).and_then(|&i| self.shapes.get_mut(i)) {
            Some(slot) => *slot = shape,
            None => {
                self.by_spid.insert(shape.spid, self.shapes.len());
                self.shapes.push(shape);
            }
        }
    }

    /// The shapes in `Map` order.
    pub fn iter(&self) -> impl Iterator<Item = &Shape<'a>> {
        self.shapes.iter()
    }

    fn size_u32(&self) -> u32 {
        u32::try_from(self.shapes.len()).unwrap_or(u32::MAX)
    }
}

/// Indexes the top-level shapes (and groups) of a Word `DgContainer` by spid (`indexDrawing`):
/// the children of each `SpgrContainer` (the patriarch's header is not indexed) and loose
/// `SpContainer`s. `z` is the size of the map before the shape is set: the draw order, back to
/// front, which is Word's Z order (a repeated spid gets the size, which did not grow). At most
/// `limits.max_shapes` shapes are read.
pub fn index_drawing<'a>(dg: &Rec<'a>, into: &mut ShapeMap<'a>, limits: &OdrawLimits) {
    let mut left = limits.max_shapes;
    index_drawing_counted(dg, into, limits, &mut left);
}

pub(crate) fn index_drawing_counted<'a>(dg: &Rec<'a>, into: &mut ShapeMap<'a>, limits: &OdrawLimits, left: &mut u32) {
    for c in dg.children() {
        if *left == 0 {
            break;
        }
        if c.rec_type == rt::SPGR_CONTAINER {
            let root = read_spgr(&c, Flavor::Word, limits, 0, left);
            for mut s in root.children.unwrap_or_default() {
                s.z = Some(into.size_u32());
                into.set(s);
            }
        } else if c.rec_type == rt::SP_CONTAINER {
            let Some(mut s) = take_sp(&c, Flavor::Word, left) else { break };
            s.z = Some(into.size_u32());
            into.set(s);
        }
    }
}

/// Word's master shapes (`applyMasters`): a shape whose `hspMaster` (0x301) names another shape
/// of the map takes the master's type when it has none and every property it lacks (appended in
/// the master's order); a boolean set it has (not complex) takes the master's bits where its own
/// `fUse*` are clear (`merge_booleans(master, own)`). The masters' spids go to `masters` (Word
/// stores a master as a hidden shape and does not draw it). Shapes are visited in map order, so
/// a master already resolved passes on what it inherited. One level per shape, no chain walk.
/// Once `limits.max_master_props` properties were added, the remaining shapes inherit nothing.
pub fn apply_masters(shapes: &mut ShapeMap<'_>, masters: &mut BTreeSet<u32>, limits: &OdrawLimits) {
    let mut left = u64::from(limits.max_master_props);
    for i in 0..shapes.shapes.len() {
        let Some(s) = shapes.shapes.get(i) else { continue };
        let Some(r) = s.props.value(PROP_HSP_MASTER) else { continue };
        let Some(&mi) = shapes.by_spid.get(&r) else { continue };
        if mi == i {
            continue;
        }
        let Some(m) = shapes.shapes.get(mi) else { continue };
        let missing = m.props.iter().filter(|p| p.id != PROP_HSP_MASTER && !s.props.contains(p.id)).count() as u64;
        if missing > left {
            break;
        }
        left -= missing;
        let (m_spid, m_type) = (m.spid, m.shape_type);
        let m_props: Vec<_> = m.props.iter().copied().collect();
        masters.insert(m_spid);
        let Some(s) = shapes.shapes.get_mut(i) else { continue };
        if s.shape_type == 0 {
            s.shape_type = m_type;
        }
        for p in m_props {
            if p.id == PROP_HSP_MASTER {
                continue;
            }
            match s.props.get(p.id).copied() {
                None => s.props.set(p),
                Some(own) if is_boolean_prop(p.id) && own.complex.is_none() => {
                    s.props.set(super::fopt::Prop { value: merge_booleans(p.value, own.value), ..own });
                }
                Some(_) => {}
            }
        }
    }
}
