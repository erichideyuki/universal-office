//! Binary shape types (msospt, [MS-ODRAW] 2.4.6) → DrawingML preset geometries
//! (`<a:prstGeom prst>`) with the conversion of the adjust values: `presetFor` of
//! `docx-core/src/doc/preset-map.ts`, the same table as Slides' `ppt/preset-map.ts` (code
//! identical, comments differ), Sheet's `xls/odraw-geom.ts` (`presetGeomXml`, `hasPreset`,
//! `rawAdjust`) and C# `DocBinary/PresetMap.cs`. The differences between the copies are
//! [`PresetVariant`]s (the entry-by-entry comparison is in the module record).
//!
//! Binary shapes live in a 21600 x 21600 space stretched to the box, and the adjust values
//! (`adjustValue` .. `adjust10Value`, 0x147..0x150) are in that space. DrawingML adjusts are
//! fractions x 100000, mostly relative to `ss = min(w, h)`, so the conversion depends on the
//! box's aspect ratio. Arithmetic in `f64` in the TS order; `Math.round` through `uo-jsnum`.

use uo_jsnum::conv::truthy;
use uo_jsnum::math::{min, round};

use super::fopt::PropTable;

/// Where the copies of the map differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetVariant {
    /// Word `.doc` and Slides `.ppt` (`presetFor`): the classic WordArt types map to `rect`
    /// without adjusts; the caller passes `w || 1` and `h || 1`.
    WordSlides,
    /// Sheet `.xls` (`presetGeomXml`): WordArt types have no preset (`None`, the caller draws the
    /// text); `w || 1` and `h || 1` are applied here.
    Sheet,
}

/// One adjust of the result (`avLst` entry), already rounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresetAdjust {
    /// `adj` when the preset has one adjust, else `adj1`, `adj2`
    pub name: &'static str,
    /// `Math.round` of the converted value (may be `-0`)
    pub val: f64,
}

/// The preset of a binary shape type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresetResult {
    pub prst: &'static str,
    av: [Option<PresetAdjust>; 2],
    /// the binary geometry is the preset flipped vertically: an extra `flipV`
    pub flip_v: bool,
}

impl PresetResult {
    /// The adjusts (`av`), in order.
    pub fn av(&self) -> impl Iterator<Item = &PresetAdjust> {
        self.av.iter().flatten()
    }
}

/// How the binary adjusts become DrawingML adjusts (the `adj` functions of the TS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Adj {
    /// no adjust converted
    Fixed,
    /// `lin(0)`: linear fraction of 21600 → x100000
    Lin0,
    /// horizontal offset (fraction of w) → relative to ss
    OffW,
    /// tip of homePlate/chevron: x of the tip start → depth relative to ss
    TipW,
    /// stars: distance of the inner vertices to the centre → inner radius x50000
    Seal,
    /// horizontal arrow (`headAtStart`): adj1 = shaft top, adj2 = head x
    ArrowH(bool),
    /// vertical arrow (`headAtStart`): adj1 = shaft left, adj2 = head y
    ArrowV(bool),
    /// callouts: tip position relative to the centre
    Wedge,
    /// vertical fraction relative to ss (can, brackets)
    Vert,
    /// braces: vertical fraction relative to ss, then linear
    Brace,
    /// smiley: mouth curve
    Smiley,
}

#[derive(Debug, Clone, Copy)]
struct Def {
    prst: &'static str,
    adj: Adj,
    /// binary defaults (`binDefaults`): the adjust is always emitted
    wedge_defaults: bool,
    flip_v: bool,
}

const fn d(prst: &'static str) -> Def {
    Def { prst, adj: Adj::Fixed, wedge_defaults: false, flip_v: false }
}

const fn da(prst: &'static str, adj: Adj) -> Def {
    Def { prst, adj, wedge_defaults: false, flip_v: false }
}

const fn dw(prst: &'static str) -> Def {
    Def { prst, adj: Adj::Wedge, wedge_defaults: true, flip_v: false }
}

