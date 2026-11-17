//! OfficeArt drawings of the `.ppt` (`ppt/officeart.ts`): the shape tree over `uo_drawingml::odraw`
//! (`Flavor::Slides`), kept in one arena for the whole document, the drawing of a slide or master
//! (`readDgContainer`) and the inheritance from master shapes (`resolveShapeMasters`), which
//! walks every drawing at once and writes into the shapes' own property tables.
//!
//! The TS shares objects (`bySpid` holds the first shape of a spid over all drawings, `done` is a
//! set of objects): arena indices play the part of the object identity.

use std::collections::{HashMap, HashSet};

use uo_drawingml::odraw::{
    AnchorKind, Flavor, OdrawLimits, Prop, PropTable, Rec, RectI32, Shape, fsp, read_sp_container, read_spgr_container, rt as ort,
};

use super::rd::{RecRead, children};

/// A rectangle of four `i32` (`Rect`).
pub type Rect = RectI32;

/// A shape or group of a drawing (`OAShape`).
#[derive(Debug, Clone)]
pub struct OaShape<'a> {
    pub spid: u32,
    /// the msospt type (`FSP.recInstance`)
    pub shape_type: u16,
    pub flags: u32,
    pub props: PropTable<'a>,
    /// the last anchor: ClientAnchor (master units of the slide) or ChildAnchor (group space)
    pub anchor: Option<Rect>,
    pub anchor_kind: Option<AnchorKind>,
    /// the group's child coordinate system (`FSPGR`)
    pub group_rect: Option<Rect>,
    /// a group's children (arena indices) in draw order; `None` for a leaf
    pub children: Option<Vec<usize>>,
    /// `OfficeArtClientData`
    pub client_data: Option<Rec<'a>>,
    /// `OfficeArtClientTextbox`
    pub client_textbox: Option<Rec<'a>>,
}

impl OaShape<'_> {
    /// `sh.flags & flag` (a JS `&` on the flags, which are below 2^31 in every flag tested).
    #[must_use]
    pub fn has_flag(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

/// The drawing of a slide or master: the patriarch group and the background shape.
#[derive(Debug, Clone, Copy, Default)]
pub struct OaDrawing {
    pub root: Option<usize>,
    pub background: Option<usize>,
}

/// Every shape of the document.
#[derive(Debug, Default)]
pub struct Shapes<'a> {
    pub list: Vec<OaShape<'a>>,
}

impl<'a> Shapes<'a> {
    /// The shape at an arena index (an index that is not there is a program error: the arena
    /// only grows and indices come from it).
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&OaShape<'a>> {
        self.list.get(i)
    }

    /// The children of a group, as shapes.
    pub fn kids(&self, sh: &OaShape<'a>) -> Vec<&OaShape<'a>> {
        sh.children.as_deref().unwrap_or_default().iter().filter_map(|&i| self.list.get(i)).collect()
    }

    fn adopt(&mut self, s: Shape<'a>) -> usize {
        let anchor = match s.last_anchor {
            Some(AnchorKind::Client) => s.client_anchor.as_ref().map(client_anchor),
            Some(AnchorKind::Child) => s.child_anchor.as_ref().map(|r| RectI32::read(r.data, r.start)),
            None => None,
        };
        let group_rect = s.group_rect();
        let children = s.children.map(|kids| kids.into_iter().map(|k| self.adopt(k)).collect::<Vec<usize>>());
        let sh = OaShape {
            spid: s.spid,
            shape_type: s.shape_type,
            flags: s.flags,
            props: s.props,
            anchor,
            anchor_kind: s.last_anchor,
            group_rect,
            children,
            client_data: s.client_data,
            client_textbox: s.client_textbox,
        };
        self.list.push(sh);
        self.list.len() - 1
    }

    /// `OfficeArtDgContainer` → drawing (`readDgContainer`): the first `SpgrContainer` is the root,
    /// a loose `SpContainer` with `fBackground` is the background shape.
    pub fn read_dg_container(&mut self, rec: &Rec<'a>) -> OaDrawing {
        let mut d = OaDrawing::default();
        let limits = OdrawLimits::WORD;
        for c in children(rec) {
            if c.rec_type == ort::SPGR_CONTAINER && d.root.is_none() {
                let g = read_spgr_container(&c, Flavor::Slides, &limits);
                d.root = Some(self.adopt(g));
            } else if c.rec_type == ort::SP_CONTAINER {
                let s = read_sp_container(&c, Flavor::Slides);
                if s.flags & fsp::BACKGROUND != 0 {
                    d.background = Some(self.adopt(s));
                }
            }
        }
        d
    }

    /// Inheritance from master shapes (`resolveShapeMasters`): a shape with `fHaveMaster` and an
    /// `hspMaster` (0x0301) takes what it does not specify from the master shape, resolved first
    /// in a chain (depth 8) and written into its own properties; boolean sets merge bit by bit by
    /// the `fUse` bits. `max_props` bounds the properties added in all (the TS has no bound; a
    /// declared difference on hostile input only, as `odraw::OdrawLimits::max_master_props`).
    pub fn resolve_shape_masters(&mut self, drawings: &[OaDrawing], max_props: u64) {
        let mut by_spid: HashMap<u32, usize> = HashMap::new();
        let mut all: Vec<usize> = Vec::new();
        for d in drawings {
            for start in [d.root, d.background].into_iter().flatten() {
                // an explicit stack keeps the pre-order of the recursive walk
                let mut stack = vec![start];
                while let Some(i) = stack.pop() {
                    let Some(sh) = self.list.get(i) else { continue };
                    by_spid.entry(sh.spid).or_insert(i);
                    all.push(i);
                    if let Some(kids) = &sh.children {
                        stack.extend(kids.iter().rev().copied());
                    }
                }
            }
        }
        let mut done: HashSet<usize> = HashSet::new();
        let mut left = max_props;
        for i in all {
            self.resolve_one(i, 0, &by_spid, &mut done, &mut left);
        }
    }

    fn resolve_one(&mut self, i: usize, depth: u32, by_spid: &HashMap<u32, usize>, done: &mut HashSet<usize>, left: &mut u64) {
        if depth > 8 || !done.insert(i) {
            return;
        }
        let Some(sh) = self.list.get(i) else { return };
        let Some(reference) = sh.props.get(0x301).map(|p| p.value) else { return };
        if !sh.has_flag(fsp::HAVE_MASTER) {
            return;
        }
        let Some(&mi) = by_spid.get(&reference) else { return };
        if mi == i {
            return;
        }
        self.resolve_one(mi, depth + 1, by_spid, done, left);
        let Some(m) = self.list.get(mi) else { return };
        let m_props: Vec<Prop<'a>> = m.props.iter().copied().collect();
        let Some(sh) = self.list.get_mut(i) else { return };
        for mp in m_props {
            let id = mp.id;
            if matches!(id, 0x80 | 0x301 | 0x380 | 0x381 | 0x3a9) {
                continue;
            }
            let own = sh.props.get(id).copied();
            let Some(own) = own else {
                if *left == 0 {
                    break;
                }
                *left -= 1;
                sh.props.set(mp);
                continue;
            };
            if (id & 0x3f) != 0x3f || own.complex.is_some() || mp.complex.is_some() {
                continue;
            }
            // boolean set: bits without a `fUse` of their own (and with one — or none at all — in
            // the master) come from the master
            let own_use = own.value >> 16;
            let m_use = mp.value >> 16;
            if own_use == 0 {
                continue;
            }
            let mut v = own.value;
            for b in 0..16u32 {
                if own_use & (1 << b) != 0 {
                    continue;
                }
                if m_use != 0 && m_use & (1 << b) == 0 {
                    continue;
                }
                v = (v & !(1 << b)) | (mp.value & (1 << b)) | (1 << (b + 16));
            }
            sh.props.set(Prop { value: v, ..own });
        }
    }
}

