//! Word 2010+ text effects (`w14:`) and OpenType typography (docx-core `text-effects.ts`), read
//! by frontier R1. The TS fill union (`{ type: 'none' } | { type: 'solid', color } | { type:
//! 'grad', … }`) is one permissive object (`FxFill`): every key combination the reader or an
//! edit can build is representable, and the digest is the TS object's.

use crate::js::{JsNumber, Str};
use crate::js_object;

js_object! {
    /// A w14 colour (`FxColor`): `rgb` or `scheme`, with modifiers `[name, val]`.
    pub struct FxColor {
        opt rgb: Str = "rgb",
        opt scheme: Str = "scheme",
        opt mods: Vec<(Str, JsNumber)> = "mods",
    }
}

js_object! {
    /// A gradient stop (`FxGradStop`): position 0–100000.
    pub struct FxGradStop {
        req pos: JsNumber = "pos",
        req color: FxColor = "color",
    }
}

js_object! {
    /// `lin` of a gradient fill: angle in 60000ths of a degree.
    pub struct FxLin {
        req ang: JsNumber = "ang",
        req scaled: bool = "scaled",
    }
}

js_object! {
    /// `path` of a gradient fill (`w14:path`), fill rectangle in 1000ths of a percent.
    pub struct FxPath {
        /// circle | rect | shape
        req kind: Str = "kind",
        req l: JsNumber = "l",
        req t: JsNumber = "t",
        req r: JsNumber = "r",
        req b: JsNumber = "b",
    }
}

js_object! {
    /// A text fill (`FxFill`): `type` is none | solid | grad.
    pub struct FxFill {
        req r#type: Str = "type",
        opt color: FxColor = "color",
        opt stops: Vec<FxGradStop> = "stops",
        opt lin: FxLin = "lin",
        opt path: FxPath = "path",
    }
}

js_object! {
    /// `w14:textOutline` (`FxOutline`).
    pub struct FxOutline {
        /// width in EMU
        opt w: JsNumber = "w",
        opt cap: Str = "cap",
        opt cmpd: Str = "cmpd",
        opt algn: Str = "algn",
        opt fill: Box<FxFill> = "fill",
        opt dash: Str = "dash",
        opt join: Str = "join",
        opt miter_lim: JsNumber = "miterLim",
    }
}

js_object! {
    /// `w14:shadow` (`FxShadow`): EMU, 60000ths of a degree, 1000ths of a percent.
    pub struct FxShadow {
        req blur_rad: JsNumber = "blurRad",
        req dist: JsNumber = "dist",
        req dir: JsNumber = "dir",
        req sx: JsNumber = "sx",
        req sy: JsNumber = "sy",
        req kx: JsNumber = "kx",
        req ky: JsNumber = "ky",
        req algn: Str = "algn",
        req color: FxColor = "color",
    }
}

js_object! {
    /// `w14:reflection` (`FxReflection`).
    pub struct FxReflection {
        req blur_rad: JsNumber = "blurRad",
        req st_a: JsNumber = "stA",
        req st_pos: JsNumber = "stPos",
        req end_a: JsNumber = "endA",
        req end_pos: JsNumber = "endPos",
        req dist: JsNumber = "dist",
        req dir: JsNumber = "dir",
        req fade_dir: JsNumber = "fadeDir",
        req sx: JsNumber = "sx",
        req sy: JsNumber = "sy",
        req kx: JsNumber = "kx",
        req ky: JsNumber = "ky",
        req algn: Str = "algn",
    }
}

js_object! {
    /// `w14:glow` (`FxGlow`): radius in EMU.
    pub struct FxGlow {
        req rad: JsNumber = "rad",
        req color: FxColor = "color",
    }
}

js_object! {
    /// The text effects of a run (`TextEffects`); 3D kept as raw XML.
    pub struct TextEffects {
        opt fill: Box<FxFill> = "fill",
        opt outline: Box<FxOutline> = "outline",
        opt shadow: Box<FxShadow> = "shadow",
        opt reflection: Box<FxReflection> = "reflection",
        opt glow: Box<FxGlow> = "glow",
        opt scene3d: Str = "scene3d",
        opt props3d: Str = "props3d",
    }
}

js_object! {
    /// OpenType typography of a run (`OpenTypeProps`).
    pub struct OpenTypeProps {
        opt ligatures: Str = "ligatures",
        /// default | lining | oldStyle
        opt num_form: Str = "numForm",
        /// default | proportional | tabular
        opt num_spacing: Str = "numSpacing",
        /// stylistic sets on (1–20)
        opt stylistic_sets: Vec<JsNumber> = "stylisticSets",
        opt cntxt_alts: bool = "cntxtAlts",
    }
}