/// The binary defaults of the callouts (`binDefaults: [1350, 25920]`).
const WEDGE_DEFAULTS: [f64; 2] = [1350.0, 25920.0];

/// The table `P` (msospt → preset), in the TS order.
fn def(spt: u16) -> Option<Def> {
    use Adj::*;
    Some(match spt {
        1 => d("rect"),
        2 => da("roundRect", Lin0),
        3 => d("ellipse"),
        4 => d("diamond"),
        5 => da("triangle", Lin0),
        6 => d("rtTriangle"),
        7 => da("parallelogram", OffW),
        8 => Def { prst: "trapezoid", adj: OffW, wedge_defaults: false, flip_v: true },
        9 => da("hexagon", OffW),
        10 => da("octagon", Lin0),
        11 => da("plus", Lin0),
        12 => d("star5"),
        13 | 14 => da("rightArrow", ArrowH(false)),
        15 => da("homePlate", TipW),
        16 => da("cube", Lin0),
        17 => dw("wedgeRoundRectCallout"),
        18 => da("star16", Seal),
        19 => d("arc"),
        20 => d("line"),
        21 => da("plaque", Lin0),
        22 => da("can", Vert),
        23 => da("donut", Lin0),
        32 => d("straightConnector1"),
        33 => d("bentConnector2"),
        34 => d("bentConnector3"),
        35 => d("bentConnector4"),
        36 => d("bentConnector5"),
        37 => d("curvedConnector2"),
        38 => d("curvedConnector3"),
        39 => d("curvedConnector4"),
        40 => d("curvedConnector5"),
        41 => d("callout1"),
        42 => d("callout2"),
        43 => d("callout3"),
        44 => d("accentCallout1"),
        45 => d("accentCallout2"),
        46 => d("accentCallout3"),
        47 => d("borderCallout1"),
        48 => d("borderCallout2"),
        49 => d("borderCallout3"),
        50 => d("accentBorderCallout1"),
        51 => d("accentBorderCallout2"),
        52 => d("accentBorderCallout3"),
        53 => d("ribbon"),
        54 => d("ribbon2"),
        55 => da("chevron", TipW),
        56 => d("pentagon"),
        57 => d("noSmoking"),
        58 => da("star8", Seal),
        59 => da("star16", Seal),
        60 => da("star32", Seal),
        61 => dw("wedgeRectCallout"),
        62 => dw("wedgeRoundRectCallout"),
        63 => dw("wedgeEllipseCallout"),
        64 => d("wave"),
        65 => d("foldedCorner"),
        66 => da("leftArrow", ArrowH(true)),
        67 => da("downArrow", ArrowV(false)),
        68 => da("upArrow", ArrowV(true)),
        69 => da("leftRightArrow", ArrowH(true)),
        70 => da("upDownArrow", ArrowV(true)),
        71 => d("irregularSeal1"),
        72 => d("irregularSeal2"),
        73 => d("lightningBolt"),
        74 => d("heart"),
        75 => d("rect"),
        76 => d("quadArrow"),
        77 => d("leftArrowCallout"),
        78 => d("rightArrowCallout"),
        79 => d("upArrowCallout"),
        80 => d("downArrowCallout"),
        81 => d("leftRightArrowCallout"),
        82 => d("upDownArrowCallout"),
        83 => d("quadArrowCallout"),
        84 => da("bevel", Lin0),
        85 => da("leftBracket", Vert),
        86 => da("rightBracket", Vert),
        87 => da("leftBrace", Brace),
        88 => da("rightBrace", Brace),
        89 => d("leftUpArrow"),
        90 => d("bentUpArrow"),
        91 => d("bentArrow"),
        92 => da("star24", Seal),
        93 => da("stripedRightArrow", ArrowH(false)),
        94 => da("notchedRightArrow", ArrowH(false)),
        95 => d("blockArc"),
        96 => da("smileyFace", Smiley),
        97 => d("verticalScroll"),
        98 => d("horizontalScroll"),
        99 | 100 => d("circularArrow"),
        101 => d("uturnArrow"),
        102 => d("curvedRightArrow"),
        103 => d("curvedLeftArrow"),
        104 => d("curvedUpArrow"),
        105 => d("curvedDownArrow"),
        106 => dw("cloudCallout"),
        107 => d("ellipseRibbon"),
        108 => d("ellipseRibbon2"),
        109 => d("flowChartProcess"),
        110 => d("flowChartDecision"),
        111 => d("flowChartInputOutput"),
        112 => d("flowChartPredefinedProcess"),
        113 => d("flowChartInternalStorage"),
        114 => d("flowChartDocument"),
        115 => d("flowChartMultidocument"),
        116 => d("flowChartTerminator"),
        117 => d("flowChartPreparation"),
        118 => d("flowChartManualInput"),
        119 => d("flowChartManualOperation"),
        120 => d("flowChartConnector"),
        121 => d("flowChartPunchedCard"),
        122 => d("flowChartPunchedTape"),
        123 => d("flowChartSummingJunction"),
        124 => d("flowChartOr"),
        125 => d("flowChartCollate"),
        126 => d("flowChartSort"),
        127 => d("flowChartExtract"),
        128 => d("flowChartMerge"),
        129 => d("flowChartOfflineStorage"),
        130 => d("flowChartOnlineStorage"),
        131 => d("flowChartMagneticTape"),
        132 => d("flowChartMagneticDisk"),
        133 => d("flowChartMagneticDrum"),
        134 => d("flowChartDisplay"),
        135 => d("flowChartDelay"),
        176 => d("flowChartAlternateProcess"),
        177 => d("flowChartOffpageConnector"),
        178 => d("callout1"),
        179 => d("accentCallout1"),
        180 => d("borderCallout1"),
        181 => d("accentBorderCallout1"),
        182 => d("leftRightUpArrow"),
        183 => da("sun", Lin0),
        184 => da("moon", Lin0),
        185 => d("bracketPair"),
        186 => d("bracePair"),
        187 => da("star4", Seal),
        188 => d("doubleWave"),
        189 => d("actionButtonBlank"),
        190 => d("actionButtonHome"),
        191 => d("actionButtonHelp"),
        192 => d("actionButtonInformation"),
        193 => d("actionButtonForwardNext"),
        194 => d("actionButtonBackPrevious"),
        195 => d("actionButtonEnd"),
        196 => d("actionButtonBeginning"),
        197 => d("actionButtonReturn"),
        198 => d("actionButtonDocument"),
        199 => d("actionButtonSound"),
        200 => d("actionButtonMovie"),
        201 | 202 => d("rect"),
        _ => return None,
    })
}

