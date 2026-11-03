//! Custom geometry (`a:custGeom`) as numeric paths: the value the documents' readers produce and
//! the painters evaluate (uo-docx model `ShapeStyle.cust`, uo-render `ShapeGeom.cust`). The shape is
//! the TS `CustPath`/`CustCmd` of `write/packages/docx-core/src/drawing.ts` (the guides are already
//! evaluated by the reader); the Word reader `readCustGeom` is ported in uo-docx (R3).
//!
//! Numbers are `f64` as the TS computes them (a guide can give NaN). After an undo the TS holds
//! `null` where a NaN was (JSON); every use of these values is arithmetic, where `null` is 0, so
//! the uo-docx history normalisation stores 0.0 there (uo-docx record, "Milestone 1 — D1a
//! foundation").

/// One command of a custom path (`CustCmd`).
#[derive(Debug, Clone, PartialEq)]
pub enum CustCmd {
    /// `a:moveTo` (`{ op: 'M', x, y }`)
    MoveTo { x: f64, y: f64 },
    /// `a:lnTo` (`{ op: 'L', x, y }`)
    LineTo { x: f64, y: f64 },
    /// `a:cubicBezTo` (`{ op: 'C', pts }`): the points as read (normally 3)
    CubicTo(Vec<[f64; 2]>),
    /// `a:quadBezTo` (`{ op: 'Q', pts }`): the points as read (normally 2)
    QuadTo(Vec<[f64; 2]>),
    /// `a:arcTo` (`{ op: 'A', wR, hR, st, sw }`), angles in degrees
    ArcTo { w_r: f64, h_r: f64, st: f64, sw: f64 },
    /// `a:close` (`{ op: 'Z' }`)
    Close,
}

impl CustCmd {
    /// The TS `op` letter.
    pub fn op(&self) -> &'static str {
        match self {
            CustCmd::MoveTo { .. } => "M",
            CustCmd::LineTo { .. } => "L",
            CustCmd::CubicTo(_) => "C",
            CustCmd::QuadTo(_) => "Q",
            CustCmd::ArcTo { .. } => "A",
            CustCmd::Close => "Z",
        }
    }
}

/// One `a:path` of a custom geometry (`CustPath`): its own coordinate space `w × h`, whether it
/// is filled and stroked, and its commands.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CustPath {
    pub w: f64,
    pub h: f64,
    pub fill: bool,
    pub stroke: bool,
    pub cmds: Vec<CustCmd>,
}
