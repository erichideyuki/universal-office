//! Equations (`math.ts`, OMML layout, frontier F4) and its own font reader `parseFont`
//! (uo-fonts S14, frontier F1).

use crate::para::StyleIx;
use uo_render::FaceId;

/// `MathItem`: a glyph of a laid-out equation (x from the left edge, y from the baseline, + down).
#[derive(Clone, Copy, Debug)]
pub struct MathItem {
    pub cp: u32,
    pub x: f64,
    pub y: f64,
    pub face: FaceId,
    pub size_pt: f64,
    pub style: StyleIx,
}

/// A fraction bar or radical rule of an equation.
#[derive(Clone, Debug)]
pub struct MathRule {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub color: Box<[u16]>,
}

/// `MathBox` (math.ts:162): a laid-out equation box (twips).
#[derive(Clone, Debug, Default)]
pub struct MathBox {
    pub w: f64,
    pub items: Vec<MathItem>,
    pub rules: Vec<MathRule>,
    pub top: f64,
    pub bottom: f64,
    pub line_top: f64,
    pub line_bottom: f64,
}

/// `MathAtom.kind`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MathAtomKind {
    Char,
    Frac,
}

/// `MathAtom` (math.ts:172).
#[derive(Clone, Debug)]
pub struct MathAtom {
    pub kind: MathAtomKind,
    pub cp: u32,
    pub face: FaceId,
    pub size_pt: f64,
    pub style: StyleIx,
    pub adv: f64,
    pub frac: Option<Box<MathBox>>,
    pub offset: u32,
}