/// DrawingML defaults (presetShapeDefinitions) of the adjusts converted (`DML_DEFAULTS`).
fn dml_defaults(prst: &str) -> &'static [f64] {
    match prst {
        "roundRect" | "plaque" => &[16667.0],
        "triangle" => &[50000.0],
        "parallelogram" | "trapezoid" | "hexagon" | "plus" | "cube" | "can" | "donut" | "sun" => &[25000.0],
        "octagon" => &[29289.0],
        "rightArrow" | "leftArrow" | "upArrow" | "downArrow" | "leftRightArrow" | "upDownArrow" | "stripedRightArrow"
        | "notchedRightArrow" => &[50000.0, 50000.0],
        "homePlate" | "chevron" | "moon" => &[50000.0],
        "bevel" | "star4" => &[12500.0],
        "smileyFace" => &[4653.0],
        "star8" => &[38250.0],
        "star16" | "star24" | "star32" => &[37500.0],
        "wedgeRectCallout" | "wedgeEllipseCallout" | "cloudCallout" => &[-20833.0, 62500.0],
        "wedgeRoundRectCallout" => &[-20833.0, 62500.0, 16667.0],
        "leftBracket" | "rightBracket" => &[8333.0],
        "leftBrace" | "rightBrace" => &[8333.0, 50000.0],
        _ => &[],
    }
}

