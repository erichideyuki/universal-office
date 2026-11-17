//! `PdfDocument` of `pdf-writer/document.ts`: the fonts, images and shadings the pages share,
//! the pages and the outline, written by [`PdfDocument::build`].
//!
//! **Object numbers and file order are the TS ones** (they are parity-relevant): the objects made
//! while pages are drawn (an image's soft mask before the image, a shading's function before its
//! dictionary) take the next number as they are made and go to the sink at once. At `build`:
//! every font (FontFile2, descriptor, CIDFont, ToUnicode, Type0), the catalog, the pages node,
//! one number for each page, then for each page its content stream and its link annotations, the
//! page object, and last the outline root and its items. The catalog is defined last, so the
//! objects after it are held until then: the page contents, compressed since their page ended.

use std::fmt::Write as _;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uo_jsnum::text::trim;

use crate::error::{PdfError, PdfLimits, Result};
use crate::font::{EmbeddedFont, FontProgram};
use crate::num::{F, J, utf16be_hex};
use crate::page::PageWriter;
use crate::sink::PdfSink;
use crate::types::{FontId, JpegImage, PdfImage, PdfLink, PdfOutlineItem, PdfShading, ShadingGeom};
use crate::uri::safe_pdf_uri;
use crate::writer::{PdfWriter, StreamBody};

/// The target of a link, as it will be written.
pub(crate) enum LinkTarget {
    /// the `/URI` string, already made safe
    Uri(String),
    Dest {
        page_index: i64,
        top_pt: f64,
    },
}

pub(crate) struct LinkRec {
    pub x_pt: f64,
    pub y_top_pt: f64,
    pub w_pt: f64,
    pub h_pt: f64,
    pub target: LinkTarget,
}

impl LinkRec {
    /// `addLink`: a link whose address is not http, https or mailto is dropped (so is a link with
    /// no address and no page).
    pub(crate) fn from_link(link: &PdfLink) -> Option<LinkRec> {
        let target = match &link.uri {
            Some(uri) => LinkTarget::Uri(safe_pdf_uri(Some(uri))?),
            None => LinkTarget::Dest { page_index: link.page_index?, top_pt: link.dest_top_pt.unwrap_or(0.0) },
        };
        Some(LinkRec { x_pt: link.x_pt, y_top_pt: link.y_top_pt, w_pt: link.w_pt, h_pt: link.h_pt, target })
    }
}

/// A finished page: its size, its content compressed (or not) and its links.
pub(crate) struct PageRec {
    pub width_pt: f64,
    pub height_pt: f64,
    pub content: StreamBody,
    pub links: Vec<LinkRec>,
}

/// A PDF document. The shared resources are created on the document (`embed_*`, `add_shading`),
/// the pages are drawn through [`PageWriter`] (`add_page`), and `build` finishes the file.
pub struct PdfDocument<S: PdfSink> {
    pub(crate) w: PdfWriter<S>,
    pub(crate) limits: PdfLimits,
    fonts: Vec<EmbeddedFont>,
    /// the object number of each image, by the index of its `Im<index>` name
    images: Vec<u32>,
    shadings: Vec<u32>,
    /// the opacity states in creation order (`GS<index>`): the rounded `ca` and `CA`
    alpha_states: Vec<(f64, f64)>,
    alpha_index: FxHashMap<(u64, u64), u32>,
    /// the extra graphics states (`GX<index>`: blend mode, soft mask...), de-duplicated by their entries
    ext_states: Vec<String>,
    ext_index: FxHashMap<String, u32>,
    /// the form XObjects (`Fm<index>`) and tiling patterns (`Pt<index>`): their object numbers
    forms: Vec<u32>,
    patterns: Vec<u32>,
    /// the `/Resources` object forms and patterns point to, allocated at the first one and defined at `build`
    res_obj: Option<u32>,
    pub(crate) pages: Vec<PageRec>,
    outline: Vec<PdfOutlineItem>,
    /// a limit or a sink failure that a page met: the document cannot be built
    pub(crate) failed: Option<PdfError>,
}

