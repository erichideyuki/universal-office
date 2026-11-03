//! Drawing anchors and shape styles (docx-core `drawing.ts`), read by frontier R3. Colours are
//! [`DrawingColor`]s (the TS spec strings, structured, G12); custom geometry is
//! [`uo_drawingml::CustPath`].

use uo_drawingml::{CustCmd, CustPath};

use crate::js::{JsNumber, JsRecord, JsValue, JsonWriter, Str};
use crate::js_object;

use super::color::DrawingColor;
use super::media::ImageData;

js_object! {
    /// `{ l, t, r, b }` (effect extents, crops, insets, fill rectangles).
    pub struct Ltrb {
        req l: JsNumber = "l",
        req t: JsNumber = "t",
        req r: JsNumber = "r",
        req b: JsNumber = "b",
    }
}

js_object! {
    /// `{ x, y }`.
    pub struct Xy {
        req x: JsNumber = "x",
        req y: JsNumber = "y",
    }
}

js_object! {
    /// `wp:positionH`/`wp:positionV` (`AnchorPos`).
    pub struct AnchorPos {
        req relative_from: Str = "relativeFrom",
        opt align: Str = "align",
        /// twips (`posOffset`)
        opt offset: JsNumber = "offset",
        /// `wp14:pctPosHOffset` (1000ths of a percent)
        opt pct: JsNumber = "pct",
    }
}

js_object! {
    /// `wrapBox` of a tight/through wrap (fractions 0..1 of the extent).
    pub struct WrapBox {
        req x0: JsNumber = "x0",
        req y0: JsNumber = "y0",
        req x1: JsNumber = "x1",
        req y1: JsNumber = "y1",
    }
}

js_object! {
    /// `wp:anchor` (`DrawingAnchor`), in the assignment order of `readWpAnchor`.
    pub struct DrawingAnchor {
        /// inline | none | square | tight | through | topAndBottom
        req wrap: Str = "wrap",
        req pos_h: AnchorPos = "posH",
        req pos_v: AnchorPos = "posV",
        opt wrap_text: Str = "wrapText",
        opt wrap_box: WrapBox = "wrapBox",
        opt wrap_poly: Vec<(JsNumber, JsNumber)> = "wrapPoly",
        opt behind_doc: bool = "behindDoc",
        opt dist_t: JsNumber = "distT",
        opt dist_b: JsNumber = "distB",
        opt dist_l: JsNumber = "distL",
        opt dist_r: JsNumber = "distR",
        opt simple_pos: Xy = "simplePos",
        opt relative_height: JsNumber = "relativeHeight",
        opt layout_in_cell: bool = "layoutInCell",
        opt allow_overlap: bool = "allowOverlap",
        opt effect_extent: Ltrb = "effectExtent",
    }
}

js_object! {
    /// `ShapeStyle.line`.
    pub struct ShapeLine {
        req color: DrawingColor = "color",
        req width_pt: JsNumber = "widthPt",
        opt dash: Str = "dash",
        opt alpha: JsNumber = "alpha",
        opt r#ref: JsNumber = "ref",
    }
}

js_object! {
    /// `ShapeStyle.lineInset` (`{ w?, ref? }`).
    pub struct LineInset {
        opt w: JsNumber = "w",
        opt r#ref: JsNumber = "ref",
    }
}

js_object! {
    /// A gradient stop of `ShapeStyle.gradient`.
    pub struct GradStop {
        req pos: JsNumber = "pos",
        req color: DrawingColor = "color",
    }
}

js_object! {
    /// `ShapeStyle.gradient`.
    pub struct ShapeGradient {
        req from: DrawingColor = "from",
        req to: DrawingColor = "to",
        req angle_deg: JsNumber = "angleDeg",
        opt stops: Vec<GradStop> = "stops",
        opt scaled: bool = "scaled",
        opt rot_with_shape: bool = "rotWithShape",
        opt path: Str = "path",
        opt fill_to: Ltrb = "fillTo",
    }
}

