//! The Slides host of the chart engine (`uo-charts`, frontier C-6): the `chart-engine.ts` bridge (`parseChart`) as a
//! [`ChartHost`] over the chart's theme, the measurement protocol over the [`MeasureTable`], the parsed-chart cache and the
//! per-model bounds cache (memory gate M5).
//!
//! - **Host** (`parseChart`, chart-engine.ts:56–95): colours through `colorFromDml` over the theme with the Office palette
//!   merged under it (`FALLBACK`, so a theme without `accent1…` still paints); `fillStyle(idx, phClr)` re-enters the paint parser
//!   over `theme.fillStyles[idx − 1]` with `phClr` set; a blip is the image of `chartImages[r:embed]`; fonts, file format and no
//!   theme line widths (the Slides host passes none: the PowerPoint rule).
//! - **Measurement**: a chart question is a [`MeasureReq`] the slide batches with its text questions: a width is
//!   `Width(StringKey { kerning: Current, letter_spacing_px: 0 })` (the painter restores `auto` and `0px` after every run), the
//!   font box of `'Hg'` is `Metrics` of the same key. The canvas default font (`10px sans-serif`, CH-5) is the face with an empty
//!   family at 10 px: the shell measures it as that string.
//! - **Caches**: [`ChartCache`] keeps up to [`MAX_CHART_MODELS`] parsed charts (`PaintResources.charts`, unbounded in the TS) and,
//!   per model, up to [`MAX_BOUNDS_KEYS`] part-bounds boxes (`boundsCache`, unbounded in the TS), both least-recently-used.

use std::sync::Arc;

use uo_charts::emit::Rect;
use uo_charts::host::{BlipKey, ChartHost, ChartImageInfo, ColorSource, FileFormat, HostFonts};
use uo_charts::measure::{ChartFace, ChartMeasure, ChartMeasureReq, FontBox};
use uo_charts::model::{ChartColor, ChartSpace, Paint};
use uo_charts::{parse_chart_space, parse_paint_node};
use uo_opc::xml::{Element, ParseOptions, parse};

use crate::color::CssColor;
use crate::model::{ChartFormat, ChartImage, ChartRef};
use crate::source::PartSource;
use crate::text::{Bits, FaceKey, Kerning, MeasureReq, MeasureTable, StringKey};
use crate::theme::{JsRecord, Theme, color_in_rules};

/// Parsed charts kept (M5; the TS map is unbounded).
pub const MAX_CHART_MODELS: usize = 32;
/// Part-bounds boxes kept per chart (M5; the TS map is unbounded).
pub const MAX_BOUNDS_KEYS: usize = 64;

/// `FALLBACK` of chart-engine.ts:65: the Office palette under the theme's colours.
const FALLBACK: [(&str, &str); 14] = [
    ("accent1", "#4472C4"),
    ("accent2", "#ED7D31"),
    ("accent3", "#A5A5A5"),
    ("accent4", "#FFC000"),
    ("accent5", "#5B9BD5"),
    ("accent6", "#70AD47"),
    ("tx1", "#000000"),
    ("bg1", "#FFFFFF"),
    ("dk1", "#000000"),
    ("lt1", "#FFFFFF"),
    ("tx2", "#44546A"),
    ("bg2", "#E7E6E6"),
    ("dk2", "#44546A"),
    ("lt2", "#E7E6E6"),
];

/// `th.colors = { ...FALLBACK, ...theme.colors }`.
fn merged_colors(theme: &Theme) -> JsRecord<CssColor> {
    let mut c: JsRecord<CssColor> = JsRecord::default();
    for (k, v) in FALLBACK {
        c.set(k, CssColor::new(v));
    }
    for (k, v) in theme.colors.iter() {
        c.set(k, v.clone());
    }
    c
}

/// `colorFromDml(<a:schemeClr val=name/>, theme)`: the synthetic node the parser asks for (accent1–6, tx1, bg1).
fn scheme_color(name: &str, colors: &JsRecord<CssColor>) -> CssColor {
    const STD: [(&str, &str); 4] = [("tx1", "dk1"), ("bg1", "lt1"), ("tx2", "dk2"), ("bg2", "lt2")];
    let alias = STD.iter().find(|(a, _)| *a == name).map_or(name, |(_, s)| *s);
    colors.get(name).or_else(|| colors.get(alias)).cloned().unwrap_or_else(|| CssColor::new("#000000"))
}

/// `ts_sys_clr`: `a:sysClr` from `lastClr` (the TS); else PowerPoint's system colour (`theme::system_color`).
fn color_of(src: ColorSource<'_>, colors: &JsRecord<CssColor>, ts_sys_clr: bool) -> Option<ChartColor> {
    let c = match src {
        ColorSource::Element(el) => color_in_rules(Some(el), colors, ts_sys_clr)?,
        ColorSource::Scheme(name) => scheme_color(name, colors),
    };
    Some(ChartColor::new(c.as_str()))
}

