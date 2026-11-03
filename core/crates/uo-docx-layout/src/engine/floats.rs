//! Floating objects (frontier F7b): `placeFloats`, `wrapFrame`, `polyBandIntervals`, objects,
//! textboxes and chains, frames (engine.ts:2891–3812, 4104–4145).

use crate::context::FlowState;
use crate::items::PassItems;
use crate::keys::{ContainerId, PageIx};
use crate::output::PlacedObject;

use super::page::FloatBox;

/// `pinnedFloats` value (engine.ts:2891, K2c): the page and y of a float placed before its anchor.
#[derive(Clone, Copy, Debug)]
pub struct Pinned {
    pub ps: PageIx,
    pub y: f64,
}

/// Which z layer of a page a float went into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZLayer {
    Behind,
    Front,
    HfBehind,
    HfFront,
}

/// `FloatReceipt` (engine.ts:4110): a placed float, to undo it.
#[derive(Clone, Debug)]
pub struct FloatReceipt {
    pub ps: PageIx,
    pub target: ZLayer,
    /// index of the `ZObj` in its layer
    pub zobj: u32,
    pub obj: PlacedObject,
    pub float_box: Option<FloatBox>,
    /// lists/SEQ/notes before the placement (a textbox): restored by the unplace
    pub flow: Option<FlowState>,
    /// a frame: items put in the FLOW (text layer)
    pub in_flow: Option<(ContainerId, PassItems)>,
}

/// `polyBandIntervals(poly, ya, yb)` (engine.ts:93) — F7b.
pub fn poly_band_intervals(_poly: &[(f64, f64)], _ya: f64, _yb: f64) -> crate::error::LResult<Vec<(f64, f64)>> {
    crate::error::unported(crate::error::Frontier::F7b, "engine::floats::poly_band_intervals")
}
