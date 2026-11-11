//! D2b-F4: the watermark commands and queries: `setWatermark`, `removeWatermark`, `watermark()`,
//! `watermarkTexts()`, `watermarkGallery()` and the model half of `paintWatermarkTemplate` (editor
//! `universal-write.ts` 11763-11956, 12090). The XML and the boxes are D1b's emitters
//! (`write/emit/background.rs`); this file is the session half: the header parts of the section, the
//! run read back from them, the run source, the picture relationship (`addPartImage`) and the
//! single undo step (`editSectionHeaders`).
//!
//! The layout's and the fonts' answers are arguments (D2a-C2, F4-D12, F4-D15): the section and the
//! variant the caret's page shows ([`HfAt`]), the box of an explicit font size
//! (`watermarkTextBox` reads the live font provider) and the pixel size of a picture
//! (`imagePixelInfo`).

use std::sync::Arc;

use uo_jsnum::conv::truthy;
use uo_jsnum::math::floor;
use uo_jsnum::text::trim;

use super::super::super::command::{CommandError, Outcome};
use super::super::super::{ExecCx, Session};
use super::super::header_footer::{HfAt, HfKind, HfReenter, MAX_HF_PARTS};
use super::PageLang;
use super::rx;
use crate::ids::{HfPartId, SourceKey};
use crate::js::JsNumber;
use crate::model::para::HfRefEdit;
use crate::model::run::Run;
use crate::model::section::SectPr;
use crate::model::sources::{RunSource, SourceStore, SrcText};
use crate::model::story::Story;
use crate::read::drawing::vml_color;
use crate::read::watermark::{WatermarkInfo, is_watermark_xml, read_watermark_xml};
use crate::word::obj;
use crate::write::emit::background::{
    EMPTY_DOC_RELS, WatermarkBox, WatermarkGalleryItem, WatermarkIds, WatermarkLayout, WatermarkPictureSpec, WatermarkSpec,
    WatermarkTextSpec, auto_text_box, first_open_tag, picture_box, vml_color_attr, watermark_gallery, watermark_run_xml, watermark_texts,
};
use crate::write::emit::settings::hf_part_xml;
use crate::write::matchers::find_literal_ci;

type Res = Result<Outcome, CommandError>;

/// The argument of `setWatermark(spec)`: a gallery item (id), or the custom dialog's spec with the
/// answers the TS reads from the live fonts and the image decoder.
#[derive(Debug, Clone, PartialEq)]
pub enum WatermarkArg {
    /// `setWatermark('confidential1')`
    Gallery(String),
    /// the text mark; `measured_box` is `watermarkTextBox(text, font, sizePt)` (the explicit size;
    /// `None` = no face: the TS fallback `{ sizePt * length * 0.6, sizePt * 1.2 }`)
    Text { spec: WatermarkTextSpec, measured_box: Option<WatermarkBox> },
    /// the picture mark; `pixel_info` is `imagePixelInfo(data)` as `[w, h, dpi]` (`None` = not an
    /// image: nothing happens)
    Picture { spec: WatermarkPictureSpec, pixel_info: Option<[f64; 3]> },
}

/// An item of `watermarkGallery()`: the D1b item plus the labels of the interface language.
#[derive(Debug, Clone, PartialEq)]
pub struct WatermarkGalleryEntry {
    pub item: WatermarkGalleryItem,
    pub label: String,
    pub category_label: &'static str,
}

fn lang_key(lang: PageLang) -> &'static str {
    match lang {
        PageLang::En => "en",
        PageLang::Pt => "pt",
        PageLang::Es => "es",
    }
}

/// `runObjXml(run)`: the XML of the object (`w:pict`/`w:drawing`) a run came from.
fn run_obj_xml<'a>(store: Option<&'a SourceStore>, run: &Run) -> Option<&'a str> {
    let store = store?;
    let sid = run.props.src?;
    let src = store.runs.get(&sid)?;
    if let Some(o) = &src.obj {
        return Some(o.resolve(&store.parts));
    }
    let head = src.obj_head?.as_f64()?;
    if head < 0.0 || head.fract() != 0.0 || head > f64::from(u32::MAX) {
        return None;
    }
    let h = store.runs.get(&SourceKey(uo_jsnum::conv::to_uint32(head)))?;
    h.obj.as_ref().map(|o| o.resolve(&store.parts))
}

