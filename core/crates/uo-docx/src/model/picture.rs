//! The picture formatting read from a `w:drawing` (docx-core `picture-xml.ts`:
//! `readPictureFormat`, the read side, frontier R4b): EMU, as in the XML. Its colours are the
//! picture-xml `DmlColor` (`{ kind, val, lastClr?, mods? }` with the modifier values parsed at
//! read), a different TS shape from the drawing spec strings of `drawing.ts`.

use crate::js::{JsNumber, JsRecord, Nullable, Str};
use crate::js_object;

use super::drawing::Ltrb;

js_object! {
    /// The picture-xml colour (`DmlColor` of `picture-xml.ts`): `kind` is srgbClr | schemeClr |
    /// prstClr | sysClr; modifiers in file order, alpha included, `parseInt(val) || 0`.
    pub struct PicColor {
        req kind: Str = "kind",
        req val: Str = "val",
        opt last_clr: Str = "lastClr",
        opt mods: Vec<(Str, JsNumber)> = "mods",
    }
}

js_object! {
    /// `a:ln` of a picture (`PicLine`).
    pub struct PicLine {
        /// EMU (absent = 9525)
        opt w: JsNumber = "w",
        opt cap: Str = "cap",
        opt cmpd: Str = "cmpd",
        opt algn: Str = "algn",
        /// colour, `null` = `a:noFill`
        opt fill: Nullable<PicColor> = "fill",
        opt fill_raw: Str = "fillRaw",
        opt dash: Str = "dash",
        opt join: Str = "join",
        opt miter_lim: JsNumber = "miterLim",
        opt tail_raw: Str = "tailRaw",
    }
}

js_object! {
    /// A picture shadow (`PicShadow`): outer | inner | prst.
    pub struct PicShadow {
        req kind: Str = "kind",
        opt blur_rad: JsNumber = "blurRad",
        opt dist: JsNumber = "dist",
        opt dir: JsNumber = "dir",
        opt sx: JsNumber = "sx",
        opt sy: JsNumber = "sy",
        opt kx: JsNumber = "kx",
        opt ky: JsNumber = "ky",
        opt algn: Str = "algn",
        opt rot_with_shape: bool = "rotWithShape",
        opt prst: Str = "prst",
        req color: PicColor = "color",
    }
}

js_object! {
    /// A picture reflection (`PicReflection`).
    pub struct PicReflection {
        opt blur_rad: JsNumber = "blurRad",
        opt st_a: JsNumber = "stA",
        opt st_pos: JsNumber = "stPos",
        opt end_a: JsNumber = "endA",
        opt end_pos: JsNumber = "endPos",
        opt dist: JsNumber = "dist",
        opt dir: JsNumber = "dir",
        opt fade_dir: JsNumber = "fadeDir",
        opt sx: JsNumber = "sx",
        opt sy: JsNumber = "sy",
        opt kx: JsNumber = "kx",
        opt ky: JsNumber = "ky",
        opt algn: Str = "algn",
        opt rot_with_shape: bool = "rotWithShape",
    }
}

js_object! {
    /// `PicEffects.glow`.
    pub struct PicGlow {
        req rad: JsNumber = "rad",
        req color: PicColor = "color",
    }
}

js_object! {
    /// The effects of a picture (`PicEffects`).
    pub struct PicEffects {
        opt shadow: Box<PicShadow> = "shadow",
        opt reflection: Box<PicReflection> = "reflection",
        opt glow: Box<PicGlow> = "glow",
        opt soft_edge: JsNumber = "softEdge",
        /// `a:blur` / `a:fillOverlay`, raw
        opt other_raw: Vec<Str> = "otherRaw",
    }
}

js_object! {
    /// `{ lat, lon, rev }` of a 3D camera or light rig.
    pub struct Rot3 {
        req lat: JsNumber = "lat",
        req lon: JsNumber = "lon",
        req rev: JsNumber = "rev",
    }
}

js_object! {
    /// `Pic3D.camera`.
    pub struct Camera {
        req prst: Str = "prst",
        opt fov: JsNumber = "fov",
        opt zoom: JsNumber = "zoom",
        opt rot: Rot3 = "rot",
    }
}

js_object! {
    /// `Pic3D.lightRig`.
    pub struct LightRig {
        req rig: Str = "rig",
        req dir: Str = "dir",
        opt rot: Rot3 = "rot",
    }
}

js_object! {
    /// `Pic3D.bevelT` / `bevelB`.
    pub struct Bevel {
        opt w: JsNumber = "w",
        opt h: JsNumber = "h",
        opt prst: Str = "prst",
    }
}

