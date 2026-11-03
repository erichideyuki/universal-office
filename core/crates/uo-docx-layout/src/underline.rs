//! Underline geometry (`underline.ts`, frontier F5): GDI metrics at 600 DPI (px of 1/600").

/// `UlMetrics`: top (P), thickness (T) and descent limit (L), px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UlMetrics {
    pub p: f64,
    pub t: f64,
    pub l: f64,
}

/// `UlGeom`: derived geometry (px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UlGeom {
    pub p: f64,
    pub t: f64,
    pub td: f64,
    pub top1: f64,
    pub top2: f64,
    pub b: f64,
}