/// `ClientAnchor` of the PPT: a `SmallRectStruct` (8 bytes: top, left, right, bottom as `i16`) or
/// a `RectStruct` (16 bytes, as `i32`).
fn client_anchor(rec: &Rec<'_>) -> Rect {
    let o = rec.start;
    if rec.end - rec.start >= 16 {
        return Rect { t: rec.i32(o), l: rec.i32(o + 4), r: rec.i32(o + 8), b: rec.i32(o + 12) };
    }
    Rect { t: i32::from(rec.i16(o)), l: i32::from(rec.i16(o + 2)), r: i32::from(rec.i16(o + 4)), b: i32::from(rec.i16(o + 6)) }
}

// ───────────────────────────── property helpers (shapeconv) ─────────────────────────────

/// `prop(sh, id)`: the value of a property.
#[must_use]
pub fn prop(t: &PropTable<'_>, id: u16) -> Option<u32> {
    t.value(id)
}

/// `sprop` / `propS`: the value as a signed `int32` (`v | 0`).
#[must_use]
pub fn sprop(t: &PropTable<'_>, id: u16) -> Option<i32> {
    t.value(id).map(|v| i32::from_ne_bytes(v.to_ne_bytes()))
}

/// A boolean of a flags set (0x..BF/FF): bit `bit` = value, bit + 16 = "fUse". Old writers do not
/// write the fUse bits: with none set in the property the value stands as it is.
#[must_use]
pub fn bool_prop(t: &PropTable<'_>, id: u16, bit: u32, dflt: bool) -> bool {
    bool_prop_opt(t, id, bit).unwrap_or(dflt)
}

/// [`bool_prop`] with the "not specified" state (`None`) kept (`boolPropOpt`); a missing
/// property is `None` too.
#[must_use]
pub fn bool_prop_opt(t: &PropTable<'_>, id: u16, bit: u32) -> Option<bool> {
    let v = t.value(id)?;
    if v & (1 << (bit + 16)) != 0 {
        return Some(v & (1 << bit) != 0);
    }
    if (v >> 16) == 0 {
        return Some(v & (1 << bit) != 0);
    }
    None
}

/// 16.16 fixed point with sign → number (`fixed(v, dflt)`).
#[must_use]
pub fn fixed(v: Option<u32>, dflt: f64) -> f64 {
    v.map_or(dflt, |x| f64::from(i32::from_ne_bytes(x.to_ne_bytes())) / 65536.0)
}