/// `isWatermarkRun(run)`.
fn is_watermark_run(store: Option<&SourceStore>, run: &Run) -> bool {
    let vml = run.ext().is_some_and(|e| {
        e.textbox.as_deref().is_some_and(|t| t.vml == Some(true)) || e.image.as_deref().is_some_and(|i| i.vml == Some(true))
    });
    vml && is_watermark_xml(run_obj_xml(store, run))
}

/// The indices of the paragraphs of a story that hold a watermark run.
fn watermark_paras(store: Option<&SourceStore>, story: &Story) -> Vec<usize> {
    story.paras.iter().enumerate().filter(|(_, p)| p.runs.iter().any(|r| is_watermark_run(store, r))).map(|(i, _)| i).collect()
}

/// Whether `ext` is accepted by `addPartImage` (F4-D13: `[A-Za-z0-9]{1,16}`).
fn ext_ok(ext: &str) -> bool {
    !ext.is_empty() && ext.len() <= 16 && ext.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl Session {
    fn wm_nothing(&mut self) -> Res {
        Ok(Outcome::Done(self.change_set()))
    }

    /// `setWatermark(spec)`.
    pub(crate) fn exec_set_watermark(
        &mut self,
        arg: WatermarkArg,
        at: HfAt,
        lang: PageLang,
        reenter: Option<HfReenter>,
        _cx: &mut ExecCx<'_>,
    ) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.wm_nothing();
        }
        let sec = self.hf_section_of(at);
        let sp = self.body_sect_pr_at(sec);
        let content_w = (sp.page_width_twips.to_number() - sp.margin_left_twips.to_number() - sp.margin_right_twips.to_number()) / 20.0;
        let (wm, box_) = match arg {
            WatermarkArg::Gallery(id) => {
                let Some(g) = watermark_gallery().into_iter().find(|x| x.id == id) else { return self.wm_nothing() };
                let (text, b) = match lang {
                    PageLang::Pt => (g.text_pt, g.box_pt),
                    PageLang::En => (g.text_en, g.box_en),
                    PageLang::Es => (g.text_es, g.box_es),
                };
                let spec = WatermarkTextSpec {
                    text: text.to_owned(),
                    font: "calibri".to_owned(),
                    size_pt: None,
                    color: "C0C0C0".to_owned(),
                    semitransparent: true,
                    layout: g.layout,
                    lang: None,
                };
                (WatermarkSpec::Text(spec), WatermarkBox { width_pt: b[0], height_pt: b[1] })
            }
            WatermarkArg::Text { spec, measured_box } => {
                if trim(&spec.text).is_empty() {
                    return self.wm_nothing();
                }
                let b = match spec.size_pt {
                    Some(s) if truthy(s) => measured_box.unwrap_or_else(|| {
                        let len = f64::from(u32::try_from(spec.text.encode_utf16().count()).unwrap_or(u32::MAX));
                        WatermarkBox { width_pt: s * len * 0.6, height_pt: s * 1.2 }
                    }),
                    _ => auto_text_box(&spec.text, spec.layout, content_w),
                };
                (WatermarkSpec::Text(spec), b)
            }
            WatermarkArg::Picture { spec, pixel_info } => {
                let Some([w, h, dpi]) = pixel_info else { return self.wm_nothing() };
                if !ext_ok(&spec.ext) {
                    return Err(CommandError::Position);
                }
                let b = picture_box((w * 72.0) / dpi, (h * 72.0) / dpi, spec.scale, content_w);
                (WatermarkSpec::Picture(spec), b)
            }
        };
        self.edit_section_headers(sec, reenter, &mut |s, part| {
            {
                // `for (const p of paras) p.runs = p.runs.filter(...)` (the store handle is dropped
                // before the run is added: a second owner would make `Arc::make_mut` copy the store)
                let store_has = s.doc.sources.clone();
                let store = store_has.as_deref();
                let gone = s.doc.hf.get(part.index()).map(|h| watermark_paras(store, &h.story)).unwrap_or_default();
                if let Some(h) = s.doc.hf.make_mut(part.index()) {
                    for i in gone {
                        if let Some(p) = h.story.paras.make_mut(i) {
                            p.runs.retain(|r| !is_watermark_run(store, r));
                        }
                    }
                }
            }
            let path = s.doc.hf.get(part.index()).map(|h| h.path.to_string()).unwrap_or_default();
            if let Some(run) = s.watermark_run(&wm, box_, &path)? {
                let empty = s.doc.hf.get(part.index()).is_none_or(|h| h.story.paras.is_empty());
                if empty {
                    let fresh = s.new_hf_story(HfKind::Header);
                    if let (Some(h), Some(p)) = (s.doc.hf.make_mut(part.index()), fresh.paras.get_arc(0)) {
                        h.story.paras.push_arc(Arc::clone(p));
                    }
                }
                if let Some(p) = s.doc.hf.make_mut(part.index()).and_then(|h| h.story.paras.make_mut(0)) {
                    p.runs.insert(0, run);
                }
            }
            Ok(())
        })
    }

    /// `removeWatermark()`: from EVERY header of the document (Word, oracle `wm-remove-multi`).
    pub(crate) fn exec_remove_watermark(&mut self, reenter: Option<HfReenter>, _cx: &mut ExecCx<'_>) -> Res {
        if self.paras().is_none() || self.locked() {
            return self.wm_nothing();
        }
        let store = self.doc.sources.clone();
        let keys: Vec<HfPartId> = self
            .sections
            .hf_parts
            .iter()
            .filter(|(k, id)| {
                k.starts_with("header:")
                    && self.doc.hf.get(id.index()).is_some_and(|h| !watermark_paras(store.as_deref(), &h.story).is_empty())
            })
            .map(|(_, id)| *id)
            .collect();
        if keys.is_empty() {
            return self.wm_nothing();
        }
        let hf = self.hf_in_edit();
        if hf.is_some() {
            self.exit_hf_silently(false);
        }
        self.begin_step(false, &keys);
        for id in &keys {
            let gone = self.doc.hf.get(id.index()).map(|h| watermark_paras(store.as_deref(), &h.story)).unwrap_or_default();
            if let Some(h) = self.doc.hf.make_mut(id.index()) {
                for i in gone {
                    if let Some(p) = h.story.paras.make_mut(i) {
                        p.runs.retain(|r| !is_watermark_run(store.as_deref(), r));
                    }
                }
            }
            self.dirty_hf.insert(*id);
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.touch_all();
        self.settle();
        if let Some((part, page, _)) = hf {
            self.reenter_hf(page, part, reenter, false);
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `editSectionHeaders(sec, fn)`: the three headers (even, default, first) of section `sec` (its
    /// own part or the inherited one; the one no previous section has is created in the section
    /// itself) in ONE undo step (Word: the mark goes into the three; oracle `page-lab`, corpus
    /// 106/110). The return to the edit is silent (no `uw-hfmode`).
    fn edit_section_headers(
        &mut self,
        sec: usize,
        reenter: Option<HfReenter>,
        f: &mut dyn FnMut(&mut Session, HfPartId) -> Result<(), CommandError>,
    ) -> Res {
        const SLOTS: [&str; 3] = ["even", "default", "first"];
        let missing = SLOTS.iter().filter(|s| self.hf_part_for(sec, HfKind::Header, s).is_none()).count();
        if self.doc.hf.len().saturating_add(missing) > MAX_HF_PARTS {
            return Err(CommandError::Position);
        }
        let hf = self.hf_in_edit();
        if hf.is_some() {
            self.exit_hf_silently(false);
        }
        let mut keys: Vec<HfPartId> = Vec::new();
        for s in SLOTS {
            if let Some((p, _)) = self.hf_part_for(sec, HfKind::Header, s)
                && !keys.contains(&p)
            {
                keys.push(p);
            }
        }
        self.begin_step(false, &keys);
        let mut done: Vec<HfPartId> = Vec::new();
        for slot in SLOTS {
            let id = match self.hf_part_for(sec, HfKind::Header, slot) {
                Some((p, _)) => p,
                None => {
                    let target = self.new_hf_target(HfKind::Header);
                    let id = match self.create_hf_part(HfKind::Header, target.clone(), None) {
                        Ok(id) => id,
                        Err(e) => {
                            self.discard_step();
                            return Err(e);
                        }
                    };
                    self.edit_section_refs(sec, |h, _| {
                        h.push(HfRefEdit { kind: "header".into(), r#type: slot.into(), rid: target.0.clone(), ..HfRefEdit::default() });
                    });
                    id
                }
            };
            if done.contains(&id) {
                continue;
            }
            done.push(id);
            if let Err(e) = f(self, id) {
                self.discard_step();
                return Err(e);
            }
            self.dirty_hf.insert(id);
        }
        self.doc.formatting_version = self.doc.formatting_version.wrapping_add(1);
        self.touch_formatting();
        self.touch_all();
        self.settle();
        if let Some((part, page, _)) = hf {
            self.reenter_hf(page, part, reenter, false);
        }
        Ok(Outcome::Done(self.finish()))
    }

    /// `watermarkRun(spec, box, path)`: the run of the mark for the header part `path` (the picture
    /// gets its relationship and media in the package first).
    fn watermark_run(&mut self, spec: &WatermarkSpec, box_: WatermarkBox, path: &str) -> Result<Option<Run>, CommandError> {
        let rid = match spec {
            WatermarkSpec::Picture(p) => Some(self.add_part_image(path, &p.data, &p.ext)),
            WatermarkSpec::Text(_) => None,
        };
        let mut used = String::new();
        for n in self.pkg.part_names() {
            if rx::is_hf_part_name(n) {
                used.push_str(self.pkg.part_text(n).ok().flatten().as_deref().unwrap_or(""));
            }
        }
        {
            let store = self.doc.sources.as_deref();
            for id in self.sections.hf_parts.values() {
                let Some(h) = self.doc.hf.get(id.index()) else { continue };
                for p in h.story.paras.iter() {
                    for r in &p.runs {
                        used.push_str(run_obj_xml(store, r).unwrap_or(""));
                    }
                }
            }
        }
        let spid = rx::next_spid(&used);
        let r1 = f64::from(self.env.random_u32()) / 4_294_967_296.0;
        let n = 100_000_000.0 + floor(r1 * 899_999_999.0);
        let r2 = f64::from(self.env.random_u32()) / 4_294_967_296.0;
        let hex = crate::word::num::js_to_radix(floor(r2 * f64::from(0x7fff_ffff_u32)), 16);
        let anchor_id = format!("{:0>8}", uo_text::case::to_uppercase_str(&hex));
        let ids = WatermarkIds { n, spid, anchor_id };
        let run_xml = watermark_run_xml(spec, box_, &ids, rid.as_deref(), Some(&vml_color_attr));
        let xml = hf_part_xml("header", &format!("<w:p>{run_xml}</w:p>"));
        let Ok(Some(story)) = self.read_part_story_no_sources(path, &xml) else { return Ok(None) };
        let Some(mut run) = story
            .paras
            .first()
            .and_then(|p| p.runs.iter().find(|r| r.ext().is_some_and(|e| e.textbox.is_some() || e.image.is_some())).cloned())
        else {
            return Ok(None);
        };
        if let Some(arc) = self.doc.sources.as_mut() {
            let store = Arc::make_mut(arc);
            let id = store.id();
            let obj_xml = rx::pict_span(&run_xml).unwrap_or("").to_owned();
            store.runs.insert(
                id,
                RunSource {
                    r_pr: Some(SrcText::Owned("<w:rPr><w:noProof/></w:rPr>".into())),
                    obj: Some(SrcText::Owned(obj_xml.as_str().into())),
                    ..RunSource::default()
                },
            );
            let mut props = (*run.props).clone();
            obj::set(&mut props, "src", |x| x.src = Some(id));
            run.props = Arc::new(props);
        }
        Ok(Some(run))
    }

    /// `addPartImage(path, data, ext)`: a new image relationship in the part's `.rels`, the media
    /// part and the content type; the `rId`.
    fn add_part_image(&mut self, path: &str, data: &[u8], ext0: &str) -> String {
        let lower = uo_text::case::to_lowercase_str(ext0);
        let ext = if lower == "jpg" { "jpeg".to_owned() } else { lower };
        let mut n = 1u64;
        while self.pkg.contains(&format!("word/media/image{n}.{ext}")) {
            n += 1;
        }
        self.pkg.set_part(format!("word/media/image{n}.{ext}"), data.to_vec());
        let name = path.rsplit('/').next().unwrap_or(path);
        let rels_path = format!("word/_rels/{name}.rels");
        let mut rels =
            self.pkg.part_text(&rels_path).ok().flatten().map_or_else(|| EMPTY_DOC_RELS.to_owned(), std::borrow::Cow::into_owned);
        let used = rid_numbers(&rels);
        let mut k = 1u64;
        while used.contains(&k) {
            k += 1;
        }
        let rel = format!(
            "<Relationship Id=\"rId{k}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image{n}.{ext}\"/></Relationships>"
        );
        rels = rels.replacen("</Relationships>", &rel, 1);
        self.pkg.set_part_text(rels_path, rels);
        if let Some(ct) = self.pkg.part_text("[Content_Types].xml").ok().flatten().map(std::borrow::Cow::into_owned)
            && !find_literal_ci(&ct, &format!("<Default Extension=\"{ext}\""))
            && let Some((_, end)) = first_open_tag(&ct, "Types")
        {
            let mime = match ext.as_str() {
                "png" => "image/png".to_owned(),
                "jpeg" => "image/jpeg".to_owned(),
                "gif" => "image/gif".to_owned(),
                "bmp" => "image/bmp".to_owned(),
                "tif" | "tiff" => "image/tiff".to_owned(),
                other => format!("image/{other}"),
            };
            let at = format!("<Default Extension=\"{ext}\" ContentType=\"{mime}\"/>");
            let out = format!("{}{at}{}", ct.get(..end).unwrap_or(""), ct.get(end..).unwrap_or(""));
            self.pkg.set_part_text("[Content_Types].xml", out);
        }
        format!("rId{k}")
    }

    // ── queries ────────────────────────────────────────────────────────────────────────────

    /// `watermark()`: the mark of the caret's section (from the default header, else from the first
    /// that has one), and the picture's bytes.
    #[must_use]
    pub fn watermark(&self, at: HfAt) -> Option<(WatermarkInfo, Option<Vec<u8>>)> {
        let sec = self.hf_section_of(at);
        let store = self.doc.sources.as_deref();
        for slot in ["default", "first", "even"] {
            let Some((part, _)) = self.hf_part_for(sec, HfKind::Header, slot) else { continue };
            let Some(h) = self.doc.hf.get(part.index()) else { continue };
            for p in h.story.paras.iter() {
                for run in &p.runs {
                    if !is_watermark_run(store, run) {
                        continue;
                    }
                    let Some(xml) = run_obj_xml(store, run) else { continue };
                    if let Some(info) = read_watermark_xml(xml, &vml_color) {
                        let image = match (&info, run.ext().and_then(|e| e.image.as_deref())) {
                            (WatermarkInfo::Picture(_), Some(img)) => Some(self.doc.media.get(img.data.0).unwrap_or(&[]).to_vec()),
                            _ => None,
                        };
                        return Some((info, image));
                    }
                }
            }
        }
        None
    }

    /// `watermarkTexts()`: the list of the custom dialog (interface language).
    #[must_use]
    pub fn watermark_texts(&self, lang: PageLang) -> &'static [&'static str; 10] {
        watermark_texts(lang_key(lang)).unwrap_or(&crate::write::emit::background::WATERMARK_TEXTS_EN)
    }

    /// `watermarkGallery()`: the items with the labels of the interface language.
    #[must_use]
    pub fn watermark_gallery_entries(&self, lang: PageLang) -> Vec<WatermarkGalleryEntry> {
        watermark_gallery()
            .into_iter()
            .map(|g| {
                let text = match lang {
                    PageLang::Pt => g.text_pt,
                    PageLang::En => g.text_en,
                    PageLang::Es => g.text_es,
                };
                let category_label = match (g.category, lang) {
                    ("confidential", PageLang::En) => "Confidential",
                    ("confidential", _) => "Confidencial",
                    ("disclaimers", PageLang::En) => "Disclaimers",
                    ("disclaimers", PageLang::Pt) => "Avisos de Isenção de Responsabilidade",
                    ("disclaimers", PageLang::Es) => "Declinaciones de responsabilidades",
                    ("urgent", PageLang::En) => "Urgent",
                    _ => "Urgente",
                };
                let label = format!("{text} {}", if g.layout == WatermarkLayout::Diagonal { 1 } else { 2 });
                WatermarkGalleryEntry { item: g, label, category_label }
            })
            .collect()
    }

    /// The model half of `paintWatermarkTemplate(id, …)`: the header story that holds the mark of
    /// the gallery item `id` and the Letter page (1" margins) the preview is laid out on.
    pub fn watermark_preview(&mut self, id: &str, lang: PageLang) -> Option<(Story, SectPr)> {
        let g = watermark_gallery().into_iter().find(|x| x.id == id)?;
        let (text, b) = match lang {
            PageLang::Pt => (g.text_pt, g.box_pt),
            PageLang::En => (g.text_en, g.box_en),
            PageLang::Es => (g.text_es, g.box_es),
        };
        let spec = WatermarkSpec::Text(WatermarkTextSpec {
            text: text.to_owned(),
            font: "calibri".to_owned(),
            size_pt: None,
            color: "C0C0C0".to_owned(),
            semitransparent: true,
            layout: g.layout,
            lang: None,
        });
        let ids = WatermarkIds { n: 1.0, spid: 1025.0, anchor_id: "00000001".to_owned() };
        let xml = watermark_run_xml(&spec, WatermarkBox { width_pt: b[0], height_pt: b[1] }, &ids, None, Some(&vml_color_attr));
        let story =
            self.read_part_story_no_sources("word/uw-preview.xml", &hf_part_xml("header", &format!("<w:p>{xml}</w:p>"))).ok().flatten()?;
        let n = JsNumber::Num;
        let sp = SectPr {
            page_width_twips: n(12240.0),
            page_height_twips: n(15840.0),
            margin_left_twips: n(1440.0),
            margin_right_twips: n(1440.0),
            margin_top_twips: n(1440.0),
            margin_bottom_twips: n(1440.0),
            header_distance_twips: Some(n(720.0)),
            footer_distance_twips: Some(n(720.0)),
            ..SectPr::default()
        };
        Some((story, sp))
    }
}

/// The `k` of every `Id="rId<k>"` substring of a `.rels` text (what the TS probes one by one with
/// `rels.includes('Id="rId' + k + '"')`), read in one linear pass: the first free `k` then costs
/// the size of the set, not `k` scans of the text (F4: a hostile `.rels` with tens of thousands of
/// ids in order).
fn rid_numbers(rels: &str) -> std::collections::HashSet<u64> {
    const NEEDLE: &str = "Id=\"rId";
    let mut out = std::collections::HashSet::new();
    let mut i = 0;
    while let Some(off) = rels.get(i..).and_then(|t| t.find(NEEDLE)) {
        let start = i + off + NEEDLE.len();
        let digits = rels.get(start..).map_or(0, |t| t.bytes().take_while(u8::is_ascii_digit).count());
        let closed = rels.as_bytes().get(start + digits) == Some(&b'"');
        let canonical = digits > 0 && (digits == 1 || rels.as_bytes().get(start) != Some(&b'0'));
        if closed
            && canonical
            && let Some(k) = rels.get(start..start + digits).and_then(|d| d.parse::<u64>().ok())
        {
            out.insert(k);
        }
        i = start;
    }
    out
}