impl<S: PdfSink> PdfDocument<S> {
    /// A document that writes to `sink`; `compress` is the TS `compress` option (on by default there).
    pub fn new(sink: S, compress: bool) -> Self {
        PdfDocument::with_limits(sink, compress, PdfLimits::DEFAULT)
    }

    pub fn with_limits(sink: S, compress: bool, limits: PdfLimits) -> Self {
        PdfDocument {
            w: PdfWriter::with_limits(sink, compress, &limits),
            limits,
            fonts: Vec::new(),
            images: Vec::new(),
            shadings: Vec::new(),
            alpha_states: Vec::new(),
            alpha_index: FxHashMap::default(),
            ext_states: Vec::new(),
            ext_index: FxHashMap::default(),
            forms: Vec::new(),
            patterns: Vec::new(),
            res_obj: None,
            pages: Vec::new(),
            outline: Vec::new(),
            failed: None,
        }
    }

    /// The low-level writer, for objects of a host's own (an object numbered here is written in
    /// number order with the document's).
    pub fn writer(&mut self) -> &mut PdfWriter<S> {
        &mut self.w
    }

    /// An entry of the navigation panel, in document order (levels 1, 2...). An entry with a blank
    /// title is ignored.
    pub fn add_outline(&mut self, item: PdfOutlineItem) {
        if trim(&item.title).is_empty() {
            return;
        }
        self.outline.push(PdfOutlineItem { level: item.level.max(1), ..item });
    }

    /// Embeds a TrueType font: its bytes (shared, not copied) and the queries of its program.
    pub fn embed_font(&mut self, name: &str, bytes: Arc<[u8]>, program: impl FontProgram + Send + 'static) -> FontId {
        let index = u32::try_from(self.fonts.len()).unwrap_or(u32::MAX);
        self.fonts.push(EmbeddedFont::new(index, name, bytes, Box::new(program)));
        FontId(index)
    }

    pub fn font(&self, id: FontId) -> Option<&EmbeddedFont> {
        self.fonts.get(usize::try_from(id.0).ok()?)
    }

    pub fn font_mut(&mut self, id: FontId) -> Option<&mut EmbeddedFont> {
        self.fonts.get_mut(usize::try_from(id.0).ok()?)
    }