/// `K = 100000 / 21600`.
const K: f64 = 100000.0 / 21600.0;

/// `ratio(w, h, horiz)`: `w / ss` or `h / ss`, `ss = Math.min(w, h) || 1`.
fn ratio(w: f64, h: f64, horiz: bool) -> f64 {
    let m = min(w, h);
    let ss = if truthy(m) { m } else { 1.0 };
    (if horiz { w } else { h }) / ss
}

/// The converted values (`def.adj(a, w, h)`): at most two, `None` = undefined.
fn convert(adj: Adj, a: [Option<f64>; 2], w: f64, h: f64) -> ([Option<f64>; 2], usize) {
    let [a0, a1] = a;
    let one = |v: Option<f64>| ([v, None], 1);
    match adj {
        Adj::Fixed => ([None, None], 0),
        Adj::Lin0 => one(a0.map(|v| v * K)),
        Adj::OffW => one(a0.map(|v| v * K * ratio(w, h, true))),
        Adj::TipW => one(a0.map(|v| (21600.0 - v) * K * ratio(w, h, true))),
        Adj::Seal => one(a0.map(|v| ((10800.0 - v) / 10800.0) * 50000.0)),
        Adj::Vert => one(a0.map(|v| v * K * ratio(w, h, false))),
        Adj::Smiley => one(a0.map(|v| ((v - 16515.0) / (17520.0 - 16515.0)) * 4653.0)),
        Adj::ArrowH(head_at_start) | Adj::ArrowV(head_at_start) => {
            let horiz = matches!(adj, Adj::ArrowH(_));
            (
                [
                    a1.map(|v| ((21600.0 - 2.0 * v) / 21600.0) * 100000.0),
                    a0.map(|v| (if head_at_start { v } else { 21600.0 - v }) * K * ratio(w, h, horiz)),
                ],
                2,
            )
        }
        Adj::Wedge => ([a0.map(|v| (v - 10800.0) * K), a1.map(|v| (v - 10800.0) * K)], 2),
        Adj::Brace => ([a0.map(|v| v * K * ratio(w, h, false)), a1.map(|v| v * K)], 2),
    }
}

/// Classic WordArt types (`isWordArtType`): text effects drawn as text in a rectangle.
pub fn is_word_art_type(spt: u16) -> bool {
    (24..=31).contains(&spt) || (136..=175).contains(&spt)
}

/// Connector types (`isConnectorType`): drawn as a connection shape.
pub fn is_connector_type(spt: u16) -> bool {
    (32..=40).contains(&spt)
}

/// Does the type have an equivalent preset (`hasPreset`, Sheet)? WordArt types do not.
pub fn has_preset(spt: u16) -> bool {
    def(spt).is_some()
}

/// The ten binary adjusts of a shape (`adjustValue` .. `adjust10Value`, ids 0x147..0x150) as
/// the callers build them (`v | 0`: the value as a signed 32-bit integer).
pub fn raw_adjust(props: &PropTable<'_>) -> [Option<f64>; 10] {
    let mut out = [None; 10];
    for (i, slot) in (0u16..).zip(out.iter_mut()) {
        *slot = props.value(0x147 + i).map(|v| f64::from(i32::from_le_bytes(v.to_le_bytes())));
    }
    out
}