js_object! {
    /// 3D of a picture (`Pic3D`).
    pub struct Pic3D {
        opt camera: Camera = "camera",
        opt light_rig: LightRig = "lightRig",
        opt bevel_t: Bevel = "bevelT",
        opt bevel_b: Bevel = "bevelB",
        opt extrusion_h: JsNumber = "extrusionH",
        opt contour_w: JsNumber = "contourW",
        opt z: JsNumber = "z",
        opt prst_material: Str = "prstMaterial",
        opt extrusion_clr: PicColor = "extrusionClr",
        opt contour_clr: PicColor = "contourClr",
    }
}

js_object! {
    /// `PicBlipFx.lum`.
    pub struct BlipLum {
        opt bright: JsNumber = "bright",
        opt contrast: JsNumber = "contrast",
    }
}

js_object! {
    /// `PicBlipFx.clrChange`.
    pub struct ClrChange {
        req from: PicColor = "from",
        req to: PicColor = "to",
        opt use_a: bool = "useA",
    }
}

js_object! {
    /// `PicBlipFx.a14` (`a14:imgProps`).
    pub struct A14Props {
        opt layer_rid: Str = "layerRid",
        opt saturation: JsNumber = "saturation",
        opt color_temp: JsNumber = "colorTemp",
        opt sharpen: JsNumber = "sharpen",
        opt bright: JsNumber = "bright",
        opt contrast: JsNumber = "contrast",
        opt other_raw: Vec<Str> = "otherRaw",
    }
}

js_object! {
    /// The blip adjustments of a picture (`PicBlipFx`).
    pub struct PicBlipFx {
        /// opacity (`a:alphaModFix amt`, 0..100000)
        opt alpha_mod_fix: JsNumber = "alphaModFix",
        opt lum: BlipLum = "lum",
        opt grayscl: bool = "grayscl",
        opt bi_level: JsNumber = "biLevel",
        opt duotone: Box<(PicColor, PicColor)> = "duotone",
        opt clr_change: Box<ClrChange> = "clrChange",
        opt a14: Box<A14Props> = "a14",
    }
}

js_object! {
    /// The position of a `wp:anchor` (`PicPos`).
    pub struct PicPos {
        req relative_from: Str = "relativeFrom",
        opt align: Str = "align",
        opt offset: JsNumber = "offset",
        opt pct: JsNumber = "pct",
        opt fallback: JsNumber = "fallback",
    }
}

js_object! {
    /// `{ relativeFrom, pct }` of `wp14:sizeRelH`/`sizeRelV`.
    pub struct SizeRel {
        req relative_from: Str = "relativeFrom",
        req pct: JsNumber = "pct",
    }
}

js_object! {
    /// `PictureFormat.dist` (`{ t, b, l, r }`).
    pub struct Dist {
        req t: JsNumber = "t",
        req b: JsNumber = "b",
        req l: JsNumber = "l",
        req r: JsNumber = "r",
    }
}

js_object! {
    /// `PictureFormat.polygon` (21600ths).
    pub struct WrapPolygon {
        req edited: bool = "edited",
        req pts: Vec<(JsNumber, JsNumber)> = "pts",
    }
}

js_object! {
    /// `PictureFormat.docPr`.
    pub struct DocPr {
        opt id: Str = "id",
        opt name: Str = "name",
        opt descr: Str = "descr",
        opt title: Str = "title",
        opt hidden: bool = "hidden",
        opt decorative: bool = "decorative",
    }
}

js_object! {
    /// The full formatting of a picture in a `w:drawing` (`PictureFormat`), EMU.
    pub struct PictureFormat {
        req inline: bool = "inline",
        req wrap: Str = "wrap",
        req wrap_text: Str = "wrapText",
        req dist: Dist = "dist",
        opt pos_h: PicPos = "posH",
        opt pos_v: PicPos = "posV",
        req cx: JsNumber = "cx",
        req cy: JsNumber = "cy",
        opt size_rel_h: SizeRel = "sizeRelH",
        opt size_rel_v: SizeRel = "sizeRelV",
        req effect_extent: Ltrb = "effectExtent",
        req locked: bool = "locked",
        req layout_in_cell: bool = "layoutInCell",
        req allow_overlap: bool = "allowOverlap",
        req behind_doc: bool = "behindDoc",
        opt relative_height: JsNumber = "relativeHeight",
        opt polygon: Box<WrapPolygon> = "polygon",
        req doc_pr: DocPr = "docPr",
        req no_change_aspect: bool = "noChangeAspect",
        req rot: JsNumber = "rot",
        req flip_h: bool = "flipH",
        req flip_v: bool = "flipV",
        /// 1000ths of a percent (can be negative)
        req src_rect: Ltrb = "srcRect",
        req geom: Str = "geom",
        opt adj: JsRecord<JsNumber> = "adj",
        opt cust_geom: bool = "custGeom",
        opt line: Nullable<Box<PicLine>> = "line",
        req effects: PicEffects = "effects",
        opt three_d: Box<Pic3D> = "threeD",
        req blip: PicBlipFx = "blip",
        opt embed: Str = "embed",
    }
}
