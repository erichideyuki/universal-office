//! The flow of paragraphs (frontier F7a): `placeParagraph`, `layoutStatic`, keep rules,
//! contextual spacing, paragraph border groups (engine.ts).

use crate::json::JsonKey;
use crate::keys::{ContainerId, LParaId};
use crate::para::ParaMarker;
use uo_render::OptF64;

/// `PrevPara` (engine.ts:154): the previous paragraph, for spacing collapse, contextual spacing
/// and border groups.
#[derive(Clone, Debug, Default)]
pub struct PrevPara {
    pub after: f64,
    pub after_auto: bool,
    /// the list (abstractNum) of a numbered paragraph
    pub list: Option<Box<str>>,
    pub style_id: Option<Box<str>>,
    pub contextual: bool,
    /// `pBdr` present (the borders are read again from the paragraph when painting)
    pub p_bdr: Option<LParaId>,
    /// `pBdrKey`: `JSON.stringify([pBdr, ind])` with the JS rules (K9, `bdrKey`, engine.ts:4089)
    pub p_bdr_key: Option<JsonKey>,
    pub box_bottom_item_idx: Option<u32>,
    pub box_bottom: OptF64,
    /// `boxPage`: the items array the box went into (K2f)
    pub box_page: Option<ContainerId>,
}

/// `CtxNeighbor` (engine.ts:171).
#[derive(Clone, Debug, Default)]
pub struct CtxNeighbor {
    pub style_id: Option<Box<str>>,
}

/// `CellNeighbors` (engine.ts:173): before the first and after the last paragraph of a cell.
#[derive(Clone, Debug, Default)]
pub struct CellNeighbors {
    pub prev: Option<CtxNeighbor>,
    pub next: Option<CtxNeighbor>,
}

/// `HiddenJoin` (engine.ts:186): a hidden-mark paragraph waiting for the next one.
#[derive(Clone, Debug)]
pub struct HiddenJoin {
    pub para: LParaId,
    pub marker: Option<ParaMarker>,
}

/// The last paragraph border box (`lastBox`): its bottom and where it went (K2f).
#[derive(Clone, Copy, Debug)]
pub struct LastBox {
    pub bottom: f64,
    pub page: Option<ContainerId>,
}