/// The preset of a type and its binary adjusts (`presetFor`; Sheet `presetGeomXml` without the
/// XML): `None` when there is no equivalent preset (a freeform: use the geometry of the FOPT).
/// `adjust[i]` is `adjustValue + i` (`None` = the shape's default); `w`, `h` are the box (only
/// its aspect ratio matters).
///
/// When the conversion gives at least one value, every adjust of the converted list is emitted
/// (DrawingML adjusts are positional), the missing ones from the DrawingML defaults.
pub fn preset_for(spt: u16, adjust: &[Option<f64>], w: f64, h: f64, variant: PresetVariant) -> Option<PresetResult> {
    if variant == PresetVariant::WordSlides && is_word_art_type(spt) {
        return Some(PresetResult { prst: "rect", av: [None, None], flip_v: false });
    }
    let def = def(spt)?;
    let mut out = PresetResult { prst: def.prst, av: [None, None], flip_v: def.flip_v };
    if def.adj == Adj::Fixed {
        return Some(out);
    }
    let (w, h) = match variant {
        PresetVariant::WordSlides => (w, h),
        PresetVariant::Sheet => (if truthy(w) { w } else { 1.0 }, if truthy(h) { h } else { 1.0 }),
    };
    let at = |i: usize| adjust.get(i).copied().flatten();
    let a = if def.wedge_defaults {
        [Some(at(0).unwrap_or(WEDGE_DEFAULTS[0])), Some(at(1).unwrap_or(WEDGE_DEFAULTS[1]))]
    } else {
        [at(0), at(1)]
    };
    let (vals, n) = convert(def.adj, a, w, h);
    let vals = vals.get(..n).unwrap_or(&[]);
    if vals.iter().any(Option::is_some) {
        let dflt = dml_defaults(def.prst);
        let mut k = 0;
        for (i, v) in vals.iter().enumerate() {
            let Some(val) = v.or_else(|| dflt.get(i).copied()) else { continue };
            let name = match (n, i) {
                (1, _) => "adj",
                (_, 0) => "adj1",
                _ => "adj2",
            };
            if let Some(slot) = out.av.get_mut(k) {
                *slot = Some(PresetAdjust { name, val: round(val) });
                k += 1;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn av(r: &PresetResult) -> Vec<(&'static str, f64)> {
        r.av().map(|a| (a.name, a.val)).collect()
    }

    #[test]
    fn known_conversions() {
        // seal8 2538 <-> star8 38250
        let r = preset_for(58, &[Some(2538.0)], 1.0, 1.0, PresetVariant::WordSlides).unwrap();
        assert_eq!((r.prst, av(&r)), ("star8", vec![("adj", 38250.0)]));
        // roundRect 1329 -> 6153
        let r = preset_for(2, &[Some(1329.0)], 5.0, 7.0, PresetVariant::WordSlides).unwrap();
        assert_eq!(av(&r), vec![("adj", 6153.0)]);
        // chevron 13800 @ 1037 x 749 (the TS gives 49996; the .pptx twin of the corpus says 50000)
        let r = preset_for(55, &[Some(13800.0)], 1037.0, 749.0, PresetVariant::WordSlides).unwrap();
        assert_eq!(av(&r), vec![("adj", 49996.0)]);
        // callouts always emit (binary defaults)
        let r = preset_for(62, &[], 1.0, 1.0, PresetVariant::WordSlides).unwrap();
        assert_eq!(av(&r), vec![("adj1", -43750.0), ("adj2", 70000.0)]);
        // one arrow adjust present: the other from the DrawingML defaults
        let r = preset_for(13, &[None, Some(5400.0)], 2.0, 1.0, PresetVariant::WordSlides).unwrap();
        assert_eq!(av(&r), vec![("adj1", 50000.0), ("adj2", 50000.0)]);
        assert!(preset_for(8, &[], 1.0, 1.0, PresetVariant::Sheet).unwrap().flip_v);
    }

    #[test]
    fn word_art_and_unknown() {
        assert_eq!(preset_for(136, &[], 1.0, 1.0, PresetVariant::WordSlides).map(|r| r.prst), Some("rect"));
        assert!(preset_for(136, &[], 1.0, 1.0, PresetVariant::Sheet).is_none());
        assert!(preset_for(0, &[], 1.0, 1.0, PresetVariant::WordSlides).is_none());
        assert!(preset_for(203, &[], 1.0, 1.0, PresetVariant::WordSlides).is_none());
        assert!(!has_preset(24) && has_preset(202));
    }
}