/// The index of the first `chartImages` entry with the same picture (the TS key is the part name, so two relationship ids of
/// one part are one image).
fn canonical(images: &[ChartImage], i: usize) -> usize {
    let Some(me) = images.get(i) else { return i };
    images.iter().position(|o| o.image.media == me.image.media).unwrap_or(i)
}

/// The host of one chart (`ChartRef`).
pub struct SlidesHost<'a> {
    theme: &'a Theme,
    colors: JsRecord<CssColor>,
    images: &'a [ChartImage],
    format: Option<ChartFormat>,
}

impl<'a> SlidesHost<'a> {
    pub fn new(chart: &'a ChartRef) -> SlidesHost<'a> {
        SlidesHost {
            theme: &chart.theme,
            colors: merged_colors(&chart.theme),
            images: chart.images.as_deref().unwrap_or(&[]),
            format: chart.format,
        }
    }
}

impl ChartHost for SlidesHost<'_> {
    fn color(&self, src: ColorSource<'_>) -> Option<ChartColor> {
        color_of(src, &self.colors, self.theme.rules.ts_sys_clr)
    }

    fn fonts(&self) -> HostFonts<'_> {
        match &self.theme.fonts {
            Some(f) => HostFonts { major: f.major.as_deref(), minor: f.minor.as_deref() },
            None => HostFonts::default(),
        }
    }

    fn has_fill_style(&self) -> bool {
        true
    }

    fn fill_style(&self, idx: u32, ph_clr: &str) -> Option<Paint> {
        let el = self.theme.styles.as_ref()?.fill_style(usize::try_from(idx.checked_sub(1)?).ok()?)?;
        let mut colors = self.colors.clone();
        colors.set("phClr", CssColor::new(ph_clr));
        let ts = self.theme.rules.ts_sys_clr;
        parse_paint_node(el, &|src| color_of(src, &colors, ts))
    }

    fn blip(&self, blip: Element<'_>) -> Option<BlipKey> {
        let rid = blip.attr("r:embed").or_else(|| blip.attr("embed"))?;
        if rid.is_empty() {
            return None;
        }
        let i = self.images.iter().position(|c| &*c.r_id == rid)?;
        Some(BlipKey(u32::try_from(canonical(self.images, i)).ok()?))
    }

    fn file_format(&self) -> Option<FileFormat> {
        self.format.map(|f| match f {
            ChartFormat::Mso2007 => FileFormat::Mso2007,
            ChartFormat::Modern => FileFormat::Modern,
        })
    }

    fn user_shapes(&self) -> Option<Element<'_>> {
        None
    }

    fn theme_line_widths(&self) -> &[f64] {
        &[]
    }
}

/// What the host knows about a picture of the chart in the current pass.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ImgAnswer {
    /// not resolved yet (the engine has not asked, or its size was requested)
    Pending,
    /// the package has no such picture, or it is not a plain decodable image (the TS `image()` returning `undefined`)
    Unavailable,
    Ready(ChartImageInfo),
}

/// [`ChartMeasure`] over the slide's table: widths and the font box are lookups, pictures are answers the host resolved.
pub struct TableMeasure<'a> {
    pub table: &'a MeasureTable,
    pub images: &'a [ImgAnswer],
}

/// The key the table holds a chart face under: the empty family is the canvas default `10px sans-serif` (CH-5), measured at 10 px.
fn face_key(face: &ChartFace, size_px: f64) -> ((&str, bool, bool), f64) {
    if face.family.is_empty() { (("", false, false), 10.0) } else { ((&face.family, face.bold, face.italic), size_px) }
}

impl ChartMeasure for TableMeasure<'_> {
    fn width(&self, face: &ChartFace, size_px: f64, text: &str) -> Option<f64> {
        let (f, s) = face_key(face, size_px);
        self.table.width(f, s, Kerning::Current, 0.0, text)
    }

    fn font_box(&self, face: &ChartFace, size_px: f64) -> Option<FontBox> {
        let (f, s) = face_key(face, size_px);
        self.table.metrics(f, s, Kerning::Current, 0.0, "Hg").map(|m| FontBox { ascent: m.font_ascent, descent: m.font_descent })
    }

    fn has_font_bounding_box(&self) -> bool {
        self.table.caps.font_bounding_box
    }

    fn image(&self, key: BlipKey) -> Option<Option<ChartImageInfo>> {
        match self.images.get(usize::try_from(key.0).ok()?)? {
            ImgAnswer::Pending => None,
            ImgAnswer::Unavailable => Some(None),
            ImgAnswer::Ready(i) => Some(Some(*i)),
        }
    }
}