    /// Embeds a baseline JPEG as it is (`DCTDecode`), with an optional soft mask.
    pub fn embed_jpeg(&mut self, jpeg: &JpegImage<'_>) -> Result<PdfImage> {
        let cs = match jpeg.components {
            1 => "/DeviceGray",
            4 => "/DeviceCMYK",
            _ => "/DeviceRGB",
        };
        // `decode`: a line per component (brightness and contrast without decoding the JPEG)
        let mut decode = String::new();
        if jpeg.components == 4 && jpeg.adobe_inverted {
            decode.push_str(" /Decode [1 0 1 0 1 0 1 0]");
        } else if !jpeg.decode.is_empty() {
            let _ = write!(decode, " /Decode [{}]", crate::num::FList(jpeg.decode));
        }
        let mut sm = String::new();
        if let Some(mask) = &jpeg.smask {
            let dict = format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8",
                mask.width, mask.height
            );
            let n = self.w.add_stream(&dict, mask.alpha, true)?;
            let _ = write!(sm, " /SMask {n} 0 R");
        }
        let dict = format!(
            "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode{decode}{sm}",
            jpeg.width_px, jpeg.height_px
        );
        // already compressed
        let obj_num = self.w.add_stream(&dict, jpeg.bytes, false)?;
        Ok(self.push_image(obj_num))
    }

    /// Embeds raw RGBA pixels as DeviceRGB with FlateDecode. When the alpha is not all 255 the
    /// channel becomes a DeviceGray `/SMask`, so the cut-out of a transparent PNG survives.
    ///
    /// The TS sizes its arrays from the dimensions and leaves what the pixels do not fill at zero;
    /// a width and height that claim more pixels than `rgba` holds are refused here instead of
    /// allocating from a number the file declared.
    pub fn embed_rgba(&mut self, rgba: &[u8], width_px: u32, height_px: u32) -> Result<PdfImage> {
        let pixels = u64::from(width_px) * u64::from(height_px);
        if pixels.saturating_mul(4) > rgba.len() as u64 {
            return Err(PdfError::Invalid("image dimensions exceed the pixel data"));
        }
        let pixels = usize::try_from(pixels).map_err(|_| PdfError::Invalid("image dimensions exceed the pixel data"))?;
        // `a !== 255`: a pixel cut short (no alpha byte) is not 255 either
        let has_alpha = rgba.chunks(4).any(|px| px.get(3) != Some(&255));
        let mut smask = String::new();
        if has_alpha {
            let mut alpha = vec![0u8; pixels];
            for (out, px) in alpha.iter_mut().zip(rgba.chunks(4)) {
                *out = px.get(3).copied().unwrap_or(0);
            }
            let dict =
                format!("/Type /XObject /Subtype /Image /Width {width_px} /Height {height_px} /ColorSpace /DeviceGray /BitsPerComponent 8");
            let n = self.w.add_stream(&dict, &alpha, true)?;
            let _ = write!(smask, " /SMask {n} 0 R");
        }
        let mut rgb = vec![0u8; pixels.checked_mul(3).ok_or(PdfError::Invalid("image dimensions exceed the pixel data"))?];
        for (out, px) in rgb.as_chunks_mut::<3>().0.iter_mut().zip(rgba.chunks(4)) {
            for (o, i) in out.iter_mut().zip(0..3) {
                *o = px.get(i).copied().unwrap_or(0);
            }
        }
        let dict = format!(
            "/Type /XObject /Subtype /Image /Width {width_px} /Height {height_px} /ColorSpace /DeviceRGB /BitsPerComponent 8{smask}"
        );
        let obj_num = self.w.add_stream(&dict, &rgb, true)?;
        Ok(self.push_image(obj_num))
    }

    fn push_image(&mut self, obj_num: u32) -> PdfImage {
        let index = u32::try_from(self.images.len()).unwrap_or(u32::MAX);
        self.images.push(obj_num);
        PdfImage { index, obj_num }
    }

    /// An axial (ShadingType 2, from point 0 to point 1) or radial (ShadingType 3, concentric
    /// circles of radius 0 to `r`) shading with sampled colours (a type 0 function of 8-bit RGB
    /// samples, as the Word writes it). Coordinates in points with y from the TOP; the end colours
    /// extend.
    pub fn add_shading(&mut self, geom: &ShadingGeom, samples: &[u8], page_height_pt: f64) -> Result<PdfShading> {
        let n = (samples.len() / 3).max(2);
        let mut dict = String::new();
        let _ =
            write!(dict, "/FunctionType 0 /Domain [0 1] /Range [0 1 0 1 0 1] /Size [{n}] /BitsPerSample 8 /Order 1 /Encode [0 {}]", n - 1);
        // `samples.subarray(0, n * 3)` clamps to the data
        let data = samples.get(..n * 3).unwrap_or(samples);
        let func = self.w.add_stream(&dict, data, true)?;
        let h = page_height_pt;
        dict.clear();
        let coords = match *geom {
            ShadingGeom::Axial { x0, y0, x1, y1 } => {
                format!("/ShadingType 2 /Coords [{} {} {} {}]", F(x0), F(h - y0), F(x1), F(h - y1))
            }
            ShadingGeom::Radial { x0, y0, r } => {
                format!("/ShadingType 3 /Coords [{} {} 0 {} {} {}]", F(x0), F(h - y0), F(x0), F(h - y0), F(r))
            }
        };
        let _ = write!(dict, "<< /ColorSpace /DeviceRGB {coords} /Extend [true true] /Function {func} 0 R >>");
        let obj_num = self.w.add_dict(&dict)?;
        let index = u32::try_from(self.shadings.len()).unwrap_or(u32::MAX);
        self.shadings.push(obj_num);
        Ok(PdfShading { index, obj_num })
    }

    /// An extra graphics state (`GX<index>`, an `/ExtGState` entry): `entries` are the dictionary
    /// entries without `<<` `>>` (`/BM /Multiply /ca 0.5`, a `/SMask`). Equal entries share a state.
    /// For the hosts that write their own content streams (the Slides painter): Write never calls it.
    pub fn add_ext_gstate(&mut self, entries: &str) -> u32 {
        if let Some(i) = self.ext_index.get(entries) {
            return *i;
        }
        let index = u32::try_from(self.ext_states.len()).unwrap_or(u32::MAX);
        self.ext_states.push(entries.to_owned());
        self.ext_index.insert(entries.to_owned(), index);
        index
    }

    /// The object number of the `/Resources` dictionary that forms and patterns use (the document
    /// resources, defined at `build`): allocated at the first need.
    fn resources_ref(&mut self) -> Result<u32> {
        if let Some(n) = self.res_obj {
            return Ok(n);
        }
        let n = self.w.alloc()?;
        self.res_obj = Some(n);
        Ok(n)
    }

    /// A form XObject (`Fm<index>`, drawn with `/Fm<index> Do`): `content` is its content stream in
    /// its own space, `bbox` its clip. With `group` (the colour space name, `DeviceRGB` or
    /// `DeviceGray`) it is a transparency group (`/S /Transparency`, isolated). Written at once;
    /// its resources are the document's (a reference defined at `build`).
    pub fn add_form(&mut self, bbox: [f64; 4], content: Vec<u8>, group: Option<&str>) -> Result<u32> {
        let res = self.resources_ref()?;
        let num = self.w.alloc()?;
        let body = self.w.compress_body(content, true)?;
        let group = match group {
            Some(cs) => format!(" /Group << /S /Transparency /CS /{cs} /I true >>"),
            None => String::new(),
        };
        let dict = format!(
            "/Type /XObject /Subtype /Form /BBox [{} {} {} {}]{group} /Resources {res} 0 R",
            F(bbox[0]),
            F(bbox[1]),
            F(bbox[2]),
            F(bbox[3])
        );
        self.w.define_body(num, &dict, body)?;
        let index = u32::try_from(self.forms.len()).unwrap_or(u32::MAX);
        self.forms.push(num);
        Ok(index)
    }

    /// The object number of form `index` (for a soft mask's `/G`).
    pub fn form_object(&self, index: u32) -> Option<u32> {
        self.forms.get(usize::try_from(index).ok()?).copied()
    }

    /// A coloured tiling pattern (`Pt<index>`, set with `/Pattern cs /Pt<index> scn`): `entries` are
    /// the dictionary entries that describe it (`/BBox`, `/XStep`, `/YStep`, `/Matrix`), `content` the
    /// cell. The matrix maps pattern space to the default space of the content stream that uses it.
    pub fn add_pattern(&mut self, entries: &str, content: Vec<u8>) -> Result<u32> {
        let res = self.resources_ref()?;
        let num = self.w.alloc()?;
        let body = self.w.compress_body(content, true)?;
        let dict = format!("/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 {entries} /Resources {res} 0 R");
        self.w.define_body(num, &dict, body)?;
        let index = u32::try_from(self.patterns.len()).unwrap_or(u32::MAX);
        self.patterns.push(num);
        Ok(index)
    }

    /// A shading with sampled colours (a type 0 function of 8-bit samples) in the coordinates of the
    /// content that paints it (no page flip, unlike [`PdfDocument::add_shading`]): axial from
    /// (`coords[0]`, `coords[1]`) to (`coords[2]`, `coords[3]`), or radial with `coords` =
    /// `x0 y0 r0 x1 y1 r1`. `gray` makes the samples 1 byte (DeviceGray, for soft masks), else 3
    /// (DeviceRGB). The end colours extend.
    pub fn add_user_shading(&mut self, radial: bool, coords: &[f64], gray: bool, samples: &[u8]) -> Result<PdfShading> {
        let comps = if gray { 1 } else { 3 };
        let n = (samples.len() / comps).max(2);
        let range = if gray { "0 1" } else { "0 1 0 1 0 1" };
        let fdict = format!("/FunctionType 0 /Domain [0 1] /Range [{range}] /Size [{n}] /BitsPerSample 8 /Order 1 /Encode [0 {}]", n - 1);
        let data = samples.get(..n * comps).unwrap_or(samples);
        let func = self.w.add_stream(&fdict, data, true)?;
        let cs = if gray { "DeviceGray" } else { "DeviceRGB" };
        let coords = crate::num::FList(coords);
        let kind = if radial { 3 } else { 2 };
        let dict = format!("<< /ColorSpace /{cs} /ShadingType {kind} /Coords [{coords}] /Extend [true true] /Function {func} 0 R >>");
        let obj_num = self.w.add_dict(&dict)?;
        let index = u32::try_from(self.shadings.len()).unwrap_or(u32::MAX);
        self.shadings.push(obj_num);
        Ok(PdfShading { index, obj_num })
    }

    /// Starts a page of `width_pt` x `height_pt` points. Draw on it and call
    /// [`PageWriter::finish`]; resources the page needs are created through
    /// [`PageWriter::document`] while it is open.
    pub fn add_page(&mut self, width_pt: f64, height_pt: f64) -> PageWriter<'_, S> {
        PageWriter::new(self, width_pt, height_pt)
    }

    /// The name (`GS<n>`) of the opacity state for fill and stroke alpha (`ca`, `CA`), created at
    /// the first use. `fmt` makes the key: two alphas that round alike share a state.
    pub(crate) fn alpha_state(&mut self, fill: f64, stroke: f64) -> u32 {
        let (fill, stroke) = (crate::num::fmt_value(fill), crate::num::fmt_value(stroke));
        let key = (fill.to_bits(), stroke.to_bits());
        if let Some(i) = self.alpha_index.get(&key) {
            return *i;
        }
        let index = u32::try_from(self.alpha_states.len()).unwrap_or(u32::MAX);
        self.alpha_states.push((fill, stroke));
        self.alpha_index.insert(key, index);
        index
    }

    /// Writes the rest of the file and gives the sink back.
    pub fn build(mut self) -> Result<S> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        // fonts
        let mut font_refs = String::new();
        for font in &self.fonts {
            let n = font.write(&mut self.w, &self.limits)?;
            if !font_refs.is_empty() {
                font_refs.push(' ');
            }
            let _ = write!(font_refs, "/F{} {n} 0 R", font.index());
        }
        let resources = self.resources(&font_refs);
        if let Some(n) = self.res_obj {
            self.w.define_text(n, &resources)?;
        }

        let catalog = self.w.alloc()?;
        let pages_node = self.w.alloc()?;

        // pages allocated first: internal links and outline entries point to any of them
        let mut page_refs = Vec::with_capacity(self.pages.len());
        for _ in &self.pages {
            page_refs.push(self.w.alloc()?);
        }
        let heights: Vec<f64> = self.pages.iter().map(|p| p.height_pt).collect();
        let dest = Dest { page_refs: &page_refs, heights: &heights };

        let pages = std::mem::take(&mut self.pages);
        for (i, page) in pages.into_iter().enumerate() {
            let Some(page_ref) = page_refs.get(i).copied() else { break };
            let content_num = self.w.alloc()?;
            self.w.define_body(content_num, "", page.content)?;
            let mut annots = String::new();
            for link in &page.links {
                let h = page.height_pt;
                let mut dict = String::new();
                let _ = write!(
                    dict,
                    "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] ",
                    F(link.x_pt),
                    F(h - link.y_top_pt - link.h_pt),
                    F(link.x_pt + link.w_pt),
                    F(h - link.y_top_pt)
                );
                match &link.target {
                    LinkTarget::Uri(uri) => {
                        let _ = write!(dict, "/A << /S /URI /URI ({uri}) >> >>");
                    }
                    LinkTarget::Dest { page_index, top_pt } => {
                        let _ = write!(dict, "/Dest {} >>", dest.text(*page_index, *top_pt));
                    }
                }
                let n = self.w.add_dict(&dict)?;
                if !annots.is_empty() {
                    annots.push(' ');
                }
                let _ = write!(annots, "{n} 0 R");
            }
            let annots_entry = if annots.is_empty() { String::new() } else { format!(" /Annots [{annots}]") };
            let dict = format!(
                "<< /Type /Page /Parent {pages_node} 0 R /MediaBox [0 0 {} {}] /Resources {resources} /Contents {content_num} 0 R{annots_entry} >>",
                F(page.width_pt),
                F(page.height_pt)
            );
            self.w.define_text(page_ref, &dict)?;
        }

        let mut kids = String::new();
        for (i, r) in page_refs.iter().enumerate() {
            if i > 0 {
                kids.push(' ');
            }
            let _ = write!(kids, "{r} 0 R");
        }
        self.w.define_text(pages_node, &format!("<< /Type /Pages /Count {} /Kids [{kids}] >>", page_refs.len()))?;

        let outlines = if !self.outline.is_empty() && !page_refs.is_empty() { Some(self.write_outline(&dest)?) } else { None };
        let catalog_dict = match outlines {
            Some(o) => format!("<< /Type /Catalog /Pages {pages_node} 0 R /Outlines {o} 0 R >>"),
            None => format!("<< /Type /Catalog /Pages {pages_node} 0 R >>"),
        };
        self.w.define_text(catalog, &catalog_dict)?;
        self.w.build(catalog)
    }

    /// The `/Resources` dictionary every page carries.
    fn resources(&self, font_refs: &str) -> String {
        let font_dict = if font_refs.is_empty() { String::new() } else { format!("/Font << {font_refs} >>") };
        let mut xobj = String::new();
        if !self.images.is_empty() || !self.forms.is_empty() {
            xobj.push_str("/XObject <<");
            for (i, n) in self.images.iter().enumerate() {
                let _ = write!(xobj, " /Im{i} {n} 0 R");
            }
            for (i, n) in self.forms.iter().enumerate() {
                let _ = write!(xobj, " /Fm{i} {n} 0 R");
            }
            xobj.push_str(" >>");
        }
        let mut gs = String::new();
        if !self.alpha_states.is_empty() || !self.ext_states.is_empty() {
            gs.push_str("/ExtGState <<");
            for (i, (ca, cs)) in self.alpha_states.iter().enumerate() {
                let _ = write!(gs, " /GS{i} << /Type /ExtGState /ca {} /CA {} >>", J(*ca), J(*cs));
            }
            for (i, entries) in self.ext_states.iter().enumerate() {
                let _ = write!(gs, " /GX{i} << /Type /ExtGState {entries} >>");
            }
            gs.push_str(" >>");
        }
        let mut sh = String::new();
        if !self.shadings.is_empty() {
            sh.push_str("/Shading <<");
            for (i, n) in self.shadings.iter().enumerate() {
                let _ = write!(sh, " /Sh{i} {n} 0 R");
            }
            sh.push_str(" >>");
        }
        let mut pat = String::new();
        if !self.patterns.is_empty() {
            pat.push_str(" /Pattern <<");
            for (i, n) in self.patterns.iter().enumerate() {
                let _ = write!(pat, " /Pt{i} {n} 0 R");
            }
            pat.push_str(" >>");
        }
        if !gs.is_empty() || !sh.is_empty() || !pat.is_empty() {
            format!("<< {font_dict} {xobj} {gs} {sh}{pat} >>")
        } else {
            format!("<< {font_dict} {xobj} >>")
        }
    }

    /// The outline tree from the flat list with levels (the port of `PdfDocument.WriteOutline` of
    /// the .NET). Returns the root object.
    fn write_outline(&mut self, dest: &Dest<'_>) -> Result<u32> {
        let root = self.w.alloc()?;
        let items = std::mem::take(&mut self.outline);
        let mut nums = Vec::with_capacity(items.len());
        for _ in &items {
            nums.push(self.w.alloc()?);
        }
        // parent, siblings and the position among them, by the stack of open levels
        let n = items.len();
        let mut parent: Vec<Option<usize>> = Vec::with_capacity(n);
        let mut position: Vec<usize> = Vec::with_capacity(n);
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut top: Vec<usize> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        for (i, it) in items.iter().enumerate() {
            while let Some(&t) = stack.last() {
                if items.get(t).is_some_and(|open| open.level >= it.level) {
                    stack.pop();
                } else {
                    break;
                }
            }
            let p = stack.last().copied();
            parent.push(p);
            let siblings = match p {
                Some(p) => children.get_mut(p),
                None => Some(&mut top),
            };
            if let Some(siblings) = siblings {
                position.push(siblings.len());
                siblings.push(i);
            }
            stack.push(i);
        }
        // descendants of each entry: children come after their parent, so one pass backwards
        let mut descendants = vec![0usize; n];
        for i in (0..n).rev() {
            if let (Some(Some(p)), Some(&own)) = (parent.get(i), descendants.get(i))
                && let Some(d) = descendants.get_mut(*p)
            {
                *d += 1 + own;
            }
        }
        let reference = |i: Option<usize>| -> u32 { i.and_then(|i| nums.get(i).copied()).unwrap_or(root) };
        for (i, it) in items.iter().enumerate() {
            let p = parent.get(i).copied().flatten();
            let siblings: &[usize] = match p {
                Some(p) => children.get(p).map_or(&[], Vec::as_slice),
                None => &top,
            };
            let k = position.get(i).copied().unwrap_or(0);
            let kids: &[usize] = children.get(i).map_or(&[], Vec::as_slice);
            let mut d = format!("<< /Title <FEFF{}> /Parent {} 0 R", utf16be_hex(&it.title).to_uppercase(), reference(p));
            if k > 0 {
                let _ = write!(d, " /Prev {} 0 R", reference(siblings.get(k - 1).copied()));
            }
            if k + 1 < siblings.len() {
                let _ = write!(d, " /Next {} 0 R", reference(siblings.get(k + 1).copied()));
            }
            if let (Some(&first), Some(&last)) = (kids.first(), kids.last()) {
                let _ = write!(
                    d,
                    " /First {} 0 R /Last {} 0 R /Count -{}",
                    reference(Some(first)),
                    reference(Some(last)),
                    descendants.get(i).copied().unwrap_or(0)
                );
            }
            let _ = write!(d, " /Dest {} >>", dest.text(it.page_index, it.top_pt));
            if let Some(num) = nums.get(i) {
                self.w.define_text(*num, &d)?;
            }
        }
        let first = reference(top.first().copied());
        let last = reference(top.last().copied());
        self.w.define_text(root, &format!("<< /Type /Outlines /First {first} 0 R /Last {last} 0 R /Count {} >>", items.len()))?;
        Ok(root)
    }
}

/// `dest(pageIndex, topPt)` of the TS: an `/XYZ` destination on the page (clamped), `topPt` from the top.
struct Dest<'a> {
    page_refs: &'a [u32],
    heights: &'a [f64],
}

impl Dest<'_> {
    fn text(&self, page_index: i64, top_pt: f64) -> String {
        let last = self.page_refs.len().saturating_sub(1);
        let pi = usize::try_from(page_index.max(0)).map_or(last, |p| p.min(last));
        let page_ref = self.page_refs.get(pi).copied().unwrap_or(0);
        let height = self.heights.get(pi).copied().unwrap_or(0.0);
        format!("[{page_ref} 0 R /XYZ 0 {} null]", F(height - top_pt))
    }
}