js_object! {
    /// `ShapeStyle.shadow` (outer shadow: offset and blur in twips).
    pub struct ShapeShadow {
        req dx: JsNumber = "dx",
        req dy: JsNumber = "dy",
        req color: DrawingColor = "color",
        opt alpha: JsNumber = "alpha",
        opt blur: JsNumber = "blur",
    }
}

js_object! {
    /// `ShapeStyle.pattern`.
    pub struct ShapePattern {
        req prst: Str = "prst",
        req fg: DrawingColor = "fg",
        req bg: DrawingColor = "bg",
    }
}

js_object! {
    /// `ShapeStyle.blip.tile` (EMU and 1000ths of a percent).
    pub struct BlipTile {
        req tx: JsNumber = "tx",
        req ty: JsNumber = "ty",
        req sx: JsNumber = "sx",
        req sy: JsNumber = "sy",
        req algn: Str = "algn",
        req flip: Str = "flip",
    }
}

js_object! {
    /// `ShapeStyle.blip` (picture or texture fill).
    pub struct ShapeBlip {
        req image: ImageData = "image",
        opt tile: BlipTile = "tile",
        opt alpha: JsNumber = "alpha",
    }
}

js_object! {
    /// `ShapeStyle.fillRef` / `effectRef` (theme style index and colour).
    pub struct StyleRef {
        req idx: JsNumber = "idx",
        req color: DrawingColor = "color",
    }
}

js_object! {
    /// `ShapeFx.outer`.
    pub struct FxOuter {
        req blur_pt: JsNumber = "blurPt",
        req dist_pt: JsNumber = "distPt",
        req dir_deg: JsNumber = "dirDeg",
        req color: DrawingColor = "color",
        req alpha: JsNumber = "alpha",
        opt sx: JsNumber = "sx",
        opt sy: JsNumber = "sy",
        opt kx_deg: JsNumber = "kxDeg",
        opt ky_deg: JsNumber = "kyDeg",
        opt algn: Str = "algn",
        opt rot_with_shape: bool = "rotWithShape",
    }
}

js_object! {
    /// `ShapeFx.inner`.
    pub struct FxInner {
        req blur_pt: JsNumber = "blurPt",
        req dist_pt: JsNumber = "distPt",
        req dir_deg: JsNumber = "dirDeg",
        req color: DrawingColor = "color",
        req alpha: JsNumber = "alpha",
    }
}

js_object! {
    /// `ShapeFx.glow`.
    pub struct FxShapeGlow {
        req rad_pt: JsNumber = "radPt",
        req color: DrawingColor = "color",
        req alpha: JsNumber = "alpha",
    }
}

js_object! {
    /// `ShapeFx.reflection`.
    pub struct FxShapeReflection {
        req blur_pt: JsNumber = "blurPt",
        req st_a: JsNumber = "stA",
        req st_pos: JsNumber = "stPos",
        req end_a: JsNumber = "endA",
        req end_pos: JsNumber = "endPos",
        req dist_pt: JsNumber = "distPt",
        req dir_deg: JsNumber = "dirDeg",
        req sy: JsNumber = "sy",
        opt algn: Str = "algn",
        opt rot_with_shape: bool = "rotWithShape",
    }
}

js_object! {
    /// Shape effects beyond the simple shadow (`ShapeFx`): pt and degrees.
    pub struct ShapeFx {
        opt outer: Box<FxOuter> = "outer",
        opt inner: Box<FxInner> = "inner",
        opt glow: Box<FxShapeGlow> = "glow",
        opt soft_edge_pt: JsNumber = "softEdgePt",
        opt reflection: Box<FxShapeReflection> = "reflection",
    }
}