/// A chart question as the shell's request (`None` for a picture: its size is asked through the media table).
pub fn request_of(r: &ChartMeasureReq) -> Option<MeasureReq> {
    let key = |face: &ChartFace, size_px: f64, text: &str| {
        let (_, s) = face_key(face, size_px);
        let face = if face.family.is_empty() {
            FaceKey { family: "".into(), bold: false, italic: false }
        } else {
            FaceKey { family: face.family.clone(), bold: face.bold, italic: face.italic }
        };
        StringKey { face, size_px: Bits::of(s), kerning: Kerning::Current, letter_spacing_px: Bits::of(0.0), text: text.into() }
    };
    match r {
        ChartMeasureReq::Width { face, size_px, text } => Some(MeasureReq::Width(key(face, *size_px, text))),
        ChartMeasureReq::FontBox { face, size_px } => Some(MeasureReq::Metrics(key(face, *size_px, "Hg"))),
        ChartMeasureReq::Image { .. } => None,
    }
}

// ---------------------------------------------------------------------------------------------------- caches

struct Bounds {
    key: Box<str>,
    rect: Option<Rect>,
}

struct Entry {
    key: Box<str>,
    space: Arc<ChartSpace>,
    /// least recently used first
    bounds: Vec<Bounds>,
}

/// The parsed charts (least recently used last) with their bounds caches.
#[derive(Default)]
pub struct ChartCache {
    entries: Vec<Entry>,
}

impl std::fmt::Debug for ChartCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartCache").field("models", &self.entries.len()).finish()
    }
}

impl ChartCache {
    pub fn clear(&mut self) {
        self.entries = Vec::new();
    }

    /// Models held and the most bounds keys any one holds.
    pub fn sizes(&self) -> (usize, usize) {
        (self.entries.len(), self.entries.iter().map(|e| e.bounds.len()).max().unwrap_or(0))
    }

    fn touch(&mut self, key: &str) -> Option<&mut Entry> {
        let i = self.entries.iter().position(|e| &*e.key == key)?;
        let e = self.entries.remove(i);
        self.entries.push(e);
        self.entries.last_mut()
    }

    pub fn get(&mut self, key: &str) -> Option<Arc<ChartSpace>> {
        self.touch(key).map(|e| e.space.clone())
    }

    pub fn put(&mut self, key: &str, space: Arc<ChartSpace>) {
        if self.touch(key).is_some() {
            return;
        }
        if self.entries.len() >= MAX_CHART_MODELS {
            self.entries.remove(0);
        }
        self.entries.push(Entry { key: key.into(), space, bounds: Vec::new() });
    }

    /// `boundsCache.get(model)?.get(key)`: the box (or `None` = nothing drawn) of an earlier call, `None` when never asked.
    pub fn bounds_get(&mut self, model: &str, bkey: &str) -> Option<Option<Rect>> {
        let e = self.touch(model)?;
        let i = e.bounds.iter().position(|b| &*b.key == bkey)?;
        let b = e.bounds.remove(i);
        let r = b.rect;
        e.bounds.push(b);
        Some(r)
    }

    pub fn bounds_put(&mut self, model: &str, bkey: &str, rect: Option<Rect>) {
        let Some(e) = self.touch(model) else { return };
        if let Some(i) = e.bounds.iter().position(|b| &*b.key == bkey) {
            e.bounds.remove(i);
        } else if e.bounds.len() >= MAX_BOUNDS_KEYS {
            e.bounds.remove(0);
        }
        e.bounds.push(Bounds { key: bkey.into(), rect });
    }
}

/// The cache key of a chart (`sh.chartXml` + the image parts in the TS): the part and the media of its pictures.
pub fn model_key(chart: &ChartRef) -> String {
    let mut k = String::from(&*chart.part);
    if let Some(images) = chart.images.as_deref() {
        k.push('\0');
        for (i, c) in images.iter().enumerate() {
            if i > 0 {
                k.push('|');
            }
            k.push_str(&c.image.media.0.to_string());
        }
    }
    k
}

/// The parsed chart: from the cache, else `parseChart` (a part that does not parse is an empty `c:chartSpace`, as the TS's catch).
/// `None` only when even the empty document does not parse (never: the literal is well formed).
pub fn model_of(src: &dyn PartSource, caches: &mut ChartCache, chart: &ChartRef) -> Option<Arc<ChartSpace>> {
    let key = model_key(chart);
    if let Some(m) = caches.get(&key) {
        return Some(m);
    }
    let host = SlidesHost::new(chart);
    let space = match src.xml(&chart.part) {
        Ok(Some(doc)) => parse_chart_space(doc.root(), &host),
        _ => {
            let doc = parse("<c:chartSpace/>".to_owned(), &ParseOptions::LENIENT).ok()?;
            parse_chart_space(doc.root(), &host)
        }
    };
    let space = Arc::new(space);
    caches.put(&key, space.clone());
    Some(space)
}

/// The bounds key of the TS (`${sel.key}@${x.toFixed(2)},…,${pxPerPt}`).
pub fn bounds_key(sel_key: &str, r: &Rect, px_per_pt: f64) -> String {
    use uo_jsnum::format::{to_fixed, to_js_string};
    let f = |v: f64| to_fixed(v, 2).unwrap_or_default();
    format!("{sel_key}@{},{},{},{},{}", f(r.x), f(r.y), f(r.w), f(r.h), to_js_string(px_per_pt))
}