js_object! {
    /// The style of a shape or picture (`ShapeStyle`), in the assignment order of `readSpPr` for
    /// a solid fill (other fills record their order).
    pub struct ShapeStyle {
        /// preset geometry or `custom`
        req geom: Str = "geom",
        /// `a:avLst`: name → value of `val N`
        opt adj: JsRecord<JsNumber> = "adj",
        opt cust: Vec<CustPath> = "cust",
        /// degrees, clockwise
        opt rot: JsNumber = "rot",
        opt flip_h: bool = "flipH",
        opt flip_v: bool = "flipV",
        opt fill: DrawingColor = "fill",
        opt fill_alpha: JsNumber = "fillAlpha",
        opt gradient: Box<ShapeGradient> = "gradient",
        opt pattern: Box<ShapePattern> = "pattern",
        opt fill_ref: Box<StyleRef> = "fillRef",
        opt effect_ref: Box<StyleRef> = "effectRef",
        opt fx: Box<ShapeFx> = "fx",
        opt shadow: Box<ShapeShadow> = "shadow",
        opt line_inset: LineInset = "lineInset",
        opt line: Box<ShapeLine> = "line",
        opt blip: Box<ShapeBlip> = "blip",
    }
}

impl JsValue for CustCmd {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let op = self.op();
        let num = |x: f64| move |w: &mut crate::canon::CanonWriter<'_>| w.num(x);
        let opf = move |w: &mut crate::canon::CanonWriter<'_>| w.str(op);
        match self {
            CustCmd::MoveTo { x, y } | CustCmd::LineTo { x, y } => {
                crate::canon::canon_entries(w, &mut [("op", &opf), ("x", &num(*x)), ("y", &num(*y))]);
            }
            CustCmd::CubicTo(pts) | CustCmd::QuadTo(pts) => {
                let p = |w: &mut crate::canon::CanonWriter<'_>| {
                    w.array(pts.len());
                    for [a, b] in pts {
                        w.array(2);
                        w.num(*a);
                        w.num(*b);
                    }
                };
                crate::canon::canon_entries(w, &mut [("op", &opf), ("pts", &p)]);
            }
            CustCmd::ArcTo { w_r, h_r, st, sw } => {
                crate::canon::canon_entries(
                    w,
                    &mut [("op", &opf), ("wR", &num(*w_r)), ("hR", &num(*h_r)), ("st", &num(*st)), ("sw", &num(*sw))],
                );
            }
            CustCmd::Close => crate::canon::canon_entries(w, &mut [("op", &opf)]),
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.begin_object();
        w.member("op", &self.op());
        match self {
            CustCmd::MoveTo { x, y } | CustCmd::LineTo { x, y } => {
                w.member("x", x);
                w.member("y", y);
            }
            CustCmd::CubicTo(pts) | CustCmd::QuadTo(pts) => {
                let pts: Vec<[f64; 2]> = pts.clone();
                w.member("pts", &pts);
            }
            CustCmd::ArcTo { w_r, h_r, st, sw } => {
                w.member("wR", w_r);
                w.member("hR", h_r);
                w.member("st", st);
                w.member("sw", sw);
            }
            CustCmd::Close => {}
        }
        w.end_object();
        true
    }
}

impl JsValue for &'static str {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        w.str(self);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.str(self);
        true
    }
}

impl JsValue for CustPath {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let (pw, ph, fill, stroke) = (self.w, self.h, self.fill, self.stroke);
        crate::canon::canon_entries(
            w,
            &mut [
                ("w", &move |w: &mut crate::canon::CanonWriter<'_>| w.num(pw)),
                ("h", &move |w: &mut crate::canon::CanonWriter<'_>| w.num(ph)),
                ("fill", &move |w: &mut crate::canon::CanonWriter<'_>| w.bool(fill)),
                ("stroke", &move |w: &mut crate::canon::CanonWriter<'_>| w.bool(stroke)),
                ("cmds", &|w: &mut crate::canon::CanonWriter<'_>| self.cmds.canon(w)),
            ],
        );
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        w.begin_object();
        w.member("w", &self.w);
        w.member("h", &self.h);
        w.member("fill", &self.fill);
        w.member("stroke", &self.stroke);
        w.member("cmds", &self.cmds);
        w.end_object();
        true
    }
}
